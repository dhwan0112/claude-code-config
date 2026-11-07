//! haetty terminal backend.
//!
//! A [`Session`] runs one program in a pseudo-terminal and keeps a
//! libghostty-vt terminal up to date with its output. Three threads are
//! involved:
//!
//! * a reader thread that only moves PTY output into a channel,
//! * a waiter thread that reports the child's exit,
//! * the engine thread, which owns the `Terminal` (`!Send`) and the PTY
//!   writer, answers terminal queries, encodes input and produces snapshots.
//!
//! A frontend talks to the engine through a cloneable [`SessionHandle`] and
//! receives [`SessionEvent`]s; on [`SessionEvent::Wakeup`] it asks for a
//! [`Snapshot`] and draws it. No libghostty object leaves the engine thread.

mod engine;
pub mod ghostty;
#[cfg(feature = "native")]
pub mod native;
pub mod osc;
pub mod theme;
pub mod types;
pub mod vt;

use std::ffi::OsString;
use std::io::Read;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use crossbeam_channel::{bounded, unbounded, Receiver, Sender};

pub use haetty_pty::{CommandSpec, ExitStatus, PtyOptions, PtySize};
pub use osc::Progress;
pub use types::*;

use engine::{Cmd, EngineConfig};

/// Version reported to applications through XTVERSION (`CSI > q`).
pub const XTVERSION: &str = concat!("haetty(", env!("CARGO_PKG_VERSION"), ")");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorSchemePref {
    Dark,
    Light,
    /// Do not answer colour-scheme queries.
    Unknown,
}

/// The colours a theme gives the terminal itself (default foreground and
/// background, colours 0–15, cursor); the rest of a theme (cursor text,
/// selection) is the renderer's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TermColors {
    pub default_fg: Rgb,
    pub default_bg: Rgb,
    /// `None`: libghostty-vt's built-in colours 0–15.
    pub palette: Option<[Rgb; 16]>,
    /// `None`: no cursor colour set (the renderer uses the foreground).
    pub cursor_color: Option<Rgb>,
}

impl TermColors {
    pub fn of(t: &theme::Theme) -> TermColors {
        TermColors {
            default_fg: t.foreground,
            default_bg: t.background,
            palette: Some(t.palette),
            cursor_color: t.cursor_color,
        }
    }
}

/// Default [`SessionConfig::scrollback_limit`]: 12 MB keeps about 11 000
/// rows of history at 120 columns (measured with `examples/mem.rs`; about
/// 1.1 KB per 120-column row), about 12.9 MB of anonymous memory per session.
pub const DEFAULT_SCROLLBACK_LIMIT: usize = 12_000_000;

#[derive(Clone, Debug)]
pub struct SessionConfig {
    pub command: CommandSpec,
    pub size: GridSize,
    /// Scrollback limit in **bytes** of terminal pages (libghostty-vt's
    /// `max_scrollback`, like Ghostty's `scrollback-limit`), rounded to whole
    /// pages. How many rows fit depends on the width: rows of 120 columns
    /// take about 1.1 KB.
    pub scrollback_limit: usize,
    pub pty: PtyOptions,
    pub color_scheme: ColorSchemePref,
    /// Default foreground/background. Applications query these with OSC 10/11
    /// (Claude Code and others pick a light or dark palette from the answer).
    pub default_fg: Rgb,
    pub default_bg: Rgb,
    /// Colours 0–15 (OSC 4 answers them; SGR 30–37/90–97 and 38;5;0–15 use
    /// them). `None`: libghostty-vt's built-in palette. 16–255 always stay
    /// xterm's colour cube and grey ramp.
    pub palette: Option<[Rgb; 16]>,
    /// Default cursor colour (OSC 12 answers it). `None`: none set; the
    /// renderer draws the cursor in the foreground colour.
    pub cursor_color: Option<Rgb>,
    /// Longest time a redraw is held back while the application keeps
    /// synchronized output (mode 2026) set.
    pub sync_timeout: Duration,
    /// Set TERM/COLORTERM/TERM_PROGRAM and drop TMUX/TMUX_PANE in the child.
    pub terminal_env: bool,
    /// Emit [`SessionEvent::Reply`] for every reply sent to a query.
    pub trace_replies: bool,
    /// Append all PTY output to this file (debugging).
    pub raw_log: Option<PathBuf>,
}

impl SessionConfig {
    /// Colours from the default theme ([`theme::DEFAULT_THEME`]).
    pub fn new(command: CommandSpec) -> Self {
        let mut c = Self {
            command,
            size: GridSize::new(120, 36),
            scrollback_limit: DEFAULT_SCROLLBACK_LIMIT,
            pty: PtyOptions::default(),
            color_scheme: ColorSchemePref::Dark,
            default_fg: Rgb::default(),
            default_bg: Rgb::default(),
            palette: None,
            cursor_color: None,
            sync_timeout: Duration::from_millis(500),
            terminal_env: true,
            trace_replies: false,
            raw_log: None,
        };
        c.apply_theme(&theme::Theme::default());
        c
    }

    /// Default colours, colours 0–15 and the cursor colour from `t` (the
    /// selection and cursor-text colours are the renderer's).
    pub fn apply_theme(&mut self, t: &theme::Theme) {
        self.default_fg = t.foreground;
        self.default_bg = t.background;
        self.palette = Some(t.palette);
        self.cursor_color = t.cursor_color;
    }
}

#[derive(Clone, Debug)]
pub enum SessionEvent {
    /// The screen changed; call [`SessionHandle::snapshot`]. Not repeated
    /// until a snapshot has been taken.
    Wakeup,
    Title(String),
    Pwd(String),
    Bell,
    /// OSC 52 write from the application (e.g. Claude Code copying a
    /// selection). The frontend decides whether to put it on the clipboard.
    ClipboardWrite(String),
    Notification {
        title: Option<String>,
        body: String,
    },
    Progress(Progress),
    /// Bytes sent back to the application in answer to a query (only with
    /// `trace_replies`).
    Reply(Vec<u8>),
    Exited(ExitStatus),
    Error(String),
}

/// Cloneable, `Send` handle to a running session.
#[derive(Clone)]
pub struct SessionHandle {
    tx: Sender<Cmd>,
    /// Set when the session is dropped: the engine stops without working
    /// through queued output first (stage S4).
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    backend: String,
    pid: Option<u32>,
}

pub struct Session {
    pub handle: SessionHandle,
    pub events: Receiver<SessionEvent>,
    engine: Option<thread::JoinHandle<()>>,
}

impl Session {
    pub fn spawn(cfg: SessionConfig) -> std::io::Result<Session> {
        let mut command = cfg.command.clone();
        if cfg.terminal_env {
            let mut env: Vec<(OsString, Option<OsString>)> = vec![
                ("TERM".into(), Some("xterm-256color".into())),
                ("COLORTERM".into(), Some("truecolor".into())),
                ("TERM_PROGRAM".into(), Some("haetty".into())),
                ("TERM_PROGRAM_VERSION".into(), Some(env!("CARGO_PKG_VERSION").into())),
                // Claude Code lowers its colour depth when it sees TMUX.
                ("TMUX".into(), None),
                ("TMUX_PANE".into(), None),
            ];
            // The caller's own settings win.
            env.append(&mut command.env);
            command.env = env;
        }

        let gs = cfg.size.clamped();
        let pty_size = PtySize {
            cols: gs.cols.max(1),
            rows: gs.rows.max(1),
            pixel_width: gs.cols.saturating_mul(gs.cell_width),
            pixel_height: gs.rows.saturating_mul(gs.cell_height),
        };
        let spawned = haetty_pty::spawn(&command, pty_size, &cfg.pty)?;
        let backend = spawned.control.describe();
        let pid = spawned.child.pid();

        let raw_log = match &cfg.raw_log {
            Some(p) => Some(std::fs::OpenOptions::new().create(true).append(true).open(p)?),
            None => None,
        };

        // Frontends' commands (unbounded: a sender never waits for a busy
        // engine) and the PTY's output (bounded: the reader waits).
        let (tx, rx) = unbounded::<Cmd>();
        let (dtx, drx) = bounded::<Cmd>(1024);
        let (ev_tx, ev_rx) = unbounded::<SessionEvent>();

        // Reader: PTY output -> engine.
        let mut reader = spawned.reader;
        let tx_r = dtx.clone();
        thread::Builder::new().name("haetty-pty-reader".into()).spawn(move || {
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if tx_r.send(Cmd::Output(buf[..n].to_vec())).is_err() {
                            return;
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    // EIO on Linux once the child side is gone.
                    Err(_) => break,
                }
            }
            let _ = tx_r.send(Cmd::OutputClosed);
        })?;

        // Waiter: child exit -> engine.
        let mut child = spawned.child;
        let tx_w = dtx;
        thread::Builder::new().name("haetty-child-waiter".into()).spawn(move || {
            let status = child.wait().unwrap_or(ExitStatus { code: None, signal: None });
            let _ = tx_w.send(Cmd::ChildExited(status));
        })?;

        let ecfg = EngineConfig {
            size: gs,
            scrollback_limit: cfg.scrollback_limit,
            color_scheme: cfg.color_scheme,
            default_fg: cfg.default_fg,
            default_bg: cfg.default_bg,
            palette: cfg.palette,
            cursor_color: cfg.cursor_color,
            sync_timeout: cfg.sync_timeout,
            xtversion: XTVERSION.to_owned(),
            trace_replies: cfg.trace_replies,
            raw_log,
        };
        let writer = spawned.writer;
        let control = spawned.control;
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stop_e = stop.clone();
        let engine = thread::Builder::new()
            .name("haetty-engine".into())
            .spawn(move || engine::run(ecfg, rx, drx, ev_tx, writer, control, stop_e))?;

        Ok(Session { handle: SessionHandle { tx, backend, pid, stop }, events: ev_rx, engine: Some(engine) })
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.handle.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        let _ = self.handle.tx.send(Cmd::Shutdown);
        if let Some(j) = self.engine.take() {
            let _ = j.join();
        }
    }
}

impl SessionHandle {
    fn send(&self, c: Cmd) {
        let _ = self.tx.send(c);
    }
    /// Description of the PTY backend (for logs and the status panel).
    pub fn backend(&self) -> &str {
        &self.backend
    }
    pub fn pid(&self) -> Option<u32> {
        self.pid
    }
    /// Raw bytes to the application, unencoded.
    pub fn write(&self, bytes: impl Into<Vec<u8>>) {
        self.send(Cmd::Input(bytes.into()));
    }
    pub fn key(&self, k: KeyInput) {
        self.send(Cmd::Key(k));
    }
    pub fn mouse(&self, m: MouseInput) {
        self.send(Cmd::Mouse(m));
    }
    /// Wheel at a cell: forwarded as buttons 4/5 when the application tracks
    /// the mouse, otherwise scrolls the viewport. Negative = up.
    pub fn wheel(&self, lines: i32, col: u16, row: u16, mods: Mods) {
        self.send(Cmd::Wheel { lines, col, row, mods });
    }
    pub fn paste(&self, text: impl Into<String>) {
        self.send(Cmd::Paste(text.into()));
    }
    pub fn focus(&self, focused: bool) {
        self.send(Cmd::Focus(focused));
    }
    pub fn resize(&self, size: GridSize) {
        self.send(Cmd::Resize(size));
    }
    pub fn scroll(&self, to: ScrollTo) {
        self.send(Cmd::Scroll(to));
    }
    pub fn set_color_scheme(&self, s: ColorSchemePref) {
        self.send(Cmd::SetColorScheme(s));
    }

    /// New default colours, colours 0–15 and cursor colour while the session
    /// runs (a theme change): queries (OSC 4/10/11/12) answer with them and
    /// everything drawn in default or palette colours takes them at once;
    /// colours a program set itself (OSC 4/10/11/12, 24-bit SGR) stay.
    pub fn set_colors(&self, c: TermColors) {
        self.send(Cmd::SetColors(c));
    }
    /// Current viewport. Blocks until the engine has processed everything
    /// queued before this call.
    pub fn snapshot(&self) -> Option<Snapshot> {
        let (tx, rx) = bounded(1);
        self.send(Cmd::Snapshot(tx));
        rx.recv().ok()
    }
    pub fn dump(&self, fmt: DumpFormat) -> Option<String> {
        let (tx, rx) = bounded(1);
        self.send(Cmd::Dump(fmt, tx));
        rx.recv().ok()
    }
}
