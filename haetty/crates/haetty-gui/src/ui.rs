//! Everything haetty draws itself, as cells (DESIGN.md §1.3, §2): the tab
//! bar, the command palette and short hints, composed with the active tab's
//! screen into one window grid that the painter draws like any terminal.
//! Neutral greys only (§2.2), mixed from the theme's background and
//! foreground; the exceptions are Claude Code's attention marks (theme
//! colour 3: waiting for you, colour 9 `?`: asks you, after ccx), usage near
//! a limit in the status row (the same two) and the diff view's tints
//! (colours 2 and 1).

use haetty_core::{Cell, CellFlags, Cursor, Rgb, Snapshot, Width};
use haetty_proto::wire::{Attention, ClaudeInfo};

use crate::keymap::{Action, Keymap};

/// `a` mixed toward `b` by `t`/255.
fn mix(a: Rgb, b: Rgb, t: u8) -> Rgb {
    let m = |x: u8, y: u8| ((x as u16 * (255 - t as u16) + y as u16 * t as u16) / 255) as u8;
    Rgb { r: m(a.r, b.r), g: m(a.g, b.g), b: m(a.b, b.b) }
}

/// haetty's own colours: greys mixed from the terminal's default colours,
/// plus the few state colours taken from the theme's palette.
#[derive(Clone, Copy, Debug)]
pub struct Greys {
    pub bg: Rgb,
    pub bar: Rgb,
    pub dim: Rgb,
    pub normal: Rgb,
    pub strong: Rgb,
    /// Colour 3: "waiting for you" (amber `●`), usage ≥ 80 %.
    pub wait: Rgb,
    /// Colour 9: "asks you" (`?`), usage ≥ 95 %.
    pub ask: Rgb,
    /// Colours 2 and 1: the diff view's added / removed tints.
    pub added: Rgb,
    pub removed: Rgb,
}

impl Greys {
    /// `fg`/`bg`: the screen's default colours; `palette`: the theme's 0–15.
    pub fn new(fg: Rgb, bg: Rgb, palette: &[Rgb; 16]) -> Greys {
        Greys {
            bg,
            bar: mix(bg, fg, 18),
            dim: mix(fg, bg, 120),
            normal: mix(fg, bg, 40),
            strong: fg,
            wait: palette[3],
            ask: palette[9],
            added: palette[2],
            removed: palette[1],
        }
    }

    /// With "haetty classic"'s palette (tests).
    #[cfg(test)]
    pub fn from(fg: Rgb, bg: Rgb) -> Greys {
        Greys::new(fg, bg, &haetty_core::theme::Theme::base().palette)
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
/// `s` in `w` cells, cut at the start with `…` when longer.
pub fn fit_left(s: &str, w: usize) -> String {
    if width(s) <= w {
        return s.to_string();
    }
    let mut out: Vec<char> = vec![];
    let mut used = 0;
    for c in s.chars().rev() {
        let cw = crate::text_width(c);
        if used + cw + 1 > w {
            break;
        }
        out.push(c);
        used += cw;
    }
    out.push('…');
    out.iter().rev().collect()
}

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
            ..Default::default()
        };
        if w == 2 {
            row[x + 1] = Cell {
                text: String::new(),
                fg: Some(fg),
                bg: Some(bg),
                flags,
                width: Width::Spacer,
                ..Default::default()
            };
        }
        x += w;
    }
    x
}

pub fn blank(fg: Rgb, bg: Rgb) -> Cell {
    Cell {
        text: String::new(),
        fg: Some(fg),
        bg: Some(bg),
        flags: CellFlags(0),
        width: Width::Narrow,
        ..Default::default()
    }
}

/// What the tab bar needs to know about a tab.
#[derive(Clone)]
pub struct TabView<'a> {
    pub name: &'a str,
    pub exited: bool,
    pub attention: Attention,
    /// Observe-only readers may see this tab (DESIGN.md §6.1): a dim `◦`.
    pub observed: bool,
    /// A worker whose context has passed `clear-hint`: a dim `/clear` at the right end of its share (a hint only).
    pub clear: bool,
}

/// Cells per tab in a tab bar `cols` wide with `n` tabs (equal shares, 6–32).
pub fn tab_share(cols: usize, n: usize) -> usize {
    (cols / n.max(1)).clamp(6, 32)
}

/// The tab bar row: ` 1 name ` per tab, equal shares, all on the terminal's
/// background (no band, no separator: the gap to the screen is space). The
/// active tab's text is in the foreground colour and the renderer underlines
/// its whole share with a thin line of that colour
/// ([`CellFlags::TAB_ACTIVE`]); the other tabs' text is dim. No accent
/// colour except the attention marks.
pub fn tab_bar(tabs: &[TabView], active: usize, cols: usize, g: Greys) -> Vec<Cell> {
    let mut row = vec![blank(g.dim, g.bg); cols];
    if tabs.is_empty() || cols == 0 {
        return row;
    }
    let share = tab_share(cols, tabs.len());
    let mut x = 0;
    for (i, t) in tabs.iter().enumerate() {
        if x >= cols {
            break;
        }
        let end = (x + share).min(cols);
        let is_active = i == active;
        let bg = g.bg;
        // An exited tab stays dimmed even when active (DESIGN §3.10).
        let fg = if is_active && !t.exited { g.strong } else { g.dim };
        for c in &mut row[x..end] {
            *c = blank(fg, bg);
        }
        // The mark, on other tabs only (the active one shows Claude Code itself).
        let mark = match (is_active, t.attention) {
            (false, Attention::Wait) => Some(("● ", g.wait)),
            (false, Attention::Ask) => Some(("? ", g.ask)),
            _ => None,
        };
        let num = format!("{} ", i + 1);
        let mut cx = put(&mut row[..end], x + 1, &num, fg, bg, is_active);
        // Observable: one dim glyph, never an accent colour (§2.2).
        if t.observed {
            cx = put(&mut row[..end], cx, "◦ ", g.dim, bg, false);
        }
        if let Some((m, color)) = mark {
            cx = put(&mut row[..end], cx, m, color, bg, true);
        }
        // The dim `/clear` mark at the right end of the share, when the share has room for it and a name.
        let hint = if t.clear && end.saturating_sub(cx) >= 10 { " /clear" } else { "" };
        let room = end.saturating_sub(cx + 1 + hint.len());
        put(&mut row[..end], cx, &fit(t.name, room), fg, bg, is_active);
        if !hint.is_empty() {
            put(&mut row[..end], end - hint.len(), hint, g.dim, bg, false);
        }
        if is_active {
            for c in &mut row[x..end] {
                c.flags.0 |= CellFlags::TAB_ACTIVE;
            }
        }
        x = end;
    }
    row
}

/// The command palette: a query and the actions matching it.
#[derive(Default, Debug)]
pub struct Palette {
    pub query: String,
    pub selected: usize,
    pub mode: PaletteMode,
}

/// What the palette offers.
#[derive(Default, Debug)]
pub enum PaletteMode {
    /// Every action (the command palette).
    #[default]
    Actions,
    /// The query is the active tab's new name; `now` is its name now.
    Rename { now: String },
    /// Closed tabs: (name, details) each.
    Closed(Vec<(String, String)>),
    /// New task, seven questions: title, description, scope, related files, model, effort (the optional ones may be
    /// blank), then repository and base (`prefill` is the start of the last one; `answers` are the first six).
    NewTask { step: u8, answers: Vec<String>, prefill: String },
    /// A message for the active worker.
    Message,
    /// Tasks whose worktree can be removed: (task, details) each.
    Tasks(Vec<(String, String)>),
    /// Every tab's agents (stage PH, H3): (agent line, tab · waiting) each.
    Agents(Vec<(String, String)>),
}

/// What Enter picks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pick {
    Action(Action),
    /// Index into the closed list given to [`Palette::closed`].
    Closed(usize),
    Rename(String),
    /// The new task's answers, and "repository base".
    NewTask(crate::tasks::Draft, String),
    Message(String),
    /// Index into the list given to [`Palette::tasks`].
    Task(usize),
    /// Index into the list given to [`Palette::agents`].
    Agent(usize),
}

/// The answers to the new-task questions, in order (a missing answer is blank).
fn draft(a: &[String]) -> crate::tasks::Draft {
    let g = |i: usize| a.get(i).cloned().unwrap_or_default();
    crate::tasks::Draft { title: g(0), description: g(1), scope: g(2), related: g(3), model: g(4), effort: g(5) }
}

fn words_match(q: &str, text: &str) -> bool {
    let l = text.to_lowercase();
    q.to_lowercase().split_whitespace().all(|w| l.contains(w))
}

impl Palette {
    pub fn rename(now: String, custom: Option<String>) -> Palette {
        Palette { query: custom.unwrap_or_default(), selected: 0, mode: PaletteMode::Rename { now } }
    }

    pub fn closed(items: Vec<(String, String)>) -> Palette {
        Palette { query: String::new(), selected: 0, mode: PaletteMode::Closed(items) }
    }

    pub fn new_task(prefill: String) -> Palette {
        let mode = PaletteMode::NewTask { step: 0, answers: Vec::new(), prefill };
        Palette { query: String::new(), selected: 0, mode }
    }

    pub fn message() -> Palette {
        Palette { query: String::new(), selected: 0, mode: PaletteMode::Message }
    }

    pub fn agents(items: Vec<(String, String)>) -> Palette {
        Palette { query: String::new(), selected: 0, mode: PaletteMode::Agents(items) }
    }

    pub fn tasks(items: Vec<(String, String)>) -> Palette {
        Palette { query: String::new(), selected: 0, mode: PaletteMode::Tasks(items) }
    }

    /// New task: Enter takes the answer and asks the next question (true); the title is required, the optional answers
    /// may be blank, and the last question (repository and base) is the pick.
    pub fn task_step(&mut self) -> bool {
        let PaletteMode::NewTask { step, answers, prefill } = &mut self.mode else { return false };
        match *step {
            0 if self.query.trim().is_empty() => true,
            0..=5 => {
                answers.push(std::mem::take(&mut self.query));
                *step += 1;
                if *step == 6 {
                    self.query = prefill.clone();
                }
                true
            }
            _ => false,
        }
    }

    /// Actions whose label contains every word of the query (any case).
    pub fn matches(&self) -> Vec<Action> {
        if !matches!(self.mode, PaletteMode::Actions) {
            return vec![];
        }
        Action::all().into_iter().filter(|a| words_match(&self.query, &a.label())).collect()
    }

    /// Closed tabs matching the query (indices).
    fn closed_matches(&self) -> Vec<usize> {
        let (PaletteMode::Closed(items) | PaletteMode::Tasks(items) | PaletteMode::Agents(items)) = &self.mode else {
            return vec![];
        };
        (0..items.len()).filter(|&i| words_match(&self.query, &format!("{} {}", items[i].0, items[i].1))).collect()
    }

    pub fn current(&self) -> Option<Pick> {
        match &self.mode {
            PaletteMode::Actions => self.matches().get(self.selected).copied().map(Pick::Action),
            PaletteMode::Rename { .. } => Some(Pick::Rename(self.query.clone())),
            PaletteMode::Closed(_) => self.closed_matches().get(self.selected).copied().map(Pick::Closed),
            PaletteMode::Tasks(_) => self.closed_matches().get(self.selected).copied().map(Pick::Task),
            PaletteMode::Agents(_) => self.closed_matches().get(self.selected).copied().map(Pick::Agent),
            PaletteMode::NewTask { step: 6, answers, .. } => Some(Pick::NewTask(draft(answers), self.query.clone())),
            PaletteMode::NewTask { .. } => None,
            PaletteMode::Message => Some(Pick::Message(self.query.clone())),
        }
    }

    pub fn move_by(&mut self, d: isize) {
        let n = match self.mode {
            PaletteMode::Actions => self.matches().len(),
            PaletteMode::Rename { .. } | PaletteMode::NewTask { .. } | PaletteMode::Message => 0,
            PaletteMode::Closed(_) | PaletteMode::Tasks(_) | PaletteMode::Agents(_) => self.closed_matches().len(),
        };
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
        // (label, right-hand text) per row; `None` for "choose": an info row.
        let items: Vec<(String, String, bool)> = match &self.mode {
            PaletteMode::Actions => self
                .matches()
                .into_iter()
                .map(|a| (a.label(), keys.chord_for(a).map(|c| c.to_string()).unwrap_or_default(), true))
                .collect(),
            PaletteMode::Rename { now } => {
                vec![(format!("Enter renames · empty: automatic name · now: {now}"), String::new(), false)]
            }
            PaletteMode::Closed(list) | PaletteMode::Tasks(list) | PaletteMode::Agents(list) => {
                let m = self.closed_matches();
                if list.is_empty() {
                    let none = match self.mode {
                        PaletteMode::Closed(_) => "No closed tabs.",
                        PaletteMode::Agents(_) => "No agents in any tab.",
                        _ => "No tasks.",
                    };
                    vec![(none.into(), String::new(), false)]
                } else {
                    m.into_iter().map(|i| (list[i].0.clone(), list[i].1.clone(), true)).collect()
                }
            }
            PaletteMode::NewTask { step, .. } => vec![(
                match step {
                    0 => "1/7 Enter: next · Esc: cancel".to_string(),
                    1 => "2/7 one line (more later with \"Message the worker\") · Enter: next".to_string(),
                    2 => "3/7 optional: the worker may touch only this · Enter skips".to_string(),
                    3 => "4/7 optional: comma-separated files to start from · Enter skips".to_string(),
                    4 => "5/7 optional: e.g. opus · Enter skips (Claude Code's default)".to_string(),
                    5 => "6/7 optional: e.g. high · Enter skips (Claude Code's default)".to_string(),
                    _ => "7/7 folder and base branch · Enter: worktree on a new branch task/…, beside the repository"
                        .to_string(),
                },
                String::new(),
                false,
            )],
            PaletteMode::Message => {
                vec![("Enter sends it as a program, only when the worker is ready".to_string(), String::new(), false)]
            }
        };
        let prompt = match self.mode {
            PaletteMode::Actions => "> ",
            PaletteMode::Rename { .. } => "Rename tab: ",
            PaletteMode::Closed(_) => "Reopen: ",
            PaletteMode::Tasks(_) => "Remove worktree: ",
            PaletteMode::Agents(_) => "Go to agent: ",
            PaletteMode::NewTask { step: 0, .. } => "Task title: ",
            PaletteMode::NewTask { step: 1, .. } => "Description: ",
            PaletteMode::NewTask { step: 2, .. } => "Scope (optional, blank: the whole repository): ",
            PaletteMode::NewTask { step: 3, .. } => "Related files (optional, comma-separated): ",
            PaletteMode::NewTask { step: 4, .. } => "Model (optional, blank: Claude Code's default): ",
            PaletteMode::NewTask { step: 5, .. } => "Effort (optional, blank: Claude Code's default): ",
            PaletteMode::NewTask { .. } => "Repository and base: ",
            PaletteMode::Message => "To the worker: ",
        };
        let h = (items.len() + 1).min(rows.saturating_sub(1)).max(2);
        let panel = g.bar;
        // Query line.
        let r = &mut grid[top];
        for c in &mut r[x0..x0 + w] {
            *c = blank(g.strong, panel);
        }
        let end = put(r, x0 + 1, prompt, g.dim, panel, false);
        // A long query keeps its end (where the cursor is) in view.
        let cursor_x = put(r, end, &fit_left(&self.query, w.saturating_sub(end - x0 + 2)), g.strong, panel, false);
        for (i, (label, key, pickable)) in items.iter().take(h - 1).enumerate() {
            let sel = *pickable && i == self.selected;
            let bg = if sel { mix(panel, g.strong, 40) } else { panel };
            let row = &mut grid[top + 1 + i];
            for c in &mut row[x0..x0 + w] {
                *c = blank(g.normal, bg);
            }
            // The right-hand text takes at most half; a long one keeps its end
            // (a folder's last parts).
            let key = &fit_left(key, (w / 2).saturating_sub(2));
            let kw = width(key);
            put(
                row,
                x0 + 2,
                &fit(label, w.saturating_sub(kw + 5)),
                if sel {
                    g.strong
                } else if *pickable {
                    g.normal
                } else {
                    g.dim
                },
                bg,
                sel,
            );
            put(row, x0 + w - 1 - kw, key, g.dim, bg, false);
        }
        Some((cursor_x as u16, top as u16))
    }
}

/// A percentage with a fixed width ("  7%", " 42%", "100%"): the row does
/// not shift while numbers change.
pub(crate) fn pct(v: f32) -> String {
    format!("{:>3.0}%", v.clamp(0.0, 999.0))
}

pub(crate) fn level_color(v: f32, g: Greys) -> Rgb {
    if v >= 95.0 {
        g.ask
    } else if v >= 80.0 {
        g.wait
    } else {
        g.normal
    }
}

/// Whether the status row has anything to show: a value from Claude Code's
/// status line, changed files, or "esc to interrupt".
fn has_status(i: &ClaudeInfo) -> bool {
    i.context_pct.is_some() || !i.limits.is_empty() || i.cost_usd.is_some() || i.files_changed > 0 || i.busy
}

/// Claude Code's status row (bottom, Claude Code tabs only): context, rate
/// limits, cost, files changed with the diff key; "esc to interrupt" while
/// it works (Claude Code hides that hint when a status line is set).
/// Greys; amber/red only near a limit (≥ 80 % / ≥ 95 %). Before Claude Code
/// has sent anything (`None`) the row is blank on the terminal's background:
/// it is reserved (its arrival must not resize the program, DESIGN §3.10)
/// but shows nothing.
pub fn status_row_with(info: Option<&ClaudeInfo>, cols: usize, g: Greys, diff_key: &str, saving: bool) -> Vec<Cell> {
    let Some(i) = info.filter(|i| has_status(i)) else {
        return vec![blank(g.bg, g.bg); cols];
    };
    let bg = g.bar;
    let mut row = vec![blank(g.dim, bg); cols];
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
    // Saving mode (`saving-mode`): a dim mark after the items, so the person sees the limits are on.
    if saving {
        if x > 1 {
            x = sep(&mut row, x);
        }
        x = put(&mut row, x, "◆ saving", g.dim, bg, false);
    }
    let _ = x;
    if i.busy {
        let t = "esc to interrupt";
        let at = cols.saturating_sub(width(t) + 1);
        put(&mut row, at, t, g.dim, bg, false);
    }
    row
}

/// The settings button's text: ⚙ asking for text presentation (VS15), or `≡` where the fonts have no single-colour ⚙
/// (DESIGN.md §2.4; `Win::gear`).
pub const GEAR: &str = "\u{2699}\u{fe0e}";
pub const GEAR_FALLBACK: &str = "\u{2261}";

/// [`status_row_with`] keeping its last two columns free for the settings button (`gear`): the items are cut
/// first, the button never moves.
pub fn status_row_gear(
    info: Option<&ClaudeInfo>,
    cols: usize,
    g: Greys,
    diff_key: &str,
    saving: bool,
    gear: bool,
) -> Vec<Cell> {
    if !gear || cols < 4 {
        return status_row_with(info, cols, g, diff_key, saving);
    }
    let mut row = status_row_with(info, cols - 2, g, diff_key, saving);
    let bg = row.last().and_then(|c| c.bg).unwrap_or(g.bg);
    row.extend([blank(g.dim, bg), blank(g.dim, bg)]);
    row
}

/// The settings panel's rows over the screen area, and its text cursor.
pub type PanelGrid = (Vec<Vec<Cell>>, Option<(u16, u16)>);

/// The settings button at the window cell `(col, row)`: dim, normal under the pointer; over a program's cell it
/// keeps that cell's background.
#[derive(Clone, Copy, Debug)]
pub struct Gear<'a> {
    pub col: usize,
    pub row: usize,
    pub text: &'a str,
    pub hover: bool,
}

/// The status row without the saving mark (the tests and the reference frames; the window uses `status_row_with`).
#[cfg(test)]
pub fn status_row(info: Option<&ClaudeInfo>, cols: usize, g: Greys, diff_key: &str) -> Vec<Cell> {
    status_row_with(info, cols, g, diff_key, false)
}

/// "1.2M", "200k", "950".
pub(crate) fn human(n: u64) -> String {
    match n {
        n if n >= 1_000_000 => format!("{:.1}M", n as f64 / 1e6),
        n if n >= 10_000 => format!("{}k", n / 1000),
        n if n >= 1000 => format!("{:.1}k", n as f64 / 1e3),
        n => n.to_string(),
    }
}

/// Local time of a Unix timestamp as `MM-DD HH:MM` (UTC offset from the
/// platform is not available without a dependency: shown in UTC, marked).
pub(crate) fn when(unix_s: i64) -> String {
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

/// One orchestrator task (the sidebar's Workers section).
#[derive(Clone, Debug, Default)]
pub struct WorkerLine {
    /// Glyph, id, title, state.
    pub text: String,
    /// The active worker's report: BLOCKERS and NEXT_ACTION first lines.
    pub blockers: Option<String>,
    pub next: Option<String>,
    /// `token-status`: the worker's tokens line (`tokens.rs`), when it is shown.
    pub tokens: Option<String>,
}

/// A hyperlink's target in `w` cells. Where it goes is what must show: the
/// scheme and host stay whole and the rest is cut in the middle (its end,
/// often a file name, stays); controls show as U+FFFD.
pub fn fit_link(uri: &str, w: usize) -> String {
    let uri: String = uri.chars().map(|c| if c.is_control() { '\u{fffd}' } else { c }).collect();
    if width(&uri) <= w {
        return uri;
    }
    // scheme://authority — up to the first '/', '?' or '#' after "//".
    let auth_end = match uri.find("//") {
        Some(i) => uri[i + 2..].find(['/', '?', '#']).map_or(uri.len(), |j| i + 2 + j),
        None => uri.find(':').map_or(0, |i| i + 1),
    };
    let (head, rest) = uri.split_at(auth_end);
    let hw = width(head);
    if hw + 2 > w {
        return fit(head, w);
    }
    format!("{head}{}", fit_left(rest, w - hw))
}

/// A hyperlink's target over a whole row (the status row's place), dim:
/// a link is shown before it can be opened (Ctrl+click), since its text
/// and its target may differ. `note` (why a click did not open it) follows
/// the target, which stays in view.
pub fn link_row(row: &mut [Cell], uri: &str, note: Option<&str>, g: Greys) {
    for c in row.iter_mut() {
        *c = blank(g.dim, g.bar);
    }
    let w = row.len().saturating_sub(2);
    let note = note.map(|n| format!("  · {n}")).unwrap_or_default();
    // The target keeps its width, or half the row when longer.
    let nw = width(&note).min(w - width(uri).min(w / 2));
    let t = fit_link(uri, w - nw);
    let x = put(row, 1, &t, g.dim, g.bar, false);
    put(row, x, &fit(&note, nw), g.normal, g.bar, false);
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
    /// The theme's colours 0–15 (state colours, [`Greys::new`]).
    pub accents: &'a [Rgb; 16],
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
    /// The hyperlink under the pointer, the window row to show it on, and
    /// why a click did not open it.
    pub link: Option<(&'a str, usize, Option<&'a str>)>,
    /// The settings panel drawn over the screen area (its rows, as wide as the area), and its text cursor.
    pub settings: Option<PanelGrid>,
    /// The settings button (DESIGN.md §2.4).
    pub gear: Option<Gear<'a>>,
}

impl Compose<'_> {
    pub fn build(&self) -> Snapshot {
        let s = self.screen;
        let g = Greys::new(s.default_fg, s.default_bg, self.accents);
        let (cols, rows) = (self.cols as usize, self.rows as usize);
        let top = usize::from(self.tab_bar.is_some());
        let mut grid: Vec<Vec<Cell>> = vec![vec![Cell::default(); cols]; rows];
        // Which cells are the program's (from the screen); every other cell
        // is haetty's own and gets CellFlags::HAETTY_UI at the end.
        let mut program = vec![vec![false; cols]; rows];
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
            for ((dst, c), p) in grid[top + y].iter_mut().zip(src).zip(program[top + y].iter_mut()) {
                *dst = c.clone();
                *p = true;
            }
        }
        let area = rows.saturating_sub(top + bottom);
        if let Some(p) = &self.panel {
            let pw = p.first().map_or(0, |r| r.len()).min(cols);
            for (y, prow) in p.iter().take(area).enumerate() {
                for (x, c) in prow.iter().take(pw).enumerate() {
                    grid[top + y][cols - pw + x] = c.clone();
                    program[top + y][cols - pw + x] = false;
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
                program[top + y][..cols - pw].fill(false);
            }
            cursor = None;
        }
        if let Some((rows_of, at)) = &self.settings {
            for (y, r) in rows_of.iter().take(area).enumerate() {
                let w = r.len().min(cols);
                grid[top + y][..w].clone_from_slice(&r[..w]);
                program[top + y][..w].fill(false);
            }
            cursor =
                at.map(|(x, y)| Cursor { x, y: y + top as u16, shape: haetty_core::CursorShape::Bar, blinking: false });
        }
        if let Some(gr) = self.gear {
            if let (Some(row), Some(prow)) = (grid.get_mut(gr.row), program.get_mut(gr.row)) {
                if let (Some(c), Some(p)) = (row.get_mut(gr.col), prow.get_mut(gr.col)) {
                    let bg = c.bg;
                    let fg = if gr.hover { g.normal } else { g.dim };
                    // Over a program's cell its background stays (the default one too: `None`).
                    *c = Cell { text: gr.text.into(), bg, ..blank(fg, g.bg) };
                    *p = false;
                }
            }
        }
        // The palette and hints draw over whatever is there: the cells they
        // changed are theirs.
        let overlay =
            |grid: &mut Vec<Vec<Cell>>, program: &mut Vec<Vec<bool>>, f: &mut dyn FnMut(&mut Vec<Vec<Cell>>)| {
                let before = grid.clone();
                f(grid);
                for ((row, old), prow) in grid.iter().zip(&before).zip(program.iter_mut()) {
                    for ((c, o), p) in row.iter().zip(old).zip(prow.iter_mut()) {
                        if c != o {
                            *p = false;
                        }
                    }
                }
            };
        if let Some((p, keys)) = self.palette {
            overlay(&mut grid, &mut program, &mut |grid| {
                if let Some((x, y)) = p.draw(grid, top, keys, g) {
                    cursor = Some(Cursor { x, y, shape: haetty_core::CursorShape::Bar, blinking: false });
                }
            });
        }
        if let Some((uri, y, note)) = self.link {
            if let (Some(row), Some(prow)) = (grid.get_mut(y), program.get_mut(y)) {
                link_row(row, uri, note, g);
                prow.fill(false);
            }
        }
        if let Some(h) = self.hint {
            overlay(&mut grid, &mut program, &mut |grid| hint(grid, h, g));
        }
        for (row, prow) in grid.iter_mut().zip(&program) {
            for (c, &p) in row.iter_mut().zip(prow) {
                if !p {
                    c.flags.0 |= CellFlags::HAETTY_UI;
                }
            }
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
    fn observable_tab_has_one_dim_mark() {
        let g = Greys::new(
            Rgb { r: 200, g: 200, b: 200 },
            Rgb { r: 20, g: 20, b: 20 },
            &haetty_core::theme::Theme::base().palette,
        );
        let tabs = [
            TabView { name: "one", exited: false, attention: Attention::None, observed: true, clear: false },
            TabView { name: "two", exited: false, attention: Attention::None, observed: false, clear: false },
        ];
        let row = tab_bar(&tabs, 0, 40, g);
        let text: String = row.iter().map(|c| if c.text.is_empty() { " " } else { c.text.as_str() }).collect();
        assert!(text.starts_with(" 1 ◦ one"), "{text:?}");
        assert_eq!(text.matches('◦').count(), 1, "only the observable tab: {text:?}");
        let mark = row.iter().find(|c| c.text == "◦").unwrap();
        assert_eq!(mark.fg, Some(g.dim), "dim, not an accent colour, even on the active tab");
    }

    #[test]
    fn tab_bar_marks_active_tab_without_accent_colours() {
        let g = Greys::from(FG, BG);
        let tabs = [
            TabView { name: "one", exited: false, attention: Attention::None, observed: false, clear: false },
            TabView { name: "two", exited: true, attention: Attention::None, observed: false, clear: false },
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
        p.query = "rename".into();
        p.selected = 0;
        assert_eq!(p.current(), Some(Pick::Action(Action::RenameTab)));
        // Rename: the query is the name; Closed: indices into the list.
        let mut r = Palette::rename("repo:main".into(), Some("mine".into()));
        assert_eq!(r.current(), Some(Pick::Rename("mine".into())));
        r.move_by(1);
        assert_eq!(r.selected, 0);
        let mut c =
            Palette::closed(vec![("a".into(), "~/x · claude · resume".into()), ("b".into(), "~/y · bash".into())]);
        assert_eq!(c.current(), Some(Pick::Closed(0)));
        c.query = "bash".into();
        assert_eq!(c.current(), Some(Pick::Closed(1)));
        let g = Greys::from(FG, BG);
        let mut grid = vec![vec![Cell::default(); 70]; 6];
        c.query.clear();
        c.draw(&mut grid, 0, &Keymap::default(), g);
        let line = |r: &Vec<Cell>| r.iter().map(|c| c.text.clone()).collect::<String>();
        assert!(line(&grid[0]).contains("Reopen:"), "{}", line(&grid[0]));
        assert!(line(&grid[2]).contains("~/y · bash"), "{}", line(&grid[2]));
        // A long folder: the name stays, the folder keeps its end.
        let mut c = Palette::closed(vec![("my tab".into(), format!("/{} · claude · resume", "deep/".repeat(30)))]);
        c.query.clear();
        let mut grid = vec![vec![Cell::default(); 70]; 4];
        c.draw(&mut grid, 0, &Keymap::default(), g);
        let l = line(&grid[1]);
        assert!(l.contains("my tab") && l.contains("…") && l.contains("claude · resume"), "{l}");
        assert_eq!(fit_left("abcdef", 4), "…def");
    }

    #[test]
    fn compose_puts_screen_below_the_tab_bar_and_moves_the_cursor() {
        let mut s = Snapshot { cols: 4, rows: 2, default_fg: FG, default_bg: BG, ..Default::default() };
        s.cells = vec![Cell { text: "x".into(), ..Default::default() }; 8];
        s.cursor = Some(Cursor { x: 1, y: 1, shape: haetty_core::CursorShape::Block, blinking: false });
        let g = Greys::from(FG, BG);
        let tv = |name| TabView { name, exited: false, attention: Attention::None, observed: false, clear: false };
        let bar = tab_bar(&[tv("a"), tv("b")], 1, 4, g);
        let out = Compose {
            screen: &s,
            accents: &haetty_core::theme::Theme::base().palette,
            cols: 4,
            rows: 3,
            tab_bar: Some(bar),
            status: None,
            panel: None,
            diff: None,
            palette: None,
            hint: None,
            link: None,
            settings: None,
            gear: None,
        }
        .build();
        assert_eq!(out.rows, 3);
        assert_eq!(out.cells[4].text, "x");
        assert_eq!(out.cursor.unwrap().y, 2);
        let plain = Compose {
            screen: &s,
            accents: &haetty_core::theme::Theme::base().palette,
            cols: 4,
            rows: 2,
            tab_bar: None,
            status: None,
            panel: None,
            diff: None,
            palette: None,
            hint: Some("hi"),
            link: None,
            settings: None,
            gear: None,
        }
        .build();
        assert_eq!(plain.cursor.unwrap().y, 1);
        assert_eq!(plain.cells[5].text, "h");
        // A status row takes the bottom; the screen keeps its rows above it.
        let st = vec![Cell { text: "S".into(), ..Default::default() }; 4];
        let with = Compose {
            screen: &s,
            accents: &haetty_core::theme::Theme::base().palette,
            cols: 4,
            rows: 3,
            tab_bar: None,
            status: Some(st),
            panel: None,
            diff: None,
            palette: None,
            hint: None,
            link: None,
            settings: None,
            gear: None,
        }
        .build();
        assert_eq!((with.cells[0].text.as_str(), with.cells[8].text.as_str()), ("x", "S"));
        // haetty's own cells are marked, the program's are not.
        let ui = |c: &Cell| c.flags.has(CellFlags::HAETTY_UI);
        assert!(!ui(&with.cells[0]) && ui(&with.cells[8]), "screen vs status row");
        assert!(out.cells[..4].iter().all(ui) && !out.cells[4..].iter().any(ui), "tab bar vs screen");
        assert!(plain.cells[5..].iter().filter(|c| ui(c)).count() >= 2 && !ui(&plain.cells[0]), "hint");
    }

    fn text(row: &[Cell]) -> String {
        row.iter().map(|c| if c.text.is_empty() { " ".into() } else { c.text.clone() }).collect()
    }

    #[test]
    fn a_worker_past_its_context_share_shows_a_dim_clear_mark() {
        let g = Greys::from(FG, BG);
        let mut tab =
            TabView { name: "t1 Fix", exited: false, attention: Attention::None, observed: false, clear: true };
        let marked = text(&tab_bar(std::slice::from_ref(&tab), 0, 40, g));
        assert!(marked.contains("/clear"), "{marked}");
        tab.clear = false;
        assert!(!text(&tab_bar(std::slice::from_ref(&tab), 0, 40, g)).contains("/clear"));
    }

    #[test]
    fn tab_bar_on_the_body_background_with_the_active_tab_underlined() {
        let g = Greys::from(FG, BG);
        let tv = |name| TabView { name, exited: false, attention: Attention::None, observed: false, clear: false };
        let row = tab_bar(&[tv("one"), tv("two"), tv("three")], 1, 60, g);
        assert!(row.iter().all(|c| c.bg == Some(g.bg)), "no band: the terminal's background everywhere");
        let line = |c: &Cell| c.flags.has(CellFlags::TAB_ACTIVE);
        let t = text(&row);
        let (a, b) = (t.find("2 two").unwrap(), t.find("3 three").unwrap());
        assert!(row[a..a + 5].iter().all(|c| c.fg == Some(g.strong) && line(c)), "active: foreground + rule");
        assert!(row[b..b + 7].iter().all(|c| c.fg == Some(g.dim) && !line(c)), "inactive: dim, no rule");
        assert!(row.iter().filter(|c| line(c)).count() >= 5 && row.iter().filter(|c| line(c)).count() <= 20);
    }

    #[test]
    fn attention_marks_only_on_other_tabs() {
        let g = Greys::from(FG, BG);
        let tabs = [
            TabView { name: "a", exited: false, attention: Attention::Wait, observed: false, clear: false },
            TabView { name: "b", exited: false, attention: Attention::Wait, observed: false, clear: false },
            TabView { name: "c", exited: false, attention: Attention::Ask, observed: false, clear: false },
        ];
        let row = tab_bar(&tabs, 0, 60, g);
        let t = text(&row);
        assert!(t.contains("1 a") && t.contains("2 ● b") && t.contains("3 ? c"), "{t}");
        assert!(row.iter().any(|c| c.text == "●" && c.fg == Some(g.wait)));
        assert!(row.iter().any(|c| c.text == "?" && c.fg == Some(g.ask)));
    }

    /// The UI follows the theme: greys from its background and foreground,
    /// marks from its colours 3 and 9; with IBM 5153 CGA the greys stay
    /// neutral (r = g = b) and the marks are brown and bright red.
    #[test]
    fn ui_colours_follow_the_theme() {
        use haetty_core::theme::Theme;
        let tabs = [
            TabView { name: "a", exited: false, attention: Attention::None, observed: false, clear: false },
            TabView { name: "b", exited: false, attention: Attention::Wait, observed: false, clear: false },
            TabView { name: "c", exited: false, attention: Attention::Ask, observed: false, clear: false },
        ];
        let mut seen = vec![];
        for name in ["IBM 5153 CGA", "IBM 5153 CGA (Black)", "haetty classic"] {
            let t = Theme::builtin(name).unwrap();
            let g = Greys::new(t.foreground, t.background, &t.palette);
            assert_eq!((g.bg, g.strong, g.wait, g.ask), (t.background, t.foreground, t.palette[3], t.palette[9]));
            let row = tab_bar(&tabs, 0, 60, g);
            assert!(row.iter().any(|c| c.text == "●" && c.fg == Some(t.palette[3])), "{name}");
            assert!(row.iter().any(|c| c.text == "?" && c.fg == Some(t.palette[9])), "{name}");
            let mut i = ClaudeInfo { context_pct: Some(97.0), ..Default::default() };
            let st = status_row(Some(&i), 60, g, "");
            assert!(st.iter().any(|c| c.fg == Some(t.palette[9])), "{name}: ≥ 95 % = colour 9");
            i.context_pct = Some(10.0);
            for c in status_row(Some(&i), 60, g, "").iter().chain(&row) {
                // Backgrounds all grey; text grey except the two marks
                // (whose trailing space carries the mark colour, unseen).
                let (f, b) = (c.fg.unwrap(), c.bg.unwrap());
                let mark = f == g.wait || f == g.ask;
                assert!(mark || f.r == f.g && f.g == f.b, "{name}: text {f:?} is not grey");
                assert!(b.r == b.g && b.g == b.b, "{name}: background {b:?} is not grey");
            }
            seen.push((g.bar, g.dim, g.wait));
        }
        let cga = Theme::builtin("IBM 5153 CGA").unwrap();
        assert_eq!(seen[0].2, Rgb { r: 0xd0, g: 0x8c, b: 0x1b }, "CGA brown");
        assert_eq!(cga.palette[9], Rgb { r: 0xdc, g: 0x4e, b: 0x4e });
        assert!(seen[0] != seen[1] && seen[0] != seen[2], "another theme, other UI colours");
    }

    #[test]
    fn the_saving_mark_is_dim_and_only_in_saving_mode() {
        let g = Greys::from(FG, BG);
        let info = ClaudeInfo { context_pct: Some(40.0), ..ClaudeInfo::default() };
        let on = text(&status_row_with(Some(&info), 60, g, "", true));
        assert!(on.contains("◆ saving"), "{on}");
        assert!(!text(&status_row_with(Some(&info), 60, g, "", false)).contains("saving"));
    }

    #[test]
    fn status_row_has_fixed_width_numbers_and_greys_when_low() {
        let g = Greys::from(FG, BG);
        let mut i = ClaudeInfo { context_pct: Some(7.0), cost_usd: Some(0.5), files_changed: 3, ..Default::default() };
        i.limits.push(haetty_proto::wire::Limit { name: "five_hour".into(), used_pct: 12.0, resets_at: None });
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
        assert!(row.iter().any(|c| c.fg == Some(g.wait)));
        assert!(text(&row).trim_end().ends_with("esc to interrupt"));
        // Nothing from Claude Code yet: a blank row on the terminal background.
        let none = status_row(None, 40, g, "");
        assert!(none.iter().all(|c| c.text.is_empty() && c.bg == Some(g.bg)), "reserved but empty");
        // Claude Code known but nothing reported yet: blank too.
        let empty = status_row(Some(&ClaudeInfo::default()), 40, g, "");
        assert!(empty.iter().all(|c| c.text.is_empty() && c.bg == Some(g.bg)), "{}", text(&empty));
    }

    #[test]
    fn times_and_sizes() {
        assert_eq!(when(0), "01-01 00:00 UTC");
        assert_eq!(human(1_234_567), "1.2M");
    }

    /// A long target keeps its scheme and host whole and its end; controls
    /// show as U+FFFD; a note follows the target.
    #[test]
    fn link_targets_keep_scheme_host_and_end() {
        let u = "https://example.com/a/very/long/path/to/some/file-name.html";
        assert_eq!(fit_link(u, 80), u);
        let f = fit_link(u, 40);
        assert!(f.starts_with("https://example.com") && f.ends_with("file-name.html") && f.contains('…'), "{f}");
        assert_eq!(width(&f), 40);
        assert!(fit_link("https://a.example/x\u{1b}[2Jy", 80).contains('\u{fffd}'));
        let g = Greys::from(Rgb { r: 200, g: 200, b: 200 }, Rgb { r: 20, g: 20, b: 20 });
        let mut row = vec![Cell::default(); 60];
        link_row(&mut row, "ssh://host/x", Some("not opened (only http, https, file)"), g);
        let t: String = row.iter().map(|c| if c.text.is_empty() { " ".to_string() } else { c.text.clone() }).collect();
        assert!(t.contains("ssh://host/x") && t.contains("not opened (only http, https, file)"), "{t}");
    }
}
