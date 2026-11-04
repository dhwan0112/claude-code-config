# ccterm handoff

State as of 2026-11-05, after the `ccterm-gui` milestone 1 commits
(`26e0056`..) and the window decoration work (`01e1d32`..). Written for whoever continues the work (Claude Code included).

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
| `main.rs` | CLI (`--font`, `--font-size` pt, `--cols`, `--rows`, settings, program), logger (`CCTERM_LOG`, `CCTERM_LOG_FILE`), fatal errors (message box on Windows; the release build uses the GUI subsystem, no console). |
| `app.rs` | winit `ApplicationHandler`: one window, one `Session`. A forwarder thread turns `SessionEvent`s into `EventLoopProxy` user events (`ControlFlow::Wait`, no polling). On `Wakeup` the snapshot is taken immediately (re-arms the next wakeup) and rows are diffed; `RedrawRequested` renders. Keyboard, mouse, wheel, IME, focus, resize, scale factor, title, bell, clipboard, exit (exit code passed on). |
| `input.rs` | `KeyCode` → libghostty `Key`; text/consumed-mods rules; mouse buttons; wheel notch accumulator; paste shortcut. Unit-tested without winit events. |
| `geom.rs` | Window pixels ↔ grid; `Layout::origin` (padding, balance) shared by renderer and input mapping. |
| `damage.rs` | Per-row dirty flags from two snapshots (no core API change was needed). |
| `font.rs` | fontdb (no fontconfig) + memory-mapped faces; primary/bold/italic, fallback lists, once-per-character scan of all faces. |
| `render/mod.rs` | `Renderer` trait (`cell_metrics`, `set_font_px`, `set_padding`, `resize`, `render(&Scene)`), `Scene`, `Preedit`. |
| `render/paint.rs` | Window-independent painter: rows → pixels; colours, attributes, wide cells, cursor shapes, preedit; glyph cache keyed by (face, glyph, size, synthetic bold/italic); glyphs wider than their cells scaled to fit. Pixel tests. |
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

- `cargo test`: 5 unit tests (OSC scanner), 9 end-to-end tests; plus 31
  `ccterm-gui` unit/pixel tests.
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
  (`C:\Users\...\.local\bin\claude.exe`): fullscreen renderer through the
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
  Listed in §6.1.
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

### Sizes (stripped)

| Build | ccterm Linux | ccterm Windows | headless Linux | headless Windows |
|---|---|---|---|---|
| `release` + strip | 14.44 MB | 10.64 MB | 9.49 MB | 7.34 MB |
| `dist` profile | 5.60 MB | 4.25 MB | 1.81 MB | 3.03 MB |

Window decoration work (`release`+strip / `dist`, Linux and Windows):
padding balance/colour + buffer-age copy +14.4 / +17.4 KB release, +17.2 /
+15.9 KB dist; chrome colours/theme +11.0 / +2.6 KB release, +3.7 / +4.1 KB
dist; `decorations` key < 1 KB. Current: release 14.46 / 10.66 MB, dist
5.63 / 4.27 MB.

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
- Rejected: GPUI (standalone crate documented for macOS/Linux only;
  gpui-libghostty is macOS + Wayland only), egui/iced for the terminal body
  (extra layers, no IME advantage), Electron/Tauri.

## 6. Next tasks

### 6.1 Manual checks still open for milestone 1

0. Ctrl+C twice in the window exits Claude Code and closes the window
   (Linux and Windows); see §3 for the partial evidence.
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
7. Font choice with `--font` on each platform; Noto Sans Symbols 2 / emoji
   rendering on a full Linux desktop.

8. Windows 10: dark title bar from the theme request (Windows 11 colours
   do not apply there).
9. Linux on a GNOME/KDE session: dark title bar on X11 (mutter/KWin read
   `_GTK_THEME_VARIANT`) and the dark adwaita CSD on Wayland; padding while
   dragging the window edge.

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

### 6.3 Later milestones

Tabs; status panel and diff pane rebuilt from the `ccx` hook scripts (status
line JSON + hook events, ideally via a local socket instead of files); saved
tab sets; config file; OpenGL renderer if needed; Windows fixes from the
smoke test (none needed so far).
