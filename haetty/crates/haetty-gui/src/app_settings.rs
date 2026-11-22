//! The window's side of the settings (stage PS, DESIGN.md §2.4, §3.16): the settings button, the panel's keys,
//! mouse and effects, writing a change into the config file and applying what applies while running. A child of
//! `app.rs` (it reaches `Win`'s fields); the panel itself is `crate::settings`.

use std::time::{Duration, Instant};

use haetty_core::{Cell, TermColors};
use winit::event::KeyEvent;

use super::{padding_style, Win};
use crate::config::{self, Config, SettingsButton};
use crate::font::Fonts;
use crate::geom::Layout;
use crate::keymap::{Action, Chord};
use crate::render::paint::Look;
use crate::render::Renderer;
use crate::settings::{Effect, SettingsPanel, Source, UpdateCmd, UpdateView};
use crate::ui;

/// Changes that resize the grid (font, padding) reach the sessions once the person has stopped for this long.
pub const RESIZE_QUIET: Duration = Duration::from_millis(150);

/// What the panel reads: the settings in effect, the command line's keys, the theme and font lists.
struct PanelSource<'a> {
    cfg: &'a Config,
    cli: &'a [(String, String)],
    fonts: &'a mut Option<Vec<String>>,
    renderer: &'a mut dyn Renderer,
}

impl Source for PanelSource<'_> {
    fn value(&self, key: &str) -> String {
        self.cfg.value(key).unwrap_or_default()
    }
    fn on_command_line(&self, key: &str) -> bool {
        self.cli.iter().any(|(k, _)| k == key || (k == "window-padding" && key.starts_with("window-padding-")))
    }
    fn themes(&self) -> Vec<String> {
        let mut v: Vec<String> = haetty_core::theme::Theme::builtin_names().map(str::to_string).collect();
        let dir = haetty_proto::paths::config_dir().join("themes");
        let mut own: Vec<String> = std::fs::read_dir(dir)
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .filter(|n| !n.starts_with('.'))
                    .collect()
            })
            .unwrap_or_default();
        own.sort();
        for n in own {
            if !v.contains(&n) {
                v.push(n);
            }
        }
        v
    }
    fn fonts(&mut self) -> Vec<String> {
        if self.fonts.is_none() {
            *self.fonts = Some(self.renderer.monospace_families());
        }
        self.fonts.clone().unwrap_or_default()
    }
    fn path(&self) -> String {
        config::file_path().display().to_string()
    }
}

/// The fonts of `family` (`None`: the platform's defaults); an error when the system does not have it.
fn load_family(family: Option<&str>) -> Result<Fonts, String> {
    let f = Fonts::load(family)?;
    match family {
        Some(n) if !f.has_family(n) => Err(format!("no font family {n:?} on this system")),
        _ => Ok(f),
    }
}

/// `key` back to its default in `c`.
fn reset_key(c: &mut Config, key: &str) {
    match key {
        "font-family" => c.font_family = None,
        k if haetty_core::theme::KEYS.contains(&k) => c.colors.retain(|(ck, _)| ck != k),
        k => {
            let d = Config::default().value(k).unwrap_or_default();
            let _ = c.set(k, &d);
        }
    }
}

impl Win {
    /// Ctrl+, the palette's "Settings" and the button: open the panel, or close it.
    pub(super) fn toggle_settings(&mut self) {
        if self.settings_panel.is_some() {
            self.close_settings();
            return;
        }
        log::info!("settings: opened");
        self.palette = None;
        self.diff = None;
        self.settings_panel = Some(SettingsPanel::new(self.update_pending()));
        self.full = true;
        self.recompose();
    }

    fn close_settings(&mut self) {
        if std::mem::take(&mut self.previewing) {
            self.show_theme(&self.cfg.clone());
        }
        self.settings_panel = None;
        log::info!("settings: closed");
        self.full = true;
        self.recompose();
    }

    /// The panel's grid for `recompose` (the screen area), and its text cursor.
    pub(super) fn draw_settings(&mut self, g: ui::Greys) -> Option<ui::PanelGrid> {
        let (cols, rows) = self.settings_area();
        let view = self.update_view();
        let panel = self.settings_panel.as_mut()?;
        panel.set_update(view);
        let mut grid = vec![vec![Cell::default(); cols]; rows];
        let mut src = PanelSource {
            cfg: &self.cfg,
            cli: &self.settings,
            fonts: &mut self.font_list,
            renderer: self.renderer.as_mut(),
        };
        let at = panel.draw(&mut grid, g, &mut src);
        Some((grid, at))
    }

    /// Columns and rows of the screen area (what the panel covers).
    fn settings_area(&self) -> (usize, usize) {
        let rows = self.grid.rows.saturating_sub(self.top() + self.status_rows()) as usize;
        let cols = self.grid.cols.saturating_sub(self.panel_cols()) as usize;
        (cols, rows)
    }

    /// A key while the panel is open: nothing reaches the program.
    pub(super) fn settings_key(&mut self, ev: &KeyEvent, chord: &Chord) {
        if self.keys.action(chord) == Some(Action::Settings) {
            self.close_settings();
            return;
        }
        let plain = !chord.ctrl && !chord.alt && !chord.sup;
        let text = ev.text.as_deref().filter(|t| plain && !t.chars().any(char::is_control));
        let Some(panel) = self.settings_panel.as_mut() else { return };
        let mut src = PanelSource {
            cfg: &self.cfg,
            cli: &self.settings,
            fonts: &mut self.font_list,
            renderer: self.renderer.as_mut(),
        };
        let e = panel.key(&chord.key, chord.shift, text, &mut src);
        self.settings_effect(e);
    }

    /// Text without a key name (an IME commit, Windows `VK_PACKET`) while the panel is open.
    pub(super) fn settings_text(&mut self, text: &str) {
        if let Some(p) = self.settings_panel.as_mut() {
            p.commit(text);
        }
        self.recompose();
    }

    /// A left press at a window cell while the panel is open.
    pub(super) fn settings_click(&mut self, col: u16, row: u16) {
        let (cols, rows) = self.settings_area();
        let Some(r) = row.checked_sub(self.top()).map(usize::from) else { return };
        if r >= rows || col as usize >= cols {
            return;
        }
        let Some(panel) = self.settings_panel.as_mut() else { return };
        let mut src = PanelSource {
            cfg: &self.cfg,
            cli: &self.settings,
            fonts: &mut self.font_list,
            renderer: self.renderer.as_mut(),
        };
        let e = panel.click(col as usize, r, cols, rows, &mut src);
        self.settings_effect(e);
    }

    pub(super) fn settings_wheel(&mut self, notches: i32) {
        if let Some(p) = self.settings_panel.as_mut() {
            p.wheel(notches);
        }
        self.recompose();
    }

    fn settings_effect(&mut self, e: Effect) {
        match e {
            Effect::None => {}
            Effect::Close => return self.close_settings(),
            Effect::Set { key, value } => {
                let r = self.change_setting(key, value.as_deref());
                if let Some(p) = self.settings_panel.as_mut() {
                    match r {
                        Ok(note) => {
                            p.clear_error(key);
                            p.note(note);
                        }
                        Err(why) => p.set_error(key, why),
                    }
                }
                self.previewing = false;
            }
            Effect::Preview(name) => {
                let mut c = self.cfg.clone();
                c.theme = name;
                self.show_theme(&c);
                self.previewing = true;
            }
            Effect::EndPreview => {
                self.show_theme(&self.cfg.clone());
                self.previewing = false;
            }
            Effect::OpenConfigFile => {
                let path = config::file_path();
                let editor = std::env::var("EDITOR").ok().filter(|e| !e.trim().is_empty());
                match editor {
                    Some(ed) => {
                        let mut parts = ed.split_whitespace().map(str::to_string);
                        let program = parts.next().unwrap_or_default();
                        let mut args: Vec<String> = parts.collect();
                        args.push(path.display().to_string());
                        let cwd = path.parent().map(|p| p.display().to_string());
                        self.close_settings();
                        let spec = super::TabSpec { program, args, cwd, ..Default::default() };
                        if let Err(e) = self.open_spec(spec) {
                            self.show_hint(format!("the editor did not start: {e}"));
                        }
                        return;
                    }
                    None => {
                        if let Some(p) = self.settings_panel.as_mut() {
                            p.note(format!("no $EDITOR: the config file is {}", path.display()));
                        }
                    }
                }
            }
            Effect::Update(cmd) => self.update_effect(cmd),
            Effect::ReloadConfig => {
                let changed = self.reload_config(true);
                if let Some(p) = self.settings_panel.as_mut() {
                    p.note(if changed {
                        "config read again: applied".into()
                    } else {
                        "config read again: no change".into()
                    });
                }
            }
        }
        self.recompose();
    }

    /// Sets `key` (`None`: the default), checks what applies at once (a font the system lacks is refused before
    /// anything is written), writes the config file, then applies. The message for the panel's bottom line, or why
    /// nothing changed.
    pub(super) fn change_setting(&mut self, key: &'static str, value: Option<&str>) -> Result<String, String> {
        let mut next = self.cfg.clone();
        match value {
            Some(v) => next.set(key, v)?,
            None => reset_key(&mut next, key),
        }
        let fonts = if next.font_family != self.cfg.font_family {
            Some(load_family(next.font_family.as_deref())?)
        } else {
            None
        };
        let staged = next.font_family.clone();
        let w = self.cfg_file.write(key, value)?;
        self.config_mtime = config::file_mtime();
        // Not the typed value: only the key goes into the log.
        log::info!("settings: {key} written ({})", if value.is_some() { "a value" } else { "the default" });
        let next = if w.changed_outside {
            // Other lines changed elsewhere: what is in effect is the file as it is now.
            let (c, bad) = config::reread(&self.settings);
            for b in bad {
                log::warn!("{}: {b} (ignored)", config::file_path().display());
            }
            c
        } else {
            next
        };
        // A file changed elsewhere may name another family than the one loaded above.
        let fonts = if next.font_family == staged {
            fonts
        } else if next.font_family != self.cfg.font_family {
            load_family(next.font_family.as_deref()).ok()
        } else {
            None
        };
        let when = config::Setting::find(key).map(|s| s.when);
        self.apply_config(next, fonts);
        let mut note = match when {
            Some(config::When::NextStart) => format!("saved: {key} (from the next start)"),
            _ => format!("saved: {key}"),
        };
        if let Some(b) = w.backup {
            note.push_str(&format!(" · backup {}", b.file_name().map(|n| n.to_string_lossy()).unwrap_or_default()));
        }
        if w.changed_outside {
            note.push_str(" · the file had changed: read again, only this key written");
        }
        Ok(note)
    }

    /// The theme of `c` on the window and every tab (the SetTheme path), without changing what is in effect.
    fn show_theme(&mut self, c: &Config) {
        let (theme, msgs) = c.resolve_theme(&haetty_proto::paths::config_dir());
        for m in msgs {
            log::warn!("{m} (ignored)");
        }
        let look = Look { min_contrast: c.minimum_contrast, ..Look::of(&theme) };
        if theme != self.theme {
            for t in &self.tabs {
                t.session.handle.set_colors(TermColors::of(&theme));
            }
        }
        if look != self.look {
            self.renderer.set_look(look);
        }
        self.theme = theme;
        self.look = look;
        self.full = true;
    }

    /// Makes `next` the settings in effect and applies what applies while running (`When::Live`): colours,
    /// `minimum-contrast`, fonts (`fonts`: already loaded for a new family), padding, the settings button and the
    /// status panel's numbers. A grid that changes size reaches the sessions after [`RESIZE_QUIET`]. Returns
    /// whether anything shown changed.
    pub(super) fn apply_config(&mut self, next: Config, fonts: Option<Fonts>) -> bool {
        let old = std::mem::replace(&mut self.cfg, next);
        let c = self.cfg.clone();
        let (theme0, look0) = (self.theme.clone(), self.look);
        self.show_theme(&c);
        let mut changed = self.theme != theme0 || self.look != look0;
        let scale = self.window.scale_factor();
        let mut metrics = false;
        if let Some(f) = fonts {
            self.renderer.set_fonts(f);
            self.gear = if self.renderer.text_glyph('\u{2699}') { ui::GEAR } else { ui::GEAR_FALLBACK };
            log::info!("settings: fonts changed; settings button {}", if self.gear == ui::GEAR { "⚙" } else { "≡" });
            metrics = true;
        }
        if c.font_size != old.font_size {
            self.renderer.set_font_px((c.font_size * 96.0 / 72.0 * scale) as f32);
            metrics = true;
        }
        let pad = padding_style(&c, scale);
        if padding_style(&old, scale) != pad {
            self.renderer.set_padding(pad);
            metrics = true;
        }
        if metrics {
            self.layout = Layout { cell: self.renderer.cell_metrics(), padding: pad.min, balance: pad.balance };
            let grid = self.layout.grid(self.size.width, self.size.height);
            if grid != self.grid && self.size.width > 0 && self.size.height > 0 {
                log::debug!("settings: grid {}x{} -> {}x{}", self.grid.cols, self.grid.rows, grid.cols, grid.rows);
                self.grid = grid;
                self.pending_relayout = Some(Instant::now() + RESIZE_QUIET);
            }
            self.ime_at = None;
            changed = true;
        }
        if c.settings_button == SettingsButton::Never {
            self.gear_near = false;
        }
        self.token_status = c.token_status;
        self.clear_hint = c.clear_hint;
        self.context_window = c.context_window;
        self.notifications = c.notifications;
        changed |= old.settings_button != c.settings_button
            || old.token_status != c.token_status
            || old.clear_hint != c.clear_hint
            || old.context_window != c.context_window
            || old.notifications != c.notifications;
        self.full = true;
        self.recompose();
        changed
    }

    /// Where the settings button is: the last cell of the window (a Claude Code tab's status row, or over the last
    /// screen cell of another tab while the pointer is near).
    pub(super) fn gear_place(&self) -> Option<(usize, usize)> {
        if self.cfg.settings_button == SettingsButton::Never || self.pending_start.is_some() || self.tabs.is_empty() {
            return None;
        }
        let (cols, rows) = (self.grid.cols as usize, self.grid.rows as usize);
        if cols < 4 || rows < 2 {
            return None;
        }
        let at = Some((rows - 1, cols - 1));
        // A Claude Code tab's status row, or the docked sidebar's corner: haetty's own cell, always shown.
        if self.tab().integrated || self.panel_cols() > 0 {
            return at;
        }
        let tracking = self.tab().snap.as_ref().is_some_and(|s| s.modes.mouse_tracking);
        (self.gear_near && !tracking && self.palette.is_none() && self.diff.is_none() && self.settings_panel.is_none())
            .then_some(at)
            .flatten()
    }

    /// The pointer moved: the button shows near the corner (other tabs) and lights up under the pointer.
    pub(super) fn gear_pointer(&mut self) {
        let (col, row) = self.layout.cell_at(self.origin(), self.mouse.x, self.mouse.y, self.grid);
        let (cols, rows) = (self.grid.cols, self.grid.rows);
        let near = match self.cfg.settings_button {
            SettingsButton::Never => false,
            SettingsButton::Always => true,
            SettingsButton::Auto => col + 6 >= cols && row + 2 >= rows,
        };
        let hover = (col + 1, row + 1) == (cols, rows);
        if near != self.gear_near || hover != self.gear_hover {
            self.gear_near = near;
            self.gear_hover = hover;
            self.recompose();
        }
    }

    /// The pointer left the window, or a key was typed: the button over a program's cell goes.
    pub(super) fn gear_away(&mut self, left: bool) {
        let hover = self.gear_hover && !left;
        if self.gear_near || hover != self.gear_hover {
            self.gear_near = false;
            self.gear_hover = hover;
            self.recompose();
        }
    }

    /// A left press on the button opens the panel; true when it took the press.
    pub(super) fn gear_click(&mut self) -> bool {
        let Some((r, c)) = self.gear_place() else { return false };
        let (col, row) = self.layout.cell_at(self.origin(), self.mouse.x, self.mouse.y, self.grid);
        if (row as usize, col as usize) != (r, c) {
            return false;
        }
        self.toggle_settings();
        true
    }

    /// The grid change of a font or padding change, once the changes stopped (`tick`).
    pub(super) fn settle_relayout(&mut self, now: Instant) -> Option<Instant> {
        match self.pending_relayout {
            Some(t) if now >= t => {
                self.pending_relayout = None;
                self.relayout();
                None
            }
            t => t,
        }
    }

    // ---- Updates (stage PU, DESIGN.md §2.4, §3.17) ------------------------------------------------------------

    /// A newer version is out or installed and waiting (the button in the text colour).
    pub(super) fn update_pending(&self) -> bool {
        self.updater.release.is_some() || self.updater.done.is_some()
    }

    /// The palette's "Updates": the panel at its Updates group.
    pub(super) fn open_updates(&mut self) {
        if self.settings_panel.is_none() {
            self.toggle_settings();
        }
        if let Some(p) = self.settings_panel.as_mut() {
            p.show_updates();
        }
        self.recompose();
    }

    /// What the Updates group shows now.
    pub(super) fn update_view(&self) -> UpdateView {
        let u = &self.updater;
        let current = format!("{} ({})", env!("HAETTY_BUILD_VERSION"), env!("HAETTY_BUILD_COMMIT"));
        let latest = u
            .release
            .as_ref()
            .map(|r| format!("{} · {}", r.manifest.version, r.manifest.built.get(..10).unwrap_or("")));
        let notes =
            u.release.as_ref().map(|r| r.manifest.notes.lines().map(str::to_string).collect()).unwrap_or_default();
        let blocked = u.release.as_ref().and_then(|r| u.blocked(r));
        let last_check = if u.last.last_ms == 0 {
            "never".into()
        } else {
            let at = haetty_update::rfc3339_s(u.last.last_ms / 1000);
            format!("{} {} UTC: {}", &at[..10], &at[11..16], u.last.note)
        };
        let asking = self.tabs.iter().filter(|t| t.asking()).count();
        let working =
            self.tabs.iter().filter(|t| t.task.is_some() && t.claude.as_ref().is_some_and(|c| c.busy)).count();
        let mut warn = vec![];
        if asking > 0 {
            warn.push(format!("{asking} tab(s) wait for a permission answer"));
        }
        if working > 0 {
            warn.push(format!("{working} worker(s) are working"));
        }
        UpdateView {
            current,
            latest,
            notes,
            blocked,
            progress: u.progress.clone(),
            ready: u.done.as_ref().map(|d| d.new.clone()),
            rollback: u.rollback_target(),
            last_check,
            warn: (!warn.is_empty()).then(|| warn.join(" · ")),
            tabs: self.tabs.len(),
            note: u.note.clone(),
        }
    }

    /// The person chose an update action in the panel (the only way one runs: keys and clicks reach the panel only
    /// through `settings_key` / `settings_click`; no IPC message, command line or program input does).
    fn update_effect(&mut self, cmd: UpdateCmd) {
        match cmd {
            UpdateCmd::CheckNow => {
                let url = self.cfg.update_url.clone();
                self.updater.check(&url);
            }
            UpdateCmd::Download => {
                log::info!("update: download and install chosen");
                self.updater.install(self.cfg.update_keep as usize);
            }
            UpdateCmd::Later => {
                if let Some(p) = self.settings_panel.as_mut() {
                    p.note(
                        "the new version starts with haetty's next start (all windows closed, the daemon ended)".into(),
                    );
                }
            }
            UpdateCmd::Restart => {
                log::info!("update: restart chosen");
                let previous = self.updater.done.as_ref().map(|d| d.previous.clone());
                self.restart_req = Some(previous);
            }
            UpdateCmd::RollBack => {
                let r = match &self.updater.form {
                    haetty_update::layout::Form::Versioned { opt, link, .. } => match self.updater.rollback_target() {
                        Some(t) => haetty_update::layout::activate(opt, link, &t).map(|_| ()),
                        None => Err("no version to go back to".into()),
                    },
                    haetty_update::layout::Form::Single { exe } => haetty_update::layout::rollback_single(exe),
                    f => Err(f.refusal().unwrap_or_default()),
                };
                match r {
                    Ok(()) => {
                        log::info!("update: rolled back by the person");
                        haetty_update::state::confirm(&crate::updater::dir());
                        self.restart_req = Some(None);
                    }
                    Err(e) => {
                        if let Some(p) = self.settings_panel.as_mut() {
                            p.note(format!("not rolled back: {e}"));
                        }
                    }
                }
            }
        }
    }

    /// The updater's timers (`tick`): a new version confirmed after 20 s up, the daily check. The next deadline.
    pub(super) fn update_tick(&mut self, now: Instant) -> Option<Instant> {
        if let Some(t) = self.updater.confirm_at {
            if now >= t {
                haetty_update::state::confirm(&crate::updater::dir());
                log::info!("update: this version stayed up; kept");
                self.updater.confirm_at = None;
            }
        }
        let allowed = self.cfg.update_check && crate::updater::may_check(&self.updater.form);
        if self.updater.due(now, allowed) == Some(true) {
            let url = self.cfg.update_url.clone();
            self.updater.check(&url);
        }
        [self.updater.confirm_at, self.updater.next_check].into_iter().flatten().min()
    }
}
