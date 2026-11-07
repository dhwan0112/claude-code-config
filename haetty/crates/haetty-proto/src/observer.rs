//! The observe-only client (DESIGN.md §3.7, §6.1): what `haetty events` and
//! `haetty screen` — and later remote adapters (§6) — are built on.
//!
//! The boundary is in the type, not only in the role: [`ObserverClient`] says
//! `Hello` with [`Role::Observe`] and can then send only the requests of
//! [`Request`] — the event log, one observable session's screen, the session
//! list. It has no method that takes a [`ClientMsg`], no session handle and
//! no input, create, close or settings path; nothing a caller passes can
//! become another message. (The daemon checks the role as well, so even a
//! hand-made Observe connection cannot type: §3.7.)

use std::io;
use std::time::Duration;

use crate::client::ConnectError;
use crate::paths::Endpoint;
use crate::transport::{dial, Dial, Stream};
use crate::wire::{read_msg, write_msg, ClientMsg, DaemonMsg, LogEvent, Role, ScreenView, SessionInfo, MAGIC, VERSION};

/// The only messages an observer can send.
enum Request {
    ReadEvents { cursor: u64, follow: bool },
    GetScreen { session: String },
    ListSessions,
}

impl From<Request> for ClientMsg {
    fn from(r: Request) -> ClientMsg {
        match r {
            Request::ReadEvents { cursor, follow } => ClientMsg::ReadEvents { cursor, follow },
            Request::GetScreen { session } => ClientMsg::GetScreen { session },
            Request::ListSessions => ClientMsg::ListSessions,
        }
    }
}

/// A read-only connection to the daemon (role Observe).
pub struct ObserverClient {
    stream: Stream,
    pub daemon_pid: u32,
}

impl ObserverClient {
    /// Connects as an observer; never starts a daemon.
    pub fn connect(ep: &Endpoint, name: &str) -> Result<ObserverClient, ConnectError> {
        let mut stream = match dial(ep, Duration::from_secs(2)) {
            Dial::Connected(s) => s,
            Dial::NotRunning => return Err(ConnectError::NotRunning),
            Dial::Failed(e) => return Err(ConnectError::Io(e)),
        };
        let hello = ClientMsg::Hello { magic: MAGIC, version: VERSION, role: Role::Observe, name: name.into() };
        write_msg(&mut stream, &hello)?;
        match read_msg::<DaemonMsg>(&mut stream)? {
            DaemonMsg::Welcome { daemon_pid, .. } => Ok(ObserverClient { stream, daemon_pid }),
            DaemonMsg::Refused { reason } => Err(ConnectError::Refused(reason)),
            _ => Err(ConnectError::Refused("unexpected answer to Hello".into())),
        }
    }

    fn send(&mut self, r: Request) -> io::Result<()> {
        let m: ClientMsg = r.into();
        write_msg(&mut self.stream, &m)
    }

    /// The next answer `pick` accepts (others, e.g. frames, are skipped);
    /// `Denied` and `NoSession` end the wait with an error.
    fn answer<T>(&mut self, mut pick: impl FnMut(DaemonMsg) -> Option<T>) -> io::Result<T> {
        loop {
            match read_msg::<DaemonMsg>(&mut self.stream)? {
                DaemonMsg::Denied { what } => return Err(io::Error::new(io::ErrorKind::PermissionDenied, what)),
                DaemonMsg::NoSession { session } => {
                    return Err(io::Error::new(io::ErrorKind::NotFound, format!("no session {session}")))
                }
                m => {
                    if let Some(t) = pick(m) {
                        return Ok(t);
                    }
                }
            }
        }
    }

    /// The events after `cursor` that observe-only readers may see, and the
    /// cursor to continue from.
    pub fn events(&mut self, cursor: u64) -> io::Result<(Vec<LogEvent>, u64)> {
        self.send(Request::ReadEvents { cursor, follow: false })?;
        self.answer(|m| match m {
            DaemonMsg::Events { events, cursor } => Some((events, cursor)),
            _ => None,
        })
    }

    /// Like [`events`](Self::events), then calls `each` for every later
    /// event as it is written, until `each` returns false or the daemon goes
    /// (`Ok` then: the daemon ended).
    pub fn follow(&mut self, cursor: u64, mut each: impl FnMut(&LogEvent) -> bool) -> io::Result<()> {
        self.send(Request::ReadEvents { cursor, follow: true })?;
        loop {
            let m = match read_msg::<DaemonMsg>(&mut self.stream) {
                Ok(m) => m,
                Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(()),
                Err(e) => return Err(e),
            };
            if let DaemonMsg::Events { events, .. } = m {
                for e in &events {
                    if !each(e) {
                        return Ok(());
                    }
                }
            } else if let DaemonMsg::Denied { what } = m {
                return Err(io::Error::new(io::ErrorKind::PermissionDenied, what));
            }
        }
    }

    /// An observable session's screen and state; `PermissionDenied` when the
    /// person has not allowed observing it.
    pub fn screen(&mut self, session: &str) -> io::Result<ScreenView> {
        self.send(Request::GetScreen { session: session.to_owned() })?;
        let want = session.to_owned();
        self.answer(|m| match m {
            DaemonMsg::Screen(v) if v.id == want => Some(*v),
            _ => None,
        })
    }

    /// The daemon's sessions (with their `observable` flag).
    pub fn sessions(&mut self) -> io::Result<Vec<SessionInfo>> {
        self.send(Request::ListSessions)?;
        self.answer(|m| match m {
            DaemonMsg::Sessions(v) => Some(v),
            _ => None,
        })
    }
}

#[cfg(test)]
mod tests {
    /// The observer's source names no message beyond its three requests and
    /// `Hello`: no input, create, close or settings message can be built here.
    #[test]
    fn observer_cannot_build_other_messages() {
        let src = include_str!("observer.rs");
        let code = &src[..src.find("#[cfg(test)]").unwrap()];
        let used: std::collections::BTreeSet<&str> = code
            .match_indices("ClientMsg::")
            .map(|(i, _)| {
                let rest = &code[i + "ClientMsg::".len()..];
                &rest[..rest.find(|c: char| !c.is_alphanumeric()).unwrap_or(rest.len())]
            })
            .collect();
        let allowed: std::collections::BTreeSet<&str> =
            ["Hello", "ReadEvents", "GetScreen", "ListSessions"].into_iter().collect();
        assert_eq!(used, allowed);
        assert!(!code.contains("RemoteHandle") && !code.contains("Origin"), "no input types");
    }
}
