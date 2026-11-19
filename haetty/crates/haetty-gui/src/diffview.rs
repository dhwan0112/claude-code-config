//! The diff view (DESIGN.md §3.13, §13.4), drawn over the tab's screen (the
//! program keeps its size and keeps running). Three views, `m`/Tab cycles:
//! the last edit Claude Code made in the active tab (before against after),
//! that file against git `HEAD`, and the whole working tree against `HEAD`
//! (untracked files included, so changes made by Bash show) — the git views
//! from `gitview.rs`, with its limits. Side by side or stacked (removed lines
//! above the added ones), by width. Hidden by default, never opens by itself.
//! Keys: j/k ↓/↑ line, Space/PageDown and b/PageUp page, g/Home and
//! G/End ends, ]/[ next and previous hunk, n/p next and previous file (the
//! caller loads it), v layout, m/Tab next view, r reread, Esc or q closes.

use haetty_cc::similar::{ChangeTag, TextDiff};
use haetty_core::{Cell, Rgb};
use haetty_proto::wire::{EditView, SnapStatus};

use crate::gitview::Collected;
use crate::ui::{fit, put, Greys};

/// Which view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Edit,
    Head,
    Tree,
}

impl Mode {
    pub fn next(self) -> Mode {
        match self {
            Mode::Edit => Mode::Head,
            Mode::Head => Mode::Tree,
            Mode::Tree => Mode::Edit,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Mode::Edit => "last edit",
            Mode::Head => "vs HEAD",
            Mode::Tree => "working tree",
        }
    }
}

/// What a key asks of the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Stay,
    Close,
    /// Show the next view.
    Next,
    /// Build this view again.
    Refresh,
    /// Load the next file of the list.
    NextFile,
    /// Load the previous file of the list.
    PrevFile,
    /// Switch between side by side and stacked.
    ToggleLayout,
}

/// How the rows are laid out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    Split,
    Stacked,
}

impl Layout {
    pub fn label(self) -> &'static str {
        match self {
            Layout::Split => "split",
            Layout::Stacked => "stacked",
        }
    }
}

/// The `diff-view` setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffLayoutPref {
    Auto,
    Split,
    Stacked,
}

impl From<crate::config::DiffViewSetting> for DiffLayoutPref {
    fn from(s: crate::config::DiffViewSetting) -> DiffLayoutPref {
        match s {
            crate::config::DiffViewSetting::Auto => DiffLayoutPref::Auto,
            crate::config::DiffViewSetting::Split => DiffLayoutPref::Split,
            crate::config::DiffViewSetting::Stacked => DiffLayoutPref::Stacked,
        }
    }
}

/// A file of the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffFile {
    pub path: String,
    pub added: usize,
    pub removed: usize,
}

/// The rectangle `draw` fills: columns `x..x + w`, rows `y..y + h` of the grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Same,
    Del,
    Add,
    /// A pair: removed on the left, added on the right.
    Change,
    Hunk,
    /// A file's name (git views).
    File,
}

#[derive(Clone, Debug)]
struct Row {
    kind: Kind,
    old: Option<(usize, String)>,
    new: Option<(usize, String)>,
}

#[derive(Default)]
struct Built {
    unified: Vec<Row>,
    split: Vec<Row>,
    hunks_u: Vec<usize>,
    hunks_s: Vec<usize>,
}

pub struct DiffView {
    pub mode: Mode,
    header: String,
    /// Time the git view took to collect.
    elapsed: Option<std::time::Duration>,
    note: Option<String>,
    unified: Vec<Row>,
    split: Vec<Row>,
    hunks_u: Vec<usize>,
    hunks_s: Vec<usize>,
    files: Vec<DiffFile>,
    current: usize,
    pref: DiffLayoutPref,
    split_cols: usize,
    root: String,
    pub scroll: usize,
}

/// One line of text for a cell row: tabs as 4 spaces, no control characters.
fn clean(s: &str) -> String {
    s.trim_end_matches(['\n', '\r']).replace('\t', "    ").chars().filter(|c| !c.is_control()).collect()
}

impl DiffView {
    pub fn new(edit: Option<&EditView>) -> DiffView {
        let Some(e) = edit else {
            return DiffView::note_only(
                Mode::Edit,
                "[last edit] none yet".into(),
                "Claude Code has not edited a file in this tab yet (m: the working tree).".into(),
            );
        };
        let m = &e.meta;
        let header = format!("[last edit] {} · +{} -{} · by {}", m.path, m.added, m.removed, m.agent);
        let file = DiffFile { path: m.path.clone(), added: m.added as usize, removed: m.removed as usize };
        if !m.diffable {
            let why = match e.status {
                SnapStatus::TooLarge => "the file is larger than 2 MiB",
                SnapStatus::Binary => "the file is not text",
                SnapStatus::Skipped => "credential-like files are never read",
                SnapStatus::New | SnapStatus::Copied => "the edit could not be rebuilt",
                SnapStatus::Error => "the file could not be read before the edit",
            };
            let mut v = DiffView::note_only(Mode::Edit, header, format!("No diff: {why}."));
            v.files = vec![file];
            return v;
        }
        let mut b = Built::default();
        diff_rows(&e.before.0, &e.after.0, &mut b);
        let note = b.unified.is_empty().then(|| "The edit changed nothing.".to_string());
        DiffView {
            mode: Mode::Edit,
            header,
            elapsed: None,
            note,
            unified: b.unified,
            split: b.split,
            hunks_u: b.hunks_u,
            hunks_s: b.hunks_s,
            files: vec![file],
            current: 0,
            pref: DiffLayoutPref::Auto,
            split_cols: DEFAULT_SPLIT_COLS,
            root: String::new(),
            scroll: 0,
        }
    }

    pub fn note_only(mode: Mode, header: String, note: String) -> DiffView {
        DiffView {
            mode,
            header,
            elapsed: None,
            note: Some(note),
            unified: vec![],
            split: vec![],
            hunks_u: vec![],
            hunks_s: vec![],
            files: vec![],
            current: 0,
            pref: DiffLayoutPref::Auto,
            split_cols: DEFAULT_SPLIT_COLS,
            root: String::new(),
            scroll: 0,
        }
    }

    /// A git view: [`Mode::Head`] (one file) or [`Mode::Tree`].
    pub fn git(mode: Mode, c: Result<Collected, String>) -> DiffView {
        let c = match c {
            Ok(c) => c,
            Err(e) => return DiffView::note_only(mode, format!("[{}]", mode.label()), format!("No diff: {e}.")),
        };
        let mut b = Built::default();
        for n in &c.notes {
            let r = Row { kind: Kind::Hunk, old: Some((0, format!("({n})"))), new: None };
            b.unified.push(r.clone());
            b.split.push(r);
        }
        let (mut added, mut removed) = (0, 0);
        let mut files = vec![];
        for f in &c.files {
            let at = b.unified.len();
            let sat = b.split.len();
            let (hu, hs) = (b.hunks_u.len(), b.hunks_s.len());
            let (mut fa, mut fd) = (0, 0);
            let title = match (&f.before, &f.after, f.why) {
                (Some(bf), Some(af), _) => {
                    diff_rows(bf, af, &mut b);
                    fa = b.unified[at..].iter().filter(|r| r.kind == Kind::Add).count();
                    fd = b.unified[at..].iter().filter(|r| r.kind == Kind::Del).count();
                    added += fa;
                    removed += fd;
                    if fa + fd == 0 {
                        // Only a mode or line-ending change git sees.
                        format!("{} · no text change", f.path)
                    } else {
                        format!("{} · +{fa} -{fd}", f.path)
                    }
                }
                (_, _, why) => format!("{} · {}", f.path, why.unwrap_or("not read")),
            };
            let full = c.top.join(&f.path).to_string_lossy().into_owned();
            files.push(DiffFile { path: full, added: fa, removed: fd });
            if mode == Mode::Tree {
                let r = Row { kind: Kind::File, old: Some((0, title)), new: None };
                b.unified.insert(at, r.clone());
                b.split.insert(sat, r);
                b.hunks_u[hu..].iter_mut().for_each(|h| *h += 1);
                b.hunks_s[hs..].iter_mut().for_each(|h| *h += 1);
                if let Some(h) = b.hunks_u.get_mut(hu) {
                    *h = at;
                }
                if let Some(h) = b.hunks_s.get_mut(hs) {
                    *h = sat;
                }
            }
        }
        let top = c.top.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let header = match mode {
            Mode::Head => {
                let p = c.files.first().map_or("", |f| f.path.as_str());
                match c.files.first().and_then(|f| f.why) {
                    Some(w) => format!("[vs HEAD] {top}/{p} · {w}"),
                    None => format!("[vs HEAD] {top}/{p} · +{added} -{removed}"),
                }
            }
            _ => format!("[working tree] {top} · {} file(s) · +{added} -{removed}", c.changed),
        };
        let note = if c.files.is_empty() {
            Some("No changes against HEAD.".to_string())
        } else if mode == Mode::Head && b.unified.is_empty() {
            Some(match c.files[0].why {
                Some(w) => format!("No diff: {w}."),
                None => "The file is as in HEAD.".to_string(),
            })
        } else {
            None
        };
        DiffView {
            mode,
            header,
            elapsed: Some(c.elapsed),
            note,
            unified: b.unified,
            split: b.split,
            hunks_u: b.hunks_u,
            hunks_s: b.hunks_s,
            files,
            current: 0,
            pref: DiffLayoutPref::Auto,
            split_cols: DEFAULT_SPLIT_COLS,
            root: c.top.to_string_lossy().into_owned(),
            scroll: 0,
        }
    }

    /// How long the git view took to collect (`None` for the edit view).
    pub fn collect_time(&self) -> Option<std::time::Duration> {
        self.elapsed
    }

    pub fn row_count(&self, cols: usize) -> usize {
        self.rows(self.layout(cols, DiffLayoutPref::Auto, DEFAULT_SPLIT_COLS)).len()
    }

    /// The layout the view is drawn with: the setting (or the person's override) and the width for auto.
    pub fn set_layout(&mut self, pref: DiffLayoutPref, split_cols: usize) {
        self.pref = pref;
        self.split_cols = split_cols;
    }

    pub fn layout_pref(&self) -> DiffLayoutPref {
        self.pref
    }

    pub fn split_cols(&self) -> usize {
        self.split_cols
    }

    /// The files of the list.
    pub fn files(&self) -> &[DiffFile] {
        &self.files
    }

    /// The index of the shown file in [`DiffView::files`].
    pub fn current_file(&self) -> usize {
        self.current
    }

    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    /// Marks file `i` of the list as the shown one (the caller loaded it).
    pub fn set_current(&mut self, i: usize) {
        self.current = i.min(self.files.len().saturating_sub(1));
    }

    /// The folder the header's file names are shown relative to: the repository's for a git view.
    pub fn root(&self) -> &str {
        &self.root
    }

    pub fn set_root(&mut self, root: &str) {
        self.root = root.trim_end_matches(['/', '\\']).to_string();
    }

    /// Replaces the list of files (the conversation's or git's) and marks file `current`.
    pub fn set_files(&mut self, files: Vec<DiffFile>, current: usize) {
        self.files = files;
        self.set_current(current);
    }

    /// Marks file `i` and, when the rows hold the file titles (the working tree), scrolls to its title.
    pub fn show_file(&mut self, i: usize, layout: Layout, height: usize) {
        self.set_current(i);
        let at = self.rows(layout).iter().enumerate().filter(|(_, r)| r.kind == Kind::File).nth(self.current);
        if let Some((y, _)) = at {
            self.scroll = y;
        }
        self.scroll = self.scroll.min(self.rows(layout).len().saturating_sub(body_rows(height)));
    }

    /// Side by side when `setting` says so, or when it is auto and the view is `split_cols` wide.
    pub fn layout(&self, cols: usize, setting: DiffLayoutPref, split_cols: usize) -> Layout {
        match setting {
            DiffLayoutPref::Split => Layout::Split,
            DiffLayoutPref::Stacked => Layout::Stacked,
            DiffLayoutPref::Auto if cols >= split_cols => Layout::Split,
            DiffLayoutPref::Auto => Layout::Stacked,
        }
    }
}

/// The width from which the auto layout is side by side.
pub const DEFAULT_SPLIT_COLS: usize = 100;

fn push_changes(b: &mut Built, dels: &mut Vec<Row>, adds: &mut Vec<Row>) {
    b.unified.append(dels);
    b.unified.append(adds);
}

/// Unified and side-by-side rows of `before` → `after`, 3 lines of context.
fn diff_rows(before: &str, after: &str, b: &mut Built) {
    // Pathological inputs give up on the exact diff after this long.
    let diff = TextDiff::configure().timeout(std::time::Duration::from_millis(500)).diff_lines(before, after);
    for (gi, group) in diff.grouped_ops(3).iter().enumerate() {
        b.hunks_u.push(b.unified.len());
        b.hunks_s.push(b.split.len());
        if let (Some(f), Some(l)) = (group.first(), group.last()) {
            let (o, n) = (f.old_range().start, f.new_range().start);
            let (oe, ne) = (l.old_range().end, l.new_range().end);
            let h = Row {
                kind: Kind::Hunk,
                old: Some((0, format!("@@ -{},{} +{},{} @@", o + 1, oe - o, n + 1, ne - n))),
                new: None,
            };
            if gi > 0 || o > 0 {
                b.unified.push(h.clone());
                b.split.push(h);
            }
        }
        for op in group {
            let mut dels = vec![];
            let mut adds = vec![];
            for c in diff.iter_changes(op) {
                let text = clean(c.value());
                match c.tag() {
                    ChangeTag::Equal => {
                        push_changes(b, &mut dels, &mut adds);
                        let row = Row {
                            kind: Kind::Same,
                            old: c.old_index().map(|i| (i + 1, text.clone())),
                            new: c.new_index().map(|i| (i + 1, text)),
                        };
                        b.unified.push(row.clone());
                        b.split.push(row);
                    }
                    ChangeTag::Delete => {
                        dels.push(Row { kind: Kind::Del, old: c.old_index().map(|i| (i + 1, text)), new: None });
                    }
                    ChangeTag::Insert => {
                        adds.push(Row { kind: Kind::Add, old: None, new: c.new_index().map(|i| (i + 1, text)) });
                    }
                }
            }
            for k in 0..dels.len().max(adds.len()) {
                let (d, a) = (dels.get(k), adds.get(k));
                let kind = match (d, a) {
                    (Some(_), Some(_)) => Kind::Change,
                    (Some(_), None) => Kind::Del,
                    _ => Kind::Add,
                };
                b.split.push(Row { kind, old: d.and_then(|r| r.old.clone()), new: a.and_then(|r| r.new.clone()) });
            }
            push_changes(b, &mut dels, &mut adds);
        }
    }
}

fn body_rows(height: usize) -> usize {
    height.saturating_sub(2).max(1)
}

impl DiffView {
    fn rows(&self, layout: Layout) -> &[Row] {
        match layout {
            Layout::Split => &self.split,
            Layout::Stacked => &self.unified,
        }
    }

    fn hunks(&self, layout: Layout) -> &[usize] {
        match layout {
            Layout::Split => &self.hunks_s,
            Layout::Stacked => &self.hunks_u,
        }
    }

    /// A key while the view is open (`height`: the rectangle's rows).
    pub fn key(&mut self, key: &str, shift: bool, height: usize, layout: Layout) -> Key {
        let n = self.rows(layout).len();
        let page = body_rows(height);
        let max = n.saturating_sub(page);
        match (key, shift) {
            ("escape", _) | ("q", false) => return Key::Close,
            ("m", false) | ("tab", false) => return Key::Next,
            ("r", false) => return Key::Refresh,
            ("n", false) => return Key::NextFile,
            ("p", false) => return Key::PrevFile,
            ("v", false) => return Key::ToggleLayout,
            ("]", _) => {
                if let Some(h) = self.hunks(layout).iter().find(|h| **h > self.scroll) {
                    self.scroll = *h;
                }
            }
            ("[", _) => {
                if let Some(h) = self.hunks(layout).iter().rev().find(|h| **h < self.scroll) {
                    self.scroll = *h;
                }
            }
            ("j", false) | ("down", _) | ("enter", _) => self.scroll += 1,
            ("k", false) | ("up", _) => self.scroll = self.scroll.saturating_sub(1),
            ("space", false) | ("pagedown", _) | ("f", false) => self.scroll += page,
            ("b", false) | ("pageup", _) => self.scroll = self.scroll.saturating_sub(page),
            ("g", false) | ("home", _) => self.scroll = 0,
            ("g", true) | ("end", _) => self.scroll = max,
            _ => {}
        }
        self.scroll = self.scroll.min(max);
        Key::Stay
    }

    /// Keeps the scroll position of `old` (the same view read again).
    pub fn keep_scroll(&mut self, old: &DiffView, height: usize, layout: Layout) {
        self.scroll = old.scroll.min(self.rows(layout).len().saturating_sub(body_rows(height)));
    }

    /// Draws into `area` of `grid` and touches nothing outside it.
    pub fn draw(&self, grid: &mut [Vec<Cell>], area: Rect, g: Greys, pref: DiffLayoutPref, split_cols: usize) {
        let Some(first) = grid.get(area.y) else { return };
        let x = area.x.min(first.len());
        let cols = area.w.min(first.len() - x);
        let height = area.h.min(grid.len() - area.y);
        if cols < 10 || height < 2 {
            return;
        }
        let layout = self.layout(cols, pref, split_cols);
        let blank = Cell { fg: Some(g.normal), bg: Some(g.bg), ..Default::default() };
        let bar = g.bar;
        for row in grid.iter_mut().skip(area.y).take(height) {
            row[x..x + cols].fill(blank.clone());
        }
        let hdr = &mut grid[area.y][x..x + cols];
        hdr.fill(Cell { fg: Some(g.strong), bg: Some(bar), ..Default::default() });
        let title = match self.files.get(self.current) {
            Some(f) => format!(
                "{}/{} {}  +{} -{}  {} {}",
                self.current + 1,
                self.files.len(),
                shown(&f.path, &self.root),
                f.added,
                f.removed,
                self.mode.label(),
                layout.label()
            ),
            None => format!("{}  {}", self.header, layout.label()),
        };
        put(hdr, 1, &fit(&title, cols - 2), g.strong, bar, true);
        let foot = &mut grid[area.y + height - 1][x..x + cols];
        foot.fill(Cell { fg: Some(g.dim), bg: Some(bar), ..Default::default() });
        let keys = "j/k · space/b · g/G · n/p file · ]/[ hunk · v layout · m view · r · esc";
        put(foot, 1, &fit(keys, cols - 2), g.dim, bar, false);
        if let Some(n) = &self.note {
            put(&mut grid[area.y + 2.min(height - 2)][x..x + cols], 2, &fit(n, cols - 4), g.dim, g.bg, false);
            return;
        }
        let add_bg = mix(g.bg, g.added, 55);
        let del_bg = mix(g.bg, g.removed, 55);
        let rows = self.rows(layout);
        let split = layout == Layout::Split;
        let nw = 5; // line-number width
        for (y, r) in rows.iter().skip(self.scroll).take(height - 2).enumerate() {
            let line = &mut grid[area.y + 1 + y][x..x + cols];
            if r.kind == Kind::Hunk {
                let t = r.old.as_ref().map_or("", |(_, t)| t.as_str());
                put(line, 1, &fit(t, cols - 2), g.dim, g.bg, false);
                continue;
            }
            if r.kind == Kind::File {
                let t = r.old.as_ref().map_or("", |(_, t)| t.as_str());
                line.fill(Cell { fg: Some(g.strong), bg: Some(bar), ..Default::default() });
                put(line, 1, &fit(t, cols - 2), g.strong, bar, true);
                continue;
            }
            if split {
                let half = cols / 2;
                let lbg = if r.old.is_some() && matches!(r.kind, Kind::Del | Kind::Change) { del_bg } else { g.bg };
                let rbg = if r.new.is_some() && matches!(r.kind, Kind::Add | Kind::Change) { add_bg } else { g.bg };
                side(&mut line[..half], r.old.as_ref(), nw, lbg, g);
                put(line, half, "│", g.dim, g.bg, false);
                side(&mut line[half + 1..], r.new.as_ref(), nw, rbg, g);
            } else {
                let (mark, bg, num) = match r.kind {
                    Kind::Del => ("-", del_bg, r.old.as_ref().map(|(n, _)| *n)),
                    Kind::Add => ("+", add_bg, r.new.as_ref().map(|(n, _)| *n)),
                    _ => (" ", g.bg, r.new.as_ref().map(|(n, _)| *n)),
                };
                let text = r.new.as_ref().or(r.old.as_ref()).map_or("", |(_, t)| t.as_str());
                line.fill(Cell { fg: Some(g.normal), bg: Some(bg), ..Default::default() });
                let x = put(
                    line,
                    0,
                    &format!("{:>w$} ", num.map(|n| n.to_string()).unwrap_or_default(), w = nw),
                    g.dim,
                    bg,
                    false,
                );
                let x = put(line, x, mark, g.strong, bg, false);
                put(line, x + 1, &fit(text, cols.saturating_sub(x + 2)), g.normal, bg, false);
            }
        }
    }
}

fn side(cells: &mut [Cell], line: Option<&(usize, String)>, nw: usize, bg: Rgb, g: Greys) {
    cells.fill(Cell { fg: Some(g.normal), bg: Some(bg), ..Default::default() });
    let Some((n, text)) = line else { return };
    let w = cells.len();
    let x = put(cells, 0, &format!("{n:>nw$} "), g.dim, bg, false);
    put(cells, x, &fit(text, w.saturating_sub(x + 1)), g.normal, bg, false);
}

fn shown<'a>(path: &'a str, root: &str) -> &'a str {
    match path.strip_prefix(root) {
        Some(rest) if !root.is_empty() && rest.starts_with(['/', '\\']) => &rest[1..],
        _ => path,
    }
}

fn mix(a: Rgb, b: Rgb, t: u8) -> Rgb {
    let m = |x: u8, y: u8| ((x as u16 * (255 - t as u16) + y as u16 * t as u16) / 255) as u8;
    Rgb { r: m(a.r, b.r), g: m(a.g, b.g), b: m(a.b, b.b) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use haetty_proto::wire::{EditMeta, Secret};

    #[test]
    fn the_header_names_files_from_the_tabs_folder() {
        assert_eq!(shown("/w/proj/src/app.rs", "/w/proj"), "src/app.rs");
        assert_eq!(shown("/w/project/src/app.rs", "/w/proj"), "/w/project/src/app.rs");
        assert_eq!(shown("/other/x.rs", "/w/proj"), "/other/x.rs");
        assert_eq!(shown("src/x.rs", ""), "src/x.rs");
    }

    fn view(before: &str, after: &str) -> DiffView {
        let e = EditView {
            meta: EditMeta {
                path: "/p/a.rs".into(),
                agent: "main".into(),
                at_ms: 0,
                added: 1,
                removed: 1,
                diffable: true,
            },
            status: SnapStatus::Copied,
            before: Secret(before.into()),
            after: Secret(after.into()),
        };
        DiffView::new(Some(&e))
    }

    fn text(row: &[Cell]) -> String {
        row.iter().map(|c| if c.text.is_empty() { " ".into() } else { c.text.clone() }).collect()
    }

    fn greys() -> Greys {
        Greys::from(Rgb { r: 220, g: 220, b: 220 }, Rgb { r: 30, g: 30, b: 30 })
    }

    fn full(w: usize, h: usize) -> Rect {
        Rect { x: 0, y: 0, w, h }
    }

    fn numbered(n: usize) -> String {
        (0..n).map(|i| format!("{i}\n")).collect()
    }

    #[test]
    fn unified_and_side_by_side() {
        let v = view("a\nb\nc\n", "a\nB\tx\nc\n");
        assert_eq!(
            v.unified.iter().map(|r| r.kind).collect::<Vec<_>>(),
            [Kind::Same, Kind::Del, Kind::Add, Kind::Same]
        );
        assert_eq!(v.split.iter().map(|r| r.kind).collect::<Vec<_>>(), [Kind::Same, Kind::Change, Kind::Same]);
        let g = greys();
        let mut grid = vec![vec![Cell::default(); 80]; 7];
        v.draw(&mut grid, full(80, 7), g, DiffLayoutPref::Auto, 100);
        assert!(text(&grid[0]).contains("1/1 /p/a.rs  +1 -1  last edit stacked"), "{}", text(&grid[0]));
        assert!(text(&grid[2]).contains("    2 - b"), "{}", text(&grid[2]));
        assert!(text(&grid[3]).contains("    2 + B    x"), "{}", text(&grid[3]));
        assert!(text(&grid[6]).contains("n/p file") && text(&grid[6]).contains("esc"), "{}", text(&grid[6]));
        let mut wide = vec![vec![Cell::default(); 120]; 7];
        v.draw(&mut wide, full(120, 7), g, DiffLayoutPref::Auto, 100);
        let t = text(&wide[2]);
        assert!(t.contains("    2 b") && t.contains("│") && t.contains("    2 B"), "{t}");
        assert!(text(&wide[0]).contains("last edit split"));
        assert!(DiffView::new(None).note.is_some());
    }

    #[test]
    fn layout_switches_by_width() {
        let v = view("a\n", "b\n");
        let l = |cols, pref| v.layout(cols, pref, 100);
        assert_eq!(l(99, DiffLayoutPref::Auto), Layout::Stacked);
        assert_eq!(l(100, DiffLayoutPref::Auto), Layout::Split);
        assert_eq!(l(101, DiffLayoutPref::Auto), Layout::Split);
        for cols in [40, 99, 100, 400] {
            assert_eq!(l(cols, DiffLayoutPref::Split), Layout::Split);
            assert_eq!(l(cols, DiffLayoutPref::Stacked), Layout::Stacked);
        }
        assert_eq!(v.layout(139, DiffLayoutPref::Auto, 140), Layout::Stacked);
        assert_eq!(v.layout(140, DiffLayoutPref::Auto, 140), Layout::Split);
    }

    #[test]
    fn stacked_puts_removed_above_added() {
        let v = view("a\nb\nc\nd\ne\n", "a\nX\nY\nd\nE\n");
        let kinds: Vec<Kind> = v.unified.iter().map(|r| r.kind).collect();
        assert_eq!(kinds, [Kind::Same, Kind::Del, Kind::Del, Kind::Add, Kind::Add, Kind::Same, Kind::Del, Kind::Add]);
        let texts: Vec<&str> =
            v.unified.iter().map(|r| r.new.as_ref().or(r.old.as_ref()).unwrap().1.as_str()).collect();
        assert_eq!(texts, ["a", "b", "c", "X", "Y", "d", "e", "E"]);
    }

    #[test]
    fn scrolling_stays_in_range() {
        let before = numbered(100);
        let after = before.replace("50\n", "fifty\n");
        let mut v = view(&before, &after);
        let st = Layout::Stacked;
        assert_eq!(v.key("end", false, 5, st), Key::Stay);
        let max = v.scroll;
        assert_eq!(max, v.unified.len() - 3);
        v.key("j", false, 5, st);
        assert_eq!(v.scroll, max);
        v.key("space", false, 5, st);
        assert_eq!(v.scroll, max);
        v.key("g", false, 5, st);
        assert_eq!(v.scroll, 0);
        v.key("k", false, 5, st);
        v.key("pageup", false, 5, st);
        assert_eq!(v.scroll, 0);
        v.key("space", false, 5, st);
        assert_eq!(v.scroll, 3);
        v.key("b", false, 5, st);
        assert_eq!(v.scroll, 0);
        assert_eq!(v.key("escape", false, 5, st), Key::Close);
        assert_eq!(v.key("q", false, 5, st), Key::Close);
        assert_eq!((v.key("m", false, 5, st), v.key("tab", false, 5, st)), (Key::Next, Key::Next));
        assert_eq!(v.key("r", false, 5, st), Key::Refresh);
        assert_eq!(Mode::Edit.next().next().next(), Mode::Edit);
        let mut short = view("a\n", "b\n");
        short.key("G", true, 30, st);
        assert_eq!(short.scroll, 0);
    }

    #[test]
    fn file_and_layout_keys_are_returned() {
        let mut v = view("a\n", "b\n");
        let st = Layout::Stacked;
        assert_eq!(v.key("n", false, 10, st), Key::NextFile);
        assert_eq!(v.key("p", false, 10, st), Key::PrevFile);
        assert_eq!(v.key("v", false, 10, st), Key::ToggleLayout);
        assert_eq!(v.scroll, 0);
    }

    #[test]
    fn hunk_moves_are_exact_and_stop_at_the_ends() {
        let before = numbered(100);
        let after = before.replace("10\n", "ten\n").replace("50\n", "fifty\n").replace("90\n", "ninety\n");
        let mut v = view(&before, &after);
        let st = Layout::Stacked;
        assert_eq!(v.hunks_u.len(), 3);
        let starts = v.hunks_u.clone();
        assert_eq!(starts[0], 0);
        for h in &starts[1..] {
            assert_eq!(v.unified[*h].kind, Kind::Hunk);
        }
        v.key("]", false, 10, st);
        assert_eq!(v.scroll, starts[1]);
        v.key("]", false, 10, st);
        assert_eq!(v.scroll, starts[2].min(v.unified.len() - 8));
        let last = v.scroll;
        v.key("]", false, 10, st);
        assert_eq!(v.scroll, last);
        v.key("[", false, 10, st);
        assert_eq!(v.scroll, starts[1]);
        v.key("[", false, 10, st);
        assert_eq!(v.scroll, 0);
        v.key("[", false, 10, st);
        assert_eq!(v.scroll, 0);
        let mut s = view(&before, &after);
        s.key("]", false, 10, Layout::Split);
        assert_eq!(s.scroll, s.hunks_s[1]);
    }

    #[test]
    fn draw_stays_inside_its_rectangle() {
        let before = numbered(30);
        let after = before.replace("5\n", "five\n");
        let v = view(&before, &after);
        let sentinel = Cell { text: "#".into(), ..Default::default() };
        for (x, w, pref) in
            [(5, 30, DiffLayoutPref::Auto), (20, 110, DiffLayoutPref::Auto), (3, 60, DiffLayoutPref::Split)]
        {
            let mut grid = vec![vec![sentinel.clone(); 140]; 12];
            let area = Rect { x, y: 2, w, h: 8 };
            v.draw(&mut grid, area, greys(), pref, 100);
            for (y, row) in grid.iter().enumerate() {
                for (cx, c) in row.iter().enumerate() {
                    let inside = (2..10).contains(&y) && (x..x + w).contains(&cx);
                    if !inside {
                        assert_eq!(c, &sentinel, "({cx},{y}) outside {area:?}");
                    }
                }
            }
            assert!(grid[2][x..x + w].iter().all(|c| c != &sentinel));
            assert!(text(&grid[9][x..x + w]).contains("j/k"));
        }
        let mut small = vec![vec![sentinel.clone(); 20]; 4];
        v.draw(&mut small, Rect { x: 10, y: 1, w: 50, h: 20 }, greys(), DiffLayoutPref::Auto, 100);
        assert!(small[0].iter().all(|c| c == &sentinel));
    }

    #[test]
    fn git_views_list_files_and_reasons() {
        use crate::gitview::FileDiff;
        let f = |p: &str, b: Option<&str>, a: Option<&str>, why| FileDiff {
            path: p.into(),
            before: b.map(Into::into),
            after: a.map(Into::into),
            why,
        };
        let c = Collected {
            top: "/w/proj".into(),
            files: vec![
                f("a.rs", Some("x\n"), Some("y\n"), None),
                f(".env", None, None, Some("credential-like: never read")),
            ],
            changed: 2,
            notes: vec!["2 changed files: the first 2 are shown".into()],
            ..Default::default()
        };
        let mut v = DiffView::git(Mode::Tree, Ok(c));
        assert_eq!(v.header, "[working tree] proj · 2 file(s) · +1 -1");
        assert_eq!(v.file_count(), 2);
        assert_eq!(v.files()[0], DiffFile { path: "/w/proj/a.rs".into(), added: 1, removed: 1 }, "the full path");
        assert_eq!(v.current_file(), 0);
        v.set_current(9);
        assert_eq!(v.current_file(), 1);
        v.set_current(0);
        let kinds: Vec<Kind> = v.unified.iter().map(|r| r.kind).collect();
        assert_eq!(kinds, [Kind::Hunk, Kind::File, Kind::Del, Kind::Add, Kind::File]);
        assert_eq!(v.hunks_u, [1], "a file's first hunk starts at its title");
        let g = greys();
        let mut grid = vec![vec![Cell::default(); 80]; 9];
        v.draw(&mut grid, full(80, 9), g, DiffLayoutPref::Auto, 100);
        assert!(text(&grid[0]).starts_with(" 1/2 a.rs  +1 -1  working tree stacked"), "{}", text(&grid[0]));
        assert!(text(&grid[2]).contains("a.rs · +1 -1"), "{}", text(&grid[2]));
        assert!(text(&grid[5]).contains(".env · credential-like: never read"), "{}", text(&grid[5]));
        let none = DiffView::git(Mode::Tree, Ok(Collected { top: "/w/p".into(), ..Default::default() }));
        assert_eq!(none.note.as_deref(), Some("No changes against HEAD."));
        assert_eq!(none.file_count(), 0);
        let err = DiffView::git(Mode::Head, Err("not in a git repository".into()));
        assert_eq!(err.note.as_deref(), Some("No diff: not in a git repository."));
    }

    #[test]
    fn empty_and_binary_notes_still_draw() {
        let same = view("a\n", "a\n");
        assert_eq!(same.note.as_deref(), Some("The edit changed nothing."));
        let mut grid = vec![vec![Cell::default(); 60]; 6];
        same.draw(&mut grid, full(60, 6), greys(), DiffLayoutPref::Auto, 100);
        assert!(text(&grid[2]).contains("The edit changed nothing."));
        let mut e = EditView {
            meta: EditMeta {
                path: "/p/b.bin".into(),
                agent: "main".into(),
                at_ms: 0,
                added: 0,
                removed: 0,
                diffable: false,
            },
            status: SnapStatus::Binary,
            before: Secret(String::new()),
            after: Secret(String::new()),
        };
        let b = DiffView::new(Some(&e));
        assert_eq!(b.note.as_deref(), Some("No diff: the file is not text."));
        assert_eq!(b.file_count(), 1);
        e.status = SnapStatus::TooLarge;
        let mut g2 = vec![vec![Cell::default(); 60]; 6];
        DiffView::new(Some(&e)).draw(&mut g2, full(60, 6), greys(), DiffLayoutPref::Auto, 100);
        assert!(text(&g2[0]).contains("1/1 /p/b.bin"));
    }
}
