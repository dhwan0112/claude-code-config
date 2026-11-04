//! ConPTY backend.
//!
//! Talks to the documented pseudoconsole API directly so that ccterm can pick
//! the ConPTY implementation and its creation flags (portable-pty 0.9 can do
//! neither). Host order: a side-loaded `conpty.dll` (`CCTERM_CONPTY_DLL`, or
//! next to the executable), the copy embedded at build time, kernel32.
//! `conpty.dll` starts the `OpenConsole.exe` in its own directory, so both
//! files are always written together.
//!
//! Status: compiles and links for `x86_64-pc-windows-gnu`; not yet run on a
//! Windows machine.

use std::ffi::{c_void, OsStr, OsString};
use std::fs::File;
use std::io;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::io::{FromRawHandle, RawHandle};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::Mutex;

use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, BOOL, FARPROC, HANDLE, HMODULE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0,
};
use windows_sys::Win32::System::Console::COORD;
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryW};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::{
    CreateProcessW, DeleteProcThreadAttributeList, GetExitCodeProcess, InitializeProcThreadAttributeList,
    TerminateProcess, UpdateProcThreadAttribute, WaitForSingleObject, CREATE_UNICODE_ENVIRONMENT,
    EXTENDED_STARTUPINFO_PRESENT, INFINITE, LPPROC_THREAD_ATTRIBUTE_LIST, PROCESS_INFORMATION, STARTF_USESTDHANDLES,
    STARTUPINFOEXW,
};

use crate::{ChildProcess, CommandSpec, ExitStatus, GlyphWidth, PtyControl, PtyOptions, PtySize, Spawned};

// Glyph-width flags of the out-of-band ConPTY (its conpty.h). The in-box host
// gets no flags: 0x8 had another meaning in older declarations.
const GLYPH_WIDTH_GRAPHEMES: u32 = 0x08;
const GLYPH_WIDTH_WCSWIDTH: u32 = 0x10;
const GLYPH_WIDTH_CONSOLE: u32 = 0x18;
// ProcThreadAttributeValue(22, FALSE, TRUE, FALSE)
const PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE: usize = 0x0002_0016;
const STILL_ACTIVE: u32 = 259;

type Hpcon = isize;
type HResult = i32;
type CreatePseudoConsoleFn = unsafe extern "system" fn(COORD, HANDLE, HANDLE, u32, *mut Hpcon) -> HResult;
type ResizePseudoConsoleFn = unsafe extern "system" fn(Hpcon, COORD) -> HResult;
type ClosePseudoConsoleFn = unsafe extern "system" fn(Hpcon);

struct ConPtyApi {
    create: CreatePseudoConsoleFn,
    resize: ResizePseudoConsoleFn,
    close: ClosePseudoConsoleFn,
    /// True for conpty.dll + OpenConsole (glyph-width flags apply).
    out_of_band: bool,
    origin: String,
}

fn wide(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(std::iter::once(0)).collect()
}

unsafe fn proc_addr(module: HMODULE, names: &[&[u8]]) -> FARPROC {
    for n in names {
        if let Some(f) = GetProcAddress(module, n.as_ptr()) {
            return Some(f);
        }
    }
    None
}

type RawFn = unsafe extern "system" fn() -> isize;

impl ConPtyApi {
    fn from_module(module: HMODULE, out_of_band: bool, origin: String) -> Option<Self> {
        if module.is_null() {
            return None;
        }
        unsafe {
            // conpty.dll exports both the Conpty* names (its conpty.h) and the
            // kernel32 names; kernel32 only the latter.
            let c = proc_addr(module, &[b"ConptyCreatePseudoConsole\0", b"CreatePseudoConsole\0"])?;
            let r = proc_addr(module, &[b"ConptyResizePseudoConsole\0", b"ResizePseudoConsole\0"])?;
            let k = proc_addr(module, &[b"ConptyClosePseudoConsole\0", b"ClosePseudoConsole\0"])?;
            Some(Self {
                create: std::mem::transmute::<RawFn, CreatePseudoConsoleFn>(c),
                resize: std::mem::transmute::<RawFn, ResizePseudoConsoleFn>(r),
                close: std::mem::transmute::<RawFn, ClosePseudoConsoleFn>(k),
                out_of_band,
                origin,
            })
        }
    }

    fn load_dll(path: &Path, origin: String) -> Option<Self> {
        if !path.is_file() {
            return None;
        }
        let module = unsafe { LoadLibraryW(wide(path.as_os_str()).as_ptr()) };
        let api = Self::from_module(module, true, origin);
        if api.is_none() {
            log::warn!("conpty: {} could not be loaded", path.display());
        }
        api
    }

    fn load(opts: &PtyOptions) -> io::Result<Self> {
        if opts.conpty_sideload {
            for path in sideload_candidates() {
                if let Some(api) = Self::load_dll(&path, format!("sideloaded {}", path.display())) {
                    return Ok(api);
                }
            }
        }
        #[cfg(feature = "embedded-conpty")]
        if opts.conpty_embedded {
            match embedded::install() {
                Ok(path) => {
                    if let Some(api) = Self::load_dll(&path, format!("embedded {}", conpty_bin_vendored::VERSION)) {
                        return Ok(api);
                    }
                }
                Err(e) => log::warn!("conpty: cannot write the embedded copy: {e}"),
            }
        }
        let k32 = unsafe { GetModuleHandleW(wide(OsStr::new("kernel32.dll")).as_ptr()) };
        Self::from_module(k32, false, "in-box conhost".into()).ok_or_else(|| {
            io::Error::other("CreatePseudoConsole is not available (Windows 10 1809 or later is required)")
        })
    }

    fn flags(&self, opts: &PtyOptions) -> u32 {
        if !self.out_of_band {
            return 0;
        }
        match opts.conpty_glyph_width {
            GlyphWidth::Graphemes => GLYPH_WIDTH_GRAPHEMES,
            GlyphWidth::Wcswidth => GLYPH_WIDTH_WCSWIDTH,
            GlyphWidth::Console => GLYPH_WIDTH_CONSOLE,
            GlyphWidth::HostDefault => 0,
        }
    }
}

#[cfg(feature = "embedded-conpty")]
mod embedded {
    use std::io::{self, Write};
    use std::path::{Path, PathBuf};

    /// Write conpty.dll, OpenConsole.exe and Microsoft's licence once per
    /// version to `%LOCALAPPDATA%\ccterm\conpty\<version>` and return the
    /// DLL path. Files already present with the right size are left alone
    /// (another ccterm may have them loaded).
    pub(super) fn install() -> io::Result<PathBuf> {
        let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let dir = base.join("ccterm").join("conpty").join(conpty_bin_vendored::VERSION);
        std::fs::create_dir_all(&dir)?;
        put(&dir.join("OpenConsole.exe"), conpty_bin_vendored::OPENCONSOLE_EXE)?;
        put(&dir.join("LICENSE-MICROSOFT.txt"), conpty_bin_vendored::MICROSOFT_LICENSE.as_bytes())?;
        let dll = dir.join("conpty.dll");
        put(&dll, conpty_bin_vendored::CONPTY_DLL)?;
        Ok(dll)
    }

    fn put(path: &Path, bytes: &[u8]) -> io::Result<()> {
        if std::fs::metadata(path).map(|m| m.len() == bytes.len() as u64).unwrap_or(false) {
            return Ok(());
        }
        let tmp = path.with_extension(format!("tmp{}", std::process::id()));
        {
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(bytes)?;
        }
        match std::fs::rename(&tmp, path) {
            Ok(()) => Ok(()),
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                // Lost a race with another process that wrote the same file.
                if path.is_file() {
                    Ok(())
                } else {
                    Err(e)
                }
            }
        }
    }
}

fn sideload_candidates() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(p) = std::env::var_os("CCTERM_CONPTY_DLL") {
        v.push(PathBuf::from(p));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            v.push(dir.join("conpty.dll"));
        }
    }
    v
}

fn coord(size: PtySize) -> COORD {
    COORD { X: size.cols.max(1) as i16, Y: size.rows.max(1) as i16 }
}

fn last_error(what: &str) -> io::Error {
    let code = unsafe { GetLastError() };
    io::Error::other(format!("{what} failed: {}", io::Error::from_raw_os_error(code as i32)))
}

struct OwnedH(HANDLE);
impl OwnedH {
    fn take(&mut self) -> HANDLE {
        std::mem::replace(&mut self.0, null_mut())
    }
}
impl Drop for OwnedH {
    fn drop(&mut self) {
        if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
            unsafe { CloseHandle(self.0) };
        }
    }
}

fn pipe() -> io::Result<(OwnedH, OwnedH)> {
    let mut r: HANDLE = null_mut();
    let mut w: HANDLE = null_mut();
    if unsafe { CreatePipe(&mut r, &mut w, null(), 0) } == 0 {
        return Err(last_error("CreatePipe"));
    }
    Ok((OwnedH(r), OwnedH(w)))
}

pub(crate) fn spawn(spec: &CommandSpec, size: PtySize, opts: &PtyOptions) -> io::Result<Spawned> {
    let api = ConPtyApi::load(opts)?;

    // ConPTY reads our input from `in_r` and writes the child's output to
    // `out_w`; we keep the other ends.
    let (mut in_r, mut in_w) = pipe()?;
    let (mut out_r, mut out_w) = pipe()?;

    let flags = api.flags(opts);
    let mut hpc: Hpcon = 0;
    let hr = unsafe { (api.create)(coord(size), in_r.0, out_w.0, flags, &mut hpc) };
    if hr < 0 {
        return Err(io::Error::other(format!("CreatePseudoConsole failed: HRESULT {hr:#x}")));
    }
    let pc = PseudoConsole { hpc, api };

    let child = spawn_child(spec, pc.hpc)?;

    // The pseudoconsole duplicated its ends; close ours so EOF propagates.
    in_r.take_and_close();
    out_w.take_and_close();

    let reader = unsafe { File::from_raw_handle(out_r.take() as RawHandle) };
    let writer = unsafe { File::from_raw_handle(in_w.take() as RawHandle) };
    let describe = format!("conpty {} flags={flags:#x}", pc.api.origin);
    Ok(Spawned {
        reader: Box::new(reader),
        writer: Box::new(writer),
        control: Box::new(ConPtyControl { pc: Mutex::new(pc), describe }),
        child: Box::new(child),
    })
}

impl OwnedH {
    fn take_and_close(&mut self) {
        let h = self.take();
        if !h.is_null() {
            unsafe { CloseHandle(h) };
        }
    }
}

struct PseudoConsole {
    hpc: Hpcon,
    api: ConPtyApi,
}

impl Drop for PseudoConsole {
    fn drop(&mut self) {
        // Closing the pseudoconsole ends the child's console session and makes
        // the output pipe report EOF. Older hosts may block here until the
        // output pipe is drained; ccterm keeps a reader thread on it.
        unsafe { (self.api.close)(self.hpc) };
    }
}

// The HPCON is an opaque handle owned by this struct; ConPTY functions may be
// called from any thread.
unsafe impl Send for PseudoConsole {}

struct ConPtyControl {
    pc: Mutex<PseudoConsole>,
    describe: String,
}

impl PtyControl for ConPtyControl {
    fn resize(&self, size: PtySize) -> io::Result<()> {
        let pc = self.pc.lock().unwrap();
        let hr = unsafe { (pc.api.resize)(pc.hpc, coord(size)) };
        if hr < 0 {
            return Err(io::Error::other(format!("ResizePseudoConsole failed: HRESULT {hr:#x}")));
        }
        Ok(())
    }
    fn describe(&self) -> String {
        self.describe.clone()
    }
}

struct WinChild {
    process: HANDLE,
    pid: u32,
}

unsafe impl Send for WinChild {}

impl Drop for WinChild {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.process) };
    }
}

impl WinChild {
    fn exit_code(&self) -> io::Result<u32> {
        let mut code = 0u32;
        if unsafe { GetExitCodeProcess(self.process, &mut code) } == 0 {
            return Err(last_error("GetExitCodeProcess"));
        }
        Ok(code)
    }
}

impl ChildProcess for WinChild {
    fn pid(&self) -> Option<u32> {
        Some(self.pid)
    }
    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        if unsafe { WaitForSingleObject(self.process, 0) } != WAIT_OBJECT_0 {
            return Ok(None);
        }
        let code = self.exit_code()?;
        Ok((code != STILL_ACTIVE).then_some(ExitStatus { code: Some(code), signal: None }))
    }
    fn wait(&mut self) -> io::Result<ExitStatus> {
        unsafe { WaitForSingleObject(self.process, INFINITE) };
        Ok(ExitStatus { code: Some(self.exit_code()?), signal: None })
    }
    fn kill(&mut self) -> io::Result<()> {
        if unsafe { TerminateProcess(self.process, 1) } == 0 {
            return Err(last_error("TerminateProcess"));
        }
        Ok(())
    }
}

struct AttrList {
    buf: Vec<u8>,
}

impl AttrList {
    fn new(hpc: Hpcon) -> io::Result<Self> {
        let mut size = 0usize;
        unsafe { InitializeProcThreadAttributeList(null_mut(), 1, 0, &mut size) };
        let mut buf = vec![0u8; size];
        let list = buf.as_mut_ptr() as LPPROC_THREAD_ATTRIBUTE_LIST;
        if unsafe { InitializeProcThreadAttributeList(list, 1, 0, &mut size) } == 0 {
            return Err(last_error("InitializeProcThreadAttributeList"));
        }
        // The attribute value is the HPCON itself (passed by value), as in
        // Microsoft's ConPTY samples.
        let ok: BOOL = unsafe {
            UpdateProcThreadAttribute(
                list,
                0,
                PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE,
                hpc as *const c_void,
                std::mem::size_of::<Hpcon>(),
                null_mut(),
                null(),
            )
        };
        if ok == 0 {
            return Err(last_error("UpdateProcThreadAttribute"));
        }
        Ok(Self { buf })
    }
    fn as_ptr(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.buf.as_mut_ptr() as LPPROC_THREAD_ATTRIBUTE_LIST
    }
}

impl Drop for AttrList {
    fn drop(&mut self) {
        unsafe { DeleteProcThreadAttributeList(self.as_ptr()) };
    }
}

fn spawn_child(spec: &CommandSpec, hpc: Hpcon) -> io::Result<WinChild> {
    let env = merged_env(&spec.env);
    let program = resolve_program(&spec.program, &env)?;
    let mut cmdline = build_cmdline(&program, &spec.args);
    let mut env_block = env_block(&env);
    let cwd = spec.cwd.clone().or_else(|| std::env::current_dir().ok());
    let cwd_w = cwd.as_ref().map(|p| wide(p.as_os_str()));

    let mut attrs = AttrList::new(hpc)?;
    let mut si: STARTUPINFOEXW = unsafe { std::mem::zeroed() };
    si.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
    // Without explicit (invalid) std handles the child could inherit ours
    // instead of attaching to the pseudoconsole.
    si.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    si.StartupInfo.hStdInput = INVALID_HANDLE_VALUE;
    si.StartupInfo.hStdOutput = INVALID_HANDLE_VALUE;
    si.StartupInfo.hStdError = INVALID_HANDLE_VALUE;
    si.lpAttributeList = attrs.as_ptr();

    let mut pi: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
    let ok = unsafe {
        CreateProcessW(
            null(),
            cmdline.as_mut_ptr(),
            null(),
            null(),
            0,
            EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT,
            env_block.as_mut_ptr() as *const c_void,
            cwd_w.as_ref().map_or(null(), |w| w.as_ptr()),
            &si.StartupInfo,
            &mut pi,
        )
    };
    if ok == 0 {
        return Err(last_error(&format!("CreateProcessW({})", program.display())));
    }
    unsafe { CloseHandle(pi.hThread) };
    Ok(WinChild { process: pi.hProcess, pid: pi.dwProcessId })
}

/// Parent environment with the spec's changes applied (keys compared
/// case-insensitively, as Windows does).
fn merged_env(changes: &[(OsString, Option<OsString>)]) -> Vec<(OsString, OsString)> {
    let mut env: Vec<(OsString, OsString)> = std::env::vars_os().collect();
    for (k, v) in changes {
        let ku = k.to_string_lossy().to_uppercase();
        env.retain(|(ek, _)| ek.to_string_lossy().to_uppercase() != ku);
        if let Some(v) = v {
            env.push((k.clone(), v.clone()));
        }
    }
    env
}

fn env_get<'a>(env: &'a [(OsString, OsString)], key: &str) -> Option<&'a OsString> {
    env.iter().find(|(k, _)| k.to_string_lossy().eq_ignore_ascii_case(key)).map(|(_, v)| v)
}

/// CreateProcessW wants the block sorted case-insensitively, `K=V\0` entries,
/// terminated by an extra NUL.
fn env_block(env: &[(OsString, OsString)]) -> Vec<u16> {
    let mut sorted: Vec<_> = env.to_vec();
    sorted.sort_by_key(|(k, _)| k.to_string_lossy().to_uppercase());
    let mut out = Vec::new();
    for (k, v) in sorted {
        out.extend(k.encode_wide());
        out.push(b'=' as u16);
        out.extend(v.encode_wide());
        out.push(0);
    }
    out.push(0);
    out
}

/// Find the executable the way a shell would: explicit paths as given,
/// otherwise each PATH entry with each PATHEXT extension.
fn resolve_program(program: &OsStr, env: &[(OsString, OsString)]) -> io::Result<PathBuf> {
    let p = Path::new(program);
    let has_ext = p.extension().is_some();
    let exts: Vec<String> = env_get(env, "PATHEXT")
        .map(|v| v.to_string_lossy().split(';').filter(|s| !s.is_empty()).map(str::to_owned).collect())
        .unwrap_or_else(|| vec![".COM".into(), ".EXE".into(), ".BAT".into(), ".CMD".into()]);
    let try_file = |base: &Path| -> Option<PathBuf> {
        if has_ext && base.is_file() {
            return Some(base.to_path_buf());
        }
        for e in &exts {
            let mut s = base.as_os_str().to_owned();
            s.push(e);
            let c = PathBuf::from(s);
            if c.is_file() {
                return Some(c);
            }
        }
        None
    };
    if p.components().count() > 1 || p.is_absolute() {
        return try_file(p)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("{} not found", p.display())));
    }
    if let Some(path) = env_get(env, "PATH") {
        for dir in std::env::split_paths(path) {
            if let Some(found) = try_file(&dir.join(p)) {
                return Ok(found);
            }
        }
    }
    Err(io::Error::new(io::ErrorKind::NotFound, format!("{} not found on PATH", p.display())))
}

/// Quote one argument for `CommandLineToArgvW` / the MSVC runtime.
fn quote_arg(arg: &OsStr, out: &mut Vec<u16>) {
    let s: Vec<u16> = arg.encode_wide().collect();
    let needs = s.is_empty()
        || s.iter().any(|&c| c == b' ' as u16 || c == b'\t' as u16 || c == b'"' as u16 || c == b'\n' as u16);
    if !needs {
        out.extend(s);
        return;
    }
    out.push(b'"' as u16);
    let mut backslashes = 0usize;
    for c in s {
        if c == b'\\' as u16 {
            backslashes += 1;
            continue;
        }
        let n = if c == b'"' as u16 { backslashes * 2 + 1 } else { backslashes };
        out.extend(std::iter::repeat_n(b'\\' as u16, n));
        backslashes = 0;
        out.push(c);
    }
    out.extend(std::iter::repeat_n(b'\\' as u16, backslashes * 2));
    out.push(b'"' as u16);
}

/// Command line for CreateProcessW. Batch files (npm installs `claude.cmd`)
/// cannot be started directly; they go through `cmd.exe /d /s /c "..."`.
fn build_cmdline(program: &Path, args: &[OsString]) -> Vec<u16> {
    let mut inner = Vec::new();
    quote_arg(program.as_os_str(), &mut inner);
    for a in args {
        inner.push(b' ' as u16);
        quote_arg(a, &mut inner);
    }
    let is_batch =
        program.extension().map(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat")).unwrap_or(false);
    let mut line = if is_batch {
        let comspec = std::env::var_os("ComSpec").unwrap_or_else(|| OsString::from("cmd.exe"));
        let mut l = Vec::new();
        quote_arg(&comspec, &mut l);
        l.extend(OsStr::new(" /d /s /c \"").encode_wide());
        l.extend(inner);
        l.push(b'"' as u16);
        l
    } else {
        inner
    };
    line.push(0);
    line
}

#[allow(dead_code)]
fn lossy(w: &[u16]) -> String {
    OsString::from_wide(w).to_string_lossy().into_owned()
}
