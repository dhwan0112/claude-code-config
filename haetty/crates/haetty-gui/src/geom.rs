//! Window pixels ↔ grid cells.

use haetty_core::GridSize;

use crate::render::CellMetrics;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    pub cell: CellMetrics,
    /// Minimum padding left/right (`.0`) and top/bottom (`.1`), physical
    /// pixels.
    pub padding: (u32, u32),
    /// Spread the space left over after whole cells evenly on all sides
    /// (Ghostty's `window-padding-balance`); otherwise it goes to the right
    /// and bottom.
    pub balance: bool,
}

impl Layout {
    /// The grid that fits a window of `width` × `height` physical pixels
    /// (at least 1×1).
    pub fn grid(&self, width: u32, height: u32) -> GridSize {
        let usable =
            |len: u32, cell: u32, pad: u32| (len.saturating_sub(2 * pad) / cell.max(1)).clamp(1, u16::MAX as u32);
        let cols = usable(width, self.cell.width, self.padding.0) as u16;
        let rows = usable(height, self.cell.height, self.padding.1) as u16;
        GridSize::new(cols, rows).with_cell(self.cell.width as u16, self.cell.height as u16)
    }

    /// Window size in physical pixels for `cols` × `rows`.
    pub fn window_size(&self, cols: u16, rows: u16) -> (u32, u32) {
        (cols as u32 * self.cell.width + 2 * self.padding.0, rows as u32 * self.cell.height + 2 * self.padding.1)
    }

    /// Top-left pixel of a `cols` × `rows` grid in a `width` × `height`
    /// window. Renderer and input mapping both use this.
    pub fn origin(&self, width: u32, height: u32, cols: u16, rows: u16) -> (u32, u32) {
        if !self.balance {
            return self.padding;
        }
        let gw = cols as u32 * self.cell.width;
        let gh = rows as u32 * self.cell.height;
        (width.saturating_sub(gw) / 2, height.saturating_sub(gh) / 2)
    }

    /// Pixel position relative to the grid origin (may be negative or past
    /// the grid near the edges).
    pub fn grid_px(&self, origin: (u32, u32), x: f64, y: f64) -> (f32, f32) {
        ((x - origin.0 as f64) as f32, (y - origin.1 as f64) as f32)
    }

    /// Cell under a window pixel position, clamped into the grid.
    pub fn cell_at(&self, origin: (u32, u32), x: f64, y: f64, grid: GridSize) -> (u16, u16) {
        let (gx, gy) = self.grid_px(origin, x, y);
        let c = (gx.max(0.0) / self.cell.width as f32) as u16;
        let r = (gy.max(0.0) / self.cell.height as f32) as u16;
        (c.min(grid.cols.saturating_sub(1)), r.min(grid.rows.saturating_sub(1)))
    }

    /// Top-left pixel of a cell (for the IME cursor area).
    pub fn cell_origin(&self, origin: (u32, u32), col: u16, row: u16) -> (u32, u32) {
        (origin.0 + col as u32 * self.cell.width, origin.1 + row as u32 * self.cell.height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(balance: bool) -> Layout {
        Layout { cell: CellMetrics { width: 9, height: 18 }, padding: (4, 4), balance }
    }

    /// Different horizontal and vertical padding (the defaults 10 / 6 px).
    #[test]
    fn padding_per_axis() {
        let l = Layout { cell: CellMetrics { width: 9, height: 18 }, padding: (10, 6), balance: false };
        assert_eq!(l.window_size(80, 24), (9 * 80 + 20, 18 * 24 + 12));
        let g = l.grid(9 * 80 + 20, 18 * 24 + 12);
        assert_eq!((g.cols, g.rows), (80, 24));
        assert_eq!(l.grid(9 * 80 + 19, 18 * 24 + 11).cols, 79, "one pixel short of the x padding");
        assert_eq!(l.origin(1000, 1000, 80, 24), (10, 6));
        let b = Layout { balance: true, ..l };
        assert_eq!(b.origin(9 * 80 + 20 + 4, 18 * 24 + 12 + 6, 80, 24), (12, 9), "leftover split evenly");
    }

    #[test]
    fn grid_from_window_size() {
        let l = layout(false);
        let g = l.grid(9 * 80 + 8, 18 * 24 + 8);
        assert_eq!((g.cols, g.rows, g.cell_width, g.cell_height), (80, 24, 9, 18));
        // Leftover pixels do not make a partial cell.
        let g = l.grid(9 * 80 + 8 + 8, 18 * 24 + 8 + 17);
        assert_eq!((g.cols, g.rows), (80, 24));
        // Tiny or zero-sized windows still give a 1x1 grid.
        let g = l.grid(0, 0);
        assert_eq!((g.cols, g.rows), (1, 1));
        assert_eq!(l.window_size(80, 24), (9 * 80 + 8, 18 * 24 + 8));
    }

    #[test]
    fn origin_with_and_without_balance() {
        // 80x24 grid = 720x432; window leaves 16 extra px horizontally and
        // 25 vertically.
        let (w, h) = (720 + 16, 432 + 25);
        assert_eq!(layout(false).origin(w, h, 80, 24), (4, 4));
        let (x, y) = layout(true).origin(w, h, 80, 24);
        assert_eq!((x, y), (8, 12));
        // Left/right and top/bottom differ by at most one pixel.
        assert!((w - 720 - x).abs_diff(x) <= 1 && (h - 432 - y).abs_diff(y) <= 1);
        // A grid larger than the window (transient during resize) clamps at 0.
        assert_eq!(layout(true).origin(100, 100, 80, 24), (0, 0));
    }

    #[test]
    fn pixel_to_cell() {
        let l = layout(false);
        let g = l.grid(9 * 10 + 8, 18 * 5 + 8);
        let o = l.origin(9 * 10 + 8, 18 * 5 + 8, g.cols, g.rows);
        assert_eq!(l.cell_at(o, 0.0, 0.0, g), (0, 0));
        assert_eq!(l.cell_at(o, 4.0 + 9.0 * 3.0 + 1.0, 4.0 + 18.0 * 2.0, g), (3, 2));
        assert_eq!(l.cell_at(o, 10_000.0, 10_000.0, g), (9, 4));
        assert_eq!(l.cell_origin(o, 3, 2), (4 + 27, 4 + 36));
        // Balanced: the origin moves, mapping follows it.
        let lb = layout(true);
        let o = lb.origin(9 * 10 + 20, 18 * 5 + 20, g.cols, g.rows);
        assert_eq!(o, (10, 10));
        assert_eq!(lb.cell_at(o, 10.0 + 9.0 * 3.0 + 1.0, 10.0, g), (3, 0));
    }
}
