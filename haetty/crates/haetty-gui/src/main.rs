//! `haetty`: terminal windows whose sessions live in the haetty daemon.
//!
//! ```text
//! haetty [gui] [--font FAMILY] [--font-size PT] [--cols N] [--rows N] [--KEY VALUE]... [--] [PROGRAM [ARGS...]]
//! haetty daemon
//! haetty status [--json | --screen ID]
//! haetty install-desktop
//! haetty hook EVENT        (run by Claude Code)
//! haetty statusline        (run by Claude Code)
//! ```
//! Without a program it runs the user's shell (`$SHELL`, else `/bin/sh`;
//! Windows: `%COMSPEC%`, else `cmd.exe`). The first word after the options is
//! the program (`haetty claude`), except the subcommand names in
//! [`SUBCOMMANDS`]; a program with one of those names goes after `--`
//! (`haetty -- status`).
//!
//! Environment: `HAETTY_LOG=off|error|warn|info|debug|trace` (default warn),
//! `HAETTY_INSTANCE=NAME` (a separate instance: own daemon, directories,
//! app id; DESIGN.md §3.9),
//! `HAETTY_LOG_FILE=path` (log there instead of stderr; on Windows the
//! release build has no console), `HAETTY_DUMP_FRAME=path.ppm` (write every
//! frame as an image, for debugging), `HAETTY_FONT_DIR=dir` (extra fonts),
//! `HAETTY_DEBUG_PADDING=1` (paint everything outside the grid red).
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
mod diffview;
mod emoji;
mod font;
mod geom;
mod gitview;
mod help;
mod icon;
mod input;
mod install;
mod keymap;
mod links;
mod migrate;
#[cfg(test)]
mod reftest;
mod render;
mod run;
mod tabsets;
mod tasks;
mod titlebar;
mod tokens;
mod ui;

use std::io::Write;
use std::sync::Mutex;

use haetty_core::CommandSpec;
use winit::event_loop::{ControlFlow, EventLoop};

#[derive(Clone, Debug)]
pub struct Options {
    pub font: Option<String>,
    pub font_size: f64,
    pub cols: u16,
    pub rows: u16,
    pub command: CommandSpec,
    /// A program was named on the command line (else the default shell).
    pub explicit_program: bool,
    /// Shown over the bottom row at start (migration from ccterm).
    pub first_hint: Option<String>,
    pub config: config::Config,
    /// The colours: `config.theme` with the colour keys on top.
    pub theme: haetty_core::theme::Theme,
    /// Settings given on the command line, applied after the config file.
    pub settings: Vec<(String, String)>,
    pub keymap: keymap::Keymap,
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
pub const SUBCOMMANDS: &[&str] = &[
    "gui",
    "daemon",
    "hook",
    "statusline",
    "run",
    "headless",
    "install-desktop",
    "status",
    "events",
    "log",
    "screen",
    "tokens",
    "completions",
];
pub const IMPLEMENTED: &[&str] = &[
    "gui",
    "daemon",
    "status",
    "install-desktop",
    "hook",
    "statusline",
    "events",
    "screen",
    "tokens",
    "run",
    "log",
    "completions",
];

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
    InstallDesktop,
    /// `events [--follow] [--json] [--since CURSOR]`: the event log of
    /// observable sessions (DESIGN.md §6.1).
    Events {
        follow: bool,
        json: bool,
        since: u64,
    },
    /// `screen [--json] SESSION`: an observable session's screen and state.
    Observe {
        id: String,
        json: bool,
    },
    /// `run [--summary-lines N] -- COMMAND [ARGS...]`: a command run quietly, its output in a run log (`run.rs`).
    Run {
        summary_lines: usize,
        argv: Vec<String>,
    },
    /// `log [FILE] [--grep TEXT] [--lines A:B] [--tail N]`: a run log in parts, or the saved runs (`run.rs`).
    Log {
        file: Option<std::path::PathBuf>,
        grep: Option<String>,
        lines: Option<(usize, usize)>,
        tail: Option<usize>,
    },
    /// `tokens [--top N] [PATH...]`: the token use of Claude Code transcripts (`haetty_cc::transcript`).
    Tokens {
        top: usize,
        paths: Vec<std::path::PathBuf>,
    },
    /// `completions SHELL`: a completion script (`help.rs`).
    Completions {
        shell: String,
    },
}

fn parse_command(args: Vec<String>) -> Result<Command, String> {
    // `haetty SUBCOMMAND --help` (the same texts as `haetty --help`).
    if let (Some(sub), Some(h)) = (args.first(), args.get(1)) {
        if (h == "--help" || h == "-h") && IMPLEMENTED.contains(&sub.as_str()) {
            return Err(help::sub_help(sub).unwrap_or_else(help::main_help));
        }
    }
    match args.first().map(String::as_str) {
        Some("daemon") => match args.len() {
            1 => Ok(Command::Daemon),
            _ => Err("usage: haetty daemon".into()),
        },
        Some("status") => match &args[1..] {
            [] => Ok(Command::Status { json: false }),
            [j] if j == "--json" => Ok(Command::Status { json: true }),
            [s, id] if s == "--screen" => Ok(Command::Screen { id: id.clone() }),
            _ => Err("usage: haetty status [--json | --screen ID]".into()),
        },
        Some("events") => {
            let (mut follow, mut json, mut since) = (false, false, 0);
            let mut it = args[1..].iter();
            while let Some(a) = it.next() {
                match a.as_str() {
                    "--follow" | "-f" => follow = true,
                    "--json" => json = true,
                    "--since" => {
                        since = it.next().and_then(|v| v.parse().ok()).ok_or("--since needs a number (a seq)")?;
                    }
                    _ => return Err("usage: haetty events [--follow] [--json] [--since CURSOR]".into()),
                }
            }
            Ok(Command::Events { follow, json, since })
        }
        Some("screen") => match &args[1..] {
            [id] if !id.starts_with('-') => Ok(Command::Observe { id: id.clone(), json: false }),
            [j, id] | [id, j] if j == "--json" && !id.starts_with('-') => {
                Ok(Command::Observe { id: id.clone(), json: true })
            }
            _ => Err("usage: haetty screen [--json] SESSION".into()),
        },
        Some("run") => {
            let mut summary = run::DEFAULT_SUMMARY_LINES;
            let mut i = 1;
            while i < args.len() && args[i] != "--" {
                match args[i].as_str() {
                    "--summary-lines" => {
                        summary = args
                            .get(i + 1)
                            .and_then(|v| v.parse::<usize>().ok())
                            .ok_or("--summary-lines needs a number")?;
                        i += 2;
                    }
                    _ => return Err(format!("error: {}", run::RUN_USAGE)),
                }
            }
            if args.get(i).map(String::as_str) != Some("--") || i + 1 >= args.len() {
                return Err(format!("error: {}", run::RUN_USAGE));
            }
            Ok(Command::Run { summary_lines: summary, argv: args[i + 1..].to_vec() })
        }
        Some("log") => {
            let (mut file, mut grep, mut lines, mut tail) = (None, None, None, None);
            let mut it = args[1..].iter();
            while let Some(a) = it.next() {
                match a.as_str() {
                    "--grep" => grep = Some(it.next().ok_or("--grep needs text")?.clone()),
                    "--lines" => {
                        let v = it.next().ok_or("--lines needs A:B")?;
                        let (x, y) = v.split_once(':').ok_or("--lines needs A:B")?;
                        let x: usize = x.parse().map_err(|_| "--lines needs A:B")?;
                        let y: usize = y.parse().map_err(|_| "--lines needs A:B")?;
                        lines = Some((x, y));
                    }
                    "--tail" => {
                        tail = Some(it.next().and_then(|v| v.parse::<usize>().ok()).ok_or("--tail needs a number")?);
                    }
                    p if !p.starts_with('-') && file.is_none() => file = Some(std::path::PathBuf::from(p)),
                    _ => return Err(format!("error: {}", run::LOG_USAGE)),
                }
            }
            Ok(Command::Log { file, grep, lines, tail })
        }
        Some("tokens") => {
            let mut top = 10usize;
            let mut paths = Vec::new();
            let mut it = args[1..].iter();
            while let Some(a) = it.next() {
                match a.as_str() {
                    "--top" => {
                        top = it.next().and_then(|v| v.parse().ok()).ok_or("--top needs a number")?;
                    }
                    p if !p.starts_with('-') => paths.push(std::path::PathBuf::from(p)),
                    _ => return Err("usage: haetty tokens [--top N] [PATH...]".into()),
                }
            }
            Ok(Command::Tokens { top, paths })
        }
        Some("completions") => match &args[1..] {
            [sh] if help::SHELLS.contains(&sh.as_str()) => Ok(Command::Completions { shell: sh.clone() }),
            _ => Err(format!("usage: haetty completions {}", help::SHELLS.join("|"))),
        },
        Some("install-desktop") => match args.len() {
            1 => Ok(Command::InstallDesktop),
            _ => Err("usage: haetty install-desktop".into()),
        },
        Some("gui") => parse_args(args.into_iter().skip(1)).map(|o| Command::Gui(Box::new(o))),
        _ => parse_args(args).map(|o| Command::Gui(Box::new(o))),
    }
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Options, String> {
    let mut o = Options {
        font: None,
        font_size: 11.0,
        cols: 120,
        rows: 36,
        command: default_shell(),
        explicit_program: false,
        first_hint: None,
        config: config::Config::default(),
        theme: haetty_core::theme::Theme::default(),
        settings: vec![],
        keymap: keymap::Keymap::default(),
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
            "-h" | "--help" => return Err(help::main_help()),
            "--version" | "-V" => return Err(version_line()),
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
                    return Err(format!("unknown option {s} (haetty --help lists them)"));
                }
                let value = match inline {
                    Some(v) => v,
                    None => it.next().ok_or(format!("--{key} needs a value"))?,
                };
                o.config.set(key, &value)?;
                o.settings.push((key.to_owned(), value));
            }
            s if IMPLEMENTED.contains(&s) => {
                return Err(format!(
                    "haetty: '{s}' is a subcommand and must come first (haetty {s} ...); \
                     to run a program named {s}, use: haetty -- {s}"
                ));
            }
            s if SUBCOMMANDS.contains(&s) => {
                return Err(format!(
                    "haetty: '{s}' is a subcommand that is not implemented yet; \
                     to run a program named {s}, use: haetty -- {s}"
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
        o.explicit_program = true;
    }
    o.cols = o.cols.max(2);
    o.rows = o.rows.max(1);
    Ok(o)
}

/// The config file (`config.rs`), then the command line's settings on top.
fn load_config_file(o: &mut Options) {
    let path = haetty_proto::paths::config_dir().join("config");
    let Ok(text) = std::fs::read_to_string(&path) else { return };
    let mut c = config::Config::default();
    for bad in c.apply_file(&text, &mut o.keymap) {
        log::warn!("{}: {bad} (ignored)", path.display());
    }
    for (k, v) in &o.settings {
        let _ = c.set(k, v);
    }
    log::info!("config: {}", path.display());
    o.config = c;
}

/// `haetty --version`: version, commit, build date (UTC) and the instance
/// this process would run as (`HAETTY_INSTANCE`; none = "stable").
pub fn version_line() -> String {
    let inst = haetty_proto::product::instance();
    format!(
        "haetty {} (commit {}, built {}, instance {})",
        env!("CARGO_PKG_VERSION"),
        env!("HAETTY_BUILD_COMMIT"),
        env!("HAETTY_BUILD_DATE"),
        if inst.is_empty() { "stable" } else { inst.as_str() }
    )
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
        m.level() <= self.level && (m.target().starts_with("haetty") || m.level() <= log::Level::Warn)
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
    let level = match haetty_proto::product::env("LOG").unwrap_or_default().as_str() {
        "error" => log::LevelFilter::Error,
        "info" => log::LevelFilter::Info,
        "debug" => log::LevelFilter::Debug,
        "trace" => log::LevelFilter::Trace,
        "off" => log::LevelFilter::Off,
        _ => log::LevelFilter::Warn,
    };
    let file = haetty_proto::product::env_os("LOG_FILE")
        .map(std::path::PathBuf::from)
        .or(default_file)
        .and_then(|p| std::fs::OpenOptions::new().create(true).append(true).open(p).ok())
        .map(Mutex::new);
    let logger = Box::leak(Box::new(Logger { level, file }));
    if log::set_logger(logger).is_ok() {
        log::set_max_level(level);
    }
    haetty_proto::product::log_legacy_env();
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
        eprintln!("haetty: {msg}");
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

/// Windows: the AppUserModelID (`io.github.dhwan0112.haetty`, a dev
/// instance `….haetty.dev`), so the taskbar groups haetty windows and keeps
/// a pinned icon whatever path the executable runs from. Set before any
/// window exists.
#[cfg(windows)]
fn set_app_user_model_id() {
    #[link(name = "shell32")]
    extern "system" {
        fn SetCurrentProcessExplicitAppUserModelID(id: *const u16) -> i32;
        fn GetCurrentProcessExplicitAppUserModelID(id: *mut *mut u16) -> i32;
    }
    #[link(name = "ole32")]
    extern "system" {
        fn CoTaskMemFree(p: *mut core::ffi::c_void);
    }
    let id = haetty_proto::product::app_id();
    let wide: Vec<u16> = id.encode_utf16().chain(Some(0)).collect();
    // SAFETY: `wide` is NUL-terminated and outlives the call.
    let hr = unsafe { SetCurrentProcessExplicitAppUserModelID(wide.as_ptr()) };
    if hr < 0 {
        log::warn!("AppUserModelID {id}: HRESULT {hr:#x}");
        return;
    }
    let mut out: *mut u16 = std::ptr::null_mut();
    // SAFETY: on success `out` is a NUL-terminated string we free below.
    if unsafe { GetCurrentProcessExplicitAppUserModelID(&mut out) } >= 0 && !out.is_null() {
        let mut n = 0;
        // SAFETY: reading up to the NUL of a string Windows allocated.
        while unsafe { *out.add(n) } != 0 {
            n += 1;
        }
        // SAFETY: `n` u16s were just read.
        let got = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(out, n) });
        unsafe { CoTaskMemFree(out.cast()) };
        log::info!("AppUserModelID {got}");
    }
}

#[cfg(windows)]
fn message_box(msg: &str) {
    #[link(name = "user32")]
    extern "system" {
        fn MessageBoxW(hwnd: *mut core::ffi::c_void, text: *const u16, caption: *const u16, kind: u32) -> i32;
    }
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let (text, caption) = (wide(msg), wide("haetty"));
    // SAFETY: both strings are NUL-terminated UTF-16 buffers that outlive the call.
    unsafe {
        MessageBoxW(std::ptr::null_mut(), text.as_ptr(), caption.as_ptr(), 0x10);
    }
}

fn main() {
    // Claude Code's hooks and status line: first, before any setup (they
    // must end within milliseconds and never print; DESIGN.md §3.13).
    let mut argv = std::env::args().skip(1);
    let help = std::env::args().nth(2).is_some_and(|a| a == "--help" || a == "-h");
    match argv.next().as_deref() {
        Some(s @ ("hook" | "statusline")) if help => {
            init_log(None);
            exit_with(&help::sub_help(s).unwrap_or_default(), 0)
        }
        Some("hook") => haetty_cc::report::main(haetty_cc::report::Kind::Hook(argv.next().unwrap_or_default())),
        Some("statusline") => haetty_cc::report::main(haetty_cc::report::Kind::StatusLine),
        _ => {}
    }
    let cmd = match parse_command(std::env::args().skip(1).collect()) {
        Ok(c) => c,
        Err(msg) => {
            init_log(None);
            exit_with(&msg, if msg.starts_with("usage") || msg.starts_with("haetty ") { 0 } else { 2 })
        }
    };
    let opts = match cmd {
        // No window, fonts or event loop for these (DESIGN.md §3.1).
        Command::Daemon => {
            let dir = haetty_proto::paths::state_dir();
            let _ = std::fs::create_dir_all(&dir);
            init_log(Some(dir.join("daemon.log")));
            migrate::run();
            std::process::exit(cli::daemon());
        }
        Command::Status { json } => {
            init_log(None);
            let (text, code) = cli::status(json);
            exit_with(&text, code)
        }
        Command::InstallDesktop => {
            init_log(None);
            match install::install_desktop() {
                Ok(msg) => exit_with(&msg, 0),
                Err(e) => exit_with(&format!("haetty install-desktop: {e}"), 1),
            }
        }
        Command::Screen { id } => {
            init_log(None);
            let (text, code) = cli::screen(&id);
            exit_with(&text, code)
        }
        Command::Run { summary_lines, argv } => {
            init_log(None);
            let (text, code) = run::run(summary_lines, &argv);
            exit_with(&text, code)
        }
        Command::Log { file, grep, lines, tail } => {
            init_log(None);
            let (text, code) = run::log(file.as_deref(), grep.as_deref(), lines, tail);
            exit_with(&text, code)
        }
        Command::Tokens { top, paths } => {
            init_log(None);
            let (text, code) = cli::tokens(top, &paths);
            exit_with(&text, code)
        }
        Command::Completions { shell } => {
            init_log(None);
            exit_with(help::completions(&shell).unwrap_or_default().trim_end(), 0)
        }
        Command::Observe { id, json } => {
            init_log(None);
            let (text, code) = cli::observe_screen(&id, json);
            exit_with(&text, code)
        }
        Command::Events { follow, json, since } => {
            init_log(None);
            #[cfg(windows)]
            windows_console(false);
            let mut out = std::io::stdout().lock();
            match cli::events(follow, json, since, &mut out) {
                Ok(()) => std::process::exit(0),
                Err(msg) => exit_with(&msg, 1),
            }
        }
        Command::Gui(o) => {
            init_log(None);
            #[cfg(windows)]
            set_app_user_model_id();
            let mut o = *o;
            o.first_hint = migrate::run().hint();
            load_config_file(&mut o);
            let (theme, msgs) = o.config.resolve_theme(&haetty_proto::paths::config_dir());
            for m in msgs {
                log::warn!("{m} (ignored)");
            }
            log::info!("theme: {}", o.config.theme);
            o.theme = theme;
            o
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
        // A bad `run` or `log` line is an error (exit 2), not usage text (exit 0).
        assert!(parse_command(vec!["run".into(), "x".into()]).unwrap_err().starts_with("error: usage: haetty run"));
        assert!(parse_command(vec!["log".into(), "--bogus".into()])
            .unwrap_err()
            .starts_with("error: usage: haetty log"));
        // --version: printed on stdout with exit 0 (the "haetty " prefix).
        let v = parse_command(vec!["--version".into()]).unwrap_err();
        assert!(
            v.starts_with("haetty 0.")
                && v.contains("(commit ")
                && v.contains(", built 20")
                && v.contains(", instance "),
            "{v}"
        );
        let c = env!("HAETTY_BUILD_COMMIT");
        assert!(c == "unknown" || (c.len() == 12 && c.chars().all(|x| x.is_ascii_hexdigit())), "{c}");
        assert!(matches!(parse_command(vec!["status".into(), "--json".into()]), Ok(Command::Status { json: true })));
        assert!(parse_command(vec!["status".into(), "--xml".into()]).is_err());
        let v = |s: &[&str]| parse_command(s.iter().map(|s| s.to_string()).collect());
        assert!(matches!(v(&["events"]), Ok(Command::Events { follow: false, json: false, since: 0 })));
        assert!(matches!(
            v(&["events", "--json", "--follow", "--since", "42"]),
            Ok(Command::Events { follow: true, json: true, since: 42 })
        ));
        assert!(v(&["events", "--since", "x"]).is_err() && v(&["events", "--bogus"]).is_err());
        assert!(matches!(v(&["screen", "--json", "a-1"]), Ok(Command::Observe { id, json: true }) if id == "a-1"));
        assert!(matches!(v(&["screen", "a-1"]), Ok(Command::Observe { id, json: false }) if id == "a-1"));
        assert!(v(&["screen"]).is_err() && v(&["screen", "--json"]).is_err());
        assert!(matches!(v(&["completions", "fish"]), Ok(Command::Completions { shell }) if shell == "fish"));
        assert!(v(&["completions", "tcsh"]).is_err());
        for sub in IMPLEMENTED {
            let h = v(&[sub, "--help"]).unwrap_err();
            assert!(h.starts_with("usage: "), "{sub} --help: {h}");
        }
        assert!(v(&["--help"]).unwrap_err().starts_with("usage: haetty"));
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
            assert!(
                e.starts_with("haetty: ") && e.contains("must come first") && e.contains(&format!("haetty -- {name}")),
                "{e}"
            );
            assert_eq!(args(&["--", name]).unwrap().command.program, *name);
        }
    }

    #[test]
    fn reserved_subcommands_are_errors_until_implemented() {
        for name in SUBCOMMANDS.iter().filter(|n| !IMPLEMENTED.contains(n)) {
            let e = args(&[name]).err().unwrap_or_else(|| panic!("{name} must be rejected"));
            assert!(
                e.starts_with("haetty: ") && e.contains("not implemented") && e.contains(&format!("haetty -- {name}")),
                "{e}"
            );
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
        assert!(!e.starts_with("usage") && !e.starts_with("haetty "));
    }

    #[test]
    fn widths() {
        assert_eq!(text_width('a'), 1);
        assert_eq!(text_width('한'), 2);
        assert_eq!(text_width('\u{0301}'), 0);
    }
}
