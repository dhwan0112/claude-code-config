//! The installed layout (DESIGN.md §3.15, §3.17), in Rust so that `tools/install-stable.sh` (through
//! `haetty install-layout`) and the in-app update use the same code:
//!
//! - Linux: `<prefix>/.local/opt/haetty/<version>/` (read-only: files and folder 0555) and the link
//!   `<prefix>/.local/bin/haetty` → `<version>/haetty`, switched atomically (a new link renamed over the old one).
//! - Windows: `<prefix>\Programs\haetty\<version>\` (read-only attribute) and the junction `…\haetty\current` →
//!   `<version>`; switched by making `current.new` and renaming it over `current` after removing the old junction
//!   (a junction cannot be replaced in one step; a start in that instant finds no `current`).
//!
//! `<version>` = `<crate version>-<12-digit commit>`. Older versions stay (rollback); [`prune`] keeps the newest few.

use std::path::{Path, PathBuf};

/// How the running executable was installed (stage PU, U0).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Form {
    /// (a) A version folder and the link (`install-stable.sh`, `.ps1`, or an earlier update).
    Versioned { opt: PathBuf, link: PathBuf, current: String },
    /// (b) One executable anywhere, in a folder this user may write.
    Single { exe: PathBuf },
    /// (c) A dev instance or a cargo build: never updated.
    Dev(String),
    /// (d) No write permission where it would have to write.
    NoWrite(String),
}

impl Form {
    /// Why an update cannot be applied here (`None`: it can).
    pub fn refusal(&self) -> Option<String> {
        match self {
            Form::Versioned { .. } | Form::Single { .. } => None,
            Form::Dev(why) => Some(format!("{why}: not updated (build or install it yourself)")),
            Form::NoWrite(why) => Some(format!("{why}: install the new version by hand (README, \"Updates\")")),
        }
    }
}

/// The executable's name in a version folder.
pub fn exe_name() -> &'static str {
    if cfg!(windows) {
        "haetty.exe"
    } else {
        "haetty"
    }
}

/// `0.3.2-622d629bb224`.
pub fn version_name(version: &str, commit: &str) -> String {
    format!("{version}-{}", &commit[..commit.len().min(12)])
}

fn writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".haetty-write-probe-{}", std::process::id()));
    match std::fs::File::create(&probe) {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// The link of a versioned layout whose versions are in `opt`.
pub fn link_of(opt: &Path) -> Option<PathBuf> {
    if cfg!(windows) {
        Some(opt.join("current"))
    } else {
        // <prefix>/.local/opt/haetty → <prefix>/.local/bin/haetty
        Some(opt.parent()?.parent()?.join("bin").join("haetty"))
    }
}

/// The version folder the link points to.
pub fn active(link: &Path) -> Option<String> {
    let t = std::fs::read_link(link).ok()?;
    let t = if cfg!(windows) { t } else { t.parent()?.to_path_buf() };
    let name = t.file_name()?.to_string_lossy().into_owned();
    // Windows junction targets may come back with a `\\?\` prefix: the last part is the name either way.
    Some(name)
}

/// What the running executable `exe` is (canonical path), for an instance named `instance`.
pub fn detect(exe: &Path, instance: &str) -> Form {
    if !instance.is_empty() {
        return Form::Dev(format!("the {instance} instance"));
    }
    let exe = std::fs::canonicalize(exe).unwrap_or_else(|_| exe.to_path_buf());
    let parts: Vec<String> = exe.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    if parts.windows(2).any(|w| {
        w[0] == "target"
            && (matches!(w[1].as_str(), "debug" | "release" | "dist")
                || w[1].contains("-unknown-")
                || w[1].contains("-pc-"))
    }) {
        return Form::Dev("a cargo build".into());
    }
    let dir = exe.parent().map(Path::to_path_buf).unwrap_or_default();
    let opt = dir.parent().map(Path::to_path_buf).unwrap_or_default();
    let in_layout = dir.join("INSTALLED").is_file()
        && opt.file_name().is_some_and(|n| n == "haetty")
        && opt.parent().and_then(|p| p.file_name()).is_some_and(|n| n == "opt" || n == "Programs");
    if in_layout {
        let Some(link) = link_of(&opt) else { return Form::NoWrite("no link".into()) };
        let current = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let link_dir = link.parent().map(Path::to_path_buf).unwrap_or_default();
        if !writable(&opt) || !writable(&link_dir) {
            return Form::NoWrite(format!("{} or {} is not writable", opt.display(), link_dir.display()));
        }
        return Form::Versioned { opt, link, current };
    }
    if !writable(&dir) {
        return Form::NoWrite(format!("{} is not writable", dir.display()));
    }
    Form::Single { exe }
}

/// What `INSTALLED` records.
pub struct Installed<'a> {
    pub version: &'a str,
    pub reference: &'a str,
    pub commit: &'a str,
    pub source: &'a str,
}

fn set_read_only(p: &Path, ro: bool) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if p.is_dir() {
            if ro {
                0o555
            } else {
                0o755
            }
        } else if ro {
            0o555
        } else {
            0o755
        };
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode))
    }
    #[cfg(windows)]
    {
        if p.is_dir() {
            return Ok(()); // Windows: the files carry the attribute (as install-stable.ps1 does)
        }
        let mut perm = std::fs::metadata(p)?.permissions();
        perm.set_readonly(ro);
        std::fs::set_permissions(p, perm)
    }
}

/// Removes a version folder (made read-only) completely.
pub fn remove_version(opt: &Path, name: &str) -> std::io::Result<()> {
    let dir = opt.join(name);
    if !dir.exists() {
        return Ok(());
    }
    let _ = set_read_only(&dir, false);
    for e in std::fs::read_dir(&dir)? {
        let p = e?.path();
        let _ = set_read_only(&p, false);
    }
    std::fs::remove_dir_all(&dir)
}

/// Writes `files` (plain names) as the version folder `opt/name`: into `opt/.name.partial` first, `INSTALLED`
/// beside them, everything read-only, then renamed into place. An existing folder of that name is kept (an error).
pub fn put_version(opt: &Path, name: &str, files: &[(String, Vec<u8>)], info: &Installed) -> Result<PathBuf, String> {
    if !crate::manifest::plain_name(name.trim_start_matches('.')) || name.contains('/') {
        return Err(format!("bad version name {name:?}"));
    }
    let dest = opt.join(name);
    if dest.exists() {
        return Err(format!("{} exists already", dest.display()));
    }
    std::fs::create_dir_all(opt).map_err(|e| format!("{}: {e}", opt.display()))?;
    let stage = opt.join(format!(".{name}.partial"));
    if stage.exists() {
        remove_version(opt, &format!(".{name}.partial")).map_err(|e| format!("{}: {e}", stage.display()))?;
    }
    let res = (|| -> std::io::Result<()> {
        std::fs::create_dir(&stage)?;
        for (n, data) in files {
            if !crate::manifest::plain_name(n) {
                return Err(std::io::Error::other(format!("bad file name {n:?}")));
            }
            let p = stage.join(n);
            std::fs::write(&p, data)?;
        }
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        std::fs::write(
            stage.join("INSTALLED"),
            format!(
                "version={}\nref={}\ncommit={}\ninstalled={}\nsource={}\n",
                info.version,
                info.reference,
                info.commit,
                crate::rfc3339_s(now),
                info.source
            ),
        )?;
        for e in std::fs::read_dir(&stage)? {
            set_read_only(&e?.path(), true)?;
        }
        set_read_only(&stage, true)?;
        std::fs::rename(&stage, &dest)
    })();
    if let Err(e) = res {
        let _ = remove_version(opt, &format!(".{name}.partial"));
        return Err(format!("installing {name}: {e}"));
    }
    Ok(dest)
}

/// Points the link at `opt/name`; returns the version it pointed at before.
pub fn activate(opt: &Path, link: &Path, name: &str) -> Result<Option<String>, String> {
    let target_dir = opt.join(name);
    if !target_dir.join(exe_name()).is_file() {
        return Err(format!("version {name} is not installed"));
    }
    let prev = active(link);
    if let Ok(m) = std::fs::symlink_metadata(link) {
        let is_link = m.file_type().is_symlink() || cfg!(windows) && junction::is_junction(link);
        if !is_link {
            return Err(format!("{} exists and is not a link (another haetty?); move it away first", link.display()));
        }
    }
    if let Some(d) = link.parent() {
        std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    let tmp =
        link.with_file_name(format!("{}.new.{}", link.file_name().unwrap().to_string_lossy(), std::process::id()));
    #[cfg(unix)]
    {
        let _ = std::fs::remove_file(&tmp);
        std::os::unix::fs::symlink(target_dir.join(exe_name()), &tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, link).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            format!("{}: {e}", link.display())
        })?;
    }
    #[cfg(windows)]
    {
        junction::create(&target_dir, &tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
        if link.exists() || std::fs::symlink_metadata(link).is_ok() {
            std::fs::remove_dir(link).map_err(|e| format!("{}: {e}", link.display()))?;
        }
        std::fs::rename(&tmp, link).map_err(|e| format!("{}: {e}", link.display()))?;
    }
    Ok(prev.filter(|p| p != name))
}

/// The installed versions, newest first (by their `installed=` line), with the active one marked.
pub fn list(opt: &Path, link: &Path) -> Vec<(String, bool, String)> {
    let cur = active(link);
    let mut v: Vec<(String, bool, String)> = std::fs::read_dir(opt)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| e.path().join("INSTALLED").is_file())
                .map(|e| {
                    let n = e.file_name().to_string_lossy().into_owned();
                    let info = std::fs::read_to_string(e.path().join("INSTALLED")).unwrap_or_default();
                    let at = info.lines().find_map(|l| l.strip_prefix("installed=")).unwrap_or("").to_string();
                    (n.clone(), cur.as_deref() == Some(n.as_str()), at)
                })
                .collect()
        })
        .unwrap_or_default();
    v.sort_by(|a, b| b.2.cmp(&a.2).then(b.0.cmp(&a.0)));
    v
}

/// Removes old versions: keeps the active one, `also` (the rollback target) and the `keep` newest others.
pub fn prune(opt: &Path, link: &Path, keep: usize, also: Option<&str>) -> Vec<String> {
    let mut removed = vec![];
    let mut kept = 0;
    for (name, is_active, _) in list(opt, link) {
        if is_active || Some(name.as_str()) == also {
            continue;
        }
        if kept < keep {
            kept += 1;
            continue;
        }
        if remove_version(opt, &name).is_ok() {
            removed.push(name);
        }
    }
    removed
}

/// (b): puts `new` in place of the running `exe`, the running file renamed to `<exe>.old` beside it (Windows lets a
/// running executable be renamed, not removed: it goes at the next start, [`clean_old`]). Restored on failure.
pub fn replace_single(exe: &Path, new: &[u8]) -> Result<(), String> {
    let old = exe.with_file_name(format!("{}.old", exe.file_name().unwrap_or_default().to_string_lossy()));
    let fresh = exe.with_file_name(format!("{}.new", exe.file_name().unwrap_or_default().to_string_lossy()));
    std::fs::write(&fresh, new).map_err(|e| format!("{}: {e}", fresh.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&fresh, std::fs::Permissions::from_mode(0o755));
    }
    let _ = std::fs::remove_file(&old);
    if let Err(e) = std::fs::rename(exe, &old) {
        let _ = std::fs::remove_file(&fresh);
        return Err(format!("{}: {e}", exe.display()));
    }
    if let Err(e) = std::fs::rename(&fresh, exe) {
        let _ = std::fs::rename(&old, exe);
        let _ = std::fs::remove_file(&fresh);
        return Err(format!("{}: {e}", exe.display()));
    }
    Ok(())
}

/// (b): the file a single-file update left (`<exe>.old`), removed at the next start.
pub fn clean_old(exe: &Path) {
    let old = exe.with_file_name(format!("{}.old", exe.file_name().unwrap_or_default().to_string_lossy()));
    let _ = std::fs::remove_file(old);
}

/// (b) rollback: the `.old` file back in place of the executable (the newer one kept as `.new`).
pub fn rollback_single(exe: &Path) -> Result<(), String> {
    let name = exe.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let old = exe.with_file_name(format!("{name}.old"));
    if !old.is_file() {
        return Err("no earlier version beside the executable".into());
    }
    let newer = exe.with_file_name(format!("{name}.new"));
    let _ = std::fs::remove_file(&newer);
    std::fs::rename(exe, &newer).map_err(|e| format!("{}: {e}", exe.display()))?;
    if let Err(e) = std::fs::rename(&old, exe) {
        let _ = std::fs::rename(&newer, exe);
        return Err(format!("{}: {e}", exe.display()));
    }
    Ok(())
}

/// Windows directory junctions (mount-point reparse points), without a shell (CLAUDE.md: no shell at run time).
#[cfg(windows)]
pub mod junction {
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    use windows_sys::Win32::Foundation::{CloseHandle, GENERIC_WRITE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        OPEN_EXISTING,
    };
    use windows_sys::Win32::System::Ioctl::FSCTL_SET_REPARSE_POINT;
    use windows_sys::Win32::System::IO::DeviceIoControl;

    const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xA000_0003;

    fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
        s.encode_wide().chain(Some(0)).collect()
    }

    pub fn is_junction(p: &Path) -> bool {
        use std::os::windows::fs::MetadataExt;
        std::fs::symlink_metadata(p).is_ok_and(|m| m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0)
    }

    /// Makes `link` a junction to the folder `target`.
    pub fn create(target: &Path, link: &Path) -> std::io::Result<()> {
        let target = std::fs::canonicalize(target)?;
        let t = target.as_os_str().to_string_lossy().into_owned();
        let t = t.strip_prefix(r"\\?\").unwrap_or(&t).to_string();
        let subst: Vec<u16> = format!(r"\??\{t}").encode_utf16().collect();
        let print: Vec<u16> = t.encode_utf16().collect();
        std::fs::create_dir(link)?;
        let path = wide(link.as_os_str());
        // SAFETY: plain Win32 calls with buffers that outlive them; the handle is closed below.
        unsafe {
            let h = CreateFileW(
                path.as_ptr(),
                GENERIC_WRITE,
                0,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
                std::ptr::null_mut(),
            );
            if h == INVALID_HANDLE_VALUE {
                let e = std::io::Error::last_os_error();
                let _ = std::fs::remove_dir(link);
                return Err(e);
            }
            // REPARSE_DATA_BUFFER, MountPointReparseBuffer: tag, length, reserved, 4 offsets/lengths, paths.
            let path_bytes = (subst.len() + 1 + print.len() + 1) * 2;
            let mut buf: Vec<u8> = vec![];
            buf.extend(IO_REPARSE_TAG_MOUNT_POINT.to_le_bytes());
            buf.extend(((8 + path_bytes) as u16).to_le_bytes());
            buf.extend(0u16.to_le_bytes());
            buf.extend(0u16.to_le_bytes());
            buf.extend(((subst.len() * 2) as u16).to_le_bytes());
            buf.extend((((subst.len() + 1) * 2) as u16).to_le_bytes());
            buf.extend(((print.len() * 2) as u16).to_le_bytes());
            for c in subst.iter().chain([0u16].iter()).chain(print.iter()).chain([0u16].iter()) {
                buf.extend(c.to_le_bytes());
            }
            let mut ret = 0u32;
            let ok = DeviceIoControl(
                h,
                FSCTL_SET_REPARSE_POINT,
                buf.as_ptr() as *const _,
                buf.len() as u32,
                std::ptr::null_mut(),
                0,
                &mut ret,
                std::ptr::null_mut(),
            );
            let e = std::io::Error::last_os_error();
            CloseHandle(h);
            if ok == 0 {
                let _ = std::fs::remove_dir(link);
                return Err(e);
            }
        }
        Ok(())
    }
}

#[cfg(not(windows))]
mod junction {
    pub fn is_junction(_: &std::path::Path) -> bool {
        false
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn files(tag: &str) -> Vec<(String, Vec<u8>)> {
        vec![("haetty".into(), format!("#!/bin/sh\necho {tag}\n").into_bytes()), ("LICENSE".into(), b"MIT".to_vec())]
    }

    fn info<'a>(v: &'a str) -> Installed<'a> {
        Installed { version: v, reference: "test", commit: "0123456789ab", source: "test" }
    }

    #[test]
    fn versions_link_prune_and_detect() {
        let base = haetty_testkit::run_dir("layout");
        let opt = base.join("p/.local/opt/haetty");
        let link = link_of(&opt).unwrap();
        assert_eq!(link, base.join("p/.local/bin/haetty"));
        let a = put_version(&opt, "0.3.2-aaaaaaaaaaaa", &files("a"), &info("0.3.2-aaaaaaaaaaaa")).unwrap();
        assert!(put_version(&opt, "0.3.2-aaaaaaaaaaaa", &files("a"), &info("x")).is_err(), "never overwritten");
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&a).unwrap().permissions().mode() & 0o777, 0o555);
        assert_eq!(std::fs::metadata(a.join("haetty")).unwrap().permissions().mode() & 0o777, 0o555);
        assert!(!opt.join(".0.3.2-aaaaaaaaaaaa.partial").exists());
        assert_eq!(activate(&opt, &link, "0.3.2-aaaaaaaaaaaa").unwrap(), None);
        assert_eq!(active(&link).as_deref(), Some("0.3.2-aaaaaaaaaaaa"));
        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        for (i, n) in ["0.3.3-bbbbbbbbbbbb", "0.3.4-cccccccccccc", "0.3.5-dddddddddddd"].iter().enumerate() {
            put_version(&opt, n, &files(n), &info(n)).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(1100 * (i == 0) as u64));
        }
        assert_eq!(activate(&opt, &link, "0.3.5-dddddddddddd").unwrap().as_deref(), Some("0.3.2-aaaaaaaaaaaa"));
        assert!(activate(&opt, &link, "9.9.9-none").is_err());
        // Detect: the active version's executable is form (a); a copy elsewhere is (b); a cargo path is (c).
        let exe = opt.join("0.3.5-dddddddddddd/haetty");
        assert_eq!(
            detect(&exe, ""),
            Form::Versioned { opt: opt.clone(), link: link.clone(), current: "0.3.5-dddddddddddd".into() }
        );
        assert!(matches!(detect(&exe, "dev"), Form::Dev(_)));
        let single = base.join("single/haetty");
        std::fs::create_dir_all(single.parent().unwrap()).unwrap();
        std::fs::write(&single, b"x").unwrap();
        assert_eq!(detect(&single, ""), Form::Single { exe: std::fs::canonicalize(&single).unwrap() });
        let cargo = base.join("w/target/release/haetty");
        std::fs::create_dir_all(cargo.parent().unwrap()).unwrap();
        std::fs::write(&cargo, b"x").unwrap();
        assert!(matches!(detect(&cargo, ""), Form::Dev(_)));
        // A folder this user cannot write: (d).
        let ro = base.join("ro");
        std::fs::create_dir_all(&ro).unwrap();
        std::fs::write(ro.join("haetty"), b"x").unwrap();
        std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o555)).unwrap();
        if !writable(&ro) {
            assert!(matches!(detect(&ro.join("haetty"), ""), Form::NoWrite(_)));
        }
        std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o755)).unwrap();
        // Prune: the active one, the rollback target and one more stay.
        let removed = prune(&opt, &link, 1, Some("0.3.2-aaaaaaaaaaaa"));
        let left: Vec<String> = list(&opt, &link).into_iter().map(|x| x.0).collect();
        assert!(left.contains(&"0.3.5-dddddddddddd".to_string()) && left.contains(&"0.3.2-aaaaaaaaaaaa".to_string()));
        assert_eq!(left.len(), 3, "{left:?} (removed {removed:?})");
        // A real file at the link's place is not replaced.
        std::fs::remove_file(&link).unwrap();
        std::fs::write(&link, b"mine").unwrap();
        assert!(activate(&opt, &link, "0.3.5-dddddddddddd").is_err());
        assert_eq!(std::fs::read(&link).unwrap(), b"mine");
        for (n, _, _) in list(&opt, &link) {
            remove_version(&opt, &n).unwrap();
        }
        haetty_testkit::remove_dir_all(&base);
    }

    #[test]
    fn single_file_replace_and_roll_back() {
        let base = haetty_testkit::run_dir("layout-single");
        let exe = base.join("haetty");
        std::fs::write(&exe, b"old").unwrap();
        replace_single(&exe, b"new").unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"new");
        assert_eq!(std::fs::read(base.join("haetty.old")).unwrap(), b"old");
        rollback_single(&exe).unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"old");
        clean_old(&exe);
        assert!(!base.join("haetty.old").exists());
        // A failure leaves the old file in place.
        let missing = base.join("nope/haetty");
        assert!(replace_single(&missing, b"x").is_err());
        haetty_testkit::remove_dir_all(&base);
    }
}
