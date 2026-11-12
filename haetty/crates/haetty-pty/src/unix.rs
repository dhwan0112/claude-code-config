use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::sync::Arc;

use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize as PpSize};

use crate::{ChildProcess, CommandSpec, ExitStatus, PtyControl, PtySize, Spawned};

fn to_pp(s: PtySize) -> PpSize {
    PpSize { rows: s.rows, cols: s.cols, pixel_width: s.pixel_width, pixel_height: s.pixel_height }
}

fn err(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}

pub(crate) fn spawn(spec: &CommandSpec, size: PtySize) -> io::Result<Spawned> {
    let pair = native_pty_system().openpty(to_pp(size)).map_err(err)?;

    let mut cmd = CommandBuilder::new(&spec.program);
    cmd.args(&spec.args);
    match &spec.cwd {
        Some(d) => cmd.cwd(d),
        None => {
            if let Ok(d) = std::env::current_dir() {
                cmd.cwd(d)
            }
        }
    }
    for (k, v) in &spec.env {
        match v {
            Some(v) => cmd.env(k, v),
            None => cmd.env_remove(k),
        }
    }

    let child = pair.slave.spawn_command(cmd).map_err(err)?;
    // Close the slave side in the parent, or the master never sees EOF after
    // the child exits.
    drop(pair.slave);

    // Our own reader and writer on copies of the master descriptor, not
    // portable-pty's: its writer writes "\n" and EOF in `Drop` with a
    // blocking `write_all`, which never returns once the child is gone and
    // left its input queue full (a closed session kept its writer thread,
    // stage P1). The master is non-blocking; both wait in `poll` on it and
    // on the close pipe, whose write end the control holds: when the control
    // is dropped (the session ended), a waiting read returns end-of-file and
    // a waiting write fails, the master's last descriptor closes and the
    // kernel hangs up the child's terminal.
    let mfd = pair.master.as_raw_fd().ok_or_else(|| err("the PTY master has no descriptor"))?;
    let (close_r, close_w) = close_pipe()?;
    let close_r = Arc::new(close_r);
    let reader = MasterReader { fd: dup_cloexec(mfd)?, closed: close_r.clone() };
    let writer = MasterWriter { fd: dup_cloexec(mfd)?, closed: close_r };
    set_nonblocking(mfd)?;
    Ok(Spawned {
        reader: Box::new(reader),
        writer: Box::new(writer),
        control: Box::new(UnixControl {
            master: pair.master,
            killer: std::sync::Mutex::new(child.clone_killer()),
            _close: close_w,
        }),
        child: Box::new(UnixChild { inner: child }),
    })
}

fn dup_cloexec(fd: RawFd) -> io::Result<OwnedFd> {
    // SAFETY: F_DUPFD_CLOEXEC on our own open descriptor; the new one is ours.
    let n = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 0) };
    if n < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `n` is a fresh descriptor owned by nobody else.
    Ok(unsafe { OwnedFd::from_raw_fd(n) })
}

/// O_NONBLOCK on the master's open file (shared by all its copies).
fn set_nonblocking(fd: RawFd) -> io::Result<()> {
    // SAFETY: F_GETFL / F_SETFL on our own open descriptor.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// (read end, write end), both close-on-exec.
fn close_pipe() -> io::Result<(OwnedFd, OwnedFd)> {
    let mut p = [0 as RawFd; 2];
    // SAFETY: pipe2 fills `p` with two new descriptors on success.
    if unsafe { libc::pipe2(p.as_mut_ptr(), libc::O_CLOEXEC) } < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: both are fresh descriptors owned by nobody else.
    Ok(unsafe { (OwnedFd::from_raw_fd(p[0]), OwnedFd::from_raw_fd(p[1])) })
}

/// Waits until `fd` is ready for `events` (or has an error or hangup to
/// report) — `true` — or the close pipe's write end is gone — `false`.
fn wait_ready(fd: &OwnedFd, events: libc::c_short, closed: &OwnedFd) -> io::Result<bool> {
    let mut p = [
        libc::pollfd { fd: fd.as_raw_fd(), events, revents: 0 },
        libc::pollfd { fd: closed.as_raw_fd(), events: libc::POLLIN, revents: 0 },
    ];
    loop {
        // SAFETY: `p` is a valid array of two pollfd.
        if unsafe { libc::poll(p.as_mut_ptr(), 2, -1) } < 0 {
            let e = io::Error::last_os_error();
            if e.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(e);
        }
        return Ok(p[1].revents == 0);
    }
}

struct MasterReader {
    fd: OwnedFd,
    closed: Arc<OwnedFd>,
}

impl io::Read for MasterReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            // SAFETY: read into `buf` of its own length on our descriptor.
            let n = unsafe { libc::read(self.fd.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
            if n >= 0 {
                return Ok(n as usize);
            }
            let e = io::Error::last_os_error();
            match e.kind() {
                io::ErrorKind::Interrupted => {}
                io::ErrorKind::WouldBlock => {
                    if !wait_ready(&self.fd, libc::POLLIN, &self.closed)? {
                        return Ok(0); // the session ended
                    }
                }
                _ => return Err(e),
            }
        }
    }
}

struct MasterWriter {
    fd: OwnedFd,
    closed: Arc<OwnedFd>,
}

impl io::Write for MasterWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        loop {
            // SAFETY: write from `buf` of its own length on our descriptor.
            let n = unsafe { libc::write(self.fd.as_raw_fd(), buf.as_ptr().cast(), buf.len()) };
            if n >= 0 {
                return Ok(n as usize);
            }
            let e = io::Error::last_os_error();
            match e.kind() {
                io::ErrorKind::Interrupted => {}
                io::ErrorKind::WouldBlock => {
                    if !wait_ready(&self.fd, libc::POLLOUT, &self.closed)? {
                        return Err(io::Error::new(io::ErrorKind::BrokenPipe, "the terminal was closed"));
                    }
                }
                _ => return Err(e),
            }
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct UnixControl {
    master: Box<dyn MasterPty + Send>,
    /// Sends the child SIGHUP (portable-pty's `ChildKiller`).
    killer: std::sync::Mutex<Box<dyn portable_pty::ChildKiller + Send + Sync>>,
    /// The close pipe's write end: dropped with the control, it releases the
    /// reader and the writer.
    _close: OwnedFd,
}

impl PtyControl for UnixControl {
    fn resize(&self, size: PtySize) -> io::Result<()> {
        self.master.resize(to_pp(size)).map_err(err)
    }
    fn describe(&self) -> String {
        "unix-pty".into()
    }
    fn hangup(&self) {
        let mut k = self.killer.lock().unwrap_or_else(|p| p.into_inner());
        let _ = k.kill();
        // Discard what the child never read. A write waiting for room also
        // ends when the control is dropped (the close pipe); this flush only
        // spares the child a queue of stale input.
        if let Some(fd) = self.master.as_raw_fd() {
            // SAFETY: tcflush on our own open master descriptor.
            unsafe { libc::tcflush(fd, libc::TCIOFLUSH) };
        }
    }
}

struct UnixChild {
    inner: Box<dyn portable_pty::Child + Send + Sync>,
}

fn conv(s: portable_pty::ExitStatus) -> ExitStatus {
    ExitStatus { code: Some(s.exit_code()), signal: s.signal().map(str::to_owned) }
}

impl ChildProcess for UnixChild {
    fn pid(&self) -> Option<u32> {
        self.inner.process_id()
    }
    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.inner.try_wait().map(|o| o.map(conv))
    }
    fn wait(&mut self) -> io::Result<ExitStatus> {
        self.inner.wait().map(conv)
    }
    fn kill(&mut self) -> io::Result<()> {
        self.inner.kill()
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    fn sh(script: &str) -> Spawned {
        let mut spec = CommandSpec::new("sh");
        spec.args = vec!["-c".into(), script.into()];
        spawn(&spec, PtySize { cols: 80, rows: 24, pixel_width: 0, pixel_height: 0 }).unwrap()
    }

    /// Output still arrives and input still reaches the child.
    #[test]
    fn input_and_output_pass() {
        let s = sh("stty -echo; read l; echo got-$l");
        let mut r = s.reader;
        let mut w = s.writer;
        w.write_all(b"abc\n").unwrap();
        let mut seen = Vec::new();
        let mut buf = [0u8; 256];
        while !String::from_utf8_lossy(&seen).contains("got-abc") {
            let n = r.read(&mut buf).unwrap();
            assert!(n > 0, "end of output before the answer: {:?}", String::from_utf8_lossy(&seen));
            seen.extend_from_slice(&buf[..n]);
        }
        let mut child = s.child;
        child.wait().unwrap();
    }

    /// The regression of stage P1: a write blocked on a child that never
    /// reads — here a child already gone — and the writer's drop return once
    /// the session's control is dropped. With portable-pty's writer both
    /// blocked for good (`closed_sessions_leave_no_threads` kept a writer
    /// thread).
    #[test]
    fn dropped_control_releases_a_blocked_writer() {
        let s = sh("stty raw -echo; echo ready; exec sleep 30");
        let mut child = s.child;
        let mut w = s.writer;
        // Raw mode first: a canonical-mode terminal discards input it has no
        // room for instead of making the writer wait.
        let mut r = s.reader;
        let mut seen = Vec::new();
        let mut buf = [0u8; 256];
        while !String::from_utf8_lossy(&seen).contains("ready") {
            let n = r.read(&mut buf).unwrap();
            assert!(n > 0, "end of output before ready");
            seen.extend_from_slice(&buf[..n]);
        }
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let res = w.write_all(&vec![b'x'; 1 << 20]);
            drop(w);
            let _ = tx.send(res);
        });
        // The writer fills the child's input queue and waits.
        assert!(rx.recv_timeout(Duration::from_millis(500)).is_err(), "the write did not wait");
        child.kill().unwrap();
        child.wait().unwrap();
        std::thread::sleep(Duration::from_millis(100));
        drop(s.control);
        let res = rx.recv_timeout(Duration::from_secs(5)).expect("the writer is still blocked");
        assert_eq!(res.unwrap_err().kind(), io::ErrorKind::BrokenPipe);
        drop(r);
    }

    /// A read waiting on a child that prints nothing returns end-of-file once
    /// the control is dropped, and the child then gets its hangup (the last
    /// master descriptor closes).
    #[test]
    fn dropped_control_releases_the_reader_and_hangs_up() {
        let s = sh("exec sleep 30");
        let mut child = s.child;
        let mut r = s.reader;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut buf = [0u8; 256];
            let n = loop {
                match r.read(&mut buf) {
                    Ok(0) | Err(_) => break 0,
                    Ok(_) => {} // the shell's start-up output, if any
                }
            };
            drop(r);
            let _ = tx.send(n);
        });
        assert!(rx.recv_timeout(Duration::from_millis(300)).is_err(), "the read did not wait");
        drop(s.writer);
        drop(s.control);
        rx.recv_timeout(Duration::from_secs(5)).expect("the reader is still blocked");
        let (wtx, wrx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = wtx.send(child.wait());
        });
        let st = wrx.recv_timeout(Duration::from_secs(5)).expect("the child got no hangup").unwrap();
        assert_eq!(st.signal.as_deref(), Some("Hangup"), "{st:?}");
    }
}
