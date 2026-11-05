//! Which rows changed between two snapshots.

use haetty_core::Snapshot;

/// Per-row dirty flags for `next` relative to `prev`. Everything is dirty
/// when there is no previous frame or the grid size or default colours
/// changed. Rows holding the old or new cursor are dirty when the cursor
/// moved or changed shape.
pub fn dirty_rows(prev: Option<&Snapshot>, next: &Snapshot) -> Vec<bool> {
    let rows = next.rows as usize;
    let Some(prev) = prev else { return vec![true; rows] };
    if prev.cols != next.cols
        || prev.rows != next.rows
        || prev.default_fg != next.default_fg
        || prev.default_bg != next.default_bg
        || prev.cursor_color != next.cursor_color
    {
        return vec![true; rows];
    }
    let cols = next.cols as usize;
    let mut dirty: Vec<bool> =
        (0..rows).map(|y| prev.cells[y * cols..(y + 1) * cols] != next.cells[y * cols..(y + 1) * cols]).collect();
    if prev.cursor != next.cursor {
        for c in [prev.cursor, next.cursor].into_iter().flatten() {
            if let Some(d) = dirty.get_mut(c.y as usize) {
                *d = true;
            }
        }
    }
    dirty
}

/// OR `more` into `acc` (frames merged before a redraw happens).
pub fn merge(acc: &mut Vec<bool>, more: &[bool]) {
    if acc.len() != more.len() {
        *acc = more.to_vec();
        return;
    }
    for (a, b) in acc.iter_mut().zip(more) {
        *a |= *b;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use haetty_core::{Cell, Cursor, CursorShape, Rgb};

    fn snap(cols: u16, rows: u16) -> Snapshot {
        Snapshot { cols, rows, cells: vec![Cell::default(); cols as usize * rows as usize], ..Default::default() }
    }

    #[test]
    fn first_frame_and_resize_are_fully_dirty() {
        let a = snap(4, 3);
        assert_eq!(dirty_rows(None, &a), vec![true; 3]);
        let b = snap(5, 3);
        assert_eq!(dirty_rows(Some(&a), &b), vec![true; 3]);
        let mut c = snap(4, 3);
        c.default_bg = Rgb { r: 1, g: 2, b: 3 };
        assert_eq!(dirty_rows(Some(&a), &c), vec![true; 3]);
    }

    #[test]
    fn only_changed_rows_are_dirty() {
        let a = snap(4, 3);
        let mut b = a.clone();
        assert_eq!(dirty_rows(Some(&a), &b), vec![false; 3]);
        b.cells[4 + 2].text = "x".into();
        assert_eq!(dirty_rows(Some(&a), &b), vec![false, true, false]);
        let mut c = a.clone();
        c.cells[0].flags.0 = haetty_core::CellFlags::BOLD;
        assert_eq!(dirty_rows(Some(&a), &c), vec![true, false, false]);
    }

    #[test]
    fn cursor_moves_dirty_old_and_new_rows() {
        let mut a = snap(4, 3);
        a.cursor = Some(Cursor { x: 0, y: 0, shape: CursorShape::Block, blinking: false });
        let mut b = a.clone();
        b.cursor = Some(Cursor { x: 1, y: 2, shape: CursorShape::Block, blinking: false });
        assert_eq!(dirty_rows(Some(&a), &b), vec![true, false, true]);
        let mut c = a.clone();
        c.cursor = None;
        assert_eq!(dirty_rows(Some(&a), &c), vec![true, false, false]);
    }

    #[test]
    fn merge_ors_flags() {
        let mut acc = vec![true, false, false];
        merge(&mut acc, &[false, false, true]);
        assert_eq!(acc, vec![true, false, true]);
        merge(&mut acc, &[false; 2]);
        assert_eq!(acc, vec![false; 2]);
    }
}
