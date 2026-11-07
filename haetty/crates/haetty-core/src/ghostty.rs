//! [`VtEngine`] on libghostty-vt (the default engine). The code is what
//! `engine.rs` did directly before stage E1; it moved here unchanged.

use std::cell::RefCell;
use std::rc::Rc;

use libghostty_vt::fmt::{Format as FmtFormat, Formatter, FormatterOptions};
use libghostty_vt::key;
use libghostty_vt::mouse;
use libghostty_vt::render::{CellIterator, CursorVisualStyle, RowIterator};
use libghostty_vt::screen::CellWide;
use libghostty_vt::style::StyleColor;
use libghostty_vt::terminal::{
    ColorScheme, ConformanceLevel, DeviceAttributeFeature, DeviceAttributes, DeviceType, Mode, Point, PointCoordinate,
    PrimaryDeviceAttributes, ScrollViewport, SecondaryDeviceAttributes, SizeReportSize,
};
use libghostty_vt::{RenderState, Terminal, TerminalOptions};

use crate::types::*;
use crate::vt::{VtEngine, VtInit, VtMode};
use crate::{ColorSchemePref, DumpFormat, SessionEvent, TermColors};

/// Things libghostty callbacks produce while `vt_write` runs. They are queued
/// and handed out after the call returns, never from inside a callback.
#[derive(Default)]
struct Pending {
    replies: Vec<u8>,
    events: Vec<SessionEvent>,
}

pub struct GhosttyEngine {
    term: Terminal<'static, 'static>,
    pending: Rc<RefCell<Pending>>,
    size: Rc<std::cell::Cell<GridSize>>,
    scheme: Rc<std::cell::Cell<ColorSchemePref>>,
    render: RenderState<'static>,
    rows: RowIterator<'static>,
    cells: CellIterator<'static>,
    key_enc: key::Encoder<'static>,
    key_ev: key::Event<'static>,
    mouse_enc: mouse::Encoder<'static>,
    mouse_ev: mouse::Event<'static>,
    /// How snapshots read the render state (DESIGN.md §10.8).
    refresh: Refresh,
    /// A character that may have joined a cell already drawn was fed since
    /// the last snapshot (`Refresh::Joined`).
    joined: bool,
    /// UTF-8 decoding across `feed` calls for that check: the code point so
    /// far and the continuation bytes still expected.
    utf8: (u32, u8),
    pub stats: SnapStats,
}

/// How a snapshot reads libghostty's render state. Its incremental update
/// copies only rows the terminal marked dirty, and libghostty does not mark
/// a row when a combining mark, variation selector or ZWJ joins a cell
/// already there (outside mode 2027), so such a row would stay stale until
/// something else changes it (DESIGN.md §10.7 finding 3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refresh {
    /// Incremental only (stale rows possible; for measurements).
    Incremental,
    /// A new render state for every snapshot: every row copied each time.
    /// The default (DESIGN.md §10.8).
    Always,
    /// A new render state only after a character that can join a drawn
    /// cell: zero width, or above U+00FF under mode 2027. Cheaper, but
    /// misses libghostty's other unmarked rows (§10.8); kept for
    /// `vtdiff --refresh`.
    Joined,
}

/// Counters for choosing a `Refresh` (DESIGN.md §10.8).
#[derive(Clone, Copy, Debug, Default)]
pub struct SnapStats {
    pub snapshots: u64,
    /// Snapshots taken from a new render state.
    pub fresh: u64,
    /// Rows the render state copied: all of them for a fresh one, the dirty
    /// ones otherwise.
    pub rows_copied: u64,
}

impl GhosttyEngine {
    /// A snapshot from a new render state, so every row is read again.
    /// The incremental one misses a row whose only change is a combining
    /// mark, variation selector or ZWJ joining a cell already drawn:
    /// libghostty does not mark that row dirty (DESIGN.md §10.7).
    /// `haetty-vtdiff` compares with this one; sessions use `snapshot`.
    pub fn snapshot_fresh(&mut self) -> Snapshot {
        if let Ok(rs) = RenderState::new() {
            self.render = rs;
        }
        build_snapshot(self, true)
    }

    pub fn set_refresh(&mut self, r: Refresh) {
        self.refresh = r;
    }

    /// Notes whether `bytes` hold a character that can join a drawn cell.
    fn scan_joiners(&mut self, bytes: &[u8]) {
        let cluster = self.term.mode(Mode::GRAPHEME_CLUSTER).unwrap_or(false);
        let (mut cp, mut need) = self.utf8;
        for &b in bytes {
            if need > 0 && b & 0xc0 == 0x80 {
                cp = (cp << 6) | (b & 0x3f) as u32;
                need -= 1;
                if need > 0 {
                    continue;
                }
            } else {
                need = 0;
                match b {
                    0xc0..=0xdf => (cp, need) = ((b & 0x1f) as u32, 1),
                    0xe0..=0xef => (cp, need) = ((b & 0x0f) as u32, 2),
                    0xf0..=0xf7 => (cp, need) = ((b & 0x07) as u32, 3),
                    _ => cp = 0,
                }
                if need > 0 {
                    continue;
                }
            }
            if cp > 0xff {
                if let Some(c) = char::from_u32(cp) {
                    if cluster || haetty_vt::width(c) == 0 {
                        self.joined = true;
                    }
                }
            }
        }
        self.utf8 = (cp, need);
    }

    pub fn new(init: &VtInit) -> Result<GhosttyEngine, String> {
        let gs = init.size.clamped();
        // Leaked once per session: the XTVERSION callback must return a &str
        // that outlives the call, and the string never changes.
        let xtversion: &'static str = Box::leak(init.xtversion.clone().into_boxed_str());
        let mut term = Terminal::new(TerminalOptions {
            cols: gs.cols.max(1),
            rows: gs.rows.max(1),
            max_scrollback: init.scrollback_limit,
        })
        .map_err(|e| format!("terminal init: {e}"))?;
        let pending = Rc::new(RefCell::new(Pending::default()));
        let size = Rc::new(std::cell::Cell::new(gs));
        let scheme = Rc::new(std::cell::Cell::new(init.color_scheme));
        install_callbacks(&mut term, &pending, &size, &scheme, xtversion)
            .map_err(|e| format!("terminal callbacks: {e}"))?;
        set_colors(&mut term, &init.colors);
        let _ = term.resize(gs.cols.max(1), gs.rows.max(1), gs.cell_width as u32, gs.cell_height as u32);
        Ok(GhosttyEngine {
            term,
            pending,
            size,
            scheme,
            render: RenderState::new().map_err(|e| format!("render state: {e}"))?,
            rows: RowIterator::new().map_err(|e| format!("row iterator: {e}"))?,
            cells: CellIterator::new().map_err(|e| format!("cell iterator: {e}"))?,
            key_enc: key::Encoder::new().map_err(|e| format!("key encoder: {e}"))?,
            key_ev: key::Event::new().map_err(|e| format!("key event: {e}"))?,
            mouse_enc: mouse::Encoder::new().map_err(|e| format!("mouse encoder: {e}"))?,
            mouse_ev: mouse::Event::new().map_err(|e| format!("mouse event: {e}"))?,
            refresh: Refresh::Always,
            joined: false,
            utf8: (0, 0),
            stats: SnapStats::default(),
        })
    }
}

impl VtEngine for GhosttyEngine {
    fn feed(&mut self, bytes: &[u8]) {
        if self.refresh == Refresh::Joined && !self.joined {
            self.scan_joiners(bytes);
        }
        self.term.vt_write(bytes);
    }

    fn take_replies(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.pending.borrow_mut().replies)
    }

    fn take_events(&mut self) -> Vec<SessionEvent> {
        std::mem::take(&mut self.pending.borrow_mut().events)
    }

    fn resize(&mut self, gs: GridSize) {
        let gs = gs.clamped();
        self.size.set(gs);
        let _ = self.term.resize(gs.cols, gs.rows, gs.cell_width as u32, gs.cell_height as u32);
    }

    fn snapshot(&mut self) -> Snapshot {
        let fresh = match self.refresh {
            Refresh::Incremental => false,
            Refresh::Always => true,
            Refresh::Joined => std::mem::take(&mut self.joined),
        };
        if fresh {
            if let Ok(rs) = RenderState::new() {
                self.render = rs;
            }
        }
        build_snapshot(self, fresh)
    }

    fn mode(&self, m: VtMode) -> bool {
        let t = &self.term;
        match m {
            VtMode::BracketedPaste => t.mode(Mode::BRACKETED_PASTE).unwrap_or(false),
            VtMode::FocusEvents => t.mode(Mode::FOCUS_EVENT).unwrap_or(false),
            VtMode::ColorSchemeReport => t.mode(Mode::COLOR_SCHEME_REPORT).unwrap_or(false),
            VtMode::SyncOutput => t.mode(Mode::SYNC_OUTPUT).unwrap_or(false),
            VtMode::MouseTracking => t.is_mouse_tracking().unwrap_or(false),
        }
    }

    fn scroll_viewport(&mut self, to: ScrollTo) {
        self.term.scroll_viewport(match to {
            ScrollTo::Top => ScrollViewport::Top,
            ScrollTo::Bottom => ScrollViewport::Bottom,
            ScrollTo::Delta(d) => ScrollViewport::Delta(d),
        });
    }

    fn set_colors(&mut self, c: &TermColors) {
        set_colors(&mut self.term, c);
    }

    fn set_color_scheme(&mut self, s: ColorSchemePref, out: &mut Vec<u8>) {
        self.scheme.set(s);
        // Report the change to applications that asked for it (mode 2031).
        if self.term.mode(Mode::COLOR_SCHEME_REPORT).unwrap_or(false) {
            if let Some(cs) = to_scheme(s) {
                let mut buf = [0u8; 16];
                if let Ok(n) = cs.encode_report(&mut buf) {
                    out.extend_from_slice(&buf[..n]);
                }
            }
        }
    }

    fn encode_key(&mut self, k: &KeyInput, out: &mut Vec<u8>) {
        let start = out.len();
        self.key_ev
            .set_action(lg_key_action(k.action))
            .set_key(lg_key(k.key))
            .set_mods(lg_mods(k.mods))
            .set_consumed_mods(lg_mods(k.consumed_mods))
            .set_composing(k.composing)
            .set_utf8(k.text.clone());
        self.key_ev.set_unshifted_codepoint(k.unshifted.unwrap_or('\0'));
        let _ = self.key_enc.set_options_from_terminal(&self.term).encode_to_vec(&self.key_ev, out);
        // Keys the encoder does not map but that carry text (e.g. a character
        // from a key code we do not know) still have to reach the application.
        if out.len() == start && !k.composing && k.action != KeyAction::Release {
            if let Some(t) = &k.text {
                out.extend_from_slice(t.as_bytes());
            }
        }
    }

    fn encode_mouse(&mut self, m: &MouseInput, gs: GridSize, out: &mut Vec<u8>) {
        let w = gs.cols as u32 * gs.cell_width as u32;
        let h = gs.rows as u32 * gs.cell_height as u32;
        self.mouse_enc
            .set_options_from_terminal(&self.term)
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
        self.mouse_ev
            .set_action(lg_mouse_action(m.action))
            .set_button(m.button.map(lg_mouse_button))
            .set_mods(lg_mods(m.mods))
            .set_position(mouse::Position { x: m.x, y: m.y });
        let _ = self.mouse_enc.encode_to_vec(&self.mouse_ev, out);
    }

    fn encode_paste(&mut self, text: &str, out: &mut Vec<u8>) {
        let bracketed = self.term.mode(Mode::BRACKETED_PASTE).unwrap_or(false);
        let mut data = text.as_bytes().to_vec();
        let mut buf = vec![0u8; data.len() + 16];
        loop {
            match libghostty_vt::paste::encode(&mut data, bracketed, &mut buf) {
                Ok(n) => {
                    out.extend_from_slice(&buf[..n]);
                    return;
                }
                Err(libghostty_vt::Error::OutOfSpace { required }) => buf.resize(required.max(buf.len() * 2), 0),
                Err(_) => {
                    out.extend_from_slice(text.as_bytes());
                    return;
                }
            }
        }
    }

    fn encode_focus(&mut self, focused: bool, out: &mut Vec<u8>) {
        if self.term.mode(Mode::FOCUS_EVENT).unwrap_or(false) {
            let ev = if focused { libghostty_vt::focus::Event::Gained } else { libghostty_vt::focus::Event::Lost };
            let mut buf = [0u8; 8];
            if let Ok(n) = ev.encode(&mut buf) {
                out.extend_from_slice(&buf[..n]);
            }
        }
    }

    fn dump(&mut self, fmt: DumpFormat) -> String {
        let format = match fmt {
            DumpFormat::Plain => FmtFormat::Plain,
            DumpFormat::Vt => FmtFormat::Vt,
            DumpFormat::Html => FmtFormat::Html,
        };
        let opts = FormatterOptions::new().with_format(format).with_trim(true);
        match Formatter::new(&self.term, opts).and_then(|mut f| f.format_alloc(None).map(|b| b.to_vec())) {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(e) => format!("<dump failed: {e}>"),
        }
    }

    fn scrollback_rows(&mut self) -> u64 {
        let rows = self.size.get().rows as u64;
        self.term.scrollbar().map_or(0, |sb| sb.total.saturating_sub(rows))
    }
}

/// Default fg/bg, colours 0–15 (16–255 stay libghostty-vt's: xterm's cube
/// and grey ramp; `None` = its built-in 0–15 too) and the cursor colour.
fn set_colors(term: &mut Terminal<'static, 'static>, c: &TermColors) {
    let rgb = |c: Rgb| libghostty_vt::style::RgbColor { r: c.r, g: c.g, b: c.b };
    let _ = term.set_default_fg_color(Some(rgb(c.default_fg)));
    let _ = term.set_default_bg_color(Some(rgb(c.default_bg)));
    let mut p = libghostty_vt::style::Palette::default();
    if let Some(p16) = c.palette {
        for (slot, c) in p.0.iter_mut().zip(p16) {
            *slot = rgb(c);
        }
    }
    let _ = term.set_default_color_palette(Some(p));
    let _ = term.set_default_cursor_color(c.cursor_color.map(rgb));
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

fn build_snapshot(e: &mut GhosttyEngine, fresh: bool) -> Snapshot {
    e.stats.snapshots += 1;
    e.stats.fresh += fresh as u64;
    let term = &e.term;
    let mut snap = Snapshot {
        title: term.title().unwrap_or("").to_owned(),
        pwd: term.pwd().unwrap_or("").to_owned(),
        ..Default::default()
    };
    snap.modes = Modes {
        // The screen shown, not the mode flags: 47, 1047 and 1049 can be
        // left set while the primary screen is back.
        alt_screen: term.active_screen().is_ok_and(|s| s == libghostty_vt::screen::Screen::Alternate),
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

    let Ok(rs) = e.render.update(term) else { return snap };
    let cols = rs.cols().unwrap_or(0);
    let rows = rs.rows().unwrap_or(0);
    snap.cols = cols;
    snap.rows = rows;
    let mut palette = None;
    if let Ok(c) = rs.colors() {
        snap.default_fg = c.foreground.into();
        snap.default_bg = c.background.into();
        snap.cursor_color = c.cursor.map(Into::into);
        palette = Some(c.palette);
    }
    let ul_color = |c: StyleColor| -> Option<Rgb> {
        match c {
            StyleColor::None => None,
            StyleColor::Rgb(c) => Some(c.into()),
            StyleColor::Palette(i) => palette.as_ref().map(|p| p[i.0 as usize].into()),
        }
    };
    // One string per hyperlink target in the snapshot, shared by its cells.
    let mut links: Vec<std::sync::Arc<str>> = Vec::new();
    let mut uri = vec![0u8; 256];
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
    let mut y: u32 = 0;
    if let Ok(mut row_it) = e.rows.update(&rs) {
        while let Some(row) = row_it.next() {
            let row_links = row.raw_row().and_then(|r| r.has_hyperlink()).unwrap_or(false);
            let mut x: u16 = 0;
            // Counted, then cleared: nothing else reads the row flags.
            if fresh || row.dirty().unwrap_or(true) {
                e.stats.rows_copied += 1;
            }
            let _ = row.set_dirty(false);
            let start = snap.cells.len();
            if let Ok(mut cell_it) = e.cells.update(row) {
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
                            use libghostty_vt::style::Underline as U;
                            if s.underline != U::None {
                                f |= CellFlags::UNDERLINE;
                                out.underline = match s.underline {
                                    U::Double => Underline::Double,
                                    U::Curly => Underline::Curly,
                                    U::Dotted => Underline::Dotted,
                                    U::Dashed => Underline::Dashed,
                                    _ => Underline::Single,
                                };
                            }
                            out.underline_color = ul_color(s.underline_color);
                            out.flags = CellFlags(f);
                        }
                    }
                    if row_links && cell.raw_cell().and_then(|c| c.has_hyperlink()).unwrap_or(false) {
                        out.link = cell_link(term, x, y, &mut uri, &mut links);
                    }
                    x = x.saturating_add(1);
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
            y += 1;
        }
    }
    snap.cells.resize(cols as usize * rows as usize, Cell::default());
    snap
}

/// The hyperlink target of the viewport cell (x, y), shared with the cells
/// before it that have the same one.
fn cell_link(
    term: &Terminal<'static, 'static>,
    x: u16,
    y: u32,
    buf: &mut Vec<u8>,
    links: &mut Vec<std::sync::Arc<str>>,
) -> Option<std::sync::Arc<str>> {
    let r = term.grid_ref(Point::Viewport(PointCoordinate { x, y })).ok()?;
    let n = match r.hyperlink_uri(buf) {
        Ok(n) => n,
        Err(libghostty_vt::Error::OutOfSpace { required }) => {
            buf.resize(required, 0);
            r.hyperlink_uri(buf).ok()?
        }
        Err(_) => return None,
    };
    if n == 0 {
        return None;
    }
    let s = String::from_utf8_lossy(&buf[..n]);
    if let Some(a) = links.iter().find(|a| ***a == *s) {
        return Some(a.clone());
    }
    let a: std::sync::Arc<str> = s.into();
    links.push(a.clone());
    Some(a)
}

// haetty-core's key and mouse types have libghostty-vt's numbering
// (`keys.rs`, generated from it), so these are casts.

impl From<libghostty_vt::style::RgbColor> for Rgb {
    fn from(c: libghostty_vt::style::RgbColor) -> Self {
        Self { r: c.r, g: c.g, b: c.b }
    }
}

fn lg_key(k: Key) -> key::Key {
    key::Key::try_from(k as u32).unwrap_or(key::Key::Unidentified)
}

fn lg_key_action(a: KeyAction) -> key::Action {
    key::Action::try_from(a as u32).unwrap_or(key::Action::Press)
}

fn lg_mods(m: Mods) -> key::Mods {
    key::Mods::from_bits_truncate(m.bits())
}

fn lg_mouse_action(a: MouseAction) -> mouse::Action {
    mouse::Action::try_from(a as u32).unwrap_or(mouse::Action::Motion)
}

fn lg_mouse_button(b: MouseButton) -> mouse::Button {
    mouse::Button::try_from(b as u32).unwrap_or(mouse::Button::Unknown)
}

#[cfg(test)]
mod keys_match_libghostty {
    use super::*;

    /// Every variant converts to the libghostty one with the same name.
    #[test]
    fn same_numbering() {
        for k in Key::ALL {
            assert_eq!(format!("{k:?}"), format!("{:?}", lg_key(k)));
        }
        for a in KeyAction::ALL {
            assert_eq!(format!("{a:?}"), format!("{:?}", lg_key_action(a)));
        }
        for a in MouseAction::ALL {
            assert_eq!(format!("{a:?}"), format!("{:?}", lg_mouse_action(a)));
        }
        for b in MouseButton::ALL {
            assert_eq!(format!("{b:?}"), format!("{:?}", lg_mouse_button(b)));
        }
        assert_eq!(lg_mods(Mods::all()).bits(), key::Mods::all().bits());
        assert_eq!(KittyKeyFlags::ALL.bits(), key::KittyKeyFlags::ALL.bits());
    }
}
