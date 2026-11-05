//! Emoji presentation (UTS #51): whether a character is drawn as a colour
//! emoji by default, and the variation selectors that override it.

/// Text or emoji presentation of a character.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Presentation {
    Text,
    Emoji,
}

impl Presentation {
    /// Presentation of a cell's text: VS16 (U+FE0F) asks for emoji, VS15
    /// (U+FE0E) for text, otherwise the first character's default
    /// (`Emoji_Presentation`). libghostty-vt keeps the selector in the cell
    /// text (`ccterm-core` test `variation_selectors_stay_in_cell_text`).
    pub fn of(text: &str) -> Presentation {
        if text.contains('\u{fe0f}') {
            Presentation::Emoji
        } else if text.contains('\u{fe0e}') {
            Presentation::Text
        } else if text.chars().next().is_some_and(emoji_by_default) {
            Presentation::Emoji
        } else {
            Presentation::Text
        }
    }
}

/// `Emoji_Presentation=Yes`: drawn as emoji without a variation selector.
pub fn emoji_by_default(c: char) -> bool {
    let c = c as u32;
    EMOJI_PRESENTATION
        .binary_search_by(|&(lo, hi)| {
            if hi < c {
                std::cmp::Ordering::Less
            } else if lo > c {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

/// `Emoji_Presentation=Yes` ranges from Unicode 16.0 `emoji-data.txt`
/// (289 entries merged into 80 ranges, 640 bytes).
#[rustfmt::skip]
const EMOJI_PRESENTATION: [(u32, u32); 80] = [
    (0x231A, 0x231B), (0x23E9, 0x23EC), (0x23F0, 0x23F0), (0x23F3, 0x23F3), (0x25FD, 0x25FE), (0x2614, 0x2615),
    (0x2648, 0x2653), (0x267F, 0x267F), (0x2693, 0x2693), (0x26A1, 0x26A1), (0x26AA, 0x26AB), (0x26BD, 0x26BE),
    (0x26C4, 0x26C5), (0x26CE, 0x26CE), (0x26D4, 0x26D4), (0x26EA, 0x26EA), (0x26F2, 0x26F3), (0x26F5, 0x26F5),
    (0x26FA, 0x26FA), (0x26FD, 0x26FD), (0x2705, 0x2705), (0x270A, 0x270B), (0x2728, 0x2728), (0x274C, 0x274C),
    (0x274E, 0x274E), (0x2753, 0x2755), (0x2757, 0x2757), (0x2795, 0x2797), (0x27B0, 0x27B0), (0x27BF, 0x27BF),
    (0x2B1B, 0x2B1C), (0x2B50, 0x2B50), (0x2B55, 0x2B55), (0x1F004, 0x1F004), (0x1F0CF, 0x1F0CF),
    (0x1F18E, 0x1F18E), (0x1F191, 0x1F19A), (0x1F1E6, 0x1F1FF), (0x1F201, 0x1F201), (0x1F21A, 0x1F21A),
    (0x1F22F, 0x1F22F), (0x1F232, 0x1F236), (0x1F238, 0x1F23A), (0x1F250, 0x1F251), (0x1F300, 0x1F320),
    (0x1F32D, 0x1F335), (0x1F337, 0x1F37C), (0x1F37E, 0x1F393), (0x1F3A0, 0x1F3CA), (0x1F3CF, 0x1F3D3),
    (0x1F3E0, 0x1F3F0), (0x1F3F4, 0x1F3F4), (0x1F3F8, 0x1F43E), (0x1F440, 0x1F440), (0x1F442, 0x1F4FC),
    (0x1F4FF, 0x1F53D), (0x1F54B, 0x1F54E), (0x1F550, 0x1F567), (0x1F57A, 0x1F57A), (0x1F595, 0x1F596),
    (0x1F5A4, 0x1F5A4), (0x1F5FB, 0x1F64F), (0x1F680, 0x1F6C5), (0x1F6CC, 0x1F6CC), (0x1F6D0, 0x1F6D2),
    (0x1F6D5, 0x1F6D7), (0x1F6DC, 0x1F6DF), (0x1F6EB, 0x1F6EC), (0x1F6F4, 0x1F6FC), (0x1F7E0, 0x1F7EB),
    (0x1F7F0, 0x1F7F0), (0x1F90C, 0x1F93A), (0x1F93C, 0x1F945), (0x1F947, 0x1F9FF), (0x1FA70, 0x1FA7C),
    (0x1FA80, 0x1FA89), (0x1FA8F, 0x1FAC6), (0x1FACE, 0x1FADC), (0x1FADF, 0x1FAE9), (0x1FAF0, 0x1FAF8),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_variation_selectors() {
        // Text by default — what Claude Code draws in its status line.
        for c in ['\u{23f8}', '\u{26a0}', '\u{2714}', '\u{2713}', '\u{25cf}', 'A', '한'] {
            assert!(!emoji_by_default(c), "{c}");
            assert_eq!(Presentation::of(&c.to_string()), Presentation::Text);
            assert_eq!(Presentation::of(&format!("{c}\u{fe0f}")), Presentation::Emoji);
        }
        // Emoji by default.
        for c in ['\u{231a}', '\u{2705}', '\u{274c}', '\u{1f600}', '\u{1fae8}'] {
            assert!(emoji_by_default(c), "{c}");
            assert_eq!(Presentation::of(&c.to_string()), Presentation::Emoji);
            assert_eq!(Presentation::of(&format!("{c}\u{fe0e}")), Presentation::Text);
        }
        assert_eq!(Presentation::of(""), Presentation::Text);
        // Sorted and disjoint, so the binary search is valid.
        assert!(EMOJI_PRESENTATION.windows(2).all(|w| w[0].1 < w[1].0));
        assert!(EMOJI_PRESENTATION.iter().all(|&(lo, hi)| lo <= hi));
    }
}
