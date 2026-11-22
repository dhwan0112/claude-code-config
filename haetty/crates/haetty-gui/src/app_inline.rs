use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant};

use haetty_core::Snapshot;

use super::Win;
use crate::inline_diff::{self, Drawn, Patch};
use crate::selection::{self, Point};
use crate::ui;
use haetty_core::TextRow;

const BUDGET: Duration = Duration::from_millis(1);

pub(super) struct Inline {
    key: u64,
    pub snap: Option<Snapshot>,
    pub drawn: Vec<Drawn>,
}

#[derive(Default)]
pub(super) struct TabPatches {
    pub gen: u64,
    pub list: Vec<Patch>,
    /// Blocks the person turned back to Claude Code's own shape (patch id, hunk).
    pub off: std::collections::HashSet<(String, usize)>,
}

impl Win {
    fn inline_fetch(&mut self) {
        let i = self.active;
        let Some(gen) = self.tabs[i].claude.as_ref().map(|c| c.patch_gen) else { return };
        if gen == self.tabs[i].patches.gen {
            return;
        }
        if let Ok((gen, list)) = self.client.patches(&self.tabs[i].session.id) {
            let p = &mut self.tabs[i].patches;
            p.gen = gen;
            p.list = list.iter().map(Patch::from_wire).collect();
        }
    }

    fn inline_key(&self, s: &Snapshot) -> u64 {
        let t = self.tab();
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (s.cols, s.rows, t.patches.gen, t.patches.off.len(), self.cfg.inline_min_cols).hash(&mut h);
        t.session.id.hash(&mut h);
        for c in &s.cells {
            c.text.hash(&mut h);
            c.bg.map(|b| (b.r, b.g, b.b)).hash(&mut h);
        }
        for (id, k) in &t.patches.off {
            (id, k).hash(&mut h);
        }
        h.finish()
    }

    /// Redraws the active tab's matched edit blocks side by side (`inline-diff = split`), cached by the screen's
    /// content; `self.inline` holds the result for drawing, clicks and copies.
    pub(super) fn inline_update(&mut self) {
        let on = self.cfg.inline_split && self.tab().claude.is_some();
        if !on {
            self.inline = None;
            return;
        }
        self.inline_fetch();
        let Some(s) = self.tab().snap.as_ref() else {
            self.inline = None;
            return;
        };
        if s.cols < self.cfg.inline_min_cols || self.tab().patches.list.is_empty() {
            self.inline = None;
            return;
        }
        let key = self.inline_key(s);
        if self.inline.as_ref().is_some_and(|c| c.key == key) {
            return;
        }
        let t0 = Instant::now();
        let t = self.tab();
        let found: Vec<_> = inline_diff::find(s, &t.patches.list)
            .into_iter()
            .filter(|f| !t.patches.off.contains(&(t.patches.list[f.patch].id.clone(), f.hunk)))
            .filter_map(|f| inline_diff::layout(s, &f, &t.patches.list[f.patch]).map(|d| (f, d)))
            .collect();
        if t0.elapsed() > BUDGET || found.is_empty() {
            if t0.elapsed() > BUDGET {
                log::debug!("inline diff: over the frame budget ({} us)", t0.elapsed().as_micros());
            }
            self.inline = Some(Inline { key, snap: None, drawn: vec![] });
            return;
        }
        let mut out = s.clone();
        let g = ui::Greys::new(s.default_fg, s.default_bg, &self.theme.palette);
        for (f, d) in &found {
            inline_diff::paint(&mut out, d, f, &t.patches.list[f.patch], g);
        }
        let drawn = found.into_iter().map(|(_, d)| d).collect();
        self.inline = Some(Inline { key, snap: Some(out), drawn });
    }

    /// The screen to compose: the redrawn one when a block was redrawn.
    pub(super) fn inline_screen(&self) -> Option<&Snapshot> {
        self.inline.as_ref().and_then(|c| c.snap.as_ref())
    }

    /// A press at a cell of the program's screen: the `⇄` mark turns its block back to Claude Code's shape, the
    /// file's name opens the diff overlay on that file. Returns whether it was one of them.
    pub(super) fn inline_click(&mut self, x: u16, y: u16) -> bool {
        let Some(c) = self.inline.as_ref() else { return false };
        if let Some(d) = c.drawn.iter().find(|d| d.mark == Some((x, y))) {
            let key = (d.id.clone(), d.hunk);
            let i = self.active;
            self.tabs[i].patches.off.insert(key);
            self.recompose();
            return true;
        }
        if let Some(d) = c.drawn.iter().find(|d| d.file.is_some_and(|(fy, a, b)| fy == y && x >= a && x < b)) {
            let path = d.path.clone();
            self.open_file_diff(&path);
            return true;
        }
        false
    }

    /// A selection over the screen that crosses a redrawn block: on the block's rows the side the drag is on gives
    /// its whole lines (no line numbers, no dim line; a side's empty row gives nothing), the other rows as usual.
    pub(super) fn inline_copy(&self, b: (Point, Point), rect: bool, top: u64, rows: &[TextRow]) -> Option<String> {
        let c = self.inline.as_ref()?;
        let r0 = u16::try_from(b.0.row.checked_sub(top)?).ok()?;
        let r1 = u16::try_from(b.1.row.checked_sub(top)?).ok()?;
        let block = |y: u16| c.drawn.iter().find(|d| y >= d.y0 && y < d.y1);
        if !(r0..=r1).any(|y| block(y).is_some()) {
            return None;
        }
        let x = if block(r0).is_some() { b.0.col } else { b.1.col };
        let mut out = vec![];
        for y in r0..=r1 {
            if let Some(d) = block(y) {
                let (l, r) = d.lines.get((y - d.y0) as usize)?;
                if let Some(t) = if x < d.x0 + d.half { l } else { r } {
                    out.push(t.clone());
                }
                continue;
            }
            let n = top + y as u64;
            let s = Point { row: n, col: if n == b.0.row || rect { b.0.col } else { 0 } };
            let e = Point { row: n, col: if n == b.1.row || rect { b.1.col } else { u16::MAX } };
            out.push(selection::text((s, e), rect, top, rows));
        }
        Some(out.join("\n"))
    }
}

impl Win {
    /// A left press over the program's screen, before a selection starts: see [`Win::inline_click`].
    pub(super) fn inline_press(&mut self) -> bool {
        let ((col, row), inside) = self.pointer_cell();
        if !inside || self.overlay_open() || self.palette.is_some() || self.settings_panel.is_some() {
            return false;
        }
        let (top, _, _) = self.term_area();
        self.inline_click(col, row - top)
    }
}
