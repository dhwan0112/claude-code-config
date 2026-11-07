//! VT fuzzing (stage S4): deterministic random and mutated byte streams go
//! through a real PTY into the real engine; afterwards the session must
//! still work (a reset and an end marker show, a resize answers) — no
//! panic, no hang. Seeds: generated escape-sequence grammar, random bytes,
//! a Claude-Code-like stream, and recorded Claude Code output when
//! `HAETTY_VT_CORPUS` names a folder of `*.raw` files (kept outside the
//! repository; `recclaude.sh` in the session notes records them with an
//! isolated home and a fake key). Unix only (sh, cat).
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use haetty_core::*;

#[path = "common/vtgen.rs"]
mod vtgen;
use vtgen::{claude_like, grammar, mutate, Rng};

fn spawn(script: &str, cols: u16, rows: u16) -> Session {
    let spec = CommandSpec::new("sh").args(["-c", script]);
    let mut cfg = SessionConfig::new(spec);
    cfg.size = GridSize::new(cols, rows);
    cfg.scrollback_limit = 2_000_000;
    Session::spawn(cfg).expect("spawn")
}

fn wait_text(s: &Session, text: &str, timeout: Duration, what: &str) {
    let end = Instant::now() + timeout;
    loop {
        if s.handle.snapshot().is_some_and(|snap| snap.contains(text)) {
            return;
        }
        assert!(Instant::now() < end, "{what}: no {text:?} after {timeout:?} (hang or engine gone)");
        let _ = s.events.recv_timeout(Duration::from_millis(20));
    }
}

/// Feeds `data` through `cat` (stdin drained by a second reader unless
/// `no_reader`), then a full reset and an end marker; the engine must get
/// there and still resize.
fn feed(dir: &Path, name: &str, data: &[u8], no_reader: bool) {
    let f = dir.join(format!("{name}.bin"));
    std::fs::write(&f, data).unwrap();
    // CAN and ST end any sequence or string, RIS resets everything.
    let drain = if no_reader { "" } else { "cat > /dev/null & " };
    let script = format!(
        "stty raw -echo; {drain}cat '{}'; printf '\\030\\033\\\\\\033c__END_{name}__'; exec sleep 30",
        f.display()
    );
    let s = spawn(&script, 80, 24);
    wait_text(&s, &format!("__END_{name}__"), Duration::from_secs(90), name);
    s.handle.resize(GridSize::new(100, 30));
    let end = Instant::now() + Duration::from_secs(5);
    while s.handle.snapshot().is_none_or(|x| x.cols != 100) {
        assert!(Instant::now() < end, "{name}: no resize after the stream");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn random_and_mutated_streams_do_not_break_the_engine() {
    let dir = haetty_testkit::run_dir("vtfuzz");
    let mut r = Rng(0x9e37_79b9_7f4a_7c15);
    let claude = claude_like();
    let mut seeds: Vec<(String, Vec<u8>)> = vec![("claude-like".into(), claude.clone())];
    if let Some(d) = std::env::var_os("HAETTY_VT_CORPUS").map(PathBuf::from) {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            if e.path().extension().is_some_and(|x| x == "raw") {
                seeds.push((e.file_name().to_string_lossy().into_owned(), std::fs::read(e.path()).unwrap()));
            }
        }
    }
    // `cargo test` builds libghostty in Zig Debug mode (safety checks on,
    // ~200x slower): a few small streams there. HAETTY_VT_FUZZ=full (with
    // `--release`) runs 48 sessions, ~2.4 MB.
    let full = std::env::var("HAETTY_VT_FUZZ").is_ok_and(|v| v == "full");
    let (cases_g, cases_m, per, scale) = if full { (24, 8, 16, 8 << 10) } else { (3, 1, 3, 1 << 10) };
    let t0 = Instant::now();
    let mut bytes = 0;
    let mut cases = 0;
    // Grammar streams and plain random bytes, one session each.
    for i in 0..cases_g {
        let data = if i % 3 == 2 {
            (0..r.below(8 * scale)).map(|_| r.below(256) as u8).collect()
        } else {
            let len = scale << r.below(4);
            grammar(&mut r, len)
        };
        bytes += data.len();
        cases += 1;
        feed(&dir, &format!("g{i}"), &data, false);
    }
    // Mutated seeds: several mutants concatenated per session.
    for (k, (name, seed)) in seeds.iter().enumerate() {
        for j in 0..cases_m {
            let mut data = vec![];
            for _ in 0..per {
                data.extend(mutate(&mut r, seed, &claude));
            }
            bytes += data.len();
            cases += 1;
            feed(&dir, &format!("m{k}-{j}"), &data, false);
        }
        eprintln!("vtfuzz: seed {name} ({} bytes) mutated", seed.len());
    }
    eprintln!("vtfuzz: {cases} sessions, {bytes} bytes, {:?}", t0.elapsed());
    haetty_testkit::remove_dir_all(&dir);
}

/// A child that floods queries and never reads its input (`cat` of a file
/// with `ESC [6n` in it): the replies must not stall output processing.
#[test]
fn query_flood_with_a_child_that_never_reads() {
    let dir = haetty_testkit::run_dir("vtfuzz-flood");
    let data = b"\x1b[6n\x1b[c\x1b]11;?\x07".repeat(20_000);
    feed(&dir, "flood", &data, true);
    haetty_testkit::remove_dir_all(&dir);
}
