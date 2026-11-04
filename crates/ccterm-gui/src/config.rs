//! Settings. Each key can be given on the command line as `--KEY VALUE` or
//! `--KEY=VALUE`; a config file (later milestone) will feed the same
//! [`Config::set`].
//!
//! | key | values | default |
//! |---|---|---|
//! | `window-padding` | logical pixels around the grid (minimum) | 4 |
//! | `window-padding-balance` | `true` / `false`: spread the leftover space evenly on all sides | `true` |
//! | `window-padding-color` | `background` / `extend` / `extend-always` | `extend` |

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

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub window_padding: f64,
    pub padding_balance: bool,
    pub padding_color: PaddingColor,
}

impl Default for Config {
    fn default() -> Self {
        Config { window_padding: 4.0, padding_balance: true, padding_color: PaddingColor::Extend }
    }
}

pub const KEYS: &[&str] = &["window-padding", "window-padding-balance", "window-padding-color"];

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
            _ => return Err(format!("unknown setting {key:?}")),
        }
        Ok(())
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
        assert_eq!(c.padding_color, PaddingColor::ExtendAlways);
        assert!(!c.padding_balance);
        assert_eq!(c.window_padding, 10.0);
        assert!(c.set("window-padding-color", "red").is_err());
        assert!(c.set("window-padding", "-1").is_err());
        assert!(c.set("nope", "1").is_err());
        for k in KEYS {
            assert!(!c.clone().set(k, "?").unwrap_err().starts_with("unknown"));
        }
    }
}
