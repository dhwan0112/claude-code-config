# haetty — instructions for Claude Code

A lightweight terminal for running Claude Code, on **Windows and Linux both**.
The backend (PTY + terminal emulation) and the first GUI milestone (`haetty`,
crate `haetty-gui`) are done. Read `docs/HANDOFF.md` (state, verified facts,
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
- Rust only, except libghostty-vt (Zig, FFI inside `haetty-core`) and the
  ConPTY binaries. No Python or shell at run time.
- libghostty-vt objects are `!Send`: they live only on the engine thread in
  `haetty-core`. Frontends use `SessionHandle`, `SessionEvent` and `Snapshot`
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

## Working rules

- Self-hosting: develop inside the stable instance; run new builds only as a
  separate instance (`HAETTY_INSTANCE=dev`: own daemon, socket, state and
  config directories, app id, window title prefix). Never replace the
  stable binary yourself; promotion to stable needs the user's approval.
  - Never signal, stop or restart the stable instance or its daemon (it
    hosts this very session).
  - Manage processes of dev builds and tests only by PID or by their own
    socket — never by name: no `pkill haetty`, `killall haetty`,
    `taskkill /IM haetty.exe`, `Stop-Process -Name haetty`, and no
    `pkill -f` patterns. Scripts: `haetty_stop_daemon BIN` /
    `Stop-HaettyDaemon EXE` (`scripts/test-guard.*`) stop the daemon of the
    run's own socket by the PID `status --json` reports; children you start
    are stopped by their `$!` / `Start-Process -PassThru` PID.
  - Installing or replacing the stable binary is the user's job only.
- Several Claude Code sessions at once: each works in its own `git
  worktree` on its own branch (never two sessions in one checkout); a
  branch is merged into `main` only after it passed the full gate (both
  targets, clippy 0 on both, all tests), one session at a time — the next
  session rebases on the new `main` and passes the gate again before its
  merge. Protected-area changes follow the same review rule in every
  session.
- Protected areas — changes need the user's review before commit:
  `docs/DESIGN.md` §2 (outer design) and any change that alters reference
  frames (`crates/haetty-gui/tests/reference/`); hook output rules
  (observe-only, no stdout, exit 0: `haetty-cc/src/report.rs`); the
  control/observe/report permission checks in IPC (`Daemon::message` and
  `Daemon::input` in `haetty-daemon`); anything on the path of Claude Code
  permission prompts (attention `Ask`, `awaiting_permission`, the settings
  haetty injects).
- Test isolation (enforced in code): tests write and remove files only in
  their run directory — Rust tests through `haetty_testkit::run_dir` /
  `check` / `remove_dir_all` (a dev-dependency); shell scripts source
  `scripts/test-guard.sh` and call `haetty_guard_init HAETTY_TEST_TMP` and
  `haetty_guard PATH`; PowerShell dot-sources `scripts/test-guard.ps1`
  (`Initialize-HaettyGuard`, `Assert-HaettyPath`). An empty base variable or
  a path in the real HOME, `~/.config`, `~/.local`, `~/.claude`, XDG dirs,
  `%APPDATA%`, `%LOCALAPPDATA%` (and, in WSL, the Windows profile) is refused
  before anything is written. Bases: Linux `/tmp/…`; Windows
  `%PUBLIC%\haetty-test` (the Windows temp folder is under `%LOCALAPPDATA%`).
  Claude Code in tests: an isolated home in the run directory and a dummy
  key only.
- Keep a recovery path: do not remove or break ccx (tmux) while haetty is
  not yet the proven default.
- Commits and reference frames:
  - Every commit passes the full gate on its own (both targets, clippy 0
    on both, all tests including the reference frames) — no commit relies
    on a later one to become green.
  - An approved change of the outer design (DESIGN.md §2) puts the code and
    the updated reference frames into **one** commit whose message says
    "DESIGN §2 change, approved".
  - Only a reference-frame update without an intended change of the look
    (e.g. new fonts on the reference machine, a rendering fix that moves
    pixels) is a commit of its own, whose message says why the frames
    changed.
  - Update with `HAETTY_UPDATE_REFERENCE=1 cargo test reftest` (Linux) and
    the Windows test binary with `HAETTY_REFERENCE_DIR` (Windows); a rerun
    must report every frame identical on both.

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
cargo test                                   # core 8+12 (Linux, python3); gui 74 + instances, migrate, permission 1 each; proto 4; cc 11; daemon 1+5 (Unix); testkit 1
cargo clippy --all-targets                   # must stay warning-free
cargo clippy --all-targets --target x86_64-pc-windows-gnu
cargo build --release --target x86_64-pc-windows-gnu
cargo build --profile dist --target <triple>   # size-optimised, stripped
cargo fmt                                    # rustfmt.toml: max_width 120
cargo run --release -p haetty-core --example mem -- 3 67108864 10000   # anon memory per session (Linux)
```

Stable installs (the user runs them; `docs/DESIGN.md` §3.15, README):
`tools/install-stable.sh REF` / `tools/install-stable.ps1 REF`, rollback
with `--activate` / `-Activate`; self-tests `tools/test-install-stable.sh
REF1 REF2` / `.ps1` (test run directory only). `haetty --version` names the
commit, build date and instance.

`haetty-headless` drives any program without a window (see README), e.g.
`haetty-headless --trace -e "wait-idle 3000 60000" -e modes -- claude`.
Use it to check behaviour against a real Claude Code before and after a change.
`HAETTY_DUMP_FRAME=f.ppm haetty ...` writes every GUI frame to a file, which
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
