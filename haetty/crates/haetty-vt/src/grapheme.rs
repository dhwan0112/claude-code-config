//! Grapheme cluster breaks for mode 2027 (DESIGN.md §10.7): uucode's
//! `computeGraphemeBreak` (UAX #29 with GB9c and an emoji modifier tailoring),
//! which libghostty-vt uses, on the classes in `grapheme_table.rs`.

use crate::grapheme_table::CLASSES;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gb {
    Other,
    Control,
    Prepend,
    Cr,
    Lf,
    RegionalIndicator,
    SpacingMark,
    L,
    V,
    T,
    Lv,
    Lvt,
    Zwj,
    Zwnj,
    ExtendedPictographic,
    EmojiModifierBase,
    EmojiModifier,
    InCBExtend,
    InCBLinker,
    InCBConsonant,
}

const ORDER: [Gb; 20] = [
    Gb::Other,
    Gb::Control,
    Gb::Prepend,
    Gb::Cr,
    Gb::Lf,
    Gb::RegionalIndicator,
    Gb::SpacingMark,
    Gb::L,
    Gb::V,
    Gb::T,
    Gb::Lv,
    Gb::Lvt,
    Gb::Zwj,
    Gb::Zwnj,
    Gb::ExtendedPictographic,
    Gb::EmojiModifierBase,
    Gb::EmojiModifier,
    Gb::InCBExtend,
    Gb::InCBLinker,
    Gb::InCBConsonant,
];

pub fn class(c: char) -> Gb {
    let c = c as u32;
    match CLASSES.binary_search_by(|&(a, b, _)| {
        if c < a {
            std::cmp::Ordering::Greater
        } else if c > b {
            std::cmp::Ordering::Less
        } else {
            std::cmp::Ordering::Equal
        }
    }) {
        Ok(i) => ORDER[CLASSES[i].2 as usize],
        Err(_) => Gb::Other,
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum State {
    #[default]
    Default,
    RegionalIndicator,
    ExtendedPictographic,
    IndicConsonant,
    IndicLinker,
}

fn is_extend(g: Gb) -> bool {
    matches!(g, Gb::Zwnj | Gb::InCBExtend | Gb::InCBLinker)
}

fn is_indic_extend(g: Gb) -> bool {
    matches!(g, Gb::InCBExtend | Gb::Zwj)
}

fn is_pictographic(g: Gb) -> bool {
    matches!(g, Gb::ExtendedPictographic | Gb::EmojiModifierBase)
}

/// Is there a break between `a` and `b`? Called in order along a cluster,
/// keeping `state`.
pub fn breaks(a: Gb, b: Gb, state: &mut State) -> bool {
    use Gb::*;
    let keeps_emoji = |g: Gb| {
        matches!(g, InCBExtend | InCBLinker | Zwnj | Zwj | ExtendedPictographic | EmojiModifierBase | EmojiModifier)
    };
    let keeps_indic = |g: Gb| matches!(g, InCBConsonant | InCBLinker | InCBExtend | Zwj);
    match *state {
        State::RegionalIndicator if a != RegionalIndicator || b != RegionalIndicator => *state = State::Default,
        State::ExtendedPictographic if !keeps_emoji(a) || !keeps_emoji(b) => *state = State::Default,
        State::IndicConsonant | State::IndicLinker if !keeps_indic(a) || !keeps_indic(b) => *state = State::Default,
        _ => {}
    }
    // GB3–GB5
    if a == Cr && b == Lf {
        return false;
    }
    if matches!(a, Control | Cr | Lf) || matches!(b, Control | Cr | Lf) {
        return true;
    }
    // GB6–GB8: Hangul
    if a == L && matches!(b, L | V | Lv | Lvt) {
        return false;
    }
    if matches!(a, Lv | V) && matches!(b, V | T) {
        return false;
    }
    if matches!(a, Lvt | T) && b == T {
        return false;
    }
    // GB9a, GB9b
    if b == SpacingMark || a == Prepend {
        return false;
    }
    // GB9c: Indic conjuncts
    if a == InCBConsonant {
        if is_indic_extend(b) {
            *state = State::IndicConsonant;
            return false;
        } else if b == InCBLinker {
            *state = State::IndicLinker;
            return false;
        }
    } else if *state == State::IndicConsonant {
        if b == InCBLinker {
            *state = State::IndicLinker;
            return false;
        } else if is_indic_extend(b) {
            return false;
        }
        *state = State::Default;
    } else if *state == State::IndicLinker {
        if b == InCBLinker || is_indic_extend(b) {
            return false;
        } else if b == InCBConsonant {
            *state = State::Default;
            return false;
        }
        *state = State::Default;
    }
    // GB11: emoji ZWJ and modifier sequences
    if is_pictographic(a) {
        if is_extend(b) || b == Zwj {
            *state = State::ExtendedPictographic;
            return false;
        }
        if a == EmojiModifierBase && b == EmojiModifier {
            *state = State::ExtendedPictographic;
            return false;
        }
    } else if *state == State::ExtendedPictographic {
        if (is_extend(a) || a == EmojiModifier) && (is_extend(b) || b == Zwj) {
            return false;
        } else if a == Zwj && is_pictographic(b) {
            *state = State::Default;
            return false;
        }
        *state = State::Default;
    }
    // GB12, GB13: regional indicator pairs
    if a == RegionalIndicator && b == RegionalIndicator {
        if *state == State::Default {
            *state = State::RegionalIndicator;
            return false;
        }
        *state = State::Default;
        return true;
    }
    // GB9
    if is_extend(b) || b == Zwj {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn joins(s: &str) -> bool {
        let cs: Vec<char> = s.chars().collect();
        let mut st = State::Default;
        cs.windows(2).all(|w| !breaks(class(w[0]), class(w[1]), &mut st))
    }

    #[test]
    fn clusters() {
        assert!(joins("e\u{301}"));
        assert!(joins("👨\u{200d}👩"));
        assert!(joins("🇰🇷"));
        assert!(!joins("🇰🇷🇰"));
        assert!(joins("👍🏽"));
        assert!(joins("한\u{11a8}"));
        assert!(!joins("ab"));
    }
}
