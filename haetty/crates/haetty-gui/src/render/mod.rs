//! Drawing. [`Renderer`] is what the window code talks to; the CPU backend
//! ([`soft::SoftRenderer`]) paints with [`paint::Painter`] into a pixel buffer
//! and hands it to softbuffer. Another backend (e.g. OpenGL) only has to
//! implement the trait.

pub mod boxdraw;
pub mod canvas;
pub mod paint;
pub mod soft;

use haetty_core::{Rgb, Snapshot};

use crate::config::PaddingColor;

/// Cell size in physical pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellMetrics {
    pub width: u32,
    pub height: u32,
}

/// IME composition shown inline at the cursor.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Preedit {
    pub text: String,
    /// Caret inside `text` as a byte range; `None` = do not draw a caret.
    pub cursor: Option<(usize, usize)>,
}

/// Everything a frame shows.
pub struct Scene<'a> {
    pub snap: &'a Snapshot,
    /// Rows that changed since the last frame (`len == snap.rows`).
    pub dirty: &'a [bool],
    /// Repaint everything, including the padding.
    pub full: bool,
    pub focused: bool,
    pub preedit: Option<&'a Preedit>,
}

/// How the area around the grid is laid out and coloured.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaddingStyle {
    /// Minimum padding, physical pixels.
    pub min: u32,
    pub balance: bool,
    pub color: PaddingColor,
    /// Paint everything outside the grid red (`HAETTY_DEBUG_PADDING=1`), to
    /// tell application padding from window-manager frames.
    pub debug: bool,
}

impl Default for PaddingStyle {
    fn default() -> Self {
        PaddingStyle { min: 0, balance: false, color: PaddingColor::Background, debug: false }
    }
}

pub const DEBUG_PADDING: Rgb = Rgb { r: 255, g: 0, b: 0 };

#[derive(Debug)]
pub struct RenderError(pub String);

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

pub trait Renderer {
    fn cell_metrics(&self) -> CellMetrics;
    /// Font size in physical pixels (point size × scale factor × 96/72).
    fn set_font_px(&mut self, px: f32);
    /// Padding around the grid (size in physical pixels, balance, colour).
    fn set_padding(&mut self, style: PaddingStyle);
    /// Window size in physical pixels.
    fn resize(&mut self, width: u32, height: u32);
    fn render(&mut self, scene: &Scene) -> Result<(), RenderError>;
    /// Heap memory held by the renderer, for the log.
    fn memory(&self) -> String;
}
