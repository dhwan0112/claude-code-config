# ccterm — instructions for Claude Code

A lightweight terminal for running Claude Code, on **Windows and Linux both**.
The backend (PTY + terminal emulation) and the first GUI milestone (`ccterm`,
crate `ccterm-gui`) are done. Read `docs/HANDOFF.md` (state, verified facts,
pitfalls, tasks) and `docs/DESIGN.md` (target outer and inner design) once at
the start of a session.

## Hard requirements

- Every change must build for `x86_64-unknown-linux-gnu` and
  `x86_64-pc-windows-gnu`. No platform-only feature without a working
  counterpart or a documented, tested fallback on the other platform.
- Lightweight: no Electron/webview, no wgpu, no general UI framework (egui,
  iced, GPUI) for the terminal itself. Check the binary size impact of every
  new dependency (`strip`ped release build) and report it.
- Standalone: one executable per platform, no installer, no runtime
  dependencies beyond system libraries (Windows: system DLLs only; Linux:
  X11/Wayland client libraries loaded at run time).
- Rust only, except libghostty-vt (Zig, FFI inside `ccterm-core`) and the
  ConPTY binaries. No Python or shell at run time.
- libghostty-vt objects are `!Send`: they live only on the engine thread in
  `ccterm-core`. Frontends use `SessionHandle`, `SessionEvent` and `Snapshot`
  — and nothing else of a session, so sessions can move into a daemon behind
  a local socket later (`docs/HANDOFF.md` §7–§8).
- Never read, store or forward Claude credentials or OAuth tokens; run the
  user's installed `claude` and observe it only through hooks, the status
  line and transcripts. Logs must not record typed text.
- Future hooks are the same executable (`<bin> hook <event>`), no cache
  files, no jq, no flock. The daemon API is a typed, versioned schema with
  durable ids and an append-only event log; remote parties (CLI over SSH,
  MCP) never answer Claude Code permission prompts (`docs/DESIGN.md` §6).
- Outer design (`docs/DESIGN.md` §2): events never change the layout; no
  accent colour on an idle screen; no sounds, pop-ups or animations.

## Environment

- Rust 1.90+, Zig **0.15.2** on `PATH` (libghostty-vt-sys builds Ghostty's
  libghostty-vt with it; first build takes several minutes).
- Optional: `GHOSTTY_SOURCE_DIR` = a Ghostty checkout at commit
  `a887df42c56f6de86c0fe6da9c4eeca37931e083` to skip the clone.
- Windows cross-build: `rustup target add x86_64-pc-windows-gnu` and
  mingw-w64 (`x86_64-w64-mingw32-gcc`).

## Commands

```sh
cargo build --release
cargo test                                   # core: 5 unit + 9 end-to-end (Linux, needs python3); gui: 31
cargo clippy --all-targets                   # must stay warning-free
cargo clippy --all-targets --target x86_64-pc-windows-gnu
cargo build --release --target x86_64-pc-windows-gnu
cargo build --profile dist --target <triple>   # size-optimised, stripped
cargo fmt                                    # rustfmt.toml: max_width 120
```

`ccterm-headless` drives any program without a window (see README), e.g.
`ccterm-headless --trace -e "wait-idle 3000 60000" -e modes -- claude`.
Use it to check behaviour against a real Claude Code before and after a change.
`CCTERM_DUMP_FRAME=f.ppm ccterm ...` writes every GUI frame to a file, which
is how rendering can be checked without a screen capture.

## Conventions

- Code, comments, docs and commit messages in English.
- Commits carry the user's git identity; no AI attribution or co-author lines.
- Do not push, tag or publish without asking.
- State what was verified and how (command and observed result); mark what
  was not verified, especially anything Windows-only, which cannot be run
  from Linux.
- Keep `docs/HANDOFF.md` current: move finished tasks to the status section,
  add new pitfalls when found.
