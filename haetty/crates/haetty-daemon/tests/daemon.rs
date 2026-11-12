//! Daemon integration: a real daemon on a temporary socket, real sessions.
//! Unix only (uses sh); the Windows equivalent is checked by hand with the
//! GUI (docs/HANDOFF.md).
#![cfg(unix)]

use std::path::PathBuf;
use std::time::{Duration, Instant};

use haetty_core::KeyInput;
use haetty_daemon::Config;
use haetty_proto::paths::Endpoint;
use haetty_proto::wire::{Attention, ClientMsg, CreateSpec, DaemonMsg, Grid, Origin, Secret, WireColors, WireKey};
use haetty_proto::{Client, RemoteSession, Role};

fn tmp(name: &str) -> PathBuf {
    haetty_testkit::run_dir(&format!("daemon-{name}"))
}

fn start(dir: &std::path::Path, idle: Duration) -> (Endpoint, std::thread::JoinHandle<std::io::Result<()>>) {
    start_with(dir, idle, haetty_daemon::HEALTH_EVERY)
}

fn start_with(
    dir: &std::path::Path,
    idle: Duration,
    health_every: Duration,
) -> (Endpoint, std::thread::JoinHandle<std::io::Result<()>>) {
    let ep = Endpoint(dir.join("rt").join("daemon.sock"));
    let cfg = Config {
        endpoint: ep.clone(),
        state_dir: dir.join("state"),
        run_dir: dir.join("rt"),
        idle_exit: Some(idle),
        health_every,
    };
    let h = haetty_daemon::spawn(cfg);
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
        palette: vec![],
        cursor_color: None,
        claude_integration: false,
        engine: String::new(),
    }
}

/// Waits (via Wakeup + snapshot) until the screen satisfies `ok`.
fn wait_screen(s: &RemoteSession, what: &str, ok: impl Fn(&haetty_core::Snapshot) -> bool) -> haetty_core::Snapshot {
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(haetty_core::SessionEvent::Wakeup) = s.events.recv_timeout(Duration::from_millis(100)) {
            let snap = s.handle.snapshot().unwrap();
            if ok(&snap) {
                return snap;
            }
        }
        assert!(Instant::now() < end, "timed out waiting for {what}");
    }
}

fn last_counter(snap: &haetty_core::Snapshot) -> u32 {
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
    haetty_testkit::remove_dir_all(&dir);
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
    obs.send(&ClientMsg::Paste { session: id.clone(), origin: Origin::Human, text: Secret("evil\n".into()) }).unwrap();
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
    s.handle.paste(Origin::Human, "ok");
    s.handle.key(Origin::Human, KeyInput::parse("Enter").unwrap());
    wait_screen(&s, "control input", |sn| sn.text().contains("got:ok"));
    haetty_testkit::remove_dir_all(&dir);
}

/// A fake `claude` (a shell script of that name) gets haetty's `--settings`
/// file; hook and status-line reports from a Report client update the
/// session's Claude Code state, which the GUI client receives; a pending
/// permission question blocks Program input but not Human input; the
/// settings file goes with the session.
#[test]
fn claude_reports_reach_the_client_and_gate_program_input() {
    let dir = tmp("claude");
    let (ep, _h) = start(&dir, Duration::from_secs(30));
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let fake = bin.join("claude");
    std::fs::write(&fake, "#!/bin/sh\nprintf '%s\n' \"$@\" > \"$ARGS_FILE\"\necho READY\nexec cat\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    let user = dir.join("user.json");
    std::fs::write(&user, r#"{"model":"x","statusLine":{"type":"command","command":"theirs"}}"#).unwrap();
    let args_file = dir.join("args.txt");

    let gui = Client::connect(&ep, Role::Control, "gui").unwrap();
    let mut sp = spec("");
    sp.program = fake.to_string_lossy().into_owned();
    sp.args = vec!["--settings".into(), user.to_string_lossy().into_owned(), "--verbose".into()];
    sp.env = vec![("ARGS_FILE".into(), args_file.to_string_lossy().into_owned())];
    sp.claude_integration = true;
    let id = gui.create(sp).unwrap();
    let s = gui.attach(&id, None).unwrap();
    wait_screen(&s, "READY", |sn| (0..sn.rows).any(|y| sn.row_text(y).contains("READY")));
    let args: Vec<String> = std::fs::read_to_string(&args_file).unwrap().lines().map(str::to_string).collect();
    assert_eq!(args.len(), 3, "{args:?}");
    assert_eq!(args[..2], ["--verbose".to_string(), "--settings".to_string()], "the user's --settings is replaced");
    let path = args[2].clone();
    assert!(path.starts_with(&dir.join("rt").to_string_lossy().to_string()), "{path}");
    let file = std::fs::read_to_string(&path).unwrap();
    assert!(file.contains(r#""args":["hook","PreToolUse"]"#), "{file}");
    assert!(file.contains(r#""model":"x""#) && !file.contains("theirs"), "merged: {file}");
    use std::os::unix::fs::MetadataExt;
    assert_eq!(std::fs::metadata(&path).unwrap().mode() & 0o777, 0o600);

    let rep = Client::connect(&ep, Role::Report, "hook").unwrap();
    let hook = |event: &str, json: &str, ts: u64| {
        rep.send(&ClientMsg::Hook {
            session: Some(id.clone()),
            event: event.into(),
            json: Secret(json.into()),
            ts_ms: ts,
            snapshot: None,
        })
        .unwrap();
        assert!(rep.sync(Duration::from_secs(5)));
    };
    let wait_info = |what: &str, ok: &dyn Fn(&haetty_proto::wire::ClaudeInfo) -> bool| {
        let end = Instant::now() + Duration::from_secs(5);
        loop {
            let _ = s.events.recv_timeout(Duration::from_millis(50));
            let _ = s.handle.snapshot();
            if let Some(i) = s.handle.claude() {
                if ok(&i) {
                    return;
                }
            }
            assert!(Instant::now() < end, "timed out waiting for {what}");
        }
    };
    rep.send(&ClientMsg::StatusLine {
        session: Some(id.clone()),
        json: Secret(r#"{"session_id":"abc-1","context_window":{"used_percentage":42}}"#.into()),
        ts_ms: 1,
    })
    .unwrap();
    assert!(rep.sync(Duration::from_secs(5)));
    wait_info("status line", &|i| i.context_pct == Some(42.0) && i.claude_session == "abc-1");
    hook("PermissionRequest", r#"{"tool_name":"Bash"}"#, 10);
    wait_info("ask", &|i| i.attention == Attention::Ask);

    // Program input is refused while the question is open, through every
    // input method of the client library; Human input passes.
    s.handle.write(Origin::Program, "robot-write\n");
    s.handle.paste(Origin::Program, "robot-paste\n");
    s.handle.key(Origin::Program, KeyInput::parse("x").unwrap());
    s.handle.wheel(Origin::Program, -3, 0, 0, haetty_core::Mods::empty());
    for what in ["Write", "Paste", "Key", "Wheel"] {
        match gui.next_reply(Duration::from_secs(5)) {
            Some(DaemonMsg::Denied { what: w }) => assert!(w.starts_with(what), "{w}"),
            other => panic!("expected Denied for {what}, got {other:?}"),
        }
    }
    s.handle.write(Origin::Human, "human\n");
    let has = |sn: &haetty_core::Snapshot, t: &str| (0..sn.rows).any(|y| sn.row_text(y).contains(t));
    wait_screen(&s, "human input", |sn| has(sn, "human"));
    assert!(!has(&s.handle.snapshot().unwrap(), "robot"));
    // Not the role of Control or Observe to report hooks (DESIGN.md §3.7).
    gui.send(&ClientMsg::Hook {
        session: Some(id.clone()),
        event: "PostToolUse".into(),
        json: Secret(r#"{"tool_name":"Bash"}"#.into()),
        ts_ms: 99,
        snapshot: None,
    })
    .unwrap();
    gui.send(&ClientMsg::StatusLine { session: Some(id.clone()), json: Secret("{}".into()), ts_ms: 99 }).unwrap();
    for what in ["Hook", "StatusLine"] {
        match gui.next_reply(Duration::from_secs(5)) {
            Some(DaemonMsg::Denied { what: w }) => assert!(w.starts_with(what), "{w}"),
            other => panic!("expected Denied for {what} from Control, got {other:?}"),
        }
    }
    let obs = Client::connect(&ep, Role::Observe, "status").unwrap();
    obs.send(&ClientMsg::Hook {
        session: Some(id.clone()),
        event: "Stop".into(),
        json: Secret("{}".into()),
        ts_ms: 99,
        snapshot: None,
    })
    .unwrap();
    assert!(matches!(obs.next_reply(Duration::from_secs(5)), Some(DaemonMsg::Denied { .. })));
    assert!(gui.sync(Duration::from_secs(5)));
    // The person's answer cleared the mark at once; the hold stays until the turn ends. The session's update arrives as
    // an event, so wait for it instead of reading the cache.
    wait_info("answered", &|i| i.attention == Attention::None && i.asking);
    assert!(s.handle.claude().unwrap().asking, "denied reports changed nothing");

    // A tool's end is not the end of the turn: program input stays refused.
    hook("PostToolUse", r#"{"tool_name":"Bash"}"#, 20);
    s.handle.write(Origin::Program, "robot-held\n");
    match gui.next_reply(Duration::from_secs(5)) {
        Some(DaemonMsg::Denied { what: w }) => assert!(w.starts_with("Write"), "{w}"),
        other => panic!("expected Denied while the turn runs, got {other:?}"),
    }
    assert!(!has(&s.handle.snapshot().unwrap(), "robot-held"));
    // The turn's end releases it: program input passes again.
    hook("Stop", "{}", 30);
    wait_info("released", &|i| i.attention == Attention::Wait && !i.asking);
    s.handle.write(Origin::Program, "robot-after\n");
    wait_screen(&s, "program input after the turn", |sn| has(sn, "robot-after"));
    gui.clear_attention(&id).unwrap();
    wait_info("cleared", &|i| i.attention == Attention::None);

    // A report for a session without the integration is ignored.
    let plain = gui.create(spec("sleep 30")).unwrap();
    rep.send(&ClientMsg::Hook {
        session: Some(plain.clone()),
        event: "Stop".into(),
        json: Secret("{}".into()),
        ts_ms: 1,
        snapshot: None,
    })
    .unwrap();
    assert!(rep.sync(Duration::from_secs(5)));
    // Report clients cannot send input.
    rep.send(&ClientMsg::Write { session: id.clone(), origin: Origin::Human, bytes: Secret(b"x".to_vec()) }).unwrap();
    assert!(matches!(rep.next_reply(Duration::from_secs(5)), Some(DaemonMsg::Denied { .. })));

    gui.close(&id).unwrap();
    gui.close(&plain).unwrap();
    assert!(gui.sync(Duration::from_secs(5)));
    assert!(!std::path::Path::new(&path).exists(), "settings file removed with the session");
    haetty_testkit::remove_dir_all(&dir);
}

/// Friction log (DESIGN.md §3.8): a key whose screen change comes after the
/// 16 ms budget gives `slow_input`; closing a session writes its latency
/// summary; a GUI friction report becomes a `gui_*` line; a Report client
/// cannot send one. No typed text in the log.
#[test]
fn friction_reaches_the_event_log() {
    let dir = tmp("friction");
    let (ep, _h) = start(&dir, Duration::from_secs(30));
    let gui = Client::connect(&ep, Role::Control, "gui").unwrap();
    // No echo: the first screen change after the key is the late answer.
    let id = gui.create(spec("stty -echo; echo ready; read x; sleep 0.08; echo answered; sleep 30")).unwrap();
    let s = gui.attach(&id, None).unwrap();
    let has = |sn: &haetty_core::Snapshot, t: &str| (0..sn.rows).any(|y| sn.row_text(y).contains(t));
    wait_screen(&s, "ready", |sn| has(sn, "ready"));
    s.handle.paste(Origin::Human, "secret-typed-text\n");
    wait_screen(&s, "answered", |sn| has(sn, "answered"));
    gui.reporter().friction("slow_draw", &[("ms", "21.5".into()), ("full", "true".into())]);
    let rep = Client::connect(&ep, Role::Report, "hook").unwrap();
    rep.send(&ClientMsg::Friction { kind: "x".into(), fields: vec![] }).unwrap();
    assert!(matches!(rep.next_reply(Duration::from_secs(5)), Some(DaemonMsg::Denied { .. })));
    gui.close(&id).unwrap();
    assert!(gui.sync(Duration::from_secs(5)));
    let log = std::fs::read_to_string(dir.join("state/events.log")).unwrap();
    let line = |kind: &str| {
        log.lines().find(|l| l.split('\t').nth(2) == Some(kind)).unwrap_or_else(|| panic!("no {kind}: {log}"))
    };
    let slow = line("slow_input");
    let ms: f64 = slow.split("key_to_screen_ms=").nth(1).unwrap().split('\t').next().unwrap().parse().unwrap();
    assert!(ms >= 70.0, "{slow}");
    let lat = line("latency");
    assert!(lat.contains("\tn=1\t") && lat.contains("over_16ms=1"), "{lat}");
    assert!(line("gui_slow_draw").contains("ms=21.5"));
    assert!(!log.contains("secret-typed-text"), "typed text must never reach the log");
    haetty_testkit::remove_dir_all(&dir);
}

/// A theme change for running sessions (`SetTheme`): the attached client
/// sees the program's palette text and the default colours change; a client
/// that attaches afterwards (a restarted GUI reattaching) gets the new
/// colours; Observe and Report clients are refused and change nothing.
#[test]
fn set_theme_recolours_running_sessions_control_only() {
    use haetty_core::theme::Theme;
    use haetty_core::TermColors;
    let dir = tmp("theme");
    let (ep, _daemon) = start(&dir, Duration::from_secs(60));
    let classic = Theme::builtin("haetty classic").unwrap();
    let cga = Theme::builtin("IBM 5153 CGA").unwrap();
    let black = Theme::builtin("IBM 5153 CGA (Black)").unwrap();
    let ctl = Client::connect(&ep, Role::Control, "gui").unwrap();
    let mut sp = spec("printf '\\033[31mRED\\033[0m\\n'; sleep 30");
    let c0: WireColors = TermColors::of(&classic).into();
    (sp.default_fg, sp.default_bg, sp.palette, sp.cursor_color) =
        (c0.default_fg, c0.default_bg, c0.palette, c0.cursor_color);
    let id = ctl.create(sp).unwrap();
    let s = ctl.attach(&id, None).unwrap();
    let red = |sn: &haetty_core::Snapshot| sn.cell(0, 0).and_then(|c| c.fg);
    wait_screen(&s, "red text in classic", |sn| red(sn) == Some(classic.palette[1]));
    // Observe and Report may not change it.
    let obs = Client::connect(&ep, Role::Observe, "status").unwrap();
    let rep = Client::connect(&ep, Role::Report, "hook").unwrap();
    for (cl, name) in [(&obs, "Observe"), (&rep, "Report")] {
        cl.send(&ClientMsg::SetTheme { session: id.clone(), colors: TermColors::of(&black).into() }).unwrap();
        match cl.next_reply(Duration::from_secs(3)) {
            Some(DaemonMsg::Denied { what }) => assert!(what.starts_with("SetTheme"), "{name}: {what}"),
            other => panic!("{name}: expected Denied, got {other:?}"),
        }
    }
    std::thread::sleep(Duration::from_millis(300));
    let snap = s.handle.snapshot().unwrap();
    assert_eq!((red(&snap), snap.default_bg), (Some(classic.palette[1]), classic.background), "unchanged");
    // Control: the open tab takes the new colours.
    s.handle.set_colors(TermColors::of(&cga));
    let snap = wait_screen(&s, "red text in CGA", |sn| red(sn) == Some(cga.palette[1]));
    assert_eq!((snap.default_fg, snap.default_bg), (cga.foreground, cga.background));
    // A GUI that comes back (new connection) attaches and sees them too.
    drop(s);
    ctl.disconnect();
    drop(ctl);
    let ctl2 = Client::connect(&ep, Role::Control, "gui-2").unwrap();
    let s2 = ctl2.attach(&id, None).unwrap();
    let snap = wait_screen(&s2, "reattached frame", |sn| sn.rows == 8);
    assert_eq!((red(&snap), snap.default_bg), (Some(cga.palette[1]), cga.background));
    // ... and can change them again.
    s2.handle.set_colors(TermColors::of(&black));
    wait_screen(&s2, "red text in CGA Black", |sn| {
        red(sn) == Some(black.palette[1]) && sn.default_bg == black.background
    });
    ctl2.close(&id).unwrap();
    haetty_testkit::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn a_socket_of_its_own_gives_its_own_run_dir() {
    let shared = PathBuf::from("/shared/haetty");
    let stable = Endpoint(PathBuf::from("/shared/haetty/daemon.sock"));
    assert_eq!(haetty_daemon::instance_run_dir(&stable, shared.clone()), shared);
    let dev = Endpoint(PathBuf::from("/shared/haetty-dev/daemon.sock"));
    assert_eq!(haetty_daemon::instance_run_dir(&dev, shared), PathBuf::from("/shared/haetty-dev"));
}

/// P1c: the daemon's own counts — `GetHealth` for observe and control clients (not report), the `health` line at its
/// interval (shortened here), and the idle exit without the old polling tick: not before its time after the last
/// client leaves.
#[test]
fn health_counts_the_hourly_line_and_the_idle_exit() {
    let dir = tmp("health");
    let (ep, daemon) = start_with(&dir, Duration::from_secs(2), Duration::from_millis(700));
    let obs = Client::connect(&ep, Role::Observe, "status").unwrap();
    let h0 = obs.health().unwrap();
    assert_eq!((h0.now.sessions, h0.now.clients), (0, 1), "{h0:?}");
    #[cfg(target_os = "linux")]
    assert!(h0.now.threads.unwrap() >= 2 && h0.now.fds.unwrap() >= 1 && h0.now.anon_kb.unwrap() > 0, "{h0:?}");
    let gui = Client::connect(&ep, Role::Control, "gui").unwrap();
    let id = gui.create(spec("sleep 30")).unwrap();
    let h1 = gui.health().unwrap();
    assert_eq!((h1.now.sessions, h1.now.clients, h1.max.sessions), (1, 2, 1), "{h1:?}");
    assert_eq!(h1.started_ms, h0.started_ms);
    #[cfg(target_os = "linux")]
    assert!(h1.max.threads >= h1.now.threads && h1.now.threads > h0.now.threads, "a session's threads: {h0:?} {h1:?}");
    let rep = Client::connect(&ep, Role::Report, "hook").unwrap();
    rep.send(&ClientMsg::GetHealth).unwrap();
    assert!(matches!(rep.next_reply(Duration::from_secs(5)), Some(DaemonMsg::Denied { .. })));
    drop(rep);
    // The interval passes: health lines in the log, no session text in them.
    std::thread::sleep(Duration::from_millis(1600));
    let log = std::fs::read_to_string(dir.join("state/events.log")).unwrap();
    let lines: Vec<&str> = log.lines().filter(|l| l.split('\t').nth(2) == Some("health")).collect();
    assert!(!lines.is_empty(), "{log}");
    assert!(
        lines[0].contains("\tsessions=1") && lines[0].contains("\tthreads=") && lines[0].contains("\tanon_kb="),
        "{}",
        lines[0]
    );
    gui.close(&id).unwrap();
    assert!(gui.sync(Duration::from_secs(5)));
    let h2 = obs.health().unwrap();
    assert_eq!((h2.now.sessions, h2.max.sessions), (0, 1), "the largest stays: {h2:?}");
    // The last client leaves: the daemon waits its idle time (2 s), then ends.
    drop(gui);
    drop(obs);
    let gone = Instant::now();
    std::thread::sleep(Duration::from_millis(1000));
    assert!(!daemon.is_finished(), "ended {:?} after the last client left, before its idle time", gone.elapsed());
    let end = Instant::now() + Duration::from_secs(10);
    while !daemon.is_finished() {
        assert!(Instant::now() < end, "the idle daemon did not end");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(gone.elapsed() >= Duration::from_millis(1900), "{:?}", gone.elapsed());
    daemon.join().unwrap().unwrap();
    haetty_testkit::remove_dir_all(&dir);
}
