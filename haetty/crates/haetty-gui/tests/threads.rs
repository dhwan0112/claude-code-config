//! Sessions leave no threads behind (stage S4 added a PTY writer thread per
//! session): 60 sessions opened and closed in a real `haetty daemon`
//! process — a third whose program floods terminal queries and never reads
//! its input (the writer may sit in a blocked `write`), a third still
//! printing, a third already ended — and the daemon's thread count returns
//! to where it was. Linux and Windows (T5): on Windows the count may differ
//! by the system's own pool threads, so up to 3 more are allowed there (a
//! leak would be 20 or more).

mod common;

use std::time::{Duration, Instant};

use common::{connect, daemon, endpoint, spec, thread_names, threads};

/// (program, args) for the three kinds of session.
fn programs() -> [(&'static str, Vec<&'static str>); 3] {
    #[cfg(unix)]
    {
        [
            (
                "sh",
                vec![
                    "-c",
                    "stty raw -echo; i=0; while [ $i -lt 4000 ]; do printf '\\033[6n\\033[c'; i=$((i+1)); done; exec sleep 60",
                ],
            ),
            ("sh", vec!["-c", "while :; do echo out; done"]),
            ("sh", vec!["-c", "true"]),
        ]
    }
    #[cfg(windows)]
    {
        [
            (
                "powershell.exe",
                vec![
                    "-NoProfile",
                    "-Command",
                    "$e=[char]27; for($i=0;$i -lt 4000;$i++){[Console]::Out.Write(\"$e[6n$e[c\")}; Start-Sleep 60",
                ],
            ),
            ("cmd.exe", vec!["/c", "for /L %i in (1,0,2) do @echo out"]),
            ("cmd.exe", vec!["/c", "exit 0"]),
        ]
    }
}

#[test]
fn closed_sessions_leave_no_threads() {
    let dir = haetty_testkit::run_dir("threads");
    let ep = endpoint(&dir, "threads");
    let mut d = daemon(&dir, &ep);
    let client = connect(&ep, "threads-test");
    let pid = d.id();
    let slack = if cfg!(windows) { 3 } else { 0 };
    // One session opened and closed first: lazily started threads count in
    // the baseline.
    let (wp, wa) = &programs()[2];
    let warm = client.create(spec(wp, wa)).unwrap();
    client.close(&warm).unwrap();
    std::thread::sleep(Duration::from_millis(800));
    let base = threads(pid);
    let progs = programs();
    for round in 0..20 {
        let ids: Vec<String> = progs.iter().map(|(p, a)| client.create(spec(p, a)).unwrap()).collect();
        std::thread::sleep(Duration::from_millis(if round % 5 == 0 { 300 } else { 50 }));
        for id in &ids {
            let _ = client.close(id);
        }
    }
    let end = Instant::now() + Duration::from_secs(30);
    loop {
        let now = threads(pid);
        let live = client.list_sessions().unwrap().len();
        if now <= base + slack && live == 0 {
            break;
        }
        assert!(
            Instant::now() < end,
            "daemon threads {base} before, {now} after 60 sessions ({live} still listed): {}",
            thread_names(pid)
        );
        std::thread::sleep(Duration::from_millis(200));
    }
    let _ = d.kill();
    let _ = d.wait();
    haetty_testkit::remove_dir_all(&dir);
}
