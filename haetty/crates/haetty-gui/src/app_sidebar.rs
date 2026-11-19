//! The window's side of the sidebar (DESIGN.md §13.3, stage PH H2): what it shows of the active tab, clicks and the
//! overlay's keys, an agent's record, and the background reads (a shell tab's git changes, the record) that never run
//! on the UI thread. A child of `app.rs` (it reaches `Win`'s fields); the drawing is `crate::sidebar`.

use std::time::{Duration, Instant};

use crate::agentview::{self, AgentView};
use crate::keymap::Chord;
use crate::sidebar::{self, GitSummary, Hit, Section, WorkerRow};
use crate::ui::Greys;

use super::{now_ms, SidebarEvent, UserEvent, Win};

/// A shell tab's git changes are read again after this long (while it is shown).
const GIT_EVERY: Duration = Duration::from_secs(10);
/// The foreground check of a shell tab (Linux), and the Log's refresh while nothing else asks.
const FG_EVERY: Duration = Duration::from_secs(2);
const LOG_EVERY: Duration = Duration::from_secs(5);
const WORKERS_EVERY: Duration = Duration::from_secs(3);

impl Win {
    /// The sidebar is on screen (docked or over the terminal).
    pub(super) fn sidebar_visible(&self) -> bool {
        self.panel_cols() > 0 || self.overlay_cols() > 0
    }

    /// The sidebar has the keyboard: over a narrow window, or docked after Ctrl+Shift+E.
    pub(super) fn sidebar_keys(&self) -> bool {
        self.diff.is_none()
            && self.agent_view.is_none()
            && (self.overlay_cols() > 0 || (self.sidebar_focus && self.panel_cols() > 0))
    }

    /// Ctrl+Shift+E: the docked sidebar takes the keyboard, or gives it back; without a docked sidebar it opens over
    /// the terminal (which has the keyboard while open).
    pub(super) fn focus_sidebar(&mut self) {
        if self.panel_cols() > 0 {
            self.sidebar_focus = !self.sidebar_focus;
            self.sidebar_sel = Some(self.sidebar_sel.unwrap_or(0).min(self.sidebar_hits.len().saturating_sub(1)));
        } else {
            self.sidebar_overlay = !self.sidebar_overlay;
            self.sidebar_sel = self.sidebar_overlay.then_some(0);
        }
        self.full = true;
        self.recompose();
    }

    pub(super) fn save_ui(&self) {
        let st =
            super::UiState { shown: self.sidebar_shown, width: Some(self.panel_width), folded: self.folded.clone() };
        super::save_ui_state(&st);
    }

    /// The sidebar's rows for the active tab and what its rows do.
    pub(super) fn sidebar_rows(
        &self,
        w: usize,
        h: usize,
        g: Greys,
    ) -> (Vec<Vec<haetty_core::Cell>>, Vec<(usize, Hit)>) {
        let tab = self.tab();
        let home = std::env::var(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).unwrap_or_default();
        let folder = tab.folder().unwrap_or_default();
        let folder = match folder.strip_prefix(home.as_str()) {
            Some(rest) if !home.is_empty() => format!("~{rest}"),
            _ => folder,
        };
        let branch = tab
            .git_name
            .as_deref()
            .and_then(|n| n.split_once(':').map(|x| x.1.to_string()))
            .or_else(|| tab.git_branch.clone());
        let workers = self.worker_rows();
        let hidden: Vec<Section> = [
            (Section::Agents, self.cfg.sidebar_agents),
            (Section::Files, self.cfg.sidebar_files),
            (Section::Workers, self.cfg.sidebar_workers),
            (Section::Log, self.cfg.sidebar_log),
        ]
        .into_iter()
        .filter(|(_, on)| !on)
        .map(|(s, _)| s)
        .collect();
        let tokens = self.token_line_of(tab);
        let m = sidebar::Model {
            claude: tab.claude.as_deref(),
            root: tab.folder().unwrap_or_default(),
            folder,
            branch,
            git: tab.git_sum.as_ref().filter(|_| tab.claude.is_none()),
            workers: &workers,
            log: &tab.log,
            unintegrated: tab.unintegrated && tab.claude.is_none(),
            folded: &self.folded,
            hidden: &hidden,
            tokens: tokens.as_deref(),
            now_ms: now_ms(),
            selected: self.sidebar_sel.filter(|_| self.sidebar_keys()),
        };
        let (mut rows, hits) = sidebar::draw(&m, w, h, g);
        if let Some(shown) = self.diff_shown() {
            for (y, hit) in &hits {
                let sidebar::Hit::File(p) = hit else { continue };
                if !super::same_file(p, &shown) {
                    continue;
                }
                if let Some(row) = rows.get_mut(*y) {
                    mark_row(row, g);
                }
            }
        }
        (rows, hits)
    }

    /// The Workers section: the orchestrator's tasks, each with the agents running in its worker tab.
    fn worker_rows(&self) -> Vec<WorkerRow> {
        let lines = self.worker_lines();
        self.tasks
            .iter()
            .zip(lines)
            .map(|(t, l)| {
                let tab = self.worker_tab(&t.id);
                WorkerRow {
                    text: l.text,
                    agents: tab
                        .and_then(|x| x.claude.as_ref())
                        .map_or(0, |c| c.workers.iter().filter(|a| a.ended_ms.is_none()).count()),
                    session: tab
                        .map(|x| x.session.id.clone())
                        .or_else(|| self.detached_worker(t).map(|s| s.id.clone()))
                        .unwrap_or_default(),
                    blockers: l.blockers,
                    next: l.next,
                    tokens: l.tokens,
                }
            })
            .collect()
    }

    /// "All agents" (stage PH, H3): every tab's agents in one palette list — the tabs that wait for the person first,
    /// the longest-waiting one at the top, then the others in tab order; each tab's agents running first. Enter goes to
    /// the line's tab. It replaces P2's "key to the longest-waiting worker".
    pub(super) fn open_all_agents(&mut self) {
        let now = Instant::now();
        let mut order: Vec<usize> = (0..self.tabs.len()).filter(|&i| self.tabs[i].exited.is_none()).collect();
        order.sort_by_key(|&i| (self.tabs[i].waiting_since.is_none(), self.tabs[i].waiting_since, i));
        let (mut items, mut pick) = (vec![], vec![]);
        let ms = now_ms();
        for i in order {
            let t = &self.tabs[i];
            let Some(c) = t.claude.as_ref() else { continue };
            let name = t.name();
            let waiting = match (t.attention(), t.waiting_since) {
                (haetty_proto::wire::Attention::Ask, Some(w)) => {
                    format!("asks · {}", sidebar::age(now.duration_since(w).as_millis() as u64))
                }
                (haetty_proto::wire::Attention::Wait, Some(w)) => {
                    format!("waits · {}", sidebar::age(now.duration_since(w).as_millis() as u64))
                }
                _ if c.busy => "working".into(),
                _ => String::new(),
            };
            let tree = haetty_cc::agents::tree(&c.workers);
            let shown: Vec<_> = tree
                .iter()
                .filter(|a| a.ended_ms.is_none())
                .chain(tree.iter().filter(|a| a.ended_ms.is_some()).take(3))
                .collect();
            if shown.is_empty() {
                items.push((format!("{name} · no agents"), waiting.clone()));
                pick.push(i);
            }
            for a in shown {
                let glyph = if a.asking {
                    "?"
                } else if a.ended_ms.is_some() {
                    "✓"
                } else if a.background {
                    "◌"
                } else {
                    "●"
                };
                let el = sidebar::age(a.ended_ms.unwrap_or(ms).saturating_sub(a.started_ms));
                let what = if a.ended_ms.is_none() && !a.tool.is_empty() {
                    format!(" {} {}", a.tool, a.target)
                } else {
                    String::new()
                };
                let indent = "  ".repeat(a.depth.max(1) as usize - 1);
                items.push((
                    format!(
                        "{name} · {indent}{glyph} {} {}{what}",
                        if a.kind.is_empty() { "agent" } else { &a.kind },
                        sidebar::short_model(&a.model)
                    ),
                    if waiting.is_empty() { el } else { format!("{waiting} · {el}") },
                ));
                pick.push(i);
            }
        }
        self.agent_pick = pick;
        self.palette = Some(crate::ui::Palette::agents(items));
        self.recompose();
    }

    /// The sidebar's column range on screen (docked or over the terminal), if shown.
    fn sidebar_cols(&self) -> Option<(u16, u16)> {
        let w = self.panel_cols().max(self.overlay_cols());
        (w > 0).then(|| (self.grid.cols - w, self.grid.cols))
    }

    /// A left press: on the sidebar it is the sidebar's (a fold, an agent's record, a worker's tab, a file's diff)
    /// and never reaches the program. True when taken.
    pub(super) fn sidebar_click(&mut self) -> bool {
        let Some((x0, _)) = self.sidebar_cols() else { return false };
        let (col, row) = self.layout.cell_at(self.origin(), self.mouse.x, self.mouse.y, self.grid);
        if col < x0 || row < self.top() {
            return false;
        }
        let y = (row - self.top()) as usize;
        if let Some((_, hit)) = self.sidebar_hits.iter().find(|(r, _)| *r == y).cloned() {
            self.sidebar_act(hit);
        }
        true
    }

    pub(super) fn sidebar_act(&mut self, hit: Hit) {
        match hit {
            Hit::Fold(s) => {
                if let Some(i) = self.folded.iter().position(|x| *x == s) {
                    self.folded.remove(i);
                } else {
                    self.folded.push(s);
                }
                self.save_ui();
            }
            Hit::Agent(id) => {
                self.sidebar_focus = false;
                self.open_agent(&id);
            }
            Hit::Worker(sid) => {
                if let Some(s) = self.detached_workers.iter().find(|s| s.id == sid).cloned() {
                    if let Err(e) = self.reattach(&s) {
                        self.show_hint(format!("worker not opened: {e}"));
                    }
                    self.detached_workers.retain(|x| x.id != sid);
                }
                if let Some(i) = self.tabs.iter().position(|t| t.session.id == sid) {
                    self.sidebar_overlay = false;
                    self.sidebar_focus = false;
                    self.select(i);
                }
            }
            Hit::File(path) => {
                let full = self.full_path(&path);
                self.sidebar_overlay = false;
                self.sidebar_focus = false;
                self.open_file_diff(&full);
            }
        }
        self.full = true;
        self.recompose();
    }

    /// The sidebar's keys while it has the keyboard (over a narrow window, or docked after Ctrl+Shift+E): ↑/↓ select a
    /// row, Enter acts, ←/→ fold, Esc closes the overlay or gives the keyboard back. Every other key is taken too.
    pub(super) fn sidebar_key(&mut self, chord: &Chord) -> bool {
        if !self.sidebar_keys() {
            return false;
        }
        let n = self.sidebar_hits.len();
        let sel = self.sidebar_sel.unwrap_or(0).min(n.saturating_sub(1));
        let plain = !(chord.ctrl || chord.alt || chord.sup);
        match chord.key.as_str() {
            _ if !plain => {}
            "escape" => {
                if self.overlay_cols() > 0 {
                    self.sidebar_overlay = false;
                } else {
                    self.sidebar_focus = false;
                }
                self.full = true;
            }
            "down" | "j" => self.sidebar_sel = Some((sel + 1).min(n.saturating_sub(1))),
            "up" | "k" => self.sidebar_sel = Some(sel.saturating_sub(1)),
            "enter" => {
                if let Some((_, h)) = self.sidebar_hits.get(sel).cloned() {
                    self.sidebar_act(h);
                }
            }
            "left" | "right" => {
                // Fold the selected section title (or the section the selected row belongs to).
                let y = self.sidebar_hits.get(sel).map_or(0, |(y, _)| *y);
                let title = self
                    .sidebar_hits
                    .iter()
                    .rev()
                    .find(|(r, h)| *r <= y && matches!(h, Hit::Fold(_)))
                    .map(|(_, h)| h.clone());
                if let Some(Hit::Fold(s)) = title {
                    let folded = self.folded.contains(&s);
                    if folded == (chord.key == "right") {
                        self.sidebar_act(Hit::Fold(s));
                    }
                }
            }
            _ => {}
        }
        self.recompose();
        true
    }

    /// Opens agent `id`'s record over the terminal; the transcript is read on a thread of its own.
    pub(super) fn open_agent(&mut self, id: &str) {
        let Some(info) = self.tab().claude.clone() else { return };
        let Some(agent) = info.workers.iter().find(|a| a.id == id).cloned() else { return };
        let path = agentview::transcript_of(&info.transcript, id);
        let note = match &path {
            Some(_) => Some("reading the record…".to_string()),
            None => Some("no record: the agent's transcript is not known yet".to_string()),
        };
        self.agent_view = Some(AgentView { agent, record: None, note, scroll: 0 });
        self.sidebar_overlay = false;
        if let Some(p) = path {
            self.read_record(id.to_string(), p);
        }
        self.full = true;
        self.recompose();
    }

    fn read_record(&self, id: String, path: std::path::PathBuf) {
        let proxy = self.proxy.clone();
        let _ = std::thread::Builder::new().name("haetty-record".into()).spawn(move || {
            let r = agentview::read(&path).map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => "no record yet (Claude Code writes it a little later)".to_string(),
                _ => format!("the record could not be read: {e}"),
            });
            let _ = proxy.send_event(UserEvent::Sidebar(SidebarEvent::Record(id, r)));
        });
    }

    /// A key while an agent's record is shown: nothing reaches the program. True when taken.
    pub(super) fn agent_key(&mut self, chord: &Chord) -> bool {
        let area = self.grid.rows.saturating_sub(self.top() + self.status_rows()) as usize;
        let w = (self.grid.cols - self.panel_cols()) as usize;
        let Some(v) = self.agent_view.as_mut() else { return false };
        let total = v.lines(w.saturating_sub(4), now_ms()).len();
        match v.key(&chord.key, chord.shift, area, total) {
            agentview::Key::Close => self.agent_view = None,
            agentview::Key::Refresh => {
                let info = self.tab().claude.clone();
                if let Some(v) = self.agent_view.as_mut() {
                    if let Some(a) = info.as_ref().and_then(|i| i.workers.iter().find(|a| a.id == v.agent.id)) {
                        v.agent = a.clone();
                    }
                    let id = v.agent.id.clone();
                    if let Some(p) = info.as_ref().and_then(|i| agentview::transcript_of(&i.transcript, &id)) {
                        self.read_record(id, p);
                    }
                }
            }
            agentview::Key::Stay => {}
        }
        self.full = true;
        self.recompose();
        true
    }

    pub(super) fn sidebar_event(&mut self, ev: SidebarEvent) {
        match ev {
            SidebarEvent::Record(id, r) => {
                if let Some(v) = self.agent_view.as_mut().filter(|v| v.agent.id == id) {
                    match r {
                        Ok(rec) => {
                            v.record = Some(rec);
                            v.note = None;
                        }
                        Err(e) => v.note = Some(e),
                    }
                }
            }
            SidebarEvent::Git(sid, sum, branch) => {
                if let Some(t) = self.tabs.iter_mut().find(|t| t.session.id == sid) {
                    t.git_sum = Some(sum);
                    t.git_branch = branch;
                    t.git_sum_busy = false;
                }
            }
        }
        self.recompose();
    }

    /// Timers of the sidebar: the elapsed times of running agents (a redraw each second; the damage tracking redraws
    /// only the rows that changed), a shell tab's git changes and foreground check, the Log. The next wake, if any.
    /// The task records again (a coordinator may have made one with `haetty task new`) and the worker sessions no
    /// window shows; true when either changed.
    pub(super) fn refresh_workers(&mut self) -> bool {
        let tasks = crate::tasks::load_all(&crate::tasks::dir());
        let live = if tasks.is_empty() { vec![] } else { self.client.list_sessions().unwrap_or_default() };
        let detached: Vec<haetty_proto::SessionInfo> = live
            .into_iter()
            .filter(|s| s.attached == 0 && s.state == haetty_proto::SessionState::Running)
            .filter(|s| self.tabs.iter().all(|t| t.session.id != s.id))
            .filter(|s| tasks.iter().any(|t| t.is_folder(&super::osc7_path(&s.cwd))))
            .collect();
        let changed = tasks != self.tasks || detached != self.detached_workers;
        self.tasks = tasks;
        self.detached_workers = detached;
        changed
    }

    pub(super) fn sidebar_tick(&mut self, now: Instant) -> Option<Instant> {
        if !self.sidebar_visible() || self.tabs.is_empty() {
            return None;
        }
        let mut next: Option<Instant> = None;
        let mut redraw = false;
        if self.cfg.sidebar_workers {
            if self.workers_at.is_none_or(|t| now.duration_since(t) >= WORKERS_EVERY) {
                self.workers_at = Some(now);
                redraw |= self.refresh_workers();
            }
            next = min(next, self.workers_at.map(|t| t + WORKERS_EVERY));
        }
        let i = self.active;
        // Shell tab: git changes (off the UI thread) and whether a claude runs without the integration.
        if self.tabs[i].claude.is_none() && !haetty_cc::settings::is_claude(&self.tabs[i].spec.program) {
            let due = self.tabs[i].git_sum_at.is_none_or(|t| now.duration_since(t) >= GIT_EVERY);
            if due && !self.tabs[i].git_sum_busy {
                if let Some(dir) = self.tabs[i].folder() {
                    self.tabs[i].git_sum_busy = true;
                    self.tabs[i].git_sum_at = Some(now);
                    let (proxy, sid) = (self.proxy.clone(), self.tabs[i].session.id.clone());
                    let _ = std::thread::Builder::new().name("haetty-gitsum".into()).spawn(move || {
                        let (sum, branch) = git_summary(std::path::Path::new(&dir));
                        let _ = proxy.send_event(UserEvent::Sidebar(SidebarEvent::Git(sid, sum, branch)));
                    });
                }
            }
            next = min(next, self.tabs[i].git_sum_at.map(|t| t + GIT_EVERY));
            if self.tabs[i].fg_at.is_none_or(|t| now.duration_since(t) >= FG_EVERY) {
                self.tabs[i].fg_at = Some(now);
                let fg = self.tabs[i].session.handle.pid().is_some_and(crate::fgproc::claude_in_foreground);
                if fg != self.tabs[i].unintegrated {
                    self.tabs[i].unintegrated = fg;
                    redraw = true;
                }
            }
            if cfg!(target_os = "linux") {
                next = min(next, self.tabs[i].fg_at.map(|t| t + FG_EVERY));
            }
        }
        // The Log: asked when the tab's Claude Code state changed (`log_at` cleared), else now and then.
        if self.cfg.sidebar_log && self.tabs[i].log_at.is_none_or(|t| now.duration_since(t) >= LOG_EVERY) {
            self.tabs[i].log_at = Some(now);
            if let Ok(l) = self.client.session_log(&self.tabs[i].session.id) {
                if l != self.tabs[i].log {
                    self.tabs[i].log = l;
                    redraw = true;
                }
            }
        }
        if self.cfg.sidebar_log {
            next = min(next, self.tabs[i].log_at.map(|t| t + LOG_EVERY));
        }
        // Running agents: their elapsed times move each second.
        let running = self.tabs[i].claude.as_ref().is_some_and(|c| c.workers.iter().any(|a| a.ended_ms.is_none()));
        if running && self.cfg.sidebar_agents {
            let due = self.sidebar_second.is_none_or(|t| now >= t);
            if due {
                redraw = true;
                self.sidebar_second = Some(now + Duration::from_secs(1));
            }
            next = min(next, self.sidebar_second);
        } else {
            self.sidebar_second = None;
        }
        if redraw {
            self.recompose();
        }
        next
    }
}

fn min(a: Option<Instant>, b: Option<Instant>) -> Option<Instant> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.min(y)),
        (x, y) => x.or(y),
    }
}

/// A folder's git changes against HEAD (numstat; untracked files counted) and its branch, with `gitview`'s limits.
pub(super) fn git_summary(dir: &std::path::Path) -> (GitSummary, Option<String>) {
    let branch = crate::gitview::repo_branch(dir).and_then(|n| n.split_once(':').map(|x| x.1.to_string()));
    let Some((top, _)) = crate::gitview::find_top(dir) else { return (GitSummary::default(), branch) };
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut sum = GitSummary::default();
    if let Ok(out) = crate::gitview::run_git(&top, &["diff", "--numstat", "HEAD", "--"], deadline) {
        for line in String::from_utf8_lossy(&out).lines().take(500) {
            let mut f = line.splitn(3, '\t');
            let (a, r, p) = (f.next().unwrap_or("-"), f.next().unwrap_or("-"), f.next().unwrap_or(""));
            if !p.is_empty() {
                sum.files.push((p.to_string(), a.parse().unwrap_or(0), r.parse().unwrap_or(0)));
            }
        }
    }
    if let Ok(out) = crate::gitview::run_git(&top, &["ls-files", "--others", "--exclude-standard"], deadline) {
        sum.untracked = out.split(|b| *b == b'\n').filter(|l| !l.is_empty()).count();
    }
    (sum, branch)
}

fn mark_row(row: &mut [haetty_core::Cell], g: Greys) {
    for c in row.iter_mut().skip(2) {
        if !c.text.is_empty() && c.fg == Some(g.normal) {
            c.fg = Some(g.strong);
            c.flags.0 |= haetty_core::CellFlags::BOLD;
        }
    }
    if let Some(c) = row.get_mut(1) {
        *c = haetty_core::Cell { text: "›".into(), fg: Some(g.strong), bg: Some(g.bg), ..Default::default() };
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn git_summary_counts_changes_and_new_files() {
        let dir = haetty_testkit::run_dir("sidebar-git");
        let sh = |args: &[&str]| {
            std::process::Command::new("git").current_dir(&dir).args(args).output().unwrap();
        };
        std::fs::create_dir_all(&dir).unwrap();
        sh(&["init", "-q", "-b", "work"]);
        std::fs::write(dir.join("a.txt"), "1\n2\n").unwrap();
        sh(&["add", "-A"]);
        sh(&["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "init"]);
        std::fs::write(dir.join("a.txt"), "1\n3\n4\n").unwrap();
        std::fs::write(dir.join("new.txt"), "x\n").unwrap();
        let (sum, branch) = super::git_summary(&dir);
        assert_eq!(branch.as_deref(), Some("work"));
        assert_eq!(sum.files, [("a.txt".to_string(), 2, 1)]);
        assert_eq!(sum.untracked, 1);
        haetty_testkit::remove_dir_all(&dir);
    }

    #[test]
    fn the_shown_file_row_is_marked() {
        use crate::ui::{put, Greys};
        use haetty_core::{Cell, Rgb};
        let g = Greys::from(Rgb { r: 220, g: 220, b: 220 }, Rgb { r: 30, g: 30, b: 30 });
        let mut row = vec![Cell::default(); 12];
        put(&mut row, 2, "a.rs", g.normal, g.bg, false);
        super::mark_row(&mut row, g);
        assert_eq!(row[1].text, "›");
        assert_eq!(row[2].fg, Some(g.strong));
        assert_eq!(row[6].text, "");
    }
}
