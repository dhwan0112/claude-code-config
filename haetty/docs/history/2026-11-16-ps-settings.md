# Stage PS: the settings button and the settings panel (2026-11-16)

NORTH-STAR point 1 (an OpenCode-like UI). The person's stage text was given in the session (Korean); its parts are
S0–S8 below. DESIGN §2 change approved in advance for the gear button and the settings panel.

## S0. Survey (before any code change)

### The keys the parser takes (commit `1dce352`)

Read from `crates/haetty-gui/src/config.rs` (`Config::set`, `SETTINGS`, `KEYS`), `main.rs` (`parse_args`,
`load_config_file`) and `app.rs` (`Win` fields, `reload_config`). "Live" = applied while running; "next start" = read
once when the window is made. The command line (`--KEY`) wins over the file; in the file the **last** line of a key
wins (`Config::set` overwrites), colour keys apply after the theme wherever they stand.

| key | format | allowed | default | when | applied by |
|---|---|---|---|---|---|
| `window-padding` | number, px | 0–200 (else rejected) | — (sets x and y) | next start | GUI (layout, renderer) |
| `window-padding-x`, `-y` | number, px | 0–200 | 10, 6 | next start | GUI |
| `window-padding-balance` | bool (`true/yes/1`, `false/no/0`) | | true | next start | GUI |
| `window-padding-color` | choice | background / extend / extend-always | extend | next start | GUI (renderer) |
| `decorations` | choice | auto / native / integrated / none | auto | next start | GUI (window creation) |
| `scrollback-limit` | bytes, `_` allowed | u64 | 12000000 | next start | engine (per session, at creation) |
| `restore-tabs` | bool | | true | next start | GUI (tab sets at start) |
| `engine` | choice | ghostty / native | ghostty (native without libghostty) | next start | daemon + engine (per session) |
| `claude-integration` | bool | | true | next start | GUI (the settings it injects at tab creation) |
| `worker-command` | text, space-split | non-empty | claude | next start | GUI (orchestrator) |
| `notifications` | bool | | true | next start | GUI |
| `observe-new-tabs` | bool | | false | next start | GUI → daemon flag per new session |
| `token-status` | bool | | false | next start | GUI (status panel) |
| `clear-hint` | percent | 0–100 | 60 | next start | GUI |
| `context-window` | tokens | ≥ 1000 | 200000 | next start | GUI |
| `saving-mode` | bool | | false | next start | GUI (environment of new Claude Code sessions) |
| `keybind` | `CHORD=ACTION` / `CHORD=unbind` | file only, repeats | — | next start | GUI (keymap) |
| `theme` | name or path | non-empty | IBM 5153 CGA | **live** (`reload_config` on focus and Reload config) | GUI + engine (`set_colors`) |
| `palette` | `N=COLOR`, N 0–15 | repeats | the theme's | **live** | GUI + engine |
| `background`, `foreground`, `cursor-color`, `cursor-text`, `selection-background`, `selection-foreground` | colour | `#rrggbb`, `rrggbb`, X11 name | the theme's | **live** | GUI + engine |
| `minimum-contrast` | number | clamped to 1–21 (not rejected) | 1 (off) | **live** | GUI (renderer `Look`) |

Not config keys: `--font FAMILY` and `--font-size PT` (4–96, default 11) are command-line options only; there are no
cursor-shape keys. `HAETTY_CONFIG_DIR` moves the config directory; the old `CCTERM_*` variables are **not** read since
K3b (only the old `ccterm` folder is migrated once, `migrate.rs`). Bad lines are skipped with a log line each.

### The panels: drawing, input, closing

- **Drawing.** Everything haetty draws goes through `ui::Compose::build` (`ui.rs`): the tab bar, the status row (Claude
  Code tabs), the status panel (right columns), the diff view (over the screen area, hides the cursor), the palette
  (centered box, its cursor in the query line), hints and the link row. Cells not taken from the program's screen get
  `CellFlags::HAETTY_UI` at the end (the renderer skips `minimum-contrast` for them). Helpers: `ui::put`, `fit`,
  `fit_left`, `blank`, `Greys` (bar, dim, normal, strong; amber/red only for attention).
- **Input.** `Win::keyboard` (`app.rs`): an IME composition first; then, on a press with a chord, an open palette gets
  the key (`palette_key`), else a shortcut runs (`Win::run`), else the diff view takes the key, else the key goes to
  the program. IME commits go to the palette's query when it is open (`Win::ime`). A key with text and no key name
  (Windows `VK_PACKET`) goes into the palette, never past it (V6).
- **Closing.** Esc sets `palette = None` / `diff = None` and recomposes; nothing else holds state.
- **Reused by the settings panel:** the `Compose` overlay (the panel draws over the screen area like the diff view;
  its cells are haetty's), the `ui::put`/`fit` helpers and `Greys`, the palette's routing in `keyboard` and `ime`
  (the panel takes every key and commit while open), `reload_config` (the live part), `Action` + the palette entry +
  a keymap default. New: `settings.rs` (the panel's state, layout, keys) and `config_edit.rs` (the line-preserving
  file edit).

### U+2699 (⚙) as a single-colour text glyph

- `emoji.rs`: U+2699 is not `Emoji_Presentation` → text presentation; the font lookup passes over colour faces for
  text presentation unless nothing else has the glyph (`font::pick`). Its width is 1 (`unicode-width`).
- Linux (kramers): `fc-list ':charset=2699'` — DejaVu Sans Mono (regular and bold, the first primary default), DejaVu
  Sans, Noto Sans Symbols, FreeMono/FreeSans, Noto Color Emoji. So the primary face draws it, single colour.
- Windows (from the fallback list, not run here): Segoe UI Symbol (single colour) comes before Segoe UI Emoji in
  `FALLBACK_DEFAULTS`; with text presentation the colour face is passed over. **Unverified** (Windows check list).
- Rule (implemented, `Win::gear`): the cell text is `⚙` + VS15 (U+FE0E, text presentation asked for explicitly). When
  the fonts give no face for U+2699, or only a colour face, the button is `≡` (U+2261, in every primary default of
  both platforms) instead. The choice is logged once at start and after a font change.

## S7. The two end-to-end failures on kramers (`orchestrator.sh`, regression `ime`)

Investigated before the settings code, since `settings.sh` uses the same input path. Both were stale or fragile test
scripts, not the machine's Xvfb and not a product defect.

- **`orchestrator.sh` (4 of 21).** The keys did reach the window: the palette takes them before the GUI's `key <text>`
  debug line, so typed palette text never shows in `gui.log` (the log had `action command_palette` and
  `action new_task`). The New task wizard has seven questions since G5 (`ac6c4a7`, 2026-11-09; three before) (title, description, scope, related files,
  model, effort, repository and base); the script answered three, so the palette stayed on "3/7 Scope (optional…)" and
  no task, worktree or worker followed (a screenshot of the run shows that prompt). Ruled out: the environment
  (`XMODIFIERS`, `QT_IM_MODULE`, `XAUTHORITY`, `DBUS_SESSION_BUS_ADDRESS` unset: same 4/17) and the focus method
  (one `windowfocus` or one before every key: same). Fix: four more Enter keys (steps 3–6 are optional). Result
  **21 passed, 0 failed** (`scripts/e2e/xvfb.sh env bash scripts/e2e/orchestrator.sh`, 2026-11-16 16:11).
- **The soak had the same stale sequence.** `soak.sh`'s task operation answered three questions too, and it counted a
  task as done whether or not one was made: since G5 the soaks (the P1c short soaks included) made no task, and their
  "tasks 3 of 3" was the number of attempts. Fixed the same way; a task now counts only when its worktree existed
  and was removed again, and "task failures (not created or not cleaned up)" are printed. The Windows twins
  (`windows/orchestrator.ps1`, `windows/soak.ps1`) got the same four Enter keys — **not run** (Windows).
- **regression `ime` (preedit=0 commit=0).** The test starts its own D-Bus and `ibus-daemon` (not the desktop's), XIM
  registers (`xprop -root XIM_SERVERS` = `@server=ibus`) and the GUI logs `ime Enabled`. But ibus-hangul 1.5.5 starts
  in Latin mode here (`gsettings … org.freedesktop.ibus.engine.hangul initial-input-mode 'latin'`), so g/k/s pass
  through without a composition. Fix: `ibus-daemon` and the `ibus` command run with a private keyfile GSettings
  backend (`GSETTINGS_BACKEND=keyfile`, `XDG_CONFIG_HOME=$HAETTY_RUN/ibuscfg`, `initial-input-mode='hangul'`); that
  also keeps ibus from writing its address file into the real `~/.config/ibus/bus`, which the old script did. Result:
  `PASS ime preedit 7, commit 1, 한 sent`, regression **30 passed, 0 failed**. Ruled out: the locale (C.UTF-8 and
  ko_KR.UTF-8 alike), an explicit Hangul toggle key.
- **The short soak with real tasks** (`soak-short.sh`, libghostty, build `e25d545` + these scripts, 16:15–16:25):
  `operations: tabs 33/200, detach 8/50, tasks 3/20, themes 2/10; task failures (not created or not cleaned up) 0`,
  `PASS tasks_done: 3 of 3`, **VERDICT PASS** — now with three tasks made (worktree, worker tab) and removed.

## S1. One schema (`e25d545`)

`config::SETTINGS` holds per key: `Kind` (bool, choice, number with range/step/int/clamp, text, font, theme, colour,
repeat), the default as the file writes it, README's text, the panel's line, the group, `When` (live or next start),
`panel` and `cli`. `Config::set` checks every value with `Setting::check` (the only place ranges live); `KEYS` is gone
(`--KEY` is accepted when the schema marks `cli`); `haetty --help` and README's table are generated from it (rows with
the same text share a README row, as before). New keys: `font-family` (`--font` stays as its short name), `font-size`,
`settings-button`. Tests: every default through the parser equals `Config::default()`; `Config::value` round-trips;
a file with every older key (CRLF, quotes, `yes`/`0`, `_` in bytes, a clamped `minimum-contrast`) reads to a whole
`Config` written out field by field; the `window-padding` family (last line wins, the shorthand sets both); the values
that stay refused; `CCTERM_CONFIG_DIR` does not move the config file.

## S2–S5. The button, the panel, saving, applying while running (the DESIGN §2 commit)

- **Code.** `settings.rs` (the panel: state, layout, keys, mouse; it returns `Effect`s and never writes or touches a
  session), `config_edit.rs` (the line-preserving edit and the file write), `app_settings.rs` (a child module of
  `app.rs`: the button, the panel's input, `change_setting`, `apply_config`, the theme preview, the 150 ms relayout).
  `app.rs` grew by about 130 lines (fields, routing); `ui::Compose` draws the panel's grid like the diff view and the
  button over its cell (`HAETTY_UI`). The renderer got `set_fonts`, `text_glyph` and `monospace_families`; `font.rs`
  `has_family`, `monospace_families` and `text_glyph`.
- **Protected areas: none touched** besides the approved DESIGN §2 change. The hook output rules, `Daemon::message` /
  `Daemon::input`, the permission-prompt path and the injected settings are unchanged; the panel only keeps keys and
  mouse events from the program while open (as the palette does). No new daemon message: the theme preview uses the
  existing `SetTheme` path (`RemoteHandle::set_colors`); font and padding changes reach the sessions as an ordinary
  resize.
- **What applies at once:** theme, colour keys, `minimum-contrast`, `font-family`, `font-size`, the padding keys,
  `settings-button`, `notifications`, `token-status`, `clear-hint`, `context-window`. Next start: `decorations`,
  `engine`, `scrollback-limit`, `restore-tabs`, `claude-integration`, `observe-new-tabs`. No cursor-shape keys exist
  (none added).

## S6. Checks

- **Unit** (`cargo test -p haetty-gui`): the schema tests above; `config_edit` — a table of edits (comment, blank line,
  unknown key, CRLF, mixed ends, duplicate key, missing key, a last line without an end, back to the default with a
  commented-out line, spaces, a quoted value), each read back by the parser to what the panel showed; the
  `window-padding` shorthand that still decides a value; a property test over 400 generated files (every untouched
  line the same bytes in the same order, the new value read back); the file write (temporary file and rename, no
  temporary left, one backup per run, a dated one next run, a file changed elsewhere, a link kept, read-only refused).
  `settings` — groups and the two actions, keys to effects (bool, choice, number steps, typed input refused out of
  range with the range on the row), the theme list's preview and Esc, a font typed with an IME commit or picked,
  search over every group, command-line keys refused, drawing at 100/40/20 columns in greys only.
- **Renderer** (`render::paint` `minimum_contrast_leaves_haettys_own_cells_alone`): the button's cell has dim-grey
  pixels, no non-grey pixel and no white one at `minimum-contrast` 4.5, and the same dim pixels at 1.
- **Reference frames** (Linux, made on kramers): `claude-panel` and `workers` with the button, new `settings` (60
  columns: the groups on one line) and `shell-gear`; the other seven frames came out identical with kramers' fonts.
  The old Linux references were made with fewer fallback fonts (2026-11-05) and were skipped on kramers; `fonts.txt`
  now names kramers' list. **Windows frames: not updated** (the Windows check list says how).
- **Xvfb e2e** `scripts/e2e/settings.sh` (in `run-all.sh`), 21 of 21: no button away from the corner, the button near
  it, a key hides it; a click on it, Ctrl+, (and again to close) and the palette open the panel; nothing reached the
  program while it was open (keys, text, clicks, the wheel); the theme preview and Esc back, nothing written; font size
  11 → 14 by three quick ←/→: the grid 80 → 64 columns and **one** session resize; the file keeps its comment and
  unknown key, `config.bak` is the file before; no button while the program reads the mouse; after a restart 14 pt
  is kept (window 740 → 900 px for 80 columns); a Claude Code tab shows the button without the pointer; a
  permission-waiting tab keeps `Ask` and gets no byte while the panel opens and closes; the real
  `~/.config/haetty/config` untouched (mtime and size).
- **run-all** (kramers, 2026-11-16 16:38–16:43): every script passes — regression 30/30, DESIGN §2.3 5/5 (system and
  integrated decorations; the Claude Code tab now shows the button: idle screens still grey only), themes 8/8, title
  row 13/13, remap 3/3, links 9/9, tabs 20/20, orchestrator 21/21, daemon crash 9/9, scale 13/13, settings 21/21.
- **Screen dumps** `docs/look/ps-*.png` (`scripts/e2e/settings-look.sh`): the Claude Code tab's button before and
  with a status line, the panel in each group, a shell tab with the pointer away and at the corner, a 46-column
  window (the group line scrolls to the current group, ‹ ›), and the three built-in themes.

## Choices made, and what was not chosen

- **One commit for S2–S5.** DESIGN §2's rule puts the code and the reference frames of an approved change into one
  commit; the panel needs the saving and applying code to do anything, so they go together. S0+S1 and S7 are their own
  commits.
- **The panel draws over the screen area** (like the diff view), not a centered box like the palette: the settings
  need the room, and the status row, tab bar and status panel stay where they are (no layout change).
- **Rows show a label made from the key** (`window-padding-x` → "Window padding x") and the key itself on the bottom
  line, right-aligned so it is never cut. Not chosen: a second hand-written label per key (two places to keep).
- **Bool values show on/off; choices `‹ value ›`; colours and the font "the theme's" / "default" when empty.**
- **Font family: Enter opens the list of the system's monospaced families with a filter line**; the filter is typed
  (IME commits too), and Enter on no match uses the typed name — one control for both picking and typing. A family
  the system lacks is refused before writing. Not chosen: a separate free-text field.
- **"Live" includes `notifications`, `token-status`, `clear-hint`, `context-window`** (window-only numbers) beyond the
  list given; **`observe-new-tabs`, `saving-mode`, `worker-command`, `claude-integration` stay next start**
  (consent, Claude Code's environment, the injected settings: not changed under a running window).
- **Not in the panel:** `saving-mode` and `worker-command` (they change how Claude Code works: output limits, the
  worker's arguments such as `--model`), `palette`, `keybind` (they repeat), the `window-padding` shorthand (x and y
  are there). `claude-integration` is the one Claude Code entry (haetty's observing hooks and status line).
- **Groups:** Appearance (theme, contrast, colours), Font, Window (padding, decorations, the button), Terminal
  (scrollback, restore tabs, notifications), Claude Code (integration, token status, the /clear hint, context window),
  Advanced (engine, observe new tabs).
- **The button's region for other tabs:** the last 6 columns of the bottom 2 rows, as asked; `always` shows it
  whenever the pointer is in the window (still not under mouse reports).
- **A key typed on the command line** shows "command line" in its row and is not changed (the file's value would
  not apply).
- **Width under 64 columns:** one column with a scrolling group line. Below 20 columns or 4 rows: no panel drawn.
- **The keys hint in the title row** shortens with the width (three forms).
- **Ctrl+, default chord.** Not plain Ctrl+letter, so §3.11's rule holds; Claude Code binds nothing to it.

## Sizes (stripped, the gate's copies)

| binary | before (`44b77c5`) | after (`4ff04b1`) | change |
|---|---|---|---|
| Linux `haetty` | 9 958 480 B | 10 140 512 B | +182 032 B (+178 KB) |
| Windows `haetty.exe` | 10 166 272 B | 10 289 152 B | +122 880 B (+120 KB) |
| `haetty-headless` (both) | 3 031 200 B / 4 795 904 B | the same | 0 |

No new dependency; the linkage is unchanged (Linux NEEDED libgcc_s, libm, libc, ld-linux; the same Windows DLLs).
