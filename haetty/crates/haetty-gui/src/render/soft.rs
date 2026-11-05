//! CPU renderer: paints into an owned [`Canvas`] and copies it to a
//! softbuffer surface.
//!
//! The canvas is the back buffer and always holds the complete current
//! frame. softbuffer's buffer is never assumed to hold the previous frame:
//! its `age()` says how many frames old its contents are (0 = unknown), and
//! the bands changed in that many frames are copied — everything when the
//! age is unknown or older than the kept history. Only the current frame's
//! bands are passed as damage to `present_with_damage`.

use std::collections::VecDeque;
use std::num::NonZeroU32;
use std::rc::Rc;

use winit::window::Window;

use super::canvas::Canvas;
use super::paint::Painter;
use super::{CellMetrics, PaddingStyle, RenderError, Renderer, Scene, DEBUG_PADDING};
use crate::geom::Layout;

/// Frames of damage remembered for buffers older than one frame.
const HISTORY: usize = 4;

/// Vertical bands `[y0, y1)` covering the full window width.
type Bands = Vec<(u32, u32)>;

pub struct SoftRenderer {
    _ctx: softbuffer::Context<Rc<Window>>,
    surface: softbuffer::Surface<Rc<Window>, Rc<Window>>,
    painter: Painter,
    canvas: Canvas,
    padding: PaddingStyle,
    /// Window contents must be redrawn completely (size, font or padding changed).
    stale: bool,
    /// Damage of previous frames, newest first; `None` = a full frame.
    history: VecDeque<Option<Bands>>,
    /// Optional path: every frame is also written there as PPM (debugging).
    dump: Option<std::path::PathBuf>,
}

impl SoftRenderer {
    pub fn new(window: Rc<Window>, painter: Painter) -> Result<Self, RenderError> {
        let err = |e: softbuffer::SoftBufferError| RenderError(e.to_string());
        let ctx = softbuffer::Context::new(window.clone()).map_err(err)?;
        let surface = softbuffer::Surface::new(&ctx, window).map_err(err)?;
        Ok(SoftRenderer {
            _ctx: ctx,
            surface,
            painter,
            canvas: Canvas::new(0, 0),
            padding: PaddingStyle::default(),
            stale: true,
            history: VecDeque::new(),
            dump: haetty_proto::product::env_os("DUMP_FRAME").map(Into::into),
        })
    }
}

/// Bands to copy into a buffer whose contents are `age` frames old, given the
/// current frame's damage (`None` = full) and the history (newest first).
/// `None` means copy everything.
pub fn bands_to_copy(age: u8, current: Option<&Bands>, history: &VecDeque<Option<Bands>>) -> Option<Bands> {
    let current = current?;
    let older = (age as usize).checked_sub(1)?; // age 0: contents unknown
    if older > history.len() {
        return None;
    }
    let mut all = current.clone();
    for frame in history.iter().take(older) {
        all.extend(frame.as_ref()?.iter().copied());
    }
    Some(merge_bands(all))
}

fn merge_bands(mut b: Bands) -> Bands {
    b.retain(|(a, z)| z > a);
    b.sort_unstable();
    let mut out: Bands = Vec::with_capacity(b.len());
    for (a, z) in b {
        match out.last_mut() {
            Some((_, end)) if a <= *end => *end = (*end).max(z),
            _ => out.push((a, z)),
        }
    }
    out
}

impl Renderer for SoftRenderer {
    fn memory(&self) -> String {
        format!(
            "canvas {}x{} {} KB, damage history {}; {}",
            self.canvas.width,
            self.canvas.height,
            self.canvas.pixels.capacity() * 4 / 1024,
            self.history.len(),
            self.painter.memory()
        )
    }

    fn cell_metrics(&self) -> CellMetrics {
        self.painter.cell_metrics()
    }

    fn set_font_px(&mut self, px: f32) {
        self.painter.set_px(px);
        self.stale = true;
    }

    fn set_padding(&mut self, style: PaddingStyle) {
        if style != self.padding {
            self.padding = style;
            self.stale = true;
        }
    }

    fn resize(&mut self, width: u32, height: u32) {
        if width == self.canvas.width && height == self.canvas.height {
            return;
        }
        self.canvas.resize(width, height);
        if let (Some(w), Some(h)) = (NonZeroU32::new(width), NonZeroU32::new(height)) {
            if let Err(e) = self.surface.resize(w, h) {
                log::error!("surface resize: {e}");
            }
        }
        self.stale = true;
        self.history.clear();
    }

    fn render(&mut self, scene: &Scene) -> Result<(), RenderError> {
        let (w, h) = (self.canvas.width, self.canvas.height);
        if w == 0 || h == 0 {
            return Ok(());
        }
        let snap = scene.snap;
        let full = scene.full || self.stale;
        self.stale = false;
        let m = self.painter.metrics();
        let pad = self.padding;
        let layout = Layout { cell: self.painter.cell_metrics(), padding: pad.min, balance: pad.balance };
        let (ox, oy) = layout.origin(w, h, snap.cols, snap.rows);
        let origin = (ox as i32, oy as i32);

        if full {
            // Every pixel, including the remainder past the last cell.
            self.canvas.reset_clip();
            let fill = if pad.debug { DEBUG_PADDING } else { snap.default_bg };
            self.canvas.fill(self.canvas.bounds(), fill);
        }
        let last = snap.rows.saturating_sub(1);
        let row_dirty = |y: u16| full || scene.dirty.get(y as usize).copied().unwrap_or(true);
        let mut bands: Bands = vec![];
        for y in 0..snap.rows {
            if row_dirty(y) {
                self.painter.paint_row(&mut self.canvas, origin, scene, y, pad);
                let top = (origin.1 + y as i32 * m.cell_h).max(0) as u32;
                bands.push((top, top + m.cell_h as u32));
            }
        }
        // Top/bottom padding follow the first/last row (extend).
        if snap.rows > 0 && row_dirty(0) {
            let (y0, hh) = self.painter.paint_vertical_padding(&mut self.canvas, origin, scene, true, pad);
            bands.push((y0.max(0) as u32, (y0 + hh).max(0) as u32));
        }
        if snap.rows > 0 && row_dirty(last) {
            let (y0, hh) = self.painter.paint_vertical_padding(&mut self.canvas, origin, scene, false, pad);
            bands.push((y0.max(0) as u32, (y0 + hh).max(0) as u32));
        }
        let bands: Bands = merge_bands(bands.into_iter().map(|(a, z)| (a.min(h), z.min(h))).collect());
        if !full && bands.is_empty() {
            return Ok(());
        }

        let current = if full { None } else { Some(bands.clone()) };
        let mut buf = self.surface.buffer_mut().map_err(|e| RenderError(e.to_string()))?;
        let copy = if buf.len() == self.canvas.pixels.len() {
            bands_to_copy(buf.age(), current.as_ref(), &self.history)
        } else {
            None
        };
        match &copy {
            None => {
                let n = buf.len().min(self.canvas.pixels.len());
                buf[..n].copy_from_slice(&self.canvas.pixels[..n]);
            }
            Some(list) => {
                for &(y0, y1) in list {
                    let (s, e) = (y0 as usize * w as usize, y1 as usize * w as usize);
                    buf[s..e].copy_from_slice(&self.canvas.pixels[s..e]);
                }
            }
        }
        let result = match &current {
            None => buf.present(),
            Some(list) => {
                let rects: Vec<softbuffer::Rect> = list
                    .iter()
                    .filter_map(|&(y0, y1)| {
                        Some(softbuffer::Rect {
                            x: 0,
                            y: y0,
                            width: NonZeroU32::new(w)?,
                            height: NonZeroU32::new(y1.checked_sub(y0)?)?,
                        })
                    })
                    .collect();
                buf.present_with_damage(&rects)
            }
        };
        result.map_err(|e| RenderError(e.to_string()))?;
        self.history.push_front(current);
        self.history.truncate(HISTORY);
        if let Some(p) = &self.dump {
            // Write and rename so readers never see a half-written file.
            let tmp = p.with_extension("tmp");
            if let Err(e) = std::fs::write(&tmp, self.canvas.to_ppm()).and_then(|_| std::fs::rename(&tmp, p)) {
                log::warn!("frame dump {}: {e}", p.display());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hist(frames: &[Option<Bands>]) -> VecDeque<Option<Bands>> {
        frames.iter().cloned().collect()
    }

    #[test]
    fn copy_depends_on_buffer_age() {
        let cur = vec![(10, 20)];
        let h = hist(&[Some(vec![(40, 50)]), Some(vec![(0, 5)]), None]);
        // Unknown contents or a full frame: copy everything.
        assert_eq!(bands_to_copy(0, Some(&cur), &h), None);
        assert_eq!(bands_to_copy(1, None, &h), None);
        // Previous frame: only the current damage.
        assert_eq!(bands_to_copy(1, Some(&cur), &h), Some(vec![(10, 20)]));
        // Two frames old (Wayland double buffering): current + previous.
        assert_eq!(bands_to_copy(2, Some(&cur), &h), Some(vec![(10, 20), (40, 50)]));
        assert_eq!(bands_to_copy(3, Some(&cur), &h), Some(vec![(0, 5), (10, 20), (40, 50)]));
        // The history reaches a full frame: copy everything.
        assert_eq!(bands_to_copy(4, Some(&cur), &h), None);
        // Older than the history: copy everything.
        assert_eq!(bands_to_copy(9, Some(&cur), &hist(&[])), None);
    }

    #[test]
    fn bands_merge() {
        assert_eq!(merge_bands(vec![(5, 10), (0, 5), (20, 30), (25, 26), (7, 7)]), vec![(0, 10), (20, 30)]);
    }
}
