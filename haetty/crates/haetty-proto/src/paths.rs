//! Where the daemon listens and where state and configuration live
//! (DESIGN.md §3.5, §3.8).

use std::io;
use std::path::PathBuf;

use crate::product;

/// A daemon endpoint: a Unix socket path, or a Windows pipe name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint(pub PathBuf);

impl Endpoint {
    /// The default endpoint for this user.
    pub fn default_for_user() -> io::Result<Endpoint> {
        if let Some(p) = product::env_os("SOCKET") {
            return Ok(Endpoint(p.into()));
        }
        #[cfg(unix)]
        {
            Ok(Endpoint(runtime_dir()?.join("daemon.sock")))
        }
        #[cfg(windows)]
        {
            let sid = crate::transport::current_user_sid()?;
            Ok(Endpoint(format!(r"\\.\pipe\{}-{}", product::dir(), sid).into()))
        }
    }
}

/// Linux: `$XDG_RUNTIME_DIR/<app>`, else `/tmp/<app>-<uid>`; created 0700 and
/// checked (real directory, owned by us, no group/other permissions).
#[cfg(unix)]
pub fn runtime_dir() -> io::Result<PathBuf> {
    let base = std::env::var_os("XDG_RUNTIME_DIR").filter(|v| !v.is_empty());
    // SAFETY: getuid has no preconditions.
    let uid = unsafe { libc::getuid() };
    let dir = match base {
        Some(b) => PathBuf::from(b).join(product::dir()),
        None => std::env::temp_dir().join(format!("{}-{uid}", product::dir())),
    };
    ensure_private_dir(&dir)?;
    Ok(dir)
}

/// Creates `dir` (mode 0700) or checks an existing one: it must be a real
/// directory owned by the current user without group or other permissions.
#[cfg(unix)]
pub fn ensure_private_dir(dir: &std::path::Path) -> io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt};
    match std::fs::DirBuilder::new().mode(0o700).recursive(false).create(dir) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            if let Some(parent) = dir.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::DirBuilder::new().mode(0o700).create(dir)?;
        }
        Err(e) => return Err(e),
    }
    let md = std::fs::symlink_metadata(dir)?;
    // SAFETY: getuid has no preconditions.
    let uid = unsafe { libc::getuid() };
    if !md.is_dir() || md.uid() != uid || md.mode() & 0o077 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("{}: not a private directory of this user (owner, mode or symlink)", dir.display()),
        ));
    }
    Ok(())
}

/// A directory only this user can read, for per-session files (the
/// settings file of a Claude Code tab): Linux the runtime directory,
/// Windows `<state dir>\run` (under the user's profile).
pub fn private_dir() -> io::Result<PathBuf> {
    #[cfg(unix)]
    {
        runtime_dir()
    }
    #[cfg(windows)]
    {
        let d = state_dir().join("run");
        std::fs::create_dir_all(&d)?;
        Ok(d)
    }
}

/// The state directory base (without the product directory).
fn state_base() -> PathBuf {
    #[cfg(windows)]
    {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir)
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("XDG_STATE_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
            .unwrap_or_else(std::env::temp_dir)
    }
}

/// The config directory base (without the product directory).
fn config_base() -> PathBuf {
    #[cfg(windows)]
    {
        std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir)
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .unwrap_or_else(std::env::temp_dir)
    }
}

/// The former name's state directory (`~/.local/state/ccterm`), for the
/// migration; `None` when the state directory is overridden.
pub fn legacy_state_dir() -> Option<PathBuf> {
    product::env_os("STATE_DIR").is_none().then(|| state_base().join(product::legacy_dir()))
}

/// The former name's config directory; `None` when overridden.
pub fn legacy_config_dir() -> Option<PathBuf> {
    product::env_os("CONFIG_DIR").is_none().then(|| config_base().join(product::legacy_dir()))
}

/// Where a daemon of the former name would listen (never created here);
/// `None` when the socket is overridden.
pub fn legacy_endpoint() -> Option<Endpoint> {
    if product::env_os("SOCKET").is_some() {
        return None;
    }
    #[cfg(unix)]
    {
        let base = std::env::var_os("XDG_RUNTIME_DIR").filter(|v| !v.is_empty());
        // SAFETY: getuid has no preconditions.
        let uid = unsafe { libc::getuid() };
        let dir = match base {
            Some(b) => PathBuf::from(b).join(product::legacy_dir()),
            None => std::env::temp_dir().join(format!("{}-{uid}", product::legacy_dir())),
        };
        Some(Endpoint(dir.join("daemon.sock")))
    }
    #[cfg(windows)]
    {
        let sid = crate::transport::current_user_sid().ok()?;
        Some(Endpoint(format!(r"\\.\pipe\{}-{}", product::legacy_dir(), sid).into()))
    }
}

/// State directory: Linux `$XDG_STATE_HOME/<app>` (`~/.local/state/<app>`),
/// Windows `%LOCALAPPDATA%\<app>`. `HAETTY_STATE_DIR` overrides (tests).
pub fn state_dir() -> PathBuf {
    if let Some(p) = product::env_os("STATE_DIR") {
        return p.into();
    }
    state_base().join(product::dir())
}

/// Config directory: Linux `$XDG_CONFIG_HOME/<app>` (`~/.config/<app>`),
/// Windows `%APPDATA%\<app>`. `HAETTY_CONFIG_DIR` overrides (tests).
pub fn config_dir() -> PathBuf {
    if let Some(p) = product::env_os("CONFIG_DIR") {
        return p.into();
    }
    config_base().join(product::dir())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn private_dir_is_created_0700_and_checked() {
        let base = haetty_testkit::run_dir("paths");
        let d = base.join("rt");
        ensure_private_dir(&d).unwrap();
        assert_eq!(std::fs::metadata(&d).unwrap().permissions().mode() & 0o777, 0o700);
        ensure_private_dir(&d).unwrap();
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(ensure_private_dir(&d).is_err(), "group/other bits must be refused");
        let link = base.join("link");
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::os::unix::fs::symlink(&d, &link).unwrap();
        assert!(ensure_private_dir(&link).is_err(), "a symlink must be refused");
        haetty_testkit::remove_dir_all(&base);
    }
}
