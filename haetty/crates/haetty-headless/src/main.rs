//! `haetty-headless`: run a program in a haetty session without a window.
//!
//! ```text
//! haetty-headless [options] -- PROGRAM [ARGS...]
//!   --cols N --rows N         grid size (default 120x36)
//!   --script FILE             steps, one per line ('#' comments)
//!   -e STEP                   a step on the command line (repeatable, after --script)
//!   --raw-log FILE            append all PTY output bytes to FILE
//!   --trace                   print replies to terminal queries and events to stderr
//!   --dump plain|vt|html|grid|none   final output on stdout (default plain)
//!   --timeout SECONDS         overall limit (default 60)
//!   --env K=V                 extra environment variable (repeatable)
//!   --cwd DIR
//!
//! Steps:
//!   sleep MS                  wait
//!   wait-text TEXT [MS]       until TEXT is on screen (default 10000 ms), else fail
//!   wait-title TEXT [MS]      until the window title (OSC 0/2) contains TEXT
//!   wait-idle MS [MAX_MS]     until no screen change for MS
//!   wait-exit [MS]            until the program exits
//!   type TEXT                 send TEXT as key presses (\n \t \e \\ escapes)
//!   send TEXT                 send raw bytes (same escapes), no encoding
//!   key SPEC                  e.g. Enter, shift+Enter, ctrl+c, ArrowDown, F5
//!   paste TEXT                bracketed paste if the application enabled it
//!   wheel up|down N COL ROW   mouse wheel at a cell
//!   click COL ROW             left click at a cell
//!   drag C1 R1 C2 R2          left button down at C1 R1, move with the button held, release at C2 R2
//!   focus in|out
//!   resize COLS ROWS
//!   expect TEXT               fail unless TEXT is on screen
//!   expect-not TEXT
//!   print                     print the screen to stderr
//!   modes                     print terminal modes to stderr
//!   save FILE                 write the screen text to FILE
//! ```
//! Exit status: 0 when every step passed, 1 when a step failed, 2 on usage errors.

use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use haetty_core::*;

struct Args {
    cols: u16,
    rows: u16,
    steps: Vec<String>,
    raw_log: Option<PathBuf>,
    trace: bool,
    dump: String,
    timeout: Duration,
    env: Vec<(String, String)>,
    cwd: Option<PathBuf>,
    program: Vec<String>,
}

fn usage(msg: &str) -> ! {
    eprintln!("haetty-headless: {msg}\nusage: haetty-headless [--cols N] [--rows N] [--script FILE] [-e STEP]... [--raw-log FILE] [--trace] [--dump plain|vt|html|grid|none] [--timeout S] [--env K=V]... [--cwd DIR] -- PROGRAM [ARGS...]");
    std::process::exit(2);
}

fn parse_args() -> Args {
    let mut a = Args {
        cols: 120,
        rows: 36,
        steps: vec![],
        raw_log: None,
        trace: false,
        dump: "plain".into(),
        timeout: Duration::from_secs(60),
        env: vec![],
        cwd: None,
        program: vec![],
    };
    let mut it = std::env::args().skip(1);
    let mut script_steps = vec![];
    let mut cli_steps = vec![];
    while let Some(arg) = it.next() {
        let mut val = |name: &str| it.next().unwrap_or_else(|| usage(&format!("{name} needs a value")));
        match arg.as_str() {
            "--cols" => a.cols = val("--cols").parse().unwrap_or_else(|_| usage("bad --cols")),
            "--rows" => a.rows = val("--rows").parse().unwrap_or_else(|_| usage("bad --rows")),
            "--script" => {
                let p = val("--script");
                let text = std::fs::read_to_string(&p).unwrap_or_else(|e| usage(&format!("{p}: {e}")));
                script_steps.extend(
                    text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).map(str::to_owned),
                );
            }
            "-e" => cli_steps.push(val("-e")),
            "--raw-log" => a.raw_log = Some(val("--raw-log").into()),
            "--trace" => a.trace = true,
            "--dump" => a.dump = val("--dump"),
            "--timeout" => {
                a.timeout = Duration::from_secs_f64(val("--timeout").parse().unwrap_or_else(|_| usage("bad --timeout")))
            }
            "--env" => {
                let kv = val("--env");
                let (k, v) = kv.split_once('=').unwrap_or_else(|| usage("--env wants K=V"));
                a.env.push((k.into(), v.into()));
            }
            "--cwd" => a.cwd = Some(val("--cwd").into()),
            "--" => {
                a.program = it.by_ref().collect();
                break;
            }
            s if s.starts_with('-') => usage(&format!("unknown option {s}")),
            _ => {
                a.program.push(arg);
                a.program.extend(it.by_ref());
                break;
            }
        }
    }
    if a.program.is_empty() {
        usage("no program given");
    }
    a.steps = script_steps;
    a.steps.extend(cli_steps);
    a
}

fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('e') => out.push('\x1b'),
            Some('\\') => out.push('\\'),
            Some('x') => {
                let h: String = chars.by_ref().take(2).collect();
                if let Ok(b) = u8::from_str_radix(&h, 16) {
                    out.push(b as char);
                }
            }
            Some(o) => {
                out.push('\\');
                out.push(o);
            }
            None => out.push('\\'),
        }
    }
    out
}

struct Runner {
    session: Session,
    size: GridSize,
    trace: bool,
    exited: Option<ExitStatus>,
    /// Last title set by the program (OSC 0/2).
    title: String,
    deadline: Instant,
    last_change: Instant,
}

impl Runner {
    /// Handle events for up to `wait`; returns true if the screen changed.
    fn pump(&mut self, wait: Duration) -> bool {
        let mut changed = false;
        let end = Instant::now() + wait;
        loop {
            let left = end.saturating_duration_since(Instant::now());
            let ev = if left.is_zero() {
                self.session.events.try_recv().ok()
            } else {
                self.session.events.recv_timeout(left).ok()
            };
            let Some(ev) = ev else { break };
            match ev {
                SessionEvent::Wakeup => {
                    changed = true;
                    self.last_change = Instant::now();
                    // Taking a snapshot re-arms the next wakeup.
                    let _ = self.session.handle.snapshot();
                }
                SessionEvent::Exited(s) => {
                    if self.trace {
                        eprintln!("[event] exited {s:?}");
                    }
                    self.exited = Some(s);
                }
                SessionEvent::Title(t) => {
                    if self.trace {
                        eprintln!("[event] title {t:?}");
                    }
                    self.title = t;
                }
                SessionEvent::Reply(bytes) => {
                    if self.trace {
                        eprintln!("[reply] {}", escape_bytes(&bytes));
                    }
                }
                other => {
                    if self.trace {
                        eprintln!("[event] {other:?}");
                    }
                }
            }
        }
        changed
    }

    fn snap(&self) -> Snapshot {
        self.session.handle.snapshot().unwrap_or_default()
    }

    fn timed_out(&self) -> bool {
        Instant::now() >= self.deadline
    }

    fn step(&mut self, line: &str) -> Result<(), String> {
        let (cmd, rest) = line.split_once(' ').map(|(c, r)| (c, r.trim())).unwrap_or((line, ""));
        let h = self.session.handle.clone();
        match cmd {
            "sleep" => {
                let ms: u64 = rest.parse().map_err(|_| "sleep MS")?;
                self.pump(Duration::from_millis(ms));
            }
            "wait-text" => {
                let (text, ms) = split_trailing_ms(rest, 10_000);
                let end = Instant::now() + Duration::from_millis(ms);
                loop {
                    if self.snap().contains(&text) {
                        break;
                    }
                    if Instant::now() >= end || self.timed_out() {
                        return Err(format!("text not shown within {ms} ms: {text:?}"));
                    }
                    self.pump(Duration::from_millis(50));
                }
            }
            "wait-title" => {
                let (text, ms) = split_trailing_ms(rest, 10_000);
                let end = Instant::now() + Duration::from_millis(ms);
                while !self.title.contains(&text) {
                    if Instant::now() >= end || self.timed_out() {
                        return Err(format!("title did not contain {text:?} within {ms} ms (title {:?})", self.title));
                    }
                    self.pump(Duration::from_millis(20));
                }
            }
            "wait-idle" => {
                let mut parts = rest.split_whitespace();
                let idle: u64 = parts.next().and_then(|s| s.parse().ok()).ok_or("wait-idle MS [MAX_MS]")?;
                let max: u64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(30_000);
                let end = Instant::now() + Duration::from_millis(max);
                self.last_change = Instant::now();
                while self.last_change.elapsed() < Duration::from_millis(idle) {
                    if Instant::now() >= end || self.timed_out() {
                        return Err(format!("screen kept changing for {max} ms"));
                    }
                    self.pump(Duration::from_millis(20));
                }
            }
            "wait-exit" => {
                let ms: u64 = if rest.is_empty() { 10_000 } else { rest.parse().map_err(|_| "wait-exit [MS]")? };
                let end = Instant::now() + Duration::from_millis(ms);
                while self.exited.is_none() {
                    if Instant::now() >= end || self.timed_out() {
                        return Err(format!("program did not exit within {ms} ms"));
                    }
                    self.pump(Duration::from_millis(50));
                }
            }
            "type" => {
                for ch in unescape(rest).chars() {
                    h.key(key_for_char(ch));
                }
                self.pump(Duration::from_millis(30));
            }
            "send" => {
                h.write(unescape(rest).into_bytes());
                self.pump(Duration::from_millis(30));
            }
            "key" => {
                let k = KeyInput::parse(rest).ok_or_else(|| format!("unknown key {rest:?}"))?;
                h.key(k);
                self.pump(Duration::from_millis(30));
            }
            "paste" => {
                h.paste(unescape(rest));
                self.pump(Duration::from_millis(30));
            }
            "wheel" => {
                let p: Vec<&str> = rest.split_whitespace().collect();
                if p.len() != 4 {
                    return Err("wheel up|down N COL ROW".into());
                }
                let n: i32 = p[1].parse().map_err(|_| "wheel N")?;
                let lines = if p[0] == "up" { -n } else { n };
                h.wheel(lines, p[2].parse().map_err(|_| "COL")?, p[3].parse().map_err(|_| "ROW")?, Mods::empty());
                self.pump(Duration::from_millis(30));
            }
            "click" => {
                let p: Vec<u16> = rest.split_whitespace().filter_map(|s| s.parse().ok()).collect();
                if p.len() != 2 {
                    return Err("click COL ROW".into());
                }
                let mut press = MouseInput::at_cell(MouseAction::Press, Some(MouseButton::Left), p[0], p[1], self.size);
                h.mouse(press);
                press.action = MouseAction::Release;
                h.mouse(press);
                self.pump(Duration::from_millis(30));
            }
            "drag" => {
                let p: Vec<u16> = rest.split_whitespace().filter_map(|s| s.parse().ok()).collect();
                if p.len() != 4 {
                    return Err("drag C1 R1 C2 R2".into());
                }
                let size = self.size;
                let at = |action, c, r, held| {
                    let mut m = MouseInput::at_cell(action, Some(MouseButton::Left), c, r, size);
                    m.any_pressed = held;
                    m
                };
                h.mouse(at(MouseAction::Press, p[0], p[1], true));
                self.pump(Duration::from_millis(30));
                // Move cell by cell, as a real drag reports motion.
                let steps = p[0].abs_diff(p[2]).max(p[1].abs_diff(p[3])).max(1);
                for i in 1..=steps {
                    let lerp = |a: u16, b: u16| (a as i32 + (b as i32 - a as i32) * i as i32 / steps as i32) as u16;
                    h.mouse(at(MouseAction::Motion, lerp(p[0], p[2]), lerp(p[1], p[3]), true));
                    self.pump(Duration::from_millis(10));
                }
                h.mouse(at(MouseAction::Release, p[2], p[3], false));
                self.pump(Duration::from_millis(50));
            }
            "focus" => {
                h.focus(rest == "in");
                self.pump(Duration::from_millis(30));
            }
            "resize" => {
                let p: Vec<u16> = rest.split_whitespace().filter_map(|s| s.parse().ok()).collect();
                if p.len() != 2 {
                    return Err("resize COLS ROWS".into());
                }
                self.size = GridSize { cols: p[0], rows: p[1], ..self.size };
                h.resize(self.size);
                self.pump(Duration::from_millis(50));
            }
            "expect" => {
                let t = unescape(rest);
                if !self.snap().contains(&t) {
                    return Err(format!("expected on screen: {t:?}"));
                }
            }
            "expect-not" => {
                let t = unescape(rest);
                if self.snap().contains(&t) {
                    return Err(format!("not expected on screen: {t:?}"));
                }
            }
            "print" => eprintln!("----- screen -----\n{}\n------------------", self.snap().text()),
            "modes" => {
                let s = self.snap();
                eprintln!("[modes] {:?} cursor={:?} title={:?}", s.modes, s.cursor, s.title);
            }
            "save" => {
                std::fs::write(rest, self.snap().text() + "\n").map_err(|e| format!("save {rest}: {e}"))?;
            }
            other => return Err(format!("unknown step {other:?}")),
        }
        Ok(())
    }
}

/// Key event for one typed character, as a keyboard would produce it.
fn key_for_char(ch: char) -> KeyInput {
    match ch {
        '\n' | '\r' => KeyInput::press(Key::Enter),
        '\t' => KeyInput::press(Key::Tab),
        '\x1b' => KeyInput::press(Key::Escape),
        '\x7f' => KeyInput::press(Key::Backspace),
        c if c.is_ascii_uppercase() => {
            let mut k = KeyInput::parse(&c.to_ascii_lowercase().to_string()).expect("letter");
            k.mods = Mods::SHIFT;
            k.consumed_mods = Mods::SHIFT;
            k.text = Some(c.to_string());
            k
        }
        c => match KeyInput::parse(&c.to_string()) {
            Some(mut k) => {
                k.text = Some(c.to_string());
                k
            }
            // Non-ASCII (e.g. Hangul): committed IME text, no key code.
            None => KeyInput::press(Key::Unidentified).with_text(c.to_string()),
        },
    }
}

fn split_trailing_ms(rest: &str, default: u64) -> (String, u64) {
    if let Some((t, last)) = rest.rsplit_once(' ') {
        if let Ok(ms) = last.parse::<u64>() {
            return (unescape(t), ms);
        }
    }
    (unescape(rest), default)
}

fn escape_bytes(b: &[u8]) -> String {
    let mut s = String::new();
    for &c in b {
        match c {
            0x1b => s.push_str("\\e"),
            b'\r' => s.push_str("\\r"),
            b'\n' => s.push_str("\\n"),
            0x07 => s.push_str("\\a"),
            0x20..=0x7e => s.push(c as char),
            _ => s.push_str(&format!("\\x{c:02x}")),
        }
    }
    s
}

fn grid_dump(s: &Snapshot) -> String {
    let mut out = format!("# {}x{} cursor={:?} modes={:?}\n", s.cols, s.rows, s.cursor, s.modes);
    for y in 0..s.rows {
        out.push_str(&format!("{y:3}| {}\n", s.row_text(y)));
    }
    out
}

fn main() {
    let args = parse_args();
    let size = GridSize::new(args.cols, args.rows);
    let mut spec = CommandSpec::new(&args.program[0]).args(&args.program[1..]);
    for (k, v) in &args.env {
        spec = spec.env(k, v);
    }
    if let Some(d) = &args.cwd {
        spec = spec.cwd(d);
    }
    let mut cfg = SessionConfig::new(spec);
    cfg.size = size;
    cfg.trace_replies = args.trace;
    cfg.raw_log = args.raw_log.clone();

    let session = match Session::spawn(cfg) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("haetty-headless: cannot start {}: {e}", args.program[0]);
            std::process::exit(2);
        }
    };
    if args.trace {
        eprintln!("[pty] {} pid={:?}", session.handle.backend(), session.handle.pid());
    }
    let mut r = Runner {
        session,
        size,
        trace: args.trace,
        exited: None,
        title: String::new(),
        deadline: Instant::now() + args.timeout,
        last_change: Instant::now(),
    };

    let mut failed = false;
    for (i, step) in args.steps.iter().enumerate() {
        if r.trace {
            eprintln!("[step {}] {step}", i + 1);
        }
        if let Err(e) = r.step(step) {
            eprintln!("haetty-headless: step {} ({step}) failed: {e}", i + 1);
            failed = true;
            break;
        }
    }
    r.pump(Duration::from_millis(50));

    let s = r.snap();
    let out = match args.dump.as_str() {
        "none" => String::new(),
        "grid" => grid_dump(&s),
        "vt" => r.session.handle.dump(DumpFormat::Vt).unwrap_or_default(),
        "html" => r.session.handle.dump(DumpFormat::Html).unwrap_or_default(),
        _ => s.text(),
    };
    if !out.is_empty() {
        let _ = writeln!(std::io::stdout(), "{out}");
    }
    std::process::exit(if failed { 1 } else { 0 });
}
