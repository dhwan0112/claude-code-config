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

use haetty_daemon::Config;
use haetty_proto::paths::Endpoint;
use haetty_proto::wire::{Attention, CreateSpec, Grid, SessionInfo};
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

#[test]
fn permission_request_is_in_the_daemon_before_the_hook_exits() {
    let dir = std::env::temp_dir().join(format!("haetty-permission-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("bin")).unwrap();
    let ep = Endpoint(dir.join("rt").join("daemon.sock"));
    let cfg = Config { endpoint: ep.clone(), state_dir: dir.join("state"), run_dir: dir.join("rt"), idle_exit: None };
    let _d = haetty_daemon::spawn(cfg);
    let end = Instant::now() + Duration::from_secs(5);
    let gui = loop {
        match Client::connect(&ep, Role::Control, "gui") {
            Ok(c) => break c,
            Err(_) if Instant::now() < end => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => panic!("daemon: {e}"),
        }
    };
    // A fake Claude Code: the integration applies to programs named claude.
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
            claude_integration: true,
        })
        .unwrap();
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
    // The answer: the tool runs, PostToolUse (started later) clears it.
    hook(&ep, &id, "PostToolUse", r#"{"session_id":"s","tool_name":"Bash","tool_use_id":"t1"}"#);
    assert_eq!(info(&gui, &id).attention, Attention::None);
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
    let _ = std::fs::remove_dir_all(&dir);
}
