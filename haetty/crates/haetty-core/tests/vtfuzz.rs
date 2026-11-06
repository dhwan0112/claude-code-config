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

/// xorshift64*: the same cases on every run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    fn pick<'a, T>(&mut self, v: &'a [T]) -> &'a T {
        &v[self.below(v.len())]
    }
}

/// What Claude Code's renderer emits (seen in recorded output): saved
/// cursor and scroll region, bracketed paste, focus events, title, the
/// alternate screen, SGR mouse, synchronized output, truecolor, cursor
/// moves, box drawing, Hangul, emoji, links, kitty keyboard, queries.
fn claude_like() -> Vec<u8> {
    let mut s = String::from(
        "\x1b7\x1b[r\x1b8\x1b[?25h\x1b[?2004h\x1b[?2031h\x1b[?1004h\x1b]0;\u{2733} Claude Code\x07\x1b[?1049h\x1b[2J\x1b[H\
         \x1b[?1000h\x1b[?1002h\x1b[?1003h\x1b[?1006h\x1b[?25l\x1b[>1u\x1b[?u\x1b[?2026$p\x1b[>q\x1b[c\x1b]11;?\x07",
    );
    for i in 0..40 {
        s.push_str(&format!(
            "\x1b[?2026h\x1b[H\r\x1b[1B\x1b[38;2;215;119;87m \u{2590}\u{259b}\u{2588}\u{2588}\u{2588}\u{259c}\u{258c}\x1b[12G\x1b[39m\x1b[1mClaude Code\x1b[22m\
             \x1b[38;2;153;153;153m v2.1.{i}\r\x1b[2B{}\r\x1b[1B\x1b[39m\u{276f}\u{a0}\x1b[2m안녕 hello 🎉 \x1b]8;;https://example.invalid/{i}\x1b\\link\x1b]8;;\x1b\\\x1b[22m\
             \x1b[{}C\x1b[{}B\u{25cf} medium \u{b7} /effort\x1b]9;4;1;{}\x07\x1b[?2026l",
            "\u{2500}".repeat(118),
            i % 100,
            i % 30,
            i * 2 % 100
        ));
    }
    s.push_str("\x1b[?1049l\x1b[?2004l\x1b[?1004l\x1b[<u");
    s.into_bytes()
}

/// A random escape-sequence-heavy stream from the VT grammar.
fn grammar(r: &mut Rng, len: usize) -> Vec<u8> {
    let mut b = vec![];
    while b.len() < len {
        match r.below(12) {
            0 => {
                // CSI with random parameters, intermediates and final.
                b.extend_from_slice(b"\x1b[");
                b.extend_from_slice(r.pick(&[&b""[..], b"?", b">", b"<", b"=", b"!"]));
                for k in 0..r.below(12) {
                    if k > 0 {
                        b.push(*r.pick(b";:"));
                    }
                    let n = *r.pick(&[0u64, 1, 2, 7, 38, 48, 255, 2026, 65535, 99_999_999_999]);
                    b.extend_from_slice((n + r.below(3) as u64).to_string().as_bytes());
                }
                if r.below(4) == 0 {
                    b.push(0x20 + r.below(16) as u8);
                }
                b.push(0x40 + r.below(63) as u8);
            }
            1 => {
                // OSC with a payload, terminated by BEL, ST or nothing.
                b.extend_from_slice(b"\x1b]");
                b.extend_from_slice(r.pick(&[
                    &b"0;"[..],
                    b"2;",
                    b"4;1;",
                    b"7;file://h/",
                    b"8;;",
                    b"9;",
                    b"9;4;",
                    b"10;",
                    b"11;?",
                    b"52;c;",
                    b"133;A",
                    b"777;notify;",
                ]));
                for _ in 0..r.below(400) {
                    b.push(0x20 + r.below(95) as u8);
                }
                b.extend_from_slice(r.pick(&[&b"\x07"[..], b"\x1b\\", b""]));
            }
            2 => {
                // DCS / APC / PM / SOS strings (kitty graphics, sixel, tmux, XTGETTCAP).
                b.extend_from_slice(r.pick(&[
                    &b"\x1bP"[..],
                    b"\x1b_G",
                    b"\x1b^",
                    b"\x1bX",
                    b"\x1bP+q",
                    b"\x1bPq",
                    b"\x1bPtmux;",
                ]));
                for _ in 0..r.below(2000) {
                    b.push(r.below(256) as u8);
                }
                b.extend_from_slice(r.pick(&[&b"\x1b\\"[..], b"\x9c", b""]));
            }
            3 => {
                // Text: ASCII, Hangul, emoji, combining marks, wide.
                let t = r.pick(&[
                    "hello ",
                    "한글 ",
                    "🎉",
                    "e\u{301}",
                    "\u{fe0f}",
                    "漢字",
                    "\t",
                    "\r\n",
                    "\u{200d}",
                    "\u{1f468}\u{200d}\u{1f469}",
                ]);
                for _ in 0..r.below(20) {
                    b.extend_from_slice(t.as_bytes());
                }
            }
            4 => {
                // Invalid or truncated UTF-8.
                b.extend_from_slice(r.pick(&[
                    &b"\xff"[..],
                    b"\xc3",
                    b"\xe2\x82",
                    b"\xf0\x9f\x98",
                    b"\xed\xa0\x80",
                    b"\xc0\xaf",
                ]));
            }
            5 => b.extend_from_slice(r.pick(&[
                &b"\x1bc"[..],
                b"\x1b7",
                b"\x1b8",
                b"\x1bD",
                b"\x1bM",
                b"\x1b#8",
                b"\x1b(0",
                b"\x1b(B",
                b"\x0e",
                b"\x0f",
                b"\x18",
                b"\x1a",
                b"\x7f",
                b"\x08",
                b"\x07",
            ])),
            6 => {
                // Modes on and off.
                let m = *r.pick(&[
                    1u32, 3, 5, 6, 7, 25, 47, 66, 69, 1000, 1002, 1003, 1004, 1005, 1006, 1015, 1047, 1048, 1049, 2004,
                    2026, 2027, 2031, 9001,
                ]);
                b.extend_from_slice(format!("\x1b[?{m}{}", r.pick(&["h", "l", "$p", "s", "r"])).as_bytes());
            }
            7 => {
                // Big moves, scroll regions, insert/delete, repeat.
                let n = *r.pick(&[0u32, 1, 500, 65535]);
                b.extend_from_slice(
                    format!(
                        "\x1b[{n};{n}{}",
                        r.pick(&["H", "r", "L", "M", "@", "P", "X", "S", "T", "b", "J", "K", "s", "t"])
                    )
                    .as_bytes(),
                );
            }
            8 => {
                // Queries the engine answers.
                b.extend_from_slice(r.pick(&[
                    &b"\x1b[6n"[..],
                    b"\x1b[c",
                    b"\x1b[>c",
                    b"\x1b[5n",
                    b"\x1b[18t",
                    b"\x1b[14t",
                    b"\x1b]10;?\x07",
                    b"\x1b[?996n",
                    b"\x1b[>q",
                ]));
            }
            _ => {
                for _ in 0..r.below(200) {
                    b.push(r.below(256) as u8);
                }
            }
        }
    }
    b
}

/// Mutations of a seed: flips, inserts, deletions, duplications, splices,
/// huge numbers, truncation.
fn mutate(r: &mut Rng, seed: &[u8], other: &[u8]) -> Vec<u8> {
    let mut b = seed.to_vec();
    for _ in 0..1 + r.below(40) {
        if b.is_empty() {
            b.push(0x1b);
        }
        let at = r.below(b.len());
        match r.below(8) {
            0 => b[at] ^= 1 << r.below(8),
            1 => b.insert(at, r.below(256) as u8),
            2 => {
                let end = (at + r.below(64)).min(b.len());
                b.drain(at..end);
            }
            3 => {
                let end = (at + r.below(256)).min(b.len());
                let chunk = b[at..end].to_vec();
                for _ in 0..r.below(8) {
                    b.splice(at..at, chunk.iter().copied());
                }
            }
            4 if !other.is_empty() => {
                let o = r.below(other.len());
                let end = (o + r.below(512)).min(other.len());
                b.splice(at..at, other[o..end].iter().copied());
            }
            5 => b.splice(at..at, b"99999999999".iter().copied()).for_each(drop),
            6 => b
                .splice(at..at, r.pick(&[&b"\x1b["[..], b"\x1b]", b"\x1bP", b"\x9b", b"\x1b"]).iter().copied())
                .for_each(drop),
            _ => b.truncate(at),
        }
    }
    b
}

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
