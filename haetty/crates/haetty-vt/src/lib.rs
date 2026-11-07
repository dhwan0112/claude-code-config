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
mod format;
mod grapheme;
pub mod grapheme_table;
mod kitty;
mod width;
pub mod width_table;
mod x11_colors;

use std::collections::VecDeque;

pub use color::{parse_color, parse_palette_entry};
pub use format::Format;
pub use width::width;

/// A colour as the program set it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Style {
    pub fg: Color,
    pub bg: Color,
    pub flags: u16,
    /// The underline's kind while `flags::UNDERLINE` is set (SGR 4:x, 21),
    /// 0 otherwise: [`underline`] numbers, as libghostty's `sgr.Attribute.Underline`.
    pub underline: u8,
    /// The underline colour (SGR 58, 59).
    pub ul: Color,
    /// The hyperlink (OSC 8), a number in the terminal's table
    /// (`Terminal::link`); 0 is none. In the pen it is set by OSC 8 only:
    /// SGR 0 and DECRC keep it, as in libghostty, where it is not part of
    /// the style.
    pub link: u16,
}

/// Underline kinds ([`Style::underline`]).
pub mod underline {
    pub const NONE: u8 = 0;
    pub const SINGLE: u8 = 1;
    pub const DOUBLE: u8 = 2;
    pub const CURLY: u8 = 3;
    pub const DOTTED: u8 = 4;
    pub const DASHED: u8 = 5;
}

/// A hyperlink's id: the one OSC 8 gave (`id=`), or a number of its own
/// for each OSC 8 without one (libghostty's implicit ids), so that two links
/// to the same target stay two.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum LinkId {
    Explicit(Box<[u8]>),
    Implicit(u32),
}

/// A hyperlink (OSC 8) as the program sent it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Link {
    pub id: LinkId,
    pub uri: Box<[u8]>,
}

/// The hyperlinks cells refer to through their style. 0 is none. Rebuilt
/// with the style table (`Terminal::compact_styles`), so it stays bounded.
#[derive(Clone, Debug)]
struct Links {
    compact_at: usize,
    list: Vec<Option<Link>>,
    ids: std::collections::HashMap<Link, u16>,
    /// The next implicit id.
    implicit: u32,
}

impl Links {
    const COMPACT_AT: usize = 0x4000;

    fn new() -> Links {
        Links { compact_at: Links::COMPACT_AT, list: vec![None], ids: Default::default(), implicit: 0 }
    }

    /// The number of `l`, added if new; 0 when the table is full.
    fn id(&mut self, l: Link) -> u16 {
        if let Some(&i) = self.ids.get(&l) {
            return i;
        }
        if self.list.len() >= u16::MAX as usize {
            return 0;
        }
        let i = self.list.len() as u16;
        self.list.push(Some(l.clone()));
        self.ids.insert(l, i);
        i
    }

    fn get(&self, id: u16) -> Option<&Link> {
        self.list.get(id as usize).and_then(|l| l.as_ref())
    }
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

/// One cell of the screen: 8 bytes, `Copy` (DESIGN.md §10.3, §10.9).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cell {
    /// 0: empty; below 0x110000: that character alone; with the top bit
    /// set: a grapheme cluster in the terminal's table (`Terminal::text`).
    content: u32,
    /// The style, a number in the terminal's table (`Terminal::style`);
    /// 0 is the default style.
    pub style: u16,
    pub wide: Wide,
    /// Written under SPA or DECSCA: erasure that respects protection
    /// leaves it.
    pub protected: bool,
}

const CLUSTER: u32 = 0x8000_0000;

impl Cell {
    fn of(c: char, style: u16, wide: Wide) -> Cell {
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
    /// A line of `blank` cells (see `Styles::blank`).
    fn new(cols: u16, blank: Cell) -> Line {
        Line { cells: vec![blank; cols as usize], wrapped: false, dirty: true, cont: false }
    }
}

/// An erased cell of style `id`, which is a background-only style (see
/// `Styles::blank`).
fn blank(id: u16) -> Cell {
    Cell { content: 0, style: id, wide: Wide::Narrow, protected: false }
}

/// The styles cells refer to by number (DESIGN.md §10.9). 0 is the default
/// style. The table only grows; `Terminal::compact_styles` rebuilds it from
/// the styles still in use when it gets large, so it stays bounded.
#[derive(Clone, Debug)]
struct Styles {
    /// Compact when the table reaches this size.
    compact_at: usize,
    /// The last style looked up and its number (the pen rarely changes),
    /// and the last erased form.
    last: (Style, u16),
    last_blank: (Color, u16),
    list: Vec<Style>,
    /// Per style: the number of its erased form (background only).
    erased: Vec<u16>,
    ids: std::collections::HashMap<Style, u16, std::hash::BuildHasherDefault<FxHasher>>,
}

/// A small fast hasher for the style table (as rustc's FxHasher); styles
/// come from the program's output, but a collision costs only time.
#[derive(Default)]
struct FxHasher(u64);

impl std::hash::Hasher for FxHasher {
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0.rotate_left(5) ^ b as u64).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
        }
    }
    fn write_u8(&mut self, i: u8) {
        self.0 = (self.0.rotate_left(5) ^ i as u64).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
    fn write_u16(&mut self, i: u16) {
        self.0 = (self.0.rotate_left(5) ^ i as u64).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
    fn write_isize(&mut self, i: isize) {
        self.0 = (self.0.rotate_left(5) ^ i as u64).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
    fn finish(&self) -> u64 {
        self.0
    }
}

impl Styles {
    /// Compaction happens above this many styles.
    const COMPACT_AT: usize = 0xc000;

    fn new() -> Styles {
        Styles {
            compact_at: Styles::COMPACT_AT,
            last: (Style::default(), 0),
            last_blank: (Color::Default, 0),
            list: vec![Style::default()],
            erased: vec![0],
            ids: [(Style::default(), 0)].into_iter().collect(),
        }
    }

    /// The number of `s`, added if new. A full table (65 535 styles, which
    /// compaction had no room to free) gives the erased form instead.
    fn id(&mut self, s: Style) -> u16 {
        if self.last.0 == s {
            return self.last.1;
        }
        if let Some(&i) = self.ids.get(&s) {
            self.last = (s, i);
            return i;
        }
        let e = Style { bg: s.bg, ..Style::default() };
        let eid = if e == s { None } else { Some(self.id(e)) };
        if self.list.len() >= u16::MAX as usize {
            return eid.unwrap_or(0);
        }
        let i = self.list.len() as u16;
        self.list.push(s);
        self.erased.push(eid.unwrap_or(i));
        self.ids.insert(s, i);
        self.last = (s, i);
        i
    }

    fn get(&self, id: u16) -> Style {
        self.list.get(id as usize).copied().unwrap_or_default()
    }

    /// An erased cell in the pen's background (as libghostty).
    fn blank(&mut self, pen: Style) -> Cell {
        if self.last_blank.0 == pen.bg {
            return blank(self.last_blank.1);
        }
        let last = self.last;
        let id = self.id(Style { bg: pen.bg, ..Style::default() });
        self.last = last;
        self.last_blank = (pen.bg, id);
        blank(id)
    }

    /// The erased form of a cell's style.
    fn erased(&self, id: u16) -> u16 {
        self.erased.get(id as usize).copied().unwrap_or(0)
    }

    fn bg_default(&self, id: u16) -> bool {
        self.get(id).bg == Color::Default
    }

    /// Memory the table takes (for measurements).
    pub(crate) fn bytes(&self) -> usize {
        self.list.capacity() * (std::mem::size_of::<Style>() + 2)
            + self.ids.capacity() * (std::mem::size_of::<Style>() + 2 + 8)
    }
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
    /// Kitty graphics images and placements (each screen has its own).
    gfx: kitty::Storage,
}

impl Screen {
    fn new(cols: u16, rows: u16) -> Screen {
        Screen {
            lines: (0..rows).map(|_| Line::new(cols, Cell::default())).collect(),
            cursor: Cursor { charsets: [b'B'; 4], ..Default::default() },
            saved: None,
            kitty: vec![0],
            protected_mode: 0,
            gfx: kitty::Storage::default(),
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

/// libghostty behaviours haetty-vt does not follow (it follows ECMA-48 and
/// xterm there), each switchable for vtdiff's KNOWN check only: with the
/// switch on, a difference the entry explains must vanish
/// (docs/vtdiff/known.md). Nothing else sets them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Compat(pub u16);

impl Compat {
    /// TBC 0 toggles the stop at the cursor instead of clearing it.
    pub const TBC_TOGGLE: Compat = Compat(1);
    /// Under origin mode CHA, HPA and HPR also move the cursor down by the
    /// scroll region's top.
    pub const ORIGIN_COLUMN_MOVES_ROW: Compat = Compat(2);
    /// Under origin mode VPA and VPR also move the cursor right by the left
    /// margin, and VPR down by the region's top (libghostty's setCursorPos
    /// with the current column/row).
    pub const ORIGIN_ROW_MOVES_COLUMN: Compat = Compat(4);
    /// A print into the last column sets the pending wrap with autowrap
    /// (DECAWM) off too; it acts once autowrap is on again.
    pub const PENDING_WRAP_WITHOUT_AUTOWRAP: Compat = Compat(8);
    /// For vtdiff's self-test: CUF moves one column too far.
    pub const SELFTEST_BREAK: Compat = Compat(0x8000);
    const NAMES: &[(&str, Compat)] = &[
        ("tbc-toggle", Compat::TBC_TOGGLE),
        ("origin-column-moves-row", Compat::ORIGIN_COLUMN_MOVES_ROW),
        ("origin-row-moves-column", Compat::ORIGIN_ROW_MOVES_COLUMN),
        ("pending-wrap-without-autowrap", Compat::PENDING_WRAP_WITHOUT_AUTOWRAP),
        ("selftest-break", Compat::SELFTEST_BREAK),
    ];

    pub fn by_name(n: &str) -> Option<Compat> {
        Compat::NAMES.iter().find(|(k, _)| *k == n).map(|(_, c)| *c)
    }

    pub fn has(self, o: Compat) -> bool {
        self.0 & o.0 != 0
    }
}

impl std::ops::BitOr for Compat {
    type Output = Compat;
    fn bitor(self, o: Compat) -> Compat {
        Compat(self.0 | o.0)
    }
}

pub struct Terminal {
    compat: Compat,
    cols: u16,
    rows: u16,
    cell: (u16, u16),
    primary: Screen,
    alt: Screen,
    scrollback: VecDeque<Line>,
    /// The scroll region, 0-based, inclusive.
    top: u16,
    bottom: u16,
    /// Left and right margins (DECSLRM, with DECLRMM; DESIGN.md §10.9).
    left: u16,
    right: u16,
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
    /// The styles cells refer to (`Cell::style`).
    styles: Styles,
    /// The hyperlinks styles refer to (`Style::link`).
    links: Links,
    /// The page the pen's hyperlink was (re-)added on (`renew_link`).
    link_page: usize,
    cluster_ids: std::collections::HashMap<Box<str>, u32>,
    /// What the scrollback holds and may hold, in bytes (as Ghostty's
    /// `scrollback-limit`; DESIGN.md §10.3).
    sb_bytes: usize,
    sb_limit: usize,
    /// XTSAVE: DEC private modes saved, for XTRESTORE.
    saved_modes: std::collections::HashMap<u16, bool>,
    /// Every row counts as changed (see `take_dirty`).
    all_dirty: bool,
}

impl Terminal {
    pub fn new(o: Options) -> Terminal {
        let (cols, rows) = (o.cols.max(1), o.rows.max(1));
        let p16 = o.colors.palette.unwrap_or(DEFAULT_16);
        Terminal {
            compat: Compat::default(),
            cols,
            rows,
            cell: (o.cell_width, o.cell_height),
            primary: Screen::new(cols, rows),
            alt: Screen::new(cols, rows),
            scrollback: VecDeque::new(),
            top: 0,
            bottom: rows - 1,
            left: 0,
            right: cols - 1,
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
            styles: Styles::new(),
            links: Links::new(),
            link_page: 0,
            cluster_ids: std::collections::HashMap::new(),
            sb_bytes: 0,
            saved_modes: std::collections::HashMap::new(),
            sb_limit: o.scrollback_limit,
        }
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        self.compact_styles();
        let mut parser = std::mem::take(&mut self.parser);
        // vte 0.15 loses a character when a call completes the character the
        // last one ended in and goes on to another character and the start
        // of one more within its first bytes (`advance_partial_utf8` counts
        // every valid byte after the completed character as consumed). So the
        // bytes that complete it go to vte on their own first.
        let k = self.pre.continuation(bytes);
        if k > 0 {
            parser.advance(self, &bytes[..k]);
        }
        // The scanner sees what vte does not tell: APC strings (vte skips
        // them; the kitty graphics ones are answered here, in order with the
        // replies around them) and whether a CSI had parameter bytes at all.
        let mut start = k;
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
                Pre::Osc => {
                    // vte dispatches on this byte: `osc` is this OSC's.
                    parser.advance(self, &bytes[start..=i]);
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

    /// An APC string (or SOS / PM: libghostty reads them the same way):
    /// kitty graphics commands (`G…`, see `kitty`).
    fn apc_dispatch(&mut self, apc: &[u8]) {
        kitty::apc(self, apc);
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

    /// A cell's style (`Cell::style` is its number in the table).
    pub fn style(&self, c: &Cell) -> Style {
        self.styles.get(c.style)
    }

    /// Distinct styles in the table and the bytes the table takes.
    pub fn style_table(&self) -> (usize, usize) {
        (self.styles.list.len(), self.styles.bytes())
    }

    /// A style's hyperlink (`Style::link`).
    pub fn link(&self, id: u16) -> Option<&Link> {
        self.links.get(id)
    }

    /// Rebuilds the style table from the styles cells still use, once it
    /// is large (DESIGN.md §10.9): styles of rows gone from the scrollback
    /// go. If nearly all are in use, the next attempt waits until half the
    /// room left is taken. The hyperlinks go the same way: those of the
    /// styles kept and of the pens (and saved cursors) stay.
    fn compact_styles(&mut self) {
        if self.styles.list.len() < self.styles.compact_at && self.links.list.len() < self.links.compact_at {
            return;
        }
        let mut used = vec![false; self.styles.list.len()];
        used[0] = true;
        for l in self.primary.lines.iter().chain(&self.alt.lines).chain(&self.scrollback) {
            for c in &l.cells {
                used[c.style as usize] = true;
            }
        }
        // The hyperlinks still referred to.
        let mut lused = vec![false; self.links.list.len()];
        for (s, &u) in self.styles.list.iter().zip(&used) {
            if u {
                lused[s.link as usize] = true;
            }
        }
        for scr in [&self.primary, &self.alt] {
            lused[scr.cursor.style.link as usize] = true;
            if let Some(s) = &scr.saved {
                lused[s.cursor.style.link as usize] = true;
            }
        }
        let old_links = std::mem::replace(&mut self.links, Links::new());
        self.links.implicit = old_links.implicit;
        let lmap: Vec<u16> = old_links
            .list
            .into_iter()
            .zip(&lused)
            .map(|(l, &u)| match l {
                Some(l) if u => self.links.id(l),
                _ => 0,
            })
            .collect();
        let relink = |s: &mut Style| s.link = lmap[s.link as usize];
        for scr in [&mut self.primary, &mut self.alt] {
            relink(&mut scr.cursor.style);
            if let Some(s) = &mut scr.saved {
                relink(&mut s.cursor.style);
            }
        }
        let old = std::mem::replace(&mut self.styles, Styles::new());
        let map: Vec<u16> = old
            .list
            .iter()
            .zip(&used)
            .map(|(s, &u)| {
                if u {
                    let mut s = *s;
                    relink(&mut s);
                    self.styles.id(s)
                } else {
                    0
                }
            })
            .collect();
        for l in self.primary.lines.iter_mut().chain(self.alt.lines.iter_mut()).chain(self.scrollback.iter_mut()) {
            for c in &mut l.cells {
                c.style = map[c.style as usize];
            }
        }
        let n = self.styles.list.len();
        self.styles.compact_at = Styles::COMPACT_AT.max(n + (u16::MAX as usize - n) / 2);
        let n = self.links.list.len();
        self.links.compact_at = Links::COMPACT_AT.max(n + (u16::MAX as usize - n) / 2);
    }

    /// See [`Compat`]: for vtdiff only.
    pub fn set_compat(&mut self, c: Compat) {
        self.compat = c;
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
        let before = sb.len() as i64;
        resize_screen(&mut self.primary, Some(&mut sb), old_cols, old_rows, cols, rows, reflow, &self.styles);
        // Kitty placements stay with their rows (not with reflowed text).
        self.primary.gfx.resized(sb.len() as i64 - before, cols);
        self.sb_bytes = 0;
        for l in sb {
            self.push_scrollback(l);
        }
        // Alternate screen: no reflow, no scrollback.
        resize_screen(&mut self.alt, None, old_cols, old_rows, cols, rows, false, &self.styles);
        self.alt.gfx.resized(0, cols);
        self.cols = cols;
        self.rows = rows;
        self.top = 0;
        self.bottom = rows - 1;
        self.left = 0;
        self.right = cols - 1;
        if cols != old_cols {
            self.tabs = default_tabs(cols);
        }
        self.viewport = 0;
        self.renew_link(true);
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
    fn wrap_flags(&self, top: u16, bottom: u16) -> Vec<(bool, bool)> {
        self.screen_ref().lines[top as usize..=bottom as usize].iter().map(|l| (l.wrapped, l.cont)).collect()
    }

    /// Wrap flags after rows `top..=bottom` shifted by `n` (IL or DL; SU
    /// and SD of a region), as libghostty's
    /// insertLines/deleteLines: a row that moved, or that another row moved
    /// into, loses its wrap and continuation (and a spacer head at its end);
    /// a row only cleared keeps its flags.
    fn shift_flags(&mut self, top: u16, bottom: u16, n: u16, old: &[(bool, bool)]) {
        let rem = bottom - top + 1;
        let n = n.min(rem);
        for k in 0..rem {
            // Moved from (DL: below n; IL: above rem - n) or into: the same rows.
            let moved = k < rem - n || k >= n;
            let l = self.line(top + k);
            if moved && n < rem {
                l.wrapped = false;
                l.cont = false;
                if let Some(c) = l.cells.last_mut() {
                    if c.wide == Wide::SpacerHead {
                        c.wide = Wide::Narrow;
                    }
                }
            } else {
                (l.wrapped, l.cont) = old[k as usize];
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

    /// The page of libghostty's page list the next print lands on, as the
    /// dump models it (`format::rows_per_page`, counted from the top).
    fn cursor_page(&self) -> usize {
        let base = if self.modes.alt_screen { 0 } else { self.scrollback.len() };
        let c = self.screen_ref().cursor;
        (base + c.y as usize + c.pending_wrap as usize) / format::rows_per_page(self.cols)
    }

    /// libghostty re-adds the pen's hyperlink when the cursor goes to another
    /// page (`cursorChangePin`) and on a resize: a link without an id gets a
    /// new one then (`startHyperlink(uri, null)`), so its cells on either
    /// side are two links (HTML dumps show `</a><a …>`).
    fn renew_link(&mut self, resized: bool) {
        let l = self.screen_ref().cursor.style.link;
        if l == 0 {
            return;
        }
        let page = self.cursor_page();
        if !resized && self.link_page == page {
            return;
        }
        self.link_page = page;
        if let Some(Link { id: LinkId::Implicit(_), uri }) = self.links.get(l).cloned() {
            let n = self.links.implicit;
            self.links.implicit = n.wrapping_add(1);
            let id = self.links.id(Link { id: LinkId::Implicit(n), uri });
            self.cur().style.link = id;
        }
    }

    fn print_char(&mut self, c: char) {
        self.renew_link(false);
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
        // As libghostty: printing stops at the right margin, unless the
        // cursor is already right of it.
        let right_limit = if self.scr().cursor.x > self.right { cols } else { self.right + 1 };
        let mut w = w;
        if w == 2 && right_limit - self.left < 2 {
            // No room for a wide character at all: an empty cell.
            w = 0;
        } else if w == 2 && self.scr().cursor.x + 1 >= right_limit {
            if !autowrap {
                return;
            }
            // As libghostty: a spacer in the last column (an empty cell at a
            // right margin), then the next row.
            let (x, y) = (self.scr().cursor.x, self.scr().cursor.y);
            let pen = self.scr().cursor.style;
            let style = self.styles.id(pen);
            let wide = if right_limit == cols { Wide::SpacerHead } else { Wide::Narrow };
            self.clear_wide_at(x, y);
            self.line(y).cells[x as usize] = Cell { content: 0, style, wide, protected: false };
            self.wrap_line();
        }
        let (x, y, style) = {
            let c = self.scr().cursor;
            (c.x, c.y, c.style)
        };
        let style = self.styles.id(style);
        // Without autowrap there is no pending wrap (xterm); libghostty sets
        // it anyway (`Compat::PENDING_WRAP_WITHOUT_AUTOWRAP`).
        let wrap_next = autowrap || self.compat.has(Compat::PENDING_WRAP_WITHOUT_AUTOWRAP);
        if w == 0 {
            self.print_clears_head(x, y, Wide::Narrow);
            self.clear_wide_halves(x, y);
            self.line(y).cells[x as usize] = Cell { content: 0, style, wide: Wide::Narrow, protected: false };
            let cur = self.cur();
            if x + 1 >= right_limit {
                cur.pending_wrap = wrap_next;
            } else {
                cur.x = x + 1;
            }
            return;
        }
        if self.modes.insert && x + (w as u16) < cols {
            self.insert_blanks(x, y, w as u16);
        }
        // Over a narrow cell a narrow character needs neither (the common
        // case, kept short).
        if w == 2 || self.screen_ref().lines[y as usize].cells[x as usize].wide != Wide::Narrow {
            self.print_clears_head(x, y, if w == 2 { Wide::Wide } else { Wide::Narrow });
            self.clear_wide_halves(x, y);
            if w == 2 {
                self.print_clears_head(x + 1, y, Wide::SpacerTail);
                self.clear_wide_halves(x + 1, y);
            }
        }
        let protected = self.screen_ref().cursor.protected;
        let l = self.line(y);
        l.cells[x as usize] = Cell { protected, ..Cell::of(c, style, if w == 2 { Wide::Wide } else { Wide::Narrow }) };
        if w == 2 {
            l.cells[x as usize + 1] = Cell { content: 0, style, wide: Wide::SpacerTail, protected: false };
        }
        let nx = x + w as u16;
        let cur = self.cur();
        if nx >= right_limit {
            cur.x = right_limit - 1;
            cur.pending_wrap = wrap_next;
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
        if matches!(c, '\u{fe0e}' | '\u{fe0f}') && !width::vs_base_2027(prev) {
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
                    let pen_id = self.styles.id(pen);
                    let head_style = if text.chars().count() > 1 { old.style } else { pen_id };
                    self.line(cur.y).cells[px as usize] =
                        Cell { content: 0, style: head_style, wide: Wide::SpacerHead, protected: false };
                    self.wrap_line();
                    let ny = self.scr().cursor.y;
                    self.clear_wide_at(0, ny);
                    self.line(ny).cells[0] =
                        Cell { content: prev_cell.content, style: pen_id, wide: Wide::Wide, protected: false };
                    cx = 0;
                    cy = ny;
                } else {
                    self.line(cur.y).cells[px as usize].wide = Wide::Wide;
                }
                let x = self.scr().cursor.x + 1;
                let yy = self.scr().cursor.y;
                self.clear_wide_at(x, yy);
                let pen_id = self.styles.id(pen);
                self.line(yy).cells[x as usize] =
                    Cell { content: 0, style: pen_id, wide: Wide::SpacerTail, protected: false };
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

    /// Printing over a wide character or its tail in the first two columns
    /// turns the row above's spacer head back into a blank, as libghostty's
    /// `printCell` (whether or not that character came from there).
    fn print_clears_head(&mut self, x: u16, y: u16, new: Wide) {
        if y == 0 || x > 1 || x >= self.cols {
            return;
        }
        let old = self.screen_ref().lines[y as usize].cells[x as usize].wide;
        if old == new || !(old == Wide::Wide && x + 1 < self.cols || old == Wide::SpacerTail) {
            return;
        }
        if let Some(c) = self.line(y - 1).cells.last_mut() {
            if c.wide == Wide::SpacerHead {
                c.wide = Wide::Narrow;
            }
        }
    }

    /// Writing over half of a wide character clears the other half.
    fn clear_wide_at(&mut self, x: u16, y: u16) {
        let i = x as usize;
        if i >= self.cols as usize {
            return;
        }
        // A wide character in the first column that came from the row above:
        // that row's spacer head goes too.
        if i == 0 && y > 0 && self.screen_ref().lines[y as usize].cells[0].wide == Wide::Wide {
            if let Some(c) = self.line(y - 1).cells.last_mut() {
                if c.wide == Wide::SpacerHead {
                    c.wide = Wide::Narrow;
                }
            }
        }
        self.clear_wide_halves(x, y);
    }

    fn clear_wide_halves(&mut self, x: u16, y: u16) {
        let cols = self.cols as usize;
        let i = x as usize;
        if i >= cols {
            return;
        }
        let other = match self.screen_ref().lines[y as usize].cells[i].wide {
            Wide::Wide if i + 1 < cols => i + 1,
            Wide::SpacerTail if i > 0 => i - 1,
            _ => return,
        };
        // In the pen's background, as libghostty.
        let pen = self.scr().cursor.style;
        let bk_pen = self.styles.blank(pen);
        self.line(y).cells[other] = bk_pen;
    }

    fn wrap_line(&mut self) {
        let (x, y) = (self.scr().cursor.x, self.scr().cursor.y);
        let mark = x == self.cols - 1;
        if mark {
            self.line(y).wrapped = true;
        }
        self.cur().pending_wrap = false;
        self.linefeed();
        let left = self.left;
        self.cur().x = left;
        if mark {
            let y = self.scr().cursor.y;
            self.line(y).cont = true;
        }
    }

    fn linefeed(&mut self) {
        let (x, y, bottom) = (self.scr().cursor.x, self.scr().cursor.y, self.bottom);
        if y == bottom {
            if x >= self.left && x <= self.right {
                self.scroll_up(1);
                kitty::region_scrolled(self, 1, true);
            }
        } else if y + 1 < self.rows {
            self.cur().y = y + 1;
        }
        self.cur().pending_wrap = false;
    }

    /// Runs `f` with the cursor at (`x`, `y`), then puts it back (with its
    /// pending wrap), as libghostty's scrollUp and scrollDown.
    fn at_corner(&mut self, x: u16, y: u16, f: impl FnOnce(&mut Self)) {
        let old = self.scr().cursor;
        let cur = self.cur();
        cur.x = x;
        cur.y = y;
        f(self);
        let cur = self.cur();
        cur.x = old.x;
        cur.y = old.y;
        cur.pending_wrap = old.pending_wrap;
    }

    /// IL (`insert`) or DL inside left and right margins, as libghostty's
    /// insertLines/deleteLines with margins: rows `y` to the region's bottom
    /// move by `n` within the margins' columns; the rows that open get blanks
    /// in the pen's background; the cursor goes to the left margin. Wrap
    /// flags stay.
    fn shift_columns(&mut self, y: u16, n: u16, insert: bool) {
        let (bottom, l, r, pen) = (self.bottom, self.left as usize, self.right as usize, self.scr().cursor.style);
        let bk_pen = self.styles.blank(pen);
        let rem = bottom - y + 1;
        let n = n.min(rem);
        let rows: Vec<u16> = if insert { (y..=bottom).rev().collect() } else { (y..=bottom).collect() };
        for (k, &row) in rows.iter().enumerate() {
            if (k as u16) < rem - n {
                let src = if insert { row - n } else { row + n };
                self.row_will_be_shifted(row);
                self.row_will_be_shifted(src);
                let cells: Vec<Cell> = self.screen_ref().lines[src as usize].cells[l..=r].to_vec();
                self.line(row).cells[l..=r].copy_from_slice(&cells);
            } else {
                self.row_will_be_shifted(row);
                for c in &mut self.line(row).cells[l..=r] {
                    *c = bk_pen;
                }
            }
        }
        self.moved(y, bottom);
        let left = self.left;
        let cur = self.cur();
        cur.x = left;
        cur.pending_wrap = false;
    }

    /// libghostty's rowWillBeShifted: wide characters cut by a margin become
    /// blank halves; a spacer head at the row's end goes when the margins
    /// reach it.
    fn row_will_be_shifted(&mut self, y: u16) {
        let (cols, l, r) = (self.cols as usize, self.left as usize, self.right as usize);
        let line = self.line(y);
        if r == cols - 1 || l < 2 {
            let end = &mut line.cells[cols - 1];
            if end.wide == Wide::SpacerHead {
                end.wide = Wide::Narrow;
            }
        }
        if line.cells[l].wide == Wide::SpacerTail && l > 0 {
            let w = &mut line.cells[l - 1];
            w.content = 0;
            w.wide = Wide::Narrow;
            line.cells[l].wide = Wide::Narrow;
        }
        if line.cells[r].wide == Wide::Wide && r + 1 < cols {
            line.cells[r].content = 0;
            line.cells[r].wide = Wide::Narrow;
            line.cells[r + 1].wide = Wide::Narrow;
        }
    }

    /// A wide character cut in two by an insert or delete loses both halves
    /// (each an erased cell of its style).
    fn fix_split_wide(&mut self, y: u16) {
        let cells = &self.screen_ref().lines[y as usize].cells;
        let n = cells.len();
        let bad: Vec<(usize, u16)> = (0..n)
            .filter(|&i| match cells[i].wide {
                Wide::Wide => i + 1 >= n || cells[i + 1].wide != Wide::SpacerTail,
                Wide::SpacerTail => i == 0 || cells[i - 1].wide != Wide::Wide,
                _ => false,
            })
            .map(|i| (i, self.styles.erased(cells[i].style)))
            .collect();
        for (i, e) in bad {
            self.line(y).cells[i] = blank(e);
        }
    }

    /// Margins narrower than the screen (DECSLRM).
    fn lr(&self) -> bool {
        self.left > 0 || self.right < self.cols - 1
    }

    fn reverse_index(&mut self) {
        let (x, y) = (self.scr().cursor.x, self.scr().cursor.y);
        // As libghostty: outside the margins it is a CUU, which does not
        // leave the region's top.
        if y == self.top && (x < self.left || x > self.right) {
            self.cur().pending_wrap = false;
            return;
        }
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
        if self.lr() {
            let (left, top) = (self.left, self.top);
            self.at_corner(left, top, |t| t.delete_lines(n));
            return;
        }
        let (top, bottom) = (self.top as usize, self.bottom as usize);
        let n = (n as usize).min(bottom - top + 1);
        let bg = self.scr().cursor.style;
        let bk_bg = self.styles.blank(bg);
        let cols = self.cols;
        let to_scrollback = top == 0 && !self.modes.alt_screen;
        let full = top == 0 && (!self.modes.alt_screen || bottom == self.rows as usize - 1);
        let old = if full { vec![] } else { self.wrap_flags(top as u16, bottom as u16) };
        for _ in 0..n {
            let l = self.scr().lines.remove(top);
            if to_scrollback {
                self.push_scrollback(l);
            }
            self.scr().lines.insert(bottom, Line::new(cols, bk_bg));
        }
        if to_scrollback {
            self.primary.gfx.top_row += n as u64;
        }
        if to_scrollback && self.viewport > 0 {
            self.viewport = (self.viewport + n).min(self.scrollback.len());
        }
        self.moved(top as u16, bottom as u16);
        // A region from the top scrolls as lines leaving the screen do;
        // any other is a deletion of lines.
        if !full {
            self.shift_flags(top as u16, bottom as u16, n as u16, &old);
        }
    }

    fn scroll_down(&mut self, n: u16) {
        if self.lr() {
            let (left, top) = (self.left, self.top);
            self.at_corner(left, top, |t| t.insert_lines(n));
            return;
        }
        let (top, bottom) = (self.top as usize, self.bottom as usize);
        let n = (n as usize).min(bottom - top + 1);
        let bg = self.scr().cursor.style;
        let bk_bg = self.styles.blank(bg);
        let cols = self.cols;
        let old = self.wrap_flags(top as u16, bottom as u16);
        for _ in 0..n {
            self.scr().lines.remove(bottom);
            self.scr().lines.insert(top, Line::new(cols, bk_bg));
        }
        self.moved(top as u16, bottom as u16);
        self.shift_flags(top as u16, bottom as u16, n as u16, &old);
    }

    fn insert_blanks(&mut self, x: u16, y: u16, n: u16) {
        if n == 0 || x < self.left || x > self.right {
            return;
        }
        let bg = self.scr().cursor.style;
        let bk_bg = self.styles.blank(bg);
        let right = self.right as usize;
        let l = self.line(y);
        let x = x as usize;
        let n = (n as usize).min(right - x + 1);
        l.cells[x..=right].rotate_right(n);
        for c in &mut l.cells[x..x + n] {
            *c = bk_bg;
        }
        self.fix_split_wide(y);
    }

    /// libghostty's `Screen.splitCellBoundary` on the cursor's row: makes `x`
    /// a clean boundary between cells. At the row's end, a wrapped row's
    /// spacer head goes; at 0 or 1 on a continued row starting with a wide
    /// character, the row above's spacer head goes; a wide character across
    /// `x` is cleared (both halves).
    fn split_cell_boundary(&mut self, x: u16) {
        let (cols, y, pen) = (self.cols, self.scr().cursor.y, self.scr().cursor.style);
        let bk_pen = self.styles.blank(pen);
        let (wrapped, cont, first) = {
            let l = &self.screen_ref().lines[y as usize];
            (l.wrapped, l.cont, l.cells[0].wide)
        };
        if x >= cols {
            if wrapped {
                let c = &mut self.line(y).cells[cols as usize - 1];
                if c.wide == Wide::SpacerHead {
                    *c = bk_pen;
                }
            }
            return;
        }
        if x <= 1 && cont && first == Wide::Wide && y > 0 {
            let c = &mut self.line(y - 1).cells[cols as usize - 1];
            if c.wide == Wide::SpacerHead {
                *c = bk_pen;
            }
        }
        if x == 0 {
            return;
        }
        let l = self.line(y);
        if l.cells[x as usize - 1].wide == Wide::Wide {
            l.cells[x as usize - 1] = bk_pen;
            l.cells[x as usize] = bk_pen;
        }
    }

    /// libghostty's `Screen.cursorResetWrap`: the cursor's row no longer
    /// wraps (its spacer head goes, the next row is no continuation).
    fn cursor_reset_wrap(&mut self) {
        let (cols, y, pen) = (self.cols as usize, self.scr().cursor.y, self.scr().cursor.style);
        let bk_pen = self.styles.blank(pen);
        self.cur().pending_wrap = false;
        if !self.screen_ref().lines[y as usize].wrapped {
            return;
        }
        let l = self.line(y);
        l.wrapped = false;
        if l.cells[cols - 1].wide == Wide::SpacerHead {
            l.cells[cols - 1] = bk_pen;
        }
        if y + 1 < self.rows {
            self.line(y + 1).cont = false;
        }
    }

    fn delete_chars(&mut self, n: u16) {
        let (x, y, bg) = {
            let c = self.scr().cursor;
            (c.x as usize, c.y, c.style)
        };
        let bk_bg = self.styles.blank(bg);
        if n == 0 || x < self.left as usize || x > self.right as usize {
            return;
        }
        let right = self.right as usize;
        let n = (n as usize).min(right - x + 1);
        // As libghostty's deleteChars.
        self.split_cell_boundary(x as u16);
        self.split_cell_boundary((x + n) as u16);
        self.split_cell_boundary(right as u16 + 1);
        let l = self.line(y);
        l.cells[x..=right].rotate_left(n);
        for c in &mut l.cells[right + 1 - n..=right] {
            *c = bk_bg;
        }
        self.fix_split_wide(y);
        self.cursor_reset_wrap();
    }

    /// Under ISO protection (SPA last), ECH leaves protected cells.
    fn erase_chars(&mut self, n: u16) {
        let protect = self.scr().protected_mode == 1;
        let (x, y, bg) = {
            let c = self.scr().cursor;
            (c.x as usize, c.y, c.style)
        };
        let bk_bg = self.styles.blank(bg);
        let cols = self.cols as usize;
        // As libghostty's eraseChars: a wide character at the end is taken
        // whole.
        let mut end = (x + n.max(1) as usize).min(cols);
        if end < cols && self.screen_ref().lines[y as usize].cells[end - 1].wide == Wide::Wide {
            end += 1;
        }
        self.split_cell_boundary(x as u16);
        self.split_cell_boundary(end as u16);
        self.cursor_reset_wrap();
        let l = self.line(y);
        for c in &mut l.cells[x..end] {
            if !(protect && c.protected) {
                *c = bk_bg;
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
        let bk_bg = self.styles.blank(bg);
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
                *c = bk_bg;
            }
        }
        if mode != 1 {
            l.wrapped = false;
        }
        self.cur().pending_wrap = false;
    }

    /// Rows `from..to` blank (protected cells stay when `protect`).
    fn erase_rows(&mut self, from: u16, to: u16, bg: Style, protect: bool) {
        let bk_bg = self.styles.blank(bg);
        let cols = self.cols;
        for r in from..to {
            if protect {
                for c in &mut self.line(r).cells {
                    if !c.protected {
                        *c = bk_bg;
                    }
                }
            } else {
                *self.line(r) = Line::new(cols, bk_bg);
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
            2 => {
                self.erase_rows(0, self.rows, bg, protect);
                kitty::screen_erased(self);
            }
            3 => {
                if !self.modes.alt_screen {
                    self.scrollback.clear();
                    self.sb_bytes = 0;
                    self.viewport = 0;
                    self.primary.gfx.history_erased();
                }
            }
            // libghostty's "scroll complete": the rows down to the last one
            // with content go to the scrollback, the screen is cleared.
            22 => {
                let st = &self.styles;
                let used = |l: &Line| l.cells.iter().any(|c| !c.is_empty() || !st.bg_default(c.style));
                let n = self.screen_ref().lines.iter().rposition(used).map_or(0, |r| r + 1);
                if !self.modes.alt_screen {
                    for r in 0..n {
                        let l = self.screen_ref().lines[r].clone();
                        self.push_scrollback(l);
                    }
                    self.primary.gfx.top_row += n as u64;
                }
                for r in 0..self.rows {
                    *self.line(r) = Line::new(cols, Cell::default());
                }
                kitty::screen_erased(self);
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
        let (x, y) = (self.scr().cursor.x, self.scr().cursor.y);
        if y < self.top || y > self.bottom || x < self.left || x > self.right {
            return;
        }
        if self.lr() {
            self.shift_columns(y, n, true);
            return;
        }
        let (top, bg, cols) = (self.top, self.scr().cursor.style, self.cols);
        let bk_bg = self.styles.blank(bg);
        let old = self.wrap_flags(y, self.bottom);
        self.top = y;
        for _ in 0..n.min(self.bottom - y + 1) {
            let b = self.bottom as usize;
            self.scr().lines.remove(b);
            self.scr().lines.insert(y as usize, Line::new(cols, bk_bg));
        }
        self.top = top;
        self.moved(y, self.bottom);
        self.shift_flags(y, self.bottom, n, &old);
        self.cur().x = 0;
        self.cur().pending_wrap = false;
    }

    fn delete_lines(&mut self, n: u16) {
        let (x, y) = (self.scr().cursor.x, self.scr().cursor.y);
        if y < self.top || y > self.bottom || x < self.left || x > self.right {
            return;
        }
        if self.lr() {
            self.shift_columns(y, n, false);
            return;
        }
        let (bg, cols) = (self.scr().cursor.style, self.cols);
        let bk_bg = self.styles.blank(bg);
        let old = self.wrap_flags(y, self.bottom);
        for _ in 0..n.min(self.bottom - y + 1) {
            self.scr().lines.remove(y as usize);
            let b = self.bottom as usize;
            self.scr().lines.insert(b, Line::new(cols, bk_bg));
        }
        self.moved(y, self.bottom);
        self.shift_flags(y, self.bottom, n, &old);
        self.cur().x = 0;
        self.cur().pending_wrap = false;
    }

    // ----------------------------------------------------- cursor moves

    /// libghostty's CHA/HPA/HPR under origin mode (`Compat`): its absolute
    /// move with the current row, which origin mode then offsets by the
    /// region's top.
    fn origin_column_moves_row(&mut self, col: u32) {
        let (top, bottom) = (self.top as u32, self.bottom as u32);
        let (left, right) = (self.left as u32, self.right as u32);
        let cur = self.cur();
        let row = cur.y as u32 + 1;
        cur.x = ((col.max(1) + left).min(right + 1) - 1) as u16;
        cur.y = ((row + top).min(bottom + 1) - 1) as u16;
        cur.pending_wrap = false;
    }

    /// Moves the cursor; with origin mode, rows are inside the region.
    fn goto(&mut self, x: u16, y: u16) {
        let (top, bottom, rows, cols) = (self.top, self.bottom, self.rows, self.cols);
        let (left, right) = (self.left, self.right);
        let origin = self.modes.origin;
        let cur = self.cur();
        cur.x = if origin { left.saturating_add(x).min(right) } else { x.min(cols - 1) };
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
        let (cols, right) = (self.cols as i32, self.right as i32);
        let cur = self.cur();
        let x = cur.x as i32;
        let max = if d > 0 && x <= right { right } else { cols - 1 };
        cur.x = (x + d).clamp(0, max) as u16;
        cur.pending_wrap = false;
    }

    /// CR, as libghostty: to the left margin, or column 0 when left of it
    /// (origin mode: always the margin).
    fn carriage_return(&mut self) {
        let (left, origin) = (self.left, self.modes.origin);
        let cur = self.cur();
        cur.x = if origin || cur.x >= left { left } else { 0 };
        cur.pending_wrap = false;
    }

    /// HT, as libghostty: to the next stop, at most to the right margin.
    fn tab(&mut self, n: u16) {
        let right = self.right;
        for _ in 0..n.max(1) {
            let mut x = self.scr().cursor.x;
            while x < right {
                x += 1;
                if self.tabs[x as usize] {
                    break;
                }
            }
            self.cur().x = x;
        }
    }

    /// CBT, as libghostty: to the previous stop, at most to column 0 (the
    /// left margin under origin mode).
    fn back_tab(&mut self, n: u16) {
        let limit = if self.modes.origin { self.left } else { 0 };
        for _ in 0..n.max(1) {
            let mut x = self.scr().cursor.x;
            while x > limit {
                x -= 1;
                if self.tabs[x as usize] {
                    break;
                }
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
        // The shape is not part of what DECSC saves, nor the hyperlink (as
        // libghostty).
        let (shape, link) = (cur.shape, cur.style.link);
        *cur = s.cursor;
        cur.shape = shape;
        cur.style.link = link;
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

    /// Modes 47, 1047 and 1049, as libghostty's `switchScreenMode` (itself
    /// from xterm's charproc.c): 1047 erases the alternate screen on the way
    /// out, 1049 saves the cursor and erases it on the way in (also when
    /// already there), each erase an ED 2 with the alternate screen's own
    /// cursor (its pen, character protection); the cursor is copied across
    /// when the screen changes, and 1049 restores it on the way out.
    fn set_alt_screen(&mut self, mode: u16, on: bool) {
        match mode {
            1047 if !on && self.modes.alt_screen => self.erase_display(2, false),
            1049 if on => self.save_cursor(),
            _ => {}
        }
        let changed = on != self.modes.alt_screen;
        // A change of screen ends the hyperlink of the screen left
        // (libghostty's `switchScreen`), and the copy does not take one
        // (`cursorCopy(.., .{ .hyperlink = false })`): no link survives it.
        if changed {
            self.scr().cursor.style.link = 0;
        }
        let old = self.scr().cursor;
        if changed {
            self.modes.alt_screen = on;
            self.all_dirty = true;
            self.viewport = 0;
        }
        match mode {
            1049 if on => {
                self.erase_display(2, false);
                if changed {
                    self.alt.cursor = old;
                }
            }
            1049 => self.restore_cursor(),
            _ if changed => *self.cur() = old,
            _ => {}
        }
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
        // As libghostty: the reset storage has its own (higher) limit.
        self.primary.gfx.limit = kitty::LIMIT_RESET;
        self.alt.gfx.limit = kitty::LIMIT_RESET;
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
                self.set_alt_screen(47, on)
            }
            66 => self.modes.app_keypad = on,
            69 => {
                self.modes.lr_margins = on;
                if !on {
                    self.left = 0;
                    self.right = self.cols - 1;
                }
            }
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
                self.set_alt_screen(1047, on)
            }
            1048 => {
                if on {
                    self.save_cursor()
                } else {
                    self.restore_cursor()
                }
            }
            1049 => {
                self.modes.alt_modes[2] = on;
                self.set_alt_screen(1049, on)
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
        // The hyperlink is not part of the style in libghostty: SGR 0 keeps it.
        style.link = self.scr().cursor.style.link;
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
    /// An APC string (`ESC _ … ESC \`, also SOS and PM); its contents are
    /// in `buf`.
    Apc,
    /// A byte 0x80–0x9F outside a UTF-8 sequence.
    RawC1,
    /// A C0 byte 0x10–0x1F (not ESC) in the ground state.
    GroundC0,
    /// A C1 byte inside an escape or CSI sequence: vte gets its 7-bit form.
    C1(u8),
    /// The byte that ends an OSC (BEL, CAN, SUB, ESC): vte dispatches it on
    /// this byte, with its whole payload in `osc`.
    Osc,
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
    /// The kitty command in `buf` passed libghostty's size limit: void.
    apc_over: bool,
    /// The current (or last) OSC's payload as vte collects it, all of it:
    /// vte splits only 16 parameters and drops what follows them.
    osc: Vec<u8>,
}

impl Scanner {
    fn keep_param(&mut self, b: u8) {
        if self.csi_raw.len() < 512 {
            self.csi_raw.push(b);
        }
    }

    /// A kitty graphics command (`G…`) is kept whole, up to libghostty's
    /// limit (past it the command is void); other strings up to `KEEP`.
    fn keep_apc(&mut self, b: u8) {
        let kitty = self.buf.first() == Some(&b'G');
        let cap = if kitty { kitty::MAX_PAYLOAD + Self::KEEP } else { Self::KEEP };
        if self.apc_over {
            return;
        }
        if self.buf.len() < cap {
            self.buf.push(b);
        } else if kitty {
            self.buf = Vec::new();
            self.apc_over = true;
        }
    }

    /// Follows UTF-8 in the ground state as vte's decoder does, to tell a raw
    /// C1 byte from a continuation byte.
    /// How many bytes at the start of `bytes` continue (and may complete)
    /// the UTF-8 character the last input ended in (none outside the ground
    /// state).
    fn continuation(&self, bytes: &[u8]) -> usize {
        if self.state != Self::GROUND {
            return 0;
        }
        let mut range = self.utf8_next;
        let mut k = 0;
        while k < self.utf8_need as usize && bytes.get(k).is_some_and(|b| (range.0..=range.1).contains(b)) {
            k += 1;
            range = (0x80, 0xbf);
        }
        k
    }

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
    /// DCS, its header and data.
    const DCS: u8 = 6;
    /// An OSC string: only to know that a CAN or SUB there ends it without
    /// being a ground-state control (vte executes it; libghostty does not).
    const OSC: u8 = 7;
    /// OSC payload kept (`osc`); more is dropped.
    const OSC_KEEP: usize = 1 << 20;
    /// What is kept of strings other than kitty graphics commands.
    const KEEP: usize = 4096;

    fn byte(&mut self, b: u8) -> Pre {
        // As libghostty (a VT500 parser): inside an escape or CSI sequence a
        // C1 byte acts as its 7-bit form, ESC and b - 0x40 (vte ignores it).
        // The same in SOS/PM/APC strings and DCS (not in OSC, whose
        // payload is UTF-8).
        if matches!(self.state, Self::ESC | Self::ESC_INT | Self::CSI | Self::DCS | Self::APC)
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
            (Self::OSC, 0x1b) => {
                self.state = Self::ESC;
                return Pre::Osc;
            }
            (Self::ESC | Self::ESC_INT | Self::CSI | Self::DCS, 0x1b) => self.state = Self::ESC,
            (Self::ESC | Self::ESC_INT | Self::CSI | Self::DCS, 0x18 | 0x1a) => self.state = Self::GROUND,
            // vte ends the OSC there and executes the byte, which is then no
            // ground-state control (`Pre::GroundC0`): nothing is printed.
            (Self::OSC, 0x07 | 0x18 | 0x1a) => {
                self.state = Self::GROUND;
                return Pre::Osc;
            }
            // As vte: other C0 controls are not part of the string.
            (Self::OSC, 0x00..=0x1f) => {}
            (Self::OSC, _) => {
                if self.osc.len() < Self::OSC_KEEP {
                    self.osc.push(b);
                }
            }
            (Self::DCS, _) => {}
            (Self::ESC, b']') => {
                self.state = Self::OSC;
                self.osc.clear();
            }
            (Self::ESC, b'P') => self.state = Self::DCS,
            // As libghostty: SOS and PM strings are read as APC ones.
            (Self::ESC, b'_' | b'X' | b'^') => {
                self.state = Self::APC;
                self.buf.clear();
                self.apc_over = false;
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
            // As libghostty: CAN and SUB end it too, and it still runs.
            (Self::APC, 0x18 | 0x1a) => {
                self.state = Self::GROUND;
                return Pre::Apc;
            }
            // As libghostty: bytes from 0xA0 up are not part of it.
            (Self::APC, 0xa0..=0xff) => {}
            (Self::APC, _) => self.keep_apc(b),
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
        l.cells.resize(n, Cell::default());
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

/// One SGR attribute other than an extended colour.
fn sgr_simple(style: &mut Style, p: &[u16]) {
    let v = p[0];
    match v {
        0 => *style = Style::default(),
        1 => style.flags |= flags::BOLD,
        2 => style.flags |= flags::FAINT,
        3 => style.flags |= flags::ITALIC,
        // `4:N` picks the kind (an unknown one is single), as libghostty.
        4 => set_underline(
            style,
            match p.get(1) {
                None => underline::SINGLE,
                Some(&k) if k <= 5 => k as u8,
                Some(_) => underline::SINGLE,
            },
        ),
        5 | 6 => style.flags |= flags::BLINK,
        7 => style.flags |= flags::INVERSE,
        8 => style.flags |= flags::INVISIBLE,
        9 => style.flags |= flags::STRIKETHROUGH,
        21 => set_underline(style, underline::DOUBLE),
        22 => style.flags &= !(flags::BOLD | flags::FAINT),
        23 => style.flags &= !flags::ITALIC,
        24 => set_underline(style, underline::NONE),
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
        59 => style.ul = Color::Default,
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
        (58, Some(c)) => style.ul = c,
        _ => {}
    }
}

/// The underline's kind; `flags::UNDERLINE` says whether there is one.
fn set_underline(style: &mut Style, kind: u8) {
    style.underline = kind;
    if kind == underline::NONE {
        style.flags &= !flags::UNDERLINE;
    } else {
        style.flags |= flags::UNDERLINE;
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
fn empty(c: &Cell, st: &Styles) -> bool {
    c.is_empty() && c.wide == Wide::Narrow && st.bg_default(c.style)
}

/// One screen through a resize (see `Terminal::resize`). `sb`: its
/// scrollback (`None`: lines pushed out are dropped).
#[allow(clippy::too_many_arguments)]
fn resize_screen(
    s: &mut Screen,
    sb: Option<&mut VecDeque<Line>>,
    old_cols: u16,
    old_rows: u16,
    cols: u16,
    rows: u16,
    reflow: bool,
    st: &Styles,
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
    // Rows with kitty placements are not trimmed either (libghostty's
    // tracked pins).
    let pins = s.gfx.pin_lines(hist);
    if !reflow || cols == old_cols {
        if cols != old_cols {
            all = all.into_iter().map(|l| cut_line(l, cols)).collect();
            cursor.x = cursor.x.min(cols - 1);
            if let Some(p) = &mut saved {
                p.x = p.x.min(cols - 1);
            }
        }
        resize_rows(&mut all, &mut active_rows, rows as usize, &mut cursor, saved.as_ref(), &pins);
    } else if cols > old_cols {
        all = reflow_lines(all, active_rows, cols, &mut cursor, saved.as_mut(), st);
        resize_rows(&mut all, &mut active_rows, rows as usize, &mut cursor, saved.as_ref(), &pins);
    } else {
        resize_rows(&mut all, &mut active_rows, rows as usize, &mut cursor, saved.as_ref(), &pins);
        all = reflow_lines(all, active_rows, cols, &mut cursor, saved.as_mut(), st);
    }
    // The active area is the last `rows` lines.
    while all.len() < rows as usize {
        all.push(Line::new(cols, Cell::default()));
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
fn resize_rows(
    all: &mut Vec<Line>,
    active_rows: &mut usize,
    rows: usize,
    cursor: &mut Pin,
    saved: Option<&Pin>,
    pins: &[usize],
) {
    let old = *active_rows;
    if rows < old {
        let mut trimmed = 0;
        while trimmed < old - rows {
            let i = all.len() - 1;
            let l = &all[i];
            if l.cells.iter().any(|c| !c.is_empty())
                || cursor.line == i
                || saved.is_some_and(|p| p.line == i)
                || pins.contains(&i)
            {
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
                all.push(Line::new(cols, Cell::default()));
            }
        } else {
            while all.len() < rows {
                all.push(Line::new(cols, Cell::default()));
            }
        }
    }
    *active_rows = rows;
}

/// Reflows every line to `cols` (`resizeCols`): soft-wrapped lines join and
/// are cut again, trailing empty cells of a hard line end are dropped,
/// blank lines are written only when content follows them, and as many rows
/// stay below the cursor as before.
fn reflow_lines(
    all: Vec<Line>,
    rows: usize,
    cols: u16,
    cursor: &mut Pin,
    mut saved: Option<&mut Pin>,
    st: &Styles,
) -> Vec<Line> {
    let n = cols as usize;
    let continuation = |all: &[Line], i: usize| all[i].cont;
    let top = all.len().saturating_sub(rows);
    let wrapped_in = |all: &[Line], from: usize, to: usize| (from..=to).filter(|&i| continuation(all, i)).count();
    let (cursor_in, remaining, wrapped_before) = if cursor.line >= top {
        (true, rows.saturating_sub(cursor.line - top + 1), wrapped_in(&all, top, cursor.line))
    } else {
        (false, 0, 0)
    };
    let mut out: Vec<Line> = vec![Line::new(cols, Cell::default())];
    let (mut x, mut pending) = (0usize, false);
    let mut new_rows = 0usize;
    let (mut cur_new, mut saved_new) = (None, None);
    for (i, src) in all.iter().enumerate() {
        let mut len = src.cells.len();
        if !src.wrapped {
            while len > 0 && empty(&src.cells[len - 1], st) {
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
            out.push(Line::new(cols, Cell::default()));
            x = 0;
            pending = false;
            new_rows -= 1;
        }
        let mut sx = 0;
        while sx < len {
            if pending {
                out.last_mut().expect("a line").wrapped = true;
                out.push(Line { cont: true, ..Line::new(cols, Cell::default()) });
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
                    out.last_mut().expect("a line").cells[x] = blank(st.erased(c.style));
                    pending = true;
                    sx += 2;
                    continue;
                }
                Wide::Wide if x == n - 1 => {
                    out.last_mut().expect("a line").cells[x] = Cell { wide: Wide::SpacerHead, ..Cell::default() };
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
            out.push(Line::new(cols, Cell::default()));
        }
        let top = out.len() - rows;
        if cursor.line >= top {
            let wrapped = wrapped_in(&out, top, cursor.line);
            let current = rows.saturating_sub(cursor.line - top + 1);
            let req = remaining.saturating_sub(wrapped.saturating_sub(wrapped_before)).saturating_sub(current);
            for _ in 0..req {
                out.push(Line::new(cols, Cell::default()));
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

impl Terminal {
    /// OSC 8 `params ; uri`, as libghostty's `osc/parsers/hyperlink.zig`:
    /// the parameters are `key=value` pairs between colons, read up to the
    /// first one without `=`; `id` with a value names the link. An empty URI
    /// ends the link, unless an id was given (then nothing happens).
    fn osc_hyperlink(&mut self, kvs: &[u8], uri: &[u8]) {
        // As libghostty's loop over NUL-separated pairs: a pair runs from
        // its start to the next separator after its first byte.
        let mut kvs = kvs.to_vec();
        kvs.push(0);
        for b in &mut kvs {
            if *b == b':' {
                *b = 0;
            }
        }
        let mut id: Option<&[u8]> = None;
        let mut start = 0;
        while start < kvs.len() {
            let Some(end) = kvs[start + 1..].iter().position(|&b| b == 0).map(|p| p + start + 1) else { break };
            let kv = &kvs[start..end];
            let Some(eq) = kv.iter().position(|&b| b == b'=') else { break };
            if &kv[..eq] == b"id" && eq + 1 < kv.len() {
                id = Some(&kvs[start + eq + 1..end]);
            }
            start = end + 1;
        }
        if uri.is_empty() {
            if id.is_none() {
                self.cur().style.link = 0;
            }
            return;
        }
        let id = match id {
            Some(i) => LinkId::Explicit(i.into()),
            None => {
                let n = self.links.implicit;
                self.links.implicit = n.wrapping_add(1);
                LinkId::Implicit(n)
            }
        };
        let l = self.links.id(Link { id, uri: uri.into() });
        self.cur().style.link = l;
        self.link_page = self.cursor_page();
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
                self.linefeed();
                if self.modes.newline {
                    self.carriage_return();
                }
            }
            0x0d => self.carriage_return(),
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
            ([], 'H' | 'f' | 'r' | 's') => 2,
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
            ([], 'd' | 'e') if self.compat.has(Compat::ORIGIN_ROW_MOVES_COLUMN) && self.modes.origin => {
                let c = self.scr().cursor;
                let row = if action == 'd' { p0(1) as u32 } else { c.y as u32 + 1 + p0(1) as u32 };
                let (top, bottom, left, right) =
                    (self.top as u32, self.bottom as u32, self.left as u32, self.right as u32);
                let cur = self.cur();
                cur.x = ((c.x as u32 + 1 + left).min(right + 1) - 1) as u16;
                cur.y = ((row.max(1) + top).min(bottom + 1) - 1) as u16;
                cur.pending_wrap = false;
            }
            ([], '@') => {
                let (x, y) = (self.scr().cursor.x, self.scr().cursor.y);
                self.insert_blanks(x, y, p0(1));
                self.cur().pending_wrap = false;
            }
            ([], 'A' | 'k') => self.move_vertical(-(p0(1) as i32)),
            ([], 'B' | 'e') => self.move_vertical(p0(1) as i32),
            ([], 'a') if self.compat.has(Compat::ORIGIN_COLUMN_MOVES_ROW) && self.modes.origin => {
                let x = self.scr().cursor.x as u32;
                self.origin_column_moves_row(x + 1 + p0(1) as u32);
            }
            ([], 'a') => {
                let (cols, right, origin) = (self.cols, self.right, self.modes.origin);
                let cur = self.cur();
                cur.x = cur.x.saturating_add(p0(1)).min(if origin { right } else { cols - 1 });
                cur.pending_wrap = false;
            }
            ([], 'C') => {
                let n = p0(1) as i32 + self.compat.has(Compat::SELFTEST_BREAK) as i32;
                self.move_horizontal(n)
            }
            ([], 'D' | 'j') => self.move_horizontal(-(p0(1) as i32)),
            ([], 'E') => {
                self.move_vertical(p0(1) as i32);
                self.carriage_return();
            }
            ([], 'F') => {
                self.move_vertical(-(p0(1) as i32));
                self.carriage_return();
            }
            ([], 'G' | '`') if self.compat.has(Compat::ORIGIN_COLUMN_MOVES_ROW) && self.modes.origin => {
                self.origin_column_moves_row(p0(1) as u32);
            }
            ([], 'G' | '`') => {
                let (cols, left, right, origin) = (self.cols, self.left, self.right, self.modes.origin);
                let cur = self.cur();
                cur.x = if origin { left.saturating_add(p0(1) - 1).min(right) } else { (p0(1) - 1).min(cols - 1) };
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
            ([], 'S') => {
                let n = p0(1);
                self.scroll_up(n);
                kitty::region_scrolled(self, n, false);
            }
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
                self.goto(0, p0(1) - 1);
                self.cur().x = x;
            }
            // As libghostty: TBC needs its parameter.
            ([], 'g') if count == 0 => {}
            ([], 'g') => match param(params, 0, 0) {
                0 => {
                    let x = self.scr().cursor.x as usize;
                    // ECMA-48 §8.3.154: clears (libghostty toggles; Compat).
                    self.tabs[x] = self.compat.has(Compat::TBC_TOGGLE) && !self.tabs[x];
                }
                3 => self.tabs.iter_mut().for_each(|t| *t = false),
                _ => {}
            },
            // CTC (ECMA-48 §8.3.17), as libghostty takes it: 0 (or none) sets
            // a tab stop at the cursor, 2 clears it (libghostty toggles, as
            // with TBC 0), 5 clears them all; anything else, or more
            // parameters, is ignored. DECST8C (`CSI ? 5 W`) resets them.
            ([], 'W') if count == 0 || (count == 1 && param(params, 0, 0) == 0) => {
                let x = self.scr().cursor.x as usize;
                self.tabs[x] = true;
            }
            ([], 'W') if count == 1 => match param(params, 0, 0) {
                2 => {
                    let x = self.scr().cursor.x as usize;
                    self.tabs[x] = self.compat.has(Compat::TBC_TOGGLE) && !self.tabs[x];
                }
                5 => self.tabs.iter_mut().for_each(|t| *t = false),
                _ => {}
            },
            ([b'?'], 'W') if count == 1 && param(params, 0, 0) == 5 => self.tabs = default_tabs(self.cols),
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
            // XTSAVE and XTRESTORE, for the modes DECRQM knows (as libghostty;
            // a mode never saved restores to its default).
            ([b'?'], 's') => {
                for p in params.iter() {
                    let st = self.mode_state(p[0]);
                    if st != 0 {
                        self.saved_modes.insert(p[0], st == 1);
                    }
                }
            }
            ([b'?'], 'r') => {
                for p in params.iter() {
                    if self.mode_state(p[0]) != 0 {
                        let v =
                            self.saved_modes.get(&p[0]).copied().unwrap_or(matches!(p[0], 7 | 25 | 1007 | 1035 | 1036));
                        self.set_mode(true, p[0], v);
                    }
                }
            }
            // As libghostty: only without parameters (with them it would be
            // DECSLRM, which needs mode 69).
            ([], 's') if count == 0 && !self.modes.lr_margins => self.save_cursor(),
            // DECSLRM, as libghostty's setLeftAndRightMargin: a valid pair
            // sets the margins and the cursor goes home.
            ([], 's') if self.modes.lr_margins => {
                let cols = self.cols;
                let l = param(params, 0, 0).max(1);
                let r = match param(params, 1, 0) {
                    0 => cols,
                    r => r.min(cols),
                };
                if l < r {
                    self.left = l - 1;
                    self.right = r - 1;
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
                self.linefeed();
                self.carriage_return();
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
                // Only the colours stay in the pen (its hyperlink too, but
                // the cells get none).
                let pen = self.scr().cursor.style;
                let style = self.styles.id(Style { fg: pen.fg, bg: pen.bg, ..Style::default() });
                self.cur().style = Style { fg: pen.fg, bg: pen.bg, link: pen.link, ..Style::default() };
                for y in 0..rows {
                    for x in 0..cols {
                        self.line(y).cells[x as usize] = Cell::of('E', style, Wide::Narrow);
                    }
                    let l = self.line(y);
                    l.wrapped = false;
                    l.cont = false;
                }
                // DECALN also resets the scroll region, the margins and
                // origin mode.
                self.modes.origin = false;
                self.top = 0;
                self.bottom = rows - 1;
                self.left = 0;
                self.right = self.cols - 1;
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
        // As libghostty: an OSC that is not valid UTF-8 is dropped whole —
        // but OSC 8, whose parser takes the bytes as they are.
        if code != "8" && params.iter().any(|p| std::str::from_utf8(p).is_err()) {
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
                // The whole rest, semicolons included (as libghostty).
                let p = params[1..].iter().map(|p| String::from_utf8_lossy(p)).collect::<Vec<_>>().join(";");
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
            "8" if params.len() >= 3 => {
                // From the scanner's copy: vte keeps only 16 parameters.
                let raw = std::mem::take(&mut self.pre.osc);
                let rest = raw.strip_prefix(b"8;").unwrap_or(&[]);
                if let Some(p) = rest.iter().position(|&c| c == b';') {
                    self.osc_hyperlink(&rest[..p], &rest[p + 1..]);
                }
                self.pre.osc = raw;
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

    /// 200 000 distinct colours through a small scrollback: the table stays
    /// bounded, and the cells still on screen keep their colours.
    #[test]
    fn style_table_stays_bounded() {
        let mut t = term(20, 5);
        t.sb_limit = 64 << 10;
        for i in 0..200_000u32 {
            let s = format!("\x1b[38;2;{};{};{}mx\r\n", i >> 16, (i >> 8) & 255, i & 255);
            t.feed(s.as_bytes());
        }
        let (n, _) = t.style_table();
        assert!(n < u16::MAX as usize, "{n} styles");
        t.feed(b"\x1b[38;2;1;2;3mA");
        let y = t.scr().cursor.y;
        let c = t.cell(0, y);
        assert_eq!(t.style(&c).fg, Color::Rgb([1, 2, 3]));
        // The row above shows the last colour of the loop.
        let above = t.cell(0, y - 1);
        assert_eq!(t.style(&above).fg, Color::Rgb([3, 13, 63]));
    }

    /// The link of cell (x, y): its id and target.
    fn link_at(t: &Terminal, x: u16, y: u16) -> Option<(LinkId, String)> {
        let c = t.cell(x, y);
        t.link(t.style(&c).link).map(|l| (l.id.clone(), String::from_utf8_lossy(&l.uri).into_owned()))
    }

    /// OSC 8 as libghostty's parser: ids, pairs read up to one without
    /// `=`, an empty target ending the link unless an id came with it.
    #[test]
    fn hyperlinks_open_and_end() {
        let mut t = term(20, 3);
        t.feed(b"\x1b]8;id=a:k=v;http://x;y\x07A\x1b]8;;\x07B");
        assert_eq!(link_at(&t, 0, 0), Some((LinkId::Explicit((*b"a").into()), "http://x;y".into())));
        assert_eq!(link_at(&t, 1, 0), None);
        // Two links without an id stay two, even to one target.
        t.feed(b"\x1b]8;;u\x07C\x1b]8;;u\x07D");
        let (c, d) = (link_at(&t, 2, 0).unwrap(), link_at(&t, 3, 0).unwrap());
        assert!(matches!(c.0, LinkId::Implicit(_)) && c.1 == "u" && c != d);
        // An id with an empty target does nothing; a key without `=` ends
        // the reading (the id after it is not seen).
        t.feed(b"\x1b]8;id=1;\x07E\x1b]8;k:id=z;v\x07F\x1b]8;;\x07");
        assert_eq!(link_at(&t, 4, 0).map(|l| l.1), Some("u".into()));
        assert!(matches!(link_at(&t, 5, 0), Some((LinkId::Implicit(_), _))));
        // No `;` after the parameters: not a link at all.
        t.feed(b"\x1b]8;id=q\x07G");
        assert_eq!(link_at(&t, 6, 0), None);
    }

    /// SGR 0 and DECRC keep the link (not part of the style in libghostty);
    /// the alternate screens start without one; erasing drops it.
    #[test]
    fn what_keeps_a_link() {
        let mut t = term(20, 3);
        t.feed(b"\x1b7\x1b]8;;u\x07\x1b[1;4mA\x1b[0mB\x1b8C");
        for x in 0..2 {
            assert!(link_at(&t, x, 0).is_some(), "col {x}");
        }
        // DECRC put the cursor back on column 0: C wrote over A, linked.
        assert!(link_at(&t, 0, 0).is_some());
        t.feed(b"\x1b[?1049hD");
        assert_eq!(link_at(&t, 0, 0), None);
        t.feed(b"\x1b[?1049lE");
        assert_eq!(link_at(&t, 1, 0), None, "a change of screen ends the link");
        assert!(link_at(&t, 0, 0).is_some(), "cells keep theirs");
        t.feed(b"\x1b[1;1H\x1b[X");
        assert_eq!(link_at(&t, 0, 0), None, "erased");
        t.feed(b"\x1b#8");
        assert_eq!(link_at(&t, 0, 0), None, "DECALN writes without links");
    }

    /// CTC sets (0, none) and clears (2, 5) tab stops; DECST8C resets them.
    #[test]
    fn ctc_and_decst8c() {
        let col = |t: &mut Terminal, s: &[u8]| {
            t.feed(s);
            t.cursor().map(|c| c.0)
        };
        let mut t = term(20, 3);
        assert_eq!(col(&mut t, b"AB\x1b[W\r\t"), Some(2));
        assert_eq!(col(&mut t, b"\x1b[2W\r\t"), Some(8), "cleared, not toggled");
        assert_eq!(col(&mut t, b"\x1b[5W\r\t"), Some(19), "all cleared");
        assert_eq!(col(&mut t, b"\x1b[?5W\r\t"), Some(8), "every 8 again");
        assert_eq!(col(&mut t, b"\x1b[3G\x1b[1;2W\r\t"), Some(8), "two parameters: ignored");
    }

    /// A character split across inputs, followed by an ASCII one and the
    /// start of another: vte 0.15 alone would lose the ASCII one (E5a).
    #[test]
    fn utf8_split_across_inputs() {
        let mut t = term(20, 3);
        t.feed(b"\xd0");
        t.feed(b"\x96e\xea");
        t.feed(b"\xb0\x80");
        assert_eq!(
            (t.text(&t.cell(0, 0)), t.text(&t.cell(1, 0)), t.text(&t.cell(2, 0))),
            ("Ж".into(), "e".into(), "가".into())
        );
        // A combining mark completed the same way, with nothing to join.
        let mut t = term(20, 3);
        t.feed(b"\xcc");
        t.feed(b"\x81e\xcc");
        assert_eq!(t.text(&t.cell(0, 0)), "e");
        assert_eq!(t.cursor(), Some((1, 0)));
    }

    /// SGR 4:x (an unknown kind is single), 21, 24, 58 and 59.
    #[test]
    fn underline_kinds_and_colours() {
        let mut t = term(20, 3);
        t.feed(b"\x1b[4:3mA\x1b[21mB\x1b[4:9mC\x1b[4:0mD\x1b[58;5;3;4mE\x1b[59mF\x1b[24mG");
        let s = |x: u16| t.style(&t.cell(x, 0));
        let k = |x: u16| (s(x).flags & flags::UNDERLINE != 0, s(x).underline);
        assert_eq!(
            (0..7).map(k).collect::<Vec<_>>(),
            vec![(true, 3), (true, 2), (true, 1), (false, 0), (true, 1), (true, 1), (false, 0)]
        );
        assert_eq!(s(4).ul, Color::Index(3));
        assert_eq!(s(5).ul, Color::Default);
    }

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
        assert_eq!(std::mem::size_of::<Cell>(), 8);
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
