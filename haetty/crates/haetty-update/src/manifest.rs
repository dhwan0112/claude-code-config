//! `manifest.json`: what a release is (DESIGN.md §3.17). Written by `tools/release.sh` (through
//! `haetty release-manifest`, so one implementation writes and reads it) and signed with minisign; the updater trusts
//! nothing in it before the signature is checked.
//!
//! ```json
//! {"format":1,"name":"haetty","version":"0.4.0","commit":"<40 hex>","built":"2026-11-16T00:00:00Z",
//!  "protocol":12,"min_update_from":"0.3.2",
//!  "assets":[{"target":"x86_64-linux","file":"haetty-0.4.0-x86_64-linux.tar.gz","size":123,
//!             "sha256":"<64 hex>","files":["haetty","haetty-headless","LICENSE","THIRD-PARTY-NOTICES.md"]}],
//!  "notes":"the CHANGELOG section","next_keys":[]}
//! ```

use haetty_cc::json::Json;

use crate::version::Version;

/// The format this build reads; a manifest of another format is refused.
pub const FORMAT: u64 = 1;
/// Longest CHANGELOG section kept (characters).
pub const NOTES_MAX: usize = 8000;
/// Largest asset accepted (bytes).
pub const ASSET_MAX: u64 = 256 * 1024 * 1024;
/// Most files in an asset.
pub const FILES_MAX: usize = 16;

#[derive(Clone, Debug, PartialEq)]
pub struct Asset {
    /// `x86_64-linux` or `x86_64-windows`.
    pub target: String,
    pub file: String,
    pub size: u64,
    /// Lower-case hex.
    pub sha256: String,
    /// The plain file names the archive must hold; only these are taken out.
    pub files: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Manifest {
    pub version: Version,
    pub commit: String,
    pub built: String,
    pub protocol: u32,
    pub min_update_from: Version,
    pub assets: Vec<Asset>,
    pub notes: String,
    /// Key rotation (design only, DESIGN.md §3.17): public keys the next releases may be signed with.
    pub next_keys: Vec<String>,
}

/// This build's asset target.
pub fn this_target() -> &'static str {
    if cfg!(windows) {
        "x86_64-windows"
    } else {
        "x86_64-linux"
    }
}

fn hex_of(s: &str, len: usize) -> bool {
    s.len() == len && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// A plain file name: no folder, no `..`, nothing a file system could read as a path or a device.
pub fn plain_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s != "."
        && s != ".."
        && !s.starts_with('.')
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
}

impl Manifest {
    pub fn parse(text: &str) -> Result<Manifest, String> {
        if text.len() > 64 * 1024 {
            return Err("manifest too large".into());
        }
        let j = Json::parse(text).map_err(|e| format!("manifest: {e}"))?;
        let s = |k: &str| j.get(k).and_then(Json::str).ok_or_else(|| format!("manifest: no {k}"));
        let n = |k: &str| {
            j.get(k)
                .and_then(Json::num)
                .filter(|v| *v >= 0.0 && v.fract() == 0.0)
                .ok_or_else(|| format!("manifest: no {k}"))
        };
        if n("format")? as u64 != FORMAT {
            return Err(format!("manifest format {} (this build reads {FORMAT})", n("format")?));
        }
        if s("name")? != "haetty" {
            return Err("manifest: not haetty's".into());
        }
        let version = Version::parse(s("version")?).ok_or("manifest: bad version")?;
        let min_update_from = Version::parse(s("min_update_from")?).ok_or("manifest: bad min_update_from")?;
        let commit = s("commit")?.to_string();
        if !hex_of(&commit, 40) {
            return Err("manifest: bad commit".into());
        }
        let built = s("built")?.chars().take(40).collect();
        let protocol = n("protocol")?;
        if protocol > u32::MAX as f64 {
            return Err("manifest: bad protocol".into());
        }
        let mut notes: String = j.get("notes").and_then(Json::str).unwrap_or("").to_string();
        if notes.chars().count() > NOTES_MAX {
            notes = notes.chars().take(NOTES_MAX).collect();
        }
        let mut assets = vec![];
        for a in j.get("assets").and_then(Json::arr).ok_or("manifest: no assets")? {
            let s = |k: &str| a.get(k).and_then(Json::str).ok_or_else(|| format!("manifest asset: no {k}"));
            let size = a
                .get("size")
                .and_then(Json::num)
                .filter(|v| *v > 0.0 && v.fract() == 0.0)
                .ok_or("manifest asset: no size")?;
            if size > ASSET_MAX as f64 {
                return Err("manifest asset: too large".into());
            }
            let file = s("file")?.to_string();
            let sha256 = s("sha256")?.to_string();
            if !plain_name(&file) || !hex_of(&sha256, 64) {
                return Err("manifest asset: bad file name or sha256".into());
            }
            let files: Vec<String> = a
                .get("files")
                .and_then(Json::arr)
                .ok_or("manifest asset: no files")?
                .iter()
                .map(|f| f.str().map(str::to_string).ok_or("manifest asset: bad files"))
                .collect::<Result<_, _>>()?;
            if files.is_empty() || files.len() > FILES_MAX || !files.iter().all(|f| plain_name(f)) {
                return Err("manifest asset: bad files".into());
            }
            assets.push(Asset { target: s("target")?.to_string(), file, size: size as u64, sha256, files });
        }
        let next_keys = j
            .get("next_keys")
            .and_then(Json::arr)
            .map(|v| v.iter().filter_map(|k| k.str().map(str::to_string)).take(4).collect())
            .unwrap_or_default();
        Ok(Manifest { version, commit, built, protocol: protocol as u32, min_update_from, assets, notes, next_keys })
    }

    /// The asset of this build's target.
    pub fn asset(&self) -> Option<&Asset> {
        self.assets.iter().find(|a| a.target == this_target())
    }

    pub fn to_json(&self) -> String {
        let st = |s: &str| Json::Str(s.to_string());
        let assets = self
            .assets
            .iter()
            .map(|a| {
                Json::Obj(vec![
                    ("target".into(), st(&a.target)),
                    ("file".into(), st(&a.file)),
                    ("size".into(), Json::Num(a.size as f64)),
                    ("sha256".into(), st(&a.sha256)),
                    ("files".into(), Json::Arr(a.files.iter().map(|f| st(f)).collect())),
                ])
            })
            .collect();
        Json::Obj(vec![
            ("format".into(), Json::Num(FORMAT as f64)),
            ("name".into(), st("haetty")),
            ("version".into(), st(&self.version.to_string())),
            ("commit".into(), st(&self.commit)),
            ("built".into(), st(&self.built)),
            ("protocol".into(), Json::Num(self.protocol as f64)),
            ("min_update_from".into(), st(&self.min_update_from.to_string())),
            ("assets".into(), Json::Arr(assets)),
            ("notes".into(), st(&self.notes.chars().take(NOTES_MAX).collect::<String>())),
            ("next_keys".into(), Json::Arr(self.next_keys.iter().map(|k| st(k)).collect())),
        ])
        .to_string()
    }
}

#[cfg(test)]
pub(crate) fn sample() -> Manifest {
    Manifest {
        version: Version::parse("0.4.0").unwrap(),
        commit: "0123456789abcdef0123456789abcdef01234567".into(),
        built: "2026-11-16T00:00:00Z".into(),
        protocol: 12,
        min_update_from: Version::parse("0.3.0").unwrap(),
        assets: vec![Asset {
            target: this_target().into(),
            file: "haetty-0.4.0-x86_64-linux.tar.gz".into(),
            size: 10,
            sha256: "0".repeat(64),
            files: vec!["haetty".into(), "LICENSE".into()],
        }],
        notes: "### Added\n- things".into(),
        next_keys: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_refuses_what_is_wrong() {
        let m = sample();
        assert_eq!(Manifest::parse(&m.to_json()).unwrap(), m);
        let bad = |from: &str, to: &str| {
            let t = m.to_json().replacen(from, to, 1);
            assert_ne!(t, m.to_json(), "{from}");
            assert!(Manifest::parse(&t).is_err(), "{to}");
        };
        bad("\"format\":1", "\"format\":2");
        bad("\"name\":\"haetty\"", "\"name\":\"other\"");
        bad("\"0.4.0\"", "\"0.4\"");
        bad("0123456789abcdef0123456789abcdef01234567", "xyz");
        bad(&"0".repeat(64), "abc");
        bad("haetty-0.4.0-x86_64-linux.tar.gz", "../evil");
        bad("\"LICENSE\"", "\"/etc/passwd\"");
        bad("\"LICENSE\"", "\"..\"");
        bad("\"size\":10", "\"size\":-1");
        bad("\"size\":10", "\"size\":1e12");
        assert!(Manifest::parse("not json").is_err());
        assert!(Manifest::parse(&"[".repeat(100_000)).is_err());
        assert!(plain_name("haetty.exe") && !plain_name("a/b") && !plain_name("a\\b") && !plain_name(".hidden"));
    }

    /// Any bytes: no panic, and what parses round-trips.
    #[test]
    fn arbitrary_input_never_panics() {
        let base = sample().to_json().into_bytes();
        let mut seed: u64 = 0x1234_5678_9abc_def1;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for _ in 0..20_000 {
            let mut b = base.clone();
            for _ in 0..(next() % 6 + 1) {
                let i = (next() as usize) % b.len();
                match next() % 3 {
                    0 => b[i] = next() as u8,
                    1 => {
                        b.remove(i);
                    }
                    _ => b.insert(i, next() as u8),
                }
            }
            if let Ok(t) = std::str::from_utf8(&b) {
                if let Ok(m) = Manifest::parse(t) {
                    assert_eq!(Manifest::parse(&m.to_json()).unwrap(), m);
                }
            }
        }
    }
}
