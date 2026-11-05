//! Colour themes in Ghostty's theme file format: `key = value` lines with
//! `palette = N=COLOR` (N 0–15), `background`, `foreground`, `cursor-color`,
//! `cursor-text`, `selection-background`, `selection-foreground`. Colours
//! use Ghostty's syntax (`#rrggbb`, `rrggbb`, X11 names, …; parsed by
//! libghostty-vt itself). The line syntax is haetty's config file syntax:
//! [`lines`] is shared by both.
//!
//! Built-in themes are the files in `assets/themes/`, embedded. A theme file
//! sets what it lists; everything else keeps the value of "haetty classic"
//! (Ghostty's default 16 colours, `#dcdcdc` on `#1e1e1e`). Palette entries
//! 16–255 are never themed: they stay xterm's colour cube and grey ramp.

use std::path::{Path, PathBuf};

use crate::Rgb;

const CLASSIC: &str = "haetty classic";

/// The theme haetty uses unless the config names another.
pub const DEFAULT_THEME: &str = "IBM 5153 CGA";

/// Name and file text of every built-in theme.
const BUILTIN: &[(&str, &str)] = &[
    ("IBM 5153 CGA", include_str!("../../../assets/themes/IBM 5153 CGA")),
    ("IBM 5153 CGA (Black)", include_str!("../../../assets/themes/IBM 5153 CGA (Black)")),
    (CLASSIC, include_str!("../../../assets/themes/haetty classic")),
];

/// Built-in themes whose selection background is filled in when it is
/// missing or equal to the foreground (both CGA files set it to the
/// foreground: selected text would vanish wherever the selection
/// foreground is not used). Only these; a user's file is used as written.
const SELECTION_FILLED: &[&str] = &["IBM 5153 CGA", "IBM 5153 CGA (Black)"];

/// The fill-in for [`SELECTION_FILLED`] themes (their colour 8).
pub const SELECTION_FILL: Rgb = Rgb { r: 0x4e, g: 0x4e, b: 0x4e };

/// Keys a theme file (and the config file, on top of the theme) may set.
pub const KEYS: &[&str] = &[
    "palette",
    "background",
    "foreground",
    "cursor-color",
    "cursor-text",
    "selection-background",
    "selection-foreground",
];

/// Largest theme file read (a theme is ~22 short lines).
const MAX_FILE: u64 = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Theme {
    /// Colours 0–15.
    pub palette: [Rgb; 16],
    pub background: Rgb,
    pub foreground: Rgb,
    /// `None`: the foreground.
    pub cursor_color: Option<Rgb>,
    /// Text under a block cursor; `None`: the cell's background.
    pub cursor_text: Option<Rgb>,
    /// `None` (both): selected cells swap their colours.
    pub selection_background: Option<Rgb>,
    pub selection_foreground: Option<Rgb>,
}

impl Default for Theme {
    /// [`DEFAULT_THEME`].
    fn default() -> Self {
        Theme::builtin(DEFAULT_THEME).expect("the default theme is built in")
    }
}

/// `key = value` lines (line number from 1, key, value): blank lines and
/// `#` comments skipped, spaces around key and value trimmed, one pair of
/// double quotes around the value removed. The value may contain `=`
/// (`palette = 1=#ff0000`, `keybind = ctrl+t=new_tab`).
pub fn lines(text: &str) -> impl Iterator<Item = (usize, Result<(&str, &str), String>)> {
    text.lines().enumerate().filter_map(|(n, line)| {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return None;
        }
        let kv = match line.split_once('=') {
            None => Err("expected key = value".to_string()),
            Some((k, v)) => {
                let (k, v) = (k.trim(), v.trim());
                Ok((k, v.strip_prefix('"').and_then(|x| x.strip_suffix('"')).unwrap_or(v)))
            }
        };
        Some((n + 1, kv))
    })
}

/// A colour in Ghostty's syntax.
pub fn parse_color(v: &str) -> Result<Rgb, String> {
    libghostty_vt::style::RgbColor::parse(v).map(Into::into).map_err(|_| format!("invalid colour {v:?}"))
}

impl Theme {
    /// Names of the built-in themes.
    pub fn builtin_names() -> impl Iterator<Item = &'static str> {
        BUILTIN.iter().map(|(n, _)| *n)
    }

    /// A built-in theme (exact name), with the selection fill-in applied.
    pub fn builtin(name: &str) -> Option<Theme> {
        let (_, text) = BUILTIN.iter().find(|(n, _)| *n == name)?;
        let (mut t, bad) = Theme::parse(text);
        debug_assert!(bad.is_empty(), "built-in theme {name}: {bad:?}");
        if SELECTION_FILLED.contains(&name) && t.selection_background.is_none_or(|b| b == t.foreground) {
            t.selection_background = Some(SELECTION_FILL);
        }
        Some(t)
    }

    /// Ghostty's default 16 colours (libghostty-vt's built-in palette) with
    /// `#dcdcdc` on `#1e1e1e`: haetty's colours before themes, the base every
    /// theme file starts from. Equal to the built-in "haetty classic".
    pub fn base() -> Theme {
        let p = libghostty_vt::style::Palette::default();
        Theme {
            palette: std::array::from_fn(|i| p.0[i].into()),
            background: Rgb { r: 0x1e, g: 0x1e, b: 0x1e },
            foreground: Rgb { r: 0xdc, g: 0xdc, b: 0xdc },
            cursor_color: None,
            cursor_text: None,
            selection_background: None,
            selection_foreground: None,
        }
    }

    /// A theme file's text over [`Theme::base`]; one message per skipped line.
    pub fn parse(text: &str) -> (Theme, Vec<String>) {
        let mut t = Theme::base();
        let mut bad = vec![];
        for (n, kv) in lines(text) {
            if let Err(e) = kv.and_then(|(k, v)| t.set(k, v)) {
                bad.push(format!("line {n}: {e}"));
            }
        }
        (t, bad)
    }

    /// Sets one theme key (see [`KEYS`]).
    pub fn set(&mut self, key: &str, value: &str) -> Result<(), String> {
        match key {
            "palette" => {
                let (i, c) = libghostty_vt::style::Palette::parse_palette_entry(value)
                    .map_err(|_| format!("invalid palette entry {value:?} (N=COLOR)"))?;
                let slot = self
                    .palette
                    .get_mut(i.0 as usize)
                    .ok_or_else(|| format!("palette {}: only colours 0–15 are themed (16–255 stay xterm's)", i.0))?;
                *slot = c.into();
            }
            "background" => self.background = parse_color(value)?,
            "foreground" => self.foreground = parse_color(value)?,
            "cursor-color" => self.cursor_color = Some(parse_color(value)?),
            "cursor-text" => self.cursor_text = Some(parse_color(value)?),
            "selection-background" => self.selection_background = Some(parse_color(value)?),
            "selection-foreground" => self.selection_foreground = Some(parse_color(value)?),
            _ => return Err(format!("{key:?} is not a theme setting")),
        }
        Ok(())
    }

    /// The colour of the cursor itself.
    pub fn cursor(&self) -> Rgb {
        self.cursor_color.unwrap_or(self.foreground)
    }
}

/// Where `theme = VALUE` comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// A file: a value with a `/` or `\` (relative to the config directory).
    File(PathBuf),
    /// `<themes dir>/NAME` (user themes win over built-ins of the same name).
    User(PathBuf),
    Builtin(&'static str),
}

/// Finds `value`: a path when it contains a separator, else a file of that
/// name in `themes_dir`, else a built-in theme.
pub fn find(value: &str, config_dir: Option<&Path>, themes_dir: Option<&Path>) -> Result<Source, String> {
    if value.is_empty() {
        return Err("empty theme name".into());
    }
    if value.contains(['/', '\\']) {
        let p = Path::new(value);
        let p = match config_dir {
            Some(d) if p.is_relative() => d.join(p),
            _ => p.to_path_buf(),
        };
        return Ok(Source::File(p));
    }
    if let Some(d) = themes_dir {
        let p = d.join(value);
        if p.is_file() {
            return Ok(Source::User(p));
        }
    }
    Theme::builtin_names().find(|n| *n == value).map(Source::Builtin).ok_or_else(|| {
        let names: Vec<&str> = Theme::builtin_names().collect();
        format!("no theme {value:?} (built in: {})", names.join(", "))
    })
}

/// Loads `value` (see [`find`]): the theme and one message per skipped line.
pub fn load(value: &str, config_dir: Option<&Path>, themes_dir: Option<&Path>) -> Result<(Theme, Vec<String>), String> {
    match find(value, config_dir, themes_dir)? {
        Source::Builtin(n) => Ok((Theme::builtin(n).expect("listed"), vec![])),
        Source::File(p) | Source::User(p) => {
            let len = std::fs::metadata(&p).map_err(|e| format!("theme {}: {e}", p.display()))?.len();
            if len > MAX_FILE {
                return Err(format!("theme {}: {len} bytes, more than a theme file has", p.display()));
            }
            let text = std::fs::read_to_string(&p).map_err(|e| format!("theme {}: {e}", p.display()))?;
            let (t, bad) = Theme::parse(&text);
            Ok((t, bad.into_iter().map(|b| format!("theme {}: {b}", p.display())).collect()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb(v: u32) -> Rgb {
        Rgb { r: (v >> 16) as u8, g: (v >> 8) as u8, b: v as u8 }
    }

    #[test]
    fn builtin_themes_and_the_selection_fill_in() {
        let cga = Theme::builtin("IBM 5153 CGA").unwrap();
        assert_eq!(Theme::default(), cga, "the default theme");
        assert_eq!((cga.background, cga.foreground), (rgb(0x141414), rgb(0xcecece)));
        let want = [
            0x141414, 0xd03333, 0x1bd01b, 0xd08c1b, 0x1b1bd0, 0xd01bd0, 0x1bd0d0, 0xcecece, 0x4e4e4e, 0xdc4e4e,
            0x4edc4e, 0xf3f34e, 0x4e4edc, 0xf34ef3, 0x4ef3f3, 0xffffff,
        ];
        assert_eq!(cga.palette, want.map(rgb));
        assert_eq!((cga.cursor_color, cga.cursor_text), (Some(rgb(0xcecece)), Some(rgb(0x141414))));
        // The file says #cecece (= foreground): filled in; its foreground kept.
        assert!(BUILTIN[0].1.contains("selection-background = #cecece"));
        assert_eq!(cga.selection_background, Some(SELECTION_FILL));
        assert_eq!(cga.selection_foreground, Some(rgb(0x141414)));
        let black = Theme::builtin("IBM 5153 CGA (Black)").unwrap();
        assert_eq!((black.background, black.foreground, black.palette[3]), (rgb(0), rgb(0xc4c4c4), rgb(0xc47e00)));
        assert_eq!(black.selection_background, Some(SELECTION_FILL));
        // The same file read as a user theme is used as written.
        let (as_file, bad) = Theme::parse(BUILTIN[0].1);
        assert!(bad.is_empty());
        assert_eq!(as_file.selection_background, Some(rgb(0xcecece)));
        // haetty classic = the colours before themes (data, not memory).
        let classic = Theme::builtin(CLASSIC).unwrap();
        assert_eq!(classic, Theme::base());
        assert_eq!((classic.palette[1], classic.palette[15]), (rgb(0xcc6666), rgb(0xeaeaea)));
        assert_eq!((classic.cursor_color, classic.selection_background), (None, None));
        assert_eq!(classic.cursor(), rgb(0xdcdcdc));
    }

    #[test]
    fn theme_files_parse_like_ghostty() {
        let (t, bad) = Theme::parse(
            "# comment\n\npalette = 1=#102030\npalette=0x0f=ff0000\nbackground = \"#000\"\nforeground = white\n\
             palette = 16=#ffffff\ncursor-style = bar\nforeground = #12\npalette 2\n",
        );
        assert_eq!(t.palette[1], rgb(0x102030));
        assert_eq!(t.palette[15], rgb(0xff0000), "hex index, colour without #");
        assert_eq!(t.background, rgb(0));
        assert_eq!(t.foreground, rgb(0xffffff), "X11 name");
        assert_eq!(t.palette[2], Theme::base().palette[2], "unset keys keep the base");
        assert_eq!(bad.len(), 4, "{bad:?}");
        assert!(bad[0].starts_with("line 7:") && bad[0].contains("16–255"));
        assert!(bad[1].starts_with("line 8:") && bad[1].contains("not a theme setting"));
        assert!(bad[2].starts_with("line 9:") && bad[3].starts_with("line 10:"));
    }

    #[test]
    fn find_prefers_paths_then_user_themes_then_builtins() {
        let cfg = Path::new("/cfg");
        assert_eq!(find("IBM 5153 CGA", Some(cfg), None), Ok(Source::Builtin("IBM 5153 CGA")));
        assert_eq!(find("themes/x", Some(cfg), None), Ok(Source::File(cfg.join("themes/x"))));
        assert_eq!(find("/abs/x", Some(cfg), None), Ok(Source::File("/abs/x".into())));
        assert!(find("nope", None, None).unwrap_err().contains("IBM 5153 CGA (Black)"));
        assert!(find("", None, None).is_err());
    }
}
