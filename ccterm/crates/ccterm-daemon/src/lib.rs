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
    /// Exit after this long without sessions and clients (`None`: never).
    pub idle_exit: Option<Duration>,
}

impl Config {
    pub fn default_for_user() -> io::Result<Config> {
        Ok(Config {
            endpoint: Endpoint::default_for_user()?,
            state_dir: ccterm_proto::paths::state_dir(),
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
    /// Set from Claude Code hook events (milestone 3): a permission prompt
    /// or question is open, so only Human input is accepted.
    awaiting_permission: bool,
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
    let mut d = Daemon { sessions: HashMap::new(), clients: HashMap::new(), tx, log, counter: 0 };
    d.log.write("daemon_start", "", &[("pid", &std::process::id().to_string())]);
    d.run(rx, cfg.idle_exit);
    d.log.write("daemon_stop", "", &[]);
    Ok(())
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
                if let Some(snap) = s.session.handle.snapshot() {
                    s.info.title.clone_from(&snap.title);
                    if !snap.pwd.is_empty() {
                        s.info.cwd.clone_from(&snap.pwd);
                    }
                    s.latest = Some(Arc::new(snap));
                }
                self.push_frames(&sid);
                return;
            }
            SessionEvent::Exited(st) => {
                if let Some(s) = self.sessions.get_mut(&sid) {
                    s.info.state = SessionState::Exited { code: st.code };
                }
                let code = st.code.map(|c| c.to_string()).unwrap_or_default();
                self.log.write("session_exited", &sid, &[("code", &code)]);
            }
            SessionEvent::Title(t) => {
                if let Some(s) = self.sessions.get_mut(&sid) {
                    s.info.title.clone_from(t);
                }
            }
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
            (ClientMsg::Hook { .. } | ClientMsg::StatusLine { .. }, _) => true,
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
                if let Some(cl) = self.clients.get_mut(&c) {
                    cl.subs.insert(session.clone(), Sub { base: None, wanted: true });
                }
                self.push_frames(&session);
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
                if self.sessions.remove(&session).is_some() {
                    self.log.write("session_closed", &session, &[]);
                    for cl in self.clients.values_mut() {
                        if cl.subs.remove(&session).is_some() {
                            let _ = cl.tx.send(DaemonMsg::NoSession { session: session.clone() });
                        }
                    }
                }
            }
            ClientMsg::Hook { .. } | ClientMsg::StatusLine { .. } => {
                // Milestone 3 (P4): Claude Code adapter.
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
        if s.awaiting_permission && origin.is_some_and(|o| o != Origin::Human) {
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
            ClientMsg::Paste { text, .. } => h.paste(text),
            ClientMsg::Write { bytes, .. } => h.write(bytes),
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

    fn create(&mut self, c: ClientId, req: u32, spec: CreateSpec) {
        let id = self.new_id();
        let mut cmd = CommandSpec::new(&spec.program).args(&spec.args);
        if let Some(cwd) = &spec.cwd {
            cmd = cmd.cwd(cwd);
        }
        for (k, v) in &spec.env {
            cmd = cmd.env(k, v);
        }
        cmd = cmd.env(product::SESSION_ENV, &id);
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
                self.sessions.insert(id.clone(), Sess { session, info, latest: None, awaiting_permission: false });
                self.reply(c, DaemonMsg::Created { req, session: id });
            }
            Err(e) => {
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
    }
}
