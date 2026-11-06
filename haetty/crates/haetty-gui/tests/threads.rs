//! Sessions leave no threads behind (stage S4 added a PTY writer thread per
//! session): 60 sessions opened and closed in a real `haetty daemon`
//! process — a third whose program floods terminal queries and never reads
//! its input (the writer may sit in a blocked `write`), a third still
//! printing, a third already ended — and the daemon's thread count returns
//! to where it was. Unix only (/proc, sh).
#![cfg(unix)]

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use haetty_proto::paths::Endpoint;
use haetty_proto::wire::{CreateSpec, Grid};
use haetty_proto::{Client, Role};

const BIN: &str = env!("CARGO_BIN_EXE_haetty");

fn threads(pid: u32) -> usize {
    std::fs::read_dir(format!("/proc/{pid}/task")).map_or(0, |d| d.count())
}

/// Thread names and how many of each.
fn names(pid: u32) -> std::collections::BTreeMap<String, usize> {
    let mut m = std::collections::BTreeMap::new();
    for e in std::fs::read_dir(format!("/proc/{pid}/task")).into_iter().flatten().flatten() {
        let n = std::fs::read_to_string(e.path().join("comm")).unwrap_or_default().trim().to_string();
        *m.entry(n).or_insert(0) += 1;
    }
    m
}

fn spec(script: &str) -> CreateSpec {
    CreateSpec {
        program: "sh".into(),
        args: vec!["-c".into(), script.into()],
        cwd: None,
        env: vec![],
        size: Grid { cols: 40, rows: 8, cell_width: 9, cell_height: 18 },
        scrollback_limit: 100_000,
        default_fg: [200, 200, 200],
        default_bg: [20, 20, 20],
        palette: vec![],
        cursor_color: None,
        claude_integration: false,
    }
}

#[test]
fn closed_sessions_leave_no_threads() {
    let dir = haetty_testkit::run_dir("threads");
    let rt = dir.join("rt");
    std::fs::create_dir_all(&rt).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&rt, std::fs::Permissions::from_mode(0o700)).unwrap();
    let ep = Endpoint(rt.join("d.sock"));
    let mut daemon = Command::new(BIN)
        .arg("daemon")
        .env("HAETTY_INSTANCE", "dev")
        .env("HAETTY_SOCKET", &ep.0)
        .env("HAETTY_STATE_DIR", dir.join("state"))
        .env("XDG_RUNTIME_DIR", &rt)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let end = Instant::now() + Duration::from_secs(10);
    let client = loop {
        match Client::connect(&ep, Role::Control, "threads-test") {
            Ok(c) => break c,
            Err(_) if Instant::now() < end => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => panic!("daemon: {e}"),
        }
    };
    let pid = daemon.id();
    // One session opened and closed first: lazily started threads count in
    // the baseline.
    let warm = client.create(spec("sleep 60")).unwrap();
    client.close(&warm).unwrap();
    std::thread::sleep(Duration::from_millis(500));
    let base = threads(pid);
    let scripts = [
        // Floods queries (the answers fill its input) and never reads.
        "stty raw -echo; i=0; while [ $i -lt 4000 ]; do printf '\\033[6n\\033[c'; i=$((i+1)); done; exec sleep 60",
        "while :; do echo out; done",
        "true",
    ];
    for round in 0..20 {
        let ids: Vec<String> = scripts.iter().map(|s| client.create(spec(s)).unwrap()).collect();
        std::thread::sleep(Duration::from_millis(if round % 5 == 0 { 300 } else { 50 }));
        for id in &ids {
            let _ = client.close(id);
        }
    }
    let end = Instant::now() + Duration::from_secs(15);
    loop {
        let now = threads(pid);
        let live = client.list_sessions().unwrap().len();
        if now <= base && live == 0 {
            break;
        }
        assert!(
            Instant::now() < end,
            "daemon threads {base} before, {now} after 60 sessions ({live} still listed): {:?}",
            names(pid)
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = daemon.kill();
    let _ = daemon.wait();
    haetty_testkit::remove_dir_all(&dir);
}
