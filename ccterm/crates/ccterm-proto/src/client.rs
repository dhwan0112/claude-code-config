//! A daemon client. [`RemoteSession`] has the shape of
//! `ccterm_core::Session` — a handle with the same methods plus a
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

use ccterm_core::{ColorSchemePref, GridSize, KeyInput, Mods, MouseInput, ScrollTo, SessionEvent, Snapshot};
use crossbeam_channel::{unbounded, Receiver, Sender};

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
                "a ccterm daemon of another version is running ({r}). It exits by itself 10 s after its                  last window closes; or end the `ccterm daemon` process (its sessions end with it)"
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
        let shared = Arc::new(Shared { mirrors: Mutex::new(HashMap::new()), writer: Mutex::new(stream) });
        let (rtx, rrx) = unbounded();
        let sh = shared.clone();
        std::thread::Builder::new().name("ccterm-client-reader".into()).spawn(move || reader_loop(reader, sh, rtx))?;
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
                let mut ms = sh.mirrors.lock().unwrap_or_else(|p| p.into_inner());
                for m in ms.values() {
                    let _ = m.tx.send(SessionEvent::Error("lost the connection to the daemon".into()));
                    let _ = m.tx.send(SessionEvent::Exited(ccterm_core::ExitStatus { code: None, signal: None }));
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
                    let _ = m.tx.send(SessionEvent::Exited(ccterm_core::ExitStatus { code: None, signal: None }));
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

/// A session in the daemon, shaped like `ccterm_core::Session`.
pub struct RemoteSession {
    pub id: SessionId,
    pub handle: RemoteHandle,
    pub events: Receiver<SessionEvent>,
}

/// Same methods as `ccterm_core::SessionHandle`; input is sent with
/// `Origin::Human` (the GUI forwards what the person typed).
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
    pub fn write(&self, bytes: impl Into<Vec<u8>>) {
        self.send(ClientMsg::Write { session: self.id.clone(), origin: Origin::Human, bytes: Secret(bytes.into()) });
    }
    pub fn key(&self, k: KeyInput) {
        self.send(ClientMsg::Key { session: self.id.clone(), origin: Origin::Human, key: WireKey::from(&k) });
    }
    pub fn mouse(&self, m: MouseInput) {
        self.send(ClientMsg::Mouse { session: self.id.clone(), origin: Origin::Human, mouse: WireMouse::from(&m) });
    }
    pub fn wheel(&self, lines: i32, col: u16, row: u16, mods: Mods) {
        self.send(ClientMsg::Wheel {
            session: self.id.clone(),
            origin: Origin::Human,
            lines,
            col,
            row,
            mods: mods.bits(),
        });
    }
    pub fn paste(&self, text: impl Into<String>) {
        self.send(ClientMsg::Paste { session: self.id.clone(), origin: Origin::Human, text: Secret(text.into()) });
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
