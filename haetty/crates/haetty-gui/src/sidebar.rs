//! The sidebar (DESIGN.md §13.3, stage PH H2; replaces the status panel): on the right, the terminal's own
//! background, a 1 px low-contrast line on its left, no boxes. Sections top to bottom: Lead (session, model and effort,
//! context, limits, cost), Agents, Files, Workers, Log, and a footer (folder and branch, last update). A shell tab:
//! its folder, branch and git change summary, then the same Workers and Log. Greys only; an agent waiting for a
//! permission answer gets the colour-9 `?`, usage at 80 % / 95 % the state colours of §2.2. Nothing blinks.

use haetty_core::{Cell, Rgb};
use haetty_proto::wire::{ClaudeInfo, LogEvent, Worker};

use crate::ui::{blank, fit, fit_left, put, width, Greys};

/// A section that folds (the Lead and the footer do not).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Section {
    Agents,
    Files,
    Workers,
    Log,
}

impl Section {
    pub const ALL: [Section; 4] = [Section::Agents, Section::Files, Section::Workers, Section::Log];

    pub fn name(self) -> &'static str {
        match self {
            Section::Agents => "agents",
            Section::Files => "files",
            Section::Workers => "workers",
            Section::Log => "log",
        }
    }

    pub fn parse(s: &str) -> Option<Section> {
        Section::ALL.into_iter().find(|x| x.name() == s.trim())
    }

    fn title(self) -> &'static str {
        match self {
            Section::Agents => "Agents",
            Section::Files => "Files",
            Section::Workers => "Workers",
            Section::Log => "Log",
        }
    }
}

/// What a click on a sidebar row (or Enter on it in the overlay) does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Hit {
    Fold(Section),
    /// The agent's record (its id).
    Agent(String),
    /// The worker's tab (its session id).
    Worker(String),
    /// The file's diff (its path).
    File(String),
}

/// One orchestrator task (DESIGN.md §8): its line, the agents running in its worker tab, its tab's session.
#[derive(Clone, Debug, Default)]
pub struct WorkerRow {
    pub text: String,
    pub agents: usize,
    pub session: String,
    pub blockers: Option<String>,
    pub next: Option<String>,
    pub tokens: Option<String>,
}

/// A shell tab's git state (collected off the UI thread).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GitSummary {
    /// (path, added, removed) of tracked changes against HEAD.
    pub files: Vec<(String, u64, u64)>,
    pub untracked: usize,
}

pub struct Model<'a> {
    pub claude: Option<&'a ClaudeInfo>,
    /// The tab's folder (`~` for the home folder) and git branch.
    pub folder: String,
    pub branch: Option<String>,
    pub git: Option<&'a GitSummary>,
    pub workers: &'a [WorkerRow],
    pub log: &'a [LogEvent],
    /// `claude` runs in the tab without haetty's integration (stage PH, H1).
    pub unintegrated: bool,
    pub folded: &'a [Section],
    /// Sections the settings turned off.
    pub hidden: &'a [Section],
    pub tokens: Option<&'a str>,
    pub now_ms: u64,
    /// The overlay's keyboard selection: an index into the returned hits.
    pub selected: Option<usize>,
}

/// The docked sidebar's columns (with its line), or 0 (DESIGN.md §13.3): `sidebar = auto`, shown by the person, and a
/// window of at least `min_cols` (and room for 40 terminal columns). Its inputs are the window and the settings only —
/// never a tab or an event — so a tab's terminal size changes only when the person resizes the window or toggles it.
pub fn docked_cols(window_cols: u16, auto: bool, min_cols: u16, width: u16, shown: bool) -> u16 {
    if auto && shown && window_cols >= min_cols.max(width + 40) {
        width.min(window_cols.saturating_sub(40))
    } else {
        0
    }
}

/// Finished agents shown before "+ N earlier".
pub const SHOWN_DONE: usize = 6;

/// The model's short name: `claude-haiku-5-5` → `haiku`.
pub fn short_model(m: &str) -> String {
    let m = m.strip_prefix("claude-").unwrap_or(m);
    m.split(['-', ' ', '[']).next().unwrap_or(m).to_ascii_lowercase()
}

pub fn age(ms: u64) -> String {
    let s = ms / 1000;
    match s {
        s if s >= 3600 => format!("{}h{:02}", s / 3600, s % 3600 / 60),
        s if s >= 60 => format!("{}m{:02}", s / 60, s % 60),
        s => format!("{s}s"),
    }
}

struct Out {
    rows: Vec<Vec<Cell>>,
    hits: Vec<(usize, Hit)>,
    y: usize,
    w: usize,
    g: Greys,
}

impl Out {
    fn room(&self) -> bool {
        self.y < self.rows.len()
    }

    /// One row: `left` at the content's start, `right` right-aligned; `None` when out of rows.
    fn line(&mut self, indent: usize, left: &[(&str, Rgb, bool)], right: Option<(&str, Rgb)>, hit: Option<Hit>) {
        if !self.room() {
            return;
        }
        let x0 = 2 + indent;
        let end = self.w.saturating_sub(1);
        let rw = right.map_or(0, |(t, _)| width(t) + 1);
        let row = &mut self.rows[self.y];
        let mut x = x0;
        for (t, c, b) in left {
            if x >= end.saturating_sub(rw) {
                break;
            }
            let t = fit(t, end.saturating_sub(rw).saturating_sub(x));
            x = put(row, x, &t, *c, self.g.bg, *b);
        }
        if let Some((t, c)) = right {
            let at = end.saturating_sub(width(t)).max(x + 1);
            put(row, at, t, c, self.g.bg, false);
        }
        if let Some(h) = hit {
            self.hits.push((self.y, h));
        }
        self.y += 1;
    }

    fn gap(&mut self) {
        if self.room() && self.y > 0 {
            self.y += 1;
        }
    }
}

/// The sidebar's rows (`w` columns, the first is the line; `h` rows) and what each clickable row does.
pub fn draw(m: &Model, w: usize, h: usize, g: Greys) -> (Vec<Vec<Cell>>, Vec<(usize, Hit)>) {
    let mut rows = vec![vec![blank(g.normal, g.bg); w]; h];
    for r in rows.iter_mut() {
        if let Some(c) = r.first_mut() {
            *c = Cell { text: "│".into(), ..blank(g.bar, g.bg) };
        }
    }
    let mut o = Out { rows, hits: vec![], y: 0, w, g };
    if w < 12 || h == 0 {
        return (o.rows, o.hits);
    }
    // The footer takes the last rows: folder and branch, the last update.
    let footer = usize::from(h >= 8) + usize::from(m.claude.is_some_and(|c| c.updated_ms > 0) && h >= 12);
    let body = h - footer;
    o.rows.truncate(body);
    match m.claude {
        Some(c) => lead(&mut o, c, m),
        None => shell_lead(&mut o, m),
    }
    let on = |s: Section| !m.hidden.contains(&s);
    let folded = |s: Section| m.folded.contains(&s);
    if let Some(c) = m.claude {
        if on(Section::Agents) {
            agents(&mut o, &c.workers, folded(Section::Agents), m.now_ms);
        }
        if on(Section::Files) && !c.files.is_empty() {
            o.gap();
            section_title(&mut o, Section::Files, &format!("{}", c.files.len()), folded(Section::Files));
            if !folded(Section::Files) {
                for f in &c.files {
                    file_row(&mut o, &f.path, f.added, f.removed);
                }
            }
        }
    } else if let Some(gs) = m.git.filter(|gs| !gs.files.is_empty()) {
        if on(Section::Files) {
            o.gap();
            section_title(&mut o, Section::Files, &format!("{}", gs.files.len()), folded(Section::Files));
            if !folded(Section::Files) {
                for (p, a, r) in &gs.files {
                    file_row(&mut o, p, *a, *r);
                }
            }
        }
    }
    if on(Section::Workers) && !m.workers.is_empty() {
        o.gap();
        section_title(&mut o, Section::Workers, &format!("{} task(s)", m.workers.len()), folded(Section::Workers));
        if !folded(Section::Workers) {
            for wr in m.workers {
                let n = if wr.agents > 0 { format!("{} agents", wr.agents) } else { String::new() };
                o.line(0, &[(&wr.text, g.normal, false)], Some((&n, g.dim)), Some(Hit::Worker(wr.session.clone())));
                for (k, v) in [("tokens", &wr.tokens), ("blockers", &wr.blockers), ("next", &wr.next)] {
                    if let Some(v) = v {
                        o.line(2, &[(k, g.dim, false), (" ", g.dim, false), (v, g.normal, false)], None, None);
                    }
                }
            }
        }
    }
    if on(Section::Log) && !m.log.is_empty() && o.y + 3 < body {
        o.gap();
        section_title(&mut o, Section::Log, "", folded(Section::Log));
        if !folded(Section::Log) {
            let room = body.saturating_sub(o.y);
            let lines: Vec<(String, String)> = m.log.iter().filter_map(|e| log_text(e, m.now_ms)).collect();
            for (t, a) in lines.iter().skip(lines.len().saturating_sub(room)) {
                o.line(0, &[(t, g.normal, false)], Some((a, g.dim)), None);
            }
        }
    }
    // The overlay's selection: that row's text in the strongest grey (no accent).
    if let Some((y, _)) = m.selected.and_then(|i| o.hits.get(i)) {
        for c in o.rows[*y].iter_mut().skip(1) {
            if !c.text.is_empty() && c.text != " " {
                c.fg = Some(g.strong);
                c.flags.0 |= haetty_core::CellFlags::UNDERLINE;
            }
        }
    }
    let mut rows = o.rows;
    let hits = o.hits;
    // Footer.
    let mut foot = |text: &[(&str, Rgb)]| {
        let mut r = vec![blank(g.normal, g.bg); w];
        r[0] = Cell { text: "│".into(), ..blank(g.bar, g.bg) };
        let mut x = 2;
        for (t, c) in text {
            x = put(&mut r, x, &fit_left(t, w.saturating_sub(x + 1)), *c, g.bg, false);
        }
        rows.push(r);
    };
    if footer >= 1 {
        let (parent, name) = match m.folder.rfind('/') {
            Some(i) if i + 1 < m.folder.len() => (&m.folder[..=i], &m.folder[i + 1..]),
            _ => ("", m.folder.as_str()),
        };
        let branch = m.branch.as_deref().map(|b| format!(":{b}")).unwrap_or_default();
        foot(&[(parent, g.dim), (name, g.normal), (&branch, g.dim)]);
    }
    if footer >= 2 {
        if let Some(c) = m.claude {
            foot(&[(&format!("updated {} ago", age(m.now_ms.saturating_sub(c.updated_ms))), g.dim)]);
        }
    }
    (rows, hits)
}

fn section_title(o: &mut Out, s: Section, count: &str, folded: bool) {
    let g = o.g;
    let mark = if folded { "▸ " } else { "" };
    o.line(
        0,
        &[(mark, g.dim, false), (s.title(), g.strong, true), ("  ", g.dim, false), (count, g.dim, false)],
        None,
        Some(Hit::Fold(s)),
    );
}

fn file_row(o: &mut Out, path: &str, added: u64, removed: u64) {
    let g = o.g;
    let counts = format!("+{added} -{removed}");
    let room = o.w.saturating_sub(3 + width(&counts) + 1);
    let p = fit_left(path, room);
    o.line(0, &[(&p, g.normal, false)], Some((&counts, g.dim)), Some(Hit::File(path.to_string())));
}

fn lead(o: &mut Out, c: &ClaudeInfo, m: &Model) {
    let g = o.g;
    let name = if c.name.is_empty() { "Claude Code" } else { &c.name };
    let via = if c.via_proxy { "  shell" } else { "" };
    o.line(0, &[(name, g.strong, true), (via, g.dim, false)], None, None);
    let me: Vec<&str> = [c.model.as_str(), c.effort.as_str()].into_iter().filter(|s| !s.is_empty()).collect();
    if !me.is_empty() {
        o.line(0, &[(&me.join(" · "), g.dim, false)], None, None);
    }
    let kv = |o: &mut Out, k: &str, v: &str, col: Rgb| {
        o.line(0, &[(&format!("{k:<9}"), g.dim, false), (v, col, false)], None, None);
    };
    if let Some(p) = c.context_pct {
        let size = c.context_size.map(|s| format!(" of {}", crate::ui::human(s))).unwrap_or_default();
        kv(o, "context", &format!("{}{size}", crate::ui::pct(p).trim_start()), crate::ui::level_color(p, g));
    }
    for l in &c.limits {
        let name = match l.name.as_str() {
            "five_hour" => "5h limit".to_string(),
            "seven_day" => "7d limit".to_string(),
            other => other.replace('_', " "),
        };
        let reset = l.resets_at.map(|t| format!(" · {}", crate::ui::when(t))).unwrap_or_default();
        kv(
            o,
            &name,
            &format!("{}{reset}", crate::ui::pct(l.used_pct).trim_start()),
            crate::ui::level_color(l.used_pct, g),
        );
    }
    if let Some(cost) = c.cost_usd {
        let est = if c.limits.is_empty() { "" } else { " (API estimate)" };
        kv(o, "cost", &format!("${cost:.2}{est}"), g.normal);
    }
    if c.lines_added + c.lines_removed > 0 {
        kv(o, "lines", &format!("+{} -{}", c.lines_added, c.lines_removed), g.normal);
    }
    if let (Some(warm), hit) = (c.cache_warm, c.cache_hit_pct) {
        let hit = hit.map(|h| format!(" · hit {h:.0}%")).unwrap_or_default();
        kv(o, "cache", &format!("{}{hit}", if warm { "warm" } else { "cold" }), g.normal);
    }
    if let Some(t) = m.tokens {
        kv(o, "tokens", t, g.normal);
    }
}

fn shell_lead(o: &mut Out, m: &Model) {
    let g = o.g;
    let name = m.folder.rsplit('/').next().filter(|s| !s.is_empty()).unwrap_or(&m.folder);
    o.line(0, &[(name, g.strong, true)], None, None);
    if let Some(b) = &m.branch {
        o.line(0, &[(b, g.dim, false)], None, None);
    }
    if let Some(gs) = m.git {
        let (a, r) = gs.files.iter().fold((0, 0), |(a, r), (_, x, y)| (a + x, r + y));
        let mut t = if gs.files.is_empty() {
            "no changes".to_string()
        } else {
            format!("{} changed +{a} -{r}", gs.files.len())
        };
        if gs.untracked > 0 {
            t.push_str(&format!(" · {} new", gs.untracked));
        }
        o.line(0, &[(&t, g.normal, false)], None, None);
    }
    if m.unintegrated {
        o.gap();
        o.line(0, &[("claude is running without", g.normal, false)], None, None);
        o.line(0, &[("haetty's integration", g.normal, false)], None, None);
        o.line(0, &[("PATH, an absolute path or an alias", g.dim, false)], None, None);
    }
}

/// The Agents section: a header with the counts, running agents (tree order), the newest finished, "+ N earlier".
fn agents(o: &mut Out, w: &[Worker], folded: bool, now_ms: u64) {
    let g = o.g;
    let tree = haetty_cc::agents::tree(w);
    let running: Vec<&Worker> = tree.iter().copied().filter(|a| a.ended_ms.is_none()).collect();
    let done: Vec<&Worker> = tree.iter().copied().filter(|a| a.ended_ms.is_some()).collect();
    if running.is_empty() && done.is_empty() {
        return;
    }
    o.gap();
    let mut counts = format!("{} running", running.len());
    let failed = done.iter().filter(|a| a.failed).count();
    counts.push_str(&format!(" · {} done", done.len() - failed));
    if failed > 0 {
        counts.push_str(&format!(" · {failed} failed"));
    }
    section_title(o, Section::Agents, &counts, folded);
    if folded {
        return;
    }
    // Finished ones newest first (the table is), within their tree order only for the shown ones.
    let mut newest: Vec<&Worker> = done.clone();
    newest.sort_by_key(|a| std::cmp::Reverse(a.ended_ms));
    let shown: Vec<&str> = newest.iter().take(SHOWN_DONE).map(|a| a.id.as_str()).collect();
    for a in running.iter().chain(done.iter().filter(|a| shown.contains(&a.id.as_str()))) {
        agent_row(o, a, now_ms);
    }
    if done.len() > SHOWN_DONE {
        o.line(2, &[(&format!("+ {} earlier", done.len() - SHOWN_DONE), g.dim, false)], None, None);
    }
    let _ = g;
}

fn agent_row(o: &mut Out, a: &Worker, now_ms: u64) {
    let g = o.g;
    let depth = a.depth.max(1) as usize - 1;
    let indent = depth.min(4) * 2;
    let tree = if depth > 0 { "└ " } else { "" };
    let (glyph, gc) = if a.asking {
        ("?", g.ask)
    } else if a.failed || a.status == "blocked" {
        ("✗", g.normal)
    } else if a.ended_ms.is_some() {
        ("✓", g.dim)
    } else if a.background {
        ("◌", g.normal)
    } else {
        ("●", g.normal)
    };
    let kind = if a.kind.is_empty() { "agent" } else { &a.kind };
    let model = short_model(&a.model);
    let elapsed = age(a.ended_ms.unwrap_or(now_ms).saturating_sub(a.started_ms));
    let mut tags = String::new();
    if a.fork && a.kind != "fork" {
        tags.push_str(" fork");
    }
    if a.worktree {
        tags.push_str(" wt");
    }
    // Running: the current tool and target, else the call's description; finished: its STATUS, else its calls.
    let detail = if a.ended_ms.is_none() {
        match (a.tool.is_empty(), a.target.is_empty()) {
            (true, _) => a.description.clone(),
            (false, true) => a.tool.clone(),
            (false, false) => format!("{} {}", a.tool, a.target),
        }
    } else if !a.status.is_empty() {
        a.status.replace('_', " ")
    } else if a.calls > 0 {
        format!("{} calls", a.calls)
    } else {
        String::new()
    };
    let kc = if a.ended_ms.is_some() { g.dim } else { g.normal };
    // The detail is cut from the left (a file name's end stays).
    let used = 2 + indent + width(tree) + 2 + width(kind) + 1 + width(&model) + width(&tags) + 1 + width(&elapsed) + 2;
    let detail =
        if detail.is_empty() { detail } else { format!(" {}", fit_left(&detail, o.w.saturating_sub(used + 1))) };
    o.line(
        indent,
        &[
            (tree, g.dim, false),
            (glyph, gc, false),
            (" ", g.normal, false),
            (kind, kc, false),
            (" ", g.dim, false),
            (&model, g.dim, false),
            (&tags, g.dim, false),
            (&detail, g.normal, false),
        ],
        Some((&elapsed, g.dim)),
        Some(Hit::Agent(a.id.clone())),
    );
}

/// A Log line: (what, how long ago).
fn log_text(e: &LogEvent, now_ms: u64) -> Option<(String, String)> {
    let f = |k: &str| e.fields.iter().find(|(a, _)| a == k).map(|(_, v)| v.as_str()).unwrap_or("");
    let t = match e.kind.as_str() {
        "agent_start" => format!("{} started", f("agent")),
        "agent_end" => format!("{} done", f("agent")),
        "edit" => format!("{} edit {}", f("agent"), f("file")),
        "ask" => format!("{} asks {}", f("agent"), f("tool")),
        "turn_end" => "turn ended".into(),
        "turn_failed" => "turn failed".into(),
        "compact" => "compacted".into(),
        "attention" if f("attention") == "None" && f("event") != "SessionStart" => return None,
        "claude_started" => "claude started".into(),
        "claude_ended" => "claude ended".into(),
        _ => return None,
    };
    Some((t, age(now_ms.saturating_sub(e.ms))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use haetty_proto::wire::FileStat;

    const FG: Rgb = Rgb { r: 206, g: 206, b: 206 };
    const BG: Rgb = Rgb { r: 20, g: 20, b: 20 };

    fn greys() -> Greys {
        let mut p = [Rgb { r: 0, g: 0, b: 0 }; 16];
        p[1] = Rgb { r: 200, g: 0, b: 0 };
        p[2] = Rgb { r: 0, g: 200, b: 0 };
        p[3] = Rgb { r: 200, g: 120, b: 0 };
        p[9] = Rgb { r: 255, g: 60, b: 60 };
        Greys::new(FG, BG, &p)
    }

    fn text(rows: &[Vec<Cell>]) -> String {
        rows.iter()
            .map(|r| r.iter().map(|c| c.text.as_str()).collect::<String>().trim_end().to_string())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn model<'a>(c: Option<&'a ClaudeInfo>, now: u64) -> Model<'a> {
        Model {
            claude: c,
            folder: "~/projects/demo".into(),
            branch: Some("main".into()),
            git: None,
            workers: &[],
            log: &[],
            unintegrated: false,
            folded: &[],
            hidden: &[],
            tokens: None,
            now_ms: now,
            selected: None,
        }
    }

    fn agent(id: &str, kind: &str, parent: &str, depth: u8, start: u64, end: Option<u64>) -> Worker {
        Worker {
            id: id.into(),
            kind: kind.into(),
            parent: parent.into(),
            depth,
            started_ms: start,
            ended_ms: end,
            model: "claude-haiku-5-5".into(),
            ..Default::default()
        }
    }

    fn info() -> ClaudeInfo {
        ClaudeInfo {
            name: "fix parser".into(),
            model: "Haiku 5.5".into(),
            effort: "medium".into(),
            context_pct: Some(34.0),
            context_size: Some(200_000),
            cost_usd: Some(1.23),
            workers: vec![
                Worker { tool: "Read".into(), target: "app.rs".into(), ..agent("a1", "Explore", "", 1, 0, None) },
                agent("a2", "general-purpose", "", 1, 0, None),
                Worker { background: true, ..agent("a3", "Explore", "a2", 2, 1000, None) },
                Worker { calls: 4, ..agent("a4", "Plan", "", 1, 0, Some(30_000)) },
                Worker { status: "blocked".into(), ..agent("a6", "Plan", "", 1, 0, Some(31_000)) },
            ],
            files: vec![FileStat {
                path: "/home/u/projects/demo/src/very/long/path/main.rs".into(),
                added: 12,
                removed: 3,
                edits: 2,
                at_ms: 1,
            }],
            updated_ms: 58_000,
            ..Default::default()
        }
    }

    #[test]
    fn lead_agents_nesting_files_and_footer() {
        let i = info();
        let (rows, hits) = draw(&model(Some(&i), 62_000), 42, 30, greys());
        let t = text(&rows);
        assert!(t.contains("fix parser") && t.contains("Haiku 5.5 · medium"), "{t}");
        assert!(t.contains("context  34% of 200k") || t.contains("context   34% of 200k"), "{t}");
        assert!(t.contains("Agents  3 running · 2 done"), "{t}");
        assert!(t.contains("✗ Plan haiku blocked"), "a finished agent's STATUS: {t}");
        assert!(t.contains("● Explore haiku Read app.rs"), "{t}");
        assert!(t.contains("└ ◌ Explore haiku"), "nested and background: {t}");
        assert!(t.contains("✓ Plan haiku 4 calls"), "{t}");
        assert!(t.contains("…") && t.contains("main.rs") && t.contains("+12 -3"), "a path cut on the left: {t}");
        assert!(t.contains("~/projects/demo:main"), "{t}");
        assert!(t.contains("updated 4s ago"), "{t}");
        // The nested one right after its parent.
        let lines: Vec<&str> = t.lines().collect();
        let p = lines.iter().position(|l| l.contains("general-purpose")).unwrap();
        assert!(lines[p + 1].contains("└"), "{t}");
        // Clickable: the section titles, the agents, the file.
        assert!(hits.iter().any(|(_, h)| *h == Hit::Fold(Section::Agents)));
        assert_eq!(hits.iter().filter(|(_, h)| matches!(h, Hit::Agent(_))).count(), 5);
        assert!(hits.iter().any(|(_, h)| matches!(h, Hit::File(p) if p.ends_with("main.rs"))));
        // Every row is the sidebar's width.
        assert!(rows.iter().all(|r| r.len() == 42) && rows.len() == 30);
    }

    #[test]
    fn grey_only_but_a_question_and_no_accent_while_agents_run() {
        let g = greys();
        let i = info();
        let (rows, _) = draw(&model(Some(&i), 62_000), 42, 30, g);
        let accents = [g.wait, g.ask, g.added, g.removed];
        assert!(
            rows.iter().flatten().all(|c| !c.fg.is_some_and(|f| accents.contains(&f))),
            "grey only with agents running"
        );
        let mut i = info();
        i.workers[0].asking = true;
        let (rows, _) = draw(&model(Some(&i), 62_000), 42, 30, g);
        let asks: Vec<&Cell> = rows.iter().flatten().filter(|c| c.fg == Some(g.ask)).collect();
        assert_eq!(asks.len(), 1, "only the waiting agent's ?");
        assert_eq!(asks[0].text, "?");
    }

    #[test]
    fn folding_more_than_six_finished_and_hidden_sections() {
        let mut i = info();
        for n in 0..10 {
            i.workers.push(agent(&format!("d{n}"), "Explore", "", 1, 0, Some(10_000 + n)));
        }
        let (rows, _) = draw(&model(Some(&i), 62_000), 42, 40, greys());
        let t = text(&rows);
        assert!(t.contains("+ 6 earlier"), "12 finished, 6 shown: {t}");
        let folded = [Section::Agents];
        let mut m = model(Some(&i), 62_000);
        m.folded = &folded;
        let t = text(&draw(&m, 42, 40, greys()).0);
        assert!(t.contains("▸ Agents") && !t.contains("Explore haiku"), "{t}");
        let hidden = [Section::Agents, Section::Files];
        let mut m = model(Some(&i), 62_000);
        m.hidden = &hidden;
        let t = text(&draw(&m, 42, 40, greys()).0);
        assert!(!t.contains("Agents") && !t.contains("Files"), "{t}");
    }

    #[test]
    fn shell_tab_git_summary_and_the_unintegrated_line() {
        let gs = GitSummary { files: vec![("src/a.rs".into(), 5, 1), ("b.md".into(), 2, 0)], untracked: 1 };
        let mut m = model(None, 0);
        m.git = Some(&gs);
        m.unintegrated = true;
        let (rows, hits) = draw(&m, 42, 24, greys());
        let t = text(&rows);
        assert!(t.contains("demo") && t.contains("main"), "{t}");
        assert!(t.contains("2 changed +7 -1 · 1 new"), "{t}");
        assert!(t.contains("claude is running without") && t.contains("haetty's integration"), "{t}");
        assert!(hits.iter().any(|(_, h)| *h == Hit::File("src/a.rs".into())));
    }

    #[test]
    fn docking_depends_on_the_window_and_settings_only() {
        assert_eq!(docked_cols(121, true, 121, 42, true), 42);
        assert_eq!(docked_cols(120, true, 121, 42, true), 0, "narrow: over the terminal on request");
        assert_eq!(docked_cols(200, false, 121, 42, true), 0, "sidebar = hide");
        assert_eq!(docked_cols(200, true, 121, 42, false), 0, "hidden by the person");
        assert_eq!(docked_cols(70, true, 60, 42, true), 0, "the terminal keeps 40 columns");
        assert_eq!(docked_cols(100, true, 60, 42, true), 42);
    }

    #[test]
    fn short_models_and_log_lines() {
        assert_eq!(short_model("claude-haiku-5-5"), "haiku");
        assert_eq!(short_model("claude-opus-5-5[1m]"), "opus");
        assert_eq!(short_model(""), "");
        let e = LogEvent {
            seq: 1,
            ms: 1_000,
            kind: "edit".into(),
            session: "s".into(),
            fields: vec![("agent".into(), "main".into()), ("file".into(), "lib.rs".into())],
        };
        assert_eq!(log_text(&e, 61_000), Some(("main edit lib.rs".into(), "1m00".into())));
    }
}
