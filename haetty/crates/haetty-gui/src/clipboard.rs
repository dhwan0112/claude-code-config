//! System clipboard via copypasta: Win32 on Windows; on Linux the Wayland
//! clipboard through the window's own `wl_display` (smithay-clipboard, works
//! on GNOME, which has no data-control protocol), else X11 (x11rb, no libxcb).
//!
//! One instance lives as long as the window: on X11 the owner of a selection
//! has to keep serving it, so dropping the context would lose the contents.

use copypasta::ClipboardProvider;
use winit::window::Window;

pub struct Clipboard {
    inner: Option<Box<dyn ClipboardProvider>>,
}

impl Clipboard {
    pub fn new(window: &Window) -> Self {
        let inner = Self::open(window);
        if inner.is_none() {
            log::warn!("clipboard unavailable");
        }
        Clipboard { inner }
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    fn open(window: &Window) -> Option<Box<dyn ClipboardProvider>> {
        use winit::raw_window_handle::{HasDisplayHandle, RawDisplayHandle};
        if let Ok(handle) = window.display_handle() {
            if let RawDisplayHandle::Wayland(wl) = handle.as_raw() {
                // SAFETY: the display pointer comes from the live winit window,
                // and the clipboard is dropped together with the window state.
                let (_primary, clip) =
                    unsafe { copypasta::wayland_clipboard::create_clipboards_from_external(wl.display.as_ptr()) };
                log::info!("clipboard: wayland");
                return Some(Box::new(clip));
            }
        }
        match copypasta::x11_clipboard::X11ClipboardContext::<copypasta::x11_clipboard::Clipboard>::new() {
            Ok(c) => {
                log::info!("clipboard: x11");
                Some(Box::new(c))
            }
            Err(e) => {
                log::warn!("x11 clipboard: {e}");
                None
            }
        }
    }

    #[cfg(not(all(unix, not(target_os = "macos"))))]
    fn open(_window: &Window) -> Option<Box<dyn ClipboardProvider>> {
        match copypasta::ClipboardContext::new() {
            Ok(c) => Some(Box::new(c)),
            Err(e) => {
                log::warn!("clipboard: {e}");
                None
            }
        }
    }

    pub fn set(&mut self, text: String) {
        if let Some(c) = self.inner.as_mut() {
            if let Err(e) = c.set_contents(text) {
                log::warn!("clipboard write: {e}");
            }
        }
    }

    pub fn get(&mut self) -> Option<String> {
        match self.inner.as_mut()?.get_contents() {
            Ok(s) => Some(s),
            Err(e) => {
                log::warn!("clipboard read: {e}");
                None
            }
        }
    }
}
