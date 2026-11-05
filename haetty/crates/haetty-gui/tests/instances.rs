//! Instances (DESIGN.md §3.9): a stable and a `HAETTY_INSTANCE=dev` daemon
//! (the real `haetty` binary) run at the same time without touching each
//! other — separate sockets, state directories, sessions; ending one leaves
//! the other serving. Unix only (uses sh); Windows: by hand (HANDOFF.md).
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use haetty_proto::paths::Endpoint;
use haetty_proto::wire::{CreateSpec, Grid};
use haetty_proto::{Client, Role};

const BIN: &str = env!("CARGO_BIN_EXE_haetty");

fn daemon(dev: bool) -> Child {
    let mut c = Command::new(BIN);
    c.arg("daemon").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    if dev {
        c.env("HAETTY_INSTANCE", "dev");
    }
    c.spawn().expect("start daemon")
}

fn connect(ep: &Endpoint) -> Client {
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        match Client::connect(ep, Role::Control, "test") {
            Ok(c) => return c,
            Err(e) => {
                assert!(Instant::now() < end, "daemon at {} did not come up: {e}", ep.0.display());
                std::thread::sleep(Duration::from_millis(30));
            }
        }
    }
}

fn spec() -> CreateSpec {
    CreateSpec {
        program: "sh".into(),
        args: vec!["-c".into(), "echo instance=[$HAETTY_INSTANCE]; sleep 30".into()],
        cwd: None,
        env: vec![],
        size: Grid { cols: 40, rows: 6, cell_width: 9, cell_height: 18 },
        scrollback_limit: 100_000,
        default_fg: [200, 200, 200],
        default_bg: [20, 20, 20],
        claude_integration: false,
    }
}

fn screen_has(c: &Client, id: &str, text: &str) -> bool {
    let s = c.attach(id, None).unwrap();
    let end = Instant::now() + Duration::from_secs(10);
    while Instant::now() < end {
        let _ = s.events.recv_timeout(Duration::from_millis(100));
        if let Some(snap) = s.handle.snapshot() {
            if (0..snap.rows).any(|y| snap.row_text(y).contains(text)) {
                return true;
            }
        }
    }
    false
}

fn events(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("events.log")).unwrap_or_default()
}

#[test]
fn stable_and_dev_instances_do_not_interfere() {
    // First, before HOME is pointed elsewhere: the guard takes the real home now.
    let tmp: PathBuf = haetty_testkit::run_dir("instances");
    std::fs::create_dir_all(tmp.join("rt")).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(tmp.join("rt"), std::fs::Permissions::from_mode(0o700)).unwrap();
    // SAFETY of the env changes: this test binary has this one test; the
    // daemons inherit the same environment.
    for v in ["HAETTY_SOCKET", "HAETTY_STATE_DIR", "HAETTY_CONFIG_DIR", "HAETTY_INSTANCE", "HAETTY_LOG_FILE"] {
        std::env::remove_var(v);
    }
    std::env::set_var("XDG_RUNTIME_DIR", tmp.join("rt"));
    std::env::set_var("XDG_STATE_HOME", tmp.join("state"));
    std::env::set_var("XDG_CONFIG_HOME", tmp.join("config"));
    std::env::set_var("HOME", &tmp);

    let stable_ep = Endpoint::default_for_user().unwrap();
    std::env::set_var("HAETTY_INSTANCE", "dev");
    let dev_ep = Endpoint::default_for_user().unwrap();
    let dev_state = haetty_proto::paths::state_dir();
    let dev_title = haetty_proto::product::title_prefix();
    let dev_app = haetty_proto::product::app_id();
    std::env::remove_var("HAETTY_INSTANCE");
    let stable_state = haetty_proto::paths::state_dir();
    assert_eq!(stable_ep.0, tmp.join("rt/haetty/daemon.sock"));
    assert_eq!(dev_ep.0, tmp.join("rt/haetty-dev/daemon.sock"));
    assert_eq!((stable_state.clone(), dev_state.clone()), (tmp.join("state/haetty"), tmp.join("state/haetty-dev")));
    assert_eq!((dev_title.as_str(), dev_app.as_str()), ("[dev] ", "io.github.dhwan0112.haetty.dev"));
    assert_eq!(haetty_proto::product::app_id(), "io.github.dhwan0112.haetty");

    let mut stable_d = daemon(false);
    let mut dev_d = daemon(true);
    let stable = connect(&stable_ep);
    let dev = connect(&dev_ep);
    assert_ne!(stable.daemon_pid, dev.daemon_pid, "two daemons");
    let a = stable.create(spec()).unwrap();
    let b = dev.create(spec()).unwrap();
    // Each daemon has only its own session; children know their instance.
    let ids = |c: &Client| c.list_sessions().unwrap().into_iter().map(|s| s.id).collect::<Vec<_>>();
    assert_eq!(ids(&stable), vec![a.clone()]);
    assert_eq!(ids(&dev), vec![b.clone()]);
    assert!(screen_has(&stable, &a, "instance=[]"));
    assert!(screen_has(&dev, &b, "instance=[dev]"));
    // `status` follows HAETTY_INSTANCE.
    let out = Command::new(BIN).args(["status", "--json"]).env("HAETTY_INSTANCE", "dev").output().unwrap();
    let json = String::from_utf8_lossy(&out.stdout);
    assert!(json.contains(&b) && !json.contains(&a), "{json}");
    // Separate event logs.
    let (es, ed) = (events(&stable_state), events(&dev_state));
    assert!(es.contains(&a) && !es.contains(&b), "stable log: {es}");
    assert!(ed.contains(&b) && !ed.contains(&a), "dev log: {ed}");
    // Ending the dev daemon leaves the stable one serving.
    dev_d.kill().unwrap();
    let _ = dev_d.wait();
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(ids(&stable), vec![a.clone()]);
    stable.close(&a).unwrap();
    assert!(stable.sync(Duration::from_secs(5)));
    stable_d.kill().unwrap();
    let _ = stable_d.wait();
    haetty_testkit::remove_dir_all(&tmp);
}
