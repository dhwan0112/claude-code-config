//! Memory per session, without a window (Linux: reads `/proc/self/status`).
//!
//! ```text
//! cargo run --release -p haetty-core --example mem -- [SESSIONS] [SCROLLBACK_LIMIT_BYTES] [LINES]
//! ```
//! Spawns SESSIONS sessions (120x36) one after another; each prints LINES
//! lines of 119 characters and then waits. After each session the process's
//! anonymous RSS and the session's history length are printed.
//! `SessionConfig::scrollback_limit` is a byte limit, not a line count.
//! Without arguments: 4 sessions, the default limit, 10 000 lines.
//! `HAETTY_ENGINE=native` runs the sessions on haetty's own engine.

use std::time::{Duration, Instant};

use haetty_core::{CommandSpec, Session, SessionConfig};

fn status_kb(field: &str) -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with(field))
                .and_then(|l| l.split_whitespace().nth(1).and_then(|v| v.parse().ok()))
        })
        .unwrap_or(0)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = |i: usize, default: usize| args.get(i).and_then(|s| s.parse().ok()).unwrap_or(default);
    let (sessions, scrollback, lines) = (arg(0, 4), arg(1, haetty_core::DEFAULT_SCROLLBACK_LIMIT), arg(2, 10_000));
    println!("scrollback {scrollback} bytes, {lines} lines of 119 chars per session");
    println!("before: RssAnon {} kB", status_kb("RssAnon:"));
    let mut keep = vec![];
    for i in 1..=sessions {
        let script = format!("seq -f '%0119g' 1 {lines}; echo DONE; exec sleep 3600");
        let mut cfg = SessionConfig::new(CommandSpec::new("sh").args(["-c", script.as_str()]));
        cfg.scrollback_limit = scrollback;
        if let Some(e) = std::env::var("HAETTY_ENGINE").ok().and_then(|e| haetty_core::vt::EngineKind::parse(&e)) {
            cfg.engine = e;
        }
        let s = Session::spawn(cfg).expect("spawn");
        let end = Instant::now() + Duration::from_secs(60);
        while !s.handle.snapshot().expect("snapshot").contains("DONE") && Instant::now() < end {
            let _ = s.events.recv_timeout(Duration::from_millis(50));
        }
        std::thread::sleep(Duration::from_millis(300));
        let snap = s.handle.snapshot().expect("snapshot");
        let history = snap.scrollbar.total.saturating_sub(snap.scrollbar.len);
        println!(
            "session {i}: RssAnon {} kB, history {history} rows (scrollbar total {}, visible {})",
            status_kb("RssAnon:"),
            snap.scrollbar.total,
            snap.scrollbar.len
        );
        keep.push(s);
    }
}
