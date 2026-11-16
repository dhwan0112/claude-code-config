//! Writing one setting into the config file (the settings panel, DESIGN.md §3.16).
//!
//! Only the line of that key changes: comments, blank lines, the order of lines, unknown keys and each line's end
//! (LF or CRLF) stay as they are. A key that is not in the file gets one line at the end; going back to the default
//! removes the key's lines (commented-out lines are not touched). When a key is in the file more than once, the parser
//! uses the last line ([`crate::config::Config::set`] overwrites), so that line is the one changed.
//!
//! The file is written to a temporary file in the same folder, synced, then renamed over it. A symbolic link stays a
//! link: the file it points to is written. A read-only file is not written. Before the first write of a run, the file
//! is copied to `config.bak` (an existing backup is kept: the copy gets the date, `config.bak.2026-11-16`). Every write
//! reads the file again first, so a change made elsewhere since the panel opened is kept and only this key changes.

use std::path::{Path, PathBuf};

use crate::config::{Config, Setting};

/// How a line ends: the file's first line end decides for new lines.
fn eol_of(text: &str) -> &'static str {
    match text.find('\n') {
        Some(i) if i > 0 && text.as_bytes()[i - 1] == b'\r' => "\r\n",
        _ => "\n",
    }
}

/// The key of a setting line (`key = value`, as `theme::lines` reads it); `None` for blank lines, comments and
/// lines without `=`.
fn key_of(line: &str) -> Option<&str> {
    let t = line.trim();
    if t.is_empty() || t.starts_with('#') {
        return None;
    }
    t.split_once('=').map(|(k, _)| k.trim())
}

/// The file's lines, each with its own end ("" for a last line without one).
fn split_lines(text: &str) -> Vec<(&str, &str)> {
    let mut out = vec![];
    let mut rest = text;
    while !rest.is_empty() {
        match rest.find('\n') {
            Some(i) => {
                let (line, end) = if i > 0 && rest.as_bytes()[i - 1] == b'\r' {
                    (&rest[..i - 1], "\r\n")
                } else {
                    (&rest[..i], "\n")
                };
                out.push((line, end));
                rest = &rest[i + 1..];
            }
            None => {
                out.push((rest, ""));
                rest = "";
            }
        }
    }
    out
}

/// A value as a line writes it: quoted when the parser would otherwise lose part of it (spaces at an end, quotes).
fn quoted(value: &str) -> String {
    if value != value.trim() || (value.starts_with('"') && value.ends_with('"') && value.len() >= 2) {
        format!("\"{value}\"")
    } else {
        value.to_string()
    }
}

/// `text` with `key` set to `value` (`None`: the key's lines removed), changing only those lines.
pub fn edit_text(text: &str, key: &str, value: Option<&str>) -> String {
    let eol = eol_of(text);
    let lines = split_lines(text);
    let last = lines.iter().rposition(|(l, _)| key_of(l) == Some(key));
    let mut out = String::with_capacity(text.len() + 64);
    let new_line = |v: &str| format!("{key} = {}", quoted(v));
    for (i, (line, end)) in lines.iter().enumerate() {
        match value {
            None if key_of(line) == Some(key) => continue,
            Some(v) if Some(i) == last => {
                out.push_str(&new_line(v));
                out.push_str(end);
            }
            _ => {
                out.push_str(line);
                out.push_str(end);
            }
        }
    }
    if let (Some(v), None) = (value, last) {
        if !out.is_empty() && !out.ends_with('\n') {
            out.push_str(eol);
        }
        out.push_str(&new_line(v));
        out.push_str(eol);
    }
    out
}

/// What the parser makes of `key` in `text` (`Config::value`).
pub fn read_value(text: &str, key: &str) -> Option<String> {
    let mut c = Config::default();
    c.apply_file(text, &mut crate::keymap::Keymap::default());
    c.value(key)
}

/// [`edit_text`], then checked by parsing the result: when another key of the file still decides the value (the
/// `window-padding` shorthand after `window-padding-x`), a line at the end sets it. `None` is the default.
pub fn set_in_text(text: &str, key: &str, value: Option<&str>) -> String {
    let s = Setting::find(key);
    let want = value.map(str::to_string).unwrap_or_else(|| Config::default().value(key).unwrap_or_default());
    let mut out = edit_text(text, key, value);
    if read_value(&out, key).as_deref() != Some(want.as_str()) && !want.is_empty() {
        let fixed = edit_text(&out, key, None);
        let eol = eol_of(text);
        out = fixed;
        if !out.is_empty() && !out.ends_with('\n') {
            out.push_str(eol);
        }
        out.push_str(&format!("{key} = {}{eol}", quoted(&want)));
    }
    debug_assert!(s.is_some(), "{key}: not a setting");
    out
}

/// The config file as the panel writes it.
pub struct ConfigFile {
    pub path: PathBuf,
    /// Modification time and size when the panel last read or wrote it.
    seen: Option<(std::time::SystemTime, u64)>,
    backed_up: bool,
}

/// The result of a write.
#[derive(Debug, PartialEq, Eq)]
pub struct Written {
    /// The file had changed since it was last read: its other lines were kept as found now.
    pub changed_outside: bool,
    /// The backup made before this write.
    pub backup: Option<PathBuf>,
}

fn stamp(p: &Path) -> Option<(std::time::SystemTime, u64)> {
    let m = std::fs::metadata(p).ok()?;
    Some((m.modified().ok()?, m.len()))
}

impl ConfigFile {
    pub fn new(path: PathBuf) -> ConfigFile {
        let seen = stamp(&path);
        ConfigFile { path, seen, backed_up: false }
    }

    /// The file changed since it was last read or written here.
    pub fn changed(&self) -> bool {
        stamp(&self.path) != self.seen
    }

    /// Writes `key` (`None`: back to the default) into the file. Errors are one line for the panel.
    pub fn write(&mut self, key: &str, value: Option<&str>) -> Result<Written, String> {
        let changed_outside = self.changed();
        // A link stays a link: write the file it points to.
        let target = match std::fs::symlink_metadata(&self.path) {
            Ok(m) if m.file_type().is_symlink() => {
                std::fs::canonicalize(&self.path).map_err(|e| format!("the config file's link: {e}"))?
            }
            _ => self.path.clone(),
        };
        let meta = std::fs::metadata(&target).ok();
        if meta.as_ref().is_some_and(|m| m.permissions().readonly()) {
            return Err(format!("{} is read-only: not changed", target.display()));
        }
        let old = match std::fs::read(&target) {
            Ok(b) => String::from_utf8(b).map_err(|_| "the config file is not UTF-8: not changed".to_string())?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(format!("reading the config file: {e}")),
        };
        let new = set_in_text(&old, key, value);
        if new == old {
            self.seen = stamp(&self.path);
            return Ok(Written { changed_outside, backup: None });
        }
        let dir = target.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
        std::fs::create_dir_all(&dir).map_err(|e| format!("making {}: {e}", dir.display()))?;
        let mut backup = None;
        if !self.backed_up && meta.is_some() {
            backup = Some(make_backup(&target)?);
            self.backed_up = true;
        }
        let name = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "config".into());
        let tmp = dir.join(format!(".{name}.tmp-{}", std::process::id()));
        let res = (|| -> std::io::Result<()> {
            use std::io::Write;
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(new.as_bytes())?;
            if let Some(m) = &meta {
                // The file keeps its permissions.
                f.set_permissions(m.permissions())?;
            }
            f.sync_all()?;
            drop(f);
            std::fs::rename(&tmp, &target)
        })();
        if let Err(e) = res {
            let _ = std::fs::remove_file(&tmp);
            return Err(format!("writing {}: {e}", target.display()));
        }
        self.seen = stamp(&self.path);
        Ok(Written { changed_outside, backup })
    }
}

/// `config.bak` beside the file, or — when that exists — `config.bak.YYYY-MM-DD` (then `-2`, `-3`, …): an existing
/// backup is never overwritten.
fn make_backup(target: &Path) -> Result<PathBuf, String> {
    let name = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "config".into());
    let dir = target.parent().unwrap_or(Path::new("."));
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
    let date = crate::cli::rfc3339(now).chars().take(10).collect::<String>();
    let mut cands = vec![dir.join(format!("{name}.bak")), dir.join(format!("{name}.bak.{date}"))];
    cands.extend((2..100).map(|n| dir.join(format!("{name}.bak.{date}-{n}"))));
    let to = cands.into_iter().find(|p| std::fs::symlink_metadata(p).is_err()).ok_or("too many backups")?;
    std::fs::copy(target, &to).map_err(|e| format!("backup {}: {e}", to.display()))?;
    Ok(to)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// (file, key, value, expected file): the table of line-preserving edits.
    #[test]
    fn edits_change_only_the_line_of_the_key() {
        let cases: &[(&str, &str, Option<&str>, &str)] = &[
            // A comment, a blank line and an unknown key stay; the value changes in place.
            ("# mine\n\nfont-size = 11\nbogus = 1\n", "font-size", Some("13"), "# mine\n\nfont-size = 13\nbogus = 1\n"),
            // CRLF stays CRLF, the new line too.
            ("theme = x\r\n# c\r\n", "font-size", Some("13"), "theme = x\r\n# c\r\nfont-size = 13\r\n"),
            ("a = 1\r\nfont-size = 9\r\n", "font-size", Some("12"), "a = 1\r\nfont-size = 12\r\n"),
            // Mixed ends: each line keeps its own.
            ("a = 1\r\nb = 2\nfont-size = 9\r\n", "font-size", Some("12"), "a = 1\r\nb = 2\nfont-size = 12\r\n"),
            // A duplicate key: the last line (the parser's) changes, the first stays.
            (
                "font-size = 9\nx = 1\nfont-size = 10\n",
                "font-size",
                Some("12"),
                "font-size = 9\nx = 1\nfont-size = 12\n",
            ),
            // A missing key: one line at the end; a last line without an end gets one first.
            ("theme = x", "font-size", Some("12"), "theme = x\nfont-size = 12\n"),
            ("", "font-size", Some("12"), "font-size = 12\n"),
            // Back to the default: every line of the key goes; a commented-out one stays.
            ("#font-size = 30\nfont-size = 9\nx = 1\n  font-size=10\n", "font-size", None, "#font-size = 30\nx = 1\n"),
            // Indented and spaced lines are still the key's.
            ("  theme   =  old  \n", "theme", Some("haetty classic"), "theme = haetty classic\n"),
            // A value with spaces at its end is quoted.
            ("", "font-family", Some(" Odd "), "font-family = \" Odd \"\n"),
        ];
        for (file, key, value, want) in cases {
            assert_eq!(&edit_text(file, key, *value), want, "{file:?} {key} {value:?}");
            // What the panel shows is what the parser reads back.
            let back = read_value(want, key).unwrap();
            let shown = value.map(str::to_string).unwrap_or_else(|| Config::default().value(key).unwrap());
            assert_eq!(back, shown.trim(), "{file:?} {key}");
        }
    }

    #[test]
    fn a_shorthand_line_that_decides_the_value_gets_a_line_after_it() {
        // window-padding after window-padding-x decides x: the edit appends.
        let t = "window-padding-x = 4\nwindow-padding = 9\n";
        let out = set_in_text(t, "window-padding-x", Some("12"));
        assert_eq!(
            out,
            "window-padding-x = 12\nwindow-padding = 9\nwindow-padding-x = 12\n"
                .replace("window-padding-x = 12\nwindow-padding", "window-padding-x = 4\nwindow-padding")
                .as_str()
                .replace(
                    "window-padding-x = 4\nwindow-padding = 9\nwindow-padding-x = 12\n",
                    "window-padding = 9\nwindow-padding-x = 12\n"
                )
        );
        assert_eq!(read_value(&out, "window-padding-x").as_deref(), Some("12"));
        assert_eq!(read_value(&out, "window-padding-y").as_deref(), Some("9"), "y untouched");
        // Back to the default with the shorthand still setting x: an explicit default line.
        let out = set_in_text("window-padding = 9\nwindow-padding-x = 3\n", "window-padding-x", None);
        assert_eq!(out, "window-padding = 9\nwindow-padding-x = 10\n");
        assert_eq!(read_value(&out, "window-padding-x").as_deref(), Some("10"));
    }

    /// Any file: the lines the edit does not touch are the same bytes, in the same order, and the parser reads the
    /// new value back.
    #[test]
    fn untouched_lines_stay_byte_for_byte() {
        let pieces = [
            "# comment",
            "",
            "   ",
            "theme = haetty classic",
            "font-size = 9",
            "font-size=10",
            "unknown-key = 7",
            "not a setting line",
            "background = #102030",
            "  # font-size = 99",
            "window-padding = 4",
            "keybind = ctrl+shift+n=new_tab",
            "palette = 1=#ff0000",
        ];
        let ends = ["\n", "\r\n"];
        let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut next = |n: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % n as u64) as usize
        };
        for round in 0..400 {
            let mut text = String::new();
            for _ in 0..next(9) {
                text.push_str(pieces[next(pieces.len())]);
                text.push_str(ends[next(2)]);
            }
            if next(4) == 0 {
                text.push_str(pieces[next(pieces.len())]); // no end on the last line
            }
            let (key, value) = [
                ("font-size", Some("14")),
                ("font-size", None),
                ("theme", Some("IBM 5153 CGA (Black)")),
                ("background", Some("#000000")),
                ("settings-button", Some("never")),
            ][next(5)];
            let out = edit_text(&text, key, value);
            let keep = |t: &str| -> Vec<String> {
                split_lines(t)
                    .into_iter()
                    .filter(|(l, _)| key_of(l) != Some(key))
                    .map(|(l, e)| format!("{l}{e}"))
                    .collect()
            };
            let (a, mut b) = (keep(&text), keep(&out));
            // A last line without an end may have got one before an appended line.
            if let (Some(x), Some(y)) = (a.last(), b.last_mut()) {
                if !x.ends_with('\n') && y.starts_with(x.as_str()) {
                    *y = x.clone();
                }
            }
            assert_eq!(a, b, "round {round}: {text:?} {key} {value:?} -> {out:?}");
            let want = value.map(str::to_string).unwrap_or_else(|| Config::default().value(key).unwrap());
            assert_eq!(read_value(&out, key), Some(want), "round {round}: {out:?}");
        }
    }

    #[test]
    fn writes_go_through_a_temporary_file_keep_links_and_back_up_once() {
        let dir = haetty_testkit::run_dir("config-edit");
        let path = dir.join("config");
        // No file yet: made on the first write, no backup (nothing to back up).
        let mut f = ConfigFile::new(path.clone());
        let w = f.write("font-size", Some("13")).unwrap();
        assert_eq!(w, Written { changed_outside: false, backup: None });
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "font-size = 13\n");
        // An existing file: one backup per run, `config.bak` kept, the next run's gets the date.
        std::fs::write(&path, "# mine\r\nfont-size = 13\r\n").unwrap();
        let mut f = ConfigFile::new(path.clone());
        let w = f.write("theme", Some("haetty classic")).unwrap();
        assert_eq!(w.backup.as_deref(), Some(dir.join("config.bak").as_path()));
        assert_eq!(std::fs::read_to_string(dir.join("config.bak")).unwrap(), "# mine\r\nfont-size = 13\r\n");
        assert_eq!(f.write("theme", Some("x")).unwrap().backup, None, "once per run");
        let mut g = ConfigFile::new(path.clone());
        let b = g.write("theme", Some("y")).unwrap().backup.unwrap();
        assert!(b.file_name().unwrap().to_string_lossy().starts_with("config.bak.20"), "{}", b.display());
        assert_eq!(std::fs::read_to_string(dir.join("config.bak")).unwrap(), "# mine\r\nfont-size = 13\r\n");
        // Changed elsewhere since it was read: the other lines are kept as they are now.
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&path, "# mine\r\nfont-size = 13\r\ntheme = y\r\nnotifications = false\r\n").unwrap();
        let w = g.write("font-size", Some("15")).unwrap();
        assert!(w.changed_outside);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "# mine\r\nfont-size = 15\r\ntheme = y\r\nnotifications = false\r\n"
        );
        // No temporary file is left.
        let left: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp-"))
            .collect();
        assert!(left.is_empty());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            // A link stays a link; the file it points to changes.
            let real = dir.join("real-config");
            std::fs::write(&real, "font-size = 9\n").unwrap();
            let link = dir.join("linked");
            std::os::unix::fs::symlink(&real, &link).unwrap();
            ConfigFile::new(link.clone()).write("font-size", Some("10")).unwrap();
            assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
            assert_eq!(std::fs::read_to_string(&real).unwrap(), "font-size = 10\n");
            // Read-only: not written, the reason in one line.
            std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o444)).unwrap();
            let e = ConfigFile::new(link.clone()).write("font-size", Some("11")).unwrap_err();
            assert!(e.contains("read-only"), "{e}");
            assert_eq!(std::fs::read_to_string(&real).unwrap(), "font-size = 10\n");
            std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o644)).unwrap();
        }
        haetty_testkit::remove_dir_all(&dir);
    }
}
