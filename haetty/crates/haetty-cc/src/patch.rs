//! A session's edits in the shape Claude Code draws them, for the inline diff (stage PD, DESIGN.md §2.5): the
//! PostToolUse `structuredPatch` of Edit and Write, and before it, while the tool waits (a permission question), the
//! hunks of the PreToolUse snapshot against the tool input. Files that look secret are left out, as for the edit
//! snapshot.

use std::collections::{HashMap, VecDeque};

use haetty_proto::wire::{PreSnapshot, WireHunk, WirePatch};

use crate::edit;
use crate::json::Json;

/// Patches kept per session, the newest.
pub const KEEP: usize = 64;

#[derive(Default)]
pub struct Patches {
    list: VecDeque<WirePatch>,
    pub gen: u64,
}

impl Patches {
    /// A hook event; returns whether the patches changed.
    pub fn hook(&mut self, event: &str, j: &Json, pending: &HashMap<String, PreSnapshot>) -> bool {
        let tool = j.get("tool_name").and_then(Json::str).unwrap_or_default();
        let (Some(id), Some(input)) = (j.get("tool_use_id").and_then(Json::str), j.get("tool_input")) else {
            return false;
        };
        let Some(path) = edit::target(input).filter(|p| edit::is_edit_tool(tool) && !edit::looks_secret(p)) else {
            return false;
        };
        let hunks = match event {
            "PreToolUse" => {
                let Some(before) = pending.get(id).and_then(edit::base) else { return false };
                let Some(after) = edit::apply_edit(tool, input, Some(before)) else { return false };
                Some(hunks_of(before, &after))
            }
            "PostToolUse" => j.at(&["tool_response", "structuredPatch"]).and_then(Json::arr).map(structured),
            _ => return false,
        };
        let done = event == "PostToolUse";
        let old = self.list.iter().position(|p| p.id == id);
        match (hunks.filter(|h| !h.is_empty()), old) {
            (Some(hunks), Some(n)) => self.list[n] = WirePatch { id: id.into(), path: path.into(), done, hunks },
            (Some(hunks), None) => {
                self.list.push_back(WirePatch { id: id.into(), path: path.into(), done, hunks });
                while self.list.len() > KEEP {
                    self.list.pop_front();
                }
            }
            (None, Some(n)) if done => {
                self.list.remove(n);
            }
            (None, _) => return false,
        }
        self.gen += 1;
        true
    }

    pub fn list(&self) -> Vec<WirePatch> {
        self.list.iter().cloned().collect()
    }
}

fn structured(hunks: &[Json]) -> Vec<WireHunk> {
    hunks
        .iter()
        .filter_map(|h| {
            Some(WireHunk {
                old_start: h.get("oldStart")?.num()? as u32,
                new_start: h.get("newStart")?.num()? as u32,
                lines: h.get("lines")?.arr()?.iter().filter_map(Json::str).map(str::to_string).collect(),
            })
        })
        .collect()
}

/// The hunks of an edit as Claude Code shows them before its `structuredPatch` exists: three lines of context, a
/// tab as two spaces. A new file has none (Claude Code shows its content, not a diff).
pub fn hunks_of(before: &str, after: &str) -> Vec<WireHunk> {
    if before.is_empty() {
        return vec![];
    }
    let diff = similar::TextDiff::configure().algorithm(similar::Algorithm::Myers).diff_lines(before, after);
    diff.grouped_ops(3)
        .iter()
        .filter_map(|group| {
            let first = group.first()?;
            let mut lines = vec![];
            for op in group {
                for c in diff.iter_changes(op) {
                    let mark = match c.tag() {
                        similar::ChangeTag::Equal => ' ',
                        similar::ChangeTag::Delete => '-',
                        similar::ChangeTag::Insert => '+',
                    };
                    lines.push(format!("{mark}{}", c.value().trim_end_matches(['\n', '\r']).replace('\t', "  ")));
                }
            }
            Some(WireHunk {
                old_start: first.old_range().start as u32 + 1,
                new_start: first.new_range().start as u32 + 1,
                lines,
            })
        })
        .collect()
}

/// A range of characters, end excluded.
pub type Span = (usize, usize);

/// The ranges of `a` and `b`, in characters, that a word diff marks as changed (the deeper tint of a changed line).
pub fn changed_words(a: &str, b: &str) -> (Vec<Span>, Vec<Span>) {
    let d = similar::TextDiff::from_words(a, b);
    let (mut ra, mut rb, mut ia, mut ib) = (vec![], vec![], 0, 0);
    for c in d.iter_all_changes() {
        let n = c.value().chars().count();
        match c.tag() {
            similar::ChangeTag::Equal => {
                ia += n;
                ib += n;
            }
            similar::ChangeTag::Delete => {
                ra.push((ia, ia + n));
                ia += n;
            }
            similar::ChangeTag::Insert => {
                rb.push((ib, ib + n));
                ib += n;
            }
        }
    }
    (ra, rb)
}

#[cfg(test)]
mod tests {
    use super::*;
    use haetty_proto::wire::{Secret, SnapStatus};

    fn j(t: &str) -> Json {
        Json::parse(t).unwrap()
    }

    fn pending(id: &str, text: &str) -> HashMap<String, PreSnapshot> {
        let snap = PreSnapshot { path: "/w/a.py".into(), status: SnapStatus::Copied, text: Secret(Some(text.into())) };
        HashMap::from([(id.to_string(), snap)])
    }

    #[test]
    fn a_waiting_edit_has_hunks_from_the_snapshot_and_its_post_tool_use_replaces_them() {
        let mut p = Patches::default();
        let pre = j(
            r#"{"tool_name":"Edit","tool_use_id":"t1","tool_input":{"file_path":"/w/a.py","old_string":"b","new_string":"B"}}"#,
        );
        assert!(p.hook("PreToolUse", &pre, &pending("t1", "a\nb\n\tc\nd\n")));
        let l = p.list();
        assert_eq!(l.len(), 1);
        assert!(!l[0].done);
        assert_eq!(l[0].hunks[0].lines, [" a", "-b", "+B", "   c", " d"]);
        assert_eq!((l[0].hunks[0].old_start, l[0].hunks[0].new_start), (1, 1));
        let post = j(
            r#"{"tool_name":"Edit","tool_use_id":"t1","tool_input":{"file_path":"/w/a.py"},"tool_response":{"structuredPatch":[{"oldStart":1,"oldLines":4,"newStart":1,"newLines":4,"lines":[" a","-b","+B","   c"," d"]}]}}"#,
        );
        assert!(p.hook("PostToolUse", &post, &HashMap::new()));
        let l = p.list();
        assert_eq!(l.len(), 1);
        assert!(l[0].done);
        assert_eq!(p.gen, 2);
    }

    #[test]
    fn secret_files_new_files_and_other_tools_give_no_patch() {
        let mut p = Patches::default();
        let env = j(
            r#"{"tool_name":"Edit","tool_use_id":"t1","tool_input":{"file_path":"/w/.env","old_string":"a","new_string":"b"}}"#,
        );
        assert!(!p.hook("PreToolUse", &env, &pending("t1", "a\n")));
        let new = j(
            r#"{"tool_name":"Write","tool_use_id":"t2","tool_input":{"file_path":"/w/n.py","content":"x\n"},"tool_response":{"type":"create","structuredPatch":[]}}"#,
        );
        assert!(!p.hook("PostToolUse", &new, &HashMap::new()));
        let read = j(r#"{"tool_name":"Read","tool_use_id":"t3","tool_input":{"file_path":"/w/a.py"}}"#);
        assert!(!p.hook("PreToolUse", &read, &pending("t3", "a\n")));
        assert!(p.list().is_empty());
        assert_eq!(p.gen, 0);
    }

    #[test]
    fn a_word_diff_marks_only_the_changed_words() {
        let (a, b) = changed_words("def add(a, b):", "def add_numbers(a, b):");
        assert_eq!(a, [(4, 10)]);
        assert_eq!(b, [(4, 18)]);
    }

    #[test]
    fn only_the_newest_patches_are_kept() {
        let mut p = Patches::default();
        for k in 0..KEEP + 5 {
            let post = j(&format!(
                r#"{{"tool_name":"Edit","tool_use_id":"t{k}","tool_input":{{"file_path":"/w/a.py"}},"tool_response":{{"structuredPatch":[{{"oldStart":1,"newStart":1,"lines":["-a","+b"]}}]}}}}"#
            ));
            p.hook("PostToolUse", &post, &HashMap::new());
        }
        let l = p.list();
        assert_eq!(l.len(), KEEP);
        assert_eq!(l[0].id, "t5");
    }
}
