//! Cell width of a character (DESIGN.md §10.2): the tables are generated from
//! libghostty-vt (`width_table.rs`), so the two engines agree by construction.

use crate::width_table::{VS_BASE, WIDE, ZERO};

fn within(t: &[(u32, u32)], c: u32) -> bool {
    t.binary_search_by(|&(a, b)| {
        if c < a {
            std::cmp::Ordering::Greater
        } else if c > b {
            std::cmp::Ordering::Less
        } else {
            std::cmp::Ordering::Equal
        }
    })
    .is_ok()
}

/// 0, 1 or 2 cells.
pub fn width(c: char) -> u8 {
    let c = c as u32;
    if c < 0x300 {
        return 1;
    }
    if within(ZERO, c) {
        0
    } else if within(WIDE, c) {
        2
    } else {
        1
    }
}

/// A variation selector (U+FE0E, U+FE0F) can follow this character.
pub fn vs_base(c: char) -> bool {
    within(VS_BASE, c as u32)
}

#[cfg(test)]
mod tests {
    #[test]
    fn widths() {
        assert_eq!(super::width('a'), 1);
        assert_eq!(super::width('\u{301}'), 0);
        assert_eq!(super::width('가'), 2);
        assert_eq!(super::width('😀'), 2);
    }
}
