//! An agent's record (stage PH, H2): a read-only view over the terminal area, opened from the sidebar's Agents
//! section — the prompt's first line, the tool calls with their times, errors, tokens, the last message. Read from
//! the agent's transcript (`<session>/subagents/agent-<id>.jsonl`, written by Claude Code a little after its hooks)
//! off the UI thread, the last 1 MiB only; before the file exists, what the hooks gave. Shown, never logged.
//! Keys: j/k ↓/↑, Space/PageDown, b/PageUp, g/Home, G/End, r reread, Esc or q closes.

use std::path::{Path, PathBuf};

use haetty_cc::json::Json;
use haetty_core::Cell;
use haetty_proto::wire::Worker;

use crate::ui::{blank, fit, put, Greys};

/// The most of a transcript read (its end).
const TAIL: u64 = 1 << 20;

/// What the transcript holds, as text for the screen.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Record {
    pub prompt: String,
    /// (time `HH:MM:SS`, tool, target).
    pub calls: Vec<(String, String, String)>,
    pub errors: Vec<String>,
    pub last: String,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub cut: bool,
}

/// The transcript of agent `id` beside the conversation's `transcript` (`<dir>/<session>.jsonl`).
pub fn transcript_of(transcript: &str, id: &str) -> Option<PathBuf> {
    let t = Path::new(transcript);
    let stem = t.file_stem()?.to_str()?;
    if !haetty_cc::resume::valid_id(stem) || id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return None;
    }
    Some(t.with_file_name(stem).join("subagents").join(format!("agent-{id}.jsonl")))
}

fn first_line(s: &str) -> String {
    s.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("").chars().take(300).collect()
}

fn time_of(j: &Json) -> String {
    // `2026-11-17T12:03:01.123Z` → `12:03:01` (UTC, as Claude Code writes it).
    let t = j.get("timestamp").and_then(Json::str).unwrap_or("");
    t.get(11..19).unwrap_or("").to_string()
}

/// Parses transcript text (one JSON object per line; a cut first line is skipped).
pub fn parse(text: &str, cut: bool) -> Record {
    let mut r = Record { cut, ..Default::default() };
    for line in text.lines() {
        let Ok(j) = Json::parse(line) else { continue };
        let kind = j.get("type").and_then(Json::str).unwrap_or("");
        let content = j.at(&["message", "content"]);
        match kind {
            "user" => {
                if r.prompt.is_empty() && !cut {
                    if let Some(s) = content.and_then(Json::str) {
                        r.prompt = first_line(s);
                    } else if let Some(Json::Arr(a)) = content {
                        if let Some(t) = a.iter().find_map(|b| {
                            (b.get("type").and_then(Json::str) == Some("text"))
                                .then(|| b.get("text").and_then(Json::str))
                                .flatten()
                        }) {
                            r.prompt = first_line(t);
                        }
                    }
                }
                if let Some(Json::Arr(a)) = content {
                    for b in a {
                        if b.get("type").and_then(Json::str) == Some("tool_result")
                            && b.get("is_error").and_then(Json::bool) == Some(true)
                        {
                            let msg = match b.get("content") {
                                Some(Json::Str(s)) => first_line(s),
                                Some(Json::Arr(x)) => x
                                    .iter()
                                    .find_map(|y| y.get("text").and_then(Json::str))
                                    .map(first_line)
                                    .unwrap_or_default(),
                                _ => String::new(),
                            };
                            r.errors.push(format!("{} {msg}", time_of(&j)));
                        }
                    }
                }
            }
            "assistant" => {
                if let Some(u) = j.at(&["message", "usage"]) {
                    let n = |k: &str| u.get(k).and_then(Json::num).unwrap_or(0.0) as u64;
                    r.tokens_in += n("input_tokens") + n("cache_creation_input_tokens") + n("cache_read_input_tokens");
                    r.tokens_out += n("output_tokens");
                }
                if let Some(Json::Arr(a)) = content {
                    for b in a {
                        match b.get("type").and_then(Json::str) {
                            Some("tool_use") => {
                                let name = b.get("name").and_then(Json::str).unwrap_or("").to_string();
                                let target = haetty_cc::agents::target(b.get("input"));
                                r.calls.push((time_of(&j), name, target));
                            }
                            Some("text") => {
                                let t = b.get("text").and_then(Json::str).unwrap_or("");
                                if !t.trim().is_empty() {
                                    r.last = t.trim().chars().take(4000).collect();
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            _ => {}
        }
    }
    r
}

/// Reads the end of `path` (at most [`TAIL`]).
pub fn read(path: &Path) -> std::io::Result<Record> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path)?;
    let len = f.metadata()?.len();
    let cut = len > TAIL;
    if cut {
        f.seek(SeekFrom::Start(len - TAIL))?;
    }
    let mut buf = vec![];
    f.take(TAIL).read_to_end(&mut buf)?;
    let text = String::from_utf8_lossy(&buf);
    let text = if cut { text.split_once('\n').map_or("", |(_, rest)| rest) } else { &text };
    Ok(parse(text, cut))
}

pub enum Key {
    Stay,
    Close,
    Refresh,
}

pub struct AgentView {
    pub agent: Worker,
    pub record: Option<Record>,
    pub note: Option<String>,
    pub scroll: usize,
}

impl AgentView {
    pub fn lines(&self, w: usize, now_ms: u64) -> Vec<(String, bool, bool)> {
        // (text, dim, strong)
        let a = &self.agent;
        let mut out = vec![];
        let state = if a.failed {
            "failed"
        } else if a.ended_ms.is_some() {
            "done"
        } else {
            "running"
        };
        let elapsed = crate::sidebar::age(a.ended_ms.unwrap_or(now_ms).saturating_sub(a.started_ms));
        let model = crate::sidebar::short_model(&a.model);
        let mut head = format!("{} · {state} · {elapsed}", if a.kind.is_empty() { "agent" } else { &a.kind });
        if !model.is_empty() {
            head.push_str(&format!(" · {model}"));
        }
        head.push_str(&format!(" · {} calls", a.calls));
        if a.background {
            head.push_str(" · background");
        }
        if a.worktree {
            head.push_str(" · worktree");
        }
        out.push((head, false, true));
        out.push((format!("id {}   Esc closes · r rereads", a.id), true, false));
        out.push((String::new(), false, false));
        if let Some(n) = &self.note {
            out.push((n.clone(), true, false));
        }
        let Some(r) = &self.record else { return out };
        let wrap = |t: &str, out: &mut Vec<(String, bool, bool)>| {
            for l in t.lines() {
                let mut s = l.to_string();
                while crate::ui::width(&s) > w {
                    let head = fit(&s, w).trim_end_matches('…').to_string();
                    let n = head.len().max(1);
                    out.push((s[..n].to_string(), false, false));
                    s = s[n..].to_string();
                }
                out.push((s, false, false));
            }
        };
        out.push(("prompt".into(), true, false));
        if r.cut {
            out.push(("(older than the last 1 MiB of the record)".into(), true, false));
        } else {
            wrap(&r.prompt, &mut out);
        }
        out.push((String::new(), false, false));
        out.push((format!("tool calls  {}", r.calls.len()), true, false));
        for (t, name, target) in &r.calls {
            out.push((format!("{t}  {name} {target}"), false, false));
        }
        if !r.errors.is_empty() {
            out.push((String::new(), false, false));
            out.push(("errors".into(), true, false));
            for e in &r.errors {
                wrap(e, &mut out);
            }
        }
        if r.tokens_in + r.tokens_out > 0 {
            out.push((String::new(), false, false));
            out.push((
                format!("tokens  in {}  out {}", crate::ui::human(r.tokens_in), crate::ui::human(r.tokens_out)),
                true,
                false,
            ));
        }
        out.push((String::new(), false, false));
        out.push(("last message".into(), true, false));
        wrap(&r.last, &mut out);
        out
    }

    pub fn key(&mut self, key: &str, shift: bool, height: usize, total: usize) -> Key {
        let page = height.saturating_sub(2).max(1);
        let max = total.saturating_sub(height);
        match (key, shift) {
            ("escape", _) | ("q", false) => return Key::Close,
            ("r", false) => return Key::Refresh,
            ("j", false) | ("down", _) => self.scroll += 1,
            ("k", false) | ("up", _) => self.scroll = self.scroll.saturating_sub(1),
            ("space", false) | ("pagedown", _) => self.scroll += page,
            ("b", false) | ("pageup", _) => self.scroll = self.scroll.saturating_sub(page),
            ("g", false) | ("home", _) => self.scroll = 0,
            ("g", true) | ("end", _) => self.scroll = max,
            _ => {}
        }
        self.scroll = self.scroll.min(max);
        Key::Stay
    }

    /// The view's rows over an area of `w` × `h`.
    pub fn draw(&self, w: usize, h: usize, g: Greys, now_ms: u64) -> Vec<Vec<Cell>> {
        let mut rows = vec![vec![blank(g.normal, g.bg); w]; h];
        let lines = self.lines(w.saturating_sub(4), now_ms);
        for (row, (t, dim, strong)) in rows.iter_mut().zip(lines.iter().skip(self.scroll)) {
            let fg = if *strong {
                g.strong
            } else if *dim {
                g.dim
            } else {
                g.normal
            };
            put(row, 2, &fit(t, w.saturating_sub(4)), fg, g.bg, *strong);
        }
        rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_agents_transcript() {
        let t = "/h/.claude/projects/-p/28341705-8f1b-4f48-9026-fb5ef080748d.jsonl";
        assert_eq!(
            transcript_of(t, "a43d61db9b1eee19a").unwrap(),
            PathBuf::from(
                "/h/.claude/projects/-p/28341705-8f1b-4f48-9026-fb5ef080748d/subagents/agent-a43d61db9b1eee19a.jsonl"
            )
        );
        assert!(transcript_of(t, "../x").is_none());
        assert!(transcript_of("/h/x/not-a-uuid.jsonl", "a1").is_none());
    }

    #[test]
    fn reads_prompt_calls_errors_tokens_and_the_last_message() {
        let text = [
            r#"{"type":"user","timestamp":"2026-11-17T12:00:00.000Z","message":{"role":"user","content":"List the files\nand more"}}"#,
            r#"{"type":"attachment"}"#,
            r#"{"type":"assistant","timestamp":"2026-11-17T12:00:02.000Z","message":{"content":[{"type":"tool_use","name":"Read","input":{"file_path":"/p/src/app.rs"}}],"usage":{"input_tokens":2,"cache_read_input_tokens":100,"output_tokens":40}}}"#,
            r#"{"type":"user","timestamp":"2026-11-17T12:00:03.000Z","message":{"content":[{"type":"tool_result","is_error":true,"content":"cat: no-such-file: No such file"}]}}"#,
            r#"{"type":"assistant","timestamp":"2026-11-17T12:00:05.000Z","message":{"content":[{"type":"text","text":"Done: 3 files."}],"usage":{"input_tokens":1,"output_tokens":10}}}"#,
            "not json",
        ]
        .join("\n");
        let r = parse(&text, false);
        assert_eq!(r.prompt, "List the files");
        assert_eq!(r.calls, [("12:00:02".to_string(), "Read".to_string(), "app.rs".to_string())]);
        assert_eq!(r.errors, ["12:00:03 cat: no-such-file: No such file"]);
        assert_eq!((r.tokens_in, r.tokens_out), (103, 50));
        assert_eq!(r.last, "Done: 3 files.");
    }
}
