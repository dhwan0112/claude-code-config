//! The settings panel (DESIGN.md §2.4, §3.16): every key of [`crate::config::SETTINGS`] the panel shows, in groups,
//! drawn over the screen area like the diff view. It keeps only its own state (group, selection, an edit, a list,
//! a search, the messages); values come from a [`Source`] and changes go out as [`Effect`]s the window carries out
//! (`Win::settings_effect`): it never writes a file or touches a session itself.
//!
//! Keys: ↑/↓ move, Tab / Shift+Tab change the group, Space or ←/→ change an on/off or a choice, ←/→ step a
//! number, Enter edits (or opens the list of a choice, a theme, a font), typing a digit starts a number's input,
//! `/` searches every group, `r` puts the selected key back to its default, Esc closes (or leaves the edit, the list
//! or the search first). Mouse: a click selects (a group, an item; on a value it acts as Space or Enter), the wheel
//! scrolls.

use haetty_core::{Cell, Rgb};

use crate::config::{fmt_num, Group, Kind, Setting, When, SETTINGS};
use crate::ui::{fit, put, width, Greys};

/// What the panel reads from the window.
pub trait Source {
    /// The value in effect, as the file writes it ("" = from the theme or the platform).
    fn value(&self, key: &str) -> String;
    /// The key was given on the command line: the file's value would not apply.
    fn on_command_line(&self, key: &str) -> bool;
    /// Theme names: built in and in the themes folder.
    fn themes(&self) -> Vec<String>;
    /// Monospaced font families the system has.
    fn fonts(&mut self) -> Vec<String>;
    /// The config file's path, as shown.
    fn path(&self) -> String;
}

/// What the window does for the panel.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    None,
    Close,
    /// Set `key` (`None`: back to the default), apply it and write it into the config file.
    Set {
        key: &'static str,
        value: Option<String>,
    },
    /// The theme list moved: show this theme without writing anything.
    Preview(String),
    /// The theme list was left without a pick: show the theme in effect again.
    EndPreview,
    OpenConfigFile,
    ReloadConfig,
    /// Stage PU: an update action the person chose (only from keys and clicks: `Win::settings_key` / `_click`).
    Update(UpdateCmd),
}

/// The update actions of the Updates group (DESIGN.md §2.4, §3.17).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateCmd {
    CheckNow,
    Download,
    Restart,
    Later,
    RollBack,
}

/// What the Updates group shows (the window fills it in before each frame: `SettingsPanel::set_update`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UpdateView {
    /// "0.4.0 (622d629bb224)".
    pub current: String,
    /// The newer version and its date ("0.4.1 · 2026-11-20").
    pub latest: Option<String>,
    /// Its CHANGELOG section, line by line.
    pub notes: Vec<String>,
    /// Why it cannot be installed here (a dev build, no write permission, too old, no key).
    pub blocked: Option<String>,
    /// "downloading 3.1 of 9.8 MB", "verifying", or why the last try failed.
    pub progress: Option<String>,
    /// Installed and waiting for a restart: that version.
    pub ready: Option<String>,
    /// The version a rollback goes back to.
    pub rollback: Option<String>,
    /// "2026-11-16 17:30: no newer version" and the like.
    pub last_check: String,
    /// What a restart would cut short ("2 tabs wait for a permission answer · 1 worker works").
    pub warn: Option<String>,
    /// Tabs that close and reopen on a restart.
    pub tabs: usize,
    /// After an automatic rollback: what happened.
    pub note: Option<String>,
}

/// A row of the Updates group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Up {
    Current,
    Latest,
    Blocked,
    Notes,
    Download,
    Progress,
    Restart,
    Later,
    RollBack,
    CheckNow,
    LastCheck,
    Note,
}

/// A row of the panel's list.
#[derive(Clone, Copy)]
pub enum Item {
    Setting(&'static Setting),
    OpenFile,
    Reload,
    Update(Up),
}

impl PartialEq for Item {
    fn eq(&self, other: &Item) -> bool {
        match (self, other) {
            (Item::Setting(a), Item::Setting(b)) => a.key == b.key,
            (Item::OpenFile, Item::OpenFile) | (Item::Reload, Item::Reload) => true,
            (Item::Update(a), Item::Update(b)) => a == b,
            _ => false,
        }
    }
}

impl std::fmt::Debug for Item {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Item::Setting(s) => write!(f, "Setting({})", s.key),
            Item::OpenFile => f.write_str("OpenFile"),
            Item::Reload => f.write_str("Reload"),
            Item::Update(u) => write!(f, "Update({u:?})"),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Mode {
    Browse,
    /// Editing a value in its row (text, number, colour).
    Edit {
        buf: String,
    },
    /// Picking from a list (a choice, a theme, a font); `filter` is typed, `entries` the full list.
    List {
        entries: Vec<String>,
        filter: String,
        sel: usize,
    },
    /// Typing a search (`/`).
    Search,
}

pub struct SettingsPanel {
    group: usize,
    sel: usize,
    scroll: usize,
    mode: Mode,
    search: String,
    /// One line per key: why the last change was not made.
    errors: Vec<(&'static str, String)>,
    /// The bottom line's message (saved, re-read, …) until the selection moves.
    note: Option<String>,
    /// The groups in their order: Updates first while a newer version is out (stage PU).
    order: [Group; 7],
    update: UpdateView,
    /// Restart or Roll back pressed once: the next press does it (the bottom line says what it ends).
    confirm: Option<Up>,
}

/// The panel's place in the screen area: two columns when wide enough.
struct Layout {
    two: bool,
    /// Width of the group column (two columns).
    gw: usize,
    /// First column of the items.
    x0: usize,
    /// Columns of a label.
    lw: usize,
    /// First and last+1 rows of the items.
    y0: usize,
    y1: usize,
}

/// Width from which the groups get their own column.
const TWO_COLUMNS: usize = 64;

fn layout(cols: usize, rows: usize) -> Layout {
    let two = cols >= TWO_COLUMNS;
    let gw = if two { 15 } else { 0 };
    let x0 = if two { gw + 2 } else { 1 };
    let lw = ((cols.saturating_sub(x0)) / 2).clamp(10, 24);
    let y0 = if two { 2 } else { 3 };
    Layout { two, gw, x0, lw, y0, y1: rows.saturating_sub(2).max(y0) }
}

/// A key's label: `window-padding-x` → "Window padding x".
pub fn label(key: &str) -> String {
    let s = key.replace('-', " ");
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => s,
    }
}

use crate::ui::blank;

fn mix(a: Rgb, b: Rgb, t: u8) -> Rgb {
    let m = |x: u8, y: u8| ((x as u16 * (255 - t as u16) + y as u16 * t as u16) / 255) as u8;
    Rgb { r: m(a.r, b.r), g: m(a.g, b.g), b: m(a.b, b.b) }
}

fn words_match(q: &str, text: &str) -> bool {
    let l = text.to_lowercase();
    q.to_lowercase().split_whitespace().all(|w| l.contains(w))
}

impl Default for SettingsPanel {
    fn default() -> Self {
        SettingsPanel {
            group: 0,
            sel: 0,
            scroll: 0,
            mode: Mode::Browse,
            search: String::new(),
            errors: vec![],
            note: None,
            order: Group::ALL,
            update: UpdateView::default(),
            confirm: None,
        }
    }
}

impl SettingsPanel {
    /// A panel opened on the Updates group (first in the order) when `update_first`.
    pub fn new(update_first: bool) -> SettingsPanel {
        let mut p = SettingsPanel::default();
        if update_first {
            p.order = [
                Group::Updates,
                Group::Appearance,
                Group::Font,
                Group::Window,
                Group::Terminal,
                Group::Claude,
                Group::Advanced,
            ];
        }
        p
    }

    /// Opens the panel's Updates group (the palette's "Updates").
    pub fn show_updates(&mut self) {
        let i = self.order.iter().position(|g| *g == Group::Updates).unwrap_or(0);
        self.set_group(i);
    }

    pub fn set_update(&mut self, v: UpdateView) {
        self.update = v;
    }

    pub fn group(&self) -> Group {
        self.order[self.group]
    }

    /// The Updates group's rows, by what there is to show.
    fn update_rows(&self) -> Vec<Item> {
        let u = &self.update;
        let mut v = vec![Up::Current];
        if u.note.is_some() {
            v.push(Up::Note);
        }
        if u.latest.is_some() {
            v.push(Up::Latest);
            if u.blocked.is_some() {
                v.push(Up::Blocked);
            }
            if !u.notes.is_empty() {
                v.push(Up::Notes);
            }
            if u.blocked.is_none() && u.ready.is_none() {
                v.push(Up::Download);
            }
        }
        if u.progress.is_some() {
            v.push(Up::Progress);
        }
        if u.ready.is_some() {
            v.extend([Up::Restart, Up::Later]);
        }
        if u.rollback.is_some() {
            v.push(Up::RollBack);
        }
        let mut items: Vec<Item> = v.into_iter().map(Item::Update).collect();
        items.extend(SETTINGS.iter().filter(|s| s.panel && s.group == Group::Updates).map(Item::Setting));
        items.extend([Item::Update(Up::CheckNow), Item::Update(Up::LastCheck)]);
        items
    }

    /// The rows the list shows: the group's settings and the two actions, or the search's matches.
    pub fn items(&self) -> Vec<Item> {
        let shown = SETTINGS.iter().filter(|s| s.panel);
        if !self.search.trim().is_empty() {
            return shown
                .filter(|s| words_match(&self.search, &format!("{} {} {}", s.key, label(s.key), s.line)))
                .map(Item::Setting)
                .collect();
        }
        let g = self.group();
        if g == Group::Updates {
            return self.update_rows();
        }
        let mut v: Vec<Item> = shown.filter(|s| s.group == g).map(Item::Setting).collect();
        v.extend([Item::OpenFile, Item::Reload]);
        v
    }

    pub fn selected(&self) -> Option<Item> {
        self.items().get(self.sel).copied()
    }

    /// Text is being typed (an edit, a list's filter, a search).
    #[cfg(test)]
    pub fn typing(&self) -> bool {
        !matches!(self.mode, Mode::Browse)
    }

    pub fn set_error(&mut self, key: &'static str, why: String) {
        self.errors.retain(|(k, _)| *k != key);
        self.errors.push((key, why));
    }

    pub fn clear_error(&mut self, key: &str) {
        self.errors.retain(|(k, _)| *k != key);
    }

    pub fn note(&mut self, text: String) {
        self.note = Some(text);
    }

    /// The list of the selected item was left with Esc (a theme preview ends there too).
    fn leave_list(&mut self) -> Effect {
        let theme = matches!(self.selected(), Some(Item::Setting(s)) if s.kind == Kind::Theme);
        self.mode = Mode::Browse;
        if theme {
            Effect::EndPreview
        } else {
            Effect::None
        }
    }

    fn move_sel(&mut self, d: isize) {
        let n = self.items().len();
        if n == 0 {
            return;
        }
        self.sel = (self.sel as isize + d).clamp(0, n as isize - 1) as usize;
        self.note = None;
    }

    fn set_group(&mut self, i: usize) {
        self.group = i % self.order.len();
        self.confirm = None;
        self.sel = 0;
        self.scroll = 0;
        self.search.clear();
        self.note = None;
    }

    /// The list entries of a choice, a theme or a font.
    fn entries(s: &Setting, src: &mut dyn Source) -> Vec<String> {
        match s.kind {
            Kind::Choice(list) => list.iter().map(|x| x.to_string()).collect(),
            Kind::Theme => src.themes(),
            Kind::Font => {
                let mut v = vec![String::new()];
                v.extend(src.fonts());
                v
            }
            _ => vec![],
        }
    }

    /// The entries a list shows (its filter applied); a font's typed name is offered too.
    fn shown_entries(entries: &[String], filter: &str) -> Vec<String> {
        entries.iter().filter(|e| filter.is_empty() || words_match(filter, e)).cloned().collect()
    }

    fn step(s: &'static Setting, now: &str, dir: f64) -> Effect {
        let Kind::Number { min, max, step, .. } = s.kind else { return Effect::None };
        let v: f64 = now.replace('_', "").parse().unwrap_or(min);
        let next = ((v / step).round() * step + dir * step).clamp(min, max);
        if next == v {
            return Effect::None;
        }
        Effect::Set { key: s.key, value: Some(fmt_num(next)) }
    }

    fn cycle(s: &'static Setting, now: &str, dir: isize) -> Effect {
        match s.kind {
            Kind::Bool => Effect::Set { key: s.key, value: Some((now != "true").to_string()) },
            Kind::Choice(list) => {
                let i = list.iter().position(|x| *x == now).unwrap_or(0) as isize;
                let n = list.len() as isize;
                Effect::Set { key: s.key, value: Some(list[((i + dir).rem_euclid(n)) as usize].to_string()) }
            }
            _ => Effect::None,
        }
    }

    /// Enter (or a click) on a row of the Updates group. Restart and Roll back take a second press; with a
    /// permission question or a working worker open, the first press moves the selection to Later instead.
    fn update_action(&mut self, u: Up) -> Effect {
        let cmd = match u {
            Up::CheckNow => return Effect::Update(UpdateCmd::CheckNow),
            Up::Download => return Effect::Update(UpdateCmd::Download),
            Up::Later => {
                self.confirm = None;
                return Effect::Update(UpdateCmd::Later);
            }
            Up::Notes => {
                let entries = self.update.notes.clone();
                self.mode = Mode::List { entries, filter: String::new(), sel: 0 };
                return Effect::None;
            }
            Up::Restart => UpdateCmd::Restart,
            Up::RollBack => UpdateCmd::RollBack,
            _ => return Effect::None,
        };
        if self.confirm == Some(u) {
            self.confirm = None;
            return Effect::Update(cmd);
        }
        self.confirm = Some(u);
        let what = if u == Up::Restart { "Restart" } else { "Roll back" };
        let mut text = format!(
            "{what}: {} tab(s) close and open again (Claude Code tabs resume); programs in shell tabs end. Enter again to go on.",
            self.update.tabs
        );
        if let Some(w) = &self.update.warn {
            text = format!("{w} — {text}");
            // The safe choice first: the selection goes to Later (or away from Roll back).
            let items = self.items();
            if let Some(i) = items
                .iter()
                .position(|i| *i == Item::Update(Up::Later))
                .or_else(|| items.iter().position(|i| *i == Item::Update(Up::Current)))
            {
                self.sel = i;
            }
        }
        self.note = Some(text);
        Effect::None
    }

    /// Enter on an item, or a click on its value.
    fn activate(&mut self, src: &mut dyn Source) -> Effect {
        let Some(item) = self.selected() else { return Effect::None };
        let s = match item {
            Item::OpenFile => return Effect::OpenConfigFile,
            Item::Reload => return Effect::ReloadConfig,
            Item::Update(u) => return self.update_action(u),
            Item::Setting(s) => s,
        };
        if src.on_command_line(s.key) {
            self.set_error(s.key, "set on the command line: change it there".into());
            return Effect::None;
        }
        let now = src.value(s.key);
        match s.kind {
            Kind::Bool => Self::cycle(s, &now, 1),
            Kind::Choice(_) | Kind::Theme | Kind::Font => {
                let entries = Self::entries(s, src);
                let sel = entries.iter().position(|e| *e == now).unwrap_or(0);
                self.mode = Mode::List { entries, filter: String::new(), sel };
                Effect::None
            }
            Kind::Number { .. } | Kind::Text { .. } | Kind::Color => {
                self.mode = Mode::Edit { buf: now };
                Effect::None
            }
            Kind::Repeat => Effect::None,
        }
    }

    /// A key while the panel is open. `text`: what the key types (a character key without Ctrl/Alt).
    pub fn key(&mut self, key: &str, shift: bool, text: Option<&str>, src: &mut dyn Source) -> Effect {
        let item = self.selected();
        let setting = match item {
            Some(Item::Setting(s)) => Some(s),
            _ => None,
        };
        match std::mem::replace(&mut self.mode, Mode::Browse) {
            Mode::Edit { mut buf } => {
                let s = setting.expect("an edit has a setting");
                match key {
                    "escape" => {
                        self.clear_error(s.key);
                        return Effect::None;
                    }
                    "enter" => {
                        let v = buf.trim().to_string();
                        if v.is_empty() && !matches!(s.kind, Kind::Color | Kind::Font) {
                            self.set_error(s.key, "empty: r puts the default back".into());
                            self.mode = Mode::Edit { buf };
                            return Effect::None;
                        }
                        if v.is_empty() {
                            return Effect::Set { key: s.key, value: None };
                        }
                        return match s.check_strict(&v) {
                            Ok(_) => Effect::Set { key: s.key, value: Some(v) },
                            Err(e) => {
                                self.set_error(s.key, e);
                                self.mode = Mode::Edit { buf };
                                Effect::None
                            }
                        };
                    }
                    "backspace" => {
                        buf.pop();
                    }
                    _ => {
                        if let Some(t) = text {
                            buf.push_str(t);
                        }
                    }
                }
                self.mode = Mode::Edit { buf };
                Effect::None
            }
            Mode::List { entries, filter, sel } if setting.is_none() => {
                // The release notes (read-only): arrows scroll, anything else leaves.
                let n = entries.len().max(1);
                let sel = match key {
                    "up" => sel.saturating_sub(1),
                    "down" => (sel + 1).min(n - 1),
                    "pageup" => sel.saturating_sub(10),
                    "pagedown" => (sel + 10).min(n - 1),
                    _ => return Effect::None,
                };
                self.mode = Mode::List { entries, filter, sel };
                Effect::None
            }
            Mode::List { entries, mut filter, mut sel } => {
                let s = setting.expect("a list has a setting");
                let shown = Self::shown_entries(&entries, &filter);
                match key {
                    "escape" => {
                        self.mode = Mode::List { entries, filter, sel };
                        return self.leave_list();
                    }
                    "enter" => {
                        let pick = shown.get(sel).cloned().or_else(|| {
                            // A font or theme not in the list: the typed name.
                            (matches!(s.kind, Kind::Font | Kind::Theme) && !filter.trim().is_empty())
                                .then(|| filter.trim().to_string())
                        });
                        return match pick {
                            Some(p) if p.is_empty() => Effect::Set { key: s.key, value: None },
                            Some(p) => Effect::Set { key: s.key, value: Some(p) },
                            None => {
                                self.mode = Mode::List { entries, filter, sel };
                                Effect::None
                            }
                        };
                    }
                    "up" | "down" => {
                        let n = shown.len().max(1);
                        sel = if key == "up" { sel.saturating_sub(1) } else { (sel + 1).min(n - 1) };
                        let preview = (s.kind == Kind::Theme).then(|| shown.get(sel).cloned()).flatten();
                        self.mode = Mode::List { entries, filter, sel };
                        return preview.map_or(Effect::None, Effect::Preview);
                    }
                    "backspace" => {
                        filter.pop();
                        sel = 0;
                    }
                    _ => {
                        if let Some(t) = text {
                            filter.push_str(t);
                            sel = 0;
                        }
                    }
                }
                self.mode = Mode::List { entries, filter, sel };
                Effect::None
            }
            Mode::Search => {
                match key {
                    "escape" => {
                        self.search.clear();
                        self.sel = 0;
                        return Effect::None;
                    }
                    "enter" | "down" | "tab" => return Effect::None,
                    "backspace" => {
                        self.search.pop();
                    }
                    _ => {
                        if let Some(t) = text {
                            self.search.push_str(t);
                        }
                    }
                }
                self.sel = 0;
                self.scroll = 0;
                self.mode = Mode::Search;
                Effect::None
            }
            Mode::Browse => match key {
                "escape" if !self.search.is_empty() => {
                    self.search.clear();
                    self.sel = 0;
                    Effect::None
                }
                "escape" => Effect::Close,
                "up" => {
                    self.move_sel(-1);
                    Effect::None
                }
                "down" => {
                    self.move_sel(1);
                    Effect::None
                }
                "tab" => {
                    let n = self.order.len();
                    self.set_group(if shift { self.group + n - 1 } else { self.group + 1 });
                    Effect::None
                }
                "/" => {
                    self.mode = Mode::Search;
                    Effect::None
                }
                "enter" => self.activate(src),
                "space" | "left" | "right" => {
                    let Some(s) = setting else {
                        return if key == "space" { self.activate(src) } else { Effect::None };
                    };
                    if src.on_command_line(s.key) {
                        self.set_error(s.key, "set on the command line: change it there".into());
                        return Effect::None;
                    }
                    let now = src.value(s.key);
                    let dir = if key == "left" { -1 } else { 1 };
                    match s.kind {
                        Kind::Bool | Kind::Choice(_) => Self::cycle(s, &now, dir),
                        Kind::Number { .. } if key != "space" => Self::step(s, &now, dir as f64),
                        _ if key == "space" => self.activate(src),
                        _ => Effect::None,
                    }
                }
                "r" => match setting {
                    Some(s) if src.on_command_line(s.key) => {
                        self.set_error(s.key, "set on the command line: change it there".into());
                        Effect::None
                    }
                    Some(s) => Effect::Set { key: s.key, value: None },
                    None => Effect::None,
                },
                _ => {
                    // Typing a number's first character starts its input.
                    if let (Some(s), Some(t)) = (setting, text) {
                        if matches!(s.kind, Kind::Number { .. })
                            && t.chars().all(|c| c.is_ascii_digit() || c == '.')
                            && !src.on_command_line(s.key)
                        {
                            self.mode = Mode::Edit { buf: t.to_string() };
                        }
                    }
                    Effect::None
                }
            },
        }
    }

    /// An IME commit (Hangul in a font name, a search).
    pub fn commit(&mut self, text: &str) {
        match &mut self.mode {
            Mode::Edit { buf } => buf.push_str(text),
            Mode::List { filter, sel, .. } => {
                filter.push_str(text);
                *sel = 0;
            }
            Mode::Search => {
                self.search.push_str(text);
                self.sel = 0;
            }
            Mode::Browse => {}
        }
    }

    pub fn wheel(&mut self, notches: i32) {
        self.move_sel(-notches as isize);
    }

    /// A left click at (`col`, `row`) of the screen area (`cols` × `rows`).
    pub fn click(&mut self, col: usize, row: usize, cols: usize, rows: usize, src: &mut dyn Source) -> Effect {
        let l = layout(cols, rows);
        if let Mode::List { entries, filter, .. } = &self.mode {
            let shown = Self::shown_entries(entries, filter);
            let (top, _) = self.list_place(&l, shown.len());
            if row > top && row <= top + shown.len() && col >= l.x0 + l.lw {
                let i = row - top - 1;
                if let Mode::List { sel, .. } = &mut self.mode {
                    *sel = i;
                }
                return self.key("enter", false, None, src);
            }
            return self.leave_list();
        }
        if matches!(self.mode, Mode::Edit { .. } | Mode::Search) {
            self.mode = Mode::Browse;
        }
        // The group column (two columns), or the group line (one column).
        if l.two && col < l.gw {
            if row >= 2 && row - 2 < self.order.len() {
                self.set_group(row - 2);
            }
            return Effect::None;
        }
        if !l.two && row == 1 {
            let (first, mut x) = group_line(&self.order, self.group, cols);
            for (i, g) in self.order.iter().enumerate().skip(first) {
                let w = width(g.label());
                if col >= x && col < x + w {
                    self.set_group(i);
                    return Effect::None;
                }
                x += w + 2;
            }
            return Effect::None;
        }
        if row < l.y0 || row >= l.y1 {
            return Effect::None;
        }
        let i = self.scroll + row - l.y0;
        if i >= self.items().len() {
            return Effect::None;
        }
        let was = self.sel;
        self.sel = i;
        self.note = None;
        // On the value: act; on the label: select (a second click on the label acts too).
        if col >= l.x0 + l.lw || was == i {
            if let Some(Item::Setting(s)) = self.selected() {
                if matches!(s.kind, Kind::Bool | Kind::Choice(_)) {
                    return self.key("space", false, None, src);
                }
            }
            return self.activate(src);
        }
        Effect::None
    }

    /// The rows of an open list: just below the item, or above it when there is no room.
    fn list_place(&self, l: &Layout, n: usize) -> (usize, usize) {
        let row = l.y0 + self.sel.saturating_sub(self.scroll);
        let h = n.min(l.y1.saturating_sub(l.y0 + 1)).max(1);
        if row + 1 + h <= l.y1 {
            (row, h)
        } else {
            (row.saturating_sub(h + 1).max(l.y0.saturating_sub(1)), h)
        }
    }

    /// Draws the panel over `grid` (the screen area: `grid.len()` rows); returns the text cursor while typing.
    pub fn draw(&mut self, grid: &mut [Vec<Cell>], g: Greys, src: &mut dyn Source) -> Option<(u16, u16)> {
        let rows = grid.len();
        let cols = grid.first().map_or(0, |r| r.len());
        if rows < 4 || cols < 20 {
            return None;
        }
        let l = layout(cols, rows);
        let bg = g.bar;
        let sel_bg = mix(bg, g.strong, 40);
        for r in grid.iter_mut() {
            for c in r.iter_mut() {
                *c = blank(g.normal, bg);
            }
        }
        let mut cursor = None;
        // Title row: the name, the search or the keys.
        put(&mut grid[0], 1, "Settings", g.strong, bg, true);
        if matches!(self.mode, Mode::Search) || !self.search.is_empty() {
            let x = put(&mut grid[0], 11, "/ ", g.dim, bg, false);
            let end = put(&mut grid[0], x, &fit(&self.search, cols.saturating_sub(x + 2)), g.strong, bg, false);
            if matches!(self.mode, Mode::Search) {
                cursor = Some((end as u16, 0));
            }
        } else {
            let room = cols.saturating_sub(12);
            let keys = [
                "↑↓ move · Tab group · / search · r default · Esc close",
                "/ search · r default · Esc close",
                "Esc close",
            ]
            .into_iter()
            .find(|k| width(k) <= room)
            .unwrap_or("");
            let k = keys.to_string();
            put(&mut grid[0], cols.saturating_sub(width(&k) + 1), &k, g.dim, bg, false);
        }
        // Groups.
        let searching = !self.search.trim().is_empty();
        if l.two {
            for (i, gr) in self.order.iter().enumerate() {
                let y = 2 + i;
                if y >= rows - 1 {
                    break;
                }
                let on = i == self.group && !searching;
                if on {
                    for c in &mut grid[y][..l.gw] {
                        *c = blank(g.normal, sel_bg);
                    }
                }
                put(
                    &mut grid[y],
                    1,
                    &fit(gr.label(), l.gw - 2),
                    if on { g.strong } else { g.normal },
                    if on { sel_bg } else { bg },
                    on,
                );
            }
            for r in grid.iter_mut().take(rows - 1).skip(1) {
                r[l.gw] = Cell { text: "│".into(), ..blank(g.dim, bg) };
            }
        } else {
            let (first, _) = group_line(&self.order, self.group, cols);
            let mut x = 1;
            if first > 0 {
                x = put(&mut grid[1], x, "‹ ", g.dim, bg, false);
            }
            for (i, gr) in self.order.iter().enumerate().skip(first) {
                let on = i == self.group && !searching;
                if x + width(gr.label()) + 2 >= cols {
                    put(&mut grid[1], cols - 2, "›", g.dim, bg, false);
                    break;
                }
                x = put(&mut grid[1], x, gr.label(), if on { g.strong } else { g.dim }, bg, on) + 2;
            }
        }
        // Items.
        let items = self.items();
        self.sel = self.sel.min(items.len().saturating_sub(1));
        let h = l.y1 - l.y0;
        if self.sel < self.scroll {
            self.scroll = self.sel;
        } else if h > 0 && self.sel >= self.scroll + h {
            self.scroll = self.sel + 1 - h;
        }
        let right = cols - 1;
        for (i, item) in items.iter().enumerate().skip(self.scroll).take(h) {
            let y = l.y0 + i - self.scroll;
            let on = i == self.sel;
            let rbg = if on { sel_bg } else { bg };
            if on {
                for c in &mut grid[y][l.x0 - 1..right] {
                    *c = blank(g.normal, rbg);
                }
            }
            let fg = if on { g.strong } else { g.normal };
            match item {
                Item::OpenFile => {
                    let x = put(&mut grid[y], l.x0, "Open config file", fg, rbg, on);
                    let p = src.path();
                    put(&mut grid[y], x + 2, &crate::ui::fit_left(&p, right.saturating_sub(x + 3)), g.dim, rbg, false);
                }
                Item::Reload => {
                    put(&mut grid[y], l.x0, "Reload config", fg, rbg, on);
                }
                Item::Update(u) => {
                    let v = &self.update;
                    let (label, value): (String, String) = match u {
                        Up::Current => ("Version".into(), v.current.clone()),
                        Up::Note => ("Rolled back".into(), v.note.clone().unwrap_or_default()),
                        Up::Latest => ("New version".into(), v.latest.clone().unwrap_or_default()),
                        Up::Blocked => ("Can't install here".into(), v.blocked.clone().unwrap_or_default()),
                        Up::Notes => (
                            "What's new".into(),
                            v.notes
                                .iter()
                                .find(|l| !l.trim().is_empty() && !l.starts_with('#'))
                                .cloned()
                                .unwrap_or_default(),
                        ),
                        Up::Download => ("Download and install".into(), String::new()),
                        Up::Progress => ("Progress".into(), v.progress.clone().unwrap_or_default()),
                        Up::Restart => ("Restart now".into(), format!("into {}", v.ready.clone().unwrap_or_default())),
                        Up::Later => ("Later".into(), "the new version starts with haetty's next start".into()),
                        Up::RollBack => {
                            (format!("Roll back to {}", v.rollback.clone().unwrap_or_default()), String::new())
                        }
                        Up::CheckNow => ("Check now".into(), String::new()),
                        Up::LastCheck => ("Last check".into(), v.last_check.clone()),
                    };
                    let x = put(&mut grid[y], l.x0, &fit(&label, right.saturating_sub(l.x0 + 1)), fg, rbg, on);
                    let vx = (l.x0 + l.lw).max(x + 2);
                    let dim_value = matches!(u, Up::Blocked | Up::Notes | Up::Later | Up::Note | Up::LastCheck);
                    let mut end = put(
                        &mut grid[y],
                        vx,
                        &fit(&value, right.saturating_sub(vx + 1)),
                        if dim_value { g.dim } else { fg },
                        rbg,
                        false,
                    );
                    if self.confirm == Some(*u) && end + 2 < right {
                        end = put(&mut grid[y], end + 2, &fit("press again", right - end - 2), g.dim, rbg, false);
                    }
                    let _ = end;
                }
                Item::Setting(s) => {
                    put(&mut grid[y], l.x0, &fit(&label(s.key), l.lw - 1), fg, rbg, on);
                    let vx = l.x0 + l.lw;
                    let room = right.saturating_sub(vx + 1);
                    let now = src.value(s.key);
                    let editing = on && matches!(self.mode, Mode::Edit { .. });
                    let shown = if let (true, Mode::Edit { buf }) = (editing, &self.mode) {
                        buf.clone()
                    } else {
                        shown_value(s, &now)
                    };
                    let dim_value = now.is_empty() && !editing;
                    let mut x = put(
                        &mut grid[y],
                        vx,
                        &if editing { crate::ui::fit_left(&shown, room) } else { fit(&shown, room / 2 + 4) },
                        if dim_value { g.dim } else { fg },
                        rbg,
                        false,
                    );
                    if editing {
                        cursor = Some((x as u16, y as u16));
                    }
                    let default =
                        if s.key == "engine" { haetty_core::vt::EngineKind::default_kind().name() } else { s.default };
                    let error = self.errors.iter().find(|(k, _)| *k == s.key).map(|(_, e)| e.as_str());
                    let mut marks: Vec<String> = vec![];
                    if !editing && now != default && !(now.is_empty() && default.is_empty()) {
                        marks.push("•".into());
                    }
                    if src.on_command_line(s.key) {
                        marks.push("command line".into());
                    } else if s.when == When::NextStart {
                        marks.push("next start".into());
                    }
                    if let Some(e) = error {
                        marks = vec![e.to_string()];
                    }
                    for m in marks {
                        if x + 2 >= right {
                            break;
                        }
                        x = put(&mut grid[y], x + 2, &fit(&m, right - x - 2), g.dim, rbg, false);
                    }
                }
            }
        }
        // The release notes: their lines over the list area, the arrows scroll.
        if let (Mode::List { entries, sel, .. }, Some(Item::Update(Up::Notes))) = (&self.mode, self.selected()) {
            let lbg = mix(bg, g.strong, 20);
            for (k, y) in (l.y0..l.y1).enumerate() {
                for c in &mut grid[y][l.x0 - 1..right] {
                    *c = blank(g.normal, lbg);
                }
                if let Some(line) = entries.get(sel + k) {
                    put(&mut grid[y], l.x0, &fit(line, right.saturating_sub(l.x0 + 1)), g.normal, lbg, false);
                }
            }
        }
        // An open list, under its item.
        if let (Mode::List { entries, filter, sel }, Some(Item::Setting(s))) = (&self.mode, self.selected()) {
            let shown = Self::shown_entries(entries, filter);
            let (top, n) = self.list_place(&l, shown.len().max(1));
            let x0 = l.x0 + l.lw - 1;
            let w = right.saturating_sub(x0).min(48);
            let lbg = mix(bg, g.strong, 20);
            // The filter line replaces the item's value.
            for c in &mut grid[top][x0..x0 + w] {
                *c = blank(g.normal, lbg);
            }
            let x =
                put(&mut grid[top], x0 + 1, if filter.is_empty() { "type to filter: " } else { "" }, g.dim, lbg, false);
            let end = put(
                &mut grid[top],
                x,
                &crate::ui::fit_left(filter, w.saturating_sub(x - x0 + 2)),
                g.strong,
                lbg,
                false,
            );
            cursor = Some((end as u16, top as u16));
            let first = sel.saturating_sub(n.saturating_sub(1));
            for (k, e) in shown.iter().enumerate().skip(first).take(n) {
                let y = top + 1 + k - first;
                if y >= rows - 1 {
                    break;
                }
                let on = k == *sel;
                let ebg = if on { sel_bg } else { lbg };
                for c in &mut grid[y][x0..x0 + w] {
                    *c = blank(g.normal, ebg);
                }
                let text = if e.is_empty() { default_name(s) } else { e.clone() };
                put(&mut grid[y], x0 + 1, &fit(&text, w - 2), if on { g.strong } else { g.normal }, ebg, on);
            }
            if shown.is_empty() {
                let y = (top + 1).min(rows - 2);
                for c in &mut grid[y][x0..x0 + w] {
                    *c = blank(g.dim, lbg);
                }
                let t =
                    if matches!(s.kind, Kind::Font | Kind::Theme) { "Enter uses the typed name" } else { "no match" };
                put(&mut grid[y], x0 + 1, t, g.dim, lbg, false);
            }
        }
        // Bottom line: the selected key's description and its name in the file, or a message.
        let y = rows - 1;
        // The key in the file at the right end, whole; the description gets the room left.
        let key = match (&self.note, self.selected()) {
            (None, Some(Item::Setting(s))) => s.key,
            _ => "",
        };
        let kx = cols.saturating_sub(width(key) + 1);
        put(&mut grid[y], kx, key, g.dim, bg, false);
        let room = if key.is_empty() { cols - 2 } else { kx.saturating_sub(3) };
        let text = match (&self.note, self.selected()) {
            (Some(n), _) => n.clone(),
            (None, Some(Item::Setting(s))) => s.line.to_string(),
            (None, Some(Item::OpenFile)) => "Opens the file in a new tab with $EDITOR (none: only the path).".into(),
            (None, Some(Item::Reload)) => "Reads the config file again.".into(),
            (None, Some(Item::Update(u))) => match u {
                Up::Download => {
                    "Downloads the release, checks its signature and sha256, installs it beside this one.".into()
                }
                Up::Notes => "Enter shows the release notes; ↑/↓ scroll, Esc goes back.".into(),
                Up::Restart => "Saves the tabs, ends this window and its sessions, starts the new version.".into(),
                Up::RollBack => "Points haetty at the version before; then the same restart as Restart now.".into(),
                Up::CheckNow => "Asks the release server once (only haetty's version is sent).".into(),
                _ => "Updates: one request a day, signed releases only (README, \"Updates\").".into(),
            },
            (None, None) => "No setting matches.".into(),
        };
        put(&mut grid[y], 1, &fit(&text, room), g.dim, bg, false);
        cursor
    }
}

/// The one-line group list of a narrow panel: the first group shown (so the current one is in view) and the column
/// of its label.
fn group_line(order: &[Group], current: usize, cols: usize) -> (usize, usize) {
    let w = |i: usize| width(order[i].label()) + 2;
    let mut first = 0;
    while first < current && 1 + 2 + (first..=current).map(w).sum::<usize>() + 2 > cols {
        first += 1;
    }
    (first, if first > 0 { 3 } else { 1 })
}

/// The name a list shows for "" (the default).
fn default_name(s: &Setting) -> String {
    match s.kind {
        Kind::Font => "default (the platform's monospaced font)".into(),
        _ => "default".into(),
    }
}

/// A value as its row shows it.
pub fn shown_value(s: &Setting, now: &str) -> String {
    match s.kind {
        Kind::Bool => if now == "true" { "on" } else { "off" }.into(),
        Kind::Choice(_) => format!("‹ {now} ›"),
        Kind::Color if now.is_empty() => "the theme's".into(),
        Kind::Font if now.is_empty() => "default".into(),
        _ => now.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake {
        values: Vec<(String, String)>,
        cli: Vec<&'static str>,
    }

    impl Source for Fake {
        fn value(&self, key: &str) -> String {
            self.values
                .iter()
                .rev()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.clone())
                .unwrap_or_else(|| crate::config::Config::default().value(key).unwrap_or_default())
        }
        fn on_command_line(&self, key: &str) -> bool {
            self.cli.contains(&key)
        }
        fn themes(&self) -> Vec<String> {
            vec!["IBM 5153 CGA".into(), "IBM 5153 CGA (Black)".into(), "haetty classic".into()]
        }
        fn fonts(&mut self) -> Vec<String> {
            vec!["DejaVu Sans Mono".into(), "Noto Sans Mono".into()]
        }
        fn path(&self) -> String {
            "/x/haetty-dev/config".into()
        }
    }

    fn fake() -> Fake {
        Fake { values: vec![], cli: vec![] }
    }

    fn select(p: &mut SettingsPanel, key: &str) {
        let gi = Group::ALL.iter().position(|g| *g == Setting::find(key).unwrap().group).unwrap();
        p.set_group(gi);
        p.sel = p.items().iter().position(|i| matches!(i, Item::Setting(s) if s.key == key)).unwrap();
    }

    fn set(key: &'static str, v: &str) -> Effect {
        Effect::Set { key, value: Some(v.into()) }
    }

    #[test]
    fn every_panel_setting_is_in_a_group_and_the_actions_end_each_group() {
        let mut p = SettingsPanel::default();
        let mut n = 0;
        for i in 0..Group::ALL.len() {
            p.set_group(i);
            let items = p.items();
            let end = if p.group() == Group::Updates {
                [Item::Update(Up::CheckNow), Item::Update(Up::LastCheck)]
            } else {
                [Item::OpenFile, Item::Reload]
            };
            assert_eq!(&items[items.len() - 2..], &end);
            n += items.iter().filter(|i| matches!(i, Item::Setting(_))).count();
        }
        assert_eq!(n, SETTINGS.iter().filter(|s| s.panel).count());
        for k in ["saving-mode", "worker-command", "palette", "keybind", "window-padding"] {
            assert!(!Setting::find(k).unwrap().panel, "{k} is not in the panel");
        }
    }

    #[test]
    fn keys_change_values_through_effects() {
        let mut src = fake();
        let mut p = SettingsPanel::default();
        // A bool: Space and ←/→ flip it.
        select(&mut p, "notifications");
        assert_eq!(p.key("space", false, None, &mut src), set("notifications", "false"));
        // A choice cycles both ways; Enter opens its list.
        select(&mut p, "window-padding-color");
        assert_eq!(p.key("right", false, None, &mut src), set("window-padding-color", "extend-always"));
        assert_eq!(p.key("left", false, None, &mut src), set("window-padding-color", "background"));
        assert_eq!(p.key("enter", false, None, &mut src), Effect::None);
        assert!(p.typing());
        assert_eq!(p.key("down", false, None, &mut src), Effect::None);
        assert_eq!(p.key("enter", false, None, &mut src), set("window-padding-color", "extend-always"));
        // A number steps, and typed input is checked strictly with the reason on its row.
        select(&mut p, "font-size");
        assert_eq!(p.key("right", false, None, &mut src), set("font-size", "12"));
        assert_eq!(p.key("1", false, Some("1"), &mut src), Effect::None);
        p.key("0", false, Some("0"), &mut src);
        p.key("0", false, Some("0"), &mut src);
        assert_eq!(p.key("enter", false, None, &mut src), Effect::None, "100 is out of range");
        assert_eq!(p.errors, vec![("font-size", "a number from 4 to 96".to_string())]);
        assert!(p.typing(), "the input stays open");
        p.key("backspace", false, None, &mut src);
        assert_eq!(p.key("enter", false, None, &mut src), set("font-size", "10"));
        // r: back to the default.
        assert_eq!(p.key("r", false, Some("r"), &mut src), Effect::Set { key: "font-size", value: None });
        // Esc closes from browsing.
        assert_eq!(p.key("escape", false, None, &mut src), Effect::Close);
    }

    #[test]
    fn the_theme_list_previews_and_esc_ends_the_preview() {
        let mut src = fake();
        let mut p = SettingsPanel::default();
        select(&mut p, "theme");
        assert_eq!(p.key("enter", false, None, &mut src), Effect::None);
        assert_eq!(p.key("down", false, None, &mut src), Effect::Preview("IBM 5153 CGA (Black)".into()));
        assert_eq!(p.key("down", false, None, &mut src), Effect::Preview("haetty classic".into()));
        assert_eq!(p.key("escape", false, None, &mut src), Effect::EndPreview);
        assert!(!p.typing());
        // Typed filter, then Enter picks the match.
        p.key("enter", false, None, &mut src);
        p.commit("classic");
        assert_eq!(p.key("enter", false, None, &mut src), set("theme", "haetty classic"));
    }

    #[test]
    fn a_font_name_can_be_typed_with_an_ime_or_picked() {
        let mut src = fake();
        let mut p = SettingsPanel::default();
        select(&mut p, "font-family");
        p.key("enter", false, None, &mut src);
        p.commit("나눔고딕코딩");
        assert_eq!(p.key("enter", false, None, &mut src), set("font-family", "나눔고딕코딩"));
        p.key("enter", false, None, &mut src);
        p.key("down", false, None, &mut src);
        assert_eq!(p.key("enter", false, None, &mut src), set("font-family", "DejaVu Sans Mono"));
        // The first entry is the default.
        p.key("enter", false, None, &mut src);
        p.key("up", false, None, &mut src);
        p.key("up", false, None, &mut src);
        assert_eq!(p.key("enter", false, None, &mut src), Effect::Set { key: "font-family", value: None });
    }

    #[test]
    fn search_lists_matches_of_every_group_and_command_line_keys_are_not_changed() {
        let mut src = Fake { values: vec![], cli: vec!["font-size"] };
        let mut p = SettingsPanel::default();
        p.key("/", false, Some("/"), &mut src);
        for c in "padding".chars() {
            p.key(&c.to_string(), false, Some(&c.to_string()), &mut src);
        }
        let keys: Vec<&str> =
            p.items().iter().filter_map(|i| if let Item::Setting(s) = i { Some(s.key) } else { None }).collect();
        assert_eq!(keys, ["window-padding-x", "window-padding-y", "window-padding-balance", "window-padding-color"]);
        p.key("escape", false, None, &mut src);
        assert!(!p.typing() && p.items().len() > 4);
        select(&mut p, "font-size");
        assert_eq!(p.key("right", false, None, &mut src), Effect::None);
        assert!(p.errors[0].1.contains("command line"));
    }

    #[test]
    fn drawing_fits_wide_and_narrow_and_uses_greys_only() {
        let g = Greys::from(Rgb { r: 200, g: 200, b: 200 }, Rgb { r: 20, g: 20, b: 20 });
        for (cols, rows) in [(100, 30), (40, 12), (20, 4)] {
            let mut grid = vec![vec![Cell::default(); cols]; rows];
            let mut p = SettingsPanel::default();
            p.set_group(5);
            p.draw(&mut grid, g, &mut fake());
            let text: String =
                grid.iter().map(|r| r.iter().map(|c| c.text.as_str()).collect::<String>() + "\n").collect();
            assert!(text.contains("Settings"), "{text}");
            if (40..TWO_COLUMNS).contains(&cols) {
                assert!(text.contains("‹ ") && text.contains("Advanced"), "the current group in view: {text}");
            }
            if cols >= TWO_COLUMNS {
                assert!(text.contains("Claude Code") && text.contains("│"), "{text}");
                assert!(text.contains("Engine") && text.contains("next start"), "{text}");
            }
            for c in grid.iter().flatten() {
                for col in [c.fg, c.bg].into_iter().flatten() {
                    assert!(col.r == col.g && col.g == col.b, "a grey only: {col:?}");
                }
            }
        }
    }

    fn update_panel(warn: Option<&str>) -> SettingsPanel {
        let mut p = SettingsPanel::new(true);
        p.set_update(UpdateView {
            current: "0.4.0 (aaaaaaaaaaaa)".into(),
            latest: Some("0.4.1 · 2026-11-20".into()),
            notes: vec!["### Fixed".into(), "- a thing".into()],
            ready: Some("0.4.1-bbbbbbbbbbbb".into()),
            rollback: Some("0.4.0-aaaaaaaaaaaa".into()),
            warn: warn.map(str::to_string),
            tabs: 3,
            ..Default::default()
        });
        p
    }

    fn select_up(p: &mut SettingsPanel, u: Up) {
        p.sel = p.items().iter().position(|i| *i == Item::Update(u)).unwrap();
    }

    #[test]
    fn updates_come_first_and_restart_takes_two_presses() {
        let mut src = fake();
        let mut p = update_panel(None);
        assert_eq!(p.group(), Group::Updates, "first while a newer version is out");
        assert_eq!(SettingsPanel::new(false).group(), Group::Appearance);
        select_up(&mut p, Up::Restart);
        assert_eq!(p.key("enter", false, None, &mut src), Effect::None, "the first press only asks");
        assert!(p.note.as_deref().unwrap().contains("3 tab(s) close"), "{:?}", p.note);
        assert_eq!(p.key("enter", false, None, &mut src), Effect::Update(UpdateCmd::Restart));
        select_up(&mut p, Up::RollBack);
        p.key("enter", false, None, &mut src);
        assert_eq!(p.key("enter", false, None, &mut src), Effect::Update(UpdateCmd::RollBack));
        select_up(&mut p, Up::CheckNow);
        assert_eq!(p.key("enter", false, None, &mut src), Effect::Update(UpdateCmd::CheckNow));
        // The notes open as a list; Esc goes back.
        select_up(&mut p, Up::Notes);
        p.key("enter", false, None, &mut src);
        assert!(p.typing());
        p.key("escape", false, None, &mut src);
        assert!(!p.typing());
    }

    #[test]
    fn with_a_permission_wait_open_the_first_press_moves_to_later() {
        let mut src = fake();
        let mut p = update_panel(Some("1 tab(s) wait for a permission answer"));
        select_up(&mut p, Up::Restart);
        assert_eq!(p.key("enter", false, None, &mut src), Effect::None);
        assert_eq!(p.selected(), Some(Item::Update(Up::Later)), "the safe choice is selected");
        assert_eq!(p.key("enter", false, None, &mut src), Effect::Update(UpdateCmd::Later), "Enter now means Later");
    }
}
