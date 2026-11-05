//! `ccterm install-desktop`: Linux desktop entry and icons for the current
//! user (`$XDG_DATA_HOME`, default `~/.local/share`): `applications/<app>.desktop`
//! with the running executable, `StartupWMClass` = app id (= Wayland
//! `app_id` = X11 `WM_CLASS`), hicolor PNGs (48, 128, 256) and an SVG.
//! Windows needs nothing: the executable carries its icon (build.rs).

use ccterm_proto::product;

#[cfg(not(windows))]
pub fn install_desktop() -> Result<String, String> {
    use std::path::PathBuf;
    let data = std::env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .ok_or("neither XDG_DATA_HOME nor HOME is set")?;
    let exe = product::exe_path();
    let id = product::APP_ID;
    let mut written = vec![];
    let mut write = |rel: String, bytes: &[u8]| -> Result<(), String> {
        let p = data.join(&rel);
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
        }
        std::fs::write(&p, bytes).map_err(|e| format!("{}: {e}", p.display()))?;
        written.push(p.display().to_string());
        Ok(())
    };
    let desktop = desktop_entry(&exe.to_string_lossy());
    write(format!("applications/{id}.desktop"), desktop.as_bytes())?;
    for s in [48u32, 128, 256] {
        write(format!("icons/hicolor/{s}x{s}/apps/{id}.png"), &crate::icon::png(s, &crate::icon::rgba(s)))?;
    }
    write(format!("icons/hicolor/scalable/apps/{id}.svg"), crate::icon::svg().as_bytes())?;
    Ok(format!("installed:\n  {}", written.join("\n  ")))
}

#[cfg(windows)]
pub fn install_desktop() -> Result<String, String> {
    Ok(format!(
        "{}: nothing to install on Windows — the executable carries its icon; pin it to Start or the taskbar",
        product::EXE
    ))
}

/// The `.desktop` file for `exe` (quoted for the Exec key).
#[cfg_attr(windows, allow(dead_code))]
pub fn desktop_entry(exe: &str) -> String {
    let quoted = exe.replace('\\', "\\\\").replace('"', "\\\"").replace('`', "\\`").replace('$', "\\$");
    format!(
        "[Desktop Entry]\nType=Application\nName={name}\nComment=Terminal for Claude Code\nExec=\"{quoted}\"\nIcon={id}\nTerminal=false\nCategories=System;TerminalEmulator;\nStartupWMClass={id}\n",
        name = product::DISPLAY_NAME,
        id = product::APP_ID
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_entry_quotes_the_path_and_matches_the_app_id() {
        let d = desktop_entry("/opt/my apps/cc$term");
        assert!(d.contains("Exec=\"/opt/my apps/cc\\$term\"\n"), "{d}");
        assert!(d.contains(&format!("StartupWMClass={}\n", product::APP_ID)));
        assert!(d.contains(&format!("Icon={}\n", product::APP_ID)));
    }

    #[cfg(not(windows))]
    #[test]
    fn installs_into_xdg_data_home() {
        let d = std::env::temp_dir().join(format!("ccterm-install-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        // SAFETY of the env change: this test is the only user of XDG_DATA_HOME.
        std::env::set_var("XDG_DATA_HOME", &d);
        let msg = install_desktop().unwrap();
        std::env::remove_var("XDG_DATA_HOME");
        assert!(msg.contains("ccterm.desktop"), "{msg}");
        assert!(d.join("icons/hicolor/256x256/apps/ccterm.png").exists());
        assert!(d.join("icons/hicolor/scalable/apps/ccterm.svg").exists());
        std::fs::remove_dir_all(&d).unwrap();
    }
}
