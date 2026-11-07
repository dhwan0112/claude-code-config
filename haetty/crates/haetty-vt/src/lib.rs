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

pub mod color;
mod grapheme;
pub mod grapheme_table;
mod width;
pub mod width_table;
mod x11_colors;

use std::collections::VecDeque;

pub use color::{parse_color, parse_palette_entry};
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

/// One cell of the screen: 16 bytes, `Copy` (DESIGN.md §10.3, §10.7).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cell {
    /// 0: empty; below 0x110000: that character alone; with the top bit
    /// set: a grapheme cluster in the terminal's table (`Terminal::text`).
    content: u32,
    pub style: Style,
    pub wide: Wide,
    /// Written under SPA or DECSCA: erasure that respects protection
    /// leaves it.
    pub protected: bool,
}

const CLUSTER: u32 = 0x8000_0000;

impl Cell {
    fn of(c: char, style: Style, wide: Wide) -> Cell {
        Cell { content: c as u32, style, wide, protected: false }
    }

    pub fn is_empty(&self) -> bool {
        self.content == 0
    }
}

#[derive(Clone, Debug, Default)]
struct Line {
    cells: Vec<Cell>,
    /// The line continues on the next one (soft wrap).
    wrapped: bool,
    /// Changed since the frontend last took the changes (`take_dirty`).
    dirty: bool,
    /// The line continues the one above (libghostty's `wrap_continuation`,
    /// cleared on its own by line insertion and deletion).
    cont: bool,
}

impl Line {
    fn new(cols: u16, bg: Style) -> Line {
        Line { cells: vec![blank(bg); cols as usize], wrapped: false, dirty: true, cont: false }
    }
}

/// An erased cell: empty, keeping the background colour (as libghostty).
fn blank(s: Style) -> Cell {
    Cell { content: 0, style: Style { fg: Color::Default, bg: s.bg, flags: 0 }, wide: Wide::Narrow, protected: false }
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
    /// DECSCUSR's shape; it goes with the cursor (saved, restored, copied
    /// between screens), as in libghostty.
    shape: CursorShape,
    /// SPA / DECSCA: characters written now are protected.
    protected: bool,
}

#[derive(Clone, Copy, Debug)]
struct Saved {
    cursor: Cursor,
    origin: bool,
}

#[derive(Clone, Debug)]
struct Screen {
    lines: Vec<Line>,
    cursor: Cursor,
    saved: Option<Saved>,
    /// Kitty keyboard flags stack (each screen has its own).
    kitty: Vec<u8>,
    /// The last protection set: 0 none yet, 1 ISO (SPA: erasure respects
    /// it), 2 DEC (DECSCA: only selective erasure does). Never back to 0.
    protected_mode: u8,
}

impl Screen {
    fn new(cols: u16, rows: u16) -> Screen {
        Screen {
            lines: (0..rows).map(|_| Line::new(cols, Style::default())).collect(),
            cursor: Cursor { charsets: [b'B'; 4], ..Default::default() },
            saved: None,
            kitty: vec![0],
            protected_mode: 0,
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
    /// DECLRMM (69): left/right margins allowed (DECSLRM itself is not implemented).
    pub lr_margins: bool,
    /// Other modes libghostty knows, kept for DECRQM only (`OTHER_MODES`).
    pub other: [bool; OTHER_MODES.len()],
    /// 47, 1047 and 1049 as set (DECRQM reports each).
    pub alt_modes: [bool; 3],
    /// xterm modifyOtherKeys state 2 (`CSI > 4 ; 2 m`).
    pub modify_other_keys_2: bool,
    /// Mouse reporting as libghostty keeps it, one value each (the last mode
    /// set wins; resetting any goes back to none / X10): event 0 none, 1 X10,
    /// 2 normal, 3 button, 4 any; format 0 X10, 1 UTF-8, 2 SGR, 3 urxvt,
    /// 4 SGR pixels.
    pub mouse_event: u8,
    pub mouse_format: u8,
}

/// Modes libghostty answers DECRQM for that haetty-vt only keeps (no
/// behaviour), with their defaults: DECCOLM, DECSCLM, DECARM, 80/132,
/// reverse wrap, DECBKM, alternate scroll, the meta/alt keys, extended
/// reverse wrap, 1048's save state.
pub const OTHER_MODES: [(u16, bool); 12] = [
    (3, false),
    (4, false),
    (8, false),
    (40, false),
    (45, false),
    (67, false),
    (1007, true),
    (1035, true),
    (1036, true),
    (1039, false),
    (1045, false),
    (1048, false),
];

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
            lr_margins: false,
            other: OTHER_MODES.map(|m| m.1),
            alt_modes: [false; 3],
            modify_other_keys_2: false,
            mouse_event: 0,
            mouse_format: 0,
        }
    }
}

impl Modes {
    /// As libghostty's `mouse_tracking`: any of the four modes set (the
    /// encoder goes by `mouse_event`, which only the last one sets).
    pub fn mouse_tracking(&self) -> bool {
        self.mouse_x10 || self.mouse_normal || self.mouse_button || self.mouse_any
    }

    /// A mode from `OTHER_MODES` (DECRQM-only modes).
    pub fn other(&self, mode: u16) -> bool {
        OTHER_MODES.iter().position(|m| m.0 == mode).is_some_and(|i| self.other[i])
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
    /// Scrollback limit in bytes.
    pub scrollback_limit: usize,
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
    dark: Option<bool>,
    xtversion: String,
    parser: vte::Parser,
    pre: Scanner,
    /// SS2/SS3: the next character from G2/G3.
    single_shift: Option<u8>,
    /// What REP repeats.
    last_char: Option<char>,
    /// The C1 control vte is executing came as a raw byte (see `execute`).
    raw_c1: bool,
    /// The C0 control vte is executing is in the ground state (see `execute`).
    ground_c0: bool,
    /// Parameters of the CSI being dispatched as libghostty counts them:
    /// `CSI s` has none, `CSI 0 s` and `CSI ; s` one, `CSI 5 ; B` one
    /// (vte would say 1, 1, 2 and 2).
    csi_count: usize,
    csi_colon: bool,
    /// Its parameter bytes.
    csi_raw: Vec<u8>,
    replies: Vec<u8>,
    events: Vec<Event>,
    /// Lines of scrollback the viewport is above the live screen.
    viewport: usize,
    /// Grapheme clusters cells refer to (`Cell::content` with the top bit),
    /// each once.
    clusters: Vec<Box<str>>,
    cluster_ids: std::collections::HashMap<Box<str>, u32>,
    /// What the scrollback holds and may hold, in bytes (as Ghostty's
    /// `scrollback-limit`; DESIGN.md §10.3).
    sb_bytes: usize,
    sb_limit: usize,
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
            dark: o.dark,
            xtversion: o.xtversion,
            parser: vte::Parser::new(),
            pre: Scanner::default(),
            single_shift: None,
            last_char: None,
            raw_c1: false,
            ground_c0: false,
            csi_count: 0,
            csi_colon: false,
            csi_raw: Vec::new(),
            replies: Vec::new(),
            events: Vec::new(),
            viewport: 0,
            all_dirty: true,
            clusters: Vec::new(),
            cluster_ids: std::collections::HashMap::new(),
            sb_bytes: 0,
            sb_limit: o.scrollback_limit,
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
                Pre::CsiEnd(count, colon) => {
                    parser.advance(self, &bytes[start..i]);
                    self.csi_count = count;
                    self.csi_colon = colon;
                    std::mem::swap(&mut self.csi_raw, &mut self.pre.csi_raw);
                    parser.advance(self, &bytes[i..=i]);
                    start = i + 1;
                }
                Pre::C1(c) => {
                    parser.advance(self, &bytes[start..i]);
                    parser.advance(self, &[0x1b]);
                    if let Some(apc) = self.pre.apc_done.take() {
                        self.apc_dispatch(&apc);
                    }
                    parser.advance(self, &[c - 0x40]);
                    start = i + 1;
                }
                Pre::RawC1 => {
                    parser.advance(self, &bytes[start..i]);
                    self.raw_c1 = true;
                    parser.advance(self, &bytes[i..=i]);
                    self.raw_c1 = false;
                    start = i + 1;
                }
                Pre::GroundC0 => {
                    parser.advance(self, &bytes[start..i]);
                    self.ground_c0 = true;
                    parser.advance(self, &bytes[i..=i]);
                    self.ground_c0 = false;
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
        // As libghostty: a C0 control inside makes the command void.
        if apc.iter().any(|&c| c < 0x20) {
            return;
        }
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
        self.screen_ref().cursor.shape
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
    /// A cell of the visible viewport (scrollback rows keep no trailing
    /// blanks: past their end a cell is blank).
    pub fn cell(&self, x: u16, y: u16) -> Cell {
        self.view_line(y).cells.get(x as usize).copied().unwrap_or_default()
    }

    /// A cell's text (empty for an empty cell).
    pub fn text(&self, c: &Cell) -> String {
        match c.content {
            0 => String::new(),
            v if v & CLUSTER != 0 => self.clusters[(v & !CLUSTER) as usize].to_string(),
            v => char::from_u32(v).map(String::from).unwrap_or_default(),
        }
    }

    pub fn text_at(&self, x: u16, y: u16) -> String {
        self.text(&self.cell(x, y))
    }

    fn first_char(&self, c: &Cell) -> Option<char> {
        match c.content {
            0 => None,
            v if v & CLUSTER != 0 => self.clusters[(v & !CLUSTER) as usize].chars().next(),
            v => char::from_u32(v),
        }
    }

    /// The cell's text with `c` after it, as the cell's new content.
    fn joined(&mut self, cell: &Cell, c: char) -> u32 {
        let mut t = self.text(cell);
        t.push(c);
        let t: Box<str> = t.into();
        if let Some(&id) = self.cluster_ids.get(&t) {
            return id | CLUSTER;
        }
        let id = self.clusters.len() as u32;
        self.clusters.push(t.clone());
        self.cluster_ids.insert(t, id);
        id | CLUSTER
    }

    /// A line goes to the scrollback: a hard line end loses its trailing
    /// blanks; the oldest lines go while the scrollback is over its limit.
    fn push_scrollback(&mut self, mut l: Line) {
        if !l.wrapped {
            while l.cells.last().is_some_and(|c| *c == Cell::default()) {
                l.cells.pop();
            }
        }
        l.cells.shrink_to_fit();
        self.sb_bytes += line_bytes(&l);
        self.scrollback.push_back(l);
        self.evict();
    }

    fn evict(&mut self) {
        while self.sb_bytes > self.sb_limit {
            let Some(old) = self.scrollback.pop_front() else { break };
            self.sb_bytes -= line_bytes(&old);
        }
        self.viewport = self.viewport.min(self.scrollback.len());
    }

    /// Bytes the scrollback holds now.
    pub fn scrollback_bytes(&self) -> usize {
        self.sb_bytes
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

    /// A new size. As libghostty (`PageList.resize`): with wider columns the
    /// lines reflow first and the rows change after, with narrower ones the
    /// other way round; lines reflow only on the primary screen and only
    /// while autowrap is on. The cursor and the saved cursor follow their
    /// cells; one that ends in the scrollback goes to the top left.
    pub fn resize(&mut self, cols: u16, rows: u16, cell_width: u16, cell_height: u16) {
        self.all_dirty = true;
        let (cols, rows) = (cols.max(1), rows.max(1));
        self.cell = (cell_width, cell_height);
        if (cols, rows) == (self.cols, self.rows) {
            return;
        }
        let (old_cols, old_rows) = (self.cols, self.rows);
        let reflow = self.modes.autowrap;
        // As libghostty, a resize ends synchronized output.
        self.modes.sync_output = false;
        // Primary screen, with its scrollback.
        let mut sb = std::mem::take(&mut self.scrollback);
        resize_screen(&mut self.primary, Some(&mut sb), old_cols, old_rows, cols, rows, reflow);
        self.sb_bytes = 0;
        for l in sb {
            self.push_scrollback(l);
        }
        // Alternate screen: no reflow, no scrollback.
        resize_screen(&mut self.alt, None, old_cols, old_rows, cols, rows, false);
        self.cols = cols;
        self.rows = rows;
        self.top = 0;
        self.bottom = rows - 1;
        if cols != old_cols {
            self.tabs = default_tabs(cols);
        }
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

    /// As libghostty's insert/delete lines: shifted rows lose their soft
    /// wraps, and a spacer head at the end becomes a plain cell.
    fn unwrap_rows(&mut self, top: u16, bottom: u16) {
        for l in &mut self.scr().lines[top as usize..=bottom as usize] {
            l.wrapped = false;
            l.cont = false;
            if let Some(c) = l.cells.last_mut() {
                if c.wide == Wide::SpacerHead {
                    c.wide = Wide::Narrow;
                }
            }
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
        if self.modes.grapheme_cluster && self.cluster(c) {
            return;
        }
        let w = width(c);
        if w == 0 {
            // Under 2027 a zero-width character that starts a cluster is
            // dropped (as libghostty).
            if !self.modes.grapheme_cluster {
                self.combine(c);
            }
            return;
        }
        let c = {
            let cur = self.scr().cursor;
            let g = self.single_shift.take().unwrap_or(cur.gl);
            if cur.charsets[g as usize & 3] == b'0' {
                dec_special(c)
            } else {
                c
            }
        };
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
            self.line(y).cells[x as usize] = Cell { content: 0, style, wide: Wide::SpacerHead, protected: false };
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
        let protected = self.screen_ref().cursor.protected;
        let l = self.line(y);
        l.cells[x as usize] = Cell { protected, ..Cell::of(c, style, if w == 2 { Wide::Wide } else { Wide::Narrow }) };
        if w == 2 {
            l.cells[x as usize + 1] = Cell { content: 0, style, wide: Wide::SpacerTail, protected: false };
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

    /// Mode 2027, as libghostty's `print`: a character that does not start a
    /// new grapheme cluster joins the cell before the cursor, which becomes
    /// wide (VS16, a part that has width of its own) or narrow (VS15). True
    /// when the character was taken (joined, or dropped as a variation
    /// selector the base does not have).
    fn cluster(&mut self, c: char) -> bool {
        let (cols, autowrap) = (self.cols, self.modes.autowrap);
        let cur = self.scr().cursor;
        if (c as u32) <= 0xff || cur.x == 0 {
            return false;
        }
        let y = cur.y as usize;
        let left: u16 = if autowrap {
            (!cur.pending_wrap) as u16
        } else if cur.x != cols - 1 {
            1
        } else {
            self.screen_ref().lines[y].cells[cur.x as usize].is_empty() as u16
        };
        let mut px = cur.x - left;
        let mut pleft = left;
        if self.screen_ref().lines[y].cells[px as usize].wide == Wide::SpacerTail && px > 0 {
            px -= 1;
            pleft += 1;
        }
        let prev_cell = self.screen_ref().lines[y].cells[px as usize];
        let text = self.text(&prev_cell);
        let mut chars = text.chars();
        let Some(first) = chars.next() else { return false };
        let mut state = grapheme::State::Default;
        let mut prev = first;
        for c2 in chars {
            grapheme::breaks(grapheme::class(prev), grapheme::class(c2), &mut state);
            prev = c2;
        }
        if grapheme::breaks(grapheme::class(prev), grapheme::class(c), &mut state) {
            return false;
        }
        let pen = self.scr().cursor.style;
        // Where the cluster is now (it moves when it wraps).
        let (mut cx, mut cy) = (px, cur.y);
        if matches!(c, '\u{fe0e}' | '\u{fe0f}') && !width::vs_base(prev) {
            return true;
        }
        let wide = if matches!(c, '\u{fe0e}' | '\u{fe0f}') {
            Some(c == '\u{fe0f}')
        } else {
            (!zero_in_grapheme(c)).then_some(true)
        };
        match wide {
            Some(true) if self.screen_ref().lines[y].cells[px as usize].wide != Wide::Wide => {
                self.cur().x = px;
                self.cur().pending_wrap = false;
                if px == cols - 1 {
                    if !autowrap {
                        return true;
                    }
                    // No room: the cluster goes to the next row, wide.
                    let old = self.line(cur.y).cells[px as usize];
                    let head_style = if text.chars().count() > 1 { old.style } else { pen };
                    self.line(cur.y).cells[px as usize] =
                        Cell { content: 0, style: head_style, wide: Wide::SpacerHead, protected: false };
                    self.wrap_line();
                    let ny = self.scr().cursor.y;
                    self.clear_wide_at(0, ny);
                    self.line(ny).cells[0] =
                        Cell { content: prev_cell.content, style: pen, wide: Wide::Wide, protected: false };
                    cx = 0;
                    cy = ny;
                } else {
                    self.line(cur.y).cells[px as usize].wide = Wide::Wide;
                }
                let x = self.scr().cursor.x + 1;
                let yy = self.scr().cursor.y;
                self.clear_wide_at(x, yy);
                self.line(yy).cells[x as usize] =
                    Cell { content: 0, style: pen, wide: Wide::SpacerTail, protected: false };
                if x == cols - 1 {
                    self.cur().x = x;
                    self.cur().pending_wrap = true;
                } else {
                    self.cur().x = x + 1;
                }
            }
            Some(false) if self.screen_ref().lines[y].cells[px as usize].wide == Wide::Wide => {
                let l = self.line(cur.y);
                l.cells[px as usize].wide = Wide::Narrow;
                let tail = (cur.x + 1 - pleft) as usize;
                if tail < l.cells.len() {
                    l.cells[tail].wide = Wide::Narrow;
                }
                if cur.x == cols - 1 {
                    self.cur().pending_wrap = false;
                } else {
                    self.cur().x = cur.x - 1;
                }
            }
            _ => {}
        }
        let cell = self.screen_ref().lines[cy as usize].cells[cx as usize];
        let joined = self.joined(&cell, c);
        self.line(cy).cells[cx as usize].content = joined;
        true
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
        if l.cells[i].is_empty() {
            return;
        }
        // A variation selector only after a character that has one (as
        // libghostty: the table is read off it).
        let cell = l.cells[i];
        if matches!(c, '\u{fe0e}' | '\u{fe0f}') && !self.first_char(&cell).is_some_and(width::vs_base) {
            return;
        }
        let joined = self.joined(&cell, c);
        self.line(cur.y).cells[i].content = joined;
    }

    /// Writing over half of a wide character clears the other half.
    fn clear_wide_at(&mut self, x: u16, y: u16) {
        let cols = self.cols as usize;
        let i = x as usize;
        if i >= cols {
            return;
        }
        // In the pen's background, as libghostty.
        let pen = self.scr().cursor.style;
        // A wide character in the first column that came from the row above:
        // that row's spacer head goes too.
        if i == 0 && y > 0 && self.screen_ref().lines[y as usize].cells[0].wide == Wide::Wide {
            if let Some(c) = self.line(y - 1).cells.last_mut() {
                if c.wide == Wide::SpacerHead {
                    c.wide = Wide::Narrow;
                }
            }
        }
        let l = self.line(y);
        match l.cells[i].wide {
            Wide::Wide if i + 1 < cols => l.cells[i + 1] = blank(pen),
            Wide::SpacerTail if i > 0 => l.cells[i - 1] = blank(pen),
            _ => {}
        }
    }

    fn wrap_line(&mut self) {
        let y = self.scr().cursor.y;
        self.line(y).wrapped = true;
        self.cur().x = 0;
        self.cur().pending_wrap = false;
        self.linefeed();
        let y = self.scr().cursor.y;
        self.line(y).cont = true;
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
                self.push_scrollback(l);
            }
            self.scr().lines.insert(bottom, Line::new(cols, bg));
        }
        if to_scrollback && self.viewport > 0 {
            self.viewport = (self.viewport + n).min(self.scrollback.len());
        }
        self.moved(top as u16, bottom as u16);
        // A region from the top scrolls as lines leaving the screen do;
        // any other is a deletion of lines.
        let full = top == 0 && (!self.modes.alt_screen || bottom == self.rows as usize - 1);
        if !full {
            self.unwrap_rows(top as u16, bottom as u16);
        }
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
        self.unwrap_rows(top as u16, bottom as u16);
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

    /// Under ISO protection (SPA last), ECH leaves protected cells.
    fn erase_chars(&mut self, n: u16) {
        let protect = self.scr().protected_mode == 1;
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
            if !(protect && c.protected) {
                *c = blank(bg);
            }
        }
    }

    /// `selective`: DECSEL (`CSI ? K`), which leaves protected cells; so does
    /// any EL after SPA (ISO protection), as in libghostty.
    fn erase_line(&mut self, mode: u16, selective: bool) {
        let protect = selective || self.scr().protected_mode == 1;
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
            if !(protect && c.protected) {
                *c = blank(bg);
            }
        }
        if mode != 1 {
            l.wrapped = false;
        }
        self.cur().pending_wrap = false;
    }

    /// Rows `from..to` blank (protected cells stay when `protect`).
    fn erase_rows(&mut self, from: u16, to: u16, bg: Style, protect: bool) {
        let cols = self.cols;
        for r in from..to {
            if protect {
                for c in &mut self.line(r).cells {
                    if !c.protected {
                        *c = blank(bg);
                    }
                }
            } else {
                *self.line(r) = Line::new(cols, bg);
            }
        }
    }

    fn erase_display(&mut self, mode: u16, selective: bool) {
        let protect = selective || self.scr().protected_mode == 1;
        let (y, bg) = {
            let c = self.scr().cursor;
            (c.y, c.style)
        };
        let cols = self.cols;
        match mode {
            0 => {
                self.erase_line(0, selective);
                self.erase_rows(y + 1, self.rows, bg, protect);
            }
            1 => {
                self.erase_rows(0, y, bg, protect);
                self.erase_line(1, selective);
            }
            2 => self.erase_rows(0, self.rows, bg, protect),
            3 => {
                if !self.modes.alt_screen {
                    self.scrollback.clear();
                    self.sb_bytes = 0;
                    self.viewport = 0;
                }
            }
            // libghostty's "scroll complete": the rows down to the last one
            // with content go to the scrollback, the screen is cleared.
            22 => {
                let used = |l: &Line| l.cells.iter().any(|c| !c.is_empty() || c.style.bg != Color::Default);
                let n = self.screen_ref().lines.iter().rposition(used).map_or(0, |r| r + 1);
                if !self.modes.alt_screen {
                    for r in 0..n {
                        let l = self.screen_ref().lines[r].clone();
                        self.push_scrollback(l);
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
        self.unwrap_rows(y, self.bottom);
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
        self.unwrap_rows(y, self.bottom);
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
        let s = Saved { cursor: self.scr().cursor, origin: self.modes.origin };
        self.scr().saved = Some(s);
    }

    fn restore_cursor(&mut self) {
        let s = self
            .scr()
            .saved
            .unwrap_or(Saved { cursor: Cursor { charsets: [b'B'; 4], ..Default::default() }, origin: false });
        let (cols, rows) = (self.cols, self.rows);
        let cur = self.cur();
        // The shape is not part of what DECSC saves.
        let shape = cur.shape;
        *cur = s.cursor;
        cur.shape = shape;
        cur.x = cur.x.min(cols - 1);
        cur.y = cur.y.min(rows - 1);
        self.modes.origin = s.origin;
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
        if on && self.modes.alt_screen && save {
            // As libghostty: 1049 again saves the cursor and erases the
            // screen (in the pen's background; entering gives a fresh one).
            self.save_cursor();
            let (cols, rows, pen) = (self.cols, self.rows, self.scr().cursor.style);
            self.alt.lines = (0..rows).map(|_| Line::new(cols, pen)).collect();
            self.all_dirty = true;
            return;
        }
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
            // As libghostty: 1049 erases on the way in only; 1047 also out.
            if clear && !save {
                let (cols, rows) = (self.cols, self.rows);
                self.alt.lines = (0..rows).map(|_| Line::new(cols, Style::default())).collect();
            }
            // As libghostty: without a saved cursor (47, 1047) the cursor
            // stays where it is on the alternate screen.
            let alt_cursor = self.alt.cursor;
            self.modes.alt_screen = false;
            if save {
                self.restore_cursor();
            } else {
                self.primary.cursor = alt_cursor;
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
            scrollback_limit: self.sb_limit,
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
            9 => {
                self.modes.mouse_x10 = on;
                self.modes.mouse_event = if on { 1 } else { 0 };
            }
            12 => self.modes.cursor_blink = on,
            25 => self.modes.cursor_visible = on,
            47 => {
                self.modes.alt_modes[0] = on;
                self.set_alt_screen(on, false, false)
            }
            66 => self.modes.app_keypad = on,
            69 => self.modes.lr_margins = on,
            1000 => {
                self.modes.mouse_normal = on;
                self.modes.mouse_event = if on { 2 } else { 0 };
            }
            1002 => {
                self.modes.mouse_button = on;
                self.modes.mouse_event = if on { 3 } else { 0 };
            }
            1003 => {
                self.modes.mouse_any = on;
                self.modes.mouse_event = if on { 4 } else { 0 };
            }
            1004 => self.modes.focus_events = on,
            1005 => {
                self.modes.mouse_utf8 = on;
                self.modes.mouse_format = if on { 1 } else { 0 };
            }
            1006 => {
                self.modes.mouse_sgr = on;
                self.modes.mouse_format = if on { 2 } else { 0 };
            }
            1015 => {
                self.modes.mouse_urxvt = on;
                self.modes.mouse_format = if on { 3 } else { 0 };
            }
            1016 => {
                self.modes.mouse_sgr_pixels = on;
                self.modes.mouse_format = if on { 4 } else { 0 };
            }
            1047 => {
                self.modes.alt_modes[1] = on;
                self.set_alt_screen(on, false, true)
            }
            1048 => {
                if on {
                    self.save_cursor()
                } else {
                    self.restore_cursor()
                }
            }
            // As libghostty, leaving restores the cursor even when the
            // alternate screen was not on.
            1049 if !on && !self.modes.alt_screen => {
                self.modes.alt_modes[2] = false;
                self.restore_cursor()
            }
            1049 => {
                self.modes.alt_modes[2] = on;
                self.set_alt_screen(on, true, true)
            }
            2004 => self.modes.bracketed_paste = on,
            2026 => self.modes.sync_output = on,
            2027 => self.modes.grapheme_cluster = on,
            2031 => self.modes.color_scheme_report = on,
            // Reports come with resizes only (as libghostty, none on enabling).
            2048 => self.modes.in_band_resize = on,
            _ => {}
        }
        if let Some(i) = OTHER_MODES.iter().position(|m| m.0 == mode) {
            self.modes.other[i] = on;
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
            47 => m.alt_modes[0],
            1047 => m.alt_modes[1],
            1049 => m.alt_modes[2],
            66 => m.app_keypad,
            69 => m.lr_margins,
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
            _ => match OTHER_MODES.iter().position(|m| m.0 == mode) {
                Some(i) => m.other[i],
                None => return 0,
            },
        };
        if v {
            1
        } else {
            2
        }
    }

    // ------------------------------------------------------------ SGR

    /// SGR, parsed from the raw parameter bytes (`csi_raw`): libghostty tells
    /// an empty field from a 0 (`48:5:` is void, `38:2::R:G:B` is not),
    /// which vte's parameters cannot.
    fn sgr(&mut self) {
        let groups = sgr_groups(&self.csi_raw);
        let mut style = self.scr().cursor.style;
        if groups.is_empty() {
            style = Style::default();
        }
        let mut i = 0;
        while i < groups.len() {
            let g = &groups[i];
            i += 1;
            let v = g[0].unwrap_or(0);
            if g.len() > 1 {
                if matches!(v, 38 | 48 | 58) {
                    let (c, rest) = colon_colour(g);
                    set_colour(&mut style, v, c);
                    // As libghostty: what the colon form leaves over counts
                    // as an SGR of its own when it is one value.
                    if let [Some(r)] = rest {
                        sgr_simple(&mut style, &[*r]);
                    }
                } else if v == 4 && g.len() == 2 {
                    sgr_simple(&mut style, &[4, g[1].unwrap_or(0)]);
                }
                // Sub-parameters on anything else make it void.
                continue;
            }
            if matches!(v, 38 | 48 | 58) {
                // `38;5;N`, `38;2;R;G;B`; with too few values or another kind
                // only the 38 goes, as libghostty.
                let val = |k: usize| groups.get(i + k).map(|g| g[0].unwrap_or(0));
                match val(0) {
                    Some(5) if val(1).is_some() => {
                        set_colour(&mut style, v, Some(Color::Index(val(1).unwrap_or(0) as u8)));
                        i += 2;
                    }
                    Some(2) if val(3).is_some() => {
                        let c = [val(1), val(2), val(3)].map(|x| x.unwrap_or(0) as u8);
                        set_colour(&mut style, v, Some(Color::Rgb(c)));
                        i += 4;
                    }
                    _ => {}
                }
                continue;
            }
            sgr_simple(&mut style, &[v]);
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
    /// A CSI's final byte, with its parameter count (see `csi_count`) and
    /// whether a colon was among the parameters.
    CsiEnd(usize, bool),
    /// An APC string (`ESC _ … ESC \`); its contents are in `buf`.
    Apc,
    /// A byte 0x80–0x9F outside a UTF-8 sequence.
    RawC1,
    /// A C0 byte 0x10–0x1F (not ESC) in the ground state.
    GroundC0,
    /// A C1 byte inside an escape or CSI sequence: vte gets its 7-bit form.
    C1(u8),
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
    csi_colon: bool,
    /// The CSI's parameter bytes (for SGR, see `Terminal::sgr`).
    csi_raw: Vec<u8>,
    /// An APC a C1 byte ended (dispatched by `feed`).
    apc_done: Option<Vec<u8>>,
    /// UTF-8 continuation bytes still expected in the ground state, and the
    /// range the next one must be in.
    utf8_need: u8,
    utf8_next: (u8, u8),
    buf: Vec<u8>,
}

impl Scanner {
    fn keep_param(&mut self, b: u8) {
        if self.csi_raw.len() < 512 {
            self.csi_raw.push(b);
        }
    }

    /// Follows UTF-8 in the ground state as vte's decoder does, to tell a raw
    /// C1 byte from a continuation byte.
    fn ground(&mut self, b: u8) -> Pre {
        if self.utf8_need > 0 {
            if (self.utf8_next.0..=self.utf8_next.1).contains(&b) {
                self.utf8_need -= 1;
                self.utf8_next = (0x80, 0xbf);
                return Pre::None;
            }
            self.utf8_need = 0;
        }
        match b {
            0x1b => self.state = Self::ESC,
            0x10..=0x1f => return Pre::GroundC0,
            0x80..=0x9f => return Pre::RawC1,
            0xc2..=0xdf => (self.utf8_need, self.utf8_next) = (1, (0x80, 0xbf)),
            0xe0 => (self.utf8_need, self.utf8_next) = (2, (0xa0, 0xbf)),
            0xed => (self.utf8_need, self.utf8_next) = (2, (0x80, 0x9f)),
            0xe1..=0xef => (self.utf8_need, self.utf8_next) = (2, (0x80, 0xbf)),
            0xf0 => (self.utf8_need, self.utf8_next) = (3, (0x90, 0xbf)),
            0xf1..=0xf3 => (self.utf8_need, self.utf8_next) = (3, (0x80, 0xbf)),
            0xf4 => (self.utf8_need, self.utf8_next) = (3, (0x80, 0x8f)),
            _ => {}
        }
        Pre::None
    }

    const GROUND: u8 = 0;
    const ESC: u8 = 1;
    const APC: u8 = 2;
    /// After ESC and an intermediate byte (`ESC (`, `ESC #` …).
    const ESC_INT: u8 = 3;
    const CSI: u8 = 4;
    /// SOS or PM string (`ESC X`, `ESC ^`): skipped.
    const STR: u8 = 5;
    /// DCS, its header and data.
    const DCS: u8 = 6;
    /// Only the control part matters; the payload is not kept past this.
    const KEEP: usize = 4096;

    fn byte(&mut self, b: u8) -> Pre {
        // As libghostty (a VT500 parser): inside an escape or CSI sequence a
        // C1 byte acts as its 7-bit form, ESC and b - 0x40 (vte ignores it).
        // The same in SOS/PM/APC strings and DCS (not in OSC, whose
        // payload is UTF-8).
        if matches!(self.state, Self::ESC | Self::ESC_INT | Self::CSI | Self::STR | Self::DCS | Self::APC)
            && (0x80..=0x9f).contains(&b)
        {
            // An APC ends there and is dispatched (as libghostty).
            if self.state == Self::APC {
                self.apc_done = Some(std::mem::take(&mut self.buf));
            }
            self.state = Self::ESC;
            let _ = self.byte(b - 0x40);
            return Pre::C1(b);
        }
        match (self.state, b) {
            (Self::GROUND, _) => return self.ground(b),
            (Self::ESC | Self::ESC_INT | Self::CSI | Self::STR | Self::DCS, 0x1b) => self.state = Self::ESC,
            (Self::ESC | Self::ESC_INT | Self::CSI | Self::STR | Self::DCS, 0x18 | 0x1a) => self.state = Self::GROUND,
            (Self::STR | Self::DCS, _) => {}
            (Self::ESC, b'X' | b'^') => self.state = Self::STR,
            (Self::ESC, b'P') => self.state = Self::DCS,
            (Self::ESC, b'_') => {
                self.state = Self::APC;
                self.buf.clear();
            }
            (Self::ESC, b'[') => {
                self.state = Self::CSI;
                self.csi_seps = 0;
                self.csi_trailing = false;
                self.csi_colon = false;
                self.csi_raw.clear();
            }
            (Self::ESC | Self::ESC_INT, 0x20..=0x2f) => self.state = Self::ESC_INT,
            (Self::ESC | Self::ESC_INT, 0x30..=0x7e) => self.state = Self::GROUND,
            // C0 controls execute inside the sequence; DEL and 0xA0–0xFF
            // are ignored.
            (Self::ESC | Self::ESC_INT, _) => {}
            (Self::CSI, b';') => {
                self.csi_seps += 1;
                self.csi_trailing = false;
                self.keep_param(b);
            }
            (Self::CSI, b':') => {
                self.csi_trailing = true;
                self.csi_colon = true;
                self.keep_param(b);
            }
            (Self::CSI, 0x30..=0x39) => {
                self.csi_trailing = true;
                self.keep_param(b);
            }
            (Self::CSI, 0x40..=0x7e) => {
                self.state = Self::GROUND;
                return Pre::CsiEnd(self.csi_seps + self.csi_trailing as usize, self.csi_colon);
            }
            (Self::CSI, _) => {}
            // As libghostty, the ESC that starts the terminator ends the APC
            // (whatever follows it).
            (Self::APC, 0x1b) => {
                self.state = Self::ESC;
                return Pre::Apc;
            }
            (Self::APC, 0x18 | 0x1a) => self.state = Self::GROUND,
            (Self::APC, _) if self.buf.len() < Self::KEEP => self.buf.push(b),
            _ => {}
        }
        Pre::None
    }
}

/// Takes no width inside a grapheme cluster (uucode's
/// `wcwidth_zero_in_grapheme`: zero-width characters, emoji modifiers,
/// Prepend).
fn zero_in_grapheme(c: char) -> bool {
    width(c) == 0 || matches!(grapheme::class(c), grapheme::Gb::EmojiModifier | grapheme::Gb::Prepend)
}

/// The DEC special graphics set (`ESC ( 0`) for 0x60–0x7e.
fn dec_special(c: char) -> char {
    const MAP: [char; 31] = [
        '◆', '▒', '␉', '␌', '␍', '␊', '°', '±', '␤', '␋', '┘', '┐', '┌', '└', '┼', '⎺', '⎻', '─', '⎼', '⎽', '├', '┤',
        '┴', '┬', '│', '≤', '≥', 'π', '≠', '£', '·',
    ];
    match c {
        '\u{60}'..='\u{7e}' => MAP[c as usize - 0x60],
        // As libghostty: the set covers one byte; anything above prints as
        // a space (in the width of the character it replaces).
        c if c as u32 > 0xff => ' ',
        _ => c,
    }
}

/// A line cut or extended to `cols` (as libghostty, a wide character cut
/// in half stays, without its spacer).
fn cut_line(mut l: Line, cols: u16) -> Line {
    let n = cols as usize;
    if l.cells.len() > n {
        l.cells.truncate(n);
    } else {
        l.cells.resize(n, blank(Style::default()));
    }
    l.dirty = true;
    l
}

/// What a scrollback line costs: its cells and its header.
fn line_bytes(l: &Line) -> usize {
    l.cells.len() * std::mem::size_of::<Cell>() + std::mem::size_of::<Line>()
}

fn default_tabs(cols: u16) -> Vec<bool> {
    (0..cols).map(|x| x > 0 && x % 8 == 0).collect()
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

/// SGR parameters as groups of fields (`;` between groups, `:` between
/// fields; an empty field is `None`). As libghostty, an empty last group is
/// not a parameter.
fn sgr_groups(raw: &[u8]) -> Vec<Vec<Option<u16>>> {
    if raw.is_empty() {
        return Vec::new();
    }
    let mut groups: Vec<Vec<Option<u16>>> = raw
        .split(|&c| c == b';')
        .map(|g| {
            g.split(|&c| c == b':')
                .map(|f| {
                    if f.is_empty() {
                        None
                    } else {
                        Some(f.iter().fold(0u32, |n, &d| (n * 10 + (d.wrapping_sub(b'0') as u32 % 10)).min(65535))
                            as u16)
                    }
                })
                .collect()
        })
        .collect();
    if groups.last().is_some_and(|g| g.len() == 1 && g[0].is_none()) {
        groups.pop();
    }
    groups
}

fn set_colour(style: &mut Style, which: u16, c: Option<Color>) {
    match (which, c) {
        (38, Some(c)) => style.fg = c,
        (48, Some(c)) => style.bg = c,
        _ => {} // no colour, or the underline colour (not in the snapshot)
    }
}

/// A colon form `38:5:N`, `38:2:R:G:B`, `38:2:ID:R:G:B`: the colour, and
/// what is left over (all after the 38 when the kind is unknown or the values
/// too few; nothing when there are too many or a needed value is empty).
fn colon_colour(g: &[Option<u16>]) -> (Option<Color>, &[Option<u16>]) {
    let byte = |f: Option<u16>| f.unwrap_or(0) as u8;
    match (g.get(1).copied().flatten(), g.len()) {
        (None, _) => (None, &[]),
        (Some(5), n) if n >= 3 => match g[2] {
            Some(x) => (Some(Color::Index(x as u8)), &g[3..]),
            None => (None, &[]),
        },
        (Some(2), 5) => (Some(Color::Rgb([byte(g[2]), byte(g[3]), byte(g[4])])), &[]),
        (Some(2), 6) => (Some(Color::Rgb([byte(g[3]), byte(g[4]), byte(g[5])])), &[]),
        (Some(2), n) if n >= 7 => (None, &[]),
        _ => (None, &g[1..]),
    }
}

/// A place followed through a resize: an absolute line (scrollback first)
/// and a column.
#[derive(Clone, Copy)]
struct Pin {
    line: usize,
    x: u16,
}

/// A cell without content (as libghostty's `isEmpty`: no text, not part of
/// a wide character, no background).
fn empty(c: &Cell) -> bool {
    c.is_empty() && c.wide == Wide::Narrow && c.style.bg == Color::Default
}

/// One screen through a resize (see `Terminal::resize`). `sb`: its
/// scrollback (`None`: lines pushed out are dropped).
fn resize_screen(
    s: &mut Screen,
    sb: Option<&mut VecDeque<Line>>,
    old_cols: u16,
    old_rows: u16,
    cols: u16,
    rows: u16,
    reflow: bool,
) {
    let keep_history = sb.is_some();
    let mut all: Vec<Line> = match &sb {
        Some(sb) => sb.iter().cloned().collect(),
        None => Vec::new(),
    };
    let hist = all.len();
    all.extend(std::mem::take(&mut s.lines));
    let mut cursor = Pin { line: hist + s.cursor.y as usize, x: s.cursor.x };
    let mut saved = s.saved.map(|v| Pin { line: hist + v.cursor.y as usize, x: v.cursor.x });
    let mut active_rows = old_rows as usize;
    if !reflow || cols == old_cols {
        if cols != old_cols {
            all = all.into_iter().map(|l| cut_line(l, cols)).collect();
            cursor.x = cursor.x.min(cols - 1);
            if let Some(p) = &mut saved {
                p.x = p.x.min(cols - 1);
            }
        }
        resize_rows(&mut all, &mut active_rows, rows as usize, &mut cursor, saved.as_ref());
    } else if cols > old_cols {
        all = reflow_lines(all, active_rows, cols, &mut cursor, saved.as_mut());
        resize_rows(&mut all, &mut active_rows, rows as usize, &mut cursor, saved.as_ref());
    } else {
        resize_rows(&mut all, &mut active_rows, rows as usize, &mut cursor, saved.as_ref());
        all = reflow_lines(all, active_rows, cols, &mut cursor, saved.as_mut());
    }
    // The active area is the last `rows` lines.
    while all.len() < rows as usize {
        all.push(Line::new(cols, Style::default()));
    }
    let top = all.len() - rows as usize;
    let active = |p: Pin| (p.line >= top).then(|| (p.x.min(cols - 1), (p.line - top) as u16));
    let (cx, cy) = active(cursor).unwrap_or((0, 0));
    s.cursor.x = cx;
    s.cursor.y = cy;
    s.cursor.pending_wrap = false;
    if let (Some(v), Some(p)) = (&mut s.saved, saved) {
        match active(p) {
            Some((x, y)) => {
                v.cursor.x = x;
                v.cursor.y = y;
                if v.cursor.pending_wrap && x != cols - 1 {
                    v.cursor.pending_wrap = false;
                    v.cursor.x += 1;
                }
            }
            None => {
                v.cursor.x = 0;
                v.cursor.y = 0;
                v.cursor.pending_wrap = false;
            }
        }
    }
    s.lines = all.split_off(top);
    // Lines from the scrollback have no trailing blanks.
    for l in &mut s.lines {
        l.cells.resize(cols as usize, Cell::default());
    }
    if let Some(sb) = sb {
        if keep_history {
            *sb = all.into();
        }
    }
}

/// Rows only (`resizeWithoutReflow`): fewer rows trim blank lines at the
/// bottom (not the cursor's), the rest of the difference goes to the
/// scrollback; more rows pull the scrollback down only with the cursor on
/// the last row, else blank lines come below.
fn resize_rows(all: &mut Vec<Line>, active_rows: &mut usize, rows: usize, cursor: &mut Pin, saved: Option<&Pin>) {
    let old = *active_rows;
    if rows < old {
        let mut trimmed = 0;
        while trimmed < old - rows {
            let i = all.len() - 1;
            let l = &all[i];
            if l.cells.iter().any(|c| !c.is_empty()) || cursor.line == i || saved.is_some_and(|p| p.line == i) {
                break;
            }
            all.pop();
            trimmed += 1;
        }
    } else if rows > old {
        let top = all.len().saturating_sub(old);
        let cols = all.first().map_or(1, |l| l.cells.len()) as u16;
        if cursor.line < top + old - 1 {
            for _ in 0..rows - old {
                all.push(Line::new(cols, Style::default()));
            }
        } else {
            while all.len() < rows {
                all.push(Line::new(cols, Style::default()));
            }
        }
    }
    *active_rows = rows;
}

/// Reflows every line to `cols` (`resizeCols`): soft-wrapped lines join and
/// are cut again, trailing empty cells of a hard line end are dropped,
/// blank lines are written only when content follows them, and as many rows
/// stay below the cursor as before.
fn reflow_lines(all: Vec<Line>, rows: usize, cols: u16, cursor: &mut Pin, mut saved: Option<&mut Pin>) -> Vec<Line> {
    let n = cols as usize;
    let continuation = |all: &[Line], i: usize| all[i].cont;
    let top = all.len().saturating_sub(rows);
    let wrapped_in = |all: &[Line], from: usize, to: usize| (from..=to).filter(|&i| continuation(all, i)).count();
    let (cursor_in, remaining, wrapped_before) = if cursor.line >= top {
        (true, rows.saturating_sub(cursor.line - top + 1), wrapped_in(&all, top, cursor.line))
    } else {
        (false, 0, 0)
    };
    let mut out: Vec<Line> = vec![Line::new(cols, Style::default())];
    let (mut x, mut pending) = (0usize, false);
    let mut new_rows = 0usize;
    let (mut cur_new, mut saved_new) = (None, None);
    for (i, src) in all.iter().enumerate() {
        let mut len = src.cells.len();
        if !src.wrapped {
            while len > 0 && empty(&src.cells[len - 1]) {
                len -= 1;
            }
        }
        if let Some(p) = saved.as_deref_mut() {
            if p.line == i {
                if p.x as usize >= len {
                    let room = if pending { 0 } else { n.saturating_sub(1 + x) };
                    p.x = p.x.min(room as u16);
                }
                len = len.max(p.x as usize + 1).min(src.cells.len());
            }
        }
        if cursor.line == i {
            len = len.max(cursor.x as usize + 1).min(src.cells.len());
        }
        if len == 0 {
            if !continuation(&all, i) {
                new_rows += 1;
            }
            continue;
        }
        while new_rows > 0 {
            out.push(Line::new(cols, Style::default()));
            x = 0;
            pending = false;
            new_rows -= 1;
        }
        let mut sx = 0;
        while sx < len {
            if pending {
                out.last_mut().expect("a line").wrapped = true;
                out.push(Line { cont: true, ..Line::new(cols, Style::default()) });
                x = 0;
                pending = false;
            }
            let here = Pin { line: out.len() - 1, x: x as u16 };
            if cursor.line == i && cursor.x as usize == sx {
                cur_new = Some(here);
            }
            if saved.as_deref().is_some_and(|p| p.line == i && p.x as usize == sx) {
                saved_new = Some(here);
            }
            let c = &src.cells[sx];
            match c.wide {
                Wide::SpacerHead => {
                    sx += 1;
                    continue;
                }
                // One column: a wide character becomes an empty cell, its
                // spacer goes.
                Wide::Wide if n < 2 => {
                    out.last_mut().expect("a line").cells[x] = blank(c.style);
                    pending = true;
                    sx += 2;
                    continue;
                }
                Wide::Wide if x == n - 1 => {
                    out.last_mut().expect("a line").cells[x] =
                        Cell { wide: Wide::SpacerHead, ..blank(Style::default()) };
                    pending = true;
                    continue;
                }
                _ => {}
            }
            out.last_mut().expect("a line").cells[x] = *c;
            if x + 1 >= n {
                pending = true;
            } else {
                x += 1;
            }
            sx += 1;
        }
        if !src.wrapped {
            new_rows += 1;
        }
    }
    if let Some(p) = cur_new {
        *cursor = p;
    }
    if let (Some(p), Some(v)) = (saved_new, saved) {
        *v = p;
    }
    // Rows below the cursor as before (fewer by the rows its own line or
    // those above it gained by wrapping).
    if cursor_in {
        while out.len() < rows {
            out.push(Line::new(cols, Style::default()));
        }
        let top = out.len() - rows;
        if cursor.line >= top {
            let wrapped = wrapped_in(&out, top, cursor.line);
            let current = rows.saturating_sub(cursor.line - top + 1);
            let req = remaining.saturating_sub(wrapped.saturating_sub(wrapped_before)).saturating_sub(current);
            for _ in 0..req {
                out.push(Line::new(cols, Style::default()));
            }
        }
    }
    out
}

fn param(params: &vte::Params, i: usize, default: u16) -> u16 {
    match params.iter().nth(i).map(|p| p[0]) {
        None | Some(0) => default,
        Some(v) => v,
    }
}

impl vte::Perform for Terminal {
    fn print(&mut self, c: char) {
        if width(c) > 0 {
            self.last_char = Some(c);
        }
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
            // As libghostty: C0 controls it has no action for, from 0x10 up,
            // print as a cell when they come between characters.
            0x10..=0x1f if self.ground_c0 => {
                let c = char::from(b);
                self.last_char = Some(c);
                self.print_char(c);
            }
            // vte executes C1 controls; libghostty prints a raw C1 byte as
            // U+FFFD (invalid UTF-8) and U+0080–U+009F sent as UTF-8 as the
            // character itself.
            0x80..=0x9f => {
                let c = if self.raw_c1 { '\u{fffd}' } else { char::from(b) };
                self.last_char = Some(c);
                self.print_char(c);
            }
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
        // As libghostty: colons are for SGR only.
        if self.csi_colon && action != 'm' {
            return;
        }
        let count = self.csi_count.min(params.len());
        if count > most {
            return;
        }
        let p0 = |d: u16| param(params, 0, d);
        let private = inter.first() == Some(&b'?');
        match (inter, action) {
            // As libghostty, an explicit 0 moves nothing for HPR and VPR.
            ([], 'a' | 'e') if count > 0 && param(params, 0, 0) == 0 => {}
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
            ([] | [b'?'], 'J') => self.erase_display(param(params, 0, 0), private),
            ([] | [b'?'], 'K') => self.erase_line(param(params, 0, 0), private),
            // DECSCA: 1 protects what is written next, 0 and 2 stop.
            ([b'"'], 'q') => match param(params, 0, 0) {
                0 | 2 => self.cur().protected = false,
                1 => {
                    self.cur().protected = true;
                    self.scr().protected_mode = 2;
                }
                _ => {}
            },
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
                // REP: the last printed character again, as written (the
                // charset in force now applies), wide ones included.
                if let Some(ch) = self.last_char {
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
            ([], 'm') => self.sgr(),
            // xterm's modify-key formats, as far as libghostty takes them:
            // only `CSI > 4 ; 2 m` turns modifyOtherKeys 2 on.
            ([b'>'], 'm') => {
                if count == 0 {
                    self.modes.modify_other_keys_2 = false;
                } else if matches!(param(params, 0, 0), 0 | 1 | 2 | 4) && count <= 2 {
                    self.modes.modify_other_keys_2 = param(params, 0, 0) == 4 && count == 2 && param(params, 1, 0) == 2;
                }
            }
            ([b'>'], 'n') => self.modes.modify_other_keys_2 = false,
            ([] | [b'?'], 'n') => self.device_status(private, param(params, 0, 0)),
            ([b'?', b'$'], 'p') if count == 0 => {}
            ([b'?', b'$'], 'p') => {
                let m = param(params, 0, 0);
                let s = self.mode_state(m);
                self.reply(format!("\x1b[?{m};{s}$y").as_bytes());
            }
            ([b' '], 'q') => {
                let (shape, blink) = match param(params, 0, 0) {
                    0 => (CursorShape::Block, false),
                    1 => (CursorShape::Block, true),
                    2 => (CursorShape::Block, false),
                    3 => (CursorShape::Underline, true),
                    4 => (CursorShape::Underline, false),
                    5 => (CursorShape::Bar, true),
                    6 => (CursorShape::Bar, false),
                    _ => return,
                };
                self.cur().shape = shape;
                self.modes.cursor_blink = blink;
            }
            ([b'>'], 'q') => {
                let v = format!("\x1bP>|{}\x1b\\", self.xtversion);
                self.reply(v.as_bytes());
            }
            ([], 'r') => self.set_region(param(params, 0, 1), param(params, 1, 0)),
            // As libghostty: only without parameters (with them it would be
            // DECSLRM, which needs mode 69).
            ([], 's') if count == 0 && !self.modes.lr_margins => self.save_cursor(),
            // DECSLRM (margins not implemented): the cursor goes home.
            ([], 's') if self.modes.lr_margins => {
                // A valid pair (left < right ≤ columns) is accepted.
                let (l, r) = (param(params, 0, 1), param(params, 1, self.cols));
                if l < r && r <= self.cols {
                    self.goto(0, 0);
                }
            }
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
            // As libghostty: flags above 31 make the whole push or set void,
            // and popping 0 entries pops none.
            ([b'>' | b'='], 'u') if param(params, 0, 0) > 31 => {}
            ([b'<'], 'u') if count > 0 && param(params, 0, 0) == 0 => {}
            ([b'>'], 'u') => {
                let f = param(params, 0, 0) as u8;
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
                let f = param(params, 0, 0) as u8;
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
            // SPA / EPA: ISO protected area.
            ([], b'V') => {
                self.cur().protected = true;
                self.scr().protected_mode = 1;
            }
            ([], b'W') => self.cur().protected = false,
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
                // In the current colours (no attributes), as libghostty.
                let (cols, rows) = (self.cols, self.rows);
                let style = Style { flags: 0, ..self.scr().cursor.style };
                for y in 0..rows {
                    for x in 0..cols {
                        self.line(y).cells[x as usize] = Cell::of('E', style, Wide::Narrow);
                    }
                    let l = self.line(y);
                    l.wrapped = false;
                    l.cont = false;
                }
                self.cur().style.flags = 0;
                // DECALN also resets the scroll region and origin mode.
                self.modes.origin = false;
                self.top = 0;
                self.bottom = rows - 1;
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
        // As libghostty: an OSC that is not valid UTF-8 is dropped whole.
        if params.iter().any(|p| std::str::from_utf8(p).is_err()) {
            return;
        }
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
            scrollback_limit: 1 << 20,
        })
    }

    #[test]
    fn prints_wraps_and_scrolls() {
        let mut t = term(4, 2);
        t.feed(b"abcdef\r\nxy");
        assert_eq!(t.text_at(0, 0), "e");
        assert_eq!(t.text_at(1, 0), "f");
        assert_eq!(t.text_at(0, 1), "x");
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
        assert_eq!((t.text_at(0, 1).as_str(), t.cell(0, 1).wide), ("가", Wide::Wide));
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
    fn cells_are_small_and_scrollback_keeps_its_limit() {
        assert_eq!(std::mem::size_of::<Cell>(), 16);
        let mut t = term(40, 4);
        t.sb_limit = 4096;
        for i in 0..2000 {
            t.feed(format!("line {i}\r\n").as_bytes());
        }
        assert!(t.scrollback_bytes() <= 4096, "{} bytes", t.scrollback_bytes());
        assert!(t.scrollback_rows() > 20, "short lines are cheap: {}", t.scrollback_rows());
        t.scroll_viewport_top();
        assert!(t.text_at(0, 0).starts_with('l'));
        t.feed("e\u{301}\u{301}".as_bytes());
        t.scroll_viewport_bottom();
        let (x, y) = t.cursor().unwrap();
        assert_eq!(t.text_at(x - 1, y), "e\u{301}\u{301}");
    }

    #[test]
    fn colours_parse() {
        assert_eq!(parse_color("#fff"), Some([255; 3]));
        assert_eq!(parse_color("rgb:ffff/0/80"), Some([255, 0, 0x80]));
        assert_eq!(base64(b"aGk="), Some(b"hi".to_vec()));
    }
}
