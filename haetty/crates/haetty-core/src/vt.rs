//! The terminal engine behind a trait (DESIGN.md §10.4): the engine thread
//! (`engine.rs`) and the differential tool (`haetty-vtdiff`) drive an engine
//! only through [`VtEngine`]. Synchronous and single-threaded: an engine
//! object never leaves the thread that made it (libghostty-vt's is `!Send`).

use crate::types::*;
use crate::{ColorSchemePref, DumpFormat, SessionEvent, TermColors};

/// What an engine starts with.
#[derive(Clone, Debug)]
pub struct VtInit {
    pub size: GridSize,
    /// Scrollback limit in bytes (§3.4).
    pub scrollback_limit: usize,
    pub colors: TermColors,
    pub color_scheme: ColorSchemePref,
    /// The XTVERSION answer (`CSI > q`).
    pub xtversion: String,
}

/// A mode the engine thread needs to know.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VtMode {
    BracketedPaste,
    FocusEvents,
    ColorSchemeReport,
    SyncOutput,
    /// Any mouse tracking mode is on.
    MouseTracking,
}

pub trait VtEngine {
    /// Program output.
    fn feed(&mut self, bytes: &[u8]);
    /// Bytes the engine has to send to the program (query replies), in order.
    fn take_replies(&mut self) -> Vec<u8>;
    /// Bell, title, working directory, clipboard writes, in order.
    fn take_events(&mut self) -> Vec<SessionEvent>;
    /// A new grid (cells and cell pixels); soft-wrapped lines reflow.
    fn resize(&mut self, size: GridSize);
    /// The visible screen and state (`seq` is the caller's).
    fn snapshot(&mut self) -> Snapshot;
    fn mode(&self, m: VtMode) -> bool;
    fn scroll_viewport(&mut self, to: ScrollTo);
    fn set_colors(&mut self, c: &TermColors);
    /// The person's colour scheme; appends a report for the program when it
    /// asked for them (mode 2031).
    fn set_color_scheme(&mut self, s: ColorSchemePref, out: &mut Vec<u8>);
    /// Appends what a key sends to the program (nothing for unmapped keys
    /// without text).
    fn encode_key(&mut self, k: &KeyInput, out: &mut Vec<u8>);
    /// Appends a mouse report, when the program tracks the mouse.
    fn encode_mouse(&mut self, m: &MouseInput, size: GridSize, out: &mut Vec<u8>);
    /// A paste, bracketed when the program asked for it.
    fn encode_paste(&mut self, text: &str, out: &mut Vec<u8>);
    /// Focus in/out, when the program asked for it (mode 1004).
    fn encode_focus(&mut self, focused: bool, out: &mut Vec<u8>);
    fn dump(&mut self, fmt: DumpFormat) -> String;
    /// Rows above the visible screen (scrollback), for comparisons.
    fn scrollback_rows(&mut self) -> u64;
}
