//! vtdiff: the same bytes into libghostty-vt (the reference) and haetty's own
//! engine, compared cell by cell (DESIGN.md §10.5).
//!
//!   cargo run -p haetty-vtdiff --release -- [--corpus DIR] [--fuzz N] [--chunk BYTES] [--report FILE]
//!
//! Inputs: the recorded corpus (every file in DIR), synthetic load, vttest-like
//! scenarios aimed at hidden state, scenarios with resizes, and N seeded fuzz
//! streams. Each input runs twice: cut at fixed chunk boundaries, and cut at
//! random points (a parser state at a chunk edge). After every chunk both
//! engines are fed; they are compared every `--every` bytes, after each resize
//! and at the end: grid size, every cell (text, width, fg, bg, flags), cursor,
//! modes, title, pwd, scrollback rows, and the reply bytes. The first
//! difference of an input is printed with the input offset, row, column, field,
//! both values and the last bytes fed. The summary gives the match rate per
//! field and per input category; exit status 1 when anything differs.
//!
//! Development aids:
//!   --probe 'BYTES' [COLS ROWS]  both engines' state after BYTES (`\e`, `\xNN`, `\\`), and what differs
//!   --bisect NAME                the named input byte by byte: the first byte after which they differ,
//!                                and a minimal input that still differs (delta debugging)
//!   --bisect-dump NAME           the shortest prefix of the named input whose dumps differ, minimised
//!   --diverge NAME               where the named input's engines first part, hidden state included (their
//!                                answers to state queries before each ESC), minimised under that
//!   --gen-width FILE             writes `haetty-vt`'s width tables from libghostty-vt
//!   --gen-keys FILE              writes haetty-core's key and mouse types (`keys.rs`) from libghostty-vt
//!   --encoders [--report FILE]   key, mouse, paste and focus encoding of both engines, compared
//!   --refresh                    the default engine's render-state refresh modes: stale rows, rows copied, time
//!   --colors                     colour and palette-entry parsing (themes, OSC) against libghostty-vt's
//!   --dump NAME [BYTES]          the named input's bytes (escaped), resizes marked
//!
//! At the end of every run the screen dumps (`dump`: plain, VT, HTML) of both
//! engines are compared too. The report counts identical dumps per format, for
//! all runs and for runs whose screens were identical throughout; the FAIL
//! accounting does not count them.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use haetty_core::ghostty::GhosttyEngine;
use haetty_core::native::NativeEngine;
use haetty_core::vt::{VtEngine, VtInit};
use haetty_core::{ColorSchemePref, DumpFormat, GridSize, Rgb, ScrollTo, Snapshot, TermColors};
use haetty_vt::Compat;

/// The fuzz test's generators (grammar, Claude-Code-like stream, mutations).
#[path = "../../haetty-core/tests/common/vtgen.rs"]
mod vtgen;

enum Step {
    Bytes(Vec<u8>),
    Resize(u16, u16),
}

struct Input {
    name: String,
    category: String,
    steps: Vec<Step>,
}

/// The fields compared, in report order.
const FIELDS: &[&str] = &[
    "size",
    "text",
    "width",
    "fg",
    "bg",
    "flags",
    "underline",
    "link",
    "cursor",
    "modes",
    "title",
    "pwd",
    "scrollback",
    "replies",
];

/// A cell's underline as compared: its kind (when there is one) and colour.
fn underline_of(c: &haetty_core::Cell) -> (Option<haetty_core::Underline>, Option<Rgb>) {
    (c.flags.has(haetty_core::CellFlags::UNDERLINE).then_some(c.underline), c.underline_color)
}

struct Diff {
    field: &'static str,
    at: String,
    reference: String,
    native: String,
}

#[derive(Default)]
struct Tally {
    checkpoints: u64,
    /// Per field: checkpoints where it matched.
    equal: BTreeMap<&'static str, u64>,
    inputs: u64,
    inputs_equal: u64,
    /// Runs whose scrollback contents were compared at the end, and equal.
    history: u64,
    history_equal: u64,
}

fn init(cols: u16, rows: u16) -> VtInit {
    VtInit {
        size: GridSize::new(cols, rows).with_cell(9, 18),
        // Large enough that neither engine drops rows: their byte accounting
        // differs on purpose (§10.3); eviction is tested on its own.
        scrollback_limit: 256 << 20,
        colors: TermColors {
            default_fg: Rgb { r: 0xcc, g: 0xcc, b: 0xcc },
            default_bg: Rgb { r: 0x14, g: 0x14, b: 0x14 },
            palette: None,
            cursor_color: None,
        },
        color_scheme: ColorSchemePref::Dark,
        xtversion: "haetty(0.2.0)".into(),
    }
}

/// Compares the two engines' state; every field that differs, with its first
/// place.
fn compare(a: &Snapshot, b: &Snapshot, sa: u64, sb: u64, ra: &[u8], rb: &[u8]) -> Vec<Diff> {
    let mut d = Vec::new();
    let mut push = |field: &'static str, at: String, x: String, y: String| {
        if !d.iter().any(|e: &Diff| e.field == field) {
            d.push(Diff { field, at, reference: x, native: y });
        }
    };
    if (a.cols, a.rows) != (b.cols, b.rows) {
        push("size", String::new(), format!("{}x{}", a.cols, a.rows), format!("{}x{}", b.cols, b.rows));
    } else {
        for y in 0..a.rows {
            for x in 0..a.cols {
                let (ca, cb) = (a.cell(x, y), b.cell(x, y));
                let (Some(ca), Some(cb)) = (ca, cb) else { continue };
                let at = format!("row {y} col {x}");
                if ca.text != cb.text {
                    push("text", at.clone(), format!("{:?}", ca.text), format!("{:?}", cb.text));
                }
                if ca.width != cb.width {
                    push("width", at.clone(), format!("{:?}", ca.width), format!("{:?}", cb.width));
                }
                if ca.fg != cb.fg {
                    push("fg", at.clone(), format!("{:?}", ca.fg), format!("{:?}", cb.fg));
                }
                if ca.bg != cb.bg {
                    push("bg", at.clone(), format!("{:?}", ca.bg), format!("{:?}", cb.bg));
                }
                if ca.flags != cb.flags {
                    push("flags", at.clone(), format!("{:#x}", ca.flags.0), format!("{:#x}", cb.flags.0));
                }
                if underline_of(ca) != underline_of(cb) {
                    push("underline", at.clone(), format!("{:?}", underline_of(ca)), format!("{:?}", underline_of(cb)));
                }
                if ca.link != cb.link {
                    push("link", at, format!("{:?}", ca.link), format!("{:?}", cb.link));
                }
            }
        }
    }
    if a.cursor != b.cursor {
        push("cursor", String::new(), format!("{:?}", a.cursor), format!("{:?}", b.cursor));
    }
    if a.modes != b.modes {
        push("modes", String::new(), format!("{:?}", a.modes), format!("{:?}", b.modes));
    }
    if a.title != b.title {
        push("title", String::new(), format!("{:?}", a.title), format!("{:?}", b.title));
    }
    if a.pwd != b.pwd {
        push("pwd", String::new(), format!("{:?}", a.pwd), format!("{:?}", b.pwd));
    }
    if sa != sb {
        push("scrollback", String::new(), sa.to_string(), sb.to_string());
    }
    if ra != rb {
        push("replies", String::new(), show(ra), show(rb));
    }
    d
}

/// Bytes as text with escapes visible (`\e`, `\xNN`).
fn show(b: &[u8]) -> String {
    let mut s = String::new();
    for &c in b.iter().take(160) {
        match c {
            0x1b => s.push_str("\\e"),
            b'\\' => s.push_str("\\\\"),
            b'\n' => s.push_str("\\n"),
            b'\r' => s.push_str("\\r"),
            0x20..=0x7e => s.push(c as char),
            _ => {
                let _ = write!(s, "\\x{c:02x}");
            }
        }
    }
    if b.len() > 160 {
        s.push('…');
    }
    s
}

/// A small deterministic generator (xorshift64*).
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

struct Run<'a> {
    reference: GhosttyEngine,
    native: NativeEngine,
    ra: Vec<u8>,
    rb: Vec<u8>,
    fed: Vec<u8>,
    first: Option<(u64, Diff, String)>,
    /// Every field that differed somewhere in the run.
    fields: BTreeSet<&'static str>,
    tally: &'a mut Tally,
    by_category: &'a mut Tally,
    all_equal: bool,
}

impl Run<'_> {
    /// At the end of a run: the scrollback contents, page by page from the
    /// top (the counts are compared at every checkpoint).
    fn history(&mut self) {
        let (ka, kb) = (self.reference.scrollback_rows(), self.native.scrollback_rows());
        if ka != kb {
            return;
        }
        for t in [&mut *self.tally, &mut *self.by_category] {
            t.history += 1;
        }
        let mut diff = None;
        if ka > 0 {
            self.reference.scroll_viewport(ScrollTo::Top);
            self.native.scroll_viewport(ScrollTo::Top);
            let rows = self.reference.snapshot_fresh().rows.max(1) as u64;
            let mut top = 0u64;
            loop {
                let (a, b) = (self.reference.snapshot_fresh(), self.native.snapshot());
                let cells = compare(&a, &b, 0, 0, &[], &[])
                    .into_iter()
                    .find(|d| matches!(d.field, "size" | "text" | "width" | "fg" | "bg" | "flags"));
                if let Some(d) = cells {
                    self.fields.insert("history");
                    diff =
                        Some(Diff { field: "history", at: format!("{} (page from scrollback row {top})", d.at), ..d });
                    break;
                }
                if top + rows >= ka {
                    break;
                }
                top += rows;
                self.reference.scroll_viewport(ScrollTo::Delta(rows as isize));
                self.native.scroll_viewport(ScrollTo::Delta(rows as isize));
            }
            self.reference.scroll_viewport(ScrollTo::Bottom);
            self.native.scroll_viewport(ScrollTo::Bottom);
        }
        match diff {
            None => {
                for t in [&mut *self.tally, &mut *self.by_category] {
                    t.history_equal += 1;
                }
            }
            Some(d) => {
                self.all_equal = false;
                if self.first.is_none() {
                    let tail = &self.fed[self.fed.len().saturating_sub(24)..];
                    self.first = Some((self.fed.len() as u64, d, show(tail)));
                }
            }
        }
    }

    fn checkpoint(&mut self, offset: u64) {
        let (a, b) = (self.reference.snapshot_fresh(), self.native.snapshot());
        self.ra.extend(self.reference.take_replies());
        self.rb.extend(self.native.take_replies());
        let (sa, sb) = (self.reference.scrollback_rows(), self.native.scrollback_rows());
        let diffs = compare(&a, &b, sa, sb, &self.ra, &self.rb);
        self.fields.extend(diffs.iter().map(|d| d.field));
        for t in [&mut *self.tally, &mut *self.by_category] {
            t.checkpoints += 1;
            for f in FIELDS {
                if !diffs.iter().any(|d| d.field == *f) {
                    *t.equal.entry(f).or_default() += 1;
                }
            }
        }
        if let Some(d) = diffs.into_iter().next() {
            self.all_equal = false;
            if self.first.is_none() {
                let tail = &self.fed[self.fed.len().saturating_sub(24)..];
                self.first = Some((offset, d, show(tail)));
            }
        }
        // Replies are compared once; each checkpoint compares what came since.
        self.ra.clear();
        self.rb.clear();
    }
}

/// What a run found: its first difference (none: identical throughout) and
/// every field that differed.
struct Outcome {
    first: Option<String>,
    fields: BTreeSet<&'static str>,
    /// Per format of `DUMPS`, at the end: where the dumps differ (none:
    /// identical).
    dumps: [Option<DumpDiff>; 3],
}

struct DumpDiff {
    /// What the first difference comes from: one of `CAUSES`.
    cause: &'static str,
    text: String,
}

/// Causes of a dump difference, in report order. The first two are state
/// haetty-vt does not keep: they are recognised by what the reference dump
/// has around the first difference.
const CAUSES: [&str; 3] = ["underline style or colour", "hyperlink", "other"];

/// The dump formats compared, in report order.
const DUMPS: [(&str, DumpFormat); 3] =
    [("plain", DumpFormat::Plain), ("vt", DumpFormat::Vt), ("html", DumpFormat::Html)];

/// The first byte where two dumps differ, with the bytes around it, and its
/// cause.
fn dump_diff(a: &str, b: &str) -> Option<DumpDiff> {
    if a == b {
        return None;
    }
    let (x, y) = (a.as_bytes(), b.as_bytes());
    let at = x.iter().zip(y).position(|(p, q)| p != q).unwrap_or(x.len().min(y.len()));
    let around = |s: &[u8]| show(&s[at.saturating_sub(40)..(at + 40).min(s.len())]);
    // Underline styles (SGR 4:2 to 4:5, 21), the underline colour (SGR 58)
    // and OSC 8 links (kept by haetty-vt since E5b; before, it kept none).
    let near = &x[at.saturating_sub(60)..(at + 200).min(x.len())];
    let has = |m: &&str| near.windows(m.len()).any(|w| w == m.as_bytes());
    let underline = [
        "\x1b[4:",
        "\x1b[58;",
        "text-decoration-color",
        "style: double",
        "style: wavy",
        "style: dotted",
        "style: dashed",
    ];
    let cause = if underline.iter().any(has) {
        CAUSES[0]
    } else if has(&"<a href") || has(&"</a>") {
        CAUSES[1]
    } else {
        CAUSES[2]
    };
    let text =
        format!("byte {at} (lengths {} / {}): reference …{}… / native …{}…", x.len(), y.len(), around(x), around(y));
    Some(DumpDiff { cause, text })
}

/// Both engines' dumps in every format, compared.
fn dumps(a: &mut dyn VtEngine, b: &mut dyn VtEngine) -> [Option<DumpDiff>; 3] {
    DUMPS.map(|(_, f)| dump_diff(&a.dump(f), &b.dump(f)))
}

fn run_input(
    inp: &Input,
    chunk: usize,
    splits: Option<u64>,
    every: u64,
    tally: &mut Tally,
    cat: &mut Tally,
    compat: Compat,
) -> Outcome {
    let mut reference = match GhosttyEngine::new(&init(80, 24)) {
        Ok(e) => e,
        Err(e) => {
            return Outcome {
                first: Some(format!("{}: reference engine: {e}", inp.name)),
                fields: BTreeSet::new(),
                dumps: [None, None, None],
            }
        }
    };
    // Applications ask for nothing at start; drain what an engine may say.
    let _ = reference.take_replies();
    let mut native = NativeEngine::new(&init(80, 24));
    native.set_compat(compat);
    let mut r = Run {
        reference,
        native,
        ra: vec![],
        rb: vec![],
        fed: vec![],
        first: None,
        fields: BTreeSet::new(),
        tally,
        by_category: cat,
        all_equal: true,
    };
    let mut rng = Rng(splits.unwrap_or(1) ^ 0x9e37_79b9_7f4a_7c15);
    let mut offset = 0u64;
    let mut next_check = every;
    for step in &inp.steps {
        match step {
            Step::Bytes(b) => {
                let mut i = 0;
                while i < b.len() {
                    let n = match splits {
                        Some(_) => 1 + rng.below(chunk as u64) as usize,
                        None => chunk,
                    }
                    .min(b.len() - i);
                    let part = &b[i..i + n];
                    r.reference.feed(part);
                    r.native.feed(part);
                    r.fed.extend_from_slice(part);
                    i += n;
                    offset += n as u64;
                    if offset >= next_check {
                        r.checkpoint(offset);
                        next_check = offset + every;
                    }
                }
            }
            Step::Resize(c, rows) => {
                let gs = GridSize::new(*c, *rows).with_cell(9, 18);
                r.reference.resize(gs);
                r.native.resize(gs);
                r.checkpoint(offset);
            }
        }
    }
    r.checkpoint(offset);
    r.history();
    let dumps = dumps(&mut r.reference, &mut r.native);
    r.tally.inputs += 1;
    r.by_category.inputs += 1;
    if r.all_equal {
        r.tally.inputs_equal += 1;
        r.by_category.inputs_equal += 1;
    }
    let mode = match splits {
        Some(s) => format!("random splits (seed {s})"),
        None => format!("chunks of {chunk}"),
    };
    let fields = std::mem::take(&mut r.fields);
    let first = r.first.map(|(off, d, tail)| {
        format!(
            "{} [{}], {mode}: first difference at input offset {off}: {} {} — reference {} / native {}; last bytes fed: {tail}",
            inp.name, inp.category, d.field, d.at, d.reference, d.native
        )
    });
    Outcome { first, fields, dumps }
}

/// The verdict on one run (`--known`, docs/vtdiff/known.md).
#[derive(Debug, PartialEq)]
enum Verdict {
    Identical,
    /// Explained by these approved entries.
    Known(Vec<String>),
    /// Not explained; would be by these candidates once approved.
    Fail(Option<Vec<String>>),
}

#[allow(clippy::too_many_arguments)]
fn judge(
    inp: &Input,
    chunk: usize,
    splits: Option<u64>,
    every: u64,
    out: &Outcome,
    base: Compat,
    approved: &[Known],
    candidates: &[Known],
) -> Verdict {
    if out.first.is_none() {
        return Verdict::Identical;
    }
    let mut rerun = |c: Compat| {
        let (mut a, mut b) = (Tally::default(), Tally::default());
        run_input(inp, chunk, splits, every, &mut a, &mut b, base | c).first.is_none()
    };
    if let Some(ids) = explained(inp, approved, &out.fields, &mut rerun) {
        return Verdict::Known(ids.into_iter().map(String::from).collect());
    }
    let both: Vec<Known> = approved.iter().chain(candidates).cloned().collect();
    Verdict::Fail(explained(inp, &both, &out.fields, &mut rerun).map(|v| v.into_iter().map(String::from).collect()))
}

/// One entry of the known-differences lists (docs/vtdiff/known.md): a
/// behaviour where haetty-vt follows the specification and libghostty does
/// not.
#[derive(Clone)]
struct Known {
    id: String,
    /// CSI finals that trigger it, with a required first parameter and a
    /// DEC private mode that must also be set somewhere in the input.
    finals: Vec<u8>,
    p0: Option<u16>,
    decset: Option<u16>,
    fields: Vec<String>,
    /// The haetty-vt switch that behaves as libghostty does.
    compat: Compat,
}

/// Reads a list: `[id]` sections with `pattern`, `fields`, `compat`, `basis`
/// and, in the approved list, `approved`.
fn load_known(path: &str, need_approval: bool) -> Result<Vec<Known>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(format!("{path}: {e}")),
    };
    let mut out = Vec::new();
    let mut cur: Option<(String, BTreeMap<String, String>)> = None;
    let finish = |c: Option<(String, BTreeMap<String, String>)>, out: &mut Vec<Known>| -> Result<(), String> {
        let Some((id, kv)) = c else { return Ok(()) };
        let get = |k: &str| kv.get(k).cloned().ok_or_else(|| format!("{path}: [{id}] has no {k}"));
        for k in ["basis", "pattern", "fields", "compat"] {
            get(k)?;
        }
        if need_approval && kv.get("approved").is_none_or(|a| a.trim().is_empty()) {
            return Err(format!("{path}: [{id}] has no approval record"));
        }
        let pat = get("pattern")?;
        let t: Vec<&str> = pat.split_whitespace().collect();
        let bad = || format!("{path}: [{id}] pattern {pat:?}: want `CSI [P] F|F… [with DECSET N]`");
        if t.first() != Some(&"CSI") {
            return Err(bad());
        }
        let mut i = 1;
        let mut p0 = None;
        if let Some(n) = t.get(i).and_then(|s| s.parse().ok()) {
            p0 = Some(n);
            i += 1;
        }
        let finals: Vec<u8> = t.get(i).ok_or_else(bad)?.split('|').filter_map(|f| f.bytes().next()).collect();
        i += 1;
        let mut decset = None;
        if t.get(i) == Some(&"with") {
            if t.get(i + 1) != Some(&"DECSET") {
                return Err(bad());
            }
            decset = Some(t.get(i + 2).and_then(|n| n.parse().ok()).ok_or_else(bad)?);
        }
        let compat = get("compat")?
            .split_whitespace()
            .map(|n| Compat::by_name(n).ok_or_else(|| format!("{path}: [{id}] unknown compat {n:?}")))
            .try_fold(Compat::default(), |a, c| c.map(|c| a | c))?;
        let fields: Vec<String> = get("fields")?.split_whitespace().map(String::from).collect();
        for f in &fields {
            if !FIELDS.contains(&f.as_str()) && f != "history" {
                return Err(format!("{path}: [{id}] unknown field {f:?}"));
            }
        }
        out.push(Known { id, finals, p0, decset, fields, compat });
        Ok(())
    };
    for line in text.lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        if let Some(id) = l.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            finish(cur.take(), &mut out)?;
            cur = Some((id.to_string(), BTreeMap::new()));
        } else if let (Some((k, v)), Some((_, kv))) = (l.split_once('='), cur.as_mut()) {
            kv.insert(k.trim().to_string(), v.trim().to_string());
        } else {
            return Err(format!("{path}: cannot read {l:?}"));
        }
    }
    finish(cur.take(), &mut out)?;
    Ok(out)
}

/// The CSI sequences in `b` (7-bit introducer): private marker, parameters,
/// final.
fn csis(b: &[u8]) -> Vec<(u8, Vec<u16>, u8)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < b.len() {
        if b[i] != 0x1b || b[i + 1] != b'[' {
            i += 1;
            continue;
        }
        let mut j = i + 2;
        let private = match b.get(j) {
            Some(&c @ 0x3c..=0x3f) => {
                j += 1;
                c
            }
            _ => 0,
        };
        let mut params = vec![];
        let mut n: Option<u32> = None;
        while let Some(&c) = b.get(j) {
            match c {
                b'0'..=b'9' => n = Some(n.unwrap_or(0) * 10 + (c - b'0') as u32).map(|v| v.min(65535)),
                b';' | b':' => params.push(n.take().unwrap_or(0) as u16),
                0x20..=0x2f => {}
                _ => break,
            }
            j += 1;
        }
        if let Some(n) = n {
            params.push(n as u16);
        }
        if let Some(&f @ 0x40..=0x7e) = b.get(j) {
            out.push((private, params, f));
        }
        i = j.max(i + 1);
    }
    out
}

fn known_matches(k: &Known, seqs: &[(u8, Vec<u16>, u8)]) -> bool {
    let hit = seqs.iter().any(|(p, ps, f)| {
        *p == 0 && k.finals.contains(f) && k.p0.is_none_or(|want| ps.first().copied().unwrap_or(0) == want)
    });
    hit && k.decset.is_none_or(|m| seqs.iter().any(|(p, ps, f)| *p == b'?' && *f == b'h' && ps.contains(&m)))
}

/// Whether a differing run is explained by `list`: entries whose pattern
/// occurs in the input, whose fields cover every field that differed, and
/// with whose switches on haetty-vt then matches libghostty throughout.
fn explained<'k>(
    inp: &Input,
    list: &'k [Known],
    fields: &BTreeSet<&'static str>,
    rerun: &mut dyn FnMut(Compat) -> bool,
) -> Option<Vec<&'k str>> {
    let bytes: Vec<u8> = inp
        .steps
        .iter()
        .flat_map(|s| match s {
            Step::Bytes(b) => b.clone(),
            Step::Resize(..) => vec![],
        })
        .collect();
    let seqs = csis(&bytes);
    let hits: Vec<&Known> = list.iter().filter(|k| known_matches(k, &seqs)).collect();
    if hits.is_empty() || !fields.iter().all(|f| hits.iter().any(|k| k.fields.iter().any(|g| g == f))) {
        return None;
    }
    let compat = hits.iter().fold(Compat::default(), |a, k| a | k.compat);
    rerun(compat).then(|| hits.iter().map(|k| k.id.as_str()).collect())
}

/// vttest-like scenarios: each probes state that only shows later.
fn scenarios() -> Vec<Input> {
    let s = |name: &str, bytes: &[u8]| Input {
        name: name.into(),
        category: "scenario".into(),
        steps: vec![Step::Bytes(bytes.to_vec())],
    };
    let lines: Vec<u8> = (0..40).flat_map(|i| format!("line {i}\r\n").into_bytes()).collect();
    let full = "x".repeat(80);
    vec![
        s("scroll region", &[b"\x1b[5;10r\x1b[5;1H".as_slice(), &lines, b"\x1b[r\x1b[24;1Hend"].concat()),
        s("insert and delete lines", &[&lines[..200], b"\x1b[3;1H\x1b[2L\x1b[5;1H\x1b[3M\x1b[1;1Habc\x1b[1;2H\x1b[2@\x1b[1P\x1b[3X".as_slice()].concat()),
        s("pending wrap", format!("{full}\rA\r\n{full}\x1b[K\r\n{full}B").as_bytes()),
        s("origin mode", b"\x1b[3;8r\x1b[?6h\x1b[1;1Ho\x1b[10;5Hp\x1b[?6l\x1b[1;1Hq\x1b[r"),
        s("tab stops", b"a\tb\tc\x1b[3g\x1bH\r\x1b[5C\x1bH\r\t1\t2\r\n\x1b[0g\x1b[3g\t3"),
        s("charsets", b"\x1b(0lqqk\r\nx  x\r\nmqqj\x1b(B ok \x0eaa\x0fbb"),
        s("saved cursor", b"\x1b[5;5Hab\x1b7\x1b[10;10Hcd\x1b8ef\x1b[s\x1b[1;1H\x1b[ugh"),
        s("alternate screen", b"main\x1b[?1049halt\x1b[2Jsecond\x1b[?1049lback"),
        s("sgr colours and styles", b"\x1b[31mr\x1b[92mg\x1b[38;5;200mp\x1b[38;2;10;20;30mt\x1b[44mb\x1b[1;3;4;7mx\x1b[0mn\x1b[2;9;53mz\x1b[0m"),
        s("erase", &[&lines[..300], b"\x1b[10;10H\x1b[1J\x1b[12;5H\x1b[0J\x1b[3;3H\x1b[1K\x1b[4;4H\x1b[2K".as_slice()].concat()),
        s("wide characters", "가나다라마바사아자차카타파하가나다라마바사아자차카타파하가나다라마바사아자차카타파하가나\r\n😀😁 x\u{fe0f} ✔\u{fe0e} 한글".as_bytes()),
        s("queries", b"\x1b[c\x1b[>c\x1b[6n\x1b[5n\x1b[>q\x1b[?2026$p\x1b[?1049$p\x1b]10;?\x07\x1b]11;?\x1b\\\x1b[14t\x1b[18t\x1b[?u"),
        s("title and pwd", b"\x1b]0;one\x07\x1b]2;two\x1b\\\x1b]7;file://host/tmp/x\x07"),
        s("cursor movement", b"\x1b[10;10H\x1b[100A\x1b[200B\x1b[300C\x1b[400D\x1b[5G\x1b[7d\x1b[2E\x1b[F*"),
        s("reverse index", &[&lines[..150], b"\x1b[3;6r\x1b[3;1H\x1bM\x1bMri\x1b[r".as_slice()].concat()),
        s("modes", b"\x1b[?2004h\x1b[?1004h\x1b[?1000h\x1b[?1006h\x1b[?1h\x1b[?25l\x1b[?2026h\x1b[?2026l\x1b[>1u"),
    ]
}

fn resizes() -> Vec<Input> {
    let long = "w".repeat(150);
    vec![
        Input {
            name: "reflow narrower and wider".into(),
            category: "resize".into(),
            steps: vec![
                Step::Bytes(format!("{long}\r\nshort\r\n{long}").into_bytes()),
                Step::Resize(50, 24),
                Step::Resize(120, 30),
                Step::Bytes(b"\r\nafter".to_vec()),
            ],
        },
        Input {
            name: "alternate screen resized".into(),
            category: "resize".into(),
            steps: vec![
                Step::Bytes(b"\x1b[?1049h\x1b[10;10Hcenter".to_vec()),
                Step::Resize(40, 12),
                Step::Bytes(b"\x1b[?1049lmain".to_vec()),
                Step::Resize(80, 24),
            ],
        },
    ]
}

/// Synthetic load: words, colours, long wrapping lines, wide characters.
fn synthetic(seed: u64) -> Input {
    let mut rng = Rng(seed);
    let words = ["alpha", "beta", "gamma", "δέλτα", "한글", "漢字", "😀", "ok", "x"];
    let mut b = Vec::new();
    for line in 0..400 {
        if rng.below(5) == 0 {
            let _ = write!(Text(&mut b), "\x1b[{}m", 30 + rng.below(8));
        }
        let n = 1 + rng.below(if line % 50 == 0 { 60 } else { 12 });
        for _ in 0..n {
            b.extend_from_slice(words[rng.below(words.len() as u64) as usize].as_bytes());
            b.push(b' ');
        }
        b.extend_from_slice(b"\x1b[0m\r\n");
    }
    Input { name: format!("synthetic {seed}"), category: "synthetic".into(), steps: vec![Step::Bytes(b)] }
}

/// Fuzz: random printable text, controls, CSI / ESC / OSC sequences.
fn fuzz(seed: u64) -> Input {
    let mut rng = Rng(seed.wrapping_add(0xf00d));
    let mut b = Vec::new();
    while b.len() < 8192 {
        match rng.below(10) {
            0..=4 => (0..1 + rng.below(20)).for_each(|_| b.push(b' ' + rng.below(95) as u8)),
            5 => b.push([b'\r', b'\n', b'\t', 0x08, 0x07, 0x0e, 0x0f][rng.below(7) as usize]),
            6 | 7 => {
                b.extend_from_slice(b"\x1b[");
                for i in 0..rng.below(4) {
                    if i > 0 {
                        b.push(b';');
                    }
                    let _ = write!(Text(&mut b), "{}", rng.below(120));
                }
                b.push(b"ABCDEFGHJKLMPSTXZ@`dfghlmnrsu"[rng.below(29) as usize]);
            }
            8 => b.extend_from_slice(
                [b"\x1b7".as_slice(), b"\x1b8", b"\x1bM", b"\x1bD", b"\x1bE", b"\x1bc", b"\x1b(0", b"\x1b(B"]
                    [rng.below(8) as usize],
            ),
            _ => {
                let _ = write!(Text(&mut b), "\x1b]{};t{}\x07", [0, 2, 7, 52][rng.below(4) as usize], rng.below(1000));
            }
        }
    }
    Input { name: format!("fuzz {seed}"), category: "fuzz".into(), steps: vec![Step::Bytes(b)] }
}

/// Splits a stream at up to two random points with a resize between the parts.
fn with_resizes(rng: &mut Rng, b: Vec<u8>) -> Vec<Step> {
    let mut steps = Vec::new();
    let mut rest = b;
    for _ in 0..rng.below(3) {
        if rest.len() < 2 {
            break;
        }
        let at = 1 + rng.below(rest.len() as u64 - 1) as usize;
        let tail = rest.split_off(at);
        steps.push(Step::Bytes(rest));
        steps.push(Step::Resize(10 + rng.below(140) as u16, 3 + rng.below(50) as u16));
        rest = tail;
    }
    steps.push(Step::Bytes(rest));
    steps
}

/// Fuzz, second mix: what the first leaves out — private modes (69 with
/// DECSLRM, DESIGN.md §10.9), non-ASCII
/// text (wide, combining, variation selectors, ZWJ, flags), SGR colon forms
/// and extended colours, OSC colours, DCS/APC strings, kitty keyboard and
/// the other replies, edits across wide characters, long lines; resizes in
/// between.
fn fuzz2(seed: u64) -> Input {
    let mut rng = Rng(seed.wrapping_add(0xbeef));
    let texts = [
        "가나다",
        "漢字",
        "😀",
        "e\u{301}",
        "x\u{fe0f}",
        "\u{2764}\u{fe0f}",
        "\u{2714}\u{fe0e}",
        "👨\u{200d}👩",
        "a",
        "hello ",
        "\u{200d}",
        "\u{301}",
        "ｗ",
        "🇰🇷",
        "\u{a0}",
        "─",
        "\u{25cf}",
    ];
    let private = [
        1u16, 5, 6, 7, 12, 25, 47, 66, 69, 69, 1000, 1002, 1003, 1004, 1005, 1006, 1015, 1016, 1047, 1048, 1049, 2004,
        2026, 2027, 2031, 2048,
    ];
    let nums = [0u16, 1, 2, 3, 5, 8, 22, 80, 200];
    let mut krng = Rng(seed.wrapping_add(0x6b69_7474));
    let mut b = Vec::new();
    while b.len() < 8192 {
        match rng.below(16) {
            0..=3 => {
                let t = texts[rng.below(texts.len() as u64) as usize];
                (0..1 + rng.below(10)).for_each(|_| b.extend_from_slice(t.as_bytes()));
            }
            4 => {
                let m = private[rng.below(private.len() as u64) as usize];
                let _ = write!(Text(&mut b), "\x1b[?{m}{}", ["h", "l", "$p"][rng.below(3) as usize]);
            }
            5 | 6 => {
                b.extend_from_slice(b"\x1b[");
                for i in 0..rng.below(4) {
                    if i > 0 {
                        b.push(b';');
                    }
                    let _ = write!(Text(&mut b), "{}", nums[rng.below(nums.len() as u64) as usize]);
                }
                b.push(b"ABCDEFGHIJKLMPSTXZ@`abdefghjklmnrstu"[rng.below(36) as usize]);
            }
            7 => {
                let n = rng.below(256);
                let (r, g, bl) = (rng.below(300), rng.below(256), rng.below(256));
                let forms = [
                    format!("38;5;{n}"),
                    format!("48;2;{r};{g};{bl}"),
                    format!("38:2::{r}:{g}:{bl}"),
                    format!("48:5:{n}"),
                    "4:3".into(),
                    format!("58;5;{n}"),
                    format!("{}", 1 + rng.below(9)),
                    format!("{}", 21 + rng.below(9)),
                    "53".into(),
                    "55".into(),
                    format!("{}", 90 + rng.below(18)),
                    "0".into(),
                    String::new(),
                    format!("38;{}", rng.below(10)),
                ];
                let _ = write!(Text(&mut b), "\x1b[{}m", forms[rng.below(forms.len() as u64) as usize]);
            }
            8 => {
                let n = rng.below(256);
                let forms = [
                    format!("4;{n};rgb:{:x}/{:x}/{:x}", rng.below(256), rng.below(256), rng.below(256)),
                    format!("4;{n};?"),
                    "10;?".into(),
                    "11;#abc".into(),
                    format!("104;{n}"),
                    "104".into(),
                    "110".into(),
                    "111".into(),
                    "112".into(),
                    "12;?".into(),
                    format!("0;t{n}"),
                    format!("2;t;{n}"),
                    format!("7;file://h/p{n}"),
                    "52;c;aGk=".into(),
                ];
                let end = ["\x07", "\x1b\\"][rng.below(2) as usize];
                let _ = write!(Text(&mut b), "\x1b]{}{end}", forms[rng.below(forms.len() as u64) as usize]);
            }
            9 => b.push([b'\r', b'\n', b'\t', 0x08, 0x07, 0x0e, 0x0f, 0x0b, 0x0c, 0x00][rng.below(10) as usize]),
            10 => b.extend_from_slice(
                [
                    b"\x1b7".as_slice(),
                    b"\x1b8",
                    b"\x1bM",
                    b"\x1bD",
                    b"\x1bE",
                    b"\x1bH",
                    b"\x1b=",
                    b"\x1b>",
                    b"\x1b(0",
                    b"\x1b(B",
                    b"\x1b)0",
                    b"\x1b*0",
                    b"\x1bn",
                    b"\x1bN",
                    b"\x1b#8",
                    b"\x1bZ",
                    b"\x1bc",
                ][rng.below(17) as usize],
            ),
            // DCS queries, or kitty graphics commands (from their own
            // generator, so the rest of the stream stays as it was).
            11 => match rng.below(4) {
                0 => b.extend_from_slice(b"\x1bP$qm\x1b\\"),
                2 => b.extend_from_slice(b"\x1bP+q544e\x1b\\"),
                _ => kitty_gfx(&mut krng, &mut b),
            },
            12 => (0..70 + rng.below(100)).for_each(|_| b.push(b'w')),
            13 => {
                let f = rng.below(32);
                let forms = [format!(">{f}u"), "<u".into(), format!("={f};{}u", 1 + rng.below(3)), "?u".into()];
                let _ = write!(Text(&mut b), "\x1b[{}", forms[rng.below(4) as usize]);
            }
            14 => {
                let _ = write!(Text(&mut b), "\x1b[{} q", rng.below(7));
            }
            _ => {
                let _ = write!(Text(&mut b), "\x1b[{}t", [14, 16, 18, 22, 23][rng.below(5) as usize]);
            }
        }
    }
    let steps = with_resizes(&mut rng, b);
    Input { name: format!("fuzz2 {seed}"), category: "fuzz2".into(), steps }
}

/// Underline kinds and colours, and OSC 8 hyperlinks (E5b): SGR 4:x, 21,
/// 24, 58, 59 and 0 among text, wide characters and wraps; links opened and
/// ended in the forms libghostty's parser treats differently (ids, keys
/// without values, an id with an empty target); and what may keep or drop a
/// link: DECSC/DECRC, the alternate screens, DECALN, RIS, erasing,
/// inserting and deleting. A category of its own, so that the others keep
/// their streams.
fn decor(seed: u64) -> Input {
    let mut r = Rng(seed.wrapping_add(0xdec0));
    let texts = ["link", "가나", "😀", "e\u{301}", "a b", "ｗｗ", "─x"];
    let uris = ["http://a.example/x", "https://b.example/é?q=1#f", "file:///tmp/f.txt", "ssh://h", "x"];
    let ids = ["", "id=1", "id=", "id=a:id=b", ":id=c", "k", "=v:id=d", "id=1:k=v"];
    let mut b = Vec::new();
    while b.len() < 4096 {
        match r.below(12) {
            0..=3 => {
                let t = texts[r.below(texts.len() as u64) as usize];
                (0..1 + r.below(6)).for_each(|_| b.extend_from_slice(t.as_bytes()));
            }
            4 | 5 => {
                let (n, x, y, z) = (r.below(256), r.below(256), r.below(256), r.below(256));
                let forms = [
                    "4".to_string(),
                    format!("4:{}", r.below(8)),
                    "21".into(),
                    "24".into(),
                    format!("58;5;{n}"),
                    format!("58;2;{x};{y};{z}"),
                    format!("58:2::{x}:{y}:{z}"),
                    format!("58:5:{n}"),
                    "59".into(),
                    "0".into(),
                    "1;4:3;58;5;1".into(),
                    format!("38;5;{n};4"),
                ];
                let _ = write!(Text(&mut b), "\x1b[{}m", forms[r.below(forms.len() as u64) as usize]);
            }
            6 | 7 => {
                let end = ["\x07", "\x1b\\"][r.below(2) as usize];
                let id = ids[r.below(ids.len() as u64) as usize];
                let uri = if r.below(4) == 0 { "" } else { uris[r.below(uris.len() as u64) as usize] };
                let _ = write!(Text(&mut b), "\x1b]8;{id};{uri}{end}");
            }
            8 => b.extend_from_slice(
                [
                    b"\x1b7".as_slice(),
                    b"\x1b8",
                    b"\x1b[?1049h",
                    b"\x1b[?1049l",
                    b"\x1b[?47h",
                    b"\x1b[?47l",
                    b"\x1b[?1047h",
                    b"\x1b[?1047l",
                    b"\x1b#8",
                    b"\x1bc",
                ][r.below(10) as usize],
            ),
            9 => {
                let n = 1 + r.below(4);
                let f = b"KJXP@LMrABCDH"[r.below(13) as usize] as char;
                let _ = write!(Text(&mut b), "\x1b[{n}{f}");
            }
            10 => b.extend_from_slice([b"\r\n".as_slice(), b"\r", b"\n", b"\t", b"\x08"][r.below(5) as usize]),
            _ => (0..20 + r.below(70)).for_each(|_| b.push(b'w')),
        }
    }
    let steps = with_resizes(&mut r, b);
    Input { name: format!("decor {seed}"), category: "decor".into(), steps }
}

/// Base64 with padding.
fn b64(b: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::new();
    for c in b.chunks(3) {
        let v = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            s.push(if i <= c.len() { T[(v >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    s
}

/// `data` as a zlib stream of stored blocks (no compressor needed).
fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let mut z = vec![0x78, 0x01];
    let mut chunks = data.chunks(65535).peekable();
    if chunks.peek().is_none() {
        z.extend_from_slice(&[1, 0, 0, 0xff, 0xff]);
    }
    while let Some(c) = chunks.next() {
        z.push(u8::from(chunks.peek().is_none()));
        z.extend_from_slice(&(c.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(c.len() as u16)).to_le_bytes());
        z.extend_from_slice(c);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in data {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());
    z
}

/// `f`'s text with probability `p` %.
fn maybe(r: &mut Rng, p: u64, f: impl FnOnce(&mut Rng) -> String) -> String {
    if r.below(100) < p {
        f(r)
    } else {
        String::new()
    }
}

/// A kitty graphics command for `fuzz2`: transmissions (chunked, zlib,
/// numbers, quiet levels, wrong sizes), placements (cells, cursor movement,
/// virtual), deletes of every kind, queries, animation, and broken ones (bad
/// keys and values, files and shared memory, bad base64).
fn kitty_gfx(r: &mut Rng, b: &mut Vec<u8>) {
    let ids = [1u32, 2, 3, 7];
    let id = ids[r.below(4) as usize];
    let mut keys = String::new();
    match r.below(16) {
        // Transmit (and display).
        0..=6 => {
            let (f, bpp) = [(24, 3), (24, 3), (32, 4), (100, 4)][r.below(4) as usize];
            let (w, h) = (1 + r.below(5) as usize, 1 + r.below(4) as usize);
            let mut len = w * h * bpp;
            if r.below(6) == 0 {
                len = (len + r.below(3) as usize).saturating_sub(1);
            }
            let raw: Vec<u8> =
                if f == 100 { b"\x89PNG\r\n\x1a\n".to_vec() } else { (0..len).map(|i| (i * 37) as u8).collect() };
            let zlib = r.below(4) == 0;
            let mut payload = if zlib { zlib_stored(&raw) } else { raw };
            if zlib && r.below(4) == 0 {
                let n = payload.len() - 1 - r.below(5) as usize;
                payload.truncate(n);
            }
            let a = ["t", "T", "T", "q"][r.below(4) as usize];
            keys += &format!("a={a},f={f}");
            if r.below(8) != 0 {
                keys += &format!(",s={w},v={h}");
            }
            keys += &match r.below(10) {
                0..=5 => format!(",i={id}"),
                6 | 7 => format!(",I={id}"),
                8 => format!(",i={id},I={id}"),
                _ => String::new(),
            };
            keys += &maybe(r, 20, |r| format!(",p={}", 1 + r.below(3)));
            keys += &maybe(r, 30, |r| format!(",q={}", r.below(3)));
            keys += &maybe(r, 30, |r| format!(",c={}", r.below(6)));
            keys += &maybe(r, 30, |r| format!(",r={}", r.below(4)));
            keys += &maybe(r, 20, |r| format!(",C={}", r.below(2)));
            keys += &maybe(r, 10, |_| ",U=1".into());
            keys += &maybe(r, 10, |r| format!(",X={},Y={}", r.below(20), r.below(40)));
            keys += &maybe(r, 10, |r| format!(",z={}", r.below(4) as i64 - 1));
            if zlib {
                keys += ",o=z";
            }
            let data = b64(&payload);
            if r.below(3) == 0 && data.len() > 4 {
                // In chunks of whole base64 quads; the later ones carry only
                // `m` (and sometimes `q` or an id).
                let mut parts = Vec::new();
                let mut rest = data.as_str();
                while rest.len() > 4 && parts.len() < 2 {
                    let at = 4 * (1 + r.below((rest.len() / 4) as u64 - 1).min(3) as usize);
                    parts.push(&rest[..at]);
                    rest = &rest[at..];
                }
                parts.push(rest);
                for (i, part) in parts.iter().enumerate() {
                    let last = i + 1 == parts.len();
                    let ctl = if i == 0 {
                        format!("{keys},m=1")
                    } else {
                        let extra = maybe(r, 20, |r| [",q=1", ",q=2", ",i=1", ",a=T"][r.below(4) as usize].into());
                        format!("m={}{extra}", u8::from(!last))
                    };
                    let _ = write!(Text(&mut *b), "\x1b_G{ctl};{part}\x1b\\");
                }
                return;
            }
            let _ = write!(Text(&mut *b), "\x1b_G{keys};{data}\x1b\\");
            return;
        }
        // Display.
        7..=9 => {
            keys += &if r.below(4) == 0 { format!("a=p,I={id}") } else { format!("a=p,i={id}") };
            keys += &maybe(r, 30, |r| format!(",p={}", 1 + r.below(3)));
            keys += &maybe(r, 40, |r| format!(",c={}", r.below(6)));
            keys += &maybe(r, 40, |r| format!(",r={}", r.below(4)));
            keys += &maybe(r, 20, |r| format!(",C={}", r.below(2)));
            keys += &maybe(r, 10, |_| ",U=1".into());
            keys += &maybe(r, 5, |_| ",P=1".into());
            keys += &maybe(r, 20, |r| format!(",q={}", r.below(3)));
            keys += &maybe(r, 10, |r| format!(",w={},h={}", r.below(30), r.below(30)));
            keys += &maybe(r, 10, |r| format!(",z={}", r.below(4) as i64 - 1));
        }
        // Delete.
        10..=12 => {
            let d = b"aAiInNcCpPqQxXyYzZrRfF"[r.below(22) as usize] as char;
            keys += &format!("a=d,d={d}");
            keys += &maybe(r, 60, |_| format!(",i={id}"));
            keys += &maybe(r, 30, |_| format!(",I={id}"));
            keys += &maybe(r, 20, |r| format!(",p={}", 1 + r.below(3)));
            keys += &maybe(r, 60, |r| format!(",x={},y={}", r.below(8), r.below(8)));
            keys += &maybe(r, 30, |r| format!(",z={}", r.below(4) as i64 - 1));
        }
        13 => keys += &format!("a={},i={id}", ["f", "a", "c"][r.below(3) as usize]),
        // Broken ones.
        _ => {
            let bad = [
                format!("a=t,i={id},zz=1;AAAA"),
                format!("a=x,i={id}"),
                format!("a=t,i={id},s=1,v=1,f=24;!!!!"),
                format!("a=t,i={id},s=1,v=1,t=f;L3RtcC94"),
                format!("a=t,i={id},s=1,v=1,t=s;eHg="),
                format!("a=q,i={id},t=t;L3RtcC94"),
                format!("a=t,i={id},f=7;AAAA"),
                "a=t,i=,s=1;AAAA".into(),
                format!("a=T,i={id},C=5,s=1,v=1,f=24;AAAA"),
                format!("a=p,i={id},c=99999999999"),
                format!("a=q,i={id},q=1;AA\nAA"),
                format!("a=t,i={id},s=1,v=1,f=24;AA=A"),
                "m=0;AAAA".into(),
                format!("a=d,d=r,x=3,y={}", r.below(5)),
            ];
            let _ = write!(Text(&mut *b), "\x1b_G{}\x1b\\", bad[r.below(bad.len() as u64) as usize]);
            return;
        }
    }
    let _ = write!(Text(&mut *b), "\x1b_G{keys}\x1b\\");
}

/// The fuzz test's grammar (escape-sequence heavy, random bytes, broken
/// UTF-8), with resizes in between.
fn grammar(seed: u64) -> Input {
    let mut r = vtgen::Rng(seed.wrapping_add(0x5eed));
    let b = vtgen::grammar(&mut r, 8192);
    let mut rng = Rng(seed);
    Input { name: format!("grammar {seed}"), category: "grammar".into(), steps: with_resizes(&mut rng, b) }
}

/// Mutations of the Claude-Code-like stream (with pieces of a corpus file
/// spliced in when there is one).
fn mutated(seed: u64, other: &[u8]) -> Input {
    let mut r = vtgen::Rng(seed.wrapping_add(0x3a7e));
    let b = vtgen::mutate(&mut r, &vtgen::claude_like(), other);
    Input { name: format!("mutated {seed}"), category: "mutated".into(), steps: vec![Step::Bytes(b)] }
}

/// `write!` into a byte vector.
struct Text<'a>(&'a mut Vec<u8>);
impl std::fmt::Write for Text<'_> {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        self.0.extend_from_slice(s.as_bytes());
        Ok(())
    }
}

/// `\e`, `\r`, `\n`, `\t`, `\xNN` and `\\` in a probe argument.
fn unescape(s: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 1 < b.len() {
            i += 1;
            match b[i] {
                b'e' => out.push(0x1b),
                b'r' => out.push(b'\r'),
                b'n' => out.push(b'\n'),
                b't' => out.push(b'\t'),
                b'a' => out.push(0x07),
                b'x' if i + 2 < b.len() => {
                    out.push(u8::from_str_radix(&s[i + 1..i + 3], 16).unwrap_or(b'?'));
                    i += 2;
                }
                c => out.push(c),
            }
        } else {
            out.push(b[i]);
        }
        i += 1;
    }
    out
}

/// `--probe BYTES`: both engines' state after BYTES, side by side (development aid).
fn probe(bytes: &[u8], cols: u16, rows: u16) {
    let mut a = GhosttyEngine::new(&init(cols, rows)).expect("reference engine");
    let mut b = NativeEngine::new(&init(cols, rows));
    // `<resize CxR>` in the input resizes both engines there.
    const MARK: &[u8] = b"<resize ";
    if bytes.windows(MARK.len()).any(|w| w == MARK) {
        let mut rest = bytes;
        loop {
            let at = rest.windows(MARK.len()).position(|w| w == MARK).unwrap_or(rest.len());
            a.feed(&rest[..at]);
            b.feed(&rest[..at]);
            if at == rest.len() {
                break;
            }
            let spec = &rest[at + MARK.len()..];
            let end = spec.iter().position(|&c| c == b'>').unwrap_or(spec.len());
            let txt = String::from_utf8_lossy(&spec[..end]).into_owned();
            let (c, r) = txt.split_once('x').unwrap_or(("80", "24"));
            let gs = GridSize::new(c.parse().unwrap_or(80), r.parse().unwrap_or(24)).with_cell(9, 18);
            a.resize(gs);
            b.resize(gs);
            rest = &spec[(end + 1).min(spec.len())..];
        }
    } else if std::env::var_os("VTDIFF_SPLIT").is_some() {
        // VTDIFF_SPLIT=1: one byte per write, as `--bisect` feeds.
        // VTDIFF_SPLIT=snap: and a snapshot after each.
        let snap = std::env::var("VTDIFF_SPLIT").is_ok_and(|v| v == "snap");
        for c in bytes.chunks(1) {
            a.feed(c);
            b.feed(c);
            if snap {
                let _ = (a.snapshot(), b.snapshot());
            }
        }
    } else {
        a.feed(bytes);
        b.feed(bytes);
    }
    let mut seen = Vec::new();
    // A fresh reference snapshot (see `GhosttyEngine::snapshot_fresh`).
    let fresh = a.snapshot_fresh();
    for (name, e) in [("reference", &mut a as &mut dyn VtEngine), ("native", &mut b as &mut dyn VtEngine)] {
        let s = if name == "reference" && std::env::var("VTDIFF_SPLIT").map_or(true, |v| v != "snap") {
            fresh.clone()
        } else {
            e.snapshot()
        };
        let r = e.take_replies();
        seen.push((s.clone(), e.scrollback_rows(), r.clone()));
        println!("== {name}: {}x{}, cursor {:?}, scrollback {}", s.cols, s.rows, s.cursor, e.scrollback_rows());
        println!("   modes {:?}", s.modes);
        println!(
            "   title {:?} pwd {:?} default fg {:?} bg {:?} cursor colour {:?}",
            s.title, s.pwd, s.default_fg, s.default_bg, s.cursor_color
        );
        println!("   replies: {}", show_all(&r));
        println!("   events: {:?}", e.take_events().iter().map(|e| format!("{e:?}")).collect::<Vec<_>>());
        for y in 0..s.rows {
            for x in 0..s.cols {
                let c = s.cell(x, y).unwrap();
                if !c.text.is_empty()
                    || c.fg.is_some()
                    || c.bg.is_some()
                    || c.flags.0 != 0
                    || c.width != haetty_core::Width::Narrow
                {
                    println!(
                        "   ({x},{y}) {:?} {:?} fg {:?} bg {:?} flags {:#x}",
                        c.text, c.width, c.fg, c.bg, c.flags.0
                    );
                }
            }
        }
    }
    let ((sa, ka, ra), (sb, kb, rb)) = (&seen[0], &seen[1]);
    for d in compare(sa, sb, *ka, *kb, ra, rb) {
        println!("!! {} {}: reference {} / native {}", d.field, d.at, d.reference, d.native);
    }
    for (name, f) in DUMPS {
        let (x, y) = (a.dump(f), b.dump(f));
        println!("== dump {name}: reference {}", show_all(x.as_bytes()));
        match dump_diff(&x, &y) {
            None => println!("   native identical"),
            Some(d) => println!("   native {}\n!! dump {name} [{}] {}", show_all(y.as_bytes()), d.cause, d.text),
        }
    }
}

/// Inputs where a joining character arrives in a later read than its base
/// (DESIGN.md §10.8): the reads are the `Step::Bytes` themselves.
fn joiners() -> Vec<Input> {
    let split = |name: &str, parts: &[&str]| Input {
        name: name.into(),
        category: "joiners".into(),
        steps: parts.iter().map(|p| Step::Bytes(p.as_bytes().to_vec())).collect(),
    };
    let mut v = vec![
        split("combining", &["cafe", "\u{301} ok"]),
        split("vs16", &["\u{2764}", "\u{fe0f}"]),
        split("vs16-narrow-to-wide", &["ab\u{263a}", "\u{fe0f}cd"]),
        split("zwj", &["\u{1f468}", "\u{200d}", "\u{1f469}"]),
        split("vs16-bytes", &["\u{2764}\u{ef}", "\u{b8}", "\u{8f}"]),
        split("2027-combining", &["\x1b[?2027hcafe", "\u{301}"]),
        split("2027-zwj", &["\x1b[?2027h\u{1f468}", "\u{200d}", "\u{1f469}"]),
        split("2027-flag", &["\x1b[?2027h\u{1f1f0}", "\u{1f1f7}"]),
        // Not a joiner: libghostty's printCell turns the row above's spacer
        // head back into a blank without marking that row (DESIGN.md §10.8).
        split("spacer-head", &[format!("\x1b[2;1H{}", "w".repeat(79)).as_str(), "\u{1f600}", "\x1b[3;1Hx"]),
    ];
    // The byte split inside VS16 has to be raw bytes.
    v[4].steps =
        vec![Step::Bytes(b"\xe2\x9d\xa4\xef".to_vec()), Step::Bytes(b"\xb8".to_vec()), Step::Bytes(b"\x8f".to_vec())];
    v
}

/// `--refresh`: libghostty's render state read three ways (`Refresh` in
/// haetty-core's ghostty.rs): a snapshot after every read, compared with a
/// render state built fresh for that snapshot; rows copied and time per
/// snapshot. Reads are 4096 bytes, then random 1–64 bytes. `VTDIFF_STALE=1`
/// prints the first stale place per input with `Refresh::Joined`.
fn refresh(inputs: &[Input]) {
    use haetty_core::ghostty::Refresh;
    use std::time::{Duration, Instant};
    let modes = [Refresh::Incremental, Refresh::Joined, Refresh::Always];
    #[derive(Default, Clone)]
    struct T {
        snapshots: u64,
        fresh: u64,
        rows: u64,
        time: Duration,
        stale: u64,
        stale_inputs: u64,
    }
    let mut by: BTreeMap<String, [T; 3]> = BTreeMap::new();
    let mut stale_names: Vec<String> = Vec::new();
    for inp in inputs {
        for small in [false, true] {
            let mut engines: Vec<GhosttyEngine> = Vec::new();
            for m in modes {
                let mut e = GhosttyEngine::new(&init(80, 24)).expect("engine");
                e.set_refresh(m);
                engines.push(e);
            }
            let mut truth = GhosttyEngine::new(&init(80, 24)).expect("engine");
            let mut rng = Rng(0x5eed ^ small as u64);
            let mut t: [T; 3] = Default::default();
            let mut stale = [false; 3];
            let mut step = |engines: &mut Vec<GhosttyEngine>,
                            truth: &mut GhosttyEngine,
                            t: &mut [T; 3],
                            last: &[u8]| {
                let want = truth.snapshot_fresh();
                for (k, e) in engines.iter_mut().enumerate() {
                    let start = Instant::now();
                    let s = e.snapshot();
                    t[k].time += start.elapsed();
                    if s.cells != want.cells || s.cursor != want.cursor {
                        t[k].stale += 1;
                        if k == 1 && !stale[k] && std::env::var_os("VTDIFF_STALE").is_some() {
                            let at = (0..want.cells.len().min(s.cells.len()))
                                .find(|&i| s.cells[i] != want.cells[i])
                                .map(|i| {
                                    format!("cell {},{}: {:?} / fresh {:?}", i % 80, i / 80, s.cells[i], want.cells[i])
                                })
                                .unwrap_or_else(|| format!("cursor {:?} / fresh {:?}", s.cursor, want.cursor));
                            eprintln!("STALE {}: {at}; last read {}", inp.name, show(last));
                        }
                        stale[k] = true;
                    }
                }
            };
            for st in &inp.steps {
                match st {
                    Step::Bytes(b) => {
                        let mut i = 0;
                        // A joiners input's steps are its reads.
                        let whole = inp.category == "joiners";
                        while i < b.len() {
                            let n = if whole {
                                b.len()
                            } else if small {
                                1 + rng.below(64) as usize
                            } else {
                                4096
                            }
                            .min(b.len() - i);
                            for e in engines.iter_mut() {
                                e.feed(&b[i..i + n]);
                                let _ = e.take_replies();
                            }
                            truth.feed(&b[i..i + n]);
                            let _ = truth.take_replies();
                            step(&mut engines, &mut truth, &mut t, &b[i..i + n]);
                            i += n;
                        }
                    }
                    Step::Resize(c, r) => {
                        let gs = GridSize::new(*c, *r).with_cell(9, 18);
                        for e in engines.iter_mut() {
                            e.resize(gs);
                        }
                        truth.resize(gs);
                        step(&mut engines, &mut truth, &mut t, b"<resize>");
                    }
                }
            }
            let cat = by.entry(inp.category.clone()).or_default();
            for k in 0..3 {
                let s = engines[k].stats;
                t[k].snapshots = s.snapshots;
                t[k].fresh = s.fresh;
                t[k].rows = s.rows_copied;
                t[k].stale_inputs = stale[k] as u64;
                let c = &mut cat[k];
                c.snapshots += t[k].snapshots;
                c.fresh += t[k].fresh;
                c.rows += t[k].rows;
                c.time += t[k].time;
                c.stale += t[k].stale;
                c.stale_inputs += t[k].stale_inputs;
            }
            if stale[2] {
                stale_names.push(format!("{} ({})", inp.name, if small { "small reads" } else { "4096" }));
            }
        }
    }
    println!("| Inputs | Mode | Snapshots | From a new render state | Rows copied per snapshot | Time per snapshot | Stale snapshots (runs) |");
    println!("|---|---|---|---|---|---|---|");
    let mut total: [T; 3] = Default::default();
    for (cat, t) in &by {
        for k in 0..3 {
            let x = &t[k];
            let tt = &mut total[k];
            tt.snapshots += x.snapshots;
            tt.fresh += x.fresh;
            tt.rows += x.rows;
            tt.time += x.time;
            tt.stale += x.stale;
            tt.stale_inputs += x.stale_inputs;
        }
        for (k, m) in modes.iter().enumerate() {
            row(cat, *m, &t[k]);
        }
    }
    for (k, m) in modes.iter().enumerate() {
        row("all", *m, &total[k]);
    }
    fn row(cat: &str, m: Refresh, x: &T) {
        let per = |v: u64| if x.snapshots == 0 { 0.0 } else { v as f64 / x.snapshots as f64 };
        println!(
            "| {cat} | {m:?} | {} | {} ({:.1} %) | {:.2} | {:.1} µs | {} ({}) |",
            x.snapshots,
            x.fresh,
            per(x.fresh) * 100.0,
            per(x.rows),
            if x.snapshots == 0 { 0.0 } else { x.time.as_secs_f64() * 1e6 / x.snapshots as f64 },
            x.stale,
            x.stale_inputs
        );
    }
    if !stale_names.is_empty() {
        println!("\nStale with Always (the default): {}", stale_names.join(", "));
        std::process::exit(1);
    }
}

/// `--colors`: colour specifications and palette entries parsed by haetty
/// (`haetty_vt::color`, what themes and OSC use) and by libghostty-vt's
/// `RgbColor::parse` / `Palette::parse_palette_entry`, compared: every X11
/// name in several spellings, the hex, `rgb:` and `rgbi:` forms, junk, and
/// every value of every built-in theme file.
fn colors() {
    use libghostty_vt::style::{Palette, RgbColor};
    let mut specs: Vec<String> = Vec::new();
    let rgb_txt = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../haetty-vt/src/x11_colors.rs"))
        .unwrap_or_default();
    for line in rgb_txt.lines() {
        if let Some(n) = line.trim().strip_prefix("(\"").and_then(|l| l.split('"').next()) {
            specs.push(n.into());
            specs.push(n.to_uppercase());
            specs.push(format!(" \t{n} "));
            let mixed: String =
                n.chars().enumerate().map(|(i, c)| if i % 2 == 0 { c.to_ascii_uppercase() } else { c }).collect();
            specs.push(mixed);
        }
    }
    let hexes = ["0", "7", "a", "F", "1e", "80", "ff", "abc", "801", "fff", "0000", "8001", "ffff"];
    for d in [1usize, 2, 3, 4] {
        for h in hexes.iter().filter(|h| h.len() == d) {
            specs.push(format!("#{}", h.repeat(3)));
            specs.push(format!("rgb:{h}/{h}/{h}"));
            specs.push(format!("rgb:{h}/0/{h}"));
            if d == 1 || d == 2 {
                specs.push(h.repeat(3));
            }
        }
    }
    for v in ["0", "1", "0.5", "0.25", "0.999", "1.5", "-0.1", "", "x", ".5"] {
        specs.push(format!("rgbi:{v}/{v}/{v}"));
    }
    for j in [
        "",
        " ",
        "#",
        "#ff",
        "#fffff",
        "#ffffffff",
        "rgb:",
        "rgb:1/2",
        "rgb:1/2/3/4",
        "rgb:12345/0/0",
        "rgba:1/2/3",
        "#ggg",
        "red ",
        "nosuchcolor",
        "rgb:/1/2",
        "１２３",
    ] {
        specs.push(j.into());
    }
    let mut entries: Vec<String> = Vec::new();
    for n in ["0", "15", "16", "255", "256", "0x0f", "0o17", "0b1111", " 3 ", "-1", "x", ""] {
        for c in ["#123456", "red", "rgb:1/2/3", "zz"] {
            entries.push(format!("{n}={c}"));
        }
    }
    entries.push("no equals".into());
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/themes");
    let mut theme_lines = 0;
    for f in std::fs::read_dir(dir).map(|d| d.flatten().collect::<Vec<_>>()).unwrap_or_default() {
        let text = std::fs::read_to_string(f.path()).unwrap_or_default();
        for line in text.lines() {
            if let Some((k, v)) = line.split_once('=') {
                let (k, v) = (k.trim(), v.trim());
                theme_lines += 1;
                if k == "palette" {
                    entries.push(v.into());
                } else {
                    specs.push(v.into());
                }
            }
        }
    }
    let (mut same, mut diff) = (0, Vec::new());
    for s in &specs {
        let a = RgbColor::parse(s).ok().map(|c| [c.r, c.g, c.b]);
        let b = haetty_vt::parse_color(s);
        if a == b {
            same += 1;
        } else {
            diff.push(format!("colour {s:?}: libghostty {a:?} / haetty {b:?}"));
        }
    }
    for e in &entries {
        let a = Palette::parse_palette_entry(e).ok().map(|(i, c)| (i.0, [c.r, c.g, c.b]));
        let b = haetty_vt::parse_palette_entry(e);
        if a == b {
            same += 1;
        } else {
            diff.push(format!("palette {e:?}: libghostty {a:?} / haetty {b:?}"));
        }
    }
    println!(
        "colours: {} specifications, {} palette entries ({theme_lines} theme lines included): {same} identical, {} differ",
        specs.len(),
        entries.len(),
        diff.len()
    );
    for d in diff.iter().take(40) {
        println!("- {d}");
    }
    std::process::exit(if diff.is_empty() { 0 } else { 1 });
}

/// `--encoders`: what each engine sends for keys, mouse, paste and focus,
/// compared byte for byte (DESIGN.md §10.7). Both engines get the same
/// modes first (fed as escape sequences), then the same inputs.
fn encoders(report: Option<String>) {
    use haetty_core::{Key, KeyAction, KeyInput, Mods, MouseAction, MouseButton, MouseInput};
    struct Cat {
        name: &'static str,
        cases: u64,
        equal: u64,
        firsts: Vec<String>,
    }
    let mut cats: Vec<Cat> = Vec::new();
    let mut tally = |cat: &str, what: String, a: &[u8], b: &[u8]| {
        let i = match cats.iter().position(|c| c.name == cat) {
            Some(i) => i,
            None => {
                let name: &'static str = Box::leak(cat.to_string().into_boxed_str());
                cats.push(Cat { name, cases: 0, equal: 0, firsts: vec![] });
                cats.len() - 1
            }
        };
        let c = &mut cats[i];
        c.cases += 1;
        if a == b {
            c.equal += 1;
        } else if c.firsts.len() < 6 {
            c.firsts.push(format!("{what}: reference {} / native {}", show(a), show(b)));
        }
    };
    let engines = |setup: &[u8]| {
        let mut a = GhosttyEngine::new(&init(80, 24)).expect("reference engine");
        let mut b = NativeEngine::new(&init(80, 24));
        a.feed(setup);
        b.feed(setup);
        let _ = (a.take_replies(), b.take_replies());
        (a, b)
    };
    let mod_sets: Vec<Mods> = (0..16u16)
        .map(|i| {
            let mut m = Mods::empty();
            for (bit, f) in [(1, Mods::SHIFT), (2, Mods::CTRL), (4, Mods::ALT), (8, Mods::SUPER)] {
                if i & bit != 0 {
                    m |= f;
                }
            }
            m
        })
        .collect();
    // A key's text on a US layout, as a frontend gives it.
    let text_of = |k: Key, m: Mods| -> Option<(String, char)> {
        let s = format!("{k:?}");
        let base = if s.len() == 1 {
            s.chars().next()?.to_ascii_lowercase()
        } else if let Some(d) = s.strip_prefix("Digit") {
            d.chars().next()?
        } else {
            match k {
                Key::Space => ' ',
                Key::Minus => '-',
                Key::Equal => '=',
                Key::Comma => ',',
                Key::Period => '.',
                Key::Slash => '/',
                Key::Semicolon => ';',
                Key::Quote => '\'',
                Key::Backquote => '`',
                Key::BracketLeft => '[',
                Key::BracketRight => ']',
                Key::Backslash => '\\',
                _ => return None,
            }
        };
        let shifted =
            if m.contains(Mods::SHIFT) && base.is_ascii_lowercase() { base.to_ascii_uppercase() } else { base };
        Some((shifted.to_string(), base))
    };
    // Keys: legacy modes, then every kitty flag set.
    let mut setups: Vec<(String, Vec<u8>)> = vec![
        ("legacy".into(), vec![]),
        ("legacy DECCKM".into(), b"\x1b[?1h".to_vec()),
        ("legacy keypad".into(), b"\x1b[?66h\x1b[?1035l".to_vec()),
        ("legacy modifyOtherKeys 2".into(), b"\x1b[>4;2m".to_vec()),
        ("legacy DECBKM".into(), b"\x1b[?67h".to_vec()),
        ("legacy no alt prefix".into(), b"\x1b[?1036l".to_vec()),
    ];
    for f in 1..32u8 {
        setups.push((format!("kitty {f}"), format!("\x1b[={f};1u").into_bytes()));
    }
    for (label, setup) in &setups {
        let cat =
            if label.starts_with("kitty") { "keys (kitty flags 1–31)" } else { "keys (legacy, modifyOtherKeys)" };
        let (mut a, mut b) = engines(setup);
        for k in Key::ALL {
            for m in &mod_sets {
                for act in [KeyAction::Press, KeyAction::Repeat, KeyAction::Release] {
                    for with_text in [false, true] {
                        let mut ev = KeyInput { action: act, mods: *m, ..KeyInput::press(k) };
                        if with_text {
                            match text_of(k, *m) {
                                Some((t, base)) => {
                                    ev.text = Some(t);
                                    ev.unshifted = Some(base);
                                    if m.contains(Mods::SHIFT) {
                                        ev.consumed_mods = Mods::SHIFT;
                                    }
                                }
                                None => continue,
                            }
                        }
                        let (mut x, mut y) = (vec![], vec![]);
                        a.encode_key(&ev, &mut x);
                        b.encode_key(&ev, &mut y);
                        tally(cat, format!("{label} {k:?} {m:?} {act:?} text {:?}", ev.text), &x, &y);
                    }
                }
            }
        }
    }
    // Mouse: every reporting mode and format; per button and action, a
    // press, a motion in the same cell, a motion to another, a release.
    let events =
        [("off", ""), ("9", "\x1b[?9h"), ("1000", "\x1b[?1000h"), ("1002", "\x1b[?1002h"), ("1003", "\x1b[?1003h")];
    let formats = [
        ("x10", ""),
        ("1005", "\x1b[?1005h"),
        ("1006", "\x1b[?1006h"),
        ("1015", "\x1b[?1015h"),
        ("1016", "\x1b[?1016h"),
    ];
    let gs = GridSize::new(80, 24).with_cell(9, 18);
    let wide = GridSize::new(300, 24).with_cell(9, 18);
    for (en, es) in events {
        for (fname, fs) in formats {
            let setup = format!("{es}{fs}");
            for size in [gs, wide] {
                let (mut a, mut b) = engines(setup.as_bytes());
                for button in [
                    None,
                    Some(MouseButton::Left),
                    Some(MouseButton::Middle),
                    Some(MouseButton::Right),
                    Some(MouseButton::Four),
                    Some(MouseButton::Five),
                    Some(MouseButton::Six),
                    Some(MouseButton::Seven),
                    Some(MouseButton::Eight),
                    Some(MouseButton::Nine),
                    Some(MouseButton::Ten),
                ] {
                    for mods in
                        [Mods::empty(), Mods::SHIFT, Mods::ALT, Mods::CTRL, Mods::SHIFT | Mods::CTRL | Mods::ALT]
                    {
                        let xs: [f32; 6] = [4.5, 13.0, 2010.0, 2700.0, -3.0, 18200.0];
                        for (i, x) in xs.iter().enumerate() {
                            let y = [9.0f32, 30.0, 200.0, 420.0, 431.0, -1.0][i];
                            for (act, pressed, dx) in [
                                (MouseAction::Press, true, 0.0),
                                (MouseAction::Motion, true, 1.0),
                                (MouseAction::Motion, true, 40.0),
                                (MouseAction::Release, false, 40.0),
                                (MouseAction::Motion, false, 90.0),
                            ] {
                                let ev = MouseInput { action: act, button, mods, x: x + dx, y, any_pressed: pressed };
                                let (mut p, mut q) = (vec![], vec![]);
                                a.encode_mouse(&ev, size, &mut p);
                                b.encode_mouse(&ev, size, &mut q);
                                tally(
                                    "mouse",
                                    format!(
                                        "{en}/{fname} {}x {button:?} {mods:?} {act:?} at {:.0},{y:.0}",
                                        size.cols, ev.x
                                    ),
                                    &p,
                                    &q,
                                );
                            }
                        }
                    }
                }
            }
        }
    }
    // Paste and focus.
    let pastes = [
        "plain",
        "two\nlines\r\n",
        "tab\there",
        "esc\x1b[201~inside",
        "ctrl \x03\x04\x08\x7f\x15\x1a end",
        "한글 😀",
        "",
    ];
    for setup in ["", "\x1b[?2004h"] {
        let (mut a, mut b) = engines(setup.as_bytes());
        for p in pastes {
            let (mut x, mut y) = (vec![], vec![]);
            a.encode_paste(p, &mut x);
            b.encode_paste(p, &mut y);
            tally("paste", format!("{} {p:?}", if setup.is_empty() { "plain" } else { "bracketed" }), &x, &y);
        }
        let (mut a, mut b) = engines(if setup.is_empty() { b"" } else { b"\x1b[?1004h" });
        for f in [true, false] {
            let (mut x, mut y) = (vec![], vec![]);
            a.encode_focus(f, &mut x);
            b.encode_focus(f, &mut y);
            tally("focus", format!("{} {f}", if setup.is_empty() { "off" } else { "1004" }), &x, &y);
        }
    }
    let pct = |n: u64, d: u64| if d == 0 { 100.0 } else { n as f64 * 100.0 / d as f64 };
    let mut out = String::from("# vtdiff --encoders: libghostty-vt (reference) vs haetty's encoders (native)\n\n| Category | Cases | Identical |\n|---|---|---|\n");
    for c in &cats {
        let _ = writeln!(out, "| {} | {} | {} ({:.2} %) |", c.name, c.cases, c.equal, pct(c.equal, c.cases));
    }
    for c in cats.iter().filter(|c| !c.firsts.is_empty()) {
        let _ = writeln!(out, "\n## First differences: {}\n", c.name);
        for f in &c.firsts {
            let _ = writeln!(out, "- {f}");
        }
    }
    print!("{out}");
    if let Some(p) = report {
        let _ = std::fs::write(p, &out);
    }
    std::process::exit(if cats.iter().all(|c| c.cases == c.equal) { 0 } else { 1 });
}

/// `--gen-keys FILE`: haetty-core's key and mouse types (`keys.rs`), the
/// same variants and numbering as libghostty-vt's, so the wire format and the
/// conversions in `ghostty.rs` stay plain casts (DESIGN.md §10.4).
fn gen_keys(path: &str) {
    use libghostty_vt::{key, mouse};
    use std::fmt::Write as _;
    fn variants<T: std::fmt::Debug>(get: impl Fn(u32) -> Option<T>) -> Vec<(u32, String)> {
        (0..1024u32).filter_map(|v| get(v).map(|t| (v, format!("{t:?}")))).collect()
    }
    fn enum_src(out: &mut String, doc: &str, name: &str, vs: &[(u32, String)]) {
        let _ = writeln!(out, "/// {doc}");
        let _ = writeln!(out, "#[repr(u32)]\n#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]\npub enum {name} {{");
        for (v, n) in vs {
            let _ = writeln!(out, "    {n} = {v},");
        }
        let _ = writeln!(
            out,
            "}}\n\nimpl {name} {{\n    /// Every variant, in numbering order.\n    pub const ALL: [{name}; {}] = [",
            vs.len()
        );
        for (_, n) in vs {
            let _ = writeln!(out, "        {name}::{n},");
        }
        let _ = writeln!(out, "    ];\n}}\n\nimpl TryFrom<u32> for {name} {{\n    type Error = u32;\n    fn try_from(v: u32) -> Result<Self, u32> {{\n        match v {{");
        for (v, n) in vs {
            let _ = writeln!(out, "            {v} => Ok({name}::{n}),");
        }
        let _ = writeln!(out, "            _ => Err(v),\n        }}\n    }}\n}}\n");
    }
    let mut out = String::from(
        "//! Key and mouse types with libghostty-vt 0.2.2's variants and numbering, generated by\n\
         //! `haetty-vtdiff --gen-keys` (DESIGN.md §10.4): haetty-core owns them, so a build without\n\
         //! libghostty (feature `native` only) has them too. Do not edit.\n\n",
    );
    enum_src(
        &mut out,
        "A physical key (W3C UI Events KeyboardEvent code).",
        "Key",
        &variants(|v| key::Key::try_from(v).ok()),
    );
    enum_src(&mut out, "Key event action.", "KeyAction", &variants(|v| key::Action::try_from(v).ok()));
    enum_src(&mut out, "Mouse event action.", "MouseAction", &variants(|v| mouse::Action::try_from(v).ok()));
    enum_src(&mut out, "Mouse button.", "MouseButton", &variants(|v| mouse::Button::try_from(v).ok()));
    let mods = [
        ("SHIFT", key::Mods::SHIFT),
        ("ALT", key::Mods::ALT),
        ("CTRL", key::Mods::CTRL),
        ("SUPER", key::Mods::SUPER),
        ("CAPS_LOCK", key::Mods::CAPS_LOCK),
        ("NUM_LOCK", key::Mods::NUM_LOCK),
        ("SHIFT_SIDE", key::Mods::SHIFT_SIDE),
        ("ALT_SIDE", key::Mods::ALT_SIDE),
        ("CTRL_SIDE", key::Mods::CTRL_SIDE),
        ("SUPER_SIDE", key::Mods::SUPER_SIDE),
    ];
    let kitty = [
        ("DISABLED", key::KittyKeyFlags::DISABLED),
        ("DISAMBIGUATE", key::KittyKeyFlags::DISAMBIGUATE),
        ("REPORT_EVENTS", key::KittyKeyFlags::REPORT_EVENTS),
        ("REPORT_ALTERNATES", key::KittyKeyFlags::REPORT_ALTERNATES),
        ("REPORT_ALL", key::KittyKeyFlags::REPORT_ALL),
        ("REPORT_ASSOCIATED", key::KittyKeyFlags::REPORT_ASSOCIATED),
        ("ALL", key::KittyKeyFlags::ALL),
    ];
    let _ = writeln!(out, "bitflags::bitflags! {{\n    /// Keyboard modifiers.\n    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]\n    pub struct Mods: u16 {{");
    for (n, m) in mods {
        let _ = writeln!(out, "        const {n} = {:#06x};", m.bits());
    }
    let _ = writeln!(out, "    }}\n\n    /// Kitty keyboard protocol flags.\n    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]\n    pub struct KittyKeyFlags: u8 {{");
    for (n, m) in kitty {
        let _ = writeln!(out, "        const {n} = {:#04x};", m.bits());
    }
    let _ = writeln!(out, "    }}\n}}");
    std::fs::write(path, out).expect("write");
    println!("{path}: written");
}

/// `--gen-width FILE`: the width libghostty-vt gives every code point, as the
/// range tables of `haetty-vt` (DESIGN.md §10.2). Each character is written
/// after an `a` on a cleared line; the cursor then says its width (column 1:
/// zero, 2: one, 3: two).
fn gen_width(path: &str) {
    let mut e = GhosttyEngine::new(&init(8, 2)).expect("reference engine");
    let mut zero: Vec<(u32, u32)> = Vec::new();
    let mut wide: Vec<(u32, u32)> = Vec::new();
    let mut vs_base: Vec<(u32, u32)> = Vec::new();
    let add = |v: &mut Vec<(u32, u32)>, c: u32| match v.last_mut() {
        Some(r) if r.1 + 1 == c => r.1 = c,
        _ => v.push((c, c)),
    };
    let mut buf = Vec::with_capacity(16);
    for cp in 0x20u32..=0x10ffff {
        if (0x7f..=0x9f).contains(&cp) {
            continue;
        }
        let Some(ch) = char::from_u32(cp) else { continue };
        buf.clear();
        buf.extend_from_slice(b"\r\x1b[2Ka");
        let mut u = [0u8; 4];
        buf.extend_from_slice(ch.encode_utf8(&mut u).as_bytes());
        e.feed(&buf);
        let _ = e.take_replies();
        let _ = e.take_events();
        let x = e.snapshot().cursor.map_or(2, |c| c.x);
        match x {
            1 => add(&mut zero, cp),
            3 => add(&mut wide, cp),
            _ => {}
        }
        if x != 1 {
            // Does a variation selector after it stay in its cell?
            buf.clear();
            buf.extend_from_slice(b"\r\x1b[2K");
            buf.extend_from_slice(ch.encode_utf8(&mut u).as_bytes());
            buf.extend_from_slice("\u{fe0f}".as_bytes());
            e.feed(&buf);
            if e.snapshot().cell(0, 0).is_some_and(|c| c.text.ends_with('\u{fe0f}')) {
                add(&mut vs_base, cp);
            }
        }
    }
    let table = |v: &[(u32, u32)]| v.iter().map(|(a, b)| format!("    (0x{a:x}, 0x{b:x}),\n")).collect::<String>();
    let out = format!(
        "//! Cell widths as libghostty-vt 0.2.2 gives them, generated by `haetty-vtdiff --gen-width` (DESIGN.md §10.2):\n\
         //! every code point written after an `a` on a cleared line, its width read from the cursor. Do not edit.\n\n\
         /// Code points that take no cell of their own (inclusive ranges).\n\
         pub const ZERO: &[(u32, u32)] = &[\n{}];\n\n\
         /// Code points that take two cells (inclusive ranges).\n\
         pub const WIDE: &[(u32, u32)] = &[\n{}];\n\n\
         /// Code points a variation selector (U+FE0E, U+FE0F) can follow; after others it is dropped.\n\
         pub const VS_BASE: &[(u32, u32)] = &[\n{}];\n",
        table(&zero),
        table(&wide),
        table(&vs_base)
    );
    std::fs::write(path, out).expect("write");
    println!(
        "{path}: {} zero-width ranges, {} wide ranges, {} variation-selector bases",
        zero.len(),
        wide.len(),
        vs_base.len()
    );
}

/// One step of a minimised input: a byte, or a resize.
#[derive(Clone, Copy)]
enum Ev {
    B(u8),
    R(u16, u16),
}

/// The engines differ after these steps (from 80x24).
fn differs(evs: &[Ev]) -> bool {
    let mut a = GhosttyEngine::new(&init(80, 24)).expect("reference engine");
    let _ = a.take_replies();
    let mut b = NativeEngine::new(&init(80, 24));
    let (mut ra, mut rb) = (Vec::new(), Vec::new());
    let mut i = 0;
    while i < evs.len() {
        match evs[i] {
            Ev::B(_) => {
                let mut run = Vec::new();
                while let Some(Ev::B(c)) = evs.get(i) {
                    run.push(*c);
                    i += 1;
                }
                a.feed(&run);
                b.feed(&run);
            }
            Ev::R(c, r) => {
                let gs = GridSize::new(c, r).with_cell(9, 18);
                a.resize(gs);
                b.resize(gs);
                i += 1;
            }
        }
        ra.extend(a.take_replies());
        rb.extend(b.take_replies());
    }
    let (sa, sb) = (a.snapshot_fresh(), b.snapshot());
    !compare(&sa, &sb, a.scrollback_rows(), b.scrollback_rows(), &ra, &rb).is_empty()
}

/// Delta debugging: drops ever smaller pieces while the engines still differ.
fn minimize(mut v: Vec<Ev>) -> Vec<Ev> {
    if !differs(&v) {
        return v;
    }
    let mut n = v.len() / 2;
    while n >= 1 {
        let mut i = 0;
        let mut changed = false;
        while i < v.len() {
            let mut c = v.clone();
            c.drain(i..(i + n).min(v.len()));
            if differs(&c) {
                v = c;
                changed = true;
            } else {
                i += n;
            }
        }
        if !changed {
            n /= 2;
        }
    }
    v
}

/// Steps as text: bytes escaped, resizes as `<resize CxR>`.
fn show_evs(evs: &[Ev]) -> String {
    let mut out = String::new();
    let mut run = Vec::new();
    for e in evs {
        match e {
            Ev::B(c) => run.push(*c),
            Ev::R(c, r) => {
                out.push_str(&show_all(&std::mem::take(&mut run)));
                let _ = write!(out, "<resize {c}x{r}>");
            }
        }
    }
    out.push_str(&show_all(&run));
    out
}

/// Like `show`, without the length cap.
fn show_all(b: &[u8]) -> String {
    b.chunks(160).map(show).collect()
}

/// What `--diverge` appends to a prefix to read the engines' hidden state:
/// queries both engines answer (cursor, DECRQSS for the pen, margins, cursor
/// style and protection, DECRQM for the modes, the saved cursor, tab stops),
/// then a print (pen, charset, wrap, insert mode on screen). Every query
/// starts with ESC: a prefix is cut only before an ESC of the input, so the
/// parser goes on as it would have.
fn state_probe() -> Vec<u8> {
    let mut p: Vec<u8> = b"\x1b[6n\x1b[?6n".to_vec();
    for q in ["m", "r", "s", " q", "\"q"] {
        p.extend_from_slice(format!("\x1bP$q{q}\x1b\\").as_bytes());
    }
    for m in [4u16, 20] {
        p.extend_from_slice(format!("\x1b[{m}$p").as_bytes());
    }
    for m in [
        1u16, 5, 6, 7, 12, 25, 45, 47, 66, 67, 69, 1000, 1002, 1003, 1004, 1005, 1006, 1015, 1016, 1035, 1036, 1047,
        1048, 1049, 2004, 2026, 2027, 2031, 2048,
    ] {
        p.extend_from_slice(format!("\x1b[?{m}$p").as_bytes());
    }
    // The saved cursor (DECRC), the tab stops of its row, then a print.
    p.extend_from_slice(b"\x1b8\x1b[6n\x1b[1G\t\x1b[6n\t\x1b[6n\t\x1b[6n\x1b8a`q");
    p
}

/// The engines after `evs` (from 80x24), then `extra`: the first difference
/// (field, place, both values), or `None`.
fn first_difference(evs: &[Ev], extra: &[u8]) -> Option<(String, String, String, String)> {
    let mut a = GhosttyEngine::new(&init(80, 24)).expect("reference engine");
    let _ = a.take_replies();
    let mut b = NativeEngine::new(&init(80, 24));
    let (mut ra, mut rb) = (Vec::new(), Vec::new());
    let mut run = Vec::new();
    for e in evs.iter().chain(std::iter::once(&Ev::R(0, 0))) {
        match *e {
            Ev::B(c) => run.push(c),
            Ev::R(c, r) => {
                a.feed(&run);
                b.feed(&run);
                run.clear();
                if c > 0 {
                    let gs = GridSize::new(c, r).with_cell(9, 18);
                    a.resize(gs);
                    b.resize(gs);
                }
                ra.extend(a.take_replies());
                rb.extend(b.take_replies());
            }
        }
    }
    let n = (ra.len(), rb.len());
    a.feed(extra);
    b.feed(extra);
    ra.extend(a.take_replies());
    rb.extend(b.take_replies());
    let (sa, sb) = (a.snapshot_fresh(), b.snapshot());
    let d = compare(&sa, &sb, a.scrollback_rows(), b.scrollback_rows(), &ra, &rb);
    d.into_iter().next().map(|d| {
        // For replies, the probe's part only when the input's own agreed.
        let (x, y) = if d.field == "replies" && ra[..n.0] == rb[..n.1] {
            (show(&ra[n.0..]), show(&rb[n.1..]))
        } else {
            (d.reference, d.native)
        };
        (d.field.to_string(), d.at, x, y)
    })
}

/// `--diverge NAME`: where the named input's engines first part, hidden or
/// not (development aid for inputs whose minimal form stays long). Feeds
/// the input up to its first visible difference, then cuts it before each
/// ESC and asks both engines about their state ([`state_probe`]): the first
/// cut where the answers differ is where the states parted. Then minimises
/// the input up to there, under "the answers differ".
fn diverge(inp: &Input) {
    let mut evs: Vec<Ev> = Vec::new();
    for step in &inp.steps {
        match step {
            Step::Bytes(v) => evs.extend(v.iter().map(|&c| Ev::B(c))),
            Step::Resize(c, r) => evs.push(Ev::R(*c, *r)),
        }
    }
    // The first visible difference (as --bisect, in steps of 64 then 1).
    let visible = |k: usize| first_difference(&evs[..k], b"").is_some();
    let mut hi = None;
    let mut k = 0;
    while k < evs.len() {
        k = (k + 64).min(evs.len());
        if visible(k) {
            hi = Some(k);
            break;
        }
    }
    let Some(mut hi) = hi else {
        println!("{}: no difference", inp.name);
        return;
    };
    while hi > 0 && visible(hi - 1) {
        hi -= 1;
    }
    println!("{}: visible after step {hi} of {}", inp.name, evs.len());
    let probe = state_probe();
    let cuts: Vec<usize> = (0..=hi).filter(|&k| k == hi || matches!(evs.get(k), Some(Ev::B(0x1b)))).collect();
    let hidden = cuts.iter().copied().find(|&k| first_difference(&evs[..k], &probe).is_some()).unwrap_or(hi);
    let (field, at, x, y) = first_difference(&evs[..hidden], &probe).unwrap_or_default();
    println!(
        "{}: states part by step {hidden} ({} steps before it shows): {field} {at} — reference {x} / native {y}",
        inp.name,
        hi - hidden
    );
    println!("steps before: {}", show_evs(&evs[hidden.saturating_sub(160)..hidden]));
    let mut v = evs[..hidden].to_vec();
    let probed = |c: &[Ev]| first_difference(c, &probe).is_some();
    let mut n = v.len() / 2;
    while n >= 1 {
        let (mut i, mut changed) = (0, false);
        while i < v.len() {
            let mut c = v.clone();
            c.drain(i..(i + n).min(v.len()));
            if probed(&c) {
                v = c;
                changed = true;
            } else {
                i += n;
            }
        }
        if !changed {
            n /= 2;
        }
    }
    let (field, at, x, y) = first_difference(&v, &probe).unwrap_or_default();
    println!("minimal ({} steps, + probe): {}", v.len(), show_evs(&v));
    println!("  probe answer: {field} {at} — reference {x} / native {y}");
    let plain = first_difference(&v, b"");
    println!(
        "  without the probe: {}",
        plain.map_or("identical".into(), |d| format!("{} {} — {} / {}", d.0, d.1, d.2, d.3))
    );
}

/// The engines' dumps (plain, VT, HTML) after `evs` (from 80x24): the first
/// that differs, as text.
fn dump_difference(evs: &[Ev]) -> Option<String> {
    let mut a = GhosttyEngine::new(&init(80, 24)).expect("reference engine");
    let mut b = NativeEngine::new(&init(80, 24));
    let mut run = Vec::new();
    for e in evs.iter().chain(std::iter::once(&Ev::R(0, 0))) {
        match *e {
            Ev::B(c) => run.push(c),
            Ev::R(c, r) => {
                a.feed(&run);
                b.feed(&run);
                run.clear();
                if c > 0 {
                    let gs = GridSize::new(c, r).with_cell(9, 18);
                    a.resize(gs);
                    b.resize(gs);
                }
            }
        }
    }
    dumps(&mut a, &mut b)
        .into_iter()
        .zip(DUMPS)
        .find_map(|(d, (n, _))| d.map(|d| format!("{n} [{}] {}", d.cause, d.text)))
}

/// `--bisect-dump NAME`: the shortest prefix of the named input after which
/// the dumps differ (binary search over prefixes), then that prefix
/// minimised under "the dumps differ" (development aid for differences the
/// screen fields do not show, e.g. links in the scrollback).
fn bisect_dump(inp: &Input) {
    let mut evs: Vec<Ev> = Vec::new();
    for step in &inp.steps {
        match step {
            Step::Bytes(v) => evs.extend(v.iter().map(|&c| Ev::B(c))),
            Step::Resize(c, r) => evs.push(Ev::R(*c, *r)),
        }
    }
    if dump_difference(&evs).is_none() {
        println!("{}: dumps identical", inp.name);
        return;
    }
    let (mut lo, mut hi) = (0, evs.len());
    while lo + 1 < hi {
        let mid = (lo + hi) / 2;
        if dump_difference(&evs[..mid]).is_some() {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    println!("{}: dumps differ after step {hi} of {}", inp.name, evs.len());
    println!("steps before: {}", show_evs(&evs[hi.saturating_sub(200)..hi]));
    let mut v = evs[..hi].to_vec();
    let mut n = v.len() / 2;
    while n >= 1 {
        let (mut i, mut changed) = (0, false);
        while i < v.len() {
            let mut c = v.clone();
            c.drain(i..(i + n).min(v.len()));
            if dump_difference(&c).is_some() {
                v = c;
                changed = true;
            } else {
                i += n;
            }
        }
        if !changed {
            n /= 2;
        }
    }
    println!("minimal ({} steps): {}", v.len(), show_evs(&v));
    println!("  {}", dump_difference(&v).unwrap_or_default());
}

/// `--bisect NAME`: feeds the named input one byte at a time and stops at the
/// first byte after which the engines differ (development aid): prints the
/// difference and the bytes that led to it.
fn bisect(inp: &Input) {
    let mut a = GhosttyEngine::new(&init(80, 24)).expect("reference engine");
    let _ = a.take_replies();
    let mut b = NativeEngine::new(&init(80, 24));
    let mut fed: Vec<u8> = Vec::new();
    let mut evs: Vec<Ev> = Vec::new();
    let (mut ra, mut rb) = (Vec::new(), Vec::new());
    for step in &inp.steps {
        let bytes: Vec<Vec<u8>> = match step {
            Step::Bytes(v) => v.iter().map(|&c| vec![c]).collect(),
            Step::Resize(c, r) => {
                let gs = GridSize::new(*c, *r).with_cell(9, 18);
                a.resize(gs);
                b.resize(gs);
                evs.push(Ev::R(*c, *r));
                vec![vec![]]
            }
        };
        for one in bytes {
            a.feed(&one);
            b.feed(&one);
            fed.extend_from_slice(&one);
            evs.extend(one.iter().map(|&c| Ev::B(c)));
            ra.extend(a.take_replies());
            rb.extend(b.take_replies());
            let (sa, sb) = (a.snapshot_fresh(), b.snapshot());
            let d = compare(&sa, &sb, a.scrollback_rows(), b.scrollback_rows(), &ra, &rb);
            if let Some(d) = d.first() {
                println!(
                    "{}: first difference after byte {}: {} {} — reference {} / native {}",
                    inp.name,
                    fed.len(),
                    d.field,
                    d.at,
                    d.reference,
                    d.native
                );
                println!("bytes before: {}", show(&fed[fed.len().saturating_sub(120)..]));
                let m = minimize(evs);
                println!("minimal input ({} steps): {}", m.len(), show_evs(&m));
                return;
            }
        }
    }
    println!("{}: no difference", inp.name);
}

fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    if raw.first().map(String::as_str) == Some("--colors") {
        colors();
    }
    if raw.first().map(String::as_str) == Some("--encoders") {
        encoders(raw.get(1).filter(|a| *a == "--report").and(raw.get(2)).cloned());
        return;
    }
    if raw.first().map(String::as_str) == Some("--gen-keys") {
        gen_keys(raw.get(1).map_or("keys.rs", String::as_str));
        return;
    }
    if raw.first().map(String::as_str) == Some("--gen-width") {
        gen_width(raw.get(1).map_or("width_table.rs", String::as_str));
        return;
    }
    if raw.first().map(String::as_str) == Some("--probe") {
        let cols = raw.get(2).and_then(|v| v.parse().ok()).unwrap_or(20);
        let rows = raw.get(3).and_then(|v| v.parse().ok()).unwrap_or(5);
        probe(&unescape(raw.get(1).map_or("", String::as_str)), cols, rows);
        return;
    }
    let mut args = raw.into_iter();
    let (mut corpus, mut nfuzz, mut chunk, mut every, mut report) = (None, 20u64, 4096usize, 1024u64, None);
    let mut bisect_name: Option<String> = None;
    let mut diverge_name: Option<String> = None;
    let mut bisect_dump_name: Option<String> = None;
    let mut dump: Option<(String, usize)> = None;
    let mut refresh_only = false;
    let docs = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/vtdiff");
    let (mut known_path, mut cand_path) =
        (format!("{docs}/known-vt-differences.txt"), format!("{docs}/known-candidates.txt"));
    while let Some(a) = args.next() {
        match a.as_str() {
            "--corpus" => corpus = args.next(),
            "--fuzz" => nfuzz = args.next().and_then(|v| v.parse().ok()).unwrap_or(20),
            "--chunk" => chunk = args.next().and_then(|v| v.parse().ok()).unwrap_or(4096),
            "--every" => every = args.next().and_then(|v| v.parse().ok()).unwrap_or(1024),
            "--report" => report = args.next(),
            "--refresh" => refresh_only = true,
            "--known" => known_path = args.next().unwrap_or_default(),
            "--candidates" => cand_path = args.next().unwrap_or_default(),
            "--bisect" => bisect_name = args.next(),
            "--diverge" => diverge_name = args.next(),
            "--bisect-dump" => bisect_dump_name = args.next(),
            "--dump" => {
                let n = args.next().unwrap_or_default();
                let len = args.next().and_then(|v| v.parse().ok()).unwrap_or(usize::MAX);
                dump = Some((n, len));
            }
            _ => {
                eprintln!("usage: vtdiff [--corpus DIR] [--fuzz N] [--chunk BYTES] [--every BYTES] [--report FILE] [--bisect NAME]");
                std::process::exit(2);
            }
        }
    }
    let mut inputs: Vec<Input> = Vec::new();
    if let Some(dir) = &corpus {
        let mut files: Vec<_> =
            std::fs::read_dir(dir).map(|d| d.flatten().map(|e| e.path()).collect()).unwrap_or_default();
        files.sort();
        for p in files {
            if let Ok(b) = std::fs::read(&p) {
                let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                inputs.push(Input { name, category: "corpus".into(), steps: vec![Step::Bytes(b)] });
            }
        }
    }
    inputs.extend((1..=3).map(synthetic));
    inputs.push(Input {
        name: "claude-like".into(),
        category: "synthetic".into(),
        steps: vec![Step::Bytes(vtgen::claude_like())],
    });
    inputs.extend(scenarios());
    inputs.extend(resizes());
    inputs.extend(joiners());
    inputs.extend((1..=nfuzz).map(fuzz));
    inputs.extend((1..=nfuzz).map(fuzz2));
    inputs.extend((1..=nfuzz).map(grammar));
    inputs.extend((1..=nfuzz).map(decor));
    let other: Vec<u8> = match inputs.iter().find(|i| i.category == "corpus") {
        Some(Input { steps, .. }) => match steps.first() {
            Some(Step::Bytes(b)) => b[..b.len().min(64 << 10)].to_vec(),
            _ => vec![],
        },
        None => vec![],
    };
    inputs.extend((1..=nfuzz).map(|s| mutated(s, &other)));
    if refresh_only {
        refresh(&inputs);
        return;
    }
    if let Some((n, len)) = dump {
        for i in inputs.iter().filter(|i| i.name == n) {
            let mut left = len;
            for st in &i.steps {
                match st {
                    Step::Bytes(b) => {
                        let k = b.len().min(left);
                        println!("{}", show_all(&b[..k]));
                        left -= k;
                    }
                    Step::Resize(c, r) => println!("<resize {c}x{r}>"),
                }
                if left == 0 {
                    break;
                }
            }
        }
        return;
    }
    if let Some(n) = bisect_dump_name {
        match inputs.iter().find(|i| i.name == n) {
            Some(i) => bisect_dump(i),
            None => eprintln!("no input named {n:?}"),
        }
        return;
    }
    if let Some(n) = diverge_name {
        match inputs.iter().find(|i| i.name == n) {
            Some(i) => diverge(i),
            None => eprintln!("no input named {n:?}"),
        }
        return;
    }
    if let Some(n) = bisect_name {
        match inputs.iter().find(|i| i.name == n) {
            Some(i) => bisect(i),
            None => eprintln!("no input named {n:?}"),
        }
        return;
    }

    let load = |p: &str, approval: bool| {
        load_known(p, approval).unwrap_or_else(|e| {
            eprintln!("vtdiff: {e}");
            std::process::exit(2)
        })
    };
    let (approved, candidates) = (load(&known_path, true), load(&cand_path, false));
    let mut total = Tally::default();
    let mut cats: BTreeMap<String, Tally> = BTreeMap::new();
    let mut firsts = Vec::new();
    let mut known_runs: BTreeMap<String, u64> = BTreeMap::new();
    let mut if_approved: BTreeMap<String, u64> = BTreeMap::new();
    let mut fails = 0u64;
    // Per dump format: runs with identical dumps; runs with identical
    // screens, and of those with identical dumps and with differences per
    // cause; the first differences not explained by a cause.
    let (mut dump_equal, mut screens_equal, mut dump_equal_screens) = ([0u64; 3], 0u64, [0u64; 3]);
    let mut dump_causes = [[0u64; 3]; 3];
    let mut dump_firsts: [Vec<String>; 3] = Default::default();
    for inp in &inputs {
        for splits in [None, Some(7u64)] {
            let cat = cats.entry(inp.category.clone()).or_default();
            let o = run_input(inp, chunk, splits, every, &mut total, cat, Compat::default());
            screens_equal += o.first.is_none() as u64;
            for (i, d) in o.dumps.iter().enumerate() {
                match d {
                    None => {
                        dump_equal[i] += 1;
                        dump_equal_screens[i] += o.first.is_none() as u64;
                    }
                    Some(d) if o.first.is_none() => {
                        let c = CAUSES.iter().position(|c| *c == d.cause).unwrap_or(2);
                        dump_causes[i][c] += 1;
                        // Every cause listed: haetty-vt keeps underlines and
                        // links since E5b.
                        let mode = splits.map_or(format!("chunks of {chunk}"), |s| format!("random splits (seed {s})"));
                        dump_firsts[i]
                            .push(format!("{} [{}], {mode} ({}): {}", inp.name, inp.category, d.cause, d.text));
                    }
                    Some(_) => {}
                }
            }
            match judge(inp, chunk, splits, every, &o, Compat::default(), &approved, &candidates) {
                Verdict::Identical => {}
                Verdict::Known(ids) => *known_runs.entry(ids.join(" + ")).or_default() += 1,
                Verdict::Fail(c) => {
                    fails += 1;
                    let f = o.first.unwrap_or_default();
                    match c {
                        Some(ids) => {
                            let ids = ids.join(" + ");
                            *if_approved.entry(ids.clone()).or_default() += 1;
                            firsts.push(format!("{f} [KNOWN if approved: {ids}]"));
                        }
                        None => firsts.push(f),
                    }
                }
            }
        }
    }
    let pct = |n: u64, d: u64| if d == 0 { 100.0 } else { n as f64 * 100.0 / d as f64 };
    let mut out = String::new();
    let _ = writeln!(out, "# vtdiff: libghostty-vt (reference) vs haetty-vt (native)\n");
    let _ = writeln!(
        out,
        "{} inputs × 2 cuttings, {} checkpoints; inputs identical throughout: {} of {} ({:.1} %)\n",
        inputs.len(),
        total.checkpoints,
        total.inputs_equal,
        total.inputs,
        pct(total.inputs_equal, total.inputs)
    );
    let differing = total.inputs - total.inputs_equal;
    let nknown: u64 = known_runs.values().sum();
    let _ = writeln!(
        out,
        "**Unexplained differences (FAIL): {fails} runs.** Differing runs {differing}: KNOWN {nknown}, FAIL {fails} \
         (of which {} would be KNOWN once candidates are approved). Lists: {} approved \
         (`docs/vtdiff/known-vt-differences.txt`), {} candidates (`known-candidates.txt`), reasons in `known.md`.\n",
        if_approved.values().sum::<u64>(),
        approved.len(),
        candidates.len()
    );
    if !known_runs.is_empty() || !if_approved.is_empty() {
        let _ = writeln!(out, "| Entries | KNOWN runs | FAIL runs, KNOWN if approved |\n|---|---|---|");
        let keys: BTreeSet<&String> = known_runs.keys().chain(if_approved.keys()).collect();
        for k in keys {
            let _ = writeln!(
                out,
                "| {k} | {} | {} |",
                known_runs.get(k).copied().unwrap_or(0),
                if_approved.get(k).copied().unwrap_or(0)
            );
        }
        let _ = writeln!(out);
    }
    let _ = writeln!(out, "| Field | Matching checkpoints |\n|---|---|");
    for f in FIELDS {
        let e = total.equal.get(f).copied().unwrap_or(0);
        let _ = writeln!(out, "| {f} | {e} of {} ({:.1} %) |", total.checkpoints, pct(e, total.checkpoints));
    }
    let _ = writeln!(
        out,
        "| history (scrollback contents, end of each run) | {} of {} ({:.1} %) |",
        total.history_equal,
        total.history,
        pct(total.history_equal, total.history)
    );
    let _ =
        writeln!(out, "\n| Input category | Inputs identical | Checkpoints with every field equal |\n|---|---|---|");
    for (c, t) in &cats {
        let all = FIELDS.iter().map(|f| t.equal.get(f).copied().unwrap_or(0)).min().unwrap_or(0);
        let _ = writeln!(
            out,
            "| {c} | {} of {} | {:.1} % (fewest-matching field) |",
            t.inputs_equal,
            t.inputs,
            pct(all, t.checkpoints)
        );
    }
    let _ = writeln!(
        out,
        "\n| Dump (end of each run) | Identical, all runs | Identical, runs with identical screens | \
         Differing there: {} | {} | {} |\n|---|---|---|---|---|---|",
        CAUSES[0], CAUSES[1], CAUSES[2]
    );
    for (i, (name, _)) in DUMPS.iter().enumerate() {
        let _ = writeln!(
            out,
            "| {name} | {} of {} ({:.1} %) | {} of {screens_equal} ({:.1} %) | {} | {} | {} |",
            dump_equal[i],
            total.inputs,
            pct(dump_equal[i], total.inputs),
            dump_equal_screens[i],
            pct(dump_equal_screens[i], screens_equal),
            dump_causes[i][0],
            dump_causes[i][1],
            dump_causes[i][2]
        );
    }
    for (i, (name, _)) in DUMPS.iter().enumerate() {
        if dump_firsts[i].is_empty() {
            continue;
        }
        let _ = writeln!(out, "\n## Dump {name}: first differences, runs with identical screens\n");
        for f in dump_firsts[i].iter().take(12) {
            let _ = writeln!(out, "- {f}");
        }
        if dump_firsts[i].len() > 12 {
            let _ = writeln!(out, "- … {} more", dump_firsts[i].len() - 12);
        }
    }
    let _ = writeln!(out, "\n## First difference per FAIL run\n");
    for f in &firsts {
        let _ = writeln!(out, "- {f}");
    }
    print!("{out}");
    if let Some(p) = report {
        let _ = std::fs::write(p, &out);
    }
    std::process::exit(if fails == 0 { 0 } else { 1 });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(name: &str, b: &[u8]) -> Input {
        Input { name: name.into(), category: "selftest".into(), steps: vec![Step::Bytes(b.to_vec())] }
    }

    fn list() -> Vec<Known> {
        let p = std::env::temp_dir().join(format!("vtdiff-known-{}.txt", std::process::id()));
        std::fs::write(
            &p,
            "[tbc]\npattern = CSI 0 g\nfields = text width fg bg flags cursor scrollback history\n\
             compat = tbc-toggle\nbasis = test\napproved = test\n\n\
             [origin]\npattern = CSI G|`|a with DECSET 6\nfields = text width fg bg flags cursor scrollback history\n\
             compat = origin-column-moves-row\nbasis = test\napproved = test\n",
        )
        .unwrap();
        let l = load_known(p.to_str().unwrap(), true).unwrap();
        let _ = std::fs::remove_file(p);
        l
    }

    fn verdict(b: &[u8], base: Compat) -> Verdict {
        let inp = input("t", b);
        let (mut a, mut c) = (Tally::default(), Tally::default());
        let o = run_input(&inp, 4096, None, 1024, &mut a, &mut c, base);
        judge(&inp, 4096, None, 1024, &o, base, &list(), &[])
    }

    /// A known behaviour is KNOWN; the same input with an unrelated
    /// difference (haetty-vt's self-test break) is FAIL; an entry missing
    /// from the approved list leaves its difference FAIL.
    #[test]
    fn known_only_when_the_entry_explains_everything() {
        let tbc: &[u8] = b"\x1b[5G\x1b[0g\r\tX";
        let origin: &[u8] = b"\x1b[5;10r\x1b[?6h\x1b[3;1HA\x1b[20GB";
        assert_eq!(verdict(tbc, Compat::default()), Verdict::Known(vec!["tbc".into()]));
        assert_eq!(verdict(origin, Compat::default()), Verdict::Known(vec!["origin".into()]));
        let mut both = tbc.to_vec();
        both.extend_from_slice(b"\r\n\x1b[3CY");
        assert_eq!(verdict(&both, Compat::SELFTEST_BREAK), Verdict::Fail(None));
        assert_eq!(verdict(b"\x1b[3CY", Compat::SELFTEST_BREAK), Verdict::Fail(None));
        assert_eq!(verdict(b"\x1b[3CY", Compat::default()), Verdict::Identical);
        let inp = input("t", tbc);
        let (mut a, mut c) = (Tally::default(), Tally::default());
        let o = run_input(&inp, 4096, None, 1024, &mut a, &mut c, Compat::default());
        let only_origin: Vec<Known> = list().into_iter().filter(|k| k.id == "origin").collect();
        let tbc_cand: Vec<Known> = list().into_iter().filter(|k| k.id == "tbc").collect();
        assert_eq!(
            judge(&inp, 4096, None, 1024, &o, Compat::default(), &only_origin, &tbc_cand),
            Verdict::Fail(Some(vec!["tbc".into()]))
        );
    }

    /// Every list in docs/vtdiff reads; the approved one has approvals.
    #[test]
    fn the_lists_read() {
        let docs = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/vtdiff");
        load_known(&format!("{docs}/known-vt-differences.txt"), true).unwrap();
        load_known(&format!("{docs}/known-candidates.txt"), false).unwrap();
    }
}
