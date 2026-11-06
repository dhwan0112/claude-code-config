//! Git for tab names and the diff view's git views (DESIGN.md §3.13).
//!
//! - [`repo_branch`]: `repo:branch` from the files under `.git` (no
//!   process), for Claude Code tab names.
//! - [`head`] / [`tree`]: the last edited file against `HEAD`, and every
//!   changed and untracked file of the working tree against `HEAD`. `git`
//!   runs as a child with a time limit (killed when over), never with
//!   optional locks (the index is not rewritten while Claude Code works),
//!   and only reads. Files are capped by count, size and total bytes;
//!   credential-like paths are listed, never read.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use haetty_cc::edit::{looks_secret, MAX_BYTES};

/// What one diff view may cost.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Files read and diffed; more are counted, not shown.
    pub files: usize,
    /// Per file (each side), as for Claude Code edits.
    pub file_bytes: u64,
    /// All files together (both sides).
    pub total_bytes: u64,
    /// Wall time for every `git` call together.
    pub time: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { files: 300, file_bytes: MAX_BYTES, total_bytes: 16 << 20, time: Duration::from_secs(3) }
    }
}

/// One file of a git view. `before`/`after` are `None` when not read; `why`
/// then says why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileDiff {
    /// Relative to the top-level folder, `/`-separated.
    pub path: String,
    pub before: Option<String>,
    pub after: Option<String>,
    pub why: Option<&'static str>,
}

#[derive(Debug, Default)]
pub struct Collected {
    pub top: PathBuf,
    pub files: Vec<FileDiff>,
    /// Changed files in all (`files` may hold fewer).
    pub changed: usize,
    /// What a limit cut short.
    pub notes: Vec<String>,
    pub elapsed: Duration,
    /// Bytes read (both sides).
    pub bytes: u64,
}

/// The working tree's top-level folder and its git directory: the nearest
/// `dir` or parent with `.git` (a folder, or a file `gitdir: …` for linked
/// worktrees and submodules).
pub fn find_top(dir: &Path) -> Option<(PathBuf, PathBuf)> {
    for d in dir.ancestors() {
        let g = d.join(".git");
        if g.is_dir() {
            return Some((d.to_path_buf(), g));
        }
        if g.is_file() {
            let text = std::fs::read_to_string(&g).ok()?;
            let p = text.lines().next()?.strip_prefix("gitdir:")?.trim();
            let p = Path::new(p);
            return Some((d.to_path_buf(), if p.is_absolute() { p.to_path_buf() } else { d.join(p) }));
        }
    }
    None
}

/// `repo:branch` for `dir` (repo = the top-level folder's name; a detached
/// `HEAD` shows its first 7 hex digits), `None` outside a repository.
pub fn repo_branch(dir: &Path) -> Option<String> {
    let (top, gitdir) = find_top(dir)?;
    let head = std::fs::read_to_string(gitdir.join("HEAD")).ok()?;
    let head = head.trim();
    let branch = match head.strip_prefix("ref:") {
        Some(r) => r.trim().strip_prefix("refs/heads/").unwrap_or(r.trim()).to_string(),
        None => head.get(..7)?.to_string(),
    };
    let repo = top.file_name()?.to_string_lossy().into_owned();
    Some(format!("{repo}:{branch}"))
}

/// Runs `git -C top ARGS` with `input` on stdin; stdout, at most `cap`
/// bytes, by `deadline` — else the child is killed.
fn git(top: &Path, args: &[&str], input: Option<Vec<u8>>, deadline: Instant, cap: u64) -> Result<Vec<u8>, String> {
    let mut c = Command::new("git");
    c.arg("-C").arg(top).args(["-c", "core.quotepath=off", "--no-pager"]).args(args);
    c.env("GIT_OPTIONAL_LOCKS", "0").env("GIT_TERMINAL_PROMPT", "0");
    c.stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() });
    c.stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = c.spawn().map_err(|e| format!("git: {e}"))?;
    if let (Some(mut stdin), Some(data)) = (child.stdin.take(), input) {
        std::thread::spawn(move || {
            let _ = stdin.write_all(&data);
        });
    }
    let mut out = child.stdout.take().ok_or("git: no output")?;
    // git's reason, for a refusal (first line, at most 4 KiB read).
    let err_reader = child.stderr.take().map(|mut e| {
        std::thread::spawn(move || {
            let mut b = vec![];
            let _ = e.by_ref().take(4096).read_to_end(&mut b);
            String::from_utf8_lossy(&b).lines().next().unwrap_or_default().trim().to_string()
        })
    });
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = vec![];
        let r = out.by_ref().take(cap + 1).read_to_end(&mut buf).map(|_| buf);
        let _ = tx.send(r);
    });
    let left = deadline.saturating_duration_since(Instant::now());
    let r = rx.recv_timeout(left);
    let over = !matches!(r, Ok(Ok(ref b)) if b.len() as u64 <= cap);
    if over {
        let _ = child.kill();
    }
    let status = child.wait().map_err(|e| format!("git: {e}"))?;
    let why = err_reader.and_then(|t| t.join().ok()).unwrap_or_default();
    match r {
        Err(_) => Err("time limit".into()),
        Ok(Err(e)) => Err(format!("git: {e}")),
        Ok(Ok(b)) if b.len() as u64 > cap => Err("size limit".into()),
        Ok(Ok(_)) if !status.success() && !why.is_empty() => Err(format!("git {}: {why}", args[0])),
        Ok(Ok(_)) if !status.success() => Err(format!("git {} failed ({status})", args[0])),
        Ok(Ok(b)) => Ok(b),
    }
}

fn text_of(bytes: Vec<u8>) -> Result<String, &'static str> {
    if bytes.contains(&0) {
        return Err("not text");
    }
    String::from_utf8(bytes).map_err(|_| "not text")
}

/// Reads `top/rel` within the limits.
fn read_worktree(top: &Path, rel: &str, l: &Limits) -> Result<String, &'static str> {
    let p = top.join(rel);
    let md = std::fs::metadata(&p).map_err(|_| "unreadable")?;
    if md.len() > l.file_bytes {
        return Err("larger than 2 MiB");
    }
    text_of(std::fs::read(&p).map_err(|_| "unreadable")?)
}

/// `HEAD` blobs of `paths` (`git cat-file --batch`, one process): `None`
/// for paths not in `HEAD`, `Err` for blobs over the limit or not text.
fn head_blobs(
    top: &Path,
    paths: &[&str],
    l: &Limits,
    deadline: Instant,
) -> Result<Vec<Option<Result<String, &'static str>>>, String> {
    if paths.is_empty() {
        return Ok(vec![]);
    }
    let input: String = paths.iter().map(|p| format!("HEAD:{p}\n")).collect();
    let cap = l.total_bytes + l.file_bytes + 64 * paths.len() as u64;
    let out = git(top, &["cat-file", "--batch"], Some(input.into_bytes()), deadline, cap)?;
    let mut res = vec![];
    let mut at = 0;
    for _ in paths {
        let nl = out[at..].iter().position(|&b| b == b'\n').ok_or("git cat-file: short output")? + at;
        let head = String::from_utf8_lossy(&out[at..nl]).into_owned();
        at = nl + 1;
        if head.ends_with(" missing") || head.ends_with(" ambiguous") {
            res.push(None);
            continue;
        }
        let size: usize = head.rsplit(' ').next().and_then(|s| s.parse().ok()).ok_or("git cat-file: bad header")?;
        let body = out.get(at..at + size).ok_or("git cat-file: short output")?.to_vec();
        at += size + 1;
        res.push(Some(if size as u64 > l.file_bytes { Err("larger than 2 MiB") } else { text_of(body) }));
    }
    Ok(res)
}

/// A git command for the orchestrator's worktrees (output up to 1 MiB).
pub fn run_git(top: &Path, args: &[&str], deadline: Instant) -> Result<Vec<u8>, String> {
    git(top, args, None, deadline, 1 << 20)
}

fn has_head(top: &Path, deadline: Instant) -> bool {
    git(top, &["rev-parse", "--verify", "-q", "HEAD"], None, deadline, 4096).is_ok()
}

/// Paths relative to `top`, `/`-separated.
fn relative(top: &Path, file: &Path) -> Option<String> {
    let r = file.strip_prefix(top).ok()?;
    Some(r.to_string_lossy().replace('\\', "/"))
}

/// The edited `file` against `HEAD` (empty when it is not in `HEAD`).
pub fn head(file: &str, l: &Limits) -> Result<Collected, String> {
    let t0 = Instant::now();
    let deadline = t0 + l.time;
    let path = Path::new(file);
    let dir = path.parent().ok_or("no folder")?;
    let (top, _) = find_top(dir).ok_or("not in a git repository: no HEAD to compare with")?;
    let rel = relative(&top, path).ok_or("the file is outside its repository's folder")?;
    let mut c = Collected { top: top.clone(), changed: 1, ..Default::default() };
    if looks_secret(file) {
        c.files.push(FileDiff { path: rel, before: None, after: None, why: Some("credential-like: never read") });
        c.elapsed = t0.elapsed();
        return Ok(c);
    }
    let after = if path.exists() { read_worktree(&top, &rel, l).map(Some) } else { Ok(Some(String::new())) };
    let before = if has_head(&top, deadline) {
        head_blobs(&top, &[rel.as_str()], l, deadline)?.pop().flatten().unwrap_or(Ok(String::new()))
    } else {
        Ok(String::new())
    };
    let f = match (before, after) {
        (Ok(b), Ok(a)) => {
            c.bytes = (b.len() + a.as_ref().map_or(0, String::len)) as u64;
            FileDiff { path: rel, before: Some(b), after: a, why: None }
        }
        (Err(w), _) | (_, Err(w)) => FileDiff { path: rel, before: None, after: None, why: Some(w) },
    };
    c.files.push(f);
    c.elapsed = t0.elapsed();
    Ok(c)
}

/// Every changed and untracked file under `dir`'s repository against `HEAD`.
pub fn tree(dir: &str, l: &Limits) -> Result<Collected, String> {
    let t0 = Instant::now();
    let deadline = t0 + l.time;
    let (top, _) = find_top(Path::new(dir)).ok_or("not in a git repository")?;
    let status = git(
        &top,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all", "--no-renames", "--ignore-submodules=all"],
        None,
        deadline,
        16 << 20,
    )?;
    let mut entries: Vec<(String, bool)> = vec![]; // (path, untracked)
    for e in status.split(|&b| b == 0).filter(|e| e.len() > 3) {
        let xy = &e[..2];
        let p = String::from_utf8_lossy(&e[3..]).into_owned();
        entries.push((p, xy == b"??"));
    }
    entries.sort();
    let mut c = Collected { top: top.clone(), changed: entries.len(), ..Default::default() };
    if entries.len() > l.files {
        c.notes.push(format!("{} changed files: the first {} are shown", entries.len(), l.files));
        entries.truncate(l.files);
    }
    let head_ok = has_head(&top, deadline);
    // Paths git can read from HEAD (a newline would break --batch).
    let ask: Vec<&str> = entries
        .iter()
        .filter(|(p, u)| !u && head_ok && !looks_secret(p) && !p.contains('\n'))
        .map(|(p, _)| p.as_str())
        .collect();
    let blobs = match head_blobs(&top, &ask, l, deadline) {
        Ok(b) => b,
        Err(e) => {
            c.notes.push(format!("HEAD not read: {e}"));
            vec![]
        }
    };
    let mut blobs = ask.iter().map(|p| p.to_string()).zip(blobs).collect::<std::collections::HashMap<_, _>>();
    for (p, untracked) in entries {
        if looks_secret(&p) {
            c.files.push(FileDiff { path: p, before: None, after: None, why: Some("credential-like: never read") });
            continue;
        }
        if c.bytes > l.total_bytes || Instant::now() > deadline {
            c.files.push(FileDiff { path: p, before: None, after: None, why: Some("over the view's limit") });
            continue;
        }
        let before = if untracked || !head_ok {
            Ok(String::new())
        } else {
            blobs.remove(&p).flatten().unwrap_or(Ok(String::new()))
        };
        let after = if top.join(&p).exists() { read_worktree(&top, &p, l) } else { Ok(String::new()) };
        match (before, after) {
            (Ok(b), Ok(a)) => {
                c.bytes += (b.len() + a.len()) as u64;
                c.files.push(FileDiff { path: p, before: Some(b), after: Some(a), why: None });
            }
            (Err(w), _) | (_, Err(w)) => c.files.push(FileDiff { path: p, before: None, after: None, why: Some(w) }),
        }
    }
    if c.bytes > l.total_bytes {
        c.notes.push(format!("stopped reading at {} MiB", l.total_bytes >> 20));
    }
    if Instant::now() > deadline {
        c.notes.push(format!("time limit {} s reached", l.time.as_secs()));
    }
    c.elapsed = t0.elapsed();
    Ok(c)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A repository in a fresh test folder (`git` with no user or system
    /// config); `None` when git is not installed.
    pub fn repo(name: &str) -> Option<PathBuf> {
        let base = haetty_testkit::run_dir(name).join("repo");
        std::fs::create_dir_all(&base).ok()?;
        let ok = sh(&base, &["init", "-q", "-b", "main"]);
        ok.then_some(base)
    }

    pub fn sh(dir: &Path, args: &[&str]) -> bool {
        Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid", "-c", "commit.gpgsign=false"])
            .args(args)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", if cfg!(windows) { "NUL" } else { "/dev/null" })
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }

    fn write(p: &Path, s: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, s).unwrap();
    }

    #[test]
    fn branch_names_from_git_files() {
        let Some(r) = repo("gv-branch") else { return };
        write(&r.join("a.txt"), "a\n");
        assert!(sh(&r, &["add", "."]) && sh(&r, &["commit", "-qm", "one"]));
        let name = r.file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(repo_branch(&r.join("sub/deeper")), Some(format!("{name}:main")));
        assert!(sh(&r, &["checkout", "-qb", "feat/x"]));
        assert_eq!(repo_branch(&r), Some(format!("{name}:feat/x")));
        // A linked worktree: `.git` is a file.
        let wt = r.parent().unwrap().join("linked");
        assert!(sh(&r, &["worktree", "add", "-q", "-b", "other", wt.to_str().unwrap()]));
        assert_eq!(repo_branch(&wt), Some("linked:other".into()));
        // Detached HEAD: the commit's first 7 hex digits.
        assert!(sh(&r, &["checkout", "-q", "--detach"]));
        let b = repo_branch(&r).unwrap();
        assert!(b.starts_with(&format!("{name}:")) && b.len() == name.len() + 8, "{b}");
        assert_eq!(repo_branch(Path::new("/")), None);
    }

    #[test]
    fn head_and_tree_views_with_limits() {
        let Some(r) = repo("gv-tree") else { return };
        write(&r.join("a.txt"), "one\ntwo\n");
        write(&r.join("gone.txt"), "x\n");
        write(&r.join("bin.dat"), "a\0b");
        assert!(sh(&r, &["add", "."]) && sh(&r, &["commit", "-qm", "one"]));
        write(&r.join("a.txt"), "one\nTWO\n");
        std::fs::remove_file(r.join("gone.txt")).unwrap();
        write(&r.join("new/b.txt"), "new\n");
        write(&r.join(".env"), "KEY=do-not-read\n");
        write(&r.join("bin.dat"), "c\0d");
        let l = Limits::default();
        let h = head(r.join("a.txt").to_str().unwrap(), &l).unwrap();
        assert_eq!(h.files[0].before.as_deref(), Some("one\ntwo\n"));
        assert_eq!(h.files[0].after.as_deref(), Some("one\nTWO\n"));
        let h = head(r.join("new/b.txt").to_str().unwrap(), &l).unwrap();
        assert_eq!((h.files[0].before.as_deref(), h.files[0].after.as_deref()), (Some(""), Some("new\n")));
        let t = tree(r.join("new").to_str().unwrap(), &l).unwrap();
        let by = |p: &str| t.files.iter().find(|f| f.path == p).unwrap().clone();
        assert_eq!(t.changed, 5);
        assert_eq!(by("a.txt").after.as_deref(), Some("one\nTWO\n"));
        assert_eq!((by("gone.txt").before.as_deref(), by("gone.txt").after.as_deref()), (Some("x\n"), Some("")));
        assert_eq!(by("new/b.txt").before.as_deref(), Some(""));
        let env = by(".env");
        assert_eq!((env.before, env.after, env.why), (None, None, Some("credential-like: never read")));
        assert_eq!(by("bin.dat").why, Some("not text"));
        // File count limit; a size limit per file.
        let t = tree(r.to_str().unwrap(), &Limits { files: 2, ..l }).unwrap();
        assert_eq!((t.files.len(), t.changed), (2, 5));
        assert!(t.notes[0].contains("first 2"), "{:?}", t.notes);
        let t = tree(r.to_str().unwrap(), &Limits { file_bytes: 4, ..l }).unwrap();
        assert_eq!(t.files.iter().find(|f| f.path == "a.txt").unwrap().why, Some("larger than 2 MiB"));
        // Outside a repository.
        if find_top(Path::new("/")).is_none() {
            assert!(tree("/", &l).is_err());
        }
    }

    /// Timing of the tree view on a large change (run by hand:
    /// `cargo test -p haetty-gui --bin haetty gitview -- --ignored --nocapture`).
    #[test]
    #[ignore]
    fn measure_tree_view() {
        let Some(r) = repo("gv-measure") else { return };
        let body: String = (0..400).map(|i| format!("line {i} of a source file\n")).collect();
        for i in 0..1500 {
            write(&r.join(format!("src/m{}/f{i}.rs", i % 30)), &body);
        }
        assert!(sh(&r, &["add", "."]) && sh(&r, &["commit", "-qm", "base"]));
        for i in 0..1200 {
            write(&r.join(format!("src/m{}/f{i}.rs", i % 30)), &body.replace("line 7 ", "LINE 7 "));
        }
        for i in 0..300 {
            write(&r.join(format!("new/n{i}.txt")), &body);
        }
        for files in [300, 1500] {
            let l = Limits { files, ..Limits::default() };
            let t = tree(r.to_str().unwrap(), &l).unwrap();
            let shown = t.files.iter().filter(|f| f.before.is_some()).count();
            let v0 = Instant::now();
            let v = crate::diffview::DiffView::git(crate::diffview::Mode::Tree, Ok(t));
            println!(
                "tree view, limit {files}: {} changed, {shown} read, collect {:?}, diff {:?}, {} rows",
                1500,
                v.collect_time(),
                v0.elapsed(),
                v.row_count(80)
            );
        }
    }
}
