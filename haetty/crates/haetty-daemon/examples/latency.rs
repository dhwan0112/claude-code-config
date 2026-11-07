//! Key → frame latency through the daemon (DESIGN.md §3.4).
//!
//! ```text
//! cargo run --release -p haetty-daemon --example latency -- [N]
//! ```
//! Starts a daemon on a temporary socket, runs `cat` (the PTY echoes) in a
//! 120x36 session, then N times: send a key from a control client and wait
//! for the frame in which the echoed character appears. Prints median, p95
//! and max. Does not include the GUI's drawing (logged separately at trace
//! level as "frame drawn in"). `HAETTY_ENGINE=native` runs the session on
//! haetty's own engine.

use std::time::{Duration, Instant};

use haetty_core::{KeyInput, SessionEvent};
use haetty_daemon::Config;
use haetty_proto::paths::Endpoint;
use haetty_proto::wire::{CreateSpec, Grid};
use haetty_proto::Origin;
use haetty_proto::{Client, Role};

fn main() {
    let n: usize = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(200);
    // Writes only inside haetty_testkit's run directory (never the real home).
    let dir = haetty_testkit::run_dir("latency");
    #[cfg(unix)]
    let ep = Endpoint(dir.join("rt").join("daemon.sock"));
    #[cfg(windows)]
    let ep = Endpoint(format!(r"\\.\pipe\haetty-latency-{}", std::process::id()).into());
    let _d = haetty_daemon::spawn(Config {
        endpoint: ep.clone(),
        state_dir: dir.join("state"),
        run_dir: dir.join("run"),
        idle_exit: None,
    });
    let c = loop {
        match Client::connect(&ep, Role::Control, "latency") {
            Ok(c) => break c,
            Err(_) => std::thread::sleep(Duration::from_millis(20)),
        }
    };
    let (program, args) = if cfg!(windows) { ("cmd.exe".to_string(), vec![]) } else { ("cat".to_string(), vec![]) };
    let id = c
        .create(CreateSpec {
            program,
            args,
            cwd: None,
            env: vec![],
            size: Grid { cols: 120, rows: 36, cell_width: 9, cell_height: 18 },
            scrollback_limit: 12_000_000,
            default_fg: [220, 220, 220],
            default_bg: [30, 30, 30],
            palette: vec![],
            cursor_color: None,
            claude_integration: false,
            engine: std::env::var("HAETTY_ENGINE").unwrap_or_default(),
        })
        .expect("create");
    let s = c.attach(&id, None).expect("attach");
    // Settle: first frames.
    let end = Instant::now() + Duration::from_secs(3);
    while Instant::now() < end {
        if let Ok(SessionEvent::Wakeup) = s.events.recv_timeout(Duration::from_millis(200)) {
            s.handle.snapshot();
        }
    }
    let mut times = vec![];
    for i in 0..n {
        let before = s.handle.snapshot().map(|sn| (sn.cursor.map(|c| (c.x, c.y)), sn.seq)).unwrap();
        let ch = (b'a' + (i % 26) as u8) as char;
        let t0 = Instant::now();
        s.handle.key(Origin::Human, KeyInput::parse(&ch.to_string()).unwrap());
        loop {
            match s.events.recv_timeout(Duration::from_secs(2)) {
                Ok(SessionEvent::Wakeup) => {
                    let sn = s.handle.snapshot().unwrap();
                    if sn.cursor.map(|c| (c.x, c.y)) != before.0 {
                        times.push(t0.elapsed());
                        break;
                    }
                }
                Ok(_) => {}
                Err(_) => panic!("no echo for key {i}"),
            }
        }
        if i % 100 == 99 {
            s.handle.key(Origin::Human, KeyInput::parse("Enter").unwrap());
            std::thread::sleep(Duration::from_millis(50));
            while let Ok(SessionEvent::Wakeup) = s.events.recv_timeout(Duration::from_millis(50)) {
                s.handle.snapshot();
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    times.sort();
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    println!(
        "key -> frame through the daemon, 120x36, {n} keys: median {:.2} ms, p95 {:.2} ms, max {:.2} ms",
        ms(times[n / 2]),
        ms(times[n * 95 / 100]),
        ms(times[n - 1])
    );
    let _ = c.close(&id);
    haetty_testkit::remove_dir_all(&dir);
}
