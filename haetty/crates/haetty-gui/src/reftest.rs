//! Reference-frame ("golden image") tests: fixed window states rendered
//! offscreen with the CPU painter and compared with committed images in
//! `tests/reference/<os>/`.
//!
//! - `HAETTY_UPDATE_REFERENCE=1` writes the references (and `fonts.txt`, the
//!   font fingerprint) instead of comparing.
//! - `HAETTY_REFERENCE_DIR=dir` replaces `tests/reference` (a test binary
//!   copied to Windows still has the Linux `CARGO_MANIFEST_DIR` built in).
//! - No references for this OS, or other fonts than the ones they were made
//!   with: the test says why and passes.
//! - Every run writes `<name>-actual.png` to `target/reftest/<os>/` (beside
//!   the repository root of the reference directory), plus `<name>-diff.png`
//!   on a mismatch.
//!
//! Format (`.ccrf`): `CCRF1 <w> <h>\n`, then runs of `u16` LE count and
//! `r g b`, row-major.

use std::path::{Path, PathBuf};

use haetty_core::theme::Theme;
use haetty_core::{Cell, Cursor, CursorShape, Rgb, Snapshot};
use haetty_proto::wire::{Attention, ClaudeInfo, EditMeta, Limit, Worker};

use crate::font::Fonts;
use crate::geom::Layout;
use crate::render::canvas::{pack, unpack, Canvas};
use crate::render::paint::{Look, Painter};
use crate::render::{PaddingStyle, Preedit, Scene};
use crate::ui::{self, Compose, Greys, TabView};

/// Colours of the synthetic comparison test (not the scenes).
const FG: Rgb = Rgb { r: 220, g: 220, b: 220 };
const BG: Rgb = Rgb { r: 30, g: 30, b: 30 };

/// The scenes are drawn in the default theme (DESIGN.md §2.2).
fn theme() -> Theme {
    Theme::default()
}
const PROMPT: Rgb = Rgb { r: 120, g: 190, b: 120 };
const COLS: u16 = 60;
const ROWS: u16 = 12;
/// 13 pt at 96 dpi.
const PX: f32 = 13.0 * 96.0 / 72.0;
/// Fixed clock for the status panel (2026-11-05 12:00:00 UTC).
const NOW_MS: u64 = 1_793_880_000_000;
/// Columns of the status panel, separator included (the app's minimum).
const PANEL_COLS: usize = 25;

/// More differing pixels than this fraction fail (0.05 %).
const MAX_DIFF_FRACTION: f64 = 0.0005;
/// A channel delta above this fails, however few pixels differ: a glyph
/// moved by one pixel changes few pixels, but by a lot.
const MAX_CHANNEL_DELTA: u8 = 48;

// ---- format ---------------------------------------------------------------

fn encode(w: u32, h: u32, pixels: &[u32]) -> Vec<u8> {
    let mut out = format!("CCRF1 {w} {h}\n").into_bytes();
    let mut i = 0;
    while i < pixels.len() {
        let p = pixels[i];
        let mut n = 1;
        while i + n < pixels.len() && pixels[i + n] == p && n < u16::MAX as usize {
            n += 1;
        }
        let c = unpack(p);
        out.extend_from_slice(&(n as u16).to_le_bytes());
        out.extend_from_slice(&[c.r, c.g, c.b]);
        i += n;
    }
    out
}

fn decode(data: &[u8]) -> Result<(u32, u32, Vec<u32>), String> {
    let nl = data.iter().position(|&b| b == b'\n').ok_or("no header")?;
    let header = std::str::from_utf8(&data[..nl]).map_err(|_| "header is not text")?;
    let mut it = header.split(' ');
    if it.next() != Some("CCRF1") {
        return Err(format!("bad header {header:?}"));
    }
    let mut num = || it.next().and_then(|s| s.parse::<u32>().ok()).ok_or_else(|| format!("bad header {header:?}"));
    let (w, h) = (num()?, num()?);
    let total = w as usize * h as usize;
    let body = &data[nl + 1..];
    if !body.len().is_multiple_of(5) {
        return Err("truncated run".into());
    }
    let mut pixels = Vec::with_capacity(total);
    for run in body.chunks(5) {
        let n = u16::from_le_bytes([run[0], run[1]]) as usize;
        if n == 0 || pixels.len() + n > total {
            return Err("bad run length".into());
        }
        let p = pack(Rgb { r: run[2], g: run[3], b: run[4] });
        pixels.extend(std::iter::repeat_n(p, n));
    }
    if pixels.len() != total {
        return Err(format!("{} pixels, expected {total}", pixels.len()));
    }
    Ok((w, h, pixels))
}

// ---- comparison -----------------------------------------------------------

#[derive(Debug)]
struct Diff {
    count: usize,
    total: usize,
    /// Inclusive `(x0, y0, x1, y1)` of the differing pixels.
    bbox: Option<(u32, u32, u32, u32)>,
    max_delta: u8,
    mask: Vec<bool>,
}

impl Diff {
    fn fails(&self) -> bool {
        self.count as f64 > self.total as f64 * MAX_DIFF_FRACTION || self.max_delta > MAX_CHANNEL_DELTA
    }

    fn describe(&self) -> String {
        let pct = 100.0 * self.count as f64 / self.total.max(1) as f64;
        let bbox = self.bbox.map_or("-".into(), |(x0, y0, x1, y1)| format!("x {x0}..={x1}, y {y0}..={y1}"));
        format!(
            "{} of {} pixels differ ({pct:.3} %), bounding box {bbox}, max channel delta {}",
            self.count, self.total, self.max_delta
        )
    }
}

/// `expected` and `actual` are `w` × `h`.
fn compare(w: u32, expected: &[u32], actual: &[u32]) -> Diff {
    let mut d = Diff { count: 0, total: actual.len(), bbox: None, max_delta: 0, mask: vec![false; actual.len()] };
    for (i, (&e, &a)) in expected.iter().zip(actual).enumerate() {
        if e == a {
            continue;
        }
        let (e, a) = (unpack(e), unpack(a));
        let delta = e.r.abs_diff(a.r).max(e.g.abs_diff(a.g)).max(e.b.abs_diff(a.b));
        d.max_delta = d.max_delta.max(delta);
        d.count += 1;
        d.mask[i] = true;
        let (x, y) = (i as u32 % w, i as u32 / w);
        d.bbox = Some(match d.bbox {
            None => (x, y, x, y),
            Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
        });
    }
    d
}

fn rgba(pixels: &[u32]) -> Vec<u8> {
    pixels.iter().flat_map(|&p| (p | 0xff00_0000).rotate_left(8).to_be_bytes()).collect()
}

/// The actual frame dimmed, differing pixels in magenta.
fn diff_image(actual: &[u32], mask: &[bool]) -> Vec<u32> {
    let grey = Rgb { r: 0, g: 0, b: 0 };
    let hit = pack(Rgb { r: 255, g: 0, b: 255 });
    actual
        .iter()
        .zip(mask)
        .map(|(&p, &m)| if m { hit } else { pack(crate::render::canvas::mix(unpack(p), grey, 90)) })
        .collect()
}

// ---- scenes ---------------------------------------------------------------

/// A `cols` × `rows` screen with `lines` from the top (a leading `$ ` in
/// the prompt colour) and the cursor.
fn screen(cols: u16, rows: u16, lines: &[&str], cursor: Option<(u16, u16)>) -> Snapshot {
    let t = theme();
    let (fg, bg) = (t.foreground, t.background);
    let mut grid = vec![vec![Cell::default(); cols as usize]; rows as usize];
    for (row, line) in grid.iter_mut().zip(lines) {
        let (x, rest) = match line.strip_prefix("$ ") {
            Some(rest) => (ui::put(row, 0, "$ ", PROMPT, bg, true), rest),
            None => (0, *line),
        };
        ui::put(row, x, rest, fg, bg, false);
    }
    Snapshot {
        cols,
        rows,
        cells: grid.into_iter().flatten().collect(),
        cursor: cursor.map(|(x, y)| Cursor { x, y, shape: CursorShape::Block, blinking: false }),
        default_fg: fg,
        default_bg: bg,
        cursor_color: t.cursor_color,
        ..Default::default()
    }
}

fn compose<'a>(screen: &'a Snapshot, accents: &'a [Rgb; 16]) -> Compose<'a> {
    Compose {
        screen,
        accents,
        cols: COLS,
        rows: ROWS,
        tab_bar: None,
        status: None,
        panel: None,
        diff: None,
        palette: None,
        hint: None,
    }
}

/// Few enough rows that the panel's last lines (the finished worker,
/// "updated") fit the 11-row screen area.
fn claude_info() -> ClaudeInfo {
    ClaudeInfo {
        model: "claude-opus".into(),
        context_pct: Some(42.0),
        context_size: Some(200_000),
        limits: vec![Limit { name: "five_hour".into(), used_pct: 12.0, resets_at: Some(1_793_890_800) }],
        cost_usd: Some(1.23),
        lines_added: 12,
        lines_removed: 4,
        files_changed: 3,
        edits: 5,
        last_edit: Some(EditMeta {
            path: "src/parser.rs".into(),
            agent: "main".into(),
            at_ms: NOW_MS - 20_000,
            added: 12,
            removed: 4,
            diffable: true,
        }),
        workers: vec![Worker {
            id: "w1".into(),
            kind: "Explore".into(),
            started_ms: NOW_MS - 95_000,
            ended_ms: Some(NOW_MS - 50_000),
        }],
        updated_ms: NOW_MS - 2_000,
        ..Default::default()
    }
}

/// The window states: name, composed grid, IME preedit.
fn scenes() -> Vec<(&'static str, Snapshot, Option<Preedit>)> {
    let t = theme();
    let acc = t.palette;
    let g = Greys::new(t.foreground, t.background, &acc);
    let compose = |s| compose(s, &acc);
    let mut out = vec![];

    let s = screen(COLS, ROWS, &[], Some((0, 0)));
    out.push(("empty", compose(&s).build(), None));

    let typed = "$ 한글 입";
    let s = screen(
        COLS,
        ROWS,
        &["$ echo 안녕하세요", "안녕하세요", "$ printf '한글 입력 테스트\\n'", "한글 입력 테스트", typed],
        Some((ui::width(typed) as u16, 4)),
    );
    let pre = Preedit { text: "력".into(), cursor: Some((3, 3)) };
    out.push(("hangul-preedit", compose(&s).build(), Some(pre)));

    let s = screen(
        COLS,
        ROWS - 1,
        &["$ cargo build", "   Compiling haetty v0.1.0", "    Finished `dev` profile in 4.2s", "$ "],
        Some((2, 3)),
    );
    let tabs = [
        TabView { name: "bash", exited: false, attention: Attention::None },
        TabView { name: "Claude Code", exited: false, attention: Attention::Wait },
        TabView { name: "vim main.rs", exited: false, attention: Attention::None },
        TabView { name: "cargo test", exited: true, attention: Attention::None },
    ];
    let bar = ui::tab_bar(&tabs, 0, COLS as usize, g);
    out.push(("tabs", Compose { tab_bar: Some(bar), ..compose(&s) }.build(), None));

    let info = claude_info();
    let area = ROWS as usize - 1;
    let s = screen(
        COLS - PANEL_COLS as u16,
        area as u16,
        &[
            "> Fix the off-by-one in parser.rs",
            "",
            "● Read src/parser.rs",
            "● Update src/parser.rs",
            "  ⎿ +12 -4",
            "",
            "● The loop now stops at len.",
            "",
            "> ",
        ],
        Some((2, 8)),
    );
    let status = ui::status_row(Some(&info), COLS as usize, g, "ctrl+shift+d");
    let panel = ui::panel(Some(&info), PANEL_COLS, area, g, NOW_MS);
    out.push(("claude-panel", Compose { status: Some(status), panel: Some(panel), ..compose(&s) }.build(), None));
    out
}

// ---- rendering ------------------------------------------------------------

/// One full frame, as `SoftRenderer::render` paints it.
fn render(painter: &mut Painter, snap: &Snapshot, preedit: Option<&Preedit>) -> Canvas {
    let pad = PaddingStyle { min: (4, 4), balance: false, ..Default::default() };
    let layout = Layout { cell: painter.cell_metrics(), padding: pad.min, balance: pad.balance };
    let (w, h) = layout.window_size(snap.cols, snap.rows);
    let mut cv = Canvas::new(w, h);
    let (ox, oy) = layout.origin(w, h, snap.cols, snap.rows);
    let origin = (ox as i32, oy as i32);
    cv.fill(cv.bounds(), snap.default_bg);
    let dirty = vec![true; snap.rows as usize];
    let scene = Scene { snap, dirty: &dirty, full: true, focused: true, preedit };
    for y in 0..snap.rows {
        painter.paint_row(&mut cv, origin, &scene, y, pad);
    }
    painter.paint_vertical_padding(&mut cv, origin, &scene, true, pad);
    painter.paint_vertical_padding(&mut cv, origin, &scene, false, pad);
    cv
}

// ---- paths ----------------------------------------------------------------

fn os() -> &'static str {
    std::env::consts::OS
}

/// `tests/reference` (or `HAETTY_REFERENCE_DIR`).
fn reference_root() -> PathBuf {
    match haetty_proto::product::env_os("REFERENCE_DIR").filter(|v| !v.is_empty()) {
        Some(d) => d.into(),
        None => Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("reference"),
    }
}

/// `reftest-<os>` in the test run's directory (haetty_testkit: never the
/// real home). Only an explicit update (`HAETTY_UPDATE_REFERENCE=1`) writes
/// elsewhere: into the reference directory itself.
fn output_dir() -> PathBuf {
    haetty_testkit::run_dir(&format!("reftest-{}", os()))
}

fn write(path: &Path, data: &[u8]) {
    if let Err(e) = std::fs::write(path, data) {
        panic!("write {}: {e}", path.display());
    }
}

// ---- tests ----------------------------------------------------------------

#[test]
fn reference_frames() {
    let update = haetty_proto::product::env_os("UPDATE_REFERENCE").is_some_and(|v| v == "1");
    let root = reference_root();
    let dir = root.join(os());
    let out = output_dir();
    println!("reftest: output in {}", out.display());
    let fonts = match Fonts::load(None) {
        Ok(f) => f,
        Err(e) => {
            println!("reftest: skipped, no fonts: {e}");
            return;
        }
    };
    let fingerprint = fonts.fingerprint();
    println!("reftest: fonts {fingerprint}");
    let fonts_file = dir.join("fonts.txt");
    if !update {
        let Ok(want) = std::fs::read_to_string(&fonts_file) else {
            println!("reftest: skipped, no references for {} in {}", os(), dir.display());
            return;
        };
        if want.trim() != fingerprint.trim() {
            println!("reftest: skipped, the references were made with other fonts:\n  {}", want.trim());
            return;
        }
    }
    if update {
        let parent = root.parent().expect("reference dir has a parent");
        assert!(parent.is_dir(), "{} does not exist; set HAETTY_REFERENCE_DIR", parent.display());
        std::fs::create_dir_all(&dir).expect("create reference dir");
        write(&fonts_file, format!("{fingerprint}\n").as_bytes());
    }
    if let Err(e) = std::fs::create_dir_all(&out) {
        println!("reftest: cannot create {}: {e}", out.display());
    }

    let mut painter = Painter::new(fonts, PX);
    painter.set_look(Look::of(&theme()));
    let mut failures = vec![];
    for (name, snap, preedit) in scenes() {
        let cv = render(&mut painter, &snap, preedit.as_ref());
        let (w, h) = (cv.width, cv.height);
        let actual_png = out.join(format!("{name}-actual.png"));
        let _ = std::fs::write(&actual_png, png_wh(w, h, &rgba(&cv.pixels)));
        let file = dir.join(format!("{name}.ccrf"));
        if update {
            let data = encode(w, h, &cv.pixels);
            write(&file, &data);
            println!("reftest {name}: wrote {} ({w}x{h}, {} bytes)", file.display(), data.len());
            continue;
        }
        let (ew, eh, expected) = match std::fs::read(&file).map_err(|e| e.to_string()).and_then(|d| decode(&d)) {
            Ok(r) => r,
            Err(e) => {
                failures.push(format!("{name}: cannot read {}: {e}", file.display()));
                continue;
            }
        };
        if (ew, eh) != (w, h) {
            failures.push(format!("{name}: size {w}x{h}, reference {ew}x{eh} (see {})", actual_png.display()));
            continue;
        }
        let d = compare(w, &expected, &cv.pixels);
        if d.count == 0 {
            println!("reftest {name}: identical ({w}x{h})");
            continue;
        }
        let diff_png = out.join(format!("{name}-diff.png"));
        let _ = std::fs::write(&diff_png, png_wh(w, h, &rgba(&diff_image(&cv.pixels, &d.mask))));
        let msg = format!("{name}: {}; see {} and {}", d.describe(), actual_png.display(), diff_png.display());
        if d.fails() {
            failures.push(msg);
        } else {
            println!("reftest {msg} (within tolerance)");
        }
    }
    assert!(failures.is_empty(), "reference frames differ:\n{}", failures.join("\n"));
}

#[test]
fn format_round_trip() {
    let a = pack(Rgb { r: 1, g: 2, b: 3 });
    let b = pack(Rgb { r: 250, g: 0, b: 128 });
    // A run longer than u16::MAX, single pixels and a short run.
    let mut px = vec![a; 70_000];
    px.extend([b, a, b, b, b]);
    px.extend(vec![b; 95]);
    let (w, h) = (701, 100);
    assert_eq!(px.len(), (w * h) as usize);
    let data = encode(w, h, &px);
    assert!(data.starts_with(b"CCRF1 701 100\n"));
    assert_eq!(decode(&data), Ok((w, h, px)));
    assert!(decode(b"CCRF2 1 1\n\x01\x00\x00\x00\x00").is_err());
    assert!(decode(b"CCRF1 2 1\n\x01\x00\x00\x00\x00").is_err(), "too few pixels");
    assert!(decode(b"CCRF1 1 1\n\x02\x00\x00\x00\x00").is_err(), "too many pixels");
}

#[test]
fn moved_glyph_fails_and_noise_passes() {
    // A frame-sized canvas with a 9x18 "glyph", then the same glyph one
    // pixel to the right: 36 pixels (0.03 %) differ, by a lot.
    let (w, h) = (600, 220);
    let frame = |dx: i32| {
        let mut cv = Canvas::new(w, h);
        cv.fill(cv.bounds(), BG);
        cv.fill(crate::render::canvas::Rect::new(100 + dx, 50, 9, 18), FG);
        cv.pixels
    };
    let base = frame(0);
    let same = compare(w, &base, &base);
    assert_eq!((same.count, same.bbox), (0, None));
    assert!(!same.fails());
    let moved = compare(w, &base, &frame(1));
    assert_eq!(moved.count, 36);
    assert_eq!(moved.bbox, Some((100, 50, 109, 67)));
    assert_eq!(moved.max_delta, 190);
    assert!(moved.fails());
    // Slightly different anti-aliasing on a few pixels is tolerated.
    let mut noisy = base.clone();
    for p in &mut noisy[..20] {
        *p = pack(Rgb { r: 40, g: 30, b: 30 });
    }
    let n = compare(w, &base, &noisy);
    assert_eq!((n.count, n.max_delta), (20, 10));
    assert!(!n.fails());
    // Many small differences are not.
    for p in &mut noisy[..200] {
        *p = pack(Rgb { r: 40, g: 30, b: 30 });
    }
    assert!(compare(w, &base, &noisy).fails());
}

fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
    }
    !c
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in data {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

/// An uncompressed RGBA PNG (stored deflate blocks): output for people to
/// look at, so size does not matter.
fn png_wh(w: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
    let mut raw = Vec::with_capacity(rgba.len() + h as usize);
    for row in rgba.chunks(w as usize * 4) {
        raw.push(0); // filter: none
        raw.extend_from_slice(row);
    }
    let mut z = vec![0x78, 0x01];
    let chunks: Vec<&[u8]> = raw.chunks(65_535).collect();
    for (i, c) in chunks.iter().enumerate() {
        z.push(u8::from(i + 1 == chunks.len()));
        z.extend_from_slice(&(c.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(c.len() as u16)).to_le_bytes());
        z.extend_from_slice(c);
    }
    z.extend_from_slice(&adler32(&raw).to_be_bytes());
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut chunk = |kind: &[u8], body: &[u8]| {
        out.extend_from_slice(&(body.len() as u32).to_be_bytes());
        let mut c = kind.to_vec();
        c.extend_from_slice(body);
        out.extend_from_slice(&c);
        out.extend_from_slice(&crc32(&c).to_be_bytes());
    };
    let mut ihdr = vec![];
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit RGBA
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &z);
    chunk(b"IEND", &[]);
    out
}
