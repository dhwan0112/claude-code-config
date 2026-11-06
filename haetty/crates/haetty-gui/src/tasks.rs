//! Orchestrator tasks (DESIGN.md §8.1): one file `tasks/<id>.task` per task
//! in the state directory (`key=value` lines, `\`, newline and tab escaped),
//! the worktree made and removed with git commands only.

use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use haetty_cc::orch;

use crate::gitview;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub description: String,
    /// The repository's top-level folder.
    pub repo: String,
    pub base: String,
    pub branch: String,
    pub worktree: String,
    pub created_ms: u64,
}

impl Task {
    pub fn prompt(&self) -> String {
        orch::prompt(&self.id, &self.title, &self.description, &self.branch, &self.base)
    }

    /// Whether `folder` is this task's worktree.
    pub fn is_folder(&self, folder: &str) -> bool {
        let t = |s: &str| s.trim_end_matches(['/', '\\']).to_string();
        !self.worktree.is_empty() && t(folder) == t(&self.worktree)
    }
}

/// A worker's start arguments without the task prompt (it is given once).
pub fn is_prompt_arg(a: &str) -> bool {
    a.starts_with("haetty task ") && a.contains("\nSTATUS: verified | blocked | needs_review\n")
}

pub fn dir() -> PathBuf {
    haetty_proto::paths::state_dir().join("tasks")
}

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\n', "\\n").replace('\t', "\\t").replace('\r', "\\r")
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
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some(o) => out.push(o),
            None => out.push('\\'),
        }
    }
    out
}

pub fn format(t: &Task) -> String {
    let mut s = String::from("# haetty task (DESIGN.md §8)\n");
    for (k, v) in [
        ("id", &t.id),
        ("title", &t.title),
        ("description", &t.description),
        ("repo", &t.repo),
        ("base", &t.base),
        ("branch", &t.branch),
        ("worktree", &t.worktree),
        ("created_ms", &t.created_ms.to_string()),
    ] {
        s.push_str(&format!("{k}={}\n", esc(v)));
    }
    s
}

pub fn parse(text: &str) -> Option<Task> {
    let mut t = Task::default();
    for l in text.lines().filter(|l| !l.starts_with('#')) {
        let Some((k, v)) = l.split_once('=') else { continue };
        let v = unesc(v);
        match k {
            "id" => t.id = v,
            "title" => t.title = v,
            "description" => t.description = v,
            "repo" => t.repo = v,
            "base" => t.base = v,
            "branch" => t.branch = v,
            "worktree" => t.worktree = v,
            "created_ms" => t.created_ms = v.parse().unwrap_or(0),
            _ => {}
        }
    }
    (!t.id.is_empty() && !t.worktree.is_empty()).then_some(t)
}

fn num(id: &str) -> u64 {
    id.strip_prefix('t').and_then(|n| n.parse().ok()).unwrap_or(0)
}

/// Every task, by number.
pub fn load_all(dir: &Path) -> Vec<Task> {
    let mut v: Vec<Task> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "task"))
        .filter_map(|e| std::fs::read_to_string(e.path()).ok().and_then(|s| parse(&s)))
        .collect();
    v.sort_by_key(|t| num(&t.id));
    v
}

pub fn save(dir: &Path, t: &Task) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let p = dir.join(format!("{}.task", t.id));
    let tmp = dir.join(format!(".{}.task.tmp", t.id));
    std::fs::write(&tmp, format(t))?;
    std::fs::rename(tmp, p)
}

fn git(top: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    gitview::run_git(top, args, Instant::now() + Duration::from_secs(10))
}

/// Makes the task: checks, the worktree on a new branch, the file. The
/// worker tab is opened by the caller.
pub fn create(dir: &Path, title: &str, description: &str, repo_dir: &str, base: &str) -> Result<Task, String> {
    let title = title.trim();
    if title.is_empty() {
        return Err("a task needs a title".into());
    }
    let (top, _) = gitview::find_top(Path::new(repo_dir)).ok_or_else(|| format!("{repo_dir}: not a git repository"))?;
    let base = base.trim();
    if base.is_empty() || base.starts_with('-') {
        return Err("no base branch".into());
    }
    git(&top, &["rev-parse", "--verify", "-q", &format!("{base}^{{commit}}")])
        .map_err(|_| format!("{base}: not a branch or commit of {}", top.display()))?;
    let id = format!("t{}", load_all(dir).iter().map(|t| num(&t.id)).max().unwrap_or(0) + 1);
    let slug = orch::slug(title);
    let branch = format!("task/{id}-{slug}");
    if git(&top, &["rev-parse", "--verify", "-q", &format!("refs/heads/{branch}")]).is_ok() {
        return Err(format!("branch {branch} exists"));
    }
    let name = top.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "repo".into());
    let parent = top.parent().ok_or("the repository has no parent folder")?;
    let path = parent.join(format!("{name}-tasks")).join(format!("{id}-{slug}"));
    if path.exists() {
        return Err(format!("{} exists", path.display()));
    }
    std::fs::create_dir_all(path.parent().unwrap_or(parent)).map_err(|e| e.to_string())?;
    let p = path.to_string_lossy().into_owned();
    git(&top, &["worktree", "add", "-q", "-b", &branch, &p, base])?;
    let t = Task {
        id,
        title: title.to_string(),
        description: description.trim().to_string(),
        repo: top.to_string_lossy().into_owned(),
        base: base.to_string(),
        branch,
        worktree: p,
        created_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64),
    };
    save(dir, &t).map_err(|e| e.to_string())?;
    Ok(t)
}

/// The person's cleanup: `git worktree remove` (no `--force`: a worktree
/// with changes is refused), then the task file. The branch stays.
pub fn remove(dir: &Path, t: &Task) -> Result<(), String> {
    if Path::new(&t.worktree).exists() {
        git(Path::new(&t.repo), &["worktree", "remove", &t.worktree])?;
    }
    std::fs::remove_file(dir.join(format!("{}.task", t.id))).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gitview::tests::{repo, sh};

    #[test]
    fn task_files_round_trip() {
        let t = Task {
            id: "t12".into(),
            title: "Fix\tparser".into(),
            description: "two\nlines \\ here".into(),
            repo: "/r".into(),
            base: "main".into(),
            branch: "task/t12-fix-parser".into(),
            worktree: "/r-tasks/t12-fix-parser".into(),
            created_ms: 7,
        };
        assert_eq!(parse(&format(&t)), Some(t.clone()));
        assert!(parse("# x\nid=t1\n").is_none(), "no worktree: not a task");
        assert!(t.is_folder("/r-tasks/t12-fix-parser/") && !t.is_folder("/r-tasks/t1"));
        assert!(is_prompt_arg(&t.prompt()) && !is_prompt_arg("haetty task t1: x"));
    }

    #[test]
    fn create_and_remove_worktrees() {
        let Some(r) = repo("tasks-git") else { return };
        std::fs::write(r.join("a.txt"), "a\n").unwrap();
        assert!(sh(&r, &["add", "."]) && sh(&r, &["commit", "-qm", "one"]));
        let d = r.parent().unwrap().join("state");
        let rs = r.to_str().unwrap();
        assert!(create(&d, " ", "", rs, "main").unwrap_err().contains("title"));
        assert!(create(&d, "x", "", rs, "nope").unwrap_err().contains("not a branch"));
        assert!(create(&d, "x", "", r.parent().unwrap().to_str().unwrap(), "main").unwrap_err().contains("not a git"));
        let t = create(&d, "Fix the parser", "CRLF", &format!("{rs}/sub"), "main").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!((t.id.as_str(), t.branch.as_str()), ("t1", "task/t1-fix-the-parser"));
        assert!(Path::new(&t.worktree).ends_with("repo-tasks/t1-fix-the-parser"), "{}", t.worktree);
        assert_eq!(
            crate::gitview::repo_branch(Path::new(&t.worktree)).as_deref(),
            Some("t1-fix-the-parser:task/t1-fix-the-parser")
        );
        assert_eq!(load_all(&d), vec![t.clone()]);
        let t2 = create(&d, "Fix the parser", "", rs, "main").unwrap();
        assert_eq!(t2.id, "t2");
        // A worktree with changes is not removed (no --force).
        std::fs::write(Path::new(&t.worktree).join("new.txt"), "x").unwrap();
        assert!(remove(&d, &t).is_err());
        assert!(Path::new(&t.worktree).exists() && load_all(&d).len() == 2);
        std::fs::remove_file(Path::new(&t.worktree).join("new.txt")).unwrap();
        remove(&d, &t).unwrap();
        assert!(!Path::new(&t.worktree).exists());
        assert_eq!(load_all(&d), vec![t2]);
        // The branch stays.
        assert!(sh(&r, &["rev-parse", "--verify", "-q", "refs/heads/task/t1-fix-the-parser"]));
    }
}
