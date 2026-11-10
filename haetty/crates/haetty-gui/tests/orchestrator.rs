//! The orchestrator's safety boundary (DESIGN.md §8.4), end to end: a real
//! daemon, the real `haetty hook` reporter, a fake `claude` that records
//! every byte it reads, and `haetty_cc::orch::send` — the only way the
//! orchestrator writes to a worker. Checked on the bytes that reach the
//! program, not only on the replies. Linux (a raw `cat`) and Windows (T5: a
//! copy of powershell.exe named claude.exe reading keys with
//! `[Console]::ReadKey` through the pseudo console).

mod common;

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use haetty_cc::orch::{self, Hold};
use haetty_daemon::Config;
use haetty_proto::paths::Endpoint;
use haetty_proto::wire::{Attention, ClaudeInfo, CreateSpec, Grid, Origin};
use haetty_proto::{Client, RemoteHandle, Role};

fn hook(ep: &Endpoint, session: &str, event: &str, json: &str) {
    let mut c = Command::new(common::bin())
        .args(["hook", event])
        .env("HAETTY_SOCKET", &ep.0)
        .env("HAETTY_SESSION_ID", session)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    c.stdin.take().unwrap().write_all(json.as_bytes()).unwrap();
    assert!(c.wait().unwrap().success());
}

/// The worker's state as this client sees it, once `ok` holds (pushed by
/// the daemon after the hook; at most 5 s).
fn claude_until(h: &RemoteHandle, ok: impl Fn(&ClaudeInfo) -> bool) -> ClaudeInfo {
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(i) = h.claude().filter(|i| ok(i)) {
            return (*i).clone();
        }
        assert!(Instant::now() < end, "Claude Code state did not arrive: {:?}", h.claude());
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_default()
}

/// What the program has read after `settle` (nothing more arrives late).
fn settled(p: &Path, settle: Duration) -> String {
    std::thread::sleep(settle);
    read(p)
}

/// Linux: raw terminal, every byte read is appended. python3, not `stty raw; cat`: the `cat` of uutils coreutils
/// 0.8.0 (Ubuntu 26.04) forwards the first read from a raw terminal and then nothing more.
#[cfg(unix)]
fn fake_claude(dir: &Path, got: &Path) -> (std::path::PathBuf, Vec<String>) {
    let fake = dir.join("bin/claude");
    let py = "import os, sys, tty\n\
              tty.setraw(0)\n\
              out = os.open(sys.argv[1], os.O_WRONLY | os.O_CREAT | os.O_APPEND, 0o600)\n\
              open(sys.argv[1] + '.up', 'w').write('ready')\n\
              while True:\n\
              \x20   b = os.read(0, 4096)\n\
              \x20   if not b:\n\
              \x20       break\n\
              \x20   os.write(out, b)\n";
    let script = dir.join("fake.py");
    std::fs::write(&script, py).unwrap();
    std::fs::write(&fake, format!("#!/bin/sh\nexec python3 -I '{}' '{}'\n", script.display(), got.display())).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    (fake, vec![])
}

/// Windows: powershell.exe copied as claude.exe runs a script that appends
/// each key's character (no echo, no line editing).
#[cfg(windows)]
fn fake_claude(dir: &Path, got: &Path) -> (std::path::PathBuf, Vec<String>) {
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let ps = Path::new(&root).join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
    let fake = dir.join(r"bin\claude.exe");
    std::fs::copy(&ps, &fake).unwrap();
    let script = dir.join("fake.ps1");
    let g = got.display().to_string().replace('\'', "''");
    std::fs::write(
        &script,
        format!(
            "[IO.File]::WriteAllText('{g}.up', 'ready')\r\n\
             while ($true) {{ $k = [Console]::ReadKey($true); [IO.File]::AppendAllText('{g}', [string]$k.KeyChar) }}\r\n"
        ),
    )
    .unwrap();
    let args = ["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"];
    let mut v: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    v.push(script.display().to_string());
    (fake, v)
}

#[test]
fn orchestrator_input_is_program_input_and_never_answers_a_question() {
    let dir = haetty_testkit::run_dir("orchestrator");
    std::fs::create_dir_all(dir.join("bin")).unwrap();
    let ep = common::endpoint(&dir, "orchestrator");
    let cfg = Config { endpoint: ep.clone(), state_dir: dir.join("state"), run_dir: dir.join("rt"), idle_exit: None };
    let _d = haetty_daemon::spawn(cfg);
    let end = Instant::now() + Duration::from_secs(5);
    let client = loop {
        match Client::connect(&ep, Role::Control, "orchestrator-test") {
            Ok(c) => break c,
            Err(_) if Instant::now() < end => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => panic!("daemon: {e}"),
        }
    };
    // A fake Claude Code (the integration applies to programs named claude):
    // every byte it reads appended to a file.
    let got = dir.join("read.bin");
    let (fake, args) = fake_claude(&dir, &got);
    let id = client
        .create(CreateSpec {
            program: fake.to_string_lossy().into_owned(),
            args,
            cwd: None,
            env: vec![],
            size: Grid { cols: 60, rows: 10, cell_width: 9, cell_height: 18 },
            scrollback_limit: 100_000,
            default_fg: [200, 200, 200],
            default_bg: [20, 20, 20],
            palette: vec![],
            cursor_color: None,
            claude_integration: true,
            engine: String::new(),
        })
        .unwrap();
    let s = client.attach(&id, None).unwrap();
    let h = &s.handle;
    let up = dir.join("read.bin.up");
    let end = Instant::now() + Duration::from_secs(5);
    while !up.exists() {
        assert!(Instant::now() < end, "the fake claude did not start");
        std::thread::sleep(Duration::from_millis(10));
    }
    std::thread::sleep(Duration::from_millis(300)); // past `stty` / into ReadKey
    let quiet = Duration::from_millis(300);

    // 1. Nothing reported yet (Claude Code may show a start-up dialog).
    let r = orch::send(h, h.claude().as_deref(), "T1");
    assert!(matches!(r, Err(Hold::Unknown | Hold::NoTurnYet)), "{r:?}");
    // 2. A hook ran (hooks may run before the trust dialog is answered): still held.
    hook(&ep, &id, "SessionStart", r#"{"session_id":"c-1"}"#);
    let i = claude_until(h, |i| i.claude_session == "c-1");
    assert_eq!(orch::send(h, Some(&i), "T2"), Err(Hold::NoTurnYet));
    // 3. A turn runs: held.
    hook(&ep, &id, "UserPromptSubmit", r#"{"session_id":"c-1"}"#);
    let i = claude_until(h, |i| i.busy);
    assert_eq!(orch::send(h, Some(&i), "T3"), Err(Hold::Busy));
    // 4. A permission question: held, and the daemon refuses program input
    //    whatever a caller saw (a raw program paste and Enter: refused).
    hook(&ep, &id, "PermissionRequest", r#"{"session_id":"c-1","tool_name":"Bash"}"#);
    let i = claude_until(h, |i| i.attention == Attention::Ask);
    assert_eq!(orch::send(h, Some(&i), "T4"), Err(Hold::Asking));
    let stale = ClaudeInfo { turns: 1, ..Default::default() }; // a caller with an old view
    assert_eq!(orch::send(h, Some(&stale), "T5"), Ok(()), "the old view says ready …");
    h.paste(Origin::Program, "T6");
    h.write(Origin::Program, b"y\r".to_vec());
    assert_eq!(settled(&got, quiet), "", "… but no program byte reaches a session that asks");
    // The person's own key still reaches it (the check above is not a dead pipe).
    h.write(Origin::Human, b"1".to_vec());
    let end = Instant::now() + Duration::from_secs(3);
    while read(&got) != "1" {
        assert!(Instant::now() < end, "the person's answer did not arrive: {:?}", read(&got));
        std::thread::sleep(Duration::from_millis(10));
    }
    // 5. Answered, the turn ends with a report: ready, one paste and one Enter.
    hook(&ep, &id, "PostToolUse", r#"{"session_id":"c-1","tool_name":"Bash"}"#);
    let stop = r#"{"session_id":"c-1","last_assistant_message":"Done.\nSTATUS: needs_review\nEVIDENCE: ran it"}"#;
    hook(&ep, &id, "Stop", stop);
    let i = claude_until(h, |i| i.turns == 1 && !i.busy && i.attention != Attention::Ask);
    assert_eq!(i.report.as_ref().map(|r| r.status.as_str()), Some("needs_review"));
    assert_eq!(orch::send(h, Some(&i), "next step\u{7}\u{1b}[2~"), Ok(()));
    let end = Instant::now() + Duration::from_secs(3);
    let want = "1next step[2~\r";
    while read(&got) != want {
        assert!(Instant::now() < end, "the message did not arrive whole: {:?}", read(&got));
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(settled(&got, quiet), want, "control characters dropped; nothing else");
    // 6. The next turn: busy again → held.
    hook(&ep, &id, "UserPromptSubmit", r#"{"session_id":"c-1"}"#);
    let i = claude_until(h, |i| i.busy);
    assert_eq!(orch::send(h, Some(&i), "T7"), Err(Hold::Busy));
    assert!(i.report.is_none(), "the old report is not this turn's");
    assert_eq!(settled(&got, quiet), want);

    // The orchestrator code cannot send as the person.
    let src = include_str!("../../haetty-cc/src/orch.rs");
    assert!(!src.contains("Origin::Human"), "orch.rs must not send Human input");
    let app = include_str!("../src/app.rs");
    for (n, l) in app.lines().enumerate() {
        if l.contains("orch::send(") {
            assert!(!l.contains("Origin"), "app.rs:{}: orch::send takes no origin", n + 1);
        }
    }
    haetty_testkit::remove_dir_all(&dir);
}
