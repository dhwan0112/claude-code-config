//! Text selection (stage PX): ranges by session row number (`Scrollbar::dropped + offset + y`, so a range keeps
//! its text while output pushes it up), word and line extension, and the text a range copies. No window state:
//! rows come from a [`Rows`] source.

use haetty_core::{TextRow, Width};

/// A cell by session row number and column.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Point {
    pub row: u64,
    pub col: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Cell,
    Word,
    Line,
}

/// Where rows come from: the screen, else the daemon.
pub trait Rows {
    fn row(&mut self, n: u64) -> Option<TextRow>;
}

/// A selection: where the press was, where the pointer is, by what unit it grows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    pub anchor: Point,
    pub head: Point,
    pub unit: Unit,
    pub rect: bool,
    /// The anchor's own word or line (a double or triple click), kept while the head moves.
    span: (Point, Point),
}

impl Selection {
    pub fn new(at: Point, unit: Unit, rect: bool, rows: &mut dyn Rows, word_chars: &str) -> Selection {
        let span = match unit {
            Unit::Cell => (at, at),
            Unit::Word => word(at, rows, word_chars),
            Unit::Line => line(at, rows),
        };
        Selection { anchor: at, head: at, unit, rect, span }
    }

    pub fn extend(&mut self, to: Point) {
        self.head = to;
    }

    /// The first and last selected cells, both included.
    pub fn bounds(&self, rows: &mut dyn Rows, word_chars: &str) -> (Point, Point) {
        if self.rect {
            let (a, b) = (self.anchor, self.head);
            return (
                Point { row: a.row.min(b.row), col: a.col.min(b.col) },
                Point { row: a.row.max(b.row), col: a.col.max(b.col) },
            );
        }
        let head = match self.unit {
            Unit::Cell => (self.head, self.head),
            Unit::Word => word(self.head, rows, word_chars),
            Unit::Line => line(self.head, rows),
        };
        let start = self.span.0.min(head.0);
        let end = self.span.1.max(head.1);
        (wide_start(start, rows), wide_end(end, rows))
    }

    /// A press and release on one cell selects nothing.
    pub fn is_empty(&self) -> bool {
        self.unit == Unit::Cell && self.anchor == self.head
    }

    /// Rows dropped from the scrollback: the selection shrinks by them, or goes when nothing of it is left.
    pub fn drop_before(&mut self, first: u64) -> bool {
        let lowest = |s: &Selection| s.anchor.row.min(s.head.row).min(s.span.0.row);
        let highest = |s: &Selection| s.anchor.row.max(s.head.row).max(s.span.1.row);
        if highest(self) < first {
            return false;
        }
        if lowest(self) < first {
            for p in [&mut self.anchor, &mut self.head, &mut self.span.0, &mut self.span.1] {
                if p.row < first {
                    *p = Point { row: first, col: 0 };
                }
            }
        }
        true
    }

    /// Rows `first..=last` hold some of the selection.
    pub fn rows(&self) -> (u64, u64) {
        let lo = self.anchor.row.min(self.head.row).min(self.span.0.row);
        let hi = self.anchor.row.max(self.head.row).max(self.span.1.row);
        (lo, hi)
    }
}

/// Whether the cell is selected, given the selection's bounds.
pub fn contains(b: (Point, Point), rect: bool, p: Point) -> bool {
    let (s, e) = b;
    if p.row < s.row || p.row > e.row {
        return false;
    }
    if rect {
        return p.col >= s.col && p.col <= e.col;
    }
    (p.row > s.row || p.col >= s.col) && (p.row < e.row || p.col <= e.col)
}

fn cell(r: &TextRow, col: u16) -> (&str, Width) {
    r.cells.get(col as usize).map_or(("", Width::Narrow), |(t, w)| (t.as_str(), *w))
}

fn boundary(text: &str, word_chars: &str) -> Option<char> {
    match text.chars().next() {
        None => Some(' '),
        Some(c) if c.is_whitespace() || word_chars.contains(c) => Some(c),
        Some(_) => None,
    }
}

/// The word (or the run of one boundary character) at `at`, across soft wraps.
fn word(at: Point, rows: &mut dyn Rows, word_chars: &str) -> (Point, Point) {
    let Some(r) = rows.row(at.row) else { return (at, at) };
    let at = wide_start(at, rows);
    let class = boundary(cell(&r, at.col).0, word_chars);
    let same = |t: &str, w: Width| w == Width::Spacer || boundary(t, word_chars) == class;
    let mut start = at;
    while let Some(r) = rows.row(start.row) {
        if start.col > 0 {
            let (t, w) = cell(&r, start.col - 1);
            if !same(t, w) {
                break;
            }
            start.col -= 1;
            continue;
        }
        let prev = start.row.checked_sub(1).and_then(|n| rows.row(n));
        match prev {
            Some(p) if p.wrapped && !p.cells.is_empty() => {
                let last = (p.cells.len() - 1) as u16;
                let (t, w) = cell(&p, last);
                if !same(t, w) {
                    break;
                }
                start = Point { row: start.row - 1, col: last };
            }
            _ => break,
        }
    }
    let mut end = at;
    while let Some(r) = rows.row(end.row) {
        let len = r.cells.len() as u16;
        if end.col + 1 < len {
            let (t, w) = cell(&r, end.col + 1);
            if !same(t, w) {
                break;
            }
            end.col += 1;
            continue;
        }
        if !r.wrapped {
            break;
        }
        match rows.row(end.row + 1) {
            Some(n) if !n.cells.is_empty() && same(cell(&n, 0).0, cell(&n, 0).1) => {
                end = Point { row: end.row + 1, col: 0 }
            }
            _ => break,
        }
    }
    (start, end)
}

/// The logical line at `at`: its rows joined by soft wraps.
fn line(at: Point, rows: &mut dyn Rows) -> (Point, Point) {
    let mut first = at.row;
    while let Some(p) = first.checked_sub(1).and_then(|n| rows.row(n)) {
        if !p.wrapped {
            break;
        }
        first -= 1;
    }
    let mut last = at.row;
    loop {
        match rows.row(last) {
            Some(r) if r.wrapped && rows.row(last + 1).is_some() => last += 1,
            _ => break,
        }
    }
    let end_col = rows.row(last).map_or(0, |r| r.cells.len().saturating_sub(1) as u16);
    (Point { row: first, col: 0 }, Point { row: last, col: end_col })
}

fn wide_start(p: Point, rows: &mut dyn Rows) -> Point {
    match rows.row(p.row) {
        Some(r) if p.col > 0 && cell(&r, p.col).1 == Width::Spacer => Point { row: p.row, col: p.col - 1 },
        _ => p,
    }
}

fn wide_end(p: Point, rows: &mut dyn Rows) -> Point {
    match rows.row(p.row) {
        Some(r) if cell(&r, p.col).1 == Width::Wide => Point { row: p.row, col: p.col + 1 },
        _ => p,
    }
}

/// The text of the cells `from..=to` of one row: a blank cell is a space, a wide character's second cell nothing.
fn piece(r: &TextRow, from: u16, to: u16) -> String {
    let mut s = String::new();
    for (t, w) in r.cells.iter().skip(from as usize).take((to as usize + 1).saturating_sub(from as usize)) {
        match w {
            Width::Spacer => {}
            _ if t.is_empty() => s.push(' '),
            _ => s.push_str(t),
        }
    }
    s
}

/// The text a selection copies, from its rows (`rows[0]` is row `first`). Rows joined by a soft wrap join without
/// a newline; a hard line end loses its trailing blanks; a rectangle gives one line per row.
pub fn text(b: (Point, Point), rect: bool, first: u64, rows: &[TextRow]) -> String {
    let (s, e) = b;
    let mut out = String::new();
    for n in s.row..=e.row {
        let Some(r) = n.checked_sub(first).and_then(|i| rows.get(i as usize)) else { continue };
        let last_col = u16::MAX;
        let (from, to) = if rect {
            (s.col, e.col)
        } else {
            (if n == s.row { s.col } else { 0 }, if n == e.row { e.col } else { last_col })
        };
        let p = piece(r, from, to.min(r.cells.len().saturating_sub(1) as u16));
        let joined = !rect && r.wrapped && n != e.row;
        if joined {
            out.push_str(&p);
        } else {
            out.push_str(p.trim_end_matches(' '));
            if n != e.row {
                out.push('\n');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake(Vec<TextRow>, u64);

    impl Rows for Fake {
        fn row(&mut self, n: u64) -> Option<TextRow> {
            n.checked_sub(self.1).and_then(|i| self.0.get(i as usize)).cloned()
        }
    }

    fn row(s: &str, cols: usize, wrapped: bool) -> TextRow {
        let mut cells = vec![];
        for c in s.chars() {
            if unicode_wide(c) {
                cells.push((c.to_string(), Width::Wide));
                cells.push((String::new(), Width::Spacer));
            } else if c == '\u{301}' {
                if let Some((t, _)) = cells.last_mut() {
                    t.push(c);
                }
            } else if c == ' ' {
                cells.push((String::new(), Width::Narrow));
            } else {
                cells.push((c.to_string(), Width::Narrow));
            }
        }
        cells.resize(cols, (String::new(), Width::Narrow));
        TextRow { cells, wrapped }
    }

    fn unicode_wide(c: char) -> bool {
        matches!(c, '가'..='힣' | '😀')
    }

    fn p(row: u64, col: u16) -> Point {
        Point { row, col }
    }

    const W: &str = crate::config::DEFAULT_WORD_CHARS;

    fn rows() -> Fake {
        Fake(
            vec![
                row("echo hello-world (x)", 20, false),
                row("a long line that wr", 19, true),
                row("aps onto two rows  ", 19, false),
                row("", 19, false),
                row("한글 abc 😀 e\u{301}nd", 20, false),
            ],
            100,
        )
    }

    fn select(f: &mut Fake, at: Point, unit: Unit, to: Point, rect: bool) -> String {
        let mut s = Selection::new(at, unit, rect, f, W);
        s.extend(to);
        let b = s.bounds(f, W);
        let rows = f.0.clone();
        text(b, rect, f.1, &rows)
    }

    #[test]
    fn cell_ranges_give_their_text_with_wraps_joined_and_trailing_blanks_dropped() {
        let mut f = rows();
        assert_eq!(select(&mut f, p(100, 5), Unit::Cell, p(100, 9), false), "hello");
        assert_eq!(select(&mut f, p(100, 13), Unit::Cell, p(101, 2), false), "rld (x)\na l");
        assert_eq!(select(&mut f, p(101, 0), Unit::Cell, p(102, 18), false), "a long line that wraps onto two rows");
        assert_eq!(select(&mut f, p(102, 0), Unit::Cell, p(104, 1), false), "aps onto two rows\n\n한");
        assert_eq!(select(&mut f, p(101, 14), Unit::Cell, p(100, 0), false).lines().count(), 2, "backwards");
    }

    #[test]
    fn a_wide_character_is_selected_with_both_its_cells_from_either_one() {
        let mut f = rows();
        assert_eq!(select(&mut f, p(104, 1), Unit::Cell, p(104, 1), false), "한");
        assert_eq!(select(&mut f, p(104, 0), Unit::Cell, p(104, 2), false), "한글");
        assert_eq!(select(&mut f, p(104, 9), Unit::Cell, p(104, 10), false), "😀");
        assert_eq!(
            select(&mut f, p(104, 12), Unit::Cell, p(104, 14), false),
            "e\u{301}nd",
            "the mark goes with its cell"
        );
    }

    #[test]
    fn a_double_click_takes_a_word_and_a_triple_click_the_logical_line() {
        let mut f = rows();
        assert_eq!(select(&mut f, p(100, 7), Unit::Word, p(100, 7), false), "hello-world", "- is a word character");
        assert_eq!(select(&mut f, p(100, 18), Unit::Word, p(100, 18), false), "x", "( and ) end a word");
        assert_eq!(select(&mut f, p(101, 17), Unit::Word, p(101, 17), false), "wraps", "a word across a wrap");
        assert_eq!(select(&mut f, p(104, 1), Unit::Word, p(104, 1), false), "한글");
        assert_eq!(select(&mut f, p(102, 3), Unit::Line, p(102, 3), false), "a long line that wraps onto two rows");
        assert_eq!(select(&mut f, p(100, 7), Unit::Word, p(100, 19), false), "hello-world (x)", "extends by word");
        assert_eq!(select(&mut f, p(100, 0), Unit::Line, p(101, 0), false).lines().count(), 2, "extends by line");
    }

    #[test]
    fn a_rectangle_gives_one_line_per_row() {
        let mut f = rows();
        assert_eq!(select(&mut f, p(100, 0), Unit::Cell, p(102, 3), true), "echo\na lo\naps");
        assert_eq!(select(&mut f, p(102, 18), Unit::Cell, p(103, 18), true), "\n");
    }

    #[test]
    fn dropped_rows_shrink_the_selection_or_end_it() {
        let mut f = rows();
        let mut s = Selection::new(p(100, 3), Unit::Cell, false, &mut f, W);
        s.extend(p(102, 4));
        assert!(s.drop_before(101));
        assert_eq!(s.bounds(&mut f, W).0, p(101, 0));
        assert!(!s.drop_before(103), "nothing left");
    }

    #[test]
    fn contains_follows_the_stream_or_the_rectangle() {
        let b = (p(10, 5), p(12, 2));
        assert!(contains(b, false, p(10, 7)) && contains(b, false, p(11, 0)) && contains(b, false, p(12, 2)));
        assert!(!contains(b, false, p(10, 4)) && !contains(b, false, p(12, 3)));
        assert!(!contains((p(10, 2), p(12, 5)), true, p(11, 7)));
        assert!(contains((p(10, 2), p(12, 5)), true, p(11, 3)));
    }
}
