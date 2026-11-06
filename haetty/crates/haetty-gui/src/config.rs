//! Settings. Each key can be given on the command line as `--KEY VALUE` or
//! `--KEY=VALUE`, or in the config file (Linux `$XDG_CONFIG_HOME/haetty/config`,
//! Windows `%APPDATA%\haetty\config`) as `key = value` lines like Ghostty's,
//! `#` comments; the command line wins. `keybind = CHORD=ACTION` (file only)
//! changes shortcuts (`crate::keymap`). Bad lines are skipped and logged.
//!
//! | key | values | default |
//! |---|---|---|
//! | `window-padding` | logical pixels around the grid (minimum), both axes | — |
//! | `window-padding-x`, `window-padding-y` | the same, left/right and top/bottom only | 10, 6 |
//! | `window-padding-balance` | `true` / `false`: spread the leftover space evenly on all sides | `true` |
//! | `window-padding-color` | `background` / `extend` / `extend-always` | `extend` |
//! | `decorations` | `native` / `integrated` / `none` | `native` |
//! | `claude-integration` | `true` / `false`: hooks and status line in Claude Code tabs | `true` |
//! | `notifications` | `true` / `false`: desktop notification when Claude Code waits and the window is unfocused (Linux `notify-send`) | `true` |
//! | `restore-tabs` | `true` / `false`: reopen the previous run's tabs at start | `true` |
//! | `scrollback-limit` | bytes of scrollback per session (as Ghostty; ~1.1 KB per 120-column row) | 12000000 |
//! | `theme` | a built-in theme name, a theme file's name in `<config dir>/themes/`, or a path (with `/` or `\`; relative to the config directory) | [`haetty_core::theme::DEFAULT_THEME`] |
//! | `palette` | `N=COLOR` (N 0–15; repeatable), on top of the theme | the theme's |
//! | `minimum-contrast` | 1–21, as Ghostty: text below this WCAG contrast with its background turns white or black | 1 (off) |
//! | `background`, `foreground`, `cursor-color`, `cursor-text`, `selection-background`, `selection-foreground` | a colour (Ghostty syntax), on top of the theme | the theme's |
//!
//! The colour keys and theme files use Ghostty's theme file format; they are
//! applied after the theme wherever they stand in the file.

use std::path::Path;

use haetty_core::theme::{self, Theme};

/// Colour of the padding around the grid (Ghostty's `window-padding-color`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaddingColor {
    /// The terminal's default background.
    Background,
    /// The background of the nearest cell. Vertically (top/bottom padding)
    /// this is skipped on the primary screen when the nearest row has a cell
    /// with the default background or a powerline glyph.
    Extend,
    /// Like `Extend`, without the heuristics.
    ExtendAlways,
}

/// Window decorations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decorations {
    /// The platform's title bar and frame (coloured to match on Windows 11).
    Native,
    /// Title bar drawn by haetty with tabs — not implemented (milestone 2's
    /// optional P6 was not done); falls back to `Native`.
    Integrated,
    /// No title bar or frame.
    None,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    /// Minimum padding left/right and top/bottom, logical pixels.
    pub window_padding_x: f64,
    pub window_padding_y: f64,
    pub padding_balance: bool,
    pub padding_color: PaddingColor,
    pub decorations: Decorations,
    /// Bytes ([`haetty_core::SessionConfig::scrollback_limit`]).
    pub scrollback_limit: usize,
    /// Reopen the previous run's tabs (tab set "previous") at start.
    pub restore_tabs: bool,
    /// Claude Code tabs get haetty's hooks and status line.
    pub claude_integration: bool,
    pub notifications: bool,
    /// `theme =`: a name or a path ([`theme::find`]).
    pub theme: String,
    /// Colour keys (`palette`, `background`, …) in the order given, applied
    /// after the theme.
    pub colors: Vec<(String, String)>,
    /// 1–21 (Ghostty's `minimum-contrast`; 1 = off).
    pub minimum_contrast: f32,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            window_padding_x: 10.0,
            window_padding_y: 6.0,
            padding_balance: true,
            padding_color: PaddingColor::Extend,
            decorations: Decorations::Native,
            scrollback_limit: haetty_core::DEFAULT_SCROLLBACK_LIMIT,
            restore_tabs: true,
            claude_integration: true,
            notifications: true,
            theme: theme::DEFAULT_THEME.to_string(),
            colors: vec![],
            minimum_contrast: 1.0,
        }
    }
}

pub const KEYS: &[&str] = &[
    "window-padding",
    "window-padding-x",
    "window-padding-y",
    "window-padding-balance",
    "window-padding-color",
    "decorations",
    "scrollback-limit",
    "restore-tabs",
    "claude-integration",
    "notifications",
    "theme",
    "palette",
    "background",
    "foreground",
    "cursor-color",
    "cursor-text",
    "selection-background",
    "selection-foreground",
    "minimum-contrast",
];

impl Config {
    pub fn set(&mut self, key: &str, value: &str) -> Result<(), String> {
        let bad = || format!("invalid value {value:?} for {key}");
        match key {
            "window-padding" | "window-padding-x" | "window-padding-y" => {
                let v: f64 = value.parse().map_err(|_| bad())?;
                if !(0.0..=200.0).contains(&v) {
                    return Err(bad());
                }
                if key != "window-padding-y" {
                    self.window_padding_x = v;
                }
                if key != "window-padding-x" {
                    self.window_padding_y = v;
                }
            }
            "restore-tabs" => self.restore_tabs = parse_bool(value).ok_or_else(bad)?,
            "claude-integration" => self.claude_integration = parse_bool(value).ok_or_else(bad)?,
            "notifications" => self.notifications = parse_bool(value).ok_or_else(bad)?,
            "window-padding-balance" => {
                self.padding_balance = match value {
                    "true" | "yes" | "1" => true,
                    "false" | "no" | "0" => false,
                    _ => return Err(bad()),
                }
            }
            "window-padding-color" => {
                self.padding_color = match value {
                    "background" => PaddingColor::Background,
                    "extend" => PaddingColor::Extend,
                    "extend-always" => PaddingColor::ExtendAlways,
                    _ => return Err(bad()),
                }
            }
            "decorations" => {
                self.decorations = match value {
                    "native" => Decorations::Native,
                    "integrated" => Decorations::Integrated,
                    "none" => Decorations::None,
                    _ => return Err(bad()),
                }
            }
            "scrollback-limit" => {
                // Plain bytes; `_` separators allowed (12_000_000).
                let v: u64 = value.replace('_', "").parse().map_err(|_| bad())?;
                self.scrollback_limit = usize::try_from(v).map_err(|_| bad())?;
            }
            "minimum-contrast" => {
                // As Ghostty: clamped to 1–21.
                let v: f32 = value.parse().map_err(|_| bad())?;
                if !v.is_finite() {
                    return Err(bad());
                }
                self.minimum_contrast = v.clamp(1.0, 21.0);
            }
            "theme" => {
                if value.is_empty() {
                    return Err(bad());
                }
                self.theme = value.to_string();
            }
            k if theme::KEYS.contains(&k) => {
                // Checked now (bad lines are reported with their number),
                // applied after the theme.
                Theme::base().set(k, value)?;
                self.colors.push((k.to_string(), value.to_string()));
            }
            _ => return Err(format!("unknown setting {key:?}")),
        }
        Ok(())
    }

    /// The theme `theme` names (in `config_dir/themes` or built in), then the
    /// colour keys on top, in order. An unknown or unreadable theme falls back
    /// to the default theme. Returns messages for what was skipped.
    pub fn resolve_theme(&self, config_dir: &Path) -> (Theme, Vec<String>) {
        let mut msgs = vec![];
        let mut t = match theme::load(&self.theme, Some(config_dir), Some(&config_dir.join("themes"))) {
            Ok((t, bad)) => {
                msgs.extend(bad);
                t
            }
            Err(e) => {
                msgs.push(format!("{e}; using {:?}", theme::DEFAULT_THEME));
                Theme::default()
            }
        };
        for (k, v) in &self.colors {
            if let Err(e) = t.set(k, v) {
                msgs.push(e);
            }
        }
        (t, msgs)
    }

    /// Applies a config file's text; returns one message per skipped line.
    pub fn apply_file(&mut self, text: &str, keys: &mut crate::keymap::Keymap) -> Vec<String> {
        let mut bad = vec![];
        // The same line syntax as theme files (haetty_core::theme::lines).
        for (n, kv) in theme::lines(text) {
            let r = kv.and_then(|(k, v)| if k == "keybind" { keys.apply(v) } else { self.set(k, v) });
            if let Err(e) = r {
                bad.push(format!("line {n}: {e}"));
            }
        }
        bad
    }
}

/// The config file's path (`config_dir()/config`).
pub fn file_path() -> std::path::PathBuf {
    haetty_proto::paths::config_dir().join("config")
}

/// The config file's modification time (`None`: no file).
pub fn file_mtime() -> Option<std::time::SystemTime> {
    std::fs::metadata(file_path()).and_then(|m| m.modified()).ok()
}

/// The config file read again, with the command line's `settings` on top
/// (key bindings are not re-read); messages for skipped lines.
pub fn reread(settings: &[(String, String)]) -> (Config, Vec<String>) {
    let mut c = Config::default();
    let mut bad = vec![];
    if let Ok(text) = std::fs::read_to_string(file_path()) {
        bad = c.apply_file(&text, &mut crate::keymap::Keymap::default());
    }
    for (k, v) in settings {
        let _ = c.set(k, v);
    }
    (c, bad)
}

fn parse_bool(v: &str) -> Option<bool> {
    match v {
        "true" | "yes" | "1" => Some(true),
        "false" | "no" | "0" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_parse_and_reject_bad_values() {
        let mut c = Config::default();
        assert_eq!(c.padding_color, PaddingColor::Extend);
        assert!(c.padding_balance);
        c.set("window-padding-color", "extend-always").unwrap();
        c.set("window-padding-balance", "false").unwrap();
        c.set("window-padding", "10").unwrap();
        c.set("decorations", "none").unwrap();
        assert_eq!(c.padding_color, PaddingColor::ExtendAlways);
        assert!(!c.padding_balance);
        assert_eq!((c.window_padding_x, c.window_padding_y), (10.0, 10.0));
        c.set("window-padding-y", "3").unwrap();
        assert_eq!((c.window_padding_x, c.window_padding_y), (10.0, 3.0));
        assert_eq!((Config::default().window_padding_x, Config::default().window_padding_y), (10.0, 6.0));
        assert_eq!(c.decorations, Decorations::None);
        assert!(c.set("window-padding-color", "red").is_err());
        assert!(c.set("decorations", "fancy").is_err());
        assert!(c.set("window-padding", "-1").is_err());
        assert!(c.set("nope", "1").is_err());
        assert_eq!(c.scrollback_limit, haetty_core::DEFAULT_SCROLLBACK_LIMIT);
        c.set("scrollback-limit", "4_000_000").unwrap();
        assert_eq!(c.scrollback_limit, 4_000_000);
        c.set("scrollback-limit", "0").unwrap();
        assert_eq!(c.scrollback_limit, 0);
        assert!(c.set("scrollback-limit", "10MB").is_err());
        assert!(c.set("scrollback-limit", "-1").is_err());
        c.set("minimum-contrast", "4.5").unwrap();
        assert_eq!(c.minimum_contrast, 4.5);
        c.set("minimum-contrast", "40").unwrap();
        assert_eq!(c.minimum_contrast, 21.0, "clamped like Ghostty");
        c.set("minimum-contrast", "0").unwrap();
        assert_eq!(c.minimum_contrast, 1.0);
        assert!(c.set("minimum-contrast", "high").is_err() && c.set("minimum-contrast", "NaN").is_err());
        // Config file: comments, quotes, keybind, bad lines reported.
        let mut keys = crate::keymap::Keymap::default();
        let mut c = Config::default();
        let bad = c.apply_file(
            "# comment\n\nwindow-padding = 8\ndecorations = \"none\"\nkeybind = ctrl+shift+n=new_tab\nnope = 1\nwindow-padding-color\nscrollback-limit = x\n",
            &mut keys,
        );
        assert_eq!((c.window_padding_x, c.window_padding_y, c.decorations), (8.0, 8.0, Decorations::None));
        assert!(keys.action(&crate::keymap::Chord::parse("ctrl+shift+n").unwrap()).is_some());
        assert_eq!(bad.len(), 3, "{bad:?}");
        assert!(bad[0].starts_with("line 6:") && bad[1].starts_with("line 7:") && bad[2].starts_with("line 8:"));
        for k in KEYS.iter().filter(|k| **k != "theme") {
            assert!(!c.clone().set(k, "?").unwrap_err().starts_with("unknown"));
        }
    }

    fn rgb(v: u32) -> haetty_core::Rgb {
        haetty_core::Rgb { r: (v >> 16) as u8, g: (v >> 8) as u8, b: v as u8 }
    }

    #[test]
    fn theme_keys_apply_after_the_theme_and_user_themes_win() {
        let dir = haetty_testkit::run_dir("config-themes");
        let themes = dir.join("themes");
        std::fs::create_dir_all(&themes).unwrap();
        std::fs::write(themes.join("Mine"), "background = #010203\npalette = 3=#aa0000\nbogus = 1\n").unwrap();
        // A user file named like a built-in theme wins over it.
        std::fs::write(themes.join("IBM 5153 CGA"), "background = #050505\n").unwrap();
        std::fs::write(dir.join("by-path"), "foreground = #0a0b0c\n").unwrap();
        let mut keys = crate::keymap::Keymap::default();
        let mut c = Config::default();
        // A colour line before `theme =` still overrides the theme.
        let bad = c.apply_file(
            "background = #102030\ntheme = \"IBM 5153 CGA (Black)\"\npalette = 1=#ff0000\npalette = 99=#ffffff\nforeground = nope\n",
            &mut keys,
        );
        assert_eq!(bad.len(), 2, "{bad:?}");
        let (t, msgs) = c.resolve_theme(&dir);
        assert!(msgs.is_empty(), "{msgs:?}");
        let black = Theme::builtin("IBM 5153 CGA (Black)").unwrap();
        assert_eq!((t.background, t.palette[1]), (rgb(0x102030), rgb(0xff0000)));
        assert_eq!((t.foreground, t.palette[2]), (black.foreground, black.palette[2]));
        // A theme in <config>/themes by name; its bad line is reported.
        c.set("theme", "Mine").unwrap();
        let (t, msgs) = c.resolve_theme(&dir);
        assert_eq!((t.palette[3], t.background), (rgb(0xaa0000), rgb(0x102030)), "the config's background still wins");
        assert!(msgs.len() == 1 && msgs[0].contains("line 3"), "{msgs:?}");
        c.colors.clear();
        c.set("theme", "IBM 5153 CGA").unwrap();
        assert_eq!(c.resolve_theme(&dir).0.background, rgb(0x050505), "user file over the built-in");
        // A path, relative to the config directory.
        c.set("theme", "./by-path").unwrap();
        assert_eq!(c.resolve_theme(&dir).0.foreground, rgb(0x0a0b0c));
        // Unknown: the default theme and a message.
        c.set("theme", "No Such Theme").unwrap();
        let (t, msgs) = c.resolve_theme(&dir);
        assert_eq!(t, Theme::default());
        assert!(msgs[0].contains("No Such Theme"));
        haetty_testkit::remove_dir_all(&dir);
    }
}
