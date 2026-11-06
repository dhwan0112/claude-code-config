//! A daemon that dies (SIGKILL, by PID — stage S4): the client notices
//! (`is_lost`, then `Exited` for every session it shows) and the sessions'
//! programs end with it — the child and a background grandchild — so no
//! orphan keeps running. The real `haetty daemon` binary in a test run
//! directory. Unix only (sh, /proc).
#![cfg(unix)]

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use haetty_proto::paths::Endpoint;
use haetty_proto::wire::{CreateSpec, Grid};
use haetty_proto::{Client, Role};

const BIN: &str = env!("CARGO_BIN_EXE_haetty");

fn running(pid: u32) -> bool {
    // A zombie is not running (state Z); its parent was the daemon.
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .is_ok_and(|s| s.rsplit(')').next().is_some_and(|r| !r.trim_start().starts_with('Z')))
}

fn wait_file(p: &Path) -> String {
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(s) = std::fs::read_to_string(p) {
            if s.ends_with('\n') {
                return s.trim().to_string();
            }
        }
        assert!(Instant::now() < end, "{} did not appear", p.display());
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_killed_daemon_is_noticed_and_leaves_no_orphans() {
    let dir = haetty_testkit::run_dir("daemoncrash");
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
        match Client::connect(&ep, Role::Control, "crash-test") {
            Ok(c) => break c,
            Err(_) if Instant::now() < end => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => panic!("daemon: {e}"),
        }
    };
    assert_eq!(client.daemon_pid, daemon.id());
    let gpid = dir.join("grandchild.pid");
    let script = format!("sleep 600 & echo $! > '{}'; wait", gpid.display());
    let id = client
        .create(CreateSpec {
            program: "sh".into(),
            args: vec!["-c".into(), script],
            cwd: None,
            env: vec![],
            size: Grid { cols: 40, rows: 8, cell_width: 9, cell_height: 18 },
            scrollback_limit: 100_000,
            default_fg: [200, 200, 200],
            default_bg: [20, 20, 20],
            palette: vec![],
            cursor_color: None,
            claude_integration: false,
        })
        .unwrap();
    let s = client.attach(&id, None).unwrap();
    let child = client.list_sessions().unwrap()[0].pid.expect("child pid");
    let grandchild: u32 = wait_file(&gpid).parse().unwrap();
    assert!(running(child) && running(grandchild));
    assert!(!client.is_lost());

    daemon.kill().unwrap(); // SIGKILL, this daemon's own PID
    let _ = daemon.wait();

    let end = Instant::now() + Duration::from_secs(5);
    let mut exited = false;
    while !exited {
        assert!(Instant::now() < end, "no Exited after the daemon died");
        if let Ok(haetty_core::SessionEvent::Exited(_)) = s.events.recv_timeout(Duration::from_millis(100)) {
            exited = true;
        }
    }
    assert!(client.is_lost(), "the client knows the daemon is gone");
    let end = Instant::now() + Duration::from_secs(5);
    while running(child) || running(grandchild) {
        assert!(
            Instant::now() < end,
            "orphans: child {child} running {}, grandchild {grandchild} running {}",
            running(child),
            running(grandchild)
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    haetty_testkit::remove_dir_all(&dir);
}
