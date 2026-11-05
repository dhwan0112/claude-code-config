//! The product name in one place (DESIGN.md §3.9). The name is not final
//! (candidates: haetty, trinitty); renaming means changing this module and
//! the `[[bin]] name` in `crates/ccterm-gui/Cargo.toml`.
//!
//! Next to it, the **instance** (DESIGN.md §3.9): `CCTERM_INSTANCE=dev` gives
//! a separate socket/pipe, runtime, state and config directories, app id and
//! window title prefix, so a development build runs beside the stable one
//! without touching its daemon, sessions, tab sets or settings. Unset or
//! empty = the stable instance.

/// Executable name without extension; used when the running executable's
/// own path is unknown.
pub const EXE: &str = "ccterm";
/// Application id: Wayland `app_id`, X11 `WM_CLASS`, `.desktop` file name,
/// Windows AppUserModelID (per instance: [`app_id`]).
pub const APP_ID: &str = "ccterm";
/// Human-readable name (window title, desktop entry name; per instance:
/// [`display_name`]).
pub const DISPLAY_NAME: &str = "ccterm";
/// Directory name under the runtime, state and config directories, and the
/// prefix of the Windows pipe name (per instance: [`dir`]).
pub const DIR: &str = "ccterm";
/// Environment variable set in every session's child: the session id, so
/// `ccterm hook` / `ccterm statusline` know which tab they belong to.
pub const SESSION_ENV: &str = "CCTERM_SESSION_ID";
/// Environment variable naming the instance; the daemon sets it in every
/// session's child, so hooks reach the daemon of their own instance.
pub const INSTANCE_ENV: &str = "CCTERM_INSTANCE";

/// The instance name: `CCTERM_INSTANCE`, lower-cased, ASCII letters, digits
/// and `-` only, at most 16 characters; "" is the stable instance.
pub fn instance() -> String {
    let raw = std::env::var(INSTANCE_ENV).unwrap_or_default();
    raw.to_ascii_lowercase().chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').take(16).collect()
}

fn with_instance(base: &str) -> String {
    match instance() {
        i if i.is_empty() => base.to_string(),
        i => format!("{base}-{i}"),
    }
}

/// Directory and pipe-name component: `ccterm`, `ccterm-dev`.
pub fn dir() -> String {
    with_instance(DIR)
}

/// Application id: `ccterm`, `ccterm-dev`.
pub fn app_id() -> String {
    with_instance(APP_ID)
}

/// Name shown to people: `ccterm`, `ccterm (dev)`.
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
