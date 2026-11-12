//! The permission question reaches the daemon before Claude Code can show
//! its dialog (DESIGN.md §3.13): PermissionRequest is a synchronous hook, so
//! Claude Code waits for `haetty hook PermissionRequest` to exit; the hook
//! exits only after the daemon has handled the report. Here the real binary
//! runs the hooks in Claude Code's order with fake JSON, and the daemon's
//! state is read right after each hook process ends. Unix only (sh).
#![cfg(unix)]

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use haetty_core::{MouseAction, MouseButton, MouseInput};
use haetty_daemon::Config;
use haetty_proto::paths::Endpoint;
use haetty_proto::wire::{Attention, ClientMsg, CreateSpec, DaemonMsg, Grid, Origin, Secret, SessionInfo, WireMouse};
use haetty_proto::{Client, Role};

const BIN: &str = env!("CARGO_BIN_EXE_haetty");

/// Runs `haetty hook EVENT` with `json` on stdin; returns how long it took.
fn hook(ep: &Endpoint, session: &str, event: &str, json: &str) -> Duration {
    let t = Instant::now();
    let mut c = Command::new(BIN)
        .args(["hook", event])
        .env("HAETTY_SOCKET", &ep.0)
        .env("HAETTY_SESSION_ID", session)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    c.stdin.take().unwrap().write_all(json.as_bytes()).unwrap();
    let out = c.wait_with_output().unwrap();
    assert!(out.status.success() && out.stdout.is_empty() && out.stderr.is_empty(), "{out:?}");
    t.elapsed()
}

fn info(c: &Client, id: &str) -> SessionInfo {
    c.list_sessions().unwrap().into_iter().find(|s| s.id == id).unwrap()
}

/// A daemon in its own run directory with one session of a fake Claude Code (the integration applies to programs named
/// claude). The daemon thread ends with the test process.
fn fake_claude(name: &str) -> (std::path::PathBuf, Endpoint, Client, String) {
    let dir = haetty_testkit::run_dir(name);
    std::fs::create_dir_all(dir.join("bin")).unwrap();
    let ep = Endpoint(dir.join("rt").join("daemon.sock"));
    let cfg = Config {
        endpoint: ep.clone(),
        state_dir: dir.join("state"),
        run_dir: dir.join("rt"),
        idle_exit: None,
        health_every: haetty_daemon::HEALTH_EVERY,
    };
    drop(haetty_daemon::spawn(cfg));
    let end = Instant::now() + Duration::from_secs(5);
    let gui = loop {
        match Client::connect(&ep, Role::Control, "gui") {
            Ok(c) => break c,
            Err(_) if Instant::now() < end => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => panic!("daemon: {e}"),
        }
    };
    let fake = dir.join("bin/claude");
    std::fs::write(&fake, "#!/bin/sh\nexec sleep 60\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    let id = gui
        .create(CreateSpec {
            program: fake.to_string_lossy().into_owned(),
            args: vec![],
            cwd: None,
            env: vec![],
            size: Grid { cols: 40, rows: 8, cell_width: 9, cell_height: 18 },
            scrollback_limit: 100_000,
            default_fg: [200, 200, 200],
            default_bg: [20, 20, 20],
            palette: vec![],
            cursor_color: None,
            claude_integration: true,
            engine: String::new(),
        })
        .unwrap();
    (dir, ep, gui, id)
}

#[test]
fn permission_request_is_in_the_daemon_before_the_hook_exits() {
    let (dir, ep, gui, id) = fake_claude("permission");
    let path = |p: &Path| p.to_string_lossy().into_owned();
    let mut times = vec![];
    // Claude Code's order for a Bash call that needs permission: PreToolUse
    // (all tools, asynchronous) — PermissionRequest (synchronous) — dialog.
    hook(&ep, &id, "PreToolUseAll", r#"{"session_id":"s","tool_name":"Bash","tool_use_id":"t1"}"#);
    times.push(hook(&ep, &id, "PermissionRequest", r#"{"session_id":"s","tool_name":"Bash","tool_use_id":"t1"}"#));
    // The hook process has ended: Claude Code would show the dialog only now.
    let i = info(&gui, &id);
    assert_eq!(i.attention, Attention::Ask, "the question is known before the dialog");
    assert_eq!(i.claude_session, "s");
    // A late asynchronous PreToolUse report (its hook started before the
    // question) does not clear it.
    hook(&ep, &id, "PreToolUseAll", r#"{"session_id":"s","tool_name":"Read"}"#);
    assert_eq!(info(&gui, &id).attention, Attention::Ask);
    // The answer: a person's key clears the mark at once (the GUI's own input); the tool's end does not end the turn,
    // the turn's end does.
    gui.send(&ClientMsg::Write { session: id.clone(), origin: Origin::Human, bytes: Secret(b"1".to_vec()) }).unwrap();
    assert!(gui.sync(Duration::from_secs(5)));
    assert_eq!(info(&gui, &id).attention, Attention::None);
    hook(&ep, &id, "PostToolUse", r#"{"session_id":"s","tool_name":"Bash","tool_use_id":"t1"}"#);
    hook(&ep, &id, "Stop", r#"{"session_id":"s"}"#);
    assert_eq!(info(&gui, &id).attention, Attention::Wait);
    // Edit tool: the synchronous PreToolUse with its file copy.
    let f = dir.join("a.txt");
    std::fs::write(&f, "x\n").unwrap();
    let pre = format!(
        r#"{{"session_id":"s","tool_name":"Edit","tool_use_id":"t2","tool_input":{{"file_path":"{}","old_string":"x","new_string":"y"}}}}"#,
        path(&f)
    );
    times.push(hook(&ep, &id, "PreToolUse", &pre));
    for _ in 0..8 {
        times.push(hook(&ep, &id, "PermissionRequest", r#"{"session_id":"s","tool_name":"Bash"}"#));
    }
    times.sort();
    eprintln!(
        "synchronous hook processes (Linux, with a daemon): median {:.1} ms, max {:.1} ms (n={})",
        times[times.len() / 2].as_secs_f64() * 1000.0,
        times[times.len() - 1].as_secs_f64() * 1000.0,
        times.len()
    );
    assert!(times[times.len() - 1] < Duration::from_millis(250), "{times:?}");
    gui.close(&id).unwrap();
    assert!(gui.sync(Duration::from_secs(5)));
    haetty_testkit::remove_dir_all(&dir);
}

/// Sends `m` and says whether the daemon took it (`Denied` while the input is held, K1). Reads the replies up to the
/// `Synced` of a `Sync` sent after it, so no reply is left over for the next call.
fn taken(c: &Client, m: ClientMsg) -> bool {
    c.send(&m).unwrap();
    c.send(&ClientMsg::Sync).unwrap();
    let mut denied = false;
    loop {
        match c.next_reply(Duration::from_secs(5)) {
            Some(DaemonMsg::Denied { .. }) => denied = true,
            Some(DaemonMsg::Synced) => return !denied,
            Some(_) => {}
            None => panic!("no Synced from the daemon"),
        }
    }
}

fn program_write_taken(c: &Client, id: &str) -> bool {
    taken(c, ClientMsg::Write { session: id.to_owned(), origin: Origin::Program, bytes: Secret(b"x".to_vec()) })
}

#[test]
fn turn_end_releases_the_hold_and_a_mouse_press_answers() {
    // K1 follow-up (P1b, approved scope): a Stop or StopFailure that started after the question opens the input hold
    // without a person's input; a mouse button press (not motion, a release or the wheel) clears the mark.
    let (dir, ep, gui, id) = fake_claude("permission-p1b");
    let orch = Client::connect(&ep, Role::Control, "orchestrator").unwrap();
    for end in ["Stop", "StopFailure"] {
        hook(&ep, &id, "UserPromptSubmit", r#"{"session_id":"s"}"#);
        assert!(program_write_taken(&orch, &id), "{end}: no question yet");
        hook(&ep, &id, "PermissionRequest", r#"{"session_id":"s","tool_name":"Bash"}"#);
        assert_eq!(info(&gui, &id).attention, Attention::Ask);
        assert!(!program_write_taken(&orch, &id), "{end}: held while the dialog waits");
        hook(&ep, &id, "PostToolUse", r#"{"session_id":"s","tool_name":"Bash"}"#);
        assert!(!program_write_taken(&orch, &id), "{end}: a tool's end is not the turn's end");
        // Nobody answered in haetty; the turn ended.
        hook(&ep, &id, end, r#"{"session_id":"s"}"#);
        assert_eq!(info(&gui, &id).attention, Attention::Wait, "{end}");
        assert!(program_write_taken(&orch, &id), "{end}: the turn's end opens the hold without a person");
    }

    // A mouse button press is the person's input; motion, a release and the wheel are not.
    hook(&ep, &id, "UserPromptSubmit", r#"{"session_id":"s"}"#);
    hook(&ep, &id, "PermissionRequest", r#"{"session_id":"s","tool_name":"Bash"}"#);
    let mouse = |action: MouseAction, button: Option<MouseButton>| {
        let m = MouseInput::at_pixel(action, button, 30.0, 40.0);
        gui.send(&ClientMsg::Mouse { session: id.clone(), origin: Origin::Human, mouse: WireMouse::from(&m) }).unwrap();
        assert!(gui.sync(Duration::from_secs(5)));
        info(&gui, &id).attention
    };
    assert_eq!(mouse(MouseAction::Motion, None), Attention::Ask, "motion");
    assert_eq!(mouse(MouseAction::Motion, Some(MouseButton::Left)), Attention::Ask, "a drag's motion");
    assert_eq!(mouse(MouseAction::Release, Some(MouseButton::Left)), Attention::Ask, "a release");
    assert_eq!(mouse(MouseAction::Press, Some(MouseButton::Four)), Attention::Ask, "the wheel as a button");
    assert_eq!(mouse(MouseAction::Press, Some(MouseButton::Seven)), Attention::Ask, "the side wheel as a button");
    gui.send(&ClientMsg::Wheel { session: id.clone(), origin: Origin::Human, lines: -3, col: 2, row: 2, mods: 0 })
        .unwrap();
    assert!(gui.sync(Duration::from_secs(5)));
    assert_eq!(info(&gui, &id).attention, Attention::Ask, "the wheel");
    // A program's click is not the person's.
    let m = MouseInput::at_pixel(MouseAction::Press, Some(MouseButton::Left), 30.0, 40.0);
    let click = ClientMsg::Mouse { session: id.clone(), origin: Origin::Program, mouse: WireMouse::from(&m) };
    assert!(!taken(&orch, click), "a program's click is refused while held");
    assert_eq!(info(&gui, &id).attention, Attention::Ask, "a program's click");
    assert_eq!(mouse(MouseAction::Press, Some(MouseButton::Left)), Attention::None, "a left press answers");
    assert!(!program_write_taken(&orch, &id), "the answer clears the mark; the hold waits for the turn's end");
    hook(&ep, &id, "Stop", r#"{"session_id":"s"}"#);
    assert!(program_write_taken(&orch, &id));
    // Another dialog: a right press answers it too.
    hook(&ep, &id, "UserPromptSubmit", r#"{"session_id":"s"}"#);
    hook(&ep, &id, "PermissionRequest", r#"{"session_id":"s","tool_name":"Write"}"#);
    assert_eq!(mouse(MouseAction::Press, Some(MouseButton::Right)), Attention::None, "a right press answers");
    gui.close(&id).unwrap();
    assert!(gui.sync(Duration::from_secs(5)));
    haetty_testkit::remove_dir_all(&dir);
}
