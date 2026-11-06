//! A daemon that dies (killed by its own PID — stage S4): the client
//! notices (`is_lost`, then `Exited` for every session it shows) and the
//! sessions' programs end with it — the child and a grandchild — so no
//! orphan keeps running. The real `haetty daemon` binary in a test run
//! directory. Linux (SIGKILL; the closed PTY hangs the programs up) and
//! Windows (TerminateProcess; the pseudo console closes) — T5.

mod common;

use std::time::{Duration, Instant};

use common::{alive, children, connect, daemon, endpoint, spec};

/// (program, args) of a session whose program starts a grandchild, and the
/// grandchild's PID once it runs.
#[cfg(unix)]
fn session(dir: &std::path::Path) -> (String, Vec<String>, std::path::PathBuf) {
    let gpid = dir.join("grandchild.pid");
    let script = format!("sleep 600 & echo $! > '{}'; wait", gpid.display());
    ("sh".into(), vec!["-c".into(), script], gpid)
}

#[cfg(windows)]
fn session(dir: &std::path::Path) -> (String, Vec<String>, std::path::PathBuf) {
    ("cmd.exe".into(), vec!["/c".into(), "ping -n 600 127.0.0.1 > nul".into()], dir.join("unused"))
}

fn grandchild(child: u32, _gpid: &std::path::Path) -> u32 {
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        #[cfg(unix)]
        if let Ok(s) = std::fs::read_to_string(_gpid) {
            if s.ends_with('\n') {
                return s.trim().parse().unwrap();
            }
        }
        #[cfg(windows)]
        if let Some(p) = children(child).into_iter().next() {
            return p;
        }
        let _ = child;
        assert!(Instant::now() < end, "the grandchild did not start");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn a_killed_daemon_is_noticed_and_leaves_no_orphans() {
    let dir = haetty_testkit::run_dir("daemoncrash");
    let ep = endpoint(&dir, "daemoncrash");
    let mut d = daemon(&dir, &ep);
    let client = connect(&ep, "crash-test");
    assert_eq!(client.daemon_pid, d.id());
    let (prog, args, gpid) = session(&dir);
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let id = client.create(spec(&prog, &args)).unwrap();
    let s = client.attach(&id, None).unwrap();
    let child = client.list_sessions().unwrap()[0].pid.expect("child pid");
    let grandchild = grandchild(child, &gpid);
    assert!(alive(child) && alive(grandchild));
    assert!(!client.is_lost());
    // Everything the daemon started (Windows: also the console host).
    let started: Vec<u32> = children(d.id());

    d.kill().unwrap(); // this daemon's own PID
    let _ = d.wait();

    let end = Instant::now() + Duration::from_secs(10);
    let mut exited = false;
    while !exited {
        assert!(Instant::now() < end, "no Exited after the daemon died");
        if let Ok(haetty_core::SessionEvent::Exited(_)) = s.events.recv_timeout(Duration::from_millis(100)) {
            exited = true;
        }
    }
    assert!(client.is_lost(), "the client knows the daemon is gone");
    let end = Instant::now() + Duration::from_secs(10);
    let left = || -> Vec<u32> { started.iter().chain([&child, &grandchild]).copied().filter(|p| alive(*p)).collect() };
    while !left().is_empty() {
        assert!(Instant::now() < end, "orphans still running: {:?} (child {child}, grandchild {grandchild})", left());
        std::thread::sleep(Duration::from_millis(100));
    }
    haetty_testkit::remove_dir_all(&dir);
}
