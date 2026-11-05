//! Build information for `haetty --version`: the commit
//! (`HAETTY_BUILD_COMMIT`, else `git rev-parse --short=12 HEAD`, else
//! "unknown") and the UTC build date (`SOURCE_DATE_EPOCH`, else now), as
//! `HAETTY_BUILD_COMMIT` / `HAETTY_BUILD_DATE` for `env!`. Re-run when the
//! checkout's HEAD or branch moves (not for uncommitted edits: a dev build
//! names the commit it is based on).
//!
//! Windows: links the application icon (`assets/icon/haetty.ico`, committed,
//! see `assets/icon/README.md`) into the executable as icon resource 1, so
//! Explorer, the taskbar, shortcuts and the window (`icon.rs`) show it.

use std::path::Path;

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new("git").current_dir(dir).args(args).output().ok()?;
    let s = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (out.status.success() && !s.is_empty()).then_some(s)
}

/// `YYYY-MM-DD` of a Unix time (UTC; days-to-civil after H. Hinnant).
fn date(secs: u64) -> String {
    let z = (secs / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

fn build_info(manifest: &Path) {
    println!("cargo:rerun-if-env-changed=HAETTY_BUILD_COMMIT");
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
    let commit = std::env::var("HAETTY_BUILD_COMMIT")
        .ok()
        .filter(|c| !c.is_empty())
        .or_else(|| git(manifest, &["rev-parse", "--short=12", "HEAD"]))
        .unwrap_or_else(|| "unknown".into());
    let commit: String = commit.chars().take(12).collect();
    println!("cargo:rustc-env=HAETTY_BUILD_COMMIT={commit}");
    let secs = std::env::var("SOURCE_DATE_EPOCH").ok().and_then(|s| s.parse().ok()).unwrap_or_else(|| {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
    });
    println!("cargo:rustc-env=HAETTY_BUILD_DATE={}", date(secs));
    // HEAD, the branch it points to, packed refs (worktrees have their own HEAD).
    let mut watch = vec!["HEAD".to_string(), "packed-refs".to_string()];
    if let Some(r) = git(manifest, &["symbolic-ref", "-q", "HEAD"]) {
        watch.push(r);
    }
    for w in watch {
        if let Some(p) = git(manifest, &["rev-parse", "--git-path", &w]) {
            let p = manifest.join(p);
            if p.exists() {
                println!("cargo:rerun-if-changed={}", p.display());
            }
        }
    }
}

fn main() {
    let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    build_info(&manifest);
    let ico = manifest.join("../../assets/icon/haetty.ico");
    println!("cargo:rerun-if-changed={}", ico.display());
    println!("cargo:rerun-if-changed=build.rs");
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if os != "windows" || env != "gnu" {
        return; // (an MSVC build would use rc.exe; not set up)
    }
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let ico = ico.canonicalize().expect("assets/icon/haetty.ico");
    let rc = out.join("haetty.rc");
    std::fs::write(&rc, format!("1 ICON \"{}\"\n", ico.display().to_string().replace('\\', "/"))).expect("write rc");
    let obj = out.join("haetty-icon.o");
    let windres = std::env::var("WINDRES").unwrap_or_else(|_| "x86_64-w64-mingw32-windres".into());
    let ok = std::process::Command::new(&windres)
        .arg("-i")
        .arg(&rc)
        .arg("-O")
        .arg("coff")
        .arg("-o")
        .arg(&obj)
        .status()
        .is_ok_and(|s| s.success());
    if ok {
        println!("cargo:rustc-link-arg-bins={}", obj.display());
    } else {
        println!("cargo:warning=windres ({windres}) not available: haetty.exe gets no icon resource");
    }
}
