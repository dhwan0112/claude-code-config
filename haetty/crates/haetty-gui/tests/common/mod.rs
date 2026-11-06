//! Shared by the integration tests that run a real `haetty daemon` process on
//! Linux and Windows (T5): the binary, an endpoint, the daemon, and process
//! queries (alive, threads, children) without extra dependencies — `/proc`
//! on Linux, PowerShell / `tasklist` on Windows.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use haetty_proto::paths::Endpoint;
use haetty_proto::wire::{CreateSpec, Grid};
use haetty_proto::{Client, Role};

/// The `haetty` binary: `HAETTY_TEST_BIN` when set (a Windows test
/// executable built in WSL carries a Linux path in `CARGO_BIN_EXE_haetty`),
/// else the one cargo built.
pub fn bin() -> PathBuf {
    std::env::var_os("HAETTY_TEST_BIN").map_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_haetty")), PathBuf::from)
}

/// A private endpoint: a socket in `dir/rt` (0700) on Unix, a named pipe on
/// Windows.
pub fn endpoint(dir: &Path, name: &str) -> Endpoint {
    #[cfg(unix)]
    {
        let _ = name;
        use std::os::unix::fs::PermissionsExt;
        let rt = dir.join("rt");
        std::fs::create_dir_all(&rt).unwrap();
        std::fs::set_permissions(&rt, std::fs::Permissions::from_mode(0o700)).unwrap();
        Endpoint(rt.join("d.sock"))
    }
    #[cfg(windows)]
    {
        // The daemon writes Claude Code settings files into dir/rt.
        std::fs::create_dir_all(dir.join("rt")).unwrap();
        Endpoint(format!(r"\\.\pipe\haetty-test-{name}-{}", std::process::id()).into())
    }
}

/// `haetty daemon` as a dev instance on `ep`, its state in `dir/state`.
pub fn daemon(dir: &Path, ep: &Endpoint) -> Child {
    let mut c = Command::new(bin());
    c.arg("daemon")
        .env("HAETTY_INSTANCE", "dev")
        .env("HAETTY_SOCKET", &ep.0)
        .env("HAETTY_STATE_DIR", dir.join("state"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    c.env("XDG_RUNTIME_DIR", dir.join("rt"));
    c.spawn().expect("start daemon")
}

pub fn connect(ep: &Endpoint, name: &str) -> Client {
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        match Client::connect(ep, Role::Control, name) {
            Ok(c) => return c,
            Err(_) if Instant::now() < end => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => panic!("daemon: {e}"),
        }
    }
}

pub fn spec(program: &str, args: &[&str]) -> CreateSpec {
    CreateSpec {
        program: program.into(),
        args: args.iter().map(|a| a.to_string()).collect(),
        cwd: None,
        env: vec![],
        size: Grid { cols: 40, rows: 8, cell_width: 9, cell_height: 18 },
        scrollback_limit: 100_000,
        default_fg: [200, 200, 200],
        default_bg: [20, 20, 20],
        palette: vec![],
        cursor_color: None,
        claude_integration: false,
    }
}

#[cfg(windows)]
fn powershell(cmd: &str) -> String {
    let out = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", cmd])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .expect("powershell");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Whether process `pid` runs (a zombie does not count).
pub fn alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        std::fs::read_to_string(format!("/proc/{pid}/stat"))
            .is_ok_and(|s| s.rsplit(')').next().is_some_and(|r| !r.trim_start().starts_with('Z')))
    }
    #[cfg(windows)]
    {
        let out = Command::new("tasklist").args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"]).output();
        out.is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains(&format!("\"{pid}\"")))
    }
}

/// Threads of process `pid`.
pub fn threads(pid: u32) -> usize {
    #[cfg(unix)]
    {
        std::fs::read_dir(format!("/proc/{pid}/task")).map_or(0, |d| d.count())
    }
    #[cfg(windows)]
    {
        powershell(&format!("(Get-Process -Id {pid}).Threads.Count")).trim().parse().unwrap_or(0)
    }
}

/// Thread names and counts (Linux); Windows threads have no names here.
pub fn thread_names(pid: u32) -> String {
    #[cfg(unix)]
    {
        let mut m = std::collections::BTreeMap::new();
        for e in std::fs::read_dir(format!("/proc/{pid}/task")).into_iter().flatten().flatten() {
            let n = std::fs::read_to_string(e.path().join("comm")).unwrap_or_default().trim().to_string();
            *m.entry(n).or_insert(0usize) += 1;
        }
        format!("{m:?}")
    }
    #[cfg(windows)]
    {
        format!("{} threads", threads(pid))
    }
}

/// Direct children of `pid`.
pub fn children(pid: u32) -> Vec<u32> {
    #[cfg(unix)]
    {
        let mut v = vec![];
        for e in std::fs::read_dir("/proc").into_iter().flatten().flatten() {
            let Some(p) = e.file_name().to_str().and_then(|n| n.parse::<u32>().ok()) else { continue };
            let Ok(stat) = std::fs::read_to_string(e.path().join("stat")) else { continue };
            let ppid = stat.rsplit(')').next().and_then(|r| r.split_whitespace().nth(1)).and_then(|x| x.parse().ok());
            if ppid == Some(pid) {
                v.push(p);
            }
        }
        v
    }
    #[cfg(windows)]
    {
        powershell(&format!(
            "Get-CimInstance Win32_Process -Filter 'ParentProcessId={pid}' | ForEach-Object {{ $_.ProcessId }}"
        ))
        .lines()
        .filter_map(|l| l.trim().parse().ok())
        .collect()
    }
}
