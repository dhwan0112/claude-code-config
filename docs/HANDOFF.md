# ccterm handoff

State as of 2026-11-04, commit after `09cdcf1`. Written for whoever continues
the work (Claude Code included).

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
| `ccterm-headless` | Done. Step-script runner (README lists the steps). |
| `ccterm-gui` | Not started. Next task, see §6. |

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

## 3. Verified (Linux x86_64, 2026-11-04)

- `cargo test`: 5 unit tests (OSC scanner), 9 end-to-end tests, stable over 5 runs.
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
  To reach the main screen without login: `HOME` with a `.claude.json`
  containing `hasCompletedOnboarding: true`, trusted project dir, and
  `ANTHROPIC_API_KEY` set to any `sk-ant-...` string (approve it once in the
  "custom API key" dialog with ArrowUp + Enter).
- Size/weight: stripped `ccterm-headless` 9.5 MB Linux, 7.3 MB Windows
  (incl. ~1.2 MB embedded ConPTY). RSS ~14 MB with Claude Code fullscreen at
  120x36, 4 threads.
- GUI spike (not in repo): winit 0.30 + softbuffer 0.4 + swash 0.2, empty
  window, `opt-level="s"`, thin LTO, strip: 2.9 MB Linux (X11+Wayland),
  0.6 MB Windows.

Not verified: anything run on Windows. `ccterm-pty` and `ccterm-headless`
compile and link for `x86_64-pc-windows-gnu` only. `scripts/windows-smoke.ps1`
is the first check to run on a Windows machine; its output decides the
Windows fixes.

## 4. Pitfalls already hit (do not repeat)

- **ConPTY flag 0x8**: was `PSEUDOCONSOLE_PASSTHROUGH_MODE` in old
  declarations; in current ConPTY headers it is
  `PSEUDOCONSOLE_GLYPH_WIDTH_GRAPHEMES`. Never send it to the in-box host
  (flags 0 there). The out-of-band host gets only glyph-width flags.
- portable-pty 0.9 hard-codes ConPTY flags and cannot pick the DLL; that is
  why Windows has its own backend.
- The in-box conhost re-renders output and has dropped mouse-mode requests
  (older builds), APC strings and kitty keyboard negotiation; kitty keyboard
  through ConPTY end-to-end is still unknown even with OpenConsole.
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

## 5. Decisions taken for the GUI (with reasons)

- **Window/input: winit** (0.30 line). IME on Windows, X11 (XIM; needs
  ibus/fcitx with XIM) and Wayland (`zwp_text_input_v3`). Check that the
  winit version includes the fix for winit#4746 (Microsoft Korean IME reported
  the preedit cursor at 0 instead of after the syllable); if not, apply the
  same workaround egui-winit uses. On Wayland sessions run the native Wayland
  backend, not XWayland (Hangul commits were reported lost under XWayland in
  another project).
- **Drawing: CPU via softbuffer** first (Win32, X11, Wayland all tier 1; no
  GPU driver dependency, works in RDP/VMs). Hide it behind a `Renderer`
  trait so an OpenGL backend (glutin + glow) can be added later only if
  profiling shows the need. wgpu is out (weight).
- **Glyphs: swash** for rasterization with a glyph cache keyed by
  (font, glyph id, size, style). Font fallback for Hangul/CJK and symbols:
  Windows `Malgun Gothic`, Linux Noto Sans CJK / Nanum via fontconfig-free
  discovery (e.g. `fontdb`), configurable primary monospace font.
- Rejected: GPUI (standalone crate documented for macOS/Linux only;
  gpui-libghostty is macOS + Wayland only), egui/iced for the terminal body
  (extra layers, no IME advantage), Electron/Tauri.

## 6. Next task: `ccterm-gui`, milestone 1

One window, one session, the user's shell or `claude` given on the command
line. Scope:

1. Window via winit, CPU frame via softbuffer, HiDPI scale factor handled.
2. Text: monospace grid from `Snapshot`; fg/bg, bold, italic, underline,
   strikethrough, inverse, faint; wide characters (`Width::Wide` spans two
   cells, `Spacer` draws nothing); box-drawing characters aligned to cells;
   cursor shapes (block, bar, underline, hollow when unfocused).
3. Redraw only on `SessionEvent::Wakeup`; repaint only rows that changed
   (compare with the previous snapshot or add row dirty flags to `Snapshot`
   in `ccterm-core`; keep the API change small and tested).
4. Input: map winit `KeyEvent` (physical key + logical text) to `KeyInput`;
   modifiers; mouse buttons, motion with button held, wheel (`wheel()`),
   focus in/out.
5. IME: `set_ime_allowed(true)`, `Ime::Preedit` drawn inline at the cursor
   (underlined), `Ime::Commit` sent as text, IME cursor area updated with
   `set_ime_cursor_area` every frame the cursor moves.
6. Resize: window size → cols/rows from cell metrics → `SessionHandle::resize`.
7. Clipboard: `ClipboardWrite` → system clipboard; paste shortcut
   (Ctrl+Shift+V on Linux, Ctrl+V on Windows when not sent to the app —
   decide and document) → `paste()`. Use a small clipboard crate and report
   its size.
8. Title from `SessionEvent::Title`; window closes on `Exited`.

Acceptance (report each with evidence):

- Builds for both targets; stripped size reported for both.
- Linux: Claude Code 2.1.x runs in fullscreen renderer; typing ASCII and
  Hangul (via IME) works; Shift+Enter makes a second line; wheel scrolls the
  conversation; resizing the window reflows; selection copy from Claude Code
  reaches the clipboard. Verify what can be verified headlessly with
  `ccterm-headless` and unit tests; list the manual checks for the user.
- Idle CPU with Claude Code idle: report (target: ~0%).
- RSS: report.
- No regression in `cargo test`.

Later milestones (not now): tabs; status panel and diff pane rebuilt from
the `ccx` hook scripts (status line JSON + hook events, ideally via a local
socket instead of files); saved tab sets; config file; OpenGL renderer if
needed; Windows fixes from the smoke test.
