# ccterm handoff

State as of 2026-11-05, after the `ccterm-gui` milestone 1 commits
(`26e0056`..), the window decoration work (`01e1d32`..) and the milestone 1
finishing work (`e9312a4`..; §3 "Milestone 1 finishing work"). The code
lives in `ccterm/` of the `dhwan0112/claude-code-config` repository.
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
| `ccterm-pty` | Done. Unix via portable-pty. Windows: own ConPTY code (`src/win/mod.rs`), host order sideloaded `conpty.dll` (`CCTERM_CONPTY_DLL` or next to the exe) → embedded Microsoft ConPTY 1.25.260930003 (crate `conpty-bin-vendored`, written to `%LOCALAPPDATA%\ccterm\conpty\<version>\`) → kernel32. PATH/PATHEXT lookup; `.cmd/.bat` (npm's `claude.cmd`) run via `cmd.exe /d /s /c`. |
| `ccterm-core` | Done. `Session::spawn(SessionConfig)` → `SessionHandle` + `Receiver<SessionEvent>`. Engine thread owns `libghostty_vt::Terminal`, `RenderState`, key/mouse encoders and the PTY writer. |
| `ccterm-headless` | Done. Step-script runner (README lists the steps; `drag` added for selection tests). |
| `ccterm-gui` | Milestone 1 done (binary `ccterm`). See below. |
| `ccterm-proto` | Milestone 2: product constants, paths, wire types (postcard), framing, local socket / named pipe transport, daemon client (`RemoteSession`). |
| `ccterm-daemon` | Milestone 2: owns the sessions; frames, roles, event log (DESIGN.md §3.5–3.8). |

Public API a frontend uses (`ccterm-core`):

- `SessionHandle::{key, mouse, wheel, paste, focus, resize, scroll, write, snapshot, dump, set_color_scheme, backend, pid}`
- `SessionEvent::{Wakeup, Title, Pwd, Bell, ClipboardWrite, Notification, Progress, Reply, Exited, Error}`
- `Snapshot { cols, rows, cells: Vec<Cell>, cursor, default_fg/bg, cursor_color, title, pwd, modes, scrollbar, seq }`;
  `Cell { text, fg, bg, flags, width: Narrow|Wide|Spacer }`
- `KeyInput { key: Key (physical, libghostty enum), action, mods, consumed_mods, text, unshifted, composing }`,
  `KeyInput::parse("shift+Enter")`; `MouseInput::{at_pixel, at_cell}`; `GridSize { cols, rows, cell_width, cell_height }`.

`Wakeup` is sent at most once until the frontend takes a snapshot. No wakeup
while the application holds synchronized output (DEC 2026), up to
`sync_timeout` (500 ms).

### `ccterm-gui` layout

| Module | Role |
|---|---|
| `config.rs` | Settings with `Config::set(key, value)`: `window-padding`, `window-padding-balance`, `window-padding-color`, `decorations`; the CLI passes `--KEY VALUE`, a config file will too. |
| `chrome.rs` | Native title bar/frame colours (Windows 11) and dark/light theme request, updated when the default colours change. |
| `main.rs` | CLI (`--font`, `--font-size` pt, `--cols`, `--rows`, settings, program; reserved subcommand names `SUBCOMMANDS` end with "not implemented yet", a program of that name goes after `--`), logger (`CCTERM_LOG`, `CCTERM_LOG_FILE`), fatal and CLI errors (Windows GUI subsystem: redirected stdout/stderr, else the parent's console, else a message box). |
| `app.rs` | winit `ApplicationHandler`: one window, one `Session`. A forwarder thread turns `SessionEvent`s into `EventLoopProxy` user events (`ControlFlow::Wait`, no polling). On `Wakeup` the snapshot is taken immediately (re-arms the next wakeup) and rows are diffed; `RedrawRequested` renders. Keyboard, mouse, wheel, IME, focus, resize, scale factor, title, bell, clipboard, exit (exit code passed on). |
| `input.rs` | `KeyCode` → libghostty `Key`; text/consumed-mods rules; which keys the IME owns during a composition (`ime_owns_key`: not Ctrl/Super shortcuts); mouse buttons; wheel notch accumulator; paste shortcut. Unit-tested without winit events. |
| `geom.rs` | Window pixels ↔ grid; `Layout::origin` (padding, balance) shared by renderer and input mapping. |
| `damage.rs` | Per-row dirty flags from two snapshots (no core API change was needed). |
| `font.rs` | fontdb (no fontconfig; `memmap` feature: the start-up scan and the faces map files, one map per file via `make_shared_face_data`, nothing read into the heap); primary/bold/italic, fallback lists, once-per-character scan of all faces; colour faces (CBDT/sbix/COLR/SVG) only when no monochrome face has a text-presentation character, first for emoji presentation (`pick`). |
| `emoji.rs` | `Presentation::of(cell text)`: VS16/VS15, else `Emoji_Presentation` (Unicode 16.0 table, 80 ranges). |
| `render/mod.rs` | `Renderer` trait (`cell_metrics`, `set_font_px`, `set_padding`, `resize`, `render(&Scene)`, `memory()` for the exit log line), `Scene`, `Preedit`. |
| `render/paint.rs` | Window-independent painter: rows → pixels; colours, attributes, wide cells, cursor shapes, preedit; glyph cache keyed by (face, glyph, size, synthetic bold/italic); glyphs wider than their cells scaled to fit; fallback CJK faces scaled to fill wide cells (§5). Pixel tests. |
| `render/boxdraw.rs` | U+2500–U+259F drawn procedurally. Pixel tests. |
| `render/soft.rs` | softbuffer backend: own canvas = back buffer (always the complete frame); bands changed during `buffer.age()` frames are copied (history of 4), everything when the age is 0 or older; only the current bands are presented as damage; `CCTERM_DUMP_FRAME`, `CCTERM_DEBUG_PADDING`. |
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
  for the default background). `decorations = integrated` is reserved for
  the tabs milestone and currently falls back to `native`.
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
  `ccterm-gui` unit/pixel tests (31 at the first GUI commits).
- Query replies: DA1 `\e[?62;1;6;22;52c`, DA2, XTVERSION `ccterm(0.1.0)`,
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
  the direct `ccterm-headless.exe` run in the next item.
- `ccterm-gui` unit tests built for `x86_64-pc-windows-gnu` and run natively:
  31 passed — including the Windows-only branches (winit#4746 workaround,
  AltGr text, Ctrl+V paste) and the pixel tests with Windows fonts
  (Malgun Gothic for the wide-character test).
- `ccterm-headless.exe` + native Claude Code 2.1.318
  (`%USERPROFILE%\.local\bin\claude.exe`): fullscreen renderer through the
  embedded ConPTY — `alt_screen, mouse_tracking, sgr_mouse,
  bracketed_paste, focus_events: true, kitty_keyboard: 5`; `안녕 hello` +
  Shift+Enter + `second line` shown as two input lines; Ctrl+C twice exits 0.
  So kitty keyboard negotiation does pass through OpenConsole 1.25.
- `ccterm.exe` window: Claude Code drawn (Cascadia Mono, synthetic bold,
  title `✳ Claude Code`, fallback symbols from Segoe UI Symbol), typing via
  SendKeys, Shift+Enter, Unicode text `안녕` (VK_PACKET, not the IME),
  Ctrl+V pastes the Windows clipboard, the window closes when the child
  exits and `ccterm.exe` returns the child's exit code (3 in the test). No
  console window (GUI subsystem); `OpenConsole.exe` runs as a hidden child.
- Idle with Claude Code idle (30 s): 0.063 s CPU time = 0.2 % of one core;
  working set 46.9 MB, private 12.8 MB, 12 threads.

### `ccterm` window on Linux (WSLg, new 2026-11-05)

- X11 backend (`WAYLAND_DISPLAY` unset), keys and mouse from xdotool
  (XTest), frames from `CCTERM_DUMP_FRAME`: Claude Code main screen
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
- Idle with Claude Code idle (60 s): ccterm 0 ticks (Wayland), 2 ticks
  = 0.03 % of one core (X11); Claude Code itself ~0.8 %. RSS ~55 MB
  (RssAnon 25.7 MB, RssFile 25 MB = shared libraries and mapped fonts),
  6 threads; Claude Code ~230 MB.
- Linkage: `readelf -d ccterm` NEEDED = libgcc_s, libm, libc, ld-linux only;
  `ccterm.exe` imports system DLLs only (KERNEL32, USER32, GDI32, IMM32,
  ADVAPI32, SHELL32, ole32, dwmapi, uxtheme, userenv, ws2_32, ntdll,
  msvcrt, bcryptprimitives, api-ms-win-core-synch).

### Window decorations (2026-11-05)

- The "white band": `CCTERM_DEBUG_PADDING=1` plus captures of the whole
  window from the Windows side (`PrintWindow` on the WSLg RAIL window) show
  the red padding strictly inside a light title bar and ~6 px grey border
  — that frame is WSLg Weston's X11 window manager, not ccterm. ccterm's
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
  `CCTERM_FONT_DIR` with fonts-nanum) and natively on Windows with Cascadia
  Mono + Malgun Gothic, at 11 pt, 14 pt, 16 px and 24 px.
  `primary_wide_hangul_is_not_rescaled` (Noto Sans Mono CJK KR as the
  primary: identical fit to before) and
  `box_drawing_is_unaffected_by_wide_fallback_scaling` pass. Frame dumps
  `hangul-linux-nanum-{11,14}pt-{before,after}` and
  `hangul-windows-malgun-{11,14}pt-{before,after}` (PNG plus 3× zoom).
- **Emoji presentation**: unit tests `defaults_and_variation_selectors`
  (table) and `fallback_order_by_presentation` (`pick`); ccterm-core
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
  headless control in "Before milestone 2" for the conclusion. One way ccterm
  itself could lose a press was found and fixed: Ctrl+C was held back
  while an IME composition was shown (`input::ime_owns_key`, unit test
  `shortcuts_pass_while_composing`); ibus-hangul commits before it forwards
  Ctrl+C, so it did not show up here — an IME that reports the key first
  would have.
- **Memory** (Claude Code idle, 120x36, 11 pt): Linux `ccterm` RssAnon
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
  `ccterm status` → message and exit 2 on Linux; on Windows with stderr
  redirected the message arrives there, exit 2; `ccterm -- status` runs a
  program `status` ("not found on PATH").
- **Public repository search**: no user names, user paths, private
  addresses or e-mail addresses in `ccterm/` besides the HANDOFF path now
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
  ccterm causes the miss (ccterm sent identical bytes in every failing
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

| Build | ccterm Linux | ccterm Windows | headless Linux | headless Windows |
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
3.27 / 1.41 MB; + session (libghostty-vt, ccterm-core) and text stack
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
  progress through callbacks: `ccterm-core/src/osc.rs` scans the stream.
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
  is not ccterm. Check with `CCTERM_DEBUG_PADDING=1` before changing
  padding code.
- DWM colour attributes (caption, border, text: 34–36) are set-only —
  `DwmGetWindowAttribute` returns E_INVALIDARG — and `PrintWindow` does not
  include the DWM border (comes back black). Check them with a real screen
  capture of the visible frame (`DWMWA_EXTENDED_FRAME_BOUNDS`).
- `cargo build --target <triple>` builds into `target/<triple>/`, not
  `target/release/`: after a size measurement with explicit targets, the
  plain `target/release/ccterm` may be stale.
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
  Claude Code's start-up state; no evidence for a ccterm cause. Tests
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
  `isSymbol`); CJK is not centred. ccterm takes the face-level idea but
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

### 6.1 Manual checks still open for milestone 1

0. Ctrl+C twice with a real keyboard right after Claude Code starts, a few
   times on each platform (automated runs: §3; rare misses on Linux only,
   within the first seconds); and Ctrl+C while composing Hangul with the
   Microsoft Korean IME.
1. Windows: type Hangul with the Microsoft Korean IME in `ccterm.exe` running
   Claude Code — composition shown inline and underlined at the cursor, no
   caret at the start of the syllable (winit#4746 workaround), candidate
   window (Hanja key) next to the cursor, commit on Space/Enter, Enter right
   after a syllable submits once.
2. Linux on a real desktop: Hangul with ibus or fcitx5 under native Wayland
   (`zwp_text_input_v3`) and under X11 (XIM).
3. A physical mouse wheel and a touchpad: one notch = one step in Claude
   Code; touchpad scrolling smooth enough.
4. HiDPI: a 150–200 % monitor on Windows and Wayland; moving the window
   between monitors with different scales (ScaleFactorChanged path).
5. Interactive resize by dragging the window edge (only programmatic
   resizes were tested).
6. Real clipboard managers: copy from Claude Code, paste in another app;
   paste a large text (bracketed) into Claude Code; the same on a Wayland
   session (only initialisation was seen) and on Windows (selection copy
   from Claude Code was only checked on X11).
7. Font choice with `--font` on each platform; on a full Linux desktop and
   on Windows, look at Hangul next to Latin (size and baseline after the
   wide-cell scaling) and at Claude Code's status symbols (⏸ ⚠ ✔ ●)
   — judged so far from frame dumps only.

8. Windows 10: dark title bar from the theme request (Windows 11 colours
   do not apply there).
9. Linux on a GNOME/KDE session: dark title bar on X11 (mutter/KWin read
   `_GTK_THEME_VARIANT`) and the dark adwaita CSD on Wayland; padding while
   dragging the window edge.
10. (M2) Tabs with a real keyboard on both platforms: Ctrl+Shift+T/W,
    Ctrl+Tab, Alt+1..9, Ctrl+Shift+P; Hangul typed into the palette; the
    tab bar and hint legible on a real display (frame dumps only so far).
11. (M2) After `ccterm install-desktop` on a GNOME/KDE desktop: the
    launcher entry, the dock icon matched to the running window (Wayland
    and X11); on Windows the taskbar, Alt+Tab and Explorer icons.

### 6.2 Known gaps (not in milestone 1's scope)

- Own text selection and copy when the application does not track the
  mouse (plain shell); Shift+drag override while it does.
- Cursor blink; font size zoom shortcuts; config file (keys exist, see
  `config.rs`).
- `decorations = integrated` (own title bar with tabs) — tabs milestone.
- `window-padding-color = extend`: prompt-row heuristic needs shell
  integration (OSC 133).
- Glyph shaping (ligatures, ZWJ emoji sequences are drawn as the first
  character).
- `scrollback-limit` takes plain bytes; Ghostty-style suffixes (`10MB`)
  are not parsed.

### 6.3 Later milestones

Tabs; status panel and diff pane rebuilt from the `ccx` hook scripts (status
line JSON + hook events, ideally via a local socket instead of files); saved
tab sets; config file; OpenGL renderer if needed; Windows fixes from the
smoke test (none needed so far).

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
  ccterm tabs). Serialization chosen by measurement: postcard
  +13.1 / +13.3 KB dist vs a hand-written codec, bincode +21.8 / +18.4 KB.
- **2026-11-05 P1 — daemon and IPC.** New crates `ccterm-proto` (product
  constants, paths, postcard wire types and framing, Unix socket / named
  pipe transport with overlapped I/O and a user-only DACL, a client whose
  `RemoteSession` has the shape of `Session`) and `ccterm-daemon` (one
  state thread; reader/writer thread per client; per-client baseline
  snapshots, row-diff frames, `NextFrame` backpressure; roles; event log
  `events.log`; exit after 10 s without sessions and clients). The GUI is
  a control client: it starts `ccterm daemon` detached when none answers,
  creates its session there (with its environment and working directory),
  and closes it when the program exits or the window is closed; a killed
  GUI leaves the session running. `ccterm status [--json | --screen ID]`
  is read-only (observe role). Evidence:
  - `cargo test`: daemon integration (`crates/ccterm-daemon/tests/daemon.rs`):
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
    ccterm-daemon --example latency`, 120x36): Linux median 2.14 ms, p95
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
    `ccterm_daemon` 114 KiB, `ccterm_proto` 97 KiB including the postcard /
    serde code for the message types; the rest is std (Unix sockets,
    process spawning) and the Windows pipe/security code.
- **2026-11-05 P2 — tabs, keys, palette, config file.** Tabs: each tab a
  daemon session; the window grid is the active tab's screen plus ccterm's
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
    after Ctrl+Shift+T in another folder; `WM_CLASS "ccterm", "ccterm"`;
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

## 7. Long-term direction (constrains current work)

ccterm grows into one standalone binary with three layers: the terminal, a
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

- **GUI ↔ session boundary: holds.** `ccterm-gui` uses `SessionHandle`
  (key, mouse, wheel, paste, focus, resize, snapshot; `backend`/`pid` only
  for a log line), `SessionEvent` and `Snapshot`, plus plain data types from
  `ccterm-core` (`GridSize`, `KeyInput`, `MouseInput`, `Rgb`, `Cell`, …).
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
  secrets are opt-in and local: `CCTERM_LOG=debug` no longer records typed or
  IME text (only non-text keys and character counts); `CCTERM_DUMP_FRAME` and
  `ccterm-headless --raw-log` / `--trace` (prints OSC 52 clipboard text)
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
  conflicts. The binary is still the GUI (`ccterm`); the first non-option
  word is the program, except the reserved subcommand names (`gui`,
  `daemon`, `hook`, `statusline`, `run`, `headless`, `install-desktop`,
  `status`; `main.rs` `SUBCOMMANDS`), which end with "not implemented yet"
  until they are added.
- **DESIGN.md §2.3 checks, current GUI (no status line, tabs or panels yet):**
  - Layout changes without user input: 0 in steady state — the grid changes
    only on `Resized` / `ScaleFactorChanged` from the window system. At
    startup under WSLg the window passes through scale 1.75 → 1.0 before the
    first frame (several resizes, see the ccterm log), which costs Claude
    Code one extra redraw; not seen on Windows.
  - Accent colours on an idle screen: 0 drawn by ccterm (it draws no UI of
    its own; padding `extend` repeats the application's own edge colours).
  - With one tab: nothing besides the terminal — the status line does not
    exist yet (adapter milestone).
  - §2.2 "no sounds, pop-ups": the bell only sets the OS attention hint, and
    only when the window is unfocused. Exception: a fatal start-up error on
    Windows shows a message box (the GUI build has no console to print to).
  - Not built yet: status line, command palette (Ctrl+Shift+P), panels,
    first-show key hints, theme struct with the neutral greys.
