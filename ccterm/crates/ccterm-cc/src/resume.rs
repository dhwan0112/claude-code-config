//! Reopening a Claude Code conversation in a restored tab: `--resume ID`
//! when Claude Code still has its transcript (after ccx's savetabs.sh). Only
//! the file's existence is checked; it is never read.

use std::path::PathBuf;

/// Claude Code's config directory: `CLAUDE_CONFIG_DIR`, else `~/.claude`.
fn claude_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("CLAUDE_CONFIG_DIR").filter(|v| !v.is_empty()) {
        return Some(d.into());
    }
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    Some(PathBuf::from(home).join(".claude"))
}

/// Claude Code's project folder name: every character that is not ASCII
/// alphanumeric becomes `-`.
pub fn project_key(cwd: &str) -> String {
    cwd.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
}

pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

/// Whether the conversation `id` started in `cwd` can be resumed.
pub fn transcript_exists(cwd: &str, id: &str) -> bool {
    valid_id(id)
        && claude_dir().is_some_and(|d| d.join("projects").join(project_key(cwd)).join(format!("{id}.jsonl")).is_file())
}

/// Claude Code arguments for a restored tab: the saved ones without an
/// earlier resume/continue request, plus `--resume ID`.
pub fn resume_args(args: &[String], id: &str) -> Vec<String> {
    let mut out = vec![];
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--resume" | "-r" => {
                // Its value, if it has one (not another option).
                if it.as_slice().first().is_some_and(|v| !v.starts_with('-')) {
                    it.next();
                }
            }
            "--continue" | "-c" => {}
            s if s.starts_with("--resume=") => {}
            _ => out.push(a.clone()),
        }
    }
    out.push("--resume".into());
    out.push(id.into());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resume_arguments_and_project_keys() {
        assert_eq!(project_key("/home/a/my proj.x"), "-home-a-my-proj-x");
        assert_eq!(project_key(r"C:\Users\a\p"), "C--Users-a-p");
        let a: Vec<String> =
            ["--model", "opus", "-c", "--resume", "old-id", "--verbose"].iter().map(|s| s.to_string()).collect();
        assert_eq!(resume_args(&a, "abc-1"), ["--model", "opus", "--verbose", "--resume", "abc-1"]);
        assert!(valid_id("0f0fc5c9-9b5a-4538-81fb-def23b2a894b") && !valid_id("../x") && !valid_id(""));
        let d = std::env::temp_dir().join(format!("ccterm-resume-{}", std::process::id()));
        let id = "0f0fc5c9-9b5a-4538-81fb-def23b2a894b";
        let p = d.join("projects").join(project_key("/tmp/p"));
        std::fs::create_dir_all(&p).unwrap();
        std::fs::write(p.join(format!("{id}.jsonl")), "").unwrap();
        // SAFETY of the env change: only this test reads CLAUDE_CONFIG_DIR.
        std::env::set_var("CLAUDE_CONFIG_DIR", &d);
        assert!(transcript_exists("/tmp/p", id));
        assert!(!transcript_exists("/tmp/q", id));
        std::env::remove_var("CLAUDE_CONFIG_DIR");
        std::fs::remove_dir_all(&d).unwrap();
    }
}
