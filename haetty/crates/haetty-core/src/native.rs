//! [`VtEngine`] on haetty's own engine (`haetty-vt`, feature `native`;
//! DESIGN.md §10). Not used by sessions yet: the default engine stays
//! libghostty-vt until the differential tests show no difference.

use crate::types::*;
use crate::vt::{VtEngine, VtInit, VtMode};
use crate::{ColorSchemePref, DumpFormat, SessionEvent, TermColors};

pub struct NativeEngine {
    term: haetty_vt::Terminal,
    colors: TermColors,
}

impl NativeEngine {
    pub fn new(init: &VtInit) -> NativeEngine {
        let gs = init.size.clamped();
        NativeEngine { term: haetty_vt::Terminal::new(gs.cols, gs.rows), colors: init.colors }
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
        Vec::new()
    }

    fn resize(&mut self, size: GridSize) {
        let gs = size.clamped();
        self.term.resize(gs.cols, gs.rows);
    }

    fn snapshot(&mut self) -> Snapshot {
        let (cols, rows) = self.term.size();
        let mut cells = Vec::with_capacity(cols as usize * rows as usize);
        for y in 0..rows {
            for x in 0..cols {
                cells.push(Cell { text: self.term.cell(x, y).text.clone(), ..Default::default() });
            }
        }
        let (cx, cy) = self.term.cursor();
        Snapshot {
            cols,
            rows,
            cells,
            cursor: Some(Cursor { x: cx.min(cols - 1), y: cy, shape: CursorShape::Block, blinking: false }),
            default_fg: self.colors.default_fg,
            default_bg: self.colors.default_bg,
            cursor_color: self.colors.cursor_color,
            modes: Modes { cursor_visible: true, ..Default::default() },
            ..Default::default()
        }
    }

    fn mode(&self, _m: VtMode) -> bool {
        false
    }

    fn scroll_viewport(&mut self, _to: ScrollTo) {}

    fn set_colors(&mut self, c: &TermColors) {
        self.colors = *c;
    }

    fn set_color_scheme(&mut self, _s: ColorSchemePref, _out: &mut Vec<u8>) {}

    fn encode_key(&mut self, k: &KeyInput, out: &mut Vec<u8>) {
        if k.action != KeyAction::Release {
            if let Some(t) = &k.text {
                out.extend_from_slice(t.as_bytes());
            }
        }
    }

    fn encode_mouse(&mut self, _m: &MouseInput, _size: GridSize, _out: &mut Vec<u8>) {}

    fn encode_paste(&mut self, text: &str, out: &mut Vec<u8>) {
        out.extend_from_slice(text.as_bytes());
    }

    fn encode_focus(&mut self, _focused: bool, _out: &mut Vec<u8>) {}

    fn dump(&mut self, _fmt: DumpFormat) -> String {
        let (cols, rows) = self.term.size();
        (0..rows)
            .map(|y| {
                let line: String = (0..cols).map(|x| self.term.cell(x, y).text.clone()).collect();
                line.trim_end().to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn scrollback_rows(&mut self) -> u64 {
        self.term.scrollback_rows()
    }
}
