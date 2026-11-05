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
        if let Some(p) = std::env::var_os("CCTERM_SOCKET") {
            return Ok(Endpoint(p.into()));
        }
        #[cfg(unix)]
        {
            Ok(Endpoint(runtime_dir()?.join("daemon.sock")))
        }
        #[cfg(windows)]
        {
            let sid = crate::transport::current_user_sid()?;
            Ok(Endpoint(format!(r"\\.\pipe\{}-{}", product::DIR, sid).into()))
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
        Some(b) => PathBuf::from(b).join(product::DIR),
        None => std::env::temp_dir().join(format!("{}-{uid}", product::DIR)),
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

/// State directory: Linux `$XDG_STATE_HOME/<app>` (`~/.local/state/<app>`),
/// Windows `%LOCALAPPDATA%\<app>`. `CCTERM_STATE_DIR` overrides (tests).
pub fn state_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("CCTERM_STATE_DIR") {
        return p.into();
    }
    #[cfg(windows)]
    {
        let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        base.join(product::DIR)
    }
    #[cfg(not(windows))]
    {
        let base = std::env::var_os("XDG_STATE_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
            .unwrap_or_else(std::env::temp_dir);
        base.join(product::DIR)
    }
}

/// Config directory: Linux `$XDG_CONFIG_HOME/<app>` (`~/.config/<app>`),
/// Windows `%APPDATA%\<app>`. `CCTERM_CONFIG_DIR` overrides (tests).
pub fn config_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("CCTERM_CONFIG_DIR") {
        return p.into();
    }
    #[cfg(windows)]
    {
        let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        base.join(product::DIR)
    }
    #[cfg(not(windows))]
    {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .unwrap_or_else(std::env::temp_dir);
        base.join(product::DIR)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn private_dir_is_created_0700_and_checked() {
        let base = std::env::temp_dir().join(format!("ccterm-paths-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
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
        std::fs::remove_dir_all(&base).unwrap();
    }
}
