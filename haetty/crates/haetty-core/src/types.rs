//! Plain, `Send` data exchanged with the engine thread. libghostty-vt objects
//! are `!Send`, so nothing from that crate crosses the thread boundary except
//! `Copy` enums and bitflags.

pub use libghostty_vt::key::{Action as KeyAction, Key, KittyKeyFlags, Mods};
pub use libghostty_vt::mouse::{Action as MouseAction, Button as MouseButton};

pub const MAX_COLS: u16 = 4096;
pub const MAX_ROWS: u16 = 2048;

/// Grid size plus the cell size in pixels the frontend draws with. The pixel
/// values feed size reports (CSI 14/16/18 t), mouse pixel→cell conversion and
/// the PTY's winsize.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridSize {
    pub cols: u16,
    pub rows: u16,
    pub cell_width: u16,
    pub cell_height: u16,
}

impl GridSize {
    pub fn new(cols: u16, rows: u16) -> Self {
        // A nominal cell for headless use; GUI frontends pass real metrics.
        Self { cols, rows, cell_width: 9, cell_height: 18 }
    }
    /// Within what a session accepts: 1..=[`MAX_COLS`] by 1..=[`MAX_ROWS`]
    /// (stage S4: a 65535 x 65535 resize from a client ended the engine).
    pub fn clamped(self) -> Self {
        Self { cols: self.cols.clamp(1, MAX_COLS), rows: self.rows.clamp(1, MAX_ROWS), ..self }
    }
    pub fn with_cell(mut self, w: u16, h: u16) -> Self {
        self.cell_width = w.max(1);
        self.cell_height = h.max(1);
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl From<libghostty_vt::style::RgbColor> for Rgb {
    fn from(c: libghostty_vt::style::RgbColor) -> Self {
        Self { r: c.r, g: c.g, b: c.b }
    }
}

/// Cell attribute bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct CellFlags(pub u16);

impl CellFlags {
    pub const BOLD: u16 = 1 << 0;
    pub const ITALIC: u16 = 1 << 1;
    pub const FAINT: u16 = 1 << 2;
    pub const UNDERLINE: u16 = 1 << 3;
    pub const STRIKETHROUGH: u16 = 1 << 4;
    pub const INVERSE: u16 = 1 << 5;
    pub const INVISIBLE: u16 = 1 << 6;
    pub const BLINK: u16 = 1 << 7;
    pub const OVERLINE: u16 = 1 << 8;
    pub const SELECTED: u16 = 1 << 9;
    /// Drawn by haetty itself (tab bar, status row, panels, palette, hints),
    /// not by the program: set by the GUI when it composes the window, never
    /// by the engine. The renderer leaves these colours as they are
    /// (`minimum-contrast` applies to the program's cells only).
    pub const HAETTY_UI: u16 = 1 << 10;
    /// The active tab in haetty's tab bar: the renderer draws a thin line
    /// in the text colour along the cell's bottom edge (set by the GUI only).
    pub const TAB_ACTIVE: u16 = 1 << 11;
    pub fn has(self, bit: u16) -> bool {
        self.0 & bit != 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Width {
    #[default]
    Narrow,
    /// First cell of a double-width character.
    Wide,
    /// Second half of a wide character; draw nothing.
    Spacer,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Cell {
    /// Grapheme cluster as UTF-8; empty for a blank cell.
    pub text: String,
    /// `None` means the default foreground/background.
    pub fg: Option<Rgb>,
    pub bg: Option<Rgb>,
    pub flags: CellFlags,
    pub width: Width,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorShape {
    Block,
    Bar,
    Underline,
    BlockHollow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cursor {
    pub x: u16,
    pub y: u16,
    pub shape: CursorShape,
    pub blinking: bool,
}

/// Terminal modes a frontend or the status panel cares about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Modes {
    pub alt_screen: bool,
    pub mouse_tracking: bool,
    pub sgr_mouse: bool,
    pub bracketed_paste: bool,
    pub focus_events: bool,
    pub sync_output: bool,
    pub app_cursor_keys: bool,
    pub cursor_visible: bool,
    pub kitty_keyboard: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Scrollbar {
    pub total: u64,
    pub offset: u64,
    pub len: u64,
}

/// A complete, self-contained picture of the viewport.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub cols: u16,
    pub rows: u16,
    /// Row-major, `rows * cols` cells.
    pub cells: Vec<Cell>,
    pub cursor: Option<Cursor>,
    pub default_fg: Rgb,
    pub default_bg: Rgb,
    pub cursor_color: Option<Rgb>,
    pub title: String,
    pub pwd: String,
    pub modes: Modes,
    pub scrollbar: Scrollbar,
    /// Engine-side counter, increases with every snapshot.
    pub seq: u64,
}

impl Snapshot {
    pub fn cell(&self, x: u16, y: u16) -> Option<&Cell> {
        if x >= self.cols || y >= self.rows {
            return None;
        }
        self.cells.get(y as usize * self.cols as usize + x as usize)
    }

    /// One row as text (wide-char spacers skipped, trailing blanks trimmed).
    pub fn row_text(&self, y: u16) -> String {
        let mut s = String::new();
        for x in 0..self.cols {
            let c = &self.cells[y as usize * self.cols as usize + x as usize];
            match c.width {
                Width::Spacer => {}
                _ if c.text.is_empty() => s.push(' '),
                _ => s.push_str(&c.text),
            }
        }
        s.truncate(s.trim_end().len());
        s
    }

    /// The viewport as plain text, one line per row, trailing blank rows removed.
    pub fn text(&self) -> String {
        let mut lines: Vec<String> = (0..self.rows).map(|y| self.row_text(y)).collect();
        while lines.last().is_some_and(|l| l.is_empty()) {
            lines.pop();
        }
        lines.join("\n")
    }

    pub fn contains(&self, needle: &str) -> bool {
        (0..self.rows).any(|y| self.row_text(y).contains(needle))
    }
}

/// A key event in terms the libghostty key encoder understands. Frontends map
/// their platform key codes to [`Key`] (physical key) plus the produced text.
#[derive(Clone, Debug)]
pub struct KeyInput {
    pub key: Key,
    pub action: KeyAction,
    pub mods: Mods,
    /// Modifiers already accounted for in `text` (e.g. Shift for 'A').
    pub consumed_mods: Mods,
    pub text: Option<String>,
    /// The character the key produces without modifiers (kitty protocol).
    pub unshifted: Option<char>,
    /// True while an IME composition is in progress.
    pub composing: bool,
}

impl KeyInput {
    pub fn press(key: Key) -> Self {
        Self {
            key,
            action: KeyAction::Press,
            mods: Mods::empty(),
            consumed_mods: Mods::empty(),
            text: None,
            unshifted: None,
            composing: false,
        }
    }
    pub fn with_mods(mut self, mods: Mods) -> Self {
        self.mods = mods;
        self
    }
    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }
    pub fn with_unshifted(mut self, c: char) -> Self {
        self.unshifted = Some(c);
        self
    }

    /// Parse names like `Enter`, `shift+Enter`, `ctrl+c`, `ArrowUp`, `F5`.
    /// Single printable characters produce text as well.
    pub fn parse(spec: &str) -> Option<Self> {
        let mut mods = Mods::empty();
        let mut parts: Vec<&str> = spec.split('+').collect();
        let name = parts.pop()?;
        for m in parts {
            match m.to_ascii_lowercase().as_str() {
                "shift" => mods |= Mods::SHIFT,
                "ctrl" | "control" => mods |= Mods::CTRL,
                "alt" | "opt" | "option" => mods |= Mods::ALT,
                "super" | "cmd" | "win" | "meta" => mods |= Mods::SUPER,
                _ => return None,
            }
        }
        let (key, ch) = key_from_name(name)?;
        let mut k = KeyInput::press(key).with_mods(mods);
        if let Some(c) = ch {
            k.unshifted = Some(c);
            if !mods.intersects(Mods::CTRL | Mods::ALT | Mods::SUPER) {
                let t = if mods.contains(Mods::SHIFT) { c.to_uppercase().to_string() } else { c.to_string() };
                if mods.contains(Mods::SHIFT) && c.is_alphabetic() {
                    k.consumed_mods = Mods::SHIFT;
                }
                k.text = Some(t);
            }
        }
        Some(k)
    }
}

fn key_from_name(name: &str) -> Option<(Key, Option<char>)> {
    use Key::*;
    let lower = name.to_ascii_lowercase();
    let named = match lower.as_str() {
        "enter" | "return" => Some(Enter),
        "tab" => Some(Tab),
        "backspace" | "bs" => Some(Backspace),
        "escape" | "esc" => Some(Escape),
        "space" => return Some((Space, Some(' '))),
        "up" | "arrowup" => Some(ArrowUp),
        "down" | "arrowdown" => Some(ArrowDown),
        "left" | "arrowleft" => Some(ArrowLeft),
        "right" | "arrowright" => Some(ArrowRight),
        "home" => Some(Home),
        "end" => Some(End),
        "pageup" | "pgup" => Some(PageUp),
        "pagedown" | "pgdn" => Some(PageDown),
        "delete" | "del" => Some(Delete),
        "insert" | "ins" => Some(Insert),
        _ => None,
    };
    if let Some(k) = named {
        return Some((k, None));
    }
    if let Some(n) = lower.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()) {
        let fk = [F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12];
        return fk.get((n as usize).checked_sub(1)?).map(|k| (*k, None));
    }
    let mut chars = name.chars();
    let c = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    let lc = c.to_ascii_lowercase();
    let letters = [A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z];
    let digits = [Digit0, Digit1, Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8, Digit9];
    let key = match lc {
        'a'..='z' => letters[(lc as u8 - b'a') as usize],
        '0'..='9' => digits[(lc as u8 - b'0') as usize],
        '-' => Minus,
        '=' => Equal,
        '[' => BracketLeft,
        ']' => BracketRight,
        '\\' => Backslash,
        ';' => Semicolon,
        '\'' => Quote,
        ',' => Comma,
        '.' => Period,
        '/' => Slash,
        '`' => Backquote,
        _ => Unidentified,
    };
    Some((key, Some(lc)))
}

/// A mouse event at a cell position (frontends with pixel positions use
/// [`MouseInput::at_pixel`]).
#[derive(Clone, Copy, Debug)]
pub struct MouseInput {
    pub action: MouseAction,
    pub button: Option<MouseButton>,
    pub mods: Mods,
    /// Position in pixels relative to the grid origin.
    pub x: f32,
    pub y: f32,
    /// Whether any button is held (drag vs plain motion).
    pub any_pressed: bool,
}

impl MouseInput {
    pub fn at_pixel(action: MouseAction, button: Option<MouseButton>, x: f32, y: f32) -> Self {
        Self { action, button, mods: Mods::empty(), x, y, any_pressed: false }
    }
    /// Centre of cell (col, row) for the given grid metrics.
    pub fn at_cell(action: MouseAction, button: Option<MouseButton>, col: u16, row: u16, size: GridSize) -> Self {
        let x = (col as f32 + 0.5) * size.cell_width as f32;
        let y = (row as f32 + 0.5) * size.cell_height as f32;
        Self::at_pixel(action, button, x, y)
    }
}

/// How to move the viewport through scrollback.
#[derive(Clone, Copy, Debug)]
pub enum ScrollTo {
    Top,
    Bottom,
    Delta(isize),
}

/// Text dump formats (libghostty-vt formatter).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DumpFormat {
    Plain,
    Vt,
    Html,
}
