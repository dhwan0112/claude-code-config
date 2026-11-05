//! Local transport (DESIGN.md §3.5): a Unix socket on Linux, a per-user
//! named pipe on Windows. Both give a [`Stream`] (Read + Write, clonable so
//! one thread reads while another writes) and a [`Listener`].

use std::io;
use std::time::Duration;

use crate::paths::Endpoint;

#[cfg(unix)]
pub use unix::*;
#[cfg(windows)]
pub use win::*;

/// Result of trying to reach a daemon.
#[derive(Debug)]
pub enum Dial {
    Connected(Stream),
    /// Nobody listens (no socket/pipe, or a stale socket).
    NotRunning,
    Failed(io::Error),
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::os::unix::net::{UnixListener, UnixStream};

    pub struct Stream(UnixStream);

    impl Stream {
        pub fn try_clone(&self) -> io::Result<Stream> {
            Ok(Stream(self.0.try_clone()?))
        }
        pub fn shutdown(&self) {
            let _ = self.0.shutdown(std::net::Shutdown::Both);
        }
    }

    impl io::Read for Stream {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.0.read(buf)
        }
    }

    impl io::Write for Stream {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.write(buf)
        }
        fn flush(&mut self) -> io::Result<()> {
            self.0.flush()
        }
    }

    impl std::fmt::Debug for Stream {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("Stream(unix)")
        }
    }

    pub fn dial(ep: &Endpoint, _timeout: Duration) -> Dial {
        match UnixStream::connect(&ep.0) {
            Ok(s) => Dial::Connected(Stream(s)),
            Err(e) if matches!(e.kind(), io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused) => {
                Dial::NotRunning
            }
            Err(e) => Dial::Failed(e),
        }
    }

    pub struct Listener(UnixListener, std::path::PathBuf);

    impl Listener {
        /// Binds the socket. A stale socket file (nobody answers) is removed;
        /// a live one means another daemon runs: `AddrInUse`.
        pub fn bind(ep: &Endpoint) -> io::Result<Listener> {
            if let Some(dir) = ep.0.parent() {
                crate::paths::ensure_private_dir(dir)?;
            }
            match UnixListener::bind(&ep.0) {
                Ok(l) => Ok(Listener(l, ep.0.clone())),
                Err(e) if e.kind() == io::ErrorKind::AddrInUse => match UnixStream::connect(&ep.0) {
                    Ok(_) => Err(io::Error::new(io::ErrorKind::AddrInUse, "a daemon is already running")),
                    Err(_) => {
                        std::fs::remove_file(&ep.0)?;
                        Ok(Listener(UnixListener::bind(&ep.0)?, ep.0.clone()))
                    }
                },
                Err(e) => Err(e),
            }
        }
        pub fn accept(&self) -> io::Result<Stream> {
            Ok(Stream(self.0.accept()?.0))
        }
    }

    impl Drop for Listener {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.1);
        }
    }
}

#[cfg(windows)]
mod win {
    use super::*;
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::{null, null_mut};
    use std::sync::Arc;

    use windows_sys::Win32::Foundation::*;
    use windows_sys::Win32::Security::Authorization::*;
    use windows_sys::Win32::Security::*;
    use windows_sys::Win32::Storage::FileSystem::*;
    use windows_sys::Win32::System::Pipes::*;
    use windows_sys::Win32::System::Threading::*;
    use windows_sys::Win32::System::IO::*;

    struct Handle(HANDLE);
    // SAFETY: a pipe handle opened for overlapped I/O may be used from
    // several threads at once (each operation has its own OVERLAPPED).
    unsafe impl Send for Handle {}
    unsafe impl Sync for Handle {}
    impl Drop for Handle {
        fn drop(&mut self) {
            // SAFETY: we own the handle.
            unsafe { CloseHandle(self.0) };
        }
    }

    #[derive(Clone)]
    pub struct Stream(Arc<Handle>);

    impl std::fmt::Debug for Stream {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("Stream(pipe)")
        }
    }

    fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
        s.encode_wide().chain(Some(0)).collect()
    }

    /// Runs one overlapped operation to completion.
    fn overlapped(h: HANDLE, op: impl FnOnce(*mut OVERLAPPED) -> BOOL) -> io::Result<usize> {
        // SAFETY: the event and OVERLAPPED live until GetOverlappedResult
        // returned, i.e. until the operation is complete.
        unsafe {
            let ev = CreateEventW(null(), 1, 0, null());
            if ev.is_null() {
                return Err(io::Error::last_os_error());
            }
            let mut ov: OVERLAPPED = std::mem::zeroed();
            ov.hEvent = ev;
            let ok = op(&mut ov);
            if ok == 0 {
                let e = GetLastError();
                if e != ERROR_IO_PENDING {
                    CloseHandle(ev);
                    return if e == ERROR_BROKEN_PIPE || e == ERROR_PIPE_NOT_CONNECTED {
                        Ok(0)
                    } else {
                        Err(io::Error::from_raw_os_error(e as i32))
                    };
                }
            }
            let mut n = 0u32;
            let ok = GetOverlappedResult(h, &ov, &mut n, 1);
            let e = GetLastError();
            CloseHandle(ev);
            if ok == 0 {
                return if e == ERROR_BROKEN_PIPE || e == ERROR_PIPE_NOT_CONNECTED {
                    Ok(0)
                } else {
                    Err(io::Error::from_raw_os_error(e as i32))
                };
            }
            Ok(n as usize)
        }
    }

    impl Stream {
        pub fn try_clone(&self) -> io::Result<Stream> {
            Ok(self.clone())
        }
        pub fn shutdown(&self) {
            // SAFETY: cancels pending I/O on our own handle.
            unsafe {
                CancelIoEx(self.0 .0, null());
                DisconnectNamedPipe(self.0 .0);
            }
        }
    }

    impl io::Read for Stream {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let h = self.0 .0;
            let len = buf.len().min(u32::MAX as usize) as u32;
            // SAFETY: buf outlives the operation (overlapped waits for it).
            overlapped(h, |ov| unsafe { ReadFile(h, buf.as_mut_ptr(), len, null_mut(), ov) })
        }
    }

    impl io::Write for Stream {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            let h = self.0 .0;
            let len = buf.len().min(u32::MAX as usize) as u32;
            // SAFETY: buf outlives the operation.
            let n = overlapped(h, |ov| unsafe { WriteFile(h, buf.as_ptr(), len, null_mut(), ov) })?;
            if n == 0 && !buf.is_empty() {
                return Err(io::Error::new(io::ErrorKind::BrokenPipe, "pipe closed"));
            }
            Ok(n)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// The current user's SID as a string (`S-1-5-21-…`).
    pub fn current_user_sid() -> io::Result<String> {
        // SAFETY: standard token query; buffers sized by the first call.
        unsafe {
            let mut token: HANDLE = null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                return Err(io::Error::last_os_error());
            }
            let token = Handle(token);
            let mut len = 0u32;
            GetTokenInformation(token.0, TokenUser, null_mut(), 0, &mut len);
            let mut buf = vec![0u8; len as usize];
            if GetTokenInformation(token.0, TokenUser, buf.as_mut_ptr() as *mut c_void, len, &mut len) == 0 {
                return Err(io::Error::last_os_error());
            }
            let user = &*(buf.as_ptr() as *const TOKEN_USER);
            let mut s: *mut u16 = null_mut();
            if ConvertSidToStringSidW(user.User.Sid, &mut s) == 0 {
                return Err(io::Error::last_os_error());
            }
            let n = (0..).take_while(|&i| *s.add(i) != 0).count();
            let out = String::from_utf16_lossy(std::slice::from_raw_parts(s, n));
            LocalFree(s as *mut c_void);
            Ok(out)
        }
    }

    pub fn dial(ep: &Endpoint, timeout: Duration) -> Dial {
        let name = wide(ep.0.as_os_str());
        let end = std::time::Instant::now() + timeout;
        loop {
            // SAFETY: plain CreateFileW on a pipe name.
            let h = unsafe {
                CreateFileW(
                    name.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    0,
                    null(),
                    OPEN_EXISTING,
                    FILE_FLAG_OVERLAPPED,
                    null_mut(),
                )
            };
            if h != INVALID_HANDLE_VALUE {
                return Dial::Connected(Stream(Arc::new(Handle(h))));
            }
            // SAFETY: no preconditions.
            let e = unsafe { GetLastError() };
            match e {
                ERROR_FILE_NOT_FOUND => return Dial::NotRunning,
                ERROR_PIPE_BUSY if std::time::Instant::now() < end => {
                    // SAFETY: waits for a free instance, bounded.
                    unsafe { WaitNamedPipeW(name.as_ptr(), 100) };
                }
                _ => return Dial::Failed(io::Error::from_raw_os_error(e as i32)),
            }
        }
    }

    pub struct Listener {
        name: Vec<u16>,
        next: std::sync::Mutex<Option<Handle>>,
        sd: SecurityDescriptor,
    }

    struct SecurityDescriptor(PSECURITY_DESCRIPTOR);
    // SAFETY: an immutable security descriptor, read by CreateNamedPipeW.
    unsafe impl Send for SecurityDescriptor {}
    unsafe impl Sync for SecurityDescriptor {}
    impl Drop for SecurityDescriptor {
        fn drop(&mut self) {
            // SAFETY: allocated by ConvertStringSecurityDescriptor…W.
            unsafe { LocalFree(self.0) };
        }
    }

    impl Listener {
        /// Creates the first pipe instance (`FILE_FLAG_FIRST_PIPE_INSTANCE`:
        /// fails if the name exists, i.e. a daemon runs or someone squats it).
        pub fn bind(ep: &Endpoint) -> io::Result<Listener> {
            let sid = current_user_sid()?;
            let sddl = wide(std::ffi::OsStr::new(&format!("D:P(A;;GA;;;{sid})")));
            let mut sd: PSECURITY_DESCRIPTOR = null_mut();
            // SAFETY: SDDL string is NUL-terminated; sd freed in Drop.
            if unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    sddl.as_ptr(),
                    SDDL_REVISION_1,
                    &mut sd,
                    null_mut(),
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            let l = Listener {
                name: wide(ep.0.as_os_str()),
                next: std::sync::Mutex::new(None),
                sd: SecurityDescriptor(sd),
            };
            let first = l.instance(true).map_err(|e| {
                if e.raw_os_error() == Some(ERROR_ACCESS_DENIED as i32)
                    || e.raw_os_error() == Some(ERROR_PIPE_BUSY as i32)
                {
                    io::Error::new(io::ErrorKind::AddrInUse, "a daemon is already running (pipe exists)")
                } else {
                    e
                }
            })?;
            *l.next.lock().unwrap() = Some(first);
            Ok(l)
        }

        fn instance(&self, first: bool) -> io::Result<Handle> {
            let sa = SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: self.sd.0,
                bInheritHandle: 0,
            };
            let flags =
                PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED | if first { FILE_FLAG_FIRST_PIPE_INSTANCE } else { 0 };
            // SAFETY: valid name and security attributes.
            let h = unsafe {
                CreateNamedPipeW(
                    self.name.as_ptr(),
                    flags,
                    PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                    PIPE_UNLIMITED_INSTANCES,
                    64 * 1024,
                    64 * 1024,
                    0,
                    &sa,
                )
            };
            if h == INVALID_HANDLE_VALUE {
                return Err(io::Error::last_os_error());
            }
            Ok(Handle(h))
        }

        /// Waits for a client on the current instance and prepares the next.
        pub fn accept(&self) -> io::Result<Stream> {
            let h = match self.next.lock().unwrap().take() {
                Some(h) => h,
                None => self.instance(false)?,
            };
            let raw = h.0;
            // SAFETY: overlapped connect on our pipe instance; a client that
            // connected before the call gives ERROR_PIPE_CONNECTED at once
            // (the event is then never signalled, so no wait).
            unsafe {
                let ev = CreateEventW(null(), 1, 0, null());
                if ev.is_null() {
                    return Err(io::Error::last_os_error());
                }
                let mut ov: OVERLAPPED = std::mem::zeroed();
                ov.hEvent = ev;
                let ok = ConnectNamedPipe(raw, &mut ov);
                let e = GetLastError();
                let res = if ok != 0 || e == ERROR_PIPE_CONNECTED {
                    Ok(())
                } else if e == ERROR_IO_PENDING {
                    let mut n = 0u32;
                    if GetOverlappedResult(raw, &ov, &mut n, 1) == 0 {
                        Err(io::Error::last_os_error())
                    } else {
                        Ok(())
                    }
                } else {
                    Err(io::Error::from_raw_os_error(e as i32))
                };
                CloseHandle(ev);
                res?;
            }
            *self.next.lock().unwrap() = Some(self.instance(false)?);
            Ok(Stream(Arc::new(h)))
        }
    }
}
