//! A 0RGB pixel buffer (softbuffer's format) with clipped fills and blending.

use ccterm_core::Rgb;

pub fn pack(c: Rgb) -> u32 {
    (c.r as u32) << 16 | (c.g as u32) << 8 | c.b as u32
}

pub fn unpack(p: u32) -> Rgb {
    Rgb { r: (p >> 16) as u8, g: (p >> 8) as u8, b: p as u8 }
}

/// `a` over `b` with coverage `alpha` (0..=255).
pub fn mix(a: Rgb, b: Rgb, alpha: u8) -> Rgb {
    let m = |x: u8, y: u8| ((x as u32 * alpha as u32 + y as u32 * (255 - alpha as u32) + 127) / 255) as u8;
    Rgb { r: m(a.r, b.r), g: m(a.g, b.g), b: m(a.b, b.b) }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }
    fn intersect(self, o: Rect) -> Rect {
        let x0 = self.x.max(o.x);
        let y0 = self.y.max(o.y);
        let x1 = (self.x + self.w).min(o.x + o.w);
        let y1 = (self.y + self.h).min(o.y + o.h);
        Rect { x: x0, y: y0, w: (x1 - x0).max(0), h: (y1 - y0).max(0) }
    }
}

pub struct Canvas {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u32>,
    clip: Rect,
}

impl Canvas {
    pub fn new(width: u32, height: u32) -> Self {
        let mut c = Canvas { width: 0, height: 0, pixels: vec![], clip: Rect::new(0, 0, 0, 0) };
        c.resize(width, height);
        c
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        self.pixels = vec![0; width as usize * height as usize];
        self.reset_clip();
    }

    pub fn bounds(&self) -> Rect {
        Rect::new(0, 0, self.width as i32, self.height as i32)
    }

    pub fn reset_clip(&mut self) {
        self.clip = self.bounds();
    }

    /// Restrict all drawing to `r` (intersected with the canvas).
    pub fn set_clip(&mut self, r: Rect) {
        self.clip = r.intersect(self.bounds());
    }

    #[cfg(test)]
    pub fn get(&self, x: i32, y: i32) -> u32 {
        self.pixels[y as usize * self.width as usize + x as usize]
    }

    pub fn fill(&mut self, r: Rect, color: Rgb) {
        let r = r.intersect(self.clip);
        if r.w <= 0 || r.h <= 0 {
            return;
        }
        let p = pack(color);
        let stride = self.width as usize;
        for y in r.y..r.y + r.h {
            let row = y as usize * stride;
            self.pixels[row + r.x as usize..row + (r.x + r.w) as usize].fill(p);
        }
    }

    /// Blend `color` onto pixel (x, y) with coverage `alpha`.
    #[inline]
    pub fn blend(&mut self, x: i32, y: i32, color: Rgb, alpha: u8) {
        let c = self.clip;
        if alpha == 0 || x < c.x || y < c.y || x >= c.x + c.w || y >= c.y + c.h {
            return;
        }
        let i = y as usize * self.width as usize + x as usize;
        self.pixels[i] = if alpha == 255 { pack(color) } else { pack(mix(color, unpack(self.pixels[i]), alpha)) };
    }

    /// Outline of `r`, `t` pixels thick.
    pub fn frame(&mut self, r: Rect, t: i32, color: Rgb) {
        self.fill(Rect::new(r.x, r.y, r.w, t), color);
        self.fill(Rect::new(r.x, r.y + r.h - t, r.w, t), color);
        self.fill(Rect::new(r.x, r.y, t, r.h), color);
        self.fill(Rect::new(r.x + r.w - t, r.y, t, r.h), color);
    }

    /// Binary PPM (P6), for frame dumps.
    pub fn to_ppm(&self) -> Vec<u8> {
        let mut out = format!("P6\n{} {}\n255\n", self.width, self.height).into_bytes();
        out.reserve(self.pixels.len() * 3);
        for &p in &self.pixels {
            out.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8]);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_respects_clip_and_bounds() {
        let mut c = Canvas::new(10, 10);
        c.set_clip(Rect::new(0, 2, 10, 3));
        c.fill(Rect::new(-5, -5, 30, 30), Rgb { r: 255, g: 0, b: 0 });
        assert_eq!(c.get(0, 1), 0);
        assert_eq!(c.get(9, 2), 0xff0000);
        assert_eq!(c.get(4, 4), 0xff0000);
        assert_eq!(c.get(4, 5), 0);
    }

    #[test]
    fn blend_mixes() {
        let white = Rgb { r: 255, g: 255, b: 255 };
        let black = Rgb::default();
        assert_eq!(mix(white, black, 255), white);
        assert_eq!(mix(white, black, 0), black);
        assert_eq!(mix(white, black, 128).r, 128);
    }
}
