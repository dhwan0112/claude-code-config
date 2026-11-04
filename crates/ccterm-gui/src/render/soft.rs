//! CPU renderer: paints into an owned [`Canvas`] and copies it to a
//! softbuffer surface.
//!
//! The canvas is the back buffer. softbuffer's own buffer may be an older one
//! (Wayland double-buffers; `age()` tells), so it is never painted into
//! directly: dirty rows are copied when the buffer is the previous frame
//! (`age() == 1`), otherwise the whole canvas is.

use std::num::NonZeroU32;
use std::rc::Rc;

use winit::window::Window;

use super::canvas::Canvas;
use super::paint::Painter;
use super::{CellMetrics, RenderError, Renderer, Scene};

pub struct SoftRenderer {
    _ctx: softbuffer::Context<Rc<Window>>,
    surface: softbuffer::Surface<Rc<Window>, Rc<Window>>,
    painter: Painter,
    canvas: Canvas,
    padding: u32,
    /// Window contents must be redrawn completely (size, font or padding changed).
    stale: bool,
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
            padding: 0,
            stale: true,
            dump: std::env::var_os("CCTERM_DUMP_FRAME").map(Into::into),
        })
    }
}

impl Renderer for SoftRenderer {
    fn cell_metrics(&self) -> CellMetrics {
        self.painter.cell_metrics()
    }

    fn set_font_px(&mut self, px: f32) {
        self.painter.set_px(px);
        self.stale = true;
    }

    fn set_padding(&mut self, px: u32) {
        if px != self.padding {
            self.padding = px;
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
    }

    fn render(&mut self, scene: &Scene) -> Result<(), RenderError> {
        let (w, h) = (self.canvas.width, self.canvas.height);
        if w == 0 || h == 0 {
            return Ok(());
        }
        let full = scene.full || self.stale;
        self.stale = false;
        let m = self.painter.metrics();
        let origin = (self.padding as i32, self.padding as i32);
        if full {
            self.canvas.reset_clip();
            self.canvas.fill(self.canvas.bounds(), scene.snap.default_bg);
        }
        let mut bands: Vec<(i32, i32)> = vec![];
        for y in 0..scene.snap.rows {
            if full || scene.dirty.get(y as usize).copied().unwrap_or(true) {
                self.painter.paint_row(&mut self.canvas, origin, scene, y);
                let top = origin.1 + y as i32 * m.cell_h;
                match bands.last_mut() {
                    Some((_, end)) if *end == top => *end = top + m.cell_h,
                    _ => bands.push((top, top + m.cell_h)),
                }
            }
        }
        if !full && bands.is_empty() {
            return Ok(());
        }

        let mut buf = self.surface.buffer_mut().map_err(|e| RenderError(e.to_string()))?;
        let copy_all = full || buf.age() != 1 || buf.len() != self.canvas.pixels.len();
        if copy_all {
            let n = buf.len().min(self.canvas.pixels.len());
            buf[..n].copy_from_slice(&self.canvas.pixels[..n]);
        } else {
            for &(y0, y1) in &bands {
                let (a, b) = (clamp_row(y0, h), clamp_row(y1, h));
                let (s, e) = (a as usize * w as usize, b as usize * w as usize);
                buf[s..e].copy_from_slice(&self.canvas.pixels[s..e]);
            }
        }
        let result = if full || copy_all {
            buf.present()
        } else {
            let rects: Vec<softbuffer::Rect> = bands
                .iter()
                .filter_map(|&(y0, y1)| {
                    let (a, b) = (clamp_row(y0, h), clamp_row(y1, h));
                    Some(softbuffer::Rect {
                        x: 0,
                        y: a,
                        width: NonZeroU32::new(w)?,
                        height: NonZeroU32::new(b.checked_sub(a)?)?,
                    })
                })
                .collect();
            buf.present_with_damage(&rects)
        };
        result.map_err(|e| RenderError(e.to_string()))?;
        if let Some(p) = &self.dump {
            if let Err(e) = std::fs::write(p, self.canvas.to_ppm()) {
                log::warn!("frame dump {}: {e}", p.display());
            }
        }
        Ok(())
    }
}

fn clamp_row(y: i32, h: u32) -> u32 {
    y.clamp(0, h as i32) as u32
}
