//! Window pixels ↔ grid cells.

use ccterm_core::GridSize;

use crate::render::CellMetrics;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    pub cell: CellMetrics,
    pub padding: u32,
}

impl Layout {
    /// The grid that fits a window of `width` × `height` physical pixels
    /// (at least 1×1; the remainder is padding on the right/bottom).
    pub fn grid(&self, width: u32, height: u32) -> GridSize {
        let usable =
            |len: u32, cell: u32| (len.saturating_sub(2 * self.padding) / cell.max(1)).clamp(1, u16::MAX as u32);
        let cols = usable(width, self.cell.width) as u16;
        let rows = usable(height, self.cell.height) as u16;
        GridSize::new(cols, rows).with_cell(self.cell.width as u16, self.cell.height as u16)
    }

    /// Window size in physical pixels for `cols` × `rows`.
    pub fn window_size(&self, cols: u16, rows: u16) -> (u32, u32) {
        (cols as u32 * self.cell.width + 2 * self.padding, rows as u32 * self.cell.height + 2 * self.padding)
    }

    /// Pixel position relative to the grid origin (may be negative or past
    /// the grid near the edges).
    pub fn grid_px(&self, x: f64, y: f64) -> (f32, f32) {
        ((x - self.padding as f64) as f32, (y - self.padding as f64) as f32)
    }

    /// Cell under a window pixel position, clamped into the grid.
    pub fn cell_at(&self, x: f64, y: f64, grid: GridSize) -> (u16, u16) {
        let (gx, gy) = self.grid_px(x, y);
        let c = (gx.max(0.0) / self.cell.width as f32) as u16;
        let r = (gy.max(0.0) / self.cell.height as f32) as u16;
        (c.min(grid.cols.saturating_sub(1)), r.min(grid.rows.saturating_sub(1)))
    }

    /// Top-left pixel of a cell (for the IME cursor area).
    pub fn cell_origin(&self, col: u16, row: u16) -> (u32, u32) {
        (self.padding + col as u32 * self.cell.width, self.padding + row as u32 * self.cell.height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout() -> Layout {
        Layout { cell: CellMetrics { width: 9, height: 18 }, padding: 4 }
    }

    #[test]
    fn grid_from_window_size() {
        let l = layout();
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
    fn pixel_to_cell() {
        let l = layout();
        let g = l.grid(9 * 10 + 8, 18 * 5 + 8);
        assert_eq!(l.cell_at(0.0, 0.0, g), (0, 0));
        assert_eq!(l.cell_at(4.0 + 9.0 * 3.0 + 1.0, 4.0 + 18.0 * 2.0, g), (3, 2));
        assert_eq!(l.cell_at(10_000.0, 10_000.0, g), (9, 4));
        assert_eq!(l.cell_origin(3, 2), (4 + 27, 4 + 36));
    }
}
