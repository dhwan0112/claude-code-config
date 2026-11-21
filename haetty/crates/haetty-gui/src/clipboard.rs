//! System clipboard via copypasta: Win32 on Windows; on Linux the Wayland
//! clipboard through the window's own `wl_display` (smithay-clipboard, works
//! on GNOME, which has no data-control protocol), else X11 (x11rb, no libxcb).
//!
//! One instance lives as long as the window: on X11 the owner of a selection
//! has to keep serving it, so dropping the context would lose the contents.
//! Linux also has the primary selection (a mouse selection, pasted with a
//! middle click): X11's PRIMARY, Wayland's primary-selection protocol.

use copypasta::ClipboardProvider;
use winit::window::Window;

type Provider = Option<Box<dyn ClipboardProvider>>;

pub struct Clipboard {
    inner: Provider,
    primary: Provider,
}

impl Clipboard {
    pub fn new(window: &Window) -> Self {
        let (inner, primary) = Self::open(window);
        if inner.is_none() {
            log::warn!("clipboard unavailable");
        }
        if primary.is_none() {
            log::info!("no primary selection: a selection is copied with Ctrl+Shift+C only");
        }
        Clipboard { inner, primary }
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    fn open(window: &Window) -> (Provider, Provider) {
        use copypasta::x11_clipboard::{Clipboard as X11Clipboard, Primary, X11ClipboardContext};
        use winit::raw_window_handle::{HasDisplayHandle, RawDisplayHandle};
        if let Ok(handle) = window.display_handle() {
            if let RawDisplayHandle::Wayland(wl) = handle.as_raw() {
                // SAFETY: the display pointer comes from the live winit window,
                // and the clipboard is dropped together with the window state.
                let (primary, clip) =
                    unsafe { copypasta::wayland_clipboard::create_clipboards_from_external(wl.display.as_ptr()) };
                log::info!("clipboard: wayland");
                return (Some(Box::new(clip)), Some(Box::new(primary)));
            }
        }
        let clip: Provider = match X11ClipboardContext::<X11Clipboard>::new() {
            Ok(c) => {
                log::info!("clipboard: x11");
                Some(Box::new(c))
            }
            Err(e) => {
                log::warn!("x11 clipboard: {e}");
                None
            }
        };
        let primary: Provider = match X11ClipboardContext::<Primary>::new() {
            Ok(c) => Some(Box::new(c)),
            Err(e) => {
                log::warn!("x11 primary selection: {e}");
                None
            }
        };
        (clip, primary)
    }

    #[cfg(not(all(unix, not(target_os = "macos"))))]
    fn open(_window: &Window) -> (Provider, Provider) {
        match copypasta::ClipboardContext::new() {
            Ok(c) => (Some(Box::new(c)), None),
            Err(e) => {
                log::warn!("clipboard: {e}");
                (None, None)
            }
        }
    }

    pub fn set(&mut self, text: String) {
        write(&mut self.inner, text, "clipboard");
    }

    pub fn get(&mut self) -> Option<String> {
        read(&mut self.inner, "clipboard")
    }

    pub fn set_primary(&mut self, text: String) {
        write(&mut self.primary, text, "primary selection");
    }

    pub fn get_primary(&mut self) -> Option<String> {
        read(&mut self.primary, "primary selection")
    }
}

fn write(p: &mut Provider, text: String, what: &str) {
    if let Some(c) = p.as_mut() {
        if let Err(e) = c.set_contents(text) {
            log::warn!("{what} write: {e}");
        }
    }
}

fn read(p: &mut Provider, what: &str) -> Option<String> {
    match p.as_mut()?.get_contents() {
        Ok(s) => Some(s),
        Err(e) => {
            log::warn!("{what} read: {e}");
            None
        }
    }
}
