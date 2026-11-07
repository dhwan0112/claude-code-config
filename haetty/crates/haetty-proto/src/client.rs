//! A daemon client. [`RemoteSession`] has the shape of
//! `haetty_core::Session` — a handle with the same methods plus a
//! `Receiver<SessionEvent>` — so a frontend does not care whether the
//! session runs in-process or in the daemon (DESIGN.md §3.1, §3.6).
//!
//! Frames arrive on a reader thread and are applied to a mirror of the
//! screen; the first frame after the last `snapshot()` becomes a `Wakeup`.
//! `snapshot()` returns the mirror and asks the daemon for the next frame
//! (`NextFrame`), which is the backpressure.

use std::collections::HashMap;
use std::io;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crossbeam_channel::{unbounded, Receiver, Sender};
use haetty_core::{ColorSchemePref, GridSize, KeyInput, Mods, MouseInput, ScrollTo, SessionEvent, Snapshot};

use crate::paths::Endpoint;
use crate::transport::{dial, Dial, Stream};
use crate::wire::*;

#[derive(Debug)]
pub enum ConnectError {
    NotRunning,
    Refused(String),
    Io(io::Error),
}

impl std::fmt::Display for ConnectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnectError::NotRunning => f.write_str("no daemon is running"),
            ConnectError::Refused(r) if r.starts_with("protocol version") => write!(
                f,
                "a haetty daemon of another version is running ({r}). It exits by itself 10 s after its                  last window closes; or end the `haetty daemon` process (its sessions end with it)"
            ),
            ConnectError::Refused(r) => write!(f, "daemon refused the connection: {r}"),
            ConnectError::Io(e) => write!(f, "daemon connection: {e}"),
        }
    }
}

impl From<io::Error> for ConnectError {
    fn from(e: io::Error) -> Self {
        ConnectError::Io(e)
    }
}

struct Mirror {
    snap: Snapshot,
    /// Claude Code state from the daemon (DESIGN.md §3.13).
    claude: Option<Arc<ClaudeInfo>>,
    /// A Wakeup was sent and `snapshot()` has not been called since.
    woken: bool,
    tx: Sender<SessionEvent>,
}

struct Shared {
    mirrors: Mutex<HashMap<SessionId, Mirror>>,
    writer: Mutex<Stream>,
    /// The daemon's connection broke (the daemon ended or crashed).
    lost: std::sync::atomic::AtomicBool,
}

impl Shared {
    fn send(&self, msg: &ClientMsg) -> io::Result<()> {
        let mut w = self.writer.lock().unwrap_or_else(|p| p.into_inner());
        write_msg(&mut *w, msg)
    }
}

pub struct Client {
    shared: Arc<Shared>,
    replies: Receiver<DaemonMsg>,
    request: Mutex<u32>,
    pub daemon_pid: u32,
    pub role: Role,
}

impl Client {
    /// Whether the connection to the daemon broke (set before the sessions'
    /// `Exited` events are sent).
    pub fn is_lost(&self) -> bool {
        self.shared.lost.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Connects and says hello. `NotRunning` when nobody listens.
    pub fn connect(ep: &Endpoint, role: Role, name: &str) -> Result<Client, ConnectError> {
        Client::connect_with(ep, role, name, Duration::from_secs(2))
    }

    /// [`Client::connect`] with a dial timeout (hooks use a short one).
    pub fn connect_with(ep: &Endpoint, role: Role, name: &str, timeout: Duration) -> Result<Client, ConnectError> {
        let mut stream = match dial(ep, timeout) {
            Dial::Connected(s) => s,
            Dial::NotRunning => return Err(ConnectError::NotRunning),
            Dial::Failed(e) => return Err(ConnectError::Io(e)),
        };
        write_msg(&mut stream, &ClientMsg::Hello { magic: MAGIC, version: VERSION, role, name: name.into() })?;
        let daemon_pid = match read_msg::<DaemonMsg>(&mut stream)? {
            DaemonMsg::Welcome { daemon_pid, .. } => daemon_pid,
            DaemonMsg::Refused { reason } => return Err(ConnectError::Refused(reason)),
            _ => return Err(ConnectError::Refused("unexpected answer to Hello".into())),
        };
        let reader = stream.try_clone()?;
        let shared = Arc::new(Shared {
            mirrors: Mutex::new(HashMap::new()),
            writer: Mutex::new(stream),
            lost: std::sync::atomic::AtomicBool::new(false),
        });
        let (rtx, rrx) = unbounded();
        let sh = shared.clone();
        std::thread::Builder::new().name("haetty-client-reader".into()).spawn(move || reader_loop(reader, sh, rtx))?;
        Ok(Client { shared, replies: rrx, request: Mutex::new(0), daemon_pid, role })
    }

    /// Connects, starting a detached daemon (`<exe> daemon`) when none runs.
    pub fn connect_or_spawn(ep: &Endpoint, role: Role, name: &str) -> Result<Client, ConnectError> {
        match Client::connect(ep, role, name) {
            Err(ConnectError::NotRunning) => {}
            other => return other,
        }
        spawn_daemon()?;
        let end = Instant::now() + Duration::from_secs(3);
        loop {
            match Client::connect(ep, role, name) {
                Err(ConnectError::NotRunning) if Instant::now() < end => std::thread::sleep(Duration::from_millis(20)),
                other => return other,
            }
        }
    }

    pub fn send(&self, msg: &ClientMsg) -> io::Result<()> {
        self.shared.send(msg)
    }

    /// Waits for the reply `pick` accepts; other replies are dropped.
    fn wait_reply<T>(&self, timeout: Duration, mut pick: impl FnMut(DaemonMsg) -> Option<T>) -> io::Result<T> {
        let end = Instant::now() + timeout;
        loop {
            let left = end.saturating_duration_since(Instant::now());
            match self.replies.recv_timeout(left) {
                Ok(DaemonMsg::Denied { what }) => {
                    return Err(io::Error::new(io::ErrorKind::PermissionDenied, format!("denied: {what}")))
                }
                Ok(m) => {
                    if let Some(t) = pick(m) {
                        return Ok(t);
                    }
                }
                Err(_) => return Err(io::Error::new(io::ErrorKind::TimedOut, "no answer from the daemon")),
            }
        }
    }

    /// Waits until the daemon has handled everything sent before.
    pub fn sync(&self, timeout: Duration) -> bool {
        let _g = self.request.lock().unwrap_or_else(|p| p.into_inner());
        self.send(&ClientMsg::Sync).is_ok()
            && self
                .wait_reply(timeout, |m| match m {
                    DaemonMsg::Synced => Some(()),
                    _ => None,
                })
                .is_ok()
    }

    /// The session's last edit, before and after (the diff view).
    pub fn get_edit(&self, id: &str) -> io::Result<Option<Box<EditView>>> {
        let _g = self.request.lock().unwrap_or_else(|p| p.into_inner());
        self.send(&ClientMsg::GetEdit { session: id.to_owned() })?;
        self.wait_reply(Duration::from_secs(5), |m| match m {
            DaemonMsg::Edit { session, edit } if session == id => Some(edit),
            _ => None,
        })
    }

    /// A handle that can send friction reports from anywhere (a panic hook):
    /// it never blocks on a busy connection.
    pub fn reporter(&self) -> Reporter {
        Reporter(self.shared.clone())
    }

    /// Allows or stops observe-only readers for a session (Control only;
    /// DESIGN.md §6.1).
    pub fn set_observable(&self, id: &str, on: bool) -> io::Result<()> {
        self.send(&ClientMsg::SetObservable { session: id.to_owned(), on })
    }

    pub fn clear_attention(&self, id: &str) -> io::Result<()> {
        self.send(&ClientMsg::ClearAttention { session: id.to_owned() })
    }

    pub fn list_sessions(&self) -> io::Result<Vec<SessionInfo>> {
        let _g = self.request.lock().unwrap_or_else(|p| p.into_inner());
        self.send(&ClientMsg::ListSessions)?;
        self.wait_reply(Duration::from_secs(5), |m| match m {
            DaemonMsg::Sessions(v) => Some(v),
            _ => None,
        })
    }

    pub fn create(&self, spec: CreateSpec) -> io::Result<SessionId> {
        let mut g = self.request.lock().unwrap_or_else(|p| p.into_inner());
        *g += 1;
        let req = *g;
        self.send(&ClientMsg::Create { req, spec })?;
        self.wait_reply(Duration::from_secs(10), |m| match m {
            DaemonMsg::Created { req: r, session } if r == req => Some(Ok(session)),
            DaemonMsg::CreateFailed { req: r, error } if r == req => Some(Err(error)),
            _ => None,
        })?
        .map_err(io::Error::other)
    }

    /// Subscribes to a session: a full frame follows, then changes.
    pub fn attach(&self, id: &str, pid: Option<u32>) -> io::Result<RemoteSession> {
        let (tx, rx) = unbounded();
        self.shared
            .mirrors
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(id.to_owned(), Mirror { snap: Snapshot::default(), claude: None, woken: false, tx });
        self.send(&ClientMsg::Attach { session: id.to_owned() })?;
        Ok(RemoteSession {
            id: id.to_owned(),
            handle: RemoteHandle { id: id.to_owned(), shared: self.shared.clone(), pid },
            events: rx,
        })
    }

    /// Stops receiving frames and events for a session.
    pub fn detach(&self, id: &str) -> io::Result<()> {
        self.shared.mirrors.lock().unwrap_or_else(|p| p.into_inner()).remove(id);
        self.send(&ClientMsg::Detach { session: id.to_owned() })
    }

    /// Drops the connection at once, as if the process had died.
    pub fn disconnect(&self) {
        self.shared.writer.lock().unwrap_or_else(|p| p.into_inner()).shutdown();
    }

    /// The next reply that is not a frame or a session event (tests, CLI).
    pub fn next_reply(&self, timeout: Duration) -> Option<DaemonMsg> {
        self.replies.recv_timeout(timeout).ok()
    }

    pub fn close(&self, id: &str) -> io::Result<()> {
        self.send(&ClientMsg::Close { session: id.to_owned() })
    }
}

/// Dropping the client closes the connection (the reader thread holds a
/// clone of the stream, so it would otherwise stay open).
impl Drop for Client {
    fn drop(&mut self) {
        self.disconnect();
    }
}

fn reader_loop(mut r: Stream, sh: Arc<Shared>, replies: Sender<DaemonMsg>) {
    loop {
        let msg = match read_msg::<DaemonMsg>(&mut r) {
            Ok(m) => m,
            Err(e) => {
                log::info!("daemon connection closed: {e}");
                sh.lost.store(true, std::sync::atomic::Ordering::SeqCst);
                let mut ms = sh.mirrors.lock().unwrap_or_else(|p| p.into_inner());
                for m in ms.values() {
                    let _ = m.tx.send(SessionEvent::Error("lost the connection to the daemon".into()));
                    let _ = m.tx.send(SessionEvent::Exited(haetty_core::ExitStatus { code: None, signal: None }));
                }
                ms.clear();
                return;
            }
        };
        match msg {
            DaemonMsg::Frame(f) => {
                let mut ms = sh.mirrors.lock().unwrap_or_else(|p| p.into_inner());
                if let Some(m) = ms.get_mut(&f.session) {
                    apply_frame(&mut m.snap, &f);
                    if !m.woken {
                        m.woken = true;
                        let _ = m.tx.send(SessionEvent::Wakeup);
                    }
                }
            }
            DaemonMsg::Claude { session, info } => {
                let mut ms = sh.mirrors.lock().unwrap_or_else(|p| p.into_inner());
                if let Some(m) = ms.get_mut(&session) {
                    m.claude = Some(Arc::from(info));
                    if !m.woken {
                        m.woken = true;
                        let _ = m.tx.send(SessionEvent::Wakeup);
                    }
                }
            }
            DaemonMsg::Event { session, event } => {
                let ms = sh.mirrors.lock().unwrap_or_else(|p| p.into_inner());
                if let Some(m) = ms.get(&session) {
                    let _ = m.tx.send(wire_to_event(event));
                }
            }
            DaemonMsg::NoSession { session } => {
                let ms = sh.mirrors.lock().unwrap_or_else(|p| p.into_inner());
                if let Some(m) = ms.get(&session) {
                    let _ = m.tx.send(SessionEvent::Error(format!("session {session} is gone")));
                    let _ = m.tx.send(SessionEvent::Exited(haetty_core::ExitStatus { code: None, signal: None }));
                }
                let _ = replies.send(DaemonMsg::NoSession { session });
            }
            other => {
                if let DaemonMsg::Denied { what } = &other {
                    log::warn!("daemon denied: {what}");
                }
                let _ = replies.send(other);
            }
        }
    }
}

/// See [`Client::reporter`].
#[derive(Clone)]
pub struct Reporter(Arc<Shared>);

impl Reporter {
    /// Sends a friction report unless the connection is in use right now.
    pub fn friction(&self, kind: &str, fields: &[(&str, String)]) {
        let msg = ClientMsg::Friction {
            kind: kind.into(),
            fields: fields.iter().map(|(k, v)| (k.to_string(), v.clone())).collect(),
        };
        if let Ok(mut w) = self.0.writer.try_lock() {
            let _ = write_msg(&mut *w, &msg);
        }
    }
}

/// A session in the daemon, shaped like `haetty_core::Session`.
pub struct RemoteSession {
    pub id: SessionId,
    pub handle: RemoteHandle,
    pub events: Receiver<SessionEvent>,
}

/// Same methods as `haetty_core::SessionHandle`, except that every input
/// method takes its [`Origin`] (DESIGN.md §3.7): `Human` only for what a
/// person typed or clicked (OS input events), `Program` for input haetty or a
/// script made up (palette actions, restored tabs, automation). There is no
/// default: the caller has to say which.
#[derive(Clone)]
pub struct RemoteHandle {
    id: SessionId,
    shared: Arc<Shared>,
    pid: Option<u32>,
}

impl RemoteHandle {
    fn send(&self, msg: ClientMsg) {
        if let Err(e) = self.shared.send(&msg) {
            log::debug!("send to daemon: {e}");
        }
    }
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn backend(&self) -> &str {
        "daemon"
    }
    pub fn pid(&self) -> Option<u32> {
        self.pid
    }
    pub fn write(&self, origin: Origin, bytes: impl Into<Vec<u8>>) {
        self.send(ClientMsg::Write { session: self.id.clone(), origin, bytes: Secret(bytes.into()) });
    }
    pub fn key(&self, origin: Origin, k: KeyInput) {
        self.send(ClientMsg::Key { session: self.id.clone(), origin, key: WireKey::from(&k) });
    }
    pub fn mouse(&self, origin: Origin, m: MouseInput) {
        self.send(ClientMsg::Mouse { session: self.id.clone(), origin, mouse: WireMouse::from(&m) });
    }
    pub fn wheel(&self, origin: Origin, lines: i32, col: u16, row: u16, mods: Mods) {
        self.send(ClientMsg::Wheel { session: self.id.clone(), origin, lines, col, row, mods: mods.bits() });
    }
    pub fn paste(&self, origin: Origin, text: impl Into<String>) {
        self.send(ClientMsg::Paste { session: self.id.clone(), origin, text: Secret(text.into()) });
    }
    pub fn focus(&self, focused: bool) {
        self.send(ClientMsg::Focus { session: self.id.clone(), focused });
    }
    pub fn resize(&self, size: GridSize) {
        self.send(ClientMsg::Resize { session: self.id.clone(), size: size.into() });
    }
    pub fn scroll(&self, to: ScrollTo) {
        self.send(ClientMsg::Scroll { session: self.id.clone(), to: to.into() });
    }
    pub fn set_color_scheme(&self, s: ColorSchemePref) {
        let scheme = match s {
            ColorSchemePref::Dark => 0,
            ColorSchemePref::Light => 1,
            ColorSchemePref::Unknown => 2,
        };
        self.send(ClientMsg::SetColorScheme { session: self.id.clone(), scheme });
    }
    /// New theme colours for this running session (Control clients only).
    pub fn set_colors(&self, c: haetty_core::TermColors) {
        self.send(ClientMsg::SetTheme { session: self.id.clone(), colors: c.into() });
    }
    /// The Claude Code state of the session, if it runs Claude Code with
    /// the integration (a change comes as a `Wakeup`, like a frame).
    pub fn claude(&self) -> Option<Arc<ClaudeInfo>> {
        self.shared.mirrors.lock().unwrap_or_else(|p| p.into_inner()).get(&self.id)?.claude.clone()
    }

    /// The mirrored screen; asks the daemon for the next frame.
    pub fn snapshot(&self) -> Option<Snapshot> {
        let snap = {
            let mut ms = self.shared.mirrors.lock().unwrap_or_else(|p| p.into_inner());
            let m = ms.get_mut(&self.id)?;
            m.woken = false;
            m.snap.clone()
        };
        self.send(ClientMsg::NextFrame { session: self.id.clone() });
        Some(snap)
    }
}

/// Starts `<this exe> daemon` detached: Linux in a new session with stdio on
/// /dev/null; Windows without a console and out of the caller's job object
/// when the job allows it (DESIGN.md §3.1).
pub fn spawn_daemon() -> io::Result<()> {
    let exe = crate::product::exe_path();
    let mut cmd = std::process::Command::new(&exe);
    cmd.arg("daemon")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: setsid is async-signal-safe; called between fork and exec.
        unsafe {
            cmd.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        cmd.spawn()?;
        Ok(())
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_BREAKAWAY_FROM_JOB);
        match cmd.spawn() {
            Ok(_) => Ok(()),
            Err(e) if e.raw_os_error() == Some(5) => {
                // The caller's job object forbids breakaway: the daemon will
                // end with that job (e.g. when a launching terminal closes).
                log::warn!("daemon: job does not allow breakaway; starting it inside the job");
                cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
                cmd.spawn().map(|_| ())
            }
            Err(e) => Err(e),
        }
    }
}
