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

use std::collections::BTreeMap;
use std::fmt::Write as _;

use haetty_core::ghostty::GhosttyEngine;
use haetty_core::native::NativeEngine;
use haetty_core::vt::{VtEngine, VtInit};
use haetty_core::{ColorSchemePref, GridSize, Rgb, Snapshot, TermColors};

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
const FIELDS: &[&str] =
    &["size", "text", "width", "fg", "bg", "flags", "cursor", "modes", "title", "pwd", "scrollback", "replies"];

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
                    push("flags", at, format!("{:#x}", ca.flags.0), format!("{:#x}", cb.flags.0));
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
    tally: &'a mut Tally,
    by_category: &'a mut Tally,
    all_equal: bool,
}

impl Run<'_> {
    fn checkpoint(&mut self, offset: u64) {
        let (a, b) = (self.reference.snapshot(), self.native.snapshot());
        self.ra.extend(self.reference.take_replies());
        self.rb.extend(self.native.take_replies());
        let (sa, sb) = (self.reference.scrollback_rows(), self.native.scrollback_rows());
        let diffs = compare(&a, &b, sa, sb, &self.ra, &self.rb);
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

fn run_input(
    inp: &Input,
    chunk: usize,
    splits: Option<u64>,
    every: u64,
    tally: &mut Tally,
    cat: &mut Tally,
) -> Option<String> {
    let mut reference = match GhosttyEngine::new(&init(80, 24)) {
        Ok(e) => e,
        Err(e) => return Some(format!("{}: reference engine: {e}", inp.name)),
    };
    // Applications ask for nothing at start; drain what an engine may say.
    let _ = reference.take_replies();
    let native = NativeEngine::new(&init(80, 24));
    let mut r = Run {
        reference,
        native,
        ra: vec![],
        rb: vec![],
        fed: vec![],
        first: None,
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
    r.first.map(|(off, d, tail)| {
        format!(
            "{} [{}], {mode}: first difference at input offset {off}: {} {} — reference {} / native {}; last bytes fed: {tail}",
            inp.name, inp.category, d.field, d.at, d.reference, d.native
        )
    })
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

/// `write!` into a byte vector.
struct Text<'a>(&'a mut Vec<u8>);
impl std::fmt::Write for Text<'_> {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        self.0.extend_from_slice(s.as_bytes());
        Ok(())
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let (mut corpus, mut nfuzz, mut chunk, mut every, mut report) = (None, 20u64, 4096usize, 1024u64, None);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--corpus" => corpus = args.next(),
            "--fuzz" => nfuzz = args.next().and_then(|v| v.parse().ok()).unwrap_or(20),
            "--chunk" => chunk = args.next().and_then(|v| v.parse().ok()).unwrap_or(4096),
            "--every" => every = args.next().and_then(|v| v.parse().ok()).unwrap_or(1024),
            "--report" => report = args.next(),
            _ => {
                eprintln!("usage: vtdiff [--corpus DIR] [--fuzz N] [--chunk BYTES] [--every BYTES] [--report FILE]");
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
    inputs.extend(scenarios());
    inputs.extend(resizes());
    inputs.extend((1..=nfuzz).map(fuzz));

    let mut total = Tally::default();
    let mut cats: BTreeMap<String, Tally> = BTreeMap::new();
    let mut firsts = Vec::new();
    for inp in &inputs {
        for splits in [None, Some(7u64)] {
            let cat = cats.entry(inp.category.clone()).or_default();
            if let Some(f) = run_input(inp, chunk, splits, every, &mut total, cat) {
                firsts.push(f);
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
    let _ = writeln!(out, "| Field | Matching checkpoints |\n|---|---|");
    for f in FIELDS {
        let e = total.equal.get(f).copied().unwrap_or(0);
        let _ = writeln!(out, "| {f} | {e} of {} ({:.1} %) |", total.checkpoints, pct(e, total.checkpoints));
    }
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
    let _ = writeln!(out, "\n## First difference per input\n");
    for f in &firsts {
        let _ = writeln!(out, "- {f}");
    }
    print!("{out}");
    if let Some(p) = report {
        let _ = std::fs::write(p, &out);
    }
    std::process::exit(if firsts.is_empty() { 0 } else { 1 });
}
