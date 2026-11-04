//! Window-independent CPU painter: snapshot rows → pixels in a [`Canvas`].

use std::collections::HashMap;

use ccterm_core::{CellFlags, CursorShape, Rgb, Snapshot, Width};
use swash::scale::{image::Content, Render, ScaleContext, Source, StrikeWith};
use swash::zeno::{Angle, Format, Transform};

use super::boxdraw;
use super::canvas::{mix, Canvas, Rect};
use super::{CellMetrics, PaddingStyle, Preedit, Scene, DEBUG_PADDING};
use crate::config::PaddingColor;
use crate::font::{Fonts, Style};

/// Pixel metrics derived from the primary face at the current size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    pub px: f32,
    pub cell_w: i32,
    pub cell_h: i32,
    /// Baseline from the top of the cell.
    pub baseline: i32,
    /// Top of the underline from the top of the cell.
    pub underline: i32,
    /// Top of the strikethrough from the top of the cell.
    pub strike: i32,
    /// Line thickness for underline, strikethrough, box drawing, bar cursor.
    pub stroke: i32,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct GlyphKey {
    face: usize,
    glyph: u16,
    px_bits: u32,
    bold: bool,
    italic: bool,
}

struct Raster {
    left: i32,
    top: i32,
    width: i32,
    height: i32,
    color: bool,
    /// Alpha mask (1 byte per pixel) or RGBA (4 bytes per pixel).
    data: Vec<u8>,
}

/// How a glyph is placed in a span of `n` cells: the size it is rendered at
/// and the horizontal offset, so wide symbols from fallback fonts fit.
#[derive(Clone, Copy)]
struct Fit {
    px: f32,
    dx: i32,
}

pub struct Painter {
    fonts: Fonts,
    ctx: ScaleContext,
    glyphs: HashMap<GlyphKey, Option<Raster>>,
    fits: HashMap<(usize, u16, u8), Fit>,
    m: Metrics,
}

impl Painter {
    pub fn new(fonts: Fonts, px: f32) -> Self {
        let mut p =
            Painter { fonts, ctx: ScaleContext::new(), glyphs: HashMap::new(), fits: HashMap::new(), m: zero() };
        p.set_px(px);
        p
    }

    pub fn metrics(&self) -> Metrics {
        self.m
    }

    pub fn cell_metrics(&self) -> CellMetrics {
        CellMetrics { width: self.m.cell_w as u32, height: self.m.cell_h as u32 }
    }

    pub fn set_px(&mut self, px: f32) {
        let px = px.clamp(4.0, 200.0);
        if px == self.m.px {
            return;
        }
        self.glyphs.clear();
        self.fits.clear();
        let font = self.fonts.primary().font();
        let fm = font.metrics(&[]).scale(px);
        let gm = font.glyph_metrics(&[]).scale(px);
        let zero_glyph = font.charmap().map('0');
        let adv = if zero_glyph != 0 { gm.advance_width(zero_glyph) } else { fm.average_width };
        let cell_w = adv.round().max(1.0) as i32;
        let cell_h = (fm.ascent + fm.descent + fm.leading).round().max(1.0) as i32;
        let baseline = (fm.ascent + fm.leading / 2.0).round() as i32;
        let stroke = fm.stroke_size.round().max(1.0) as i32;
        // Underline below the baseline by the font's offset (sign conventions
        // differ between fonts and swash versions; the distance is what counts).
        let underline = (baseline + fm.underline_offset.abs().round().max(1.0) as i32).clamp(0, cell_h - stroke);
        let strike_off = if fm.strikeout_offset > 0.0 { fm.strikeout_offset } else { fm.x_height / 2.0 };
        let strike = (baseline - strike_off.round() as i32 - stroke / 2).clamp(0, cell_h - stroke);
        self.m = Metrics { px, cell_w, cell_h, baseline, underline, strike, stroke };
        log::debug!("metrics: {:?}", self.m);
    }

    /// Paint row `y` of the scene with the grid origin at `origin`. The whole
    /// window-wide band of the row is cleared first, so the row can be
    /// repainted on its own.
    pub fn paint_row(&mut self, cv: &mut Canvas, origin: (i32, i32), scene: &Scene, y: u16, pad: PaddingStyle) {
        let snap = scene.snap;
        let m = self.m;
        let top = origin.1 + y as i32 * m.cell_h;
        let band = Rect::new(0, top, cv.width as i32, m.cell_h);
        cv.reset_clip();
        cv.set_clip(band);
        cv.fill(band, snap.default_bg);
        // Left and right padding of this row.
        if snap.cols > 0 {
            let right = origin.0 + snap.cols as i32 * m.cell_w;
            cv.fill(Rect::new(0, top, origin.0, m.cell_h), side_color(snap, 0, y, pad));
            cv.fill(Rect::new(right, top, cv.width as i32 - right, m.cell_h), side_color(snap, snap.cols - 1, y, pad));
        }

        let cursor = snap.cursor.filter(|c| c.y == y);
        let cursor_block = cursor.is_some_and(|c| scene.focused && c.shape == CursorShape::Block);
        let cursor_color = snap.cursor_color.unwrap_or(snap.default_fg);

        // Pass 1: backgrounds.
        for x in 0..snap.cols {
            let cell = snap.cell(x, y).expect("cell");
            let (_, mut bg) = colors(snap, x, y);
            if cursor_block && cursor.is_some_and(|c| c.x == x || (c.x + 1 == x && cell.width == Width::Spacer)) {
                bg = cursor_color;
            }
            cv.fill(Rect::new(origin.0 + x as i32 * m.cell_w, top, m.cell_w, m.cell_h), bg);
        }

        // Pass 2: glyphs and decorations.
        for x in 0..snap.cols {
            let cell = snap.cell(x, y).expect("cell");
            if cell.width == Width::Spacer {
                continue;
            }
            let span = if cell.width == Width::Wide { 2 } else { 1 };
            let (mut fg, bg) = colors(snap, x, y);
            let on_cursor = cursor_block && cursor.is_some_and(|c| c.x == x);
            if on_cursor {
                fg = bg;
            }
            let rect = Rect::new(origin.0 + x as i32 * m.cell_w, top, m.cell_w * span, m.cell_h);
            let f = cell.flags;
            if !cell.text.is_empty() && cell.text != " " && !f.has(CellFlags::INVISIBLE) {
                let style = Style { bold: f.has(CellFlags::BOLD), italic: f.has(CellFlags::ITALIC) };
                self.draw_text(cv, &cell.text, rect, span as u8, style, fg);
            }
            if f.has(CellFlags::UNDERLINE) {
                cv.fill(Rect::new(rect.x, top + m.underline, rect.w, m.stroke), fg);
            }
            if f.has(CellFlags::STRIKETHROUGH) {
                cv.fill(Rect::new(rect.x, top + m.strike, rect.w, m.stroke), fg);
            }
            if f.has(CellFlags::OVERLINE) {
                cv.fill(Rect::new(rect.x, top, rect.w, m.stroke), fg);
            }
        }

        // Pass 3: non-block cursor shapes (and hollow when unfocused).
        if let Some(c) = cursor {
            let wide = snap.cell(c.x, y).is_some_and(|cell| cell.width == Width::Wide);
            let r = Rect::new(origin.0 + c.x as i32 * m.cell_w, top, m.cell_w * if wide { 2 } else { 1 }, m.cell_h);
            let t = m.stroke.max(1);
            match (scene.focused, c.shape) {
                (true, CursorShape::Block) => {}
                (true, CursorShape::Bar) => cv.fill(Rect::new(r.x, r.y, (t * 2).max(2), r.h), cursor_color),
                (true, CursorShape::Underline) => cv.fill(Rect::new(r.x, r.y + r.h - t * 2, r.w, t * 2), cursor_color),
                _ => cv.frame(r, t, cursor_color),
            }
        }

        // Pass 4: IME composition over the cells at the cursor.
        if let (Some(pre), Some(c)) = (scene.preedit, snap.cursor.filter(|c| c.y == y)) {
            self.paint_preedit(cv, origin, snap, c.x, y, pre);
        }
        cv.reset_clip();
    }

    /// Paint the padding above the grid (`top`) or below it, with the colours
    /// of the nearest row when extending applies. Returns the band painted
    /// (y, height); height 0 when there is no such padding.
    pub fn paint_vertical_padding(
        &mut self,
        cv: &mut Canvas,
        origin: (i32, i32),
        scene: &Scene,
        top: bool,
        pad: PaddingStyle,
    ) -> (i32, i32) {
        let snap = scene.snap;
        let m = self.m;
        let grid_bottom = origin.1 + snap.rows as i32 * m.cell_h;
        let (y0, h) = if top { (0, origin.1) } else { (grid_bottom, cv.height as i32 - grid_bottom) };
        if h <= 0 || snap.rows == 0 || snap.cols == 0 {
            return (y0, 0);
        }
        cv.reset_clip();
        let band = Rect::new(0, y0, cv.width as i32, h);
        let row = if top { 0 } else { snap.rows - 1 };
        if pad.debug {
            cv.fill(band, DEBUG_PADDING);
        } else if extends_vertically(snap, row, pad.color) {
            let right = origin.0 + snap.cols as i32 * m.cell_w;
            cv.fill(Rect::new(0, y0, origin.0, h), side_color(snap, 0, row, pad));
            cv.fill(Rect::new(right, y0, cv.width as i32 - right, h), side_color(snap, snap.cols - 1, row, pad));
            for x in 0..snap.cols {
                cv.fill(Rect::new(origin.0 + x as i32 * m.cell_w, y0, m.cell_w, h), colors(snap, x, row).1);
            }
        } else {
            cv.fill(band, snap.default_bg);
        }
        (y0, h)
    }

    fn paint_preedit(&mut self, cv: &mut Canvas, origin: (i32, i32), snap: &Snapshot, cx: u16, y: u16, pre: &Preedit) {
        let m = self.m;
        let top = origin.1 + y as i32 * m.cell_h;
        let (fg, bg) = (snap.default_fg, snap.default_bg);
        let mut col = cx as i32;
        let mut caret_x = None;
        for (i, ch) in pre.text.char_indices() {
            if pre.cursor.is_some_and(|(s, _)| s == i) {
                caret_x = Some(col);
            }
            let w = crate::text_width(ch) as i32;
            if w == 0 {
                continue;
            }
            let rect = Rect::new(origin.0 + col * m.cell_w, top, m.cell_w * w, m.cell_h);
            cv.fill(rect, bg);
            let mut buf = [0u8; 4];
            self.draw_text(cv, ch.encode_utf8(&mut buf), rect, w as u8, Style::default(), fg);
            cv.fill(Rect::new(rect.x, top + m.underline, rect.w, m.stroke), fg);
            col += w;
        }
        if pre.cursor.is_some_and(|(s, _)| s >= pre.text.len()) {
            caret_x = Some(col);
        }
        if let Some(x) = caret_x {
            let r = Rect::new(origin.0 + x * m.cell_w, top, (m.stroke * 2).max(2), m.cell_h);
            cv.fill(r, snap.cursor_color.unwrap_or(fg));
        }
    }

    fn draw_text(&mut self, cv: &mut Canvas, text: &str, rect: Rect, span: u8, style: Style, fg: Rgb) {
        let mut chars = text.chars();
        let Some(first) = chars.next() else { return };
        if boxdraw::draw(cv, first, rect, self.m.stroke, fg) {
            return;
        }
        // Combining marks after the base character are drawn at the same
        // origin; ZWJ sequences and variation selectors are not composed.
        let zwj = text.contains('\u{200d}');
        self.draw_char(cv, first, rect, span, style, fg);
        if !zwj {
            for c in chars.filter(|c| !matches!(*c as u32, 0xFE00..=0xFE0F | 0x200C | 0x200D)) {
                self.draw_char(cv, c, rect, span, style, fg);
            }
        }
    }

    fn draw_char(&mut self, cv: &mut Canvas, c: char, rect: Rect, span: u8, style: Style, fg: Rgb) {
        let Some(res) = self.fonts.resolve(c, style) else { return };
        let fit = self.fit(res.face, res.glyph, span);
        let key = GlyphKey {
            face: res.face,
            glyph: res.glyph,
            px_bits: fit.px.to_bits(),
            bold: res.synth_bold,
            italic: res.synth_italic,
        };
        if !self.glyphs.contains_key(&key) {
            let r = self.rasterize(key, fit.px);
            self.glyphs.insert(key, r);
        }
        let Some(g) = self.glyphs.get(&key).and_then(|g| g.as_ref()) else { return };
        let x0 = rect.x + fit.dx + g.left;
        let y0 = rect.y + self.m.baseline - g.top;
        // Clip to the cell span so a glyph never spills into a neighbour that
        // may be repainted without it.
        cv.set_clip(Rect::new(rect.x, rect.y, rect.w, rect.h));
        if g.color {
            for gy in 0..g.height {
                for gx in 0..g.width {
                    let i = ((gy * g.width + gx) * 4) as usize;
                    let (r, gg, b, a) = (g.data[i], g.data[i + 1], g.data[i + 2], g.data[i + 3]);
                    cv.blend(x0 + gx, y0 + gy, Rgb { r, g: gg, b }, a);
                }
            }
        } else {
            for gy in 0..g.height {
                for gx in 0..g.width {
                    let a = g.data[(gy * g.width + gx) as usize];
                    cv.blend(x0 + gx, y0 + gy, fg, a);
                }
            }
        }
        cv.set_clip(Rect::new(0, rect.y, cv.width as i32, rect.h));
    }

    /// Size and offset that make a glyph fit `span` cells: glyphs wider than
    /// the span are scaled down, and narrower glyphs in wide cells centred.
    fn fit(&mut self, face: usize, glyph: u16, span: u8) -> Fit {
        if let Some(f) = self.fits.get(&(face, glyph, span)) {
            return *f;
        }
        let px = self.m.px;
        let span_w = (self.m.cell_w * span as i32) as f32;
        let adv = self.fonts.face(face).font().glyph_metrics(&[]).scale(px).advance_width(glyph);
        let f = if adv > span_w * 1.05 && adv > 0.0 {
            let px2 = px * span_w / adv;
            Fit { px: px2, dx: 0 }
        } else if span > 1 && adv > 0.0 && adv < span_w {
            Fit { px, dx: ((span_w - adv) / 2.0).round() as i32 }
        } else {
            Fit { px, dx: 0 }
        };
        self.fits.insert((face, glyph, span), f);
        f
    }

    fn rasterize(&mut self, key: GlyphKey, px: f32) -> Option<Raster> {
        let font = self.fonts.face(key.face).font();
        let mut scaler = self.ctx.builder(font).size(px).hint(true).build();
        let sources = [
            Source::ColorOutline(0),
            Source::ColorBitmap(StrikeWith::BestFit),
            Source::Outline,
            Source::Bitmap(StrikeWith::BestFit),
        ];
        let mut render = Render::new(&sources);
        render.format(Format::Alpha);
        if key.bold {
            render.embolden((px / 28.0).max(0.5));
        }
        if key.italic {
            render.transform(Some(Transform::skew(Angle::from_degrees(12.0), Angle::ZERO)));
        }
        let img = render.render(&mut scaler, key.glyph)?;
        let p = img.placement;
        let (width, height) = (p.width as i32, p.height as i32);
        if width == 0 || height == 0 {
            return None;
        }
        let mut r = Raster { left: p.left, top: p.top, width, height, color: false, data: img.data };
        match img.content {
            Content::Mask => {}
            Content::Color => {
                r.color = true;
                // A colour bitmap strike may come back larger than asked for:
                // scale it down to the requested size (nearest neighbour).
                let want = (px * 1.2).ceil() as i32;
                if height > want {
                    r = downscale_color(r, want as f32 / height as f32);
                }
            }
            Content::SubpixelMask => {
                // Not requested; reduce to a plain mask.
                r.data = r.data.chunks(4).map(|c| ((c[0] as u16 + c[1] as u16 + c[2] as u16) / 3) as u8).collect();
            }
        }
        Some(r)
    }
}

fn zero() -> Metrics {
    Metrics { px: 0.0, cell_w: 1, cell_h: 1, baseline: 0, underline: 0, strike: 0, stroke: 1 }
}

fn downscale_color(r: Raster, s: f32) -> Raster {
    let w = ((r.width as f32 * s).round() as i32).max(1);
    let h = ((r.height as f32 * s).round() as i32).max(1);
    let mut data = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let sx = ((x as f32 + 0.5) / s) as i32;
            let sy = ((y as f32 + 0.5) / s) as i32;
            let i = ((sy.min(r.height - 1) * r.width + sx.min(r.width - 1)) * 4) as usize;
            data.extend_from_slice(&r.data[i..i + 4]);
        }
    }
    Raster {
        left: (r.left as f32 * s).round() as i32,
        top: (r.top as f32 * s).round() as i32,
        width: w,
        height: h,
        color: true,
        data,
    }
}

/// Colour of the left/right padding next to cell (x, y).
fn side_color(snap: &Snapshot, x: u16, y: u16, pad: PaddingStyle) -> Rgb {
    if pad.debug {
        return DEBUG_PADDING;
    }
    match pad.color {
        PaddingColor::Background => snap.default_bg,
        PaddingColor::Extend | PaddingColor::ExtendAlways => colors(snap, x, y).1,
    }
}

/// Whether the top/bottom padding takes the colours of row `y` (Ghostty's
/// heuristics for `extend`: not on the primary screen when the row has a cell
/// with the default background — default padding looks right then — or a
/// powerline glyph, which looks wrong stretched). Prompt rows, Ghostty's
/// third case, are not known here (no shell integration yet).
fn extends_vertically(snap: &Snapshot, y: u16, mode: PaddingColor) -> bool {
    match mode {
        PaddingColor::Background => false,
        PaddingColor::ExtendAlways => true,
        PaddingColor::Extend => {
            if snap.modes.alt_screen {
                return true;
            }
            !(0..snap.cols).any(|x| {
                let c = snap.cell(x, y).expect("cell");
                let default_bg = c.bg.is_none() && c.flags.has(CellFlags::INVERSE) == c.flags.has(CellFlags::SELECTED);
                default_bg || c.text.chars().any(is_powerline)
            })
        }
    }
}

fn is_powerline(c: char) -> bool {
    matches!(c as u32, 0xE0B0..=0xE0D7)
}

/// Foreground and background of cell (x, y) after inverse, selection, faint
/// and invisible.
pub fn colors(snap: &Snapshot, x: u16, y: u16) -> (Rgb, Rgb) {
    let cell = snap.cell(x, y).expect("cell");
    let f = cell.flags;
    let mut fg = cell.fg.unwrap_or(snap.default_fg);
    let mut bg = cell.bg.unwrap_or(snap.default_bg);
    if f.has(CellFlags::INVERSE) != f.has(CellFlags::SELECTED) {
        std::mem::swap(&mut fg, &mut bg);
    }
    if f.has(CellFlags::FAINT) {
        fg = mix(fg, bg, 128);
    }
    if f.has(CellFlags::INVISIBLE) {
        fg = bg;
    }
    (fg, bg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ccterm_core::{Cell, Cursor};

    const FG: Rgb = Rgb { r: 200, g: 200, b: 200 };
    const BG: Rgb = Rgb { r: 10, g: 10, b: 10 };
    const RED: Rgb = Rgb { r: 255, g: 0, b: 0 };

    fn snap(cols: u16, rows: u16) -> Snapshot {
        Snapshot {
            cols,
            rows,
            cells: vec![Cell::default(); cols as usize * rows as usize],
            default_fg: FG,
            default_bg: BG,
            ..Default::default()
        }
    }

    fn set(s: &mut Snapshot, x: u16, y: u16, c: Cell) {
        let i = y as usize * s.cols as usize + x as usize;
        s.cells[i] = c;
    }

    #[test]
    fn colors_apply_inverse_selection_faint_invisible() {
        let mut s = snap(4, 1);
        set(&mut s, 0, 0, Cell { fg: Some(RED), flags: CellFlags(CellFlags::INVERSE), ..Default::default() });
        set(&mut s, 1, 0, Cell { flags: CellFlags(CellFlags::SELECTED), ..Default::default() });
        set(&mut s, 2, 0, Cell { flags: CellFlags(CellFlags::INVERSE | CellFlags::SELECTED), ..Default::default() });
        set(&mut s, 3, 0, Cell { flags: CellFlags(CellFlags::FAINT), ..Default::default() });
        assert_eq!(colors(&s, 0, 0), (BG, RED));
        assert_eq!(colors(&s, 1, 0), (BG, FG));
        assert_eq!(colors(&s, 2, 0), (FG, BG));
        assert_eq!(colors(&s, 3, 0).0, mix(FG, BG, 128));
        let mut s = snap(1, 1);
        set(&mut s, 0, 0, Cell { flags: CellFlags(CellFlags::INVISIBLE), ..Default::default() });
        assert_eq!(colors(&s, 0, 0), (BG, BG));
    }

    // Tests below need a system font; they are skipped when none is found
    // (e.g. a minimal CI image).
    fn painter() -> Option<Painter> {
        Fonts::load(None).ok().map(|f| Painter::new(f, 16.0))
    }

    fn scene<'a>(s: &'a Snapshot, dirty: &'a [bool], focused: bool, pre: Option<&'a Preedit>) -> Scene<'a> {
        Scene { snap: s, dirty, full: true, focused, preedit: pre }
    }

    fn render(p: &mut Painter, sc: &Scene) -> Canvas {
        let m = p.metrics();
        let mut cv = Canvas::new((sc.snap.cols as i32 * m.cell_w) as u32, (sc.snap.rows as i32 * m.cell_h) as u32);
        for y in 0..sc.snap.rows {
            p.paint_row(&mut cv, (0, 0), sc, y, PaddingStyle::default());
        }
        cv
    }

    fn count(cv: &Canvas, r: Rect, pred: impl Fn(u32) -> bool) -> usize {
        let mut n = 0;
        for y in r.y..r.y + r.h {
            for x in r.x..r.x + r.w {
                if pred(cv.get(x, y)) {
                    n += 1;
                }
            }
        }
        n
    }

    #[test]
    fn wide_character_covers_two_cells_and_spacer_draws_nothing() {
        let Some(mut p) = painter() else { return };
        let mut s = snap(4, 1);
        set(&mut s, 0, 0, Cell { text: "한".into(), width: Width::Wide, ..Default::default() });
        set(&mut s, 1, 0, Cell { width: Width::Spacer, ..Default::default() });
        let cv = render(&mut p, &scene(&s, &[true], true, None));
        let m = p.metrics();
        let bgp = crate::render::canvas::pack(BG);
        let first = count(&cv, Rect::new(0, 0, m.cell_w, m.cell_h), |px| px != bgp);
        let second = count(&cv, Rect::new(m.cell_w, 0, m.cell_w, m.cell_h), |px| px != bgp);
        let rest = count(&cv, Rect::new(2 * m.cell_w, 0, 2 * m.cell_w, m.cell_h), |px| px != bgp);
        if p.fonts.resolve('한', Style::default()).is_some() {
            assert!(first > 0 && second > 0, "glyph spans both cells ({first}, {second})");
        }
        assert_eq!(rest, 0);
    }

    #[test]
    fn underline_and_strikethrough_rows() {
        let Some(mut p) = painter() else { return };
        let mut s = snap(2, 1);
        set(&mut s, 0, 0, Cell { flags: CellFlags(CellFlags::UNDERLINE), ..Default::default() });
        set(&mut s, 1, 0, Cell { flags: CellFlags(CellFlags::STRIKETHROUGH), ..Default::default() });
        let cv = render(&mut p, &scene(&s, &[true], true, None));
        let m = p.metrics();
        let fgp = crate::render::canvas::pack(FG);
        assert_eq!(cv.get(0, m.underline), fgp);
        assert_eq!(cv.get(m.cell_w - 1, m.underline), fgp);
        assert_eq!(cv.get(m.cell_w, m.strike), fgp);
        assert!(m.strike < m.underline);
        assert_ne!(cv.get(0, m.strike), fgp);
    }

    #[test]
    fn cursor_shapes_and_hollow_when_unfocused() {
        let Some(mut p) = painter() else { return };
        let m = p.metrics();
        let cc = crate::render::canvas::pack(FG);
        let mut s = snap(3, 1);
        let cell = Rect::new(m.cell_w, 0, m.cell_w, m.cell_h);
        for (shape, focused) in [
            (CursorShape::Block, true),
            (CursorShape::Bar, true),
            (CursorShape::Underline, true),
            (CursorShape::Block, false),
        ] {
            s.cursor = Some(Cursor { x: 1, y: 0, shape, blinking: false });
            let cv = render(&mut p, &scene(&s, &[true], focused, None));
            let lit = count(&cv, cell, |px| px == cc);
            let total = (m.cell_w * m.cell_h) as usize;
            let center = cv.get(m.cell_w + m.cell_w / 2, m.cell_h / 2) == cc;
            match (shape, focused) {
                (CursorShape::Block, true) => assert_eq!(lit, total),
                (CursorShape::Bar, true) => {
                    assert!(cv.get(m.cell_w, m.cell_h / 2) == cc && !center && lit < total / 2)
                }
                (CursorShape::Underline, true) => {
                    assert!(cv.get(m.cell_w + 1, m.cell_h - 1) == cc && !center && lit < total / 2)
                }
                _ => assert!(cv.get(m.cell_w, 0) == cc && cv.get(2 * m.cell_w - 1, m.cell_h - 1) == cc && !center),
            }
            // Neighbouring cells untouched.
            assert_eq!(count(&cv, Rect::new(0, 0, m.cell_w, m.cell_h), |px| px == cc), 0);
        }
    }

    #[test]
    fn block_cursor_shows_glyph_in_background_colour() {
        let Some(mut p) = painter() else { return };
        let mut s = snap(1, 1);
        set(&mut s, 0, 0, Cell { text: "M".into(), ..Default::default() });
        s.cursor = Some(Cursor { x: 0, y: 0, shape: CursorShape::Block, blinking: false });
        let cv = render(&mut p, &scene(&s, &[true], true, None));
        let bgp = crate::render::canvas::pack(BG);
        let m = p.metrics();
        assert!(count(&cv, Rect::new(0, 0, m.cell_w, m.cell_h), |px| px == bgp) > 0);
    }

    #[test]
    fn preedit_is_drawn_underlined_at_the_cursor() {
        let Some(mut p) = painter() else { return };
        let mut s = snap(6, 1);
        s.cursor = Some(Cursor { x: 2, y: 0, shape: CursorShape::Block, blinking: false });
        let pre = Preedit { text: "한".into(), cursor: None };
        let cv = render(&mut p, &scene(&s, &[true], true, Some(&pre)));
        let m = p.metrics();
        let fgp = crate::render::canvas::pack(FG);
        // Underline under both cells of the wide syllable, nothing after.
        assert_eq!(cv.get(2 * m.cell_w, m.underline), fgp);
        assert_eq!(cv.get(4 * m.cell_w - 1, m.underline), fgp);
        assert_ne!(cv.get(4 * m.cell_w + 1, m.underline), fgp);
    }

    #[test]
    fn rows_repaint_independently() {
        let Some(mut p) = painter() else { return };
        let mut s = snap(3, 2);
        set(&mut s, 0, 0, Cell { text: "g".into(), ..Default::default() });
        set(&mut s, 0, 1, Cell { text: "Ê".into(), ..Default::default() });
        let full = render(&mut p, &scene(&s, &[true, true], true, None));
        // Repaint row 1 alone on top of the full frame: identical result, so
        // no glyph leaks across rows.
        let mut again = Canvas::new(full.width, full.height);
        again.pixels.clone_from(&full.pixels);
        p.paint_row(&mut again, (0, 0), &scene(&s, &[false, true], true, None), 1, PaddingStyle::default());
        assert!(again.pixels == full.pixels);
    }

    const BLUE: Rgb = Rgb { r: 0, g: 0, b: 200 };

    fn padded(p: &mut Painter, s: &Snapshot, color: PaddingColor, debug: bool, pad: i32) -> Canvas {
        let m = p.metrics();
        let w = s.cols as i32 * m.cell_w + 2 * pad + 3; // +3: uneven leftover on the right
        let h = s.rows as i32 * m.cell_h + 2 * pad + 1;
        let mut cv = Canvas::new(w as u32, h as u32);
        let style = PaddingStyle { min: pad as u32, balance: false, color, debug };
        let sc = Scene { snap: s, dirty: &[], full: true, focused: true, preedit: None };
        for y in 0..s.rows {
            p.paint_row(&mut cv, (pad, pad), &sc, y, style);
        }
        p.paint_vertical_padding(&mut cv, (pad, pad), &sc, true, style);
        p.paint_vertical_padding(&mut cv, (pad, pad), &sc, false, style);
        cv
    }

    /// 2x2 grid, every cell blue except (0,1) which keeps the default bg.
    fn two_by_two() -> Snapshot {
        let mut s = snap(2, 2);
        for (x, y) in [(0, 0), (1, 0), (1, 1)] {
            set(&mut s, x, y, Cell { bg: Some(BLUE), ..Default::default() });
        }
        s
    }

    #[test]
    fn padding_background_mode_uses_default_bg_everywhere() {
        let Some(mut p) = painter() else { return };
        let cv = padded(&mut p, &two_by_two(), PaddingColor::Background, false, 6);
        let bg = crate::render::canvas::pack(BG);
        assert_eq!(cv.get(0, 0), bg);
        assert_eq!(cv.get(cv.width as i32 - 1, cv.height as i32 - 1), bg);
        assert_eq!(cv.get(0, 6), bg);
    }

    #[test]
    fn padding_extend_copies_edge_cells_with_heuristics() {
        let Some(mut p) = painter() else { return };
        let m = p.metrics();
        let s = two_by_two();
        let (blue, bg) = (crate::render::canvas::pack(BLUE), crate::render::canvas::pack(BG));
        let cv = padded(&mut p, &s, PaddingColor::Extend, false, 6);
        let (w, h) = (cv.width as i32, cv.height as i32);
        // Left/right padding always extends: row 0 is blue on both sides,
        // row 1 left is the default bg (cell (0,1)), right blue — including
        // the uneven leftover pixels.
        assert_eq!(cv.get(0, 6), blue);
        assert_eq!(cv.get(w - 1, 6), blue);
        assert_eq!(cv.get(0, 6 + m.cell_h), bg);
        assert_eq!(cv.get(w - 1, 6 + m.cell_h), blue);
        // Top: row 0 has no default-bg cell -> extended.
        assert_eq!(cv.get(6, 0), blue);
        assert_eq!(cv.get(w - 1, 0), blue);
        // Bottom: row 1 has a default-bg cell -> not extended (primary screen).
        assert_eq!(cv.get(6 + m.cell_w, h - 1), bg);
        // Alternate screen: heuristics off.
        let mut alt = s.clone();
        alt.modes.alt_screen = true;
        let cv = padded(&mut p, &alt, PaddingColor::Extend, false, 6);
        assert_eq!(cv.get(6 + m.cell_w, cv.height as i32 - 1), blue);
        // extend-always: no heuristics either.
        let cv = padded(&mut p, &s, PaddingColor::ExtendAlways, false, 6);
        assert_eq!(cv.get(6 + m.cell_w, cv.height as i32 - 1), blue);
        assert_eq!(cv.get(0, cv.height as i32 - 1), bg); // corner follows cell (0,1)
    }

    #[test]
    fn padding_extend_skips_powerline_rows() {
        let Some(mut p) = painter() else { return };
        let mut s = snap(2, 1);
        set(&mut s, 0, 0, Cell { bg: Some(BLUE), text: "\u{e0b0}".into(), ..Default::default() });
        set(&mut s, 1, 0, Cell { bg: Some(BLUE), ..Default::default() });
        let cv = padded(&mut p, &s, PaddingColor::Extend, false, 6);
        assert_eq!(cv.get(6, 0), crate::render::canvas::pack(BG));
    }

    #[test]
    fn debug_padding_is_red_and_grid_is_not() {
        let Some(mut p) = painter() else { return };
        let m = p.metrics();
        let cv = padded(&mut p, &two_by_two(), PaddingColor::Extend, true, 6);
        let red = crate::render::canvas::pack(DEBUG_PADDING);
        let (w, h) = (cv.width as i32, cv.height as i32);
        for (x, y) in [(0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1), (5, 10), (w - 2, 10), (10, 5), (10, h - 2)] {
            assert_eq!(cv.get(x, y), red, "({x},{y})");
        }
        assert_ne!(cv.get(6, 6), red);
        assert_ne!(cv.get(6 + 2 * m.cell_w - 1, 6 + 2 * m.cell_h - 1), red);
    }
}
