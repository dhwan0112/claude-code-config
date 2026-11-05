//! The diff view (DESIGN.md §3.13): the last edit Claude Code made in the
//! active tab, before against after, drawn over the tab's screen (the
//! program keeps its size and keeps running). Side by side from 110
//! columns, unified below. Hidden by default, never opens by itself.
//! Keys: j/k ↓/↑ line, Space/PageDown and b/PageUp page, g/Home and
//! G/End ends, Esc or q closes.

use haetty_cc::similar::{ChangeTag, TextDiff};
use haetty_core::{Cell, Rgb};
use haetty_proto::wire::{EditView, SnapStatus};

use crate::ui::{fit, put, width, Greys};

/// Side by side from this many columns.
pub const SPLIT_COLS: usize = 110;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Same,
    Del,
    Add,
    /// A pair: removed on the left, added on the right.
    Change,
    Hunk,
}

#[derive(Clone, Debug)]
struct Row {
    kind: Kind,
    old: Option<(usize, String)>,
    new: Option<(usize, String)>,
}

pub struct DiffView {
    header: String,
    note: Option<String>,
    unified: Vec<Row>,
    split: Vec<Row>,
    pub scroll: usize,
}

/// One line of text for a cell row: tabs as 4 spaces, no control characters.
fn clean(s: &str) -> String {
    s.trim_end_matches(['\n', '\r']).replace('\t', "    ").chars().filter(|c| !c.is_control()).collect()
}

impl DiffView {
    pub fn new(edit: Option<&EditView>) -> DiffView {
        let Some(e) = edit else {
            return DiffView {
                header: "[last edit] none yet".into(),
                note: Some("Claude Code has not edited a file in this tab yet.".into()),
                unified: vec![],
                split: vec![],
                scroll: 0,
            };
        };
        let m = &e.meta;
        let header = format!("[last edit] {} · +{} -{} · by {}", m.path, m.added, m.removed, m.agent);
        if !m.diffable {
            let why = match e.status {
                SnapStatus::TooLarge => "the file is larger than 2 MiB",
                SnapStatus::Binary => "the file is not text",
                SnapStatus::Skipped => "credential-like files are never read",
                SnapStatus::New | SnapStatus::Copied => "the edit could not be rebuilt",
                SnapStatus::Error => "the file could not be read before the edit",
            };
            return DiffView {
                header,
                note: Some(format!("No diff: {why}.")),
                unified: vec![],
                split: vec![],
                scroll: 0,
            };
        }
        let diff = TextDiff::from_lines(e.before.0.as_str(), e.after.0.as_str());
        let mut unified = vec![];
        let mut split = vec![];
        for (gi, group) in diff.grouped_ops(3).iter().enumerate() {
            let (first, last) = (group.first(), group.last());
            if let (Some(f), Some(l)) = (first, last) {
                let (o, n) = (f.old_range().start, f.new_range().start);
                let (oe, ne) = (l.old_range().end, l.new_range().end);
                let h = Row {
                    kind: Kind::Hunk,
                    old: Some((0, format!("@@ -{},{} +{},{} @@", o + 1, oe - o, n + 1, ne - n))),
                    new: None,
                };
                if gi > 0 || o > 0 {
                    unified.push(h.clone());
                    split.push(h);
                }
            }
            for op in group {
                let mut dels = vec![];
                let mut adds = vec![];
                for c in diff.iter_changes(op) {
                    let text = clean(c.value());
                    match c.tag() {
                        ChangeTag::Equal => {
                            let row = Row {
                                kind: Kind::Same,
                                old: c.old_index().map(|i| (i + 1, text.clone())),
                                new: c.new_index().map(|i| (i + 1, text)),
                            };
                            unified.push(row.clone());
                            split.push(row);
                        }
                        ChangeTag::Delete => {
                            let r = Row { kind: Kind::Del, old: c.old_index().map(|i| (i + 1, text)), new: None };
                            unified.push(r.clone());
                            dels.push(r);
                        }
                        ChangeTag::Insert => {
                            let r = Row { kind: Kind::Add, old: None, new: c.new_index().map(|i| (i + 1, text)) };
                            unified.push(r.clone());
                            adds.push(r);
                        }
                    }
                }
                // Side by side: removed and added lines of one change share rows.
                for k in 0..dels.len().max(adds.len()) {
                    let (d, a) = (dels.get(k), adds.get(k));
                    let kind = match (d, a) {
                        (Some(_), Some(_)) => Kind::Change,
                        (Some(_), None) => Kind::Del,
                        _ => Kind::Add,
                    };
                    split.push(Row { kind, old: d.and_then(|r| r.old.clone()), new: a.and_then(|r| r.new.clone()) });
                }
            }
        }
        let note = unified.is_empty().then(|| "The edit changed nothing.".to_string());
        DiffView { header, note, unified, split, scroll: 0 }
    }

    fn rows(&self, cols: usize) -> &[Row] {
        if cols >= SPLIT_COLS {
            &self.split
        } else {
            &self.unified
        }
    }

    /// A key while the view is open; `true` closes it.
    pub fn key(&mut self, key: &str, shift: bool, height: usize, cols: usize) -> bool {
        let n = self.rows(cols).len();
        let page = height.saturating_sub(2).max(1);
        let max = n.saturating_sub(page);
        match (key, shift) {
            ("escape", _) | ("q", false) => return true,
            ("j", false) | ("down", _) | ("enter", _) => self.scroll += 1,
            ("k", false) | ("up", _) => self.scroll = self.scroll.saturating_sub(1),
            ("space", false) | ("pagedown", _) | ("f", false) => self.scroll += page,
            ("b", false) | ("pageup", _) => self.scroll = self.scroll.saturating_sub(page),
            ("g", false) | ("home", _) => self.scroll = 0,
            ("g", true) | ("end", _) => self.scroll = max,
            _ => {}
        }
        self.scroll = self.scroll.min(max);
        false
    }

    /// Draws into `grid` rows `top..top + height`.
    pub fn draw(&self, grid: &mut [Vec<Cell>], top: usize, height: usize, g: Greys) {
        let cols = grid.first().map_or(0, |r| r.len());
        if cols < 10 || height < 2 {
            return;
        }
        let blank = Cell { fg: Some(g.normal), bg: Some(g.bg), ..Default::default() };
        for row in grid.iter_mut().skip(top).take(height) {
            row.fill(blank.clone());
        }
        let bar = g.bar;
        let hdr = &mut grid[top];
        hdr.fill(Cell { fg: Some(g.strong), bg: Some(bar), ..Default::default() });
        let keys = "j/k · space/b · g/G · esc";
        let kw = width(keys);
        put(hdr, 1, &fit(&self.header, cols.saturating_sub(kw + 4)), g.strong, bar, true);
        put(hdr, cols.saturating_sub(kw + 1), keys, g.dim, bar, false);
        if let Some(n) = &self.note {
            put(&mut grid[top + 2.min(height - 1)], 2, &fit(n, cols - 4), g.dim, g.bg, false);
            return;
        }
        let add_bg = mix(g.bg, Rgb { r: 60, g: 170, b: 90 }, 55);
        let del_bg = mix(g.bg, Rgb { r: 200, g: 70, b: 70 }, 55);
        let rows = self.rows(cols);
        let split = cols >= SPLIT_COLS;
        let nw = 5; // line-number width
        for (y, r) in rows.iter().skip(self.scroll).take(height - 1).enumerate() {
            let line = &mut grid[top + 1 + y];
            if r.kind == Kind::Hunk {
                let t = r.old.as_ref().map_or("", |(_, t)| t.as_str());
                put(line, 1, &fit(t, cols - 2), g.dim, g.bg, false);
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

fn mix(a: Rgb, b: Rgb, t: u8) -> Rgb {
    let m = |x: u8, y: u8| ((x as u16 * (255 - t as u16) + y as u16 * t as u16) / 255) as u8;
    Rgb { r: m(a.r, b.r), g: m(a.g, b.g), b: m(a.b, b.b) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use haetty_proto::wire::{EditMeta, Secret};

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

    #[test]
    fn unified_and_side_by_side() {
        let v = view("a\nb\nc\n", "a\nB\tx\nc\n");
        assert_eq!(
            v.unified.iter().map(|r| r.kind).collect::<Vec<_>>(),
            [Kind::Same, Kind::Del, Kind::Add, Kind::Same]
        );
        assert_eq!(v.split.iter().map(|r| r.kind).collect::<Vec<_>>(), [Kind::Same, Kind::Change, Kind::Same]);
        let g = Greys::from(Rgb { r: 220, g: 220, b: 220 }, Rgb { r: 30, g: 30, b: 30 });
        let mut grid = vec![vec![Cell::default(); 80]; 6];
        v.draw(&mut grid, 0, 6, g);
        assert!(text(&grid[0]).contains("[last edit] /p/a.rs · +1 -1 · by main"));
        assert!(text(&grid[2]).contains("    2 - b"), "{}", text(&grid[2]));
        assert!(text(&grid[3]).contains("    2 + B    x"), "{}", text(&grid[3]));
        let mut wide = vec![vec![Cell::default(); 120]; 6];
        v.draw(&mut wide, 0, 6, g);
        let t = text(&wide[2]);
        assert!(t.contains("    2 b") && t.contains("│") && t.contains("    2 B"), "{t}");
        assert!(DiffView::new(None).note.is_some());
    }

    #[test]
    fn scrolling_stays_in_range() {
        let before: String = (0..100).map(|i| format!("{i}\n")).collect();
        let after = before.replace("50\n", "fifty\n");
        let mut v = view(&before, &after);
        assert!(!v.key("end", false, 10, 80));
        let max = v.scroll;
        assert!(max > 0);
        v.key("j", false, 10, 80);
        assert_eq!(v.scroll, max);
        v.key("g", false, 10, 80);
        assert_eq!(v.scroll, 0);
        assert!(v.key("escape", false, 10, 80));
    }
}
