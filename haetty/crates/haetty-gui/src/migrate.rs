//! First start under the new name (haetty, formerly ccterm): when the old
//! config file or saved tab sets exist and haetty has none, they are copied
//! (the originals stay). A daemon of the old name that is still running is
//! only reported, never touched: its sessions keep running until its last
//! ccterm window closes (DESIGN.md §3.9).

use std::path::Path;

use haetty_proto::paths;

/// What happened, for the log and the window's first hint.
#[derive(Debug, Default)]
pub struct Report {
    pub copied: Vec<String>,
    pub old_daemon: bool,
}

impl Report {
    pub fn hint(&self) -> Option<String> {
        let mut parts = vec![];
        if !self.copied.is_empty() {
            parts.push("settings and tab sets copied from ccterm".to_string());
        }
        if self.old_daemon {
            parts.push("a ccterm daemon (old name) is still running; its sessions are not shown here".to_string());
        }
        (!parts.is_empty()).then(|| parts.join("; "))
    }
}

fn has_tsv(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|rd| rd.flatten().any(|e| e.path().extension().is_some_and(|x| x == "tsv")))
}

/// Copies what is missing and checks for an old daemon. Does nothing when
/// the config or state directory is overridden (`HAETTY_CONFIG_DIR`,
/// `HAETTY_STATE_DIR`: tests, scripts).
pub fn run() -> Report {
    let mut r = Report::default();
    if let (Some(old_cfg), Some(old_state)) = (paths::legacy_config_dir(), paths::legacy_state_dir()) {
        let (new_cfg, new_state) = (paths::config_dir(), paths::state_dir());
        let (from, to) = (old_cfg.join("config"), new_cfg.join("config"));
        if from.is_file() && !to.exists() {
            match std::fs::create_dir_all(&new_cfg).and_then(|_| std::fs::copy(&from, &to)) {
                Ok(_) => r.copied.push(to.display().to_string()),
                Err(e) => log::warn!("copying {} to {}: {e}", from.display(), to.display()),
            }
        }
        let (from, to) = (old_state.join("tabs"), new_state.join("tabs"));
        if has_tsv(&from) && !has_tsv(&to) {
            let _ = std::fs::create_dir_all(&to);
            for e in std::fs::read_dir(&from).into_iter().flatten().flatten() {
                let p = e.path();
                if p.extension().is_some_and(|x| x == "tsv") {
                    let dest = to.join(e.file_name());
                    match std::fs::copy(&p, &dest) {
                        Ok(_) => r.copied.push(dest.display().to_string()),
                        Err(err) => log::warn!("copying {}: {err}", p.display()),
                    }
                }
            }
        }
    }
    for c in &r.copied {
        log::warn!("copied from ccterm (first start as haetty; the original stays): {c}");
    }
    if let Some(ep) = paths::legacy_endpoint() {
        if matches!(
            haetty_proto::transport::dial(&ep, std::time::Duration::from_millis(50)),
            haetty_proto::transport::Dial::Connected(_)
        ) {
            r.old_daemon = true;
            log::warn!(
                "a daemon of the old name (ccterm) is running at {}; left alone — its sessions are not shown here",
                ep.0.display()
            );
        }
    }
    r
}
