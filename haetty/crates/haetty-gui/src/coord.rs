//! Commands for a coordinating session (stage P2b): `haetty workers`, `haetty task new` and `haetty wait worker`.
//! They read the task records and the daemon's session list; `task new` also makes a task and its worker session,
//! which stays detached until the person opens it in the window (DESIGN.md §8.1).

use std::time::{Duration, Instant};

use haetty_cc::orch::WorkerState;
use haetty_proto::paths::Endpoint;
use haetty_proto::wire::{rgb, Attention, CreateSpec, Grid};
use haetty_proto::{Client, ConnectError, Role, SessionInfo, SessionState};

use crate::cli::json_str;
use crate::tasks::{self, Task};

/// The session in the task's worktree: a running one first, then the newest.
pub fn task_session<'a>(t: &Task, list: &'a [SessionInfo]) -> Option<&'a SessionInfo> {
    let mut mine: Vec<&SessionInfo> = list.iter().filter(|s| t.is_folder(&crate::app::osc7_path(&s.cwd))).collect();
    mine.sort_by_key(|s| (s.state != SessionState::Running, std::cmp::Reverse(s.created_ms)));
    mine.first().copied()
}

pub fn task_state(s: Option<&SessionInfo>) -> WorkerState {
    let alive = s.is_some_and(|s| s.state == SessionState::Running);
    WorkerState::of(alive, s.is_some_and(|s| s.busy), s.and_then(|s| s.report.as_ref()))
        .with_background(alive, s.map_or(0, |s| s.background))
}

/// One worker as `haetty workers` and `haetty wait worker` see it.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub id: String,
    pub title: String,
    pub branch: String,
    pub worktree: String,
    pub session: Option<String>,
    pub state: WorkerState,
    pub attention: Attention,
    /// A running worker session that no window shows.
    pub detached: bool,
}

impl Row {
    /// The state word, `asks` while a permission question waits.
    pub fn word(&self) -> &'static str {
        if self.attention == Attention::Ask {
            "asks"
        } else {
            self.state.key()
        }
    }
}

pub fn rows(tasks: &[Task], list: &[SessionInfo]) -> Vec<Row> {
    tasks
        .iter()
        .map(|t| {
            let s = task_session(t, list);
            let running = s.is_some_and(|s| s.state == SessionState::Running);
            Row {
                id: t.id.clone(),
                title: t.title.clone(),
                branch: t.branch.clone(),
                worktree: t.worktree.clone(),
                session: s.map(|s| s.id.clone()),
                state: task_state(s),
                attention: if running { s.map_or(Attention::None, |s| s.attention) } else { Attention::None },
                detached: running && s.is_some_and(|s| s.attached == 0),
            }
        })
        .collect()
}

pub const WORDS: &[&str] = &["running", "verified", "blocked", "needs_review", "no_report", "background", "asks"];

fn text(rows: &[Row]) -> String {
    if rows.is_empty() {
        return "no tasks".into();
    }
    let w = rows.iter().map(|r| r.id.len()).max().unwrap_or(2);
    rows.iter()
        .map(|r| {
            format!(
                "{:<w$}  {:<12}  {:<9}  {}{}",
                r.id,
                r.word(),
                r.session.as_deref().unwrap_or("-"),
                r.title,
                if r.detached { "  (detached)" } else { "" }
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn json(rows: &[Row]) -> String {
    let items: Vec<String> = rows
        .iter()
        .map(|r| {
            format!(
                r#"{{"id":{},"state":{},"session":{},"detached":{},"title":{},"branch":{},"worktree":{}}}"#,
                json_str(&r.id),
                json_str(r.word()),
                r.session.as_deref().map_or("null".into(), json_str),
                r.detached,
                json_str(&r.title),
                json_str(&r.branch),
                json_str(&r.worktree)
            )
        })
        .collect();
    format!("[{}]", items.join(","))
}

fn sessions(name: &str) -> Result<Vec<SessionInfo>, String> {
    let ep = Endpoint::default_for_user().map_err(|e| e.to_string())?;
    match Client::connect(&ep, Role::Observe, name) {
        Ok(c) => c.list_sessions().map_err(|e| e.to_string()),
        Err(ConnectError::NotRunning) => Ok(vec![]),
        Err(e) => Err(e.to_string()),
    }
}

/// `haetty workers [--json]`: one line per task — id, state, session, title.
pub fn workers(as_json: bool) -> (String, i32) {
    match sessions("workers") {
        Ok(list) => {
            let r = rows(&tasks::load_all(&tasks::dir()), &list);
            (if as_json { json(&r) } else { text(&r) }, 0)
        }
        Err(e) => (format!("haetty workers: {e}"), 1),
    }
}

/// What `haetty wait worker` waits for: one task's or any task's state word changes, or reaches one of `until`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkerWait {
    /// `None`: any task.
    pub task: Option<String>,
    pub until: Vec<String>,
}

impl WorkerWait {
    pub fn parse(args: &[&str]) -> Option<WorkerWait> {
        let (task, rest) = match args {
            ["any", rest @ ..] => (None, rest),
            [t, rest @ ..] if !t.starts_with('-') => (Some(t.to_string()), rest),
            _ => return None,
        };
        let until = match rest {
            [] => vec![],
            ["--until", list] => list.split(',').map(str::to_string).collect(),
            _ => return None,
        };
        until.iter().all(|w| WORDS.contains(&w.as_str())).then_some(WorkerWait { task, until })
    }

    pub fn describe(&self) -> String {
        let who = self.task.as_deref().map_or("a worker".to_string(), |t| format!("worker {t}"));
        match self.until.as_slice() {
            [] => format!("{who} changes state"),
            u => format!("{who} is {}", u.join(" or ")),
        }
    }

    /// Looks at the rows once: `Some(line)` when the wait is over. `seen` holds each task's word from the first look.
    pub fn check(&self, rows: &[Row], seen: &mut Vec<(String, &'static str)>) -> Result<Option<String>, String> {
        let watched: Vec<&Row> = rows.iter().filter(|r| self.task.as_ref().is_none_or(|t| *t == r.id)).collect();
        if let (Some(t), true) = (&self.task, watched.is_empty()) {
            return Err(format!("no task {t}"));
        }
        for r in watched {
            let now = r.word();
            if self.until.iter().any(|u| u == now) {
                return Ok(Some(format!("{} is {now}", r.id)));
            }
            match seen.iter_mut().find(|(id, _)| *id == r.id) {
                None => seen.push((r.id.clone(), now)),
                Some((_, was)) if *was != now && self.until.is_empty() => {
                    return Ok(Some(format!("{}: {was} -> {now}", r.id)));
                }
                Some((_, was)) => *was = now,
            }
        }
        Ok(None)
    }
}

/// Waits for `w`, looking once a second; the line to print and the exit code (0, 124 on timeout, 1 on an error).
pub fn wait_worker(w: &WorkerWait, timeout: Duration) -> (String, i32) {
    let t0 = Instant::now();
    let mut seen = vec![];
    loop {
        let list = match sessions("wait") {
            Ok(l) => l,
            Err(e) => return (format!("haetty wait: {e}"), 1),
        };
        match w.check(&rows(&tasks::load_all(&tasks::dir()), &list), &mut seen) {
            Ok(Some(line)) => return (format!("{line} after {:.1} s", t0.elapsed().as_secs_f64()), 0),
            Ok(None) => {}
            Err(e) => return (format!("haetty wait: {e}"), 1),
        }
        let left = timeout.saturating_sub(t0.elapsed());
        if left.is_zero() {
            return (format!("timed out after {} s: {}", timeout.as_secs(), w.describe()), crate::wait::EXIT_TIMEOUT);
        }
        std::thread::sleep(Duration::from_secs(1).min(left));
    }
}

/// `haetty task new`'s answers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NewTask {
    pub draft: tasks::Draft,
    pub brief: Option<String>,
    pub repo: Option<String>,
    pub base: Option<String>,
    pub json: bool,
}

pub const TASK_USAGE: &str = "usage: haetty task new --title TEXT [--brief FILE|-] [--scope TEXT] [--related TEXT] \
     [--model MODEL] [--effort LEVEL] [--repo DIR] [--base BRANCH] [--json]";

pub fn parse_task(args: &[String]) -> Result<NewTask, String> {
    let Some((first, rest)) = args.split_first() else { return Err(TASK_USAGE.into()) };
    if first != "new" {
        return Err(TASK_USAGE.into());
    }
    let mut n = NewTask::default();
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        if a == "--json" {
            n.json = true;
            continue;
        }
        let v = it.next().ok_or_else(|| format!("{a} needs a value\n{TASK_USAGE}"))?.clone();
        match a.as_str() {
            "--title" => n.draft.title = v,
            "--brief" => n.brief = Some(v),
            "--scope" => n.draft.scope = v,
            "--related" => n.draft.related = v,
            "--model" => n.draft.model = v,
            "--effort" => n.draft.effort = v,
            "--repo" => n.repo = Some(v),
            "--base" => n.base = Some(v),
            _ => return Err(TASK_USAGE.into()),
        }
    }
    if n.draft.title.trim().is_empty() {
        return Err(format!("a task needs --title\n{TASK_USAGE}"));
    }
    Ok(n)
}

const MAX_BRIEF: u64 = 64 << 10;

fn read_brief(path: &str) -> Result<String, String> {
    use std::io::Read as _;
    let mut s = String::new();
    let r = if path == "-" {
        std::io::stdin().take(MAX_BRIEF + 1).read_to_string(&mut s)
    } else {
        std::fs::File::open(path).and_then(|f| f.take(MAX_BRIEF + 1).read_to_string(&mut s))
    };
    r.map_err(|e| format!("{path}: {e}"))?;
    if s.len() as u64 > MAX_BRIEF {
        return Err(format!("{path}: longer than {} KiB", MAX_BRIEF >> 10));
    }
    Ok(s)
}

/// The worker's environment: this process's, without a Claude Code parent's markers and without the `claude` proxy of
/// the tab it runs in, so the worker is not a child of the coordinator.
pub fn worker_env(inherited: Vec<(String, String)>, saving: bool) -> Vec<(String, String)> {
    use haetty_cc::proxy;
    let dir = inherited.iter().find(|(k, _)| k == proxy::DIR_ENV).map(|(_, v)| v.clone());
    let drop = [proxy::DIR_ENV, proxy::SETTINGS_ENV, proxy::PROXIED_ENV, haetty_proto::product::SESSION_ENV];
    let env = inherited
        .into_iter()
        .filter(|(k, _)| !drop.contains(&k.as_str()))
        .map(|(k, v)| match (&dir, k.as_str()) {
            (Some(d), "PATH") => {
                let kept: Vec<_> = std::env::split_paths(&v).filter(|p| p.as_os_str() != d.as_str()).collect();
                let joined = std::env::join_paths(kept).map_or(v, |j| j.to_string_lossy().into_owned());
                (k, joined)
            }
            _ => (k, v),
        })
        .collect();
    crate::config::child_env(env, saving)
}

/// `haetty task new`: the task (worktree, branch, record) and its worker session, as the palette's "New task" makes
/// them, but detached: no window changes; the person opens it from the sidebar's Workers or "Attach detached sessions".
/// Never starts a daemon.
pub fn task_new(n: &NewTask) -> (String, i32) {
    match make(n) {
        Ok((t, sid, observable)) => {
            if n.json {
                return (
                    format!(
                        r#"{{"id":{},"branch":{},"worktree":{},"session":{},"observable":{observable}}}"#,
                        json_str(&t.id),
                        json_str(&t.branch),
                        json_str(&t.worktree),
                        json_str(&sid)
                    ),
                    0,
                );
            }
            (
                format!(
                    "{} on {} in {}: session {sid}, detached (the person opens it: the task in the sidebar's Workers, \
                     or \"Attach detached sessions\")",
                    t.id, t.branch, t.worktree
                ),
                0,
            )
        }
        Err(e) => (format!("haetty task: {e}"), 1),
    }
}

fn make(n: &NewTask) -> Result<(Task, String, bool), String> {
    let mut draft = n.draft.clone();
    if let Some(b) = &n.brief {
        draft.description = read_brief(b)?;
    }
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let repo = n.repo.clone().unwrap_or_else(|| cwd.to_string_lossy().into_owned());
    let base = match &n.base {
        Some(b) => b.clone(),
        None => crate::gitview::repo_branch(std::path::Path::new(&repo))
            .and_then(|r| r.split_once(':').map(|x| x.1.to_string()))
            .ok_or_else(|| format!("{repo}: no current branch; give --base"))?,
    };
    let (cfg, _) = crate::config::reread(&[]);
    let words: Vec<String> = cfg.worker_command.split_whitespace().map(str::to_string).collect();
    let (program, base_args) = words.split_first().ok_or("worker-command is empty")?;
    let ep = Endpoint::default_for_user().map_err(|e| e.to_string())?;
    let client = match Client::connect(&ep, Role::Control, "task") {
        Ok(c) => c,
        Err(ConnectError::NotRunning) => return Err("no haetty daemon runs: open haetty first".into()),
        Err(e) => return Err(e.to_string()),
    };
    let t = tasks::create(&tasks::dir(), &draft, &repo, &base)?;
    log::info!("orchestrator: task {} made from the command line (worktree and branch created)", t.id);
    let (theme, _) = cfg.resolve_theme(&haetty_proto::paths::config_dir());
    let inherited: Vec<(String, String)> =
        std::env::vars_os().filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?))).collect();
    let mut args = tasks::worker_args(base_args, &t);
    args.push(t.prompt());
    let spec = CreateSpec {
        program: program.clone(),
        args,
        cwd: Some(t.worktree.clone()),
        env: worker_env(inherited, cfg.saving_mode && haetty_cc::settings::is_claude(program)),
        size: Grid { cols: 120, rows: 36, cell_width: 0, cell_height: 0 },
        scrollback_limit: cfg.scrollback_limit as u64,
        default_fg: rgb(theme.foreground),
        default_bg: rgb(theme.background),
        palette: theme.palette.iter().map(|c| rgb(*c)).collect(),
        cursor_color: theme.cursor_color.map(rgb),
        claude_integration: cfg.claude_integration,
        engine: cfg.engine.name().into(),
    };
    let sid = client.create(spec).map_err(|e| format!("task {} made, worker not started: {e}", t.id))?;
    let observable = cfg.observe_workers && client.set_observable(&sid, true).is_ok();
    client.disconnect();
    Ok((t, sid, observable))
}

#[cfg(test)]
mod tests {
    use super::*;
    use haetty_proto::wire::WorkerReport;

    fn sess(id: &str, cwd: &str, attached: u32) -> SessionInfo {
        SessionInfo {
            id: id.into(),
            program: "claude".into(),
            title: String::new(),
            cwd: cwd.into(),
            state: SessionState::Running,
            created_ms: 1,
            pid: None,
            cols: 80,
            rows: 24,
            args: vec![],
            attached,
            claude_session: String::new(),
            attention: Attention::None,
            busy: false,
            turns: 0,
            background: 0,
            report: None,
            observable: false,
            scrollback: 0,
        }
    }

    fn task(id: &str, wt: &str) -> Task {
        Task { id: id.into(), title: format!("title {id}"), worktree: wt.into(), ..Default::default() }
    }

    #[test]
    fn one_line_per_worker_with_report_spellings() {
        let tasks = [task("t1", "/w/t1"), task("t2", "/w/t2"), task("t3", "/w/t3")];
        let mut a = sess("s-1", "/w/t1", 1);
        a.report = Some(WorkerReport { status: "needs_review".into(), ..Default::default() });
        let mut b = sess("s-2", "/w/t2/", 0);
        b.attention = Attention::Ask;
        let r = rows(&tasks, &[a, b]);
        assert_eq!(r.iter().map(Row::word).collect::<Vec<_>>(), ["needs_review", "asks", "no_report"]);
        assert_eq!((r[1].detached, r[2].session.as_deref()), (true, None));
        let t = text(&r);
        assert_eq!(t.lines().count(), 3);
        assert!(t.lines().nth(1).unwrap().ends_with("title t2  (detached)"), "{t}");
        let j = json(&r);
        assert!(j.contains(r#""id":"t1","state":"needs_review","session":"s-1","detached":false"#), "{j}");
        assert!(j.contains(r#""id":"t3","state":"no_report","session":null"#), "{j}");
    }

    #[test]
    fn a_wait_ends_at_a_change_or_a_named_state() {
        let row = |id: &str, s: WorkerState| Row {
            id: id.into(),
            title: String::new(),
            branch: String::new(),
            worktree: String::new(),
            session: None,
            state: s,
            attention: Attention::None,
            detached: false,
        };
        let any = WorkerWait::parse(&["any"]).unwrap();
        let mut seen = vec![];
        assert_eq!(any.check(&[row("t1", WorkerState::Running)], &mut seen), Ok(None));
        let new = [row("t1", WorkerState::Running), row("t2", WorkerState::NoReport)];
        assert_eq!(any.check(&new, &mut seen), Ok(None), "a new task is not a change");
        let done = [row("t1", WorkerState::Running), row("t2", WorkerState::Background)];
        assert_eq!(any.check(&done, &mut seen), Ok(Some("t2: no_report -> background".into())));
        let until = WorkerWait::parse(&["t1", "--until", "verified,blocked"]).unwrap();
        let mut seen = vec![];
        assert_eq!(until.check(&[row("t1", WorkerState::NeedsReview)], &mut seen), Ok(None));
        assert_eq!(until.check(&[row("t1", WorkerState::Blocked)], &mut seen), Ok(Some("t1 is blocked".into())));
        assert_eq!(until.check(&[], &mut seen), Err("no task t1".into()));
        assert!(WorkerWait::parse(&["t1", "--until", "done"]).is_none());
        assert!(WorkerWait::parse(&[]).is_none());
    }

    #[test]
    fn task_new_arguments() {
        let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let n = parse_task(&v(&["new", "--title", "Fix", "--brief", "-", "--model", "haiku", "--json"])).unwrap();
        assert_eq!(
            (n.draft.title.as_str(), n.brief.as_deref(), n.draft.model.as_str(), n.json),
            ("Fix", Some("-"), "haiku", true)
        );
        assert!(parse_task(&v(&["new"])).is_err());
        assert!(parse_task(&v(&["new", "--title"])).is_err());
        assert!(parse_task(&v(&["list"])).is_err());
        assert!(parse_task(&v(&["new", "--title", "x", "--force", "y"])).is_err());
    }

    #[test]
    fn the_worker_does_not_inherit_the_coordinators_claude() {
        let p = |a: &[&str]| std::env::join_paths(a).unwrap().to_string_lossy().into_owned();
        let env = vec![
            ("PATH".to_string(), p(&["/run/h/bin", "/usr/bin"])),
            (haetty_cc::proxy::DIR_ENV.to_string(), "/run/h/bin".to_string()),
            (haetty_cc::proxy::SETTINGS_ENV.to_string(), "/run/h/settings-1.json".to_string()),
            (haetty_proto::product::SESSION_ENV.to_string(), "s-1".to_string()),
            ("CLAUDECODE".to_string(), "1".to_string()),
            ("LANG".to_string(), "C".to_string()),
        ];
        let out = worker_env(env, false);
        assert_eq!(out, vec![("PATH".to_string(), p(&["/usr/bin"])), ("LANG".to_string(), "C".to_string())]);
    }
}
