//! `haetty wait`: waits for one thing and prints one line about it (stage G3).
//!
//! * `pid PID`: the process ends (Linux: `/proc`; Windows: the process handle).
//! * `file PATH --contains TEXT`: the file has TEXT, counting from its start (so a text already there is found).
//! * `file PATH --changed`: the file appears, or its size or time changes.
//! * `screen SESSION --contains TEXT`: an observable tab's screen has TEXT (now or later).
//! * `event SESSION --stop`: an observable tab's turn ends (Claude Code's Stop or StopFailure hook), after the wait
//!   starts.
//! * `event SESSION --permission`: an observable tab asks for a permission answer (attention `Ask`), now or after the
//!   wait starts.
//!
//! Exit 0 when the condition holds; 124 when `--timeout` (600 seconds by default) passes first; 3 when the tab is not
//! observable (its screen and events are for observers only, as `haetty screen` and `haetty events`); 1 on another
//! error, a tab without Claude Code's integration (no turn events come from it), or a session that exits or is closed
//! before the event; 2 for a bad command line. The file, process and screen waits look again every 200 ms. The event
//! waits are sent by the daemon as the events are written, so they never read the event log again.

use haetty_proto::wire::{Attention, LogEvent, ScreenView};
use haetty_proto::ObserverClient;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant, SystemTime};

/// The exit code of a wait that timed out (as `timeout(1)`).
pub const EXIT_TIMEOUT: i32 = 124;
/// The exit code of a tab that is not observable.
pub const EXIT_NOT_OBSERVABLE: i32 = 3;
const POLL: Duration = Duration::from_millis(200);
const WAIT_USAGE: &str = "error: usage: haetty wait pid PID | file PATH (--contains TEXT | --changed) | \
     screen SESSION --contains TEXT | event SESSION (--stop | --permission) [--timeout SECONDS]";

/// What to wait for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cond {
    Pid(u32),
    FileContains { path: PathBuf, text: String },
    FileChanged(PathBuf),
    ScreenContains { session: String, text: String },
    EventStop(String),
    EventPermission(String),
}

impl Cond {
    /// The condition in words, for the timeout line.
    fn describe(&self) -> String {
        match self {
            Cond::Pid(p) => format!("pid {p} ends"),
            Cond::FileContains { path, text } => format!("file {} contains {text:?}", path.display()),
            Cond::FileChanged(path) => format!("file {} changes", path.display()),
            Cond::ScreenContains { session, text } => format!("screen of {session} shows {text:?}"),
            Cond::EventStop(s) => format!("session {s} ends its turn"),
            Cond::EventPermission(s) => format!("session {s} waits for a permission answer"),
        }
    }
}

/// The condition from the command line, which has no `--timeout` in it (the caller takes that out).
pub fn parse(args: &[String]) -> Result<Cond, String> {
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    let cond = match a.as_slice() {
        ["pid", p] => Cond::Pid(p.parse().map_err(|_| WAIT_USAGE.to_string())?),
        ["file", path, "--changed"] => Cond::FileChanged(PathBuf::from(*path)),
        ["file", path, "--contains", text] if !text.is_empty() => {
            Cond::FileContains { path: PathBuf::from(*path), text: text.to_string() }
        }
        ["screen", s, "--contains", text] if !text.is_empty() => {
            Cond::ScreenContains { session: s.to_string(), text: text.to_string() }
        }
        ["event", s, "--stop"] => Cond::EventStop(s.to_string()),
        ["event", s, "--permission"] => Cond::EventPermission(s.to_string()),
        _ => return Err(WAIT_USAGE.to_string()),
    };
    if cond == Cond::Pid(0) {
        return Err(WAIT_USAGE.to_string());
    }
    Ok(cond)
}

/// A failed check: the exit code and the line to print.
struct Fail {
    code: i32,
    msg: String,
}

fn fail(code: i32, msg: impl Into<String>) -> Fail {
    Fail { code, msg: msg.into() }
}

/// The line of a met condition, with the time it took.
fn met(what: &str, t0: Instant) -> (String, i32) {
    (format!("{what} after {:.1} s", t0.elapsed().as_secs_f64()), 0)
}

/// Whether the process has ended (or is a zombie waiting to be reaped).
#[cfg(target_os = "linux")]
fn pid_ended(pid: u32) -> Result<bool, String> {
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Err(_) => Ok(true),
        Ok(s) => {
            let state = s.rsplit_once(')').and_then(|(_, rest)| rest.trim_start().chars().next());
            Ok(matches!(state, Some('Z') | Some('X')))
        }
    }
}

/// Whether the process has ended: its handle is signalled, or there is no such process.
#[cfg(windows)]
fn pid_ended(pid: u32) -> Result<bool, String> {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ACCESS_DENIED, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE};
    // SAFETY: the handle is closed before returning; the arguments are plain values.
    unsafe {
        let h = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if h.is_null() {
            // No such process (it ended), or one we may not open, which still runs.
            return Ok(GetLastError() != ERROR_ACCESS_DENIED);
        }
        let ended = WaitForSingleObject(h, 0) == WAIT_OBJECT_0;
        CloseHandle(h);
        Ok(ended)
    }
}

/// Other systems have no `/proc` here, so the wait is not supported there.
#[cfg(all(not(windows), not(target_os = "linux")))]
fn pid_ended(_pid: u32) -> Result<bool, String> {
    Err("haetty wait pid is supported on Linux and Windows only".into())
}

/// The bytes of `file` from `offset` on.
fn read_from(file: &mut std::fs::File, offset: u64) -> std::io::Result<Vec<u8>> {
    file.seek(SeekFrom::Start(offset))?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)?;
    Ok(buf)
}

/// The length of the part of `bytes` that ends on a character boundary. The bytes after it start a UTF-8 character
/// that the writer has not finished yet.
fn complete_len(bytes: &[u8]) -> usize {
    let n = bytes.len();
    for back in 1..=n.min(4) {
        let b = bytes[n - back];
        if (b & 0xC0) == 0x80 {
            continue; // a continuation byte: its lead byte is further back
        }
        let need = match b {
            0x00..=0x7F => 1,
            0xC0..=0xDF => 2,
            0xE0..=0xEF => 3,
            0xF0..=0xF7 => 4,
            _ => 1, // not a lead byte: the lossy decoding deals with it
        };
        return if need > back { n - back } else { n };
    }
    n
}

/// A file read from its start, piece by piece as it grows.
struct FileText {
    path: PathBuf,
    text: String,
    /// The bytes read so far.
    offset: u64,
    /// The end of what was read, kept because a match may start there.
    tail: String,
    /// Bytes at the end that are the start of a character not finished yet.
    pending: Vec<u8>,
}

impl FileText {
    fn new(path: PathBuf, text: String) -> FileText {
        FileText { path, text, offset: 0, tail: String::new(), pending: Vec::new() }
    }

    /// Reads what the file has grown by; true when the text is in the file now or was earlier.
    fn check(&mut self) -> Result<bool, Fail> {
        let mut file = match std::fs::File::open(&self.path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(fail(1, format!("haetty wait: {}: {e}", self.path.display()))),
        };
        if file.metadata().map(|m| m.len()).unwrap_or(0) < self.offset {
            // The file was replaced or truncated: read it again from the start.
            self.offset = 0;
            self.tail.clear();
            self.pending.clear();
        }
        let buf = read_from(&mut file, self.offset)
            .map_err(|e| fail(1, format!("haetty wait: {}: {e}", self.path.display())))?;
        self.offset += buf.len() as u64;
        let mut bytes = std::mem::take(&mut self.pending);
        bytes.extend_from_slice(&buf);
        let complete = complete_len(&bytes);
        self.pending = bytes.split_off(complete);
        let seen = format!("{}{}", self.tail, String::from_utf8_lossy(&bytes));
        if seen.contains(&self.text) {
            return Ok(true);
        }
        // A match can start only in the last `text` bytes of what was seen, so keep those.
        let mut cut = seen.len().saturating_sub(self.text.len());
        while !seen.is_char_boundary(cut) {
            cut += 1;
        }
        self.tail = seen[cut..].to_string();
        Ok(false)
    }
}

/// The size and time of a file, or None when there is no such file.
fn meta_of(path: &std::path::Path) -> Option<(u64, Option<SystemTime>)> {
    std::fs::metadata(path).ok().map(|m| (m.len(), m.modified().ok()))
}

fn observer() -> Result<ObserverClient, Fail> {
    crate::cli::observer("wait").map_err(|e| fail(1, format!("haetty wait: {e}")))
}

/// The failure of a read from the daemon: a tab that is not observable exits 3, anything else 1.
fn read_error(session: &str, e: std::io::Error) -> Fail {
    if e.kind() == std::io::ErrorKind::PermissionDenied {
        fail(EXIT_NOT_OBSERVABLE, format!("haetty wait: session {session} is not observable (observing it is off)"))
    } else {
        fail(1, format!("haetty wait: {e}"))
    }
}

/// Reads the tab's screen, which the daemon gives only for a tab the person allowed to be observed.
fn observed(client: &mut ObserverClient, session: &str) -> Result<ScreenView, Fail> {
    client.screen(session).map_err(|e| read_error(session, e))
}

/// A condition that is looked at again every `POLL`.
enum Waiting {
    Pid(u32),
    Text(FileText),
    Changed { path: PathBuf, first: Option<(u64, Option<SystemTime>)> },
    Screen { client: ObserverClient, session: String, text: String },
}

impl Waiting {
    fn start(cond: &Cond) -> Result<Waiting, Fail> {
        Ok(match cond {
            Cond::Pid(p) => Waiting::Pid(*p),
            Cond::FileContains { path, text } => Waiting::Text(FileText::new(path.clone(), text.clone())),
            Cond::FileChanged(path) => Waiting::Changed { path: path.clone(), first: meta_of(path) },
            Cond::ScreenContains { session, text } => {
                let mut client = observer()?;
                observed(&mut client, session)?;
                Waiting::Screen { client, session: session.clone(), text: text.clone() }
            }
            Cond::EventStop(_) | Cond::EventPermission(_) => {
                return Err(fail(1, "haetty wait: an event is streamed as it is written, not polled"));
            }
        })
    }

    /// `Some(what happened)` when the condition holds now.
    fn check(&mut self) -> Result<Option<String>, Fail> {
        match self {
            Waiting::Pid(pid) => {
                let p = *pid;
                let ended = pid_ended(p).map_err(|e| fail(1, e))?;
                Ok(ended.then(|| format!("pid {p} ended")))
            }
            Waiting::Text(t) => {
                let found = t.check()?;
                Ok(found.then(|| format!("file {} contains {:?}", t.path.display(), t.text)))
            }
            Waiting::Changed { path, first } => {
                let now = meta_of(path);
                Ok((now != *first).then(|| format!("file {} changed", path.display())))
            }
            Waiting::Screen { client, session, text } => match client.screen(session.as_str()) {
                Ok(v) => Ok(v.text.0.contains(text.as_str()).then(|| format!("screen of {session} shows {text:?}"))),
                Err(e) => Err(read_error(session.as_str(), e)),
            },
        }
    }
}

/// The timeout line and its exit code.
fn timed_out(cond: &Cond, timeout: Duration) -> (String, i32) {
    (format!("timed out after {} s: {}", timeout.as_secs(), cond.describe()), EXIT_TIMEOUT)
}

/// Waits for `cond` until `timeout` has passed since `t0`; returns the one line to print and the exit code.
pub fn wait(cond: &Cond, timeout: Duration) -> (String, i32) {
    let t0 = Instant::now();
    if let Cond::EventStop(s) | Cond::EventPermission(s) = cond {
        let (view, events) = match watch_events(s) {
            Ok(v) => v,
            Err(f) => return (f.msg, f.code),
        };
        let Some(claude) = view.claude else {
            return (
                format!("haetty wait: session {s} has no Claude Code integration: no turn or permission events come from it"),
                1,
            );
        };
        if matches!(cond, Cond::EventPermission(_)) && claude.attention == Attention::Ask {
            return met(&format!("session {s} waits for a permission answer"), t0);
        }
        return wait_events(&events, cond, t0, timeout);
    }
    let mut waiting = match Waiting::start(cond) {
        Ok(w) => w,
        Err(f) => return (f.msg, f.code),
    };
    loop {
        match waiting.check() {
            Ok(Some(what)) => return met(&what, t0),
            Ok(None) => {}
            Err(f) => return (f.msg, f.code),
        }
        let left = timeout.saturating_sub(t0.elapsed());
        if left.is_zero() {
            return timed_out(cond, timeout);
        }
        std::thread::sleep(POLL.min(left));
    }
}

/// Connects as an observer, checks that the tab is observable, and streams the tab's events written from now on. The
/// daemon sends each one as it is written; the thread ends with the daemon, or when the receiver is dropped.
fn watch_events(session: &str) -> Result<(ScreenView, Receiver<LogEvent>), Fail> {
    let mut client = observer()?;
    let view = observed(&mut client, session)?;
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        // Cursor u64::MAX: nothing written before now is sent, only what is written from now on.
        let _ = client.follow(u64::MAX, |e| tx.send(e.clone()).is_ok());
    });
    Ok((view, rx))
}

/// Waits for the first event of `events` that says what `cond` waits for; the timeout runs from `t0`.
fn wait_events(events: &Receiver<LogEvent>, cond: &Cond, t0: Instant, timeout: Duration) -> (String, i32) {
    loop {
        let left = timeout.saturating_sub(t0.elapsed());
        if left.is_zero() {
            return timed_out(cond, timeout);
        }
        match events.recv_timeout(left) {
            Ok(e) => {
                if let Some(what) = event_hit(&e, cond) {
                    return met(&what, t0);
                }
                if let Some(session) = ended_session(&e, cond) {
                    return (format!("haetty wait: session {session} ended before the event"), 1);
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                return (
                    "haetty wait: the event stream ended (the daemon stopped, or the tab is no longer observable)"
                        .into(),
                    1,
                );
            }
        }
    }
}

/// The value of field `key` of the event, if it has one.
fn field<'a>(e: &'a LogEvent, key: &str) -> Option<&'a str> {
    e.fields.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

/// What `e` says about the tab that `cond` waits for: its turn ended (`attention` with `event` Stop or StopFailure),
/// or it asks for a permission answer (`attention` is `Ask`).
fn event_hit(e: &LogEvent, cond: &Cond) -> Option<String> {
    let (session, stop) = match cond {
        Cond::EventStop(s) => (s.as_str(), true),
        Cond::EventPermission(s) => (s.as_str(), false),
        _ => return None,
    };
    if e.kind != "attention" || e.session != session {
        return None;
    }
    if stop && matches!(field(e, "event"), Some("Stop") | Some("StopFailure")) {
        return Some(format!("session {session} ends its turn"));
    }
    if !stop && field(e, "attention") == Some("Ask") {
        return Some(format!("session {session} waits for a permission answer"));
    }
    None
}

/// The session of `cond`, when `e` says that it ended (its process exited, or it was closed): its turn cannot end now.
fn ended_session(e: &LogEvent, cond: &Cond) -> Option<String> {
    let session = match cond {
        Cond::EventStop(s) | Cond::EventPermission(s) => s,
        _ => return None,
    };
    let ended = matches!(e.kind.as_str(), "session_exited" | "session_closed");
    (ended && e.session == *session).then(|| session.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    fn event(kind: &str, session: &str, fields: &[(&str, &str)]) -> LogEvent {
        LogEvent {
            seq: 1,
            ms: 0,
            kind: kind.into(),
            session: session.into(),
            fields: fields.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        }
    }

    #[test]
    fn the_command_line_is_parsed_or_refused_with_usage() {
        assert_eq!(parse(&words("pid 12")).unwrap(), Cond::Pid(12));
        assert_eq!(
            parse(&words("file a.log --contains done")).unwrap(),
            Cond::FileContains { path: "a.log".into(), text: "done".into() }
        );
        assert_eq!(parse(&words("file a.log --changed")).unwrap(), Cond::FileChanged("a.log".into()));
        assert_eq!(parse(&words("event t1 --stop")).unwrap(), Cond::EventStop("t1".into()));
        assert_eq!(parse(&words("event t1 --permission")).unwrap(), Cond::EventPermission("t1".into()));
        for bad in ["pid", "pid 0", "pid x", "file a", "file a --contains", "event t1", "screen t1"] {
            let e = parse(&words(bad)).unwrap_err();
            assert!(e.starts_with("error: usage: haetty wait"), "{bad}: {e}");
        }
    }

    #[test]
    fn a_character_still_being_written_waits_for_its_last_bytes() {
        assert_eq!(complete_len(b"abc"), 3);
        assert_eq!(complete_len(&[b'a', 0xC3]), 1);
        assert_eq!(complete_len("\u{c644}".as_bytes()), 3);
        assert_eq!(complete_len(&"\u{c644}".as_bytes()[..2]), 0);
    }

    #[test]
    fn a_file_text_counts_from_its_start_and_across_reads() {
        let dir = haetty_testkit::run_dir("wait-file-text");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("log.txt");
        std::fs::write(&path, "first line\nsec").unwrap();
        let mut t = FileText::new(path.clone(), "second".into());
        assert!(!t.check().ok().unwrap());
        // the word arrives in a second append: found once the second part is there
        let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        std::io::Write::write_all(&mut f, b"ond\n").unwrap();
        drop(f);
        assert!(t.check().ok().unwrap());
        // a text already there is found from the start
        let mut u = FileText::new(path.clone(), "first".into());
        assert!(u.check().ok().unwrap());
        haetty_testkit::remove_dir_all(&dir);
    }

    #[test]
    fn a_text_cut_inside_a_character_is_still_found() {
        let dir = haetty_testkit::run_dir("wait-file-utf8");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("u.txt");
        // "é" takes two bytes; the first read stops between them
        let bytes = "a\u{e9}b\n".as_bytes();
        std::fs::write(&path, &bytes[..2]).unwrap();
        let mut t = FileText::new(path.clone(), "a\u{e9}b".into());
        assert!(!t.check().ok().unwrap());
        let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        std::io::Write::write_all(&mut f, &bytes[2..]).unwrap();
        drop(f);
        assert!(t.check().ok().unwrap());
        haetty_testkit::remove_dir_all(&dir);
    }

    #[test]
    fn a_changed_file_and_a_timeout() {
        let dir = haetty_testkit::run_dir("wait-changed");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("f.txt");
        std::fs::write(&path, "a").unwrap();
        let cond = Cond::FileChanged(path.clone());
        let mut w = Waiting::start(&cond).ok().unwrap();
        assert!(w.check().ok().unwrap().is_none());
        std::fs::write(&path, "abc").unwrap();
        assert!(w.check().ok().unwrap().is_some());
        // nothing ever contains the text: the wait gives up with 124
        let missing = Cond::FileContains { path: dir.join("never.txt"), text: "x".into() };
        let (msg, code) = wait(&missing, Duration::from_millis(400));
        assert_eq!(code, EXIT_TIMEOUT, "{msg}");
        assert!(msg.starts_with("timed out after 0 s: file"), "{msg}");
        haetty_testkit::remove_dir_all(&dir);
    }

    #[test]
    fn an_event_counts_only_when_it_says_what_was_asked() {
        let stop = Cond::EventStop("t1".into());
        let ask = Cond::EventPermission("t1".into());
        let turn_end = event("attention", "t1", &[("event", "Stop"), ("attention", "Wait")]);
        let asking = event("attention", "t1", &[("event", "Notification"), ("attention", "Ask")]);
        let other_tab = event("attention", "t2", &[("event", "Stop"), ("attention", "Wait")]);
        let not_attention = event("latency", "t1", &[("event", "Stop")]);
        assert!(event_hit(&turn_end, &stop).is_some());
        assert!(event_hit(&turn_end, &ask).is_none());
        assert!(event_hit(&asking, &ask).is_some());
        assert!(event_hit(&asking, &stop).is_none());
        assert!(event_hit(&other_tab, &stop).is_none());
        assert!(event_hit(&not_attention, &stop).is_none());
    }

    #[test]
    fn a_failed_turn_counts_as_a_turn_end_and_an_ended_session_is_seen() {
        let stop = Cond::EventStop("t1".into());
        let failed = event("attention", "t1", &[("event", "StopFailure"), ("attention", "Wait")]);
        assert!(event_hit(&failed, &stop).is_some());
        let exited = event("session_exited", "t1", &[("code", "0")]);
        assert_eq!(ended_session(&exited, &stop).as_deref(), Some("t1"));
        assert!(ended_session(&event("session_exited", "t2", &[]), &stop).is_none());
        assert!(ended_session(&event("attention", "t1", &[]), &stop).is_none());
    }

    #[test]
    fn the_event_wait_returns_on_a_match_times_out_or_reports_a_closed_stream() {
        let cond = Cond::EventStop("t1".into());
        let (tx, rx) = channel();
        tx.send(event("attention", "t2", &[("event", "Stop")])).unwrap();
        tx.send(event("attention", "t1", &[("event", "Stop"), ("attention", "Wait")])).unwrap();
        let (msg, code) = wait_events(&rx, &cond, Instant::now(), Duration::from_secs(5));
        assert_eq!(code, 0, "{msg}");
        assert!(msg.starts_with("session t1 ends its turn after"), "{msg}");

        let (msg, code) = wait_events(&rx, &cond, Instant::now(), Duration::from_millis(100));
        assert_eq!(code, EXIT_TIMEOUT, "{msg}");

        drop(tx);
        let (msg, code) = wait_events(&rx, &cond, Instant::now(), Duration::from_secs(5));
        assert_eq!(code, 1, "{msg}");
        assert!(msg.starts_with("haetty wait: the event stream ended"), "{msg}");
    }

    #[test]
    fn a_session_that_exits_ends_the_event_wait_at_once() {
        let cond = Cond::EventPermission("t1".into());
        let (tx, rx) = channel();
        tx.send(event("session_exited", "t1", &[("code", "0")])).unwrap();
        let (msg, code) = wait_events(&rx, &cond, Instant::now(), Duration::from_secs(5));
        assert_eq!(code, 1, "{msg}");
        assert!(msg.starts_with("haetty wait: session t1 ended before the event"), "{msg}");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_process_end_is_seen() {
        let mut child = std::process::Command::new("sh").args(["-c", "sleep 0.3"]).spawn().unwrap();
        let pid = child.id();
        let (msg, code) = wait(&Cond::Pid(pid), Duration::from_secs(20));
        assert_eq!(code, 0, "{msg}");
        assert!(msg.starts_with(&format!("pid {pid} ended")), "{msg}");
        let _ = child.wait();
    }
}
