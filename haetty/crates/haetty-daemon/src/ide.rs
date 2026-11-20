//! The editor server of one session (DESIGN.md §13.5, §13.9): a WebSocket listener on 127.0.0.1 for the session's
//! whole life, its lock file, and the proposals waiting for the person's answer. The protocol is in `haetty_cc::ide`.

use std::collections::HashMap;
use std::io;
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crossbeam_channel::Sender;
use haetty_cc::ide::{self, Call, Frame};
use haetty_cc::json::Json;
use haetty_proto::wire::{IdeProposal, Secret, SessionId};

use crate::Ev;

pub(crate) enum IdeEv {
    Open { conn: u64, rpc: Json, path: String, new_text: String, tab: String },
    CloseTab { conn: u64, tab: String },
    CloseAll { conn: u64 },
    Gone { conn: u64 },
}

struct Pending {
    id: u64,
    conn: u64,
    rpc: Json,
    tab: String,
    path: String,
    old: String,
    new: String,
}

type Conns = Arc<Mutex<HashMap<u64, TcpStream>>>;

pub(crate) struct Server {
    pub port: u16,
    lock: PathBuf,
    closed: Arc<AtomicBool>,
    conns: Conns,
    pending: Vec<Pending>,
    next: u64,
}

#[cfg(unix)]
fn random(b: &mut [u8]) -> io::Result<()> {
    use std::io::Read as _;
    std::fs::File::open("/dev/urandom")?.read_exact(b)
}

#[cfg(not(unix))]
fn random(_: &mut [u8]) -> io::Result<()> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "no editor server on this system yet"))
}

#[cfg(target_os = "linux")]
fn alive(pid: u32) -> bool {
    Path::new("/proc").join(pid.to_string()).exists()
}

#[cfg(not(target_os = "linux"))]
fn alive(_: u32) -> bool {
    true
}

fn remove_stale(dir: &Path) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.extension().is_some_and(|x| x == "lock")
            && std::fs::read_to_string(&p).is_ok_and(|t| ide::stale_lock(&t, alive))
        {
            let _ = std::fs::remove_file(&p);
        }
    }
}

fn bind() -> io::Result<TcpListener> {
    let mut last = io::Error::from(io::ErrorKind::AddrInUse);
    for _ in 0..32 {
        let mut b = [0u8; 2];
        random(&mut b)?;
        let port = 10000 + (u16::from_le_bytes(b) as u32 % 55536) as u16;
        match TcpListener::bind(("127.0.0.1", port)) {
            Ok(l) => return Ok(l),
            Err(e) => last = e,
        }
    }
    Err(last)
}

fn write_lock(dir: &Path, port: u16, text: &str) -> io::Result<PathBuf> {
    if !dir.exists() {
        std::fs::create_dir_all(dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
        }
    }
    let path = dir.join(format!("{port}.lock"));
    crate::write_private(&path, text.as_bytes())?;
    Ok(path)
}

fn send(conns: &Conns, conn: u64, text: &str) {
    let g = conns.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(mut s) = g.get(&conn) {
        if let Err(e) = ide::write_frame(&mut s, ide::OP_TEXT, text.as_bytes()) {
            log::info!("editor connection {conn}: {e}");
        }
    }
}

impl Server {
    /// Binds a port, writes the lock file and starts accepting. Any failure: no server (the hook-based diff stays).
    pub fn start(sid: &SessionId, lock_dir: &Path, tx: Sender<Ev>) -> io::Result<Server> {
        let token = ide::token(|b| random(b))?;
        let listener = bind()?;
        let port = listener.local_addr()?.port();
        remove_stale(lock_dir);
        let lock = write_lock(lock_dir, port, &ide::lock_json(std::process::id(), &token))?;
        let closed = Arc::new(AtomicBool::new(false));
        let conns: Conns = Arc::default();
        let (c2, k2, sid2) = (closed.clone(), conns.clone(), sid.clone());
        let started = std::thread::Builder::new()
            .name(format!("haetty-ide-{sid}"))
            .spawn(move || accept(listener, token, sid2, tx, c2, k2));
        if let Err(e) = started {
            let _ = std::fs::remove_file(&lock);
            return Err(e);
        }
        Ok(Server { port, lock, closed, conns, pending: Vec::new(), next: 1 })
    }

    /// The oldest proposal still waiting.
    pub fn current(&self) -> Option<IdeProposal> {
        self.pending.first().map(|p| IdeProposal {
            id: p.id,
            path: p.path.clone(),
            old: Secret(p.old.clone()),
            new: Secret(p.new.clone()),
        })
    }

    /// A proposal from Claude Code; `false` when it cannot be shown (the file is not read: the terminal answers).
    pub fn open(&mut self, conn: u64, rpc: Json, path: String, new_text: String, tab: String) -> bool {
        if new_text.len() > ide::MAX_BYTES {
            return false;
        }
        let snap = haetty_cc::edit::snapshot(&path);
        let Some(old) = haetty_cc::edit::base(&snap).map(str::to_string) else { return false };
        let id = self.next;
        self.next += 1;
        self.pending.push(Pending { id, conn, rpc, tab, path, old, new: new_text });
        true
    }

    /// The person's answer; `false` when that proposal is gone.
    pub fn answer(&mut self, id: u64, accept: bool) -> bool {
        let Some(i) = self.pending.iter().position(|p| p.id == id) else { return false };
        let p = self.pending.remove(i);
        let text = if accept { ide::accepted(&p.rpc, &p.new) } else { ide::rejected(&p.rpc, &p.tab) };
        send(&self.conns, p.conn, &text);
        true
    }

    /// Drops proposals Claude Code closed; `true` when one went.
    pub fn close(&mut self, e: &IdeEv) -> bool {
        let n = self.pending.len();
        match e {
            IdeEv::CloseTab { conn, tab } => self.pending.retain(|p| !(p.conn == *conn && p.tab == *tab)),
            IdeEv::CloseAll { conn } | IdeEv::Gone { conn } => self.pending.retain(|p| p.conn != *conn),
            IdeEv::Open { .. } => {}
        }
        self.pending.len() != n
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.closed.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect_timeout(&([127, 0, 0, 1], self.port).into(), Duration::from_millis(200));
        for s in self.conns.lock().unwrap_or_else(|p| p.into_inner()).values() {
            let _ = s.shutdown(Shutdown::Both);
        }
        let _ = std::fs::remove_file(&self.lock);
    }
}

fn accept(listener: TcpListener, token: String, sid: SessionId, tx: Sender<Ev>, closed: Arc<AtomicBool>, conns: Conns) {
    let mut next = 0u64;
    for s in listener.incoming() {
        if closed.load(Ordering::SeqCst) {
            return;
        }
        let Ok(s) = s else { continue };
        next += 1;
        let (token, sid, tx, conns, closed) = (token.clone(), sid.clone(), tx.clone(), conns.clone(), closed.clone());
        let id = next;
        let _ = std::thread::Builder::new()
            .name(format!("haetty-ide-{sid}-{id}"))
            .spawn(move || serve(s, id, &token, &sid, &tx, &conns, &closed));
    }
}

fn serve(s: TcpStream, conn: u64, token: &str, sid: &SessionId, tx: &Sender<Ev>, conns: &Conns, closed: &AtomicBool) {
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = s.set_write_timeout(Some(Duration::from_secs(2)));
    let mut r = &s;
    let reply = match ide::handshake(&mut r, token) {
        Ok(r) => r,
        Err(_) => return,
    };
    let mut w = &s;
    match reply {
        Ok(text) => {
            if io::Write::write_all(&mut w, text.as_bytes()).is_err() {
                return;
            }
        }
        Err(text) => {
            log::warn!("session {sid}: editor connection refused ({})", text.lines().next().unwrap_or_default());
            let _ = io::Write::write_all(&mut w, text.as_bytes());
            return;
        }
    }
    let Ok(writer) = s.try_clone() else { return };
    conns.lock().unwrap_or_else(|p| p.into_inner()).insert(conn, writer);
    if closed.load(Ordering::SeqCst) {
        let _ = s.shutdown(Shutdown::Both);
    }
    let _ = s.set_read_timeout(None);
    log::info!("session {sid}: editor connection {conn}");
    let version = env!("CARGO_PKG_VERSION");
    while let Ok(f) = ide::read_frame(&mut r) {
        let ev = match f {
            Frame::Text(t) => match ide::call(&t, version) {
                Call::Reply(text) => {
                    send(conns, conn, &text);
                    None
                }
                Call::Quiet => None,
                Call::OpenDiff { id, path, new_text, tab } => Some(IdeEv::Open { conn, rpc: id, path, new_text, tab }),
                Call::CloseTab { tab, reply } => {
                    send(conns, conn, &reply);
                    Some(IdeEv::CloseTab { conn, tab })
                }
                Call::CloseAll { reply } => {
                    send(conns, conn, &reply);
                    Some(IdeEv::CloseAll { conn })
                }
            },
            Frame::Ping(p) => {
                let g = conns.lock().unwrap_or_else(|p| p.into_inner());
                if let Some(mut s) = g.get(&conn) {
                    let _ = ide::write_frame(&mut s, ide::OP_PONG, &p);
                }
                None
            }
            Frame::Pong => None,
            Frame::Close => {
                let g = conns.lock().unwrap_or_else(|p| p.into_inner());
                if let Some(mut s) = g.get(&conn) {
                    let _ = ide::write_frame(&mut s, ide::OP_CLOSE, &[]);
                }
                break;
            }
        };
        if let Some(e) = ev {
            if tx.send(Ev::Ide(sid.clone(), e)).is_err() {
                break;
            }
        }
    }
    conns.lock().unwrap_or_else(|p| p.into_inner()).remove(&conn);
    log::info!("session {sid}: editor connection {conn} closed");
    let _ = tx.send(Ev::Ide(sid.clone(), IdeEv::Gone { conn }));
}
