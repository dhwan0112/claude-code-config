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
    /// What the worker may touch (blank: the whole repository).
    pub scope: String,
    /// Files to start from, comma-separated (blank: none named).
    pub related: String,
    /// Claude Code's `--model` (blank: its default).
    pub model: String,
    /// Claude Code's `--effort` (blank: its default).
    pub effort: String,
    /// The repository's top-level folder.
    pub repo: String,
    pub base: String,
    pub branch: String,
    pub worktree: String,
    /// The briefing file the worker reads first: beside the task record, not in the repository.
    pub briefing: String,
    pub created_ms: u64,
}

impl Task {
    pub fn prompt(&self) -> String {
        orch::prompt(&self.id, &self.title, &self.briefing, &self.branch, &self.base)
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
        ("scope", &t.scope),
        ("related", &t.related),
        ("model", &t.model),
        ("effort", &t.effort),
        ("repo", &t.repo),
        ("base", &t.base),
        ("branch", &t.branch),
        ("worktree", &t.worktree),
        ("briefing", &t.briefing),
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
            "scope" => t.scope = v,
            "related" => t.related = v,
            "model" => t.model = v,
            "effort" => t.effort = v,
            "briefing" => t.briefing = v,
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

/// What the person typed for a new task. The optional answers may be blank.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Draft {
    pub title: String,
    pub description: String,
    pub scope: String,
    pub related: String,
    pub model: String,
    pub effort: String,
}

/// A model or effort value, given to Claude Code as the argument of its flag: letters, digits and `. - _ : /`, up to
/// 64 characters, not starting with `-`. Blank is no value.
pub fn flag_value(v: &str) -> Result<String, String> {
    let v = v.trim();
    let ok = v.len() <= 64
        && !v.starts_with('-')
        && v.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':' | '/'));
    if ok {
        Ok(v.to_string())
    } else {
        Err(format!("{v}: use letters, digits and . - _ : / only, not starting with -"))
    }
}

/// The briefing a worker reads first: the title, the scope, the related files and when the task is done. A blank
/// answer says so. The person may edit the file until the worker reads it.
pub fn briefing_text(t: &Task) -> String {
    let or = |s: &str, none: &str| if s.trim().is_empty() { none.to_string() } else { s.trim().to_string() };
    format!(
        "# {title}\n\n\
         Task {id} on branch {branch}, from {base}. Work only in this worktree: {worktree}\n\n\
         ## Scope\n\n{scope}\n\n\
         ## Related files\n\n{related}\n\n\
         ## Done when\n\n{done}\n",
        title = t.title,
        id = t.id,
        branch = t.branch,
        base = t.base,
        worktree = t.worktree,
        scope = or(t.scope.as_str(), "The whole repository, as far as the task needs."),
        related = or(t.related.as_str(), "None named: find them as you need."),
        done = or(
            t.description.as_str(),
            "Not stated: if you cannot tell when it is done, report STATUS: blocked and say what is missing."
        ),
    )
}

/// The launch arguments of a worker: the `worker-command` arguments, with the task's model and effort (when it has
/// them) in place of any the command already sets. They are arguments only: no settings file changes.
pub fn worker_args(base: &[String], t: &Task) -> Vec<String> {
    let set = |flag: &str| match flag {
        "--model" => !t.model.is_empty(),
        "--effort" => !t.effort.is_empty(),
        _ => false,
    };
    let mut out = Vec::new();
    let mut it = base.iter();
    while let Some(a) = it.next() {
        match a.split_once('=') {
            Some((flag, _)) if set(flag) => {}
            Some(_) => out.push(a.clone()),
            None if set(a.as_str()) => {
                it.next();
            }
            None => out.push(a.clone()),
        }
    }
    if !t.model.is_empty() {
        out.extend(["--model".to_string(), t.model.clone()]);
    }
    if !t.effort.is_empty() {
        out.extend(["--effort".to_string(), t.effort.clone()]);
    }
    out
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
/// Makes a task: a branch and a worktree beside the repository, the record, and the briefing file. The model and effort
/// are checked before anything is made.
pub fn create(dir: &Path, draft: &Draft, repo_dir: &str, base: &str) -> Result<Task, String> {
    let title = draft.title.trim();
    if title.is_empty() {
        return Err("a task needs a title".into());
    }
    let model = flag_value(&draft.model).map_err(|e| format!("model: {e}"))?;
    let effort = flag_value(&draft.effort).map_err(|e| format!("effort: {e}"))?;
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
    let briefing = dir.join(format!("{id}.briefing.md")).to_string_lossy().into_owned();
    let t = Task {
        id,
        title: title.to_string(),
        description: draft.description.trim().to_string(),
        scope: draft.scope.trim().to_string(),
        related: draft.related.trim().to_string(),
        model,
        effort,
        repo: top.to_string_lossy().into_owned(),
        base: base.to_string(),
        branch,
        worktree: p,
        briefing,
        created_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64),
    };
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    std::fs::write(&t.briefing, briefing_text(&t)).map_err(|e| e.to_string())?;
    save(dir, &t).map_err(|e| e.to_string())?;
    Ok(t)
}

/// The person's cleanup: `git worktree remove` (no `--force`: a worktree
/// with changes is refused), then the task file. The branch stays.
pub fn remove(dir: &Path, t: &Task) -> Result<(), String> {
    if Path::new(&t.worktree).exists() {
        git(Path::new(&t.repo), &["worktree", "remove", &t.worktree])?;
    }
    if !t.briefing.is_empty() {
        let _ = std::fs::remove_file(&t.briefing);
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
            scope: "parser.rs only".into(),
            related: "src/parser.rs, tests/crlf.rs".into(),
            model: "opus".into(),
            effort: "high".into(),
            repo: "/r".into(),
            base: "main".into(),
            branch: "task/t12-fix-parser".into(),
            worktree: "/r-tasks/t12-fix-parser".into(),
            briefing: "/state/tasks/t12.briefing.md".into(),
            created_ms: 7,
        };
        assert_eq!(parse(&format(&t)), Some(t.clone()));
        assert!(parse("# x\nid=t1\n").is_none(), "no worktree: not a task");
        assert!(t.is_folder("/r-tasks/t12-fix-parser/") && !t.is_folder("/r-tasks/t1"));
        assert!(is_prompt_arg(&t.prompt()) && !is_prompt_arg("haetty task t1: x"));
        assert!(t.prompt().contains("Read the briefing first: /state/tasks/t12.briefing.md\n"));
        // A task file from before the optional answers has them blank.
        let old = parse("id=t3\ntitle=Old\nworktree=/w\n").unwrap();
        assert_eq!((old.scope.as_str(), old.model.as_str(), old.briefing.as_str()), ("", "", ""));
    }

    #[test]
    fn optional_answers_are_checked_and_passed_as_arguments() {
        assert_eq!(flag_value("  claude-opus-5-5 ").unwrap(), "claude-opus-5-5");
        assert_eq!(flag_value("").unwrap(), "");
        assert!(flag_value("--dangerously").is_err());
        assert!(flag_value("opus high").is_err());
        assert!(flag_value("a".repeat(65).as_str()).is_err());
        let mut t = Task::default();
        let base: Vec<String> =
            ["--verbose", "--model", "haiku", "--effort=low"].iter().map(|s| s.to_string()).collect();
        assert_eq!(worker_args(&base, &t), base, "blank answers: the command as it is");
        t.model = "opus".into();
        t.effort = "high".into();
        assert_eq!(worker_args(&base, &t), ["--verbose", "--model", "opus", "--effort", "high"]);
    }

    #[test]
    fn the_briefing_says_what_is_blank() {
        let mut t = Task {
            id: "t4".into(),
            title: "Fix".into(),
            branch: "task/t4-fix".into(),
            base: "main".into(),
            worktree: "/w".into(),
            ..Task::default()
        };
        let text = briefing_text(&t);
        assert!(text.starts_with("# Fix\n") && text.contains("## Scope\n\nThe whole repository"));
        assert!(text.contains("None named") && text.contains("STATUS: blocked"));
        t.scope = " parser.rs only ".into();
        t.description = "cargo test passes".into();
        let text = briefing_text(&t);
        assert!(text.contains("## Scope\n\nparser.rs only\n") && text.contains("## Done when\n\ncargo test passes\n"));
    }

    #[test]
    fn create_and_remove_worktrees() {
        let Some(r) = repo("tasks-git") else { return };
        std::fs::write(r.join("a.txt"), "a\n").unwrap();
        assert!(sh(&r, &["add", "."]) && sh(&r, &["commit", "-qm", "one"]));
        let d = r.parent().unwrap().join("state");
        let rs = r.to_str().unwrap();
        let draft = |title: &str| Draft { title: title.into(), ..Draft::default() };
        assert!(create(&d, &draft(" "), rs, "main").unwrap_err().contains("title"));
        assert!(create(&d, &draft("x"), rs, "nope").unwrap_err().contains("not a branch"));
        let not_git = r.parent().unwrap().to_str().unwrap();
        assert!(create(&d, &draft("x"), not_git, "main").unwrap_err().contains("not a git"));
        let bad = Draft { model: "--x".into(), ..draft("x") };
        assert!(create(&d, &bad, rs, "main").unwrap_err().starts_with("model:"));
        assert!(!d.exists(), "a refused model makes nothing");
        let first = Draft {
            title: "Fix the parser".into(),
            description: "CRLF".into(),
            scope: "parser.rs".into(),
            related: "src/parser.rs".into(),
            model: "opus".into(),
            effort: "high".into(),
        };
        let t = create(&d, &first, &format!("{rs}/sub"), "main").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!((t.id.as_str(), t.branch.as_str()), ("t1", "task/t1-fix-the-parser"));
        assert!(Path::new(&t.worktree).ends_with("repo-tasks/t1-fix-the-parser"), "{}", t.worktree);
        assert_eq!(
            crate::gitview::repo_branch(Path::new(&t.worktree)).as_deref(),
            Some("t1-fix-the-parser:task/t1-fix-the-parser")
        );
        assert_eq!(load_all(&d), vec![t.clone()]);
        let brief = std::fs::read_to_string(&t.briefing).unwrap();
        assert!(brief.contains("## Scope\n\nparser.rs\n") && brief.contains("CRLF"), "{brief}");
        let t2 = create(&d, &draft("Fix the parser"), rs, "main").unwrap();
        assert_eq!(t2.id, "t2");
        // A worktree with changes is not removed (no --force), and its briefing stays with it.
        std::fs::write(Path::new(&t.worktree).join("new.txt"), "x").unwrap();
        assert!(remove(&d, &t).is_err());
        assert!(Path::new(&t.worktree).exists() && load_all(&d).len() == 2 && Path::new(&t.briefing).exists());
        std::fs::remove_file(Path::new(&t.worktree).join("new.txt")).unwrap();
        remove(&d, &t).unwrap();
        assert!(!Path::new(&t.worktree).exists() && !Path::new(&t.briefing).exists());
        assert_eq!(load_all(&d), vec![t2]);
        // The branch stays.
        assert!(sh(&r, &["rev-parse", "--verify", "-q", "refs/heads/task/t1-fix-the-parser"]));
    }
}
