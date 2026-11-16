//! Release versions: `MAJOR.MINOR.PATCH` with an optional pre-release (`-rc.1`), compared as semver does (a
//! pre-release is lower than its release; build metadata `+…` is ignored).

use std::cmp::Ordering;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    /// Dot-separated identifiers after `-`; empty for a release.
    pub pre: Vec<String>,
}

impl Version {
    /// `0.3.2`, `v0.3.2`, `haetty-v0.3.2`, `1.0.0-rc.1`, `1.0.0+build`.
    pub fn parse(s: &str) -> Option<Version> {
        let s = s.trim();
        let s = s.strip_prefix("haetty-").unwrap_or(s);
        let s = s.strip_prefix('v').unwrap_or(s);
        let s = s.split('+').next()?;
        let (core, pre) = match s.split_once('-') {
            Some((c, p)) => (c, Some(p)),
            None => (s, None),
        };
        let mut it = core.split('.');
        let num = |x: Option<&str>| -> Option<u64> {
            let x = x?;
            if x.is_empty()
                || x.len() > 18
                || !x.bytes().all(|b| b.is_ascii_digit())
                || (x.len() > 1 && x.starts_with('0'))
            {
                return None;
            }
            x.parse().ok()
        };
        let v = Version { major: num(it.next())?, minor: num(it.next())?, patch: num(it.next())?, pre: vec![] };
        if it.next().is_some() {
            return None;
        }
        let pre = match pre {
            None => vec![],
            Some(p) => {
                let ids: Vec<String> = p.split('.').map(str::to_string).collect();
                if ids.iter().any(|i| i.is_empty() || !i.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')) {
                    return None;
                }
                ids
            }
        };
        Some(Version { pre, ..v })
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if !self.pre.is_empty() {
            write!(f, "-{}", self.pre.join("."))?;
        }
        Ok(())
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, o: &Version) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

impl Ord for Version {
    fn cmp(&self, o: &Version) -> Ordering {
        (self.major, self.minor, self.patch).cmp(&(o.major, o.minor, o.patch)).then_with(|| {
            match (self.pre.is_empty(), o.pre.is_empty()) {
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => {
                    for (a, b) in self.pre.iter().zip(&o.pre) {
                        let c = match (a.parse::<u64>(), b.parse::<u64>()) {
                            (Ok(x), Ok(y)) => x.cmp(&y),
                            (Ok(_), Err(_)) => Ordering::Less,
                            (Err(_), Ok(_)) => Ordering::Greater,
                            (Err(_), Err(_)) => a.cmp(b),
                        };
                        if c != Ordering::Equal {
                            return c;
                        }
                    }
                    self.pre.len().cmp(&o.pre.len())
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_orders_as_semver() {
        let v = |s| Version::parse(s).unwrap();
        assert_eq!(v("haetty-v0.3.2"), v("0.3.2"));
        assert_eq!(v("v1.2.3+abc").to_string(), "1.2.3");
        let order = [
            "0.3.1",
            "0.3.2-alpha",
            "0.3.2-alpha.1",
            "0.3.2-alpha.beta",
            "0.3.2-beta.2",
            "0.3.2-beta.11",
            "0.3.2-rc.1",
            "0.3.2",
            "0.3.10",
            "0.4.0",
            "1.0.0",
        ];
        for w in order.windows(2) {
            assert!(v(w[0]) < v(w[1]), "{} < {}", w[0], w[1]);
        }
        for bad in ["", "1", "1.2", "1.2.3.4", "01.2.3", "1.2.x", "1.2.3-", "1.2.3-a..b", "-1.2.3", "1.2.3-ä"] {
            assert!(Version::parse(bad).is_none(), "{bad}");
        }
    }
}
