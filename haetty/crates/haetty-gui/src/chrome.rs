//! Native window chrome matched to the terminal's colours.
//!
//! - Windows 11 (build 22000+): title bar background and window border take
//!   the terminal background, the title text the foreground (DWM caption,
//!   border and text colour attributes via winit's Windows extensions).
//! - Windows 10: those DWM attributes do not exist there (the calls are
//!   ignored); the dark/light theme request (immersive dark mode) applies.
//! - Linux: the theme request — X11 sets `_GTK_THEME_VARIANT` for the window
//!   manager, Wayland picks the client-side decoration theme.
//!
//! The theme follows the background's luminance: dark for the default
//! background, light if an application switches to a light one (OSC 11).

use haetty_core::Rgb;
use winit::window::{Theme, Window, WindowAttributes};

pub fn theme_for(bg: Rgb) -> Theme {
    // Relative luminance (sRGB coefficients, no linearisation needed for a
    // dark/light decision).
    let l = 0.2126 * bg.r as f32 + 0.7152 * bg.g as f32 + 0.0722 * bg.b as f32;
    if l < 128.0 {
        Theme::Dark
    } else {
        Theme::Light
    }
}

/// Window attributes so the first frame already has the right chrome.
pub fn attributes(attrs: WindowAttributes, bg: Rgb, fg: Rgb) -> WindowAttributes {
    let attrs = attrs.with_theme(Some(theme_for(bg)));
    #[cfg(windows)]
    let attrs = {
        use winit::platform::windows::WindowAttributesExtWindows;
        attrs
            .with_title_background_color(Some(win_color(bg)))
            .with_border_color(Some(win_color(bg)))
            .with_title_text_color(win_color(fg))
    };
    #[cfg(not(windows))]
    let _ = fg;
    attrs
}

#[cfg(windows)]
fn win_color(c: Rgb) -> winit::platform::windows::Color {
    winit::platform::windows::Color::from_rgb(c.r, c.g, c.b)
}

/// Applies colour changes (e.g. an application setting the background with
/// OSC 11) to the window chrome.
#[derive(Default)]
pub struct Chrome {
    applied: Option<(Rgb, Rgb)>,
}

impl Chrome {
    pub fn new(bg: Rgb, fg: Rgb) -> Self {
        Chrome { applied: Some((bg, fg)) }
    }

    pub fn update(&mut self, window: &Window, bg: Rgb, fg: Rgb) {
        if self.applied == Some((bg, fg)) {
            return;
        }
        self.applied = Some((bg, fg));
        log::debug!("chrome: bg {bg:?} fg {fg:?} theme {:?}", theme_for(bg));
        window.set_theme(Some(theme_for(bg)));
        #[cfg(windows)]
        {
            use winit::platform::windows::WindowExtWindows;
            window.set_title_background_color(Some(win_color(bg)));
            window.set_border_color(Some(win_color(bg)));
            window.set_title_text_color(win_color(fg));
        }
        #[cfg(not(windows))]
        let _ = fg;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_follows_background_luminance() {
        assert_eq!(theme_for(Rgb { r: 0x1e, g: 0x1e, b: 0x1e }), Theme::Dark);
        assert_eq!(theme_for(Rgb { r: 0xfa, g: 0xfa, b: 0xfa }), Theme::Light);
        // Saturated blue is dark, saturated yellow is light.
        assert_eq!(theme_for(Rgb { r: 0, g: 0, b: 255 }), Theme::Dark);
        assert_eq!(theme_for(Rgb { r: 255, g: 255, b: 0 }), Theme::Light);
    }
}
