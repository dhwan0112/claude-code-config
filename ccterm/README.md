# ccterm

A terminal built for running Claude Code on Linux and Windows: one window,
one session, a single executable per platform. The backend handles
pseudo-terminals, terminal emulation (libghostty-vt) and input encoding; the
`ccterm` window draws on the CPU (winit + softbuffer + swash).
`ccterm-headless` runs programs without a window for testing.

```sh
ccterm                     # your shell ($SHELL / %COMSPEC%)
ccterm claude              # or any program with arguments
ccterm --font "Iosevka" --font-size 12 --cols 140 --rows 40 -- claude --resume
```

## Crates

| Crate | Role |
|---|---|
| `ccterm-pty` | Pseudo-terminals. Unix: `portable-pty`. Windows: the ConPTY API called directly, preferring Microsoft's out-of-band `conpty.dll` + `OpenConsole.exe` (embedded, feature `embedded-conpty`) over the in-box console host. |
| `ccterm-core` | `Session`: child in a PTY, a libghostty-vt `Terminal` on its own engine thread, replies to terminal queries, key/mouse/paste/focus encoding, synchronized-output handling, OSC notification/progress events, snapshots. |
| `ccterm-headless` | CLI that runs a program in a session and drives it with a step script (type, keys, wheel, drag, resize, wait for text, dump the screen). |
| `ccterm-gui` | The `ccterm` window: winit (X11 and Wayland loaded at run time, Win32), CPU rendering into softbuffer behind a `Renderer` trait, glyphs by swash, fonts found with fontdb, clipboard via copypasta. |

### Threads and data flow

```
PTY reader thread ──Output──▶ engine thread ──Wakeup/Title/Notification/Exited──▶ frontend
child waiter thread ─Exited─▶   (Terminal, RenderState, encoders, PTY writer)
frontend ──Key/Mouse/Paste/Resize/Snapshot──▶ engine ──Snapshot (plain data)──▶ frontend
```

libghostty-vt objects are `!Send`, so they never leave the engine thread.
Every byte to the child (query replies and user input) is written from that
thread, in order. A frontend receives `SessionEvent::Wakeup` at most once until
it takes a snapshot, and no wakeup is sent while the application holds
synchronized output (mode 2026), up to `sync_timeout` (500 ms default).

## Building

Needs Rust 1.90+ and Zig 0.15.2 on `PATH` (libghostty-vt is built from
Ghostty's source by `libghostty-vt-sys`).

```sh
# optional: reuse a Ghostty checkout at the pinned commit instead of cloning
export GHOSTTY_SOURCE_DIR=/path/to/ghostty
cargo build --release
cargo test
```

Windows binaries can be cross-built from Linux with `rustup target add
x86_64-pc-windows-gnu` and mingw-w64 (`cargo build --release --target
x86_64-pc-windows-gnu`); the result depends only on system DLLs. On Linux the
binary links only libc/libm/libgcc_s; X11, Wayland and xkbcommon are loaded at
run time.

For shipping, `cargo build --profile dist [--target ...]` (opt-level "s",
thin LTO, stripped): `ccterm` 5.6 MB on Linux, 4.2 MB on Windows.

## ccterm (the window)

```text
ccterm [--font FAMILY] [--font-size PT] [--cols N] [--rows N] [--KEY VALUE]... [--] [PROGRAM [ARGS...]]
```

- `ccterm claude` runs a program directly; without one, the user's shell.
- Reserved subcommand names: `gui`, `daemon`, `hook`, `statusline`, `run`,
  `headless`, `install-desktop`, `status`. As the first word they end with
  "subcommand not implemented yet" (exit code 2) until a later milestone
  adds them. To run a program with one of these names, put it after `--`:
  `ccterm -- status`.
- Paste: Ctrl+Shift+V or Shift+Insert; on Windows also Ctrl+V. Plain Ctrl+V
  reaches the application on Linux (Claude Code pastes images with it).
- Copy: applications copy with OSC 52 — in Claude Code's fullscreen renderer
  (`CLAUDE_CODE_NO_FLICKER=1`) select with the mouse.
- Wheel: sent to applications that track the mouse, otherwise scrolls the
  scrollback.
- IME: composition is drawn underlined at the cursor; the candidate window
  follows the cursor.
- Fonts: `--font FAMILY` (default Cascadia Mono / Consolas on Windows,
  DejaVu Sans Mono / Noto Sans Mono on Linux), fallback for Hangul/CJK
  (Malgun Gothic, Noto Sans CJK, Nanum) and symbols, then any installed
  font. Box drawing and block characters are drawn, not taken from fonts.
- Settings, as `--KEY VALUE` or `--KEY=VALUE` (a config file will use the
  same keys):

  | key | values | default |
  |---|---|---|
  | `window-padding` | minimum padding, logical px | `4` |
  | `window-padding-balance` | `true` / `false` — spread leftover space evenly | `true` |
  | `window-padding-color` | `background` / `extend` / `extend-always` (as in Ghostty) | `extend` |
  | `decorations` | `native` / `integrated` (tabs milestone; native until then) / `none` | `native` |

- Window chrome: on Windows 11 the title bar and border take the terminal
  background and the title text its foreground; Windows 10 and Linux get
  the dark theme request (light if the background is light).
- Environment: `CCTERM_LOG=info|debug`, `CCTERM_LOG_FILE=path` (the Windows
  build has no console), `CCTERM_FONT_DIR=dir`, `CCTERM_DUMP_FRAME=file.ppm`
  (writes every frame, for debugging), `CCTERM_DEBUG_PADDING=1` (paints
  everything outside the grid red, to tell ccterm's padding from a window
  manager's frame).

## ccterm-headless

```sh
ccterm-headless --cols 120 --rows 36 --trace \
  -e "wait-idle 3000 60000" -e "modes" -e "type hello" -e "key shift+Enter" -e "print" \
  -- claude
```

Steps: `sleep MS`, `wait-text TEXT [MS]`, `wait-idle MS [MAX]`, `wait-exit [MS]`,
`type TEXT`, `send TEXT` (raw bytes), `key SPEC` (`Enter`, `shift+Enter`,
`ctrl+c`, `ArrowUp`, `F5`, …), `paste TEXT`, `wheel up|down N COL ROW`,
`click COL ROW`, `drag C1 R1 C2 R2`, `focus in|out`, `resize COLS ROWS`, `expect TEXT`,
`expect-not TEXT`, `print`, `modes`, `save FILE`. `--raw-log FILE` keeps every
output byte; `--trace` prints replies to queries and events.

## Status

See `docs/HANDOFF.md` §3 for details and evidence. Verified on Linux (x86_64):

- `cargo test`: 5 unit tests (OSC scanner) and 10 end-to-end tests with real
  children in real PTYs (query replies, key/mouse/paste/focus encoding,
  synchronized output hold and timeout, OSC title/notification/progress/bell,
  resize, cell colours and wide characters, exit reporting, environment).
- Claude Code 2.1.318: onboarding screens drawn and navigated with arrow keys;
  main screen in the fullscreen renderer (alternate screen, SGR mouse
  tracking, kitty keyboard flags 5, bracketed paste, focus events), 24-bit
  colour output, Hangul input, Shift+Enter for a second input line, Ctrl+C
  twice to exit with status 0.

- `ccterm` window with Claude Code under X11 and native Wayland: rendering,
  typing (ASCII, Hangul through ibus-hangul/XIM), Shift+Enter, wheel,
  resize, selection copy to the clipboard and paste (X11), idle CPU ~0%.

Windows 11 (native, x86_64): the PTY checks of `scripts/windows-smoke.ps1`
pass (embedded ConPTY 1.25); its Claude Code checks need a prepared Claude
Code config (see `docs/HANDOFF.md` §3) and pass with one, run through
`ccterm-headless` (fullscreen renderer, kitty keyboard flags 5 included).
The `ccterm` window runs Claude Code (typing, Shift+Enter, Ctrl+V paste,
exit code passed on), and the GUI unit tests pass there. Not yet checked by
hand: the Microsoft IME, a real mouse wheel and HiDPI monitors.

## Windows notes

ConPTY sits between the child and the terminal. The in-box host re-renders
output and has dropped mouse-mode requests (older builds), APC strings and
kitty keyboard negotiation. ccterm therefore loads, in order: a `conpty.dll`
named by `CCTERM_CONPTY_DLL` or placed next to the executable; the copy
embedded in the binary, written once to
`%LOCALAPPDATA%\ccterm\conpty\<version>\` together with `OpenConsole.exe`
and Microsoft's licence; kernel32. With the out-of-band host it passes
`PSEUDOCONSOLE_GLYPH_WIDTH_GRAPHEMES` (0x8); the in-box host gets no flags,
because 0x8 meant `PSEUDOCONSOLE_PASSTHROUGH_MODE` in older declarations.
npm's `claude.cmd` is started through `cmd.exe /d /s /c`.

## Third-party components

- libghostty-vt (Ghostty, MIT) via `libghostty-vt` / `libghostty-vt-sys`
  (MIT OR Apache-2.0).
- `portable-pty` (MIT) on Unix.
- winit, softbuffer (Apache-2.0 / MIT), swash, fontdb, copypasta, memmap2,
  unicode-width (MIT / Apache-2.0).
- Microsoft.Windows.Console.ConPTY `conpty.dll` / `OpenConsole.exe` (MIT,
  Microsoft) via `conpty-bin-vendored` (MIT) on Windows.

## Licence

MIT, see `LICENSE`.
