//! The product name in one place (DESIGN.md §3.9). The name is not final
//! (candidates: haetty, trinitty); renaming means changing this module and
//! the `[[bin]] name` in `crates/ccterm-gui/Cargo.toml`.

/// Executable name without extension; used when the running executable's
/// own path is unknown.
pub const EXE: &str = "ccterm";
/// Application id: Wayland `app_id`, X11 `WM_CLASS`, `.desktop` file name,
/// Windows AppUserModelID.
pub const APP_ID: &str = "ccterm";
/// Human-readable name (window title prefix, desktop entry name).
pub const DISPLAY_NAME: &str = "ccterm";
/// Directory name under the runtime, state and config directories, and the
/// prefix of the Windows pipe name.
pub const DIR: &str = "ccterm";
/// Environment variable set in every session's child: the session id, so
/// `ccterm hook` / `ccterm statusline` know which tab they belong to.
pub const SESSION_ENV: &str = "CCTERM_SESSION_ID";

/// Path of the running executable, for spawning the daemon and writing hook
/// commands; falls back to the bare name (found on PATH).
pub fn exe_path() -> std::path::PathBuf {
    std::env::current_exe().unwrap_or_else(|_| EXE.into())
}
