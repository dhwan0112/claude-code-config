//! Side-channel scanner for OSC sequences that libghostty-vt parses but does
//! not surface through `Terminal` callbacks: desktop notifications (OSC 9,
//! OSC 777;notify, OSC 99) and ConEmu-style progress (OSC 9;4).
//!
//! The scanner only observes the byte stream; the same bytes still go to the
//! terminal. It is streaming: sequences split across reads are reassembled.

/// Progress reported with OSC 9;4 (ConEmu / Windows Terminal convention).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Progress {
    /// `9;4;0` — remove the indicator.
    Clear,
    /// `9;4;1;N` — normal, N percent.
    Normal(u8),
    /// `9;4;2;N` — error state.
    Error(u8),
    /// `9;4;3` — indeterminate.
    Indeterminate,
    /// `9;4;4;N` — paused/warning.
    Paused(u8),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OscEvent {
    Notification { title: Option<String>, body: String },
    Progress(Progress),
}

const MAX_OSC: usize = 64 * 1024;

#[derive(Default)]
enum State {
    #[default]
    Ground,
    Esc,
    Osc,
    OscEsc,
}

#[derive(Default)]
pub struct OscScanner {
    state: State,
    buf: Vec<u8>,
    overflow: bool,
}

impl OscScanner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed output bytes; returns events completed in this chunk.
    pub fn feed(&mut self, data: &[u8]) -> Vec<OscEvent> {
        let mut out = Vec::new();
        for &b in data {
            match self.state {
                State::Ground => {
                    if b == 0x1b {
                        self.state = State::Esc;
                    } else if b == 0x9d {
                        // C1 OSC is only valid outside UTF-8 text; libghostty
                        // treats input as UTF-8, so ignore it here as well.
                    }
                }
                State::Esc => {
                    if b == b']' {
                        self.state = State::Osc;
                        self.buf.clear();
                        self.overflow = false;
                    } else if b == 0x1b {
                        self.state = State::Esc;
                    } else {
                        self.state = State::Ground;
                    }
                }
                State::Osc => match b {
                    0x07 => {
                        self.finish(&mut out);
                        self.state = State::Ground;
                    }
                    0x1b => self.state = State::OscEsc,
                    0x18 | 0x1a => self.state = State::Ground, // CAN / SUB abort
                    _ => self.push(b),
                },
                State::OscEsc => {
                    if b == b'\\' {
                        self.finish(&mut out);
                        self.state = State::Ground;
                    } else if b == b']' {
                        // ESC ] inside an unterminated OSC: start a new one.
                        self.buf.clear();
                        self.overflow = false;
                        self.state = State::Osc;
                    } else {
                        self.state = State::Ground;
                    }
                }
            }
        }
        out
    }

    fn push(&mut self, b: u8) {
        if self.buf.len() < MAX_OSC {
            self.buf.push(b);
        } else {
            self.overflow = true;
        }
    }

    fn finish(&mut self, out: &mut Vec<OscEvent>) {
        if self.overflow {
            return;
        }
        let s = String::from_utf8_lossy(&self.buf);
        if let Some(ev) = parse(&s) {
            out.push(ev);
        }
    }
}

fn parse(s: &str) -> Option<OscEvent> {
    let (num, rest) = s.split_once(';').unwrap_or((s, ""));
    match num {
        "9" => {
            if let Some(p) = rest.strip_prefix("4;").or(if rest == "4" { Some("") } else { None }) {
                return parse_progress(p).map(OscEvent::Progress);
            }
            // Other ConEmu 9;N subcommands are numeric; a notification body
            // never is a bare small number followed by ';'.
            if let Some((first, _)) = rest.split_once(';') {
                if !first.is_empty() && first.chars().all(|c| c.is_ascii_digit()) {
                    return None;
                }
            }
            Some(OscEvent::Notification { title: None, body: rest.to_owned() })
        }
        "777" => {
            let mut parts = rest.splitn(3, ';');
            if parts.next()? != "notify" {
                return None;
            }
            let title = parts.next().unwrap_or("").to_owned();
            let body = parts.next().unwrap_or("").to_owned();
            Some(OscEvent::Notification { title: Some(title).filter(|t| !t.is_empty()), body })
        }
        "99" => {
            // Kitty: OSC 99 ; metadata ; payload. Only the simple, unchunked
            // form is handled: p=title or p=body (default body).
            let (meta, payload) = rest.split_once(';')?;
            let mut part = "title";
            let mut done = true;
            for kv in meta.split(':') {
                match kv.split_once('=') {
                    Some(("p", v)) => {
                        part = if v == "body" {
                            "body"
                        } else if v == "title" {
                            "title"
                        } else {
                            "other"
                        }
                    }
                    Some(("d", "0")) => done = false,
                    Some(("e", "1")) => return None, // base64 payloads not handled yet
                    _ => {}
                }
            }
            if !done || part == "other" {
                return None;
            }
            if part == "title" {
                Some(OscEvent::Notification { title: Some(payload.to_owned()), body: String::new() })
            } else {
                Some(OscEvent::Notification { title: None, body: payload.to_owned() })
            }
        }
        _ => None,
    }
}

fn parse_progress(p: &str) -> Option<Progress> {
    let mut it = p.split(';');
    let st: u8 = it.next().filter(|s| !s.is_empty()).map(|s| s.parse().ok()).unwrap_or(Some(0))?;
    let val: u8 = it.next().and_then(|v| v.parse::<u16>().ok()).map(|v| v.min(100) as u8).unwrap_or(0);
    Some(match st {
        0 => Progress::Clear,
        1 => Progress::Normal(val),
        2 => Progress::Error(val),
        3 => Progress::Indeterminate,
        4 => Progress::Paused(val),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn osc9_notification_bel_and_st() {
        let mut s = OscScanner::new();
        assert_eq!(
            s.feed(b"x\x1b]9;Claude needs you\x07y"),
            vec![OscEvent::Notification { title: None, body: "Claude needs you".into() }]
        );
        assert_eq!(s.feed(b"\x1b]9;done\x1b\\"), vec![OscEvent::Notification { title: None, body: "done".into() }]);
    }

    #[test]
    fn split_across_chunks() {
        let mut s = OscScanner::new();
        assert!(s.feed(b"\x1b]77").is_empty());
        assert!(s.feed(b"7;notify;Title;Bo").is_empty());
        assert_eq!(
            s.feed(b"dy\x07"),
            vec![OscEvent::Notification { title: Some("Title".into()), body: "Body".into() }]
        );
    }

    #[test]
    fn progress() {
        let mut s = OscScanner::new();
        assert_eq!(s.feed(b"\x1b]9;4;1;42\x07"), vec![OscEvent::Progress(Progress::Normal(42))]);
        assert_eq!(s.feed(b"\x1b]9;4;3\x07"), vec![OscEvent::Progress(Progress::Indeterminate)]);
        assert_eq!(s.feed(b"\x1b]9;4;0\x07"), vec![OscEvent::Progress(Progress::Clear)]);
    }

    #[test]
    fn ignores_other_osc() {
        let mut s = OscScanner::new();
        assert!(s.feed(b"\x1b]0;title\x07\x1b]8;;http://x\x1b\\link\x1b]8;;\x1b\\").is_empty());
        assert!(s.feed(b"\x1b]9;9;C:\\\\path\x07").is_empty());
    }

    #[test]
    fn kitty_99() {
        let mut s = OscScanner::new();
        assert_eq!(
            s.feed(b"\x1b]99;;Hello\x1b\\"),
            vec![OscEvent::Notification { title: Some("Hello".into()), body: String::new() }]
        );
        assert_eq!(
            s.feed(b"\x1b]99;p=body;World\x1b\\"),
            vec![OscEvent::Notification { title: None, body: "World".into() }]
        );
    }
}
