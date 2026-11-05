//! The application icon — a **placeholder** until a real one exists. This
//! file is the one place to change: `rgba` feeds the window icon (X11,
//! Windows), `ccterm install-desktop` (hicolor PNGs and SVG) and `build.rs`
//! (the Windows .exe resource, via `include!`). Neutral greys: a dark
//! rounded square with a light prompt `>_`.
//!
//! Also a minimal PNG writer (stored deflate blocks) and ICO container, so
//! no image crate is needed.

/// RGBA pixels of the icon at `size`×`size` (4× supersampled).
pub fn rgba(size: u32) -> Vec<u8> {
    let s = size as f32;
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    let (bg, fg, edge) = ([30u8, 30, 30], [220u8, 220, 220], [70u8, 70, 70]);
    for py in 0..size {
        for px in 0..size {
            let (mut cov_bg, mut cov_fg, mut cov_edge) = (0.0f32, 0.0f32, 0.0f32);
            for sy in 0..4 {
                for sx in 0..4 {
                    let x = (px as f32 + (sx as f32 + 0.5) / 4.0) / s;
                    let y = (py as f32 + (sy as f32 + 0.5) / 4.0) / s;
                    let d = rounded_square(x, y, 0.06, 0.2);
                    if d > 0.0 {
                        continue;
                    }
                    if d > -0.025 {
                        cov_edge += 1.0;
                    } else if prompt(x, y) {
                        cov_fg += 1.0;
                    } else {
                        cov_bg += 1.0;
                    }
                }
            }
            let a = (cov_bg + cov_fg + cov_edge) / 16.0;
            let tot = (cov_bg + cov_fg + cov_edge).max(1e-6);
            for c in 0..3 {
                let v = (bg[c] as f32 * cov_bg + fg[c] as f32 * cov_fg + edge[c] as f32 * cov_edge) / tot;
                out.push(v.round() as u8);
            }
            out.push((a * 255.0).round() as u8);
        }
    }
    out
}

/// Signed distance-ish to a rounded square inset by `m` with corner `r`
/// (negative inside).
fn rounded_square(x: f32, y: f32, m: f32, r: f32) -> f32 {
    let (cx, cy) = ((x - 0.5).abs(), (y - 0.5).abs());
    let h = 0.5 - m - r;
    let (dx, dy) = ((cx - h).max(0.0), (cy - h).max(0.0));
    if cx <= h || cy <= h {
        return (cx.max(cy)) - (0.5 - m);
    }
    (dx * dx + dy * dy).sqrt() - r
}

fn seg_dist(x: f32, y: f32, a: (f32, f32), b: (f32, f32)) -> f32 {
    let (vx, vy) = (b.0 - a.0, b.1 - a.1);
    let t = (((x - a.0) * vx + (y - a.1) * vy) / (vx * vx + vy * vy)).clamp(0.0, 1.0);
    let (qx, qy) = (a.0 + t * vx - x, a.1 + t * vy - y);
    (qx * qx + qy * qy).sqrt()
}

/// The `>_` prompt.
fn prompt(x: f32, y: f32) -> bool {
    let w = 0.045;
    seg_dist(x, y, (0.26, 0.32), (0.46, 0.5)) < w
        || seg_dist(x, y, (0.46, 0.5), (0.26, 0.68)) < w
        || ((0.52..=0.76).contains(&x) && (0.64..=0.72).contains(&y))
}

/// The same drawing as SVG (hicolor `scalable/apps`).
#[cfg_attr(windows, allow(dead_code))] // Linux install-desktop only
pub fn svg() -> String {
    r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">
<rect x="6" y="6" width="88" height="88" rx="20" fill="#1e1e1e" stroke="#464646" stroke-width="2.5"/>
<path d="M26 32 L46 50 L26 68" fill="none" stroke="#dcdcdc" stroke-width="9" stroke-linecap="round" stroke-linejoin="round"/>
<rect x="52" y="64" width="24" height="8" fill="#dcdcdc"/>
</svg>
"##
    .to_string()
}

#[cfg_attr(windows, allow(dead_code))]
fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
    }
    !c
}

#[cfg_attr(windows, allow(dead_code))]
fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in data {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

/// An RGBA PNG (zlib with stored blocks: larger than compressed, but no
/// dependency; icons are written once).
#[cfg_attr(windows, allow(dead_code))]
pub fn png(size: u32, rgba: &[u8]) -> Vec<u8> {
    let mut raw = Vec::with_capacity(rgba.len() + size as usize);
    for row in rgba.chunks(size as usize * 4) {
        raw.push(0); // filter: none
        raw.extend_from_slice(row);
    }
    let mut z = vec![0x78, 0x01];
    let chunks: Vec<&[u8]> = raw.chunks(65_535).collect();
    for (i, c) in chunks.iter().enumerate() {
        z.push(u8::from(i + 1 == chunks.len()));
        z.extend_from_slice(&(c.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(c.len() as u16)).to_le_bytes());
        z.extend_from_slice(c);
    }
    z.extend_from_slice(&adler32(&raw).to_be_bytes());
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut chunk = |kind: &[u8], body: &[u8]| {
        out.extend_from_slice(&(body.len() as u32).to_be_bytes());
        let mut c = kind.to_vec();
        c.extend_from_slice(body);
        out.extend_from_slice(&c);
        out.extend_from_slice(&crc32(&c).to_be_bytes());
    };
    let mut ihdr = vec![];
    ihdr.extend_from_slice(&size.to_be_bytes());
    ihdr.extend_from_slice(&size.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit RGBA
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &z);
    chunk(b"IEND", &[]);
    out
}

/// An ICO file with PNG entries (Windows Vista and later). Used by build.rs.
#[allow(dead_code)]
pub fn ico(sizes: &[u32]) -> Vec<u8> {
    let pngs: Vec<Vec<u8>> = sizes.iter().map(|&s| png(s, &rgba(s))).collect();
    let mut out = vec![0, 0, 1, 0];
    out.extend_from_slice(&(sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len() as u32;
    for (s, p) in sizes.iter().zip(&pngs) {
        let d = if *s >= 256 { 0 } else { *s as u8 };
        out.extend_from_slice(&[d, d, 0, 0, 1, 0, 32, 0]);
        out.extend_from_slice(&(p.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += p.len() as u32;
    }
    for p in pngs {
        out.extend_from_slice(&p);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_png_and_ico_are_well_formed() {
        let px = rgba(32);
        assert_eq!(px.len(), 32 * 32 * 4);
        assert_eq!(px[3], 0, "corner is transparent");
        let mid = (16 * 32 + 16) * 4;
        assert_eq!(px[mid + 3], 255, "centre is opaque");
        // Only greys.
        assert!(px.chunks(4).all(|p| p[0] == p[1] && p[1] == p[2]));
        let p = png(32, &px);
        assert_eq!(&p[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(&p[12..16], b"IHDR");
        assert_eq!(crc32(b"IEND"), 0xAE42_6082);
        let i = ico(&[16, 32, 256]);
        assert_eq!(&i[..6], &[0, 0, 1, 0, 3, 0]);
        assert_eq!(i[6 + 32], 0, "256 is stored as 0");
        assert!(svg().contains("<svg"));
    }
}
