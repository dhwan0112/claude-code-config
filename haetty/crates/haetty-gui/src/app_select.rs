//! The window's side of text selection (stage PX, DESIGN.md §2.1 and §3.7): the mouse, the keys that clear it, copy
//! and the primary selection, following the rows while output pushes them, and marking the selected cells in the
//! composed frame. A child of `app.rs`; the ranges and their text are `crate::selection`.

use std::time::{Duration, Instant};

use haetty_core::{CellFlags, ScrollTo, Snapshot, TextRow};
use haetty_proto::paths::Endpoint;
use haetty_proto::{Client, Role};
use winit::event_loop::EventLoopProxy;

use crate::config::CopyOnSelect;
use crate::selection::{self, Point, Rows, Selection, Unit};

use super::{UserEvent, Win};

/// Clicks this close in time on one cell count as a double or triple click.
const MULTI_CLICK: Duration = Duration::from_millis(400);
/// While the pointer is above or below the terminal during a drag, one row scrolls by every this long.
const AUTOSCROLL: Duration = Duration::from_millis(50);
/// A copy stops at this many bytes of text and says so.
pub(super) const COPY_LIMIT: usize = 16 << 20;
/// Rows asked of the daemon at a time for a selection beyond the screen.
const ROWS_AT_ONCE: u32 = 2000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Clipboard,
    Primary,
    Both,
}

/// What the selection is over: the active tab's rows (numbered, scrollback included), or the cells of a read-only
/// overlay (the diff, an agent's record) as drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Over {
    Term { session: String, cols: u16, alt: bool },
    Overlay,
}

#[derive(Clone, Debug)]
pub(super) struct Sel {
    pub over: Over,
    pub sel: Selection,
    /// The left button is still down for it.
    pub dragging: bool,
    /// Row text when last seen on screen, to notice a program rewriting it.
    seen: Vec<(u64, String)>,
    /// The first and last selected cells, worked out when the selection changes (never while drawing).
    bounds: (Point, Point),
}

/// Rows of the screen, then of the daemon, for word and line extension.
struct ScreenRows<'a> {
    snap: &'a Snapshot,
    top: u64,
    client: &'a Client,
    session: &'a str,
    cache: Vec<(u64, Option<TextRow>)>,
}

impl Rows for ScreenRows<'_> {
    fn row(&mut self, n: u64) -> Option<TextRow> {
        if n >= self.top && n < self.top + self.snap.rows as u64 {
            return Some(self.snap.text_row((n - self.top) as u16));
        }
        if let Some((_, r)) = self.cache.iter().find(|(k, _)| *k == n) {
            return r.clone();
        }
        let r =
            self.client.rows(self.session, n, 1).ok().filter(|r| r.first == n).and_then(|r| r.rows.into_iter().next());
        self.cache.push((n, r.clone()));
        r
    }
}

/// The composed frame's cells, for an overlay selection.
struct DrawnRows<'a>(&'a Snapshot);

impl Rows for DrawnRows<'_> {
    fn row(&mut self, n: u64) -> Option<TextRow> {
        u16::try_from(n).ok().filter(|&y| y < self.0.rows).map(|y| TextRow { wrapped: false, ..self.0.text_row(y) })
    }
}

fn top_of(s: &Snapshot) -> u64 {
    s.scrollbar.dropped + s.scrollbar.offset
}

impl Win {
    /// The terminal's rows on the window grid, and its columns.
    pub(super) fn term_area(&self) -> (u16, u16, u16) {
        let rows = self.tab().snap.as_ref().map_or(0, |s| s.rows);
        let cols = self.grid.cols.saturating_sub(self.panel_cols());
        (self.top(), rows, cols)
    }

    /// The pointer's window cell, and whether it is over the terminal area.
    pub(super) fn pointer_cell(&self) -> ((u16, u16), bool) {
        let (col, row) = self.layout.cell_at(self.origin(), self.mouse.x, self.mouse.y, self.grid);
        let (top, rows, cols) = self.term_area();
        let (_, y) = self.layout.grid_px(self.origin(), self.mouse.x, self.mouse.y);
        let inside = y >= 0.0 && row >= top && row < top + rows && col < cols;
        ((col, row), inside)
    }

    pub(super) fn overlay_open(&self) -> bool {
        self.diff.is_some() || self.agent_view.is_some()
    }

    fn word_chars(&self) -> String {
        self.cfg.selection_word_chars.clone()
    }

    /// A press that starts, extends or ends a selection, or pastes the primary selection; true when it was taken
    /// (it does not reach the program).
    pub(super) fn select_press(&mut self, b: haetty_core::MouseButton) -> bool {
        use haetty_core::MouseButton as B;
        let ((col, row), inside) = self.pointer_cell();
        if !inside || self.palette.is_some() || self.settings_panel.is_some() {
            return false;
        }
        let bypass = self.mods.shift_key();
        let overlay = self.overlay_open();
        if !overlay && self.mouse_tracking() && !bypass {
            if b == B::Left {
                self.clear_selection();
            }
            return false;
        }
        match b {
            B::Middle if !overlay => {
                self.paste_primary();
                true
            }
            B::Left => {
                let count = self.click_count(col, row);
                let unit = match count {
                    2 => Unit::Word,
                    3 => Unit::Line,
                    _ => Unit::Cell,
                };
                let rect = self.mods.alt_key();
                let extend = bypass && count == 1 && self.sel.as_ref().is_some_and(|s| s.over == self.over_now());
                if overlay {
                    self.start_overlay(col, row, unit, rect, extend);
                } else {
                    self.start_term(col, row, unit, rect, extend);
                }
                true
            }
            _ => false,
        }
    }

    fn over_now(&self) -> Over {
        if self.overlay_open() {
            return Over::Overlay;
        }
        let s = self.tab().snap.as_ref();
        Over::Term {
            session: self.tab().session.id.clone(),
            cols: s.map_or(0, |s| s.cols),
            alt: s.is_some_and(|s| s.modes.alt_screen),
        }
    }

    fn click_count(&mut self, col: u16, row: u16) -> u8 {
        let now = Instant::now();
        let n = match self.last_click {
            Some((t, at, n)) if at == (col, row) && now.duration_since(t) <= MULTI_CLICK => n % 3 + 1,
            _ => 1,
        };
        self.last_click = Some((now, (col, row), n));
        n
    }

    fn term_point(&self, col: u16, row: u16) -> Option<Point> {
        let s = self.tab().snap.as_ref()?;
        let y = row.checked_sub(self.top())?.min(s.rows.saturating_sub(1));
        Some(Point { row: top_of(s) + y as u64, col: col.min(s.cols.saturating_sub(1)) })
    }

    fn start_term(&mut self, col: u16, row: u16, unit: Unit, rect: bool, extend: bool) {
        let Some(at) = self.term_point(col, row) else { return };
        if extend {
            if let Some(s) = self.sel.as_mut() {
                s.sel.extend(at);
                s.dragging = true;
                self.sel_changed();
                return;
            }
        }
        let wc = self.word_chars();
        let Some(snap) = self.tab().snap.as_ref() else { return };
        let mut src = ScreenRows {
            snap,
            top: top_of(snap),
            client: &self.client,
            session: &self.tab().session.id,
            cache: vec![],
        };
        let sel = Selection::new(at, unit, rect, &mut src, &wc);
        self.sel = Some(Sel { over: self.over_now(), bounds: (at, at), sel, dragging: true, seen: vec![] });
        self.sel_changed();
    }

    fn start_overlay(&mut self, col: u16, row: u16, unit: Unit, rect: bool, extend: bool) {
        let at = Point { row: row as u64, col };
        if extend {
            if let Some(s) = self.sel.as_mut() {
                s.sel.extend(at);
                s.dragging = true;
                self.sel_changed();
                return;
            }
        }
        let wc = self.word_chars();
        let Some(drawn) = self.composed.as_ref() else { return };
        let sel = Selection::new(at, unit, rect, &mut DrawnRows(drawn), &wc);
        self.sel = Some(Sel { over: Over::Overlay, bounds: (at, at), sel, dragging: true, seen: vec![] });
        self.sel_changed();
    }

    /// The pointer moved; true when a selection's drag took it (it does not reach the program).
    pub(super) fn select_motion(&mut self) -> bool {
        if !self.sel.as_ref().is_some_and(|s| s.dragging) {
            return false;
        }
        let (col, row) = self.layout.cell_at(self.origin(), self.mouse.x, self.mouse.y, self.grid);
        let (top, rows, cols) = self.term_area();
        let (_, py) = self.layout.grid_px(self.origin(), self.mouse.x, self.mouse.y);
        let term = matches!(self.sel.as_ref().map(|s| &s.over), Some(Over::Term { .. }));
        let col = col.min(cols.saturating_sub(1));
        if term {
            let above = py < (top as u32 * self.layout.cell.height) as f32;
            let below = row >= top + rows;
            self.sel_scroll = (above || below).then(|| self.sel_scroll.unwrap_or_else(Instant::now));
            let row = row.clamp(top, (top + rows).saturating_sub(1));
            if let Some(at) = self.term_point(col, row) {
                if let Some(s) = self.sel.as_mut() {
                    s.sel.extend(at);
                }
            }
        } else {
            let row = row.clamp(top, (top + rows).saturating_sub(1));
            if let Some(s) = self.sel.as_mut() {
                s.sel.extend(Point { row: row as u64, col });
            }
        }
        self.sel_changed();
        true
    }

    /// The left button came up; true when a selection had it.
    pub(super) fn select_release(&mut self) -> bool {
        let Some(s) = self.sel.as_mut().filter(|s| s.dragging) else { return false };
        s.dragging = false;
        self.sel_scroll = None;
        if s.sel.is_empty() {
            self.clear_selection();
            return true;
        }
        match self.cfg.copy_on_select {
            CopyOnSelect::Off => {}
            CopyOnSelect::Primary => self.copy_selection(Target::Primary),
            CopyOnSelect::Both => self.copy_selection(Target::Both),
        }
        true
    }

    /// During a drag outside the terminal, one more row scrolls; the next time to come back.
    pub(super) fn select_tick(&mut self, now: Instant) -> Option<Instant> {
        let since = self.sel_scroll?;
        if now.duration_since(since) < AUTOSCROLL {
            return Some(since + AUTOSCROLL);
        }
        let (_, py) = self.layout.grid_px(self.origin(), self.mouse.x, self.mouse.y);
        let up = py < (self.top() as u32 * self.layout.cell.height) as f32;
        self.handle().scroll(ScrollTo::Delta(if up { -1 } else { 1 }));
        self.sel_scroll = Some(now);
        Some(now + AUTOSCROLL)
    }

    pub(super) fn clear_selection(&mut self) {
        if self.sel.take().is_some() {
            self.sel_scroll = None;
            self.recompose();
        }
    }

    fn sel_changed(&mut self) {
        if let Some(s) = self.sel.as_mut() {
            s.seen.clear();
        }
        self.rebound();
        self.remember_rows();
        self.recompose();
    }

    /// Works out the selection's first and last cells (a word or line may need rows from the daemon).
    fn rebound(&mut self) {
        let wc = self.word_chars();
        let Some(s) = self.sel.as_ref() else { return };
        let b = match &s.over {
            Over::Overlay => match self.composed.as_ref() {
                Some(frame) => s.sel.bounds(&mut DrawnRows(frame), &wc),
                None => return,
            },
            Over::Term { session, .. } => {
                let Some(snap) = self.tab().snap.as_ref() else { return };
                let mut src = ScreenRows { snap, top: top_of(snap), client: &self.client, session, cache: vec![] };
                s.sel.bounds(&mut src, &wc)
            }
        };
        if let Some(s) = self.sel.as_mut() {
            s.bounds = b;
        }
    }

    /// The active tab's screen changed: the selection follows its rows, shrinks by rows the scrollback dropped,
    /// and goes when the program rewrote a selected row, the size changed or the screen switched.
    pub(super) fn select_follow(&mut self) {
        if self.sel.as_ref().is_some_and(|s| s.over == Over::Overlay) && !self.overlay_open() {
            self.sel = None;
        }
        let Some(s) = self.sel.as_ref() else { return };
        let Over::Term { session, cols, alt } = &s.over else { return };
        let Some(snap) = self.tab().snap.as_ref() else { return };
        if *session != self.tab().session.id || *cols != snap.cols || *alt != snap.modes.alt_screen {
            self.sel = None;
            return;
        }
        let top = top_of(snap);
        let dropped = snap.scrollbar.dropped;
        let changed = s
            .seen
            .iter()
            .any(|(n, text)| *n >= top && *n < top + snap.rows as u64 && snap.row_text((*n - top) as u16) != *text);
        let dragging = s.dragging;
        let Some(s) = self.sel.as_mut() else { return };
        if changed || !s.sel.drop_before(dropped) {
            self.sel = None;
            self.sel_scroll = None;
            return;
        }
        if dragging {
            let (col, row) = self.layout.cell_at(self.origin(), self.mouse.x, self.mouse.y, self.grid);
            let (t, rows, cols) = self.term_area();
            let row = row.clamp(t, (t + rows).saturating_sub(1));
            if let Some(at) = self.term_point(col.min(cols.saturating_sub(1)), row) {
                if let Some(s) = self.sel.as_mut() {
                    s.sel.extend(at);
                }
            }
        }
        self.rebound();
        self.remember_rows();
    }

    fn remember_rows(&mut self) {
        let Some(snap) = self.tabs.get(self.active).and_then(|t| t.snap.as_ref()) else { return };
        let top = top_of(snap);
        let Some(s) = self.sel.as_mut() else { return };
        if s.over == Over::Overlay {
            return;
        }
        let (lo, hi) = s.sel.rows();
        s.seen = (lo.max(top)..=hi.min(top + snap.rows as u64).saturating_sub(0))
            .filter(|n| *n < top + snap.rows as u64)
            .map(|n| (n, snap.row_text((n - top) as u16)))
            .collect();
    }

    /// Marks the selected cells of the composed frame (DESIGN.md §2.1: the theme's selection colours).
    pub(super) fn mark_selection(&self, frame: &mut Snapshot) {
        let Some(s) = self.sel.as_ref() else { return };
        let b = s.bounds;
        match &s.over {
            Over::Term { .. } => {
                let Some(snap) = self.tab().snap.as_ref() else { return };
                let top = top_of(snap);
                let (t, rows, cols) = self.term_area();
                for y in 0..rows {
                    let n = top + y as u64;
                    if n < b.0.row || n > b.1.row {
                        continue;
                    }
                    for x in 0..cols.min(snap.cols) {
                        if selection::contains(b, s.sel.rect, Point { row: n, col: x }) {
                            mark(frame, x, t + y, false);
                        }
                    }
                }
            }
            Over::Overlay if !self.overlay_open() => {}
            Over::Overlay => {
                for y in b.0.row..=b.1.row.min(frame.rows.saturating_sub(1) as u64) {
                    for x in 0..frame.cols {
                        if selection::contains(b, s.sel.rect, Point { row: y, col: x }) {
                            mark(frame, x, y as u16, true);
                        }
                    }
                }
            }
        }
    }

    /// The selection's text to the clipboard, the primary selection or both. Rows on screen are read at once; a
    /// selection that reaches beyond the screen is read from the daemon on a thread of its own, so the window never
    /// waits for it.
    pub(super) fn copy_selection(&mut self, to: Target) {
        let Some(s) = self.sel.as_ref() else { return };
        let b = s.bounds;
        match &s.over {
            Over::Overlay => {
                let Some(frame) = self.composed.as_ref() else { return };
                let rows: Vec<TextRow> =
                    (0..frame.rows).map(|y| TextRow { wrapped: false, ..frame.text_row(y) }).collect();
                let text = selection::text(b, s.sel.rect, 0, &rows);
                self.put(to, text, false);
            }
            Over::Term { session, .. } => {
                let Some(snap) = self.tab().snap.as_ref() else { return };
                let top = top_of(snap);
                let rect = s.sel.rect;
                if b.0.row >= top && b.1.row < top + snap.rows as u64 {
                    let rows: Vec<TextRow> = (0..snap.rows).map(|y| snap.text_row(y)).collect();
                    let text =
                        self.inline_copy(b, rect, top, &rows).unwrap_or_else(|| selection::text(b, rect, top, &rows));
                    let cut = text.len() > COPY_LIMIT;
                    self.put(to, cap(text), cut);
                    return;
                }
                let (session, proxy) = (session.clone(), self.proxy.clone());
                std::thread::Builder::new()
                    .name("copy".into())
                    .spawn(move || copy_in_background(session, b, rect, to, proxy))
                    .map_err(|e| log::warn!("copy thread: {e}"))
                    .ok();
            }
        }
    }

    /// A finished copy: into the clipboard and/or the primary selection; a cut one says so.
    pub(super) fn put(&mut self, to: Target, text: String, cut: bool) {
        log::debug!("copied {} bytes to {to:?}", text.len());
        match to {
            Target::Clipboard => self.clipboard.set(text),
            Target::Primary => self.clipboard.set_primary(text),
            Target::Both => {
                self.clipboard.set_primary(text.clone());
                self.clipboard.set(text);
            }
        }
        if cut {
            log::info!("copy cut at {COPY_LIMIT} bytes");
            self.show_hint(format!("copied the first {} MiB of the selection", COPY_LIMIT >> 20));
        }
    }

    fn paste_primary(&mut self) {
        let Some(text) = self.clipboard.get_primary() else { return };
        if self.tab().exited.is_some() || text.is_empty() {
            return;
        }
        self.handle().paste(haetty_proto::wire::Origin::Human, text);
    }
}

fn mark(frame: &mut Snapshot, x: u16, y: u16, overlay: bool) {
    let cols = frame.cols as usize;
    if let Some(c) = frame.cells.get_mut(y as usize * cols + x as usize) {
        if overlay || !c.flags.has(CellFlags::HAETTY_UI) {
            c.flags.0 |= CellFlags::SELECTED;
        }
    }
}

fn cap(mut text: String) -> String {
    if text.len() > COPY_LIMIT {
        let mut end = COPY_LIMIT;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    text
}

/// A selection beyond the screen: its rows from the daemon over a connection of this thread's own, then the text
/// back to the window.
fn copy_in_background(session: String, b: (Point, Point), rect: bool, to: Target, proxy: EventLoopProxy<UserEvent>) {
    let result = (|| -> Result<(String, bool), String> {
        let ep = Endpoint::default_for_user().map_err(|e| e.to_string())?;
        let client = Client::connect(&ep, Role::Control, "copy").map_err(|e| e.to_string())?;
        let mut rows: Vec<TextRow> = vec![];
        let mut first = None;
        let mut next = b.0.row;
        let mut bytes = 0usize;
        while next <= b.1.row && bytes <= COPY_LIMIT {
            let want = ((b.1.row - next + 1).min(ROWS_AT_ONCE as u64)) as u32;
            let got = client.rows(&session, next, want).map_err(|e| e.to_string())?;
            if got.rows.is_empty() {
                break;
            }
            first.get_or_insert(got.first);
            next = got.first + got.rows.len() as u64;
            bytes += got.rows.iter().map(|r| r.cells.iter().map(|(t, _)| t.len().max(1)).sum::<usize>()).sum::<usize>();
            rows.extend(got.rows);
        }
        let Some(first) = first else { return Ok((String::new(), false)) };
        let start = Point { row: b.0.row.max(first), col: if b.0.row < first { 0 } else { b.0.col } };
        let text = selection::text((start, b.1), rect, first, &rows);
        let cut = text.len() > COPY_LIMIT || next <= b.1.row;
        Ok((cap(text), cut))
    })();
    let ev = match result {
        Ok((text, cut)) => UserEvent::Copied { to, text: haetty_proto::wire::Secret(text), cut },
        Err(e) => {
            log::warn!("copy: {e}");
            return;
        }
    };
    let _ = proxy.send_event(ev);
}
