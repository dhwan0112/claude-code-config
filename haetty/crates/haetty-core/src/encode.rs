//! Input encoders for the native engine (DESIGN.md §10.7): keys, mouse,
//! paste. A port of libghostty-vt's input/key_encode.zig, function_keys.zig,
//! kitty.zig, mouse_encode.zig and paste.zig, so both engines send the same
//! bytes (`haetty-vtdiff --encoders` compares them).

use crate::types::{Key, KeyAction, KeyInput, Mods, MouseAction, MouseButton, MouseInput};

/// What the terminal state says about key encoding.
#[derive(Clone, Copy, Debug, Default)]
pub struct KeyOptions {
    /// DECCKM (1).
    pub cursor_app: bool,
    /// DECKPAM / mode 66.
    pub keypad_app: bool,
    /// DECBKM (67): backspace sends BS.
    pub backarrow: bool,
    /// Mode 1035.
    pub ignore_keypad_with_numlock: bool,
    /// Mode 1036: Alt prefixes ESC.
    pub alt_esc_prefix: bool,
    /// xterm modifyOtherKeys state 2 (`CSI > 4 ; 2 m`).
    pub modify_other_keys_2: bool,
    /// Kitty keyboard flags (bits as `KittyKeyFlags`).
    pub kitty: u8,
}

#[cfg(test)]
const DISAMBIGUATE: u8 = 0x01;
const REPORT_EVENTS: u8 = 0x02;
const REPORT_ALTERNATES: u8 = 0x04;
const REPORT_ALL: u8 = 0x08;
const REPORT_ASSOCIATED: u8 = 0x10;

/// The modifiers that matter for bindings (shift, ctrl, alt, super), as a
/// tuple that compares like libghostty's `binding().int()`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
struct Bm {
    shift: bool,
    ctrl: bool,
    alt: bool,
    sup: bool,
}

impl Bm {
    fn of(m: Mods) -> Bm {
        Bm {
            shift: m.contains(Mods::SHIFT),
            ctrl: m.contains(Mods::CTRL),
            alt: m.contains(Mods::ALT),
            sup: m.contains(Mods::SUPER),
        }
    }
    fn empty(self) -> bool {
        self == Bm::default()
    }
}

/// function_keys.modifiers: the xterm modifier codes 2–16, in order.
const MODIFIERS: [Bm; 15] = {
    [
        bm(true, false, false, false),
        bm(false, true, false, false),
        bm(true, true, false, false),
        bm(false, false, true, false),
        bm(true, false, true, false),
        bm(false, true, true, false),
        bm(true, true, true, false),
        bm(false, false, false, true),
        bm(true, false, false, true),
        bm(false, true, false, true),
        bm(true, true, false, true),
        bm(false, false, true, true),
        bm(true, false, true, true),
        bm(false, true, true, true),
        bm(true, true, true, true),
    ]
};

fn modifier_code(m: Bm) -> Option<usize> {
    MODIFIERS.iter().position(|x| *x == m).map(|i| i + 2)
}

/// A key's character on a US layout (`Key.codepoint()`).
fn key_codepoint(k: Key) -> Option<char> {
    use Key::*;
    Some(match k {
        A => 'a',
        B => 'b',
        C => 'c',
        D => 'd',
        E => 'e',
        F => 'f',
        G => 'g',
        H => 'h',
        I => 'i',
        J => 'j',
        K => 'k',
        L => 'l',
        M => 'm',
        N => 'n',
        O => 'o',
        P => 'p',
        Q => 'q',
        R => 'r',
        S => 's',
        T => 't',
        U => 'u',
        V => 'v',
        W => 'w',
        X => 'x',
        Y => 'y',
        Z => 'z',
        Digit0 | Numpad0 => '0',
        Digit1 | Numpad1 => '1',
        Digit2 | Numpad2 => '2',
        Digit3 | Numpad3 => '3',
        Digit4 | Numpad4 => '4',
        Digit5 | Numpad5 => '5',
        Digit6 | Numpad6 => '6',
        Digit7 | Numpad7 => '7',
        Digit8 | Numpad8 => '8',
        Digit9 | Numpad9 => '9',
        Semicolon => ';',
        Space => ' ',
        Quote => '\'',
        Comma => ',',
        Backquote => '`',
        Period | NumpadDecimal => '.',
        Slash | NumpadDivide => '/',
        Minus | NumpadSubtract => '-',
        Equal | NumpadEqual => '=',
        BracketLeft => '[',
        BracketRight => ']',
        Backslash => '\\',
        Tab => '\t',
        NumpadMultiply => '*',
        NumpadAdd => '+',
        _ => return None,
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Any,
    Normal,
    App,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mok {
    Any,
    /// Only without modifyOtherKeys 2.
    Set,
    /// Only with it.
    SetOther,
}

struct Entry {
    mods: Bm,
    mods_empty_is_any: bool,
    cursor: Mode,
    keypad: Mode,
    mok: Mok,
    seq: String,
    seq_decbkm: Option<&'static str>,
}

fn entry(seq: impl Into<String>) -> Entry {
    Entry {
        mods: Bm::default(),
        mods_empty_is_any: true,
        cursor: Mode::Any,
        keypad: Mode::Any,
        mok: Mok::Any,
        seq: seq.into(),
        seq_decbkm: None,
    }
}

fn pc_style(fmt: &str) -> Vec<Entry> {
    MODIFIERS
        .iter()
        .enumerate()
        .map(|(i, m)| Entry { mods: *m, ..entry(fmt.replace("{}", &(i + 2).to_string())) })
        .collect()
}

fn cursor_key(normal: &str, app: &str) -> Vec<Entry> {
    vec![Entry { cursor: Mode::Normal, ..entry(normal) }, Entry { cursor: Mode::App, ..entry(app) }]
}

fn kp_keys(suffix: &str) -> Vec<Entry> {
    let mut v = vec![Entry { mods_empty_is_any: false, keypad: Mode::App, ..entry(format!("\x1bO{suffix}")) }];
    v.extend(pc_style(&format!("\x1bO{{}}{suffix}")).into_iter().map(|e| Entry { keypad: Mode::App, ..e }));
    v
}

/// One modified-key table (backspace/tab/enter/escape): `(mods, mok, seq)`.
fn table(rows: &[(Bm, Mok, &str)]) -> Vec<Entry> {
    rows.iter().map(|(m, k, s)| Entry { mods: *m, mok: *k, ..entry(*s) }).collect()
}

const fn bm(shift: bool, alt: bool, ctrl: bool, sup: bool) -> Bm {
    Bm { shift, ctrl, alt, sup }
}

/// `function_keys.keys`: the entries for a key, in order.
fn function_key_entries(k: Key) -> Vec<Entry> {
    use Key::*;
    let cat = |mut a: Vec<Entry>, b: Vec<Entry>| {
        a.extend(b);
        a
    };
    match k {
        ArrowUp | NumpadUp => cat(pc_style("\x1b[1;{}A"), cursor_key("\x1b[A", "\x1bOA")),
        ArrowDown | NumpadDown => cat(pc_style("\x1b[1;{}B"), cursor_key("\x1b[B", "\x1bOB")),
        ArrowRight | NumpadRight => cat(pc_style("\x1b[1;{}C"), cursor_key("\x1b[C", "\x1bOC")),
        ArrowLeft | NumpadLeft => cat(pc_style("\x1b[1;{}D"), cursor_key("\x1b[D", "\x1bOD")),
        NumpadBegin => cat(pc_style("\x1b[1;{}E"), cursor_key("\x1b[E", "\x1bOE")),
        Home | NumpadHome => cat(pc_style("\x1b[1;{}H"), cursor_key("\x1b[H", "\x1bOH")),
        End | NumpadEnd => cat(pc_style("\x1b[1;{}F"), cursor_key("\x1b[F", "\x1bOF")),
        Insert | NumpadInsert => cat(pc_style("\x1b[2;{}~"), vec![entry("\x1b[2~")]),
        Delete | NumpadDelete => cat(pc_style("\x1b[3;{}~"), vec![entry("\x1b[3~")]),
        PageUp | NumpadPageUp => cat(pc_style("\x1b[5;{}~"), vec![entry("\x1b[5~")]),
        PageDown | NumpadPageDown => cat(pc_style("\x1b[6;{}~"), vec![entry("\x1b[6~")]),
        F1 => cat(pc_style("\x1b[1;{}P"), vec![entry("\x1bOP")]),
        F2 => cat(pc_style("\x1b[1;{}Q"), vec![entry("\x1bOQ")]),
        F3 => cat(pc_style("\x1b[13;{}~"), vec![entry("\x1bOR")]),
        F4 => cat(pc_style("\x1b[1;{}S"), vec![entry("\x1bOS")]),
        F5 => cat(pc_style("\x1b[15;{}~"), vec![entry("\x1b[15~")]),
        F6 => cat(pc_style("\x1b[17;{}~"), vec![entry("\x1b[17~")]),
        F7 => cat(pc_style("\x1b[18;{}~"), vec![entry("\x1b[18~")]),
        F8 => cat(pc_style("\x1b[19;{}~"), vec![entry("\x1b[19~")]),
        F9 => cat(pc_style("\x1b[20;{}~"), vec![entry("\x1b[20~")]),
        F10 => cat(pc_style("\x1b[21;{}~"), vec![entry("\x1b[21~")]),
        F11 => cat(pc_style("\x1b[23;{}~"), vec![entry("\x1b[23~")]),
        F12 => cat(pc_style("\x1b[24;{}~"), vec![entry("\x1b[24~")]),
        Numpad0 => kp_keys("p"),
        Numpad1 => kp_keys("q"),
        Numpad2 => kp_keys("r"),
        Numpad3 => kp_keys("s"),
        Numpad4 => kp_keys("t"),
        Numpad5 => kp_keys("u"),
        Numpad6 => kp_keys("v"),
        Numpad7 => kp_keys("w"),
        Numpad8 => kp_keys("x"),
        Numpad9 => kp_keys("y"),
        NumpadDecimal => kp_keys("n"),
        NumpadDivide => kp_keys("o"),
        NumpadMultiply => kp_keys("j"),
        NumpadSubtract => kp_keys("m"),
        NumpadAdd => kp_keys("k"),
        NumpadEnter => cat(kp_keys("M"), vec![entry("\r")]),
        Backspace => {
            let (s, o) = (Mok::Set, Mok::SetOther);
            let mut v = table(&[
                (bm(true, false, false, false), s, "\x7f"),
                (bm(false, true, false, false), s, "\x1b\x7f"),
                (bm(true, true, false, false), s, "\x1b\x7f"),
                (bm(true, false, true, false), s, "\x08"),
                (bm(false, true, true, false), s, "\x1b\x08"),
                (bm(false, false, false, true), s, "\x7f"),
                (bm(true, false, false, true), s, "\x7f"),
                (bm(false, true, false, true), s, "\x1b\x7f"),
                (bm(true, true, false, true), s, "\x1b\x7f"),
                (bm(false, false, true, true), s, "\x08"),
                (bm(true, false, true, true), s, "\x08"),
                (bm(false, true, true, true), s, "\x1b\x08"),
                (bm(true, true, true, true), s, "\x1b\x08"),
                (bm(true, false, false, false), o, "\x1b[27;2;127~"),
                (bm(false, true, false, false), o, "\x1b[27;3;127~"),
                (bm(true, true, false, false), o, "\x1b[27;4;127~"),
                (bm(true, false, true, false), o, "\x1b[27;6;127~"),
                (bm(false, true, true, false), o, "\x1b[27;7;127~"),
                (bm(true, true, true, false), o, "\x1b[27;8;127~"),
                (bm(false, false, false, true), o, "\x1b[27;9;127~"),
                (bm(true, false, false, true), o, "\x1b[27;10;127~"),
                (bm(false, true, false, true), o, "\x1b[27;11;127~"),
                (bm(true, true, false, true), o, "\x1b[27;12;127~"),
                (bm(false, false, true, true), o, "\x1b[27;13;127~"),
                (bm(true, false, true, true), o, "\x1b[27;14;127~"),
                (bm(false, true, true, true), o, "\x1b[27;15;127~"),
                (bm(true, true, true, true), o, "\x1b[27;16;127~"),
            ]);
            v.push(Entry { mods: bm(false, false, true, false), seq_decbkm: Some("\x7f"), ..entry("\x08") });
            v.push(Entry { seq_decbkm: Some("\x08"), ..entry("\x7f") });
            v
        }
        Tab => {
            let (s, o, a) = (Mok::Set, Mok::SetOther, Mok::Any);
            let mut v = table(&[
                (bm(true, false, false, false), s, "\x1b[Z"),
                (bm(false, true, false, false), s, "\x1b\t"),
                (bm(true, false, false, false), o, "\x1b[27;2;9~"),
                (bm(false, true, false, false), o, "\x1b[27;3;9~"),
            ]);
            v.extend(modified_rows(9, &[4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16], a));
            v.push(entry("\t"));
            v
        }
        Enter => {
            let (s, o, a) = (Mok::Set, Mok::SetOther, Mok::Any);
            let mut v = table(&[(bm(true, false, false, false), a, "\x1b[27;2;13~")]);
            v.extend(table(&[
                (bm(false, true, false, false), s, "\x1b\r"),
                (bm(false, true, false, false), o, "\x1b[27;3;13~"),
            ]));
            v.extend(modified_rows(13, &[4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16], a));
            v.push(entry("\r"));
            v
        }
        Escape => {
            let a = Mok::Any;
            let mut v = table(&[
                (bm(true, false, false, false), a, "\x1b[27;2;27~"),
                (bm(false, true, false, false), a, "\x1b\x1b"),
            ]);
            v.extend(modified_rows(27, &[4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16], a));
            v.push(entry("\x1b"));
            v
        }
        _ => vec![],
    }
}

/// `ESC [27;N;CODE~` rows for the given xterm modifier codes.
fn modified_rows(code: u32, ns: &[usize], mok: Mok) -> Vec<Entry> {
    ns.iter().map(|&n| Entry { mods: MODIFIERS[n - 2], mok, ..entry(format!("\x1b[27;{n};{code}~")) }).collect()
}

fn pc_style_function_key(k: Key, mods: Mods, o: &KeyOptions) -> Option<String> {
    let m = Bm::of(mods);
    let keypad_app = !o.ignore_keypad_with_numlock && o.keypad_app;
    for e in function_key_entries(k) {
        match e.cursor {
            Mode::Any => {}
            Mode::Normal if o.cursor_app => continue,
            Mode::App if !o.cursor_app => continue,
            _ => {}
        }
        match e.keypad {
            Mode::Any => {}
            Mode::Normal if keypad_app => continue,
            Mode::App if !keypad_app => continue,
            _ => {}
        }
        match e.mok {
            Mok::Any => {}
            Mok::Set if o.modify_other_keys_2 => continue,
            Mok::SetOther if !o.modify_other_keys_2 => continue,
            _ => {}
        }
        if e.mods.empty() {
            if !m.empty() && !e.mods_empty_is_any {
                continue;
            }
        } else if e.mods != m {
            continue;
        }
        if o.backarrow {
            if let Some(s) = e.seq_decbkm {
                return Some(s.into());
            }
        }
        return Some(e.seq);
    }
    None
}

fn is_control(c: u32) -> bool {
    c < 0x20 || c == 0x7f
}

fn is_control_utf8(s: &str) -> bool {
    s.len() == 1 && is_control(s.as_bytes()[0] as u32)
}

fn ctrl_seq(k: Key, utf8: &str, unshifted: u32, mods: Mods) -> Option<u8> {
    if !mods.contains(Mods::CTRL) {
        return None;
    }
    let ctrl_only = Bm { ctrl: true, ..Bm::default() };
    let mut unset = Bm::of(mods);
    unset.alt = false;
    let mut ch: u8 = if utf8.len() == 1 {
        utf8.as_bytes()[0]
    } else {
        let cp = key_codepoint(k)? as u32;
        let b = u8::try_from(cp).ok()?;
        if unset != ctrl_only {
            return None;
        }
        b
    };
    if unset.shift && !ch.is_ascii_uppercase() && ch != b'@' {
        unset.shift = false;
    }
    if ch.is_ascii_uppercase() && unshifted > 0 {
        if let Ok(b) = u8::try_from(unshifted) {
            ch = b;
        }
    }
    if unset != ctrl_only {
        return None;
    }
    Some(match ch {
        b' ' => 0,
        b'/' => 31,
        b'0' => 48,
        b'1' => 49,
        b'2' => 0,
        b'3' => 27,
        b'4' => 28,
        b'5' => 29,
        b'6' => 30,
        b'7' => 31,
        b'8' => 127,
        b'9' => 57,
        b'?' => 127,
        b'@' => 0,
        b'\\' => 28,
        b']' => 29,
        b'^' => 30,
        b'_' => 31,
        b'a'..=b'h' | b'j'..=b'l' | b'n'..=b'z' => ch - b'a' + 1,
        b'~' => 30,
        _ => return None,
    })
}

fn legacy_alt_prefix(ev: &KeyInput, binding: Bm, o: &KeyOptions) -> Option<u8> {
    if !binding.alt || !o.alt_esc_prefix {
        return None;
    }
    let utf8 = ev.text.as_deref().unwrap_or("");
    if utf8.len() == 1 {
        return Some(utf8.as_bytes()[0]);
    }
    let u = ev.unshifted.map_or(0, |c| c as u32);
    if u > 0 {
        if let Ok(b) = u8::try_from(u) {
            return Some(b);
        }
    }
    None
}

fn effective_mods(ev: &KeyInput) -> Mods {
    if ev.text.as_deref().unwrap_or("").is_empty() {
        ev.mods
    } else {
        ev.mods.difference(ev.consumed_mods)
    }
}

fn legacy(ev: &KeyInput, o: &KeyOptions, out: &mut Vec<u8>) {
    let utf8 = ev.text.as_deref().unwrap_or("");
    let binding = Bm::of(effective_mods(ev));
    if ev.action != KeyAction::Press && ev.action != KeyAction::Repeat {
        return;
    }
    if ev.composing {
        return;
    }
    if let Some(seq) = pc_style_function_key(ev.key, ev.mods, o) {
        let mut use_seq = true;
        if !utf8.is_empty() && matches!(ev.key, Key::Backspace | Key::Enter | Key::Escape) && !is_control_utf8(utf8) {
            if ev.key == Key::Backspace {
                return;
            }
            use_seq = false;
        }
        if use_seq {
            out.extend_from_slice(seq.as_bytes());
            return;
        }
    }
    let unshifted = ev.unshifted.map_or(0, |c| c as u32);
    if let Some(c) = ctrl_seq(ev.key, utf8, unshifted, ev.mods) {
        if binding.alt {
            out.push(0x1b);
        }
        out.push(c);
        return;
    }
    if utf8.is_empty() {
        if let Some(b) = legacy_alt_prefix(ev, binding, o) {
            out.extend_from_slice(&[0x1b, b]);
        }
        return;
    }
    if o.modify_other_keys_2 {
        let mut cs = utf8.chars();
        if let (Some(cp), None) = (cs.next(), cs.next()) {
            let m = Bm::of(ev.mods);
            let cpn = cp as u32;
            let no_shift = Bm { shift: false, ..m };
            let should = (0x40..=0x7f).contains(&cpn) || !no_shift.empty() || cp == ' ';
            if should {
                if let Some(code) = modifier_code(m) {
                    out.extend_from_slice(format!("\x1b[27;{code};{cpn}~").as_bytes());
                    return;
                }
            }
        }
    }
    if ev.mods.contains(Mods::CTRL) {
        let mut cs = utf8.chars();
        if let (Some(mut ch), None) = (cs.next(), cs.next()) {
            let (mut shift, alt, ctrl) =
                (ev.mods.contains(Mods::SHIFT), ev.mods.contains(Mods::ALT), ev.mods.contains(Mods::CTRL));
            if ch.is_ascii_uppercase() && shift {
                ch = ch.to_ascii_lowercase();
            }
            if ev.unshifted != Some(ch) {
                shift = false;
            }
            let n = 1 + shift as u32 + 2 * alt as u32 + 4 * ctrl as u32;
            out.extend_from_slice(format!("\x1b[{};{n}u", ch as u32).as_bytes());
            return;
        }
    }
    if let Some(b) = legacy_alt_prefix(ev, binding, o) {
        out.extend_from_slice(&[0x1b, b]);
        return;
    }
    out.extend_from_slice(utf8.as_bytes());
}

/// kitty.zig: (key, code, final, modifier).
fn kitty_entry(k: Key) -> Option<(u32, u8, bool)> {
    use Key::*;
    Some(match k {
        Escape => (27, b'u', false),
        Enter => (13, b'u', false),
        Tab => (9, b'u', false),
        Backspace => (127, b'u', false),
        Insert => (2, b'~', false),
        Delete => (3, b'~', false),
        ArrowLeft => (1, b'D', false),
        ArrowRight => (1, b'C', false),
        ArrowUp => (1, b'A', false),
        ArrowDown => (1, b'B', false),
        PageUp => (5, b'~', false),
        PageDown => (6, b'~', false),
        Home => (1, b'H', false),
        End => (1, b'F', false),
        CapsLock => (57358, b'u', true),
        ScrollLock => (57359, b'u', false),
        NumLock => (57360, b'u', true),
        PrintScreen => (57361, b'u', false),
        Pause => (57362, b'u', false),
        F1 => (1, b'P', false),
        F2 => (1, b'Q', false),
        F3 => (13, b'~', false),
        F4 => (1, b'S', false),
        F5 => (15, b'~', false),
        F6 => (17, b'~', false),
        F7 => (18, b'~', false),
        F8 => (19, b'~', false),
        F9 => (20, b'~', false),
        F10 => (21, b'~', false),
        F11 => (23, b'~', false),
        F12 => (24, b'~', false),
        F13 => (57376, b'u', false),
        F14 => (57377, b'u', false),
        F15 => (57378, b'u', false),
        F16 => (57379, b'u', false),
        F17 => (57380, b'u', false),
        F18 => (57381, b'u', false),
        F19 => (57382, b'u', false),
        F20 => (57383, b'u', false),
        F21 => (57384, b'u', false),
        F22 => (57385, b'u', false),
        F23 => (57386, b'u', false),
        F24 => (57387, b'u', false),
        F25 => (57388, b'u', false),
        Numpad0 => (57399, b'u', false),
        Numpad1 => (57400, b'u', false),
        Numpad2 => (57401, b'u', false),
        Numpad3 => (57402, b'u', false),
        Numpad4 => (57403, b'u', false),
        Numpad5 => (57404, b'u', false),
        Numpad6 => (57405, b'u', false),
        Numpad7 => (57406, b'u', false),
        Numpad8 => (57407, b'u', false),
        Numpad9 => (57408, b'u', false),
        NumpadDecimal => (57409, b'u', false),
        NumpadDivide => (57410, b'u', false),
        NumpadMultiply => (57411, b'u', false),
        NumpadSubtract => (57412, b'u', false),
        NumpadAdd => (57413, b'u', false),
        NumpadEnter => (57414, b'u', false),
        NumpadEqual => (57415, b'u', false),
        NumpadSeparator => (57416, b'u', false),
        NumpadLeft => (57417, b'u', false),
        NumpadRight => (57418, b'u', false),
        NumpadUp => (57419, b'u', false),
        NumpadDown => (57420, b'u', false),
        NumpadPageUp => (57421, b'u', false),
        NumpadPageDown => (57422, b'u', false),
        NumpadHome => (57423, b'u', false),
        NumpadEnd => (57424, b'u', false),
        NumpadInsert => (57425, b'u', false),
        NumpadDelete => (57426, b'u', false),
        NumpadBegin => (57427, b'u', false),
        ShiftLeft => (57441, b'u', true),
        ShiftRight => (57447, b'u', true),
        ControlLeft => (57442, b'u', true),
        ControlRight => (57448, b'u', true),
        MetaLeft => (57444, b'u', true),
        MetaRight => (57450, b'u', true),
        AltLeft => (57443, b'u', true),
        AltRight => (57449, b'u', true),
        _ => return None,
    })
}

/// KittyMods (shift, alt, ctrl, super, hyper, meta, caps, num) + 1.
fn kitty_mods(m: Mods) -> u32 {
    let b = |f: Mods, bit: u32| if m.contains(f) { bit } else { 0 };
    1 + b(Mods::SHIFT, 1)
        + b(Mods::ALT, 2)
        + b(Mods::CTRL, 4)
        + b(Mods::SUPER, 8)
        + b(Mods::CAPS_LOCK, 64)
        + b(Mods::NUM_LOCK, 128)
}

fn kitty(ev: &KeyInput, o: &KeyOptions, out: &mut Vec<u8>) {
    let f = o.kitty;
    let utf8 = ev.text.as_deref().unwrap_or("");
    if ev.action == KeyAction::Release {
        if f & REPORT_EVENTS == 0 {
            return;
        }
        if f & REPORT_ALL == 0 && matches!(ev.key, Key::Enter | Key::Backspace | Key::Tab) {
            return;
        }
    }
    let binding = Bm::of(effective_mods(ev));
    let unshifted = ev.unshifted.map_or(0, |c| c as u32);
    let entry = kitty_entry(ev.key).or(if unshifted > 0 { Some((unshifted, b'u', false)) } else { None });
    // Preprocessing.
    if ev.composing {
        if !entry.is_some_and(|e| e.2) {
            return;
        }
    } else {
        if !utf8.is_empty() && matches!(ev.key, Key::Enter | Key::Backspace) && !is_control_utf8(utf8) {
            if ev.key == Key::Backspace {
                return;
            }
            out.extend_from_slice(utf8.as_bytes());
            return;
        }
        if f & REPORT_ALL == 0 {
            if binding.empty() {
                match ev.key {
                    Key::Enter => return out.push(b'\r'),
                    Key::Tab => return out.push(b'\t'),
                    Key::Backspace => return out.push(0x7f),
                    _ => {}
                }
            }
            if !utf8.is_empty()
                && binding.empty()
                && ev.action != KeyAction::Release
                && !utf8.chars().any(|c| is_control(c as u32))
            {
                out.extend_from_slice(utf8.as_bytes());
                return;
            }
        }
    }
    let Some((code, fin, modifier)) = entry else {
        if !utf8.is_empty() {
            out.extend_from_slice(utf8.as_bytes());
        }
        return;
    };
    if modifier && f & REPORT_ALL == 0 {
        return;
    }
    let mods = kitty_mods(ev.mods);
    let event = if f & REPORT_EVENTS != 0 {
        match ev.action {
            KeyAction::Press => 1,
            KeyAction::Repeat => 2,
            KeyAction::Release => 3,
        }
    } else {
        0
    };
    let mut alternates: [Option<u32>; 2] = [None, None];
    if f & REPORT_ALTERNATES != 0 && !is_control(code) {
        let mut cs = utf8.chars();
        let base = key_codepoint(ev.key).map(|c| c as u32);
        if let Some(cp1) = cs.next() {
            let cp1 = cp1 as u32;
            if cp1 != code && ev.mods.contains(Mods::SHIFT) {
                alternates[0] = Some(cp1);
            }
            let has_cp2 = cs.next().is_some();
            if let Some(b) = base {
                if b != code && cp1 != b && !has_cp2 {
                    alternates[1] = Some(b);
                }
            }
        } else if let Some(b) = base {
            if b != code {
                alternates[1] = Some(b);
            }
        }
    }
    let prevents_text = ev.mods.intersects(Mods::ALT | Mods::CTRL | Mods::SUPER);
    let text = if f & REPORT_ASSOCIATED != 0 && event != 3 && !prevents_text { utf8 } else { "" };
    if fin == b'u' || fin == b'~' {
        let mut s = format!("\x1b[{code}");
        if let Some(a) = alternates[0] {
            s += &format!(":{a}");
        }
        if let Some(b) = alternates[1] {
            s += &if alternates[0].is_none() { format!("::{b}") } else { format!(":{b}") };
        }
        let mut prior = false;
        if event != 0 && event != 1 {
            s += &format!(";{mods}:{event}");
            prior = true;
        } else if mods > 1 {
            s += &format!(";{mods}");
            prior = true;
        }
        let mut count = 0;
        for c in text.chars() {
            if is_control(c as u32) {
                continue;
            }
            if count == 0 {
                if !prior {
                    s.push(';');
                }
                s.push(';');
            } else {
                s.push(':');
            }
            s += &(c as u32).to_string();
            count += 1;
        }
        s.push(fin as char);
        out.extend_from_slice(s.as_bytes());
    } else if event != 0 {
        out.extend_from_slice(format!("\x1b[1;{mods}:{event}{}", fin as char).as_bytes());
    } else if mods > 1 {
        out.extend_from_slice(format!("\x1b[1;{mods}{}", fin as char).as_bytes());
    } else {
        out.extend_from_slice(format!("\x1b[{}", fin as char).as_bytes());
    }
}

/// A key as libghostty's encoder sends it; plus, as `GhosttyEngine` does,
/// the text of a key the encoder has no sequence for.
pub fn key(ev: &KeyInput, o: &KeyOptions, out: &mut Vec<u8>) {
    let start = out.len();
    if o.kitty & 0x1f != 0 {
        kitty(ev, o, out);
    } else {
        legacy(ev, o, out);
    }
    if out.len() == start && !ev.composing && ev.action != KeyAction::Release {
        if let Some(t) = &ev.text {
            out.extend_from_slice(t.as_bytes());
        }
    }
}

/// Mouse reporting state (`flags.mouse_event`, `flags.mouse_format`).
#[derive(Clone, Copy, Debug, Default)]
pub struct MouseOptions {
    /// 0 none, 1 X10, 2 normal, 3 button, 4 any.
    pub event: u8,
    /// 0 X10, 1 UTF-8, 2 SGR, 3 urxvt, 4 SGR pixels.
    pub format: u8,
    pub screen_w: u32,
    pub screen_h: u32,
    pub cell_w: u32,
    pub cell_h: u32,
    pub cols: u16,
    pub rows: u16,
}

/// A mouse report; `last` keeps the last reported cell (motion within one
/// cell is not reported again).
pub fn mouse(m: &MouseInput, o: &MouseOptions, last: &mut Option<(u32, u32)>, out: &mut Vec<u8>) {
    let should = match o.event {
        0 => false,
        1 => {
            m.action == MouseAction::Press
                && matches!(m.button, Some(MouseButton::Left | MouseButton::Middle | MouseButton::Right))
        }
        2 => m.action != MouseAction::Motion,
        3 => m.button.is_some(),
        _ => true,
    };
    if !should {
        return;
    }
    let out_of_view = m.x < 0.0 || m.y < 0.0 || m.x > o.screen_w as f32 || m.y > o.screen_h as f32;
    if m.action != MouseAction::Release && out_of_view {
        if o.event < 3 {
            return;
        }
        if !m.any_pressed {
            return;
        }
    }
    let to_cell = |v: f32, cell: u32, n: u16| -> u32 {
        let c = (v.max(0.0) as f64 / cell.max(1) as f64).floor() as u32;
        c.min(n.saturating_sub(1) as u32)
    };
    let cell = (to_cell(m.x, o.cell_w, o.cols), to_cell(m.y, o.cell_h, o.rows));
    if m.action == MouseAction::Motion && o.format != 4 && *last == Some(cell) {
        return;
    }
    *last = Some(cell);
    let code = {
        let base: u8 = match m.button {
            None => 3,
            Some(_) if m.action == MouseAction::Release && o.format != 2 && o.format != 4 => 3,
            Some(MouseButton::Left) => 0,
            Some(MouseButton::Middle) => 1,
            Some(MouseButton::Right) => 2,
            Some(MouseButton::Four) => 64,
            Some(MouseButton::Five) => 65,
            Some(MouseButton::Six) => 66,
            Some(MouseButton::Seven) => 67,
            Some(MouseButton::Eight) => 128,
            Some(MouseButton::Nine) => 129,
            Some(_) => return,
        };
        let mut acc = base;
        if o.event != 1 {
            if m.mods.contains(Mods::SHIFT) {
                acc += 4;
            }
            if m.mods.contains(Mods::ALT) {
                acc += 8;
            }
            if m.mods.contains(Mods::CTRL) {
                acc += 16;
            }
        }
        if m.action == MouseAction::Motion {
            acc += 32;
        }
        acc
    };
    let fin = if m.action == MouseAction::Release { 'm' } else { 'M' };
    match o.format {
        0 => {
            if cell.0 > 222 || cell.1 > 222 {
                return;
            }
            out.extend_from_slice(b"\x1b[M");
            out.extend_from_slice(&[32 + code, 32 + cell.0 as u8 + 1, 32 + cell.1 as u8 + 1]);
        }
        1 => {
            out.extend_from_slice(b"\x1b[M");
            out.push(32u8.wrapping_add(code));
            for v in [cell.0 + 33, cell.1 + 33] {
                if let Some(c) = char::from_u32(v) {
                    let mut b = [0u8; 4];
                    out.extend_from_slice(c.encode_utf8(&mut b).as_bytes());
                }
            }
        }
        2 => out.extend_from_slice(format!("\x1b[<{code};{};{}{fin}", cell.0 + 1, cell.1 + 1).as_bytes()),
        3 => out.extend_from_slice(format!("\x1b[{};{};{}M", 32 + code as u32, cell.0 + 1, cell.1 + 1).as_bytes()),
        _ => {
            let (x, y) = (m.x.round() as i32, m.y.round() as i32);
            out.extend_from_slice(format!("\x1b[<{code};{x};{y}{fin}").as_bytes());
        }
    }
}

/// A paste (paste.zig): control characters that could act are replaced
/// by spaces; bracketed, or with LF as CR.
pub fn paste(text: &str, bracketed: bool, out: &mut Vec<u8>) {
    const STRIP: &[u8] =
        &[0x00, 0x08, 0x05, 0x04, 0x1b, 0x7f, 0x03, 0x1c, 0x15, 0x1a, 0x11, 0x13, 0x17, 0x16, 0x12, 0x0f];
    let mut data: Vec<u8> = text.bytes().map(|b| if STRIP.contains(&b) { b' ' } else { b }).collect();
    if bracketed {
        out.extend_from_slice(b"\x1b[200~");
        out.append(&mut data);
        out.extend_from_slice(b"\x1b[201~");
    } else {
        for b in &mut data {
            if *b == b'\n' {
                *b = b'\r';
            }
        }
        out.append(&mut data);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(k: Key, text: Option<&str>, mods: Mods) -> KeyInput {
        KeyInput { text: text.map(Into::into), mods, ..KeyInput::press(k) }
    }

    #[test]
    fn common_keys() {
        let o = KeyOptions { alt_esc_prefix: true, ..KeyOptions::default() };
        let enc = |ev: KeyInput, o: &KeyOptions| {
            let mut v = vec![];
            key(&ev, o, &mut v);
            v
        };
        assert_eq!(enc(press(Key::A, Some("a"), Mods::empty()), &o), b"a");
        assert_eq!(enc(press(Key::C, Some("c"), Mods::CTRL), &o), b"\x03");
        assert_eq!(enc(press(Key::ArrowUp, None, Mods::empty()), &o), b"\x1b[A");
        assert_eq!(enc(press(Key::ArrowUp, None, Mods::SHIFT), &o), b"\x1b[1;2A");
        assert_eq!(enc(press(Key::Enter, Some("\r"), Mods::empty()), &o), b"\r");
        let k = KeyOptions { kitty: DISAMBIGUATE, ..o };
        assert_eq!(enc(press(Key::Escape, None, Mods::empty()), &k), b"\x1b[27u");
    }
}
