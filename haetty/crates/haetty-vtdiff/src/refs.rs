//! The reference engines of vtdiff, and how the differential names them.
//!
//! - `ghostty-main` (the default from F1): the newest Ghostty, pinned in `tools/ghostty-ref`. It runs as a helper
//!   process (`ghostty-ref`, a C program on the static libghostty-vt of the pin) that speaks the line protocol in
//!   `tools/ghostty-ref/README.md`. Because the helper is a separate process, the 0.2.2 crate and the newest
//!   libghostty-vt never link into one binary (both are named `ghostty-vt`).
//! - `ghostty-0.2.2` (E0–E5): libghostty-vt 0.2.2 in this process, through haetty-core's `GhosttyEngine`. Kept for the
//!   side-by-side report; it is no longer the target.
//!
//! Both are `Ref`: the `VtEngine` calls of the differential, and `fresh`, a snapshot read through a new render state
//! (haetty-core's `GhosttyEngine::snapshot_fresh`, which the differential compares at every checkpoint).

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Arc;

use haetty_core::ghostty::GhosttyEngine;
use haetty_core::vt::{VtEngine, VtInit, VtMode};
use haetty_core::{
    Cell, CellFlags, ColorSchemePref, Cursor, CursorShape, DumpFormat, GridSize, KeyInput, Modes, MouseInput, Rgb,
    ScrollTo, Scrollbar, SessionEvent, Snapshot, TermColors, Underline, Width,
};

/// A reference engine, by the name the known lists and the command line use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefKind {
    /// The newest Ghostty (the helper process). The default reference.
    Main,
    /// libghostty-vt 0.2.2, in process.
    Old,
}

impl RefKind {
    pub fn parse(s: &str) -> Option<RefKind> {
        match s {
            "ghostty-main" => Some(RefKind::Main),
            "ghostty-0.2.2" => Some(RefKind::Old),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            RefKind::Main => "ghostty-main",
            RefKind::Old => "ghostty-0.2.2",
        }
    }
}

/// What the differential needs from a reference.
pub trait Ref: VtEngine {
    /// The screen read through a fresh render state, for every comparison.
    fn fresh(&mut self) -> Snapshot;
}

impl Ref for GhosttyEngine {
    fn fresh(&mut self) -> Snapshot {
        self.snapshot_fresh()
    }
}

/// The reference of `kind`, started on `init` (the same settings as haetty's engine).
pub fn make_ref(kind: RefKind, init: &VtInit) -> Result<Box<dyn Ref>, String> {
    match kind {
        RefKind::Old => Ok(Box::new(GhosttyEngine::new(init)?)),
        RefKind::Main => Ok(Box::new(ProcRef::new(init)?)),
    }
}

/// The reference of the single-reference tools (`--probe`, `--diverge`, `--bisect`, `--bisect-dump`): set by
/// `--ref`, ghostty-main otherwise.
static DEFAULT: std::sync::OnceLock<RefKind> = std::sync::OnceLock::new();

pub fn set_default_ref(kind: RefKind) {
    let _ = DEFAULT.set(kind);
}

pub fn default_ref() -> RefKind {
    DEFAULT.get().copied().unwrap_or(RefKind::Main)
}

/// The helper's binary: `HAETTY_GHOSTTY_REF`, else `target-ghostty-ref/ghostty-ref` in the haetty folder.
pub fn helper_path() -> PathBuf {
    if let Some(p) = std::env::var_os("HAETTY_GHOSTTY_REF") {
        return PathBuf::from(p);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target-ghostty-ref/ghostty-ref")
}

/// The newest Ghostty as a helper process (see the module doc and `tools/ghostty-ref/README.md`).
pub struct ProcRef {
    child: Child,
    to: ChildStdin,
    from: BufReader<ChildStdout>,
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Result<Vec<u8>, String> {
    if !s.len().is_multiple_of(2) {
        return Err(format!("odd hex length {}", s.len()));
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| format!("bad hex {:?}: {e}", &s[i..i + 2])))
        .collect()
}

/// A text field of the protocol: hex of UTF-8, or `-` for none. Invalid UTF-8 is replaced, as haetty's mapping
/// does for hyperlink targets (`String::from_utf8_lossy` in `cell_link`).
fn text_of(hexed: &str) -> Result<String, String> {
    if hexed == "-" {
        return Ok(String::new());
    }
    Ok(String::from_utf8_lossy(&unhex(hexed)?).into_owned())
}

/// The title and the working directory: haetty's mapping takes them only when they are valid UTF-8, else empty
/// (`term.title().unwrap_or("")`, where the crate returns an error for invalid UTF-8).
fn strict_text(hexed: &str) -> Result<String, String> {
    if hexed == "-" {
        return Ok(String::new());
    }
    Ok(String::from_utf8(unhex(hexed)?).unwrap_or_default())
}

/// A colour field: six hex digits (RRGGBB), or `-` for none.
fn rgb_of(s: &str) -> Result<Option<Rgb>, String> {
    if s == "-" {
        return Ok(None);
    }
    let v = u32::from_str_radix(s, 16).map_err(|e| format!("colour {s:?}: {e}"))?;
    Ok(Some(Rgb { r: (v >> 16) as u8, g: (v >> 8) as u8, b: v as u8 }))
}

fn rgb_or_default(s: &str) -> Result<Rgb, String> {
    Ok(rgb_of(s)?.unwrap_or_default())
}

impl ProcRef {
    /// Starts the helper and gives it the settings of `init` (size, cells, scrollback; colours and the XTVERSION
    /// answer are the helper's constants, which equal the ones vtdiff's `init()` sets).
    pub fn new(init: &VtInit) -> Result<ProcRef, String> {
        let path = helper_path();
        let mut child = Command::new(&path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| {
                format!(
                    "cannot start the ghostty-main helper {}: {e}\n  build it: tools/heavy.sh --label ghostty-ref -- bash tools/ghostty-ref/build.sh",
                    path.display()
                )
            })?;
        let to = child.stdin.take().ok_or("helper: no stdin")?;
        let from = BufReader::new(child.stdout.take().ok_or("helper: no stdout")?);
        let mut p = ProcRef { child, to, from };
        let g = init.size;
        p.call(&format!("init {} {} {} {} {}", g.cols, g.rows, g.cell_width, g.cell_height, init.scrollback_limit))?;
        Ok(p)
    }

    fn send(&mut self, line: &str) -> Result<(), String> {
        writeln!(self.to, "{line}").and_then(|_| self.to.flush()).map_err(|e| format!("helper: {e}"))
    }

    /// One request whose answer is one line: `ok`, or a value line.
    fn call(&mut self, line: &str) -> Result<String, String> {
        self.send(line)?;
        let mut s = String::new();
        self.from.read_line(&mut s).map_err(|e| format!("helper: {e}"))?;
        if s.is_empty() {
            return Err("helper: closed its output".into());
        }
        let s = s.trim_end().to_string();
        if let Some(e) = s.strip_prefix("err ") {
            return Err(format!("helper: {e}"));
        }
        Ok(s)
    }

    fn snapshot_fresh(&mut self) -> Result<Snapshot, String> {
        self.send("snap")?;
        let mut snap = Snapshot::default();
        let mut line = String::new();
        loop {
            line.clear();
            if self.from.read_line(&mut line).map_err(|e| format!("helper: {e}"))? == 0 {
                return Err("helper: closed its output in a snapshot".into());
            }
            let l = line.trim_end();
            if l == "end" {
                break;
            }
            if let Some(e) = l.strip_prefix("err ") {
                return Err(format!("helper: {e}"));
            }
            let mut it = l.split(' ');
            match it.next() {
                Some("size") => {
                    snap.cols = it.next().ok_or("size")?.parse().map_err(|_| "size cols")?;
                    snap.rows = it.next().ok_or("size")?.parse().map_err(|_| "size rows")?;
                    snap.cells = vec![Cell::default(); snap.cols as usize * snap.rows as usize];
                }
                Some("colors") => {
                    snap.default_fg = rgb_or_default(it.next().ok_or("colors fg")?)?;
                    snap.default_bg = rgb_or_default(it.next().ok_or("colors bg")?)?;
                    snap.cursor_color = rgb_of(it.next().ok_or("colors cursor")?)?;
                }
                Some("title") => snap.title = strict_text(it.next().unwrap_or("-"))?,
                Some("pwd") => snap.pwd = strict_text(it.next().unwrap_or("-"))?,
                Some("modes") => {
                    let v: Vec<&str> = it.collect();
                    let on = |i: usize| v.get(i).is_some_and(|x| *x == "1");
                    snap.modes = Modes {
                        alt_screen: on(0),
                        mouse_tracking: on(1),
                        sgr_mouse: on(2),
                        bracketed_paste: on(3),
                        focus_events: on(4),
                        sync_output: on(5),
                        app_cursor_keys: on(6),
                        cursor_visible: on(7),
                        kitty_keyboard: v.get(8).and_then(|x| x.parse().ok()).unwrap_or(0),
                    };
                }
                Some("scroll") => {
                    let v: Vec<u64> = it.map(|x| x.parse().unwrap_or(0)).collect();
                    if v.len() != 3 {
                        return Err(format!("scroll line {l:?}"));
                    }
                    snap.scrollbar = Scrollbar { total: v[0], offset: v[1], len: v[2] };
                }
                Some("cursor") => {
                    let f: Vec<&str> = it.collect();
                    snap.cursor = if f.first() == Some(&"none") {
                        None
                    } else {
                        if f.len() != 4 {
                            return Err(format!("cursor line {l:?}"));
                        }
                        let shape = match f[2] {
                            "1" => CursorShape::Bar,
                            "2" => CursorShape::Underline,
                            "3" => CursorShape::BlockHollow,
                            _ => CursorShape::Block,
                        };
                        Some(Cursor {
                            x: f[0].parse().map_err(|_| "cursor x")?,
                            y: f[1].parse().map_err(|_| "cursor y")?,
                            shape,
                            blinking: f[3] == "1",
                        })
                    };
                }
                Some("r") => {
                    let y: usize = it.next().ok_or("r")?.parse().map_err(|_| "row number")?;
                    let cols = snap.cols as usize;
                    for (x, tok) in it.enumerate() {
                        if x >= cols || y >= snap.rows as usize {
                            return Err(format!("row {y} has more cells than {cols} columns"));
                        }
                        snap.cells[y * cols + x] = parse_cell(tok)?;
                    }
                }
                _ => return Err(format!("unknown snapshot line {l:?}")),
            }
        }
        Ok(snap)
    }
}

/// One cell token: `flags:width:fg:bg:ul:ulk:ulc:text:link` (tools/ghostty-ref/README.md).
fn parse_cell(tok: &str) -> Result<Cell, String> {
    let f: Vec<&str> = tok.split(':').collect();
    if f.len() != 9 {
        return Err(format!("cell {tok:?} has {} fields, want 9", f.len()));
    }
    let flags: u16 = f[0].parse().map_err(|_| format!("cell flags {tok:?}"))?;
    let width = match f[1] {
        "1" => Width::Wide,
        "2" => Width::Spacer,
        _ => Width::Narrow,
    };
    let underline = match f[5] {
        "2" => Underline::Double,
        "3" => Underline::Curly,
        "4" => Underline::Dotted,
        "5" => Underline::Dashed,
        _ => Underline::Single,
    };
    let link = if f[8] == "-" { None } else { Some(Arc::<str>::from(text_of(f[8])?)) };
    Ok(Cell {
        text: text_of(f[7])?,
        fg: rgb_of(f[2])?,
        bg: rgb_of(f[3])?,
        flags: CellFlags(flags),
        width,
        underline,
        underline_color: rgb_of(f[6])?,
        link,
    })
}

impl VtEngine for ProcRef {
    fn feed(&mut self, bytes: &[u8]) {
        if let Err(e) = self.call(&format!("feed {}", hex(bytes))) {
            panic!("ghostty-main: {e}");
        }
    }

    fn take_replies(&mut self) -> Vec<u8> {
        match self.call("replies") {
            Ok(s) => s.strip_prefix("replies ").map(|h| unhex(h).unwrap_or_default()).unwrap_or_default(),
            Err(e) => panic!("ghostty-main: {e}"),
        }
    }

    /// The differential does not compare events; the helper sends none.
    fn take_events(&mut self) -> Vec<SessionEvent> {
        Vec::new()
    }

    fn resize(&mut self, g: GridSize) {
        if let Err(e) = self.call(&format!("resize {} {} {} {}", g.cols, g.rows, g.cell_width, g.cell_height)) {
            panic!("ghostty-main: {e}");
        }
    }

    fn snapshot(&mut self) -> Snapshot {
        self.fresh()
    }

    fn mode(&self, m: VtMode) -> bool {
        panic!("ghostty-main: mode queries ({m:?}) are not in the helper protocol; use the snapshot's modes");
    }

    fn scroll_viewport(&mut self, to: ScrollTo) {
        let line = match to {
            ScrollTo::Top => "scroll top".to_string(),
            ScrollTo::Bottom => "scroll bottom".to_string(),
            ScrollTo::Delta(d) => format!("scroll delta {d}"),
        };
        if let Err(e) = self.call(&line) {
            panic!("ghostty-main: {e}");
        }
    }

    fn set_colors(&mut self, _c: &TermColors) {
        panic!("ghostty-main: colours are set at init (the helper's constants)");
    }

    fn set_color_scheme(&mut self, _s: ColorSchemePref, _out: &mut Vec<u8>) {
        panic!("ghostty-main: colour scheme reports are not in the helper protocol");
    }

    fn encode_key(&mut self, _k: &KeyInput, _out: &mut Vec<u8>) {
        panic!("ghostty-main: encoders are not in the helper protocol; vtdiff --encoders uses the 0.2.2 engine");
    }

    fn encode_mouse(&mut self, _m: &MouseInput, _size: GridSize, _out: &mut Vec<u8>) {
        panic!("ghostty-main: encoders are not in the helper protocol; vtdiff --encoders uses the 0.2.2 engine");
    }

    fn encode_paste(&mut self, _text: &str, _out: &mut Vec<u8>) {
        panic!("ghostty-main: encoders are not in the helper protocol; vtdiff --encoders uses the 0.2.2 engine");
    }

    fn encode_focus(&mut self, _focused: bool, _out: &mut Vec<u8>) {
        panic!("ghostty-main: encoders are not in the helper protocol; vtdiff --encoders uses the 0.2.2 engine");
    }

    fn dump(&mut self, fmt: DumpFormat) -> String {
        let f = match fmt {
            DumpFormat::Plain => "plain",
            DumpFormat::Vt => "vt",
            DumpFormat::Html => "html",
        };
        // An empty dump is answered `dump` alone (the reply line is trimmed).
        match self.call(&format!("dump {f}")) {
            Ok(s) => match s.strip_prefix("dump") {
                Some(rest) => String::from_utf8_lossy(&unhex(rest.trim_start()).unwrap_or_default()).into_owned(),
                None => panic!("ghostty-main: dump answered {s:?}"),
            },
            Err(e) => panic!("ghostty-main: {e}"),
        }
    }

    fn scrollback_rows(&mut self) -> u64 {
        match self.call("sbrows") {
            Ok(s) => s
                .strip_prefix("sbrows ")
                .and_then(|n| n.parse().ok())
                .unwrap_or_else(|| panic!("ghostty-main: sbrows answered {s:?}")),
            Err(e) => panic!("ghostty-main: {e}"),
        }
    }
}

impl Ref for ProcRef {
    fn fresh(&mut self) -> Snapshot {
        match self.snapshot_fresh() {
            Ok(s) => s,
            Err(e) => panic!("ghostty-main: {e}"),
        }
    }
}

impl Drop for ProcRef {
    fn drop(&mut self) {
        let _ = writeln!(self.to, "quit");
        let _ = self.to.flush();
        let _ = self.child.wait();
    }
}
