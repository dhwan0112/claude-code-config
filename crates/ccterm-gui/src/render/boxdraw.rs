//! Box drawing (U+2500–U+257F) and block elements (U+2580–U+259F) drawn
//! procedurally so lines meet exactly at cell edges, whatever the font.

use ccterm_core::Rgb;

use super::canvas::{Canvas, Rect};

/// Arms of U+2500..=U+257F as "up right down left" weights: 0 none,
/// 1 light, 2 heavy, 3 double; `x` = drawn by a special case.
const ARMS: &str = "0101 0202 1010 2020 x x x x x x x x 0110 0210 0120 0220 \
0011 0012 0021 0022 1100 1200 2100 2200 1001 1002 2001 2002 \
1110 1210 2110 1120 2120 2210 1220 2220 1011 1012 2011 1021 2021 2012 1022 2022 \
0111 0112 0211 0212 0121 0122 0221 0222 1101 1102 1201 1202 2101 2102 2201 2202 \
1111 1112 1211 1212 2111 1121 2121 2112 2211 1122 1221 2212 1222 2122 2221 2222 \
x x x x \
0303 3030 0310 0130 0330 0013 0031 0033 1300 3100 3300 1003 3001 3003 1310 3130 \
3330 1013 3031 3033 0313 0131 0333 1303 3101 3303 1313 3131 3333 \
x x x x x x x \
0001 1000 0100 0010 0002 2000 0200 0020 0201 1020 0102 2010";

fn arms(c: char) -> Option<[u8; 4]> {
    let i = (c as u32).checked_sub(0x2500)? as usize;
    let tok = ARMS.split_ascii_whitespace().nth(i)?;
    let b = tok.as_bytes();
    if b.len() != 4 {
        return None;
    }
    Some([b[0] - b'0', b[1] - b'0', b[2] - b'0', b[3] - b'0'])
}

/// Draw `c` into the cell `r`. `light` is the light stroke width in pixels.
/// Returns false if the character is not handled here.
pub fn draw(canvas: &mut Canvas, c: char, r: Rect, light: i32, fg: Rgb) -> bool {
    let cp = c as u32;
    if !(0x2500..=0x259F).contains(&cp) {
        return false;
    }
    if cp >= 0x2580 {
        block(canvas, cp, r, fg);
        return true;
    }
    let light = light.max(1);
    match cp {
        // Dashed lines: (count, heavy, vertical).
        0x2504..=0x250B | 0x254C..=0x254F => {
            let (n, heavy, vertical) = match cp {
                0x2504 => (3, false, false),
                0x2505 => (3, true, false),
                0x2506 => (3, false, true),
                0x2507 => (3, true, true),
                0x2508 => (4, false, false),
                0x2509 => (4, true, false),
                0x250A => (4, false, true),
                0x250B => (4, true, true),
                0x254C => (2, false, false),
                0x254D => (2, true, false),
                0x254E => (2, false, true),
                _ => (2, true, true),
            };
            dashes(canvas, r, n, if heavy { light * 2 } else { light }, vertical, fg);
        }
        0x256D..=0x2570 => arc(canvas, r, light, cp, fg),
        0x2571..=0x2573 => {
            let (x0, y0, x1, y1) = (r.x as f32, r.y as f32, (r.x + r.w) as f32, (r.y + r.h) as f32);
            if cp != 0x2572 {
                line_aa(canvas, r, (x1, y0), (x0, y1), light as f32, fg);
            }
            if cp != 0x2571 {
                line_aa(canvas, r, (x0, y0), (x1, y1), light as f32, fg);
            }
        }
        _ => match arms(c) {
            Some(a) => lines(canvas, r, a, light, fg),
            None => return false,
        },
    }
    true
}

fn thickness(weight: u8, light: i32) -> i32 {
    match weight {
        0 => 0,
        1 => light,
        2 => light * 2,
        _ => light * 3,
    }
}

/// Start of a centred stroke of thickness `t` across a span of `len` pixels.
fn centred(len: i32, t: i32) -> i32 {
    (len - t) / 2
}

fn lines(cv: &mut Canvas, r: Rect, [up, right, down, left]: [u8; 4], light: i32, fg: Rgb) {
    // Thickness of the vertical and horizontal strokes through the centre;
    // arms extend to cover the crossing stroke so joints have no gaps.
    let tv = thickness(up.max(down), light);
    let th = thickness(left.max(right), light);
    let vx = centred(r.w, tv.max(light));
    let hy = centred(r.h, th.max(light));
    let v_end = vx + tv.max(light); // right edge of the vertical stroke
    let h_end = hy + th.max(light); // bottom edge of the horizontal stroke

    let hbar = |cv: &mut Canvas, w: u8, x0: i32, x1: i32| {
        let t = thickness(w, light);
        if w == 3 {
            let y = centred(r.h, t);
            cv.fill(Rect::new(r.x + x0, r.y + y, x1 - x0, light), fg);
            cv.fill(Rect::new(r.x + x0, r.y + y + 2 * light, x1 - x0, light), fg);
        } else if w > 0 {
            cv.fill(Rect::new(r.x + x0, r.y + centred(r.h, t), x1 - x0, t), fg);
        }
    };
    hbar(cv, left, 0, if up | down != 0 { v_end } else { r.w / 2 + 1 });
    hbar(cv, right, if up | down != 0 { vx } else { r.w / 2 }, r.w);

    let vbar = |cv: &mut Canvas, w: u8, y0: i32, y1: i32| {
        let t = thickness(w, light);
        if w == 3 {
            let x = centred(r.w, t);
            cv.fill(Rect::new(r.x + x, r.y + y0, light, y1 - y0), fg);
            cv.fill(Rect::new(r.x + x + 2 * light, r.y + y0, light, y1 - y0), fg);
        } else if w > 0 {
            cv.fill(Rect::new(r.x + centred(r.w, t), r.y + y0, t, y1 - y0), fg);
        }
    };
    vbar(cv, up, 0, if left | right != 0 { h_end } else { r.h / 2 + 1 });
    vbar(cv, down, if left | right != 0 { hy } else { r.h / 2 }, r.h);
}

fn dashes(cv: &mut Canvas, r: Rect, n: i32, t: i32, vertical: bool, fg: Rgb) {
    let len = if vertical { r.h } else { r.w } as f32;
    let period = len / n as f32;
    let gap = (period / 3.0).round().max(1.0);
    for i in 0..n {
        let a = (i as f32 * period + gap / 2.0).round() as i32;
        let b = ((i + 1) as f32 * period - gap / 2.0).round() as i32;
        if vertical {
            cv.fill(Rect::new(r.x + centred(r.w, t), r.y + a, t, b - a), fg);
        } else {
            cv.fill(Rect::new(r.x + a, r.y + centred(r.h, t), b - a, t), fg);
        }
    }
}

/// Rounded corners ╭╮╯╰: a quarter circle joining the centre lines, drawn
/// with distance-based antialiasing.
fn arc(cv: &mut Canvas, r: Rect, t: i32, cp: u32, fg: Rgb) {
    // Canonical shape is ╭ (arms right and down); mirror for the others.
    let (mx, my) = match cp {
        0x256D => (false, false),
        0x256E => (true, false),
        0x256F => (true, true),
        _ => (false, true),
    };
    let (w, h) = (r.w as f32, r.h as f32);
    let cx = centred(r.w, t) as f32 + t as f32 / 2.0;
    let cy = centred(r.h, t) as f32 + t as f32 / 2.0;
    let (cx, cy) = (if mx { w - cx } else { cx }, if my { h - cy } else { cy });
    let rad = (w - cx).min(h - cy).max(1.0);
    let (ox, oy) = (cx + rad, cy + rad);
    let half = t as f32 / 2.0;
    for py in 0..r.h {
        for px in 0..r.w {
            let mut fx = px as f32 + 0.5;
            let mut fy = py as f32 + 0.5;
            if mx {
                fx = w - fx;
            }
            if my {
                fy = h - fy;
            }
            let d = if fx <= cx + rad && fy <= cy + rad && fx < ox && fy < oy {
                // Quarter-circle region.
                ((fx - ox).hypot(fy - oy) - rad).abs()
            } else if fy >= oy {
                // Straight part going down.
                (fx - cx).abs()
            } else {
                // Straight part going right.
                (fy - cy).abs()
            };
            let a = (half + 0.5 - d).clamp(0.0, 1.0);
            if a > 0.0 {
                cv.blend(r.x + px, r.y + py, fg, (a * 255.0) as u8);
            }
        }
    }
}

fn line_aa(cv: &mut Canvas, r: Rect, (x0, y0): (f32, f32), (x1, y1): (f32, f32), t: f32, fg: Rgb) {
    let (dx, dy) = (x1 - x0, y1 - y0);
    let len = dx.hypot(dy).max(1e-3);
    for py in r.y..r.y + r.h {
        for px in r.x..r.x + r.w {
            let (fx, fy) = (px as f32 + 0.5, py as f32 + 0.5);
            let d = ((fx - x0) * dy - (fy - y0) * dx).abs() / len;
            let a = (t / 2.0 + 0.5 - d).clamp(0.0, 1.0);
            if a > 0.0 {
                cv.blend(px, py, fg, (a * 255.0) as u8);
            }
        }
    }
}

fn block(cv: &mut Canvas, cp: u32, r: Rect, fg: Rgb) {
    let eighth_h = |k: i32| (r.h * k + 4) / 8;
    let eighth_w = |k: i32| (r.w * k + 4) / 8;
    match cp {
        0x2580 => cv.fill(Rect::new(r.x, r.y, r.w, eighth_h(4)), fg),
        0x2581..=0x2588 => {
            let k = (cp - 0x2580) as i32;
            let hh = eighth_h(k);
            cv.fill(Rect::new(r.x, r.y + r.h - hh, r.w, hh), fg);
        }
        0x2589..=0x258F => {
            let k = 8 - (cp - 0x2588) as i32;
            cv.fill(Rect::new(r.x, r.y, eighth_w(k), r.h), fg);
        }
        0x2590 => {
            let x = eighth_w(4);
            cv.fill(Rect::new(r.x + x, r.y, r.w - x, r.h), fg);
        }
        0x2591..=0x2593 => {
            let a = [64u8, 128, 191][(cp - 0x2591) as usize];
            for y in r.y..r.y + r.h {
                for x in r.x..r.x + r.w {
                    cv.blend(x, y, fg, a);
                }
            }
        }
        0x2594 => cv.fill(Rect::new(r.x, r.y, r.w, eighth_h(1)), fg),
        0x2595 => {
            let x = eighth_w(7);
            cv.fill(Rect::new(r.x + x, r.y, r.w - x, r.h), fg);
        }
        _ => {
            // Quadrants: bit 1 upper-left, 2 upper-right, 4 lower-left, 8 lower-right.
            let q = match cp {
                0x2596 => 4,
                0x2597 => 8,
                0x2598 => 1,
                0x2599 => 1 | 4 | 8,
                0x259A => 1 | 8,
                0x259B => 1 | 2 | 4,
                0x259C => 1 | 2 | 8,
                0x259D => 2,
                0x259E => 2 | 4,
                _ => 2 | 4 | 8,
            };
            let (hw, hh) = (eighth_w(4), eighth_h(4));
            if q & 1 != 0 {
                cv.fill(Rect::new(r.x, r.y, hw, hh), fg);
            }
            if q & 2 != 0 {
                cv.fill(Rect::new(r.x + hw, r.y, r.w - hw, hh), fg);
            }
            if q & 4 != 0 {
                cv.fill(Rect::new(r.x, r.y + hh, hw, r.h - hh), fg);
            }
            if q & 8 != 0 {
                cv.fill(Rect::new(r.x + hw, r.y + hh, r.w - hw, r.h - hh), fg);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FG: Rgb = Rgb { r: 255, g: 255, b: 255 };

    fn cell(c: char, w: i32, h: i32) -> Canvas {
        let mut cv = Canvas::new(w as u32, h as u32);
        assert!(draw(&mut cv, c, Rect::new(0, 0, w, h), 1, FG));
        cv
    }

    fn lit(cv: &Canvas, x: i32, y: i32) -> bool {
        cv.get(x, y) != 0
    }

    #[test]
    fn arms_table_is_complete() {
        assert_eq!(ARMS.split_ascii_whitespace().count(), 128);
        assert_eq!(arms('─'), Some([0, 1, 0, 1]));
        assert_eq!(arms('┼'), Some([1, 1, 1, 1]));
        assert_eq!(arms('╋'), Some([2, 2, 2, 2]));
        assert_eq!(arms('╬'), Some([3, 3, 3, 3]));
        assert_eq!(arms('╿'), Some([2, 0, 1, 0]));
        assert_eq!(arms('╭'), None);
    }

    #[test]
    fn horizontal_line_spans_the_cell_edge_to_edge() {
        let cv = cell('─', 9, 18);
        // Leftmost and rightmost columns are lit on the same row, so lines in
        // neighbouring cells join without gaps.
        let row = (0..18).find(|&y| lit(&cv, 0, y)).expect("lit row");
        assert!(lit(&cv, 8, row));
        assert_eq!((0..18).filter(|&y| lit(&cv, 4, y)).count(), 1);
    }

    #[test]
    fn vertical_line_spans_top_to_bottom() {
        let cv = cell('│', 9, 18);
        let col = (0..9).find(|&x| lit(&cv, x, 0)).expect("lit column");
        assert!(lit(&cv, col, 17));
    }

    #[test]
    fn corner_joins_line_positions() {
        // ┌ must meet ─ (to its right) and │ (below) at the same pixels.
        let h = cell('─', 9, 18);
        let v = cell('│', 9, 18);
        let c = cell('┌', 9, 18);
        let hrow = (0..18).find(|&y| lit(&h, 8, y)).unwrap();
        let vcol = (0..9).find(|&x| lit(&v, x, 17)).unwrap();
        assert!(lit(&c, 8, hrow));
        assert!(lit(&c, vcol, 17));
        assert!(!lit(&c, 0, hrow));
        assert!(!lit(&c, vcol, 0));
    }

    #[test]
    fn rounded_corner_reaches_both_edges() {
        let h = cell('─', 9, 18);
        let v = cell('│', 9, 18);
        let hrow = (0..18).find(|&y| lit(&h, 8, y)).unwrap();
        let vcol = (0..9).find(|&x| lit(&v, x, 17)).unwrap();
        let c = cell('╭', 9, 18);
        assert!(lit(&c, 8, hrow), "right edge");
        assert!(lit(&c, vcol, 17), "bottom edge");
        assert!(!lit(&c, 0, 0) && !lit(&c, 8, 17));
        let c = cell('╯', 9, 18);
        assert!(lit(&c, 0, hrow), "left edge");
        assert!(lit(&c, vcol, 0), "top edge");
    }

    #[test]
    fn blocks_fill_exact_fractions() {
        let full = cell('█', 9, 18);
        assert!(full.pixels.iter().all(|&p| p == 0xffffff));
        let upper = cell('▀', 9, 18);
        assert!(lit(&upper, 0, 8) && !lit(&upper, 0, 9));
        let right = cell('▐', 10, 18);
        assert!(!lit(&right, 4, 0) && lit(&right, 5, 0));
        let q = cell('▛', 10, 18);
        assert!(lit(&q, 0, 0) && lit(&q, 9, 0) && lit(&q, 0, 17) && !lit(&q, 9, 17));
        let shade = cell('░', 4, 4);
        assert!(shade.pixels.iter().all(|&p| p == 0x404040));
    }
}
