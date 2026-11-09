//! `haetty run` and `haetty log`: a command run quietly, its output kept in a run log, and the log read in parts.
//!
//! `haetty run [--summary-lines N] -- COMMAND [ARGS...]` runs COMMAND with an empty standard input. Its standard output
//! and standard error go to a new log under `<state dir>/runs/`, in the order they arrive. The terminal gets the
//! command's own exit code, the time, the log's path, the error and warning lines (the first ten), and the last N lines
//! (ten by default). `haetty log [FILE] [--grep TEXT] [--lines A:B] [--tail N]` reads a log in parts: the lines that
//! contain TEXT, the lines A to B (1-based, inclusive), or the last N of what was selected (the last 50 by default).
//! Without FILE it lists the saved logs, newest first.
//!
//! The logs are kept for 14 days and at most 50 of them; each run removes the older ones. A log holds what the command
//! printed and its command line. A password or token that a program prints, or takes in its arguments, is in the log:
//! keep that in mind before sharing one.

use haetty_cc::transcript::grouped;
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// The logs kept, and their age limit in days.
pub const KEEP_LOGS: usize = 50;
pub const KEEP_DAYS: u64 = 14;
/// The error-like lines shown, the default number of last lines of a run, and of `haetty log`.
const ERROR_LINES_SHOWN: usize = 10;
pub const DEFAULT_SUMMARY_LINES: usize = 10;
const DEFAULT_LOG_TAIL: usize = 50;
/// Lines are cut at this many characters in the terminal (a log keeps them whole).
const SHOW_CHARS: usize = 200;
/// The length of the time at the start of a log's name (milliseconds since the Unix epoch).
const STAMP_LEN: usize = 13;

/// Words that make a line look like an error or a warning (cargo, rustc, pytest and most tools), in any case.
const ERROR_WORDS: &[&str] = &["error", "warning", "failed", "failure", "panicked", "traceback", "fatal", "exception"];

pub const RUN_USAGE: &str = "usage: haetty run [--summary-lines N] -- COMMAND [ARGS...]";
pub const LOG_USAGE: &str = "usage: haetty log [FILE] [--grep TEXT] [--lines A:B] [--tail N]";

/// Whether a line looks like an error or a warning.
pub fn looks_like_error(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    ERROR_WORDS.iter().any(|w| l.contains(w))
}

fn cut(s: &str) -> String {
    if s.chars().count() <= SHOW_CHARS {
        s.to_string()
    } else {
        let head: String = s.chars().take(SHOW_CHARS).collect();
        format!("{head}…")
    }
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// The folder of the run logs: `runs` under the state directory.
pub fn runs_dir() -> PathBuf {
    haetty_proto::paths::state_dir().join("runs")
}

/// The time a log was started, from the first 13 characters of its name.
fn stamp(p: &Path) -> Option<u64> {
    let name = p.file_name()?.to_str()?;
    let digits = name.get(..STAMP_LEN)?;
    if digits.bytes().all(|b| b.is_ascii_digit()) {
        digits.parse().ok()
    } else {
        None
    }
}

/// Whether a file is a run log (named `<13-digit time>-<pid>-<command>.log`).
fn is_log(p: &Path) -> bool {
    p.extension().is_some_and(|e| e == "log") && stamp(p).is_some()
}

/// The run logs in `dir`, oldest first.
fn logs_in(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut logs: Vec<PathBuf> =
        std::fs::read_dir(dir)?.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| is_log(p)).collect();
    logs.sort();
    Ok(logs)
}

/// Removes the logs older than `KEEP_DAYS` and all but the `KEEP_LOGS` newest. Returns how many were removed.
pub fn prune(dir: &Path, now: u64) -> std::io::Result<usize> {
    let logs = logs_in(dir)?;
    let n = logs.len();
    let max_age = KEEP_DAYS * 24 * 3600 * 1000;
    let mut removed = 0;
    for (i, p) in logs.iter().enumerate() {
        let started = stamp(p).unwrap_or(0);
        // `n - i` logs are this one and the newer ones: more than KEEP_LOGS means it is not among the newest.
        let too_many = n - i > KEEP_LOGS;
        let too_old = now.saturating_sub(started) > max_age;
        if (too_many || too_old) && std::fs::remove_file(p).is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}

fn exit_code(s: &ExitStatus) -> i32 {
    if let Some(c) = s.code() {
        return c;
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(sig) = s.signal() {
            return 128 + sig;
        }
    }
    1
}

/// Reads a pipe line by line into the channel; the newline (and a carriage return before it) is not sent.
fn pump(pipe: impl Read, tx: mpsc::Sender<Vec<u8>>) {
    let mut r = BufReader::new(pipe);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match r.read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                if buf.last() == Some(&b'\n') {
                    buf.pop();
                    if buf.last() == Some(&b'\r') {
                        buf.pop();
                    }
                }
                if tx.send(buf.clone()).is_err() {
                    break;
                }
            }
        }
    }
}

/// The terminal's summary of a run.
fn summary_text(
    code: i32,
    secs: f64,
    log: &Path,
    lines: usize,
    errors: &[String],
    error_count: usize,
    tail: &VecDeque<String>,
) -> String {
    let mut o = format!("exit {code} · {secs:.1} s · {} lines · log: {}\n", grouped(lines as u64), log.display());
    if error_count > 0 {
        let shown = if error_count > errors.len() { format!(", first {}", errors.len()) } else { String::new() };
        o.push_str(&format!("errors and warnings ({}{shown}):\n", grouped(error_count as u64)));
        for e in errors {
            o.push_str(&format!("  {e}\n"));
        }
    }
    if !tail.is_empty() {
        o.push_str(&format!("last {} lines:\n", tail.len()));
        for t in tail {
            o.push_str(&format!("  {t}\n"));
        }
    }
    o.trim_end().to_string()
}

/// Runs `argv` quietly, its output kept in a new log in `dir`. Returns the terminal text and the command's exit code.
pub fn run_in(dir: &Path, summary_lines: usize, argv: &[String]) -> (String, i32) {
    if argv.is_empty() {
        return (RUN_USAGE.to_string(), 2);
    }
    if let Err(e) = std::fs::create_dir_all(dir) {
        return (format!("haetty run: {}: {e}", dir.display()), 2);
    }
    let started = now_ms();
    let _ = prune(dir, started);
    let slug: String = Path::new(&argv[0])
        .file_name()
        .map(|n| {
            n.to_string_lossy()
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
                .take(24)
                .collect()
        })
        .unwrap_or_default();
    let slug = if slug.is_empty() { "run".to_string() } else { slug };
    let log_path = dir.join(format!("{started:013}-{}-{slug}.log", std::process::id()));
    let file = match std::fs::File::create(&log_path) {
        Ok(f) => f,
        Err(e) => return (format!("haetty run: {}: {e}", log_path.display()), 2),
    };
    let mut log = BufWriter::new(file);
    let _ = writeln!(log, "# command: {}", argv.join(" "));
    let t0 = Instant::now();
    let mut child = match Command::new(&argv[0])
        .args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            let _ = writeln!(log, "# could not start: {e}");
            let _ = log.flush();
            return (format!("haetty run: cannot start {}: {e}", argv[0]), 127);
        }
    };
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    let mut readers = Vec::new();
    if let Some(out) = child.stdout.take() {
        let tx = tx.clone();
        readers.push(std::thread::spawn(move || pump(out, tx)));
    }
    if let Some(err) = child.stderr.take() {
        let tx = tx.clone();
        readers.push(std::thread::spawn(move || pump(err, tx)));
    }
    drop(tx);
    let mut lines = 0usize;
    let mut errors: Vec<String> = Vec::new();
    let mut error_count = 0usize;
    let mut tail: VecDeque<String> = VecDeque::with_capacity(summary_lines.max(1));
    for raw in rx {
        let line = String::from_utf8_lossy(&raw).into_owned();
        let _ = log.write_all(line.as_bytes());
        let _ = log.write_all(b"\n");
        lines += 1;
        if looks_like_error(&line) {
            error_count += 1;
            if errors.len() < ERROR_LINES_SHOWN {
                errors.push(cut(&line));
            }
        }
        if summary_lines > 0 {
            if tail.len() == summary_lines {
                tail.pop_front();
            }
            tail.push_back(cut(&line));
        }
    }
    for r in readers {
        let _ = r.join();
    }
    let code = match child.wait() {
        Ok(s) => exit_code(&s),
        Err(_) => 1,
    };
    let secs = t0.elapsed().as_secs_f64();
    let _ = writeln!(log, "# exit {code} after {secs:.1} s");
    let _ = log.flush();
    (summary_text(code, secs, &log_path, lines, &errors, error_count, &tail), code)
}

/// The lines of a log that the options select, with their line numbers (1-based): those in the range `lines`, those
/// containing `grep`, and the last `tail` of them.
pub fn select<'a>(
    text: &'a str,
    grep: Option<&str>,
    lines: Option<(usize, usize)>,
    tail: Option<usize>,
) -> Vec<(usize, &'a str)> {
    let mut sel: Vec<(usize, &str)> = text.lines().enumerate().map(|(i, l)| (i + 1, l)).collect();
    if let Some((a, b)) = lines {
        sel.retain(|(n, _)| *n >= a && *n <= b);
    }
    if let Some(g) = grep {
        sel.retain(|(_, l)| l.contains(g));
    }
    if let Some(t) = tail {
        let skip = sel.len().saturating_sub(t);
        sel.drain(..skip);
    }
    sel
}

/// `haetty log`: the saved logs in `dir` (no FILE), or the selected lines of one log. FILE is a path, or a name in `dir`.
pub fn log_in(
    dir: &Path,
    file: Option<&Path>,
    grep: Option<&str>,
    lines: Option<(usize, usize)>,
    tail: Option<usize>,
) -> (String, i32) {
    let Some(file) = file else {
        let mut logs = match logs_in(dir) {
            Ok(l) => l,
            Err(_) => return ("haetty log: no runs yet".to_string(), 0),
        };
        logs.reverse();
        if logs.is_empty() {
            return ("haetty log: no runs yet".to_string(), 0);
        }
        let mut o = String::new();
        for p in &logs {
            let size = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
            let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            o.push_str(&format!("{name}  {} B\n", grouped(size)));
        }
        return (o.trim_end().to_string(), 0);
    };
    let path = if file.exists() { file.to_path_buf() } else { dir.join(file) };
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => return (format!("haetty log: {}: {e}", path.display()), 1),
    };
    let text = String::from_utf8_lossy(&bytes);
    // Without a selection, the last lines.
    let tail = if grep.is_none() && lines.is_none() && tail.is_none() { Some(DEFAULT_LOG_TAIL) } else { tail };
    let sel = select(&text, grep, lines, tail);
    let mut o = String::new();
    for (n, l) in &sel {
        o.push_str(&format!("{n}: {}\n", cut(l)));
    }
    (o.trim_end().to_string(), 0)
}

/// `haetty run` in the runs folder of the state directory.
pub fn run(summary_lines: usize, argv: &[String]) -> (String, i32) {
    run_in(&runs_dir(), summary_lines, argv)
}

/// `haetty log` in the runs folder of the state directory.
pub fn log(
    file: Option<&Path>,
    grep: Option<&str>,
    lines: Option<(usize, usize)>,
    tail: Option<usize>,
) -> (String, i32) {
    log_in(&runs_dir(), file, grep, lines, tail)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_like_lines_in_any_case() {
        assert!(looks_like_error("error[E0425]: cannot find value"));
        assert!(looks_like_error("FAILED tests/test_x.py::test_y"));
        assert!(looks_like_error("warning: unused variable"));
        assert!(looks_like_error("thread 'main' panicked at src/lib.rs"));
        assert!(!looks_like_error("Compiling haetty-cc v0.3.0"));
        assert!(!looks_like_error("test result: ok. 3 passed"));
    }

    #[test]
    fn selections_combine_range_grep_and_tail() {
        let text = "a\nerror one\nb\nerror two\nc\n";
        let all = select(text, None, None, None);
        assert_eq!(all.len(), 5);
        assert_eq!(select(text, Some("error"), None, None), vec![(2, "error one"), (4, "error two")]);
        assert_eq!(select(text, None, Some((2, 3)), None), vec![(2, "error one"), (3, "b")]);
        assert_eq!(select(text, None, None, Some(2)), vec![(4, "error two"), (5, "c")]);
        assert_eq!(select(text, Some("error"), None, Some(1)), vec![(4, "error two")]);
    }

    #[test]
    fn a_run_keeps_its_output_and_gives_the_exit_code() {
        let dir = haetty_testkit::run_dir("run-quiet");
        let _ = std::fs::remove_dir_all(&dir);
        #[cfg(unix)]
        let argv: Vec<String> =
            ["sh", "-c", "echo hello; echo 'an error here' 1>&2; exit 3"].iter().map(|s| s.to_string()).collect();
        #[cfg(windows)]
        let argv: Vec<String> = ["cmd", "/C", "exit /b 3"].iter().map(|s| s.to_string()).collect();
        let (text, code) = run_in(&dir, 2, &argv);
        assert_eq!(code, 3, "{text}");
        assert!(text.starts_with("exit 3 · "), "{text}");
        #[cfg(unix)]
        {
            assert!(text.contains("errors and warnings (1):"), "{text}");
            assert!(text.contains("an error here"), "{text}");
            assert!(text.contains("last 2 lines:"), "{text}");
            let logs = logs_in(&dir).unwrap();
            assert_eq!(logs.len(), 1);
            let content = std::fs::read_to_string(&logs[0]).unwrap();
            assert!(content.contains("hello\n"), "{content}");
            assert!(content.contains("# exit 3 after "), "{content}");
        }
        haetty_testkit::remove_dir_all(&dir);
    }

    #[test]
    fn the_logs_are_kept_fifty_and_two_weeks() {
        let dir = haetty_testkit::run_dir("run-prune");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let now: u64 = 1_750_000_000_000;
        for i in 0..55u64 {
            std::fs::write(dir.join(format!("{:013}-1-x.log", now - i * 1000)), "").unwrap();
        }
        std::fs::write(dir.join(format!("{:013}-1-old.log", now - 20 * 24 * 3600 * 1000)), "").unwrap();
        let removed = prune(&dir, now).unwrap();
        assert_eq!(removed, 6);
        assert_eq!(logs_in(&dir).unwrap().len(), KEEP_LOGS);
        haetty_testkit::remove_dir_all(&dir);
    }
}
