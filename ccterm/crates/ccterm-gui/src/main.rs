//! `ccterm`: terminal windows whose sessions live in the ccterm daemon.
//!
//! ```text
//! ccterm [gui] [--font FAMILY] [--font-size PT] [--cols N] [--rows N] [--KEY VALUE]... [--] [PROGRAM [ARGS...]]
//! ccterm daemon
//! ccterm status [--json | --screen ID]
//! ```
//! Without a program it runs the user's shell (`$SHELL`, else `/bin/sh`;
//! Windows: `%COMSPEC%`, else `cmd.exe`). The first word after the options is
//! the program (`ccterm claude`), except the subcommand names in
//! [`SUBCOMMANDS`]; a program with one of those names goes after `--`
//! (`ccterm -- status`).
//!
//! Environment: `CCTERM_LOG=error|warn|info|debug` (default warn),
//! `CCTERM_LOG_FILE=path` (log there instead of stderr; on Windows the
//! release build has no console), `CCTERM_DUMP_FRAME=path.ppm` (write every
//! frame as an image, for debugging), `CCTERM_FONT_DIR=dir` (extra fonts),
//! `CCTERM_DEBUG_PADDING=1` (paint everything outside the grid red).
//!
//! `KEY` is any setting in [`config`] (`--window-padding-color extend` or
//! `--window-padding-color=extend`).
#![cfg_attr(all(windows, not(debug_assertions), not(test)), windows_subsystem = "windows")]

mod app;
mod chrome;
mod cli;
mod clipboard;
mod config;
mod damage;
mod emoji;
mod font;
mod geom;
mod input;
mod render;

use std::io::Write;
use std::sync::Mutex;

use ccterm_core::CommandSpec;
use winit::event_loop::{ControlFlow, EventLoop};

#[derive(Clone, Debug)]
pub struct Options {
    pub font: Option<String>,
    pub font_size: f64,
    pub cols: u16,
    pub rows: u16,
    pub command: CommandSpec,
    pub config: config::Config,
}

fn default_shell() -> CommandSpec {
    #[cfg(windows)]
    let prog = std::env::var_os("COMSPEC").unwrap_or_else(|| "cmd.exe".into());
    #[cfg(not(windows))]
    let prog = std::env::var_os("SHELL").filter(|s| !s.is_empty()).unwrap_or_else(|| "/bin/sh".into());
    CommandSpec::new(prog)
}

/// All subcommand names (`docs/DESIGN.md` §3.2, §6). Implemented ones are
/// in [`IMPLEMENTED`] and must be the first word; the others are reserved:
/// as a first word they end with an error instead of running a program of
/// that name, so adding them later does not change what a command line does.
pub const SUBCOMMANDS: &[&str] =
    &["gui", "daemon", "hook", "statusline", "run", "headless", "install-desktop", "status"];
pub const IMPLEMENTED: &[&str] = &["gui", "daemon", "status"];

/// What the command line asks for.
#[derive(Debug)]
pub enum Command {
    Gui(Box<Options>),
    Daemon,
    Status {
        json: bool,
    },
    /// `status --screen ID`: one session's screen as text (read-only).
    Screen {
        id: String,
    },
}

fn parse_command(args: Vec<String>) -> Result<Command, String> {
    match args.first().map(String::as_str) {
        Some("daemon") => match args.len() {
            1 => Ok(Command::Daemon),
            _ => Err("usage: ccterm daemon".into()),
        },
        Some("status") => match &args[1..] {
            [] => Ok(Command::Status { json: false }),
            [j] if j == "--json" => Ok(Command::Status { json: true }),
            [s, id] if s == "--screen" => Ok(Command::Screen { id: id.clone() }),
            _ => Err("usage: ccterm status [--json | --screen ID]".into()),
        },
        Some("gui") => parse_args(args.into_iter().skip(1)).map(|o| Command::Gui(Box::new(o))),
        _ => parse_args(args).map(|o| Command::Gui(Box::new(o))),
    }
}

const USAGE: &str =
    "usage: ccterm [--font FAMILY] [--font-size PT] [--cols N] [--rows N] [--KEY VALUE]... [--] [PROGRAM [ARGS...]]\n\
     settings (KEY): window-padding N, window-padding-balance true|false,\n\
     window-padding-color background|extend|extend-always, decorations native|integrated|none,\n\
     scrollback-limit BYTES (default 12000000, ~1.1 KB per 120-column row)\n\
     subcommands: ccterm gui [OPTIONS] [PROGRAM...], ccterm daemon, ccterm status [--json | --screen ID]\n\
     reserved (not implemented yet): hook, statusline, run, headless, install-desktop;\n\
     to run a program with a subcommand's name: ccterm [OPTIONS] -- NAME [ARGS...]";

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Options, String> {
    let mut o = Options {
        font: None,
        font_size: 11.0,
        cols: 120,
        rows: 36,
        command: default_shell(),
        config: config::Config::default(),
    };
    let mut it = args.into_iter();
    let mut program: Vec<String> = vec![];
    while let Some(a) = it.next() {
        let mut val = |name: &str| it.next().ok_or(format!("{name} needs a value"));
        match a.as_str() {
            "--font" => o.font = Some(val("--font")?),
            "--font-size" => {
                o.font_size = val("--font-size")?.parse().map_err(|_| "bad --font-size")?;
                if !(4.0..=96.0).contains(&o.font_size) {
                    return Err("--font-size must be between 4 and 96".into());
                }
            }
            "--cols" => o.cols = val("--cols")?.parse().map_err(|_| "bad --cols")?,
            "--rows" => o.rows = val("--rows")?.parse().map_err(|_| "bad --rows")?,
            "-h" | "--help" => return Err(USAGE.into()),
            "--version" => return Err(format!("ccterm {}", env!("CARGO_PKG_VERSION"))),
            "--" => {
                program.extend(it.by_ref());
                break;
            }
            s if s.starts_with("--") => {
                let (key, inline) = match s[2..].split_once('=') {
                    Some((k, v)) => (k, Some(v.to_owned())),
                    None => (&s[2..], None),
                };
                if !config::KEYS.contains(&key) {
                    return Err(format!("unknown option {s}\n{USAGE}"));
                }
                let value = match inline {
                    Some(v) => v,
                    None => it.next().ok_or(format!("--{key} needs a value"))?,
                };
                o.config.set(key, &value)?;
            }
            s if IMPLEMENTED.contains(&s) => {
                return Err(format!(
                    "ccterm: '{s}' is a subcommand and must come first (ccterm {s} ...); \
                     to run a program named {s}, use: ccterm -- {s}"
                ));
            }
            s if SUBCOMMANDS.contains(&s) => {
                return Err(format!(
                    "ccterm: '{s}' is a subcommand that is not implemented yet; \
                     to run a program named {s}, use: ccterm -- {s}"
                ));
            }
            _ => {
                program.push(a);
                program.extend(it.by_ref());
                break;
            }
        }
    }
    if let Some((prog, rest)) = program.split_first() {
        o.command = CommandSpec::new(prog).args(rest);
    }
    o.cols = o.cols.max(2);
    o.rows = o.rows.max(1);
    Ok(o)
}

/// Display width of a character in cells (0, 1 or 2).
pub fn text_width(c: char) -> usize {
    unicode_width::UnicodeWidthChar::width(c).unwrap_or(0)
}

struct Logger {
    level: log::LevelFilter,
    file: Option<Mutex<std::fs::File>>,
}

impl log::Log for Logger {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= self.level && (m.target().starts_with("ccterm") || m.level() <= log::Level::Warn)
    }
    fn log(&self, r: &log::Record) {
        if !self.enabled(r.metadata()) {
            return;
        }
        let line = format!("[{}] {}: {}\n", r.level(), r.target(), r.args());
        match &self.file {
            Some(f) => {
                let _ = f.lock().map(|mut f| f.write_all(line.as_bytes()));
            }
            None => eprint!("{line}"),
        }
    }
    fn flush(&self) {}
}

fn init_log(default_file: Option<std::path::PathBuf>) {
    let level = match std::env::var("CCTERM_LOG").unwrap_or_default().as_str() {
        "error" => log::LevelFilter::Error,
        "info" => log::LevelFilter::Info,
        "debug" => log::LevelFilter::Debug,
        "trace" => log::LevelFilter::Trace,
        "off" => log::LevelFilter::Off,
        _ => log::LevelFilter::Warn,
    };
    let file = std::env::var_os("CCTERM_LOG_FILE")
        .map(std::path::PathBuf::from)
        .or(default_file)
        .and_then(|p| std::fs::OpenOptions::new().create(true).append(true).open(p).ok())
        .map(Mutex::new);
    let logger = Box::leak(Box::new(Logger { level, file }));
    if log::set_logger(logger).is_ok() {
        log::set_max_level(level);
    }
}

/// Report an error the user must see and exit. On Windows the release build
/// has no console, so a message box is shown unless stderr is redirected
/// (scripts, tests), where the log line above already went.
pub fn fatal(msg: &str) -> ! {
    log::error!("{msg}");
    #[cfg(windows)]
    {
        if !std_handle_valid(true) {
            message_box(msg);
        }
    }
    #[cfg(not(windows))]
    {
        eprintln!("ccterm: {msg}");
    }
    std::process::exit(1);
}

/// Print usage, version or a command-line error and exit with `code`. The
/// Windows release build has no console of its own: the text goes to a
/// redirected stdout/stderr, else to the console of the parent (a shell),
/// else into a message box.
fn exit_with(msg: &str, code: i32) -> ! {
    #[cfg(windows)]
    {
        if !windows_console(code != 0) {
            message_box(msg);
            std::process::exit(code);
        }
    }
    if code == 0 {
        println!("{msg}");
    } else {
        eprintln!("{msg}");
    }
    std::process::exit(code);
}

/// Whether stdout (or stderr) leads somewhere: a redirected handle, or the
/// parent's console after attaching to it.
#[cfg(windows)]
fn windows_console(stderr: bool) -> bool {
    #[link(name = "kernel32")]
    extern "system" {
        fn AttachConsole(pid: u32) -> i32;
    }
    const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
    // SAFETY: plain Win32 call without pointer arguments.
    std_handle_valid(stderr) || unsafe { AttachConsole(ATTACH_PARENT_PROCESS) != 0 }
}

/// Whether stderr (or stdout) is a real handle, e.g. redirected to a file.
#[cfg(windows)]
fn std_handle_valid(stderr: bool) -> bool {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetStdHandle(which: u32) -> *mut core::ffi::c_void;
    }
    const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
    const STD_ERROR_HANDLE: u32 = -12i32 as u32;
    // SAFETY: plain Win32 call without pointer arguments.
    let h = unsafe { GetStdHandle(if stderr { STD_ERROR_HANDLE } else { STD_OUTPUT_HANDLE }) };
    !h.is_null() && h as isize != -1
}

#[cfg(windows)]
fn message_box(msg: &str) {
    #[link(name = "user32")]
    extern "system" {
        fn MessageBoxW(hwnd: *mut core::ffi::c_void, text: *const u16, caption: *const u16, kind: u32) -> i32;
    }
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let (text, caption) = (wide(msg), wide("ccterm"));
    // SAFETY: both strings are NUL-terminated UTF-16 buffers that outlive the call.
    unsafe {
        MessageBoxW(std::ptr::null_mut(), text.as_ptr(), caption.as_ptr(), 0x10);
    }
}

fn main() {
    let cmd = match parse_command(std::env::args().skip(1).collect()) {
        Ok(c) => c,
        Err(msg) => {
            init_log(None);
            exit_with(&msg, if msg.starts_with("usage") || msg.starts_with("ccterm ") { 0 } else { 2 })
        }
    };
    let opts = match cmd {
        // No window, fonts or event loop for these (DESIGN.md §3.1).
        Command::Daemon => {
            let dir = ccterm_proto::paths::state_dir();
            let _ = std::fs::create_dir_all(&dir);
            init_log(Some(dir.join("daemon.log")));
            std::process::exit(cli::daemon());
        }
        Command::Status { json } => {
            init_log(None);
            let (text, code) = cli::status(json);
            exit_with(&text, code)
        }
        Command::Screen { id } => {
            init_log(None);
            let (text, code) = cli::screen(&id);
            exit_with(&text, code)
        }
        Command::Gui(o) => {
            init_log(None);
            *o
        }
    };
    let fonts = font::Fonts::load(opts.font.as_deref()).unwrap_or_else(|e| fatal(&e));
    let el =
        EventLoop::<app::UserEvent>::with_user_event().build().unwrap_or_else(|e| fatal(&format!("event loop: {e}")));
    el.set_control_flow(ControlFlow::Wait);
    let mut app = app::App::new(opts, el.create_proxy(), fonts);
    if let Err(e) = el.run_app(&mut app) {
        fatal(&format!("event loop: {e}"));
    }
    std::process::exit(app.exit_code);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &[&str]) -> Result<Options, String> {
        parse_args(s.iter().map(|s| s.to_string()))
    }

    #[test]
    fn parses_options_and_program() {
        let o = args(&["--font", "Iosevka", "--font-size", "13", "--cols", "100", "claude", "--resume"]).unwrap();
        assert_eq!(o.font.as_deref(), Some("Iosevka"));
        assert_eq!(o.font_size, 13.0);
        assert_eq!(o.cols, 100);
        assert_eq!(o.command.program, "claude");
        assert_eq!(o.command.args, vec!["--resume"]);
        let o = args(&["--", "--weird-program", "-x"]).unwrap();
        assert_eq!(o.command.program, "--weird-program");
        assert!(args(&["--font-size", "1000"]).is_err());
        assert!(args(&["--bogus"]).is_err());
        let o = args(&["--window-padding-color=background", "--window-padding-balance", "false", "sh"]).unwrap();
        assert_eq!(o.config.padding_color, config::PaddingColor::Background);
        assert!(!o.config.padding_balance);
        assert_eq!(o.command.program, "sh");
        assert!(args(&["--window-padding-color", "pink"]).is_err());
        assert_eq!(args(&["--decorations", "none"]).unwrap().config.decorations, config::Decorations::None);
        assert!(args(&["--decorations"]).is_err());

        assert!(!args(&[]).unwrap().command.program.is_empty());
    }

    #[test]
    fn implemented_subcommands_come_first() {
        assert!(matches!(parse_command(vec!["daemon".into()]), Ok(Command::Daemon)));
        assert!(matches!(parse_command(vec!["status".into()]), Ok(Command::Status { json: false })));
        assert!(matches!(parse_command(vec!["status".into(), "--json".into()]), Ok(Command::Status { json: true })));
        assert!(parse_command(vec!["status".into(), "--xml".into()]).is_err());
        assert!(parse_command(vec!["daemon".into(), "x".into()]).is_err());
        assert!(
            matches!(parse_command(vec!["status".into(), "--screen".into(), "a-1".into()]), Ok(Command::Screen { id }) if id == "a-1")
        );
        match parse_command(vec!["gui".into(), "--cols".into(), "90".into(), "claude".into()]) {
            Ok(Command::Gui(o)) => assert_eq!((o.cols, o.command.program.to_str()), (90, Some("claude"))),
            other => panic!("{other:?}"),
        }
        for name in IMPLEMENTED {
            let e = args(&["--cols", "80", name]).unwrap_err();
            assert!(e.contains("must come first") && e.contains(&format!("ccterm -- {name}")), "{e}");
            assert_eq!(args(&["--", name]).unwrap().command.program, *name);
        }
    }

    #[test]
    fn reserved_subcommands_are_errors_until_implemented() {
        for name in SUBCOMMANDS.iter().filter(|n| !IMPLEMENTED.contains(n)) {
            let e = args(&[name]).err().unwrap_or_else(|| panic!("{name} must be rejected"));
            assert!(e.contains("not implemented") && e.contains(&format!("ccterm -- {name}")), "{e}");
            // Also after options, and with arguments.
            assert!(args(&["--cols", "80", name, "--json"]).is_err());
            // After `--` the word is a program again.
            let o = args(&["--rows", "10", "--", name, "x"]).unwrap();
            assert_eq!(o.command.program, *name);
            assert_eq!(o.command.args, vec!["x"]);
        }
        // Not reserved: still a program, including near misses and reserved
        // words as arguments of a program.
        for prog in ["claude", "statusx", "Status", "gui2", "./run"] {
            assert_eq!(args(&[prog]).unwrap().command.program, prog);
        }
        let o = args(&["claude", "status"]).unwrap();
        assert_eq!(o.command.program, "claude");
        assert_eq!(o.command.args, vec!["status"]);
        // Not mistaken for usage or version output (those exit 0).
        let e = args(&["hook"]).unwrap_err();
        assert!(!e.starts_with("usage") && !e.starts_with("ccterm "));
    }

    #[test]
    fn widths() {
        assert_eq!(text_width('a'), 1);
        assert_eq!(text_width('한'), 2);
        assert_eq!(text_width('\u{0301}'), 0);
    }
}
