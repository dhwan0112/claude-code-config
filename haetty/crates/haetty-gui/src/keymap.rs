//! Application shortcuts (DESIGN.md §3.11): actions, key chords, the default
//! bindings and `keybind = CHORD=ACTION` lines from the config file.
//! Defaults avoid everything Claude Code binds (its interactive-mode
//! reference): plain Ctrl+letters, Alt+M/P/T/O/Y/B/F/D/V, Shift+Tab, Tab, Esc.

use std::fmt;

/// Something the window can do; every action is in the command palette.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    NewTab,
    CloseTab,
    NextTab,
    PreviousTab,
    /// 1-based tab number.
    GotoTab(u8),
    CommandPalette,
    Paste,
    /// Save the open tabs as the tab set "saved".
    SaveTabs,
    /// Open the tabs of the set "saved".
    OpenSavedTabs,
    /// Open the tabs of the previous run (set "previous").
    OpenPreviousTabs,
    /// Claude Code tabs: the last edit's diff over the screen.
    DiffView,
    /// Claude Code tabs: the status panel on the right.
    StatusPanel,
    StatusPanelWider,
    StatusPanelNarrower,
    /// Attach the daemon's sessions that no window shows (a crashed GUI).
    AttachDetached,
    /// Read the config file again and apply its theme and colours to every
    /// tab (also done when the window gets focus and the file changed).
    ReloadConfig,
    /// Give the active tab a name of one's own (empty: the automatic name).
    RenameTab,
    /// Pick one of the last 20 closed tabs to open again.
    ReopenClosedTab,
    /// Take the active tab out of the window; its session keeps running
    /// (`AttachDetached` brings it back).
    DetachTab,
    /// Orchestrator (DESIGN.md §8): a task → a worker tab in a worktree.
    NewTask,
    /// Send a message to the active worker (as a program, when it is ready).
    MessageWorker,
    /// `git worktree remove` for a task whose worker is closed.
    RemoveTaskWorktree,
    /// Allow or stop observe-only readers (`haetty screen`, `haetty events`)
    /// for the active tab (DESIGN.md §6.1).
    ToggleObserve,
}

impl Action {
    /// Config-file name (`keybind = ctrl+shift+t=new_tab`).
    pub fn name(self) -> String {
        match self {
            Action::NewTab => "new_tab".into(),
            Action::CloseTab => "close_tab".into(),
            Action::NextTab => "next_tab".into(),
            Action::PreviousTab => "previous_tab".into(),
            Action::GotoTab(n) => format!("goto_tab:{n}"),
            Action::CommandPalette => "command_palette".into(),
            Action::Paste => "paste".into(),
            Action::SaveTabs => "save_tabs".into(),
            Action::OpenSavedTabs => "open_saved_tabs".into(),
            Action::OpenPreviousTabs => "open_previous_tabs".into(),
            Action::DiffView => "diff_view".into(),
            Action::StatusPanel => "status_panel".into(),
            Action::StatusPanelWider => "status_panel_wider".into(),
            Action::StatusPanelNarrower => "status_panel_narrower".into(),
            Action::AttachDetached => "attach_detached".into(),
            Action::ReloadConfig => "reload_config".into(),
            Action::RenameTab => "rename_tab".into(),
            Action::ReopenClosedTab => "reopen_closed_tab".into(),
            Action::DetachTab => "detach_tab".into(),
            Action::NewTask => "new_task".into(),
            Action::MessageWorker => "message_worker".into(),
            Action::RemoveTaskWorktree => "remove_task_worktree".into(),
            Action::ToggleObserve => "toggle_observe".into(),
        }
    }

    /// Palette label.
    pub fn label(self) -> String {
        match self {
            Action::NewTab => "New tab".into(),
            Action::CloseTab => "Close tab".into(),
            Action::NextTab => "Next tab".into(),
            Action::PreviousTab => "Previous tab".into(),
            Action::GotoTab(n) => format!("Go to tab {n}"),
            Action::CommandPalette => "Command palette".into(),
            Action::Paste => "Paste".into(),
            Action::SaveTabs => "Save tabs (set \"saved\")".into(),
            Action::OpenSavedTabs => "Open saved tabs".into(),
            Action::OpenPreviousTabs => "Open the previous run's tabs".into(),
            Action::DiffView => "Diff of the last edit (Claude Code)".into(),
            Action::StatusPanel => "Status panel (Claude Code)".into(),
            Action::StatusPanelWider => "Status panel wider".into(),
            Action::StatusPanelNarrower => "Status panel narrower".into(),
            Action::AttachDetached => "Attach detached sessions".into(),
            Action::ReloadConfig => "Reload config (theme and colours)".into(),
            Action::RenameTab => "Rename tab".into(),
            Action::ReopenClosedTab => "Reopen a closed tab".into(),
            Action::DetachTab => "Detach tab (keeps running)".into(),
            Action::NewTask => "New task (a worker in a git worktree)".into(),
            Action::MessageWorker => "Message the worker".into(),
            Action::RemoveTaskWorktree => "Remove a task's worktree".into(),
            Action::ToggleObserve => "Observing: allow or stop for this tab (haetty screen, haetty events)".into(),
        }
    }

    pub fn parse(s: &str) -> Option<Action> {
        Some(match s {
            "new_tab" => Action::NewTab,
            "close_tab" => Action::CloseTab,
            "next_tab" => Action::NextTab,
            "previous_tab" => Action::PreviousTab,
            "command_palette" => Action::CommandPalette,
            "paste" => Action::Paste,
            "save_tabs" => Action::SaveTabs,
            "open_saved_tabs" => Action::OpenSavedTabs,
            "open_previous_tabs" => Action::OpenPreviousTabs,
            "diff_view" => Action::DiffView,
            "status_panel" => Action::StatusPanel,
            "status_panel_wider" => Action::StatusPanelWider,
            "status_panel_narrower" => Action::StatusPanelNarrower,
            "attach_detached" => Action::AttachDetached,
            "reload_config" => Action::ReloadConfig,
            "rename_tab" => Action::RenameTab,
            "reopen_closed_tab" => Action::ReopenClosedTab,
            "detach_tab" => Action::DetachTab,
            "new_task" => Action::NewTask,
            "message_worker" => Action::MessageWorker,
            "remove_task_worktree" => Action::RemoveTaskWorktree,
            "toggle_observe" => Action::ToggleObserve,
            _ => {
                let n: u8 = s.strip_prefix("goto_tab:")?.parse().ok()?;
                if !(1..=9).contains(&n) {
                    return None;
                }
                Action::GotoTab(n)
            }
        })
    }

    /// Every action, in palette order.
    pub fn all() -> Vec<Action> {
        let mut v = vec![
            Action::NewTab,
            Action::CloseTab,
            Action::NextTab,
            Action::PreviousTab,
            Action::RenameTab,
            Action::ReopenClosedTab,
            Action::DetachTab,
            Action::ToggleObserve,
        ];
        v.extend((1..=9).map(Action::GotoTab));
        v.extend([
            Action::DiffView,
            Action::StatusPanel,
            Action::StatusPanelWider,
            Action::StatusPanelNarrower,
            Action::AttachDetached,
            Action::NewTask,
            Action::MessageWorker,
            Action::RemoveTaskWorktree,
            Action::ReloadConfig,
            Action::SaveTabs,
            Action::OpenSavedTabs,
            Action::OpenPreviousTabs,
            Action::Paste,
            Action::CommandPalette,
        ]);
        v
    }
}

/// A key with modifiers. `key` is the key without modifiers, lower case:
/// a character (`t`, `1`, `-`) or a name (`tab`, `insert`, `enter`, `f5`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Chord {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub sup: bool,
    pub key: String,
}

impl Chord {
    pub fn parse(s: &str) -> Option<Chord> {
        let mut c = Chord { ctrl: false, shift: false, alt: false, sup: false, key: String::new() };
        let parts: Vec<&str> = s.split('+').map(str::trim).collect();
        let (key, mods) = parts.split_last()?;
        for m in mods {
            match m.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => c.ctrl = true,
                "shift" => c.shift = true,
                "alt" | "opt" | "option" => c.alt = true,
                "super" | "cmd" | "win" => c.sup = true,
                _ => return None,
            }
        }
        if key.is_empty() {
            return None;
        }
        c.key = key.to_lowercase();
        Some(c)
    }
}

impl fmt::Display for Chord {
    /// Display form for the palette: `Ctrl+Shift+T`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts = vec![];
        if self.ctrl {
            parts.push("Ctrl".to_string());
        }
        if self.alt {
            parts.push("Alt".to_string());
        }
        if self.sup {
            parts.push("Super".to_string());
        }
        if self.shift {
            parts.push("Shift".to_string());
        }
        let mut k = self.key.clone();
        if let Some(first) = k.get(..1) {
            k = first.to_uppercase() + &k[1..];
        }
        parts.push(k);
        f.write_str(&parts.join("+"))
    }
}

#[derive(Clone, Debug)]
pub struct Keymap {
    binds: Vec<(Chord, Action)>,
}

impl Default for Keymap {
    fn default() -> Self {
        let mut binds = vec![];
        let mut b = |s: &str, a| binds.push((Chord::parse(s).expect("default chord"), a));
        b("ctrl+shift+t", Action::NewTab);
        b("ctrl+shift+w", Action::CloseTab);
        b("ctrl+tab", Action::NextTab);
        b("ctrl+shift+tab", Action::PreviousTab);
        b("ctrl+pagedown", Action::NextTab);
        b("ctrl+pageup", Action::PreviousTab);
        for n in 1..=9u8 {
            b(&format!("alt+{n}"), Action::GotoTab(n));
        }
        b("ctrl+shift+p", Action::CommandPalette);
        // Claude Code binds Ctrl+D (exit) and Ctrl+S (stash), not these.
        b("ctrl+shift+d", Action::DiffView);
        b("ctrl+shift+s", Action::StatusPanel);
        // Claude Code binds Ctrl+R (history search), not this.
        b("ctrl+shift+r", Action::RenameTab);
        b("ctrl+shift+v", Action::Paste);
        b("shift+insert", Action::Paste);
        b("paste", Action::Paste); // a keyboard's Paste key
                                   // Windows Terminal convention; on Linux plain Ctrl+V reaches the
                                   // application (Claude Code pastes images with it).
        if cfg!(windows) {
            b("ctrl+v", Action::Paste);
        }
        Keymap { binds }
    }
}

impl Keymap {
    /// Applies `keybind` values: `CHORD=ACTION` or `CHORD=unbind`.
    pub fn apply(&mut self, value: &str) -> Result<(), String> {
        let (chord, action) = value.rsplit_once('=').ok_or_else(|| format!("keybind {value:?}: want CHORD=ACTION"))?;
        let chord = Chord::parse(chord).ok_or_else(|| format!("keybind {value:?}: bad key {chord:?}"))?;
        self.binds.retain(|(c, _)| *c != chord);
        if action.trim() == "unbind" {
            return Ok(());
        }
        let a = Action::parse(action.trim()).ok_or_else(|| format!("keybind {value:?}: unknown action {action:?}"))?;
        self.binds.push((chord, a));
        Ok(())
    }

    pub fn action(&self, chord: &Chord) -> Option<Action> {
        self.binds.iter().find(|(c, _)| c == chord).map(|(_, a)| *a)
    }

    /// First chord bound to `a` (for hints and the palette).
    pub fn chord_for(&self, a: Action) -> Option<&Chord> {
        self.binds.iter().find(|(_, x)| *x == a).map(|(c, _)| c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_parse_rebind_and_unbind() {
        let mut k = Keymap::default();
        let ch = |s: &str| Chord::parse(s).unwrap();
        assert_eq!(k.action(&ch("ctrl+shift+t")), Some(Action::NewTab));
        assert_eq!(k.action(&ch("Ctrl+Shift+T")), Some(Action::NewTab));
        assert_eq!(k.action(&ch("alt+3")), Some(Action::GotoTab(3)));
        assert_eq!(k.action(&ch("ctrl+shift+tab")), Some(Action::PreviousTab));
        // Claude Code's own keys stay free.
        assert_eq!(k.action(&ch("ctrl+shift+d")), Some(Action::DiffView));
        assert_eq!(k.action(&ch("ctrl+shift+r")), Some(Action::RenameTab));
        assert_eq!(k.action(&ch("ctrl+r")), None);
        for a in Action::all() {
            assert_eq!(Action::parse(&a.name()), Some(a), "{a:?}");
        }
        for s in
            ["ctrl+c", "ctrl+d", "ctrl+s", "ctrl+t", "ctrl+r", "alt+p", "alt+m", "alt+t", "shift+tab", "tab", "escape"]
        {
            assert_eq!(k.action(&ch(s)), None, "{s} must reach the application");
        }
        assert_eq!(k.action(&ch("ctrl+v")).is_some(), cfg!(windows));
        k.apply("ctrl+shift+n=new_tab").unwrap();
        k.apply("ctrl+shift+t=unbind").unwrap();
        assert_eq!(k.action(&ch("ctrl+shift+n")), Some(Action::NewTab));
        assert_eq!(k.action(&ch("ctrl+shift+t")), None);
        assert_eq!(k.chord_for(Action::NewTab).unwrap().to_string(), "Ctrl+Shift+N");
        assert!(k.apply("ctrl+shift+x=fly").is_err());
        assert!(k.apply("hyper+x=new_tab").is_err());
        assert!(k.apply("nothing").is_err());
        assert_eq!(Action::parse("goto_tab:10"), None);
        for a in Action::all() {
            assert_eq!(Action::parse(&a.name()), Some(a));
        }
    }
}
