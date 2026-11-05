//! The engine thread. It owns the libghostty-vt `Terminal` (which is `!Send`)
//! and the PTY writer, so every byte written to the child — replies to
//! terminal queries and encoded user input alike — leaves from one thread in
//! the order it was produced.

use std::cell::RefCell;
use std::fs::File;
use std::io::Write;
use std::rc::Rc;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};
use libghostty_vt::fmt::{Format as FmtFormat, Formatter, FormatterOptions};
use libghostty_vt::key;
use libghostty_vt::mouse;
use libghostty_vt::render::{CellIterator, CursorVisualStyle, RowIterator};
use libghostty_vt::screen::CellWide;
use libghostty_vt::terminal::{
    ColorScheme, ConformanceLevel, DeviceAttributeFeature, DeviceAttributes, DeviceType, Mode, PrimaryDeviceAttributes,
    ScrollViewport, SecondaryDeviceAttributes, SizeReportSize,
};
use libghostty_vt::{RenderState, Terminal, TerminalOptions};

use ccterm_pty::{ExitStatus, PtyControl, PtySize};

use crate::osc::{OscEvent, OscScanner};
use crate::types::*;
use crate::{ColorSchemePref, SessionEvent};

const EXIT_GRACE: Duration = Duration::from_millis(500);

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
    Shutdown,
}

pub(crate) struct EngineConfig {
    pub size: GridSize,
    /// Bytes (libghostty-vt `max_scrollback`).
    pub scrollback_limit: usize,
    pub color_scheme: ColorSchemePref,
    pub default_fg: Rgb,
    pub default_bg: Rgb,
    pub sync_timeout: Duration,
    pub xtversion: String,
    pub trace_replies: bool,
    pub raw_log: Option<File>,
}

/// Things libghostty callbacks produce while `vt_write` runs. They are queued
/// and handled after the call returns, never from inside a callback.
#[derive(Default)]
struct Pending {
    replies: Vec<u8>,
    events: Vec<SessionEvent>,
}

pub(crate) fn run(
    cfg: EngineConfig,
    rx: Receiver<Cmd>,
    events: Sender<SessionEvent>,
    mut writer: Box<dyn Write + Send>,
    control: Box<dyn PtyControl>,
) {
    let mut cfg = cfg;
    let pending = Rc::new(RefCell::new(Pending::default()));
    let size = Rc::new(std::cell::Cell::new(cfg.size));
    let scheme = Rc::new(std::cell::Cell::new(cfg.color_scheme));
    // Leaked once per session: the XTVERSION callback must return a &str that
    // outlives the call, and the string never changes.
    let xtversion: &'static str = Box::leak(cfg.xtversion.clone().into_boxed_str());

    let mut term = match Terminal::new(TerminalOptions {
        cols: cfg.size.cols.max(1),
        rows: cfg.size.rows.max(1),
        max_scrollback: cfg.scrollback_limit,
    }) {
        Ok(t) => t,
        Err(e) => {
            let _ = events.send(SessionEvent::Error(format!("terminal init: {e}")));
            return;
        }
    };
    if let Err(e) = install_callbacks(&mut term, &pending, &size, &scheme, xtversion) {
        let _ = events.send(SessionEvent::Error(format!("terminal callbacks: {e}")));
        return;
    }
    let rgb = |c: Rgb| libghostty_vt::style::RgbColor { r: c.r, g: c.g, b: c.b };
    let _ = term.set_default_fg_color(Some(rgb(cfg.default_fg)));
    let _ = term.set_default_bg_color(Some(rgb(cfg.default_bg)));
    let _ = term.resize(
        cfg.size.cols.max(1),
        cfg.size.rows.max(1),
        cfg.size.cell_width as u32,
        cfg.size.cell_height as u32,
    );

    let mut st = State {
        render: RenderState::new().expect("render state"),
        rows: RowIterator::new().expect("row iterator"),
        cells: CellIterator::new().expect("cell iterator"),
        key_enc: key::Encoder::new().expect("key encoder"),
        key_ev: key::Event::new().expect("key event"),
        mouse_enc: mouse::Encoder::new().expect("mouse encoder"),
        mouse_ev: mouse::Event::new().expect("mouse event"),
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

    loop {
        // While the application holds synchronized output, wait no longer
        // than the timeout before forcing a redraw; after the child exited,
        // no longer than the grace period for the last output.
        let deadline = [st.sync_since.map(|t0| t0 + cfg.sync_timeout), exit_seen.map(|t| t + EXIT_GRACE)]
            .into_iter()
            .flatten()
            .min();
        let first = match deadline {
            Some(d) => match rx.recv_deadline(d) {
                Ok(c) => Some(c),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            },
            None => match rx.recv() {
                Ok(c) => Some(c),
                Err(_) => break,
            },
        };

        let mut batch: Vec<Cmd> = first.into_iter().collect();
        // Drain whatever else is queued so a burst of output costs one wakeup.
        while batch.len() < 256 {
            match rx.try_recv() {
                Ok(c) => batch.push(c),
                Err(_) => break,
            }
        }

        let mut shutdown = false;
        for cmd in batch {
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
                    term.vt_write(&data);
                    st.dirty = true;
                    flush_pending(&pending, &mut writer, &events, cfg.trace_replies);
                }
                Cmd::OutputClosed => output_closed = true,
                Cmd::ChildExited(s) => {
                    exit = Some(s);
                    exit_seen = Some(Instant::now());
                }
                Cmd::Input(bytes) => write_all(&mut writer, &bytes, &events),
                Cmd::Key(k) => {
                    st.out_buf.clear();
                    if encode_key(&mut st, &term, &k).is_ok() {
                        let buf = std::mem::take(&mut st.out_buf);
                        log_key_bytes(&k, &buf);
                        write_all(&mut writer, &buf, &events);
                        st.out_buf = buf;
                    }
                    // Typing returns the viewport to the live screen.
                    if k.action != KeyAction::Release {
                        term.scroll_viewport(ScrollViewport::Bottom);
                    }
                }
                Cmd::Mouse(m) => {
                    st.out_buf.clear();
                    let gs = size.get();
                    if encode_mouse(&mut st, &term, &m, gs).is_ok() && !st.out_buf.is_empty() {
                        let buf = std::mem::take(&mut st.out_buf);
                        write_all(&mut writer, &buf, &events);
                        st.out_buf = buf;
                    }
                }
                Cmd::Wheel { lines, col, row, mods } => {
                    let gs = size.get();
                    if term.is_mouse_tracking().unwrap_or(false) {
                        let btn = if lines < 0 { MouseButton::Four } else { MouseButton::Five };
                        st.out_buf.clear();
                        // Wheel "buttons" are reported as presses only, as
                        // xterm and Ghostty do; a release would read as a
                        // second notch to some applications.
                        for _ in 0..lines.unsigned_abs() {
                            let mut m = MouseInput::at_cell(MouseAction::Press, Some(btn), col, row, gs);
                            m.mods = mods;
                            let _ = encode_mouse_append(&mut st, &term, &m, gs);
                        }
                        let buf = std::mem::take(&mut st.out_buf);
                        write_all(&mut writer, &buf, &events);
                        st.out_buf = buf;
                    } else {
                        term.scroll_viewport(ScrollViewport::Delta(lines as isize));
                        st.dirty = true;
                    }
                }
                Cmd::Paste(text) => {
                    let bracketed = term.mode(Mode::BRACKETED_PASTE).unwrap_or(false);
                    let bytes = encode_paste(&text, bracketed);
                    write_all(&mut writer, &bytes, &events);
                    term.scroll_viewport(ScrollViewport::Bottom);
                }
                Cmd::Focus(focused) => {
                    if term.mode(Mode::FOCUS_EVENT).unwrap_or(false) {
                        let ev = if focused {
                            libghostty_vt::focus::Event::Gained
                        } else {
                            libghostty_vt::focus::Event::Lost
                        };
                        let mut buf = [0u8; 8];
                        if let Ok(n) = ev.encode(&mut buf) {
                            write_all(&mut writer, &buf[..n], &events);
                        }
                    }
                }
                Cmd::Resize(gs) => {
                    let gs = GridSize { cols: gs.cols.max(1), rows: gs.rows.max(1), ..gs };
                    if gs != size.get() {
                        size.set(gs);
                        let _ = term.resize(gs.cols, gs.rows, gs.cell_width as u32, gs.cell_height as u32);
                        let _ = control.resize(PtySize {
                            cols: gs.cols,
                            rows: gs.rows,
                            pixel_width: gs.cols.saturating_mul(gs.cell_width),
                            pixel_height: gs.rows.saturating_mul(gs.cell_height),
                        });
                        st.dirty = true;
                        // A resize may answer an in-band size report (mode 2048).
                        flush_pending(&pending, &mut writer, &events, cfg.trace_replies);
                    }
                }
                Cmd::Scroll(to) => {
                    term.scroll_viewport(match to {
                        ScrollTo::Top => ScrollViewport::Top,
                        ScrollTo::Bottom => ScrollViewport::Bottom,
                        ScrollTo::Delta(d) => ScrollViewport::Delta(d),
                    });
                    st.dirty = true;
                }
                Cmd::Snapshot(reply) => {
                    st.wakeup_pending = false;
                    let _ = reply.send(build_snapshot(&mut st, &term));
                }
                Cmd::Dump(fmt, reply) => {
                    let _ = reply.send(dump(&term, fmt));
                }
                Cmd::SetColorScheme(s) => {
                    scheme.set(s);
                    cfg.color_scheme = s;
                    // Report the change to applications that asked for it (mode 2031).
                    if term.mode(Mode::COLOR_SCHEME_REPORT).unwrap_or(false) {
                        if let Some(cs) = to_scheme(s) {
                            let mut buf = [0u8; 16];
                            if let Ok(n) = cs.encode_report(&mut buf) {
                                write_all(&mut writer, &buf[..n], &events);
                            }
                        }
                    }
                }
                Cmd::Shutdown => shutdown = true,
            }
        }

        // Synchronized output (mode 2026): hold redraws while it is set, up
        // to the timeout.
        if st.dirty {
            let sync = term.mode(Mode::SYNC_OUTPUT).unwrap_or(false);
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
        } else if st.sync_since.is_some() && !term.mode(Mode::SYNC_OUTPUT).unwrap_or(false) {
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
}

struct State<'a> {
    render: RenderState<'a>,
    rows: RowIterator<'a>,
    cells: CellIterator<'a>,
    key_enc: key::Encoder<'a>,
    key_ev: key::Event<'a>,
    mouse_enc: mouse::Encoder<'a>,
    mouse_ev: mouse::Event<'a>,
    osc: OscScanner,
    seq: u64,
    wakeup_pending: bool,
    dirty: bool,
    sync_since: Option<Instant>,
    out_buf: Vec<u8>,
}

fn to_scheme(p: ColorSchemePref) -> Option<ColorScheme> {
    match p {
        ColorSchemePref::Dark => Some(ColorScheme::Dark),
        ColorSchemePref::Light => Some(ColorScheme::Light),
        ColorSchemePref::Unknown => None,
    }
}

fn install_callbacks(
    term: &mut Terminal<'static, 'static>,
    pending: &Rc<RefCell<Pending>>,
    size: &Rc<std::cell::Cell<GridSize>>,
    scheme: &Rc<std::cell::Cell<ColorSchemePref>>,
    xtversion: &'static str,
) -> Result<(), libghostty_vt::Error> {
    let p = pending.clone();
    term.on_pty_write(move |_t, data| p.borrow_mut().replies.extend_from_slice(data))?;

    let p = pending.clone();
    term.on_bell(move |_t| p.borrow_mut().events.push(SessionEvent::Bell))?;

    let p = pending.clone();
    term.on_title_changed(move |t| {
        let title = t.title().unwrap_or("").to_owned();
        p.borrow_mut().events.push(SessionEvent::Title(title));
    })?;

    let p = pending.clone();
    term.on_pwd_changed(move |t| {
        let pwd = t.pwd().unwrap_or("").to_owned();
        p.borrow_mut().events.push(SessionEvent::Pwd(pwd));
    })?;

    let p = pending.clone();
    term.on_clipboard_write(move |_t, w| {
        // Prefer text/plain; take the first representation otherwise.
        let mut chosen: Option<String> = None;
        for c in w.contents() {
            if chosen.is_none() || c.mime.starts_with("text/plain") {
                chosen = Some(c.data.to_owned());
            }
        }
        p.borrow_mut().events.push(SessionEvent::ClipboardWrite(chosen.unwrap_or_default()));
        Ok(())
    })?;

    let s = size.clone();
    term.on_size(move |_t| {
        let gs = s.get();
        Some(SizeReportSize {
            rows: gs.rows,
            columns: gs.cols,
            cell_width: gs.cell_width as u32,
            cell_height: gs.cell_height as u32,
        })
    })?;

    term.on_device_attributes(|_t| {
        Some(DeviceAttributes {
            primary: PrimaryDeviceAttributes::new(
                ConformanceLevel::VT220,
                &[
                    DeviceAttributeFeature::COLUMNS_132,
                    DeviceAttributeFeature::SELECTIVE_ERASE,
                    DeviceAttributeFeature::ANSI_COLOR,
                    DeviceAttributeFeature::CLIPBOARD,
                ],
            ),
            secondary: SecondaryDeviceAttributes {
                device_type: DeviceType::VT220,
                firmware_version: 1,
                rom_cartridge: 0,
            },
            tertiary: Default::default(),
        })
    })?;

    term.on_xtversion(move |_t| Some(xtversion))?;

    let sc = scheme.clone();
    term.on_color_scheme(move |_t| to_scheme(sc.get()))?;
    Ok(())
}

fn flush_pending(
    pending: &Rc<RefCell<Pending>>,
    writer: &mut Box<dyn Write + Send>,
    events: &Sender<SessionEvent>,
    trace: bool,
) {
    let (replies, evs) = {
        let mut p = pending.borrow_mut();
        (std::mem::take(&mut p.replies), std::mem::take(&mut p.events))
    };
    if !replies.is_empty() {
        write_all(writer, &replies, events);
        if trace {
            let _ = events.send(SessionEvent::Reply(replies));
        }
    }
    for e in evs {
        let _ = events.send(e);
    }
}

fn write_all(writer: &mut Box<dyn Write + Send>, bytes: &[u8], events: &Sender<SessionEvent>) {
    if bytes.is_empty() {
        return;
    }
    if let Err(e) = writer.write_all(bytes).and_then(|_| writer.flush()) {
        let _ = events.send(SessionEvent::Error(format!("pty write: {e}")));
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

fn encode_key(
    st: &mut State<'static>,
    term: &Terminal<'static, 'static>,
    k: &KeyInput,
) -> Result<(), libghostty_vt::Error> {
    st.key_ev
        .set_action(k.action)
        .set_key(k.key)
        .set_mods(k.mods)
        .set_consumed_mods(k.consumed_mods)
        .set_composing(k.composing)
        .set_utf8(k.text.clone());
    if let Some(c) = k.unshifted {
        st.key_ev.set_unshifted_codepoint(c);
    } else {
        st.key_ev.set_unshifted_codepoint('\0');
    }
    st.key_enc.set_options_from_terminal(term).encode_to_vec(&st.key_ev, &mut st.out_buf)?;
    // Keys the encoder does not map but that carry text (e.g. a character
    // from a key code we do not know) still have to reach the application.
    if st.out_buf.is_empty() && !k.composing && k.action != KeyAction::Release {
        if let Some(t) = &k.text {
            st.out_buf.extend_from_slice(t.as_bytes());
        }
    }
    Ok(())
}

fn configure_mouse(st: &mut State<'static>, term: &Terminal<'static, 'static>, m: &MouseInput, gs: GridSize) {
    let w = gs.cols as u32 * gs.cell_width as u32;
    let h = gs.rows as u32 * gs.cell_height as u32;
    st.mouse_enc
        .set_options_from_terminal(term)
        .set_size(mouse::EncoderSize {
            screen_width: w,
            screen_height: h,
            cell_width: gs.cell_width as u32,
            cell_height: gs.cell_height as u32,
            padding_top: 0,
            padding_bottom: 0,
            padding_left: 0,
            padding_right: 0,
        })
        .set_any_button_pressed(m.any_pressed)
        .set_track_last_cell(true);
    st.mouse_ev
        .set_action(m.action)
        .set_button(m.button)
        .set_mods(m.mods)
        .set_position(mouse::Position { x: m.x, y: m.y });
}

fn encode_mouse(
    st: &mut State<'static>,
    term: &Terminal<'static, 'static>,
    m: &MouseInput,
    gs: GridSize,
) -> Result<(), libghostty_vt::Error> {
    st.out_buf.clear();
    encode_mouse_append(st, term, m, gs)
}

fn encode_mouse_append(
    st: &mut State<'static>,
    term: &Terminal<'static, 'static>,
    m: &MouseInput,
    gs: GridSize,
) -> Result<(), libghostty_vt::Error> {
    configure_mouse(st, term, m, gs);
    st.mouse_enc.encode_to_vec(&st.mouse_ev, &mut st.out_buf)
}

fn encode_paste(text: &str, bracketed: bool) -> Vec<u8> {
    let mut data = text.as_bytes().to_vec();
    let mut buf = vec![0u8; data.len() + 16];
    loop {
        match libghostty_vt::paste::encode(&mut data, bracketed, &mut buf) {
            Ok(n) => {
                buf.truncate(n);
                return buf;
            }
            Err(libghostty_vt::Error::OutOfSpace { required }) => {
                buf.resize(required.max(buf.len() * 2), 0);
            }
            Err(_) => return text.as_bytes().to_vec(),
        }
    }
}

fn build_snapshot(st: &mut State<'static>, term: &Terminal<'static, 'static>) -> Snapshot {
    st.seq += 1;
    let mut snap = Snapshot { seq: st.seq, ..Default::default() };
    snap.title = term.title().unwrap_or("").to_owned();
    snap.pwd = term.pwd().unwrap_or("").to_owned();
    snap.modes = Modes {
        alt_screen: term.mode(Mode::ALT_SCREEN_SAVE).unwrap_or(false) || term.mode(Mode::ALT_SCREEN).unwrap_or(false),
        mouse_tracking: term.is_mouse_tracking().unwrap_or(false),
        sgr_mouse: term.mode(Mode::SGR_MOUSE).unwrap_or(false),
        bracketed_paste: term.mode(Mode::BRACKETED_PASTE).unwrap_or(false),
        focus_events: term.mode(Mode::FOCUS_EVENT).unwrap_or(false),
        sync_output: term.mode(Mode::SYNC_OUTPUT).unwrap_or(false),
        app_cursor_keys: term.mode(Mode::DECCKM).unwrap_or(false),
        cursor_visible: term.mode(Mode::CURSOR_VISIBLE).unwrap_or(true),
        kitty_keyboard: term.kitty_keyboard_flags().map(|f| f.bits()).unwrap_or(0),
    };
    if let Ok(sb) = term.scrollbar() {
        snap.scrollbar = Scrollbar { total: sb.total, offset: sb.offset, len: sb.len };
    }

    let Ok(rs) = st.render.update(term) else { return snap };
    let cols = rs.cols().unwrap_or(0);
    let rows = rs.rows().unwrap_or(0);
    snap.cols = cols;
    snap.rows = rows;
    if let Ok(c) = rs.colors() {
        snap.default_fg = c.foreground.into();
        snap.default_bg = c.background.into();
        snap.cursor_color = c.cursor.map(Into::into);
    }
    if rs.cursor_visible().unwrap_or(false) {
        if let Ok(Some(cv)) = rs.cursor_viewport() {
            let shape = match rs.cursor_visual_style() {
                Ok(CursorVisualStyle::Bar) => CursorShape::Bar,
                Ok(CursorVisualStyle::Underline) => CursorShape::Underline,
                Ok(CursorVisualStyle::BlockHollow) => CursorShape::BlockHollow,
                _ => CursorShape::Block,
            };
            snap.cursor = Some(Cursor { x: cv.x, y: cv.y, shape, blinking: rs.cursor_blinking().unwrap_or(false) });
        }
    }

    snap.cells.reserve(cols as usize * rows as usize);
    let mut text = String::with_capacity(8);
    if let Ok(mut row_it) = st.rows.update(&rs) {
        while let Some(row) = row_it.next() {
            let start = snap.cells.len();
            if let Ok(mut cell_it) = st.cells.update(row) {
                while let Some(cell) = cell_it.next() {
                    let mut out = Cell::default();
                    if cell.graphemes_len().unwrap_or(0) > 0 {
                        text.clear();
                        if cell.graphemes_utf8(&mut text).is_ok() {
                            out.text = text.clone();
                        }
                    }
                    out.fg = cell.fg_color().ok().flatten().map(Into::into);
                    out.bg = cell.bg_color().ok().flatten().map(Into::into);
                    if cell.has_styling().unwrap_or(false) {
                        if let Ok(s) = cell.style() {
                            let mut f = 0u16;
                            if s.bold {
                                f |= CellFlags::BOLD
                            }
                            if s.italic {
                                f |= CellFlags::ITALIC
                            }
                            if s.faint {
                                f |= CellFlags::FAINT
                            }
                            if s.blink {
                                f |= CellFlags::BLINK
                            }
                            if s.inverse {
                                f |= CellFlags::INVERSE
                            }
                            if s.invisible {
                                f |= CellFlags::INVISIBLE
                            }
                            if s.strikethrough {
                                f |= CellFlags::STRIKETHROUGH
                            }
                            if s.overline {
                                f |= CellFlags::OVERLINE
                            }
                            if s.underline != libghostty_vt::style::Underline::None {
                                f |= CellFlags::UNDERLINE
                            }
                            out.flags = CellFlags(f);
                        }
                    }
                    if cell.is_selected().unwrap_or(false) {
                        out.flags.0 |= CellFlags::SELECTED;
                    }
                    out.width = match cell.raw_cell().and_then(|c| c.wide()) {
                        Ok(CellWide::Wide) => Width::Wide,
                        Ok(CellWide::SpacerTail) | Ok(CellWide::SpacerHead) => Width::Spacer,
                        _ => Width::Narrow,
                    };
                    snap.cells.push(out);
                }
            }
            // Keep the grid rectangular even if a row came back short.
            let got = snap.cells.len() - start;
            for _ in got..cols as usize {
                snap.cells.push(Cell::default());
            }
            snap.cells.truncate(start + cols as usize);
        }
    }
    snap.cells.resize(cols as usize * rows as usize, Cell::default());
    snap
}

fn dump(term: &Terminal<'static, 'static>, fmt: DumpFormat) -> String {
    let format = match fmt {
        DumpFormat::Plain => FmtFormat::Plain,
        DumpFormat::Vt => FmtFormat::Vt,
        DumpFormat::Html => FmtFormat::Html,
    };
    let opts = FormatterOptions::new().with_format(format).with_trim(true);
    match Formatter::new(term, opts).and_then(|mut f| f.format_alloc(None).map(|b| b.to_vec())) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(e) => format!("<dump failed: {e}>"),
    }
}
