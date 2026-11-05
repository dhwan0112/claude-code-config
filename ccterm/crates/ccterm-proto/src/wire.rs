//! Wire types and framing (DESIGN.md §3.6). Frames are a `u32` little-endian
//! length followed by a postcard-encoded message. postcard encodes enum
//! variants by index, and a receiver cannot skip one it does not know: any
//! new variant or field bumps [`VERSION`] (an older daemon refuses the
//! newer client in `Hello`, which the GUI explains).
//!
//! Text that came from the user or a program (pastes, hook payloads, file
//! contents) is wrapped in [`Secret`], whose `Debug` shows only its size, so
//! logging a message never logs its content.

use std::io::{self, Read, Write};

use ccterm_core::{
    Cell, CellFlags, Cursor, CursorShape, GridSize, Key, KeyAction, KeyInput, Modes, Mods, MouseAction, MouseButton,
    MouseInput, Progress, Rgb, ScrollTo, Scrollbar, SessionEvent, Snapshot, Width,
};
use serde::{Deserialize, Serialize};

pub const MAGIC: [u8; 4] = *b"CCTM";
pub const VERSION: u32 = 2;
/// Largest accepted frame; a bigger length closes the connection.
pub const MAX_FRAME: usize = 32 << 20;

pub type SessionId = String;

/// Content that must not reach a log: `Debug` prints its size only.
#[derive(Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(transparent)]
pub struct Secret<T>(pub T);

impl std::fmt::Debug for Secret<String> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "<{} bytes>", self.0.len())
    }
}

impl std::fmt::Debug for Secret<Vec<u8>> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "<{} bytes>", self.0.len())
    }
}

impl std::fmt::Debug for Secret<Option<String>> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            Some(s) => write!(f, "<{} bytes>", s.len()),
            None => f.write_str("None"),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// The local GUI: input and session management.
    Control,
    /// Read-only: lists and frames, never input.
    Observe,
    /// Hook and status-line processes: only `Hook` and `StatusLine`.
    Report,
}

/// Who produced an input message (DESIGN.md §3.7). While a session waits
/// for a Claude Code permission answer, only `Human` input is accepted.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    Human,
    Program,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Grid {
    pub cols: u16,
    pub rows: u16,
    pub cell_width: u16,
    pub cell_height: u16,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct WireKey {
    pub key: u32,
    pub action: u32,
    pub mods: u16,
    pub consumed_mods: u16,
    pub text: Option<String>,
    pub unshifted: Option<char>,
    pub composing: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct WireMouse {
    pub action: u32,
    pub button: Option<u32>,
    pub mods: u16,
    pub x: f32,
    pub y: f32,
    pub any_pressed: bool,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum WireScroll {
    Top,
    Bottom,
    Delta(i64),
}

/// What a new session runs. The environment is used for the child only and
/// never written to disk (DESIGN.md §3.6).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CreateSpec {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub env: Vec<(String, String)>,
    pub size: Grid,
    pub scrollback_limit: u64,
    pub default_fg: [u8; 3],
    pub default_bg: [u8; 3],
    /// Claude Code integration (hooks, status line) when the program is
    /// Claude Code (DESIGN.md §3.13).
    pub claude_integration: bool,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionState {
    Running,
    Exited { code: Option<u32> },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SessionInfo {
    pub id: SessionId,
    pub program: String,
    pub title: String,
    pub cwd: String,
    pub state: SessionState,
    /// Unix time in milliseconds.
    pub created_ms: u64,
    pub pid: Option<u32>,
    pub cols: u16,
    pub rows: u16,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum ClientMsg {
    Hello {
        magic: [u8; 4],
        version: u32,
        role: Role,
        name: String,
    },
    ListSessions,
    Create {
        req: u32,
        spec: CreateSpec,
    },
    Attach {
        session: SessionId,
    },
    Detach {
        session: SessionId,
    },
    /// The client applied the last frame of `session` and wants the next.
    NextFrame {
        session: SessionId,
    },
    Key {
        session: SessionId,
        origin: Origin,
        key: WireKey,
    },
    Mouse {
        session: SessionId,
        origin: Origin,
        mouse: WireMouse,
    },
    Wheel {
        session: SessionId,
        origin: Origin,
        lines: i32,
        col: u16,
        row: u16,
        mods: u16,
    },
    Paste {
        session: SessionId,
        origin: Origin,
        text: Secret<String>,
    },
    Write {
        session: SessionId,
        origin: Origin,
        bytes: Secret<Vec<u8>>,
    },
    Focus {
        session: SessionId,
        focused: bool,
    },
    Resize {
        session: SessionId,
        size: Grid,
    },
    Scroll {
        session: SessionId,
        to: WireScroll,
    },
    /// 0 dark, 1 light, 2 unknown.
    SetColorScheme {
        session: SessionId,
        scheme: u8,
    },
    /// End the session (kills the program) and forget it.
    Close {
        session: SessionId,
    },
    /// A Claude Code hook event; `json` is the hook's stdin. `ts_ms`: when
    /// the hook process started (orders events of concurrent hooks).
    /// `snapshot`: for an edit tool's PreToolUse, the file before the edit.
    Hook {
        session: Option<SessionId>,
        event: String,
        json: Secret<String>,
        ts_ms: u64,
        snapshot: Option<PreSnapshot>,
    },
    /// Claude Code status line JSON.
    StatusLine {
        session: Option<SessionId>,
        json: Secret<String>,
        ts_ms: u64,
    },
    /// The user saw the tab: a "waiting" mark goes (a question stays).
    ClearAttention {
        session: SessionId,
    },
    /// The last edit of the session, before and after (answer: `Edit`).
    GetEdit {
        session: SessionId,
    },
    /// Answered with `Synced` once every earlier message was handled.
    Sync,
}

/// A file as the hook found it before an edit tool ran.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PreSnapshot {
    pub path: String,
    pub status: SnapStatus,
    pub text: Secret<Option<String>>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapStatus {
    /// The file did not exist.
    New,
    Copied,
    /// Larger than the 2 MiB limit.
    TooLarge,
    /// Not UTF-8 text.
    Binary,
    /// Looks like a credential or key file: never read.
    Skipped,
    Error,
}

/// What a tab marks (DESIGN.md §3.13): `Wait` — Claude Code finished or
/// idles at its prompt; `Ask` — it waits for a permission answer or input.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Attention {
    #[default]
    None,
    Wait,
    Ask,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Limit {
    /// `five_hour`, `seven_day`, `spend_limit`, …
    pub name: String,
    pub used_pct: f32,
    /// Unix seconds.
    pub resets_at: Option<i64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct EditMeta {
    pub path: String,
    /// `main` or the subagent type.
    pub agent: String,
    pub at_ms: u64,
    pub added: u32,
    pub removed: u32,
    /// Before/after known (else only the path is shown).
    pub diffable: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Worker {
    pub id: String,
    pub kind: String,
    pub started_ms: u64,
    pub ended_ms: Option<u64>,
}

/// What ccterm knows about the Claude Code session in a tab, from hooks and
/// status-line JSON (derived fields only; never prompts or file contents).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct ClaudeInfo {
    /// Claude Code's session id (UUID), for `--resume`.
    pub claude_session: String,
    pub name: String,
    pub model: String,
    pub attention: Attention,
    /// Between UserPromptSubmit and Stop.
    pub busy: bool,
    pub context_pct: Option<f32>,
    pub context_size: Option<u64>,
    pub limits: Vec<Limit>,
    pub cost_usd: Option<f64>,
    pub lines_added: u64,
    pub lines_removed: u64,
    pub files_changed: u32,
    pub edits: u32,
    pub last_edit: Option<EditMeta>,
    /// Running subagents, then the last finished ones.
    pub workers: Vec<Worker>,
    pub cache_warm: Option<bool>,
    pub cache_hit_pct: Option<f32>,
    /// Last status-line update, Unix ms.
    pub updated_ms: u64,
}

/// The last edit with its text before and after.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct EditView {
    pub meta: EditMeta,
    pub status: SnapStatus,
    pub before: Secret<String>,
    pub after: Secret<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct WireCell {
    pub text: String,
    pub fg: Option<[u8; 3]>,
    pub bg: Option<[u8; 3]>,
    pub flags: u16,
    /// 0 narrow, 1 wide, 2 spacer.
    pub width: u8,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct WireCursor {
    pub x: u16,
    pub y: u16,
    /// 0 block, 1 bar, 2 underline, 3 hollow block.
    pub shape: u8,
    pub blinking: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Frame {
    pub session: SessionId,
    pub seq: u64,
    /// All rows follow (attach, size change); otherwise only changed rows.
    pub full: bool,
    pub cols: u16,
    pub rows: u16,
    pub changed: Vec<(u16, Vec<WireCell>)>,
    pub cursor: Option<WireCursor>,
    pub default_fg: [u8; 3],
    pub default_bg: [u8; 3],
    pub cursor_color: Option<[u8; 3]>,
    pub title: String,
    pub pwd: String,
    /// Bit set of `Modes` booleans (see [`modes_bits`]) and the kitty flags.
    pub modes: u16,
    pub kitty_keyboard: u8,
    pub scrollbar: (u64, u64, u64),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum WireProgress {
    Clear,
    Normal(u8),
    Error(u8),
    Indeterminate,
    Paused(u8),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum WireEvent {
    Title(String),
    Pwd(String),
    Bell,
    ClipboardWrite(String),
    Notification { title: Option<String>, body: String },
    Progress(WireProgress),
    Exited { code: Option<u32>, signal: Option<String> },
    Error(String),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum DaemonMsg {
    Welcome {
        version: u32,
        daemon_pid: u32,
    },
    Refused {
        reason: String,
    },
    Sessions(Vec<SessionInfo>),
    Created {
        req: u32,
        session: SessionId,
    },
    CreateFailed {
        req: u32,
        error: String,
    },
    Frame(Frame),
    Event {
        session: SessionId,
        event: WireEvent,
    },
    /// A message the client's role or the session's state does not allow.
    Denied {
        what: String,
    },
    /// The session is gone (closed, or never existed).
    NoSession {
        session: SessionId,
    },
    /// Claude Code state of a session (on attach and on every change).
    Claude {
        session: SessionId,
        info: Box<ClaudeInfo>,
    },
    /// Answer to `GetEdit`.
    Edit {
        session: SessionId,
        edit: Option<Box<EditView>>,
    },
    Synced,
}

// ---------------------------------------------------------------- framing

pub fn write_msg<T: Serialize>(w: &mut impl Write, msg: &T) -> io::Result<()> {
    let body = postcard::to_stdvec(msg).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let mut buf = Vec::with_capacity(body.len() + 4);
    buf.extend_from_slice(&(body.len() as u32).to_le_bytes());
    buf.extend_from_slice(&body);
    w.write_all(&buf)?;
    w.flush()
}

/// Reads one frame; `UnexpectedEof` at a frame boundary means the peer
/// closed the connection.
pub fn read_msg<T: for<'de> Deserialize<'de>>(r: &mut impl Read) -> io::Result<T> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len)?;
    let n = u32::from_le_bytes(len) as usize;
    if n > MAX_FRAME {
        return Err(io::Error::new(io::ErrorKind::InvalidData, format!("frame of {n} bytes")));
    }
    let mut body = vec![0u8; n];
    r.read_exact(&mut body)?;
    postcard::from_bytes(&body).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

// ------------------------------------------------------------ conversions

pub fn rgb(c: Rgb) -> [u8; 3] {
    [c.r, c.g, c.b]
}

pub fn from_rgb(c: [u8; 3]) -> Rgb {
    Rgb { r: c[0], g: c[1], b: c[2] }
}

impl From<GridSize> for Grid {
    fn from(g: GridSize) -> Self {
        Grid { cols: g.cols, rows: g.rows, cell_width: g.cell_width, cell_height: g.cell_height }
    }
}

impl From<Grid> for GridSize {
    fn from(g: Grid) -> Self {
        GridSize::new(g.cols, g.rows).with_cell(g.cell_width, g.cell_height)
    }
}

impl From<&KeyInput> for WireKey {
    fn from(k: &KeyInput) -> Self {
        WireKey {
            key: k.key as u32,
            action: k.action as u32,
            mods: k.mods.bits(),
            consumed_mods: k.consumed_mods.bits(),
            text: k.text.clone(),
            unshifted: k.unshifted,
            composing: k.composing,
        }
    }
}

impl WireKey {
    pub fn to_key_input(&self) -> Option<KeyInput> {
        Some(KeyInput {
            key: Key::try_from(self.key).ok()?,
            action: KeyAction::try_from(self.action).ok()?,
            mods: Mods::from_bits_truncate(self.mods),
            consumed_mods: Mods::from_bits_truncate(self.consumed_mods),
            text: self.text.clone(),
            unshifted: self.unshifted,
            composing: self.composing,
        })
    }
}

impl From<&MouseInput> for WireMouse {
    fn from(m: &MouseInput) -> Self {
        WireMouse {
            action: m.action as u32,
            button: m.button.map(|b| b as u32),
            mods: m.mods.bits(),
            x: m.x,
            y: m.y,
            any_pressed: m.any_pressed,
        }
    }
}

impl WireMouse {
    pub fn to_mouse_input(&self) -> Option<MouseInput> {
        let button = match self.button {
            Some(b) => Some(MouseButton::try_from(b).ok()?),
            None => None,
        };
        Some(MouseInput {
            action: MouseAction::try_from(self.action).ok()?,
            button,
            mods: Mods::from_bits_truncate(self.mods),
            x: self.x,
            y: self.y,
            any_pressed: self.any_pressed,
        })
    }
}

impl From<ScrollTo> for WireScroll {
    fn from(s: ScrollTo) -> Self {
        match s {
            ScrollTo::Top => WireScroll::Top,
            ScrollTo::Bottom => WireScroll::Bottom,
            ScrollTo::Delta(d) => WireScroll::Delta(d as i64),
        }
    }
}

impl From<WireScroll> for ScrollTo {
    fn from(s: WireScroll) -> Self {
        match s {
            WireScroll::Top => ScrollTo::Top,
            WireScroll::Bottom => ScrollTo::Bottom,
            WireScroll::Delta(d) => ScrollTo::Delta(d as isize),
        }
    }
}

pub fn cell_to_wire(c: &Cell) -> WireCell {
    WireCell {
        text: c.text.clone(),
        fg: c.fg.map(rgb),
        bg: c.bg.map(rgb),
        flags: c.flags.0,
        width: match c.width {
            Width::Narrow => 0,
            Width::Wide => 1,
            Width::Spacer => 2,
        },
    }
}

pub fn wire_to_cell(c: &WireCell) -> Cell {
    Cell {
        text: c.text.clone(),
        fg: c.fg.map(from_rgb),
        bg: c.bg.map(from_rgb),
        flags: CellFlags(c.flags),
        width: match c.width {
            1 => Width::Wide,
            2 => Width::Spacer,
            _ => Width::Narrow,
        },
    }
}

/// `Modes` booleans as bits, in declaration order.
pub fn modes_bits(m: &Modes) -> u16 {
    [
        m.alt_screen,
        m.mouse_tracking,
        m.sgr_mouse,
        m.bracketed_paste,
        m.focus_events,
        m.sync_output,
        m.app_cursor_keys,
        m.cursor_visible,
    ]
    .iter()
    .enumerate()
    .fold(0, |acc, (i, &b)| acc | ((b as u16) << i))
}

pub fn modes_from_bits(bits: u16, kitty: u8) -> Modes {
    let b = |i: u16| bits & (1 << i) != 0;
    Modes {
        alt_screen: b(0),
        mouse_tracking: b(1),
        sgr_mouse: b(2),
        bracketed_paste: b(3),
        focus_events: b(4),
        sync_output: b(5),
        app_cursor_keys: b(6),
        cursor_visible: b(7),
        kitty_keyboard: kitty,
    }
}

fn cursor_to_wire(c: &Cursor) -> WireCursor {
    let shape = match c.shape {
        CursorShape::Block => 0,
        CursorShape::Bar => 1,
        CursorShape::Underline => 2,
        CursorShape::BlockHollow => 3,
    };
    WireCursor { x: c.x, y: c.y, shape, blinking: c.blinking }
}

fn wire_to_cursor(c: &WireCursor) -> Cursor {
    let shape = match c.shape {
        1 => CursorShape::Bar,
        2 => CursorShape::Underline,
        3 => CursorShape::BlockHollow,
        _ => CursorShape::Block,
    };
    Cursor { x: c.x, y: c.y, shape, blinking: c.blinking }
}

/// The frame that brings a client from `base` (what it has) to `next`: full
/// when there is no base or the size differs, else the changed rows.
pub fn frame_between(session: &str, base: Option<&Snapshot>, next: &Snapshot) -> Frame {
    let same_size = base.is_some_and(|b| b.cols == next.cols && b.rows == next.rows);
    let cols = next.cols as usize;
    fn row(s: &Snapshot, y: usize, cols: usize) -> &[Cell] {
        &s.cells[y * cols..(y + 1) * cols]
    }
    let mut changed = vec![];
    for y in 0..next.rows as usize {
        let differs = match base {
            Some(b) if same_size => row(b, y, cols) != row(next, y, cols),
            _ => true,
        };
        if differs {
            changed.push((y as u16, row(next, y, cols).iter().map(cell_to_wire).collect()));
        }
    }
    Frame {
        session: session.to_owned(),
        seq: next.seq,
        full: !same_size,
        cols: next.cols,
        rows: next.rows,
        changed,
        cursor: next.cursor.as_ref().map(cursor_to_wire),
        default_fg: rgb(next.default_fg),
        default_bg: rgb(next.default_bg),
        cursor_color: next.cursor_color.map(rgb),
        title: next.title.clone(),
        pwd: next.pwd.clone(),
        modes: modes_bits(&next.modes),
        kitty_keyboard: next.modes.kitty_keyboard,
        scrollbar: (next.scrollbar.total, next.scrollbar.offset, next.scrollbar.len),
    }
}

/// Applies a frame to the client's mirror of the screen.
pub fn apply_frame(mirror: &mut Snapshot, f: &Frame) {
    if f.full || mirror.cols != f.cols || mirror.rows != f.rows {
        mirror.cols = f.cols;
        mirror.rows = f.rows;
        mirror.cells = vec![Cell::default(); f.cols as usize * f.rows as usize];
    }
    let cols = f.cols as usize;
    for (y, cells) in &f.changed {
        let start = *y as usize * cols;
        for (i, c) in cells.iter().take(cols).enumerate() {
            if let Some(slot) = mirror.cells.get_mut(start + i) {
                *slot = wire_to_cell(c);
            }
        }
    }
    mirror.seq = f.seq;
    mirror.cursor = f.cursor.as_ref().map(wire_to_cursor);
    mirror.default_fg = from_rgb(f.default_fg);
    mirror.default_bg = from_rgb(f.default_bg);
    mirror.cursor_color = f.cursor_color.map(from_rgb);
    mirror.title.clone_from(&f.title);
    mirror.pwd.clone_from(&f.pwd);
    mirror.modes = modes_from_bits(f.modes, f.kitty_keyboard);
    mirror.scrollbar = Scrollbar { total: f.scrollbar.0, offset: f.scrollbar.1, len: f.scrollbar.2 };
}

pub fn event_to_wire(e: &SessionEvent) -> Option<WireEvent> {
    Some(match e {
        SessionEvent::Title(t) => WireEvent::Title(t.clone()),
        SessionEvent::Pwd(p) => WireEvent::Pwd(p.clone()),
        SessionEvent::Bell => WireEvent::Bell,
        SessionEvent::ClipboardWrite(t) => WireEvent::ClipboardWrite(t.clone()),
        SessionEvent::Notification { title, body } => {
            WireEvent::Notification { title: title.clone(), body: body.clone() }
        }
        SessionEvent::Progress(p) => WireEvent::Progress(match *p {
            Progress::Clear => WireProgress::Clear,
            Progress::Normal(n) => WireProgress::Normal(n),
            Progress::Error(n) => WireProgress::Error(n),
            Progress::Indeterminate => WireProgress::Indeterminate,
            Progress::Paused(n) => WireProgress::Paused(n),
        }),
        SessionEvent::Exited(s) => WireEvent::Exited { code: s.code, signal: s.signal.clone() },
        SessionEvent::Error(e) => WireEvent::Error(e.clone()),
        SessionEvent::Wakeup | SessionEvent::Reply(_) => return None,
    })
}

pub fn wire_to_event(e: WireEvent) -> SessionEvent {
    match e {
        WireEvent::Title(t) => SessionEvent::Title(t),
        WireEvent::Pwd(p) => SessionEvent::Pwd(p),
        WireEvent::Bell => SessionEvent::Bell,
        WireEvent::ClipboardWrite(t) => SessionEvent::ClipboardWrite(t),
        WireEvent::Notification { title, body } => SessionEvent::Notification { title, body },
        WireEvent::Progress(p) => SessionEvent::Progress(match p {
            WireProgress::Clear => Progress::Clear,
            WireProgress::Normal(n) => Progress::Normal(n),
            WireProgress::Error(n) => Progress::Error(n),
            WireProgress::Indeterminate => Progress::Indeterminate,
            WireProgress::Paused(n) => Progress::Paused(n),
        }),
        WireEvent::Exited { code, signal } => SessionEvent::Exited(ccterm_core::ExitStatus { code, signal }),
        WireEvent::Error(e) => SessionEvent::Error(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(cols: u16, rows: u16, fill: &str) -> Snapshot {
        Snapshot {
            cols,
            rows,
            cells: vec![Cell { text: fill.into(), ..Default::default() }; cols as usize * rows as usize],
            seq: 1,
            ..Default::default()
        }
    }

    #[test]
    fn frames_round_trip_and_carry_only_changed_rows() {
        let a = snap(4, 3, "a");
        let mut b = a.clone();
        b.cells[5] =
            Cell { text: "한".into(), width: Width::Wide, fg: Some(Rgb { r: 1, g: 2, b: 3 }), ..Default::default() };
        b.seq = 2;
        b.title = "t".into();
        b.modes.alt_screen = true;
        b.modes.kitty_keyboard = 5;
        let full = frame_between("s", None, &a);
        assert!(full.full && full.changed.len() == 3);
        let diff = frame_between("s", Some(&a), &b);
        assert!(!diff.full);
        assert_eq!(diff.changed.iter().map(|(y, _)| *y).collect::<Vec<_>>(), vec![1]);
        // Through the wire.
        let mut buf = vec![];
        write_msg(&mut buf, &DaemonMsg::Frame(diff.clone())).unwrap();
        let back: DaemonMsg = read_msg(&mut &buf[..]).unwrap();
        assert_eq!(back, DaemonMsg::Frame(diff.clone()));
        let mut mirror = Snapshot::default();
        apply_frame(&mut mirror, &full);
        apply_frame(&mut mirror, &diff);
        assert!(mirror.cells == b.cells);
        assert_eq!((mirror.seq, mirror.title.as_str(), mirror.modes), (2, "t", b.modes));
        // A size change is a full frame.
        let c = snap(5, 3, "c");
        assert!(frame_between("s", Some(&b), &c).full);
    }

    #[test]
    fn keys_and_mice_round_trip() {
        let k = KeyInput::parse("ctrl+c").unwrap();
        let w = WireKey::from(&k);
        let back = w.to_key_input().unwrap();
        assert_eq!((back.key, back.action, back.mods, back.unshifted), (k.key, k.action, k.mods, k.unshifted));
        let m = MouseInput::at_pixel(MouseAction::Press, Some(MouseButton::Left), 3.0, 4.0);
        let mb = WireMouse::from(&m).to_mouse_input().unwrap();
        assert_eq!((mb.action, mb.button, mb.x), (m.action, m.button, m.x));
        assert!(WireKey { key: 99_999, ..w }.to_key_input().is_none());
    }

    #[test]
    fn oversized_and_truncated_frames_are_errors() {
        let mut big = vec![];
        big.extend_from_slice(&((MAX_FRAME as u32) + 1).to_le_bytes());
        assert!(read_msg::<DaemonMsg>(&mut &big[..]).is_err());
        let mut buf = vec![];
        write_msg(&mut buf, &ClientMsg::ListSessions).unwrap();
        assert!(read_msg::<ClientMsg>(&mut &buf[..buf.len() - 1]).is_err());
    }
}
