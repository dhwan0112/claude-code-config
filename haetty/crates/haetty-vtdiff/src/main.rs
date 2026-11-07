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
//!   --gen-width FILE             writes `haetty-vt`'s width tables from libghostty-vt
//!   --dump NAME [BYTES]          the named input's bytes (escaped), resizes marked

use std::collections::BTreeMap;
use std::fmt::Write as _;

use haetty_core::ghostty::GhosttyEngine;
use haetty_core::native::NativeEngine;
use haetty_core::vt::{VtEngine, VtInit};
use haetty_core::{ColorSchemePref, GridSize, Rgb, ScrollTo, Snapshot, TermColors};

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
    r.history();
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

/// Fuzz, second mix: what the first leaves out — private modes (not 69:
/// left/right margins are not implemented, DESIGN.md §10.7), non-ASCII
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
        1u16, 5, 6, 7, 12, 25, 47, 66, 1000, 1002, 1003, 1004, 1005, 1006, 1015, 1016, 1047, 1048, 1049, 2004, 2026,
        2027, 2031, 2048,
    ];
    let nums = [0u16, 1, 2, 3, 5, 8, 22, 80, 200];
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
            11 => b.extend_from_slice(
                [
                    b"\x1bP$qm\x1b\\".as_slice(),
                    b"\x1b_Ga=q,i=3;AAAA\x1b\\",
                    b"\x1bP+q544e\x1b\\",
                    b"\x1b_Ga=q,i=9\x1b\\",
                ][rng.below(4) as usize],
            ),
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
    let mut dump: Option<(String, usize)> = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--corpus" => corpus = args.next(),
            "--fuzz" => nfuzz = args.next().and_then(|v| v.parse().ok()).unwrap_or(20),
            "--chunk" => chunk = args.next().and_then(|v| v.parse().ok()).unwrap_or(4096),
            "--every" => every = args.next().and_then(|v| v.parse().ok()).unwrap_or(1024),
            "--report" => report = args.next(),
            "--bisect" => bisect_name = args.next(),
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
    inputs.extend((1..=nfuzz).map(fuzz));
    inputs.extend((1..=nfuzz).map(fuzz2));
    inputs.extend((1..=nfuzz).map(grammar));
    let other: Vec<u8> = match inputs.iter().find(|i| i.category == "corpus") {
        Some(Input { steps, .. }) => match steps.first() {
            Some(Step::Bytes(b)) => b[..b.len().min(64 << 10)].to_vec(),
            _ => vec![],
        },
        None => vec![],
    };
    inputs.extend((1..=nfuzz).map(|s| mutated(s, &other)));
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
    if let Some(n) = bisect_name {
        match inputs.iter().find(|i| i.name == n) {
            Some(i) => bisect(i),
            None => eprintln!("no input named {n:?}"),
        }
        return;
    }

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
