//! Daemon integration: a real daemon on a temporary socket, real sessions.
//! Unix only (uses sh); the Windows equivalent is checked by hand with the
//! GUI (docs/HANDOFF.md).
#![cfg(unix)]

use std::path::PathBuf;
use std::time::{Duration, Instant};

use ccterm_core::KeyInput;
use ccterm_daemon::Config;
use ccterm_proto::paths::Endpoint;
use ccterm_proto::wire::{ClientMsg, CreateSpec, DaemonMsg, Grid, Origin, WireKey};
use ccterm_proto::{Client, RemoteSession, Role};

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ccterm-daemon-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn start(dir: &std::path::Path, idle: Duration) -> (Endpoint, std::thread::JoinHandle<std::io::Result<()>>) {
    let ep = Endpoint(dir.join("rt").join("daemon.sock"));
    let cfg = Config { endpoint: ep.clone(), state_dir: dir.join("state"), idle_exit: Some(idle) };
    let h = ccterm_daemon::spawn(cfg);
    let end = Instant::now() + Duration::from_secs(5);
    while Client::connect(&ep, Role::Observe, "probe").is_err() {
        assert!(Instant::now() < end, "daemon did not come up");
        std::thread::sleep(Duration::from_millis(20));
    }
    (ep, h)
}

fn spec(script: &str) -> CreateSpec {
    CreateSpec {
        program: "sh".into(),
        args: vec!["-c".into(), script.into()],
        cwd: None,
        env: vec![],
        size: Grid { cols: 40, rows: 8, cell_width: 9, cell_height: 18 },
        scrollback_limit: 1_000_000,
        default_fg: [200, 200, 200],
        default_bg: [20, 20, 20],
    }
}

/// Waits (via Wakeup + snapshot) until the screen satisfies `ok`.
fn wait_screen(s: &RemoteSession, what: &str, ok: impl Fn(&ccterm_core::Snapshot) -> bool) -> ccterm_core::Snapshot {
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(ccterm_core::SessionEvent::Wakeup) = s.events.recv_timeout(Duration::from_millis(100)) {
            let snap = s.handle.snapshot().unwrap();
            if ok(&snap) {
                return snap;
            }
        }
        assert!(Instant::now() < end, "timed out waiting for {what}");
    }
}

fn last_counter(snap: &ccterm_core::Snapshot) -> u32 {
    (0..snap.rows)
        .filter_map(|y| snap.row_text(y).strip_prefix("line").and_then(|n| n.trim().parse().ok()))
        .max()
        .unwrap_or(0)
}

#[test]
fn session_survives_a_dead_client_and_reattaches_with_continuing_seq() {
    let dir = tmp("reattach");
    let (ep, daemon) = start(&dir, Duration::from_secs(2));
    let c = Client::connect(&ep, Role::Control, "gui-1").unwrap();
    let id =
        c.create(spec("i=0; while [ $i -lt 2000 ]; do echo line$i; i=$((i+1)); sleep 0.01; done; sleep 30")).unwrap();
    let s = c.attach(&id, None).unwrap();
    let first = wait_screen(&s, "line5", |sn| last_counter(sn) >= 5);
    let (seq1, n1) = (first.seq, last_counter(&first));
    // The client dies; the program keeps printing.
    c.disconnect();
    drop(s);
    drop(c);
    std::thread::sleep(Duration::from_millis(800));
    let c2 = Client::connect(&ep, Role::Control, "gui-2").unwrap();
    let list = c2.list_sessions().unwrap();
    assert!(list.iter().any(|i| i.id == id), "session listed after the client died: {list:?}");
    let s2 = c2.attach(&id, None).unwrap();
    let again = wait_screen(&s2, "a full frame", |sn| sn.rows == 8);
    assert!(again.seq > seq1, "seq continues: {} after {seq1}", again.seq);
    assert!(last_counter(&again) > n1 + 20, "output went on while detached: {} after {n1}", last_counter(&again));
    // Frames keep coming after the reattach.
    let later = wait_screen(&s2, "more output", |sn| last_counter(sn) > last_counter(&again));
    assert!(later.seq > again.seq);
    c2.close(&id).unwrap();
    drop(s2);
    c2.disconnect();
    drop(c2);
    // Idle exit: no sessions, no clients.
    let end = Instant::now() + Duration::from_secs(8);
    while !daemon.is_finished() {
        assert!(Instant::now() < end, "daemon did not exit when idle");
        std::thread::sleep(Duration::from_millis(100));
    }
    daemon.join().unwrap().unwrap();
    let log = std::fs::read_to_string(dir.join("state/events.log")).unwrap();
    assert!(
        log.contains("\tsession_created\t") && log.contains("\tsession_closed\t") && log.contains("\tdaemon_stop\t"),
        "{log}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn observe_and_report_clients_cannot_send_input_or_create() {
    let dir = tmp("roles");
    let (ep, _daemon) = start(&dir, Duration::from_secs(60));
    let ctl = Client::connect(&ep, Role::Control, "gui").unwrap();
    let id = ctl.create(spec("read x; echo got:$x; sleep 30")).unwrap();
    let obs = Client::connect(&ep, Role::Observe, "status").unwrap();
    // Observe may list and watch...
    assert_eq!(obs.list_sessions().unwrap().len(), 1);
    let watched = obs.attach(&id, None).unwrap();
    wait_screen(&watched, "first frame", |sn| sn.rows == 8);
    // ...but not type.
    let k = WireKey::from(&KeyInput::parse("x").unwrap());
    obs.send(&ClientMsg::Key { session: id.clone(), origin: Origin::Human, key: k.clone() }).unwrap();
    obs.send(&ClientMsg::Paste { session: id.clone(), origin: Origin::Human, text: "evil\n".into() }).unwrap();
    obs.send(&ClientMsg::Close { session: id.clone() }).unwrap();
    for what in ["Key", "Paste", "Close"] {
        match obs.next_reply(Duration::from_secs(3)) {
            Some(DaemonMsg::Denied { what: w }) => assert!(w.starts_with(what), "{w}"),
            other => panic!("expected Denied for {what}, got {other:?}"),
        }
    }
    let rep = Client::connect(&ep, Role::Report, "hook").unwrap();
    rep.send(&ClientMsg::Create { req: 1, spec: spec("true") }).unwrap();
    assert!(matches!(rep.next_reply(Duration::from_secs(3)), Some(DaemonMsg::Denied { .. })));
    // The session got nothing: it still waits for its line; control input works.
    std::thread::sleep(Duration::from_millis(300));
    let s = ctl.attach(&id, None).unwrap();
    let snap = wait_screen(&s, "frame", |sn| sn.rows == 8);
    assert!(!snap.text().contains("got:"), "observe input reached the program:\n{}", snap.text());
    s.handle.paste("ok");
    s.handle.key(KeyInput::parse("Enter").unwrap());
    wait_screen(&s, "control input", |sn| sn.text().contains("got:ok"));
    let _ = std::fs::remove_dir_all(&dir);
}
