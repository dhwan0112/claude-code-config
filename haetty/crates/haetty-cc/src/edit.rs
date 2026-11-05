//! Edits made by Claude Code's file tools (after ccx's `apply_edit`): the
//! file is copied before the tool runs (PreToolUse, in the hook process)
//! and the text after is rebuilt from the copy and the tool's input, so the
//! diff shows exactly that edit even if the file changed since.

use haetty_proto::wire::{PreSnapshot, Secret, SnapStatus};

use crate::json::Json;

/// Larger files are not copied, and edits that would exceed it not rebuilt.
pub const MAX_BYTES: u64 = 2 << 20;

pub const EDIT_TOOLS: &[&str] = &["Write", "Edit", "MultiEdit", "NotebookEdit"];

pub fn is_edit_tool(name: &str) -> bool {
    EDIT_TOOLS.contains(&name)
}

/// The file a tool input names.
pub fn target(tool_input: &Json) -> Option<&str> {
    tool_input.get("file_path").or_else(|| tool_input.get("notebook_path")).and_then(Json::str)
}

/// Paths haetty never reads, even when Claude Code edits them: credential,
/// key and token files (DESIGN.md §3.13).
pub fn looks_secret(path: &str) -> bool {
    let p = path.replace('\\', "/").to_ascii_lowercase();
    let name = p.rsplit('/').next().unwrap_or(&p);
    let dirs = ["/.ssh/", "/.gnupg/", "/.aws/", "/.azure/", "/.kube/", "/.docker/", "/.config/gcloud/"];
    let names = [
        ".credentials.json",
        "credentials",
        "credentials.json",
        ".netrc",
        "_netrc",
        ".pgpass",
        ".npmrc",
        ".pypirc",
        ".git-credentials",
        "id_rsa",
        "id_dsa",
        "id_ecdsa",
        "id_ed25519",
    ];
    let exts = [".pem", ".key", ".p12", ".pfx", ".jks", ".keystore", ".gpg", ".asc", ".kdbx"];
    dirs.iter().any(|d| p.contains(d))
        || names.contains(&name)
        || name.starts_with("id_rsa")
        || name.starts_with("id_ed25519")
        || name == ".env"
        || name.starts_with(".env.")
        || exts.iter().any(|e| name.ends_with(e))
        || name.contains("token")
        || name.contains("secret")
}

/// The file before an edit tool runs.
pub fn snapshot(path: &str) -> PreSnapshot {
    let snap = |status, text| PreSnapshot { path: path.to_string(), status, text: Secret(text) };
    if looks_secret(path) {
        return snap(SnapStatus::Skipped, None);
    }
    let md = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return snap(SnapStatus::New, None),
        Err(_) => return snap(SnapStatus::Error, None),
    };
    if md.len() > MAX_BYTES {
        return snap(SnapStatus::TooLarge, None);
    }
    match std::fs::read(path) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(t) => snap(SnapStatus::Copied, Some(t)),
            Err(_) => snap(SnapStatus::Binary, None),
        },
        Err(_) => snap(SnapStatus::Error, None),
    }
}

/// The text before, as far as the snapshot tells: a new file is empty.
pub fn base(s: &PreSnapshot) -> Option<&str> {
    match s.status {
        SnapStatus::New => Some(""),
        SnapStatus::Copied => s.text.0.as_deref(),
        _ => None,
    }
}

/// The text after the tool, rebuilt from `base` and the tool input; `None`
/// when it cannot be known (no base, the old text is not there, notebooks).
pub fn apply_edit(tool: &str, input: &Json, base: Option<&str>) -> Option<String> {
    let after = match tool {
        "Write" => input.get("content").and_then(Json::str).map(str::to_string),
        "Edit" => one_edit(base?, input),
        "MultiEdit" => {
            let mut text = base?.to_string();
            for e in input.get("edits").and_then(Json::arr)? {
                text = one_edit(&text, e)?;
            }
            Some(text)
        }
        _ => None,
    }?;
    (after.len() as u64 <= MAX_BYTES).then_some(after)
}

fn one_edit(base: &str, e: &Json) -> Option<String> {
    let old = e.get("old_string").and_then(Json::str)?;
    let new = e.get("new_string").and_then(Json::str)?;
    let all = e.get("replace_all").and_then(Json::bool).unwrap_or(false);
    if old.is_empty() {
        return base.is_empty().then(|| new.to_string());
    }
    if !base.contains(old) {
        return None;
    }
    Some(if all { base.replace(old, new) } else { base.replacen(old, new, 1) })
}

/// Lines added and removed between two texts.
pub fn line_counts(before: &str, after: &str) -> (u32, u32) {
    let diff = similar::TextDiff::from_lines(before, after);
    let (mut added, mut removed) = (0u32, 0u32);
    for c in diff.iter_all_changes() {
        match c.tag() {
            similar::ChangeTag::Insert => added += 1,
            similar::ChangeTag::Delete => removed += 1,
            similar::ChangeTag::Equal => {}
        }
    }
    (added, removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn j(t: &str) -> Json {
        Json::parse(t).unwrap()
    }

    #[test]
    fn apply_edit_follows_the_tools() {
        assert_eq!(apply_edit("Write", &j(r#"{"content":"x\n"}"#), None).as_deref(), Some("x\n"));
        let e = j(r#"{"old_string":"a","new_string":"b"}"#);
        assert_eq!(apply_edit("Edit", &e, Some("a a")).as_deref(), Some("b a"));
        let all = j(r#"{"old_string":"a","new_string":"b","replace_all":true}"#);
        assert_eq!(apply_edit("Edit", &all, Some("a a")).as_deref(), Some("b b"));
        assert_eq!(apply_edit("Edit", &e, Some("zzz")), None, "old text not there");
        assert_eq!(apply_edit("Edit", &e, None), None, "no base");
        let create = j(r#"{"old_string":"","new_string":"new"}"#);
        assert_eq!(apply_edit("Edit", &create, Some("")).as_deref(), Some("new"));
        assert_eq!(apply_edit("Edit", &create, Some("x")), None);
        let multi = j(r#"{"edits":[{"old_string":"1","new_string":"2"},{"old_string":"2","new_string":"3"}]}"#);
        assert_eq!(apply_edit("MultiEdit", &multi, Some("1")).as_deref(), Some("3"));
        assert_eq!(apply_edit("NotebookEdit", &e, Some("a")), None);
        assert_eq!(line_counts("a\nb\nc\n", "a\nB\nc\nd\n"), (2, 1));
    }

    #[test]
    fn secrets_are_never_read() {
        for p in [
            "/home/a/.claude/.credentials.json",
            "/home/a/.ssh/config",
            r"C:\Users\a\.aws\credentials",
            "/p/.env",
            "/p/.env.local",
            "/p/server.key",
            "/p/cert.PEM",
            "/p/github_token.txt",
            "/p/client_secret.json",
        ] {
            assert!(looks_secret(p), "{p}");
            assert_eq!(snapshot(p).status, SnapStatus::Skipped);
        }
        for p in ["/p/src/main.rs", "/p/.envrc.example2", "/p/keyboard.rs", "/p/README.md"] {
            assert!(!looks_secret(p), "{p}");
        }
    }

    #[test]
    fn snapshot_statuses() {
        let d = std::env::temp_dir().join(format!("haetty-snap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("a.txt");
        assert_eq!(snapshot(f.to_str().unwrap()).status, SnapStatus::New);
        std::fs::write(&f, "hello\n").unwrap();
        let s = snapshot(f.to_str().unwrap());
        assert_eq!((s.status, base(&s)), (SnapStatus::Copied, Some("hello\n")));
        std::fs::write(&f, [0xff, 0xfe, 0]).unwrap();
        assert_eq!(snapshot(f.to_str().unwrap()).status, SnapStatus::Binary);
        std::fs::write(&f, vec![b'a'; (MAX_BYTES + 1) as usize]).unwrap();
        assert_eq!(snapshot(f.to_str().unwrap()).status, SnapStatus::TooLarge);
        std::fs::remove_dir_all(&d).unwrap();
    }
}
