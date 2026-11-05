//! Subcommands that run without a window: `daemon` and `status`. They are
//! dispatched before any font or windowing code runs (DESIGN.md §3.1).

use ccterm_proto::paths::Endpoint;
use ccterm_proto::{Client, ConnectError, Role, SessionInfo, SessionState};

/// `ccterm daemon`: serve until idle. Another daemon already listening is
/// not an error (two GUIs may start one at the same time).
pub fn daemon() -> i32 {
    let cfg = match ccterm_daemon::Config::default_for_user() {
        Ok(c) => c,
        Err(e) => {
            log::error!("daemon: {e}");
            return 1;
        }
    };
    match ccterm_daemon::run(cfg) {
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

/// `ccterm status [--json]`: the daemon's sessions, read-only (observe
/// role). Never starts a daemon. Returns the text and the exit code.
pub fn status(json: bool) -> (String, i32) {
    let ep = match Endpoint::default_for_user() {
        Ok(e) => e,
        Err(e) => return (format!("ccterm status: {e}"), 1),
    };
    let client = match Client::connect(&ep, Role::Observe, "status") {
        Ok(c) => c,
        Err(ConnectError::NotRunning) => {
            return (if json { r#"{"daemon_pid":null,"sessions":[]}"#.into() } else { "no daemon running".into() }, 0)
        }
        Err(e) => return (format!("ccterm status: {e}"), 1),
    };
    match client.list_sessions() {
        Ok(list) if json => (status_json(client.daemon_pid, &list), 0),
        Ok(list) => (status_text(client.daemon_pid, &list), 0),
        Err(e) => (format!("ccterm status: {e}"), 1),
    }
}

/// `ccterm status --screen ID`: the session's screen as plain text, read
/// with the observe role (no input possible). For scripts and tests.
pub fn screen(id: &str) -> (String, i32) {
    let ep = match Endpoint::default_for_user() {
        Ok(e) => e,
        Err(e) => return (format!("ccterm status: {e}"), 1),
    };
    let client = match Client::connect(&ep, Role::Observe, "status-screen") {
        Ok(c) => c,
        Err(e) => return (format!("ccterm status: {e}"), 1),
    };
    let s = match client.attach(id, None) {
        Ok(s) => s,
        Err(e) => return (format!("ccterm status: {e}"), 1),
    };
    let end = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while std::time::Instant::now() < end {
        match s.events.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(ccterm_core::SessionEvent::Wakeup) => {
                if let Some(snap) = s.handle.snapshot() {
                    return (snap.text(), 0);
                }
            }
            Ok(ccterm_core::SessionEvent::Exited(_)) => return (format!("ccterm status: no session {id}"), 1),
            _ => {}
        }
    }
    (format!("ccterm status: no screen from session {id}"), 1)
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
                r#"{{"id":{},"title":{},"cwd":{},"state":"{state}","exit_code":{code},"created_ms":{},"created":{},"program":{},"pid":{},"cols":{},"rows":{}}}"#,
                json_str(&s.id),
                json_str(&s.title),
                json_str(&s.cwd),
                s.created_ms,
                json_str(&rfc3339(s.created_ms)),
                json_str(&s.program),
                s.pid.map_or("null".into(), |p| p.to_string()),
                s.cols,
                s.rows
            )
        })
        .collect();
    format!(r#"{{"daemon_pid":{pid},"sessions":[{}]}}"#, sessions.join(","))
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
        };
        let j = status_json(7, &[s]);
        assert!(j.starts_with(r#"{"daemon_pid":7,"sessions":[{"id":"x-1","title":"✳ Claude Code","cwd":"/tmp/p","state":"exited","exit_code":3,"#), "{j}");
        assert!(j.ends_with(r#""program":"claude","pid":42,"cols":120,"rows":36}]}"#), "{j}");
    }
}
