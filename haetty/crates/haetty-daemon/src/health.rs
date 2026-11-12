//! The daemon's own resource counts in everyday use (P1c): one `health` line in the event log every hour — the
//! daemon's only timer besides the idle exit — the current and largest counts for `haetty status --json`, and one
//! `health_warning` line when threads or descriptors keep growing while the sessions and clients stay the same.
//! The counts are the soak's (`scripts/e2e/soak.sh`): threads, descriptors, anonymous memory.

use std::time::Duration;

use haetty_proto::wire::{DaemonHealth, HealthSample};

/// How often the daemon writes its `health` line.
pub const EVERY: Duration = Duration::from_secs(3600);

/// Hourly lines in a row with more threads or descriptors (and neither fewer) at the same sessions and clients before
/// the warning: three hours of growth. One warning per run of growth; it may come again after the run breaks.
pub const RISES_TO_WARN: u32 = 3;

/// The daemon's counts now, with the sessions and clients it holds.
pub fn sample(sessions: u32, clients: u32) -> HealthSample {
    let (threads, fds, anon_kb) = counts();
    HealthSample { threads, fds, anon_kb, sessions, clients }
}

#[cfg(target_os = "linux")]
fn counts() -> (Option<u32>, Option<u32>, Option<u64>) {
    let entries = |d: &str| std::fs::read_dir(d).ok().map(|r| r.count() as u32);
    let threads = entries("/proc/self/task");
    // The listing's own descriptor is open while it is read: not one of the daemon's.
    let fds = entries("/proc/self/fd").map(|n| n.saturating_sub(1));
    let anon_kb = std::fs::read_to_string("/proc/self/status").ok().and_then(|s| {
        s.lines()
            .find_map(|l| l.strip_prefix("RssAnon:"))
            .and_then(|v| v.trim().trim_end_matches("kB").trim().parse().ok())
    });
    (threads, fds, anon_kb)
}

#[cfg(windows)]
fn counts() -> (Option<u32>, Option<u32>, Option<u64>) {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
    };
    use windows_sys::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetCurrentProcessId, GetProcessHandleCount};
    // SAFETY: plain Win32 calls on the current process with owned, correctly sized out-structures; the snapshot
    // handle is closed before returning.
    unsafe {
        let me = GetCurrentProcess();
        let mut handles = 0u32;
        let fds = (GetProcessHandleCount(me, &mut handles) != 0).then_some(handles);
        let mut pmc: PROCESS_MEMORY_COUNTERS_EX = std::mem::zeroed();
        pmc.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
        let anon_kb =
            (K32GetProcessMemoryInfo(me, (&raw mut pmc).cast(), pmc.cb) != 0).then_some(pmc.PrivateUsage as u64 / 1024);
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
        let threads = if snap == INVALID_HANDLE_VALUE || snap.is_null() {
            None
        } else {
            let pid = GetCurrentProcessId();
            let mut te: THREADENTRY32 = std::mem::zeroed();
            te.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
            let mut n = 0u32;
            let mut ok = Thread32First(snap, &mut te) != 0;
            while ok {
                n += u32::from(te.th32OwnerProcessID == pid);
                ok = Thread32Next(snap, &mut te) != 0;
            }
            CloseHandle(snap);
            Some(n)
        };
        (threads, fds, anon_kb)
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
fn counts() -> (Option<u32>, Option<u32>, Option<u64>) {
    (None, None, None)
}

fn max_opt<T: Ord>(a: Option<T>, b: Option<T>) -> Option<T> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    }
}

/// The largest counts since the start, and the run of hourly growth.
pub struct Health {
    started_ms: u64,
    max: HealthSample,
    last_hourly: Option<HealthSample>,
    rises: u32,
    warned: bool,
}

impl Health {
    pub fn new(started_ms: u64) -> Self {
        Health { started_ms, max: HealthSample::default(), last_hourly: None, rises: 0, warned: false }
    }

    /// A count taken now (hourly, after a session start, for `GetHealth`): the largest values follow it.
    pub fn seen(&mut self, s: &HealthSample) {
        let m = &mut self.max;
        m.threads = max_opt(m.threads, s.threads);
        m.fds = max_opt(m.fds, s.fds);
        m.anon_kb = max_opt(m.anon_kb, s.anon_kb);
        m.sessions = m.sessions.max(s.sessions);
        m.clients = m.clients.max(s.clients);
    }

    /// The answer to `GetHealth`.
    pub fn report(&mut self, now: HealthSample) -> DaemonHealth {
        self.seen(&now);
        DaemonHealth { now, max: self.max, started_ms: self.started_ms }
    }

    /// The hourly count. Returns the number of hourly rises in a row when the warning is due (once per run).
    pub fn hourly(&mut self, s: HealthSample) -> Option<u32> {
        self.seen(&s);
        let rose = self.last_hourly.is_some_and(|p| {
            let up = |a: Option<u32>, b: Option<u32>| matches!((a, b), (Some(a), Some(b)) if b > a);
            let down = |a: Option<u32>, b: Option<u32>| matches!((a, b), (Some(a), Some(b)) if b < a);
            p.sessions == s.sessions
                && p.clients == s.clients
                && (up(p.threads, s.threads) || up(p.fds, s.fds))
                && !down(p.threads, s.threads)
                && !down(p.fds, s.fds)
        });
        self.last_hourly = Some(s);
        if !rose {
            self.rises = 0;
            self.warned = false;
            return None;
        }
        self.rises += 1;
        (self.rises >= RISES_TO_WARN && !self.warned).then(|| {
            self.warned = true;
            self.rises
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(threads: u32, fds: u32, sessions: u32, clients: u32) -> HealthSample {
        HealthSample { threads: Some(threads), fds: Some(fds), anon_kb: Some(1000), sessions, clients }
    }

    #[test]
    fn three_hourly_rises_at_the_same_sessions_warn_once() {
        let mut h = Health::new(5);
        assert_eq!(h.hourly(s(10, 20, 2, 1)), None);
        assert_eq!(h.hourly(s(11, 20, 2, 1)), None, "one rise");
        assert_eq!(h.hourly(s(11, 21, 2, 1)), None, "two");
        assert_eq!(h.hourly(s(12, 22, 2, 1)), Some(3), "three: the warning");
        assert_eq!(h.hourly(s(13, 22, 2, 1)), None, "the same run: no second warning");
        assert_eq!(h.hourly(s(13, 22, 2, 1)), None, "level: the run breaks");
        for t in 14..16 {
            assert_eq!(h.hourly(s(t, 22, 2, 1)), None);
        }
        assert_eq!(h.hourly(s(16, 22, 2, 1)), Some(3), "a new run warns again");
    }

    #[test]
    fn growth_with_more_sessions_or_clients_or_a_fall_is_not_counted() {
        let mut h = Health::new(5);
        h.hourly(s(10, 20, 2, 1));
        h.hourly(s(14, 28, 3, 1)); // a new session
        h.hourly(s(16, 30, 3, 2)); // a new client
        h.hourly(s(17, 25, 3, 2)); // threads up, fds down
        assert_eq!(h.hourly(s(18, 26, 3, 2)), None, "one rise only");
        assert_eq!(h.hourly(s(19, 27, 3, 2)), None, "two");
        assert_eq!(h.hourly(s(20, 28, 3, 2)), Some(3));
    }

    #[test]
    fn the_largest_values_are_kept_field_by_field() {
        let mut h = Health::new(5);
        h.seen(&s(30, 10, 4, 1));
        let r = h.report(s(12, 40, 1, 2));
        assert_eq!(r.now, s(12, 40, 1, 2));
        assert_eq!((r.max.threads, r.max.fds, r.max.sessions, r.max.clients), (Some(30), Some(40), 4, 2));
        assert_eq!(r.started_ms, 5);
        let none = HealthSample::default();
        assert_eq!(h.report(none).max.threads, Some(30), "a count the system did not give keeps the largest");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_counts_follow_the_process() {
        let a = sample(0, 0);
        let f = std::fs::File::open("/proc/self/status").unwrap();
        let t = std::thread::spawn(|| std::thread::sleep(Duration::from_millis(300)));
        std::thread::sleep(Duration::from_millis(50));
        let b = sample(0, 0);
        drop(f);
        t.join().unwrap();
        assert!(a.anon_kb.is_some_and(|k| k > 0), "{a:?}");
        // Other tests of this binary may start or end threads and files at the same time: at least the one more.
        assert!(b.fds.unwrap() >= 1 && b.threads.unwrap() >= 2, "{a:?} {b:?}");
        let fds_here = std::fs::read_dir("/proc/self/fd").unwrap().count() as u32 - 1;
        let now = sample(0, 0).fds.unwrap();
        assert!(now.abs_diff(fds_here) <= 2, "the listing's own descriptor is not counted: {now} vs {fds_here}");
    }
}
