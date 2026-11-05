//! Settings. Each key can be given on the command line as `--KEY VALUE` or
//! `--KEY=VALUE`, or in the config file (Linux `$XDG_CONFIG_HOME/ccterm/config`,
//! Windows `%APPDATA%\ccterm\config`) as `key = value` lines like Ghostty's,
//! `#` comments; the command line wins. `keybind = CHORD=ACTION` (file only)
//! changes shortcuts (`crate::keymap`). Bad lines are skipped and logged.
//!
//! | key | values | default |
//! |---|---|---|
//! | `window-padding` | logical pixels around the grid (minimum) | 4 |
//! | `window-padding-balance` | `true` / `false`: spread the leftover space evenly on all sides | `true` |
//! | `window-padding-color` | `background` / `extend` / `extend-always` | `extend` |
//! | `decorations` | `native` / `integrated` / `none` | `native` |
//! | `claude-integration` | `true` / `false`: hooks and status line in Claude Code tabs | `true` |
//! | `restore-tabs` | `true` / `false`: reopen the previous run's tabs at start | `true` |
//! | `scrollback-limit` | bytes of scrollback per session (as Ghostty; ~1.1 KB per 120-column row) | 12000000 |

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
    /// Title bar drawn by ccterm with tabs — planned for the tabs milestone;
    /// falls back to `Native` until then.
    Integrated,
    /// No title bar or frame.
    None,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub window_padding: f64,
    pub padding_balance: bool,
    pub padding_color: PaddingColor,
    pub decorations: Decorations,
    /// Bytes ([`ccterm_core::SessionConfig::scrollback_limit`]).
    pub scrollback_limit: usize,
    /// Reopen the previous run's tabs (tab set "previous") at start.
    pub restore_tabs: bool,
    /// Claude Code tabs get ccterm's hooks and status line.
    pub claude_integration: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            window_padding: 4.0,
            padding_balance: true,
            padding_color: PaddingColor::Extend,
            decorations: Decorations::Native,
            scrollback_limit: ccterm_core::DEFAULT_SCROLLBACK_LIMIT,
            restore_tabs: true,
            claude_integration: true,
        }
    }
}

pub const KEYS: &[&str] = &[
    "window-padding",
    "window-padding-balance",
    "window-padding-color",
    "decorations",
    "scrollback-limit",
    "restore-tabs",
    "claude-integration",
];

impl Config {
    pub fn set(&mut self, key: &str, value: &str) -> Result<(), String> {
        let bad = || format!("invalid value {value:?} for {key}");
        match key {
            "window-padding" => {
                let v: f64 = value.parse().map_err(|_| bad())?;
                if !(0.0..=200.0).contains(&v) {
                    return Err(bad());
                }
                self.window_padding = v;
            }
            "restore-tabs" => self.restore_tabs = parse_bool(value).ok_or_else(bad)?,
            "claude-integration" => self.claude_integration = parse_bool(value).ok_or_else(bad)?,
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
            _ => return Err(format!("unknown setting {key:?}")),
        }
        Ok(())
    }

    /// Applies a config file's text; returns one message per skipped line.
    pub fn apply_file(&mut self, text: &str, keys: &mut crate::keymap::Keymap) -> Vec<String> {
        let mut bad = vec![];
        for (n, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                bad.push(format!("line {}: expected key = value", n + 1));
                continue;
            };
            let (k, v) = (k.trim(), v.trim());
            let v = v.strip_prefix('"').and_then(|x| x.strip_suffix('"')).unwrap_or(v);
            let r = if k == "keybind" { keys.apply(v) } else { self.set(k, v) };
            if let Err(e) = r {
                bad.push(format!("line {}: {e}", n + 1));
            }
        }
        bad
    }
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
        assert_eq!(c.window_padding, 10.0);
        assert_eq!(c.decorations, Decorations::None);
        assert!(c.set("window-padding-color", "red").is_err());
        assert!(c.set("decorations", "fancy").is_err());
        assert!(c.set("window-padding", "-1").is_err());
        assert!(c.set("nope", "1").is_err());
        assert_eq!(c.scrollback_limit, ccterm_core::DEFAULT_SCROLLBACK_LIMIT);
        c.set("scrollback-limit", "4_000_000").unwrap();
        assert_eq!(c.scrollback_limit, 4_000_000);
        c.set("scrollback-limit", "0").unwrap();
        assert_eq!(c.scrollback_limit, 0);
        assert!(c.set("scrollback-limit", "10MB").is_err());
        assert!(c.set("scrollback-limit", "-1").is_err());
        // Config file: comments, quotes, keybind, bad lines reported.
        let mut keys = crate::keymap::Keymap::default();
        let mut c = Config::default();
        let bad = c.apply_file(
            "# comment\n\nwindow-padding = 8\ndecorations = \"none\"\nkeybind = ctrl+shift+n=new_tab\nnope = 1\nwindow-padding-color\nscrollback-limit = x\n",
            &mut keys,
        );
        assert_eq!((c.window_padding, c.decorations), (8.0, Decorations::None));
        assert!(keys.action(&crate::keymap::Chord::parse("ctrl+shift+n").unwrap()).is_some());
        assert_eq!(bad.len(), 3, "{bad:?}");
        assert!(bad[0].starts_with("line 6:") && bad[1].starts_with("line 7:") && bad[2].starts_with("line 8:"));
        for k in KEYS {
            assert!(!c.clone().set(k, "?").unwrap_err().starts_with("unknown"));
        }
    }
}
