//! haetty's own terminal engine in pure Rust (DESIGN.md §10). It knows
//! nothing of haetty's sessions or wire types: `haetty-core` wraps it in its
//! engine trait (feature `native`), and `haetty-vtdiff` compares it with
//! libghostty-vt — the reference for every behaviour here. Where this file
//! says "as libghostty", the behaviour was read off libghostty-vt 0.2.2 with
//! `haetty-vtdiff --probe`.
//!
//! Stage E2: the screen, cursor and the usual operations, SGR and colours,
//! scroll regions, the alternate screen, wide characters, modes, replies to
//! queries. Encoders and the rest are stage E3.

mod width;
pub mod width_table;

use std::collections::VecDeque;

pub use width::width;

/// A colour as the program set it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Color {
    #[default]
    Default,
    Index(u8),
    Rgb([u8; 3]),
}

/// Cell attribute bits (the same bits as `haetty_core::CellFlags`).
pub mod flags {
    pub const BOLD: u16 = 1 << 0;
    pub const ITALIC: u16 = 1 << 1;
    pub const FAINT: u16 = 1 << 2;
    pub const UNDERLINE: u16 = 1 << 3;
    pub const STRIKETHROUGH: u16 = 1 << 4;
    pub const INVERSE: u16 = 1 << 5;
    pub const INVISIBLE: u16 = 1 << 6;
    pub const BLINK: u16 = 1 << 7;
    pub const OVERLINE: u16 = 1 << 8;
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Style {
    pub fg: Color,
    pub bg: Color,
    pub flags: u16,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Wide {
    #[default]
    Narrow,
    Wide,
    /// The second half of a wide character.
    SpacerTail,
    /// The last column of a row whose wide character went to the next row.
    SpacerHead,
}

/// One cell of the screen.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cell {
    /// The grapheme cluster; empty for a blank cell.
    pub text: String,
    pub style: Style,
    pub wide: Wide,
}

#[derive(Clone, Debug, Default)]
struct Line {
    cells: Vec<Cell>,
    /// The line continues on the next one (soft wrap).
    wrapped: bool,
    /// Changed since the frontend last took the changes (`take_dirty`).
    dirty: bool,
}

impl Line {
    fn new(cols: u16, bg: Style) -> Line {
        Line { cells: vec![blank(bg); cols as usize], wrapped: false, dirty: true }
    }
}

/// An erased cell: empty, keeping the background colour (as libghostty).
fn blank(s: Style) -> Cell {
    Cell { text: String::new(), style: Style { fg: Color::Default, bg: s.bg, flags: 0 }, wide: Wide::Narrow }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CursorShape {
    #[default]
    Block,
    Underline,
    Bar,
}

#[derive(Clone, Copy, Debug, Default)]
struct Cursor {
    x: u16,
    y: u16,
    /// Written in the last column: the next character wraps first.
    pending_wrap: bool,
    style: Style,
    /// G0–G3: b'B' ASCII, b'0' DEC special graphics.
    charsets: [u8; 4],
    /// Which of G0–G3 is in GL (SI/SO).
    gl: u8,
}

#[derive(Clone, Copy, Debug)]
struct Saved {
    cursor: Cursor,
    origin: bool,
    autowrap: bool,
}

#[derive(Clone, Debug)]
struct Screen {
    lines: Vec<Line>,
    cursor: Cursor,
    saved: Option<Saved>,
    /// Kitty keyboard flags stack (each screen has its own).
    kitty: Vec<u8>,
}

impl Screen {
    fn new(cols: u16, rows: u16) -> Screen {
        Screen {
            lines: (0..rows).map(|_| Line::new(cols, Style::default())).collect(),
            cursor: Cursor { charsets: [b'B'; 4], ..Default::default() },
            saved: None,
            kitty: vec![0],
        }
    }
}

/// Things the program caused that the frontend should know.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Bell,
    Title(String),
    Pwd(String),
    ClipboardWrite(String),
}

/// Modes kept as state (DEC private unless noted).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Modes {
    pub app_cursor_keys: bool,
    pub origin: bool,
    pub autowrap: bool,
    pub cursor_visible: bool,
    pub insert: bool,
    pub newline: bool,
    pub reverse_video: bool,
    pub mouse_x10: bool,
    pub mouse_normal: bool,
    pub mouse_button: bool,
    pub mouse_any: bool,
    pub mouse_utf8: bool,
    pub mouse_sgr: bool,
    pub mouse_urxvt: bool,
    pub mouse_sgr_pixels: bool,
    pub focus_events: bool,
    pub bracketed_paste: bool,
    pub sync_output: bool,
    pub grapheme_cluster: bool,
    pub color_scheme_report: bool,
    pub in_band_resize: bool,
    pub alt_screen: bool,
    pub cursor_blink: bool,
    pub app_keypad: bool,
}

impl Default for Modes {
    fn default() -> Modes {
        Modes {
            app_cursor_keys: false,
            origin: false,
            autowrap: true,
            cursor_visible: true,
            insert: false,
            newline: false,
            reverse_video: false,
            mouse_x10: false,
            mouse_normal: false,
            mouse_button: false,
            mouse_any: false,
            mouse_utf8: false,
            mouse_sgr: false,
            mouse_urxvt: false,
            mouse_sgr_pixels: false,
            focus_events: false,
            bracketed_paste: false,
            sync_output: false,
            grapheme_cluster: false,
            color_scheme_report: false,
            in_band_resize: false,
            alt_screen: false,
            cursor_blink: false,
            app_keypad: false,
        }
    }
}

impl Modes {
    pub fn mouse_tracking(&self) -> bool {
        self.mouse_x10 || self.mouse_normal || self.mouse_button || self.mouse_any
    }
}

/// The colours a session starts with (the theme).
#[derive(Clone, Copy, Debug)]
pub struct Colors {
    pub fg: [u8; 3],
    pub bg: [u8; 3],
    /// Colours 0–15; `None`: libghostty's built-in ones.
    pub palette: Option<[[u8; 3]; 16]>,
    pub cursor: Option<[u8; 3]>,
}

/// libghostty-vt's built-in colours 0–15.
pub const DEFAULT_16: [[u8; 3]; 16] = [
    [0x1d, 0x1f, 0x21],
    [0xcc, 0x66, 0x66],
    [0xb5, 0xbd, 0x68],
    [0xf0, 0xc6, 0x74],
    [0x81, 0xa2, 0xbe],
    [0xb2, 0x94, 0xbb],
    [0x8a, 0xbe, 0xb7],
    [0xc5, 0xc8, 0xc6],
    [0x66, 0x66, 0x66],
    [0xd5, 0x4e, 0x53],
    [0xb9, 0xca, 0x4a],
    [0xe7, 0xc5, 0x47],
    [0x7a, 0xa6, 0xda],
    [0xc3, 0x97, 0xd8],
    [0x70, 0xc0, 0xb1],
    [0xea, 0xea, 0xea],
];

/// The 256-colour palette: 0–15 given, then xterm's 6×6×6 cube and grey ramp.
pub fn palette256(p16: &[[u8; 3]; 16]) -> [[u8; 3]; 256] {
    let mut p = [[0u8; 3]; 256];
    p[..16].copy_from_slice(p16);
    let level = |v: usize| if v == 0 { 0 } else { (55 + v * 40) as u8 };
    for i in 0..216 {
        p[16 + i] = [level(i / 36), level(i / 6 % 6), level(i % 6)];
    }
    for i in 0..24 {
        let g = (8 + i * 10) as u8;
        p[232 + i] = [g, g, g];
    }
    p
}

pub struct Options {
    pub cols: u16,
    pub rows: u16,
    pub cell_width: u16,
    pub cell_height: u16,
    pub colors: Colors,
    /// `None`: unknown.
    pub dark: Option<bool>,
    pub xtversion: String,
}

pub struct Terminal {
    cols: u16,
    rows: u16,
    cell: (u16, u16),
    primary: Screen,
    alt: Screen,
    scrollback: VecDeque<Line>,
    /// The scroll region, 0-based, inclusive.
    top: u16,
    bottom: u16,
    tabs: Vec<bool>,
    pub modes: Modes,
    title: String,
    pwd: String,
    base: Colors,
    palette: [[u8; 3]; 256],
    fg: [u8; 3],
    bg: [u8; 3],
    cursor_color: Option<[u8; 3]>,
    cursor_shape: CursorShape,
    dark: Option<bool>,
    xtversion: String,
    parser: vte::Parser,
    pre: Scanner,
    /// SS2/SS3: the next character from G2/G3.
    single_shift: Option<u8>,
    /// Parameters of the CSI being dispatched as libghostty counts them:
    /// `CSI s` has none, `CSI 0 s` and `CSI ; s` one, `CSI 5 ; B` one
    /// (vte would say 1, 1, 2 and 2).
    csi_count: usize,
    replies: Vec<u8>,
    events: Vec<Event>,
    /// Lines of scrollback the viewport is above the live screen.
    viewport: usize,
    /// Every row counts as changed (see `take_dirty`).
    all_dirty: bool,
}

impl Terminal {
    pub fn new(o: Options) -> Terminal {
        let (cols, rows) = (o.cols.max(1), o.rows.max(1));
        let p16 = o.colors.palette.unwrap_or(DEFAULT_16);
        Terminal {
            cols,
            rows,
            cell: (o.cell_width, o.cell_height),
            primary: Screen::new(cols, rows),
            alt: Screen::new(cols, rows),
            scrollback: VecDeque::new(),
            top: 0,
            bottom: rows - 1,
            tabs: default_tabs(cols),
            modes: Modes::default(),
            title: String::new(),
            pwd: String::new(),
            base: o.colors,
            palette: palette256(&p16),
            fg: o.colors.fg,
            bg: o.colors.bg,
            cursor_color: o.colors.cursor,
            cursor_shape: CursorShape::Block,
            dark: o.dark,
            xtversion: o.xtversion,
            parser: vte::Parser::new(),
            pre: Scanner::default(),
            single_shift: None,
            csi_count: 0,
            replies: Vec::new(),
            events: Vec::new(),
            viewport: 0,
            all_dirty: true,
        }
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        let mut parser = std::mem::take(&mut self.parser);
        // The scanner sees what vte does not tell: APC strings (vte skips
        // them; the kitty graphics ones are answered here, in order with the
        // replies around them) and whether a CSI had parameter bytes at all.
        let mut start = 0;
        for i in 0..bytes.len() {
            match self.pre.byte(bytes[i]) {
                Pre::None => {}
                Pre::CsiEnd(count) => {
                    parser.advance(self, &bytes[start..i]);
                    self.csi_count = count;
                    parser.advance(self, &bytes[i..=i]);
                    start = i + 1;
                }
                Pre::Apc => {
                    parser.advance(self, &bytes[start..=i]);
                    start = i + 1;
                    let apc = std::mem::take(&mut self.pre.buf);
                    self.apc_dispatch(&apc);
                }
            }
        }
        parser.advance(self, &bytes[start..]);
        self.parser = parser;
    }

    /// Kitty graphics (`ESC _ G … ESC \`): only the support query is
    /// answered (`a=q` with an image id), as libghostty answers it.
    fn apc_dispatch(&mut self, apc: &[u8]) {
        let Some(body) = apc.strip_prefix(b"G") else { return };
        let control = body.split(|&c| c == b';').next().unwrap_or(b"");
        let (mut action, mut id, mut quiet) = (b't', 0u32, 0u32);
        for kv in control.split(|&c| c == b',') {
            if let [k, b'=', v @ ..] = kv {
                let num = || std::str::from_utf8(v).ok().and_then(|v| v.parse::<u32>().ok());
                match k {
                    b'a' => action = v.first().copied().unwrap_or(b't'),
                    b'i' => id = num().unwrap_or(0),
                    b'q' => quiet = num().unwrap_or(0),
                    _ => {}
                }
            }
        }
        if action == b'q' && id != 0 && quiet == 0 {
            self.reply(format!("\x1b_Gi={id};OK\x1b\\").as_bytes());
        }
    }

    pub fn take_replies(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.replies)
    }

    pub fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    pub fn size(&self) -> (u16, u16) {
        (self.cols, self.rows)
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn pwd(&self) -> &str {
        &self.pwd
    }

    pub fn colors(&self) -> ([u8; 3], [u8; 3], Option<[u8; 3]>) {
        (self.fg, self.bg, self.cursor_color)
    }

    /// A colour as RGB, or `None` for the default.
    pub fn resolve(&self, c: Color) -> Option<[u8; 3]> {
        match c {
            Color::Default => None,
            Color::Index(i) => Some(self.palette[i as usize]),
            Color::Rgb(v) => Some(v),
        }
    }

    pub fn set_colors(&mut self, c: Colors) {
        self.all_dirty = true;
        self.base = c;
        self.palette = palette256(&c.palette.unwrap_or(DEFAULT_16));
        self.fg = c.fg;
        self.bg = c.bg;
        self.cursor_color = c.cursor;
    }

    pub fn set_dark(&mut self, dark: Option<bool>) {
        self.dark = dark;
    }

    pub fn kitty_flags(&self) -> u8 {
        *self.screen_ref().kitty.last().unwrap_or(&0)
    }

    pub fn cursor_shape(&self) -> CursorShape {
        self.cursor_shape
    }

    pub fn scrollback_rows(&self) -> u64 {
        if self.modes.alt_screen {
            0
        } else {
            self.scrollback.len() as u64
        }
    }

    /// The cursor on the visible screen (`None` while the viewport is
    /// scrolled back past it).
    pub fn cursor(&self) -> Option<(u16, u16)> {
        let c = self.screen_ref().cursor;
        let y = c.y as usize + self.viewport;
        (y < self.rows as usize).then_some((c.x, y as u16))
    }

    /// A cell of the visible viewport.
    pub fn cell(&self, x: u16, y: u16) -> &Cell {
        &self.view_line(y).cells[x as usize]
    }

    fn view_line(&self, y: u16) -> &Line {
        let v = self.viewport.min(self.scrollback.len());
        if (y as usize) < v {
            &self.scrollback[self.scrollback.len() - v + y as usize]
        } else {
            &self.screen_ref().lines[y as usize - v]
        }
    }

    /// Rows the viewport is above the live screen.
    pub fn viewport_offset(&self) -> usize {
        self.viewport
    }

    pub fn scroll_viewport_top(&mut self) {
        self.all_dirty = true;
        self.viewport = if self.modes.alt_screen { 0 } else { self.scrollback.len() };
    }

    pub fn scroll_viewport_bottom(&mut self) {
        self.all_dirty = true;
        self.viewport = 0;
    }

    pub fn scroll_viewport_delta(&mut self, d: isize) {
        self.all_dirty = true;
        if self.modes.alt_screen {
            return;
        }
        let v = self.viewport as isize - d;
        self.viewport = v.clamp(0, self.scrollback.len() as isize) as usize;
    }

    fn screen_ref(&self) -> &Screen {
        if self.modes.alt_screen {
            &self.alt
        } else {
            &self.primary
        }
    }

    fn scr(&mut self) -> &mut Screen {
        if self.modes.alt_screen {
            &mut self.alt
        } else {
            &mut self.primary
        }
    }

    fn cur(&mut self) -> &mut Cursor {
        &mut self.scr().cursor
    }

    // ----------------------------------------------------------- resize

    pub fn resize(&mut self, cols: u16, rows: u16, cell_width: u16, cell_height: u16) {
        self.all_dirty = true;
        let (cols, rows) = (cols.max(1), rows.max(1));
        self.cell = (cell_width, cell_height);
        if (cols, rows) == (self.cols, self.rows) {
            return;
        }
        let old_cols = self.cols;
        // Primary screen: reflow soft-wrapped lines (scrollback included).
        let (lines, cur) = reflow(
            self.scrollback.drain(..).chain(std::mem::take(&mut self.primary.lines)).collect(),
            old_cols,
            cols,
            (self.primary.cursor.x, self.primary.cursor.y as usize + self.scrollback.len()),
        );
        let _ = cur;
        let mut lines = lines;
        let cy_abs = {
            // The cursor's line after reflow.
            cur.1
        };
        // Keep the cursor's line on screen: rows below it are dropped first
        // when the screen shrinks, then rows from the top go to scrollback.
        let mut keep_end = lines.len().max(cy_abs + 1);
        while keep_end > cy_abs + 1 && keep_end > rows as usize && lines.get(keep_end - 1).is_some_and(is_blank) {
            keep_end -= 1;
        }
        lines.truncate(keep_end);
        while lines.len() < rows as usize {
            lines.push(Line::new(cols, Style::default()));
        }
        let split = lines.len() - rows as usize;
        let screen: Vec<Line> = lines.split_off(split);
        self.scrollback = lines.into();
        self.primary.lines = screen;
        self.primary.cursor.x = cur.0.min(cols - 1);
        self.primary.cursor.y = (cy_abs.saturating_sub(split)).min(rows as usize - 1) as u16;
        self.primary.cursor.pending_wrap = false;
        // Alternate screen: cut or extended, no reflow.
        let mut alt = std::mem::take(&mut self.alt.lines);
        for l in &mut alt {
            l.cells.resize(cols as usize, blank(Style::default()));
            l.wrapped = false;
        }
        alt.resize(rows as usize, Line::new(cols, Style::default()));
        if alt.len() > rows as usize {
            alt.truncate(rows as usize);
        }
        self.alt.lines = alt;
        self.alt.cursor.x = self.alt.cursor.x.min(cols - 1);
        self.alt.cursor.y = self.alt.cursor.y.min(rows - 1);
        self.alt.cursor.pending_wrap = false;
        self.cols = cols;
        self.rows = rows;
        self.top = 0;
        self.bottom = rows - 1;
        self.tabs = default_tabs(cols);
        self.viewport = 0;
        if self.modes.in_band_resize {
            self.size_report_2048();
        }
    }

    fn size_report_2048(&mut self) {
        let (r, c) = (self.rows, self.cols);
        let (ph, pw) = (r as u32 * self.cell.1 as u32, c as u32 * self.cell.0 as u32);
        self.reply(format!("\x1b[48;{r};{c};{ph};{pw}t").as_bytes());
    }

    // ---------------------------------------------------------- writing

    fn reply(&mut self, b: &[u8]) {
        self.replies.extend_from_slice(b);
    }

    fn line(&mut self, y: u16) -> &mut Line {
        let l = &mut self.scr().lines[y as usize];
        l.dirty = true;
        l
    }

    /// Rows `top..=bottom` moved (scrolling, inserted or deleted lines).
    fn moved(&mut self, top: u16, bottom: u16) {
        for l in &mut self.scr().lines[top as usize..=bottom as usize] {
            l.dirty = true;
        }
    }

    /// Which rows of the viewport changed since the last call (as
    /// libghostty's render state keeps them); every row after a resize, a
    /// screen switch, a viewport scroll or new colours.
    pub fn take_dirty(&mut self) -> Vec<bool> {
        let all = std::mem::take(&mut self.all_dirty) || self.viewport > 0;
        let rows = self.rows as usize;
        let mut out = vec![all; rows];
        for (i, l) in self.scr().lines.iter_mut().enumerate() {
            out[i] |= std::mem::take(&mut l.dirty);
        }
        out
    }

    fn print_char(&mut self, c: char) {
        let c = {
            let cur = self.scr().cursor;
            let g = self.single_shift.take().unwrap_or(cur.gl);
            if cur.charsets[g as usize & 3] == b'0' {
                dec_special(c)
            } else {
                c
            }
        };
        let w = width(c);
        if w == 0 {
            self.combine(c);
            return;
        }
        let autowrap = self.modes.autowrap;
        let cols = self.cols;
        if self.scr().cursor.pending_wrap && autowrap {
            self.wrap_line();
        }
        if w == 2 && self.scr().cursor.x + 1 >= cols {
            if cols < 2 {
                return;
            }
            if !autowrap {
                return;
            }
            // As libghostty: a spacer in the last column, then the next row.
            let (x, y) = (self.scr().cursor.x, self.scr().cursor.y);
            let style = self.scr().cursor.style;
            self.clear_wide_at(x, y);
            self.line(y).cells[x as usize] =
                Cell { text: String::new(), style: Style { flags: 0, ..style }, wide: Wide::SpacerHead };
            self.wrap_line();
        }
        let (x, y, style) = {
            let c = self.scr().cursor;
            (c.x, c.y, c.style)
        };
        if self.modes.insert {
            self.insert_blanks(x, y, w as u16);
        }
        self.clear_wide_at(x, y);
        if w == 2 {
            self.clear_wide_at(x + 1, y);
        }
        let l = self.line(y);
        l.cells[x as usize] = Cell { text: c.to_string(), style, wide: if w == 2 { Wide::Wide } else { Wide::Narrow } };
        if w == 2 {
            l.cells[x as usize + 1] = Cell { text: String::new(), style, wide: Wide::SpacerTail };
        }
        let nx = x + w as u16;
        let cur = self.cur();
        if nx >= cols {
            cur.x = cols - 1;
            cur.pending_wrap = autowrap;
        } else {
            cur.x = nx;
        }
    }

    /// A zero-width character joins the cell before the cursor (dropped at
    /// the start of a row, as libghostty).
    fn combine(&mut self, c: char) {
        let cur = self.scr().cursor;
        let x = if cur.pending_wrap {
            cur.x
        } else if cur.x == 0 {
            return;
        } else {
            cur.x - 1
        };
        let l = self.line(cur.y);
        let mut i = x as usize;
        if l.cells[i].wide == Wide::SpacerTail && i > 0 {
            i -= 1;
        }
        if l.cells[i].text.is_empty() {
            return;
        }
        // A variation selector only after a character that has one (as
        // libghostty: the table is read off it).
        if matches!(c, '\u{fe0e}' | '\u{fe0f}') && !l.cells[i].text.chars().next().is_some_and(width::vs_base) {
            return;
        }
        l.cells[i].text.push(c);
    }

    /// Writing over half of a wide character clears the other half.
    fn clear_wide_at(&mut self, x: u16, y: u16) {
        let cols = self.cols as usize;
        let l = self.line(y);
        let i = x as usize;
        if i >= cols {
            return;
        }
        match l.cells[i].wide {
            Wide::Wide if i + 1 < cols => {
                let s = l.cells[i + 1].style;
                l.cells[i + 1] = blank(s);
            }
            Wide::SpacerTail if i > 0 => {
                let s = l.cells[i - 1].style;
                l.cells[i - 1] = blank(s);
            }
            _ => {}
        }
    }

    fn wrap_line(&mut self) {
        let y = self.scr().cursor.y;
        self.line(y).wrapped = true;
        self.cur().x = 0;
        self.cur().pending_wrap = false;
        self.linefeed();
    }

    fn linefeed(&mut self) {
        let (y, bottom) = (self.scr().cursor.y, self.bottom);
        if y == bottom {
            self.scroll_up(1);
        } else if y + 1 < self.rows {
            self.cur().y = y + 1;
        }
        self.cur().pending_wrap = false;
    }

    fn reverse_index(&mut self) {
        let y = self.scr().cursor.y;
        // As libghostty: scrolling keeps a pending wrap, moving up does not.
        if y == self.top {
            self.scroll_down(1);
        } else if y > 0 {
            self.cur().y = y - 1;
            self.cur().pending_wrap = false;
        } else {
            self.cur().pending_wrap = false;
        }
    }

    /// Scrolls the region up; lines leaving a region that starts at the top
    /// of the primary screen go to the scrollback (as libghostty, whatever the
    /// region's bottom).
    fn scroll_up(&mut self, n: u16) {
        let (top, bottom) = (self.top as usize, self.bottom as usize);
        let n = (n as usize).min(bottom - top + 1);
        let bg = self.scr().cursor.style;
        let cols = self.cols;
        let to_scrollback = top == 0 && !self.modes.alt_screen;
        for _ in 0..n {
            let l = self.scr().lines.remove(top);
            if to_scrollback {
                self.scrollback.push_back(l);
            }
            self.scr().lines.insert(bottom, Line::new(cols, bg));
        }
        if to_scrollback && self.viewport > 0 {
            self.viewport = (self.viewport + n).min(self.scrollback.len());
        }
        self.moved(top as u16, bottom as u16);
    }

    fn scroll_down(&mut self, n: u16) {
        let (top, bottom) = (self.top as usize, self.bottom as usize);
        let n = (n as usize).min(bottom - top + 1);
        let bg = self.scr().cursor.style;
        let cols = self.cols;
        for _ in 0..n {
            self.scr().lines.remove(bottom);
            self.scr().lines.insert(top, Line::new(cols, bg));
        }
        self.moved(top as u16, bottom as u16);
    }

    fn insert_blanks(&mut self, x: u16, y: u16, n: u16) {
        let bg = self.scr().cursor.style;
        let cols = self.cols as usize;
        let l = self.line(y);
        let x = x as usize;
        let n = (n as usize).min(cols - x);
        for _ in 0..n {
            l.cells.pop();
            l.cells.insert(x, blank(bg));
        }
        fix_split_wide(l);
    }

    fn delete_chars(&mut self, n: u16) {
        let (x, y, bg) = {
            let c = self.scr().cursor;
            (c.x as usize, c.y, c.style)
        };
        let cols = self.cols as usize;
        let l = self.line(y);
        let n = (n as usize).min(cols - x);
        for _ in 0..n {
            l.cells.remove(x);
            l.cells.push(blank(bg));
        }
        fix_split_wide(l);
    }

    fn erase_chars(&mut self, n: u16) {
        let (x, y, bg) = {
            let c = self.scr().cursor;
            (c.x as usize, c.y, c.style)
        };
        let cols = self.cols as usize;
        let end = (x + n.max(1) as usize).min(cols);
        self.clear_wide_at(x as u16, y);
        if end > 0 {
            self.clear_wide_at(end as u16 - 1, y);
        }
        let l = self.line(y);
        for c in &mut l.cells[x..end] {
            *c = blank(bg);
        }
    }

    fn erase_line(&mut self, mode: u16) {
        let (x, y, bg) = {
            let c = self.scr().cursor;
            (c.x as usize, c.y, c.style)
        };
        let cols = self.cols as usize;
        let (a, b) = match mode {
            0 => (x, cols),
            1 => (0, x + 1),
            2 => (0, cols),
            _ => return,
        };
        if a < cols {
            self.clear_wide_at(a as u16, y);
        }
        if b > 0 && b - 1 < cols {
            self.clear_wide_at(b as u16 - 1, y);
        }
        let l = self.line(y);
        for c in &mut l.cells[a..b.min(cols)] {
            *c = blank(bg);
        }
        if mode != 1 {
            l.wrapped = false;
        }
        self.cur().pending_wrap = false;
    }

    fn erase_display(&mut self, mode: u16) {
        let (y, bg) = {
            let c = self.scr().cursor;
            (c.y, c.style)
        };
        let cols = self.cols;
        match mode {
            0 => {
                self.erase_line(0);
                for r in y + 1..self.rows {
                    *self.line(r) = Line::new(cols, bg);
                }
            }
            1 => {
                for r in 0..y {
                    *self.line(r) = Line::new(cols, bg);
                }
                self.erase_line(1);
            }
            2 => {
                for r in 0..self.rows {
                    *self.line(r) = Line::new(cols, bg);
                }
            }
            3 => {
                if !self.modes.alt_screen {
                    self.scrollback.clear();
                    self.viewport = 0;
                }
            }
            // libghostty's "scroll complete": the rows down to the last one
            // with content go to the scrollback, the screen is cleared.
            22 => {
                let used = |l: &Line| l.cells.iter().any(|c| !c.text.is_empty() || c.style.bg != Color::Default);
                let n = self.screen_ref().lines.iter().rposition(used).map_or(0, |r| r + 1);
                if !self.modes.alt_screen {
                    for r in 0..n {
                        let l = self.screen_ref().lines[r].clone();
                        self.scrollback.push_back(l);
                    }
                }
                for r in 0..self.rows {
                    *self.line(r) = Line::new(cols, Style::default());
                }
                let cur = self.cur();
                if cur.y as usize >= n {
                    cur.y -= n as u16;
                } else {
                    cur.x = 0;
                    cur.y = 0;
                }
            }
            _ => {}
        }
        // As libghostty, ED 3 and unknown modes keep a pending wrap.
        if matches!(mode, 0 | 1 | 2 | 22) {
            self.cur().pending_wrap = false;
        }
    }

    fn insert_lines(&mut self, n: u16) {
        let y = self.scr().cursor.y;
        if y < self.top || y > self.bottom {
            return;
        }
        let (top, bg, cols) = (self.top, self.scr().cursor.style, self.cols);
        self.top = y;
        for _ in 0..n.min(self.bottom - y + 1) {
            let b = self.bottom as usize;
            self.scr().lines.remove(b);
            self.scr().lines.insert(y as usize, Line::new(cols, bg));
        }
        self.top = top;
        self.moved(y, self.bottom);
        self.cur().x = 0;
        self.cur().pending_wrap = false;
    }

    fn delete_lines(&mut self, n: u16) {
        let y = self.scr().cursor.y;
        if y < self.top || y > self.bottom {
            return;
        }
        let (bg, cols) = (self.scr().cursor.style, self.cols);
        for _ in 0..n.min(self.bottom - y + 1) {
            self.scr().lines.remove(y as usize);
            let b = self.bottom as usize;
            self.scr().lines.insert(b, Line::new(cols, bg));
        }
        self.moved(y, self.bottom);
        self.cur().x = 0;
        self.cur().pending_wrap = false;
    }

    // ----------------------------------------------------- cursor moves

    /// Moves the cursor; with origin mode, rows are inside the region.
    fn goto(&mut self, x: u16, y: u16) {
        let (top, bottom, rows, cols) = (self.top, self.bottom, self.rows, self.cols);
        let origin = self.modes.origin;
        let cur = self.cur();
        cur.x = x.min(cols - 1);
        cur.y = if origin { (top + y).min(bottom) } else { y.min(rows - 1) };
        cur.pending_wrap = false;
    }

    fn move_vertical(&mut self, d: i32) {
        let (top, bottom, rows) = (self.top as i32, self.bottom as i32, self.rows as i32);
        let y = self.scr().cursor.y as i32;
        // As libghostty: going down from above the region's bottom stops at
        // it, going up from below its top stops at that.
        let ny = if d > 0 {
            (y + d).min(if y <= bottom { bottom } else { rows - 1 })
        } else {
            (y + d).max(if y >= top { top } else { 0 })
        };
        let cur = self.cur();
        cur.y = ny as u16;
        cur.pending_wrap = false;
    }

    fn move_horizontal(&mut self, d: i32) {
        let cols = self.cols as i32;
        let cur = self.cur();
        cur.x = (cur.x as i32 + d).clamp(0, cols - 1) as u16;
        cur.pending_wrap = false;
    }

    fn tab(&mut self, n: u16) {
        let cols = self.cols;
        for _ in 0..n.max(1) {
            let mut x = self.scr().cursor.x + 1;
            while x < cols - 1 && !self.tabs[x as usize] {
                x += 1;
            }
            self.cur().x = x.min(cols - 1);
        }
    }

    fn back_tab(&mut self, n: u16) {
        for _ in 0..n.max(1) {
            let mut x = self.scr().cursor.x;
            if x == 0 {
                break;
            }
            x -= 1;
            while x > 0 && !self.tabs[x as usize] {
                x -= 1;
            }
            self.cur().x = x;
        }
    }

    fn save_cursor(&mut self) {
        let s = Saved { cursor: self.scr().cursor, origin: self.modes.origin, autowrap: self.modes.autowrap };
        self.scr().saved = Some(s);
    }

    fn restore_cursor(&mut self) {
        let s = self.scr().saved.unwrap_or(Saved {
            cursor: Cursor { charsets: [b'B'; 4], ..Default::default() },
            origin: false,
            autowrap: true,
        });
        let (cols, rows) = (self.cols, self.rows);
        let cur = self.cur();
        *cur = s.cursor;
        cur.x = cur.x.min(cols - 1);
        cur.y = cur.y.min(rows - 1);
        self.modes.origin = s.origin;
        self.modes.autowrap = s.autowrap;
    }

    fn set_region(&mut self, top: u16, bottom: u16) {
        let rows = self.rows;
        let t = top.max(1) - 1;
        let b = if bottom == 0 { rows } else { bottom.min(rows) } - 1;
        if t < b {
            self.top = t;
            self.bottom = b;
            self.goto(0, 0);
        }
    }

    fn set_alt_screen(&mut self, on: bool, save: bool, clear: bool) {
        if on == self.modes.alt_screen {
            return;
        }
        self.all_dirty = true;
        if on {
            if save {
                self.save_cursor();
            }
            let cur = self.primary.cursor;
            self.modes.alt_screen = true;
            self.alt.cursor = cur;
            if clear {
                let (cols, rows) = (self.cols, self.rows);
                self.alt.lines = (0..rows).map(|_| Line::new(cols, Style::default())).collect();
            }
        } else {
            if clear {
                let (cols, rows) = (self.cols, self.rows);
                self.alt.lines = (0..rows).map(|_| Line::new(cols, Style::default())).collect();
            }
            self.modes.alt_screen = false;
            if save {
                self.restore_cursor();
            }
        }
        self.viewport = 0;
    }

    fn full_reset(&mut self) {
        let o = Options {
            cols: self.cols,
            rows: self.rows,
            cell_width: self.cell.0,
            cell_height: self.cell.1,
            colors: self.base,
            dark: self.dark,
            xtversion: std::mem::take(&mut self.xtversion),
        };
        let replies = std::mem::take(&mut self.replies);
        let events = std::mem::take(&mut self.events);
        // The scanner has already read past this point of the input (`feed`);
        // starting it over would lose where it is.
        let pre = std::mem::take(&mut self.pre);
        *self = Terminal::new(o);
        self.replies = replies;
        self.events = events;
        self.pre = pre;
    }

    // ---------------------------------------------------------- modes

    fn set_mode(&mut self, private: bool, mode: u16, on: bool) {
        if !private {
            match mode {
                4 => self.modes.insert = on,
                20 => self.modes.newline = on,
                _ => {}
            }
            return;
        }
        match mode {
            1 => self.modes.app_cursor_keys = on,
            5 => self.modes.reverse_video = on,
            6 => {
                self.modes.origin = on;
                self.goto(0, 0);
            }
            7 => self.modes.autowrap = on,
            9 => self.modes.mouse_x10 = on,
            12 => self.modes.cursor_blink = on,
            25 => self.modes.cursor_visible = on,
            47 => self.set_alt_screen(on, false, false),
            66 => self.modes.app_keypad = on,
            1000 => self.modes.mouse_normal = on,
            1002 => self.modes.mouse_button = on,
            1003 => self.modes.mouse_any = on,
            1004 => self.modes.focus_events = on,
            1005 => self.modes.mouse_utf8 = on,
            1006 => self.modes.mouse_sgr = on,
            1015 => self.modes.mouse_urxvt = on,
            1016 => self.modes.mouse_sgr_pixels = on,
            1047 => self.set_alt_screen(on, false, true),
            1048 => {
                if on {
                    self.save_cursor()
                } else {
                    self.restore_cursor()
                }
            }
            1049 => self.set_alt_screen(on, true, true),
            2004 => self.modes.bracketed_paste = on,
            2026 => self.modes.sync_output = on,
            2027 => self.modes.grapheme_cluster = on,
            2031 => self.modes.color_scheme_report = on,
            2048 => {
                self.modes.in_band_resize = on;
                if on {
                    self.size_report_2048();
                }
            }
            _ => {}
        }
    }

    /// DECRQM: 1 set, 2 reset, 0 not recognised (private modes only, as
    /// libghostty).
    fn mode_state(&self, mode: u16) -> u8 {
        let m = &self.modes;
        let v = match mode {
            1 => m.app_cursor_keys,
            5 => m.reverse_video,
            6 => m.origin,
            7 => m.autowrap,
            9 => m.mouse_x10,
            12 => m.cursor_blink,
            25 => m.cursor_visible,
            47 | 1047 | 1049 => m.alt_screen,
            66 => m.app_keypad,
            1000 => m.mouse_normal,
            1002 => m.mouse_button,
            1003 => m.mouse_any,
            1004 => m.focus_events,
            1005 => m.mouse_utf8,
            1006 => m.mouse_sgr,
            1015 => m.mouse_urxvt,
            1016 => m.mouse_sgr_pixels,
            2004 => m.bracketed_paste,
            2026 => m.sync_output,
            2027 => m.grapheme_cluster,
            2031 => m.color_scheme_report,
            2048 => m.in_band_resize,
            _ => return 0,
        };
        if v {
            1
        } else {
            2
        }
    }

    // ------------------------------------------------------------ SGR

    /// SGR over the first `count` parameters (vte adds an empty last one
    /// libghostty does not count).
    fn sgr(&mut self, params: &vte::Params, count: usize) {
        let ps: Vec<&[u16]> = params.iter().take(count).collect();
        let mut it = ps.iter().copied().peekable();
        let mut style = self.scr().cursor.style;
        if count == 0 {
            style = Style::default();
        }
        while let Some(p) = it.next() {
            match p[0] {
                38 | 48 | 58 => {
                    let c = extended_color(p, &mut it);
                    match (p[0], c) {
                        (38, Some(c)) => style.fg = c,
                        (48, Some(c)) => style.bg = c,
                        _ => {} // the underline colour is not in the snapshot
                    }
                    if let Some(v) = unknown_colour_rest(p) {
                        sgr_simple(&mut style, v);
                    }
                }
                _ => sgr_simple(&mut style, p),
            }
        }
        self.cur().style = style;
    }

    // -------------------------------------------------------- replies

    fn device_status(&mut self, private: bool, n: u16) {
        match (private, n) {
            (false, 5) => self.reply(b"\x1b[0n"),
            (false, 6) => {
                let c = self.scr().cursor;
                let y = if self.modes.origin { c.y.saturating_sub(self.top) } else { c.y };
                self.reply(format!("\x1b[{};{}R", y + 1, c.x + 1).as_bytes());
            }
            (true, 996) => {
                if let Some(d) = self.dark {
                    self.reply(format!("\x1b[?997;{}n", if d { 1 } else { 2 }).as_bytes());
                }
            }
            _ => {}
        }
    }

    fn color_reply(&mut self, prefix: &str, c: [u8; 3], bell: bool) {
        let h = |v: u8| format!("{v:02x}{v:02x}");
        let end = if bell { "\x07" } else { "\x1b\\" };
        self.reply(format!("\x1b]{prefix};rgb:{}/{}/{}{end}", h(c[0]), h(c[1]), h(c[2])).as_bytes());
    }
}

/// What the scanner saw end at a byte.
enum Pre {
    None,
    /// A CSI's final byte, with its parameter count (see `csi_count`).
    CsiEnd(usize),
    /// An APC string (`ESC _ … ESC \`); its contents are in `buf`.
    Apc,
}

/// Follows the byte stream as vte's parser does, as far as CSI and APC go:
/// CAN and SUB cancel either, ESC starts over.
#[derive(Default)]
struct Scanner {
    state: u8,
    /// `;` seen in this CSI.
    csi_seps: usize,
    /// A digit or `:` since the last `;`.
    csi_trailing: bool,
    buf: Vec<u8>,
}

impl Scanner {
    const GROUND: u8 = 0;
    const ESC: u8 = 1;
    const APC: u8 = 2;
    const APC_ESC: u8 = 3;
    const CSI: u8 = 4;
    /// Only the control part matters; the payload is not kept past this.
    const KEEP: usize = 4096;

    fn byte(&mut self, b: u8) -> Pre {
        match (self.state, b) {
            (Self::GROUND, 0x1b) => self.state = Self::ESC,
            (Self::GROUND, _) => {}
            (Self::ESC, b'_') => {
                self.state = Self::APC;
                self.buf.clear();
            }
            (Self::ESC, b'[') => {
                self.state = Self::CSI;
                self.csi_seps = 0;
                self.csi_trailing = false;
            }
            (Self::ESC, 0x1b) => {}
            (Self::ESC, _) => self.state = Self::GROUND,
            (Self::CSI, b';') => {
                self.csi_seps += 1;
                self.csi_trailing = false;
            }
            (Self::CSI, 0x30..=0x3a) => self.csi_trailing = true,
            (Self::CSI, 0x40..=0x7e) => {
                self.state = Self::GROUND;
                return Pre::CsiEnd(self.csi_seps + self.csi_trailing as usize);
            }
            (Self::CSI, 0x1b) => self.state = Self::ESC,
            (Self::CSI, 0x18 | 0x1a) => self.state = Self::GROUND,
            (Self::CSI, _) => {}
            (Self::APC, 0x1b) => self.state = Self::APC_ESC,
            (Self::APC, 0x18 | 0x1a) => self.state = Self::GROUND,
            (Self::APC, _) => {
                if self.buf.len() < Self::KEEP {
                    self.buf.push(b)
                }
            }
            (_, b'\\') => {
                self.state = Self::GROUND;
                return Pre::Apc;
            }
            (_, b'_') => {
                self.state = Self::APC;
                self.buf.clear();
            }
            (_, b'[') => {
                self.state = Self::CSI;
                self.csi_seps = 0;
                self.csi_trailing = false;
            }
            (_, 0x1b) => self.state = Self::ESC,
            _ => self.state = Self::GROUND,
        }
        Pre::None
    }
}

/// The DEC special graphics set (`ESC ( 0`) for 0x60–0x7e.
fn dec_special(c: char) -> char {
    const MAP: [char; 31] = [
        '◆', '▒', '␉', '␌', '␍', '␊', '°', '±', '␤', '␋', '┘', '┐', '┌', '└', '┼', '⎺', '⎻', '─', '⎼', '⎽', '├', '┤',
        '┴', '┬', '│', '≤', '≥', 'π', '≠', '£', '·',
    ];
    match c {
        '\u{60}'..='\u{7e}' => MAP[c as usize - 0x60],
        _ => c,
    }
}

fn default_tabs(cols: u16) -> Vec<bool> {
    (0..cols).map(|x| x > 0 && x % 8 == 0).collect()
}

fn is_blank(l: &Line) -> bool {
    !l.wrapped && l.cells.iter().all(|c| c.text.is_empty() && c.style == Style::default())
}

/// A wide character cut in two by an insert or delete loses both halves.
fn fix_split_wide(l: &mut Line) {
    let n = l.cells.len();
    for i in 0..n {
        let bad = match l.cells[i].wide {
            Wide::Wide => i + 1 >= n || l.cells[i + 1].wide != Wide::SpacerTail,
            Wide::SpacerTail => i == 0 || l.cells[i - 1].wide != Wide::Wide,
            _ => false,
        };
        if bad {
            let s = l.cells[i].style;
            l.cells[i] = blank(s);
        }
    }
}

/// One SGR attribute other than an extended colour.
fn sgr_simple(style: &mut Style, p: &[u16]) {
    let v = p[0];
    match v {
        0 => *style = Style::default(),
        1 => style.flags |= flags::BOLD,
        2 => style.flags |= flags::FAINT,
        3 => style.flags |= flags::ITALIC,
        4 => {
            if p.len() > 1 && p[1] == 0 {
                style.flags &= !flags::UNDERLINE
            } else {
                style.flags |= flags::UNDERLINE
            }
        }
        5 | 6 => style.flags |= flags::BLINK,
        7 => style.flags |= flags::INVERSE,
        8 => style.flags |= flags::INVISIBLE,
        9 => style.flags |= flags::STRIKETHROUGH,
        21 => style.flags |= flags::UNDERLINE,
        22 => style.flags &= !(flags::BOLD | flags::FAINT),
        23 => style.flags &= !flags::ITALIC,
        24 => style.flags &= !flags::UNDERLINE,
        25 => style.flags &= !flags::BLINK,
        27 => style.flags &= !flags::INVERSE,
        28 => style.flags &= !flags::INVISIBLE,
        29 => style.flags &= !flags::STRIKETHROUGH,
        30..=37 => style.fg = Color::Index((v - 30) as u8),
        39 => style.fg = Color::Default,
        40..=47 => style.bg = Color::Index((v - 40) as u8),
        49 => style.bg = Color::Default,
        53 => style.flags |= flags::OVERLINE,
        55 => style.flags &= !flags::OVERLINE,
        90..=97 => style.fg = Color::Index((v - 90 + 8) as u8),
        100..=107 => style.bg = Color::Index((v - 100 + 8) as u8),
        _ => {}
    }
}

/// `38;5;N`, `38;2;R;G;B`, also the colon forms (`38:5:N`, `38:2::R:G:B`).
/// As libghostty: values wrap to a byte, and with any other kind (or too few
/// values) only the 38 is dropped, the parameters after it count on their own.
fn extended_color<'a>(
    p: &[u16],
    it: &mut std::iter::Peekable<impl Iterator<Item = &'a [u16]> + Clone>,
) -> Option<Color> {
    if p.len() > 1 {
        return match p[1] {
            5 if p.len() > 2 => Some(Color::Index(p[2] as u8)),
            2 if p.len() >= 5 => {
                let v = &p[p.len() - 3..];
                Some(Color::Rgb([v[0] as u8, v[1] as u8, v[2] as u8]))
            }
            _ => None,
        };
    }
    let kind = it.peek().map(|q| q[0])?;
    let mut ahead = it.clone();
    ahead.next();
    match kind {
        5 => {
            let n = ahead.next()?[0];
            it.next();
            it.next();
            Some(Color::Index(n as u8))
        }
        2 => {
            let r = ahead.next()?[0];
            let g = ahead.next()?[0];
            let b = ahead.next()?[0];
            for _ in 0..4 {
                it.next();
            }
            Some(Color::Rgb([r as u8, g as u8, b as u8]))
        }
        _ => None,
    }
}

/// The colon form with a kind libghostty does not know (`38:9`): it reads the
/// value after the 38 as an SGR of its own.
fn unknown_colour_rest(p: &[u16]) -> Option<&[u16]> {
    match p.get(1) {
        Some(2) if p.len() >= 5 => None,
        Some(5) if p.len() > 2 => None,
        Some(_) => Some(&p[1..2]),
        None => None,
    }
}

/// Reflows lines (scrollback and screen together) to a new width: soft-wrapped
/// lines are joined and cut again. Returns the lines and the cursor's place.
fn reflow(lines: Vec<Line>, old: u16, new: u16, cursor: (u16, usize)) -> (Vec<Line>, (u16, usize)) {
    if old == new {
        return (lines, cursor);
    }
    let mut out: Vec<Line> = Vec::with_capacity(lines.len());
    let mut cur = (0u16, 0usize);
    let mut logical: Vec<Cell> = Vec::new();
    let mut cursor_at: Option<usize> = None;
    let flush =
        |out: &mut Vec<Line>, logical: &mut Vec<Cell>, cursor_at: &mut Option<usize>, cur: &mut (u16, usize)| {
            // Trailing blanks of a logical line are not carried over.
            let mut len = logical.len();
            while len > 0 && logical[len - 1].text.is_empty() && logical[len - 1].style == Style::default() {
                len -= 1;
            }
            if let Some(c) = *cursor_at {
                len = len.max(c + 1).min(logical.len().max(c + 1));
            }
            let cells: Vec<Cell> = logical.drain(..).take(len).collect();
            let start = out.len();
            let mut i = 0;
            loop {
                let mut line = Line::new(new, Style::default());
                let mut x = 0usize;
                while i < cells.len() && x < new as usize {
                    let c = &cells[i];
                    if c.wide == Wide::SpacerHead {
                        i += 1;
                        continue;
                    }
                    if c.wide == Wide::Wide && new < 2 {
                        // One column: a wide character has no room at all.
                        i += 1;
                        if cells.get(i).is_some_and(|c| c.wide == Wide::SpacerTail) {
                            i += 1;
                        }
                        continue;
                    }
                    if c.wide == Wide::Wide && x + 1 >= new as usize {
                        line.cells[x] = Cell { wide: Wide::SpacerHead, ..blank(Style::default()) };
                        break;
                    }
                    line.cells[x] = c.clone();
                    if let Some(cp) = *cursor_at {
                        if cp == i {
                            *cur = (x as u16, out.len());
                        }
                    }
                    x += 1;
                    i += 1;
                }
                let more = i < cells.len();
                line.wrapped = more;
                out.push(line);
                if !more {
                    break;
                }
            }
            if let Some(cp) = *cursor_at {
                if cp >= cells.len() {
                    // The cursor stood past the text: keep its column on the last row.
                    let last = out.len() - 1;
                    let col = cp - (cells.len() / new as usize) * new as usize;
                    *cur = ((col as u16).min(new - 1), last.max(start));
                }
            }
            *cursor_at = None;
        };
    for (n, l) in lines.into_iter().enumerate() {
        if n == cursor.1 {
            cursor_at = Some(logical.len() + cursor.0 as usize);
        }
        let wrapped = l.wrapped;
        logical.extend(l.cells);
        if !wrapped {
            flush(&mut out, &mut logical, &mut cursor_at, &mut cur);
        }
    }
    if !logical.is_empty() || cursor_at.is_some() {
        flush(&mut out, &mut logical, &mut cursor_at, &mut cur);
    }
    (out, cur)
}

fn param(params: &vte::Params, i: usize, default: u16) -> u16 {
    match params.iter().nth(i).map(|p| p[0]) {
        None | Some(0) => default,
        Some(v) => v,
    }
}

impl vte::Perform for Terminal {
    fn print(&mut self, c: char) {
        self.print_char(c);
    }

    fn execute(&mut self, b: u8) {
        match b {
            0x07 => self.events.push(Event::Bell),
            0x08 => {
                let cur = self.cur();
                cur.pending_wrap = false;
                cur.x = cur.x.saturating_sub(1);
            }
            0x09 => self.tab(1),
            0x0a..=0x0c => {
                if self.modes.newline {
                    self.cur().x = 0;
                }
                self.linefeed();
            }
            0x0d => {
                let cur = self.cur();
                cur.x = 0;
                cur.pending_wrap = false;
            }
            0x0e => self.cur().gl = 1,
            0x0f => self.cur().gl = 0,
            _ => {}
        }
    }

    fn csi_dispatch(&mut self, params: &vte::Params, inter: &[u8], ignore: bool, action: char) {
        if ignore {
            return;
        }
        // As libghostty, a sequence with more parameters than it takes is
        // ignored as a whole.
        let most = match (inter, action) {
            ([], 'H' | 'f' | 'r') => 2,
            (
                [] | [b'?'],
                'A'..='G'
                | 'I'..='M'
                | 'P'
                | 'S'
                | 'T'
                | 'X'
                | 'Z'
                | '@'
                | '`'
                | 'a'
                | 'b'
                | 'd'
                | 'j'
                | 'k'
                | 'e'
                | 'g'
                | 'n'
                | 't',
            )
            | ([b' '], 'q')
            | ([b'?', b'$'], 'p') => 1,
            _ => usize::MAX,
        };
        let count = self.csi_count.min(params.len());
        if count > most {
            return;
        }
        let p0 = |d: u16| param(params, 0, d);
        let private = inter.first() == Some(&b'?');
        match (inter, action) {
            ([], '@') => {
                let (x, y) = (self.scr().cursor.x, self.scr().cursor.y);
                self.insert_blanks(x, y, p0(1));
                self.cur().pending_wrap = false;
            }
            ([], 'A' | 'k') => self.move_vertical(-(p0(1) as i32)),
            ([], 'B' | 'e') => self.move_vertical(p0(1) as i32),
            ([], 'C' | 'a') => self.move_horizontal(p0(1) as i32),
            ([], 'D' | 'j') => self.move_horizontal(-(p0(1) as i32)),
            ([], 'E') => {
                self.move_vertical(p0(1) as i32);
                self.cur().x = 0;
            }
            ([], 'F') => {
                self.move_vertical(-(p0(1) as i32));
                self.cur().x = 0;
            }
            ([], 'G' | '`') => {
                let y = self.scr().cursor.y;
                let cols = self.cols;
                let cur = self.cur();
                cur.x = (p0(1) - 1).min(cols - 1);
                cur.y = y;
                cur.pending_wrap = false;
            }
            ([], 'H' | 'f') => {
                let (r, c) = (param(params, 0, 1), param(params, 1, 1));
                self.goto(c - 1, r - 1);
            }
            ([], 'I') if count > 0 && param(params, 0, 0) == 0 => {}
            ([], 'I') => self.tab(p0(1)),
            ([], 'J') | ([b'?'], 'J') => self.erase_display(param(params, 0, 0)),
            ([], 'K') | ([b'?'], 'K') => self.erase_line(param(params, 0, 0)),
            // As libghostty, an explicit 0 does nothing here (elsewhere it is 1).
            ([], 'L' | 'M' | 'P' | 'S' | 'T' | 'Z') if count > 0 && param(params, 0, 0) == 0 => {}
            ([], 'L') => self.insert_lines(p0(1)),
            ([], 'M') => self.delete_lines(p0(1)),
            ([], 'P') => {
                self.delete_chars(p0(1));
                self.cur().pending_wrap = false;
            }
            ([], 'S') => self.scroll_up(p0(1)),
            ([], 'T') => self.scroll_down(p0(1)),
            ([], 'X') => {
                self.erase_chars(p0(1));
                self.cur().pending_wrap = false;
            }
            ([], 'Z') => self.back_tab(p0(1)),
            ([], 'b') => {
                // REP: the last printed character again.
                let c = self.scr().cursor;
                let x = if c.pending_wrap { c.x } else { c.x.saturating_sub(1) };
                let ch = self.screen_ref().lines[c.y as usize].cells[x as usize].text.chars().next();
                if let Some(ch) = ch {
                    for _ in 0..p0(1) {
                        self.print_char(ch);
                    }
                }
            }
            ([], 'c') => self.reply(b"\x1b[?62;1;6;22;52c"),
            ([b'>'], 'c') => self.reply(b"\x1b[>1;1;0c"),
            ([b'='], 'c') => self.reply(b"\x1bP!|00000000\x1b\\"),
            ([], 'd') => {
                let x = self.scr().cursor.x;
                self.goto(x, p0(1) - 1);
            }
            // As libghostty: TBC needs its parameter.
            ([], 'g') if count == 0 => {}
            ([], 'g') => match param(params, 0, 0) {
                0 => {
                    let x = self.scr().cursor.x as usize;
                    self.tabs[x] = false;
                }
                3 => self.tabs.iter_mut().for_each(|t| *t = false),
                _ => {}
            },
            ([] | [b'?'], 'h') => {
                for p in params.iter() {
                    self.set_mode(private, p[0], true);
                }
            }
            ([] | [b'?'], 'l') => {
                for p in params.iter() {
                    self.set_mode(private, p[0], false);
                }
            }
            ([], 'm') => self.sgr(params, count),
            ([] | [b'?'], 'n') => self.device_status(private, param(params, 0, 0)),
            ([b'?', b'$'], 'p') => {
                let m = param(params, 0, 0);
                let s = self.mode_state(m);
                self.reply(format!("\x1b[?{m};{s}$y").as_bytes());
            }
            ([b' '], 'q') => {
                let (shape, blink) = match param(params, 0, 0) {
                    0 | 1 => (CursorShape::Block, true),
                    2 => (CursorShape::Block, false),
                    3 => (CursorShape::Underline, true),
                    4 => (CursorShape::Underline, false),
                    5 => (CursorShape::Bar, true),
                    6 => (CursorShape::Bar, false),
                    _ => return,
                };
                self.cursor_shape = shape;
                self.modes.cursor_blink = blink;
            }
            ([b'>'], 'q') => {
                let v = format!("\x1bP>|{}\x1b\\", self.xtversion);
                self.reply(v.as_bytes());
            }
            ([], 'r') => self.set_region(param(params, 0, 1), param(params, 1, 0)),
            // As libghostty: only without parameters (with them it would be
            // DECSLRM, which needs mode 69).
            ([], 's') if count == 0 => self.save_cursor(),
            ([], 't') => {
                let (r, c, cw, ch) = (self.rows as u32, self.cols as u32, self.cell.0 as u32, self.cell.1 as u32);
                match param(params, 0, 0) {
                    14 => self.reply(format!("\x1b[4;{};{}t", r * ch, c * cw).as_bytes()),
                    16 => self.reply(format!("\x1b[6;{ch};{cw}t").as_bytes()),
                    18 => self.reply(format!("\x1b[8;{r};{c}t").as_bytes()),
                    _ => {}
                }
            }
            ([], 'u') => self.restore_cursor(),
            ([b'?'], 'u') => {
                let f = self.kitty_flags();
                self.reply(format!("\x1b[?{f}u").as_bytes());
            }
            ([b'>'], 'u') => {
                let f = param(params, 0, 0) as u8 & 0x1f;
                let k = &mut self.scr().kitty;
                if k.len() >= 8 {
                    k.remove(0);
                }
                k.push(f);
            }
            ([b'<'], 'u') => {
                let n = p0(1) as usize;
                let k = &mut self.scr().kitty;
                for _ in 0..n {
                    if k.len() > 1 {
                        k.pop();
                    } else {
                        k[0] = 0;
                    }
                }
            }
            ([b'='], 'u') => {
                let f = param(params, 0, 0) as u8 & 0x1f;
                let mode = param(params, 1, 1);
                let k = self.scr().kitty.last_mut().expect("kitty stack");
                *k = match mode {
                    1 => f,
                    2 => *k | f,
                    3 => *k & !f,
                    _ => *k,
                };
            }
            _ => {}
        }
    }

    fn esc_dispatch(&mut self, inter: &[u8], ignore: bool, byte: u8) {
        if ignore {
            return;
        }
        match (inter, byte) {
            ([], b'7') => self.save_cursor(),
            ([], b'8') => self.restore_cursor(),
            ([], b'D') => self.linefeed(),
            ([], b'E') => {
                self.cur().x = 0;
                self.linefeed();
            }
            ([], b'H') => {
                let x = self.scr().cursor.x as usize;
                self.tabs[x] = true;
            }
            ([], b'M') => self.reverse_index(),
            ([], b'c') => self.full_reset(),
            // DECID: as DA1.
            ([], b'Z') => self.reply(b"\x1b[?62;1;6;22;52c"),
            // Locking and single shifts (G2/G3 into GL).
            ([], b'n') => self.cur().gl = 2,
            ([], b'o') => self.cur().gl = 3,
            ([], b'N') => self.single_shift = Some(2),
            ([], b'O') => self.single_shift = Some(3),
            ([], b'=') => self.modes.app_keypad = true,
            ([], b'>') => self.modes.app_keypad = false,
            ([b'#'], b'8') => {
                let (cols, rows) = (self.cols, self.rows);
                for y in 0..rows {
                    for x in 0..cols {
                        self.line(y).cells[x as usize] = Cell { text: "E".into(), ..Default::default() };
                    }
                }
                self.goto(0, 0);
            }
            ([g @ (b'(' | b')' | b'*' | b'+')], set) => {
                let i = (*g - b'(') as usize;
                self.cur().charsets[i] = if set == b'0' { b'0' } else { b'B' };
            }
            _ => {}
        }
    }

    fn osc_dispatch(&mut self, params: &[&[u8]], bell: bool) {
        let Some(code) = params.first().and_then(|p| std::str::from_utf8(p).ok()) else { return };
        if matches!(code, "4" | "10" | "11" | "12" | "104" | "110" | "111" | "112") {
            // Colours of cells already drawn may change.
            self.all_dirty = true;
        }
        let text = |i: usize| params.get(i).map(|p| String::from_utf8_lossy(p).into_owned()).unwrap_or_default();
        match code {
            "0" | "2" => {
                // The whole rest, semicolons included.
                let t = params[1..].iter().map(|p| String::from_utf8_lossy(p)).collect::<Vec<_>>().join(";");
                self.title = t.clone();
                self.events.push(Event::Title(t));
            }
            "7" => {
                let p = text(1);
                self.pwd = p.clone();
                self.events.push(Event::Pwd(p));
            }
            "4" => {
                let mut i = 1;
                while i + 1 < params.len() {
                    let n: usize = text(i).parse().unwrap_or(usize::MAX);
                    let spec = text(i + 1);
                    if n < 256 {
                        if spec == "?" {
                            let c = self.palette[n];
                            self.color_reply(&format!("4;{n}"), c, bell);
                        } else if let Some(c) = parse_color(&spec) {
                            self.palette[n] = c;
                        }
                    }
                    i += 2;
                }
            }
            "10" | "11" | "12" => {
                let spec = text(1);
                let n: u8 = code.parse().unwrap_or(10);
                if spec == "?" {
                    let c = match n {
                        10 => self.fg,
                        11 => self.bg,
                        _ => self.cursor_color.unwrap_or(self.fg),
                    };
                    self.color_reply(code, c, bell);
                } else if let Some(c) = parse_color(&spec) {
                    match n {
                        10 => self.fg = c,
                        11 => self.bg = c,
                        _ => self.cursor_color = Some(c),
                    }
                }
            }
            "104" => {
                if params.len() <= 1 {
                    self.palette = palette256(&self.base.palette.unwrap_or(DEFAULT_16));
                } else {
                    let base = palette256(&self.base.palette.unwrap_or(DEFAULT_16));
                    for p in &params[1..] {
                        if let Some(n) = std::str::from_utf8(p).ok().and_then(|s| s.parse::<usize>().ok()) {
                            if n < 256 {
                                self.palette[n] = base[n];
                            }
                        }
                    }
                }
            }
            "110" => self.fg = self.base.fg,
            "111" => self.bg = self.base.bg,
            "112" => self.cursor_color = self.base.cursor,
            "52" => {
                let data = params.get(2).copied().unwrap_or(b"");
                if data != b"?" {
                    if let Some(t) = base64(data).and_then(|b| String::from_utf8(b).ok()) {
                        self.events.push(Event::ClipboardWrite(t));
                    }
                }
            }
            _ => {}
        }
    }
}

/// `#rgb`, `#rrggbb`, `rgb:r/g/b` (1–4 hex digits each).
pub fn parse_color(s: &str) -> Option<[u8; 3]> {
    if let Some(h) = s.strip_prefix('#') {
        let n = h.len() / 3;
        if n == 0 || h.len() % 3 != 0 || n > 4 {
            return None;
        }
        let part = |i: usize| scale_hex(&h[i * n..(i + 1) * n]);
        return Some([part(0)?, part(1)?, part(2)?]);
    }
    if let Some(r) = s.strip_prefix("rgb:") {
        let v: Vec<&str> = r.split('/').collect();
        if v.len() == 3 {
            return Some([scale_hex(v[0])?, scale_hex(v[1])?, scale_hex(v[2])?]);
        }
    }
    None
}

fn scale_hex(h: &str) -> Option<u8> {
    if h.is_empty() || h.len() > 4 {
        return None;
    }
    let v = u32::from_str_radix(h, 16).ok()?;
    let max = (1u32 << (4 * h.len())) - 1;
    Some(((v * 255 + max / 2) / max) as u8)
}

fn base64(s: &[u8]) -> Option<Vec<u8>> {
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32)
    };
    let mut out = Vec::new();
    let mut acc = 0u32;
    let mut bits = 0;
    for &c in s {
        if c == b'=' || c == b'\n' || c == b'\r' {
            continue;
        }
        acc = (acc << 6) | val(c)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn term(cols: u16, rows: u16) -> Terminal {
        Terminal::new(Options {
            cols,
            rows,
            cell_width: 9,
            cell_height: 18,
            colors: Colors { fg: [204; 3], bg: [20; 3], palette: None, cursor: None },
            dark: Some(true),
            xtversion: "haetty(test)".into(),
        })
    }

    #[test]
    fn prints_wraps_and_scrolls() {
        let mut t = term(4, 2);
        t.feed(b"abcdef\r\nxy");
        assert_eq!(t.cell(0, 0).text, "e");
        assert_eq!(t.cell(1, 0).text, "f");
        assert_eq!(t.cell(0, 1).text, "x");
        assert_eq!(t.cursor(), Some((2, 1)));
        assert_eq!(t.scrollback_rows(), 1);
    }

    #[test]
    fn pending_wrap_and_wide_at_the_edge() {
        let mut t = term(6, 3);
        t.feed(b"abcdef");
        assert_eq!(t.cursor(), Some((5, 0)));
        t.feed("\r".as_bytes());
        assert_eq!(t.cursor(), Some((0, 0)));
        let mut t = term(6, 3);
        t.feed("abcde가".as_bytes());
        assert_eq!(t.cell(5, 0).wide, Wide::SpacerHead);
        assert_eq!((t.cell(0, 1).text.as_str(), t.cell(0, 1).wide), ("가", Wide::Wide));
        assert_eq!(t.cursor(), Some((2, 1)));
    }

    #[test]
    fn replies_as_libghostty() {
        let mut t = term(20, 5);
        t.feed(b"\x1b[c\x1b[>c\x1b[6n\x1b[?2026$p\x1b]10;?\x07\x1b[18t\x1b[?996n");
        let r = String::from_utf8(t.take_replies()).unwrap();
        assert_eq!(
            r,
            "\x1b[?62;1;6;22;52c\x1b[>1;1;0c\x1b[1;1R\x1b[?2026;2$y\x1b]10;rgb:cccc/cccc/cccc\x07\x1b[8;5;20t\x1b[?997;1n"
        );
    }

    #[test]
    fn rows_report_changes() {
        let mut t = term(10, 4);
        assert_eq!(t.take_dirty(), vec![true; 4], "everything is new at first");
        assert_eq!(t.take_dirty(), vec![false; 4]);
        t.feed(b"\x1b[3;1Hx");
        assert_eq!(t.take_dirty(), vec![false, false, true, false]);
        t.feed(b"\x1b[2;3r\x1b[3;1H\n");
        assert_eq!(t.take_dirty(), vec![false, true, true, false], "the scrolled region");
        t.feed(b"\x1b[?1049h");
        assert_eq!(t.take_dirty(), vec![true; 4], "another screen");
        t.feed(b"\x1b]11;#102030\x07");
        assert_eq!(t.take_dirty(), vec![true; 4], "new colours");
    }

    /// Random bytes, biased towards escape sequences, with resizes and
    /// viewport scrolls between them: no panic, and the screen stays whole.
    #[test]
    fn random_input_does_not_panic() {
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut next = move || {
            seed ^= seed >> 12;
            seed ^= seed << 25;
            seed ^= seed >> 27;
            seed.wrapping_mul(0x2545_f491_4f6c_dd1d)
        };
        let pieces: [&[u8]; 16] = [
            b"\x1b[",
            b"\x1b]",
            b"\x1b",
            b";",
            b"?",
            b"\x07",
            b"\x1b\\",
            b"\r\n",
            "가".as_bytes(),
            "\u{301}".as_bytes(),
            "😀".as_bytes(),
            "\u{fe0f}".as_bytes(),
            b"\x1b_G",
            b"\x1bP",
            b"\x1b(0",
            b"99999",
        ];
        for round in 0..300 {
            let (c, r) = (1 + (next() % 120) as u16, 1 + (next() % 40) as u16);
            let mut t = term(c, r);
            let mut buf = Vec::new();
            for _ in 0..2000 {
                match next() % 4 {
                    0 => buf.extend_from_slice(pieces[(next() % 16) as usize]),
                    1 => buf.extend_from_slice(format!("{}", next() % 3000).as_bytes()),
                    _ => buf.push((next() % 256) as u8),
                }
            }
            for part in buf.chunks(1 + (next() % 700) as usize) {
                t.feed(part);
                match next() % 8 {
                    0 => t.resize(1 + (next() % 150) as u16, 1 + (next() % 50) as u16, 9, 18),
                    1 => t.scroll_viewport_delta((next() % 41) as isize - 20),
                    2 => t.scroll_viewport_top(),
                    _ => {}
                }
                let (c, r) = t.size();
                for y in 0..r {
                    for x in 0..c {
                        let _ = t.cell(x, y);
                    }
                }
                let _ = (t.cursor(), t.take_replies(), t.take_events(), t.take_dirty());
            }
            assert!(t.size().0 >= 1 && t.size().1 >= 1, "round {round}");
        }
    }

    #[test]
    fn colours_parse() {
        assert_eq!(parse_color("#fff"), Some([255; 3]));
        assert_eq!(parse_color("rgb:ffff/0/80"), Some([255, 0, 0x80]));
        assert_eq!(base64(b"aGk="), Some(b"hi".to_vec()));
    }
}
