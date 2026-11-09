//! The orchestrator's pieces that the daemon, the GUI, the CLI and the
//! boundary test share (DESIGN.md §8): the worker report grammar, worker
//! states, the readiness rule and the one function that sends text to a
//! worker. Report texts stay in [`Secret`]s: never logged.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use haetty_proto::wire::{ClaudeInfo, Origin, Secret, WorkerReport};
use haetty_proto::RemoteHandle;

use crate::json::Json;

/// Report values longer than this are cut.
pub const MAX_FIELD: usize = 2000;
/// Transcript bytes read from the end (fallback only).
pub const TRANSCRIPT_TAIL: u64 = 256 << 10;

const KEYS: [&str; 5] = ["STATUS", "ARTIFACT", "EVIDENCE", "BLOCKERS", "NEXT_ACTION"];

/// `KEY: rest` on a line, with markdown marks (`#`, `*`, `>`, `-`, spaces)
/// around the key (ccx's `hook.sh` grammar).
fn key_line(line: &str) -> Option<(usize, String)> {
    let t = line.trim_start_matches(|c: char| c.is_whitespace() || "#*>-".contains(c));
    for (k, key) in KEYS.iter().enumerate() {
        let Some(rest) = t.get(..key.len()).filter(|p| p.eq_ignore_ascii_case(key)).map(|_| &t[key.len()..]) else {
            continue;
        };
        let rest = rest.trim_start_matches(|c: char| c == '*' || c.is_whitespace());
        let Some(rest) = rest.strip_prefix(':') else { continue };
        let rest = rest.trim_start_matches(|c: char| c == '*' || c.is_whitespace());
        return Some((k, rest.trim_end().to_string()));
    }
    None
}

/// The worker report in an assistant message, if it has a valid STATUS
/// (`verified`, `blocked`, `needs_review`; the last STATUS line counts).
pub fn parse_report(text: &str, at_ms: u64, from_transcript: bool) -> Option<WorkerReport> {
    let mut fields: [Option<String>; 5] = Default::default();
    let mut current: Option<usize> = None;
    for line in text.lines() {
        if let Some((k, rest)) = key_line(line) {
            fields[k] = Some(rest);
            current = Some(k);
        } else if let Some(k) = current.filter(|&k| k > 0) {
            let f = fields[k].get_or_insert_with(String::new);
            if f.chars().count() < MAX_FIELD {
                if !f.is_empty() {
                    f.push('\n');
                }
                f.push_str(line.trim_end());
            }
        }
    }
    // Exactly one value: the template line `verified | blocked | …` is not
    // a report.
    let status = fields[0].as_deref()?.trim_matches(|c: char| c == '*' || c == '`' || c == '.' || c.is_whitespace());
    let status = status.to_ascii_lowercase();
    if !matches!(status.as_str(), "verified" | "blocked" | "needs_review") {
        return None;
    }
    let take = |i: usize, f: &mut [Option<String>; 5]| -> Secret<String> {
        let s = f[i].take().unwrap_or_default();
        Secret(s.trim().chars().take(MAX_FIELD).collect())
    };
    Some(WorkerReport {
        status,
        artifact: take(1, &mut fields),
        evidence: take(2, &mut fields),
        blockers: take(3, &mut fields),
        next_action: take(4, &mut fields),
        at_ms,
        from_transcript,
    })
}

/// A worker's state (DESIGN.md §8.2), never guessed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkerState {
    Running,
    Verified,
    Blocked,
    NeedsReview,
    NoReport,
}

impl WorkerState {
    pub fn of(alive: bool, busy: bool, report: Option<&WorkerReport>) -> WorkerState {
        if alive && busy {
            return WorkerState::Running;
        }
        match report.map(|r| r.status.as_str()) {
            Some("verified") => {
                let e = report.map_or("", |r| r.evidence.0.trim());
                if e.is_empty() || e.eq_ignore_ascii_case("none") || e == "-" {
                    WorkerState::NeedsReview
                } else {
                    WorkerState::Verified
                }
            }
            Some("blocked") => WorkerState::Blocked,
            Some("needs_review") => WorkerState::NeedsReview,
            _ => WorkerState::NoReport,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            WorkerState::Running => "running",
            WorkerState::Verified => "verified",
            WorkerState::Blocked => "blocked",
            WorkerState::NeedsReview => "needs review",
            WorkerState::NoReport => "no report",
        }
    }

    /// The tab mark (grey, in front of the title).
    pub fn glyph(self) -> &'static str {
        match self {
            WorkerState::Running => "…",
            WorkerState::Verified => "✓",
            WorkerState::Blocked => "✗",
            WorkerState::NeedsReview => "◇",
            WorkerState::NoReport => "–",
        }
    }
}

/// Why the orchestrator does not send now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hold {
    /// No Claude Code report at all (not started, or no integration).
    Unknown,
    /// No turn has finished yet: Claude Code may still show a start-up
    /// dialog (folder trust, bypass acceptance), which input could answer.
    NoTurnYet,
    /// A turn is running: a tool call may be about to ask.
    Busy,
    /// A permission dialog is open, or a person answered one and the turn has not ended since.
    Asking,
}

impl Hold {
    pub fn reason(self) -> &'static str {
        match self {
            Hold::Unknown => "the worker has not reported to haetty yet",
            Hold::NoTurnYet => "the worker has not finished a turn yet (a start-up dialog may be open)",
            Hold::Busy => "the worker is busy; send when it stops",
            Hold::Asking => "the worker is in a permission dialog or has answered one; send after its turn ends, or after the person's next prompt in its tab (Esc gives no event)",
        }
    }
}

/// The readiness rule (DESIGN.md §8.4).
pub fn ready(info: Option<&ClaudeInfo>) -> Result<(), Hold> {
    let i = info.ok_or(Hold::Unknown)?;
    if i.asking {
        return Err(Hold::Asking);
    }
    if i.busy {
        return Err(Hold::Busy);
    }
    if i.turns == 0 {
        return Err(Hold::NoTurnYet);
    }
    Ok(())
}

/// Sends `text` and Enter to a worker, as a program (never as the person):
/// one paste, one Enter — only when [`ready`]. The daemon refuses program
/// input while a permission question waits, whatever this function saw.
pub fn send(h: &RemoteHandle, info: Option<&ClaudeInfo>, text: &str) -> Result<(), Hold> {
    ready(info)?;
    let clean: String = text.chars().filter(|c| *c == '\n' || !c.is_control()).collect();
    h.paste(Origin::Program, clean);
    h.write(Origin::Program, b"\r".to_vec());
    Ok(())
}

/// The task prompt given to the worker as its positional argument: the briefing it reads first, and the report its
/// final message must end with. The report's STATUS is the only part of that message haetty keeps.
pub fn prompt(id: &str, title: &str, briefing: &str, branch: &str, base: &str) -> String {
    let mut p = format!("haetty task {id}: {title}\n\n");
    p.push_str(&format!(
        "Read the briefing first: {briefing}\n\
         It has the scope, the related files and when the task is done. If you cannot read it, report STATUS: blocked.\n\n"
    ));
    p.push_str(&format!(
        "You work in a git worktree on branch {branch} (from {base}); stay in it. \
         End your final message with exactly this report and nothing after it:\n\n\
         STATUS: verified | blocked | needs_review\n\
         ARTIFACT: <changed files or commits, or the findings>\n\
         EVIDENCE: <each check you actually ran: the command and what it showed>\n\
         BLOCKERS: <what stopped you, or none>\n\
         NEXT_ACTION: <what the person must decide or do next>\n\n\
         Never list a check you did not run. If you could not finish, use STATUS: blocked and say why. \
         Use needs_review when the work is done but correctness depends on a judgement you could not verify.\n"
    ));
    p
}

/// `2026-11-06T12:34:56.789Z` → Unix ms (UTC only; else `None`).
fn iso_ms(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    if b.len() < 20 || b[4] != b'-' || b[10] != b'T' || !s.ends_with('Z') {
        return None;
    }
    let n = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, mo, d, h, mi, sec) = (n(0..4)?, n(5..7)?, n(8..10)?, n(11..13)?, n(14..16)?, n(17..19)?);
    let ms = if b.get(19) == Some(&b'.') {
        let frac: String = s[20..s.len() - 1].chars().take(3).collect();
        format!("{frac:0<3}").parse::<i64>().ok()?
    } else {
        0
    };
    // Days from civil (Howard Hinnant).
    let y2 = if mo <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from(((days * 24 + h) * 60 + mi) * 60_000 + sec * 1000 + ms).ok()
}

/// Fallback when the Stop hook carried no final message: the last
/// main-chain assistant text in the transcript written at or after
/// `since_ms` (the turn's start). Only a `.jsonl` file in a `projects`
/// folder, only its last [`TRANSCRIPT_TAIL`] bytes.
pub fn transcript_text(path: &str, since_ms: u64) -> Option<String> {
    let p = Path::new(path);
    let in_projects = p.parent()?.parent()?.file_name()? == "projects";
    if !in_projects || p.extension()? != "jsonl" {
        return None;
    }
    let mut f = std::fs::File::open(p).ok()?;
    let len = f.metadata().ok()?.len();
    f.seek(SeekFrom::Start(len.saturating_sub(TRANSCRIPT_TAIL))).ok()?;
    let mut buf = vec![];
    f.take(TRANSCRIPT_TAIL).read_to_end(&mut buf).ok()?;
    let text = String::from_utf8_lossy(&buf);
    let mut best: Option<String> = None;
    for line in text.lines() {
        let Ok(j) = Json::parse(line) else { continue };
        if j.get("type").and_then(Json::str) != Some("assistant")
            || j.get("isSidechain").and_then(Json::bool) == Some(true)
        {
            continue;
        }
        let at = j.get("timestamp").and_then(Json::str).and_then(iso_ms);
        if at.is_none_or(|t| t < since_ms) {
            continue;
        }
        let parts: Vec<&str> = j
            .at(&["message", "content"])
            .and_then(Json::arr)
            .unwrap_or_default()
            .iter()
            .filter(|c| c.get("type").and_then(Json::str) == Some("text"))
            .filter_map(|c| c.get("text").and_then(Json::str))
            .collect();
        if !parts.is_empty() {
            best = Some(parts.join("\n"));
        }
    }
    best
}

/// Branch-name part from a title: lower-case ASCII letters and digits,
/// `-` between words, at most 30 characters (`task` when nothing is left).
pub fn slug(title: &str) -> String {
    let mut s = String::new();
    for c in title.chars() {
        if c.is_ascii_alphanumeric() {
            s.push(c.to_ascii_lowercase());
        } else if !s.ends_with('-') && !s.is_empty() {
            s.push('-');
        }
    }
    let s: String = s.trim_matches('-').chars().take(30).collect();
    let s = s.trim_end_matches('-').to_string();
    if s.is_empty() {
        "task".into()
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use haetty_proto::wire::Attention;

    #[test]
    fn report_grammar_like_ccx() {
        let t = "Done.\n\n**STATUS:** verified\nARTIFACT: src/a.rs\n  and src/b.rs\n- EVIDENCE: cargo test → 12 passed\nBLOCKERS: none\n## NEXT_ACTION: merge task/t1\n";
        let r = parse_report(t, 5, false).unwrap();
        assert_eq!(r.status, "verified");
        assert_eq!(r.artifact.0, "src/a.rs\n  and src/b.rs");
        assert_eq!(r.evidence.0, "cargo test → 12 passed");
        assert_eq!(r.blockers.0, "none");
        assert_eq!(r.next_action.0, "merge task/t1");
        assert_eq!(WorkerState::of(true, false, Some(&r)), WorkerState::Verified);
        // The last STATUS line counts; any case; markdown around the value.
        let r = parse_report("STATUS: verified\n...\n> status: `BLOCKED`\nEVIDENCE: x", 0, false).unwrap();
        assert_eq!(r.status, "blocked");
        // Not a report: unknown value, the template line itself, prose.
        assert!(parse_report("STATUS: done", 0, false).is_none());
        assert!(parse_report("STATUS: verified | blocked | needs_review", 0, false).is_none());
        assert!(parse_report("STATUS: verified.", 0, false).is_some());
        assert!(parse_report("the status is verified", 0, false).is_none());
        assert!(parse_report("", 0, false).is_none());
        // Field length cap.
        let long = format!("STATUS: needs_review\nARTIFACT: {}", "x".repeat(5000));
        assert_eq!(parse_report(&long, 0, false).unwrap().artifact.0.chars().count(), MAX_FIELD);
    }

    #[test]
    fn states_are_not_guessed() {
        let r = |s: &str, e: &str| WorkerReport { status: s.into(), evidence: Secret(e.into()), ..Default::default() };
        use WorkerState as W;
        assert_eq!(W::of(true, true, Some(&r("verified", "ran it"))), W::Running, "busy wins");
        assert_eq!(W::of(true, false, None), W::NoReport);
        assert_eq!(W::of(false, false, None), W::NoReport);
        assert_eq!(W::of(false, true, None), W::NoReport, "an exited worker is not running");
        assert_eq!(W::of(true, false, Some(&r("verified", "none"))), W::NeedsReview, "no evidence");
        assert_eq!(W::of(true, false, Some(&r("verified", ""))), W::NeedsReview);
        assert_eq!(W::of(true, false, Some(&r("blocked", ""))), W::Blocked);
        assert_eq!(W::of(true, false, Some(&r("needs_review", "x"))), W::NeedsReview);
    }

    #[test]
    fn readiness() {
        let mut i = ClaudeInfo::default();
        assert_eq!(ready(None), Err(Hold::Unknown));
        assert_eq!(ready(Some(&i)), Err(Hold::NoTurnYet));
        i.turns = 1;
        assert_eq!(ready(Some(&i)), Ok(()));
        i.busy = true;
        assert_eq!(ready(Some(&i)), Err(Hold::Busy));
        i.attention = Attention::Ask;
        assert_eq!(ready(Some(&i)), Err(Hold::Busy), "the mark alone does not hold");
        i.asking = true;
        assert_eq!(ready(Some(&i)), Err(Hold::Asking), "a held input outranks busy");
    }

    #[test]
    fn transcript_fallback_reads_only_this_turn() {
        let d = haetty_testkit::run_dir("orch-transcript").join("projects").join("-p");
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("s.jsonl");
        let line = |ts: &str, side: bool, text: &str| {
            format!(
                r#"{{"type":"assistant","isSidechain":{side},"timestamp":"{ts}","message":{{"role":"assistant","content":[{{"type":"text","text":"{text}"}}]}}}}"#
            )
        };
        let body = [
            line("2026-11-06T10:00:00.000Z", false, "old turn\\nSTATUS: verified"),
            r#"{"type":"user","timestamp":"2026-11-06T10:05:00.000Z","message":{"role":"user","content":"STATUS: blocked"}}"#.into(),
            line("2026-11-06T10:06:00.000Z", true, "subagent\\nSTATUS: blocked"),
            line("2026-11-06T10:07:00.500Z", false, "new\\nSTATUS: needs_review"),
        ]
        .join("\n");
        std::fs::write(&f, body).unwrap();
        let since = iso_ms("2026-11-06T10:05:00.000Z").unwrap();
        assert_eq!(transcript_text(f.to_str().unwrap(), since).as_deref(), Some("new\nSTATUS: needs_review"));
        assert_eq!(transcript_text(f.to_str().unwrap(), since + 3_600_000), None, "nothing from this turn yet");
        // Not a transcript path.
        let other = d.parent().unwrap().parent().unwrap().join("x.jsonl");
        std::fs::write(&other, line("2026-11-06T10:07:00.000Z", false, "STATUS: verified")).unwrap();
        assert_eq!(transcript_text(other.to_str().unwrap(), 0), None);
        assert_eq!(iso_ms("1970-01-01T00:00:01.5Z"), Some(1500));
        assert_eq!(iso_ms("2026-11-06T10:05:00+09:00"), None);
    }

    #[test]
    fn prompt_and_slug() {
        let p = prompt("t3", "Fix parser", "/state/tasks/t3.briefing.md", "task/t3-fix-parser", "main");
        assert!(p.starts_with("haetty task t3: Fix parser\n\nRead the briefing first: /state/tasks/t3.briefing.md\n"));
        assert!(p.contains("STATUS: verified | blocked | needs_review\nARTIFACT:"));
        assert_eq!(slug("Fix the CRLF parser!! (urgent)"), "fix-the-crlf-parser-urgent");
        assert_eq!(slug("한글만"), "task");
        assert_eq!(slug(&"ab ".repeat(40)).len(), 29);
    }
}
