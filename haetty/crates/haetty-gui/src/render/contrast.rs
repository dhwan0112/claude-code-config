//! `minimum-contrast` as Ghostty does it (`src/renderer/shaders`:
//! `contrasted_color`): when text and its background have a WCAG 2.0
//! contrast ratio below the minimum, the text becomes white or black,
//! whichever contrasts more with that background. 1 (the default) changes
//! nothing. Graphics characters (box drawing, block elements, legacy
//! computing symbols, Powerline) are left alone, as in Ghostty
//! (`noMinContrast`); colour emoji ignore the text colour anyway.

use haetty_core::Rgb;

const WHITE: Rgb = Rgb { r: 255, g: 255, b: 255 };
const BLACK: Rgb = Rgb { r: 0, g: 0, b: 0 };

fn linear(c: u8) -> f64 {
    let c = c as f64 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// WCAG 2.0 relative luminance (sRGB, linearised).
pub fn luminance(c: Rgb) -> f64 {
    0.2126 * linear(c.r) + 0.7152 * linear(c.g) + 0.0722 * linear(c.b)
}

/// WCAG 2.0 contrast ratio, 1–21.
pub fn ratio(a: Rgb, b: Rgb) -> f64 {
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// The text colour to draw `fg` on `bg` with at least `min` contrast.
pub fn contrasted(fg: Rgb, bg: Rgb, min: f32) -> Rgb {
    if min <= 1.0 || ratio(fg, bg) >= min as f64 {
        return fg;
    }
    if ratio(WHITE, bg) > ratio(BLACK, bg) {
        WHITE
    } else {
        BLACK
    }
}

/// Cells `minimum-contrast` leaves alone (Ghostty's `isGraphicsElement`).
pub fn exempt(text: &str) -> bool {
    text.chars()
        .next()
        .is_some_and(|c| matches!(c as u32, 0x2500..=0x259F | 0x1FB00..=0x1FBFF | 0x1CC00..=0x1CEBF | 0xE0B0..=0xE0D7))
}

#[cfg(test)]
mod tests {
    use super::*;
    use haetty_core::theme::Theme;

    const NAMES: [&str; 16] = [
        "Black",
        "Red",
        "Green",
        "Yellow (brown)",
        "Blue",
        "Magenta",
        "Cyan",
        "White",
        "Bright Black",
        "Bright Red",
        "Bright Green",
        "Bright Yellow",
        "Bright Blue",
        "Bright Magenta",
        "Bright Cyan",
        "Bright White",
    ];

    /// The table in assets/themes/README.md is this test's output
    /// (`cargo test -p haetty-gui contrast -- --nocapture`).
    #[test]
    fn cga_contrast_against_its_background() {
        let t = Theme::builtin("IBM 5153 CGA").unwrap();
        let r: Vec<f64> = t.palette.iter().map(|&c| ratio(c, t.background)).collect();
        println!("| # | name | colour | contrast on #141414 | at minimum-contrast 3 | at 4.5 |");
        println!("|---:|---|---|---:|---|---|");
        for (i, c) in t.palette.iter().enumerate() {
            let show = |m: f32| {
                let o = contrasted(*c, t.background, m);
                if o == *c {
                    "unchanged".to_string()
                } else {
                    format!("→ #{:02x}{:02x}{:02x}", o.r, o.g, o.b)
                }
            };
            println!(
                "| {i} | {} | `#{:02x}{:02x}{:02x}` | {:.2} | {} | {} |",
                NAMES[i],
                c.r,
                c.g,
                c.b,
                r[i],
                show(3.0),
                show(4.5)
            );
        }
        let fg = ratio(t.foreground, t.background);
        println!("| – | foreground | `#cecece` | {fg:.2} | unchanged | unchanged |");
        // The figures the user quoted: Blue 1.85, Bright Black 2.21.
        assert_eq!(format!("{:.2}", r[4]), "1.85");
        assert_eq!(format!("{:.2}", r[8]), "2.21");
        assert_eq!(format!("{:.2}", r[0]), "1.00", "colour 0 is the background");
        // Selection: #141414 on the #4e4e4e fill-in, and the cell's own text.
        assert_eq!(format!("{:.2}", ratio(t.background, Rgb { r: 0x4e, g: 0x4e, b: 0x4e })), "2.21");
        assert_eq!(format!("{:.2}", ratio(t.foreground, Rgb { r: 0x4e, g: 0x4e, b: 0x4e })), "5.29");
        assert_eq!(format!("{:.2}", ratio(BLACK, Rgb { r: 0x4e, g: 0x4e, b: 0x4e })), "2.52");
    }

    #[test]
    fn below_the_minimum_text_turns_white_or_black() {
        let bg = Rgb { r: 0x14, g: 0x14, b: 0x14 };
        let blue = Rgb { r: 0x1b, g: 0x1b, b: 0xd0 };
        assert_eq!(contrasted(blue, bg, 1.0), blue, "1 = off");
        assert_eq!(contrasted(blue, bg, 1.5), blue, "1.85 ≥ 1.5");
        assert_eq!(contrasted(blue, bg, 3.0), WHITE, "dark background → white");
        let light = Rgb { r: 0xf0, g: 0xf0, b: 0xf0 };
        assert_eq!(contrasted(Rgb { r: 0xe0, g: 0xe0, b: 0x40 }, light, 3.0), BLACK, "light background → black");
        assert_eq!(contrasted(bg, bg, 1.1), WHITE, "invisible text becomes visible");
        assert!((ratio(WHITE, BLACK) - 21.0).abs() < 1e-9);
        assert!(exempt("─") && exempt("█") && exempt("\u{e0b0}") && exempt("\u{1fb00}") && !exempt("a") && !exempt(""));
    }
}
