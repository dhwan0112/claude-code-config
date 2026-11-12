# haetty — instructions for Claude Code

A lightweight terminal for running Claude Code, on **Windows and Linux both**.
The backend (PTY + terminal emulation) and the first GUI milestone (`haetty`,
crate `haetty-gui`) are done. Read the list under "Read at session start" once at
the start of a session.

## Read at session start

- `docs/NORTH-STAR.md`: what haetty is for — the four points every stage moves forward (name them in each
  stage's report), the conditions that stay, and where each point stands. The person's stage text: `docs/STAGES.md`.
- `docs/HANDOFF.md`: the state, the rules in short, the open items and the person's checks.
- `docs/DESIGN.md`: the index of the design (one line per section, with its file). Open only the file of the
  section you need, in `docs/design/`.
- `docs/OPERATING.md`: how a stage runs (one stage per session, the scripts in `tools/stage/`, the limits); read it
  at the start of each stage.
- Read `docs/SELF-HOSTING.md` (the day, dev → stable, rollback), and `docs/dev-commands.md` (commands, environment,
  tools) when you need them.

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
  a local socket later (`docs/history/2026-11-04-05-milestone-1.md`, old
  HANDOFF §7–§8).
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
  Start the dev instance with `tools/dev-run.sh` (Windows `tools/dev-run.ps1`):
  it builds the tree, shows dev beside stable and refuses anything shared.
  `docs/SELF-HOSTING.md` has the day, dev → stable, rollback and the way
  back to ccx.
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
- Keep a recovery path: do not remove or break ccx (tmux) while haetty is
  not yet the proven default.
- Test isolation: tests write and remove files only in their run directory; the guard scripts
  (`scripts/test-guard.*`) refuse an empty base and the real home, config and profile folders. Full rule:
  `docs/dev-commands.md`.
- Commits: every commit passes the full gate on its own; before each push `tools/check-range.sh
  origin/main..HEAD` must end with "0 failing". Adding, changing or removing an entry of
  `tools/known-gate-failures` needs the person's approval; a pushed commit that fails is recorded in HANDOFF,
  not rewritten. An approved change of the outer design (DESIGN §2) is one commit, "DESIGN §2 change, approved".
  Full rules, including reference frames: `docs/dev-commands.md`.
- Heavy jobs: at most two at a time, Linux and Windows together (`tools/heavy.sh`, `tools/heavy.ps1`; the test
  and build scripts take a slot themselves; `tools/heavy.sh --status` shows who holds them).
- vtdiff known differences (`docs/vtdiff/known-vt-differences.txt`): an entry is added directly only when
  `ghostty-main` agrees with haetty and an ECMA-48 or xterm clause gives the reason; otherwise it goes to
  `docs/vtdiff/known-candidates.txt` for the person. Changing or removing an entry needs the person's approval.
  Details: `docs/dev-commands.md`.

## Tools for long jobs (haetty)

- Builds and test runs that print much: `haetty run -- COMMAND` (the full output goes to a log; the terminal gets the
  exit code, the error lines and the last lines). Read the log in parts with `haetty log`.
- Waiting for a background job or a tab: `haetty wait …` (one line; exit 124 on timeout), not a `sleep` loop.
- A tab's screen: read it as text (`haetty screen`) before taking an image.
- Where a session's tokens went: `haetty tokens --top 10 PATH`.
- Big files: read them in parts (`Read` with `offset` and `limit`, or `grep -n` first); the list and the rules are in
  `docs/tokens/partial-reads.md`.
- The guide for Claude Code is `docs/tokens/claude-guide.md`; haetty adds it to no session.

## Commands, environment and tools

Commands, environment (Rust, Zig), the vtdiff and end-to-end tools, the stable installs and `haetty-headless`:
`docs/dev-commands.md`.

## Conventions

- Code, comments, docs and commit messages in English.
- Commits carry the user's git identity; no AI attribution or co-author lines.
- Do not push, tag or publish without asking.
- State what was verified and how (command and observed result); mark what
  was not verified, especially anything Windows-only, which cannot be run
  from Linux.
- Keep `docs/HANDOFF.md` short (under 400 lines): current state, rules in
  short, open defects and tasks, the person's checks (§6.1), the pitfall
  list (one line each). Dated progress records, measurements and solved
  defects go to the newest file in `docs/history/`; new pitfalls get one
  line in HANDOFF §4 (details in the record). When moving text, check with
  `tools/check-moved-lines.py OLD NEW…` that no line was lost.
