//! Tab sets (after ccx's `savetabs.sh`): the window's tabs as a file in
//! the state directory, `tabs/<NAME>.tsv`. One line per tab in order:
//! `name  cwd  program  args  claude_session_id  active`, tab-separated;
//! args are joined with U+001F; tabs, newlines and backslashes in fields are
//! escaped. A `#` header line says what the file is.
//!
//! `last` follows the open tabs; when a window starts, `last` becomes
//! `previous` and its tabs are reopened after the requested one (config
//! `restore-tabs = false` or `--restore-tabs false` turns that off).
//! Environment variables are never stored (DESIGN.md §3.6).

use std::io;
use std::path::{Path, PathBuf};

use ccterm_proto::wire::{SessionInfo, SessionState};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedTab {
    pub name: String,
    pub cwd: String,
    pub program: String,
    pub args: Vec<String>,
    /// Claude Code session id (UUID), for `--resume` (milestone 3).
    pub claude_session: String,
    pub active: bool,
}

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\t', "\\t").replace('\n', "\\n").replace('\r', "\\r")
}

fn unesc(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some(o) => out.push(o),
            None => out.push('\\'),
        }
    }
    out
}

pub fn dir() -> PathBuf {
    ccterm_proto::paths::state_dir().join("tabs")
}

pub fn path(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{name}.tsv"))
}

pub fn format(name: &str, tabs: &[SavedTab]) -> String {
    let mut s = format!(
        "# ccterm tabs \"{name}\": {} tab(s). Columns: name, folder, program, args (U+001F-separated), Claude Code session id, active\n",
        tabs.len()
    );
    for t in tabs {
        let args: Vec<String> = t.args.iter().map(|a| esc(a)).collect();
        s.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\n",
            esc(&t.name),
            esc(&t.cwd),
            esc(&t.program),
            args.join("\u{1f}"),
            esc(&t.claude_session),
            u8::from(t.active)
        ));
    }
    s
}

pub fn parse(text: &str) -> Vec<SavedTab> {
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            if f.len() < 6 || f[2].is_empty() {
                return None;
            }
            Some(SavedTab {
                name: unesc(f[0]),
                cwd: unesc(f[1]),
                program: unesc(f[2]),
                args: if f[3].is_empty() { vec![] } else { f[3].split('\u{1f}').map(unesc).collect() },
                claude_session: unesc(f[4]),
                active: f[5] == "1",
            })
        })
        .collect()
}

/// Writes atomically (temporary file + rename); an unchanged body is not
/// rewritten.
pub fn save(dir: &Path, name: &str, tabs: &[SavedTab]) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let p = path(dir, name);
    let text = format(name, tabs);
    if std::fs::read_to_string(&p).is_ok_and(|old| old == text) {
        return Ok(());
    }
    let tmp = dir.join(format!(".{name}.tsv.tmp"));
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, &p)
}

pub fn load(dir: &Path, name: &str) -> Option<Vec<SavedTab>> {
    std::fs::read_to_string(path(dir, name)).ok().map(|t| parse(&t))
}

fn plain_dir(p: &str) -> &str {
    let p = match p.strip_prefix("file://") {
        Some(r) => r.find('/').map_or(r, |i| &r[i..]),
        None => p,
    };
    p.trim_end_matches('/')
}

/// The saved tabs that are not running in the daemon (DESIGN.md §3.8): a
/// Claude Code tab is there when a live session has its conversation id;
/// any other tab when a live session runs the same program with the same
/// arguments (resume requests aside) in the same folder — each live
/// session counts for one saved tab.
pub fn not_live(saved: &[SavedTab], live: &[SessionInfo]) -> Vec<SavedTab> {
    let mut free: Vec<&SessionInfo> = live.iter().filter(|s| s.state == SessionState::Running).collect();
    let mut out = vec![];
    for t in saved {
        let hit = if !t.claude_session.is_empty() {
            free.iter().position(|s| s.claude_session == t.claude_session)
        } else {
            let args = ccterm_cc::resume::without_resume(&t.args);
            free.iter().position(|s| {
                s.program == t.program
                    && ccterm_cc::resume::without_resume(&s.args) == args
                    && plain_dir(&s.cwd) == plain_dir(&t.cwd)
            })
        };
        match hit {
            Some(i) => {
                free.remove(i);
            }
            None => out.push(t.clone()),
        }
    }
    out
}

/// At start: `last` becomes `previous` (returned) and `last` is removed.
pub fn rotate_last(dir: &Path) -> Vec<SavedTab> {
    let Some(tabs) = load(dir, "last") else { return vec![] };
    if !tabs.is_empty() {
        let _ = save(dir, "previous", &tabs);
    }
    let _ = std::fs::remove_file(path(dir, "last"));
    tabs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(name: &str, active: bool) -> SavedTab {
        SavedTab {
            name: name.into(),
            cwd: "/tmp/a b\tc".into(),
            program: "claude".into(),
            args: vec!["--model".into(), "x\ty".into()],
            claude_session: "0f0fc5c9-9b5a-4538-81fb-def23b2a894b".into(),
            active,
        }
    }

    #[test]
    fn round_trip_escapes_and_rotation() {
        let tabs = vec![
            t("한글 탭", false),
            t("two\\x", true),
            SavedTab { args: vec![], claude_session: String::new(), ..t("three", false) },
        ];
        let text = format("last", &tabs);
        assert!(text.starts_with("# ccterm tabs \"last\": 3 tab(s)"));
        assert_eq!(parse(&text), tabs);
        assert!(parse("# x\nbad line\n\t\t\t\t\t1\n").is_empty());
        let d = std::env::temp_dir().join(format!("ccterm-tabsets-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        save(&d, "last", &tabs).unwrap();
        assert_eq!(rotate_last(&d), tabs);
        assert!(load(&d, "last").is_none());
        assert_eq!(load(&d, "previous").unwrap(), tabs);
        assert!(rotate_last(&d).is_empty());
        std::fs::remove_dir_all(&d).unwrap();
    }

    fn live(program: &str, args: &[&str], cwd: &str, claude: &str, running: bool) -> SessionInfo {
        SessionInfo {
            id: "x".into(),
            program: program.into(),
            title: String::new(),
            cwd: cwd.into(),
            state: if running { SessionState::Running } else { SessionState::Exited { code: Some(0) } },
            created_ms: 0,
            pid: None,
            cols: 80,
            rows: 24,
            args: args.iter().map(|s| s.to_string()).collect(),
            attached: 0,
            claude_session: claude.into(),
            attention: Default::default(),
        }
    }

    #[test]
    fn only_tabs_missing_from_the_daemon_are_restored() {
        let tab = |prog: &str, cwd: &str, sid: &str| SavedTab {
            name: "n".into(),
            cwd: cwd.into(),
            program: prog.into(),
            args: vec![],
            claude_session: sid.into(),
            active: false,
        };
        let saved = vec![
            tab("claude", "/p", "aaa"),
            tab("claude", "/p", "bbb"),
            tab("bash", "/home", ""),
            tab("bash", "/home", ""),
        ];
        let live = vec![
            // Conversation aaa runs (resumed earlier: its args say so).
            live("claude", &["--resume", "aaa"], "file://host/p/", "aaa", true),
            // One bash in /home runs; an exited one does not count.
            live("bash", &[], "file://host/home", "", true),
            live("bash", &[], "/home", "", false),
        ];
        let missing = not_live(&saved, &live);
        assert_eq!(
            missing.iter().map(|t| (t.program.as_str(), t.claude_session.as_str())).collect::<Vec<_>>(),
            [("claude", "bbb"), ("bash", "")]
        );
        assert_eq!(not_live(&saved, &[]).len(), 4);
    }
}
