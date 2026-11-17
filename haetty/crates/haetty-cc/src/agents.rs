//! The agent table of a Claude Code session (stage PH, H2): every subagent, its parent, model, current tool and
//! state, from the hooks haetty already gets (measured in H0: `docs/history/2026-11-17-ph-harness.md`).
//!
//! - `SubagentStart` / `SubagentStop` (`agent_id`, `agent_type`): start and end; a second start of the same id is a
//!   resumed agent; a stop without a start is one of Claude Code's internal agents (empty type) and is ignored.
//! - The Agent tool's `PreToolUse` (`tool_input.isolation`) and `PostToolUse` (`tool_response.agentId`,
//!   `.status`, `.resolvedModel`), whose own `agent_id` is the parent (none: the main agent). The child's start
//!   comes about 30 ms before the parent's `PostToolUse`; either may come first.
//! - Tool hooks with an `agent_id`: the current tool and its target, the number of calls.
//! - `PermissionRequest` with an `agent_id`: that agent's question is open until its next tool end or stop.
//!
//! This table never changes the attention mark or the input hold (`state.rs` keeps those rules).

use std::collections::HashMap;

use haetty_proto::wire::Worker;

use crate::json::Json;

/// Finished agents kept per session (running ones always are).
pub const KEEP_DONE: usize = 200;
/// Agent tool calls waiting for their `PostToolUse` (a call that never finishes is dropped past this many).
const MAX_CALLS: usize = 64;

/// The Agent tool's name (older Claude Code: `Task`).
fn is_agent_tool(name: &str) -> bool {
    matches!(name, "Agent" | "Task")
}

fn text(j: &Json, path: &[&str]) -> String {
    j.at(path).and_then(Json::str).unwrap_or_default().to_string()
}

/// A file name or pattern a tool works on, for the screen only: the last path part.
pub fn target(input: Option<&Json>) -> String {
    let Some(i) = input else { return String::new() };
    for k in ["file_path", "notebook_path", "path"] {
        if let Some(v) = i.get(k).and_then(Json::str).filter(|v| !v.is_empty()) {
            let v = v.trim_end_matches(['/', '\\']);
            return v.rsplit(['/', '\\']).next().unwrap_or(v).chars().take(80).collect();
        }
    }
    // A search pattern is shown whole (`src/**/*.rs`).
    if let Some(v) = i.get("pattern").and_then(Json::str) {
        return v.chars().take(80).collect();
    }
    String::new()
}

#[derive(Default)]
pub struct Agents {
    /// Agent tool calls by `tool_use_id`: the requested isolation and the call's description.
    calls: HashMap<String, (bool, String)>,
}

impl Agents {
    /// Applies a hook event to `w` (the session's table). Returns whether it changed.
    pub fn hook(&mut self, w: &mut Vec<Worker>, event: &str, j: &Json, ts_ms: u64) -> bool {
        let before = w.clone();
        let me = text(j, &["agent_id"]);
        let tool = text(j, &["tool_name"]);
        match event {
            "SessionStart" => {
                // A cleared or resumed conversation starts with no agents of its own.
                if matches!(text(j, &["source"]).as_str(), "clear" | "resume" | "startup") {
                    w.clear();
                    self.calls.clear();
                }
            }
            "SubagentStart" => {
                if me.is_empty() {
                    return false;
                }
                let kind = text(j, &["agent_type"]);
                match w.iter_mut().find(|a| a.id == me) {
                    // Resumed (or its parent's PostToolUse came first): running again.
                    Some(a) => {
                        a.ended_ms = None;
                        if a.kind.is_empty() {
                            a.kind = kind;
                        }
                    }
                    None => w.push(Worker {
                        id: me,
                        fork: kind == "fork",
                        kind,
                        started_ms: ts_ms,
                        depth: 1,
                        ..Default::default()
                    }),
                }
            }
            "SubagentStop" => {
                // Internal agents (no start, empty type) are not in the table.
                if let Some(a) = w.iter_mut().find(|a| a.id == me) {
                    a.ended_ms = Some(ts_ms.max(a.started_ms));
                    a.tool.clear();
                    a.target.clear();
                    a.asking = false;
                    // The STATUS its last message ended with (the orchestrator's report lines, DESIGN.md §8.2).
                    if let Some(r) = j
                        .get("last_assistant_message")
                        .and_then(Json::str)
                        .and_then(|m| crate::orch::parse_report(m, ts_ms, false))
                    {
                        a.status = r.status;
                    }
                }
            }
            "PreToolUse" | crate::settings::PRE_ALL => {
                if is_agent_tool(&tool) {
                    if let Some(id) = j.get("tool_use_id").and_then(Json::str) {
                        if self.calls.len() >= MAX_CALLS {
                            self.calls.clear();
                        }
                        let wt = j.at(&["tool_input", "isolation"]).and_then(Json::str) == Some("worktree");
                        let d: String = text(j, &["tool_input", "description"]).chars().take(120).collect();
                        self.calls.insert(id.to_string(), (wt, d));
                    }
                }
                if let Some(a) = w.iter_mut().find(|a| a.id == me) {
                    a.tool = tool;
                    a.target = target(j.get("tool_input"));
                }
            }
            "PostToolUse" | "PostToolUseFailure" => {
                if is_agent_tool(&tool) {
                    self.agent_call(w, j, &me, event == "PostToolUseFailure", ts_ms);
                }
                if let Some(a) = w.iter_mut().find(|a| a.id == me) {
                    a.calls = a.calls.saturating_add(1);
                    a.tool.clear();
                    a.target.clear();
                    a.asking = false;
                }
            }
            "PermissionRequest" => {
                if let Some(a) = w.iter_mut().find(|a| a.id == me) {
                    a.asking = true;
                }
            }
            _ => return false,
        }
        // Running first (in start order), then the newest finished; at most KEEP_DONE finished.
        w.sort_by_key(|a| (a.ended_ms.is_some(), std::cmp::Reverse(a.ended_ms.unwrap_or(0)), a.started_ms));
        let running = w.iter().filter(|a| a.ended_ms.is_none()).count();
        w.truncate(running + KEEP_DONE);
        *w != before
    }

    /// The Agent tool call ended: link the child to its parent, its model and mode.
    fn agent_call(&mut self, w: &mut Vec<Worker>, j: &Json, parent: &str, failed: bool, ts_ms: u64) {
        let call = j.get("tool_use_id").and_then(Json::str).unwrap_or_default().to_string();
        let (worktree, description) = self.calls.remove(&call).unwrap_or_default();
        let depth = w.iter().find(|a| a.id == parent).map_or(1, |p| p.depth.saturating_add(1));
        let child = text(j, &["tool_response", "agentId"]);
        if failed || child.is_empty() {
            // No agent came of it: one failed line, named after the requested type.
            if failed {
                w.push(Worker {
                    id: if call.is_empty() { format!("failed-{ts_ms}") } else { call },
                    kind: text(j, &["tool_input", "subagent_type"]),
                    started_ms: ts_ms,
                    ended_ms: Some(ts_ms),
                    parent: parent.to_string(),
                    depth,
                    worktree,
                    description,
                    failed: true,
                    ..Default::default()
                });
            }
            return;
        }
        let i = match w.iter().position(|a| a.id == child) {
            Some(i) => i,
            None => {
                // The parent's PostToolUse came before the child's SubagentStart.
                w.push(Worker {
                    id: child.clone(),
                    kind: text(j, &["tool_input", "subagent_type"]),
                    started_ms: ts_ms,
                    ..Default::default()
                });
                w.len() - 1
            }
        };
        let a = &mut w[i];
        a.parent = parent.to_string();
        a.depth = depth;
        a.worktree |= worktree;
        if !description.is_empty() {
            a.description = description;
        }
        let model = text(j, &["tool_response", "resolvedModel"]);
        if !model.is_empty() {
            a.model = model;
        }
        a.background = text(j, &["tool_response", "status"]) == "async_launched";
        // A child's own children were linked before its parent was known: their depth follows.
        let ids: Vec<(String, u8)> = vec![(child, depth)];
        let mut stack = ids;
        while let Some((id, d)) = stack.pop() {
            for c in w.iter_mut().filter(|c| c.parent == id) {
                c.depth = d.saturating_add(1);
                stack.push((c.id.clone(), c.depth));
            }
        }
    }
}

/// The agents in display order: running before finished, each parent followed by its children (depth-first).
pub fn tree(w: &[Worker]) -> Vec<&Worker> {
    let mut out = vec![];
    let ids: std::collections::HashSet<&str> = w.iter().map(|a| a.id.as_str()).collect();
    let roots: Vec<&Worker> = w.iter().filter(|a| a.parent.is_empty() || !ids.contains(a.parent.as_str())).collect();
    fn walk<'a>(a: &'a Worker, w: &'a [Worker], out: &mut Vec<&'a Worker>, seen: &mut Vec<&'a str>) {
        if seen.contains(&a.id.as_str()) {
            return;
        }
        seen.push(&a.id);
        out.push(a);
        for c in w.iter().filter(|c| c.parent == a.id) {
            walk(c, w, out, seen);
        }
    }
    let mut seen = vec![];
    for r in roots {
        walk(r, w, &mut out, &mut seen);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Replays a recorded run (`tests/data/h0-*.jsonl`: the hooks of a real Claude Code 2.1.326 run, redacted).
    fn replay(name: &str) -> (Agents, Vec<Worker>, Vec<(u64, String)>) {
        let path = format!("{}/tests/data/{name}", env!("CARGO_MANIFEST_DIR"));
        let text = std::fs::read_to_string(path).unwrap();
        let (mut ag, mut w, mut seen) = (Agents::default(), vec![], vec![]);
        for line in text.lines() {
            let r = Json::parse(line).unwrap();
            if r.get("kind").and_then(Json::str) != Some("hook") {
                continue;
            }
            let i = r.get("input").unwrap();
            let ev = i.get("hook_event_name").and_then(Json::str).unwrap().to_string();
            let t = r.get("t_ms").and_then(Json::num).unwrap() as u64;
            ag.hook(&mut w, &ev, i, t);
            seen.push((t, ev));
        }
        (ag, w, seen)
    }

    #[test]
    fn replays_three_parallel_a_nested_one_and_a_fork() {
        let (_, w, _) = replay("h0-run1.jsonl");
        // Four subagents and the fork; the five internal agents (stop without start) are not there.
        assert_eq!(w.len(), 5, "{w:#?}");
        assert!(w.iter().all(|a| a.ended_ms.is_some()), "all finished");
        let gp = w.iter().find(|a| a.kind == "general-purpose").unwrap();
        assert_eq!((gp.parent.as_str(), gp.depth, gp.background), ("", 1, true));
        assert_eq!(gp.model, "claude-haiku-5-5");
        let nested: Vec<_> = w.iter().filter(|a| a.parent == gp.id).collect();
        assert_eq!(nested.len(), 1);
        assert_eq!((nested[0].kind.as_str(), nested[0].depth), ("Explore", 2));
        let explore_top = w.iter().filter(|a| a.kind == "Explore" && a.parent.is_empty()).count();
        assert_eq!(explore_top, 2);
        let fork = w.iter().find(|a| a.fork).unwrap();
        assert_eq!(fork.kind, "fork");
        assert!(w.iter().filter(|a| !a.fork).all(|a| a.calls >= 1), "each did tool calls: {w:#?}");
        // Display order: the nested one right after its parent.
        let t = tree(&w);
        let p = t.iter().position(|a| a.id == gp.id).unwrap();
        assert_eq!(t[p + 1].parent, gp.id);
    }

    #[test]
    fn replays_worktree_isolation() {
        let (_, w, _) = replay("h0-run2.jsonl");
        assert_eq!(w.len(), 1);
        assert!(w[0].worktree && w[0].background && w[0].ended_ms.is_some());
        assert_eq!(w[0].calls, 3, "Read, Bash, SubagentHandback");
    }

    fn j(t: &str) -> Json {
        Json::parse(t).unwrap()
    }

    #[test]
    fn current_tool_questions_failures_and_arrival_order() {
        let (mut ag, mut w) = (Agents::default(), vec![]);
        // The parent's PostToolUse before the child's start (out of order).
        ag.hook(
            &mut w,
            "PreToolUse",
            &j(r#"{"tool_name":"Agent","tool_use_id":"t1","tool_input":{"subagent_type":"Explore"}}"#),
            1,
        );
        ag.hook(&mut w, "PostToolUse", &j(r#"{"tool_name":"Agent","tool_use_id":"t1","tool_input":{"subagent_type":"Explore"},"tool_response":{"status":"completed","agentId":"a1","resolvedModel":"claude-sonnet-5-5"}}"#), 2);
        ag.hook(&mut w, "SubagentStart", &j(r#"{"agent_id":"a1","agent_type":"Explore"}"#), 3);
        assert_eq!(w.len(), 1);
        assert!(w[0].ended_ms.is_none() && !w[0].background);
        assert_eq!(w[0].model, "claude-sonnet-5-5");
        // Its current tool and target (the last path part only).
        ag.hook(
            &mut w,
            "PreToolUse",
            &j(r#"{"agent_id":"a1","tool_name":"Read","tool_input":{"file_path":"/x/y/src/app.rs"}}"#),
            4,
        );
        assert_eq!((w[0].tool.as_str(), w[0].target.as_str()), ("Read", "app.rs"));
        // A question of this agent; another agent's tool ending does not close it.
        ag.hook(&mut w, "SubagentStart", &j(r#"{"agent_id":"a2","agent_type":"Plan"}"#), 5);
        ag.hook(&mut w, "PermissionRequest", &j(r#"{"agent_id":"a1","tool_name":"Bash"}"#), 6);
        ag.hook(&mut w, "PostToolUse", &j(r#"{"agent_id":"a2","tool_name":"Glob"}"#), 7);
        assert!(w.iter().find(|a| a.id == "a1").unwrap().asking);
        ag.hook(&mut w, "PostToolUse", &j(r#"{"agent_id":"a1","tool_name":"Bash"}"#), 8);
        let a1 = w.iter().find(|a| a.id == "a1").unwrap();
        assert!(!a1.asking && a1.tool.is_empty() && a1.calls == 1);
        // The description of the call and the STATUS of the last message.
        ag.hook(&mut w, "PreToolUse", &j(r#"{"tool_name":"Agent","tool_use_id":"t5","tool_input":{"subagent_type":"Plan","description":"Plan the port"}}"#), 8);
        ag.hook(&mut w, "PostToolUse", &j(r#"{"tool_name":"Agent","tool_use_id":"t5","tool_response":{"status":"async_launched","agentId":"a5"}}"#), 8);
        ag.hook(&mut w, "SubagentStop", &j(r#"{"agent_id":"a5","agent_type":"Plan","last_assistant_message":"Done.\nSTATUS: blocked\nBLOCKERS: x"}"#), 8);
        let a5 = w.iter().find(|a| a.id == "a5").unwrap();
        assert_eq!((a5.description.as_str(), a5.status.as_str()), ("Plan the port", "blocked"));
        // A failed Agent call: one failed line.
        ag.hook(
            &mut w,
            "PostToolUseFailure",
            &j(r#"{"tool_name":"Agent","tool_use_id":"t9","tool_input":{"subagent_type":"Plan"}}"#),
            9,
        );
        assert!(w.iter().any(|a| a.failed && a.kind == "Plan" && a.ended_ms == Some(9)));
        // An internal agent's stop changes nothing; a stop ends; a second start resumes.
        assert!(!ag.hook(&mut w, "SubagentStop", &j(r#"{"agent_id":"zz","agent_type":""}"#), 10));
        ag.hook(&mut w, "SubagentStop", &j(r#"{"agent_id":"a2","agent_type":"Plan"}"#), 11);
        assert_eq!(w.iter().find(|a| a.id == "a2").unwrap().ended_ms, Some(11));
        ag.hook(&mut w, "SubagentStart", &j(r#"{"agent_id":"a2","agent_type":"Plan"}"#), 12);
        assert!(w.iter().find(|a| a.id == "a2").unwrap().ended_ms.is_none());
        // Running first.
        assert!(w[0].ended_ms.is_none() && w[1].ended_ms.is_none());
        // /clear empties the table.
        ag.hook(&mut w, "SessionStart", &j(r#"{"source":"clear"}"#), 13);
        assert!(w.is_empty());
    }

    #[test]
    fn keeps_at_most_200_finished() {
        let (mut ag, mut w) = (Agents::default(), vec![]);
        for n in 0..250u64 {
            let id = format!("a{n}");
            ag.hook(&mut w, "SubagentStart", &j(&format!(r#"{{"agent_id":"{id}","agent_type":"Explore"}}"#)), n * 10);
            ag.hook(&mut w, "SubagentStop", &j(&format!(r#"{{"agent_id":"{id}"}}"#)), n * 10 + 5);
        }
        ag.hook(&mut w, "SubagentStart", &j(r#"{"agent_id":"live","agent_type":"Explore"}"#), 9_999);
        assert_eq!(w.len(), 1 + KEEP_DONE);
        assert_eq!(w[0].id, "live");
        assert_eq!(w[1].id, "a249", "the newest finished next");
    }
}
