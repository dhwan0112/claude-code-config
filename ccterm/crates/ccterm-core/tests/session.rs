//! End-to-end tests: a real child in a real PTY, the real libghostty-vt
//! engine. Unix only for now (they use python3 and sh as test programs).
#![cfg(unix)]

use std::time::{Duration, Instant};

use ccterm_core::*;

const PROBE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/helpers/probe.py");

fn spawn(args: &[&str], cols: u16, rows: u16) -> Session {
    let spec = CommandSpec::new(args[0]).args(&args[1..]);
    let mut cfg = SessionConfig::new(spec);
    cfg.size = GridSize::new(cols, rows);
    Session::spawn(cfg).expect("spawn")
}

/// Wait until the screen contains `text`, draining events. Returns the events
/// seen while waiting.
fn wait_text(s: &Session, text: &str, timeout: Duration) -> Vec<SessionEvent> {
    let end = Instant::now() + timeout;
    let mut seen = vec![];
    loop {
        if s.handle.snapshot().unwrap().contains(text) {
            return seen;
        }
        assert!(
            Instant::now() < end,
            "timed out waiting for {text:?}; screen:\n{}",
            s.handle.snapshot().unwrap().text()
        );
        if let Ok(ev) = s.events.recv_timeout(Duration::from_millis(50)) {
            seen.push(ev);
        }
    }
}

fn screen_lines(s: &Session) -> Vec<String> {
    let snap = s.handle.snapshot().unwrap();
    (0..snap.rows).map(|y| snap.row_text(y)).collect()
}

fn line_with(s: &Session, prefix: &str) -> String {
    screen_lines(s)
        .into_iter()
        .find(|l| l.starts_with(prefix))
        .unwrap_or_else(|| panic!("no line starting with {prefix:?}"))
}

#[test]
fn answers_startup_queries() {
    let s = spawn(&["python3", PROBE, "queries"], 100, 30);
    wait_text(&s, "DONE", Duration::from_secs(20));
    assert_eq!(line_with(&s, "XTVERSION="), format!("XTVERSION=\\eP>|{XTVERSION}\\e\\"));
    // Synchronized output is supported (2 = currently reset).
    assert_eq!(line_with(&s, "DECRQM_2026="), "DECRQM_2026=\\e[?2026;2$y");
    assert_eq!(line_with(&s, "KITTY_Q="), "KITTY_Q=\\e[?0u");
    assert_eq!(line_with(&s, "KITTY_AFTER_PUSH="), "KITTY_AFTER_PUSH=\\e[?1u");
    assert_eq!(line_with(&s, "SIZE_CELLS="), "SIZE_CELLS=\\e[8;30;100t");
    assert_eq!(line_with(&s, "COLOR_SCHEME="), "COLOR_SCHEME=\\e[?997;1n");
    assert_eq!(line_with(&s, "OSC11_BG="), "OSC11_BG=\\e]11;rgb:1e1e/1e1e/1e1e\\e\\");
    assert!(line_with(&s, "DA1=").starts_with("DA1=\\e[?62;"));
}

#[test]
fn encodes_keys_mouse_paste_focus() {
    let s = spawn(&["python3", PROBE, "echo-input", "1006", "1003", "2004", "1004", "kitty=1"], 80, 30);
    wait_text(&s, "READY", Duration::from_secs(10));
    let h = &s.handle;
    let gs = GridSize::new(80, 30);

    h.key(KeyInput::parse("shift+Enter").unwrap());
    wait_text(&s, "GOT \\e[13;2u", Duration::from_secs(5));
    h.key(KeyInput::parse("ctrl+c").unwrap());
    wait_text(&s, "GOT \\e[99;5u", Duration::from_secs(5));
    h.key(KeyInput::press(Key::Unidentified).with_text("한"));
    wait_text(&s, "GOT \\xed\\x95\\x9c", Duration::from_secs(5));

    h.wheel(-2, 10, 5, Mods::empty());
    wait_text(&s, "GOT \\e[<64;11;6M\\e[<64;11;6M", Duration::from_secs(5));

    let press = MouseInput::at_cell(MouseAction::Press, Some(MouseButton::Left), 3, 4, gs);
    h.mouse(press);
    wait_text(&s, "GOT \\e[<0;4;5M", Duration::from_secs(5));

    h.paste("a\nb");
    wait_text(&s, "GOT \\e[200~a", Duration::from_secs(5));

    h.focus(true);
    wait_text(&s, "GOT \\e[I", Duration::from_secs(5));
    h.key(KeyInput::parse("q").unwrap());
    wait_text(&s, "DONE", Duration::from_secs(5));
}

#[test]
fn osc_events_title_notification_progress_bell() {
    let script = r#"printf '\033]0;my title\007'; printf '\033]9;needs input\007'; printf '\033]777;notify;T;B\033\\'; printf '\033]9;4;1;40\007'; printf '\007'; echo end"#;
    let s = spawn(&["sh", "-c", script], 40, 5);
    let evs = wait_text(&s, "end", Duration::from_secs(5));
    // Give queued events a moment.
    let mut evs = evs;
    while let Ok(e) = s.events.recv_timeout(Duration::from_millis(200)) {
        evs.push(e);
    }
    let has = |pred: &dyn Fn(&SessionEvent) -> bool| evs.iter().any(pred);
    assert!(has(&|e| matches!(e, SessionEvent::Title(t) if t == "my title")), "{evs:?}");
    assert!(
        has(&|e| matches!(e, SessionEvent::Notification { title: None, body } if body == "needs input")),
        "{evs:?}"
    );
    assert!(
        has(&|e| matches!(e, SessionEvent::Notification { title: Some(t), body } if t == "T" && body == "B")),
        "{evs:?}"
    );
    assert!(has(&|e| matches!(e, SessionEvent::Progress(Progress::Normal(40)))), "{evs:?}");
    assert!(has(&|e| matches!(e, SessionEvent::Bell)), "{evs:?}");
    assert_eq!(s.handle.snapshot().unwrap().title, "my title");
}

#[test]
fn synchronized_output_holds_wakeups() {
    // Begin a synchronized update, draw, wait 400 ms, end it.
    let script = r#"sleep 0.3; printf '\033[?2026hhalf'; sleep 0.4; printf ' done\033[?2026l'; sleep 0.5"#;
    let s = spawn(&["sh", "-c", script], 40, 5);
    let t0 = Instant::now();
    let mut wakeups = vec![];
    while t0.elapsed() < Duration::from_millis(1100) {
        if let Ok(SessionEvent::Wakeup) = s.events.recv_timeout(Duration::from_millis(20)) {
            wakeups.push(t0.elapsed());
            let snap = s.handle.snapshot().unwrap(); // re-arm
                                                     // Never show the half-drawn frame through a wakeup.
            assert!(!(snap.contains("half") && !snap.contains("done")), "half frame exposed at {:?}", t0.elapsed());
        }
    }
    assert!(s.handle.snapshot().unwrap().contains("half done"));
    assert!(!wakeups.is_empty());
}

#[test]
fn sync_timeout_forces_redraw() {
    // An application that never ends the update must not freeze the screen.
    let script = r#"printf '\033[?2026hstuck'; sleep 2"#;
    let mut cfg = SessionConfig::new(CommandSpec::new("sh").args(["-c", script]));
    cfg.size = GridSize::new(40, 5);
    cfg.sync_timeout = Duration::from_millis(200);
    let s = Session::spawn(cfg).unwrap();
    let t0 = Instant::now();
    let mut got = None;
    while t0.elapsed() < Duration::from_millis(1500) {
        if let Ok(SessionEvent::Wakeup) = s.events.recv_timeout(Duration::from_millis(20)) {
            if s.handle.snapshot().unwrap().contains("stuck") {
                got = Some(t0.elapsed());
                break;
            }
        }
    }
    let at = got.expect("no redraw while sync output stayed on");
    assert!(at >= Duration::from_millis(150), "redrawn too early: {at:?}");
}

#[test]
fn resize_reaches_child() {
    let s = spawn(&["sh", "-c", "stty size; read x; stty size; sleep 0.2"], 50, 10);
    wait_text(&s, "10 50", Duration::from_secs(5));
    s.handle.resize(GridSize::new(70, 20));
    // Let the resize land before the child asks again.
    let _ = s.handle.snapshot();
    s.handle.write(b"\r".to_vec());
    wait_text(&s, "20 70", Duration::from_secs(5));
    let snap = s.handle.snapshot().unwrap();
    assert_eq!((snap.cols, snap.rows), (70, 20));
}

#[test]
fn cell_attributes_and_wide_chars() {
    let s = spawn(&["sh", "-c", r#"printf '\033[1;38;2;255;0;0mR\033[0m한x\n'; sleep 0.3"#], 20, 3);
    wait_text(&s, "R한x", Duration::from_secs(5));
    let snap = s.handle.snapshot().unwrap();
    let r = snap.cell(0, 0).unwrap();
    assert_eq!(r.text, "R");
    assert_eq!(r.fg, Some(Rgb { r: 255, g: 0, b: 0 }));
    assert!(r.flags.has(CellFlags::BOLD));
    assert_eq!(snap.cell(1, 0).unwrap().width, Width::Wide);
    assert_eq!(snap.cell(2, 0).unwrap().width, Width::Spacer);
    assert_eq!(snap.cell(3, 0).unwrap().text, "x");
}

#[test]
fn reports_exit_and_strips_tmux_env() {
    let spec =
        CommandSpec::new("sh").args(["-c", "echo TMUX=${TMUX-unset} TERM=$TERM; exit 3"]).env("TMUX", "/tmp/fake,1,0");
    let mut cfg = SessionConfig::new(spec);
    // An explicit TMUX from the caller wins over the default removal...
    cfg.size = GridSize::new(60, 4);
    let s = Session::spawn(cfg).unwrap();
    let end = Instant::now() + Duration::from_secs(5);
    let mut status = None;
    while Instant::now() < end && status.is_none() {
        if let Ok(SessionEvent::Exited(st)) = s.events.recv_timeout(Duration::from_millis(50)) {
            status = Some(st);
        }
    }
    assert_eq!(status.unwrap().code, Some(3));
    assert!(s.handle.snapshot().unwrap().contains("TMUX=/tmp/fake,1,0 TERM=xterm-256color"));

    // ...and without it, TMUX is removed.
    std::env::set_var("TMUX", "/tmp/outer,1,0");
    let s = spawn(&["sh", "-c", "echo TMUX=${TMUX-unset}; sleep 0.2"], 60, 4);
    wait_text(&s, "TMUX=unset", Duration::from_secs(5));
}

#[test]
fn exit_reported_even_if_pty_stays_open() {
    // A background job inherits the PTY and outlives the shell.
    let s = spawn(&["sh", "-c", "sleep 5 & echo started; exit 7"], 40, 4);
    let t0 = Instant::now();
    let mut status = None;
    while t0.elapsed() < Duration::from_secs(3) && status.is_none() {
        if let Ok(SessionEvent::Exited(st)) = s.events.recv_timeout(Duration::from_millis(50)) {
            status = Some(st);
        }
    }
    assert_eq!(status.expect("no exit event").code, Some(7));
}
