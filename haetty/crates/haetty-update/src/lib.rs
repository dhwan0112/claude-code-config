//! haetty's updates (stage PU, DESIGN.md §2.4, §3.17): what a release is (`manifest`), versions, the archive reader,
//! the installed layout shared with `tools/install-stable.sh`, and the updater's state files. The network and the
//! signature check are `net` and `sign`.

pub mod archive;
pub mod flow;
pub mod hash;
pub mod layout;
pub mod manifest;
pub mod net;
pub mod sign;
pub mod state;
#[cfg(feature = "test-support")]
pub mod testkey;
#[cfg(all(feature = "test-support", not(windows)))]
pub mod testserver;

/// Milliseconds since the epoch.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}
pub mod version;

/// `YYYY-MM-DDTHH:MM:SSZ` of seconds since the epoch (UTC; no date crate).
pub fn rfc3339_s(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

#[cfg(test)]
mod tests {
    #[test]
    fn rfc3339() {
        assert_eq!(super::rfc3339_s(0), "1970-01-01T00:00:00Z");
        assert_eq!(super::rfc3339_s(1_794_816_000), "2026-11-16T08:00:00Z");
        assert_eq!(super::rfc3339_s(951_782_400), "2000-02-29T00:00:00Z");
    }
}
