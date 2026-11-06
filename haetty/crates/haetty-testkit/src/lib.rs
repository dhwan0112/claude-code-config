//! Test isolation (a dev-dependency only; never in the binary): every path a
//! test writes or removes must lie inside this run's own temporary
//! directory, and that directory must not be, or lie inside, the user's real
//! home or configuration: `HOME` (Windows `USERPROFILE`), `~/.config`,
//! `~/.local`, `~/.claude`, `XDG_{CONFIG,DATA,STATE}_HOME`, `%APPDATA%`,
//! `%LOCALAPPDATA%`. Anything else panics before a byte is written.
//!
//! The run directory is `<base>/haetty-test-<pid>-<time>/`, where `<base>` is
//! `HAETTY_TEST_TMP` when set (empty or not absolute = refused: a relative
//! base would land wherever the test happens to run, e.g. the repository),
//! else the system temporary
//! directory on Unix and `%PUBLIC%\haetty-test` on Windows (the Windows temp
//! directory is under `%LOCALAPPDATA%`). The real home and configuration
//! directories are taken from the environment at the first call, before a
//! test points `HOME` or `XDG_*` elsewhere — so call [`run_dir`] first.
//!
//! The same rules for scripts: `scripts/test-guard.sh` (Linux, Git Bash) and
//! `scripts/test-guard.ps1` (PowerShell).

use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

struct Guard {
    root: PathBuf,
    protected: Vec<PathBuf>,
}

static GUARD: OnceLock<Guard> = OnceLock::new();

/// Lexically absolute and normalised (`.`/`..` resolved, no symlink
/// lookups, so not-yet-existing paths work); on Windows lower-cased with
/// `\` separators.
fn norm(p: &Path) -> PathBuf {
    let abs = if p.is_absolute() { p.to_path_buf() } else { std::env::current_dir().unwrap_or_default().join(p) };
    let mut out = PathBuf::new();
    for c in abs.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    if cfg!(windows) {
        PathBuf::from(out.to_string_lossy().replace('/', "\\").to_lowercase())
    } else {
        out
    }
}

fn inside(p: &Path, dir: &Path) -> bool {
    let (p, dir) = (norm(p), norm(dir));
    p == dir || p.starts_with(&dir)
}

/// The directories no test may write in (from the current environment).
pub fn protected_dirs() -> Vec<PathBuf> {
    let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    let mut v = vec![];
    for home in [var("HOME"), var("USERPROFILE")].into_iter().flatten() {
        for sub in ["", ".config", ".local", ".claude"] {
            v.push(if sub.is_empty() { home.clone() } else { home.join(sub) });
        }
    }
    for k in ["XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_STATE_HOME", "APPDATA", "LOCALAPPDATA"] {
        v.extend(var(k));
    }
    v
}

/// Why `path` may not be written, given the protected directories and the
/// run directory; `None` when it may.
pub fn refusal(path: &Path, root: &Path, protected: &[PathBuf]) -> Option<String> {
    if path.as_os_str().is_empty() {
        return Some("empty path".into());
    }
    if let Some(p) = protected.iter().find(|d| inside(path, d) || inside(root, d)) {
        return Some(format!("{} is (inside) the real {}", path.display(), p.display()));
    }
    if !inside(path, root) {
        return Some(format!("{} is outside this test run's directory {}", path.display(), root.display()));
    }
    None
}

/// Why `HAETTY_TEST_TMP` cannot be the base: empty, or not absolute
/// (`C:foo` and `\foo` are not absolute on Windows either).
pub fn base_refusal(base: &Path) -> Option<String> {
    if base.as_os_str().is_empty() {
        return Some("HAETTY_TEST_TMP is set but empty".into());
    }
    if !base.is_absolute() {
        return Some(format!("HAETTY_TEST_TMP={} is not an absolute path", base.display()));
    }
    None
}

fn guard() -> &'static Guard {
    GUARD.get_or_init(|| {
        let protected = protected_dirs();
        let base = match std::env::var_os("HAETTY_TEST_TMP") {
            Some(v) => {
                let v = PathBuf::from(v);
                if let Some(why) = base_refusal(&v) {
                    panic!("test isolation: {why} — refusing to run");
                }
                v
            }
            None if cfg!(windows) => {
                PathBuf::from(std::env::var_os("PUBLIC").expect("test isolation: %PUBLIC% unset")).join("haetty-test")
            }
            None => std::env::temp_dir(),
        };
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let root = base.join(format!("haetty-test-{}-{stamp}", std::process::id()));
        if let Some(p) = protected.iter().find(|d| inside(&root, d)) {
            panic!(
                "test isolation: the test directory {} is inside the real {} — set HAETTY_TEST_TMP elsewhere",
                root.display(),
                p.display()
            );
        }
        std::fs::create_dir_all(&root).unwrap_or_else(|e| panic!("test isolation: {}: {e}", root.display()));
        Guard { root, protected }
    })
}

/// This run's directory (created).
pub fn root() -> &'static Path {
    &guard().root
}

/// A fresh directory `<run dir>/<name>` for one test (emptied if it exists).
pub fn run_dir(name: &str) -> PathBuf {
    let d = check(&guard().root.join(name)).to_path_buf();
    if d.exists() {
        std::fs::remove_dir_all(&d).expect("test isolation: clearing the test directory");
    }
    std::fs::create_dir_all(&d).expect("test isolation: creating the test directory");
    d
}

/// Panics unless `path` may be written or removed by a test.
pub fn check(path: &Path) -> &Path {
    let g = guard();
    if let Some(why) = refusal(path, &g.root, &g.protected) {
        panic!("test isolation: refusing to write {}: {why}", path.display());
    }
    path
}

/// `std::fs::remove_dir_all` after [`check`]; a missing directory is fine.
pub fn remove_dir_all(path: &Path) {
    let _ = std::fs::remove_dir_all(check(path));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_real_home_config_and_outside_paths() {
        let home = PathBuf::from(if cfg!(windows) { r"C:\Users\someone" } else { "/home/someone" });
        let protected: Vec<PathBuf> = ["", ".config", ".local", ".claude"]
            .iter()
            .map(|s| if s.is_empty() { home.clone() } else { home.join(s) })
            .collect();
        let root = PathBuf::from(if cfg!(windows) { r"C:\Users\Public\haetty-test\run-1" } else { "/tmp/run-1" });
        assert_eq!(refusal(&root.join("a/b.txt"), &root, &protected), None);
        assert!(refusal(&root.join("../escape"), &root, &protected).is_some(), "`..` out of the run");
        assert!(refusal(&home, &root, &protected).is_some());
        assert!(refusal(&home.join(".local/share/applications/x.desktop"), &root, &protected).is_some());
        assert!(refusal(&home.join(".claude/settings.json"), &root, &protected).is_some());
        assert!(refusal(Path::new(""), &root, &protected).is_some(), "empty variable");
        // The base itself: empty or relative (a path whose separators a
        // shell ate, e.g. `C:UserssomeoneTemp`) is refused.
        assert!(base_refusal(Path::new("")).is_some());
        assert!(base_refusal(Path::new("C:UserssomeoneAppDataLocalTempx")).is_some());
        assert!(base_refusal(Path::new("haetty-test")).is_some());
        assert!(base_refusal(&root).is_none());
        // A run directory inside the home is refused even for its own files.
        let bad_root = home.join("tmp/run");
        assert!(refusal(&bad_root.join("x"), &bad_root, &protected).is_some());
        // Our own run directory works and is outside every protected one.
        let d = run_dir("self-test");
        std::fs::write(d.join("f"), "x").unwrap();
        assert!(protected_dirs().iter().all(|p| !inside(&d, p)));
        remove_dir_all(&d);
        assert!(!d.exists());
    }
}
