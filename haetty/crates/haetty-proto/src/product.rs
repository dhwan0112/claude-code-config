//! The product name in one place (DESIGN.md §3.9): **haetty** (formerly
//! the working name `ccterm`). Besides this module, a rename touches the
//! `[[bin]]` names in the Cargo manifests (Cargo needs literals) and the two
//! strings that programs in a session see (`TERM_PROGRAM`, XTVERSION in
//! `haetty-core`, which sits below this crate).
//!
//! Next to it, the **instance** (DESIGN.md §3.9): `HAETTY_INSTANCE=dev` gives
//! a separate socket/pipe, runtime, state and config directories, app id and
//! window title prefix, so a development build runs beside the stable one
//! without touching its daemon, sessions, tab sets or settings. Unset or
//! empty = the stable instance.
//!
//! Environment variables are `HAETTY_*`. The old `CCTERM_*` names were read as
//! a fallback until K3b and are not any more; the file migration of the former
//! name stays ([`legacy_dir`]).

use std::ffi::OsString;

/// Executable name without extension; used when the running executable's
/// own path is unknown.
pub const EXE: &str = "haetty";
/// Application id: Wayland `app_id`, X11 `WM_CLASS` (instance and class),
/// `.desktop` file name, hicolor icon name (per instance: [`app_id`]).
pub const APP_ID: &str = "io.github.dhwan0112.haetty";
/// Human-readable name (window title, desktop entry name; per instance:
/// [`display_name`]).
pub const DISPLAY_NAME: &str = "haetty";
/// Directory name under the runtime, state and config directories, and the
/// prefix of the Windows pipe name (per instance: [`dir`]).
pub const DIR: &str = "haetty";
/// The former name's directory (migration, an old daemon still running).
pub const LEGACY_DIR: &str = "ccterm";
/// Prefix of every environment variable.
pub const ENV_PREFIX: &str = "HAETTY_";
/// Environment variable set in every session's child: the session id, so
/// `haetty hook` / `haetty statusline` know which tab they belong to.
pub const SESSION_ENV: &str = "HAETTY_SESSION_ID";
/// Environment variable naming the instance; the daemon sets it in every
/// session's child, so hooks reach the daemon of their own instance.
pub const INSTANCE_ENV: &str = "HAETTY_INSTANCE";

/// `HAETTY_<name>` (an empty value counts as set).
pub fn env_os(name: &str) -> Option<OsString> {
    std::env::var_os(format!("{ENV_PREFIX}{name}"))
}

/// [`env_os`] as a string (non-UTF-8 values count as unset).
pub fn env(name: &str) -> Option<String> {
    env_os(name).and_then(|v| v.into_string().ok())
}

/// The instance name: `HAETTY_INSTANCE`, lower-cased, ASCII letters, digits
/// and `-` only, at most 16 characters; "" is the stable instance.
pub fn instance() -> String {
    let raw = env("INSTANCE").unwrap_or_default();
    raw.to_ascii_lowercase().chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').take(16).collect()
}

fn with_instance(base: &str) -> String {
    match instance() {
        i if i.is_empty() => base.to_string(),
        i => format!("{base}-{i}"),
    }
}

/// Directory and pipe-name component: `haetty`, `haetty-dev`.
pub fn dir() -> String {
    with_instance(DIR)
}

/// The same for the former name: `ccterm`, `ccterm-dev`.
pub fn legacy_dir() -> String {
    with_instance(LEGACY_DIR)
}

/// Application id: `io.github.dhwan0112.haetty`, `io.github.dhwan0112.haetty.dev`
/// (a further reverse-DNS component, so the instance has its own desktop
/// file and dock icon).
pub fn app_id() -> String {
    match instance() {
        i if i.is_empty() => APP_ID.to_string(),
        i => format!("{APP_ID}.{i}"),
    }
}

/// Name shown to people: `haetty`, `haetty (dev)`.
pub fn display_name() -> String {
    match instance() {
        i if i.is_empty() => DISPLAY_NAME.to_string(),
        i => format!("{DISPLAY_NAME} ({i})"),
    }
}

/// Prefix of window titles: "", "[dev] ".
pub fn title_prefix() -> String {
    match instance() {
        i if i.is_empty() => String::new(),
        i => format!("[{i}] "),
    }
}

/// Path of the running executable, for spawning the daemon and writing hook
/// commands; falls back to the bare name (found on PATH).
pub fn exe_path() -> std::path::PathBuf {
    std::env::current_exe().unwrap_or_else(|_| EXE.into())
}
