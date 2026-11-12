# haetty history: pitfalls in full (the one-line list is HANDOFF.md §4)

Moved verbatim from `docs/HANDOFF.md` on 2026-11-07 (stage V2). Section numbers and "§" references inside are those of the old HANDOFF; its section map (HANDOFF.md §7) says where each went.

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
- portable-pty 0.9's Unix master writer writes `"\n"` + EOF with a blocking
  `write_all` in `Drop`; on a child that is gone and left its input queue
  full, the drop waits forever (Linux does not wake a master writer when the
  child's side closes). haetty uses its own non-blocking reader and writer
  with a close pipe (P1a, `docs/history/2026-11-12-p1.md`).
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
