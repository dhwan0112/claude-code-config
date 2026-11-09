//! `haetty wait` against a real daemon (stage G3). A session the person allowed to observe ends a screen wait when the
//! text is on it; a session nobody allowed exits 3 at once; a wait that never sees its condition exits 124. The event
//! waits take the hooks of a session with Claude Code's integration (a fake `claude` program: Unix only), sent by a
//! `Report` client as the `haetty hook` process does.

mod common;

use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use common::*;

/// The `haetty` command line against the test daemon (as observe.rs does).
fn cli(dir: &std::path::Path, ep: &haetty_proto::paths::Endpoint) -> Command {
    let mut c = Command::new(bin());
    c.env("HAETTY_INSTANCE", "dev").env("HAETTY_SOCKET", &ep.0).env("HAETTY_STATE_DIR", dir.join("state"));
    #[cfg(unix)]
    c.env("XDG_RUNTIME_DIR", dir.join("rt"));
    c.stdin(Stdio::null());
    c
}

/// Runs the command; its exit code and everything it printed.
fn run(cmd: &mut Command) -> (Option<i32>, String) {
    let out = cmd.output().unwrap();
    (out.status.code(), format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)))
}

/// A program that prints `marker` and then waits.
fn marked(marker: &str) -> haetty_proto::wire::CreateSpec {
    #[cfg(unix)]
    {
        spec("sh", &["-c", &format!("echo {marker}; cat > /dev/null")])
    }
    #[cfg(windows)]
    {
        spec("cmd", &["/c", &format!("echo {marker}& ping -n 120 127.0.0.1 > nul")])
    }
}

/// A child process that runs for about `secs` seconds.
fn sleeper(secs: u32) -> Child {
    #[cfg(unix)]
    {
        Command::new("sleep").arg(secs.to_string()).spawn().unwrap()
    }
    #[cfg(windows)]
    {
        Command::new("ping").args(["-n", &(secs + 1).to_string(), "127.0.0.1"]).stdout(Stdio::null()).spawn().unwrap()
    }
}

#[test]
fn a_process_end_or_a_timeout_ends_the_wait() {
    let dir = haetty_testkit::run_dir("wait-cli-pid");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let ep = endpoint(&dir, "wait-pid");

    let mut short = sleeper(1);
    let pid = short.id().to_string();
    let (code, text) = run(cli(&dir, &ep).args(["wait", "pid", pid.as_str()]));
    assert_eq!(code, Some(0), "{text}");
    assert!(text.contains("ended after"), "{text}");
    let _ = short.wait();

    let mut long = sleeper(60);
    let pid = long.id().to_string();
    let (code, text) = run(cli(&dir, &ep).args(["wait", "pid", pid.as_str(), "--timeout", "1"]));
    assert_eq!(code, Some(124), "{text}");
    assert!(text.contains("timed out after 1 s"), "{text}");
    let _ = long.kill();
    let _ = long.wait();
    haetty_testkit::remove_dir_all(&dir);
}

#[test]
fn a_file_text_ends_the_wait_and_a_bad_line_exits_2() {
    let dir = haetty_testkit::run_dir("wait-cli-file");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let ep = endpoint(&dir, "wait-file");
    let path = dir.join("build.log");
    std::fs::write(&path, "compiling\n").unwrap();

    let later = path.clone();
    let writer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(400));
        let mut f = std::fs::OpenOptions::new().append(true).open(&later).unwrap();
        f.write_all(b"BUILD OK\n").unwrap();
    });
    let (code, text) =
        run(cli(&dir, &ep).args(["wait", "file"]).arg(&path).args(["--contains", "BUILD OK", "--timeout", "20"]));
    writer.join().unwrap();
    assert_eq!(code, Some(0), "{text}");
    assert!(text.contains("contains \"BUILD OK\" after"), "{text}");

    let (code, text) =
        run(cli(&dir, &ep).args(["wait", "file"]).arg(&path).args(["--contains", "NEVER", "--timeout", "1"]));
    assert_eq!(code, Some(124), "{text}");

    let (code, text) = run(cli(&dir, &ep).args(["wait", "file"]));
    assert_eq!(code, Some(2), "{text}");
    assert!(text.contains("error: usage: haetty wait"), "{text}");
    haetty_testkit::remove_dir_all(&dir);
}

#[test]
fn a_screen_text_ends_the_wait_only_for_a_session_allowed_to_be_observed() {
    let dir = haetty_testkit::run_dir("wait-cli-screen");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let ep = endpoint(&dir, "wait-screen");
    let mut d = daemon(&dir, &ep);
    let c = connect(&ep, "test-control");
    let shown = c.create(marked("wait-shown-marker")).unwrap();
    let hidden = c.create(marked("wait-hidden-marker")).unwrap();
    c.set_observable(&shown, true).unwrap();
    assert!(c.sync(Duration::from_secs(5)));

    // Not allowed: refused at once with exit 3, not after the timeout.
    let t0 = Instant::now();
    let (code, text) = run(cli(&dir, &ep).args([
        "wait",
        "screen",
        hidden.as_str(),
        "--contains",
        "wait-hidden-marker",
        "--timeout",
        "20",
    ]));
    assert_eq!(code, Some(3), "{text}");
    assert!(text.contains("is not observable"), "{text}");
    assert!(t0.elapsed() < Duration::from_secs(15), "refused late: {:?}", t0.elapsed());

    // Allowed, and the text is on the screen: exit 0.
    let (code, text) = run(cli(&dir, &ep).args([
        "wait",
        "screen",
        shown.as_str(),
        "--contains",
        "wait-shown-marker",
        "--timeout",
        "20",
    ]));
    assert_eq!(code, Some(0), "{text}");
    assert!(text.contains("shows \"wait-shown-marker\" after"), "{text}");

    // Allowed, and the text never comes: exit 124.
    let (code, text) =
        run(cli(&dir, &ep).args(["wait", "screen", shown.as_str(), "--contains", "never-shown", "--timeout", "1"]));
    assert_eq!(code, Some(124), "{text}");

    // A session without Claude Code's integration has no turn events: refused at once with exit 1.
    let t0 = Instant::now();
    let (code, text) = run(cli(&dir, &ep).args(["wait", "event", shown.as_str(), "--stop", "--timeout", "20"]));
    assert_eq!(code, Some(1), "{text}");
    assert!(text.contains("has no Claude Code integration"), "{text}");
    assert!(t0.elapsed() < Duration::from_secs(15), "refused late: {:?}", t0.elapsed());

    drop(c);
    let _ = d.kill();
    let _ = d.wait();
    haetty_testkit::remove_dir_all(&dir);
}

/// A turn end and a permission ask of an observed session, through the hooks the daemon gives Claude Code's sessions.
#[cfg(unix)]
#[test]
fn a_turn_end_and_a_permission_ask_end_the_event_waits() {
    use haetty_proto::wire::{ClientMsg, Secret};
    use haetty_proto::{Client, Role};
    use std::os::unix::fs::PermissionsExt;
    use std::time::{SystemTime, UNIX_EPOCH};

    let dir = haetty_testkit::run_dir("wait-cli-events");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let ep = endpoint(&dir, "wait-events");
    let mut d = daemon(&dir, &ep);
    let c = connect(&ep, "test-control");

    // A program named `claude` gets Claude Code's integration; this one only prints a line and waits.
    let bin_dir = dir.join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let claude = bin_dir.join("claude");
    std::fs::write(&claude, "#!/bin/sh\necho fake-claude-ready\ncat > /dev/null\n").unwrap();
    std::fs::set_permissions(&claude, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut s = spec(claude.to_str().unwrap(), &[]);
    s.claude_integration = true;
    let id = c.create(s).unwrap();
    c.set_observable(&id, true).unwrap();
    assert!(c.sync(Duration::from_secs(5)));

    // The hooks come from a Report client, as `haetty hook` sends them. Each has a later time than the one before,
    // so a Stop after an Ask is not taken as older than the Ask.
    let hooks = Client::connect(&ep, Role::Report, "test-hooks").unwrap();
    let mut ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;
    let mut hook = |event: &str| {
        ts += 1;
        hooks
            .send(&ClientMsg::Hook {
                session: Some(id.clone()),
                event: event.into(),
                json: Secret("{}".into()),
                ts_ms: ts,
                snapshot: None,
            })
            .unwrap();
    };

    // A turn end: the wait sees only events from its start, so hooks go on until it has seen one.
    let deadline = Instant::now() + Duration::from_secs(25);
    let stop_out = dir.join("wait-stop.txt");
    let mut wait_stop = cli(&dir, &ep)
        .args(["wait", "event", id.as_str(), "--stop", "--timeout", "30"])
        .stdout(std::fs::File::create(&stop_out).unwrap())
        .spawn()
        .unwrap();
    let status = loop {
        if let Some(st) = wait_stop.try_wait().unwrap() {
            break st;
        }
        assert!(Instant::now() < deadline, "the --stop wait never saw the turn end");
        hook("PermissionRequest"); // Wait -> Ask
        hook("Stop"); // Ask -> Wait: a turn end
        std::thread::sleep(Duration::from_millis(100));
    };
    let text = std::fs::read_to_string(&stop_out).unwrap();
    assert_eq!(status.code(), Some(0), "{text}");
    assert!(text.contains("ends its turn after"), "{text}");

    // A permission ask: the same, with the Ask as the event.
    let deadline = Instant::now() + Duration::from_secs(25);
    let ask_out = dir.join("wait-ask.txt");
    let mut wait_ask = cli(&dir, &ep)
        .args(["wait", "event", id.as_str(), "--permission", "--timeout", "30"])
        .stdout(std::fs::File::create(&ask_out).unwrap())
        .spawn()
        .unwrap();
    let status = loop {
        if let Some(st) = wait_ask.try_wait().unwrap() {
            break st;
        }
        assert!(Instant::now() < deadline, "the --permission wait never saw the ask");
        hook("Stop"); // Ask -> Wait
        hook("PermissionRequest"); // Wait -> Ask: the permission wait
        std::thread::sleep(Duration::from_millis(100));
    };
    let text = std::fs::read_to_string(&ask_out).unwrap();
    assert_eq!(status.code(), Some(0), "{text}");
    assert!(text.contains("waits for a permission answer after"), "{text}");

    // Already asking when the wait starts: it counts at once.
    hook("PermissionRequest");
    assert!(hooks.sync(Duration::from_secs(5)));
    let t0 = Instant::now();
    let (code, text) = run(cli(&dir, &ep).args(["wait", "event", id.as_str(), "--permission", "--timeout", "20"]));
    assert_eq!(code, Some(0), "{text}");
    assert!(t0.elapsed() < Duration::from_secs(15), "not at once: {:?}", t0.elapsed());
    assert!(text.contains("waits for a permission answer after"), "{text}");

    drop(hook);
    drop(hooks);
    drop(c);
    let _ = d.kill();
    let _ = d.wait();
    haetty_testkit::remove_dir_all(&dir);
}
