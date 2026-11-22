//! Claude Code's edit blocks found on the screen and matched to the edits the hooks reported (stage PD, DESIGN.md
//! §2.5). A block is drawn side by side only when every visible row of it matches one edit exactly.

use haetty_core::{Cell, Rgb, Snapshot, Width};

use crate::ui::{fit, mix, put, Greys};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tag {
    Ctx,
    Del,
    Ins,
}

/// One hunk as Claude Code shows it: lines with their tags, numbered from `old_start` and `new_start`.
#[derive(Clone, Debug, PartialEq)]
pub struct Hunk {
    pub old_start: u32,
    pub new_start: u32,
    pub lines: Vec<(Tag, String)>,
}

/// One edit of a Claude Code session.
#[derive(Clone, Debug, PartialEq)]
pub struct Patch {
    pub id: String,
    pub path: String,
    pub hunks: Vec<Hunk>,
}

impl Hunk {
    /// Parses `structuredPatch` lines (`" x"`, `"-x"`, `"+x"`); other lines are skipped.
    pub fn from_lines(old_start: u32, new_start: u32, lines: &[&str]) -> Hunk {
        let lines = lines
            .iter()
            .filter_map(|l| {
                let mut c = l.chars();
                let tag = match c.next()? {
                    ' ' => Tag::Ctx,
                    '-' => Tag::Del,
                    '+' => Tag::Ins,
                    _ => return None,
                };
                Some((tag, c.as_str().to_string()))
            })
            .collect();
        Hunk { old_start, new_start, lines }
    }

    fn numbered(&self) -> impl Iterator<Item = (Tag, u32, &str)> {
        let (mut o, mut n) = (self.old_start, self.new_start);
        self.lines.iter().map(move |(t, s)| {
            let num = match t {
                Tag::Del => {
                    o += 1;
                    o - 1
                }
                Tag::Ins => {
                    n += 1;
                    n - 1
                }
                Tag::Ctx => {
                    o += 1;
                    n += 1;
                    n - 1
                }
            };
            (*t, num, s.as_str())
        })
    }
}

impl Patch {
    pub fn from_wire(p: &haetty_proto::wire::WirePatch) -> Patch {
        let hunks = p
            .hunks
            .iter()
            .map(|h| {
                let lines: Vec<&str> = h.lines.iter().map(String::as_str).collect();
                Hunk::from_lines(h.old_start, h.new_start, &lines)
            })
            .collect();
        Patch { id: p.id.clone(), path: p.path.clone(), hunks }
    }
}

/// A row that looks like a line of an edit block: a right-aligned line number, a space, the mark and the text.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub y: u16,
    /// Screen rows the line takes (a long line goes on in the next rows, with the number column blank).
    pub h: u16,
    pub num: u32,
    pub tag: Tag,
    /// The text of each screen row, trailing blanks dropped.
    pub segs: Vec<String>,
    /// The column of the mark.
    pub mark: u16,
    /// The background of the mark's cell.
    pub bg: Option<Rgb>,
}

fn cell_text(s: &Snapshot, x: u16, y: u16) -> &str {
    s.cell(x, y).map_or("", |c| c.text.as_str())
}

fn blank(t: &str) -> bool {
    t.is_empty() || t == " "
}

pub fn diff_row(s: &Snapshot, y: u16) -> Option<Row> {
    let mut x = (0..s.cols).find(|&x| !blank(cell_text(s, x, y)))?;
    let start = x;
    let mut num: u32 = 0;
    while x < s.cols {
        let t = cell_text(s, x, y);
        let d = t.parse::<u32>().ok().filter(|_| t.len() == 1)?;
        num = num.checked_mul(10)?.checked_add(d)?;
        x += 1;
        if x - start > 7 {
            return None;
        }
        if !cell_text(s, x, y).chars().next().is_some_and(|c| c.is_ascii_digit()) {
            break;
        }
    }
    if x + 1 >= s.cols || !blank(cell_text(s, x, y)) {
        return None;
    }
    let mark = x + 1;
    let tag = match cell_text(s, mark, y) {
        "-" => Tag::Del,
        "+" => Tag::Ins,
        t if blank(t) => Tag::Ctx,
        _ => return None,
    };
    let bg = s.cell(mark, y).and_then(|c| c.bg);
    Some(Row { y, h: 1, num, tag, segs: vec![row_text(s, mark + 1, y)], mark, bg })
}

fn row_text(s: &Snapshot, from: u16, y: u16) -> String {
    let mut text = String::new();
    for x in from..s.cols {
        let Some(c) = s.cell(x, y) else { break };
        if c.width == Width::Spacer {
            continue;
        }
        text.push_str(if c.text.is_empty() { " " } else { &c.text });
    }
    text.trim_end().to_string()
}

fn continues(s: &Snapshot, r: &Row, y: u16) -> Option<String> {
    if (0..r.mark).any(|x| !blank(cell_text(s, x, y))) {
        return None;
    }
    let m = cell_text(s, r.mark, y);
    let same = match r.tag {
        Tag::Del => m == "-",
        Tag::Ins => m == "+",
        Tag::Ctx => blank(m),
    };
    let text = row_text(s, r.mark + 1, y);
    (same && s.cell(r.mark, y).and_then(|c| c.bg) == r.bg && !text.is_empty()).then_some(text)
}

/// Groups of consecutive diff rows with the same mark column and at least one coloured line.
pub fn candidates(s: &Snapshot) -> Vec<Vec<Row>> {
    let mut out: Vec<Vec<Row>> = vec![];
    let mut cur: Vec<Row> = vec![];
    let flush = |cur: &mut Vec<Row>, out: &mut Vec<Vec<Row>>| {
        if cur.iter().any(|r| r.tag != Tag::Ctx) {
            out.push(std::mem::take(cur));
        } else {
            cur.clear();
        }
    };
    for y in 0..s.rows {
        if let Some(last) = cur.last_mut() {
            if last.y + last.h == y {
                if let Some(t) = continues(s, last, y) {
                    last.segs.push(t);
                    last.h += 1;
                    continue;
                }
            }
        }
        let r = diff_row(s, y).filter(|r| match r.tag {
            Tag::Ctx => r.bg.is_none(),
            _ => r.bg.is_some(),
        });
        match r {
            Some(r) if cur.last().is_none_or(|l| l.mark == r.mark && l.y + l.h == r.y) => cur.push(r),
            Some(r) => {
                flush(&mut cur, &mut out);
                cur.push(r);
            }
            None => flush(&mut cur, &mut out),
        }
    }
    flush(&mut cur, &mut out);
    out
}

/// A block matched to one hunk of one patch: the visible rows are the hunk's lines from `start` on.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub rows: Vec<Row>,
    pub patch: usize,
    pub hunk: usize,
    pub start: usize,
}

fn same_text(segs: &[String], line: &str, cut: bool) -> bool {
    let line = line.trim_end();
    let mut rest = line;
    for (k, seg) in segs.iter().enumerate() {
        let seg = if k > 0 {
            rest = rest.trim_start_matches(' ');
            seg.trim_start()
        } else {
            seg.as_str()
        };
        let Some(r) = rest.strip_prefix(seg) else { return false };
        rest = r;
    }
    rest.is_empty() || cut && !segs[0].is_empty()
}

fn places(rows: &[Row], patches: &[Patch]) -> Vec<(usize, usize, usize)> {
    let mut out = vec![];
    for (p, patch) in patches.iter().enumerate() {
        for (h, hunk) in patch.hunks.iter().enumerate() {
            let lines: Vec<_> = hunk.numbered().collect();
            if rows.len() > lines.len() {
                continue;
            }
            for start in 0..=lines.len() - rows.len() {
                let last = rows.len() - 1;
                let fits = rows
                    .iter()
                    .zip(&lines[start..])
                    .enumerate()
                    .all(|(k, (r, (t, n, s)))| r.tag == *t && r.num == *n && same_text(&r.segs, s, k == last));
                if fits {
                    out.push((p, h, start));
                }
            }
        }
    }
    out
}

fn unique(rows: &[Row], patches: &[Patch]) -> Option<Found> {
    if !rows.iter().any(|r| r.tag != Tag::Ctx) {
        return None;
    }
    let pl = places(rows, patches);
    let first = *pl.first()?;
    let id = &patches[first.0].id;
    pl.iter().all(|q| patches[q.0].id == *id).then(|| Found {
        rows: rows.to_vec(),
        patch: first.0,
        hunk: first.1,
        start: first.2,
    })
}

/// The blocks on the screen that match exactly one patch. A first or last row that does not match (cut, or covered
/// by Claude Code's own label) is left out, and the rest must match.
pub fn find(s: &Snapshot, patches: &[Patch]) -> Vec<Found> {
    candidates(s)
        .into_iter()
        .filter_map(|rows| {
            let n = rows.len();
            [(0, n), (0, n - 1), (1, n), (1, n - 1)]
                .into_iter()
                .filter(|(a, b)| a < b)
                .find_map(|(a, b)| unique(&rows[a..b], patches))
        })
        .collect()
}

/// A block drawn side by side: where, the line of each side per screen row (for a copy), and the mark.
#[derive(Clone, Debug, PartialEq)]
pub struct Drawn {
    pub id: String,
    pub hunk: usize,
    pub path: String,
    pub y0: u16,
    pub y1: u16,
    pub x0: u16,
    pub x1: u16,
    /// Columns of the left side; the dim line is at `x0 + half`.
    pub half: u16,
    pub lines: Vec<(Option<String>, Option<String>)>,
    /// The `⇄` mark: column and row (a click turns the block back).
    pub mark: Option<(u16, u16)>,
    /// The header row and the columns of the file's name in it (a click opens the diff overlay).
    pub file: Option<(u16, u16, u16)>,
}

type Side = Option<(u32, String)>;

fn pairs(lines: &[(Tag, u32, &str)]) -> Vec<(Side, Side)> {
    let mut out = vec![];
    let mut k = 0;
    while k < lines.len() {
        let (t, n, s) = lines[k];
        if t == Tag::Ctx {
            out.push((Some((n, s.to_string())), Some((n, s.to_string()))));
            k += 1;
            continue;
        }
        let dels: Vec<_> = lines[k..].iter().take_while(|l| l.0 == Tag::Del).collect();
        let ins: Vec<_> = lines[k + dels.len()..].iter().take_while(|l| l.0 == Tag::Ins).collect();
        for j in 0..dels.len().max(ins.len()) {
            let side = |v: &Vec<&(Tag, u32, &str)>| v.get(j).map(|l| (l.1, l.2.to_string()));
            out.push((side(&dels), side(&ins)));
        }
        k += dels.len() + ins.len();
    }
    out
}

fn span(s: &Snapshot, f: &Found) -> Option<(u16, u16)> {
    let r = f.rows.iter().find(|r| r.tag != Tag::Ctx)?;
    let xs: Vec<u16> = (0..s.cols).filter(|&x| s.cell(x, r.y).and_then(|c| c.bg) == r.bg).collect();
    let (x0, x1) = (*xs.first()?, *xs.last()? + 1);
    (x0 < r.mark).then_some((x0, x1))
}

type Mark = Option<(u16, u16)>;
type FileName = Option<(u16, u16, u16)>;

fn header(s: &Snapshot, y0: u16, x1: u16, path: &str) -> (Mark, FileName) {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    for y in (y0.saturating_sub(3)..y0).rev() {
        let text: Vec<&str> = (0..s.cols).map(|x| cell_text(s, x, y)).collect();
        if text.iter().all(|t| blank(t)) || text.iter().any(|t| *t == "╌" || *t == "─") {
            continue;
        }
        let line: String = text.iter().map(|t| if t.is_empty() { " " } else { t }).collect();
        let file = line.find(name).map(|b| {
            let from = line[..b].chars().count() as u16;
            (y, from, from + name.chars().count() as u16)
        });
        let mark = (x1 > 0 && blank(text[x1 as usize - 1])).then_some((x1 - 1, y));
        return (mark, file);
    }
    (None, None)
}

/// Where and what to draw for a matched block; `None` when it does not fit in the rows the block took.
pub fn layout(s: &Snapshot, f: &Found, p: &Patch) -> Option<Drawn> {
    let hunk = p.hunks.get(f.hunk)?;
    let lines: Vec<_> = hunk.numbered().skip(f.start).take(f.rows.len()).collect();
    let last = f.rows.last()?;
    let (y0, y1) = (f.rows[0].y, last.y + last.h);
    let (x0, x1) = span(s, f)?;
    let rows = pairs(&lines);
    if rows.len() > (y1 - y0) as usize || x1 - x0 < 20 {
        return None;
    }
    let mut out: Vec<(Option<String>, Option<String>)> =
        rows.into_iter().map(|(a, b)| (a.map(|v| v.1), b.map(|v| v.1))).collect();
    out.resize((y1 - y0) as usize, (None, None));
    let (mark, file) = header(s, y0, x1, &p.path);
    Some(Drawn {
        id: p.id.clone(),
        hunk: f.hunk,
        path: p.path.clone(),
        y0,
        y1,
        x0,
        x1,
        half: (x1 - x0 - 1) / 2,
        lines: out,
        mark,
        file,
    })
}

fn side(
    cells: &mut [Cell],
    line: Option<(u32, &str)>,
    nw: usize,
    tint: (Rgb, Rgb),
    words: &[(usize, usize)],
    g: Greys,
) {
    let w = cells.len();
    let Some((n, text)) = line else { return };
    let x = put(cells, 0, &format!(" {n:>nw$} "), g.dim, tint.0, false);
    let shown = fit(text, w.saturating_sub(x));
    let mut cx = x;
    for (i, ch) in shown.chars().enumerate() {
        let deep =
            words.iter().any(|&(a, b)| i >= a && i < b) && !(ch == '…' && shown.chars().count() < text.chars().count());
        cx = put(cells, cx, &ch.to_string(), g.normal, if deep { tint.1 } else { tint.0 }, false);
    }
    for c in &mut cells[x..] {
        if c.bg.is_none() {
            c.bg = Some(tint.0);
        }
    }
}

/// Draws a block side by side into a frame whose program screen starts at row `dy`, inside the block's rows and
/// columns only.
pub fn paint(s: &mut Snapshot, dy: u16, d: &Drawn, f: &Found, p: &Patch, g: Greys) {
    let Some(hunk) = p.hunks.get(d.hunk) else { return };
    let lines: Vec<_> = hunk.numbered().skip(f.start).take(f.rows.len()).collect();
    let nums = lines.iter().map(|l| l.1).max().unwrap_or(0);
    let nw = nums.to_string().len();
    let del = (mix(g.bg, g.removed, 55), mix(g.bg, g.removed, 120));
    let add = (mix(g.bg, g.added, 55), mix(g.bg, g.added, 120));
    let plain = (g.bg, g.bg);
    let cols = s.cols as usize;
    if d.x1 as usize > cols || (d.y1 + dy) as usize > s.rows as usize {
        return;
    }
    for (k, (a, b)) in pairs(&lines).into_iter().chain(std::iter::repeat((None, None))).take(d.lines.len()).enumerate()
    {
        let y = (d.y0 + dy) as usize + k;
        let Some(row) = s.cells.get_mut(y * cols..(y + 1) * cols) else { return };
        let area = &mut row[d.x0 as usize..d.x1 as usize];
        area.fill(Cell::default());
        if a.is_none() && b.is_none() {
            continue;
        }
        let ctx = a.as_ref().zip(b.as_ref()).is_some_and(|(x, y)| x.1 == y.1);
        let (wa, wb) = match (&a, &b) {
            (Some(x), Some(y)) if !ctx => haetty_cc::patch::changed_words(&x.1, &y.1),
            _ => (vec![], vec![]),
        };
        let half = d.half as usize;
        let (left, rest) = area.split_at_mut(half);
        side(left, a.as_ref().map(|v| (v.0, v.1.as_str())), nw, if ctx { plain } else { del }, &wa, g);
        put(rest, 0, "│", g.dim, g.bg, false);
        side(&mut rest[1..], b.as_ref().map(|v| (v.0, v.1.as_str())), nw, if ctx { plain } else { add }, &wb, g);
    }
    if let Some((x, y)) = d.mark {
        if let Some(c) = s.cells.get_mut((y + dy) as usize * cols + x as usize) {
            *c = Cell { text: "⇄".into(), fg: Some(g.dim), bg: c.bg, ..Default::default() };
        }
    }
}

#[cfg(test)]
mod tests_support {
    pub use super::*;
    use haetty_cc::json::Json;
    use haetty_core::vt::{new_engine, EngineKind, VtEngine, VtInit};
    use haetty_core::{ColorSchemePref, GridSize, TermColors};
    use std::path::{Path, PathBuf};

    fn data() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/inline-diff")
    }

    pub struct Rec {
        pub cols: u16,
        pub rows: u16,
        pub chunks: Vec<Vec<u8>>,
        /// The output paused at least 50 ms after this chunk (a whole frame is on the screen).
        pub settled: Vec<bool>,
        pub post: Vec<Patch>,
        pub pre: Vec<Patch>,
    }

    fn patch_of_post(j: &Json) -> Option<Patch> {
        let id = j.get("tool_use_id")?.str()?.to_string();
        let input = j.get("tool_input")?;
        let path = haetty_cc::edit::target(input)?.to_string();
        let r = j.get("tool_response")?;
        let hunks = r
            .get("structuredPatch")
            .and_then(Json::arr)?
            .iter()
            .filter_map(|h| {
                let lines: Vec<&str> = h.get("lines")?.arr()?.iter().filter_map(Json::str).collect();
                Some(Hunk::from_lines(h.get("oldStart")?.num()? as u32, h.get("newStart")?.num()? as u32, &lines))
            })
            .collect();
        Some(Patch { id, path, hunks })
    }

    fn patch_of_pre(pre: &Json, post: &Json) -> Option<Patch> {
        let id = pre.get("tool_use_id")?.str()?.to_string();
        let tool = pre.get("tool_name")?.str()?;
        let input = pre.get("tool_input")?;
        let path = haetty_cc::edit::target(input)?.to_string();
        let before = post.at(&["tool_response", "originalFile"]).and_then(Json::str).unwrap_or("");
        let after = haetty_cc::edit::apply_edit(tool, input, Some(before))?;
        let hunks = haetty_cc::patch::hunks_of(before, &after);
        Some(Patch::from_wire(&haetty_proto::wire::WirePatch { id, path, done: false, hunks }))
    }

    pub fn load(name: &str) -> Rec {
        let d = data().join(name);
        let meta = std::fs::read_to_string(d.join("meta")).unwrap();
        let val =
            |k: &str| meta.lines().find_map(|l| l.strip_prefix(k).map(|v| v.trim().parse::<u16>().unwrap())).unwrap();
        let raw = std::fs::read(d.join("screen.raw")).unwrap();
        let mut at = raw.iter().position(|&b| b == b'\n').unwrap() + 1;
        let mut chunks = vec![];
        let mut delays = vec![];
        for l in std::fs::read_to_string(d.join("timing")).unwrap().lines() {
            let mut w = l.split_whitespace();
            delays.push(w.next().unwrap().parse::<f64>().unwrap());
            let n: usize = w.next().unwrap().parse().unwrap();
            chunks.push(raw[at..(at + n).min(raw.len())].to_vec());
            at += n;
        }
        let settled = (0..chunks.len()).map(|i| delays.get(i + 1).is_none_or(|d| *d >= 0.05)).collect();
        let hooks: Vec<Json> =
            std::fs::read_to_string(d.join("hooks.jsonl")).unwrap().lines().map(|l| Json::parse(l).unwrap()).collect();
        let ev = |j: &Json, e: &str| j.get("hook_event_name").and_then(Json::str) == Some(e);
        let posts: Vec<&Json> = hooks.iter().filter(|j| ev(j, "PostToolUse")).collect();
        let post = posts.iter().filter_map(|j| patch_of_post(j)).collect();
        let pre = hooks
            .iter()
            .filter(|j| ev(j, "PreToolUse"))
            .filter_map(|p| {
                let id = p.get("tool_use_id")?.str()?;
                let q = posts.iter().find(|j| j.get("tool_use_id").and_then(Json::str) == Some(id))?;
                patch_of_pre(p, q)
            })
            .collect();
        Rec { cols: val("cols"), rows: val("rows"), chunks, settled, post, pre }
    }

    pub fn engine(cols: u16, rows: u16) -> Box<dyn VtEngine> {
        engine_of(EngineKind::default_kind(), cols, rows)
    }

    pub fn engine_of(kind: EngineKind, cols: u16, rows: u16) -> Box<dyn VtEngine> {
        let init = VtInit {
            size: GridSize::new(cols, rows),
            scrollback_limit: 64 << 20,
            colors: TermColors {
                default_fg: Rgb { r: 220, g: 220, b: 220 },
                default_bg: Rgb { r: 20, g: 20, b: 20 },
                palette: None,
                cursor_color: None,
            },
            color_scheme: ColorSchemePref::Dark,
            xtversion: "haetty test".into(),
        };
        new_engine(kind, &init).unwrap().0
    }

    #[test]
    #[ignore]
    fn pre_against_post() {
        let rec = load("run1");
        for q in &rec.post {
            let Some(p) = rec.pre.iter().find(|p| p.id == q.id) else { continue };
            if p.hunks != q.hunks {
                eprintln!(
                    "{} {}: pre {} hunks, post {} hunks",
                    q.id,
                    q.path.rsplit('/').next().unwrap(),
                    p.hunks.len(),
                    q.hunks.len()
                );
                for (a, b) in p.hunks.iter().zip(&q.hunks) {
                    if a != b {
                        eprintln!(
                            "  pre  {} {} {:?}\n  post {} {} {:?}",
                            a.old_start, a.new_start, a.lines, b.old_start, b.new_start, b.lines
                        );
                        break;
                    }
                }
            }
        }
    }

    #[test]
    #[ignore]
    fn replay_report() {
        for name in ["run1", "run2"] {
            let rec = load(name);
            let mut e = engine(rec.cols, rec.rows);
            let mut all = rec.post.clone();
            all.extend(rec.pre.iter().cloned());
            let mut seen: std::collections::BTreeMap<String, (usize, bool)> = Default::default();
            for (c, ok) in rec.chunks.iter().zip(&rec.settled) {
                e.feed(c);
                if !ok {
                    continue;
                }
                let s = e.snapshot();
                let found = find(&s, &all);
                for rows in candidates(&s) {
                    let key = format!("{:?}", rows.iter().map(|r| (r.tag, r.num, r.segs.concat())).collect::<Vec<_>>());
                    let ok = found.iter().any(|f| f.rows.first().is_some_and(|r| rows.contains(r)));
                    let ent = seen.entry(key).or_insert((0, false));
                    ent.0 += 1;
                    ent.1 |= ok;
                }
            }
            let ok = seen.values().filter(|v| v.1).count();
            eprintln!("{name}: {} candidate shapes, {ok} matched", seen.len());
            for (label, set) in [("post only", &rec.post), ("pre only", &rec.pre)] {
                let mut e = engine(rec.cols, rec.rows);
                let mut ids = std::collections::BTreeSet::new();
                let mut shapes = std::collections::BTreeSet::new();
                for (c, ok) in rec.chunks.iter().zip(&rec.settled) {
                    e.feed(c);
                    if *ok {
                        for f in find(&e.snapshot(), set) {
                            ids.insert(set[f.patch].id.clone());
                            shapes.insert(format!(
                                "{:?}",
                                f.rows.iter().map(|r| (r.num, r.segs.concat())).collect::<Vec<_>>()
                            ));
                        }
                    }
                }
                eprintln!("  {label}: {} shapes, {} distinct edits of {}", shapes.len(), ids.len(), set.len());
            }
            {
                let mut e = engine(rec.cols, rec.rows);
                let mut views: std::collections::BTreeMap<(String, u16), bool> = Default::default();
                for (c, ok) in rec.chunks.iter().zip(&rec.settled) {
                    e.feed(c);
                    if !*ok {
                        continue;
                    }
                    let s = e.snapshot();
                    let found = find(&s, &all);
                    for rows in candidates(&s) {
                        let hit = found.iter().find(|f| f.rows.first().is_some_and(|r| rows.contains(r)));
                        let id = match hit {
                            Some(f) => all[f.patch].id.clone(),
                            None => {
                                let k = rows.len().saturating_sub(1).max(1);
                                match unique(&rows[..k], &all) {
                                    Some(f) => all[f.patch].id.clone(),
                                    None => format!("?{}", rows[0].num),
                                }
                            }
                        };
                        let v = views.entry((id, rows[0].mark)).or_insert(false);
                        *v |= hit.is_some();
                    }
                }
                let marks: std::collections::BTreeSet<u16> = views.keys().map(|k| k.1).collect();
                eprintln!(
                    "  blocks (edit, view by mark column {marks:?}): {}, matched {}",
                    views.len(),
                    views.values().filter(|v| **v).count()
                );
            }
            for (k, v) in &seen {
                if !v.1 {
                    eprintln!("  unmatched: {k}");
                }
            }
            let mut e = engine(rec.cols, rec.rows);
            let mut shown = std::collections::BTreeSet::new();
            for (c, ok) in rec.chunks.iter().zip(&rec.settled) {
                e.feed(c);
                if !ok {
                    continue;
                }
                let s = e.snapshot();
                for rows in candidates(&s) {
                    if !places(&rows, &all).is_empty()
                        || !shown.insert(format!("{}:{}", rows[0].num, rows[0].segs.concat()))
                        || shown.len() > 12
                    {
                        continue;
                    }
                    let mut best = (0, String::new());
                    for patch in &all {
                        for hunk in &patch.hunks {
                            let lines: Vec<_> = hunk.numbered().collect();
                            for st in 0..lines.len() {
                                let mut k = 0;
                                while k < rows.len() && st + k < lines.len() {
                                    let (t, n, l) = lines[st + k];
                                    let r = &rows[k];
                                    if !(r.tag == t && r.num == n && same_text(&r.segs, l, false)) {
                                        break;
                                    }
                                    k += 1;
                                }
                                if k > best.0 || best.1.is_empty() {
                                    let why = match (rows.get(k), lines.get(st + k)) {
                                        (Some(r), Some((t, n, l))) => format!(
                                            "row {k}: screen {:?} {} {:?} | hunk {:?} {} {:?}",
                                            r.tag, r.num, r.segs, t, n, l
                                        ),
                                        (Some(r), None) => {
                                            format!("row {k}: screen {:?} {} {:?} | hunk ended", r.tag, r.num, r.segs)
                                        }
                                        _ => "?".into(),
                                    };
                                    if k >= best.0 {
                                        best = (k, why);
                                    }
                                }
                            }
                        }
                    }
                    eprintln!("  [{} rows] best {}: {}", rows.len(), best.0, best.1);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::tests_support::*;
    use haetty_core::vt::EngineKind;
    use haetty_core::{Cell, Rgb, Snapshot};

    const DEL: Rgb = Rgb { r: 61, g: 1, b: 0 };
    const INS: Rgb = Rgb { r: 2, g: 40, b: 0 };

    /// A screen with Claude Code's block shape: `(mark, number, text)` rows from row 1 under a header.
    fn block_screen(rows: &[(char, u32, &str)]) -> Snapshot {
        let (cols, h) = (120u16, 12u16);
        let mut cells = vec![Cell::default(); cols as usize * h as usize];
        let mut put = |y: u16, x: u16, t: &str, bg: Option<Rgb>| {
            for (k, ch) in t.chars().enumerate() {
                let c = &mut cells[y as usize * cols as usize + x as usize + k];
                c.text = ch.to_string();
                c.bg = bg;
            }
        };
        put(0, 2, "⎿  Added 1 line, removed 1 line", None);
        for (k, (m, n, t)) in rows.iter().enumerate() {
            let y = k as u16 + 1;
            let bg = match m {
                '-' => Some(DEL),
                '+' => Some(INS),
                _ => None,
            };
            if bg.is_some() {
                put(y, 6, &" ".repeat(108), bg);
            }
            put(y, 6, &format!("{n:>3} {m}{t}"), bg);
        }
        Snapshot { cols, rows: h, cells, ..Default::default() }
    }

    fn patch(id: &str, lines: &[&str]) -> Patch {
        Patch { id: id.into(), path: "/w/src/calc.py".into(), hunks: vec![Hunk::from_lines(5, 5, lines)] }
    }

    const LINES: &[&str] =
        &[" SCALE = 10", " ", "-def add(a, b):", "+def add_numbers(a, b):", "     # add two numbers"];

    fn shown() -> Snapshot {
        block_screen(&[
            (' ', 5, "SCALE = 10"),
            (' ', 6, ""),
            ('-', 7, "def add(a, b):"),
            ('+', 7, "def add_numbers(a, b):"),
            (' ', 8, "    # add two numbers"),
        ])
    }

    #[test]
    fn a_block_that_matches_one_edit_is_found_and_drawn_inside_its_rows() {
        let s = shown();
        let p = vec![patch("t1", LINES)];
        let f = find(&s, &p);
        assert_eq!(f.len(), 1);
        let d = layout(&s, &f[0], &p[0]).unwrap();
        assert_eq!((d.y0, d.y1, d.x0, d.x1), (1, 6, 6, 114));
        assert_eq!(d.lines[2], (Some("def add(a, b):".into()), Some("def add_numbers(a, b):".into())));
        assert_eq!(d.mark, Some((113, 0)), "the mark on the header row");
        let mut out = s.clone();
        let g = crate::ui::Greys::from(Rgb { r: 220, g: 220, b: 220 }, Rgb { r: 20, g: 20, b: 20 });
        paint(&mut out, 0, &d, &f[0], &p[0], g);
        for y in 0..s.rows {
            for x in 0..s.cols {
                let inside = (d.y0..d.y1).contains(&y) && (d.x0..d.x1).contains(&x) || d.mark == Some((x, y));
                if !inside {
                    assert_eq!(out.cell(x, y), s.cell(x, y), "({x}, {y}) is outside the block");
                }
            }
        }
        let row = out.row_text(3);
        let (l, r) = row.split_once('│').unwrap();
        assert_eq!((l.trim(), r.trim()), ("7 def add(a, b):", "7 def add_numbers(a, b):"));
        assert_eq!(out.cell(d.x0 + d.half, 1).unwrap().text, "│", "the dim line in the middle");
    }

    #[test]
    fn a_layout_wider_or_taller_than_the_frame_is_not_painted() {
        let s = shown();
        let p = vec![patch("t1", LINES)];
        let f = find(&s, &p);
        let d = layout(&s, &f[0], &p[0]).unwrap();
        let g = crate::ui::Greys::from(Rgb { r: 220, g: 220, b: 220 }, Rgb { r: 20, g: 20, b: 20 });
        let narrow = Snapshot {
            cols: 100,
            rows: s.rows,
            cells: vec![Cell::default(); 100 * s.rows as usize],
            ..Default::default()
        };
        let mut out = narrow.clone();
        paint(&mut out, 0, &d, &f[0], &p[0], g);
        assert_eq!(out.cells, narrow.cells, "the window got narrower before the screen did: nothing painted, no panic");
        let mut out = s.clone();
        paint(&mut out, s.rows, &d, &f[0], &p[0], g);
        assert_eq!(out.cells, s.cells, "below the frame: nothing painted");
    }

    #[test]
    fn one_character_off_or_two_matching_edits_leave_the_block_alone() {
        let s = shown();
        let mut off = LINES.to_vec();
        off[3] = "+def add_number(a, b):";
        assert!(find(&s, &[patch("t1", &off)]).is_empty(), "one character different");
        assert!(find(&s, &[patch("t1", LINES), patch("t2", LINES)]).is_empty(), "two edits with the same content");
        assert_eq!(find(&s, &[patch("t1", LINES), patch("t1", LINES)]).len(), 1, "one edit seen twice (pre and post)");
    }

    #[test]
    fn a_block_cut_at_the_top_matches_with_its_visible_part() {
        let s = block_screen(&[
            ('-', 7, "def add(a, b):"),
            ('+', 7, "def add_numbers(a, b):"),
            (' ', 8, "    # add two numbers"),
        ]);
        let f = find(&s, &[patch("t1", LINES)]);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].start, 2);
    }

    #[test]
    fn without_an_edit_record_nothing_is_touched_until_the_hook_arrives() {
        let s = shown();
        assert!(find(&s, &[]).is_empty(), "a shell tab or a hook not arrived yet");
        assert_eq!(find(&s, &[patch("t1", LINES)]).len(), 1, "the hook arrived: the next frame redraws");
    }

    #[test]
    fn the_recorded_real_runs_match_on_both_engines() {
        let mut kinds = vec![EngineKind::Native];
        if cfg!(feature = "ghostty") {
            kinds.push(EngineKind::Ghostty);
        }
        for kind in kinds {
            for (name, want) in [("run1", (62, 62)), ("run2", (41, 39))] {
                let rec = load(name);
                let mut all = rec.post.clone();
                all.extend(rec.pre.iter().cloned());
                let mut e = engine_of(kind, rec.cols, rec.rows);
                let mut shapes: std::collections::BTreeMap<String, bool> = Default::default();
                for (c, ok) in rec.chunks.iter().zip(&rec.settled) {
                    e.feed(c);
                    if !*ok {
                        continue;
                    }
                    let s = e.snapshot();
                    let found = find(&s, &all);
                    for f in &found {
                        let d = layout(&s, f, &all[f.patch]);
                        assert!(d.is_some(), "{name} {kind:?}: a matched block fits in its rows");
                    }
                    for rows in candidates(&s) {
                        let key =
                            format!("{:?}", rows.iter().map(|r| (r.tag, r.num, r.segs.concat())).collect::<Vec<_>>());
                        let hit = found.iter().any(|f| f.rows.first().is_some_and(|r| rows.contains(r)));
                        *shapes.entry(key).or_default() |= hit;
                    }
                }
                let got = (shapes.len(), shapes.values().filter(|v| **v).count());
                assert_eq!(got, want, "{name} {kind:?}: candidate shapes and matched ones");
            }
        }
    }
}

#[cfg(test)]
mod timing {
    use super::tests_support::*;

    #[test]
    #[ignore]
    fn time_per_frame() {
        let rec = load("run1");
        let mut all = rec.post.clone();
        all.extend(rec.pre.iter().cloned());
        let mut e = engine(rec.cols, rec.rows);
        let g = crate::ui::Greys::from(
            haetty_core::Rgb { r: 220, g: 220, b: 220 },
            haetty_core::Rgb { r: 20, g: 20, b: 20 },
        );
        let (mut t_find, mut t_paint, mut n) = (0u128, 0u128, 0u32);
        for (c, ok) in rec.chunks.iter().zip(&rec.settled) {
            e.feed(c);
            if !*ok {
                continue;
            }
            let s = e.snapshot();
            let t0 = std::time::Instant::now();
            let found: Vec<_> =
                find(&s, &all).into_iter().filter_map(|f| layout(&s, &f, &all[f.patch]).map(|d| (f, d))).collect();
            let t1 = std::time::Instant::now();
            let mut out = s.clone();
            for (f, d) in &found {
                paint(&mut out, 0, d, f, &all[f.patch], g);
            }
            t_paint += t1.elapsed().as_micros();
            t_find += (t1 - t0).as_micros();
            n += 1;
        }
        eprintln!(
            "{n} frames: find+layout {} us, clone+paint {} us on average",
            t_find / n as u128,
            t_paint / n as u128
        );
    }
}
