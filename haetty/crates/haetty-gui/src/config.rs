//! Settings. Each key can be given on the command line as `--KEY VALUE` or
//! `--KEY=VALUE`, or in the config file (Linux `$XDG_CONFIG_HOME/haetty/config`,
//! Windows `%APPDATA%\haetty\config`) as `key = value` lines like Ghostty's,
//! `#` comments; the command line wins. `keybind = CHORD=ACTION` (file only)
//! changes shortcuts (`crate::keymap`). Bad lines are skipped and logged.
//!
//! The keys, their types, ranges, defaults and texts are [`SETTINGS`] — the one
//! place they are described: the parser checks values with [`Setting::check`],
//! the settings panel (`settings.rs`) lists them, and `haetty --help` and
//! README.md's table are generated from it (a unit test fails when README.md
//! differs).
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
    /// `Native` where the system can colour its title bar to the theme,
    /// `Integrated` where it cannot ([`Decorations::effective`]).
    Auto,
    /// The platform's title bar and frame (coloured to match on Windows 11).
    Native,
    /// haetty's own title row (`titlebar.rs`): the tab bar row with the
    /// window buttons; Linux only for now (Windows: `Native`).
    Integrated,
    /// No title bar or frame.
    None,
}

/// The window system a window runs on, for [`Decorations::effective`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowSystem {
    Windows,
    X11,
    Wayland,
}

impl Decorations {
    /// What the setting means on `ws` (DESIGN.md §3.12). `Auto`: native where
    /// the title bar takes the theme's colour — Windows 11 (DWM caption and
    /// border colours) and X11 (the window manager draws it; GNOME's mutter
    /// follows `_GTK_THEME_VARIANT=dark`) — integrated on Wayland, where winit
    /// draws its own Adwaita frame with fixed colours. `Integrated` is not
    /// implemented on Windows yet (snap layouts): native there.
    pub fn effective(self, ws: WindowSystem) -> Decorations {
        match (self, ws) {
            (Decorations::Auto, WindowSystem::Wayland) => Decorations::Integrated,
            (Decorations::Auto, _) => Decorations::Native,
            (Decorations::Integrated, WindowSystem::Windows) => Decorations::Native,
            (d, _) => d,
        }
    }
}

/// The settings button (`settings-button`, DESIGN.md §2.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsButton {
    /// Claude Code tabs: always, in the status row's last column; other tabs: drawn over the last cell only while the
    /// pointer is near the bottom-right corner.
    Auto,
    /// As `Auto` in Claude Code tabs; other tabs draw it too whenever the pointer is in the window.
    Always,
    /// No button (the shortcut and the palette entry stay).
    Never,
}

/// The diff overlay's layout (`diff-view`, DESIGN.md §13.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffViewSetting {
    /// Side by side from `diff-split-cols` columns on, else stacked.
    Auto,
    Split,
    Stacked,
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
    /// `restore-claude = resume`: a restored shell tab resumes the conversation that ran in it (stage PH, H1).
    pub restore_claude: bool,
    /// The sidebar (stage PH, H2): `sidebar = auto`, its width, the window width it docks from, the sections shown.
    pub sidebar_auto: bool,
    pub sidebar_width: u16,
    pub sidebar_min_cols: u16,
    pub sidebar_agents: bool,
    pub sidebar_files: bool,
    pub sidebar_workers: bool,
    pub sidebar_log: bool,
    /// New tabs start observable (DESIGN.md §6.1).
    pub observe_new_tabs: bool,
    /// The status panel shows the tokens of the tab and of each worker (`token-status`, DESIGN.md §8.3).
    pub token_status: bool,
    /// The share of a worker's context (percent of `context_window`) past which its tab shows a dim `/clear`; 0 is off.
    pub clear_hint: u8,
    /// Saving mode (`saving-mode`): Claude Code sessions start with the output limits of `SAVING_ENV`.
    pub saving_mode: bool,
    /// The context window in tokens, for `clear_hint`.
    pub context_window: u64,
    /// Claude Code tabs get haetty's hooks and status line.
    pub claude_integration: bool,
    /// The orchestrator's worker command (DESIGN.md §8.1).
    pub worker_command: String,
    pub notifications: bool,
    /// `theme =`: a name or a path ([`theme::find`]).
    pub theme: String,
    /// Colour keys (`palette`, `background`, …) in the order given, applied
    /// after the theme.
    pub colors: Vec<(String, String)>,
    /// 1–21 (Ghostty's `minimum-contrast`; 1 = off).
    pub minimum_contrast: f32,
    /// The terminal engine of new tabs (DESIGN.md §10.4).
    pub engine: haetty_core::vt::EngineKind,
    /// `font-family` (`--font`): `None` is the platform's defaults (`font.rs`).
    pub font_family: Option<String>,
    /// `font-size`, points (4–96).
    pub font_size: f64,
    pub settings_button: SettingsButton,
    /// Stage PU (DESIGN.md §3.17): look for a newer release once a day.
    pub update_check: bool,
    /// Where releases are asked for; empty: the GitHub repository.
    pub update_url: String,
    /// Older versions kept beside the active one and the rollback target.
    pub update_keep: u8,
    /// The diff overlay's layout (`diff-view`).
    pub diff_view: DiffViewSetting,
    /// The width from which `diff-view = auto` shows the diff side by side (`diff-split-cols`).
    pub diff_split_cols: usize,
    /// Open the diff of a file as soon as Claude Code edits it (`diff-after-edit`).
    pub diff_after_edit: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            window_padding_x: 10.0,
            window_padding_y: 6.0,
            padding_balance: true,
            padding_color: PaddingColor::Extend,
            decorations: Decorations::Auto,
            scrollback_limit: haetty_core::DEFAULT_SCROLLBACK_LIMIT,
            restore_tabs: true,
            restore_claude: true,
            sidebar_auto: true,
            sidebar_width: 42,
            sidebar_min_cols: 121,
            sidebar_agents: true,
            sidebar_files: true,
            sidebar_workers: true,
            sidebar_log: true,
            observe_new_tabs: false,
            token_status: false,
            clear_hint: 60,
            saving_mode: false,
            context_window: 200_000,
            claude_integration: true,
            engine: haetty_core::vt::EngineKind::default_kind(),
            worker_command: "claude".into(),
            notifications: true,
            theme: theme::DEFAULT_THEME.to_string(),
            colors: vec![],
            minimum_contrast: 1.0,
            font_family: None,
            font_size: 11.0,
            settings_button: SettingsButton::Auto,
            update_check: true,
            update_url: String::new(),
            update_keep: 2,
            diff_view: DiffViewSetting::Auto,
            diff_split_cols: 100,
            diff_after_edit: false,
        }
    }
}

/// The settings panel's groups, in their order (DESIGN.md §2.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Appearance,
    Font,
    Window,
    Terminal,
    Claude,
    /// Stage PH: the sidebar (DESIGN.md §13.3).
    Sidebar,
    Advanced,
    /// Stage PU: first when a newer version is out, else last (`SettingsPanel::new`).
    Updates,
}

impl Group {
    pub const ALL: [Group; 8] = [
        Group::Appearance,
        Group::Font,
        Group::Window,
        Group::Terminal,
        Group::Claude,
        Group::Sidebar,
        Group::Advanced,
        Group::Updates,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Group::Appearance => "Appearance",
            Group::Font => "Font",
            Group::Window => "Window",
            Group::Terminal => "Terminal",
            Group::Claude => "Claude Code",
            Group::Sidebar => "Sidebar",
            Group::Advanced => "Advanced",
            Group::Updates => "Updates",
        }
    }
}

/// A setting's value type, its range and how the panel edits it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    /// `true/yes/1`, `false/no/0`.
    Bool,
    Choice(&'static [&'static str]),
    /// A number in `min..=max`; outside it the file's value is `clamp`ed (as Ghostty does) or rejected. `int`: whole
    /// numbers only; `_` separators allowed (`scrollback-limit`). `step`: the panel's ←/→.
    Number {
        min: f64,
        max: f64,
        step: f64,
        int: bool,
        clamp: bool,
    },
    /// Free text (non-empty unless `empty_ok`).
    Text {
        empty_ok: bool,
    },
    /// A font family the system has (picked from its monospaced ones); empty = the platform's.
    Font,
    /// A theme name (built in or in the themes folder) or a path.
    Theme,
    /// A colour (`#rrggbb`, `rrggbb`, an X11 name); empty = the theme's.
    Color,
    /// May repeat (`palette`, `keybind`): file only, not in the panel.
    Repeat,
}

/// When a changed value takes effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum When {
    /// At once, in every open tab.
    Live,
    /// Read when the window is made: the next start.
    NextStart,
}

/// One setting: the one place its key, type, range, default and text are written. The parser checks values with
/// [`Setting::check`], the settings panel lists them, `haetty --help` and README.md's table are generated from them.
pub struct Setting {
    pub key: &'static str,
    pub kind: Kind,
    /// The default as the file writes it; "" when it comes from elsewhere (the theme, the platform).
    pub default: &'static str,
    /// README's "Default" text when it is not just `default` in backticks.
    pub default_note: Option<&'static str>,
    /// README's and `--help`'s "Values" text (Markdown).
    pub doc: &'static str,
    /// The panel's one-line description.
    pub line: &'static str,
    pub group: Group,
    pub when: When,
    /// Shown in the settings panel. Not: keys that may repeat, the `window-padding` shorthand, and keys that change
    /// what Claude Code does (`saving-mode`, `worker-command`).
    pub panel: bool,
    /// `--KEY` on the command line too.
    pub cli: bool,
}

const BOOL: Kind = Kind::Bool;
/// No upper limit (`u64`).
const U64_MAX: f64 = u64::MAX as f64;
const fn num(min: f64, max: f64, step: f64, int: bool, clamp: bool) -> Kind {
    Kind::Number { min, max, step, int, clamp }
}

const PADDING_DOC: &str = "the same, left/right or top/bottom only";
const COLOR_DOC: &str = "a colour (`#rrggbb`, `rrggbb`, an X11 name) — on top of the theme";

/// Every setting, in the panel's order within each group.
pub const SETTINGS: &[Setting] = &[
    Setting {
        key: "window-padding",
        kind: num(0.0, 200.0, 1.0, false, false),
        default: "",
        default_note: Some("—"),
        doc: "minimum padding around the grid on all sides, logical px (0–200)",
        line: "Padding on all sides (sets both below).",
        group: Group::Window,
        when: When::Live,
        panel: false,
        cli: true,
    },
    Setting {
        key: "window-padding-x",
        kind: num(0.0, 200.0, 1.0, false, false),
        default: "10",
        default_note: None,
        doc: PADDING_DOC,
        line: "Minimum padding left and right of the grid, logical pixels (0–200).",
        group: Group::Window,
        when: When::Live,
        panel: true,
        cli: true,
    },
    Setting {
        key: "window-padding-y",
        kind: num(0.0, 200.0, 1.0, false, false),
        default: "6",
        default_note: None,
        doc: PADDING_DOC,
        line: "Minimum padding above and below the grid, logical pixels (0–200).",
        group: Group::Window,
        when: When::Live,
        panel: true,
        cli: true,
    },
    Setting {
        key: "window-padding-balance",
        kind: BOOL,
        default: "true",
        default_note: None,
        doc: "`true` / `false` — spread leftover space evenly on all sides",
        line: "Spread the space left over around the grid evenly on all sides.",
        group: Group::Window,
        when: When::Live,
        panel: true,
        cli: true,
    },
    Setting {
        key: "window-padding-color",
        kind: Kind::Choice(&["background", "extend", "extend-always"]),
        default: "extend",
        default_note: None,
        doc: "`background` / `extend` / `extend-always` (as in Ghostty)",
        line: "Colour of the padding: the background, or the nearest cell's (extend).",
        group: Group::Window,
        when: When::Live,
        panel: true,
        cli: true,
    },
    Setting {
        key: "decorations",
        kind: Kind::Choice(&["auto", "native", "integrated", "none"]),
        default: "auto",
        default_note: None,
        doc: "`auto` / `native` / `integrated` / `none` — auto: the system's title bar where it takes the theme's colour (Windows 11, X11), haetty's own (integrated) on Wayland; integrated is Linux only (Windows: native)",
        line: "Title bar: the system's, haetty's own row (integrated), or none.",
        group: Group::Window,
        when: When::NextStart,
        panel: true,
        cli: true,
    },
    Setting {
        key: "settings-button",
        kind: Kind::Choice(&["auto", "always", "never"]),
        default: "auto",
        default_note: None,
        doc: "`auto` / `always` / `never` — the dim ⚙ that opens the settings: auto: always in a Claude Code tab's status row, in other tabs only while the pointer is near the bottom-right corner; always: in other tabs whenever the pointer is in the window; never: no button (Ctrl+, and the palette's \"Settings\" stay)",
        line: "The ⚙ button: auto (Claude Code tabs; others near the corner), always, never.",
        group: Group::Window,
        when: When::Live,
        panel: true,
        cli: true,
    },
    Setting {
        key: "scrollback-limit",
        kind: num(0.0, U64_MAX, 1_000_000.0, true, false),
        default: "12000000",
        default_note: None,
        doc: "bytes of scrollback per session, as in Ghostty (plain bytes, `_` allowed; rows of 120 columns take ~1.1 KB, the default keeps ~11 000 of them)",
        line: "Bytes of scrollback per tab (12000000 keeps about 11 000 rows).",
        group: Group::Terminal,
        when: When::NextStart,
        panel: true,
        cli: true,
    },
    Setting {
        key: "restore-tabs",
        kind: BOOL,
        default: "true",
        default_note: None,
        doc: "`true` / `false` — reopen the previous run's tabs at start",
        line: "Reopen the previous run's tabs at start.",
        group: Group::Terminal,
        when: When::NextStart,
        panel: true,
        cli: true,
    },
    Setting {
        key: "sidebar",
        kind: Kind::Choice(&["auto", "hide"]),
        default: "auto",
        default_note: None,
        doc: "`auto` / `hide` — auto: the sidebar on the right from `sidebar-min-cols` columns on, for every tab (Ctrl+Shift+S hides or shows it; a narrower window shows it over the terminal); hide: only over the terminal, on Ctrl+Shift+S",
        line: "The sidebar on the right when the window is wide enough, or only on Ctrl+Shift+S.",
        group: Group::Sidebar,
        when: When::NextStart,
        panel: true,
        cli: true,
    },
    Setting {
        key: "sidebar-width",
        kind: num(24.0, 80.0, 2.0, true, true),
        default: "42",
        default_note: None,
        doc: "columns, the line included (24–80); the palette's \"Sidebar wider\" / \"Sidebar narrower\" change it for the window",
        line: "The sidebar's width in columns.",
        group: Group::Sidebar,
        when: When::NextStart,
        panel: true,
        cli: true,
    },
    Setting {
        key: "sidebar-min-cols",
        kind: num(60.0, 1000.0, 1.0, true, true),
        default: "121",
        default_note: None,
        doc: "the window's width in columns from which the sidebar stays on the right (narrower: over the terminal, on Ctrl+Shift+S)",
        line: "From this many window columns the sidebar stays on the right.",
        group: Group::Sidebar,
        when: When::NextStart,
        panel: true,
        cli: true,
    },
    Setting {
        key: "sidebar-agents",
        kind: Kind::Bool,
        default: "true",
        default_note: None,
        doc: "`true` / `false` — the Agents section (subagents of the tab's Claude Code)",
        line: "Show the Agents section.",
        group: Group::Sidebar,
        when: When::Live,
        panel: true,
        cli: true,
    },
    Setting {
        key: "sidebar-files",
        kind: Kind::Bool,
        default: "true",
        default_note: None,
        doc: "`true` / `false` — the Files section (files changed in the conversation; a shell tab: git changes)",
        line: "Show the Files section.",
        group: Group::Sidebar,
        when: When::Live,
        panel: true,
        cli: true,
    },
    Setting {
        key: "sidebar-workers",
        kind: Kind::Bool,
        default: "true",
        default_note: None,
        doc: "`true` / `false` — the Workers section (orchestrator tasks)",
        line: "Show the Workers section.",
        group: Group::Sidebar,
        when: When::Live,
        panel: true,
        cli: true,
    },
    Setting {
        key: "sidebar-log",
        kind: Kind::Bool,
        default: "true",
        default_note: None,
        doc: "`true` / `false` — the Log section (the tab's recent events)",
        line: "Show the Log section.",
        group: Group::Sidebar,
        when: When::Live,
        panel: true,
        cli: true,
    },
    Setting {
        key: "diff-view",
        kind: Kind::Choice(&["auto", "split", "stacked"]),
        default: "auto",
        default_note: None,
        doc: "`auto` / `split` / `stacked` — the diff overlay's layout: auto: side by side from `diff-split-cols` columns on, else stacked; split: side by side; stacked: one column",
        line: "The diff's layout: side by side or one column (auto: by the window's width).",
        group: Group::Sidebar,
        when: When::Live,
        panel: true,
        cli: true,
    },
    Setting {
        key: "diff-split-cols",
        kind: num(40.0, 400.0, 1.0, true, true),
        default: "100",
        default_note: None,
        doc: "columns (40–400): the width from which `diff-view = auto` shows the diff side by side",
        line: "From this many columns the diff shows side by side (auto).",
        group: Group::Sidebar,
        when: When::Live,
        panel: true,
        cli: true,
    },
    Setting {
        key: "diff-after-edit",
        kind: Kind::Bool,
        default: "false",
        default_note: None,
        doc: "`true` / `false` — an edit by Claude Code in the active tab opens the diff overlay on that file",
        line: "Open the diff of a file as soon as Claude Code edits it.",
        group: Group::Sidebar,
        when: When::Live,
        panel: true,
        cli: true,
    },
    Setting {
        key: "restore-claude",
        kind: Kind::Choice(&["resume", "shell"]),
        default: "resume",
        default_note: None,
        doc: "`resume` / `shell` — a shell tab saved while `claude` ran in it: reopen the shell and run `claude --resume ID` there, or the shell only",
        line: "A shell tab that ran claude: resume the conversation in it, or open the shell only.",
        group: Group::Claude,
        when: When::NextStart,
        panel: true,
        cli: true,
    },
    Setting {
        key: "engine",
        kind: Kind::Choice(&["ghostty", "native"]),
        default: "ghostty",
        default_note: None,
        doc: "`ghostty` / `native` — the terminal engine of new tabs: libghostty-vt, or haetty's own (DESIGN.md §10; a build without libghostty always runs native)",
        line: "Terminal engine of new tabs: libghostty-vt or haetty's own (native).",
        group: Group::Advanced,
        when: When::NextStart,
        panel: true,
        cli: true,
    },
    Setting {
        key: "claude-integration",
        kind: BOOL,
        default: "true",
        default_note: None,
        doc: "`true` / `false` — hooks and status line in Claude Code tabs",
        line: "haetty's observing hooks and status line in Claude Code tabs.",
        group: Group::Claude,
        when: When::NextStart,
        panel: true,
        cli: true,
    },
    Setting {
        key: "worker-command",
        kind: Kind::Text { empty_ok: false },
        default: "claude",
        default_note: None,
        doc: "the orchestrator's worker: a program and its arguments, space-separated (DESIGN.md §8.1)",
        line: "The orchestrator's worker program and arguments.",
        group: Group::Claude,
        when: When::NextStart,
        panel: false,
        cli: true,
    },
    Setting {
        key: "notifications",
        kind: BOOL,
        default: "true",
        default_note: None,
        doc: "`true` / `false` — desktop notification (Linux `notify-send`) when Claude Code waits and the window is unfocused",
        line: "A desktop notification when Claude Code waits and the window is not focused.",
        group: Group::Terminal,
        when: When::Live,
        panel: true,
        cli: true,
    },
    Setting {
        key: "observe-new-tabs",
        kind: BOOL,
        default: "false",
        default_note: None,
        doc: "`true` / `false` — new tabs start observable by `haetty screen` / `haetty events` ([Observing a tab](#observing-a-tab-read-only))",
        line: "New tabs start readable by haetty screen and haetty events.",
        group: Group::Advanced,
        when: When::NextStart,
        panel: true,
        cli: true,
    },
    Setting {
        key: "token-status",
        kind: BOOL,
        default: "false",
        default_note: None,
        doc: "`true` / `false` — the sidebar shows the tokens of the active tab and of each worker, read from the Claude Code transcripts ([`haetty tokens`](docs/tokens/README.md))",
        line: "The status panel shows the tokens of the tab and of each worker.",
        group: Group::Claude,
        when: When::Live,
        panel: true,
        cli: true,
    },
    Setting {
        key: "clear-hint",
        kind: num(0.0, 100.0, 5.0, true, false),
        default: "60",
        default_note: None,
        doc: "a percentage, 0 for off: a worker tab shows a dim `/clear` once its context has passed that share of `context-window`. A hint only: nothing is sent ([Orchestrator](#orchestrator-tasks-and-workers))",
        line: "A worker tab shows a dim /clear past this share of its context (0 = off).",
        group: Group::Claude,
        when: When::Live,
        panel: true,
        cli: true,
    },
    Setting {
        key: "context-window",
        kind: num(1000.0, U64_MAX, 1000.0, true, false),
        default: "200000",
        default_note: None,
        doc: "the context window of the workers' model, in tokens, for `clear-hint` (at least 1000)",
        line: "The workers' context window in tokens, for the /clear hint.",
        group: Group::Claude,
        when: When::Live,
        panel: true,
        cli: true,
    },
    Setting {
        key: "saving-mode",
        kind: BOOL,
        default: "false",
        default_note: None,
        doc: "`true` / `false` — saving mode: Claude Code sessions haetty starts get `BASH_MAX_OUTPUT_LENGTH=12000` and `MAX_MCP_OUTPUT_TOKENS=12000`, the official limits on Bash output read back and on MCP tool output ([the values and sources](docs/tokens/README.md#saving-mode)); the status row shows a dim `◆ saving`",
        line: "Output limits for the Claude Code sessions haetty starts.",
        group: Group::Claude,
        when: When::NextStart,
        panel: false,
        cli: true,
    },
    Setting {
        key: "keybind",
        kind: Kind::Repeat,
        default: "",
        default_note: Some("see [Tabs and keys](#tabs-and-keys)"),
        doc: "`CHORD=ACTION` or `CHORD=unbind` (file only, may repeat)",
        line: "A shortcut.",
        group: Group::Advanced,
        when: When::NextStart,
        panel: false,
        cli: false,
    },
    Setting {
        key: "theme",
        kind: Kind::Theme,
        default: theme::DEFAULT_THEME,
        default_note: None,
        doc: "a built-in theme, a theme file's name in the themes folder, or a path (see [Themes](#themes))",
        line: "The colour theme: built in, a file in the themes folder, or a path.",
        group: Group::Appearance,
        when: When::Live,
        panel: true,
        cli: true,
    },
    Setting {
        key: "palette",
        kind: Kind::Repeat,
        default: "",
        default_note: Some("the theme's"),
        doc: "`N=COLOR`, N 0–15 (may repeat) — on top of the theme",
        line: "One of the 16 colours.",
        group: Group::Appearance,
        when: When::Live,
        panel: false,
        cli: true,
    },
    Setting {
        key: "minimum-contrast",
        kind: num(1.0, 21.0, 0.5, false, true),
        default: "1",
        default_note: Some("`1` (off)"),
        doc: "1–21, as in Ghostty: text whose WCAG contrast with its background is lower turns white (dark background) or black (light)",
        line: "Text below this contrast with its background turns white or black (1 = off).",
        group: Group::Appearance,
        when: When::Live,
        panel: true,
        cli: true,
    },
    color("background", "The background colour (empty: the theme's)."),
    color("foreground", "The text colour (empty: the theme's)."),
    color("cursor-color", "The cursor's colour (empty: the theme's)."),
    color("cursor-text", "The colour of text under the cursor (empty: the theme's)."),
    color("selection-background", "The selection's background (empty: the theme's)."),
    color("selection-foreground", "The selection's text colour (empty: the theme's)."),
    Setting {
        key: "font-family",
        kind: Kind::Font,
        default: "",
        default_note: Some("a monospaced font of the system (Linux: DejaVu Sans Mono, Noto Sans Mono, …; Windows: Cascadia Mono, Consolas, …)"),
        doc: "a font family installed on the system (`--font` on the command line); a missing one falls back to the default",
        line: "The font family (empty: the platform's monospaced default).",
        group: Group::Font,
        when: When::Live,
        panel: true,
        cli: true,
    },
    Setting {
        key: "font-size",
        kind: num(4.0, 96.0, 1.0, false, false),
        default: "11",
        default_note: None,
        doc: "points, 4–96",
        line: "Font size in points (4–96).",
        group: Group::Font,
        when: When::Live,
        panel: true,
        cli: true,
    },
    Setting {
        key: "update-check",
        kind: BOOL,
        default: "true",
        default_note: None,
        doc: "`true` / `false` — ask the release server for a newer version 30 s after the window opens and then once a day; nothing else is sent ([Updates](#updates))",
        line: "Look for a newer haetty once a day (one request, nothing about you sent).",
        group: Group::Updates,
        when: When::Live,
        panel: true,
        cli: true,
    },
    Setting {
        key: "update-channel",
        kind: Kind::Choice(&["stable"]),
        default: "stable",
        default_note: None,
        doc: "`stable` (the only channel for now)",
        line: "The release channel.",
        group: Group::Updates,
        when: When::Live,
        panel: false,
        cli: true,
    },
    Setting {
        key: "update-url",
        kind: Kind::Text { empty_ok: true },
        default: "",
        default_note: Some("the GitHub repository's latest release"),
        doc: "an `https://` folder holding `manifest.json`, its `.minisig` and the assets (your own server, a test); empty: the GitHub repository",
        line: "Where releases are looked for.",
        group: Group::Updates,
        when: When::Live,
        panel: false,
        cli: true,
    },
    Setting {
        key: "update-keep",
        kind: num(1.0, 10.0, 1.0, true, false),
        default: "2",
        default_note: None,
        doc: "how many older versions an update keeps besides the active one and the one to roll back to (1–10)",
        line: "Older versions kept after an update (1–10).",
        group: Group::Updates,
        when: When::Live,
        panel: true,
        cli: true,
    },
];

const fn color(key: &'static str, line: &'static str) -> Setting {
    Setting {
        key,
        kind: Kind::Color,
        default: "",
        default_note: Some("the theme's"),
        doc: COLOR_DOC,
        line,
        group: Group::Appearance,
        when: When::Live,
        panel: true,
        cli: true,
    }
}

/// A value checked against its setting's kind.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Bool(bool),
    Choice(&'static str),
    Number(f64),
    /// A whole number (`int` kinds), exact.
    Int(u64),
    Text(String),
}

impl Setting {
    pub fn find(key: &str) -> Option<&'static Setting> {
        SETTINGS.iter().find(|s| s.key == key)
    }

    /// The value checked against the kind and range: the one place they are applied. A clamping number outside its
    /// range is clamped (the file's rule); [`Setting::check_strict`] rejects it (the panel's).
    pub fn check(&self, value: &str) -> Result<Value, String> {
        let bad = || format!("invalid value {value:?} for {}", self.key);
        Ok(match self.kind {
            Kind::Bool => Value::Bool(parse_bool(value).ok_or_else(bad)?),
            Kind::Choice(list) => Value::Choice(list.iter().copied().find(|c| *c == value).ok_or_else(bad)?),
            Kind::Number { min, max, int: true, .. } => {
                let n: u64 = self.number_text(value).parse().map_err(|_| bad())?;
                if (n as f64) < min || (n as f64) > max {
                    return Err(bad());
                }
                Value::Int(n)
            }
            Kind::Number { min, max, clamp, .. } => {
                let v: f64 = self.number_text(value).parse().map_err(|_| bad())?;
                if !v.is_finite() || (!clamp && (v < min || v > max)) {
                    return Err(bad());
                }
                Value::Number(v.clamp(min, max))
            }
            Kind::Text { empty_ok } => {
                if !empty_ok && value.trim().is_empty() {
                    return Err(bad());
                }
                Value::Text(value.trim().to_string())
            }
            Kind::Font => Value::Text(value.trim().to_string()),
            Kind::Theme => {
                if value.is_empty() {
                    return Err(bad());
                }
                Value::Text(value.to_string())
            }
            Kind::Color | Kind::Repeat => Value::Text(value.to_string()),
        })
    }

    /// A number's text as parsed: trimmed; `_` separators dropped where allowed (`scrollback-limit`, as Ghostty).
    fn number_text(&self, value: &str) -> String {
        let t = value.trim();
        if self.key == "scrollback-limit" {
            t.replace('_', "")
        } else {
            t.to_string()
        }
    }

    /// [`Setting::check`] without clamping, and with the range in the message: the panel's input.
    pub fn check_strict(&self, value: &str) -> Result<Value, String> {
        if let Kind::Number { min, max, .. } = self.kind {
            match self.number_text(value).parse::<f64>() {
                Ok(raw) if raw.is_finite() && (min..=max).contains(&raw) => {}
                _ => return Err(self.range_text()),
            }
            return self.check(value).map_err(|_| self.range_text());
        }
        if self.kind == Kind::Color && !value.is_empty() {
            Theme::base().set(self.key, value)?;
        }
        self.check(value)
    }

    /// "between 0 and 200", "a whole number from 1000"…
    pub fn range_text(&self) -> String {
        match self.kind {
            Kind::Number { min, max, int, .. } if max >= U64_MAX => {
                format!("{} from {}", if int { "a whole number" } else { "a number" }, fmt_num(min))
            }
            Kind::Number { min, max, int, .. } => {
                format!("{} from {} to {}", if int { "a whole number" } else { "a number" }, fmt_num(min), fmt_num(max))
            }
            _ => String::new(),
        }
    }

    /// README's "Default" cell.
    pub fn shown_default(&self) -> String {
        match self.default_note {
            Some(n) => n.to_string(),
            None => format!("`{}`", self.default),
        }
    }
}

/// A number as the config file writes it: no `.0` on whole numbers.
pub fn fmt_num(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

fn fmt_f32(v: f32) -> String {
    if v.fract() == 0.0 {
        format!("{}", v as i64)
    } else {
        v.to_string()
    }
}

/// The keys the parser knows, from [`SETTINGS`].
#[cfg(test)]
pub fn keys() -> impl Iterator<Item = &'static str> {
    SETTINGS.iter().map(|s| s.key)
}

/// A key `--KEY` may set.
pub fn is_cli_key(key: &str) -> bool {
    Setting::find(key).is_some_and(|s| s.cli)
}

/// The settings as README.md's Markdown table (checked by a test): neighbours with the same text share a row.
#[cfg(test)]
pub fn markdown_table() -> String {
    let mut s = String::from("| Key | Values | Default |\n|---|---|---|\n");
    let mut i = 0;
    while i < SETTINGS.len() {
        let mut j = i + 1;
        while j < SETTINGS.len() && SETTINGS[j].doc == SETTINGS[i].doc {
            j += 1;
        }
        let rows = &SETTINGS[i..j];
        let keys: Vec<String> = rows.iter().map(|r| format!("`{}`", r.key)).collect();
        let mut defaults: Vec<String> = rows.iter().map(Setting::shown_default).collect();
        defaults.dedup();
        s.push_str(&format!("| {} | {} | {} |\n", keys.join(", "), rows[0].doc, defaults.join(", ")));
        i = j;
    }
    s
}

/// The settings for `haetty --help`: `--KEY` and the values, one per line.
pub fn help_lines() -> String {
    // No Markdown in a terminal: no backticks, links as their text.
    let plain = |t: &str| {
        let mut o = String::new();
        let mut rest = t;
        while let Some(i) = rest.find('[') {
            match rest[i..].find("](").and_then(|j| rest[i + j..].find(')').map(|k| (j, j + k))) {
                Some((j, k)) => {
                    o.push_str(&rest[..i]);
                    o.push_str(&rest[i + 1..i + j]);
                    rest = &rest[i + k + 1..];
                }
                None => break,
            }
        }
        o.push_str(rest);
        o.replace('`', "")
    };
    let mut s = String::new();
    let mut i = 0;
    while i < SETTINGS.len() {
        let mut j = i + 1;
        while j < SETTINGS.len() && SETTINGS[j].doc == SETTINGS[i].doc {
            j += 1;
        }
        let rows = &SETTINGS[i..j];
        let keys: Vec<String> = rows
            .iter()
            .map(|r| if r.cli { format!("--{}", r.key) } else { format!("{} (file only)", r.key) })
            .collect();
        let mut defaults: Vec<String> = rows.iter().map(|r| plain(&r.shown_default())).collect();
        defaults.dedup();
        s.push_str(&format!("  {}\n      {} (default {})\n", keys.join(", "), plain(rows[0].doc), defaults.join(", ")));
        i = j;
    }
    s
}

impl Config {
    pub fn set(&mut self, key: &str, value: &str) -> Result<(), String> {
        let Some(s) = Setting::find(key) else { return Err(format!("unknown setting {key:?}")) };
        if s.kind == Kind::Repeat && key == "keybind" {
            return Err(format!("unknown setting {key:?}"));
        }
        let v = s.check(value)?;
        let num = || match v {
            Value::Number(n) => n,
            Value::Int(n) => n as f64,
            _ => 0.0,
        };
        let int = || match v {
            Value::Int(n) => n,
            _ => 0,
        };
        let flag = || matches!(v, Value::Bool(true));
        let text = || match &v {
            Value::Text(t) => t.clone(),
            Value::Choice(c) => c.to_string(),
            _ => String::new(),
        };
        match key {
            "window-padding" => (self.window_padding_x, self.window_padding_y) = (num(), num()),
            "window-padding-x" => self.window_padding_x = num(),
            "window-padding-y" => self.window_padding_y = num(),
            "restore-tabs" => self.restore_tabs = flag(),
            "restore-claude" => self.restore_claude = text() == "resume",
            "sidebar" => self.sidebar_auto = text() == "auto",
            "sidebar-width" => self.sidebar_width = int() as u16,
            "sidebar-min-cols" => self.sidebar_min_cols = int() as u16,
            "sidebar-agents" => self.sidebar_agents = flag(),
            "sidebar-files" => self.sidebar_files = flag(),
            "sidebar-workers" => self.sidebar_workers = flag(),
            "sidebar-log" => self.sidebar_log = flag(),
            "diff-view" => {
                self.diff_view = match text().as_str() {
                    "split" => DiffViewSetting::Split,
                    "stacked" => DiffViewSetting::Stacked,
                    _ => DiffViewSetting::Auto,
                }
            }
            "diff-split-cols" => self.diff_split_cols = int() as usize,
            "diff-after-edit" => self.diff_after_edit = flag(),
            "observe-new-tabs" => self.observe_new_tabs = flag(),
            "token-status" => self.token_status = flag(),
            "clear-hint" => self.clear_hint = int() as u8,
            "saving-mode" => self.saving_mode = flag(),
            "context-window" => self.context_window = int(),
            "claude-integration" => self.claude_integration = flag(),
            "worker-command" => self.worker_command = text(),
            "notifications" => self.notifications = flag(),
            "window-padding-balance" => self.padding_balance = flag(),
            "window-padding-color" => {
                self.padding_color = match text().as_str() {
                    "background" => PaddingColor::Background,
                    "extend" => PaddingColor::Extend,
                    _ => PaddingColor::ExtendAlways,
                }
            }
            "decorations" => {
                self.decorations = match text().as_str() {
                    "auto" => Decorations::Auto,
                    "native" => Decorations::Native,
                    "integrated" => Decorations::Integrated,
                    _ => Decorations::None,
                }
            }
            "settings-button" => {
                self.settings_button = match text().as_str() {
                    "auto" => SettingsButton::Auto,
                    "always" => SettingsButton::Always,
                    _ => SettingsButton::Never,
                }
            }
            "engine" => {
                self.engine = haetty_core::vt::EngineKind::parse(&text())
                    .ok_or_else(|| format!("invalid value {value:?} for engine"))?
            }
            "scrollback-limit" => {
                self.scrollback_limit =
                    usize::try_from(int()).map_err(|_| format!("invalid value {value:?} for scrollback-limit"))?
            }
            "minimum-contrast" => self.minimum_contrast = num() as f32,
            "theme" => self.theme = text(),
            "font-family" => self.font_family = Some(text()).filter(|t| !t.is_empty()),
            "font-size" => self.font_size = num(),
            "update-check" => self.update_check = flag(),
            "update-channel" => {}
            "update-url" => {
                let t = text();
                if !t.is_empty() && !t.starts_with("https://") {
                    return Err(format!("invalid value {value:?} for update-url (https:// only)"));
                }
                self.update_url = t;
            }
            "update-keep" => self.update_keep = int() as u8,
            k if theme::KEYS.contains(&k) => {
                // Checked now (bad lines are reported with their number),
                // applied after the theme.
                Theme::base().set(k, value)?;
                self.colors.push((k.to_string(), value.to_string()));
            }
            _ => return Err(format!("setting {key:?} has no field (config.rs)")),
        }
        Ok(())
    }

    /// The value of `key` as the config file would write it; "" for one that comes from elsewhere (the theme's
    /// colour, the platform's font). `None` for keys that may repeat.
    pub fn value(&self, key: &str) -> Option<String> {
        let b = |v: bool| v.to_string();
        Some(match key {
            "window-padding-x" => fmt_num(self.window_padding_x),
            "window-padding-y" => fmt_num(self.window_padding_y),
            "window-padding" if self.window_padding_x == self.window_padding_y => fmt_num(self.window_padding_x),
            "window-padding" => String::new(),
            "window-padding-balance" => b(self.padding_balance),
            "window-padding-color" => match self.padding_color {
                PaddingColor::Background => "background",
                PaddingColor::Extend => "extend",
                PaddingColor::ExtendAlways => "extend-always",
            }
            .into(),
            "decorations" => match self.decorations {
                Decorations::Auto => "auto",
                Decorations::Native => "native",
                Decorations::Integrated => "integrated",
                Decorations::None => "none",
            }
            .into(),
            "settings-button" => match self.settings_button {
                SettingsButton::Auto => "auto",
                SettingsButton::Always => "always",
                SettingsButton::Never => "never",
            }
            .into(),
            "scrollback-limit" => self.scrollback_limit.to_string(),
            "restore-tabs" => b(self.restore_tabs),
            "restore-claude" => (if self.restore_claude { "resume" } else { "shell" }).into(),
            "sidebar" => (if self.sidebar_auto { "auto" } else { "hide" }).into(),
            "sidebar-width" => self.sidebar_width.to_string(),
            "sidebar-min-cols" => self.sidebar_min_cols.to_string(),
            "sidebar-agents" => b(self.sidebar_agents),
            "sidebar-files" => b(self.sidebar_files),
            "sidebar-workers" => b(self.sidebar_workers),
            "sidebar-log" => b(self.sidebar_log),
            "diff-view" => match self.diff_view {
                DiffViewSetting::Auto => "auto",
                DiffViewSetting::Split => "split",
                DiffViewSetting::Stacked => "stacked",
            }
            .into(),
            "diff-split-cols" => self.diff_split_cols.to_string(),
            "diff-after-edit" => b(self.diff_after_edit),
            "engine" => self.engine.name().into(),
            "claude-integration" => b(self.claude_integration),
            "worker-command" => self.worker_command.clone(),
            "notifications" => b(self.notifications),
            "observe-new-tabs" => b(self.observe_new_tabs),
            "token-status" => b(self.token_status),
            "clear-hint" => self.clear_hint.to_string(),
            "context-window" => self.context_window.to_string(),
            "saving-mode" => b(self.saving_mode),
            "theme" => self.theme.clone(),
            "minimum-contrast" => fmt_f32(self.minimum_contrast),
            "font-family" => self.font_family.clone().unwrap_or_default(),
            "font-size" => fmt_num(self.font_size),
            "update-check" => b(self.update_check),
            "update-channel" => "stable".into(),
            "update-url" => self.update_url.clone(),
            "update-keep" => self.update_keep.to_string(),
            k if theme::KEYS.contains(&k) && k != "palette" => {
                self.colors.iter().rev().find(|(ck, _)| ck == k).map(|(_, v)| v.clone()).unwrap_or_default()
            }
            _ => return None,
        })
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

/// Variables of a Claude Code session that mark it as a child of another one, or belong to that parent. haetty does not
/// pass them on: a tab started from inside Claude Code would turn transcript saving off ("inherited
/// CLAUDE_CODE_CHILD_SESSION marker") and could not be resumed. The names were read from a Claude Code session's
/// environment on 2026-11-09 (the values are never printed). CLAUDE_CODE_ENTRYPOINT, CLAUDE_CODE_EXECPATH and
/// CLAUDE_EFFORT are not here: they are not child markers. docs/history/2026-11-09-k2-child-markers.md has the test.
pub const CHILD_MARKERS: [&str; 8] = haetty_core::CHILD_MARKERS;

/// The environment of a new session: the inherited variables without the child markers, then the saving mode's limits
/// when `saving` is on (the caller decides that: a Claude Code program in saving mode).
pub fn child_env(inherited: Vec<(String, String)>, saving: bool) -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> =
        inherited.into_iter().filter(|(k, _)| !CHILD_MARKERS.contains(&k.as_str())).collect();
    if saving {
        saving_env(&mut env);
    }
    env
}

/// Saving mode's environment: the Bash output read back into a result, and the MCP tool output in tokens. The values
/// are from the G1 measurement (docs/tokens/README.md, "Saving mode"); the meanings are the official ones.
pub const SAVING_ENV: [(&str, &str); 2] = [("BASH_MAX_OUTPUT_LENGTH", "12000"), ("MAX_MCP_OUTPUT_TOKENS", "12000")];

/// Sets saving mode's variables in a session's environment, replacing a value the environment already has.
pub fn saving_env(env: &mut Vec<(String, String)>) {
    for (key, value) in SAVING_ENV {
        match env.iter_mut().find(|(k, _)| k == key) {
            Some(entry) => entry.1 = value.to_string(),
            None => env.push((key.to_string(), value.to_string())),
        }
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

    /// SETTINGS and the parser know the same keys; each default, set through the parser, gives `Config::default()`;
    /// `Config::value` writes back what `set` read.
    #[test]
    fn settings_table_and_parser_agree() {
        let mut seen = std::collections::HashSet::new();
        let mut c = Config::default();
        for s in SETTINGS {
            assert!(seen.insert(s.key), "{} twice", s.key);
            if s.key == "keybind" {
                continue;
            }
            assert!(
                !matches!(c.set(s.key, "\u{1}"), Err(e) if e.starts_with("unknown setting") || e.contains("no field")),
                "{}: unknown to set()",
                s.key
            );
            if s.kind == Kind::Repeat || !s.panel {
                continue;
            }
            assert!(s.line.len() <= 90 && !s.line.contains('`'), "{}: the panel's line is one plain line", s.key);
            let mut d = Config::default();
            if !s.default.is_empty() {
                let want = if s.key == "engine" { d.engine.name() } else { s.default };
                d.set(s.key, want).unwrap();
                assert_eq!(d, Config::default(), "{}: the default {:?} is not Config::default()", s.key, s.default);
            }
            assert_eq!(
                Config::default().value(s.key).as_deref(),
                Some(if s.key == "engine" { Config::default().engine.name() } else { s.default }),
                "{}: value() of the default",
                s.key
            );
        }
        // Each panel key round-trips through value() and set().
        let mut c2 = Config::default();
        for (k, v) in [
            ("window-padding-x", "12.5"),
            ("font-size", "13"),
            ("minimum-contrast", "4.5"),
            ("theme", "haetty classic"),
            ("background", "#102030"),
            ("font-family", "Noto Sans Mono"),
            ("scrollback-limit", "4_000_000"),
            ("settings-button", "never"),
        ] {
            c2.set(k, v).unwrap();
            let back = c2.value(k).unwrap();
            let mut c3 = c2.clone();
            c3.set(k, &back).unwrap();
            assert_eq!(c3.value(k), Some(back), "{k}");
        }
        assert_eq!(c2.value("scrollback-limit").as_deref(), Some("4000000"));
        assert_eq!(c2.value("minimum-contrast").as_deref(), Some("4.5"));
        assert!(c.set("keybind", "x").unwrap_err().starts_with("unknown setting"), "keybind is the file's own");
        // The panel's strict check: no clamping, the range in the message.
        let mc = Setting::find("minimum-contrast").unwrap();
        assert_eq!(mc.check("40"), Ok(Value::Number(21.0)), "the file clamps, as Ghostty");
        assert_eq!(mc.check_strict("40").unwrap_err(), "a number from 1 to 21");
        assert_eq!(Setting::find("context-window").unwrap().check_strict("5").unwrap_err(), "a whole number from 1000");
        assert!(Setting::find("background").unwrap().check_strict("nope").is_err());
        assert_eq!(
            Setting::find("clear-hint").unwrap().check_strict("2.5").unwrap_err(),
            "a whole number from 0 to 100"
        );
        let h = help_lines();
        assert!(
            h.contains("--worker-command")
                && h.contains("keybind (file only)")
                && !h.contains('`')
                && !h.contains("](#"),
            "{h}"
        );
    }

    /// README.md's table is generated from SETTINGS; a difference fails here
    /// (the gate). `HAETTY_UPDATE_README=1 cargo test -p haetty-gui config_table`
    /// writes it.
    #[test]
    fn readme_config_table_is_generated() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../README.md");
        // A test binary run away from its source tree (Windows, built in WSL):
        // nothing to compare; the Linux gate checks it.
        let Ok(readme) = std::fs::read_to_string(path) else {
            eprintln!("SKIP: {path} not found");
            return;
        };
        let (a, b) = ("<!-- config-table:start", "<!-- config-table:end -->");
        let i = readme.find(a).expect("README.md: config-table:start marker");
        let i = i + readme[i..].find("-->\n").unwrap() + 4;
        let j = readme.find(b).expect("README.md: config-table:end marker");
        let want = markdown_table();
        if readme[i..j] != want {
            if std::env::var_os("HAETTY_UPDATE_README").is_some() {
                std::fs::write(path, format!("{}{want}{}", &readme[..i], &readme[j..])).unwrap();
                return;
            }
            panic!(
                "README.md's config table differs from config::SETTINGS; run\n  HAETTY_UPDATE_README=1 cargo test -p haetty-gui config_table\n--- README\n{}--- SETTINGS\n{want}",
                &readme[i..j]
            );
        }
    }

    #[test]
    fn diff_settings_parse_reject_and_round_trip() {
        let mut c = Config::default();
        assert_eq!((c.diff_view, c.diff_split_cols, c.diff_after_edit), (DiffViewSetting::Auto, 100, false));
        for (text, want) in
            [("split", DiffViewSetting::Split), ("stacked", DiffViewSetting::Stacked), ("auto", DiffViewSetting::Auto)]
        {
            c.set("diff-view", text).unwrap();
            assert_eq!(c.diff_view, want);
            assert_eq!(c.value("diff-view").as_deref(), Some(text));
        }
        assert!(c.set("diff-view", "both").unwrap_err().starts_with("invalid value"));
        assert_eq!(c.diff_view, DiffViewSetting::Auto, "a rejected value changes nothing");
        c.set("diff-split-cols", "40").unwrap();
        c.set("diff-split-cols", "400").unwrap();
        assert_eq!(c.value("diff-split-cols").as_deref(), Some("400"));
        for bad in ["39", "401", "wide", "2.5"] {
            assert!(c.set("diff-split-cols", bad).is_err(), "{bad}");
        }
        assert_eq!(c.diff_split_cols, 400, "a rejected value changes nothing");
        c.set("diff-after-edit", "yes").unwrap();
        assert!(c.diff_after_edit);
        assert_eq!(c.value("diff-after-edit").as_deref(), Some("true"));
        assert!(c.set("diff-after-edit", "maybe").is_err());
    }

    /// S1 (stage PS): the schema rewrite reads every existing config file as before. The defaults are written out
    /// here field by field, and files of the shapes people have are read to a whole `Config`.
    #[test]
    fn existing_config_files_read_as_before() {
        let defaults = Config {
            window_padding_x: 10.0,
            window_padding_y: 6.0,
            padding_balance: true,
            padding_color: PaddingColor::Extend,
            decorations: Decorations::Auto,
            scrollback_limit: 12_000_000,
            restore_tabs: true,
            restore_claude: true,
            sidebar_auto: true,
            sidebar_width: 42,
            sidebar_min_cols: 121,
            sidebar_agents: true,
            sidebar_files: true,
            sidebar_workers: true,
            sidebar_log: true,
            observe_new_tabs: false,
            token_status: false,
            clear_hint: 60,
            saving_mode: false,
            context_window: 200_000,
            claude_integration: true,
            worker_command: "claude".into(),
            notifications: true,
            theme: "IBM 5153 CGA".into(),
            colors: vec![],
            minimum_contrast: 1.0,
            engine: haetty_core::vt::EngineKind::default_kind(),
            font_family: None,
            font_size: 11.0,
            settings_button: SettingsButton::Auto,
            update_check: true,
            update_url: String::new(),
            update_keep: 2,
            diff_view: DiffViewSetting::Auto,
            diff_split_cols: 100,
            diff_after_edit: false,
        };
        assert_eq!(Config::default(), defaults);
        let read = |text: &str| {
            let mut c = Config::default();
            let bad = c.apply_file(text, &mut crate::keymap::Keymap::default());
            (c, bad)
        };
        // A file with every older key, quotes, comments, CRLF, `yes`/`0` booleans, `_` in bytes.
        let (c, bad) = read(
            "# mine\r\nwindow-padding = 8\r\nwindow-padding-y = 3\r\nwindow-padding-balance = no\r\n\
             window-padding-color = background\r\ndecorations = \"integrated\"\r\nscrollback-limit = 4_000_000\r\n\
             restore-tabs = 0\r\nobserve-new-tabs = yes\r\ntoken-status = 1\r\nclear-hint = 0\r\n\
             context-window = 1000000\r\nsaving-mode = true\r\nclaude-integration = false\r\n\
             worker-command =  claude --model opus \r\nnotifications = false\r\ntheme = haetty classic\r\n\
             palette = 1=#ff0000\r\nbackground = 102030\r\nminimum-contrast = 40\r\nengine = native\r\n",
        );
        assert!(bad.is_empty(), "{bad:?}");
        let want = Config {
            window_padding_x: 8.0,
            window_padding_y: 3.0,
            padding_balance: false,
            padding_color: PaddingColor::Background,
            decorations: Decorations::Integrated,
            scrollback_limit: 4_000_000,
            restore_tabs: false,
            restore_claude: true,
            sidebar_auto: true,
            sidebar_width: 42,
            sidebar_min_cols: 121,
            sidebar_agents: true,
            sidebar_files: true,
            sidebar_workers: true,
            sidebar_log: true,
            observe_new_tabs: true,
            token_status: true,
            clear_hint: 0,
            saving_mode: true,
            context_window: 1_000_000,
            claude_integration: false,
            worker_command: "claude --model opus".into(),
            notifications: false,
            theme: "haetty classic".into(),
            colors: vec![("palette".into(), "1=#ff0000".into()), ("background".into(), "102030".into())],
            minimum_contrast: 21.0,
            engine: haetty_core::vt::EngineKind::Native,
            ..defaults.clone()
        };
        assert_eq!(c, want);
        // The window-padding family: the last line wins, the shorthand sets both.
        let pad = |t: &str| {
            let c = read(t).0;
            (c.window_padding_x, c.window_padding_y)
        };
        assert_eq!(pad("window-padding-x = 4\nwindow-padding = 9\n"), (9.0, 9.0));
        assert_eq!(pad("window-padding = 9\nwindow-padding-x = 4\n"), (4.0, 9.0));
        assert_eq!(pad("window-padding-y = 0\n"), (10.0, 0.0));
        assert_eq!(read("window-padding = 201\n").1.len(), 1, "out of range: skipped, reported");
        assert_eq!(pad("window-padding = 201\n"), (10.0, 6.0));
        // Old values that were refused stay refused.
        for line in [
            "clear-hint = 101",
            "context-window = 999",
            "scrollback-limit = 10MB",
            "minimum-contrast = NaN",
            "restore-tabs = on",
            "worker-command = ",
            "theme = ",
            "decorations = fancy",
        ] {
            assert_eq!(read(line).1.len(), 1, "{line}");
        }
        // The colour keys apply after the theme, the last of a key wins (theme test above for the files).
        let (c, _) = read("background = #000001\ntheme = haetty classic\nbackground = #000002\n");
        assert_eq!(c.value("background").as_deref(), Some("#000002"));
        assert_eq!(c.colors.len(), 2);
    }

    /// The old `CCTERM_*` variables are not read (since K3b): `CCTERM_CONFIG_DIR` does not move the config file;
    /// `HAETTY_CONFIG_DIR` does (`haetty_proto::paths::config_dir`, read by `file_path`).
    #[test]
    fn old_ccterm_variables_do_not_move_the_config_file() {
        let dir = haetty_testkit::run_dir("config-ccterm");
        // Nothing else reads CCTERM_*: setting it cannot disturb a test running beside this one.
        std::env::set_var("CCTERM_CONFIG_DIR", &dir);
        let path = file_path();
        std::env::remove_var("CCTERM_CONFIG_DIR");
        assert!(!path.starts_with(&dir), "{}", path.display());
        if let Some(h) = haetty_proto::product::env_os("CONFIG_DIR") {
            assert_eq!(path, std::path::Path::new(&h).join("config"));
        }
        haetty_testkit::remove_dir_all(&dir);
    }

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
        assert_eq!(Config::default().decorations, Decorations::Auto);
        c.set("decorations", "auto").unwrap();
        {
            use Decorations as D;
            use WindowSystem as W;
            assert_eq!(D::Auto.effective(W::Wayland), D::Integrated);
            assert_eq!((D::Auto.effective(W::X11), D::Auto.effective(W::Windows)), (D::Native, D::Native));
            assert_eq!(D::Integrated.effective(W::X11), D::Integrated, "Linux: integrated on request");
            assert_eq!(D::Integrated.effective(W::Windows), D::Native, "not on Windows yet");
            assert_eq!((D::None.effective(W::Wayland), D::Native.effective(W::Wayland)), (D::None, D::Native));
        }
        assert!(c.set("worker-command", " ").is_err());
        c.set("worker-command", "/opt/claude --model x").unwrap();
        assert_eq!(c.worker_command, "/opt/claude --model x");
        for k in super::keys().filter(|k| !matches!(*k, "theme" | "worker-command" | "keybind" | "font-family")) {
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

    #[test]
    fn saving_mode_sets_both_limits_and_replaces_a_value_it_finds() {
        let mut env =
            vec![("PATH".to_string(), "/bin".to_string()), ("MAX_MCP_OUTPUT_TOKENS".to_string(), "99".to_string())];
        saving_env(&mut env);
        let get = |k: &str| env.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone());
        assert_eq!(get("BASH_MAX_OUTPUT_LENGTH").as_deref(), Some("12000"));
        assert_eq!(get("MAX_MCP_OUTPUT_TOKENS").as_deref(), Some("12000"));
        assert_eq!(get("PATH").as_deref(), Some("/bin"));
        assert_eq!(env.len(), 3);
    }

    #[test]
    fn child_markers_are_not_passed_on_and_saving_mode_applies_on_top() {
        let inherited = vec![
            ("CLAUDE_CODE_CHILD_SESSION".to_string(), "1".to_string()),
            ("CLAUDE_CODE_MESSAGING_TOKEN".to_string(), "x".to_string()),
            ("HOME".to_string(), "/h".to_string()),
        ];
        assert_eq!(child_env(inherited.clone(), false), vec![("HOME".to_string(), "/h".to_string())]);
        let saved = child_env(inherited, true);
        assert!(saved.iter().any(|(k, v)| k == "BASH_MAX_OUTPUT_LENGTH" && v == "12000"));
        assert!(!saved.iter().any(|(k, _)| CHILD_MARKERS.contains(&k.as_str())));
        assert_eq!(saved.len(), 3, "HOME and the two limits");
    }
}
