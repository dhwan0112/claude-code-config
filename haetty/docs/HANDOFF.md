# haetty handoff

State as of 2026-11-05, after milestones 2 (daemon, IPC, tabs, tab sets,
config file, palette, install-desktop) and 3 (Claude Code integration) —
§6.4 is the progress log of those; sections 1–5 and 8 describe milestone 1
(`26e0056`.., `01e1d32`.., `e9312a4`..) and are kept as its record. The code
lives in `haetty/` of the `dhwan0112/claude-code-config` repository.
Written for whoever continues the work (Claude Code included).














## 1. Goal

A terminal made for Claude Code that replaces the user's tmux-based `ccx`
workspace (repository `dhwan0112/claude-code-config`: tabs, side-by-side
diff pane, right-hand "status panel" fed by Claude Code hooks and the status
line JSON). Requirements set by the user:

1. Windows and Linux both, natively (no WSL, no Zellij).
2. Lightweight.
3. Standalone (single executable).
4. Ghostty-like: a real GUI terminal, built in Rust; backend first.

## 2. What exists

| Crate | State |
|---|---|
| `haetty-pty` | Done. Unix via portable-pty. Windows: own ConPTY code (`src/win/mod.rs`), host order sideloaded `conpty.dll` (`HAETTY_CONPTY_DLL` or next to the exe) → embedded Microsoft ConPTY 1.25.260930003 (crate `conpty-bin-vendored`, written to `%LOCALAPPDATA%\haetty\conpty\<version>\`) → kernel32. PATH/PATHEXT lookup; `.cmd/.bat` (npm's `claude.cmd`) run via `cmd.exe /d /s /c`. |
| `haetty-core` | Done. `Session::spawn(SessionConfig)` → `SessionHandle` + `Receiver<SessionEvent>`. Engine thread owns `libghostty_vt::Terminal`, `RenderState`, key/mouse encoders and the PTY writer. |
| `haetty-headless` | Done. Step-script runner (README lists the steps; `drag` added for selection tests). |
| `haetty-gui` | Milestone 1 done (binary `haetty`). See below. |
| `haetty-proto` | Milestone 2: product constants, paths, wire types (postcard), framing, local socket / named pipe transport, daemon client (`RemoteSession`). |
| `haetty-daemon` | Milestone 2: owns the sessions; frames, roles, event log (DESIGN.md §3.5–3.8). |
| `haetty-cc` | Milestone 3: Claude Code integration — `--settings`, `hook`/`statusline` reporters, per-session state, edit snapshots and diffs, resume (DESIGN.md §3.13). |

Public API a frontend uses (`haetty-core`):

- `SessionHandle::{key, mouse, wheel, paste, focus, resize, scroll, write, snapshot, dump, set_color_scheme, backend, pid}`
- `SessionEvent::{Wakeup, Title, Pwd, Bell, ClipboardWrite, Notification, Progress, Reply, Exited, Error}`
- `Snapshot { cols, rows, cells: Vec<Cell>, cursor, default_fg/bg, cursor_color, title, pwd, modes, scrollbar, seq }`;
  `Cell { text, fg, bg, flags, width: Narrow|Wide|Spacer }`
- `KeyInput { key: Key (physical, libghostty enum), action, mods, consumed_mods, text, unshifted, composing }`,
  `KeyInput::parse("shift+Enter")`; `MouseInput::{at_pixel, at_cell}`; `GridSize { cols, rows, cell_width, cell_height }`.

`Wakeup` is sent at most once until the frontend takes a snapshot. No wakeup
while the application holds synchronized output (DEC 2026), up to
`sync_timeout` (500 ms).

### `haetty-gui` layout

| Module | Role |
|---|---|
| `config.rs` | Settings with `Config::set(key, value)`: `window-padding`, `window-padding-balance`, `window-padding-color`, `decorations`; the CLI passes `--KEY VALUE`, the config file (`key = value`, milestone 2) too. |
| `chrome.rs` | Native title bar/frame colours (Windows 11) and dark/light theme request, updated when the default colours change. |
| `main.rs` | CLI (`--font`, `--font-size` pt, `--cols`, `--rows`, settings, program; reserved subcommand names `SUBCOMMANDS` end with "not implemented yet", a program of that name goes after `--`), logger (`HAETTY_LOG`, `HAETTY_LOG_FILE`), fatal and CLI errors (Windows GUI subsystem: redirected stdout/stderr, else the parent's console, else a message box). |
| `app.rs` | winit `ApplicationHandler`: one window, one `Session`. A forwarder thread turns `SessionEvent`s into `EventLoopProxy` user events (`ControlFlow::Wait`, no polling). On `Wakeup` the snapshot is taken immediately (re-arms the next wakeup) and rows are diffed; `RedrawRequested` renders. Keyboard, mouse, wheel, IME, focus, resize, scale factor, title, bell, clipboard, exit (exit code passed on). |
| `input.rs` | `KeyCode` → libghostty `Key`; text/consumed-mods rules; which keys the IME owns during a composition (`ime_owns_key`: not Ctrl/Super shortcuts); mouse buttons; wheel notch accumulator; paste shortcut. Unit-tested without winit events. |
| `geom.rs` | Window pixels ↔ grid; `Layout::origin` (padding, balance) shared by renderer and input mapping. |
| `damage.rs` | Per-row dirty flags from two snapshots (no core API change was needed). |
| `font.rs` | fontdb (no fontconfig; `memmap` feature: the start-up scan and the faces map files, one map per file via `make_shared_face_data`, nothing read into the heap); primary/bold/italic, fallback lists, once-per-character scan of all faces; colour faces (CBDT/sbix/COLR/SVG) only when no monochrome face has a text-presentation character, first for emoji presentation (`pick`). |
| `emoji.rs` | `Presentation::of(cell text)`: VS16/VS15, else `Emoji_Presentation` (Unicode 16.0 table, 80 ranges). |
| `render/mod.rs` | `Renderer` trait (`cell_metrics`, `set_font_px`, `set_padding`, `resize`, `render(&Scene)`, `memory()` for the exit log line), `Scene`, `Preedit`. |
| `render/paint.rs` | Window-independent painter: rows → pixels; colours, attributes, wide cells, cursor shapes, preedit; glyph cache keyed by (face, glyph, size, synthetic bold/italic); glyphs wider than their cells scaled to fit; fallback CJK faces scaled to fill wide cells (§5). Pixel tests. |
| `render/boxdraw.rs` | U+2500–U+259F drawn procedurally. Pixel tests. |
| `render/soft.rs` | softbuffer backend: own canvas = back buffer (always the complete frame); bands changed during `buffer.age()` frames are copied (history of 4), everything when the age is 0 or older; only the current bands are presented as damage; `HAETTY_DUMP_FRAME`, `HAETTY_DEBUG_PADDING`. |
| `clipboard.rs` | copypasta: Win32; Wayland via the window's `wl_display`; X11 via x11rb. |

Key decisions made while building it (in addition to §5):

- Paste: Ctrl+Shift+V and Shift+Insert everywhere, plus Ctrl+V on Windows
  (Windows Terminal convention). On Linux plain Ctrl+V goes to the
  application (Claude Code pastes images with it). No own copy shortcut yet:
  copying is the application's job via OSC 52 (Claude Code fullscreen
  selection).
- Wheel: one wheel event per notch when the application tracks the mouse,
  three lines of scrollback otherwise.
- Wayland decorations: winit `wayland-csd-adwaita` (title drawn on GNOME).
  Measured on Linux: adwaita 3.27 MB, adwaita-notitle 3.10 MB, none 2.74 MB
  (window-only build).
- No cursor blink (keeps idle CPU at 0; Claude Code draws its own caret).
- Padding (as Ghostty): `window-padding-balance` default **true** (Ghostty's
  default is false; chosen because uneven right/bottom margins were the
  visible problem). `window-padding-color` default `extend`: left/right
  padding takes the edge cell's background; top/bottom the first/last
  row's, except on the primary screen when that row has a default-background
  cell or a powerline glyph (Ghostty's third case, prompt rows, needs shell
  integration — not done). Full frames and resizes paint every pixel.
- Chrome: Windows 11 caption/border = terminal background, caption text =
  foreground; the theme request follows the background's luminance (dark
  for the default background). `decorations = integrated` is still not
  implemented (milestone 2's optional P6) and falls back to `native`.
- `release` profile unchanged (debug info for cargo-bloat); `dist` profile
  added for shipping.

## 3. Verified

Environment of the 2026-11-05 session: Windows 11 host (x86_64, 16 threads)
with WSL2 Ubuntu 24.04 + WSLg (Weston, X11 via XWayland and native
Wayland). Rust 1.99.1, Zig 0.15.2, mingw-w64 (Ubuntu package), Claude Code
2.1.318 in both. Builds and tests ran in WSL; Windows binaries were
cross-built there and run natively on the host.

### Backend (Linux, first verified 2026-11-04, re-run 2026-11-05)

- `cargo test`: 5 unit tests (OSC scanner), 10 end-to-end tests; plus 47
  `haetty-gui` unit/pixel tests (31 at the first GUI commits).
- Query replies: DA1 `\e[?62;1;6;22;52c`, DA2, XTVERSION `haetty(0.1.0)`,
  DECRQM 2026/2004/1006 supported, kitty keyboard query and push, CPR,
  CSI 14/16/18 t, CSI ?996 n, OSC 10/11 (needs default colours set — done).
- Encoding through the probe: Shift+Enter `\e[13;2u` with kitty flags 1,
  Ctrl+C `\e[99;5u`, SGR wheel `\e[<64;x;yM` (press only), click,
  bracketed paste, focus `\e[I`, Hangul text as UTF-8.
- Real Claude Code 2.1.318 (npm package): onboarding screens drawn and
  navigated with arrows; main screen in fullscreen renderer
  (`CLAUDE_CODE_NO_FLICKER=1`): alt screen, SGR mouse tracking, kitty flags 5,
  bracketed paste, focus events; output 24-bit colour (33 `38;2` SGRs, zero
  `38;5`) because `TMUX` is removed from the child environment; Hangul input,
  Shift+Enter second line, Ctrl+C twice exits with 0.
- Selection in Claude Code's fullscreen renderer (headless `drag 5 9 19 9`
  over `!echo selectme-12345` output) → `ClipboardWrite("selectme-12345")`.

How to reach Claude Code's main screen without a login (used for all checks):
an isolated home with a `.claude.json` containing
`hasCompletedOnboarding: true`, the project directory under `projects` with
`hasTrustDialogAccepted: true`, and `customApiKeyResponses.approved`
holding the last 20 characters of `ANTHROPIC_API_KEY`, which is set to any
`sk-ant-...` string (then no "custom API key" dialog appears; without that
entry approve it once with ArrowUp + Enter). If `CLAUDE_CONFIG_DIR` is set,
the file must be `$CLAUDE_CONFIG_DIR/.claude.json`. Strip `CLAUDE_CODE_*` /
`CLAUDECODE` from the environment first (§4). Claude Code's `!command` bash
mode runs locally without the API — useful for scrollable content.

### Windows 11, native (new 2026-11-05)

- `scripts/windows-smoke.ps1` (release and dist builds): `[pty] conpty
  embedded 1.25.260930003 flags=0x8`; PASS cmd echo with `ok-한글`, PASS
  PowerShell input (`42`), PASS resize (`77`). Its two Claude Code checks
  FAIL unless Claude Code is prepared (onboarding/login screen otherwise):
  run with the isolated config below and `ANTHROPIC_API_KEY` set, or use
  the direct `haetty-headless.exe` run in the next item.
- `haetty-gui` unit tests built for `x86_64-pc-windows-gnu` and run natively:
  31 passed — including the Windows-only branches (winit#4746 workaround,
  AltGr text, Ctrl+V paste) and the pixel tests with Windows fonts
  (Malgun Gothic for the wide-character test).
- `haetty-headless.exe` + native Claude Code 2.1.318
  (`%USERPROFILE%\.local\bin\claude.exe`): fullscreen renderer through the
  embedded ConPTY — `alt_screen, mouse_tracking, sgr_mouse,
  bracketed_paste, focus_events: true, kitty_keyboard: 5`; `안녕 hello` +
  Shift+Enter + `second line` shown as two input lines; Ctrl+C twice exits 0.
  So kitty keyboard negotiation does pass through OpenConsole 1.25.
- `haetty.exe` window: Claude Code drawn (Cascadia Mono, synthetic bold,
  title `✳ Claude Code`, fallback symbols from Segoe UI Symbol), typing via
  SendKeys, Shift+Enter, Unicode text `안녕` (VK_PACKET, not the IME),
  Ctrl+V pastes the Windows clipboard, the window closes when the child
  exits and `haetty.exe` returns the child's exit code (3 in the test). No
  console window (GUI subsystem); `OpenConsole.exe` runs as a hidden child.
- Idle with Claude Code idle (30 s): 0.063 s CPU time = 0.2 % of one core;
  working set 46.9 MB, private 12.8 MB, 12 threads.

### `haetty` window on Linux (WSLg, new 2026-11-05)

- X11 backend (`WAYLAND_DISPLAY` unset), keys and mouse from xdotool
  (XTest), frames from `HAETTY_DUMP_FRAME`: Claude Code main screen
  rendered (block-element logo, bold, faint, colours, `─` rules continuous,
  `⚠` from the symbol fallback, block cursor); `hello 안녕` (xdotool keysym
  typing) + Shift+Enter + `second line` → two input lines; `!seq 1 200`
  then wheel up/down moves Claude Code's view (159–183 ↔ bottom); window
  resized to 640×400 → log `grid 70x23`, content reflowed by Claude Code;
  mouse drag over output → Claude Code "copied 14 chars", `xclip -o` reads
  `selectme-12345`; Ctrl+Shift+V pastes `pasted-from-clipboard 한글`.
- Ctrl+C twice in the window: one run logged both presses
  (`key C Press mods=Mods(CTRL)` ×2), then `child exited: code 0`, and the
  window closed with exit 0. Another run did not exit (its log was lost).
  A dedicated 3-run re-test could not run: while the Windows desktop was in
  use, WSLg windows did not get keyboard focus (hollow cursor, no key
  events). Encoding itself is fine (headless Ctrl+C twice always exits).
  Re-tested in 20-run batches: see "Milestone 1 finishing work" below.
- Real IME path: ibus-daemon (XIM, `XMODIFIERS=@im=ibus`) + ibus-hangul,
  keys `g k s r m f` from xdotool → winit `Preedit("ㅎ")`, `("하")`,
  `("한")`, `Commit("한")`, `Preedit("그")`…`Commit("글")`; frame shows the
  composing syllable underlined at the cursor with the caret after it, then
  `한글` in Claude Code's input.
- Native Wayland (WSLg Weston): window opens, `clipboard: wayland`
  (initialised only; copy/paste not exercised there), libwayland-client
  mapped, frame identical to X11. Wheel was checked only with XTest under
  X11 (each click counts twice, §4); IME only under X11/XIM.
- Idle with Claude Code idle (60 s): haetty 0 ticks (Wayland), 2 ticks
  = 0.03 % of one core (X11); Claude Code itself ~0.8 %. RSS ~55 MB
  (RssAnon 25.7 MB, RssFile 25 MB = shared libraries and mapped fonts),
  6 threads; Claude Code ~230 MB.
- Linkage: `readelf -d haetty` NEEDED = libgcc_s, libm, libc, ld-linux only;
  `haetty.exe` imports system DLLs only (KERNEL32, USER32, GDI32, IMM32,
  ADVAPI32, SHELL32, ole32, dwmapi, uxtheme, userenv, ws2_32, ntdll,
  msvcrt, bcryptprimitives, api-ms-win-core-synch).

### Window decorations (2026-11-05)

- The "white band": `HAETTY_DEBUG_PADDING=1` plus captures of the whole
  window from the Windows side (`PrintWindow` on the WSLg RAIL window) show
  the red padding strictly inside a light title bar and ~6 px grey border
  — that frame is WSLg Weston's X11 window manager, not haetty. haetty's
  client area had no light pixels (also before the change). Native
  Wayland windows get no frame under WSLg; native Windows draws its own.
- Balance/colour: frame dumps of a 60x10 grid with blue first/last rows:
  `extend` → top/bottom padding blue, left of a row whose first cell is
  default stays default; `background` → all padding `#1e1e1e`;
  `extend-always` → no heuristics. Windows 11, 921x572 client: red padding
  6/6 px left/right, 5/6 px top/bottom.
- Windows 11 (build 26300), screen capture of the visible frame: edges and
  title bar `#1e1e1e` (before: border `#3b3b3b`/`#353535`, title bar
  `#202020`), title text `#dcdcdc`; a child setting OSC 11/10 to
  `#f2efe6`/`#202830` turns the title bar and text to those colours.
- X11: `_GTK_THEME_VARIANT = "dark"` on the window (before: unset). WSLg's
  Weston WM ignores it (frame stays `#ffffff`/`#cccccc`); needs a GTK-aware
  WM (mutter, KWin) to show.
- `decorations none`: Windows window rect = client rect; X11
  `_MOTIF_WM_HINTS` decorations 0, no frame under WSLg. `integrated`:
  native frame and a warning in the log.
- Not checked: Windows 10 (no machine), the Wayland CSD theme (WSLg shows
  no CSD), live resizing by dragging.

### Milestone 1 finishing work (2026-11-05)

Linux checks ran under a private Xvfb (96 dpi, scale 1, §4) with Claude
Code 2.1.318, an isolated `HOME` and a dummy `sk-ant-` key; Windows checks
natively on Windows 11 with the cross-built binaries. Frame dumps are kept
outside the repository on the dev machine
(`%USERPROFILE%\ccterm-win\m2\dumps`, `…\m2\ctrlc`, `…\m2\mem`).

- **Wide fallback glyphs** (§5): pixel test
  `wide_fallback_hangul_fills_two_cells` (unclipped glyph box: ink ≥ 72 %
  of two cells for 한/글/스/트 and ≥ 60 % for every syllable, never
  narrower than at the font's own size by more than 1 px, left/right
  margins within 1 px, no row outside the cell) passes with DejaVu Sans
  Mono + Noto Sans Mono CJK KR, DejaVu + NanumGothicCoding (Noto hidden,
  `HAETTY_FONT_DIR` with fonts-nanum) and natively on Windows with Cascadia
  Mono + Malgun Gothic, at 11 pt, 14 pt, 16 px and 24 px.
  `primary_wide_hangul_is_not_rescaled` (Noto Sans Mono CJK KR as the
  primary: identical fit to before) and
  `box_drawing_is_unaffected_by_wide_fallback_scaling` pass. Frame dumps
  `hangul-linux-nanum-{11,14}pt-{before,after}` and
  `hangul-windows-malgun-{11,14}pt-{before,after}` (PNG plus 3× zoom).
- **Emoji presentation**: unit tests `defaults_and_variation_selectors`
  (table) and `fallback_order_by_presentation` (`pick`); haetty-core
  `variation_selectors_stay_in_cell_text`. Frame dumps
  `emoji-linux-{emoji,emoji-sym2}-14pt-{before,after}`: with Noto Color
  Emoji and fonts-noto-core, ⏸ was the orange emoji before (Symbols2 was
  not even in the fallback list, §4) and is Noto Sans Symbols2's glyph
  after, ⏸+VS16 stays colour, 😀 turned from DejaVu Sans's outline into the
  colour emoji; with Noto Color Emoji only, ⏸ stays the colour emoji (no
  monochrome font has it — colour beats tofu). Windows
  (`emoji-windows-14pt-*`): ⏸ Segoe UI Symbol, ⏸+VS16 and 😀 Segoe UI
  Emoji (COLR).
- **Ctrl+C twice** (Claude Code idle, press, gap, press; "exited" = the
  window process ended with code 0 within 4 s):

  | Condition | Linux GUI (XTest) | Windows GUI (SendInput) | Linux headless¹ |
  |---|---|---|---|
  | 30 ms, 1.5 s after the title | 20/20 | 20/20 | – |
  | 300 ms, 1.5 s after the title | 19/20, then 20/20 | 20/20 | 20/20 |
  | 700 ms, 1.5 s after the title | 18/20 | 20/20 | 20/20 |
  | 300 / 700 ms, 6 s after the title | 20/20 / 20/20 | – | – |
  | 1500 ms | 0/20 | 0/20 | – |
  | 300 ms, ibus over XIM active | 20/20 | – | – |
  | ibus-hangul composing 한, then 300 ms | 5/5 (before and after the fix) | – | – |

  ¹ headless waited for the footer text (`manual mode on`), drawn after the
  title, then 1.5 s — so it pressed somewhat later than the GUI runs; it is
  not an exact control for the start-up timing. Windows used the build
  before the IME fix and took the foreground with `AttachThreadInput`
  while the desktop was idle.

  In every failing run the log has both presses with the same bytes as in
  passing runs (`key C Press mods=Mods(CTRL) -> [1b 5b 39 39 3b 35 75]`),
  focus was gained, and the key event sequence (Ctrl press, C press, Ctrl
  release, C release) is the same as in passing runs; Claude Code's screen
  showed its 401 "remote managed settings" notice. Not reproduced when
  pressed 6 s after start (GUI 40/40), nor on Windows (60/60). See the
  headless control in "Before milestone 2" for the conclusion. One way haetty
  itself could lose a press was found and fixed: Ctrl+C was held back
  while an IME composition was shown (`input::ime_owns_key`, unit test
  `shortcuts_pass_while_composing`); ibus-hangul commits before it forwards
  Ctrl+C, so it did not show up here — an IME that reports the key first
  would have.
- **Memory** (Claude Code idle, 120x36, 11 pt): Linux `haetty` RssAnon
  24.8 MB (`[heap]` 19.9 MB; anonymous mappings of 1.6, 1.2, 0.8, 0.4 MB =
  see below), RssFile 26.0 MB, RssShmem 2.5 MB (the MIT-SHM
  image), 6 threads; Windows private working set 12.5 MB (private bytes
  14.4 MB, working set 34.9 MB, 12 threads). Exit log line: canvas 1088×620
  2.6 MB, glyph cache 59 entries (4 KB of rasters, 7 KB table), snapshot
  4320 cells 168 KB, fonts mapped file-backed (Linux 113 MB after the
  last-resort scan opened all 99 faces). The rest of the Linux heap is
  freed memory glibc kept: with
  `GLIBC_TUNABLES=glibc.malloc.mmap_threshold=131072` the same run has
  RssAnon 6.6 MB and `[heap]` 0.7 MB, and with the CJK collections in
  `/usr/share/fonts/opentype/noto` hidden RssAnon 6.8 MB and `[heap]`
  3.8 MB — the start-up font scan reads those ~20 MB files into temporary
  buffers, which raises glibc's dynamic mmap threshold, and the freed
  memory stays in the heap. The 1.6/1.2/0.8/0.4 MB anonymous mappings are
  probably libghostty-vt pages (an empty session adds 2.2 MB, below). Per session (`examples/mem.rs`, 120x36, no
  window): empty +2.2 MB; 10 000 lines kept +11.6 MB; the default limit
  keeps 678 lines, +2.9 MB.
- **Subcommands**: unit test `reserved_subcommands_are_errors_until_implemented`;
  `haetty status` → message and exit 2 on Linux; on Windows with stderr
  redirected the message arrives there, exit 2; `haetty -- status` runs a
  program `status` ("not found on PATH").
- **Public repository search**: no user names, user paths, private
  addresses or e-mail addresses in `haetty/` besides the HANDOFF path now
  written `%USERPROFILE%`; the `LICENSE` and `Cargo.toml` name the author
  and repository on purpose.

### Before milestone 2 (2026-11-05, second round)

- **Font scan heap** (fontdb `memmap`, `88a49c9`), Claude Code idle,
  120x36, 11 pt, two runs each, same session: Linux RssAnon 24.8 → 8.5 MB
  (`[heap]` 19.9 → 0.7 MB; RssFile 25.5 → 25.9 MB); Windows private
  working set 12.1–12.5 MB before and after (private bytes 14.0–14.4 MB).
- **Scrollback limit** (`876d8db`): rows of 120 columns kept per byte limit
  (`examples/mem.rs`, 30 000 lines printed): 2 MB 1 328, 5 MB 4 424,
  10 MB 9 068, 11 MB 10 229, 12 MB 11 003, 16 MB 14 873 — about 1.1 KB
  per row; one session with a full 12 MB limit is 12.9 MB anonymous. With
  the default filled (12 000 lines printed before Claude Code started) and
  Claude Code idle: Linux RssAnon 19.2 MB, Windows private working set
  22.6 MB (private bytes 30.2 MB) — under the 25 MB budget.
- **Ctrl+C control** (`6ee18b3`, headless `wait-title`): presses 1.5 s
  after the title showed "Claude Code", Linux, all runs in one session:

  | Path | 300 ms | 700 ms | total misses |
  |---|---|---|---|
  | GUI (Xvfb, XTest), earlier + this round | 19/20, 20/20, 20/20 | 18/20, 19/20 | 4/100 |
  | headless, no focus report | 20/20, 40/40 | 20/20 | 0/80 |
  | headless, focus out/in report 0.3 s before | 19/20, 40/40 | 20/20 | 1/80 |

  None of the differences is significant (Fisher's exact test: GUI vs
  headless without focus reports p ≈ 0.09, GUI vs headless with them
  p ≈ 0.26). Decisive run, headless, alternating, 1.5 s after the title,
  presses 300 ms apart: with a focus out/in report *between* the two
  presses 20/20 exited, without it 20/20 (Claude Code had focus reporting
  on — `focus_events: true` at the title — so the reports reached it).
  Conclusion, item closed: there is no evidence that a GUI-only path in
  haetty causes the miss (haetty sent identical bytes in every failing
  run); Claude Code's state right after start is the likely cause; focus
  reports showed no effect. At the title Claude Code had not yet switched
  to the alternate screen or kitty keyboard (`alt_screen: false,
  kitty_keyboard: 0`), so it is still starting up in that window.
- **VS16 emoji in a narrow cell** (mode 2027 off): `⏸️`, `⚠️` from Noto
  Color Emoji are scaled to the cell width and clipped to it; colour pixels
  at x 0..10 and 1..10 of an 11 px cell, none in the neighbouring cells
  (frame dump `%USERPROFILE%\ccterm-win\m3\dumps\vs16-narrow.png` on the dev machine). `😀` without
  a selector is wide and spans its two cells.

- **Engine comparison redone** with about the same history (DESIGN.md §4
  table): libghostty-vt 11 MB (~10 200 rows) max RSS 31.6 MB, alacritty
  10 000 lines 47.6 MB; the old 20 MB figure was the 680-row condition.

### Sizes (stripped)

| Build | haetty Linux | haetty Windows | headless Linux | headless Windows |
|---|---|---|---|---|
| `release` + strip | 14.44 MB | 10.64 MB | 9.49 MB | 7.34 MB |
| `dist` profile | 5.60 MB | 4.25 MB | 1.81 MB | 3.03 MB |

Window decoration work (`release`+strip / `dist`, Linux and Windows):
padding balance/colour + buffer-age copy +14.4 / +17.4 KB release, +17.2 /
+15.9 KB dist; chrome colours/theme +11.0 / +2.6 KB release, +3.7 / +4.1 KB
dist; `decorations` key < 1 KB. Then release 14.46 / 10.66 MB, dist
5.63 / 4.27 MB.

Milestone 1 finishing work (`dist`, Linux / Windows, bytes): wide fallback
scaling 5 632 456 / 4 278 272 after; emoji presentation (80-range table,
640 bytes of data, plus code) +2 240 / +2 048; Ctrl+C/IME fix, key byte
log and memory log line +12 952 / +9 728. Then release 14.49 / 10.69
MB, dist 5.65 / 4.29 MB.

Before milestone 2 (`dist`, Linux / Windows, bytes): fontdb `memmap`
+736 / −1 536 (memmap2 was already linked); `scrollback-limit` +1 264 /
+1 536. Current: release 14.48 / 10.69 MB, dist 5.65 / 4.29 MB.

Per step (`release` + strip, Linux / Windows): empty winit+softbuffer window
3.27 / 1.41 MB; + session (libghostty-vt, haetty-core) and text stack
13.76 / 10.60 MB — text stack .text per cargo-bloat ~1.0 MiB Linux (skrifa
397 KiB, swash 279, read-fonts 150, ttf-parser 90, zeno 74), ~1.9 MiB Windows
(skrifa 655, swash 576, read-fonts 550, yazi 38, fontdb 32, zeno 21,
memmap2 15); input/IME +35 KB / +20 KB; copypasta +644 KB / +25 KB
(x11rb's pure-Rust connection and smithay-clipboard on Linux).

### Not verified (needs a person or other hardware)

See §6.1.

## 4. Pitfalls already hit (do not repeat)

- **ConPTY flag 0x8**: was `PSEUDOCONSOLE_PASSTHROUGH_MODE` in old
  declarations; in current ConPTY headers it is
  `PSEUDOCONSOLE_GLYPH_WIDTH_GRAPHEMES`. Never send it to the in-box host
  (flags 0 there). The out-of-band host gets only glyph-width flags.
- portable-pty 0.9 hard-codes ConPTY flags and cannot pick the DLL; that is
  why Windows has its own backend.
- The in-box conhost re-renders output and has dropped mouse-mode requests
  (older builds), APC strings and kitty keyboard negotiation. Through the
  embedded OpenConsole 1.25 kitty flags 5 do arrive (verified 2026-11-05).
- `conpty.dll` starts `OpenConsole.exe` from its own directory and silently
  falls back to the system conhost if it is missing: always write both.
- libghostty-vt callbacks run inside `vt_write`; they only queue replies and
  events (`Pending`), flushed after the call. Do not call back into the
  terminal from a callback.
- libghostty-vt does not surface OSC 9 / 777 / 99 notifications or OSC 9;4
  progress through callbacks: `haetty-core/src/osc.rs` scans the stream.
- OSC 10/11 get no reply unless default fg/bg are set on the terminal.
- Wheel: send button 4/5 presses only; a release reads as a second notch.
- Claude Code caps colours at 256 when `TMUX` is set; `Session` removes
  `TMUX`/`TMUX_PANE` by default (caller env wins).
- Claude Code takes several seconds before its first byte on a cold start:
  in scripts use `wait-text` or `wait-idle` with a large max, never a short
  idle first.
- A background grandchild can keep the PTY open after the child exits:
  `Exited` is sent after a 500 ms grace period even without EOF.
- The libghostty-vt crate is pinned to `=0.2.2` (API pre-1.0; crate `main`
  already differs, e.g. `set_terminfo_name`). Upgrade deliberately.

New in the GUI work (2026-11-05):

- **winit#4746 is not fixed in winit 0.30.13** (PR still open). The
  egui-winit workaround is in `app.rs::preedit_cursor` (Windows only: a
  preedit caret `Some((0,0))` = unknown, no caret drawn). Remove it once a
  winit release has the fix.
- **winit 0.30 X11 double-counts wheel clicks from XTest**: buttons 4–7 give
  a `MouseWheel` on press *and* release unless the server flags them as
  pointer-emulated. `xdotool click 4` therefore scrolls two notches. Real
  wheels arrive as XI2 scroll valuators (their emulated button events are
  dropped), so do not "fix" this by halving. Also, `xdotool click --window`
  uses XSendEvent, which winit ignores: use plain XTest (`mousemove --window`
  then `click` without `--window`).
- softbuffer's buffer is not a back buffer (Wayland double-buffers): keep
  the own canvas and copy, checking `buffer.age()`.
- A snapshot with a different grid size must force a full repaint: after
  shrinking, the old last row stayed in what became padding.
- On X11 `request_inner_size` right after creation was ignored: set
  `with_inner_size` in the attributes, computing cell metrics with the
  primary monitor's scale before the window exists.
- smithay-clipboard 0.7.3 depends on smithay-client-toolkit 0.20 while winit
  0.30 uses 0.19 (second sctk + calloop, +89 KB): pinned to `=0.7.2`.
- fontdb 0.24 without its `memmap` feature has no `Source::SharedFile`; the
  GUI maps font files itself (memmap2) and keeps the maps alive.
- swash's `underline_offset` sign is not reliable across fonts: the painter
  uses its absolute value below the baseline.
- Cascadia Mono on Windows 11 is a variable font; fontdb sees only the
  default (regular) instance, so bold/italic are synthesised. Fine for now;
  variable-axis support would need swash variations.
- Linux in WSL has no Noto Sans Symbols 2: some Claude Code symbols (`⏸`)
  had no glyph there; on Windows they come from Segoe UI Symbol.
- **Testing Claude Code from inside a Claude Code session**: the parent's
  `CLAUDE_CODE_*` variables (e.g. `CLAUDE_CODE_CHILD_SESSION`) leak into
  the child ("Transcript saving is off — inherited CLAUDE_CODE_CHILD_SESSION
  marker"). Strip `CLAUDE_CODE_*` and `CLAUDECODE` before test runs.
- With `CLAUDE_CONFIG_DIR` set, Claude Code reads
  `$CLAUDE_CONFIG_DIR/.claude.json`, not `~/.claude.json`. Write that JSON
  with a JSON library: a shell heredoc with Windows paths produced `\U`
  escapes and Claude Code reported the file as corrupted.
- In Claude Code's fullscreen renderer Ctrl+C with an active selection
  copies instead of exiting: a test that drags must not expect Ctrl+C twice
  to exit.
- WSLg windows only get keyboard focus when the Windows desktop allows it:
  with the user active in another Windows app, `xdotool windowfocus` has no
  effect (cursor drawn hollow, no key events). Check the cursor in the
  frame dump before blaming input code.
- Windows automation: `SetForegroundWindow` is refused while the user is
  active elsewhere; check the foreground window's process before
  `SendKeys`, never type blind. Windows PowerShell 5 reads BOM-less UTF-8
  scripts as ANSI (Hangul garbled): use `pwsh` or a BOM.
- WSLg's X11 frame (Weston XWM) is white/grey and ignores
  `_GTK_THEME_VARIANT`: what looks like a white band around the window there
  is not haetty. Check with `HAETTY_DEBUG_PADDING=1` before changing
  padding code.
- DWM colour attributes (caption, border, text: 34–36) are set-only —
  `DwmGetWindowAttribute` returns E_INVALIDARG — and `PrintWindow` does not
  include the DWM border (comes back black). Check them with a real screen
  capture of the visible frame (`DWMWA_EXTENDED_FRAME_BOUNDS`).
- `cargo build --target <triple>` builds into `target/<triple>/`, not
  `target/release/`: after a size measurement with explicit targets, the
  plain `target/release/haetty` may be stale.
- Dev setup on a Windows host with WSL: `wsl.exe -- bash -lc '...'` from Git
  Bash expands `$VARS` on the Windows side and MSYS rewrites `/mnt/...`
  paths; run script files with `MSYS_NO_PATHCONV=1`. The WSL VM shuts down
  when idle and `/tmp` is cleared: keep logs elsewhere. `sctk_adwaita`
  logs an ERROR about the XDG settings portal timing out under WSLg —
  harmless.

New in the milestone 1 finishing work (2026-11-05):

- **libghostty-vt's `max_scrollback` is bytes, not lines**
  (`src/terminal/Screen.zig`: "the amount of scrollback to keep in bytes",
  rounded to whole pages). The old `SessionConfig::scrollback = 10_000`
  kept 678 rows at 120 columns, and the first engine comparison gave
  libghostty-vt 10 000 bytes against alacritty's 10 000 lines. Now
  `scrollback_limit` / `scrollback-limit`, in bytes, default 12 MB.
- glibc keeps freed large buffers in the heap once its dynamic mmap
  threshold has risen: reading ~20 MB font files into memory at start-up
  left 16 MB of anonymous memory behind. Map files (fontdb `memmap`).
- Debian/Ubuntu `fonts-noto-core` 20201225 names Noto Sans Symbols 2
  **"Noto Sans Symbols2"**; a fallback list with only the spaced name
  silently skipped it (both names are listed now). Check family names with
  `fc-scan --format "%{family}\n" FILE`.
- libghostty-vt keeps VS15/VS16 in the cell text; VS16 makes the cell wide
  only with mode 2027 (grapheme clustering). Presentation must come from
  the cell text, not the width.
- A larger font size can give a 1 px narrower glyph after hinting (Malgun
  Gothic 글 at 14.67 vs 15.67 px): pixel tests need 1 px tolerance.
- **Claude Code double press**: the second Ctrl+C must come within Claude
  Code's own window (between 700 and 1500 ms: 1500 ms never exits). With
  the test setup's dummy API key Claude Code also shows "Remote managed
  settings failed to load (authentication rejected (401))" a moment after
  start. A second Ctrl+C is occasionally ignored within the first seconds
  after start (GUI 4/100 at 1.5 s after the title, headless 1/80 and 0/80,
  differences not significant; §3 "Before milestone 2"): most likely
  Claude Code's start-up state; no evidence for a haetty cause. Tests
  that need a reliable exit wait a few seconds (GUI at 6 s: 40/40).
- **Xvfb without sudo** (WSL, `/tmp/.X11-unix` is WSLg's read-only mount,
  Xvfb runs `/usr/bin/xkbcomp` by absolute path): `apt-get download` +
  `dpkg -x` xvfb, libxfont2, x11-xkb-utils, xfonts-base, xserver-common
  into a private root; `unshare -rm` (root in a user namespace) to overlay
  `/usr/bin` with the extracted `xkbcomp` and mount a tmpfs on the X11
  socket directory (and on font directories to hide fonts), then
  `unshare -U --map-user=<uid>` back to the normal user and start Xvfb and
  the test in that one script (background processes do not survive the
  tool call). No window manager: `xdotool windowfocus`, not
  `windowactivate`; set `WINIT_X11_SCALE_FACTOR=1` and `-dpi 96`.
- From a Windows shell, `wsl -- bash -c '…'` goes through the WSL user's
  login shell (zsh here), which expands globs and `$vars` first: run
  script files instead.
- Windows SendInput tests: `SetForegroundWindow` alone is refused while
  another app (here Firefox) holds the foreground, even after minutes of
  user idle; attaching to the foreground thread's input
  (`AttachThreadInput`) first is accepted. Still check the foreground
  process before every send.

## 5. Decisions taken for the GUI (with reasons)

- **Window/input: winit** (0.30 line). IME on Windows, X11 (XIM; needs
  ibus/fcitx with XIM) and Wayland (`zwp_text_input_v3`). Check that the
  winit version includes the fix for winit#4746 (Microsoft Korean IME reported
  the preedit cursor at 0 instead of after the syllable); if not, apply the
  same workaround egui-winit uses. On Wayland sessions run the native Wayland
  backend, not XWayland (Hangul commits were reported lost under XWayland in
  another project). — Applied: 0.30.13 lacks the fix, workaround in place;
  winit picks Wayland when `WAYLAND_DISPLAY` is set.
- **Drawing: CPU via softbuffer** first (Win32, X11, Wayland all tier 1; no
  GPU driver dependency, works in RDP/VMs). Hide it behind a `Renderer`
  trait so an OpenGL backend (glutin + glow) can be added later only if
  profiling shows the need. wgpu is out (weight). — Applied.
- **Glyphs: swash** for rasterization with a glyph cache keyed by
  (font, glyph id, size, style). Font fallback for Hangul/CJK and symbols:
  Windows `Malgun Gothic`, Linux Noto Sans CJK / Nanum via fontconfig-free
  discovery (e.g. `fontdb`), configurable primary monospace font. — Applied.
- **Wide cells from a fallback CJK font fill both cells** (2026-11-05).
  Ghostty (source at the pinned commit) scales fallback *faces*, not
  glyphs: `default_fallback_adjustment = .ic_width`
  (`src/font/Collection.zig` `scaleFactor`, like CSS `font-size-adjust`)
  matches the fallback's ideograph width (advance of 水; else ex height →
  cap height → line height) to the primary's, and a primary without 水 is
  estimated as `min(ascii height, 2 cells)` (`src/font/Metrics.zig`
  `icWidth`), so Ghostty itself can stay under two cells. Per-glyph
  constraints (`src/font/Glyph.zig` `Constraint`; `.fit` only shrinks) are
  applied only to "symbols" and Nerd Font icons
  (`src/renderer/generic.zig` `addGlyph`, `src/renderer/cell.zig`
  `isSymbol`); CJK is not centred. haetty takes the face-level idea but
  targets the full two cells (requirement: Hangul from DejaVu Sans Mono +
  Nanum looked spaced out, "한 글 입 력"): factor = 2 cells / advance of 한
  (else 水), capped so the face's tall Hangul (else ideograph) reference
  glyphs stay inside the cell on the primary baseline, and never below 1
  when the natural size already fits; a single glyph that still overflows
  is shrunk. Hangul syllables and ideographs are centred on their ink,
  everything else (jamo, kana, 。，「」) on its advance, so punctuation
  keeps its designed place. Primary faces (a font with its own two-cell
  Hangul such as D2Coding) and colour faces are untouched; box drawing is
  procedural anyway. Hinting can make a larger size 1 px narrower (Malgun
  Gothic 글 at 14.67 / 15.67 px). Measured ink of 한 / two-cell width,
  before → after, 11 pt and 14 pt at 96 dpi: DejaVu + NanumGothicCoding
  14/18 → 16/18, 18/22 → 20/22; DejaVu + Noto Sans Mono CJK 13/18 →
  15/18, 17/22 → 17/22 (cell height binds); Cascadia + Malgun Gothic
  14/18 → 15/18, 17/22 → 17/22 (height binds).
- Rejected: GPUI (standalone crate documented for macOS/Linux only;
  gpui-libghostty is macOS + Wayland only), egui/iced for the terminal body
  (extra layers, no IME advantage), Electron/Tauri.

## 6. Next tasks

### 6.1 Manual checks (by a person; regrouped 2026-11-05 after the rename)

What automation could not show: real keyboards, IMEs, displays, desktops,
and a logged-in Claude Code (the tests use a fake key, so Claude Code never
answers, edits or asks for permission). "Real account" = Claude Code logged
in. Commands below assume `haetty` is on `PATH` (or run it by its path);
for a build beside your stable one use `HAETTY_INSTANCE=dev haetty …`.

#### A. Linux, no real account needed (a shell, `vim`/`less`, or Claude Code's start screen)

1. **Name and icon.** `haetty install-desktop`, then on GNOME/KDE: the
   launcher shows "haetty" with the new icon (window with an orange bar,
   grey `❯_`); a started window is matched to it in the dock (X11 and
   Wayland session: `WM_CLASS`/`app_id` = `io.github.dhwan0112.haetty`);
   the icon at the smallest sizes (top bar, Alt+Tab, a 16/24 px panel) —
   `❯`, cursor and orange bar still recognisable.
2. **Migration from ccterm** (only if you used ccterm): the first `haetty`
   start shows "settings and tab sets copied from ccterm"; your
   `~/.config/ccterm/config` settings apply; the palette's "Open the
   previous run's tabs" offers your old tabs; the ccterm files are still
   there. With a `ccterm` window still open: the hint says a ccterm daemon
   is running, and that window keeps working.
3. **Old variable names**: `CCTERM_LOG=info haetty` still logs, with one
   line asking to rename it to `HAETTY_LOG`.
4. **Hangul IME**: ibus or fcitx5 under native Wayland
   (`zwp_text_input_v3`) and under X11 (XIM): composition inline at the
   cursor, commit on Space/Enter, Hangul typed into the palette (Ctrl+Shift+P).
5. **Keys and tabs** with a real keyboard: Ctrl+Shift+T/W (W twice),
   Ctrl+Tab / Ctrl+Shift+Tab, Alt+1..9, Ctrl+Shift+P, Ctrl+Shift+V,
   Shift+Insert; the tab bar and hints legible on your display.
6. **Mouse**: a wheel notch = one step in a full-screen program; touchpad
   scrolling smooth enough.
7. **Window**: drag the edge to resize (padding while dragging); HiDPI
   150–200 % on Wayland and moving between monitors of different scale;
   dark title bar on GNOME/KDE X11 (`_GTK_THEME_VARIANT`) and the dark
   adwaita decorations on Wayland.
8. **Clipboard** with a real clipboard manager on X11 and Wayland: copy in
   the terminal, paste elsewhere; paste a large text (bracketed paste).
9. **Fonts**: `--font` with a few families; Hangul next to Latin (size and
   baseline), the symbols ⏸ ⚠ ✔ ● (judged from frame dumps only so far).
10. **Reattach** without Claude Code: open two tabs with shells, `kill -9`
    the haetty window (or let it crash), start `haetty` again from the
    launcher — both shells come back as they were (same history), nothing
    restarted; a second window killed → palette "Attach detached sessions"
    takes its tabs.

25. **Integrated title bar on GNOME** (Wayland session, `decorations`
    unset = auto): one colour from the title row to the bottom (no `#303030`
    bar); the window buttons where GNOME puts them (Ubuntu: minimize,
    maximize, close on the right; change it in Tweaks → Window Titlebars and
    restart haetty); drag empty title space to move, double-click to
    maximize/restore, drag each edge and corner to resize — also at 200 %
    scale; buttons dim, brighter under the pointer, no colour on ×; the
    window's corners and shadow (mutter's, not haetty's: record what you
    see); a right click on the title row does nothing. X11 session: the
    system title bar again (auto = native), dark.

26. **Tab features with a real Claude Code** (stage S2): in a git
    repository a new Claude Code tab is named `repo:branch` until Claude
    Code names the session; `git checkout` elsewhere renames it within a
    few seconds; Ctrl+Shift+R with Korean typed through the IME (fcitx5 /
    ibus) renames the tab and the name survives closing and reopening the
    window; close a Claude Code tab after a few turns, "Reopen a closed tab"
    resumes the same conversation (the earlier messages are there); choose
    it twice quickly — only one tab; "Detach tab", then "Attach detached
    sessions" (also from a second window); Ctrl+Shift+D, `m`, `m` in a real
    repository — the working tree view lists what Claude Code changed with
    Bash too, and returns within a moment.

27. **Orchestrator with a real Claude Code** (stage S3; follow README
    "Orchestrator: tasks and workers" — steps 1–5 and the safety boundary
    are written for exactly this check): palette "New task"
    in a real repository — the worktree appears beside it on
    `task/t1-…`; the worker tab shows Claude Code's folder-trust question
    and the task is submitted only after you answer it (a positional prompt
    with haetty's `--settings` after it must reach Claude Code as the first
    prompt); with your bypass setting on or off, the worker asks or does not
    ask as your own Claude Code does; the final message ends with the report
    and the tab turns `✓` / `✗` / `◇`; "Message the worker" while it asks
    for a permission is refused with a hint, and after the turn it arrives
    as one message; `haetty status --json` shows the report; close the
    tab, "Remove a task's worktree" removes it (and refuses while there are
    uncommitted files); restore the tab set — the worker resumes without
    getting the task again.

28. **Daemon crash with real Claude Code tabs** (stage S4): with two Claude
    Code tabs in a dev instance, end the dev daemon by its PID (`haetty
    status --json` → `daemon_pid`); the window stays with the hint, no
    `claude` process of those tabs is left (`ps` / Task Manager); start haetty
    again — both conversations resume. On Windows also check that no
    `node`/`claude` or `OpenConsole` process of those tabs stays.

29. **Windows GUI end-to-end runs** (T5; need a desktop nobody uses): with
    the computer idle (no input for 2 minutes; do not touch it while they
    run), run `scripts/e2e/windows/tabs.ps1` and
    `scripts/e2e/windows/orchestrator.ps1` (scripts/e2e/README.md,
    Windows) against the candidate's `haetty.exe`; both must end with 0
    failed. They skip by themselves when the desktop is in use.

30. **Soak v2, one hour each, on the candidate's own build** (T4; needed
    for the `haetty-v0.2.0` tag): Linux `HAETTY_TEST_TMP=/tmp/haetty-test
    scripts/e2e/xvfb.sh bash scripts/e2e/soak.sh soak.csv` and Windows
    `scripts/e2e/windows/soak.ps1` (idle desktop for the whole hour, do
    not touch the computer); both print `VERDICT PASS`. Run them one at a
    time or on a machine with enough free memory (this session's runs were
    stopped by the tool for low memory).

#### B. Linux with a real account (Claude Code logged in)

11. **Ctrl+C twice** right after Claude Code starts, a few times (rare
    misses were seen only within the first seconds, §3).
12. **Attention marks**: in a background tab, a permission prompt shows the
    red `?` *already when the dialog appears* (the hook is synchronous; no
    noticeable delay before the dialog); Stop shows the amber dot;
    selecting the tab clears amber; answering clears `?`.
13. **Unfocused window**: when Claude Code finishes or asks, the window gets
    the urgency hint and a desktop notification (`notify-send`; off with
    `notifications = false`), with no session text in it.
14. **Status row and panel**: context %, 5h/7d limits, cost match Claude
    Code's `/status`; "esc to interrupt" while it works; Ctrl+Shift+S panel
    (reset times, subagents with durations, last edit).
15. **Edits and diff**: after an Edit or Write by Claude Code, "N files
    changed · Ctrl+Shift+D"; Ctrl+Shift+D shows exactly that edit (side by
    side at ≥ 110 columns); a multi-MB Write shows no hook error, with and
    without the daemon running.
16. **Permission prompt and the palette**: while Claude Code asks, the
    palette's "Paste" is refused with a hint; Ctrl+Shift+V typed by you
    goes through.
17. **Resume and reattach**: kill the window during a long Claude Code turn
    and start haetty again → the same conversation continues in its tab
    (not restarted, no second `--resume`); after a reboot (daemon gone) the
    tabs reopen and each Claude Code tab resumes its conversation once.

#### C. Windows 11 (and 10 where noted)

18. **Name and icon**: `haetty.exe` in Explorer (file icon, Properties),
    pinned to Start and the taskbar, in Alt+Tab, in the title bar — the new
    icon at every size; the window title "haetty"; `haetty install-desktop`
    says nothing is needed. Pin haetty to the taskbar, then start a copy of
    `haetty.exe` from another folder: its window joins the pinned button
    (same AppUserModelID); a `HAETTY_INSTANCE=dev` window gets its own.
19. **Migration and the old daemon**: with a ccterm installation, the first
    start copies `%APPDATA%\ccterm\config` and the tab sets
    (`%LOCALAPPDATA%\ccterm\tabs`); with `ccterm.exe` still open, haetty runs
    beside it (separate pipes) and says so; the ccterm window keeps working.
20. **Microsoft Korean IME** in `haetty.exe` running Claude Code:
    composition inline and underlined at the cursor, no caret at the start
    of the syllable, Hanja candidate window next to the cursor, commit on
    Space/Enter, Enter right after a syllable submits once; Ctrl+C while
    composing.
21. **Keys, mouse, clipboard, fonts, HiDPI** as in 5–9 (Ctrl+V also pastes
    on Windows); dark title bar on Windows 10 (Windows 11 colours do not
    apply there).
22. **With a real account**: 11–17 on Windows (taskbar flash instead of a
    notification for 13; the permission dialog after the synchronous hook,
    ~70 ms on the test machine).
23. **Status line on a machine without Git Bash** (Claude Code then runs it
    through PowerShell; could not be produced here — Claude Code finds Git
    Bash at its default install location even with Git off `PATH`): with
    haetty in a folder with spaces (e.g. `C:\Program Files\haetty`) or in a
    user folder with a space, the status row at the bottom of a Claude Code
    tab fills (`$ 0.00` …) within a few seconds. The injected command is the
    8.3 short path (checked to run in PowerShell 5.1 and 7 directly).
24. **Theme** (Linux and Windows): IBM 5153 CGA on your display — text
    `#cecece` on `#141414`, colour 3 brown, blue dark; the title bar
    (Windows 11), padding, tab bar and status row in the same greys; the
    waiting mark brown, the question mark red. `theme = haetty classic`
    brings the old look back; a Ghostty theme file in the themes folder
    works by its name (for a `HAETTY_INSTANCE=dev` build the folder is
    `~/.config/haetty-dev/themes`, Windows `%APPDATA%\haetty-dev\themes`);
    `minimum-contrast = 3` makes dark blue text white.
    In Claude Code, `/theme` → "Dark mode (ANSI colors only)": its screen
    then uses the theme's colours.

### 6.2 Known gaps (not in milestone 1's scope)

- Own text selection and copy when the application does not track the
  mouse (plain shell); Shift+drag override while it does.
- Cursor blink; font size zoom shortcuts.
- Integrated decorations on Windows (snap layouts need the caption hit
  test); the title row's window menu (right click).
- `window-padding-color = extend`: prompt-row heuristic needs shell
  integration (OSC 133).
- Glyph shaping (ligatures, ZWJ emoji sequences are drawn as the first
  character).
- `scrollback-limit` takes plain bytes; Ghostty-style suffixes (`10MB`)
  are not parsed.

### 6.3 Later milestones

Done in milestones 2–3 (§6.4). Next: the orchestrator (milestone 4), MCP;
integrated decorations on Windows (Linux done 2026-11-06, stage S1); diff views against HEAD and the work tree; a
closed-tabs list; OpenGL renderer if needed.

### 6.4 Progress log (milestones 2–3)

Phases P0–P6 of the milestone 2+3 work; each entry is written when the
phase passed its gate (both builds, clippy 0 on both, all tests, the
regression list, measurements), then committed, pushed and mirrored.

- **2026-11-05 P0 — design.** DESIGN.md §3 filled: processes and daemon
  lifecycle (§3.1), crates (§3.2), local socket (§3.5), wire format and
  frame protocol (§3.6), roles and the `origin` rule for permission waits
  (§3.7), ids, state directory and event log (§3.8), product name constant
  (§3.9), layout rules from §2 (§3.10: exited tabs stay, the status line row
  is reserved, Ctrl+Shift for app shortcuts, `statusLine` replaced in
  haetty tabs). Serialization chosen by measurement: postcard
  +13.1 / +13.3 KB dist vs a hand-written codec, bincode +21.8 / +18.4 KB.
- **2026-11-05 P1 — daemon and IPC.** New crates `haetty-proto` (product
  constants, paths, postcard wire types and framing, Unix socket / named
  pipe transport with overlapped I/O and a user-only DACL, a client whose
  `RemoteSession` has the shape of `Session`) and `haetty-daemon` (one
  state thread; reader/writer thread per client; per-client baseline
  snapshots, row-diff frames, `NextFrame` backpressure; roles; event log
  `events.log`; exit after 10 s without sessions and clients). The GUI is
  a control client: it starts `haetty daemon` detached when none answers,
  creates its session there (with its environment and working directory),
  and closes it when the program exits or the window is closed; a killed
  GUI leaves the session running. `haetty status [--json | --screen ID]`
  is read-only (observe role). Evidence:
  - `cargo test`: daemon integration (`crates/haetty-daemon/tests/daemon.rs`):
    a client killed while `sh` keeps printing, reattach → session listed,
    seq continues (larger), output went on (+20 lines or more), frames keep
    coming; observe client's `Key`/`Paste`/`Close` and report client's
    `Create` → `Denied`, the program got nothing, control input works; idle
    exit; event log lines. Unit tests: frames, key/mouse round trip, frame
    limits, private dir (0700, symlink and mode refused), event log.
  - Regression list through the daemon (Linux, Xvfb, `reg.sh`, 10/10): probe
    receives `a`, `Z`, Shift+Enter `\e[13;2u`, Ctrl+Shift+V paste
    `\e[200~pasted-text\e[201~`, wheel `\e[<64;`, focus `\e[I`; OSC 52 →
    X clipboard `osc52-ok`; resize 80x20 → 54x17; Ctrl+C twice exits Claude
    Code 3/3 (6 s after start); ibus-hangul preedit 7 events, commit, 한
    received; Hangul two-cell and monochrome-first symbols unchanged (frame
    dump).
  - Windows 11 (native): `status --json` without a daemon →
    `{"daemon_pid":null,"sessions":[]}`; GUI started the daemon (parent =
    GUI); after `Stop-Process -Force` on the GUI the session is still listed
    as running and its output went on (`tick 4` → `tick 10` 5 s later),
    daemon alive; closing the last window → daemon gone after its 10 s idle.
  - Latency key → frame through the daemon (`cargo run --release -p
    haetty-daemon --example latency`, 120x36): Linux median 2.14 ms, p95
    2.59, max 3.73 (300 keys); Windows median 1.65, p95 2.03, max 2.44 (200
    keys, cmd.exe echo). GUI draw of an incremental frame (trace log
    "frame drawn in"): median 0.08 ms. Total well under 16 ms.
  - Idle 30 s with Claude Code idle: Linux GUI 0 ticks, RssAnon 5.5 MB;
    daemon 1 tick, RssAnon 5.2 MB (it now holds the session: engine,
    screen, scrollback), 8 threads. (Corrected in P2: the first P1 figures,
    11.6 MB and 12 threads, were taken from a leftover test daemon.) Windows GUI private working set 8.7 MB,
    0.016 s CPU; daemon 6.1 MB, 0 s.
  - Size (`dist`, stripped): Linux 5 649 648 → 5 901 656 (+252 KB), Windows
    4 290 048 → 4 558 848 (+269 KB). cargo-bloat (release `.text`):
    `haetty_daemon` 114 KiB, `haetty_proto` 97 KiB including the postcard /
    serde code for the message types; the rest is std (Unix sockets,
    process spawning) and the Windows pipe/security code.
- **2026-11-05 P2 — tabs, keys, palette, config file.** Tabs: each tab a
  daemon session; the window grid is the active tab's screen plus haetty's
  own cells (`ui.rs`: tab bar from two tabs on, one cell row, neutral
  greys, active tab on the terminal background; command palette as one
  overlay; hints). Tab names: the title without Claude Code's leading
  state glyph ("✳ ", Braille spinner frames), else the program name; an
  exited tab stays, dimmed with `[exited N]`, until closed (one tab: the
  window ends with its program, as before). Keys (`keymap.rs`, DESIGN.md
  §3.11): Ctrl+Shift+T new tab in the active tab's directory, Ctrl+Shift+W
  close (second press within 3 s; hint over the bottom row), Ctrl+Tab /
  Ctrl+Shift+Tab, Ctrl+PageDown/Up, Alt+1..9, Ctrl+Shift+P palette (type
  to filter, arrows, Enter, Esc; shows each action's shortcut), paste
  chords; the release of a consumed chord is not sent either. Config file
  (`config_dir()/config`, Ghostty-style `key = value`, own parser, bad lines
  logged and skipped; command line wins; `keybind = CHORD=ACTION|unbind`).
  Evidence:
  - Unit tests (gui 54): keymap defaults/rebind/unbind, Claude Code's keys
    left alone; tab names; tab bar greys only; palette filter; composition
    (screen below the bar, cursor moved); config file (comments, quotes,
    keybind, bad lines with numbers).
  - `reg.sh` (Linux, Xvfb) 17 checks: the 10 of P1 plus Ctrl+Shift+T → 2
    sessions; typed keys reach the new tab only; Alt+1 → tab 1 gets the
    input; first Ctrl+Shift+W leaves 2 sessions, the second closes; palette
    "new" + Enter opens a tab; `keybind = ctrl+shift+n=new_tab` from the
    config file works. Frame dumps (dev machine `ccterm-win\reg\p2`):
    tab bar, close hint, palette.
  - Windows 11 (SendInput, `p2win.ps1`): Ctrl+Shift+T → 2 sessions, Alt+1
    action logged, one Ctrl+Shift+W → 2, the second → 1, palette → new tab.
  - Memory and idle CPU, Claude Code idle in every tab (Linux): 1 / 4 / 8
    tabs → GUI RssAnon 5.5 / 6.8 / 8.1 MB, daemon 5.4 / 17.8 / 34.3 MB, 0–1
    tick per 20 s each: ~4.1 MB per session (daemon) + ~0.3 MB per tab
    (GUI). DESIGN.md §3.4 now budgets ≤ 25 MB for GUI + daemon with one
    session and ≤ 16 MB per additional session with a full default
    scrollback.
  - Size (`dist`): Linux 5 901 656 → 5 957 016 (+55 KB), Windows 4 558 848
    → 4 618 752 (+60 KB); no new dependency.
- **2026-11-05 P3 — tab sets, `install-desktop`, icons.** `tabsets.rs`
  (TSV with escapes, atomic write, `last` → `previous` at start, DESIGN.md
  §3.8), palette actions save/open tab sets, config/CLI `restore-tabs`;
  `icon.rs` (placeholder icon in code, own PNG/ICO writer), `install.rs`,
  `build.rs` (Windows icon resource), app id on Wayland/X11 (DESIGN.md
  §3.12). Evidence:
  - Unit tests (gui 58): tab set round trip with tabs/backslashes/Hangul,
    bad lines, rotation; icon PNG/ICO structure and greys only; desktop
    entry quoting; install into `XDG_DATA_HOME`.
  - `reg.sh p3` (Linux, Xvfb) 21/21: the 17 of P2 plus `last` has 2 tabs
    after Ctrl+Shift+T in another folder; `WM_CLASS "haetty", "haetty"`;
    `_NET_WM_ICON` set; restart reopens 2 tabs + the requested one and
    writes `previous`; `--restore-tabs false` opens one; install-desktop
    files. (One of three runs had Ctrl+C twice exit 2/3, then 3/3 twice —
    the Claude Code timing case of §3, not a regression.) The regression
    runs now pass `--restore-tabs false` except in the restore checks.
  - Windows 11 (`p3win.ps1`): one icon resource in the exe, WM_GETICON big
    and small set, `last.tsv` with the cmd tab and its folder, restart →
    2 sessions and `previous.tsv`, `install-desktop` prints that nothing
    is needed.
  - Idle 30 s (Linux, Claude Code idle): GUI 1 tick, RssAnon 6.0 MB; daemon
    1 tick, 5.2 MB, 8 threads; key → frame through the daemon median
    2.04 ms, p95 2.65 ms; draw median 0.04 ms.
  - Size (`dist`): Linux 5 957 016 → 5 984 152 (+27 KB), Windows 4 618 752
    → 4 673 536 (+55 KB; with a 256 px ICO entry it was +318 KB, hence
    16–64 px). No new dependency; `windres` from mingw at build time.
  - Not done: reattaching sessions left by a killed GUI (they keep running
    and show in `status`); per-window sets.
- **2026-11-05 P4a — Claude Code adapter: settings injection, `hook`,
  `statusline`, daemon state.** New crate `haetty-cc` (json, settings,
  report, state, edit, resume); protocol version 2 (DESIGN.md §3.6: any new
  variant bumps it; `Secret` for content); DESIGN.md §3.13. Evidence:
  - Unit tests (cc 10): JSON round trip/escapes/surrogates/depth;
    settings (exec form, sync edit PreToolUse with matcher, async rest,
    status-line quoting, merge with the user's settings, `--settings`
    split); attention ordering (an older event does not clear `Ask`;
    looking does not clear `Ask`; idle does not replace it); edits rebuilt
    from snapshots; credential paths never read; snapshot statuses;
    status-line fields and "no change on refresh"; resume arguments.
  - Daemon integration test (fake `claude` script): the program gets
    `--settings <run dir>/settings-ID.json` (0600) instead of the user's,
    merged (user's `model` kept, their `statusLine` dropped); status-line
    and hook reports reach the GUI client; `Ask` refuses Program input,
    Human input passes; ClearAttention clears `Wait`; Report clients cannot
    send input; reports for sessions without integration are ignored; the
    settings file goes with the session.
  - `haetty hook` timing (Linux): no session env 2.7 ms, no daemon 2.9 ms,
    statusline 2.8 ms, with a daemon 3.2 ms (medians of 30; max 4.3 ms);
    stdin left open: 8 ms without a daemon, 1017 ms with one (deadline);
    stdout and stderr empty, exit 0 every time. Windows: 64 ms median
    against 51–53 ms for `hostname.exe`/`whoami.exe` (process start on this
    machine) — the 50 ms target is not met on Windows; recorded in §3.13.
  - Real Claude Code 2.1.318 (isolated HOME, fake key, Linux under Xvfb):
    no settings error on its screen; SessionStart, statusline (every 3 s),
    UserPromptSubmit, StopFailure reach the daemon (9 report clients in
    15 s); `events.log` gets `attention event=StopFailure attention=Wait`.
    Windows 11 with `claude.exe`: settings file in `%LOCALAPPDATA%`-style
    run dir, hook commands with forward slashes, hooks' parent is
    `claude.exe` (exec form, no shell), status line and SessionStart
    reached the daemon (6 report clients), settings file removed on close.
    Typing a prompt on Windows was skipped (foreground lock).
  - Regression `reg.sh p4a` 21/21 (Ctrl+C twice now with the integration
    on).
  - Size (`dist`): Linux 5 984 152 → 6 214 224 (+230 KB, of which
    `similar` 87 KB), Windows 4 673 536 → 4 877 824 (+204 KB). New
    dependency: `similar` 2 (text feature only).
- **2026-11-05 P4b — attention, status row, status panel, diff view,
  resume.** `app.rs` (per-tab Claude Code state from `Claude` messages,
  attention handling, notifications, layout with status row and panel),
  `ui.rs` (tab marks, `status_row`, `panel`), `diffview.rs`; keys
  Ctrl+Shift+D / Ctrl+Shift+S (+ palette: panel wider/narrower); config
  `notifications`; DESIGN.md §3.13. Evidence:
  - Unit tests (gui 63): marks only on other tabs, amber/red; status row
    fixed-width numbers (a column does not move from 7 % to 42 %), greys
    below 80 %, "esc to interrupt" while busy; panel rows and UTC reset
    time; diff unified and side by side, tabs expanded, scrolling bounds;
    Ctrl+Shift+D bound, Ctrl+D/Ctrl+S left to Claude Code.
  - `e2e2.sh` (Linux, Xvfb, two real Claude Code tabs, fake key): a prompt
    in tab 2 then Alt+1 → log `tab 2 attention None -> Wait (active 1)`,
    amber `●` on tab 2 in the frame; Alt+2 → `Wait -> None`. Fake
    PreToolUse/PostToolUse JSON for an Edit through the real `haetty hook`
    (the synchronous one with its file copy took 5 ms) → status row
    "1 file changed · Ctrl+Shift+D", Ctrl+Shift+D shows `- two` / `+ TWO`
    / `+ and a half` with the header `+2 -1 · by main`; Ctrl+Shift+S panel
    (id, cost, files, last edit, updated). Tab set `last` has both Claude
    Code session ids; after a restart the tab whose transcript exists runs
    `claude --resume bd6a5564-… --settings …`, the other (no prompt sent,
    no transcript) starts a new conversation. Frames:
    `ccterm-win\reg\e2e2\*.png`.
  - Windows 11 (`p4win.ps1` with a frame dump): status row under Claude
    Code (`ctx – $ 0.00`), settings file removed on close.
  - Idle 30 s, Claude Code idle with the integration (status line every
    3 s): GUI 0 ticks, RssAnon 5.9 MB; daemon 1 tick, 5.1 MB, 8 threads —
    an unchanged status line wakes nothing. Key → frame median 1.95 ms.
  - Regression `reg.sh p4b` 21/21 (the frame-dump helper now passes
    `--restore-tabs false`; it had reopened the regression's tab set).
  - Size (`dist`): Linux 6 214 224 → 6 271 928 (+58 KB), Windows
    4 877 824 → 4 926 976 (+49 KB); no new dependency (`similar` comes
    through `haetty-cc`).

- **2026-11-05 P5 (+ requests added during the work) — instances,
  friction log, reference frames, §2.3 checks, budgets, README.**
  - Instances (DESIGN.md §3.9): `HAETTY_INSTANCE=dev` → own socket/pipe,
    runtime/state/config directories, app id, title prefix `[dev] `; the
    daemon passes the instance to session children (hooks). Test
    `crates/haetty-gui/tests/instances.rs`: the real binary as a stable and
    a dev daemon at once — separate sockets and state dirs, each lists only
    its own session, children see their instance, `status` follows the
    variable, separate event logs, killing dev leaves stable serving.
    Windows 11 (`instwin.ps1`): pipes `ccterm-<SID>` and `ccterm-dev-<SID>`,
    two daemon pids, `%LOCALAPPDATA%\haetty` and `ccterm-dev`, stable still
    answers after dev is killed. Protocol version 3 (`Friction`).
  - Friction log (DESIGN.md §3.8): `slow_input`, `latency` summaries,
    `panic`, `error`, `gui_slow_draw`, `gui_panic`. Test
    `friction_reaches_the_event_log`: a no-echo program answering after
    80 ms gives `slow_input key_to_screen_ms≈80`, close writes `latency
    n=1 … over_16ms=1`, the GUI's report is `gui_slow_draw ms=21.5`, a
    Report client's `Friction` is denied, and the pasted marker text is
    not in the log.
  - Reference frames (`crates/haetty-gui/src/reftest.rs`): empty screen,
    Hangul with preedit, four tabs with an attention mark and an exited
    tab, Claude Code tab with status row and panel — rendered offscreen
    like `SoftRenderer` (60x12, 13 pt), compared with committed run-length
    references per OS (`tests/reference/{linux,windows}`, ~140 KB each)
    when the font fingerprint matches (else skipped with a message).
    Reports differing pixels (count, %, bounding box, max delta) and
    writes `target/reftest/<os>/*-actual.png` / `*-diff.png`; fails above
    0.05 % or a channel delta > 48 (a test shows a 1 px glyph move fails).
    `HAETTY_UPDATE_REFERENCE=1` rewrites them — in a separate commit with
    the reason (CLAUDE.md). Linux and Windows (test exe run on Windows
    with `HAETTY_REFERENCE_DIR`): all four identical.
  - DESIGN.md §2.3 checks (`e2e3.sh`, Linux/Xvfb, a fake `claude` that
    draws nothing, fake hook/status-line JSON through the real binary;
    pixel analysis of frame dumps):
    - one Claude Code tab: the only non-background band is the status row
      (rows 327–347 of 348), 0 non-grey pixels → only terminal + status
      line, no accent;
    - two idle tabs: tab bar + status row, 0 non-grey pixels;
    - a permission question, Stop, status-line changes and a subagent
      without input: session grids 80x18 / 80x18 before and after, 0
      resize log lines → no layout change;
    - sensitivity: the red `?` gives 37 non-grey pixels, 96 % context 231.
  - Budgets (DESIGN.md §3.4 table): scrollback filled at 120 columns,
    Linux: 1 tab GUI 6.3 + daemon 14.3 = 20.5 MB (≤ 25); 2nd tab +14.0 MB,
    3rd +13.4 MB (≤ 16). (A first run waited too little for the fill and
    showed 10.4 MB / +17.7 MB.) Idle with Claude Code: GUI 0 ticks, daemon
    1. Hook process on Windows 64 ms > 50 ms target (process start 52 ms).
  - README: usage (starting, tabs and keys, config, tab sets, Claude Code
    integration, desktop, environment, known limits); inconsistencies it
    found fixed (USAGE text, decorations comments, HANDOFF header/§2/§6.2/
    §6.3/§8 notes, notification wording, log levels).
  - CLAUDE.md: working rules (self-hosting with `HAETTY_INSTANCE=dev`,
    protected areas needing review, keep ccx as recovery path, reference
    frame updates as separate commits).
  - Regression `reg.sh p5` 21/21. Size (`dist`): Linux 6 271 928 →
    6 294 088 (+22 KB), Windows 4 926 976 → 4 950 016 (+23 KB).
  - Not done: P6 (integrated decorations, Windows 11 snap layouts) —
    optional, and window chrome is outer design (DESIGN.md §2, now a
    protected area).

- **2026-11-05 Review A (protected area, approved) — permission path.**
  - A1: the client library's input methods (`key`, `mouse`, `wheel`,
    `paste`, `write`) take an `Origin`; no default. The GUI passes `Human`
    only from OS input events (keyboard, IME commit, mouse, wheel, a paste
    shortcut pressed by the person) and `Program` for what its own UI makes
    up (a palette action, e.g. "Paste" chosen in the palette; while
    Claude Code asks, the GUI does not send it and shows a hint). Tab
    restore sends no input. Test: with `Ask` open, `write`/`paste`/`key`/
    `wheel` with `Program` through the client library are each `Denied`
    and do not reach the program; `Human` passes.
  - A2: `Hook` / `StatusLine` only from `Report` clients. Test: Control
    and Observe sending them get `Denied`, the state is unchanged.
  - A4: PermissionRequest keeps the newest ask time (`max`); SessionStart
    and SessionEnd clear `Ask` only when their hook started later. Test
    with shuffled orders (`out_of_order_events_never_clear_a_newer_question`).
  - A5: DESIGN.md §3.7: roles are a cooperative separation between the same
    user's processes, not a security boundary; remote adapters only on a
    client type that cannot send Control messages.

- **2026-11-05 Review A3 + B6 (protected area, approved) — synchronous
  PermissionRequest, 250 ms for synchronous hooks.**
  - PermissionRequest is a synchronous hook (no output, exit 0). Test
    `crates/haetty-gui/tests/permission.rs` runs the real `haetty hook` in
    Claude Code's order with fake JSON (PreToolUseAll → PermissionRequest →
    [dialog] → late PreToolUseAll → PostToolUse) against a daemon and reads
    the daemon state right after each hook process ends: `Ask` is there as
    soon as the PermissionRequest process has exited (before Claude Code
    could show the dialog); the late report does not clear it; PostToolUse
    does. `SessionInfo` now carries `attention`, the Claude Code session id,
    the client's arguments and the number of attached Control clients
    (protocol version 4).
  - Synchronous hooks end within 250 ms (`SYNC_DEADLINE`); the edit
    PreToolUse copies the file within 150 ms or reports without the copy.
    Asynchronous hooks keep 1 s.
  - Synchronous hook process time with a daemon: Linux median 4.4 ms, max
    5.1 ms (n=10, in the test); Windows 11 PermissionRequest median 69.4 ms,
    p90 88.0, max 92.3; PreToolUse median 76.1 ms (n=30) against
    `hostname.exe` 56.7 ms on the same run (`synctime.ps1`).

- **2026-11-05 Review C8 + D — session reattach, docs.**
  - At start the GUI reattaches the daemon's running sessions that no
    window shows, closes exited orphans, then restores only the saved tabs
    the daemon does not have; a running conversation is never resumed
    again; palette "Attach detached sessions" (DESIGN.md §3.8).
  - Unit test `only_tabs_missing_from_the_daemon_are_restored`.
  - Regression section 9 (`reg.sh`, a fake `claude` logging every start
    with its arguments; 27/27 with the rest): tab set `last` holds both
    conversation ids; `kill -9` of the GUI → restart without a program:
    2 sessions, 2 attached, 2 Claude starts in total (none new), no
    `--resume`; restart with the program: 3 attached, 3 starts, no
    `--resume`; GUI and daemon killed → restart: 3 tabs, `--resume
    aaaa-1111` and `--resume bbbb-2222` exactly once each; a window killed
    beside a running one → palette "Attach detached sessions" → 4 attached.
    The regression's `stop()` now also ends the daemon (sections must not
    reattach each other's sessions).
  - Real Claude Code (`e2e5.sh`): a prompt, `kill -9` of the GUI, restart
    without a program → the same Claude Code process (pid unchanged)
    attached again, no `--resume` in its command line.
  - D9: DESIGN.md §3.4 hook budget is now Claude Code's wait for a
    synchronous hook (≤ 250 ms enforced; measured Linux 4.4 ms, Windows
    69–76 ms median). D10: README — haetty does not change Claude Code's
    permission mode; the user's settings apply unchanged.

- **2026-11-05 Review B7 — large hook input without a daemon: no change
  needed.** Real Claude Code 2.1.318 (isolated HOME, fake key) driven by
  `haetty-headless`; a synchronous exec-form UserPromptSubmit hook (the
  event whose input carries the whole prompt) with pasted prompts of
  2 bytes, 64 KB, 1 MB and 4 MB; three hooks compared: one reading all of
  stdin, one exiting without reading, `haetty hook` with no daemon.
  - Claude Code passes hook input over a **socket** (not a pipe), written
    with `MSG_DONTWAIT|MSG_NOSIGNAL`; ~214 KB fit in its buffer. From 1 MB
    on, a hook that exits without reading makes Claude Code's write fail
    with `EPIPE` (strace: 1 EPIPE in every 1 MB / 4 MB run of `haetty
    hook` and of the plain no-read hook; none for the reading hook).
  - Claude Code ignores it: no line on the screen (also in the ctrl+o
    detail view), `--debug-file` logs `finished with status 0` for the hook
    and no extra error/warning line compared with the reading hook at the
    same size, nothing in the transcript. The prompt goes through.
  - `haetty hook` with 4 MB of input and no daemon exits in 2.4–7.5 ms
    (pipe, socket pair, file, slow writer: ~10 ms), exit 0.
  - Conclusion: exiting without draining stdin causes no hook error, so the
    "drain stdin with a time limit" fix is not made (it would only add
    work to a hook Claude Code may wait for). Not covered: a multi-MB
    PreToolUse (Write) payload — no model with a fake key; it goes through
    the same hook-spawning path. Scripts and results: session notes
    `scratchpad\b7\` (`drive2.sh`, `matrix3.sh`, `summary3.txt`,
    `errdiff.txt`).

- **2026-11-05 Rename to haetty (user-visible names).** Executables
  `haetty` / `haetty-headless`; app id `io.github.dhwan0112.haetty`
  (Wayland `app_id`, X11 `WM_CLASS`, desktop file, hicolor icon name);
  directories, socket and pipe `haetty` (instances `haetty-dev`, app id
  `….haetty.dev`); window title, usage, messages, `TERM_PROGRAM` and
  XTVERSION; environment `HAETTY_*` with the old `CCTERM_*` read as a
  fallback and announced once in the log; the hook and status-line
  commands injected into Claude Code tabs use the running executable, now
  `haetty` (`HAETTY_SESSION_ID`, `HAETTY_INSTANCE` in the children).
  Migration (`migrate.rs`, GUI and daemon start): the old config file and
  tab sets are copied when haetty has none; a running `ccterm daemon` is
  reported and left alone (DESIGN.md §3.9). Crates and the repository
  directory keep `ccterm` until their own commit. Evidence:
  - `crates/haetty-gui/tests/migrate.rs` (real binary): old config and
    `last.tsv`/`saved.tsv` copied byte-identical, originals kept; daemon
    log says so; an "old daemon" listening at `rt/ccterm/daemon.sock` with a
    session keeps its session and answers, the new daemon has its own; a
    second start does not overwrite a changed haetty config;
    `CCTERM_SOCKET` still reaches a daemon and is announced exactly once.
  - Instances test updated (`haetty-dev`, `io.github.dhwan0112.haetty.dev`).

- **2026-11-05 Final icon.** The user's design (`assets/icon/haetty.svg`,
  metadata block removed, drawing byte-identical) replaces the placeholder
  drawn in code; generated files committed (`assets/icon/README.md`:
  resvg 0.49.0, Python 3.12 standard library). `icon.rs` now only embeds
  them; `build.rs` links the committed `.ico`. Evidence:
  - Unit test `committed_icon_files_are_what_they_claim`: hicolor PNG
    sizes and RGBA, the SVG's drawing elements, the `.ico`'s 8 sizes with
    PNG entries, the RGBA window icon.
  - Small sizes, magnified: `assets/icon/check/haetty-16-rendered-x16.png`,
    `haetty-24-rendered-x12.png` (also 32 ×8, 48 ×6): `❯`, cursor and bar
    keep their shape; no pixel edits. Seen at 16 px: the 1 px `❯` comes out
    lighter-than-body grey (#84807C) on the orange-tinted corner, the bar's
    right third fades to dark brown, the window edges are soft
    (part-pixels), the 8 % outline is invisible below 64 px — inherent to
    the design at that size, reported, not changed.
  - Windows 11 (`iconwin.ps1`): one icon resource; the resource loaded at
    16, 24, 48, 256 px and Explorer's 32 px icon show the design; the
    window's small (16 px) and big (32 px) icons are set from it.
  - Linux: regression `install-desktop files` and `_NET_WM_ICON set`.

- **2026-11-05 Rename checks.** Crates `haetty-*` and directory
  `haetty/` (commit of their own), leftover "ccterm:" message prefixes
  fixed (separate commit). Old and new side by side with the real ccterm
  binary saved before the rename (`coexist.sh`, `coexist.ps1`, isolated
  directories): Linux — sockets `rt/ccterm` and `rt/haetty`, each daemon
  lists only its own session, the haetty window copied the old config and
  `last.tsv` and logged the old daemon (the GUI and the daemon it starts
  each say it once), the ccterm daemon keeps its session after haetty
  ends; Windows — pipes `ccterm-<SID>` and `haetty-<SID>`, two daemon
  pids, `%APPDATA%\haetty\config` copied, same log lines, ccterm still
  answers after haetty's daemon is killed. `.desktop` after
  `install-desktop`: `Icon=io.github.dhwan0112.haetty`,
  `StartupWMClass=io.github.dhwan0112.haetty`, 8 hicolor sizes + SVG
  installed. Regression 27/27 under the new names. Size (`dist`, after
  the reviews and the rename): Linux 6 346 136 (rename) → 6 402 544
  (icon: +56 KB), Windows 4 992 000 → 4 974 592 (−17 KB).
  - Session pitfall: a `wsl -- bash -c '… $(mktemp -d) …'` from Git Bash
    gave an empty variable, so a test `install-desktop` wrote into the real
    `~/.local/share` (10 new haetty files, removed right after, nothing
    else touched). Put such commands in a script file run by `wsl bash`.

- **2026-11-06 Test isolation in code (after the stray `install-desktop`
  of the rename).** New dev-only crate `haetty-testkit`: a run directory
  `<base>/haetty-test-<pid>-<time>` (base `HAETTY_TEST_TMP`, else `/tmp` on
  Unix and `%PUBLIC%\haetty-test` on Windows), `run_dir`, `check`,
  `remove_dir_all`; refuses an empty base and any path that is, or is
  inside, the real home, `~/.config`, `~/.local`, `~/.claude`, the XDG dirs,
  `%APPDATA%`, `%LOCALAPPDATA%` (real = taken before a test changes `HOME`),
  and anything outside the run directory, before writing. Every test that
  writes files uses it (proto paths; cc snapshot, resume; daemon event log
  and integration tests; gui install, tab sets, reftest output, instances,
  migrate, permission; the latency example). Scripts:
  `scripts/test-guard.sh` (Linux/WSL/Git Bash; in WSL the Windows profile
  is protected too; Git Bash compares Windows paths — `cygpath -u` maps the
  Windows temp folder to `/tmp` and would have hidden `%LOCALAPPDATA%`) and
  `scripts/test-guard.ps1`; `windows-smoke.ps1` uses it and now runs Claude
  Code only with an isolated home and a dummy key in the run directory
  (before: the user's real profile). Evidence:
  - Rust: `HAETTY_TEST_TMP=` (empty), `$HOME/x`, `~/.local/share/x` → the
    writing test panics with "test isolation: …", nothing created; unset →
    all pass (testkit's own test: real home, `.local`, `.claude`, empty,
    `..` escape, a base inside the home refused).
  - `test-guard.sh` 12 cases each on WSL and Git Bash, `test-guard.ps1` 12
    cases: unset/empty base, base = home, under `~/.local`, `~/.claude`,
    `%TEMP%`/`%APPDATA%` (Windows), `/mnt/c/Users/<you>` (WSL) → refused;
    a write inside the run allowed; writes to `~/.local/share`, `%APPDATA%`,
    outside the run, `..` escapes, empty paths → refused; a test `HOME` set
    after loading does not move the protection. Nothing left behind.
  - The harness (`xns.sh`, `reg.sh`, `cchome.sh`, `meas.sh`, …, kept with
    the session notes) writes only in `$HAETTY_RUN`; with the base unset,
    empty or under `$HOME` it exits 3 before writing; with
    `HAETTY_TEST_TMP=/tmp/haetty-test` the regression passes 27/27.
  - `windows-smoke.ps1`: empty base or one in `%LOCALAPPDATA%` → refused;
    `%PUBLIC%\haetty-test` → 7/7 PASS including Claude Code, nothing in
    the real `~\.claude\projects` for the run's project. (Found on the way:
    Claude Code on Windows keys trusted projects with forward slashes; the
    script writes both forms.)
  - Note: PowerShell may refuse to dot-source a script from
    `\\wsl.localhost\…` inside a `-File` run; copy `test-guard.ps1` locally.
  - 2026-11-06 follow-up: a relative base is refused too (all three guards).
    An earlier guard check had passed a Windows path whose backslashes the
    shell ate (`C:UsersdennyAppDataLocalTempx`): a relative path, which the
    guard resolved against the current folder — the repository — and an
    empty `haetty-run.*` folder appeared in `haetty/` (removed, never
    committed). Now `haetty-test`, `C:Users…`, `./x`, `x/../y` (and in WSL
    `C:/…`) are refused before anything is created: `test-guard.sh` on WSL
    and Git Bash, `test-guard.ps1` (`IsPathFullyQualified`), and
    `haetty_testkit` (`base_refusal`, unit-tested); nothing appeared in the
    scratch folder the checks ran from.
- **2026-11-06 Windows status line with spaces in paths (protected area,
  approved: the injected settings).** Claude Code's docs (status line
  guide, settings reference) show no shell-free form for `statusLine`
  (unlike hooks' `command` + `args`). So the status line command now uses a
  path no shell mangles: unchanged when plain, else the 8.3 short path,
  else a refreshed copy in `%ProgramData%\haetty\<dir>-<SID>`; quoted
  only as a last resort (DESIGN.md §3.13). Evidence:
  - Real Claude Code 2.1.318 (dummy key, isolated profile in the test run
    directory, `statusline-matrix.ps1`): haetty installed in
    `…\Program Files X\haetty\` and in a user folder `…\Users\Kim Test\…`,
    each with Git on `PATH` and with Git removed from `PATH` — injected
    command `C:/Users/Public/HAETTY~1/…/PROGRA~1/haetty/haetty.exe
    statusline` / `…/Users/KIMTES~1/…`, 6 report connections in 16 s in
    every case (SessionStart + the status line every 3 s), the frame dump's
    status row shows `$ 0.00` (cost comes only from the status line). Claude
    Code's debug log: `Using bash path: "C:\Program Files\Git\bin\bash.exe"`
    in all four, also with Git off `PATH` — a PowerShell-only machine could
    not be made here (manual check 23).
  - The command run directly (`statusline-shells.ps1`): the new form
    reaches the daemon from Git Bash, Windows PowerShell 5.1 and
    PowerShell 7; the old quoted form from Git Bash only (both PowerShells:
    parser error).
  - Windows unit test `windows_status_line_path_without_spaces`: short path
    plain; the copy fallback (with `ProgramData` pointed into the test run
    directory) gives a plain path and is refreshed after the executable
    changes.

- **2026-11-06 Windows AppUserModelID.** The GUI sets
  `SetCurrentProcessExplicitAppUserModelID(app id)` before creating windows
  (DESIGN.md §3.12). Evidence (`aumid.ps1`, `aumid2.ps1`, test-guarded):
  the GUI log reads `AppUserModelID io.github.dhwan0112.haetty` for copies
  in `…\copy one\` and `…\copy two folder\`, and `….haetty.dev` for a dev
  instance; with all three open, the taskbar (UI Automation, Windows'
  "always combine") has two buttons: "haetty.exe - 2 running windows" (the
  two folders together) and "haetty.exe - 1 running window" (dev). Pinning
  is not scriptable: manual check 18. (The button says "haetty.exe": the
  executable has no version resource with a file description.)

- **2026-11-06 Size after the renames** (`dist`, stripped): Linux
  6 402 872 B, Windows 4 981 760 B. Against the icon commit (6 402 544 /
  4 974 592): +328 B / +7 168 B — the crate and directory rename changed
  nothing; the Windows growth is the status-line short-path/copy code and
  the AppUserModelID call. DESIGN.md §3.4 updated; both far inside the
  12 / 10 MB budget.

- **2026-11-06 Themes (infrastructure; the default stays "haetty
  classic").** `haetty_core::theme` (Ghostty theme files, shared `key =
  value` parser, built-ins embedded from `assets/themes/`: the two IBM 5153
  CGA files copied byte-identical — sha256 `6e59cd9c…` / `3bc41442…`, as in
  the source zip — and "haetty classic"), config keys `theme`, `palette`,
  `background`, `foreground`, `cursor-color`, `cursor-text`,
  `selection-background`, `selection-foreground`; themes folder
  `<config dir>/themes`; `haetty-headless --theme`; palette and cursor
  colour into the engine and over the wire (`CreateSpec`, protocol 5);
  cursor-text and selection colours in the painter; `THIRD-PARTY-NOTICES.md`
  (sources, MIT text) and the README. DESIGN.md §3.14. Evidence:
  - `haetty-core` test `theme_answers_colour_queries_and_colours_cells`
    (real `sh` in a PTY, IBM 5153 CGA): replies `OSC 4;1 rgb:d0d0/3333/3333`,
    `4;9 dcdc/4e4e/4e4e`, `4;3 d0d0/8c8c/1b1b`, `4;16 0000/0000/0000`,
    `4;196 ffff/0000/0000`, `4;232 0808/0808/0808`, `4;255 eeee/eeee/eeee`,
    `10 cece/cece/cece`, `11 1414/1414/1414`, `12 cece/cece/cece`; cells
    from SGR 31 / 91 / 38;5;3 are `#d03333` / `#dc4e4e` / `#d08c1b`,
    38;5;16/196/231/232/255 the xterm values.
  - Unit tests: built-in values and the `#4e4e4e` fill-in (only for the
    built-in CGA files; the same text as a user file is used as written);
    "haetty classic" = libghostty-vt's built-in palette + `#dcdcdc` /
    `#1e1e1e` (from the library at run time, not typed in); Ghostty syntax
    (hex index, colour without `#`, X11 name, 3-digit hex), bad lines with
    their numbers, 16–255 refused; lookup order (path, user folder over
    built-in, built-in, unknown → default + message); a `background =` line
    before `theme =` still wins; painter selection colours.
  - The reference frames are unchanged (the default is still "haetty
    classic", whose colours are the old ones).

- **2026-11-06 Default theme IBM 5153 CGA (DESIGN §2 protected area,
  approved).** `DEFAULT_THEME = "IBM 5153 CGA"` (GUI, daemon default for
  sessions without a palette, headless); haetty's state colours come from
  the theme: Wait mark and usage ≥ 80 % = colour 3, Ask mark and usage ≥
  95 % = colour 9, diff tints = colours 2/1 (were fixed amber `#dea532`, red
  `#e25046`, green/red tints). Greys are mixed from the screen's default
  colours as before. DESIGN.md §2.2 and §3.13, README. Evidence:
  - Unit test `ui_colours_follow_the_theme`: for IBM 5153 CGA, its Black
    variant and haetty classic, the tab bar's `●`/`?` and the status row at
    97 % use the theme's colours 3/9 (CGA: `#d08c1b`, `#dc4e4e`); every
    background and all other text are greys (r = g = b); the three themes
    give different UI colours.
  - Core test `answers_startup_queries`: OSC 11 with the default theme is
    `rgb:1414/1414/1414`.
  - The reference frames are still the old ones: their scenes are drawn
    with the colours they were made with (`#dcdcdc` on `#1e1e1e`, amber
    and red) until the next commit makes them with the default theme.

- **2026-11-06 Reference frames in the default theme (DESIGN §2 protected
  area, approved; commit of its own).** The scenes use `Theme::default()`
  (and the painter its cursor/selection colours); frames remade on Linux
  and natively on Windows 11, rerun identical on both. Old vs new (8
  frames): same size; background masks identical (0 pixels differ); every
  old colour maps to new colours at most 2 levels apart (rounding of one
  edge blend over another background — a 1 px glyph move would map one
  colour to colours ~180 apart); largest channel change 14 (greys), 25 (the
  waiting mark). Script: session notes `refdiff.py`.

- **2026-11-06 `minimum-contrast`.** Config/CLI key, 1–21 (clamped, as
  Ghostty), default 1 = off. Implemented as Ghostty's shader
  (`contrasted_color`): below the minimum the text becomes white or black,
  whichever contrasts more with the cell's background; exempt: the text
  under a block cursor, box drawing / block elements / legacy computing /
  Powerline (Ghostty `noMinContrast`). DESIGN.md §3.14. Evidence:
  - Unit tests: WCAG luminance on linearised sRGB; IBM 5153 CGA on
    `#141414`: Blue 1.85, Bright Black 2.21 (asserted), colour 0 1.00;
    selection `#141414` on `#4e4e4e` 2.21, `#cecece` on it 5.29; off at 1,
    white on dark / black on light, graphics exempt; config clamping.
    Table in `assets/themes/README.md` (the test's output).
  - Frame dumps of the real GUI (Linux, Xvfb, dev instance, isolated
    socket/state): `docs/theme/minimum-contrast-{1,3,4.5}.png` — at 3,
    colours 0, 4, 8 are white; at 4.5 also 1, 5, 12 and the faint text;
    the others unchanged; box drawing unchanged.

- **2026-11-06 Theme checks: Windows frame, §2.3, Claude Code's ANSI
  theme, size.**
  - Windows 11 (`chromewin.ps1`, test-guarded, dev instance; screen capture
    of the visible frame; the script made each window topmost and asked
    for no focus, but every newly started window became the foreground
    window by itself):
    title bar, border, left and bottom padding are `#141414` (IBM 5153
    CGA), `#1e1e1e` (haetty classic), `#000000` (CGA Black) and `#203040`
    (`--background "#203040"` on CGA) — each exactly the theme background.
  - DESIGN.md §2.3 re-run with the new default (`e2e3.sh`, Linux/Xvfb,
    frames' most common colour `#141414`): one tab → only the status row
    besides the background, 0 non-grey pixels; two idle tabs → 0 non-grey;
    events → no layout change (grids 80x18 before and after, 0 resizes);
    sensitivity: the `?` mark 37 non-grey pixels, 96 % context 230. 5/5.
    Regression `reg.sh` 27/27.
  - Windows natively: GUI unit tests 69/69 (reference frames identical),
    core unit tests 8/8.
  - Claude Code 2.1.318 (headless, isolated home, dummy key; raw PTY logs
    kept in the test run directory, counts only): `/theme` lists "Dark mode
    (ANSI colors only)" (4 below "Dark mode"); choosing it writes `"theme":
    "dark-ansi"` to `~/.claude/settings.json`. Same screens (start screen,
    `/theme` with its diff preview), SGR colours counted: default theme
    `38;2` 51, `48;2` 12, palette numbers 0; ANSI theme `38;2`/`48;2` 0,
    palette numbers 40 (`37`×21, `94`×6, `91`×4, `92`×3, `93`×3, `96`×1,
    `40`×2). So with that theme Claude Code's screen takes the haetty
    theme's colours (e.g. `37` → `#cecece`, `94` → `#4e4edc`). README says
    how to choose it.
  - The palette really travels through `CreateSpec` and the daemon
    (the default theme alone would hide a dropped palette, since sessions
    fall back to it): Linux frame dumps of the GUI — `--theme "haetty
    classic"` has `#cc6666`/`#b5bd68`/`#81a2be` (classic 1/2/4) 45–46 px
    each and none of CGA's `#d03333`/`#1bd01b`/`#1b1bd0`, the default dump
    the reverse; Windows (`winpal.ps1`, PowerShell DarkRed/Red text through
    ConPTY = SGR 31/91): CGA `#d03333` 151 px / `#dc4e4e` 231 px, classic
    `#cc6666` / `#d54e53` the same counts, `--palette 1=#00ff00 --palette
    9=#ff00ff` exactly those, never another theme's colours.
  - `haetty-headless --theme "IBM 5153 CGA" --trace` (the binary): replies
    `4;1 rgb:d0d0/3333/3333`, `4;196 ffff/0000/0000`, `10 cece/cece/cece`,
    `11 1414/1414/1414`, `12 cece/cece/cece`; `--theme "haetty classic"`:
    `4;1 cccc/6666/6666`, `11 1e1e/1e1e/1e1e`; an unknown name is refused
    with the built-in names.
  - `minimum-contrast` also applies to haetty's own cells (they are cells
    like the program's). With IBM 5153 CGA, on the bar grey `#212121`: `?`
    mark and usage ≥ 95 % (colour 9) 4.02, waiting mark (colour 3) 5.71,
    dim labels `#767676` 3.55 (4.06 on the background), normal `#b0b0b0`
    7.42. So at 3 nothing of haetty's changes; at 4.5 the `?` mark turns
    white (loses its red) and the dim labels turn white (brighter than
    normal text) — against DESIGN §2.2's accent and dimming rules. Not
    changed (Ghostty has no own UI to compare with); exempting haetty's
    cells is a small change if wanted.
  - Size (`dist`, stripped) after the themes and `minimum-contrast`: Linux
    6 402 872 → 6 422 992 (+20 120 B), Windows 4 981 760 → 5 001 216
    (+19 456 B); no new dependency (the theme parser, contrast code, ~1.2
    KB of embedded theme files and libghostty-vt's colour parser).

- **2026-11-06 Self-hosting rules for parallel work (CLAUDE.md).** The
  stable instance and its daemon are never signalled, stopped or
  restarted; dev builds and test processes are managed only by PID or by
  their own socket, never by name; parallel Claude Code sessions each use
  their own worktree and branch and merge into `main` one at a time after
  the gate; protected areas need the same review in every session; only
  the user installs or replaces the stable binary. New helpers
  `haetty_stop_daemon BIN [SIGNAL]` (`test-guard.sh`) and
  `Stop-HaettyDaemon EXE` (`test-guard.ps1`): stop the daemon of the run's
  own socket by the PID from `status --json`, refuse any socket outside
  the run. Checked: a socket outside the run or the user's runtime
  directory → refused (exit 3); with two dev daemons on two sockets of the
  run only the addressed one stops. The session harness (`reg.sh`,
  `e2e*.sh`, `coexist.sh`, …) no longer uses `pkill -f`.

- **2026-11-06 `minimum-contrast` only on the program's cells.** New
  renderer-only `CellFlags::HAETTY_UI` (bit 10, set by `ui::Compose` on
  every cell not taken from the screen: tab bar, status row, panel, diff
  view, palette, hints — the latter two by comparing the grid before and
  after they draw); the painter skips `minimum-contrast` for them. Tests:
  `compose_puts_screen_below_the_tab_bar_and_moves_the_cursor` (tab bar,
  status row, hint cells marked; screen cells not);
  `minimum_contrast_leaves_haettys_own_cells_alone` (real painter, IBM 5153
  CGA, 4.5: the program's blue text turns white; the tab bar's `?` keeps
  colour 9 and the status row its dim grey, no white pixel in either row;
  at 1 the program's text is unchanged). Reference frames unchanged.

- **2026-11-06 Selected text: the theme's colour only if not worse.**
  `paint::colors_with`: a selected cell uses the theme's
  `selection-foreground` only when its contrast on the selection
  background is not lower than the cell's own text colour's (ties: the
  theme's). Unit test with IBM 5153 CGA (`#141414` on the `#4e4e4e` fill-in
  = 2.21): default text `#cecece` (5.29) and white (8.32) keep their own;
  blue (1.19) and colour 8 (1.00) take `#141414`. Rule, formula and the
  16-colour table in `assets/themes/README.md`.

- **2026-11-06 Theme change while running (protected area, approved: a
  Control-only IPC message).** `ClientMsg::SetTheme { session, colors }`
  (protocol version 6) → `SessionHandle::set_colors` (default fg/bg,
  palette 0–15, cursor colour; the engine redraws). The GUI rereads the
  config file when the window gets focus and the file changed, or on the
  new palette action "Reload config"; it applies a changed theme, colour
  key or `minimum-contrast` to its renderer and sends `SetTheme` to every
  tab; reattached tabs get it too. DESIGN.md §3.14, README (incl. Claude
  Code keeping its own light/dark look). Evidence:
  - Core `theme_change_while_running`: text drawn in colour 1 before the
    change turns from classic `#cc6666` to CGA `#d03333`; default colours
    and cursor change; OSC 4;1 and 11 asked afterwards answer with the
    CGA values.
  - Daemon `set_theme_recolours_running_sessions_control_only`: Observe
    and Report sending `SetTheme` → `Denied` ("SetTheme as …"), nothing
    changed; Control → the attached client's frame has the new colours; a
    new Control connection attaching afterwards (a GUI coming back) sees
    them and can change them again.
  - Real GUI, Linux/Xvfb (`themerun.sh`, dev instance, own socket and
    config; 2 tabs, red text = colour 1): start classic (`#1e1e1e`,
    `#cc6666`) → config CGA + palette "Reload config" → both tabs
    `#141414` / `#d03333` → config CGA Black + focus away and back → both
    `#000000` / `#c40000` → window killed (`kill -9` by PID), config
    classic, new window reattaches 2 sessions → all `#1e1e1e` / `#cc6666`.
    8/8 checks.
  - Windows 11 (`winretheme.ps1`, named pipe): a PowerShell tab started in
    CGA Black (`#000000`, DarkRed `#c40000`), window stopped by PID,
    config → classic, a new window reattaches it → `#1e1e1e` / `#cc6666`.

- **2026-11-06 Size after the theme review** (`dist`, stripped): Linux
  6 422 992 → 6 435 840 (+12 848 B), Windows 5 001 216 → 5 012 992
  (+11 776 B) for `minimum-contrast` on the program's cells only, the
  selection-text rule and `SetTheme` / config reload; no new dependency.
  Regression `reg.sh` 27/27 and §2.3 `e2e3.sh` 5/5 with the harness
  stopping dev daemons only through `haetty_stop_daemon`.

- **2026-11-06 Stable installers and `--version`.** `haetty --version`
  prints version, commit, UTC build date and instance (`d414c4b`);
  `install-desktop` writes `Exec=` with the launching link.
  `tools/install-stable.sh` / `.ps1` (DESIGN.md §3.15, README "Installing a
  stable haetty"): build a commit or tag in a temporary worktree (`dist`),
  install read-only into a versioned folder, activate through the link
  (Linux) or junction (Windows), `--list`/`-List`, `--activate`/`-Activate`
  for rollback, `install-desktop`, `--version` check, and print (only) a
  `permissions.deny` / `blockWritesOutsideWorkingDirectories` suggestion
  with the machine's real paths. The Windows script builds in WSL (native
  Windows has no Zig) through `install-stable.sh --build-only`. Self-tests
  `tools/test-install-stable.sh` / `.ps1` (test run directory only; real
  prefix refused before building). Found on the way: `test-guard.sh` under
  `set -u` (the installer's) stopped listing the protected directories at
  the first unset XDG variable — fixed (`${VAR:-}`), checked refusing the
  real home and `~/.local` under `set -euo pipefail` with no XDG variables.
  Also corrected a stale README limit (the status-line path with spaces on
  Windows was fixed earlier). Evidence: see the next entries.

- **2026-11-06 Installer self-tests and a guard fix.**
  - `tools/test-install-stable.sh d414c4b 00c6eee` (Linux, prefix in the
    test run): 18/18 — real prefix refused (exit 3) before building,
    nothing written; version folder and files 0555, no file can be created
    in it; link → the version; `--version` of the link `commit
    d414c4bb61e9 … instance stable`; desktop `Exec="<prefix>/.local/bin/haetty"`;
    deny/blockWrites suggestion with the prefix's paths; second version:
    link moves, first kept, "previous: …" printed; `--list` marks the
    active one; `--activate` rolls back (and `--version` follows); unknown
    version refused; reinstall only activates (no build); no worktree left;
    the real `~/.local/opt/haetty`, `~/.local/bin/haetty`, desktop entry
    unchanged. (`00c6eee` predates the build info: the installer says so
    instead of "check: OK".)
  - `tools/test-install-stable.ps1 d414c4b 00c6eee` (Windows, build in WSL,
    prefix and APPDATA in the test run): 17/17 — the same checks with the
    read-only attribute, the `current` junction, the Start menu shortcut →
    `current\haetty.exe`, user `PATH` and the real
    `%LOCALAPPDATA%\Programs\haetty` untouched.
  - Guard fix found by the Windows test: both guards read `APPDATA`,
    `LOCALAPPDATA`, `HOME`, `XDG_*` live, so a test that points them into
    its run directory made its own run directory "protected" (and a nested
    script would no longer protect the real ones). Now the real values are
    taken when the guard first loads and exported (`HAETTY_REAL_*`).
    Checked: fake `APPDATA` / `XDG_DATA_HOME` in the run → writable; the
    real `APPDATA`, `~/.local/share`, `~/.claude` → refused, also from a
    nested script started with the fake values.

- **2026-11-06 First stable candidate: `4d75fcd`** (not tagged; the user
  decides). Checks on that commit's own build (`haetty --version` →
  `commit 4d75fcdedbe1`): gate (both targets, clippy 0 on both, all tests);
  regression `reg.sh` 27/27 (Linux, Xvfb); DESIGN §2.3 `e2e3.sh` 5/5; theme
  change while running `themerun.sh` 8/8; installer self-tests installing
  it (`d414c4b` → `4d75fcd`, rollback and back) Linux 18/18, Windows 17/17;
  Windows natively: GUI unit tests 70/70 (reference frames identical),
  `windows-smoke.ps1` 0 failures (cmd, PowerShell, resize, Claude Code
  fullscreen modes and multi-line input). Size (`dist`): Linux 6 437 584,
  Windows 5 014 016 (+1 744 / +1 024 B for `--version`). Not covered: the
  manual checks of §6.1 (real keyboards and IMEs, a logged-in Claude
  Code, HiDPI, a machine without Git Bash).

- **2026-11-06 Look from Ubuntu 26.04 GNOME (DESIGN §2 protected area,
  approved).** Tab bar on the terminal background, active tab in the
  foreground colour with a thin rule (`CellFlags::TAB_ACTIVE`, the font's
  stroke) under its share, others dim, no band and no separator; default
  padding 10 / 6 logical px (`window-padding-x` / `-y`; `window-padding`
  sets both; balance unchanged); the status row of a Claude Code tab blank
  until Claude Code reports a value (context, limits, cost, changed files,
  busy), shell tabs without one (already so: their grid is the full area).
  Evidence:
  - Unit tests: tab bar all on the body background, active foreground +
    rule, inactive dim without; status row blank for no report and for an
    empty report; padding per axis (window size, grid, origin, balance).
  - Real GUI frame dumps (`docs/look/`): Linux X11 (Xvfb) shell tab, Claude
    Code tab before and after its first status line, after 20 switches;
    Linux native Wayland (WSLg's compositor, not GNOME — a frame dump is the
    window's content, the same for every backend); Windows 11 Claude Code
    tab before/after the status line beside a cmd tab.
  - Tab switching: shell tab 70x11, Claude Code tab 70x10 (daemon `status`);
    after 20 Alt+1/Alt+2 switches the same, 0 resize log lines.
  - Pitfall (test scripts): in PowerShell, `'…' | & haetty.exe statusline`
    connects but the report never arrives (reproduced 3/3); `cmd /c "type
    f | haetty.exe statusline"`, `… | powershell -Command "<path>
    statusline"`, `… | pwsh -Command …` deliver (state updated). The paths
    Claude Code uses (Git Bash, PowerShell `-Command`) are the delivering
    ones; write test JSON through `cmd /c type`.

- Note on history: `19d2844` (the look) alone fails the gate on the
  reference frame "tabs"; read it together with `5292b74` (the updated
  frames), where the gate passes again. That split followed the old rule
  ("reference frames in a separate commit"); since then an approved §2
  change and its frames go into one commit, and every commit passes the
  gate on its own (CLAUDE.md). The past commits are left as they are.
- **2026-11-06 GNOME title bar, tmux note.** DESIGN.md §3.12: on GNOME
  Wayland the title bar is winit's sctk-adwaita with constant colours
  (winit 0.30.13 maps only dark/light/auto to `FrameConfig`; dark
  `#303030`/`#242424`), so it cannot take the theme background;
  `decorations = integrated`, Linux first, is now the named candidate in
  §6.3 (not implemented). README: a shell that starts tmux by itself puts
  Claude Code inside tmux (256 colours because of `TMUX`; no synchronized
  output — measured with tmux 3.4 through `haetty-headless`: DECRQM 2026
  `\e[?2026;2$y` directly, no reply inside tmux; inside it `TERM=tmux-256color`,
  `TERM_PROGRAM=tmux`), with a `.bashrc` example that skips tmux when
  `TERM_PROGRAM=haetty`.

- **2026-11-06 Stable candidate haetty-v0.1.1: `07e67ee`** (not tagged; the
  user decides; `haetty-v0.1.0` = `4d75fcd`). Contains the GNOME look
  (`19d2844` + `5292b74`), the GNOME/tmux notes (`124d64a`) and the commit
  rule (`07e67ee`). Checks on that commit's own build (`haetty --version` →
  `commit 07e67ee00a6d`): gate passed at `5292b74`, `124d64a` and `07e67ee`;
  regression `reg.sh` 27/27; DESIGN §2.3 `e2e3.sh` 5/5; `themerun.sh` 8/8;
  installer self-tests (`haetty-v0.1.0` → `07e67ee`, rollback and back)
  Linux 18/18, Windows 17/17; Windows natively: GUI unit tests 72/72
  (reference frames identical), `windows-smoke.ps1` 0 failures. Size
  (`dist`): Linux 6 438 512, Windows 5 014 016. Note: the crate version is
  still 0.1.0, so this build calls itself `haetty 0.1.0 (commit
  07e67ee00a6d, …)` and installs as `0.1.0-07e67ee00a6d`.

- **2026-11-06 Stage S0 — version 0.1.1, tag check in the installers.**
  Workspace version 0.1.1 (`47cb67f`). `install-stable.sh` (and through it
  the `.ps1`) refuses a tag `haetty-vX.Y.Z` before building when the tagged
  commit's crate version is not X.Y.Z; hashes, branches and other names as
  before (`21505dc`). Self-tests with the mismatch case (a throwaway tag
  `haetty-v9.9.9` in a clone inside the test run directory / WSL `/tmp`;
  the real repository gets no tag) and per-commit version folders: Linux
  19/19, Windows 18/18 (`haetty-v0.1.0` → `21505dc`, rollback, reinstall).
  The stable-release checks on the merged commit and the tag follow in the
  next entry.

- **2026-11-06 Tag haetty-v0.1.1 = `d5ba833`** (approved in the stage
  instructions). Checks on that commit's own build (`haetty 0.1.1 (commit
  d5ba833db27b …)`): gate; regression 27/27; DESIGN §2.3 5/5; theme run
  8/8; installer self-tests installing it after `haetty-v0.1.0` (and the
  `haetty-v9.9.9` refusal) Linux 19/19, Windows 18/18; Windows natively: GUI
  unit tests 72/72, `windows-smoke.ps1` 0 failures. Annotated tag pushed
  (the tag only). The stable install recorded for the user stays v0.1.0
  until they install this one.

- **2026-11-06 Stage S1 — integrated decorations, Linux (DESIGN §2 change,
  approved).** `decorations = auto|native|integrated|none` (default auto:
  native on Windows and X11, integrated on Wayland; integrated on Windows
  means native); `titlebar.rs`: the title row (tabs, or the window title dim
  with one tab), minimize/maximize/close glyphs per GNOME's `button-layout`
  (`HAETTY_TITLE_BUTTONS` → portal via `dbus-send` → `gsettings` → right),
  drag to move, double-click to maximize, edge/corner resize (6 logical px ×
  scale; off while maximized), buttons act on release, right click nothing.
  DESIGN.md §2.2 / §3.12, README "Title bar". Evidence:
  - Unit tests: GNOME layouts parsed (quotes, left/right, ignored and
    duplicate entries, no colon); layout and hit tests agree with the drawn
    row (gap, tab shares, buttons left); the row's cells (title dim, one
    background, hovered button strong, restore glyph, greys only); edges and
    corners; `effective` for every setting and window system.
  - Reference frames (new scenes, same commit): `integrated-title` (one tab,
    the title, buttons) and `integrated-tabs` (two tabs, the pointer on ×);
    the four existing frames byte-identical; Linux and Windows written and
    re-run identical.
  - Xvfb (`titlerun.sh`, `--decorations integrated`, layout from
    `HAETTY_TITLE_BUTTONS`): 13/13 — integrated chosen; the program keeps
    60x10 (window one row taller); the × glyph dim, foreground under the
    pointer; maximize → `set_maximized(true)`, minimize → `set_minimized`;
    press on the title and release on × closes nothing; press on empty space
    → `drag_window`; double click → maximize toggled; left edge →
    `drag_resize_window West`, corner → `SouthEast`; right click → no action;
    a second tab: still 60x10, no resize; × closes the window. (No window
    manager under Xvfb: the calls are what is checked.)
  - WSLg X11 (Weston's window manager): double click on the title row →
    560x165 → 1920x1032 (maximized), again → 560x165.
  - WSLg Wayland, `decorations` unset: log `Auto on Wayland -> Integrated`,
    buttons from `gsettings` (this machine: `appmenu:close`, only ×).
  - DESIGN §2.3 with integrated (`e2e3.sh --decorations integrated`): one
    tab → the title row and the status row only, 0 non-grey; two tabs idle 0
    non-grey; events → no layout change; 5/5. Default run: regression 27/27,
    §2.3 5/5, theme run 8/8.
  - Frame dumps: `docs/look/linux-x11-integrated-{one-tab,hover-close,two-tabs,buttons-left}.png`,
    `docs/look/linux-wayland-integrated-1-start.png`.
  - Not checkable here: a real GNOME session (mutter's shadow/corners,
    pointer moves and resizes, HiDPI) — §6.1 item 25.

- **2026-11-06 Stage S2 — ccx features that were deferred.** Rename tab
  (Ctrl+Shift+R / palette; own name first, saved in tab sets with a new
  `custom` column, empty = automatic; never typed into Claude Code, not
  logged); `repo:branch` names for Claude Code tabs (`gitview.rs`, from
  `.git` files, after Claude Code's session name); closed-tabs list (set
  `closed`, 20, palette "Reopen a closed tab" with folder/program/resume,
  same rules as tab sets); diff view `m`/Tab cycles last edit → vs HEAD →
  working tree, `r` rereads (git with time/size/file limits); palette
  "Detach tab". DESIGN.md §3.8 / §3.11 / §3.13, README. Evidence:
  - Unit tests (82 + 1 ignored measurement): branch names from real
    repositories (subfolder, branch switch, linked worktree, detached
    HEAD, outside a repository); vs HEAD and tree with a modified, deleted,
    untracked, binary and `.env` file and the file-count and size limits;
    the git views' rows, file headers and reasons; key `m`/Tab/`r`; closed
    list order, cap 20, one entry per conversation, removal; tab-set files
    without the new column; palette modes (rename query, closed filter, a
    long folder cut at the start so the name stays); every action name
    parses back; Ctrl+Shift+R bound, Ctrl+R free. Windows native (the test
    executable): 80 + 1 ignored, the git tests run with Git for Windows
    (`CREATE_NO_WINDOW`).
  - Integration (Linux, Xvfb, `tabsrun.sh`, a fake `claude` logging its
    arguments, isolated HOME and `CLAUDE_CONFIG_DIR`): 20/20 — `proj:feat/x`,
    then `proj:other` after `git checkout` (3 s); session name first; own
    name (with Hangul) saved with custom=1, not in the debug log, kept over
    the session name, empty → automatic; diff view `m` → vs HEAD (3.7 ms, 4
    rows) → working tree (5.5 ms, 8 rows: a.txt, the Bash-made file, `.env`
    "never read") → `r` → last edit, Esc; closed tab kept with folder,
    command, conversation; reopened with `--resume` and removed from the
    list; a second entry for the same conversation right after → not
    resumed again (**defect found and fixed**: the check knew only reported
    conversation ids, and Claude Code reports its id after starting — now a
    live session started with `--resume ID` counts too); detach → 1 tab,
    the session unattached and running; the only tab not detached; attach →
    back.
  - Measurement (release, `measure_tree_view`): 1 500 tracked files, 1 200
    changed + 300 untracked — limit 300: collect 104 ms, diff 43 ms; limit
    1 500: 966 files within 16 MiB, collect 333 ms, diff 72 ms.
  - On 22cd266's own release build: regression 27/27, DESIGN §2.3 5/5, theme
    run 8/8. Size (`dist`): Linux 6 465 920 → 6 539 640 (+73 720), Windows
    5 026 304 → 5 106 176 (+79 872); no new dependency (git is a child
    process; the palette modes, the git views and the closed list are the
    growth).
  - Frames: `docs/look/s2-*.png` (names, rename prompt, the three diff
    views, the closed list).

- **2026-11-06 Stage S3 — orchestrator, first step (DESIGN.md §8, written
  and committed first: 6faa4dd).** One task → one Claude Code worker in a
  worktree (`git worktree add -b task/<id>-<slug>` beside the repository),
  the task as the worker's positional prompt (never typed: start-up dialogs
  stay with the person; the prompt is kept out of tab sets, the closed list
  and re-attach), reports from the Stop hook's `last_assistant_message`
  parsed in the daemon (transcript fallback limited to this turn), states
  running / verified / blocked / needs review / no report, grey state
  glyphs in tab names, a "workers" panel section, `status --json` workers,
  palette "Message the worker" (`haetty_cc::orch::send`: Program origin
  only, only after a finished turn, not busy, no question) and "Remove a
  task's worktree" (no `--force`, branch kept). Protocol 7 (`SessionInfo`:
  busy, turns, report; `ClaudeInfo`: turns, report). Config
  `worker-command`. Evidence:
  - Boundary (cargo integration test `tests/orchestrator.rs`, real daemon,
    real `haetty hook`, a fake `claude` in raw mode recording every byte):
    nothing reported → held; after SessionStart (hooks can run before the
    trust dialog) → held; busy → held; a permission question → held, and
    with a stale "ready" view the orchestrator's paste + Enter and a raw
    Program paste and "y\r" reach nothing (0 bytes); the person's key
    arrives; answered + Stop with a report → one paste + Enter arrives
    whole, control characters dropped; next turn → held, old report gone;
    `orch.rs` has no `Origin::Human`, and no `orch::send` call in `app.rs`
    passes an origin. Passed.
  - Unit tests: grammar (markdown around keys, last STATUS wins, the
    template line and prose are not reports, multi-line fields, 2 000-char
    cap), states (busy wins, verified without evidence → needs review,
    exited → not running), readiness order, transcript fallback (only this
    turn, main chain, only `projects/*.jsonl`), the daemon's Stop/
    UserPromptSubmit handling and that report text never shows in Debug,
    task files, worktree create/refuse/remove (dirty → refused, branch
    kept), `status --json` workers, the WORKERS panel lines. GUI 85 + 1
    ignored, cc 17; Windows native 83 + 1 ignored (git worktrees with Git
    for Windows).
  - GUI end to end (Linux, Xvfb, `orchrun.sh`): 19/19 — the palette's three
    steps make `proj-tasks/t1-fix-the-parser` on `task/t1-fix-the-parser`
    and `tasks/t1.task`; the worker runs there with the prompt and only
    haetty's own `--settings` (no permission flag); `status --json` t1 →
    no report → running (UserPromptSubmit) → verified with evidence (Stop)
    → blocked; tab names `… Fix the parser` / `✓ Fix the parser`; the tab
    set stores no prompt; a question waiting → "Message the worker" does
    not open, 0 bytes; after the turn the message arrives with Enter; no
    title, description or message in the debug log; remove refused while
    the worker is open and with an untracked file, done after, branch kept.
  - Reference frame `workers` (new scene: worker tabs with glyphs, the
    panel's workers section), Linux and Windows, in the same commit; the
    other seven frames byte-identical. Frames: `docs/look/s3-*.png`.
  - On 30f20db's own release build: regression 27/27, DESIGN §2.3 5/5, theme
    run 8/8, S2 run 20/20; `orchrun.sh` then also checks §2.3 with an idle
    worker tab showing ✓: 0 non-grey pixels (20/20). Size (`dist`): Linux
    6 539 640 → 6 625 560 (+85 920), Windows 5 106 176 → 5 185 536
    (+79 360); no new dependency.
  - Fixed on the way: a long palette query now keeps its end in view (the
    branch in "repository and base" was cut off).
  - Protected-area commits (approved for S3): the Stop/UserPromptSubmit
    report handling in `haetty-cc/src/state.rs` and the `SessionInfo`
    fields in `haetty-daemon` (no change to `Daemon::input`'s check, the
    permission-prompt path, `report.rs` or the injected settings).

- **2026-11-06 Stage S4 — robustness.** Four defects found and fixed, each
  with a regression test:
  1. **A query flood froze every tab**: a program that writes terminal
     queries (`ESC [6n`, DA, OSC 11 ?) without reading its input (e.g.
     `cat` of a binary file) filled the PTY's input buffer; the engine
     blocked in `write`, and every `snapshot()` — the daemon's main loop
     calls it — waited for ever. Now a PTY writer thread per session takes
     the bytes in order from a queue; replies are dropped beyond 64 KiB
     waiting, input beyond 64 MiB (error event). Test
     `vtfuzz::query_flood_with_a_child_that_never_reads` (60 000 queries,
     child never reads): hung before, 0.18 s after.
  2. **A huge resize ended the session's engine**: a 65535 x 65535 `Resize`
     from a Control client; sizes are clamped to 4096 x 2048 (engine start
     and every resize). Test `ipcfuzz` (extreme sizes then output).
  3. **A daemon crash lost the tab set and the window**: every tab got
     `Exited`; with one tab the window closed without a word, and the next
     save dropped the exited tabs from `last`, so a restart reopened
     nothing. Now the client marks the connection lost before the
     `Exited`s, the GUI keeps the window with a hint that does not expire,
     freezes `last` and `closed`. Tests `daemoncrash` (cargo: real daemon
     process killed by PID; `is_lost`; child and background grandchild
     gone) and `crashrun.sh` (Xvfb, 9/9: window stays, log line, `last`
     byte-identical, closed list empty, no orphans, hint after 4 s, restart
     reopens both tabs and resumes the conversation). Windows
     (`crash-win.ps1`, the dev GUI, daemon stopped by PID): no orphan —
     both sessions' programs and their console hosts ended — the window
     stays, `last` unchanged, logged.
  4. **`test-guard.ps1` did not work under Windows PowerShell 5.1** (what
     `windows-smoke.ps1` is run with): `[IO.Path]::IsPathFullyQualified`
     does not exist on .NET Framework, so the absolute-path check failed and
     a script without try/catch went on. Now a regex check (drive or UNC).
     New `scripts/test-guard-selftest.ps1`: 8 bases x powershell 5.1 and
     pwsh 7 — old guard 7/8 wrong under 5.1, new 16/16.
  Also: a dropped `Session` stops without working through queued output
  (it waited behind the backlog), and `release` builds libghostty in
  ReleaseFast instead of Zig Debug (found while fuzzing: release 20 000
  lines 21 s and 3.3 M REP characters 50 s, `dist` 0.11 s / 0.16 s; after
  the profile override release 0.11 s / 0.11 s — users of `dist` builds
  were never affected; earlier release-build measurements and the soak
  below ran the slow engine).
  Evidence:
  - IPC fuzz (`crates/haetty-daemon/tests/ipcfuzz.rs`, fixed seed): 915
    connections — length prefixes 0xFFFFFFFF, over 32 MiB, 0, short;
    magic/version mismatches (each answered `Refused`); 300 random first
    frames and frames after Hello; unknown variants 23–255; truncated
    frames; 600 mutated Paste/Write/Resize/Focus/Attach/Detach/GetEdit/
    ListSessions/Sync frames (variant byte kept: no made-up Create or
    Close) as Control and Observe; extreme sizes. After each group a new
    client lists the session and the attached client still gets output;
    9.5 s.
  - VT fuzz (`crates/haetty-core/tests/vtfuzz.rs`, fixed seed, real PTY):
    a generated grammar (CSI with huge and many parameters, OSC/DCS/APC/PM
    strings with random payloads and missing terminators, invalid UTF-8,
    modes, big moves, REP, queries), random bytes, and mutations (flips,
    inserts, deletions, duplications, splices, huge numbers, truncation)
    of a Claude-Code-like stream and of recorded Claude Code output
    (`HAETTY_VT_CORPUS`; `recclaude.sh`: haetty-headless with
    `--raw-log`, isolated HOME and `CLAUDE_CONFIG_DIR`, a fake key, nothing
    submitted, no API call; the key and run path scrubbed; kept outside
    the repository): full run (`HAETTY_VT_FUZZ=full`, release) 48 sessions,
    2.4 MB, no panic, no hang, 0.48 s; the gate's quick run (dev, Zig
    Debug with safety checks) 4 sessions, 1 s. After each stream a reset and
    an end marker show and a resize answers.
  - Soak, 1 hour, 4 tabs (a burst of 500 coloured lines every 10 s, a
    line every 0.2 s, two idle; Linux also switching tabs every 30 s), the
    e2ec468 build (release, so the Zig Debug engine), sampled every minute:
    Linux (Xvfb, 59 samples): GUI anonymous memory 7 168 KB from start to
    end, 16 fds, 7 threads; daemon anonymous 10.6 MB → 30.2 MB at 42 min,
    then flat to the end (scrollback of the two output tabs full), 20 fds,
    20 threads. Windows (the desktop, 61 samples): GUI private 12.4–13.2
    MB, handles 285 → 279, threads 13 → 7; daemon private 15.3 → 46.7 MB at
    40 min, then flat, handles 189–190, threads 23 → 20. No growth in
    handles, fds or threads; memory flat once scrollback is full. CSVs:
    `~/m4/soak/soak-linux.csv`, `~/m4/soak/soak-win.csv` (WSL home).
  - On 9fabc3b's own release build: regression 27/27, DESIGN §2.3 5/5, theme
    run 8/8, S1 title run 13/13, S2 run 20/20, S3 run 20/20, crash run 9/9,
    full VT fuzz 48 sessions 0.39 s; Windows GUI unit tests 83 + 1 ignored.
    Size (`dist`): Linux 6 625 560 → 6 645 256 (+19 696), Windows
    5 185 536 → 5 202 432 (+16 896); `postcard` added as a dev-dependency of
    haetty-daemon only (the IPC fuzz test; not in any binary).

- **2026-11-06 Stage S5 — version 0.2.0, candidate `haetty-v0.2.0` =
  `22dcba9`** (not tagged: the tag waits for approval). Checks on that
  commit's own build (`haetty 0.2.0 (commit 22dcba95021a, built
  2026-11-06, instance stable)`, Linux and Windows):
  - first candidate `e9f5bee` (the version bump) failed a gate re-run:
    two clippy warnings in the S4 fuzz tests (Unix-only test files) that
    the gate script had missed — it ran clippy twice and judged the second
    run, which does not always replay cached warnings — so `9fabc3b` and
    `e9f5bee` were not clippy-clean. Fixed in `22dcba9`; the gate script
    now judges one run after touching the sources, and its test step fails
    on `FAILED` / `panicked` / compile errors (before, only when no test
    ran at all). Every check below was repeated on `22dcba9`.
  - gate (both targets, clippy 0, all tests); regression 27/27; DESIGN §2.3
    5/5, and 5/5 with `--decorations integrated`; theme run 8/8;
  - stage runs: title row 13/13, tab features 20/20, orchestrator 20/20
    (incl. §2.3 with a worker tab), daemon crash 9/9; IPC fuzz 915
    connections; full VT fuzz 48 sessions / 2.4 MB (recorded Claude Code
    output included);
  - installer self-tests installing `haetty-v0.1.1` then `22dcba9`
    (mismatched `haetty-v9.9.9` refused before building, rollback,
    reinstall, real install folders untouched): Linux 19/19, Windows 18/18;
  - Windows natively: GUI unit tests 83 + 1 ignored; `windows-smoke.ps1`
    0 failures (ConPTY 1.25 embedded, cmd, PowerShell input, resize, Claude
    Code fullscreen modes and multi-line input); `test-guard-selftest.ps1`
    0 failures under PowerShell 5.1 and 7;
  - S4's soaks (1 hour, Linux and Windows) ran on `e2ec468`, before S4
    (no per-session PTY writer thread; the Zig Debug engine) and were not
    repeated on the candidate. (Corrected in the next entry, which also
    replaces this candidate.)
  - Size (`dist`): Linux 6 645 264, Windows 5 202 944 (0.1.1: 6 438 512 /
    5 014 016).
  The stable install recorded for the user stays v0.1.0 (they install
  0.1.1 / 0.2.0 themselves).

- **2026-11-06 After S5 — a thread leak found and fixed; candidate
  `haetty-v0.2.0` is now `8d66d29`** (replaces `22dcba9`; not tagged).
  - Checked every code commit of S0–S5 with the corrected gate method
    (sources touched, one clippy run per target, throwaway worktrees):
    `47cb67f`, `21505dc`, `d5ba833` (tagged 0.1.1), `e997d00`, `22cd266`,
    `30f20db`, `22dcba9` clean; `9fabc3b` 4 warning lines (Linux; the two
    test warnings) — so `9fabc3b`, `bad5247`, `174928b`, `e9f5bee` do not
    pass the gate alone. No history rewritten.
  - New `tests/threads.rs` (60 sessions opened and closed in a real daemon
    process): the daemon went 4 → 84 threads. 20 sessions — a child that
    floods queries and never reads — kept their child, PTY reader, child
    waiter, PTY writer and the daemon's event forwarder: S4's writer thread
    sat in a blocked `write`, holding the terminal open, so the child never
    got its hangup (a regression from S4). Fix: at the end of a session
    with bytes still queued the writer stops after its current 4 KiB chunk
    and the engine calls `PtyControl::hangup` (Unix: SIGHUP through
    portable-pty's `ChildKiller`, then `tcflush` on the master — Linux does
    not wake the blocked write when the child's side closes; Windows:
    nothing needed). Now 4 → 4.
  - The same test inside the parallel gate (dev build, Zig Debug engine)
    showed the daemon not answering: its main loop waited for a snapshot
    from a session printing in a loop, queued behind up to 1024 output
    chunks. Frontend commands now have their own unbounded channel read
    first, at most 16 output chunks between looks. Measured (release,
    Xvfb): `haetty status` during a `yes` tab avg 6 ms / max 8 ms, idle 5 /
    7 ms. `cargo test` three times in a row: 0 failures.
  - `libc` is now a direct (Unix) dependency of haetty-pty — already in the
    tree through portable-pty, same version.
  - Checks on `8d66d29`'s own build (`haetty 0.2.0 (commit 8d66d298e46c …)`):
    gate (corrected script); regression 27/27; DESIGN §2.3 5/5 and 5/5
    integrated; theme run 8/8; title 13/13, tab features 20/20,
    orchestrator 20/20, crash 9/9; IPC fuzz 915 connections; full VT fuzz
    48 sessions; `threads` and `daemoncrash` (release) passed; installer
    self-tests (`haetty-v0.1.1` then `8d66d29`) Linux 19/19, Windows 18/18;
    Windows natively: GUI unit tests 83 + 1 ignored, `windows-smoke.ps1` 0
    failures, guard self-test 0 failures, `crash-win.ps1` (no orphans,
    window stays, `last` unchanged, logged). Size (`dist`): Linux
    6 656 016, Windows 5 210 624 (S5: 6 645 264 / 5 202 944).
  - The 1-hour soaks were not repeated on `8d66d29` (they ran on
    `e2ec468`, before S4); DESIGN §3.4 now records them with that note,
    and that `release`-build measurements before S4 ran the Zig Debug
    engine.

- **2026-11-06 T1 — commits that fail the gate on their own; `tools/check-range.sh`.**
  History is not rewritten; these commits stay as they are.

  | Commit | Subject | Failing gate step | Why |
  |---|---|---|---|
  | `9fabc3b` | Robustness: IPC and VT fuzzing, daemon-crash handling, four defects fixed | clippy (Linux, `--all-targets`) | `crates/haetty-daemon/tests/ipcfuzz.rs:109:21` `unusual_byte_groupings` (`0x0dd_b1a5_e5`); `crates/haetty-core/tests/vtfuzz.rs:245:44` `needless_borrows_for_generic_args`. Both files are `#![cfg(unix)]`, so the Windows clippy, fmt, the build and every test pass. |
  | `bad5247` | HANDOFF: stage S4 results on its own build, size | clippy (Linux) | same code as `9fabc3b` (HANDOFF only) |
  | `174928b` | HANDOFF: in progress - stage s5 | clippy (Linux) | same code as `9fabc3b` (HANDOFF only) |
  | `e9f5bee` | Version 0.2.0 | clippy (Linux) | the version bump on top of `9fabc3b`'s code; the same two warnings |

  Fixed in `22dcba9`. How they got through: the gate script of the time
  (session harness, not in the repository) ran clippy twice and judged the
  second run, which does not always replay cached warnings; it also
  failed the test step only when no test ran at all. Fixed in the script,
  and now in the repository's tooling:
  - `tools/check-range.sh RANGE` (and `tools/check-range.ps1`, which runs
    it in WSL): every commit of the range that changes `haetty/` is checked
    out in a temporary worktree (test run directory) and gets fmt, clippy
    for Linux and `x86_64-pc-windows-gnu` (all `.rs` touched first, one run
    per target, judged on its own output) and `cargo test`; a commit with
    the same code as the last one checked (only docs changed) reuses its
    result. One line per commit, exit code = failing commits. CLAUDE.md:
    run it on `origin/main..HEAD` before every push.
  - Run on `haetty-v0.1.1..HEAD` (`a293541`, 20 commits changing
    `haetty/`): **4 failing — exactly the four above**, with the same
    reason; the other 16 pass (`bef5e22` … `be3fcd0`, `22dcba9` …
    `a293541`). `check-range.ps1 9fabc3b^..9fabc3b --no-test` from
    Windows: the same failure, exit code 1.
  - Found on the way: a fresh target directory failed every commit
    (`zig` not on a non-login shell's PATH); the script now adds
    `~/.local/bin` when `zig` is missing and stops with a clear message if
    it is still missing, and it uses the usual target directory.

- **2026-11-06 T2 — the test harness is in the repository.** The gate and
  the end-to-end scripts that lived in a session folder (`~/m4/harness`)
  moved: `scripts/gate.sh` (the full gate), `scripts/dist-sizes.sh`,
  `scripts/e2e/` (`regression.sh`, `design-2-3.sh`, `themes.sh`,
  `titlebar.sh`, `tabs.sh`, `orchestrator.sh`, `daemon-crash.sh`,
  `look.sh`, `dump-sample.sh`, `firehose.sh`, `record-claude.sh`,
  `claude-home.sh` + `mkcfg.py`, `sizes.py`, `nongrey.py`, `lib.sh`,
  `xvfb.sh`, `run-all.sh`, `windows/{daemon-crash,soak}.ps1`,
  `README.md`). Session-only helpers that commit, stage and push
  (`c.sh`, `stage.sh`, `push.sh`) stay outside; they call
  `scripts/gate.sh` and `tools/check-range.sh`.
  - Everything is resolved from the script's own location (`lib.sh`: repo,
    `BIN`, `HL`, probe, guard, Claude Code variables removed); `xvfb.sh`
    uses a system Xvfb, or one unpacked without root (`HAETTY_XROOT`, how
    to unpack it in the README); no `pkill`: the IME daemon is started
    in the foreground and ended by its `$!`, session programs by the PIDs
    `status --json` reports.
  - Personal-data scan of `scripts/`, `tools/`, every crate's `src`,
    `tests`, `examples`, `CLAUDE.md`, `README.md`, `Cargo.toml` (user
    names, home paths, `/mnt/c/Users`, mail, `sk-ant-…` other than the
    all-`A` dummy, GitHub/AWS/Slack token shapes): three hits, fixed —
    `tools/test-install-stable.ps1`'s default repository path with a user
    name (now taken from where the script lives, as `check-range.ps1`),
    and two test strings in `haetty-testkit` (now `someone`). Remaining
    `C:\Users\a\…` strings are test fixtures without a name.
  - From the repository (`scripts/e2e/run-all.sh`, Xvfb from
    `HAETTY_XROOT`): §2.3 5/5 and 5/5 integrated, themes 8/8, title row
    13/13, tab features 20/20, orchestrator 20/20, daemon crash 9/9;
    the regression failed 6 tab checks on the first run — the port had
    replaced `pkill ibus-daemon` by `ibus exit`, which left the daemon
    holding the keyboard for the next sections; with the PID-based stop:
    0 failed, no `ibus-daemon` left.
  - The gate from a fresh clone (`git clone` of `haetty/t2`, `env -i` with
    only `HOME`, `PATH` with cargo and zig, a new target directory, so
    Ghostty fetched and built afresh): first run failed `instances` —
    "path must be shorter than SUN_LEN": the test base
    (`/tmp/haetty-fresh/haetty-run.…/t`) was too long for the sockets the
    tests create under it. `scripts/gate.sh` now refuses a base longer than
    32 characters with that reason (and finds `zig` in `~/.local/bin`).
    Second run (`870b4f4`, base `/tmp/hf-t`): `GATE fail=0`, 302 s. The
    gate uses nothing outside the repository.
  - The VT fuzz corpus stays outside the repository; how to record it
    (isolated `HOME` and `CLAUDE_CONFIG_DIR`, dummy key, nothing
    submitted, scrubbed) is in `scripts/e2e/README.md`. The gate needs no
    corpus: `vtfuzz`'s quick mode mutates a built-in Claude-Code-like
    stream and generated streams.

- **2026-11-06 T3 — every build runs the optimized engine; budgets
  re-measured.** Order note: T5 and T6 come before T4 (T4 must run on the
  final candidate build, and T5/T6 change code).
  - Cause, proven from the zig command line (`scripts/zig-mode.sh`: a `zig`
    shim logs `zig build …` and the build script's `PROFILE`/`DEBUG`/
    `OPT_LEVEL`; a fresh target per profile, the Ghostty checkout reused):

    | Build | Build script env | Zig mode |
    |---|---|---|
    | `release` without the S4 override (Cargo.toml as before S4) | `DEBUG=true OPT_LEVEL=3` | **Debug** |
    | `release` with the S4 override | `DEBUG=false OPT_LEVEL=3` | ReleaseFast |
    | `dev`, `test` before T3 (the gate's `cargo test`) | `DEBUG=true OPT_LEVEL=0` | **Debug** |
    | `dev`, `test` after T3 (`[profile.dev.package.libghostty-vt-sys] debug = false`) | `DEBUG=false OPT_LEVEL=0` | ReleaseFast |
    | `dist` (installers) | `DEBUG=false OPT_LEVEL=s` | ReleaseSmall |

    So yes: `release`'s `debug = 1` made the build script choose Debug; the
    package override fixes it, and now `dev`/`test` too — the gate and every
    measurement run an optimized engine. No measurement-only profile was
    needed: measurements use `dist` (shipped) and `release`.
  - Zig's safety checks are no longer in the gate's tests; instead a full
    VT fuzz with `LIBGHOSTTY_VT_SYS_OPTIMIZE=ReleaseSafe` (48 sessions,
    2.4 MB, recorded Claude Code output included): no safety panic, 0.6 s
    (README "Testing").
  - DESIGN.md §3.4 re-measured with `scripts/e2e/measure.sh` (dist and
    release) and the table replaced, the Debug-engine values kept as
    "before": key → frame median 1.95 → 0.96 ms (dist) / 0.88 (release);
    throughput (plain lines) ~5 KB/s → 24 MiB/s (dist) / 34 (release);
    one filled session GUI + daemon 20.5 → 19.2 MB; each further filled
    session +14.0 → +13.5 / +13.1 MB; one idle Claude Code tab 5.9 + 5.1 →
    5.6 + 1.6 MB; idle CPU 0–1 tick in 30 s. Every budget is met.

- **2026-11-06 T5 — Windows test coverage.**
  - Moved to Windows (one source for both platforms,
    `crates/haetty-gui/tests/common/mod.rs`: the binary from
    `HAETTY_TEST_BIN`, a named pipe instead of a socket, process queries
    through `tasklist` / PowerShell instead of `/proc`):
    - `orchestrator` — the safety boundary on the bytes that reach the
      program: on Windows the fake Claude Code is `powershell.exe` copied
      as `claude.exe`, reading keys with `[Console]::ReadKey` through the
      pseudo console. Passed natively (Windows 11): held before a turn,
      while busy and while asking; no program byte while asking; the
      person's key and one paste + Enter after the turn arrive exactly.
    - `daemoncrash` — `cmd /c ping -n 600 …` as child and grandchild; the
      daemon killed by its PID (TerminateProcess): `is_lost`, `Exited`,
      no orphan (child, grandchild, console host). Passed natively.
    - `threads` — 60 sessions (PowerShell flooding queries without
      reading, `cmd` printing in a loop, `cmd /c exit 0`): the daemon's
      thread count back to its baseline (+3 allowed for Windows' own pool
      threads; a leak is 20+). Passed natively.
    - Found while porting: the test helper did not create the run folder
      the daemon writes Claude Code's settings into on Windows, so the
      integration was silently off — fixed in the helper (the daemon logs
      the failed write; no product change).
  - Not moved (outside T5's three; reason): `instances` (Unix default
    socket and XDG directories by design), `permission` (could be ported
    like `orchestrator`; not asked), `ipcfuzz` (raw Unix socket client;
    a named-pipe client would need `\\.\pipe` opened as a file — possible
    later), `vtfuzz` and the core session tests (sh, `cat`, python3
    helpers), `daemon` tests (sh).
  - Windows GUI end-to-end for S2 and S3: `scripts/e2e/windows/tabs.ps1`
    and `orchestrator.ps1` (+ `lib.ps1`: isolated dev instance, the fake
    Claude Code, keys through SendKeys only while the haetty window is in
    the foreground, hook/status calls as hidden processes — `cmd /c` console
    windows took the foreground and keys went missing). `tabs.ps1` ran 13/14
    (repo:branch names, branch follow, session name first, typed name not
    logged, empty name → automatic, diff vs HEAD and working tree, closed
    tab with its conversation, reopen with `--resume`, never twice, detach,
    the only tab kept, attach); the one failure (rename) was lost keys: the
    foreground was taken by another application the person was using
    (a game, seen as the foreground process). Because SendInput types into
    whatever window is in front, the scripts now SKIP unless the desktop
    had no input for 120 s and stop at once when haetty loses the
    foreground; `orchestrator.ps1` was therefore not run (the desktop was in
    use) — §6.1 item 29.

- **2026-11-06 T6 — the orchestrator explained for people.** README
  "Orchestrator: tasks and workers" replaces the short section: what a
  task and a worker are; before you start (`worker-command`, permission
  mode = your own settings); step 1 making a task (the three questions,
  what haetty creates, refusals, the trust/bypass questions answered by
  you), step 2 the states (table of marks and meanings, never guessed, the
  workers panel), the report format with an example and where to read it
  (`status --json`), step 3 looking at the work, step 4 sending more
  (when it is sent and when it is held, with the hint), step 5 finishing
  (merge yourself, close, remove without `--force`, branch kept), the
  safety boundary, restarting. §6.1 item 27 points to it.
  - Found while writing it: a new tab opened in haetty's start folder, not
    the active tab's, when the program reports no folder (Claude Code does
    not send OSC 7) — so "a shell next to the worker" did not open in the
    worktree. `open_spec` now falls back to the active tab's own folder
    (`Tab::folder`: reported, else where it started). Checked:
    `scripts/e2e/orchestrator.sh` 21/21 with the new check (a new tab next
    to the worker opens in its worktree), `tabs.sh` 20/20.

- **2026-11-06 T4 — soak v2: not completed; T7 — no tag.**
  - Scripts: `scripts/e2e/soak.sh` (Linux, Xvfb) and
    `scripts/e2e/windows/soak.ps1`: four tabs (a burst of 500 lines every
    10 s, a line every 0.2 s, two idle) and, spread over the hour, 200 tab
    open/close, 50 detach/attach, 20 tasks (palette "New task", a worker
    that ends after 3 s, its tab closed, "Remove a task's worktree"), 10
    theme changes (config + "Reload config"); a sample every 300 s between
    operations: threads, fds (Windows: handles), anonymous memory
    (Windows: private working set), the daemon's descendant processes,
    sessions, task worktrees left.
  - Verdict, written in the scripts before any run: threads and fds the
    same at the end as at t=0 (Windows: threads +3, handles +20 allowed for
    the system's own pools), descendants and sessions the same, 0
    worktrees, GUI memory at the end ≤ at 15 min + 4 MB, daemon memory at
    the end ≤ at 45 min + 4 MB (the burst tab's scrollback fills by ~40
    min), every operation done, window and daemon alive.
  - Linux 6-minute trial (scaled counts: 20 tabs, 5 detach, 2 tasks, 1
    theme): every check passed (threads 7/24, fds 17/21, descendants 6,
    sessions 4 unchanged; worktrees 0). It found a fault in the script
    first (the remove list needs a second Enter).
  - Linux one-hour run on `b5ac040`: invalid — an end-to-end run at the
    same time reached its X server (both Xvfb on `:77`; X clients use the
    abstract socket, which `unshare -rm` does not separate) and typed into
    it (sessions 4 → 7). Fixed in `xvfb.sh` (`63822de`); the earlier S4
    soak may have had the same overlap with `crashrun.sh`, its figures
    stay as recorded.
  - Linux one-hour run on `63822de`: stopped by the tool after 10 minutes
    (the system ran low on memory); not restarted, as the tool asks. The
    three samples (0, 308, 609 s): GUI 7 threads, 17 fds; daemon 24
    threads, 21 fds, 6 descendants; 4 sessions; 0 worktrees throughout;
    memory GUI 6.7 → 7.2 MB, daemon 3.9 → 19.1 MB (scrollback filling);
    31 tabs, 7 detach, 5 tasks, 1 theme done. Too short for the verdict.
    `~/m4/soak2/soak2-linux-partial.csv`.
  - Windows: not run. The desktop was in use the whole time (a game in
    the foreground; no 180 s without input), so the waiting run never
    started; it was then stopped with the Linux run.
  - T7, all on `63822de`'s own build: gate passed; `check-range
    haetty-v0.1.1..b5ac040` 34 commits, 4 failing — exactly the four of
    T1 (`9fabc3b`, `bad5247`, `174928b`, `e9f5bee`, history not
    rewritten), every commit since T1 passing — and `b5ac040..63822de` 0
    failing; Linux end-to-end (`run-all.sh`): regression 27/27 (Ctrl+C
    twice 3/3 on a second run; 2/3 in the first, the known Claude Code
    timing case, under the soak's load), §2.3 5/5 and 5/5 integrated,
    themes 8/8, title row 13/13, tab features 20/20, orchestrator 21/21,
    daemon crash 9/9; installer self-tests Linux 19/19, Windows 18/18;
    Windows natively: GUI unit tests 83 + 1 ignored, `orchestrator`,
    `daemoncrash`, `threads` passed, guard self-test 0 failed,
    `windows-smoke.ps1` 0 failures; `--version` `haetty 0.2.0 (commit
    63822de98dae …)`.
  - **No `haetty-v0.2.0` tag**: T7's conditions include the T4 soak on
    both platforms, which did not complete. Candidate: `63822de`. §6.1
    item 30.

### ccx comparison (milestone 3)

| ccx | haetty | |
|---|---|---|
| `--settings` with hooks + statusLine; user settings merged (hooks appended, statusLine dropped with warning) | same, a per-session file; exec-form hooks | ported |
| permissions.ask (39 git/gh rules), bypassPermissions default, `--safe` tab, tui fullscreen, autoCompactWindow, NO_FLICKER/TRUECOLOR env | — | intentionally dropped (a terminal does not change Claude Code's behaviour) |
| attention: amber on Stop/idle, red `?` on permission/elicitation/agent input, cleared by selecting (amber) or prompt/tool (red) | same marks; Ask cleared only by later-started events, not by PreToolUse; PermissionRequest also sets Ask | ported with changes |
| terminal silent | taskbar/urgency hint + Linux `notify-send` when unfocused | new |
| statusline-tee (prints nothing) | `haetty statusline` (prints nothing) | ported |
| panel: context bar, limits with reset, cost, cache, lines, workers, updated, model·effort, key rows | panel without bar graph, model·effort and key rows (Claude Code / palette show those) | ported with changes |
| status row in Claude's status line | haetty's own bottom row incl. "esc to interrupt" | ported with changes |
| pre/post snapshots, apply_edit, 2 MiB, 20 newest | same in Rust; the copy is taken in the synchronous hook; credential-like paths never read | ported with changes |
| diff views: last edit / vs HEAD / git diff tree; keys j/k space/b g/G r m Tab | last edit, vs HEAD, working tree with untracked files; same keys; limits 300 files / 2 MiB / 16 MiB / 3 s | ported (2026-11-06, stage S2) |
| orchestration hooks (Agent pre/post, STATUS: verified/blocked/needs_review) | SubagentStart/Stop durations only | deferred to M4 (orchestrator) |
| tab sets per server socket, last/previous, auto restore, `CCX_RESTORE=0`, closed.tsv (20) | last/previous/saved/closed (20, palette reopen), `restore-tabs = false`; skip missing folders and open conversations; `--resume` when the transcript exists | ported (closed list 2026-11-06, stage S2) |
| tab name: git top-level:branch, then session_name | own name, session_name, `repo:branch` (Claude Code tabs), title without Claude's state glyph, program | ported with changes (session name first, as asked; 2026-11-06) |
| tmux prefix keys (c, C, S, n/p, arrows, hjkl, d, w, W, `,`, D) | Ctrl+Shift shortcuts + palette (P2); panel width actions; automatic size/state persistence; rename Ctrl+Shift+R, detach (palette) | replaced (rename and detach 2026-11-06); renaming does not type `/rename` into Claude Code |
| tab bar drag, `+` and safe buttons | click to select | deferred |

## 7. Long-term direction (constrains current work)

haetty grows into one standalone binary with three layers: the terminal, a
Claude Code adapter (hook events, status line JSON, session ids, transcripts,
permission waits) and an orchestrator (task queue, worker sessions, worktrees,
worker reports in the STATUS/ARTIFACT/EVIDENCE/BLOCKERS/NEXT_ACTION format).

Rules for work done now:
- GUI code talks to sessions only through SessionHandle, SessionEvent and
  Snapshot, so sessions can later move into a daemon behind a local socket
  (Unix socket / Windows named pipe) without rewriting the GUI.
- Hooks will be the same executable (`<bin> hook <event>`) forwarding stdin
  JSON to the daemon; no cache files, no jq, no flock.
- Never read, store or forward Claude credentials or OAuth tokens. Always run
  the user's installed `claude`; observe it only through hooks, the status
  line and transcript files.

The target design (outer and inner) is in `docs/DESIGN.md`; external
integration (Rakazo) in its §6.

## 8. Where the code stands against §7 and DESIGN.md §2/§6 (audit 2026-11-05)

(Milestone 1 audit. Superseded by milestones 2–3, see §6.4: the wire
format, durable ids, event log, roles, hooks, adapter and CLI now exist;
§2.3 was re-measured in the P5 entry.)

- **GUI ↔ session boundary: holds.** `haetty-gui` uses `SessionHandle`
  (key, mouse, wheel, paste, focus, resize, snapshot; `backend`/`pid` only
  for a log line), `SessionEvent` and `Snapshot`, plus plain data types from
  `haetty-core` (`GridSize`, `KeyInput`, `MouseInput`, `Rgb`, `Cell`, …).
  The one in-process seam is `Session::spawn` and owning the `Session` in
  `app.rs::App::create`; a daemon client replaces exactly that. Nothing in
  the GUI touches libghostty or the PTY.
- **Not yet in place (needed for the daemon / CLI / MCP):** the commands and
  events are Rust types without a wire format or version (the engine's `Cmd`
  is private; the public `SessionHandle` methods plus `SessionEvent` are the
  natural schema — serde would add size, measure first). Sessions have no
  durable id (only a pid). `snapshot()` is a blocking request returning the
  whole viewport; over a socket it wants row diffs keyed by `seq`. No event
  log yet.
- **Input stays human-only for permission prompts:** `SessionHandle`'s
  `key` / `paste` / `write` are exactly what could answer a Claude Code
  permission prompt. When the API goes over a socket, input commands are for
  the local GUI only; CLI over SSH and MCP get read-only access by default
  and never input to a session waiting on a permission.
- **Credentials:** no code reads credential files, keychains or token
  variables. The child inherits the parent environment unchanged (except
  `TMUX`/`TMUX_PANE` removal and `TERM*`), so a user's own
  `ANTHROPIC_API_KEY` / `CLAUDE_CODE_OAUTH_TOKEN` setup keeps working;
  nothing is forwarded anywhere else. Debug outputs that could capture
  secrets are opt-in and local: `HAETTY_LOG=debug` no longer records typed or
  IME text (only non-text keys and character counts); `HAETTY_DUMP_FRAME` and
  `haetty-headless --raw-log` / `--trace` (prints OSC 52 clipboard text)
  write screen contents or PTY bytes — never use them around `claude
  /login` or `setup-token`, and never ship them enabled.
- **Test practice:** tests use an isolated `HOME` / `USERPROFILE` and a dummy
  `sk-ant-...` key, never the user's login. Strip every `CLAUDE_CODE_*`,
  `CLAUDECODE` and `CLAUDE_*SESSION*` variable before starting a test
  `claude` from inside a Claude Code session: on 2026-11-05 some Windows
  headless runs started from Git Bash removed only three of them, so the
  test child inherited the host session's `CLAUDE_CODE_SESSION_ID` and
  `CLAUDE_CODE_MESSAGING_TOKEN` (not Claude credentials; nothing logged or
  stored them). The test scripts kept outside the repository
  (`%USERPROFILE%\ccterm-win\scripts` on the dev machine) strip them all
  (`cchome.sh`, the `.ps1` files).
- **Hooks / adapter / orchestrator / CLI JSON / MCP:** not started; nothing
  conflicts. The binary is still the GUI (`haetty`); the first non-option
  word is the program, except the reserved subcommand names (`gui`,
  `daemon`, `hook`, `statusline`, `run`, `headless`, `install-desktop`,
  `status`; `main.rs` `SUBCOMMANDS`), which end with "not implemented yet"
  until they are added.
- **DESIGN.md §2.3 checks, current GUI (no status line, tabs or panels yet):**
  - Layout changes without user input: 0 in steady state — the grid changes
    only on `Resized` / `ScaleFactorChanged` from the window system. At
    startup under WSLg the window passes through scale 1.75 → 1.0 before the
    first frame (several resizes, see the haetty log), which costs Claude
    Code one extra redraw; not seen on Windows.
  - Accent colours on an idle screen: 0 drawn by haetty (it draws no UI of
    its own; padding `extend` repeats the application's own edge colours).
  - With one tab: nothing besides the terminal — the status line does not
    exist yet (adapter milestone).
  - §2.2 "no sounds, pop-ups": the bell only sets the OS attention hint, and
    only when the window is unfocused. Exception: a fatal start-up error on
    Windows shows a message box (the GUI build has no console to print to).
  - Not built yet: status line, command palette (Ctrl+Shift+P), panels,
    first-show key hints, theme struct with the neutral greys.
