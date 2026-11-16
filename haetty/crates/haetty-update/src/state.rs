//! The updater's files in the instance's state folder (`<state>/update/`): when the last check ran and what it
//! found, and the start guard of a freshly installed version (DESIGN.md §3.17).
//!
//! - `check`: `last_ms=`, `failures=`, `latest=` (a newer version found, or empty), `note=` (one line).
//! - `pending`: written when an update is installed: `new=` (the version folder, or `single`), `previous=`,
//!   `starts=`. A start of the new version adds one; staying up for [`CONFIRM_AFTER`] removes the file. A start that
//!   finds a start before it never confirmed rolls back (the new version died within the first seconds).
//! - `note`: the line the panel shows after an automatic rollback, until the person has seen it.

use std::path::{Path, PathBuf};
use std::time::Duration;

/// Between two checks.
pub const EVERY: Duration = Duration::from_secs(24 * 3600);
/// A new version that stays up this long is kept.
pub const CONFIRM_AFTER: Duration = Duration::from_secs(20);

fn read_kv(p: &Path) -> Vec<(String, String)> {
    std::fs::read_to_string(p)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.split_once('=').map(|(k, v)| (k.to_string(), v.to_string())))
        .collect()
}

fn write_kv(p: &Path, kv: &[(&str, String)]) -> std::io::Result<()> {
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    let text: String = kv.iter().map(|(k, v)| format!("{k}={}\n", v.replace(['\n', '\r'], " "))).collect();
    let tmp = p.with_extension("tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(tmp, p)
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Check {
    pub last_ms: u64,
    pub failures: u32,
    pub latest: String,
    pub note: String,
}

impl Check {
    pub fn load(dir: &Path) -> Check {
        let kv = read_kv(&dir.join("check"));
        let get = |k: &str| kv.iter().find(|(x, _)| x == k).map(|(_, v)| v.clone()).unwrap_or_default();
        Check {
            last_ms: get("last_ms").parse().unwrap_or(0),
            failures: get("failures").parse().unwrap_or(0),
            latest: get("latest"),
            note: get("note"),
        }
    }

    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        write_kv(
            &dir.join("check"),
            &[
                ("last_ms", self.last_ms.to_string()),
                ("failures", self.failures.to_string()),
                ("latest", self.latest.clone()),
                ("note", self.note.clone()),
            ],
        )
    }

    /// When the next check is due (ms since the epoch): a day after the last; after failures 2, 4, then 8 days.
    pub fn next_due_ms(&self) -> u64 {
        if self.last_ms == 0 {
            return 0;
        }
        let days = 1u64 << self.failures.min(3);
        self.last_ms + days * EVERY.as_millis() as u64
    }
}

/// What a start of a freshly installed version does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Start {
    /// Not an update's first starts: nothing.
    Normal,
    /// The first start of the new version: confirm it after [`CONFIRM_AFTER`].
    Watch,
    /// The start before this one did not stay up: go back to `previous`.
    RollBack { new: String, previous: String },
}

pub fn pending_path(dir: &Path) -> PathBuf {
    dir.join("pending")
}

/// Written when an update is installed (`new`: the version folder, or `single`).
pub fn set_pending(dir: &Path, new: &str, previous: &str) -> std::io::Result<()> {
    write_kv(&pending_path(dir), &[("new", new.into()), ("previous", previous.into()), ("starts", "0".into())])
}

/// At start, for the version `me` (its folder name, or `single`).
pub fn begin_start(dir: &Path, me: &str) -> Start {
    let p = pending_path(dir);
    let kv = read_kv(&p);
    let get = |k: &str| kv.iter().find(|(x, _)| x == k).map(|(_, v)| v.clone()).unwrap_or_default();
    if kv.is_empty() || get("new") != me {
        return Start::Normal;
    }
    let starts: u32 = get("starts").parse().unwrap_or(0);
    if starts >= 1 {
        let _ = std::fs::remove_file(&p);
        return Start::RollBack { new: get("new"), previous: get("previous") };
    }
    let _ = write_kv(&p, &[("new", get("new")), ("previous", get("previous")), ("starts", (starts + 1).to_string())]);
    Start::Watch
}

/// The new version stayed up: keep it.
pub fn confirm(dir: &Path) {
    let _ = std::fs::remove_file(pending_path(dir));
}

/// The window of version `name` is up (written once the window exists): the restart's watch then knows a quick end
/// was the window closing (its last tab ended), not a version that cannot start.
pub fn mark_up(dir: &Path, name: &str) {
    let _ = write_kv(&dir.join("up"), &[("name", name.into())]);
}

/// Whether version `name` got as far as its window (`mark_up`), and forget the mark.
pub fn take_up(dir: &Path, name: &str) -> bool {
    let p = dir.join("up");
    let up = read_kv(&p).into_iter().any(|(k, v)| k == "name" && v == name);
    let _ = std::fs::remove_file(&p);
    up
}

pub fn set_note(dir: &Path, note: &str) {
    let _ = write_kv(&dir.join("note"), &[("note", note.into())]);
}

/// The rollback note, once.
pub fn take_note(dir: &Path) -> Option<String> {
    let p = dir.join("note");
    let kv = read_kv(&p);
    let _ = std::fs::remove_file(&p);
    kv.into_iter().find(|(k, _)| k == "note").map(|(_, v)| v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_backs_off_and_the_start_guard_rolls_back_a_version_that_died() {
        let d = haetty_testkit::run_dir("update-state");
        let mut c = Check::load(&d);
        assert_eq!(c.next_due_ms(), 0, "never checked: due now");
        c.last_ms = 1000;
        assert_eq!(c.next_due_ms(), 1000 + 86_400_000);
        c.failures = 5;
        assert_eq!(c.next_due_ms(), 1000 + 8 * 86_400_000);
        c.latest = "0.4.0".into();
        c.note = "a\nb".into();
        c.save(&d).unwrap();
        assert_eq!(Check::load(&d), Check { note: "a b".into(), ..c });
        // Start guard: first start watched, a second unconfirmed start rolls back; a confirmed one is normal.
        set_pending(&d, "0.4.0-aaaaaaaaaaaa", "0.3.2-bbbbbbbbbbbb").unwrap();
        assert_eq!(begin_start(&d, "0.3.2-bbbbbbbbbbbb"), Start::Normal);
        assert_eq!(begin_start(&d, "0.4.0-aaaaaaaaaaaa"), Start::Watch);
        assert_eq!(
            begin_start(&d, "0.4.0-aaaaaaaaaaaa"),
            Start::RollBack { new: "0.4.0-aaaaaaaaaaaa".into(), previous: "0.3.2-bbbbbbbbbbbb".into() }
        );
        assert_eq!(begin_start(&d, "0.4.0-aaaaaaaaaaaa"), Start::Normal, "once");
        set_pending(&d, "single", "single").unwrap();
        assert_eq!(begin_start(&d, "single"), Start::Watch);
        confirm(&d);
        assert_eq!(begin_start(&d, "single"), Start::Normal);
        mark_up(&d, "0.4.0-aaaaaaaaaaaa");
        assert!(!take_up(&d, "other"), "another version's mark");
        mark_up(&d, "0.4.0-aaaaaaaaaaaa");
        assert!(take_up(&d, "0.4.0-aaaaaaaaaaaa"));
        assert!(!take_up(&d, "0.4.0-aaaaaaaaaaaa"), "taken once");
        set_note(&d, "rolled back");
        assert_eq!(take_note(&d).as_deref(), Some("rolled back"));
        assert_eq!(take_note(&d), None);
        haetty_testkit::remove_dir_all(&d);
    }
}
