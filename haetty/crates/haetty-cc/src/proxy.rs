//! The `claude` proxy (stage PH, H1; DESIGN.md §3.13): `claude` typed in a shell tab becomes a Claude Code tab.
//!
//! The daemon puts its instance folder `<run dir>/bin` first on a shell tab's `PATH`, holding `claude` (Linux: a link
//! to haetty's executable; Windows: `claude.cmd` calling `haetty claude-proxy`). Started that way, haetty removes the
//! folder from `PATH`, finds the real `claude` and runs it with the same arguments plus the `--settings` a Claude Code
//! tab gets (hooks and the status line: observing only), merged with a `--settings` the user gave. Nothing else of
//! Claude Code's arguments or behaviour changes, except that with `ide-integration` on the real claude also gets the
//! tab's editor port (`CLAUDE_CODE_SSE_PORT`, DESIGN.md §13.9). Without injection (integration off, `-p`, not a terminal, a
//! subcommand that is not a conversation, already proxied) the arguments pass unchanged. Unix: `exec`, so the exit
//! code and signals are claude's own; Windows: the child's exit code.

use std::path::{Path, PathBuf};

use crate::json::Json;
use crate::settings;

/// The proxy folder (`<run dir>/bin`), set by the daemon for a shell tab.
pub const DIR_ENV: &str = "HAETTY_PROXY_DIR";
/// The session's settings file (hooks and status line), set by the daemon only with `claude-integration` on.
pub const SETTINGS_ENV: &str = "HAETTY_CLAUDE_SETTINGS";
/// Set for the real claude: a `claude` started inside it passes unchanged (recursion).
pub const PROXIED_ENV: &str = "HAETTY_PROXIED";
/// The Windows shim's subcommand (`claude.cmd` → `haetty claude-proxy ARGS`).
pub const SUBCOMMAND: &str = "claude-proxy";

/// Claude Code's subcommands that are not a conversation (`claude --help`, 2.1.326).
pub const NOT_CONVERSATION: &[&str] = &[
    "agents",
    "attach",
    "auth",
    "auto-mode",
    "doctor",
    "gateway",
    "import",
    "install",
    "logs",
    "mcp",
    "plugin",
    "plugins",
    "purge",
    "respawn",
    "rm",
    "setup-token",
    "stop",
    "kill",
    "ultrareview",
    "update",
    "upgrade",
];

/// Flags that make a run non-interactive or end it at once.
const PASS_FLAGS: &[&str] = &["-p", "--print", "--bg", "--background", "-v", "--version", "-h", "--help"];

/// Why the arguments pass unchanged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pass {
    /// `claude-integration = false` (the daemon gave no settings file).
    Off,
    /// Already inside a proxied claude.
    Recursion,
    /// Standard output is not a terminal.
    NotTerminal,
    /// `-p`, `--print`, `--bg`, `--version`, `--help`.
    NonInteractive(String),
    /// `claude mcp`, `claude update` …
    Subcommand(String),
}

/// Whether to add haetty's `--settings` to these arguments.
pub fn decide(args: &[String], have_settings: bool, proxied: bool, stdout_tty: bool) -> Result<(), Pass> {
    if !have_settings {
        return Err(Pass::Off);
    }
    if proxied {
        return Err(Pass::Recursion);
    }
    if !stdout_tty {
        return Err(Pass::NotTerminal);
    }
    for a in args {
        if a == "--" {
            break;
        }
        if PASS_FLAGS.contains(&a.as_str()) || a.starts_with("--print=") {
            return Err(Pass::NonInteractive(a.clone()));
        }
    }
    // A subcommand is Claude Code's first word (`claude mcp list`); later words are the prompt or option values.
    if let Some(first) = args.first().filter(|a| NOT_CONVERSATION.contains(&a.as_str())) {
        return Err(Pass::Subcommand(first.clone()));
    }
    Ok(())
}

/// `PATH` without the proxy folder.
pub fn path_without(path: &std::ffi::OsStr, dir: &Path) -> std::ffi::OsString {
    let dir = canon(dir);
    let kept: Vec<PathBuf> = std::env::split_paths(path).filter(|p| canon(p) != dir).collect();
    std::env::join_paths(kept).unwrap_or_default()
}

fn canon(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// The names a `claude` program has on this platform, in the shell's order.
fn names() -> Vec<String> {
    if cfg!(windows) {
        let ext = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
        ext.split(';').filter(|e| !e.is_empty()).map(|e| format!("claude{}", e.to_ascii_lowercase())).collect()
    } else {
        vec!["claude".into()]
    }
}

/// The real `claude` on `path` (the proxy folder already removed): the first file that is not haetty itself.
pub fn find_real(path: &std::ffi::OsStr, own_exe: &Path) -> Option<PathBuf> {
    let own = canon(own_exe);
    for d in std::env::split_paths(path) {
        for n in names() {
            let c = d.join(&n);
            if c.is_file() && canon(&c) != own && executable(&c) {
                return Some(c);
            }
        }
    }
    None
}

#[cfg(unix)]
fn executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn executable(_: &Path) -> bool {
    true
}

/// The arguments for the real claude: the user's `--settings` merged into a file of its own next to the session's,
/// else the session's file.
pub fn with_settings(args: &[String], base: &Path, cwd: Option<&Path>, pid: u32) -> Result<Vec<String>, String> {
    let (mut rest, user) = settings::split_user_settings(args);
    let file = match user {
        None => base.to_path_buf(),
        Some(v) => {
            let ours = std::fs::read_to_string(base).map_err(|e| format!("{}: {e}", base.display()))?;
            let ours = Json::parse(&ours).map_err(|e| format!("{}: {e}", base.display()))?;
            let user = settings::load_user(&v, cwd)?;
            let (merged, warnings) = settings::merge(ours, Some(user));
            for w in warnings {
                eprintln!("haetty: {w}");
            }
            let stem = base.file_stem().and_then(|s| s.to_str()).unwrap_or("settings");
            let p = base.with_file_name(format!("{stem}-p{pid}.json"));
            write_private(&p, merged.to_string().as_bytes()).map_err(|e| format!("{}: {e}", p.display()))?;
            p
        }
    };
    rest.push("--settings".into());
    rest.push(file.to_string_lossy().into_owned());
    Ok(rest)
}

fn write_private(p: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    o.open(p)?.write_all(bytes)
}

/// Runs as `claude` (or `haetty claude-proxy`): never returns.
pub fn main(args: Vec<String>) -> ! {
    use std::io::IsTerminal as _;
    let path = std::env::var_os("PATH").unwrap_or_default();
    let dir = std::env::var_os(DIR_ENV).map(PathBuf::from);
    let path = match &dir {
        Some(d) => path_without(&path, d),
        None => path,
    };
    let own = std::env::current_exe().unwrap_or_default();
    let Some(real) = find_real(&path, &own) else {
        eprintln!("haetty: claude: command not found");
        std::process::exit(127);
    };
    let settings_file = std::env::var_os(SETTINGS_ENV).map(PathBuf::from).filter(|p| p.is_file());
    let proxied = std::env::var_os(PROXIED_ENV).is_some();
    let mut ide_port = None;
    let args = match (&settings_file, decide(&args, settings_file.is_some(), proxied, std::io::stdout().is_terminal()))
    {
        (Some(base), Ok(())) => {
            let cwd = std::env::current_dir().ok();
            match with_settings(&args, base, cwd.as_deref(), std::process::id()) {
                Ok(a) => {
                    report_args(&args);
                    ide_port = std::env::var(crate::ide::PORT_ENV).ok().filter(|p| p.parse::<u16>().is_ok());
                    a
                }
                Err(e) => {
                    eprintln!("haetty: {e}; claude starts without haetty's integration");
                    args
                }
            }
        }
        _ => args,
    };
    let mut cmd = std::process::Command::new(&real);
    cmd.args(&args).env("PATH", &path).env(PROXIED_ENV, "1");
    if let Some(p) = ide_port {
        cmd.env(crate::ide::SSE_PORT_ENV, p);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        let e = cmd.exec();
        eprintln!("haetty: {}: {e}", real.display());
        std::process::exit(126);
    }
    #[cfg(not(unix))]
    match cmd.status() {
        Ok(s) => std::process::exit(s.code().unwrap_or(1)),
        Err(e) => {
            eprintln!("haetty: {}: {e}", real.display());
            std::process::exit(126);
        }
    }
}

/// Tells the tab's daemon which arguments to give again when the conversation is resumed
/// (`crate::launch::kept`). Without a daemon, at once nothing.
fn report_args(args: &[String]) {
    use haetty_proto::wire::{ClientMsg, Secret};
    use haetty_proto::{paths::Endpoint, product, Client, Role};
    let Some(session) = product::env("SESSION_ID").filter(|s| !s.is_empty()) else { return };
    let Ok(ep) = Endpoint::default_for_user() else { return };
    let dial = std::time::Duration::from_millis(50);
    let Ok(c) = Client::connect_with(&ep, Role::Report, "claude-proxy", dial) else { return };
    if c.send(&ClientMsg::ClaudeArgs { session, args: Secret(crate::launch::kept(args)) }).is_ok() {
        let _ = c.sync(std::time::Duration::from_millis(500));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn decides_when_to_inject() {
        assert_eq!(decide(&v(&[]), true, false, true), Ok(()));
        assert_eq!(decide(&v(&["--model", "haiku", "fix the tests"]), true, false, true), Ok(()));
        assert_eq!(decide(&v(&["--resume", "abc"]), true, false, true), Ok(()));
        assert_eq!(decide(&v(&[]), false, false, true), Err(Pass::Off));
        assert_eq!(decide(&v(&[]), true, true, true), Err(Pass::Recursion));
        assert_eq!(decide(&v(&[]), true, false, false), Err(Pass::NotTerminal));
        assert_eq!(decide(&v(&["-p", "hi"]), true, false, true), Err(Pass::NonInteractive("-p".into())));
        assert_eq!(decide(&v(&["x", "--print"]), true, false, true), Err(Pass::NonInteractive("--print".into())));
        assert_eq!(decide(&v(&["--bg"]), true, false, true), Err(Pass::NonInteractive("--bg".into())));
        assert_eq!(decide(&v(&["mcp", "list"]), true, false, true), Err(Pass::Subcommand("mcp".into())));
        assert_eq!(decide(&v(&["update"]), true, false, true), Err(Pass::Subcommand("update".into())));
        // A prompt that starts with a subcommand's word after an option is a conversation.
        assert_eq!(decide(&v(&["--model", "x", "update the docs"]), true, false, true), Ok(()));
        // After `--` everything is the prompt.
        assert_eq!(decide(&v(&["--", "-p"]), true, false, true), Ok(()));
    }

    #[test]
    fn path_loses_only_the_proxy_folder() {
        let p = std::env::join_paths(["/a", "/proxy/bin", "/b", "/proxy/bin"]).unwrap();
        let out = path_without(&p, Path::new("/proxy/bin"));
        assert_eq!(std::env::split_paths(&out).collect::<Vec<_>>(), [PathBuf::from("/a"), PathBuf::from("/b")]);
    }
}
