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
