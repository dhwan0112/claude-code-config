//! Tokens in the status panel (`token-status`, off by default): the totals of the active tab's Claude Code transcript and
//! of each worker's (`haetty_cc::transcript`). A background thread reads the transcripts as they grow, so the window never
//! waits for them; the panel shows the totals read so far.

use haetty_cc::transcript::{Summary, Tracker};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The transcripts to follow: a Claude Code session id and its transcript file.
pub type Watched = Vec<(String, PathBuf)>;

/// How often the thread reads what was appended.
const EVERY: Duration = Duration::from_secs(2);

/// The watcher: `follow` sets what the thread reads, `summary` gives the totals read so far.
pub struct Watch {
    /// The channel to the thread; the thread starts with the first `follow` that has something to follow.
    tx: Mutex<Option<mpsc::Sender<Watched>>>,
    /// The latest totals per session id.
    totals: Arc<Mutex<HashMap<String, Summary>>>,
    /// The list last followed: the same list is not sent again.
    last: Mutex<Option<Watched>>,
}

impl Default for Watch {
    fn default() -> Self {
        Self::new()
    }
}

impl Watch {
    pub fn new() -> Watch {
        Watch { tx: Mutex::new(None), totals: Arc::new(Mutex::new(HashMap::new())), last: Mutex::new(None) }
    }

    /// Makes the thread follow exactly these transcripts from now on. Does nothing when the list is unchanged.
    pub fn follow(&self, watched: Watched) {
        let mut last = self.last.lock().unwrap_or_else(|e| e.into_inner());
        if last.as_ref() == Some(&watched) {
            return;
        }
        let mut tx = self.tx.lock().unwrap_or_else(|e| e.into_inner());
        if tx.is_none() {
            let (s, rx) = mpsc::channel();
            let totals = Arc::clone(&self.totals);
            if std::thread::Builder::new().name("token-watch".into()).spawn(move || run(rx, totals)).is_err() {
                return;
            }
            *tx = Some(s);
        }
        if let Some(s) = tx.as_ref() {
            let _ = s.send(watched.clone());
        }
        *last = Some(watched);
    }

    /// The totals of a session's transcript as read so far (None before its first read).
    pub fn summary(&self, session: &str) -> Option<Summary> {
        self.totals.lock().ok()?.get(session).copied()
    }
}

/// Whether a context of `context` tokens has passed `percent` of a `window` (0 percent is off, never).
pub fn over_share(context: u64, window: u64, percent: u8) -> bool {
    percent > 0 && context.saturating_mul(100) >= window.saturating_mul(u64::from(percent))
}

/// The thread: reads the followed transcripts every `EVERY`, and whenever the list changes.
fn run(rx: mpsc::Receiver<Watched>, totals: Arc<Mutex<HashMap<String, Summary>>>) {
    let mut trackers: HashMap<String, Tracker> = HashMap::new();
    let mut watched: Watched = vec![];
    loop {
        match rx.recv_timeout(EVERY) {
            Ok(w) => watched = w,
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        // Sessions no longer followed are dropped with their totals.
        trackers.retain(|id, _| watched.iter().any(|(w, _)| w == id));
        if let Ok(mut m) = totals.lock() {
            m.retain(|id, _| watched.iter().any(|(w, _)| w == id));
        }
        for (id, path) in &watched {
            let t = trackers.entry(id.clone()).or_insert_with(|| Tracker::new(path.clone()));
            let _ = t.refresh();
            if let Ok(mut m) = totals.lock() {
                m.insert(id.clone(), t.summary());
            }
        }
    }
}

/// A token count for the panel: `987`, `12.35K`, `4.56M`, `2.08G`.
pub fn short(n: u64) -> String {
    let (div, unit) = if n >= 1_000_000_000 {
        (1_000_000_000.0, "G")
    } else if n >= 1_000_000 {
        (1_000_000.0, "M")
    } else if n >= 1_000 {
        (1_000.0, "K")
    } else {
        return n.to_string();
    };
    format!("{:.2}{unit}", n as f64 / div)
}

/// The panel's line for a session: `tokens 2.08G (new 20.10M) · hit 99.3 %`. "new" is the input, the cache writes and
/// the output (what was processed fresh); "hit" is the cache's share of the input side.
pub fn line(s: &Summary) -> String {
    let t = &s.totals;
    let fresh = t.input.saturating_add(t.cache_write).saturating_add(t.output);
    let hit = t.hit_ratio().map(|h| format!(" · hit {:.1} %", h * 100.0)).unwrap_or_default();
    format!("tokens {} (new {}){hit}", short(t.sum()), short(fresh))
}

#[cfg(test)]
mod tests {
    use super::*;
    use haetty_cc::transcript::Tokens;

    #[test]
    fn counts_are_short_and_the_line_says_total_new_and_hit() {
        assert_eq!(short(987), "987");
        assert_eq!(short(12_300), "12.30K");
        assert_eq!(short(4_560_000), "4.56M");
        assert_eq!(short(2_078_814_641), "2.08G");
        let s = Summary {
            responses: 2,
            context: 0,
            totals: Tokens { input: 10, output: 4, cache_write: 100, cache_read: 1120 },
        };
        assert_eq!(line(&s), "tokens 1.23K (new 114) · hit 91.1 %");
        assert_eq!(line(&Summary::default()), "tokens 0 (new 0)");
    }

    #[test]
    fn the_clear_mark_starts_at_the_share_and_is_off_at_zero() {
        assert!(over_share(120_000, 200_000, 60));
        assert!(!over_share(119_999, 200_000, 60));
        assert!(!over_share(u64::MAX, 200_000, 0), "0 is off");
    }
}
