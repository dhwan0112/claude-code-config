//! Subcommands that run without a window: `daemon` and `status`. They are
//! dispatched before any font or windowing code runs (DESIGN.md §3.1).

use haetty_proto::paths::Endpoint;
use haetty_proto::wire::{LogEvent, ScreenView};
use haetty_proto::{Client, ConnectError, ObserverClient, Role, SessionInfo, SessionState};

/// `haetty daemon`: serve until idle. Another daemon already listening is
/// not an error (two GUIs may start one at the same time).
pub fn daemon() -> i32 {
    let cfg = match haetty_daemon::Config::default_for_user() {
        Ok(c) => c,
        Err(e) => {
            log::error!("daemon: {e}");
            return 1;
        }
    };
    match haetty_daemon::run(cfg) {
        Ok(()) => 0,
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            log::info!("daemon: {e}; exiting");
            0
        }
        Err(e) => {
            log::error!("daemon: {e}");
            1
        }
    }
}

/// `haetty status [--json]`: the daemon's sessions, read-only (observe
/// role). Never starts a daemon. Returns the text and the exit code.
pub fn status(json: bool) -> (String, i32) {
    let ep = match Endpoint::default_for_user() {
        Ok(e) => e,
        Err(e) => return (format!("haetty status: {e}"), 1),
    };
    let client = match Client::connect(&ep, Role::Observe, "status") {
        Ok(c) => c,
        Err(ConnectError::NotRunning) => {
            let w = workers_json(&crate::tasks::load_all(&crate::tasks::dir()), &[]);
            return (
                if json {
                    format!(r#"{{"daemon_pid":null,"sessions":[],"workers":{w}}}"#)
                } else {
                    "no daemon running".into()
                },
                0,
            );
        }
        Err(e) => return (format!("haetty status: {e}"), 1),
    };
    match client.list_sessions() {
        Ok(list) if json => {
            let mut s = status_json(client.daemon_pid, &list);
            let w = workers_json(&crate::tasks::load_all(&crate::tasks::dir()), &list);
            s.truncate(s.len() - 1);
            (format!("{s},\"workers\":{w}}}"), 0)
        }
        Ok(list) => (status_text(client.daemon_pid, &list), 0),
        Err(e) => (format!("haetty status: {e}"), 1),
    }
}

/// `haetty status --screen ID`: the session's screen as plain text, read
/// with the observe role (no input possible). For scripts and tests.
pub fn screen(id: &str) -> (String, i32) {
    let ep = match Endpoint::default_for_user() {
        Ok(e) => e,
        Err(e) => return (format!("haetty status: {e}"), 1),
    };
    let client = match Client::connect(&ep, Role::Observe, "status-screen") {
        Ok(c) => c,
        Err(e) => return (format!("haetty status: {e}"), 1),
    };
    let s = match client.attach(id, None) {
        Ok(s) => s,
        Err(e) => return (format!("haetty status: {e}"), 1),
    };
    let end = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while std::time::Instant::now() < end {
        match s.events.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(haetty_core::SessionEvent::Wakeup) => {
                if let Some(snap) = s.handle.snapshot() {
                    return (snap.text(), 0);
                }
            }
            Ok(haetty_core::SessionEvent::Exited(_)) => return (format!("haetty status: no session {id}"), 1),
            _ => {}
        }
    }
    (format!("haetty status: no screen from session {id}"), 1)
}

/// An observer connection for `events` / `screen` (never starts a daemon).
pub(crate) fn observer(cmd: &str) -> Result<ObserverClient, String> {
    let ep = Endpoint::default_for_user().map_err(|e| format!("haetty {cmd}: {e}"))?;
    ObserverClient::connect(&ep, cmd).map_err(|e| match e {
        ConnectError::NotRunning => format!("haetty {cmd}: no daemon running"),
        e => format!("haetty {cmd}: {e}"),
    })
}

/// `haetty events [--follow] [--json] [--since CURSOR]` (DESIGN.md §6.1):
/// the event log after CURSOR, only events of sessions while they were
/// observable — derived fields, never typed or shown text. One line per
/// event (`--json`: one JSON object per line); `--follow` keeps printing new
/// ones until the daemon ends.
pub fn events(follow: bool, json: bool, since: u64, out: &mut dyn std::io::Write) -> Result<(), String> {
    let mut o = observer("events")?;
    let line = |e: &LogEvent| if json { event_json(e) } else { event_text(e) };
    if follow {
        let mut failed = None;
        o.follow(since, |e| match writeln!(out, "{}", line(e)).and_then(|_| out.flush()) {
            Ok(()) => true,
            Err(err) => {
                failed = Some(err);
                false
            }
        })
        .map_err(|e| format!("haetty events: {e}"))?;
        // A closed pipe (`| head`) ends quietly.
        return match failed {
            Some(e) if e.kind() != std::io::ErrorKind::BrokenPipe => Err(format!("haetty events: {e}")),
            _ => Ok(()),
        };
    }
    let (list, _) = o.events(since).map_err(|e| format!("haetty events: {e}"))?;
    for e in &list {
        if writeln!(out, "{}", line(e)).is_err() {
            break;
        }
    }
    Ok(())
}

pub fn event_json(e: &LogEvent) -> String {
    let fields: Vec<String> = e.fields.iter().map(|(k, v)| format!("{}:{}", json_str(k), json_str(v))).collect();
    format!(
        r#"{{"seq":{},"ms":{},"time":{},"kind":{},"session":{},"fields":{{{}}}}}"#,
        e.seq,
        e.ms,
        json_str(&rfc3339(e.ms)),
        json_str(&e.kind),
        json_str(&e.session),
        fields.join(",")
    )
}

fn event_text(e: &LogEvent) -> String {
    let mut s = format!("{}  {}  {}  {}", e.seq, rfc3339(e.ms), e.kind, e.session);
    for (k, v) in &e.fields {
        s.push_str(&format!("  {k}={v}"));
    }
    s
}

/// `haetty screen [--json] SESSION` (DESIGN.md §6.1): the screen text and
/// state of a session the person made observable; refused otherwise.
pub fn observe_screen(id: &str, json: bool) -> (String, i32) {
    let mut o = match observer("screen") {
        Ok(o) => o,
        Err(e) => return (e, 1),
    };
    match o.screen(id) {
        Ok(v) if json => (screen_json(&v), 0),
        Ok(v) => (screen_text(&v), 0),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => (
            format!(
                "haetty screen: session {id} is not observable (allow it in its tab: palette \"Observing: allow or stop for this tab\")"
            ),
            3,
        ),
        Err(e) => (format!("haetty screen: {e}"), 1),
    }
}

pub fn screen_json(v: &ScreenView) -> String {
    let (state, code) = match v.state {
        SessionState::Running => ("running", "null".to_string()),
        SessionState::Exited { code } => ("exited", code.map_or("null".into(), |c| c.to_string())),
    };
    let cursor = v.cursor.map_or("null".into(), |(x, y)| format!(r#"{{"x":{x},"y":{y}}}"#));
    let claude = v.claude.as_ref().map_or("null".into(), |c| {
        format!(
            r#"{{"attention":"{:?}","busy":{},"turns":{},"name":{},"model":{},"report_status":{}}}"#,
            c.attention,
            c.busy,
            c.turns,
            json_str(&c.name),
            json_str(&c.model),
            c.report_status.as_deref().map_or("null".into(), json_str)
        )
    });
    let lines: Vec<String> = v.text.0.split('\n').map(json_str).collect();
    format!(
        r#"{{"id":{},"program":{},"title":{},"cwd":{},"state":"{state}","exit_code":{code},"cols":{},"rows":{},"cursor":{cursor},"claude":{claude},"lines":[{}]}}"#,
        json_str(&v.id),
        json_str(&v.program),
        json_str(&v.title),
        json_str(&v.cwd),
        v.cols,
        v.rows,
        lines.join(",")
    )
}

fn screen_text(v: &ScreenView) -> String {
    let state = match v.state {
        SessionState::Running => "running".to_string(),
        SessionState::Exited { code } => format!("exited {}", code.map_or("?".into(), |c| c.to_string())),
    };
    let mut s = format!("{}  {}  {}  {}x{}  {}\ntitle: {}\n", v.id, v.program, state, v.cols, v.rows, v.cwd, v.title);
    if let Some(c) = &v.claude {
        s.push_str(&format!(
            "claude: attention {:?}, {}, {} turn(s){}\n",
            c.attention,
            if c.busy { "busy" } else { "idle" },
            c.turns,
            c.report_status.as_deref().map(|r| format!(", report {r}")).unwrap_or_default()
        ));
    }
    s.push('\n');
    s.push_str(&v.text.0);
    s
}

fn status_text(pid: u32, list: &[SessionInfo]) -> String {
    let mut out = format!("daemon pid {pid}, {} session(s)", list.len());
    for s in list {
        let state = match s.state {
            SessionState::Running => "running".to_string(),
            SessionState::Exited { code } => format!("exited {}", code.map_or("?".into(), |c| c.to_string())),
        };
        out.push_str(&format!("\n{}  {}  {}  {}  {}", s.id, rfc3339(s.created_ms), state, s.cwd, s.title));
    }
    out
}

pub fn status_json(pid: u32, list: &[SessionInfo]) -> String {
    let sessions: Vec<String> = list
        .iter()
        .map(|s| {
            let (state, code) = match s.state {
                SessionState::Running => ("running", "null".to_string()),
                SessionState::Exited { code } => ("exited", code.map_or("null".into(), |c| c.to_string())),
            };
            format!(
                r#"{{"id":{},"title":{},"cwd":{},"state":"{state}","exit_code":{code},"created_ms":{},"created":{},"program":{},"pid":{},"cols":{},"rows":{},"attached":{},"claude_session":{},"attention":"{:?}","observable":{},"scrollback":{}}}"#,
                json_str(&s.id),
                json_str(&s.title),
                json_str(&s.cwd),
                s.created_ms,
                json_str(&rfc3339(s.created_ms)),
                json_str(&s.program),
                s.pid.map_or("null".into(), |p| p.to_string()),
                s.cols,
                s.rows,
                s.attached,
                json_str(&s.claude_session),
                s.attention,
                s.observable,
                s.scrollback
            )
        })
        .collect();
    format!(r#"{{"daemon_pid":{pid},"sessions":[{}]}}"#, sessions.join(","))
}

/// The orchestrator's tasks with their workers (DESIGN.md §8.3): the
/// session in the task's worktree (a running one first), its state and its
/// latest report.
pub fn workers_json(tasks: &[crate::tasks::Task], list: &[SessionInfo]) -> String {
    let items: Vec<String> = tasks
        .iter()
        .map(|t| {
            let mut mine: Vec<&SessionInfo> =
                list.iter().filter(|s| t.is_folder(&crate::app::osc7_path(&s.cwd))).collect();
            mine.sort_by_key(|s| (s.state != SessionState::Running, std::cmp::Reverse(s.created_ms)));
            let s = mine.first();
            let alive = s.is_some_and(|s| s.state == SessionState::Running);
            let report = s.and_then(|s| s.report.as_ref());
            let state = haetty_cc::orch::WorkerState::of(alive, s.is_some_and(|s| s.busy), report);
            let r = |f: fn(&haetty_proto::wire::WorkerReport) -> &str| report.map_or("null".into(), |x| json_str(f(x)));
            format!(
                r#"{{"id":{},"title":{},"repository":{},"base":{},"branch":{},"worktree":{},"session":{},"state":{},"status":{},"artifact":{},"evidence":{},"blockers":{},"next_action":{},"report_from_transcript":{}}}"#,
                json_str(&t.id),
                json_str(&t.title),
                json_str(&t.repo),
                json_str(&t.base),
                json_str(&t.branch),
                json_str(&t.worktree),
                s.map_or("null".into(), |s| json_str(&s.id)),
                json_str(state.name()),
                r(|x| &x.status),
                r(|x| &x.artifact.0),
                r(|x| &x.evidence.0),
                r(|x| &x.blockers.0),
                r(|x| &x.next_action.0),
                report.map_or("null".into(), |x| x.from_transcript.to_string()),
            )
        })
        .collect();
    format!("[{}]", items.join(","))
}

/// A JSON string literal.
pub fn json_str(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// Unix milliseconds as an RFC 3339 UTC timestamp.
pub fn rfc3339(ms: u64) -> String {
    let secs = (ms / 1000) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_and_dates() {
        assert_eq!(json_str("a\"b\\c\n\u{1}한"), r#""a\"b\\c\n\u0001한""#);
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(1_762_300_800_000), "2025-11-05T00:00:00Z");
        assert_eq!(rfc3339(951_782_400_000), "2000-02-29T00:00:00Z");
        let s = SessionInfo {
            id: "x-1".into(),
            program: "claude".into(),
            title: "✳ Claude Code".into(),
            cwd: "/tmp/p".into(),
            state: SessionState::Exited { code: Some(3) },
            created_ms: 0,
            pid: Some(42),
            cols: 120,
            rows: 36,
            args: vec![],
            attached: 0,
            claude_session: String::new(),
            attention: Default::default(),
            busy: false,
            turns: 0,
            report: None,
            observable: false,
            scrollback: 0,
        };
        let j = status_json(7, &[s]);
        assert!(j.starts_with(r#"{"daemon_pid":7,"sessions":[{"id":"x-1","title":"✳ Claude Code","cwd":"/tmp/p","state":"exited","exit_code":3,"#), "{j}");
        assert!(j.ends_with(r#""program":"claude","pid":42,"cols":120,"rows":36,"attached":0,"claude_session":"","attention":"None","observable":false,"scrollback":0}]}"#), "{j}");
    }

    #[test]
    fn workers_in_status_json() {
        use haetty_proto::wire::{Secret, WorkerReport};
        let t = crate::tasks::Task {
            id: "t1".into(),
            title: "Fix \"it\"".into(),
            repo: "/r".into(),
            base: "main".into(),
            branch: "task/t1-fix-it".into(),
            worktree: "/r-tasks/t1-fix-it".into(),
            ..Default::default()
        };
        let none = workers_json(std::slice::from_ref(&t), &[]);
        assert!(none.contains(r#""session":null,"state":"no report","status":null"#), "{none}");
        let s = SessionInfo {
            id: "x-1".into(),
            program: "claude".into(),
            title: String::new(),
            cwd: "/r-tasks/t1-fix-it".into(),
            state: SessionState::Running,
            created_ms: 1,
            pid: None,
            cols: 80,
            rows: 24,
            args: vec![],
            attached: 1,
            claude_session: "c".into(),
            attention: Default::default(),
            busy: false,
            turns: 1,
            report: Some(WorkerReport {
                status: "verified".into(),
                evidence: Secret("cargo test: ok".into()),
                ..Default::default()
            }),

            observable: false,

            scrollback: 0,
        };
        let j = workers_json(&[t], &[s]);
        assert!(j.starts_with(r#"[{"id":"t1","title":"Fix \"it\"","repository":"/r""#), "{j}");
        assert!(j.contains(r#""session":"x-1","state":"verified","status":"verified""#), "{j}");
        assert!(j.contains(r#""evidence":"cargo test: ok""#), "{j}");
    }
}

/// `haetty tokens`: the token use of Claude Code sessions, from their transcripts (`haetty_cc::transcript`). Read-only:
/// the text and the exit code. With no PATH, the newest transcript under the Claude Code projects folder.
pub fn tokens(top: usize, paths: &[std::path::PathBuf]) -> (String, i32) {
    let files = match transcript_files(paths) {
        Ok(f) => f,
        Err(e) => return (format!("haetty tokens: {e}"), 2),
    };
    if files.is_empty() {
        return ("haetty tokens: no transcript (.jsonl) found; give a file or a folder".into(), 1);
    }
    let mut reports = Vec::with_capacity(files.len());
    for f in &files {
        let name = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let read = std::fs::File::open(f)
            .and_then(|file| haetty_cc::transcript::analyze_reader(&name, std::io::BufReader::new(file)));
        match read {
            Ok(r) => reports.push(r),
            Err(e) => return (format!("haetty tokens: {}: {e}", f.display()), 2),
        }
    }
    let mut text = String::new();
    for r in &reports {
        text.push_str(&haetty_cc::transcript::render(r, top));
        text.push('\n');
    }
    if reports.len() > 1 {
        let all = haetty_cc::transcript::merge(&format!("all {} transcripts", reports.len()), &reports);
        text.push_str(&haetty_cc::transcript::render(&all, top));
    }
    (text.trim_end().to_string(), 0)
}

/// The transcripts to read: the files given, the `.jsonl` files in the folders given (recursively, sorted), or with
/// no PATH the newest `.jsonl` under `~/.claude/projects` (`USERPROFILE` on Windows).
fn transcript_files(paths: &[std::path::PathBuf]) -> Result<Vec<std::path::PathBuf>, String> {
    if paths.is_empty() {
        let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"));
        let Some(home) = home else {
            return Err("no home folder (HOME or USERPROFILE)".into());
        };
        let dir = std::path::PathBuf::from(home).join(".claude").join("projects");
        let mut all = Vec::new();
        collect_jsonl(&dir, 0, &mut all).map_err(|e| format!("{}: {e}", dir.display()))?;
        return Ok(newest(all).into_iter().collect());
    }
    let mut out = Vec::new();
    for p in paths {
        let meta = std::fs::metadata(p).map_err(|e| format!("{}: {e}", p.display()))?;
        if meta.is_dir() {
            let mut found = Vec::new();
            collect_jsonl(p, 0, &mut found).map_err(|e| format!("{}: {e}", p.display()))?;
            found.sort_by(|a, b| a.0.cmp(&b.0));
            out.extend(found.into_iter().map(|(f, _)| f));
        } else {
            out.push(p.clone());
        }
    }
    Ok(out)
}

/// The `.jsonl` files under `dir` with their modification times; folders deeper than 8 levels are not searched, and
/// a subfolder that cannot be read is skipped.
fn collect_jsonl(
    dir: &std::path::Path,
    depth: u32,
    out: &mut Vec<(std::path::PathBuf, std::time::SystemTime)>,
) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            if depth < 8 {
                let _ = collect_jsonl(&path, depth + 1, out);
            }
        } else if path.extension().is_some_and(|e| e == "jsonl") {
            let t = entry.metadata()?.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            out.push((path, t));
        }
    }
    Ok(())
}

/// The file modified last.
fn newest(files: Vec<(std::path::PathBuf, std::time::SystemTime)>) -> Option<std::path::PathBuf> {
    files.into_iter().max_by_key(|(_, t)| *t).map(|(p, _)| p)
}

#[cfg(test)]
mod tokens_tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime};

    #[test]
    fn the_newest_transcript_is_the_one_modified_last() {
        let t = |s: u64| SystemTime::UNIX_EPOCH + Duration::from_secs(s);
        let files =
            vec![(PathBuf::from("a.jsonl"), t(5)), (PathBuf::from("b.jsonl"), t(9)), (PathBuf::from("c.jsonl"), t(7))];
        assert_eq!(newest(files), Some(PathBuf::from("b.jsonl")));
        assert_eq!(newest(Vec::new()), None);
    }
}
