# ccterm

A terminal built for running Claude Code, aiming at Linux and Windows. This
repository currently holds the backend: pseudo-terminal handling, terminal
emulation, input encoding and a Send-able snapshot API that a GUI frontend
will draw from. There is no window yet; `ccterm-headless` runs programs
without one for testing.

## Crates

| Crate | Role |
|---|---|
| `ccterm-pty` | Pseudo-terminals. Unix: `portable-pty`. Windows: the ConPTY API called directly, preferring Microsoft's out-of-band `conpty.dll` + `OpenConsole.exe` (embedded, feature `embedded-conpty`) over the in-box console host. |
| `ccterm-core` | `Session`: child in a PTY, a libghostty-vt `Terminal` on its own engine thread, replies to terminal queries, key/mouse/paste/focus encoding, synchronized-output handling, OSC notification/progress events, snapshots. |
| `ccterm-headless` | CLI that runs a program in a session and drives it with a step script (type, keys, wheel, resize, wait for text, dump the screen). |

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
x86_64-pc-windows-gnu`); the result depends only on system DLLs.

## ccterm-headless

```sh
ccterm-headless --cols 120 --rows 36 --trace \
  -e "wait-idle 3000 60000" -e "modes" -e "type hello" -e "key shift+Enter" -e "print" \
  -- claude
```

Steps: `sleep MS`, `wait-text TEXT [MS]`, `wait-idle MS [MAX]`, `wait-exit [MS]`,
`type TEXT`, `send TEXT` (raw bytes), `key SPEC` (`Enter`, `shift+Enter`,
`ctrl+c`, `ArrowUp`, `F5`, …), `paste TEXT`, `wheel up|down N COL ROW`,
`click COL ROW`, `focus in|out`, `resize COLS ROWS`, `expect TEXT`,
`expect-not TEXT`, `print`, `modes`, `save FILE`. `--raw-log FILE` keeps every
output byte; `--trace` prints replies to queries and events.

## Status

Verified on Linux (x86_64):

- `cargo test`: 5 unit tests (OSC scanner) and 9 end-to-end tests with real
  children in real PTYs (query replies, key/mouse/paste/focus encoding,
  synchronized output hold and timeout, OSC title/notification/progress/bell,
  resize, cell colours and wide characters, exit reporting, environment).
- Claude Code 2.1.318: onboarding screens drawn and navigated with arrow keys;
  main screen in the fullscreen renderer (alternate screen, SGR mouse
  tracking, kitty keyboard flags 5, bracketed paste, focus events), 24-bit
  colour output, Hangul input, Shift+Enter for a second input line, Ctrl+C
  twice to exit with status 0.

Windows: `ccterm-pty` and `ccterm-headless` compile and link for
`x86_64-pc-windows-gnu`. Nothing has been run on Windows yet;
`scripts/windows-smoke.ps1` is the first check to run there.

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
- Microsoft.Windows.Console.ConPTY `conpty.dll` / `OpenConsole.exe` (MIT,
  Microsoft) via `conpty-bin-vendored` (MIT) on Windows.

## Licence

MIT, see `LICENSE`.
