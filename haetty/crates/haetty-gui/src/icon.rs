//! The application icon (DESIGN.md §3.12): the user's design
//! `assets/icon/haetty.svg` and the files generated from it, all committed
//! (`assets/icon/README.md` says how they were made), so building needs no
//! conversion tool. Embedded here:
//! - Linux: the hicolor PNGs and the SVG for `haetty install-desktop`, and
//!   the 64 px image as raw RGBA for the X11 window icon (no PNG decoder at
//!   run time);
//! - Windows: nothing here — the window icons are loaded from the `.exe`'s
//!   icon resource (`build.rs` links `assets/icon/haetty.ico`), so Windows
//!   picks the right size from the `.ico` itself.

/// The master drawing (hicolor `scalable/apps`).
#[cfg(not(windows))]
pub const SVG: &str = include_str!("../../../assets/icon/haetty.svg");

/// hicolor sizes and their PNGs.
#[cfg(not(windows))]
pub const HICOLOR: &[(u32, &[u8])] = &[
    (16, include_bytes!("../../../assets/icon/png/haetty-16.png")),
    (24, include_bytes!("../../../assets/icon/png/haetty-24.png")),
    (32, include_bytes!("../../../assets/icon/png/haetty-32.png")),
    (48, include_bytes!("../../../assets/icon/png/haetty-48.png")),
    (64, include_bytes!("../../../assets/icon/png/haetty-64.png")),
    (128, include_bytes!("../../../assets/icon/png/haetty-128.png")),
    (256, include_bytes!("../../../assets/icon/png/haetty-256.png")),
    (512, include_bytes!("../../../assets/icon/png/haetty-512.png")),
];

/// The window icon on X11 (`_NET_WM_ICON`): 64×64 RGBA.
#[cfg(not(windows))]
pub fn window_icon() -> Option<winit::window::Icon> {
    const RGBA: &[u8] = include_bytes!("../../../assets/icon/rgba/haetty-64.rgba");
    winit::window::Icon::from_rgba(RGBA.to_vec(), 64, 64).ok()
}

/// Windows: the icon resource of the executable (ordinal 1, `build.rs`) at
/// the system's small and large icon sizes (`SM_CXSMICON`, `SM_CXICON`);
/// Windows takes the matching image from the `.ico`.
#[cfg(windows)]
pub fn window_icons() -> (Option<winit::window::Icon>, Option<winit::window::Icon>) {
    use winit::platform::windows::IconExtWindows;
    #[link(name = "user32")]
    extern "system" {
        fn GetSystemMetrics(index: i32) -> i32;
    }
    const SM_CXICON: i32 = 11;
    const SM_CXSMICON: i32 = 49;
    let load = |metric: i32| {
        // SAFETY: plain Win32 call without pointer arguments.
        let px = unsafe { GetSystemMetrics(metric) }.max(16) as u32;
        winit::window::Icon::from_resource(1, Some(winit::dpi::PhysicalSize::new(px, px)))
            .map_err(|e| log::warn!("window icon from the exe resource: {e}"))
            .ok()
    };
    (load(SM_CXSMICON), load(SM_CXICON))
}

#[cfg(all(test, not(windows)))]
mod tests {
    use super::*;

    fn png_size(p: &[u8]) -> (u32, u32) {
        assert_eq!(&p[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(&p[12..16], b"IHDR");
        (u32::from_be_bytes(p[16..20].try_into().unwrap()), u32::from_be_bytes(p[20..24].try_into().unwrap()))
    }

    #[test]
    fn committed_icon_files_are_what_they_claim() {
        for (s, png) in HICOLOR {
            assert_eq!(png_size(png), (*s, *s), "{s} px");
            assert_eq!(png[25], 6, "{s} px is RGBA");
        }
        // The user's drawing, unchanged (only the metadata block removed).
        assert!(SVG.starts_with("<svg ") && SVG.contains(r#"viewBox="0 0 256 256""#));
        for part in [
            r##"<rect x="12" y="36" width="232" height="184" fill="#0A0A0B"/>"##,
            r##"<polyline points="58,104 100,136 58,168" fill="none" stroke="#B4B2AF" stroke-width="16""##,
            r##"<rect x="116" y="158" width="58" height="15" rx="2" fill="#B4B2AF"/>"##,
        ] {
            assert!(SVG.contains(part), "{part}");
        }
        assert!(!SVG.contains("<metadata"));
        assert!(window_icon().is_some());
        // The Windows icon file: 8 images, PNG entries, sizes as asked.
        let ico: &[u8] = include_bytes!("../../../assets/icon/haetty.ico");
        assert_eq!(&ico[..6], &[0, 0, 1, 0, 8, 0]);
        let sizes: Vec<u32> = (0..8).map(|i| ico[6 + 16 * i] as u32).map(|b| if b == 0 { 256 } else { b }).collect();
        assert_eq!(sizes, [16, 20, 24, 32, 40, 48, 64, 256]);
        for (i, size) in sizes.iter().enumerate() {
            let off = u32::from_le_bytes(ico[6 + 16 * i + 12..6 + 16 * i + 16].try_into().unwrap()) as usize;
            assert_eq!(png_size(&ico[off..]), (*size, *size));
        }
    }
}
