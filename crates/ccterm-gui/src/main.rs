//! `ccterm`: a terminal window for one ccterm-core session.
//!
//! ```text
//! ccterm [--font FAMILY] [--font-size PT] [--cols N] [--rows N] [--] [PROGRAM [ARGS...]]
//! ```
//! Without a program it runs the user's shell (`$SHELL`, else `/bin/sh`;
//! Windows: `%COMSPEC%`, else `cmd.exe`).
//!
//! Environment: `CCTERM_LOG=error|warn|info|debug` (default warn),
//! `CCTERM_LOG_FILE=path` (log there instead of stderr; on Windows the
//! release build has no console), `CCTERM_DUMP_FRAME=path.ppm` (write every
//! frame as an image, for debugging), `CCTERM_FONT_DIR=dir` (extra fonts).
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod damage;
mod font;
mod geom;
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
}

fn default_shell() -> CommandSpec {
    #[cfg(windows)]
    let prog = std::env::var_os("COMSPEC").unwrap_or_else(|| "cmd.exe".into());
    #[cfg(not(windows))]
    let prog = std::env::var_os("SHELL").filter(|s| !s.is_empty()).unwrap_or_else(|| "/bin/sh".into());
    CommandSpec::new(prog)
}

const USAGE: &str = "usage: ccterm [--font FAMILY] [--font-size PT] [--cols N] [--rows N] [--] [PROGRAM [ARGS...]]";

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Options, String> {
    let mut o = Options { font: None, font_size: 11.0, cols: 120, rows: 36, command: default_shell() };
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
            s if s.starts_with("--") => return Err(format!("unknown option {s}\n{USAGE}")),
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

fn init_log() {
    let level = match std::env::var("CCTERM_LOG").unwrap_or_default().as_str() {
        "error" => log::LevelFilter::Error,
        "info" => log::LevelFilter::Info,
        "debug" => log::LevelFilter::Debug,
        "trace" => log::LevelFilter::Trace,
        "off" => log::LevelFilter::Off,
        _ => log::LevelFilter::Warn,
    };
    let file = std::env::var_os("CCTERM_LOG_FILE")
        .and_then(|p| std::fs::OpenOptions::new().create(true).append(true).open(p).ok())
        .map(Mutex::new);
    let logger = Box::leak(Box::new(Logger { level, file }));
    if log::set_logger(logger).is_ok() {
        log::set_max_level(level);
    }
}

/// Report an error the user must see and exit. On Windows the release build
/// has no console, so a message box is shown.
pub fn fatal(msg: &str) -> ! {
    log::error!("{msg}");
    #[cfg(windows)]
    {
        message_box(msg);
    }
    #[cfg(not(windows))]
    {
        eprintln!("ccterm: {msg}");
    }
    std::process::exit(1);
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
    init_log();
    let opts = match parse_args(std::env::args().skip(1)) {
        Ok(o) => o,
        Err(msg) => {
            println!("{msg}");
            std::process::exit(if msg.starts_with("usage") || msg.starts_with("ccterm ") { 0 } else { 2 });
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
        assert!(!args(&[]).unwrap().command.program.is_empty());
    }

    #[test]
    fn widths() {
        assert_eq!(text_width('a'), 1);
        assert_eq!(text_width('한'), 2);
        assert_eq!(text_width('\u{0301}'), 0);
    }
}
