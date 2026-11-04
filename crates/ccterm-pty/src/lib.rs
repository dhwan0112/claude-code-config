//! Pseudo-terminal layer for ccterm.
//!
//! One interface over two very different OS facilities:
//!
//! * Unix: a classic PTY pair (via `portable-pty`). Bytes pass through unchanged.
//! * Windows: ConPTY. The in-box console host (conhost) sits between the
//!   child and the terminal, re-renders the child's output and does not pass
//!   everything on (mouse-mode requests on older builds, APC strings, kitty
//!   keyboard negotiation). Microsoft ships a newer, out-of-band ConPTY —
//!   `conpty.dll` plus `OpenConsole.exe`, MIT-licensed, NuGet package
//!   `Microsoft.Windows.Console.ConPTY` — and ccterm prefers it: a copy given
//!   by `CCTERM_CONPTY_DLL` or placed next to the executable, else the copy
//!   embedded at build time (feature `embedded-conpty`), else the in-box host.
//!   `portable-pty` cannot choose the DLL or the creation flags, so the
//!   Windows backend (`win/`) talks to the ConPTY API directly.
//!
//! Note on flags: `0x8` meant `PSEUDOCONSOLE_PASSTHROUGH_MODE` in older
//! declarations but is `PSEUDOCONSOLE_GLYPH_WIDTH_GRAPHEMES` in current
//! ConPTY headers, so ccterm never sends it to the in-box host and only sends
//! glyph-width flags to the out-of-band one.

use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::path::PathBuf;

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod win;

/// Size of the terminal grid. Pixel sizes are advisory (Unix `TIOCSWINSZ`
/// carries them, ConPTY ignores them).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PtySize {
    pub cols: u16,
    pub rows: u16,
    pub pixel_width: u16,
    pub pixel_height: u16,
}

impl PtySize {
    pub fn cells(cols: u16, rows: u16) -> Self {
        Self { cols, rows, pixel_width: 0, pixel_height: 0 }
    }
}

/// What to run inside the pseudo-terminal.
#[derive(Clone, Debug, Default)]
pub struct CommandSpec {
    pub program: OsString,
    pub args: Vec<OsString>,
    pub cwd: Option<PathBuf>,
    /// Environment changes applied on top of the parent's environment, in
    /// order. `None` removes the variable.
    pub env: Vec<(OsString, Option<OsString>)>,
}

impl CommandSpec {
    pub fn new(program: impl Into<OsString>) -> Self {
        Self { program: program.into(), ..Default::default() }
    }
    pub fn arg(mut self, a: impl Into<OsString>) -> Self {
        self.args.push(a.into());
        self
    }
    pub fn args<I, S>(mut self, it: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        self.args.extend(it.into_iter().map(Into::into));
        self
    }
    pub fn cwd(mut self, dir: impl Into<PathBuf>) -> Self {
        self.cwd = Some(dir.into());
        self
    }
    pub fn env(mut self, k: impl Into<OsString>, v: impl Into<OsString>) -> Self {
        self.env.push((k.into(), Some(v.into())));
        self
    }
    pub fn env_remove(mut self, k: impl Into<OsString>) -> Self {
        self.env.push((k.into(), None));
        self
    }
}

/// How the out-of-band ConPTY measures text width (Windows only). Ignored
/// by the in-box console host.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum GlyphWidth {
    /// Grapheme clusters, closest to libghostty-vt's own measurement.
    #[default]
    Graphemes,
    Wcswidth,
    Console,
    /// Send no width flag; OpenConsole picks its default.
    HostDefault,
}

/// Platform knobs. Ignored where they do not apply.
#[derive(Clone, Debug)]
pub struct PtyOptions {
    /// Windows: look for `conpty.dll` in `CCTERM_CONPTY_DLL` or next to the
    /// executable first.
    pub conpty_sideload: bool,
    /// Windows: otherwise use the copy embedded in the binary (written once to
    /// `%LOCALAPPDATA%\ccterm\conpty\<version>`), if built with
    /// `embedded-conpty`.
    pub conpty_embedded: bool,
    pub conpty_glyph_width: GlyphWidth,
}

impl Default for PtyOptions {
    fn default() -> Self {
        Self { conpty_sideload: true, conpty_embedded: true, conpty_glyph_width: GlyphWidth::Graphemes }
    }
}

/// How the child ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExitStatus {
    pub code: Option<u32>,
    /// Unix only: the terminating signal, if any.
    pub signal: Option<String>,
}

impl ExitStatus {
    pub fn success(&self) -> bool {
        self.code == Some(0)
    }
}

/// Resizing and backend information, kept apart from the byte streams so it
/// can live on another thread.
pub trait PtyControl: Send {
    fn resize(&self, size: PtySize) -> io::Result<()>;
    /// Description of the backend actually in use, for logs and the status
    /// panel, e.g. `"unix-pty"` or `"conpty embedded 1.25.260930003 flags=0x8"`.
    fn describe(&self) -> String;
}

pub trait ChildProcess: Send {
    fn pid(&self) -> Option<u32>;
    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>>;
    fn wait(&mut self) -> io::Result<ExitStatus>;
    fn kill(&mut self) -> io::Result<()>;
}

/// A running child attached to a pseudo-terminal.
pub struct Spawned {
    pub reader: Box<dyn Read + Send>,
    pub writer: Box<dyn Write + Send>,
    pub control: Box<dyn PtyControl>,
    pub child: Box<dyn ChildProcess>,
}

/// Start `spec` in a new pseudo-terminal of `size`.
pub fn spawn(spec: &CommandSpec, size: PtySize, opts: &PtyOptions) -> io::Result<Spawned> {
    #[cfg(unix)]
    {
        let _ = opts;
        unix::spawn(spec, size)
    }
    #[cfg(windows)]
    {
        win::spawn(spec, size, opts)
    }
}
