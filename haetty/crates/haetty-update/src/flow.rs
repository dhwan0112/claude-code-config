//! The update's steps (DESIGN.md §3.17), in the order nothing unverified is trusted:
//!
//! 1. **check**: the latest release's `manifest.json` and `manifest.json.minisig` (from GitHub's "latest release", or
//!    `update-url`), the signature checked against the built-in keys, then the version: newer than this one, and this
//!    one at least `min_update_from`.
//! 2. **download** (only after the person pressed "Download and install"): the asset of this platform to a temporary
//!    file in the state folder, resumed where an earlier try stopped; its size and sha256 compared with the manifest;
//!    the named files taken out (`archive`). Anything wrong: the files are removed and the reason is one line.
//! 3. **install** (`layout`): a new version folder and the link switched, or (b) the executable replaced; the start
//!    guard written (`state::set_pending`).

use std::io::Write as _;
use std::path::{Path, PathBuf};

use crate::layout::{self, Form, Installed};
use crate::manifest::Manifest;
use crate::version::Version;

/// GitHub: the repository's latest release.
pub const GITHUB_API: &str = "https://api.github.com/repos/dhwan0112/claude-code-config/releases/latest";
/// The hosts a GitHub release may be fetched or redirected from.
pub const GITHUB_HOSTS: &[&str] =
    &["api.github.com", "github.com", "release-assets.githubusercontent.com", "objects.githubusercontent.com"];

/// How the bytes come (`net::Https`, or a test's).
pub trait Fetch {
    /// A small document (at most `max` bytes).
    fn get(&self, url: &str, max: usize) -> Result<Vec<u8>, String>;
    /// A large one appended to `out` from byte `from` (`Range`), `progress(done, total)` after each chunk.
    fn get_to(
        &self,
        url: &str,
        from: u64,
        out: &mut std::fs::File,
        progress: &mut dyn FnMut(u64, u64),
    ) -> Result<(), String>;
}

/// Where a release's files are.
#[derive(Clone, Debug, PartialEq)]
pub enum Origin {
    GitHub,
    /// `update-url`: a folder with `manifest.json`, its `.minisig` and the assets.
    Url(String),
}

impl Origin {
    pub fn of(update_url: &str) -> Origin {
        if update_url.trim().is_empty() {
            Origin::GitHub
        } else {
            Origin::Url(update_url.trim().trim_end_matches('/').to_string())
        }
    }

    /// The hosts allowed for this origin's requests and redirects.
    pub fn hosts(&self) -> Vec<String> {
        match self {
            Origin::GitHub => GITHUB_HOSTS.iter().map(|h| h.to_string()).collect(),
            Origin::Url(u) => host_of(u).into_iter().collect(),
        }
    }
}

/// `https://host[:port]/…` → `host[:port]`; nothing for another scheme.
pub fn host_of(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://")?;
    let host = rest.split(['/', '?', '#']).next()?;
    (!host.is_empty() && !host.contains('@')).then(|| host.to_ascii_lowercase())
}

/// A URL without its query and fragment (for logs: download links carry signed tokens).
pub fn loggable(url: &str) -> &str {
    url.split(['?', '#']).next().unwrap_or(url)
}

/// A release found newer than this build.
#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub manifest: Manifest,
    /// Where the assets are (`…/releases/download/TAG/` or `update-url/`).
    pub base: String,
    /// Why this build cannot install it (too old for an in-app update), when so.
    pub blocked: Option<String>,
}

/// What a check found.
#[derive(Clone, Debug, PartialEq)]
pub enum Found {
    UpToDate(Version),
    Newer(Box<Release>),
}

/// Step 1: the latest release, its signature checked (`keys`), compared with `current`.
pub fn check(f: &dyn Fetch, origin: &Origin, current: &Version, keys: &[String]) -> Result<Found, String> {
    let (mtext, sig, base) = match origin {
        Origin::Url(u) => {
            let m = f.get(&format!("{u}/manifest.json"), 64 * 1024)?;
            let s = f.get(&format!("{u}/manifest.json.minisig"), 4096)?;
            (m, s, format!("{u}/"))
        }
        Origin::GitHub => {
            let api = f.get(GITHUB_API, 1024 * 1024)?;
            let j = haetty_cc::json::Json::parse(&String::from_utf8_lossy(&api))
                .map_err(|e| format!("release list: {e}"))?;
            let tag = j.get("tag_name").and_then(|t| t.str()).ok_or("release list: no tag")?.to_string();
            let url_of = |name: &str| {
                j.get("assets")
                    .and_then(|a| a.arr())
                    .and_then(|a| a.iter().find(|x| x.get("name").and_then(|n| n.str()) == Some(name)))
                    .and_then(|x| x.get("browser_download_url").and_then(|u| u.str()))
                    .map(str::to_string)
            };
            let m = url_of("manifest.json").ok_or(format!("release {tag} has no manifest.json"))?;
            let s = url_of("manifest.json.minisig").ok_or(format!("release {tag} has no signature"))?;
            let base = m.strip_suffix("manifest.json").unwrap_or(&m).to_string();
            (f.get(&m, 64 * 1024)?, f.get(&s, 4096)?, base)
        }
    };
    let sig = String::from_utf8(sig).map_err(|_| "signature unreadable".to_string())?;
    crate::sign::verify(&mtext, &sig, keys)?;
    let text = String::from_utf8(mtext).map_err(|_| "manifest is not UTF-8".to_string())?;
    let manifest = Manifest::parse(&text)?;
    // Not newer: up to date — also against a signed older manifest served again (no going back).
    if manifest.version <= *current {
        return Ok(Found::UpToDate(manifest.version));
    }
    let blocked = (*current < manifest.min_update_from).then(|| {
        format!(
            "{} cannot be installed from {current} in the app (it needs {} or newer): manual install needed (README, \"Updates\")",
            manifest.version, manifest.min_update_from
        )
    });
    if manifest.asset().is_none() {
        return Err(format!("{} has no asset for {}", manifest.version, crate::manifest::this_target()));
    }
    Ok(Found::Newer(Box::new(Release { manifest, base, blocked })))
}

/// Step 2: the asset into `dir` (the state folder's `update/`), resumed, checked, unpacked. The partial file is kept
/// for a resume after a broken connection; anything that does not match is removed.
pub fn download(
    f: &dyn Fetch,
    rel: &Release,
    dir: &Path,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<Vec<(String, Vec<u8>)>, String> {
    if let Some(b) = &rel.blocked {
        return Err(b.clone());
    }
    let a = rel.manifest.asset().ok_or("no asset for this platform")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let part = dir.join(format!("{}.part", a.file));
    let have = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    let have = if have > a.size { 0 } else { have };
    let mut out = std::fs::OpenOptions::new()
        .create(true)
        .append(have > 0)
        .write(true)
        .truncate(have == 0)
        .open(&part)
        .map_err(|e| format!("{}: {e}", part.display()))?;
    if have < a.size {
        if let Err(e) = f.get_to(&format!("{}{}", rel.base, a.file), have, &mut out, progress) {
            // Kept for a resume only when something came and the server was not refused.
            drop(out);
            if e.contains("refused") || std::fs::metadata(&part).map_or(true, |m| m.len() == 0) {
                let _ = std::fs::remove_file(&part);
            }
            return Err(e);
        }
    }
    out.flush().map_err(|e| e.to_string())?;
    drop(out);
    let bytes = std::fs::read(&part).map_err(|e| format!("{}: {e}", part.display()))?;
    let wrong = |why: String| {
        let _ = std::fs::remove_file(&part);
        Err(why)
    };
    if bytes.len() as u64 != a.size {
        return wrong(format!("{}: {} bytes, the manifest says {}", a.file, bytes.len(), a.size));
    }
    if crate::hash::sha256_hex(&bytes) != a.sha256 {
        return wrong(format!("{}: sha256 does not match the manifest", a.file));
    }
    let files = crate::archive::extract(&a.file, &bytes, &a.files);
    let _ = std::fs::remove_file(&part);
    files
}

/// What an install did.
#[derive(Clone, Debug, PartialEq)]
pub struct Done {
    /// The new version folder (or `single`).
    pub new: String,
    /// The version before (or `single`): the rollback target.
    pub previous: String,
}

/// Step 3: installs `files` by `form`. The new executable must report its commit (`--version`) before the link moves.
pub fn install(
    form: &Form,
    rel: &Release,
    files: &[(String, Vec<u8>)],
    state: &Path,
    keep: usize,
) -> Result<Done, String> {
    if let Some(why) = form.refusal() {
        return Err(why);
    }
    let m = &rel.manifest;
    let exe_name = layout::exe_name();
    let exe = files.iter().find(|(n, _)| n == exe_name).ok_or(format!("the release has no {exe_name}"))?;
    match form {
        Form::Versioned { opt, link, current } => {
            let name = layout::version_name(&m.version.to_string(), &m.commit);
            if !opt.join(&name).exists() {
                let info = Installed {
                    version: &name,
                    reference: &format!("haetty-v{}", m.version),
                    commit: &m.commit,
                    source: "update",
                };
                layout::put_version(opt, &name, files, &info)?;
            }
            self_check(&opt.join(&name).join(exe_name), &m.commit)?;
            crate::state::set_pending(state, &name, current).map_err(|e| e.to_string())?;
            let prev = layout::activate(opt, link, &name)?;
            let previous = prev.unwrap_or_else(|| current.clone());
            layout::prune(opt, link, keep, Some(&previous));
            Ok(Done { new: name, previous })
        }
        Form::Single { exe: path } => {
            let probe = path.with_file_name(format!(".{exe_name}.check"));
            std::fs::write(&probe, &exe.1).map_err(|e| format!("{}: {e}", probe.display()))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&probe, std::fs::Permissions::from_mode(0o755));
            }
            let checked = self_check(&probe, &m.commit);
            let _ = std::fs::remove_file(&probe);
            checked?;
            crate::state::set_pending(state, "single", "single").map_err(|e| e.to_string())?;
            layout::replace_single(path, &exe.1)?;
            Ok(Done { new: "single".into(), previous: "single".into() })
        }
        _ => unreachable!("refused above"),
    }
}

/// `<exe> --version` (outside any instance) names `commit`.
pub fn self_check(exe: &Path, commit: &str) -> Result<(), String> {
    let out = std::process::Command::new(exe)
        .arg("--version")
        .env_remove("HAETTY_INSTANCE")
        .output()
        .map_err(|e| format!("the new version does not start: {e}"))?;
    let v = String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    if !v.contains(&commit[..commit.len().min(12)]) {
        return Err(format!("the new version reports {:?}, not commit {}", v.trim(), &commit[..commit.len().min(12)]));
    }
    Ok(())
}

/// The update folder in the instance's state folder.
pub fn dir(state_dir: &Path) -> PathBuf {
    state_dir.join("update")
}
