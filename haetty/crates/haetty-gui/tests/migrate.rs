//! Migration from the former name (DESIGN.md §3.9), with the real binary:
//! the old config file and tab sets are copied when haetty has none (the
//! originals stay, an existing haetty file is never overwritten); a daemon
//! of the old name that is running is reported and left alone; the old
//! `CCTERM_*` environment variables still work and are announced. Unix only.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use haetty_proto::paths::Endpoint;
use haetty_proto::wire::{CreateSpec, Grid};
use haetty_proto::{Client, Role};

const BIN: &str = env!("CARGO_BIN_EXE_haetty");

fn wait_for(ep: &Path) -> Client {
    let ep = Endpoint(ep.to_path_buf());
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        match Client::connect(&ep, Role::Control, "test") {
            Ok(c) => return c,
            Err(e) => {
                assert!(Instant::now() < end, "no daemon at {}: {e}", ep.0.display());
                std::thread::sleep(Duration::from_millis(30));
            }
        }
    }
}

fn daemon(env: &[(&str, &Path)]) -> Child {
    let mut c = Command::new(BIN);
    c.arg("daemon").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    for (k, v) in env {
        c.env(k, v);
    }
    c.spawn().unwrap()
}

#[test]
fn old_settings_are_copied_and_an_old_daemon_is_left_alone() {
    // First, before HOME is pointed elsewhere: the guard takes the real home now.
    let tmp: PathBuf = haetty_testkit::run_dir("migrate");
    let rt = tmp.join("rt");
    std::fs::create_dir_all(&rt).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&rt, std::fs::Permissions::from_mode(0o700)).unwrap();
    // SAFETY of the env changes: this test binary has this one test; the
    // processes it starts inherit the same environment.
    for v in ["SOCKET", "STATE_DIR", "CONFIG_DIR", "INSTANCE", "LOG_FILE", "LOG"] {
        std::env::remove_var(format!("HAETTY_{v}"));
        std::env::remove_var(format!("CCTERM_{v}"));
    }
    std::env::set_var("XDG_RUNTIME_DIR", &rt);
    std::env::set_var("XDG_STATE_HOME", tmp.join("state"));
    std::env::set_var("XDG_CONFIG_HOME", tmp.join("config"));
    std::env::set_var("HOME", &tmp);

    // What ccterm left: a config file and tab sets.
    let old_cfg = tmp.join("config/ccterm/config");
    let old_tabs = tmp.join("state/ccterm/tabs");
    std::fs::create_dir_all(old_cfg.parent().unwrap()).unwrap();
    std::fs::create_dir_all(&old_tabs).unwrap();
    std::fs::write(&old_cfg, "restore-tabs = false\nkeybind = ctrl+shift+n=new_tab\n").unwrap();
    std::fs::write(old_tabs.join("last.tsv"), "# ccterm tabs \"last\"\nn\t/tmp\tbash\t\t\t1\n").unwrap();
    std::fs::write(old_tabs.join("saved.tsv"), "# ccterm tabs \"saved\"\nm\t/tmp\tclaude\t\tabc-1\t0\n").unwrap();

    // An old daemon still running at the old socket (played by this binary
    // with its own directories, so that it does not migrate anything itself).
    let old_sock = rt.join("ccterm/daemon.sock");
    std::fs::create_dir_all(rt.join("ccterm")).unwrap();
    std::fs::set_permissions(rt.join("ccterm"), std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut old = daemon(&[
        ("HAETTY_SOCKET", &old_sock),
        ("HAETTY_STATE_DIR", &tmp.join("old-daemon-state")),
        ("HAETTY_CONFIG_DIR", &tmp.join("old-daemon-config")),
    ]);
    let old_client = wait_for(&old_sock);
    let old_session = old_client
        .create(CreateSpec {
            program: "sh".into(),
            args: vec!["-c".into(), "sleep 30".into()],
            cwd: None,
            env: vec![],
            size: Grid { cols: 40, rows: 6, cell_width: 9, cell_height: 18 },
            scrollback_limit: 100_000,
            default_fg: [200, 200, 200],
            default_bg: [20, 20, 20],
            claude_integration: false,
        })
        .unwrap();

    // First start of haetty (the daemon runs the same migration as the GUI).
    let mut new = daemon(&[]);
    let new_client = wait_for(&rt.join("haetty/daemon.sock"));
    let new_cfg = tmp.join("config/haetty/config");
    let new_tabs = tmp.join("state/haetty/tabs");
    assert_eq!(std::fs::read(&new_cfg).unwrap(), std::fs::read(&old_cfg).unwrap(), "config copied");
    for f in ["last.tsv", "saved.tsv"] {
        assert_eq!(std::fs::read(new_tabs.join(f)).unwrap(), std::fs::read(old_tabs.join(f)).unwrap(), "{f} copied");
    }
    assert!(old_cfg.exists() && old_tabs.join("last.tsv").exists(), "originals stay");
    let log = std::fs::read_to_string(tmp.join("state/haetty/daemon.log")).unwrap();
    assert!(log.contains("copied from ccterm"), "{log}");
    assert!(log.contains("daemon of the old name (ccterm) is running"), "{log}");
    // The old daemon was left alone: its session runs, it still answers.
    let list = old_client.list_sessions().unwrap();
    assert_eq!(list.iter().map(|s| s.id.clone()).collect::<Vec<_>>(), vec![old_session.clone()]);
    assert!(new_client.list_sessions().unwrap().is_empty(), "the new daemon has its own sessions");

    // A second start never overwrites what haetty has now.
    drop(new_client);
    new.kill().unwrap();
    let _ = new.wait();
    std::fs::write(&new_cfg, "restore-tabs = true\n").unwrap();
    std::fs::write(old_cfg.clone(), "restore-tabs = false\n# changed later in ccterm\n").unwrap();
    let mut again = daemon(&[]);
    let c = wait_for(&rt.join("haetty/daemon.sock"));
    assert_eq!(std::fs::read_to_string(&new_cfg).unwrap(), "restore-tabs = true\n", "not overwritten");
    drop(c);

    // The old CCTERM_* names still work and are announced once.
    let out = Command::new(BIN)
        .args(["status", "--json"])
        .env("CCTERM_SOCKET", &old_sock)
        .env("CCTERM_LOG", "warn")
        .output()
        .unwrap();
    let (stdout, stderr) = (String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(stdout.contains(&old_session), "CCTERM_SOCKET read as HAETTY_SOCKET: {stdout} {stderr}");
    assert!(stderr.contains("CCTERM_SOCKET is the old name of HAETTY_SOCKET"), "{stderr}");
    assert_eq!(stderr.matches("CCTERM_SOCKET is the old name").count(), 1, "{stderr}");

    old_client.close(&old_session).unwrap();
    for p in [&mut old, &mut again] {
        let _ = p.kill();
        let _ = p.wait();
    }
    haetty_testkit::remove_dir_all(&tmp);
}
