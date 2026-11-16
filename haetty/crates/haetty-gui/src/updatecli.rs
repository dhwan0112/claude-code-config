//! Command lines of the updates (stage PU, DESIGN.md §3.17):
//!
//! - `haetty update --check [--json]`: asks the release server once and prints what it found. There is no command
//!   line that downloads, applies or rolls back an update: only the person's input in the settings panel does.
//! - `haetty install-layout put|activate|list|prune …`: the installed layout (`haetty_update::layout`), the engine of
//!   `tools/install-stable.sh` — the same code the in-app update uses. It never downloads anything.
//! - `haetty release-manifest …`: writes a release's `manifest.json` for `tools/release.sh`.

use std::path::{Path, PathBuf};

use haetty_update::layout::{self, Installed};
use haetty_update::manifest::{Asset, Manifest};
use haetty_update::version::Version;

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn need(args: &[String], name: &str) -> Result<String, String> {
    flag(args, name).ok_or_else(|| format!("{name} is needed"))
}

/// A test (`HAETTY_TEST_TMP` or `HAETTY_RUN` set) may only write inside its run directory.
fn guard(p: &Path) -> Result<(), String> {
    let base = haetty_proto::product::env_os("RUN").or_else(|| haetty_proto::product::env_os("TEST_TMP"));
    if let Some(b) = base {
        let b = PathBuf::from(b);
        let abs = if p.is_absolute() { p.to_path_buf() } else { std::env::current_dir().unwrap_or_default().join(p) };
        if !abs.starts_with(&b) {
            return Err(format!("refusing {}: outside the test run directory {}", abs.display(), b.display()));
        }
    }
    Ok(())
}

pub fn install_layout(args: &[String]) -> Result<String, String> {
    let opt = PathBuf::from(need(args, "--opt")?);
    guard(&opt)?;
    let link = layout::link_of(&opt).ok_or("no link for that folder")?;
    guard(&link)?;
    match args.first().map(String::as_str) {
        Some("put") => {
            let name = need(args, "--name")?;
            let files: Vec<String> = match args.iter().position(|a| a == "--") {
                Some(i) => args[i + 1..].to_vec(),
                None => return Err("files after --".into()),
            };
            let mut data = vec![];
            for f in &files {
                let p = Path::new(f);
                let n = p.file_name().ok_or("bad file")?.to_string_lossy().into_owned();
                data.push((n, std::fs::read(p).map_err(|e| format!("{f}: {e}"))?));
            }
            let info = Installed {
                version: &name,
                reference: &flag(args, "--ref").unwrap_or_default(),
                commit: &flag(args, "--commit").unwrap_or_default(),
                source: &flag(args, "--source").unwrap_or_default(),
            };
            let dest = layout::put_version(&opt, &name, &data, &info)?;
            Ok(format!("installed: {} (read-only)", dest.display()))
        }
        Some("activate") => {
            let name = args.get(1).filter(|a| !a.starts_with("--")).ok_or("activate NAME --opt DIR")?;
            let prev = layout::activate(&opt, &link, name)?;
            let mut s = format!("active: {name}  ({} -> {})", link.display(), opt.join(name).display());
            if let Some(p) = prev {
                s.push_str(&format!("\nprevious: {p}"));
            }
            Ok(s)
        }
        Some("list") => {
            let v = layout::list(&opt, &link);
            if v.is_empty() {
                return Ok(format!("nothing installed in {}", opt.display()));
            }
            Ok(v.into_iter()
                .map(|(n, a, at)| format!("{} {n}  installed={at}", if a { '*' } else { ' ' }))
                .collect::<Vec<_>>()
                .join("\n"))
        }
        Some("prune") => {
            let keep: usize = flag(args, "--keep").and_then(|k| k.parse().ok()).unwrap_or(2);
            let removed = layout::prune(&opt, &link, keep, None);
            Ok(format!("removed: {}", if removed.is_empty() { "nothing".into() } else { removed.join(" ") }))
        }
        _ => Err("usage: haetty install-layout put|activate|list|prune --opt DIR …".into()),
    }
}

pub fn release_manifest(args: &[String]) -> Result<String, String> {
    let version = Version::parse(&need(args, "--version")?).ok_or("bad --version")?;
    let notes = match flag(args, "--notes") {
        Some(p) => std::fs::read_to_string(&p).map_err(|e| format!("{p}: {e}"))?.trim().to_string(),
        None => String::new(),
    };
    let mut assets = vec![];
    for (i, a) in args.iter().enumerate() {
        if a != "--asset" {
            continue;
        }
        let spec = args.get(i + 1).ok_or("--asset TARGET=PATH:FILE,FILE")?;
        let (target, rest) = spec.split_once('=').ok_or("--asset TARGET=PATH:FILES")?;
        let (path, files) = rest.rsplit_once(':').ok_or("--asset TARGET=PATH:FILES")?;
        let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
        assets.push(Asset {
            target: target.into(),
            file: Path::new(path).file_name().unwrap_or_default().to_string_lossy().into_owned(),
            size: bytes.len() as u64,
            sha256: haetty_update::hash::sha256_hex(&bytes),
            files: files.split(',').map(str::to_string).collect(),
        });
    }
    let m = Manifest {
        version,
        commit: need(args, "--commit")?,
        built: need(args, "--built")?,
        protocol: need(args, "--protocol")?.parse().map_err(|_| "bad --protocol")?,
        min_update_from: Version::parse(&flag(args, "--min-update-from").unwrap_or("0.0.0".into()))
            .ok_or("bad --min-update-from")?,
        assets,
        notes,
        next_keys: vec![],
    };
    let text = m.to_json();
    Manifest::parse(&text).map_err(|e| format!("the manifest would not read back: {e}"))?;
    Ok(text)
}

/// `haetty update --check [--json]`: one request (or, with `update-check = false`, none), the answer printed.
pub fn check(json: bool) -> String {
    let (cfg, _) = crate::config::reread(&[]);
    let current = crate::updater::current();
    let origin = haetty_update::flow::Origin::of(&cfg.update_url);
    let https = haetty_update::net::Https::new(origin.hosts(), haetty_update::net::TIMEOUT);
    let r = haetty_update::flow::check(&https, &origin, &current, &haetty_update::sign::trusted());
    let q = |s: &str| haetty_cc::json::Json::Str(s.to_string()).to_string();
    match (r, json) {
        (Ok(haetty_update::flow::Found::UpToDate(v)), false) => format!("haetty {current}: up to date (latest {v})"),
        (Ok(haetty_update::flow::Found::UpToDate(v)), true) => {
            format!(r#"{{"current":{},"latest":{},"newer":false}}"#, q(&current.to_string()), q(&v.to_string()))
        }
        (Ok(haetty_update::flow::Found::Newer(rel)), false) => format!(
            "haetty {current}: {} is out ({}){}",
            rel.manifest.version,
            rel.manifest.built,
            rel.blocked.as_ref().map(|b| format!("; {b}")).unwrap_or_default()
        ),
        (Ok(haetty_update::flow::Found::Newer(rel)), true) => format!(
            r#"{{"current":{},"latest":{},"newer":true,"blocked":{}}}"#,
            q(&current.to_string()),
            q(&rel.manifest.version.to_string()),
            rel.blocked.as_deref().map(q).unwrap_or_else(|| "null".into())
        ),
        (Err(e), false) => format!("haetty {current}: the check failed: {}", haetty_update::flow::loggable(&e)),
        (Err(e), true) => {
            format!(r#"{{"current":{},"error":{}}}"#, q(&current.to_string()), q(haetty_update::flow::loggable(&e)))
        }
    }
}
