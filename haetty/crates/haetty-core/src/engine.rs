//! The engine thread. It owns the terminal engine ([`VtEngine`]; libghostty-vt's
//! is `!Send`).
//! Every byte for the child — replies to terminal queries and encoded user
//! input alike — goes, in the order it was produced, through one queue to
//! the session's PTY writer thread ([`Out`]): the engine never blocks on a
//! child that does not read its input (stage S4: a query flood from such a
//! child filled the PTY's input buffer, the engine blocked in `write`, and
//! every snapshot request — the daemon's too — waited for ever).

use std::fs::File;
use std::io::Write;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Select, Sender, TryRecvError};

use haetty_pty::{ExitStatus, PtyControl, PtySize};

use crate::osc::{OscEvent, OscScanner};
use crate::types::*;
use crate::vt::{VtEngine, VtInit, VtMode};
use crate::{ColorSchemePref, SessionEvent};

const EXIT_GRACE: Duration = Duration::from_millis(500);

/// Output chunks (each up to 64 KiB) taken between looks at the frontends'
/// commands.
const DATA_PER_TURN: usize = 16;

/// Replies to terminal queries waiting for the child beyond this are
/// dropped (a child that floods queries and does not read gets no more
/// answers until it reads).
pub(crate) const REPLY_CAP: usize = 64 << 10;
/// Input (keys, paste, mouse) waiting for the child beyond this is dropped,
/// with an error event.
pub(crate) const INPUT_CAP: usize = 64 << 20;

/// The queue to the PTY writer thread.
struct Out {
    tx: Option<Sender<Vec<u8>>>,
    queued: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    /// The session ends: the writer discards what is left.
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    events: Sender<SessionEvent>,
    dropped_replies: u64,
    dropped_input: u64,
}

impl Out {
    fn new(mut writer: Box<dyn Write + Send>, events: Sender<SessionEvent>) -> Out {
        use std::sync::atomic::Ordering;
        let (tx, rx) = crossbeam_channel::unbounded::<Vec<u8>>();
        let queued = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (q, ev, st) = (queued.clone(), events.clone(), stop.clone());
        let spawned = std::thread::Builder::new().name("haetty-pty-writer".into()).spawn(move || {
            let mut failed = false;
            for b in rx {
                // At most 4 KiB per write, the stop flag checked between
                // them: once the session ends and the terminal's queues are
                // flushed (`PtyControl::hangup`), a blocked write completes
                // and nothing more is written.
                for chunk in b.chunks(4096) {
                    if failed || st.load(Ordering::Relaxed) {
                        break;
                    }
                    if let Err(e) = writer.write_all(chunk).and_then(|_| writer.flush()) {
                        failed = true; // the child is gone; drain the rest
                                       // After the session's end a failed write is expected
                                       // (the terminal was closed under it): no error then.
                        if !st.load(Ordering::Relaxed) {
                            let _ = ev.send(SessionEvent::Error(format!("pty write: {e}")));
                        }
                    }
                }
                q.fetch_sub(b.len(), Ordering::Relaxed);
            }
        });
        let tx = match spawned {
            Ok(_) => Some(tx),
            Err(e) => {
                let _ = events.send(SessionEvent::Error(format!("pty writer thread: {e}")));
                None
            }
        };
        Out { tx, queued, stop, events, dropped_replies: 0, dropped_input: 0 }
    }

    fn queued(&self) -> usize {
        self.queued.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn push(&self, bytes: &[u8]) {
        if let Some(tx) = &self.tx {
            self.queued.fetch_add(bytes.len(), std::sync::atomic::Ordering::Relaxed);
            let _ = tx.send(bytes.to_vec());
        }
    }

    /// Encoded user input: in order, up to [`INPUT_CAP`] waiting.
    fn input(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        if self.queued() + bytes.len() > INPUT_CAP {
            self.dropped_input += 1;
            let _ = self.events.send(SessionEvent::Error(format!(
                "input dropped ({} bytes): the program has not read {} bytes",
                bytes.len(),
                self.queued()
            )));
            return;
        }
        self.push(bytes);
    }

    /// Answers to the child's queries: dropped while more than
    /// [`REPLY_CAP`] bytes wait (it is not reading).
    fn reply(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        if self.queued() + bytes.len() > REPLY_CAP {
            self.dropped_replies += 1;
            if self.dropped_replies.is_power_of_two() {
                log::debug!("{} query replies dropped: the program is not reading its input", self.dropped_replies);
            }
            return;
        }
        self.push(bytes);
    }
}

pub(crate) enum Cmd {
    Output(Vec<u8>),
    OutputClosed,
    ChildExited(ExitStatus),
    Input(Vec<u8>),
    Key(KeyInput),
    Mouse(MouseInput),
    Wheel { lines: i32, col: u16, row: u16, mods: Mods },
    Paste(String),
    Focus(bool),
    Resize(GridSize),
    Scroll(ScrollTo),
    Snapshot(Sender<Snapshot>),
    Dump(DumpFormat, Sender<String>),
    SetColorScheme(ColorSchemePref),
    SetColors(crate::TermColors),
    Shutdown,
}

pub(crate) struct EngineConfig {
    pub size: GridSize,
    /// Bytes (libghostty-vt `max_scrollback`).
    pub scrollback_limit: usize,
    pub color_scheme: ColorSchemePref,
    pub default_fg: Rgb,
    pub default_bg: Rgb,
    pub palette: Option<[Rgb; 16]>,
    pub cursor_color: Option<Rgb>,
    pub sync_timeout: Duration,
    pub xtversion: String,
    pub trace_replies: bool,
    pub raw_log: Option<File>,
    pub engine: crate::vt::EngineKind,
}

pub(crate) fn run(
    cfg: EngineConfig,
    rx: Receiver<Cmd>,
    data: Receiver<Cmd>,
    events: Sender<SessionEvent>,
    writer: Box<dyn Write + Send>,
    control: Box<dyn PtyControl>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    let mut out = Out::new(writer, events.clone());
    let mut cfg = cfg;
    cfg.size = cfg.size.clamped();
    let init = VtInit {
        size: cfg.size,
        scrollback_limit: cfg.scrollback_limit,
        colors: crate::TermColors {
            default_fg: cfg.default_fg,
            default_bg: cfg.default_bg,
            palette: cfg.palette,
            cursor_color: cfg.cursor_color,
        },
        color_scheme: cfg.color_scheme,
        xtversion: cfg.xtversion.clone(),
    };
    let mut term: Box<dyn VtEngine> = match crate::vt::new_engine(cfg.engine, &init) {
        Ok((t, warning)) => {
            if let Some(w) = warning {
                log::warn!("{w}");
            }
            t
        }
        Err(e) => {
            let _ = events.send(SessionEvent::Error(e));
            return;
        }
    };
    let mut size = cfg.size;

    let mut st = State {
        osc: OscScanner::new(),
        seq: 0,
        wakeup_pending: false,
        dirty: false,
        sync_since: None,
        out_buf: Vec::with_capacity(256),
    };

    let mut exit: Option<ExitStatus> = None;
    let mut exit_seen: Option<Instant> = None;
    let mut output_closed = false;
    // The child's exit was seen (its pid may be reused after that).
    let mut child_gone = false;
    // The PTY reader and the child waiter still send.
    let mut data_open = true;

    loop {
        // While the application holds synchronized output, wait no longer
        // than the timeout before forcing a redraw; after the child exited,
        // no longer than the grace period for the last output.
        let deadline = [st.sync_since.map(|t0| t0 + cfg.sync_timeout), exit_seen.map(|t| t + EXIT_GRACE)]
            .into_iter()
            .flatten()
            .min();
        // Commands from frontends (snapshots, input, resizes, shutdown) come
        // before queued output: a session busy with output (a firehose, or a
        // slow stream) answers a snapshot after at most DATA_PER_TURN output
        // chunks, and nobody blocks sending to it (stage S4 follow-up).
        let first = match rx.try_recv() {
            Ok(c) => Some(c),
            Err(TryRecvError::Disconnected) => break,
            Err(TryRecvError::Empty) => {
                let mut sel = Select::new_biased();
                let i_ctl = sel.recv(&rx);
                if data_open {
                    sel.recv(&data);
                }
                let op = match deadline {
                    Some(d) => sel.select_deadline(d).ok(),
                    None => Some(sel.select()),
                };
                match op {
                    None => None,
                    Some(op) if op.index() == i_ctl => match op.recv(&rx) {
                        Ok(c) => Some(c),
                        Err(_) => break,
                    },
                    Some(op) => match op.recv(&data) {
                        Ok(c) => Some(c),
                        Err(_) => {
                            data_open = false; // reader and waiter are done
                            None
                        }
                    },
                }
            }
        };

        let mut batch: Vec<Cmd> = first.into_iter().collect();
        // Drain what is queued: all commands, and output so a burst costs one
        // wakeup — at most DATA_PER_TURN chunks before commands are looked at
        // again.
        let mut data_taken = 0;
        while batch.len() < 256 {
            if let Ok(c) = rx.try_recv() {
                batch.push(c);
                continue;
            }
            if !data_open || data_taken >= DATA_PER_TURN {
                break;
            }
            match data.try_recv() {
                Ok(c) => {
                    data_taken += 1;
                    batch.push(c);
                }
                Err(TryRecvError::Disconnected) => {
                    data_open = false;
                    break;
                }
                Err(TryRecvError::Empty) => break,
            }
        }

        let mut shutdown = false;
        for cmd in batch {
            // Dropped: no more work on queued output.
            if stop.load(std::sync::atomic::Ordering::Relaxed) {
                shutdown = true;
                break;
            }
            match cmd {
                Cmd::Output(data) => {
                    if let Some(f) = cfg.raw_log.as_mut() {
                        let _ = f.write_all(&data);
                    }
                    for ev in st.osc.feed(&data) {
                        let _ = events.send(match ev {
                            OscEvent::Notification { title, body } => SessionEvent::Notification { title, body },
                            OscEvent::Progress(p) => SessionEvent::Progress(p),
                        });
                    }
                    term.feed(&data);
                    st.dirty = true;
                    flush_pending(term.as_mut(), &mut out, &events, cfg.trace_replies);
                }
                Cmd::OutputClosed => output_closed = true,
                Cmd::ChildExited(s) => {
                    child_gone = true;
                    exit = Some(s);
                    exit_seen = Some(Instant::now());
                }
                Cmd::Input(bytes) => out.input(&bytes),
                Cmd::Key(k) => {
                    st.out_buf.clear();
                    term.encode_key(&k, &mut st.out_buf);
                    let buf = std::mem::take(&mut st.out_buf);
                    log_key_bytes(&k, &buf);
                    out.input(&buf);
                    st.out_buf = buf;
                    // Typing returns the viewport to the live screen.
                    if k.action != KeyAction::Release {
                        term.scroll_viewport(ScrollTo::Bottom);
                    }
                }
                Cmd::Mouse(m) => {
                    st.out_buf.clear();
                    term.encode_mouse(&m, size, &mut st.out_buf);
                    if !st.out_buf.is_empty() {
                        let buf = std::mem::take(&mut st.out_buf);
                        out.input(&buf);
                        st.out_buf = buf;
                    }
                }
                Cmd::Wheel { lines, col, row, mods } => {
                    let gs = size;
                    if term.mode(VtMode::MouseTracking) {
                        let btn = if lines < 0 { MouseButton::Four } else { MouseButton::Five };
                        st.out_buf.clear();
                        // Wheel "buttons" are reported as presses only, as
                        // xterm and Ghostty do; a release would read as a
                        // second notch to some applications.
                        for _ in 0..lines.unsigned_abs() {
                            let mut m = MouseInput::at_cell(MouseAction::Press, Some(btn), col, row, gs);
                            m.mods = mods;
                            term.encode_mouse(&m, gs, &mut st.out_buf);
                        }
                        let buf = std::mem::take(&mut st.out_buf);
                        out.input(&buf);
                        st.out_buf = buf;
                    } else {
                        term.scroll_viewport(ScrollTo::Delta(lines as isize));
                        st.dirty = true;
                    }
                }
                Cmd::Paste(text) => {
                    let mut bytes = Vec::with_capacity(text.len() + 12);
                    term.encode_paste(&text, &mut bytes);
                    out.input(&bytes);
                    term.scroll_viewport(ScrollTo::Bottom);
                }
                Cmd::Focus(focused) => {
                    let mut buf = Vec::new();
                    term.encode_focus(focused, &mut buf);
                    out.input(&buf);
                }
                Cmd::Resize(gs) => {
                    let gs = gs.clamped();
                    if gs != size {
                        size = gs;
                        term.resize(gs);
                        let _ = control.resize(PtySize {
                            cols: gs.cols,
                            rows: gs.rows,
                            pixel_width: gs.cols.saturating_mul(gs.cell_width),
                            pixel_height: gs.rows.saturating_mul(gs.cell_height),
                        });
                        st.dirty = true;
                        // A resize may answer an in-band size report (mode 2048).
                        flush_pending(term.as_mut(), &mut out, &events, cfg.trace_replies);
                    }
                }
                Cmd::Scroll(to) => {
                    term.scroll_viewport(to);
                    st.dirty = true;
                }
                Cmd::Snapshot(reply) => {
                    st.wakeup_pending = false;
                    st.seq += 1;
                    let mut snap = term.snapshot();
                    snap.seq = st.seq;
                    let _ = reply.send(snap);
                }
                Cmd::Dump(fmt, reply) => {
                    let _ = reply.send(term.dump(fmt));
                }
                Cmd::SetColors(c) => {
                    term.set_colors(&c);
                    // Everything is drawn anew in the new colours.
                    st.dirty = true;
                }
                Cmd::SetColorScheme(s) => {
                    cfg.color_scheme = s;
                    let mut buf = Vec::new();
                    term.set_color_scheme(s, &mut buf);
                    out.input(&buf);
                }
                Cmd::Shutdown => shutdown = true,
            }
        }

        // Synchronized output (mode 2026): hold redraws while it is set, up
        // to the timeout.
        if st.dirty {
            let sync = term.mode(VtMode::SyncOutput);
            let now = Instant::now();
            let hold = match (sync, st.sync_since) {
                (true, None) => {
                    st.sync_since = Some(now);
                    true
                }
                (true, Some(t0)) => now < t0 + cfg.sync_timeout,
                (false, _) => false,
            };
            if !hold {
                st.sync_since = if sync { Some(now) } else { None };
                st.dirty = false;
                if !st.wakeup_pending {
                    st.wakeup_pending = true;
                    let _ = events.send(SessionEvent::Wakeup);
                }
            }
        } else if st.sync_since.is_some() && !term.mode(VtMode::SyncOutput) {
            st.sync_since = None;
        }

        // Report the exit once the output is drained, or after a grace period
        // if something else (a background grandchild) keeps the PTY open.
        let grace_over = exit_seen.is_some_and(|t| t.elapsed() >= EXIT_GRACE);
        if exit.is_some() && (output_closed || grace_over) {
            let _ = events.send(SessionEvent::Exited(exit.take().unwrap()));
            exit_seen = None;
        }
        if shutdown {
            break;
        }
    }
    // Bytes still wait for a child that does not read: the PTY writer
    // thread may be blocked in `write`, holding the terminal open, so the
    // child would never get its hangup. Give it the hangup directly (stage
    // S4 follow-up: closed sessions had kept their child and four threads).
    out.stop.store(true, std::sync::atomic::Ordering::Relaxed);
    if !child_gone && out.queued() > 0 {
        log::debug!("session ends with {} bytes unread by the child: hangup", out.queued());
        control.hangup();
    }
}

struct State {
    osc: OscScanner,
    seq: u64,
    wakeup_pending: bool,
    dirty: bool,
    sync_since: Option<Instant>,
    out_buf: Vec<u8>,
}

fn flush_pending(term: &mut dyn VtEngine, out: &mut Out, events: &Sender<SessionEvent>, trace: bool) {
    let replies = term.take_replies();
    if !replies.is_empty() {
        out.reply(&replies);
        if trace {
            let _ = events.send(SessionEvent::Reply(replies));
        }
    }
    for e in term.take_events() {
        let _ = events.send(e);
    }
}

/// Debug log of what a key sent to the PTY — only for keys that do not type
/// text (no text, or with Ctrl/Alt/Super), so typed text never reaches a log.
fn log_key_bytes(k: &KeyInput, bytes: &[u8]) {
    if !log::log_enabled!(log::Level::Debug) || !is_shortcut_or_special(k) {
        return;
    }
    let hex: Vec<String> = bytes.iter().map(|b| format!("{b:02x}")).collect();
    log::debug!("key {:?} {:?} mods={:?} -> [{}]", k.key, k.action, k.mods, hex.join(" "));
}

/// A key whose encoding does not carry typed text: no text and no
/// unshifted character (Enter, arrows, F-keys…), or a Ctrl/Alt/Super
/// combination.
pub(crate) fn is_shortcut_or_special(k: &KeyInput) -> bool {
    k.mods.intersects(Mods::CTRL | Mods::ALT | Mods::SUPER) || (k.text.is_none() && k.unshifted.is_none())
}
