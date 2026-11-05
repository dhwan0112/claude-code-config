//! Everything ccterm draws itself, as cells (DESIGN.md §1.3, §2): the tab
//! bar, the command palette and short hints, composed with the active tab's
//! screen into one window grid that the painter draws like any terminal.
//! Neutral greys only; no accent colour (§2.2).

use ccterm_core::{Cell, CellFlags, Cursor, Rgb, Snapshot, Width};

use crate::keymap::{Action, Keymap};

/// `a` mixed toward `b` by `t`/255.
fn mix(a: Rgb, b: Rgb, t: u8) -> Rgb {
    let m = |x: u8, y: u8| ((x as u16 * (255 - t as u16) + y as u16 * t as u16) / 255) as u8;
    Rgb { r: m(a.r, b.r), g: m(a.g, b.g), b: m(a.b, b.b) }
}

/// Neutral greys derived from the terminal's default colours.
#[derive(Clone, Copy, Debug)]
pub struct Greys {
    pub bg: Rgb,
    pub bar: Rgb,
    pub dim: Rgb,
    pub normal: Rgb,
    pub strong: Rgb,
}

impl Greys {
    pub fn from(fg: Rgb, bg: Rgb) -> Greys {
        Greys { bg, bar: mix(bg, fg, 18), dim: mix(fg, bg, 120), normal: mix(fg, bg, 40), strong: fg }
    }
}

/// Name of a tab from its title. Claude Code prefixes its title with a
/// state glyph and a space ("✳ Claude Code", a Braille spinner while it
/// works): one leading symbol followed by a space is dropped. Empty → the
/// program's file name.
pub fn tab_name(title: &str, program: &str) -> String {
    let t = title.trim();
    let mut chars = t.chars();
    let stripped = match (chars.next(), chars.next()) {
        (Some(c), Some(' ')) if is_state_glyph(c) => chars.as_str().trim(),
        (Some(c), None) if is_state_glyph(c) => "",
        _ => t,
    };
    if !stripped.is_empty() {
        return stripped.to_string();
    }
    let p = program.rsplit(['/', '\\']).next().unwrap_or(program);
    p.strip_suffix(".exe").unwrap_or(p).to_string()
}

fn is_state_glyph(c: char) -> bool {
    matches!(c as u32, 0x2800..=0x28FF) // Braille spinner frames
        || matches!(c, '✳' | '✻' | '✽' | '✶' | '✢' | '✷' | '✸' | '✹' | '✺' | '*' | '·' | '•' | '●' | '○' | '◐' | '◓' | '◑' | '◒')
}

/// Display width of a string in cells.
pub fn width(s: &str) -> usize {
    s.chars().map(crate::text_width).sum()
}

/// `s` cut to `w` cells, with "…" when it was longer.
pub fn fit(s: &str, w: usize) -> String {
    if width(s) <= w {
        return s.to_string();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in s.chars() {
        let cw = crate::text_width(c);
        if used + cw + 1 > w {
            break;
        }
        out.push(c);
        used += cw;
    }
    out.push('…');
    out
}

/// Writes `text` into `row` from column `x` (wide characters take two
/// cells), clipped at the row end. Returns the column after the text.
pub fn put(row: &mut [Cell], x: usize, text: &str, fg: Rgb, bg: Rgb, bold: bool) -> usize {
    let mut x = x;
    for c in text.chars() {
        let w = crate::text_width(c).max(1);
        if x + w > row.len() {
            break;
        }
        let flags = CellFlags(if bold { CellFlags::BOLD } else { 0 });
        row[x] = Cell {
            text: c.to_string(),
            fg: Some(fg),
            bg: Some(bg),
            flags,
            width: if w == 2 { Width::Wide } else { Width::Narrow },
        };
        if w == 2 {
            row[x + 1] = Cell { text: String::new(), fg: Some(fg), bg: Some(bg), flags, width: Width::Spacer };
        }
        x += w;
    }
    x
}

fn blank(fg: Rgb, bg: Rgb) -> Cell {
    Cell { text: String::new(), fg: Some(fg), bg: Some(bg), flags: CellFlags(0), width: Width::Narrow }
}

/// What the tab bar needs to know about a tab.
pub struct TabView<'a> {
    pub name: &'a str,
    pub exited: bool,
}

/// The tab bar row: ` 1 name ` per tab, equal shares, the active tab on the
/// terminal's background, the others on the bar grey and dimmed.
pub fn tab_bar(tabs: &[TabView], active: usize, cols: usize, g: Greys) -> Vec<Cell> {
    let mut row = vec![blank(g.dim, g.bar); cols];
    if tabs.is_empty() || cols == 0 {
        return row;
    }
    let share = (cols / tabs.len()).clamp(6, 32);
    let mut x = 0;
    for (i, t) in tabs.iter().enumerate() {
        if x >= cols {
            break;
        }
        let end = (x + share).min(cols);
        let is_active = i == active;
        let bg = if is_active { g.bg } else { g.bar };
        let fg = match (is_active, t.exited) {
            (_, true) => g.dim,
            (true, false) => g.strong,
            (false, false) => g.normal,
        };
        for c in &mut row[x..end] {
            *c = blank(fg, bg);
        }
        let label = format!("{} {}", i + 1, t.name);
        let label = fit(&label, end - x - 2);
        put(&mut row[..end], x + 1, &label, fg, bg, is_active);
        x = end;
    }
    row
}

/// The command palette: a query and the actions matching it.
#[derive(Default, Debug)]
pub struct Palette {
    pub query: String,
    pub selected: usize,
}

impl Palette {
    /// Actions whose label contains every word of the query (any case).
    pub fn matches(&self) -> Vec<Action> {
        let q = self.query.to_lowercase();
        Action::all()
            .into_iter()
            .filter(|a| {
                let l = a.label().to_lowercase();
                q.split_whitespace().all(|w| l.contains(w))
            })
            .collect()
    }

    pub fn current(&self) -> Option<Action> {
        self.matches().get(self.selected).copied()
    }

    pub fn move_by(&mut self, d: isize) {
        let n = self.matches().len();
        if n == 0 {
            self.selected = 0;
            return;
        }
        self.selected = (self.selected as isize + d).rem_euclid(n as isize) as usize;
    }

    /// Draws the palette over `grid` rows `top..` (one overlay, §1.3).
    pub fn draw(&self, grid: &mut [Vec<Cell>], top: usize, keys: &Keymap, g: Greys) -> Option<(u16, u16)> {
        let cols = grid.first().map_or(0, |r| r.len());
        let rows = grid.len().saturating_sub(top);
        if cols < 12 || rows < 3 {
            return None;
        }
        let w = (cols - 4).min(64);
        let x0 = (cols - w) / 2;
        let items = self.matches();
        let h = (items.len() + 1).min(rows.saturating_sub(1)).max(2);
        let panel = g.bar;
        // Query line.
        let r = &mut grid[top];
        for c in &mut r[x0..x0 + w] {
            *c = blank(g.strong, panel);
        }
        let end = put(r, x0 + 1, "> ", g.dim, panel, false);
        let cursor_x = put(r, end, &fit(&self.query, w - 4), g.strong, panel, false);
        for (i, a) in items.iter().take(h - 1).enumerate() {
            let sel = i == self.selected;
            let bg = if sel { mix(panel, g.strong, 40) } else { panel };
            let row = &mut grid[top + 1 + i];
            for c in &mut row[x0..x0 + w] {
                *c = blank(g.normal, bg);
            }
            let key = keys.chord_for(*a).map(|c| c.to_string()).unwrap_or_default();
            let kw = width(&key);
            put(
                row,
                x0 + 2,
                &fit(&a.label(), w.saturating_sub(kw + 5)),
                if sel { g.strong } else { g.normal },
                bg,
                sel,
            );
            put(row, x0 + w - 1 - kw, &key, g.dim, bg, false);
        }
        Some((cursor_x as u16, top as u16))
    }
}

/// One short hint over the bottom row, right-aligned, dim (§2.2: no pop-ups).
pub fn hint(grid: &mut [Vec<Cell>], text: &str, g: Greys) {
    let Some(row) = grid.last_mut() else { return };
    let cols = row.len();
    let t = fit(text, cols.saturating_sub(2));
    let w = width(&t) + 2;
    let x0 = cols.saturating_sub(w);
    for c in &mut row[x0..] {
        *c = blank(g.normal, g.bar);
    }
    put(row, x0 + 1, &t, g.normal, g.bar, false);
}

/// The window grid: `top` chrome rows (tab bar) above the tab's screen,
/// then overlays. The cursor moves with the screen; while the palette is
/// open it sits in the query line.
pub struct Compose<'a> {
    pub screen: &'a Snapshot,
    pub cols: u16,
    pub rows: u16,
    pub tab_bar: Option<Vec<Cell>>,
    pub palette: Option<(&'a Palette, &'a Keymap)>,
    pub hint: Option<&'a str>,
}

impl Compose<'_> {
    pub fn build(&self) -> Snapshot {
        let s = self.screen;
        let g = Greys::from(s.default_fg, s.default_bg);
        let (cols, rows) = (self.cols as usize, self.rows as usize);
        let top = usize::from(self.tab_bar.is_some());
        let mut grid: Vec<Vec<Cell>> = vec![vec![Cell::default(); cols]; rows];
        if let Some(bar) = &self.tab_bar {
            for (i, c) in bar.iter().take(cols).enumerate() {
                grid[0][i] = c.clone();
            }
        }
        for y in 0..rows.saturating_sub(top).min(s.rows as usize) {
            let src = &s.cells[y * s.cols as usize..(y + 1) * s.cols as usize];
            for (dst, c) in grid[top + y].iter_mut().zip(src) {
                *dst = c.clone();
            }
        }
        let mut cursor = s.cursor.map(|c| Cursor { y: c.y + top as u16, ..c });
        if let Some((p, keys)) = self.palette {
            if let Some((x, y)) = p.draw(&mut grid, top, keys, g) {
                cursor = Some(Cursor { x, y, shape: ccterm_core::CursorShape::Bar, blinking: false });
            }
        }
        if let Some(h) = self.hint {
            hint(&mut grid, h, g);
        }
        Snapshot {
            cols: self.cols,
            rows: self.rows,
            cells: grid.into_iter().flatten().collect(),
            cursor,
            default_fg: s.default_fg,
            default_bg: s.default_bg,
            cursor_color: s.cursor_color,
            title: s.title.clone(),
            pwd: s.pwd.clone(),
            modes: s.modes,
            scrollbar: s.scrollbar,
            seq: s.seq,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FG: Rgb = Rgb { r: 220, g: 220, b: 220 };
    const BG: Rgb = Rgb { r: 30, g: 30, b: 30 };

    #[test]
    fn tab_names_drop_claude_state_glyphs() {
        assert_eq!(tab_name("✳ Claude Code", "claude"), "Claude Code");
        assert_eq!(tab_name("⠐ Fix the parser", "claude"), "Fix the parser");
        assert_eq!(tab_name("✻ ", "claude"), "claude");
        assert_eq!(tab_name("", "/usr/bin/bash"), "bash");
        assert_eq!(tab_name("", r"C:\Windows\System32\cmd.exe"), "cmd");
        assert_eq!(tab_name("vim main.rs", "vim"), "vim main.rs");
        assert_eq!(tab_name("* not a glyph?", "x"), "not a glyph?");
        assert_eq!(tab_name("한글 제목", "x"), "한글 제목");
    }

    #[test]
    fn fit_and_put_respect_wide_characters() {
        assert_eq!(fit("abcdef", 4), "abc…");
        assert_eq!(fit("한글한글", 5), "한글…");
        assert_eq!(fit("ab", 4), "ab");
        let mut row = vec![Cell::default(); 5];
        let end = put(&mut row, 0, "a한글", FG, BG, false);
        assert_eq!(end, 5);
        assert_eq!((row[1].width, row[2].width), (Width::Wide, Width::Spacer));
    }

    #[test]
    fn tab_bar_marks_active_tab_without_accent_colours() {
        let g = Greys::from(FG, BG);
        let tabs = [TabView { name: "one", exited: false }, TabView { name: "two", exited: true }];
        let row = tab_bar(&tabs, 0, 40, g);
        let text: String = row.iter().map(|c| if c.text.is_empty() { " ".into() } else { c.text.clone() }).collect();
        assert!(text.starts_with(" 1 one") && text.contains("2 two"), "{text}");
        assert_eq!(row[1].bg, Some(BG));
        assert!(row[1].flags.has(CellFlags::BOLD));
        // Every colour is a grey between bg and fg (r == g == b here).
        for c in &row {
            for col in [c.fg.unwrap(), c.bg.unwrap()] {
                assert!(col.r == col.g && col.g == col.b, "{col:?} is not grey");
            }
        }
    }

    #[test]
    fn palette_filters_and_wraps() {
        let mut p = Palette::default();
        assert_eq!(p.matches().len(), Action::all().len());
        p.query = "tab go 3".into();
        assert_eq!(p.matches(), vec![Action::GotoTab(3)]);
        p.query = "TAB".into();
        let n = p.matches().len();
        p.move_by(-1);
        assert_eq!(p.selected, n - 1);
        p.query = "zzz".into();
        assert_eq!(p.current(), None);
    }

    #[test]
    fn compose_puts_screen_below_the_tab_bar_and_moves_the_cursor() {
        let mut s = Snapshot { cols: 4, rows: 2, default_fg: FG, default_bg: BG, ..Default::default() };
        s.cells = vec![Cell { text: "x".into(), ..Default::default() }; 8];
        s.cursor = Some(Cursor { x: 1, y: 1, shape: ccterm_core::CursorShape::Block, blinking: false });
        let g = Greys::from(FG, BG);
        let bar = tab_bar(&[TabView { name: "a", exited: false }, TabView { name: "b", exited: false }], 1, 4, g);
        let out = Compose { screen: &s, cols: 4, rows: 3, tab_bar: Some(bar), palette: None, hint: None }.build();
        assert_eq!(out.rows, 3);
        assert_eq!(out.cells[4].text, "x");
        assert_eq!(out.cursor.unwrap().y, 2);
        let plain = Compose { screen: &s, cols: 4, rows: 2, tab_bar: None, palette: None, hint: Some("hi") }.build();
        assert_eq!(plain.cursor.unwrap().y, 1);
        assert_eq!(plain.cells[5].text, "h");
    }
}
