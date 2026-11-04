//! winit input → ccterm-core input.

use ccterm_core::{Key, KeyAction, KeyInput, Mods, MouseButton};
use winit::event::{ElementState, MouseButton as WButton, MouseScrollDelta};
use winit::keyboard::{KeyCode, ModifiersState};

pub fn mods(m: ModifiersState) -> Mods {
    let mut out = Mods::empty();
    if m.shift_key() {
        out |= Mods::SHIFT;
    }
    if m.control_key() {
        out |= Mods::CTRL;
    }
    if m.alt_key() {
        out |= Mods::ALT;
    }
    if m.super_key() {
        out |= Mods::SUPER;
    }
    out
}

/// The parts of a winit `KeyEvent` the mapping needs (so it can be tested
/// without constructing one, which winit does not allow).
#[derive(Clone, Debug)]
pub struct RawKey<'a> {
    pub code: Option<KeyCode>,
    pub state: ElementState,
    pub repeat: bool,
    /// `KeyEvent::text`: what the key produces with the current modifiers.
    pub text: Option<&'a str>,
    /// First character of `key_without_modifiers()`, if it is a character.
    pub unshifted: Option<char>,
    pub mods: ModifiersState,
}

/// Build the engine's key event. Text is kept only when it is printable and
/// was not produced together with Ctrl/Alt/Super (the encoder derives those
/// sequences from the key and the unshifted character), except for AltGr on
/// Windows, which arrives as Ctrl+Alt with printable text.
pub fn key_input(k: &RawKey) -> KeyInput {
    let key = k.code.map_or(Key::Unidentified, map_code);
    let action = match (k.state, k.repeat) {
        (ElementState::Released, _) => KeyAction::Release,
        (ElementState::Pressed, true) => KeyAction::Repeat,
        (ElementState::Pressed, false) => KeyAction::Press,
    };
    let mut m = mods(k.mods);
    let printable = k.text.filter(|t| !t.is_empty() && !t.chars().any(char::is_control));
    let altgr = cfg!(windows)
        && m.contains(Mods::CTRL | Mods::ALT)
        && printable.is_some_and(|t| t.chars().next() != k.unshifted);
    if altgr {
        m.remove(Mods::CTRL | Mods::ALT);
    }
    let text =
        if altgr || !m.intersects(Mods::CTRL | Mods::ALT | Mods::SUPER) { printable.map(str::to_owned) } else { None };
    let mut consumed = Mods::empty();
    if m.contains(Mods::SHIFT) && text.as_deref().is_some_and(|t| t.chars().next() != k.unshifted) {
        consumed |= Mods::SHIFT;
    }
    KeyInput {
        key,
        action,
        mods: m,
        consumed_mods: consumed,
        text,
        unshifted: k.unshifted.map(|c| c.to_lowercase().next().unwrap_or(c)),
        composing: false,
    }
}

pub fn map_code(code: KeyCode) -> Key {
    use KeyCode as C;
    match code {
        C::Backquote => Key::Backquote,
        C::Backslash => Key::Backslash,
        C::BracketLeft => Key::BracketLeft,
        C::BracketRight => Key::BracketRight,
        C::Comma => Key::Comma,
        C::Digit0 => Key::Digit0,
        C::Digit1 => Key::Digit1,
        C::Digit2 => Key::Digit2,
        C::Digit3 => Key::Digit3,
        C::Digit4 => Key::Digit4,
        C::Digit5 => Key::Digit5,
        C::Digit6 => Key::Digit6,
        C::Digit7 => Key::Digit7,
        C::Digit8 => Key::Digit8,
        C::Digit9 => Key::Digit9,
        C::Equal => Key::Equal,
        C::IntlBackslash => Key::IntlBackslash,
        C::IntlRo => Key::IntlRo,
        C::IntlYen => Key::IntlYen,
        C::KeyA => Key::A,
        C::KeyB => Key::B,
        C::KeyC => Key::C,
        C::KeyD => Key::D,
        C::KeyE => Key::E,
        C::KeyF => Key::F,
        C::KeyG => Key::G,
        C::KeyH => Key::H,
        C::KeyI => Key::I,
        C::KeyJ => Key::J,
        C::KeyK => Key::K,
        C::KeyL => Key::L,
        C::KeyM => Key::M,
        C::KeyN => Key::N,
        C::KeyO => Key::O,
        C::KeyP => Key::P,
        C::KeyQ => Key::Q,
        C::KeyR => Key::R,
        C::KeyS => Key::S,
        C::KeyT => Key::T,
        C::KeyU => Key::U,
        C::KeyV => Key::V,
        C::KeyW => Key::W,
        C::KeyX => Key::X,
        C::KeyY => Key::Y,
        C::KeyZ => Key::Z,
        C::Minus => Key::Minus,
        C::Period => Key::Period,
        C::Quote => Key::Quote,
        C::Semicolon => Key::Semicolon,
        C::Slash => Key::Slash,
        C::AltLeft => Key::AltLeft,
        C::AltRight => Key::AltRight,
        C::Backspace => Key::Backspace,
        C::CapsLock => Key::CapsLock,
        C::ContextMenu => Key::ContextMenu,
        C::ControlLeft => Key::ControlLeft,
        C::ControlRight => Key::ControlRight,
        C::Enter => Key::Enter,
        C::SuperLeft => Key::MetaLeft,
        C::SuperRight => Key::MetaRight,
        C::ShiftLeft => Key::ShiftLeft,
        C::ShiftRight => Key::ShiftRight,
        C::Space => Key::Space,
        C::Tab => Key::Tab,
        C::Convert => Key::Convert,
        C::KanaMode => Key::KanaMode,
        C::NonConvert => Key::NonConvert,
        C::Delete => Key::Delete,
        C::End => Key::End,
        C::Help => Key::Help,
        C::Home => Key::Home,
        C::Insert => Key::Insert,
        C::PageDown => Key::PageDown,
        C::PageUp => Key::PageUp,
        C::ArrowDown => Key::ArrowDown,
        C::ArrowLeft => Key::ArrowLeft,
        C::ArrowRight => Key::ArrowRight,
        C::ArrowUp => Key::ArrowUp,
        C::NumLock => Key::NumLock,
        C::Numpad0 => Key::Numpad0,
        C::Numpad1 => Key::Numpad1,
        C::Numpad2 => Key::Numpad2,
        C::Numpad3 => Key::Numpad3,
        C::Numpad4 => Key::Numpad4,
        C::Numpad5 => Key::Numpad5,
        C::Numpad6 => Key::Numpad6,
        C::Numpad7 => Key::Numpad7,
        C::Numpad8 => Key::Numpad8,
        C::Numpad9 => Key::Numpad9,
        C::NumpadAdd => Key::NumpadAdd,
        C::NumpadBackspace => Key::NumpadBackspace,
        C::NumpadClear => Key::NumpadClear,
        C::NumpadClearEntry => Key::NumpadClearEntry,
        C::NumpadComma => Key::NumpadComma,
        C::NumpadDecimal => Key::NumpadDecimal,
        C::NumpadDivide => Key::NumpadDivide,
        C::NumpadEnter => Key::NumpadEnter,
        C::NumpadEqual => Key::NumpadEqual,
        C::NumpadMemoryAdd => Key::NumpadMemoryAdd,
        C::NumpadMemoryClear => Key::NumpadMemoryClear,
        C::NumpadMemoryRecall => Key::NumpadMemoryRecall,
        C::NumpadMemoryStore => Key::NumpadMemoryStore,
        C::NumpadMemorySubtract => Key::NumpadMemorySubtract,
        C::NumpadMultiply => Key::NumpadMultiply,
        C::NumpadParenLeft => Key::NumpadParenLeft,
        C::NumpadParenRight => Key::NumpadParenRight,
        C::NumpadSubtract => Key::NumpadSubtract,
        C::Escape => Key::Escape,
        C::F1 => Key::F1,
        C::F2 => Key::F2,
        C::F3 => Key::F3,
        C::F4 => Key::F4,
        C::F5 => Key::F5,
        C::F6 => Key::F6,
        C::F7 => Key::F7,
        C::F8 => Key::F8,
        C::F9 => Key::F9,
        C::F10 => Key::F10,
        C::F11 => Key::F11,
        C::F12 => Key::F12,
        C::F13 => Key::F13,
        C::F14 => Key::F14,
        C::F15 => Key::F15,
        C::F16 => Key::F16,
        C::F17 => Key::F17,
        C::F18 => Key::F18,
        C::F19 => Key::F19,
        C::F20 => Key::F20,
        C::F21 => Key::F21,
        C::F22 => Key::F22,
        C::F23 => Key::F23,
        C::F24 => Key::F24,
        C::F25 => Key::F25,
        C::Fn => Key::Fn,
        C::FnLock => Key::FnLock,
        C::PrintScreen => Key::PrintScreen,
        C::ScrollLock => Key::ScrollLock,
        C::Pause => Key::Pause,
        C::BrowserBack => Key::BrowserBack,
        C::BrowserFavorites => Key::BrowserFavorites,
        C::BrowserForward => Key::BrowserForward,
        C::BrowserHome => Key::BrowserHome,
        C::BrowserRefresh => Key::BrowserRefresh,
        C::BrowserSearch => Key::BrowserSearch,
        C::BrowserStop => Key::BrowserStop,
        C::Eject => Key::Eject,
        C::LaunchApp1 => Key::LaunchApp1,
        C::LaunchApp2 => Key::LaunchApp2,
        C::LaunchMail => Key::LaunchMail,
        C::MediaPlayPause => Key::MediaPlayPause,
        C::MediaSelect => Key::MediaSelect,
        C::MediaStop => Key::MediaStop,
        C::MediaTrackNext => Key::MediaTrackNext,
        C::MediaTrackPrevious => Key::MediaTrackPrevious,
        C::Power => Key::Power,
        C::Sleep => Key::Sleep,
        C::AudioVolumeDown => Key::AudioVolumeDown,
        C::AudioVolumeMute => Key::AudioVolumeMute,
        C::AudioVolumeUp => Key::AudioVolumeUp,
        C::WakeUp => Key::WakeUp,
        C::Copy => Key::Copy,
        C::Cut => Key::Cut,
        C::Paste => Key::Paste,
        _ => Key::Unidentified,
    }
}

pub fn mouse_button(b: WButton) -> Option<MouseButton> {
    match b {
        WButton::Left => Some(MouseButton::Left),
        WButton::Right => Some(MouseButton::Right),
        WButton::Middle => Some(MouseButton::Middle),
        WButton::Back => Some(MouseButton::Eight),
        WButton::Forward => Some(MouseButton::Nine),
        WButton::Other(_) => None,
    }
}

/// Turns wheel deltas into whole notches. Touchpads send pixel deltas;
/// those are accumulated by cell height.
#[derive(Default, Debug)]
pub struct Wheel {
    acc: f64,
}

impl Wheel {
    /// Notches for this event, negative = up (the sign `wheel()` expects).
    pub fn notches(&mut self, delta: MouseScrollDelta, cell_height: u32) -> i32 {
        let up = match delta {
            MouseScrollDelta::LineDelta(_, y) => y as f64,
            MouseScrollDelta::PixelDelta(p) => p.y / cell_height.max(1) as f64,
        };
        // winit: positive y = content moves down = wheel up.
        self.acc += up;
        let whole = self.acc.trunc();
        self.acc -= whole;
        -(whole as i32)
    }
}

/// Lines to scroll for `notches`: one wheel event per notch when the
/// application tracks the mouse (it decides the step), three lines of
/// scrollback otherwise.
pub fn wheel_lines(notches: i32, mouse_tracking: bool) -> i32 {
    if mouse_tracking {
        notches
    } else {
        notches * 3
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::dpi::PhysicalPosition;

    fn raw<'a>(code: KeyCode, text: Option<&'a str>, unshifted: Option<char>, mods: ModifiersState) -> RawKey<'a> {
        RawKey { code: Some(code), state: ElementState::Pressed, repeat: false, text, unshifted, mods }
    }

    #[test]
    fn plain_and_shifted_characters_keep_text() {
        let k = key_input(&raw(KeyCode::KeyA, Some("a"), Some('a'), ModifiersState::empty()));
        assert_eq!((k.key, k.text.as_deref(), k.unshifted), (Key::A, Some("a"), Some('a')));
        assert_eq!(k.action, KeyAction::Press);
        let k = key_input(&raw(KeyCode::KeyA, Some("A"), Some('a'), ModifiersState::SHIFT));
        assert_eq!(k.text.as_deref(), Some("A"));
        assert_eq!(k.mods, Mods::SHIFT);
        assert_eq!(k.consumed_mods, Mods::SHIFT);
    }

    #[test]
    fn control_characters_and_ctrl_text_are_dropped() {
        // winit reports Ctrl+C with text "\x03" on X11 and Windows.
        let k = key_input(&raw(KeyCode::KeyC, Some("\u{3}"), Some('c'), ModifiersState::CONTROL));
        assert_eq!((k.key, k.text, k.unshifted, k.mods), (Key::C, None, Some('c'), Mods::CTRL));
        let k = key_input(&raw(KeyCode::Enter, Some("\r"), None, ModifiersState::SHIFT));
        assert_eq!((k.key, k.text, k.mods, k.consumed_mods), (Key::Enter, None, Mods::SHIFT, Mods::empty()));
        let k = key_input(&raw(KeyCode::KeyX, Some("x"), Some('x'), ModifiersState::ALT));
        assert_eq!((k.text, k.mods), (None, Mods::ALT));
    }

    #[test]
    fn altgr_on_windows_keeps_text() {
        let k = key_input(&raw(KeyCode::KeyQ, Some("@"), Some('q'), ModifiersState::CONTROL | ModifiersState::ALT));
        if cfg!(windows) {
            assert_eq!((k.text.as_deref(), k.mods), (Some("@"), Mods::empty()));
        } else {
            assert_eq!(k.text, None);
        }
    }

    #[test]
    fn repeat_and_release() {
        let mut r = raw(KeyCode::KeyA, Some("a"), Some('a'), ModifiersState::empty());
        r.repeat = true;
        assert_eq!(key_input(&r).action, KeyAction::Repeat);
        r.state = ElementState::Released;
        assert_eq!(key_input(&r).action, KeyAction::Release);
    }

    #[test]
    fn unknown_physical_key_with_text_still_carries_text() {
        let mut r = raw(KeyCode::Lang1, Some("한"), None, ModifiersState::empty());
        assert_eq!(key_input(&r).key, Key::Unidentified);
        assert_eq!(key_input(&r).text.as_deref(), Some("한"));
        r.code = None;
        assert_eq!(key_input(&r).key, Key::Unidentified);
    }

    #[test]
    fn wheel_lines_and_pixels() {
        let mut w = Wheel::default();
        assert_eq!(w.notches(MouseScrollDelta::LineDelta(0.0, 1.0), 18), -1);
        assert_eq!(w.notches(MouseScrollDelta::LineDelta(0.0, -2.0), 18), 2);
        // Half notches add up.
        assert_eq!(w.notches(MouseScrollDelta::LineDelta(0.0, 0.5), 18), 0);
        assert_eq!(w.notches(MouseScrollDelta::LineDelta(0.0, 0.5), 18), -1);
        // Touchpad pixels: 18 px per line.
        assert_eq!(w.notches(MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, 9.0)), 18), 0);
        assert_eq!(w.notches(MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, 27.0)), 18), -2);
        assert_eq!(wheel_lines(-1, true), -1);
        assert_eq!(wheel_lines(-1, false), -3);
    }
}
