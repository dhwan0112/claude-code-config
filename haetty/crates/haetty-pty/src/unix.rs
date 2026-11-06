use std::io;

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

    let reader = pair.master.try_clone_reader().map_err(err)?;
    let writer = pair.master.take_writer().map_err(err)?;
    Ok(Spawned {
        reader,
        writer,
        control: Box::new(UnixControl { master: pair.master, killer: std::sync::Mutex::new(child.clone_killer()) }),
        child: Box::new(UnixChild { inner: child }),
    })
}

struct UnixControl {
    master: Box<dyn MasterPty + Send>,
    /// Sends the child SIGHUP (portable-pty's `ChildKiller`).
    killer: std::sync::Mutex<Box<dyn portable_pty::ChildKiller + Send + Sync>>,
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
        // Discard what the child never read: a `write` blocked on a full
        // input queue returns (Linux does not wake it when the child's side
        // closes).
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
