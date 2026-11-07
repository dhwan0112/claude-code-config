//! Colour specifications as libghostty-vt reads them (`RGB.parse` in
//! terminal/color.zig): `#rgb`, `#rrggbb`, `#rrrgggbbb`, `#rrrrggggbbbb`,
//! `rgb`/`rrggbb` without `#`, `rgb:R/G/B` (1–4 hex digits each),
//! `rgbi:R/G/B` (0.0–1.0) and the X11 colour names. OSC 4/10/11/12 and the
//! theme files use the same syntax.

use crate::x11_colors::X11;

/// One component: 1–4 hex digits scaled to a byte, truncating (as libghostty).
fn hex(v: &str) -> Option<u8> {
    if v.is_empty() || v.len() > 4 {
        return None;
    }
    let c = u32::from_str_radix(v, 16).ok()?;
    let max = (1u32 << (4 * v.len())) - 1;
    Some((c * 255 / max) as u8)
}

fn intensity(v: &str) -> Option<u8> {
    let i: f64 = v.parse().ok()?;
    if !(0.0..=1.0).contains(&i) {
        return None;
    }
    Some((i * 255.0) as u8)
}

pub fn parse_color(value: &str) -> Option<[u8; 3]> {
    let s = value.trim_matches([' ', '\t']);
    if s.is_empty() || !s.is_ascii() {
        return None;
    }
    if let Some(h) = s.strip_prefix('#') {
        let n = match h.len() {
            3 => 1,
            6 => 2,
            9 => 3,
            12 => 4,
            _ => return None,
        };
        return Some([hex(&h[..n])?, hex(&h[n..2 * n])?, hex(&h[2 * n..])?]);
    }
    let lower = s.to_ascii_lowercase();
    if let Ok(i) = X11.binary_search_by(|(n, _)| (*n).cmp(lower.as_str())) {
        return Some(X11[i].1);
    }
    match s.len() {
        3 => return Some([hex(&s[..1])?, hex(&s[1..2])?, hex(&s[2..])?]),
        6 => return Some([hex(&s[..2])?, hex(&s[2..4])?, hex(&s[4..])?]),
        _ => {}
    }
    let (spec, scale): (&str, fn(&str) -> Option<u8>) = match s.strip_prefix("rgbi:") {
        Some(r) => (r, intensity),
        None => (s.strip_prefix("rgb:")?, hex),
    };
    let mut parts = spec.splitn(3, '/');
    let (r, g, b) = (parts.next()?, parts.next()?, parts.next()?);
    Some([scale(r)?, scale(g)?, scale(b)?])
}

/// A palette entry in Ghostty's config syntax, `N=COLOR` (N 0–255, decimal
/// or `0x`/`0o`/`0b`).
pub fn parse_palette_entry(value: &str) -> Option<(u8, [u8; 3])> {
    let (n, c) = value.split_once('=')?;
    let n = n.trim_matches([' ', '\t']);
    let i = if let Some(h) = n.strip_prefix("0x") {
        u8::from_str_radix(h, 16).ok()?
    } else if let Some(o) = n.strip_prefix("0o") {
        u8::from_str_radix(o, 8).ok()?
    } else if let Some(b) = n.strip_prefix("0b") {
        u8::from_str_radix(b, 2).ok()?
    } else {
        n.parse().ok()?
    };
    Some((i, parse_color(c)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours() {
        assert_eq!(parse_color("#fff"), Some([255; 3]));
        assert_eq!(parse_color(" 1e1e1e "), Some([0x1e; 3]));
        assert_eq!(parse_color("rgb:ffff/0/80"), Some([255, 0, 0x80]));
        assert_eq!(parse_color("rgb:801/0/0"), Some([127, 0, 0]), "truncates (0x801 is 127.6)");
        assert_eq!(parse_color("rgbi:1/0.5/0"), Some([255, 127, 0]));
        assert_eq!(parse_color("Ghost White"), Some([248, 248, 255]));
        assert_eq!(parse_color("#ff"), None);
        assert_eq!(parse_palette_entry("0x0f = #000"), Some((15, [0; 3])));
    }
}
