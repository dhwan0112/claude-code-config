//! Append-only event log (DESIGN.md §3.8): one tab-separated line per event,
//! `seq  unix_ms  kind  session  key=value...`. Only derived fields are ever
//! written — never environment variables, prompts, tool input or
//! credentials. A reader continues from a `seq` cursor ([`EventLog::read_from`]).

use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

pub struct EventLog {
    path: PathBuf,
    file: Option<File>,
    seq: u64,
}

/// Tabs, newlines and backslashes escaped so a line stays one record.
fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\t', "\\t").replace('\n', "\\n").replace('\r', "\\r")
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

    pub fn write(&mut self, kind: &str, session: &str, fields: &[(&str, &str)]) {
        self.seq += 1;
        let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
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
        let dir = std::env::temp_dir().join(format!("ccterm-evlog-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
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
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
