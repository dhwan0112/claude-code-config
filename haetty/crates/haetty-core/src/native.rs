//! [`VtEngine`] on haetty's own engine (`haetty-vt`, feature `native`;
//! DESIGN.md §10). Not used by sessions yet: the default engine stays
//! libghostty-vt until the differential tests show no difference.

use crate::types::*;
use crate::vt::{VtEngine, VtInit, VtMode};
use crate::{ColorSchemePref, DumpFormat, SessionEvent, TermColors};

pub struct NativeEngine {
    term: haetty_vt::Terminal,
}

fn arr(c: Rgb) -> [u8; 3] {
    [c.r, c.g, c.b]
}

fn rgb(c: [u8; 3]) -> Rgb {
    Rgb { r: c[0], g: c[1], b: c[2] }
}

fn colors(c: &TermColors) -> haetty_vt::Colors {
    haetty_vt::Colors {
        fg: arr(c.default_fg),
        bg: arr(c.default_bg),
        palette: c.palette.map(|p| p.map(arr)),
        cursor: c.cursor_color.map(arr),
    }
}

fn dark(s: ColorSchemePref) -> Option<bool> {
    match s {
        ColorSchemePref::Dark => Some(true),
        ColorSchemePref::Light => Some(false),
        ColorSchemePref::Unknown => None,
    }
}

impl NativeEngine {
    pub fn new(init: &VtInit) -> NativeEngine {
        let gs = init.size.clamped();
        NativeEngine {
            term: haetty_vt::Terminal::new(haetty_vt::Options {
                cols: gs.cols,
                rows: gs.rows,
                cell_width: gs.cell_width,
                cell_height: gs.cell_height,
                colors: colors(&init.colors),
                dark: dark(init.color_scheme),
                xtversion: init.xtversion.clone(),
                scrollback_limit: init.scrollback_limit,
            }),
        }
    }
}

impl VtEngine for NativeEngine {
    fn feed(&mut self, bytes: &[u8]) {
        self.term.feed(bytes);
    }

    fn take_replies(&mut self) -> Vec<u8> {
        self.term.take_replies()
    }

    fn take_events(&mut self) -> Vec<SessionEvent> {
        self.term
            .take_events()
            .into_iter()
            .map(|e| match e {
                haetty_vt::Event::Bell => SessionEvent::Bell,
                haetty_vt::Event::Title(t) => SessionEvent::Title(t),
                haetty_vt::Event::Pwd(p) => SessionEvent::Pwd(p),
                haetty_vt::Event::ClipboardWrite(t) => SessionEvent::ClipboardWrite(t),
            })
            .collect()
    }

    fn resize(&mut self, size: GridSize) {
        let gs = size.clamped();
        self.term.resize(gs.cols, gs.rows, gs.cell_width, gs.cell_height);
    }

    fn snapshot(&mut self) -> Snapshot {
        let t = &self.term;
        let (cols, rows) = t.size();
        let mut cells = Vec::with_capacity(cols as usize * rows as usize);
        for y in 0..rows {
            for x in 0..cols {
                let c = t.cell(x, y);
                cells.push(Cell {
                    text: t.text(&c),
                    fg: t.resolve(c.style.fg).map(rgb),
                    bg: t.resolve(c.style.bg).map(rgb),
                    flags: CellFlags(c.style.flags),
                    width: match c.wide {
                        haetty_vt::Wide::Narrow => Width::Narrow,
                        haetty_vt::Wide::Wide => Width::Wide,
                        haetty_vt::Wide::SpacerTail | haetty_vt::Wide::SpacerHead => Width::Spacer,
                    },
                });
            }
        }
        let m = t.modes;
        let cursor = if m.cursor_visible {
            t.cursor().map(|(x, y)| Cursor {
                x,
                y,
                shape: match t.cursor_shape() {
                    haetty_vt::CursorShape::Block => CursorShape::Block,
                    haetty_vt::CursorShape::Underline => CursorShape::Underline,
                    haetty_vt::CursorShape::Bar => CursorShape::Bar,
                },
                blinking: m.cursor_blink,
            })
        } else {
            None
        };
        let (fg, bg, cc) = t.colors();
        let sb = t.scrollback_rows();
        Snapshot {
            cols,
            rows,
            cells,
            cursor,
            default_fg: rgb(fg),
            default_bg: rgb(bg),
            cursor_color: cc.map(rgb),
            title: t.title().to_owned(),
            pwd: t.pwd().to_owned(),
            modes: Modes {
                alt_screen: m.alt_screen,
                mouse_tracking: m.mouse_tracking(),
                sgr_mouse: m.mouse_sgr,
                bracketed_paste: m.bracketed_paste,
                focus_events: m.focus_events,
                sync_output: m.sync_output,
                app_cursor_keys: m.app_cursor_keys,
                cursor_visible: m.cursor_visible,
                kitty_keyboard: t.kitty_flags(),
            },
            scrollbar: Scrollbar { total: sb + rows as u64, offset: sb - t.viewport_offset() as u64, len: rows as u64 },
            seq: 0,
        }
    }

    fn mode(&self, mode: VtMode) -> bool {
        let m = &self.term.modes;
        match mode {
            VtMode::BracketedPaste => m.bracketed_paste,
            VtMode::FocusEvents => m.focus_events,
            VtMode::ColorSchemeReport => m.color_scheme_report,
            VtMode::SyncOutput => m.sync_output,
            VtMode::MouseTracking => m.mouse_tracking(),
        }
    }

    fn scroll_viewport(&mut self, to: ScrollTo) {
        match to {
            ScrollTo::Top => self.term.scroll_viewport_top(),
            ScrollTo::Bottom => self.term.scroll_viewport_bottom(),
            ScrollTo::Delta(d) => self.term.scroll_viewport_delta(d),
        }
    }

    fn set_colors(&mut self, c: &TermColors) {
        self.term.set_colors(colors(c));
    }

    fn set_color_scheme(&mut self, s: ColorSchemePref, out: &mut Vec<u8>) {
        self.term.set_dark(dark(s));
        if self.term.modes.color_scheme_report {
            if let Some(d) = dark(s) {
                out.extend_from_slice(format!("\x1b[?997;{}n", if d { 1 } else { 2 }).as_bytes());
            }
        }
    }

    fn encode_key(&mut self, k: &KeyInput, out: &mut Vec<u8>) {
        let m = &self.term.modes;
        let o = crate::encode::KeyOptions {
            cursor_app: m.app_cursor_keys,
            keypad_app: m.app_keypad,
            backarrow: m.other(67),
            ignore_keypad_with_numlock: m.other(1035),
            alt_esc_prefix: m.other(1036),
            modify_other_keys_2: m.modify_other_keys_2,
            kitty: self.term.kitty_flags(),
        };
        crate::encode::key(k, &o, out);
    }

    fn encode_mouse(&mut self, m: &MouseInput, gs: GridSize, out: &mut Vec<u8>) {
        let o = crate::encode::MouseOptions {
            event: self.term.modes.mouse_event,
            format: self.term.modes.mouse_format,
            screen_w: gs.cols as u32 * gs.cell_width as u32,
            screen_h: gs.rows as u32 * gs.cell_height as u32,
            cell_w: gs.cell_width as u32,
            cell_h: gs.cell_height as u32,
            cols: gs.cols,
            rows: gs.rows,
        };
        // No motion dedupe: GhosttyEngine sets the encoder's options and
        // size before every event, and libghostty clears its last cell on
        // both, so motion within a cell is reported there too.
        crate::encode::mouse(m, &o, &mut None, out);
    }

    fn encode_paste(&mut self, text: &str, out: &mut Vec<u8>) {
        crate::encode::paste(text, self.term.modes.bracketed_paste, out);
    }

    fn encode_focus(&mut self, focused: bool, out: &mut Vec<u8>) {
        if self.term.modes.focus_events {
            out.extend_from_slice(if focused { b"\x1b[I" } else { b"\x1b[O" });
        }
    }

    fn dump(&mut self, _fmt: DumpFormat) -> String {
        let s = self.snapshot();
        (0..s.rows).map(|y| s.row_text(y)).collect::<Vec<_>>().join("\n")
    }

    fn scrollback_rows(&mut self) -> u64 {
        self.term.scrollback_rows()
    }
}
