//! Append-only event log (DESIGN.md §3.8): one tab-separated line per event,
//! `seq  unix_ms  kind  session  key=value...`. Only derived fields are ever
//! written — never environment variables, prompts, tool input or
//! credentials. A reader continues from a `seq` cursor ([`EventLog::read_from`]).

use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use haetty_proto::wire::LogEvent;

pub struct EventLog {
    path: PathBuf,
    file: Option<File>,
    seq: u64,
}

/// Tabs, newlines and backslashes escaped so a line stays one record.
fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\t', "\\t").replace('\n', "\\n").replace('\r', "\\r")
}

fn unesc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some(o) => out.push(o),
            None => out.push('\\'),
        }
    }
    out
}

/// One log line back into its fields (`None`: not an event line).
pub fn parse_line(line: &str) -> Option<LogEvent> {
    let mut it = line.split('\t');
    let seq = it.next()?.parse().ok()?;
    let ms = it.next()?.parse().ok()?;
    let kind = unesc(it.next()?);
    let session = unesc(it.next().unwrap_or(""));
    let fields = it
        .map(|kv| match kv.split_once('=') {
            Some((k, v)) => (unesc(k), unesc(v)),
            None => (unesc(kv), String::new()),
        })
        .collect();
    Some(LogEvent { seq, ms, kind, session, fields })
}

impl EventLog {
    pub fn open(path: &Path) -> io::Result<EventLog> {
        let seq = match File::open(path) {
            Ok(f) => BufReader::new(f)
                .lines()
                .map_while(Result::ok)
                .filter_map(|l| l.split('\t').next().and_then(|s| s.parse::<u64>().ok()))
                .last()
                .unwrap_or(0),
            Err(_) => 0,
        };
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(EventLog { path: path.to_owned(), file: Some(file), seq })
    }

    /// The last sequence number written (or found in the file).
    pub fn seq(&self) -> u64 {
        self.seq
    }

    /// Appends one event and returns it (for live readers).
    pub fn write(&mut self, kind: &str, session: &str, fields: &[(&str, &str)]) -> LogEvent {
        self.seq += 1;
        let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0)
            as u64;
        let mut line = format!("{}\t{ms}\t{}\t{}", self.seq, esc(kind), esc(session));
        for (k, v) in fields {
            line.push('\t');
            line.push_str(&esc(k));
            line.push('=');
            line.push_str(&esc(v));
        }
        line.push('\n');
        if let Some(f) = self.file.as_mut() {
            if let Err(e) = f.write_all(line.as_bytes()) {
                log::warn!("{}: {e}", self.path.display());
            }
        }
        LogEvent {
            seq: self.seq,
            ms,
            kind: kind.to_owned(),
            session: session.to_owned(),
            fields: fields.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        }
    }

    /// The events with a sequence number greater than `cursor`, and the
    /// highest sequence number in the file (at least `cursor`).
    pub fn events_after(path: &Path, cursor: u64) -> io::Result<(Vec<LogEvent>, u64)> {
        let mut last = cursor;
        let mut v = vec![];
        for e in Self::read_from(path, cursor)?.iter().filter_map(|l| parse_line(l)) {
            last = last.max(e.seq);
            v.push(e);
        }
        Ok((v, last))
    }

    /// Lines with a sequence number greater than `cursor`.
    pub fn read_from(path: &Path, cursor: u64) -> io::Result<Vec<String>> {
        let f = File::open(path)?;
        Ok(BufReader::new(f)
            .lines()
            .map_while(Result::ok)
            .filter(|l| l.split('\t').next().and_then(|s| s.parse::<u64>().ok()).is_some_and(|s| s > cursor))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_escape_and_resume_from_cursor() {
        let dir = haetty_testkit::run_dir("evlog");
        let p = dir.join("events.log");
        let mut l = EventLog::open(&p).unwrap();
        l.write("a", "s1", &[("title", "x\ty\nz")]);
        l.write("b", "s1", &[]);
        drop(l);
        let mut l = EventLog::open(&p).unwrap(); // continues the sequence
        l.write("c", "s2", &[]);
        let all = EventLog::read_from(&p, 0).unwrap();
        assert_eq!(all.len(), 3);
        assert!(all[0].ends_with("title=x\\ty\\nz"));
        assert!(all[2].starts_with("3\t"));
        assert_eq!(EventLog::read_from(&p, 2).unwrap().len(), 1);
        let (ev, last) = EventLog::events_after(&p, 0).unwrap();
        assert_eq!(last, 3);
        assert_eq!(ev[0].fields, vec![("title".to_string(), "x\ty\nz".to_string())]);
        assert_eq!((ev[2].seq, ev[2].kind.as_str(), ev[2].session.as_str()), (3, "c", "s2"));
        assert_eq!(EventLog::events_after(&p, 3).unwrap(), (vec![], 3));
        haetty_testkit::remove_dir_all(&dir);
    }
}
