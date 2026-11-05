//! The ccterm daemon (DESIGN.md §3.1, §3.6–3.8): owns the sessions, serves
//! frames and events to clients over the local socket, enforces roles.
//!
//! One thread owns all state and handles every event in order (connections,
//! client messages, session events); each client has a reader thread and a
//! writer thread, each session a thread forwarding its events. No lock is
//! held across a socket write.

mod log_file;

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ccterm_cc::{settings, Claude, Json};
use ccterm_core::{ColorSchemePref, CommandSpec, GridSize, Session, SessionConfig, SessionEvent, Snapshot};
use ccterm_proto::paths::Endpoint;
use ccterm_proto::product;
use ccterm_proto::transport::{Listener, Stream};
use ccterm_proto::wire::*;
use crossbeam_channel::{unbounded, Receiver, Sender};

pub use log_file::EventLog;

pub struct Config {
    pub endpoint: Endpoint,
    pub state_dir: PathBuf,
    /// Private directory for per-session files (Claude Code settings).
    pub run_dir: PathBuf,
    /// Exit after this long without sessions and clients (`None`: never).
    pub idle_exit: Option<Duration>,
}

impl Config {
    pub fn default_for_user() -> io::Result<Config> {
        Ok(Config {
            endpoint: Endpoint::default_for_user()?,
            state_dir: ccterm_proto::paths::state_dir(),
            run_dir: ccterm_proto::paths::private_dir()?,
            idle_exit: Some(Duration::from_secs(10)),
        })
    }
}

type ClientId = u64;

enum Ev {
    Hello(ClientId, Role, String, Sender<DaemonMsg>),
    Msg(ClientId, ClientMsg),
    Gone(ClientId),
    Session(SessionId, SessionEvent),
}

struct Sess {
    session: Session,
    info: SessionInfo,
    latest: Option<Arc<Snapshot>>,
    /// Claude Code state (a Claude Code tab with the integration). While it
    /// waits for a permission answer, only Human input is accepted.
    claude: Option<Claude>,
    /// The `--settings` file given to Claude Code; removed with the session.
    settings_file: Option<PathBuf>,
    /// Key → screen latency (DESIGN.md §3.8, friction log).
    lat: Latency,
}

/// Key → screen-update latency of a session: the time from a key (or
/// paste) arriving to the next change of the screen. Samples are summarised
/// in the event log; one over the budget is logged at most every 10 s.
#[derive(Default)]
struct Latency {
    key_at: Option<Instant>,
    samples_us: Vec<u32>,
    over: u32,
    last_slow: Option<Instant>,
    last_summary: Option<Instant>,
}

/// The key → screen budget (DESIGN.md §2.3, §3.4).
const LATENCY_BUDGET: Duration = Duration::from_millis(16);
const MAX_SAMPLES: usize = 20_000;
const SUMMARY_EVERY: Duration = Duration::from_secs(600);

impl Latency {
    /// A screen change: if a key waits for it, one sample; returns the
    /// latency when it is over budget and may be logged now.
    fn screen_changed(&mut self) -> Option<Duration> {
        let d = self.key_at.take()?.elapsed();
        if self.samples_us.len() < MAX_SAMPLES {
            self.samples_us.push(d.as_micros().min(u32::MAX as u128) as u32);
        }
        if d <= LATENCY_BUDGET {
            return None;
        }
        self.over += 1;
        let quiet = self.last_slow.is_none_or(|t| t.elapsed() >= Duration::from_secs(10));
        quiet.then(|| {
            self.last_slow = Some(Instant::now());
            d
        })
    }

    /// Summary fields (and resets the samples), or `None` without samples.
    fn summary(&mut self) -> Option<Vec<(&'static str, String)>> {
        if self.samples_us.is_empty() {
            return None;
        }
        let mut v = std::mem::take(&mut self.samples_us);
        v.sort_unstable();
        let q = |p: f64| v[((v.len() - 1) as f64 * p).round() as usize] as f64 / 1000.0;
        let out = vec![
            ("n", v.len().to_string()),
            ("p50_ms", format!("{:.2}", q(0.5))),
            ("p95_ms", format!("{:.2}", q(0.95))),
            ("max_ms", format!("{:.2}", q(1.0))),
            ("over_16ms", std::mem::take(&mut self.over).to_string()),
        ];
        self.last_summary = Some(Instant::now());
        Some(out)
    }
}

impl Sess {
    fn awaiting_permission(&self) -> bool {
        self.claude.as_ref().is_some_and(Claude::awaiting_permission)
    }

    fn drop_settings_file(&mut self) {
        if let Some(p) = self.settings_file.take() {
            let _ = std::fs::remove_file(p);
        }
    }
}

struct Sub {
    base: Option<Arc<Snapshot>>,
    wanted: bool,
}

struct Cli {
    role: Role,
    name: String,
    tx: Sender<DaemonMsg>,
    subs: HashMap<SessionId, Sub>,
}

/// Binds the endpoint and serves until idle. `AddrInUse` means another
/// daemon already runs (the caller should exit quietly).
pub fn run(cfg: Config) -> io::Result<()> {
    let listener = Listener::bind(&cfg.endpoint)?;
    std::fs::create_dir_all(&cfg.state_dir)?;
    let log = EventLog::open(&cfg.state_dir.join("events.log"))?;
    install_panic_log(cfg.state_dir.join("events.log"));
    // Settings files of sessions a killed daemon left behind (we are the
    // only daemon now: none of them is in use).
    if let Ok(rd) = std::fs::read_dir(&cfg.run_dir) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if n.starts_with("settings-") && n.ends_with(".json") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    let (tx, rx) = unbounded::<Ev>();
    let atx = tx.clone();
    std::thread::Builder::new().name("ccterm-accept".into()).spawn(move || {
        let mut next: ClientId = 1;
        loop {
            match listener.accept() {
                Ok(s) => {
                    let id = next;
                    next += 1;
                    let tx = atx.clone();
                    let _ = std::thread::Builder::new()
                        .name(format!("ccterm-client-{id}"))
                        .spawn(move || client_reader(id, s, tx));
                }
                Err(e) => {
                    log::error!("accept: {e}");
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        }
    })?;
    log::info!("daemon {} listening on {}", std::process::id(), cfg.endpoint.0.display());
    let mut d = Daemon { sessions: HashMap::new(), clients: HashMap::new(), tx, log, counter: 0, run_dir: cfg.run_dir };
    d.log.write("daemon_start", "", &[("pid", &std::process::id().to_string())]);
    d.run(rx, cfg.idle_exit);
    let ids: Vec<SessionId> = d.sessions.keys().cloned().collect();
    for id in ids {
        if let Some(f) = d.sessions.get_mut(&id).and_then(|s| s.lat.summary()) {
            d.log_latency(&id, f);
        }
    }
    d.log.write("daemon_stop", "", &[]);
    Ok(())
}

/// A panic anywhere in the daemon goes to the event log as well (its own
/// line; the payload is a message from our code, not session text).
fn install_panic_log(path: PathBuf) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let what = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_default();
        let at = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
        let thread = std::thread::current().name().unwrap_or("?").to_string();
        if let Ok(mut l) = EventLog::open(&path) {
            l.write("panic", "", &[("what", &what), ("at", &at), ("thread", &thread)]);
        }
        previous(info);
    }));
}

/// Runs [`run`] on a thread (tests, embedding).
pub fn spawn(cfg: Config) -> std::thread::JoinHandle<io::Result<()>> {
    std::thread::spawn(move || run(cfg))
}

fn client_reader(id: ClientId, mut s: Stream, tx: Sender<Ev>) {
    let hello = read_msg::<ClientMsg>(&mut s);
    let (role, name) = match hello {
        Ok(ClientMsg::Hello { magic, version, role, name }) if magic == MAGIC && version == VERSION => (role, name),
        Ok(ClientMsg::Hello { version, .. }) => {
            let _ = write_msg(
                &mut s,
                &DaemonMsg::Refused { reason: format!("protocol version {version}, daemon speaks {VERSION}") },
            );
            return;
        }
        _ => {
            let _ = write_msg(&mut s, &DaemonMsg::Refused { reason: "expected Hello".into() });
            return;
        }
    };
    let Ok(mut w) = s.try_clone() else { return };
    if write_msg(&mut w, &DaemonMsg::Welcome { version: VERSION, daemon_pid: std::process::id() }).is_err() {
        return;
    }
    let (wtx, wrx) = unbounded::<DaemonMsg>();
    let _ = std::thread::Builder::new().name(format!("ccterm-writer-{id}")).spawn(move || {
        for m in wrx {
            if write_msg(&mut w, &m).is_err() {
                w.shutdown();
                break;
            }
        }
    });
    if tx.send(Ev::Hello(id, role, name, wtx)).is_err() {
        return;
    }
    loop {
        match read_msg::<ClientMsg>(&mut s) {
            Ok(m) => {
                if tx.send(Ev::Msg(id, m)).is_err() {
                    return;
                }
            }
            Err(_) => {
                let _ = tx.send(Ev::Gone(id));
                return;
            }
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

struct Daemon {
    sessions: HashMap<SessionId, Sess>,
    clients: HashMap<ClientId, Cli>,
    tx: Sender<Ev>,
    log: EventLog,
    counter: u64,
    run_dir: PathBuf,
}

impl Daemon {
    fn run(&mut self, rx: Receiver<Ev>, idle_exit: Option<Duration>) {
        let mut idle_since = Instant::now();
        loop {
            match rx.recv_timeout(Duration::from_millis(500)) {
                Ok(ev) => self.handle(ev),
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                Err(_) => return,
            }
            if !self.sessions.is_empty() || !self.clients.is_empty() {
                idle_since = Instant::now();
            } else if idle_exit.is_some_and(|d| idle_since.elapsed() >= d) {
                log::info!("no sessions and no clients: exiting");
                return;
            }
        }
    }

    fn new_id(&mut self) -> SessionId {
        self.counter += 1;
        format!("{:x}-{:x}", now_ms(), self.counter)
    }

    fn reply(&self, c: ClientId, m: DaemonMsg) {
        if let Some(cl) = self.clients.get(&c) {
            let _ = cl.tx.send(m);
        }
    }

    fn handle(&mut self, ev: Ev) {
        match ev {
            Ev::Hello(id, role, name, tx) => {
                log::info!("client {id} ({name}) connected as {role:?}");
                self.clients.insert(id, Cli { role, name, tx, subs: HashMap::new() });
            }
            Ev::Gone(id) => {
                if let Some(c) = self.clients.remove(&id) {
                    log::info!("client {id} ({}) gone", c.name);
                }
            }
            Ev::Msg(id, m) => self.message(id, m),
            Ev::Session(sid, e) => self.session_event(sid, e),
        }
    }

    fn session_event(&mut self, sid: SessionId, e: SessionEvent) {
        match &e {
            SessionEvent::Wakeup => {
                let Some(s) = self.sessions.get_mut(&sid) else { return };
                let mut slow = None;
                if let Some(snap) = s.session.handle.snapshot() {
                    s.info.title.clone_from(&snap.title);
                    if !snap.pwd.is_empty() {
                        s.info.cwd.clone_from(&snap.pwd);
                    }
                    if s.latest.as_ref().is_none_or(|l| l.seq != snap.seq) {
                        slow = s.lat.screen_changed();
                    }
                    s.latest = Some(Arc::new(snap));
                }
                let summary =
                    s.lat.last_summary.is_none_or(|t| t.elapsed() >= SUMMARY_EVERY) && s.lat.samples_us.len() >= 1000;
                let summary = if summary { s.lat.summary() } else { None };
                self.push_frames(&sid);
                if let Some(d) = slow {
                    let ms = format!("{:.1}", d.as_secs_f64() * 1000.0);
                    self.log.write("slow_input", &sid, &[("key_to_screen_ms", &ms), ("budget_ms", "16")]);
                }
                if let Some(f) = summary {
                    self.log_latency(&sid, f);
                }
                return;
            }
            SessionEvent::Exited(st) => {
                let mut summary = None;
                if let Some(s) = self.sessions.get_mut(&sid) {
                    s.info.state = SessionState::Exited { code: st.code };
                    s.drop_settings_file();
                    summary = s.lat.summary();
                }
                if let Some(f) = summary {
                    self.log_latency(&sid, f);
                }
                let code = st.code.map(|c| c.to_string()).unwrap_or_default();
                self.log.write("session_exited", &sid, &[("code", &code)]);
            }
            SessionEvent::Title(t) => {
                if let Some(s) = self.sessions.get_mut(&sid) {
                    s.info.title.clone_from(t);
                }
            }
            SessionEvent::Error(e) => self.log.write("error", &sid, &[("what", e)]),
            _ => {}
        }
        if let Some(we) = event_to_wire(&e) {
            for c in self.clients.values() {
                if c.subs.contains_key(&sid) {
                    let _ = c.tx.send(DaemonMsg::Event { session: sid.clone(), event: we.clone() });
                }
            }
        }
    }

    /// Sends each waiting subscriber of `sid` the frame from its baseline to
    /// the newest snapshot.
    fn push_frames(&mut self, sid: &str) {
        let Some(latest) = self.sessions.get(sid).and_then(|s| s.latest.clone()) else { return };
        for c in self.clients.values_mut() {
            if let Some(sub) = c.subs.get_mut(sid) {
                if sub.wanted && sub.base.as_ref().is_none_or(|b| b.seq != latest.seq) {
                    let f = frame_between(sid, sub.base.as_deref(), &latest);
                    let _ = c.tx.send(DaemonMsg::Frame(f));
                    sub.base = Some(latest.clone());
                    sub.wanted = false;
                }
            }
        }
    }

    fn denied(&self, c: ClientId, what: &str) {
        log::warn!("client {c}: denied {what}");
        self.reply(c, DaemonMsg::Denied { what: what.into() });
    }

    fn message(&mut self, c: ClientId, m: ClientMsg) {
        let Some(role) = self.clients.get(&c).map(|cl| cl.role) else { return };
        // Role check (DESIGN.md §3.7).
        let allowed = match (&m, role) {
            (ClientMsg::Hello { .. }, _) => false,
            // Hook and status-line reports only from Report clients (the
            // `ccterm hook` / `ccterm statusline` processes).
            (ClientMsg::Hook { .. } | ClientMsg::StatusLine { .. }, Role::Report) => true,
            (ClientMsg::Hook { .. } | ClientMsg::StatusLine { .. }, _) => false,
            (ClientMsg::Sync, _) => true,
            (_, Role::Report) => false,
            (
                ClientMsg::ListSessions
                | ClientMsg::Attach { .. }
                | ClientMsg::Detach { .. }
                | ClientMsg::NextFrame { .. },
                _,
            ) => true,
            (_, Role::Observe) => false,
            (_, Role::Control) => true,
        };
        if !allowed {
            self.denied(c, &format!("{} as {role:?}", msg_name(&m)));
            return;
        }
        match m {
            ClientMsg::Hello { .. } => {}
            ClientMsg::ListSessions => {
                let mut v: Vec<SessionInfo> = self.sessions.values().map(|s| s.info.clone()).collect();
                v.sort_by(|a, b| a.created_ms.cmp(&b.created_ms).then(a.id.cmp(&b.id)));
                self.reply(c, DaemonMsg::Sessions(v));
            }
            ClientMsg::Create { req, spec } => self.create(c, req, spec),
            ClientMsg::Attach { session } => {
                let Some(s) = self.sessions.get_mut(&session) else {
                    self.reply(c, DaemonMsg::NoSession { session });
                    return;
                };
                if s.latest.is_none() {
                    s.latest = s.session.handle.snapshot().map(Arc::new);
                }
                let claude = s.claude.as_ref().map(|cl| Box::new(cl.info.clone()));
                if let Some(cl) = self.clients.get_mut(&c) {
                    cl.subs.insert(session.clone(), Sub { base: None, wanted: true });
                }
                self.push_frames(&session);
                if let Some(info) = claude {
                    self.reply(c, DaemonMsg::Claude { session, info });
                }
            }
            ClientMsg::Detach { session } => {
                if let Some(cl) = self.clients.get_mut(&c) {
                    cl.subs.remove(&session);
                }
            }
            ClientMsg::NextFrame { session } => {
                if let Some(sub) = self.clients.get_mut(&c).and_then(|cl| cl.subs.get_mut(&session)) {
                    sub.wanted = true;
                }
                self.push_frames(&session);
            }
            ClientMsg::Close { session } => {
                if let Some(mut s) = self.sessions.remove(&session) {
                    s.drop_settings_file();
                    if let Some(f) = s.lat.summary() {
                        self.log_latency(&session, f);
                    }
                    self.log.write("session_closed", &session, &[]);
                    for cl in self.clients.values_mut() {
                        if cl.subs.remove(&session).is_some() {
                            let _ = cl.tx.send(DaemonMsg::NoSession { session: session.clone() });
                        }
                    }
                }
            }
            ClientMsg::Hook { session, event, json, ts_ms, snapshot } => {
                self.claude_report(session, |cl, j| cl.hook(&event, j, ts_ms, snapshot), &json.0, &event)
            }
            ClientMsg::StatusLine { session, json, ts_ms } => {
                self.claude_report(session, |cl, j| cl.statusline(j, ts_ms), &json.0, "statusline")
            }
            ClientMsg::ClearAttention { session } => {
                if self.sessions.get_mut(&session).and_then(|s| s.claude.as_mut()).is_some_and(Claude::clear_wait) {
                    self.push_claude(&session);
                }
            }
            ClientMsg::GetEdit { session } => {
                let edit = self
                    .sessions
                    .get(&session)
                    .and_then(|s| s.claude.as_ref())
                    .and_then(Claude::last_edit)
                    .map(Box::new);
                self.reply(c, DaemonMsg::Edit { session, edit });
            }
            ClientMsg::Sync => self.reply(c, DaemonMsg::Synced),
            ClientMsg::Friction { kind, fields } => {
                // Kinds and field names from the GUI: short, printable.
                let clean = |s: &str| s.chars().filter(|c| !c.is_control()).take(200).collect::<String>();
                let kind = format!("gui_{}", clean(&kind));
                let f: Vec<(String, String)> = fields.iter().take(8).map(|(k, v)| (clean(k), clean(v))).collect();
                let refs: Vec<(&str, &str)> = f.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
                self.log.write(&kind, "", &refs);
            }
            input => self.input(c, input),
        }
    }

    fn input(&mut self, c: ClientId, m: ClientMsg) {
        let (session, origin) = match &m {
            ClientMsg::Key { session, origin, .. }
            | ClientMsg::Mouse { session, origin, .. }
            | ClientMsg::Wheel { session, origin, .. }
            | ClientMsg::Paste { session, origin, .. }
            | ClientMsg::Write { session, origin, .. } => (session.clone(), Some(*origin)),
            ClientMsg::Focus { session, .. }
            | ClientMsg::Resize { session, .. }
            | ClientMsg::Scroll { session, .. }
            | ClientMsg::SetColorScheme { session, .. } => (session.clone(), None),
            _ => return,
        };
        let Some(s) = self.sessions.get_mut(&session) else {
            self.reply(c, DaemonMsg::NoSession { session });
            return;
        };
        if matches!(m, ClientMsg::Key { .. } | ClientMsg::Paste { .. }) && s.lat.key_at.is_none() {
            s.lat.key_at = Some(Instant::now());
        }
        if s.awaiting_permission() && origin.is_some_and(|o| o != Origin::Human) {
            let what = format!("{} from a program while the session waits for a permission answer", msg_name(&m));
            self.denied(c, &what);
            return;
        }
        let h = &s.session.handle;
        match m {
            ClientMsg::Key { key, .. } => {
                if let Some(k) = key.to_key_input() {
                    h.key(k)
                }
            }
            ClientMsg::Mouse { mouse, .. } => {
                if let Some(mi) = mouse.to_mouse_input() {
                    h.mouse(mi)
                }
            }
            ClientMsg::Wheel { lines, col, row, mods, .. } => {
                h.wheel(lines, col, row, ccterm_core::Mods::from_bits_truncate(mods))
            }
            ClientMsg::Paste { text, .. } => h.paste(text.0),
            ClientMsg::Write { bytes, .. } => h.write(bytes.0),
            ClientMsg::Focus { focused, .. } => h.focus(focused),
            ClientMsg::Resize { size, .. } => {
                s.info.cols = size.cols;
                s.info.rows = size.rows;
                h.resize(size.into());
            }
            ClientMsg::Scroll { to, .. } => h.scroll(to.into()),
            ClientMsg::SetColorScheme { scheme, .. } => h.set_color_scheme(match scheme {
                0 => ColorSchemePref::Dark,
                1 => ColorSchemePref::Light,
                _ => ColorSchemePref::Unknown,
            }),
            _ => {}
        }
    }

    /// A hook or status-line report for a session: update its Claude Code
    /// state and tell the subscribers when something shown changed. The
    /// payload is never logged; the event log gets the event name and the
    /// resulting attention only.
    fn claude_report(
        &mut self,
        session: Option<SessionId>,
        apply: impl FnOnce(&mut Claude, &Json) -> bool,
        json: &str,
        what: &str,
    ) {
        let Some(sid) = session else { return };
        let Some(cl) = self.sessions.get_mut(&sid).and_then(|s| s.claude.as_mut()) else {
            log::debug!("{what} for session {sid}, which has no Claude Code integration");
            return;
        };
        let j = match Json::parse(json) {
            Ok(j) => j,
            Err(e) => {
                log::warn!("{what} for {sid}: payload is not JSON ({} bytes): {e}", json.len());
                return;
            }
        };
        let attention = cl.info.attention;
        if !apply(cl, &j) {
            return;
        }
        let now = cl.info.attention;
        if now != attention {
            self.log.write("attention", &sid, &[("event", what), ("attention", &format!("{now:?}"))]);
        }
        self.push_claude(&sid);
    }

    fn log_latency(&mut self, sid: &str, f: Vec<(&'static str, String)>) {
        let refs: Vec<(&str, &str)> = f.iter().map(|(k, v)| (*k, v.as_str())).collect();
        self.log.write("latency", sid, &refs);
    }

    fn push_claude(&self, sid: &str) {
        let Some(info) = self.sessions.get(sid).and_then(|s| s.claude.as_ref()).map(|c| &c.info) else { return };
        for c in self.clients.values() {
            if c.subs.contains_key(sid) {
                let _ = c.tx.send(DaemonMsg::Claude { session: sid.to_string(), info: Box::new(info.clone()) });
            }
        }
    }

    /// Claude Code with the integration: its arguments with ccterm's
    /// `--settings` file (merged with the user's own `--settings`).
    fn claude_args(&self, id: &str, spec: &CreateSpec) -> Option<(Vec<String>, PathBuf)> {
        if !spec.claude_integration || !settings::is_claude(&spec.program) {
            return None;
        }
        let (rest, user) = settings::split_user_settings(&spec.args);
        let user = user.and_then(|v| {
            settings::load_user(&v, spec.cwd.as_deref().map(std::path::Path::new))
                .map_err(|e| log::warn!("session {id}: {e}; ccterm's settings only"))
                .ok()
        });
        let exe = product::exe_path().to_string_lossy().into_owned();
        let (merged, warnings) = settings::merge(settings::ours(&exe), user);
        for w in warnings {
            log::warn!("session {id}: {w}");
        }
        let path = self.run_dir.join(format!("settings-{id}.json"));
        if let Err(e) = write_private(&path, merged.to_string().as_bytes()) {
            log::error!("session {id}: {}: {e}", path.display());
            return None;
        }
        let mut args = rest;
        args.push("--settings".into());
        args.push(path.to_string_lossy().into_owned());
        Some((args, path))
    }

    fn create(&mut self, c: ClientId, req: u32, spec: CreateSpec) {
        let id = self.new_id();
        let claude = self.claude_args(&id, &spec);
        let args = claude.as_ref().map_or(&spec.args, |(a, _)| a);
        let mut cmd = CommandSpec::new(&spec.program).args(args);
        if let Some(cwd) = &spec.cwd {
            cmd = cmd.cwd(cwd);
        }
        for (k, v) in &spec.env {
            cmd = cmd.env(k, v);
        }
        cmd = cmd.env(product::SESSION_ENV, &id);
        // Hooks of this session must reach this instance's daemon.
        cmd = cmd.env(product::INSTANCE_ENV, product::instance());
        let mut cfg = SessionConfig::new(cmd);
        cfg.size = GridSize::from(spec.size);
        cfg.scrollback_limit = spec.scrollback_limit as usize;
        cfg.default_fg = from_rgb(spec.default_fg);
        cfg.default_bg = from_rgb(spec.default_bg);
        match Session::spawn(cfg) {
            Ok(session) => {
                let info = SessionInfo {
                    id: id.clone(),
                    program: spec.program.clone(),
                    title: String::new(),
                    cwd: spec.cwd.clone().unwrap_or_default(),
                    state: SessionState::Running,
                    created_ms: now_ms(),
                    pid: session.handle.pid(),
                    cols: spec.size.cols,
                    rows: spec.size.rows,
                };
                let events = session.events.clone();
                let tx = self.tx.clone();
                let sid = id.clone();
                let _ = std::thread::Builder::new().name(format!("ccterm-sess-{id}")).spawn(move || {
                    for e in events {
                        if tx.send(Ev::Session(sid.clone(), e)).is_err() {
                            return;
                        }
                    }
                });
                self.log.write("session_created", &id, &[("program", &spec.program), ("cwd", &info.cwd)]);
                log::info!("session {id}: {} (pid {:?}) in {:?}", spec.program, info.pid, spec.cwd);
                let (claude, settings_file) = match claude {
                    Some((_, p)) => (Some(Claude::default()), Some(p)),
                    None => (None, None),
                };
                self.sessions.insert(
                    id.clone(),
                    Sess { session, info, latest: None, claude, settings_file, lat: Latency::default() },
                );
                self.reply(c, DaemonMsg::Created { req, session: id });
            }
            Err(e) => {
                if let Some((_, p)) = claude {
                    let _ = std::fs::remove_file(p);
                }
                self.log.write("error", &id, &[("what", "create failed"), ("program", &spec.program)]);
                self.reply(c, DaemonMsg::CreateFailed { req, error: format!("cannot start {:?}: {e}", spec.program) })
            }
        }
    }
}

fn msg_name(m: &ClientMsg) -> &'static str {
    match m {
        ClientMsg::Hello { .. } => "Hello",
        ClientMsg::ListSessions => "ListSessions",
        ClientMsg::Create { .. } => "Create",
        ClientMsg::Attach { .. } => "Attach",
        ClientMsg::Detach { .. } => "Detach",
        ClientMsg::NextFrame { .. } => "NextFrame",
        ClientMsg::Key { .. } => "Key",
        ClientMsg::Mouse { .. } => "Mouse",
        ClientMsg::Wheel { .. } => "Wheel",
        ClientMsg::Paste { .. } => "Paste",
        ClientMsg::Write { .. } => "Write",
        ClientMsg::Focus { .. } => "Focus",
        ClientMsg::Resize { .. } => "Resize",
        ClientMsg::Scroll { .. } => "Scroll",
        ClientMsg::SetColorScheme { .. } => "SetColorScheme",
        ClientMsg::Close { .. } => "Close",
        ClientMsg::Hook { .. } => "Hook",
        ClientMsg::StatusLine { .. } => "StatusLine",
        ClientMsg::ClearAttention { .. } => "ClearAttention",
        ClientMsg::GetEdit { .. } => "GetEdit",
        ClientMsg::Sync => "Sync",
        ClientMsg::Friction { .. } => "Friction",
    }
}

/// Writes a file only this user can read (Unix mode 0600; on Windows the
/// directory is in the user's profile).
fn write_private(path: &std::path::Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    o.open(path)?.write_all(bytes)
}
