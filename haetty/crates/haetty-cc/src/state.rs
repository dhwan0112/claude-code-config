//! What the daemon tracks per Claude Code session, from hook events and
//! status-line JSON (DESIGN.md §3.13). Only derived fields are kept in
//! [`ClaudeInfo`]; edit texts stay here for the diff view and are never
//! logged.
//!
//! Attention (after ccx): `Wait` when Claude Code stops or idles at its
//! prompt, `Ask` when it needs a permission answer or input. Hooks run as
//! separate processes, partly asynchronously, so events can arrive out of
//! order; each carries the time its hook started, and `Ask` is cleared only
//! by an event that started later (so the permission state fails closed).

use std::collections::{BTreeSet, HashMap, VecDeque};

use haetty_proto::wire::{Attention, ClaudeInfo, EditMeta, EditView, Limit, PreSnapshot, Secret, SnapStatus, Worker};

use crate::edit;
use crate::json::Json;

const KEEP_EDITS: usize = 20;
const KEEP_DONE_WORKERS: usize = 6;
const MAX_PENDING: usize = 64;

/// Notification types that mean "Claude Code asks you something".
const ASK: &[&str] = &["permission_prompt", "elicitation_dialog", "elicitation_url_dialog", "agent_needs_input"];

struct EditRec {
    meta: EditMeta,
    status: SnapStatus,
    before: String,
    after: String,
}

#[derive(Default)]
pub struct Claude {
    pub info: ClaudeInfo,
    ask_ts: u64,
    pending: HashMap<String, PreSnapshot>,
    edits: VecDeque<EditRec>,
    files: BTreeSet<String>,
}

fn text(j: &Json, path: &[&str]) -> String {
    j.at(path).and_then(Json::str).unwrap_or_default().to_string()
}

impl Claude {
    pub fn awaiting_permission(&self) -> bool {
        self.info.attention == Attention::Ask
    }

    /// A hook event. Returns whether `info` changed.
    pub fn hook(&mut self, event: &str, j: &Json, ts_ms: u64, snapshot: Option<PreSnapshot>) -> bool {
        let before = self.info.clone();
        let sid = text(j, &["session_id"]);
        if !sid.is_empty() {
            self.info.claude_session = sid;
        }
        let i = &mut self.info;
        let later_than_ask = ts_ms >= self.ask_ts;
        match event {
            "SessionStart" => {
                i.busy = false;
                if i.attention != Attention::Ask || later_than_ask {
                    i.attention = Attention::None;
                }
            }
            "UserPromptSubmit" => {
                i.busy = true;
                if i.attention != Attention::Ask || later_than_ask {
                    i.attention = Attention::None;
                }
            }
            crate::settings::PRE_ALL if edit::is_edit_tool(&text(j, &["tool_name"])) => {
                // Reported by the synchronous PreToolUse entry.
            }
            "PreToolUse" | crate::settings::PRE_ALL => {
                i.busy = true;
                if i.attention == Attention::Wait {
                    i.attention = Attention::None;
                }
                let tool = text(j, &["tool_name"]);
                if let (true, Some(snap), Some(id)) =
                    (edit::is_edit_tool(&tool), snapshot, j.get("tool_use_id").and_then(Json::str))
                {
                    if self.pending.len() >= MAX_PENDING {
                        self.pending.clear(); // a tool that never finished
                    }
                    self.pending.insert(id.to_string(), snap);
                }
            }
            "PermissionRequest" => {
                i.attention = Attention::Ask;
                // A late, older request must not move the mark back in time.
                self.ask_ts = self.ask_ts.max(ts_ms);
            }
            "Notification" => {
                let kind = text(j, &["notification_type"]);
                if ASK.contains(&kind.as_str()) {
                    i.attention = Attention::Ask;
                    self.ask_ts = self.ask_ts.max(ts_ms);
                } else if kind == "idle_prompt" && i.attention != Attention::Ask {
                    i.attention = Attention::Wait;
                    i.busy = false;
                }
            }
            "PostToolUse" | "PostToolUseFailure" | "PermissionDenied" => {
                if i.attention == Attention::Ask && later_than_ask {
                    i.attention = Attention::None;
                }
                if event == "PostToolUse" {
                    self.edited(j, ts_ms);
                }
            }
            "Stop" | "StopFailure" => {
                i.busy = false;
                if i.attention != Attention::Ask || later_than_ask {
                    i.attention = Attention::Wait;
                }
            }
            "SubagentStart" => {
                let id = text(j, &["agent_id"]);
                if !id.is_empty() && !i.workers.iter().any(|w| w.id == id) {
                    i.workers.push(Worker { id, kind: text(j, &["agent_type"]), started_ms: ts_ms, ended_ms: None });
                }
            }
            "SubagentStop" => {
                let id = text(j, &["agent_id"]);
                if let Some(w) = i.workers.iter_mut().find(|w| w.id == id && w.ended_ms.is_none()) {
                    w.ended_ms = Some(ts_ms);
                }
                // Running first, then the newest finished ones.
                i.workers
                    .sort_by_key(|w| (w.ended_ms.is_some(), std::cmp::Reverse(w.ended_ms.unwrap_or(w.started_ms))));
                let running = i.workers.iter().filter(|w| w.ended_ms.is_none()).count();
                i.workers.truncate(running + KEEP_DONE_WORKERS);
            }
            "SessionEnd" => {
                i.busy = false;
                if i.attention != Attention::Ask || later_than_ask {
                    i.attention = Attention::None;
                }
            }
            _ => {}
        }
        self.info != before
    }

    fn edited(&mut self, j: &Json, ts_ms: u64) {
        let tool = text(j, &["tool_name"]);
        if !edit::is_edit_tool(&tool) {
            return;
        }
        let Some(input) = j.get("tool_input") else { return };
        let Some(path) = edit::target(input).map(str::to_string) else { return };
        let snap = j.get("tool_use_id").and_then(Json::str).and_then(|id| self.pending.remove(id));
        let status = snap.as_ref().map_or(SnapStatus::Error, |s| s.status);
        let before = snap.as_ref().and_then(edit::base).map(str::to_string);
        let after = edit::apply_edit(&tool, input, before.as_deref());
        let (added, removed, diffable) = match (&before, &after) {
            (Some(b), Some(a)) => {
                let (x, y) = edit::line_counts(b, a);
                (x, y, true)
            }
            _ => (0, 0, false),
        };
        let agent = j.get("agent_type").and_then(Json::str).filter(|s| !s.is_empty()).unwrap_or("main").to_string();
        let meta = EditMeta { path: path.clone(), agent, at_ms: ts_ms, added, removed, diffable };
        self.files.insert(path);
        self.info.files_changed = self.files.len() as u32;
        self.info.edits += 1;
        self.info.last_edit = Some(meta.clone());
        let (before, after) =
            if diffable { (before.unwrap_or_default(), after.unwrap_or_default()) } else { Default::default() };
        self.edits.push_back(EditRec { meta, status, before, after });
        while self.edits.len() > KEEP_EDITS {
            self.edits.pop_front();
        }
    }

    /// Status-line JSON. Returns whether anything shown changed (the
    /// update time alone does not count: no redraw every refresh).
    pub fn statusline(&mut self, j: &Json, now_ms: u64) -> bool {
        let before = self.info.clone();
        let i = &mut self.info;
        let sid = text(j, &["session_id"]);
        if !sid.is_empty() {
            i.claude_session = sid;
        }
        i.name = text(j, &["session_name"]);
        i.model = text(j, &["model", "display_name"]);
        i.context_pct = j.at(&["context_window", "used_percentage"]).and_then(Json::num).map(|v| v as f32);
        i.context_size = j.at(&["context_window", "context_window_size"]).and_then(Json::num).map(|v| v as u64);
        i.limits = match j.get("rate_limits") {
            Some(Json::Obj(m)) => m
                .iter()
                .filter_map(|(name, v)| {
                    Some(Limit {
                        name: name.clone(),
                        used_pct: v.get("used_percentage").and_then(Json::num)? as f32,
                        resets_at: v.get("resets_at").and_then(Json::num).map(|t| t as i64),
                    })
                })
                .collect(),
            _ => vec![],
        };
        i.cost_usd = j.at(&["cost", "total_cost_usd"]).and_then(Json::num);
        i.lines_added = j.at(&["cost", "total_lines_added"]).and_then(Json::num).unwrap_or(0.0) as u64;
        i.lines_removed = j.at(&["cost", "total_lines_removed"]).and_then(Json::num).unwrap_or(0.0) as u64;
        i.cache_warm = j.at(&["prompt_cache", "warm"]).and_then(Json::bool);
        i.cache_hit_pct = j.at(&["prompt_cache", "hit_ratio"]).and_then(Json::num).map(|r| {
            let r = r as f32;
            if r <= 1.0 {
                r * 100.0
            } else {
                r
            }
        });
        let changed = self.info != before;
        self.info.updated_ms = now_ms;
        changed
    }

    /// The user looked at the tab: a "waiting" mark goes; a question stays.
    pub fn clear_wait(&mut self) -> bool {
        if self.info.attention == Attention::Wait {
            self.info.attention = Attention::None;
            return true;
        }
        false
    }

    pub fn last_edit(&self) -> Option<EditView> {
        self.edits.back().map(|e| EditView {
            meta: e.meta.clone(),
            status: e.status,
            before: Secret(e.before.clone()),
            after: Secret(e.after.clone()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn j(t: &str) -> Json {
        Json::parse(t).unwrap()
    }

    #[test]
    fn attention_follows_hooks_and_fails_closed() {
        let mut c = Claude::default();
        assert!(c.hook("UserPromptSubmit", &j(r#"{"session_id":"s-1"}"#), 10, None));
        assert!(c.info.busy);
        assert_eq!(c.info.claude_session, "s-1");
        c.hook("PermissionRequest", &j(r#"{"tool_name":"Bash"}"#), 20, None);
        assert!(c.awaiting_permission());
        // An older event (its hook started before the request) must not clear it.
        c.hook("PostToolUse", &j(r#"{"tool_name":"Read"}"#), 15, None);
        assert!(c.awaiting_permission());
        c.hook("PreToolUse", &j(r#"{"tool_name":"Bash"}"#), 25, None);
        assert!(c.awaiting_permission(), "PreToolUse does not answer a question");
        c.hook("PostToolUse", &j(r#"{"tool_name":"Bash"}"#), 30, None);
        assert_eq!(c.info.attention, Attention::None);
        c.hook("Stop", &j("{}"), 40, None);
        assert_eq!((c.info.attention, c.info.busy), (Attention::Wait, false));
        assert!(c.clear_wait() && !c.clear_wait());
        c.hook("Notification", &j(r#"{"notification_type":"permission_prompt"}"#), 50, None);
        assert!(c.awaiting_permission());
        assert!(!c.clear_wait(), "selecting the tab does not clear a question");
        c.hook("Notification", &j(r#"{"notification_type":"idle_prompt"}"#), 60, None);
        assert!(c.awaiting_permission(), "idle does not replace a question");
        c.hook("UserPromptSubmit", &j("{}"), 70, None);
        assert_eq!(c.info.attention, Attention::None);
        c.hook("Stop", &j("{}"), 75, None);
        c.hook("PreToolUseAll", &j(r#"{"tool_name":"Edit"}"#), 76, None);
        assert_eq!(c.info.attention, Attention::Wait, "edit tools are reported by the synchronous entry");
        c.hook("PreToolUseAll", &j(r#"{"tool_name":"Read"}"#), 77, None);
        assert_eq!(c.info.attention, Attention::None);
        c.hook("SubagentStart", &j(r#"{"agent_id":"a1","agent_type":"Explore"}"#), 80, None);
        c.hook("SubagentStop", &j(r#"{"agent_id":"a1"}"#), 90, None);
        assert_eq!(c.info.workers[0].ended_ms, Some(90));
    }

    #[test]
    fn out_of_order_events_never_clear_a_newer_question() {
        let mut c = Claude::default();
        // Two questions; the older one's report arrives last.
        c.hook("PermissionRequest", &j("{}"), 30, None);
        c.hook("PermissionRequest", &j("{}"), 20, None);
        c.hook("PostToolUse", &j(r#"{"tool_name":"Bash"}"#), 25, None);
        assert!(c.awaiting_permission(), "25 is older than the newest question (30)");
        // A SessionStart / SessionEnd that started before the question.
        c.hook("SessionStart", &j(r#"{"source":"clear"}"#), 10, None);
        c.hook("SessionEnd", &j("{}"), 12, None);
        assert!(c.awaiting_permission(), "older session events do not clear it");
        c.hook("SessionStart", &j(r#"{"source":"resume"}"#), 40, None);
        assert_eq!(c.info.attention, Attention::None, "a later SessionStart does");
        // Notification ask with an older time keeps the newest time.
        c.hook("PermissionRequest", &j("{}"), 50, None);
        c.hook("Notification", &j(r#"{"notification_type":"permission_prompt"}"#), 45, None);
        c.hook("Stop", &j("{}"), 48, None);
        assert!(c.awaiting_permission());
        c.hook("SessionEnd", &j("{}"), 60, None);
        assert_eq!(c.info.attention, Attention::None);
    }

    #[test]
    fn edits_are_rebuilt_from_the_snapshot() {
        let mut c = Claude::default();
        let snap =
            PreSnapshot { path: "/p/a.rs".into(), status: SnapStatus::Copied, text: Secret(Some("a\nb\n".into())) };
        let pre = j(r#"{"tool_name":"Edit","tool_use_id":"t1","tool_input":{"file_path":"/p/a.rs"}}"#);
        c.hook("PreToolUse", &pre, 1, Some(snap));
        let post = j(
            r#"{"tool_name":"Edit","tool_use_id":"t1","agent_type":"","tool_input":{"file_path":"/p/a.rs","old_string":"b","new_string":"B\nc"}}"#,
        );
        assert!(c.hook("PostToolUse", &post, 2, None));
        let e = c.last_edit().unwrap();
        assert_eq!((e.before.0.as_str(), e.after.0.as_str()), ("a\nb\n", "a\nB\nc\n"));
        assert_eq!((e.meta.added, e.meta.removed, e.meta.agent.as_str()), (2, 1, "main"));
        assert_eq!(c.info.files_changed, 1);
        // Without a snapshot: counted, not diffable.
        let w = j(
            r#"{"tool_name":"Edit","tool_use_id":"t2","tool_input":{"file_path":"/p/b.rs","old_string":"x","new_string":"y"}}"#,
        );
        c.hook("PostToolUse", &w, 3, None);
        assert_eq!(c.info.files_changed, 2);
        assert!(!c.info.last_edit.as_ref().unwrap().diffable);
    }

    #[test]
    fn statusline_fields_and_no_change_on_refresh_alone() {
        let mut c = Claude::default();
        let t = r#"{"session_id":"s","session_name":"fix parser","model":{"display_name":"Opus"},
            "context_window":{"used_percentage":42.5,"context_window_size":200000},
            "rate_limits":{"five_hour":{"used_percentage":12,"resets_at":1793900000},"seven_day":{"used_percentage":30}},
            "cost":{"total_cost_usd":1.234,"total_lines_added":10,"total_lines_removed":3},
            "prompt_cache":{"warm":true,"hit_ratio":0.9}}"#;
        assert!(c.statusline(&j(t), 1000));
        let i = &c.info;
        assert_eq!((i.name.as_str(), i.model.as_str(), i.context_pct), ("fix parser", "Opus", Some(42.5)));
        assert_eq!(i.limits.len(), 2);
        assert_eq!(i.limits[0].resets_at, Some(1_793_900_000));
        assert_eq!((i.cost_usd, i.lines_added, i.cache_warm), (Some(1.234), 10, Some(true)));
        assert!((i.cache_hit_pct.unwrap() - 90.0).abs() < 0.01);
        assert!(!c.statusline(&j(t), 4000), "same content three seconds later");
        assert_eq!(c.info.updated_ms, 4000);
        assert!(c.statusline(&j(r#"{"context_window":{"used_percentage":null}}"#), 5000));
        assert_eq!(c.info.context_pct, None);
    }
}
