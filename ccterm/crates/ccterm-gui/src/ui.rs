//! Everything ccterm draws itself, as cells (DESIGN.md §1.3, §2): the tab
//! bar, the command palette and short hints, composed with the active tab's
//! screen into one window grid that the painter draws like any terminal.
//! Neutral greys only (§2.2); the two exceptions are Claude Code's
//! attention marks (amber: waiting for you, red `?`: asks you, after ccx)
//! and usage near a limit in the status row.

use ccterm_core::{Cell, CellFlags, Cursor, Rgb, Snapshot, Width};
use ccterm_proto::wire::{Attention, ClaudeInfo};

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

pub const AMBER: Rgb = Rgb { r: 222, g: 165, b: 50 };
pub const RED: Rgb = Rgb { r: 226, g: 80, b: 70 };

/// What the tab bar needs to know about a tab.
pub struct TabView<'a> {
    pub name: &'a str,
    pub exited: bool,
    pub attention: Attention,
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
        // The mark, on other tabs only (the active one shows Claude Code itself).
        let mark = match (is_active, t.attention) {
            (false, Attention::Wait) => Some(("● ", AMBER)),
            (false, Attention::Ask) => Some(("? ", RED)),
            _ => None,
        };
        let num = format!("{} ", i + 1);
        let mut cx = put(&mut row[..end], x + 1, &num, fg, bg, is_active);
        if let Some((m, color)) = mark {
            cx = put(&mut row[..end], cx, m, color, bg, true);
        }
        let room = end.saturating_sub(cx + 1);
        put(&mut row[..end], cx, &fit(t.name, room), fg, bg, is_active);
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

/// A percentage with a fixed width ("  7%", " 42%", "100%"): the row does
/// not shift while numbers change.
fn pct(v: f32) -> String {
    format!("{:>3.0}%", v.clamp(0.0, 999.0))
}

fn level_color(v: f32, g: Greys) -> Rgb {
    if v >= 95.0 {
        RED
    } else if v >= 80.0 {
        AMBER
    } else {
        g.normal
    }
}

/// Claude Code's status row (bottom, Claude Code tabs only): context, rate
/// limits, cost, files changed with the diff key; "esc to interrupt" while
/// it works (Claude Code hides that hint when a status line is set).
/// Greys; amber/red only near a limit (≥ 80 % / ≥ 95 %).
pub fn status_row(info: Option<&ClaudeInfo>, cols: usize, g: Greys, diff_key: &str) -> Vec<Cell> {
    let bg = g.bar;
    let mut row = vec![blank(g.dim, bg); cols];
    let Some(i) = info else {
        put(&mut row, 1, "Claude Code", g.dim, bg, false);
        return row;
    };
    let mut x = 1;
    let sep = |row: &mut Vec<Cell>, x: usize| put(row, x, "  ", g.dim, bg, false);
    let item = |row: &mut Vec<Cell>, x: &mut usize, label: &str, value: String, color: Rgb| {
        if *x > 1 {
            *x = sep(row, *x);
        }
        *x = put(row, *x, label, g.dim, bg, false);
        *x = put(row, *x, &value, color, bg, false);
    };
    match i.context_pct {
        Some(c) => item(&mut row, &mut x, "ctx ", pct(c), level_color(c, g)),
        None => item(&mut row, &mut x, "ctx ", "   –".into(), g.dim),
    }
    for l in &i.limits {
        let label = match l.name.as_str() {
            "five_hour" => "5h ".to_string(),
            "seven_day" => "7d ".to_string(),
            "spend_limit" => "spend ".to_string(),
            other => format!("{} ", other.replace('_', " ")),
        };
        item(&mut row, &mut x, &label, pct(l.used_pct), level_color(l.used_pct, g));
    }
    if let Some(c) = i.cost_usd {
        item(&mut row, &mut x, "", format!("${c:>6.2}"), g.normal);
    }
    if i.files_changed > 0 {
        let n = i.files_changed;
        let text = format!("{n} file{} changed", if n == 1 { "" } else { "s" });
        item(&mut row, &mut x, "", text, g.normal);
        if !diff_key.is_empty() {
            x = put(&mut row, x, &format!(" · {diff_key}"), g.dim, bg, false);
        }
    }
    let _ = x;
    if i.busy {
        let t = "esc to interrupt";
        let at = cols.saturating_sub(width(t) + 1);
        put(&mut row, at, t, g.dim, bg, false);
    }
    row
}

/// "1.2M", "200k", "950".
fn human(n: u64) -> String {
    match n {
        n if n >= 1_000_000 => format!("{:.1}M", n as f64 / 1e6),
        n if n >= 10_000 => format!("{}k", n / 1000),
        n if n >= 1000 => format!("{:.1}k", n as f64 / 1e3),
        n => n.to_string(),
    }
}

fn duration(ms: u64) -> String {
    let s = ms / 1000;
    match s {
        s if s >= 3600 => format!("{}h{:02}m", s / 3600, s % 3600 / 60),
        s if s >= 60 => format!("{}m{:02}s", s / 60, s % 60),
        s => format!("{s}s"),
    }
}

/// Local time of a Unix timestamp as `MM-DD HH:MM` (UTC offset from the
/// platform is not available without a dependency: shown in UTC, marked).
fn when(unix_s: i64) -> String {
    let days = unix_s.div_euclid(86_400);
    let secs = unix_s.rem_euclid(86_400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{m:02}-{d:02} {:02}:{:02} UTC", secs / 3600, secs % 3600 / 60)
}

/// The status panel (right column, hidden by default): what Claude Code's
/// own screen does not show — limits with their reset times, subagents,
/// the last edit, cache, lines, session id. `w` columns, `h` rows; the
/// first column is the separator.
pub fn panel(info: Option<&ClaudeInfo>, w: usize, h: usize, g: Greys, now_ms: u64) -> Vec<Vec<Cell>> {
    let bg = g.bar;
    let mut rows = vec![vec![blank(g.normal, bg); w]; h];
    for r in rows.iter_mut() {
        if let Some(c) = r.first_mut() {
            *c = Cell { text: "│".into(), ..blank(g.dim, bg) };
        }
    }
    let mut lines: Vec<(String, String, Rgb)> = vec![];
    let mut add = |k: &str, v: String, c: Rgb| lines.push((k.to_string(), v, c));
    let Some(i) = info else {
        add("", "No Claude Code status yet.".into(), g.dim);
        draw_lines(&mut rows, &lines, g);
        return rows;
    };
    if !i.name.is_empty() {
        add("session", i.name.clone(), g.strong);
    }
    if !i.claude_session.is_empty() {
        add("id", i.claude_session.chars().take(8).collect(), g.dim);
    }
    if let Some(c) = i.context_pct {
        let size = i.context_size.map(|s| format!(" of {}", human(s))).unwrap_or_default();
        add("context", format!("{}{size}", pct(c).trim_start()), level_color(c, g));
    }
    for l in &i.limits {
        let name = match l.name.as_str() {
            "five_hour" => "5h limit".to_string(),
            "seven_day" => "7d limit".to_string(),
            other => other.replace('_', " "),
        };
        let reset = l.resets_at.map(|t| format!(" · {}", when(t))).unwrap_or_default();
        add(&name, format!("{}{reset}", pct(l.used_pct).trim_start()), level_color(l.used_pct, g));
    }
    if let Some(c) = i.cost_usd {
        let est = if i.limits.is_empty() { "" } else { " (API estimate)" };
        add("cost", format!("${c:.2}{est}"), g.normal);
    }
    if i.lines_added + i.lines_removed > 0 {
        add("lines", format!("+{} -{}", i.lines_added, i.lines_removed), g.normal);
    }
    if let (Some(w), hit) = (i.cache_warm, i.cache_hit_pct) {
        let hit = hit.map(|h| format!(" · hit {h:.0}%")).unwrap_or_default();
        add("cache", format!("{}{hit}", if w { "warm" } else { "cold" }), g.normal);
    }
    if i.files_changed > 0 {
        add("files", format!("{} changed, {} edits", i.files_changed, i.edits), g.normal);
    }
    if let Some(e) = &i.last_edit {
        let name = e.path.rsplit(['/', '\\']).next().unwrap_or(&e.path);
        add("last edit", format!("{name} +{} -{} · {}", e.added, e.removed, e.agent), g.normal);
    }
    let running: Vec<_> = i.workers.iter().filter(|w| w.ended_ms.is_none()).collect();
    if !i.workers.is_empty() {
        add("subagents", format!("{} running, {} done", running.len(), i.workers.len() - running.len()), g.normal);
    }
    for wk in &i.workers {
        let (mark, t) = match wk.ended_ms {
            None => ("●", duration(now_ms.saturating_sub(wk.started_ms))),
            Some(e) => ("✓", format!("took {}", duration(e.saturating_sub(wk.started_ms)))),
        };
        add("", format!("{mark} {} {t}", if wk.kind.is_empty() { "agent" } else { &wk.kind }), g.normal);
    }
    if i.updated_ms > 0 {
        add("updated", format!("{} ago", duration(now_ms.saturating_sub(i.updated_ms))), g.dim);
    }
    draw_lines(&mut rows, &lines, g);
    rows
}

fn draw_lines(rows: &mut [Vec<Cell>], lines: &[(String, String, Rgb)], g: Greys) {
    let w = rows.first().map_or(0, |r| r.len());
    if w < 6 {
        return;
    }
    for (r, (k, v, c)) in rows.iter_mut().skip(1).zip(lines) {
        if k.is_empty() {
            put(r, 3, &fit(v, w - 4), *c, g.bar, false);
        } else {
            let x = put(r, 2, &fit(k, 10), g.dim, g.bar, false);
            let x = x.max(12);
            put(r, x, &fit(v, w.saturating_sub(x + 1)), *c, g.bar, false);
        }
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
    /// Claude Code's status row, at the bottom.
    pub status: Option<Vec<Cell>>,
    /// The status panel: columns at the right of the screen area.
    pub panel: Option<Vec<Vec<Cell>>>,
    /// The diff view over the screen area (hides the cursor).
    pub diff: Option<&'a crate::diffview::DiffView>,
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
        let bottom = usize::from(self.status.is_some());
        if let (Some(st), Some(last)) = (&self.status, grid.last_mut()) {
            for (dst, c) in last.iter_mut().zip(st) {
                *dst = c.clone();
            }
        }
        for y in 0..rows.saturating_sub(top + bottom).min(s.rows as usize) {
            let src = &s.cells[y * s.cols as usize..(y + 1) * s.cols as usize];
            for (dst, c) in grid[top + y].iter_mut().zip(src) {
                *dst = c.clone();
            }
        }
        let area = rows.saturating_sub(top + bottom);
        if let Some(p) = &self.panel {
            let pw = p.first().map_or(0, |r| r.len()).min(cols);
            for (y, prow) in p.iter().take(area).enumerate() {
                for (x, c) in prow.iter().take(pw).enumerate() {
                    grid[top + y][cols - pw + x] = c.clone();
                }
            }
        }
        let mut cursor = s.cursor.map(|c| Cursor { y: c.y + top as u16, ..c });
        if let Some(d) = self.diff {
            let pw = self.panel.as_ref().and_then(|p| p.first()).map_or(0, |r| r.len()).min(cols);
            let mut area_grid: Vec<Vec<Cell>> = grid[top..top + area].iter().map(|r| r[..cols - pw].to_vec()).collect();
            d.draw(&mut area_grid, 0, area, g);
            for (y, r) in area_grid.into_iter().enumerate() {
                grid[top + y][..cols - pw].clone_from_slice(&r);
            }
            cursor = None;
        }
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
        let tabs = [
            TabView { name: "one", exited: false, attention: Attention::None },
            TabView { name: "two", exited: true, attention: Attention::None },
        ];
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
        let tv = |name| TabView { name, exited: false, attention: Attention::None };
        let bar = tab_bar(&[tv("a"), tv("b")], 1, 4, g);
        let out = Compose {
            screen: &s,
            cols: 4,
            rows: 3,
            tab_bar: Some(bar),
            status: None,
            panel: None,
            diff: None,
            palette: None,
            hint: None,
        }
        .build();
        assert_eq!(out.rows, 3);
        assert_eq!(out.cells[4].text, "x");
        assert_eq!(out.cursor.unwrap().y, 2);
        let plain = Compose {
            screen: &s,
            cols: 4,
            rows: 2,
            tab_bar: None,
            status: None,
            panel: None,
            diff: None,
            palette: None,
            hint: Some("hi"),
        }
        .build();
        assert_eq!(plain.cursor.unwrap().y, 1);
        assert_eq!(plain.cells[5].text, "h");
        // A status row takes the bottom; the screen keeps its rows above it.
        let st = vec![Cell { text: "S".into(), ..Default::default() }; 4];
        let with = Compose {
            screen: &s,
            cols: 4,
            rows: 3,
            tab_bar: None,
            status: Some(st),
            panel: None,
            diff: None,
            palette: None,
            hint: None,
        }
        .build();
        assert_eq!((with.cells[0].text.as_str(), with.cells[8].text.as_str()), ("x", "S"));
    }

    fn text(row: &[Cell]) -> String {
        row.iter().map(|c| if c.text.is_empty() { " ".into() } else { c.text.clone() }).collect()
    }

    #[test]
    fn attention_marks_only_on_other_tabs() {
        let g = Greys::from(FG, BG);
        let tabs = [
            TabView { name: "a", exited: false, attention: Attention::Wait },
            TabView { name: "b", exited: false, attention: Attention::Wait },
            TabView { name: "c", exited: false, attention: Attention::Ask },
        ];
        let row = tab_bar(&tabs, 0, 60, g);
        let t = text(&row);
        assert!(t.contains("1 a") && t.contains("2 ● b") && t.contains("3 ? c"), "{t}");
        assert!(row.iter().any(|c| c.text == "●" && c.fg == Some(AMBER)));
        assert!(row.iter().any(|c| c.text == "?" && c.fg == Some(RED)));
    }

    #[test]
    fn status_row_has_fixed_width_numbers_and_greys_when_low() {
        let g = Greys::from(FG, BG);
        let mut i = ClaudeInfo { context_pct: Some(7.0), cost_usd: Some(0.5), files_changed: 3, ..Default::default() };
        i.limits.push(ccterm_proto::wire::Limit { name: "five_hour".into(), used_pct: 12.0, resets_at: None });
        let a = text(&status_row(Some(&i), 80, g, "Ctrl+Shift+D"));
        assert!(a.starts_with(" ctx   7%  5h  12%  $  0.50  3 files changed · Ctrl+Shift+D"), "{a}");
        i.context_pct = Some(42.0);
        let b = text(&status_row(Some(&i), 80, g, "Ctrl+Shift+D"));
        assert_eq!(a.len(), b.len());
        assert_eq!(a.find("5h"), b.find("5h"), "numbers keep their width");
        let row = status_row(Some(&i), 80, g, "");
        assert!(row.iter().all(|c| c.fg.is_some_and(|f| f.r == f.g && f.g == f.b)), "greys only when low");
        i.context_pct = Some(85.0);
        i.busy = true;
        let row = status_row(Some(&i), 80, g, "");
        assert!(row.iter().any(|c| c.fg == Some(AMBER)));
        assert!(text(&row).trim_end().ends_with("esc to interrupt"));
        assert!(text(&status_row(None, 40, g, "")).contains("Claude Code"));
    }

    #[test]
    fn panel_shows_what_claude_code_does_not() {
        let g = Greys::from(FG, BG);
        let mut i = ClaudeInfo {
            claude_session: "0f0fc5c9-9b5a".into(),
            context_pct: Some(42.0),
            context_size: Some(200_000),
            ..Default::default()
        };
        i.limits.push(ccterm_proto::wire::Limit {
            name: "five_hour".into(),
            used_pct: 12.0,
            resets_at: Some(1_793_900_000),
        });
        i.workers.push(ccterm_proto::wire::Worker {
            id: "a".into(),
            kind: "Explore".into(),
            started_ms: 1000,
            ended_ms: None,
        });
        i.updated_ms = 5000;
        let p = panel(Some(&i), 40, 12, g, 66_000);
        let all: Vec<String> = p.iter().map(|r| text(r)).collect();
        let joined = all.join("\n");
        assert!(joined.contains("context   42% of 200k"), "{joined}");
        assert!(joined.contains("5h limit  12% · 11-05 17:33 UTC"), "{joined}");
        assert!(joined.contains("● Explore 1m05s"), "{joined}");
        assert!(joined.contains("updated   1m01s ago"), "{joined}");
        assert!(all.iter().all(|r| r.starts_with('│')));
        assert_eq!(when(0), "01-01 00:00 UTC");
        assert_eq!(human(1_234_567), "1.2M");
    }
}
