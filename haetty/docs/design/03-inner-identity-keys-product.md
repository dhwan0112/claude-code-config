# haetty design: inner design: identity, names, layout, keys, icon (3.8-3.12)

> Part of the design. Index: [`../DESIGN.md`](../DESIGN.md) (section
> numbers are the same there). Text below is moved verbatim from the former `docs/DESIGN.md`.

### 3.8 Identity, state and event log

- Session and tab ids: `<unix ms, hex>-<counter, hex>` (sortable, unique per
  machine). A restored tab is a new session (new id): a tab set stores what
  to start, not the session; Claude Code session ids (UUIDs, from hooks) are
  stored with it so that Claude Code itself resumes (`--resume`).
- Reattach (review C8): a window that is gone (crashed, killed) leaves its
  sessions in the daemon. At start the GUI asks the daemon first
  (`SessionInfo.attached` = Control clients attached, `claude_session`):
  running sessions with no window become tabs again as they are; exited
  ones nobody shows are closed. Tab-set restore then applies only to saved
  tabs the daemon does not have (`tabsets::not_live`: Claude Code tabs by
  conversation id, others by program, arguments without resume requests,
  folder; each live session matches one saved tab), and a conversation
  running anywhere in the daemon is never resumed again (checked against
  the daemon's list at that moment, not the tabs' Claude Code state, which
  arrives later). `haetty` without a program opens no extra tab when
  anything came back; `haetty PROGRAM` still opens `PROGRAM`. Palette:
  "Attach detached sessions". Sessions of another open window
  (`attached` > 0) are never taken.
- Tab sets (`tabs/<name>.tsv` in the state directory, after ccx's
  `savetabs.sh`): per tab name, folder (OSC 7, else the start folder),
  program, arguments, Claude Code session id, active flag; never
  environment variables. `last` follows the open tabs (rewritten only when
  the content changes); at start it becomes `previous` and is reopened
  after the requested tab; `restore-tabs = false` (config or
  `--restore-tabs false`) turns that off; palette: save as `saved`, open
  `saved`, open `previous`. A seventh column `custom` (1: the person named
  the tab; files without it read as 0) keeps a renamed tab's name across
  restores; automatic names are stored for display only and recomputed.
  Closed tabs (2026-11-06, ccx's `closed.tsv`): a tab closed on purpose —
  Ctrl+Shift+W, an exited tab closed, the last tab — goes first into the
  set `closed` (20 at most, one entry per conversation); closing the window
  adds nothing (that is `last`). Palette "Reopen a closed tab" lists name,
  folder, program and "resume"; Enter opens it like a tab-set entry (the
  same rules: missing folder skipped, a running conversation never opened
  twice, `--resume` when the transcript exists) and removes it from the
  list. A conversation counts as running when a live session reports its id
  *or* was started with `--resume ID` (Claude Code reports its id a moment
  after starting; found by the stage S2 test reopening twice in a row).
- Tab names (first that applies): the name the person gave (Rename tab,
  Ctrl+Shift+R or the palette; up to 64 characters, control characters
  removed; empty puts the automatic name back; nothing is sent to the
  program and the text is not logged); Claude Code's session name;
  Claude Code tabs only: `repo:branch` of the tab's folder (`gitview.rs`,
  read from `.git/HEAD` — linked worktrees and submodules followed, a
  detached HEAD as 7 hex digits — no process, again when the folder changes
  or after 3 s); a restored name while the program has no title; the title
  without Claude Code's state glyph; the program. Decision: `repo:branch`
  only for Claude Code tabs — a shell's title (user, host, folder, the
  running command) says more than the branch.
- Detach (palette "Detach tab"): the active tab leaves the window, its
  session keeps running unattached in the daemon (not in `last`, not in
  `closed`); "Attach detached sessions" shows it again. The only tab is not
  detached (the window would close); a hint says so. Decision: restore *after* the requested tab
  rather than instead of it, so `haetty PROGRAM` always runs `PROGRAM` and
  nothing is lost when a set is broken; tabs whose folder is gone are
  skipped with a log line. Alternatives not taken: reattaching the
  sessions a killed GUI left in the daemon (possible later: they are in
  `status`), a set per window (one window today; with several windows the
  last writer wins `last`), JSON (no parser in the binary; TSV with
  escapes is enough).
- State directory: Linux `$XDG_STATE_HOME/<app>` (default
  `~/.local/state/<app>`), Windows `%LOCALAPPDATA%\<app>`. Config:
  Linux `$XDG_CONFIG_HOME/<app>/config` (`~/.config/<app>/config`), Windows
  `%APPDATA%\<app>\config`.
- `events.log` in the state directory: append-only, one tab-separated line
  per event — `seq`, unix ms, kind, session id, derived fields. A reader
  continues from a `seq` cursor. **Only derived fields** are written: event
  names, tool names, file paths, Claude Code session ids, exit codes,
  titles. Never environment variables, prompts (`UserPromptSubmit` carries
  the typed prompt), `tool_input` contents, transcripts or credentials.
- Tab sets and the daemon's session list are files in the state directory
  (milestone 2, P3).
- Friction log (added on request after P4), in the same `events.log`:
  - `slow_input` — key or paste to the next screen change over the 16 ms
    budget (measured in the daemon; includes the program's own reaction,
    e.g. Claude Code redrawing), at most one line per session per 10 s;
  - `latency` — per session at close/exit, daemon stop, and every 10 min
    with ≥ 1000 samples: `n`, `p50_ms`, `p95_ms`, `max_ms`, `over_16ms`;
  - `panic` — daemon panics (message from our code, location, thread);
  - `error` — session errors and failed starts;
  - `gui_slow_draw` (a GUI frame over 16 ms, at most every 10 s) and
    `gui_panic`, sent by the GUI as `Friction` messages (Control role only,
    through the existing role rules).
  Never typed or shown text (a test pastes a marker and checks it is not
  in the log).

### 3.9 Product name

The product name, **haetty** (decided 2026-11-05; working name `ccterm`
before), lives in one constant module, `haetty_proto::product`: executable
name used when spawning the daemon and in hook commands (taken from
`current_exe()` at run time, falling back to the constant), app id
`io.github.dhwan0112.haetty` (Wayland `app_id`, X11 `WM_CLASS` instance and
class, `.desktop` file and hicolor icon name), socket, pipe, state and
config directory names (`haetty`), window title, the environment prefix
`HAETTY_`. Renaming also touches the `[[bin]]` names (`haetty`,
`haetty-headless`; Cargo needs literals) and `TERM_PROGRAM` / XTVERSION in
`haetty-core` (below this crate, so not taken from the module).

Former name: `product::env` reads `HAETTY_<NAME>` and falls back to
`CCTERM_<NAME>`, logging once per variable that the old name was used (also
for variables read before the logger existed). On start (GUI and daemon)
`migrate::run` copies the old config file and `tabs/*.tsv` when haetty has
none (originals stay; nothing is overwritten; skipped when the directories
are overridden) and checks the old socket/pipe: a running `ccterm daemon`
is reported in the log and a hint, never touched — the names do not
overlap, so both daemons run side by side. Checked by
`crates/haetty-gui/tests/migrate.rs` with the real binary.

Instances (added on request after P4): next to the name constants,
`product::instance()` reads `HAETTY_INSTANCE` (lower-case ASCII letters,
digits, `-`, ≤ 16; unset or empty = stable). A named instance gets its own
socket/pipe (`haetty-dev`), runtime, state and config directories
(`~/.local/state/haetty-dev`, …), app id (`io.github.dhwan0112.haetty.dev`,
so its desktop file and dock icon are separate) and window title prefix (`[dev] `). The daemon
sets `HAETTY_INSTANCE` in every session's child, so hooks of a dev session
report to the dev daemon. Purpose: run a development build beside the
stable one (self-hosting) without touching its sessions, tab sets or
settings. Checked by `crates/haetty-gui/tests/instances.rs` with two real
daemons.

### 3.10 Layout rules derived from §2

- The grid asked for (`--cols` × `--rows`, plus the title row) is what the
  first session gets, at the scale of the monitor the window is on, not the
  primary monitor's (A7, 2026-11-07). Wayland reports the scale only after
  the surface is shown on an output, so a Wayland window first shows a
  blank frame and opens its tabs on the first `ScaleFactorChanged`, or
  300 ms after its first frame when none comes: no PTY is sized from a
  guess, and Claude Code does not draw once at a wrong size. On every scale
  change — the first one, or a move to a monitor with another scale — the
  grid stays and the window asks for the pixels that grid needs at the new
  scale (winit's `inner_size_writer`). Before, the window kept its pixels:
  `--cols 80 --rows 24` gave 64x19 at 125 % (kramers, 2026-11-06).

- A tab whose program exits stays, dimmed with its exit status, until the
  user closes it; with one tab, the window closes when its program exits
  (as in milestone 1). So an event never removes a tab or the tab bar.
- The status line row is part of a Claude Code tab's layout from the first
  frame (integration on): it stays blank on the terminal's background until
  Claude Code reports something (context, limits, cost, changed files, or
  work in progress), so its arrival does not resize the terminal. Window
  size requests add that row, so the terminal keeps the requested rows.
- Shell (non-Claude Code) tabs have no status row: their program gets that
  row too. Decision on tab switching (2026-11-06): every tab keeps its own
  grid size for its whole life — shell tabs the full area, Claude Code tabs
  one row less — and switching tabs only changes which one is shown, never
  a size. So switching never resizes a session (a resize would make Claude
  Code redraw its whole screen) and a Claude Code tab is never reflowed by
  its status row appearing. Alternatives not taken: one size for all tabs
  (a shell tab would lose a row to a status line it never has) and
  reserving the row only once content arrives (one full Claude Code redraw
  per tab). Checked (Linux, Xvfb): a shell tab 70x11 beside a Claude Code
  tab 70x10; 20 switches → the same sizes, no resize.
- The diff indicator says "N files changed · Ctrl+Shift+D": plain Ctrl+D
  reaches Claude Code (and exits it on an empty prompt), so app shortcuts
  use Ctrl+Shift.
- `statusLine` is a single value: injecting haetty's replaces the user's own
  status line in haetty tabs (as in ccx). `haetty statusline` prints
  nothing, so Claude Code draws no line of its own; the information goes to
  haetty's status line instead.
- Settings are injected only into tabs whose program is `claude`.

### 3.11 Keys

Application shortcuts use Ctrl+Shift (plus Ctrl+Tab and Alt+digit), never
plain Ctrl+letter, which belongs to the program. Checked against Claude
Code's interactive-mode reference (code.claude.com/docs/en/interactive-mode,
2026-11-05): it binds Ctrl+C/D/G/L/O/R/V/B/T/S/Z/A/E/K/U/W/Y/P/N/_,
Ctrl+Shift+- (undo), Ctrl+Enter, Ctrl+X chords, Shift+Tab, Tab, Esc,
Alt+M/P/T/O/Y/B/F/D/V.

| Action | Default | Clash check |
|---|---|---|
| New tab | Ctrl+Shift+T | Claude Code: Ctrl+T (task list) only; distinct with the kitty protocol, legacy terminals cannot tell them apart but haetty consumes the chord before the program |
| Close tab (press twice) | Ctrl+Shift+W | Claude Code: Ctrl+W (delete word); same remark |
| Next / previous tab | Ctrl+Tab / Ctrl+Shift+Tab, Ctrl+PageDown / Ctrl+PageUp | free in Claude Code; Shift+Tab alone stays with the program |
| Go to tab N | Alt+1 … Alt+9 | free in Claude Code; bash/readline uses Alt+digit as a numeric argument (rarely typed) — rebind if needed |
| Command palette | Ctrl+Shift+P | free (Claude Code: Ctrl+P = previous history line) |
| Paste | Ctrl+Shift+V, Shift+Insert, Paste key; Windows also Ctrl+V | Linux plain Ctrl+V stays with Claude Code (image paste) |
| Diff pane / status panel (milestone 3) | Ctrl+Shift+D / Ctrl+Shift+S | free (Claude Code: Ctrl+D exit, Ctrl+S stash) |
| Rename tab | Ctrl+Shift+R | free (Claude Code: Ctrl+R history search) |
| Reopen a closed tab, detach tab, attach detached sessions | palette only | — (no chord: rarely used; detach is easy to mistake for close) |
| New task, message the worker, remove a task's worktree (§8) | palette only | — (no chord: each makes or sends something; a chord pressed by mistake must not) |
| In the diff view: next view / read again | m or Tab / r | the view has the keyboard while open; nothing reaches the program |

Linux: GNOME/KDE terminals use the same Ctrl+Shift family inside their own
windows only; haetty's window gets the keys first. Windows: Windows
Terminal uses Ctrl+Shift+T/W and Ctrl+Tab the same way; Alt+digit is free.
Every binding can be changed in the config file (`keybind = CHORD=ACTION`,
`CHORD=unbind`). Irreversible actions (close) ask for the same chord again
within 3 s, with a short hint over the bottom row; no dialog.

### 3.12 Icon and desktop integration

The icon is the user's design (`assets/icon/haetty.svg`, 256×256: a
window-shaped body #0A0A0B with transparent margins, an orange #FF6A2C
gradient bar on top and an orange glow fading into the black from the top
left, a grey #B4B2AF `❯` and underscore cursor). Its colours, shapes and
proportions are not changed by code. Everything derived from it is
generated once and committed (`assets/icon/README.md`: resvg 0.49 and
small Python scripts), so a build needs no conversion tool:
- `png/haetty-N.png` for N = 16, 20, 24, 32, 40, 48, 64, 128, 256, 512;
  16 and 24 px were checked magnified (`assets/icon/check/`) and are the
  plain renders (the shapes hold; no hand-tuned pixels);
- `haetty.ico` with 16, 20, 24, 32, 40, 48, 64, 256 (PNG entries);
- `rgba/haetty-64.rgba`, the 64 px image as raw RGBA.

Uses (`crates/haetty-gui/src/icon.rs`, `build.rs`):
- Windows: `build.rs` links `haetty.ico` as icon resource 1 (`windres`), so
  Explorer, shortcuts and the taskbar show it; the window's small and big
  icons are loaded from that resource at the system's small/large icon
  sizes, so Windows picks the matching image from the `.ico`.
- Linux X11: the window icon (`_NET_WM_ICON`) is the embedded 64 px RGBA
  (no PNG decoder at run time). Wayland has no window icon in winit; the
  dock uses the desktop entry.
- `haetty install-desktop` (Linux) writes
  `~/.local/share/applications/io.github.dhwan0112.haetty.desktop`
  (`Icon=` and `StartupWMClass=` = the app id) and the icon as
  `icons/hicolor/{16…512}x{16…512}/apps/io.github.dhwan0112.haetty.png`
  plus `scalable/apps/io.github.dhwan0112.haetty.svg`; on Windows it has
  nothing to install.
- Wayland `app_id` = X11 `WM_CLASS` (instance and class) = the app id, so
  docks match the window to the desktop entry.
- Title bar colours, Linux (checked in the sources, 2026-11-06):
  - GNOME on Wayland gives Wayland clients no server-side decorations, so
    winit draws its own: sctk-adwaita 0.10.1 (winit 0.30.13
    `platform_impl/linux/wayland/window/state.rs`, `into_sctk_adwaita_config`).
    winit passes only a theme — `Theme::Dark`/`Light`/`None` →
    `FrameConfig::dark()`/`light()`/`auto()` — and the colours are constants
    in sctk-adwaita (`theme.rs`: dark header bar `#303030` active, `#242424`
    inactive; light `#ebebeb` / `#fafafa`). haetty asks for dark or light
    from the theme background (`chrome.rs`), so the bar is dark, but its
    colour cannot be the terminal's `#141414`: winit has no call for
    colours.
  - GNOME on X11: mutter draws the frame; `_GTK_THEME_VARIANT=dark` (set by
    haetty) selects its dark variant; the colour is mutter's.
  - The way to a title bar in the terminal's colours on Linux is
    `decorations = integrated` (haetty draws the bar and its buttons, with
    the tabs in it), done 2026-11-06 (`titlebar.rs`).
- `decorations = auto | native | integrated | none`, default `auto`
  (`Decorations::effective`): `native` where the system can give its title
  bar the theme's colour — Windows 11 (DWM caption/border colours,
  `chrome.rs`) and X11 (the window manager draws it; mutter takes
  `_GTK_THEME_VARIANT=dark`) — and `integrated` where it cannot: Wayland,
  where winit draws an Adwaita frame with fixed colours (above). On Windows
  `integrated` is not implemented (snap layouts would need the caption hit
  test) and means `native`. The window system is winit's (`is_wayland`).
- Integrated, how it works: the window has no system decorations
  (`with_decorations(false)`); the title row is row 0 of the window grid
  (`top()` = 1 with any number of tabs, so a second tab no longer adds a
  row), and the window asks for one more row so the program keeps the
  requested size. One layout function (`titlebar::layout`) places tabs and
  buttons for drawing and for hit tests. Mouse: a press on empty title
  space starts `drag_window` (move), two within 400 ms and 4 px toggle
  maximize, a press on a tab selects it, a button acts on release over the
  same button; within 6 logical px (× scale) of a window edge or corner the
  cursor becomes a resize cursor and a press starts `drag_resize_window` —
  not while maximized; a right click does nothing (no window menu yet).
  Button layout: `HAETTY_TITLE_BUTTONS` (GNOME syntax) if set, else the
  settings portal via `dbus-send` (as winit's Adwaita frame reads its own
  settings), else `gsettings`, each ≤ 150 ms at window creation, else
  right-side minimize/maximize/close. Taken from GNOME: `appmenu`, `menu`,
  `spacer` entries are ignored.
- Windows: the GUI process sets its AppUserModelID to the app id
  (`io.github.dhwan0112.haetty`, a dev instance `….haetty.dev`) before any
  window exists, so the taskbar groups haetty windows and a pinned icon
  stays the same whatever folder the executable runs from.
