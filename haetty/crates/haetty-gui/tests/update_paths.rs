//! Stage PU (U4): an update is downloaded, applied, restarted into or rolled back only by the person's keys and
//! clicks in the settings panel. This reads the source: the update actions run only from `update_effect`, which only
//! `settings_effect` calls, which only the panel's key, text and click handlers call; those are reached only from
//! winit's keyboard, IME and mouse events (`Origin::Human`); the daemon protocol has no update message; the command
//! line only checks.

fn src(f: &str) -> String {
    std::fs::read_to_string(format!("{}/src/{f}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

/// The functions whose body calls `callee(`.
fn callers(text: &str, callee: &str) -> Vec<String> {
    let mut out = vec![];
    let mut current = String::new();
    for line in text.lines() {
        let t = line.trim_start();
        if let Some(rest) =
            t.strip_prefix("fn ").or_else(|| t.strip_prefix("pub fn ")).or_else(|| t.strip_prefix("pub(super) fn "))
        {
            current = rest.split(['(', '<']).next().unwrap_or("").to_string();
        }
        if line.contains(&format!("{callee}("))
            && !t.starts_with("fn ")
            && !t.starts_with("pub")
            && !t.starts_with("//")
        {
            out.push(current.clone());
        }
    }
    out.sort();
    out.dedup();
    out
}

#[test]
fn update_actions_are_reached_only_from_the_persons_input() {
    let glue = src("app_settings.rs");
    let app = src("app.rs");
    let all = format!("{glue}\n{app}");
    for (f, want) in [
        ("self.updater.install", vec!["update_effect"]),
        ("haetty_update::layout::rollback_single", vec!["update_effect"]),
        ("self.update_effect", vec!["settings_effect"]),
        ("self.settings_effect", vec!["settings_click", "settings_key"]),
    ] {
        assert_eq!(callers(&all, f), want, "{f}");
    }
    // The panel's handlers are called from winit's input events only.
    for f in ["settings_key", "settings_click", "settings_text"] {
        let c = callers(&app, &format!("win.{f}"))
            .into_iter()
            .chain(callers(&app, &format!("self.{f}")))
            .collect::<Vec<_>>();
        assert!(c.iter().all(|c| matches!(c.as_str(), "window_event" | "keyboard" | "ime")), "{f} from {c:?}");
    }
    // The restart request is set only by update_effect.
    assert!(glue.matches("self.restart_req = Some").count() == 2 && app.matches("restart_req = Some").count() == 0);
    // No daemon message, no command line applies an update.
    let wire = std::fs::read_to_string(format!("{}/../haetty-proto/src/wire.rs", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let i = wire.find("pub enum ClientMsg").unwrap();
    let block = &wire[i..i + wire[i..].find("\n}").unwrap()];
    for bad in ["Update", "Install", "Rollback", "RollBack", "Restart"] {
        assert!(!block.contains(&format!("    {bad}")), "the protocol has no {bad} message");
    }
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_haetty")).args(["update", "--apply"]).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success() && text.contains("only from the settings panel"), "{text}");
}
