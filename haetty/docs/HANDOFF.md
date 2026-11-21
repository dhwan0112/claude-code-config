# haetty handoff

The current state, the rules in short, open defects and tasks, the person's checks and the pitfalls. History — dated
progress records, past measurements, solved defects, milestone 1's record — is in [`docs/history/`](history/) (§7 maps
the old sections). The code lives in `haetty/` of the `dhwan0112/claude-code-config` repository. Read `CLAUDE.md`,
`docs/DESIGN.md` and `docs/SELF-HOSTING.md` with this file. New progress records go to the newest file in
`docs/history/`, not here.

**Start with [NORTH-STAR.md](NORTH-STAR.md)** (the four points every stage serves) and "Next session" below.

> **In progress:** stage pq (branch `haetty/pq`, worktree `~/projects/claude-code-config-wt/pq`). Remaining: register stage bundle PQ (STAGES, HANDOFF order, OPERATING); PX first, PK after it

## 1. Current state

- **Version** 0.4.0: **`haetty-v0.4.0` = `b1fd31b`** (candidate 2, after PM, P2b, PI; Linux candidate check passed
  2026-11-20; tagged by the person 2026-11-20, release built with `tools/release.sh`, signed and uploaded as a draft
  by the person; record [history/2026-11-20-pi.md](history/2026-11-20-pi.md)); the first candidate `98a9b71` was
  never tagged. Earlier tags: **`haetty-v0.3.3` = `ca70269`** (2026-11-17, [history](history/2026-11-17-v0.3.3.md)),
  **`haetty-v0.3.2` = `622d629`** (2026-11-16, [history](history/2026-11-12-p1.md), "v0.3.2"), `haetty-v0.1.0` =
  `4d75fcd`, `haetty-v0.1.1` = `d5ba833`, **`haetty-v0.2.0` = `63822de`** (2026-11-08,
  [history](history/2026-11-06-07-stages-t-u-v.md)).
- **Stable (installed by the user):** kramers has **v0.4.0 (`b1fd31b`)**, installed 2026-11-20 01:06Z with
  `tools/install-stable.sh haetty-v0.4.0` (its folder's `INSTALLED`, read 2026-11-20; `~/.local/bin/haetty` points at
  `0.4.0-b1fd31b1c7f8`; 0.1.0, 0.1.1, 0.2.0 and 0.3.2 stay for rollback). This line changes only when the user
  installs another.
- **After `63822de`, in 0.3.2:** V3's observe-only interface, V6's palette fix, the native engine (E1–E5, F0–F4),
  K1–K4, #1, #2, P1a–P1c (CHANGELOG; protocol 12).
- Last stages (records in [history](history/)): T1–T7, U1–U6, V1–V7, E0–E5, F0–F6, G0–G7, K0–K4, H.
- **Development machine: kramers** (ThinkPad T480, Ubuntu 26.04, 8 threads, 29 GB; from 2026-11-10). Per-machine
  values: `tools/machine.sh`. The stable haetty installed here is the person's: no command goes to it.

## Next session

**Direction (2026-11-12): [NORTH-STAR.md](NORTH-STAR.md)** — the four points (OpenCode-like UI; SSH and Tailscale;
Claude Code with many jobs; other tools such as Rakazo), the conditions, and where each point stands. Every stage
report names the point(s) it moves. The person's full stage text: **[STAGES.md](STAGES.md)** (read the part of the
stage at hand). Roles: kramers (Linux) develops; the Windows PC only checks (`checks/windows-<date>`;
`docs/dev-commands.md`). One stage per session (`docs/OPERATING.md`); after `/clear` the person says "next stage".
The text this list replaced: [history/2026-11-12-p0-north-star.md](history/2026-11-12-p0-north-star.md).

**Windows (the person, 2026-11-19):** Windows checks and fixes happen once, after all other stages, in **PW**. Until
then no stage waits for a Windows result or does Windows-only work: the gate's `x86_64-pc-windows-gnu` build and
clippy stay (fix what fails); no merging of `windows/*`, no handling of `checks/windows-*`, no `#[cfg(windows)]` defect
fixes, no `.ps1` work, no Windows reference frames, no test ports to Windows. A new feature that needs Windows gets
the least that builds (a fallback or one line "not supported on Windows yet") and one line in PW's list.

Stages:
- **Done:** P0, P1a–P1c, PS, PU, P3 research, v0.3.3, PH, PM (P2 in real use, P3b), P2b — their lines, as they stood,
  are in [history/2026-11-20-p3c.md](history/2026-11-20-p3c.md), "Moved from HANDOFF", each with its record.
  **PI** (2026-11-20, [record](history/2026-11-20-pi.md), DESIGN §13.8): the shell integration (zsh through a
  `ZDOTDIR` folder, bash through `PROMPT_COMMAND`: the proxy folder first at every prompt; `shell-integration`;
  protocol 17; protected, approved), the sidebar's keyboard (Ctrl+Shift+E, the `›` mark; §2 change, approved), full
  paths between the sidebar and the diff overlay, **0.4.0 candidate 2** (tagged `haetty-v0.4.0`). **P3c** (2026-11-20,
  [record](history/2026-11-20-p3c.md), DESIGN §13.9): haetty as Claude Code's editor (`ide-integration`, off; a server
  per tab, `CLAUDE_CODE_SSE_PORT`, the proposal in the diff overlay, `a`/`d` as the person; protocol 18; protected and
  §2, approved; checked with a real Claude Code 2.1.329); `release.sh` executable, its notes beside the assets.
- **Next (proposed): P4 session browser (point 1), run the P2 way** while the person sits through it — a coordinating
  session in a dev instance makes the tasks with `haetty task new`; the person answers the workers' trust and
  permission questions. Left from PM's list: a queued message to a busy worker (P19, §8.4 protected), a merge helper
  (C11).
- P4: session browser, fork, names, `haetty sessions --json` (a `claude` started outside the proxy — another `claude`
  first on PATH, an alias — is still saved as the shell; the sidebar names it). P5: connections, `haetty mcp` (stdio,
  Observe) + install.
- P6: SSH, Tailscale (system OpenSSH). P7/P8: remote daemon, delegation (docs). P9: tidy-up (token tools, Windows
  hand-over, `NOT-RUN (foreground)`, the auto-mode deny path).
- **PW: Windows catch-up (last).** Inputs, untouched until then: the branches `checks/windows-2026-11-17` (record
  `docs/checks/2026-11-17-windows.md`; its list "Not fixed here" is under "For kramers"), `windows/2026-11-17-fixes`
  (11 commits on `e8a48be`; three merge conflicts and their resolution in that record's "For kramers" table) and
  `windows/2026-11-17-ph-fixes` (4 commits on `58386ab`); the Windows check list in `docs/dev-commands.md`; and the
  lines stages add here:
  - P3c (2026-11-20): no editor server on Windows (`ide-integration` does nothing there; the token's random source is
    `/dev/urandom`); `tests/ide.rs` and `ide.sh` Unix only; no Windows reference frame `ide-proposal`.
  - PI (2026-11-20): no shell integration on Windows (the daemon makes no `zsh` folder there; zsh/bash under
    MSYS or Git Bash untested; `tests/shell.rs` Unix only); the sidebar's keyboard (Ctrl+Shift+E) and its `›` mark
    unverified, no Windows reference frame `sidebar-focus`; the full-path match not run on Windows paths.
  - P2b (2026-11-19): `haetty` in the proxy folder is Unix only (Windows: a `haetty.cmd` beside `claude.cmd`);
    `task new`, `workers`, `wait worker` and the detached-worker click unverified on Windows (`coordinator.sh` is
    Linux only); P13's `(deleted)` path is Linux behaviour (Windows locks a running executable).
  - PM (2026-11-19): the diff overlay (P3b) unverified on Windows — `diff.sh` is Linux only, the three `diff-*`
    scenes have no Windows reference frames; the Windows items of §6.0 and the Windows pitfalls of §4 moved to
    [history/2026-11-19-pm.md](history/2026-11-19-pm.md), "Moved from HANDOFF".

**On hold** (no point moves): the differences to the newest Ghostty (F2), the default engine, H2, slow memory growth;
since PH (2026-11-17) the terminal itself — engine, soaks, look polish, release tools — gets defect fixes only.

## 3. Rules in short (`CLAUDE.md` is the source)

- Both targets build; clippy 0 on both; every commit passes the gate on its
  own; before a push `tools/check-range.sh origin/main..HEAD` ends with
  "0 failing". `tools/known-gate-failures` lists the four pushed commits
  of stage S4 that fail (KNOWN); a new entry needs the user's approval.
- At most two heavy jobs at a time, Linux and Windows together, through
  `tools/heavy.sh` / `tools/heavy.ps1` (the scripts do it themselves).
- Tests: `HAETTY_TEST_TMP` run directories only, an isolated home and a
  fake key for Claude Code, `CLAUDE_CODE_*` / `CLAUDECODE` /
  `CLAUDE_*SESSION*` stripped.
- Never signal, stop or replace the stable instance, its daemon or its
  install. Processes only by PID or socket, never by name. New builds run
  as `HAETTY_INSTANCE=dev` (`tools/dev-run.sh`, `docs/SELF-HOSTING.md`).
- Protected areas (review before commit): DESIGN §2 and the reference
  frames, `haetty-cc/src/report.rs` hook output rules, the IPC role checks
  (`Daemon::message`, `Daemon::input`), the permission-prompt path and the
  settings haetty injects.
- No typed or shown text in logs (`Secret`). `CCTERM_*` names stay readable.
  Commits carry the user's identity, without AI or co-author lines.

## 4. Pitfalls already hit (do not repeat)

One line each; the full text with details and evidence is in
[`history/pitfalls.md`](history/pitfalls.md) (same order).

Backend and ConPTY:
- ConPTY flag 0x8 is `PSEUDOCONSOLE_GLYPH_WIDTH_GRAPHEMES` now: never send it to the in-box host.
- portable-pty hard-codes ConPTY flags and cannot pick the DLL: hence our own Windows backend.
- The in-box conhost drops mouse modes, APC and kitty keys; the embedded OpenConsole 1.25 passes kitty flags.
- `conpty.dll` silently falls back to the system conhost without `OpenConsole.exe` beside it: ship both.
- libghostty-vt callbacks run inside `vt_write`: only queue there, never call back into the terminal.
- OSC 9 / 777 / 99 and OSC 9;4 are not surfaced by libghostty-vt: `haetty-core/src/osc.rs` scans the stream.
- OSC 10/11 get no reply unless default fg/bg are set on the terminal.
- Wheel: send button 4/5 presses only; a release reads as a second notch.
- Claude Code caps colours at 256 when `TMUX` is set: `Session` removes `TMUX`/`TMUX_PANE`.
- Claude Code's first byte takes seconds on a cold start: wait with `wait-text` / a large `wait-idle`.
- A background grandchild can hold the PTY: `Exited` comes after 500 ms grace without EOF.
- portable-pty's Unix writer types "\n"+EOF with a blocking write on drop: haetty uses its own non-blocking reader/writer (P1a).
- The libghostty-vt crate is pinned `=0.2.2`; upgrade deliberately.

GUI (winit, fonts, rendering):
- winit#4746 is not fixed in 0.30.13: `app.rs::preedit_cursor` works around it (Windows); remove when fixed.
- winit X11 double-counts XTest wheel clicks (`xdotool click 4` = two notches); do not halve; `click` without `--window`.
- softbuffer's buffer is not a back buffer: keep our canvas, check `buffer.age()`.
- A snapshot with another grid size must force a full repaint.
- X11 ignored `request_inner_size` right after creation: set `with_inner_size` up front.
- smithay-clipboard pinned `=0.7.2` (0.7.3 pulls a second sctk, +89 KB).
- fontdb without `memmap` has no shared files: the GUI maps font files itself (memmap2).
- swash's `underline_offset` sign varies: use its absolute value below the baseline.
- Cascadia Mono on Windows 11 is variable: fontdb sees only regular, bold/italic synthesised.
- WSL has no Noto Sans Symbols 2 (`⏸` missing there); Windows uses Segoe UI Symbol.
- libghostty-vt's `max_scrollback` is bytes, not lines (`scrollback-limit`, default 12 MB).
- glibc keeps freed large buffers: map font files instead of reading them (16 MB anonymous memory saved).
- Debian names it "Noto Sans Symbols2" (no space): list both names; check with `fc-scan`.
- VS15/VS16 stay in the cell text; presentation must come from the text, not the width.
- Hinting can make a larger size 1 px narrower: pixel tests need 1 px tolerance.

Testing and Claude Code:
- Testing Claude Code inside a Claude Code session leaks `CLAUDE_CODE_*` markers: strip them (and `CLAUDECODE`).
- With `CLAUDE_CONFIG_DIR`, Claude Code reads `$CLAUDE_CONFIG_DIR/.claude.json`; write it with a JSON library.
- Fullscreen Claude Code: Ctrl+C with a selection copies; a test that drags must not expect Ctrl+C twice to exit.
- Claude Code's double Ctrl+C window is 700–1500 ms; right after start a second press may be missed: wait a few seconds.
- Two private Xvfb on the same display number share the abstract X socket across `unshare -rm` (T4): `xvfb.sh` picks a free one.
- check-range of commits before `0a23bf0` / K2 on kramers needs GNU `cat` and no session markers: `check-range.sh` sets both (P1c).
- Unix socket paths are limited to 108 bytes: keep `HAETTY_TEST_TMP` short (the gate refuses > 32 characters).
- Never probe an older haetty with a new subcommand: it opens a window and reaches the running daemon; grep `--help` (PU).
- Asking a daemon whether it still runs by connecting keeps it busy (no idle exit): watch its PID instead (PU).
- A script typing blind into a palette breaks silently when it gains a step: check the result (soaks made no task G5→PS).
- Ubuntu 26.04 has uutils coreutils: its `cat` stops after the first read from a raw terminal (use python3 in fixtures); kramers' `/tmp` is a tmpfs (RAM). `echo "$out" | grep -q` under `pipefail` fails when grep matches early (SIGPIPE): read a here-string (the gate did this until 2026-11-10).

The dev machine (the Windows and WSL ones: [history/2026-11-19-pm.md](history/2026-11-19-pm.md), for PW):
- `cargo build --target <triple>` writes `target/<triple>/`: plain `target/release/haetty` may be stale.
- Xvfb without sudo: an extracted root, `unshare -rm` overlay for `xkbcomp` and a tmpfs X socket dir (`scripts/e2e/README.md`).
- Build directories other than `target/` (the gate's `target-native`, worktrees' `target-*`) reached the Dropbox mirror until E4; `tools/mirror-dropbox.sh` now skips `target-*/`. Files Dropbox holds are removed from Windows (`rmdir /s /q`), not through `/mnt/c`.
- `tools/check-range.sh` builds into the usual target dir, and our crates' artifacts are named the same in every checkout: a build of another worktree into it meanwhile got stale or missing crates (E5). Give other builds their own `CARGO_TARGET_DIR` while it runs. The same with `tools/stage/e.sh`: a main-checkout test can run the worktree's build (`touch` the file; P3).
- Claude Code may stop idle background shells when memory runs low (seen on Windows too, against its docs, U3): keep long runs outside Claude Code.
- Replacing a running daemon's binary file in place: since P2b the hooks use the file now at that path (a new build there may speak another protocol than the running daemon); still start a new build beside, never `mv` over a running one (P13, history/2026-11-19-pm.md).
- Claude Code with `blockReadsOutsideWorkingDirectories` asks before any Read outside its folders: briefs, test output and frame dumps a worker must open belong inside its worktree (PM).
- In a tab of a daemon older than PI, `haetty` and `claude` are not the proxy's when `.zshrc` puts `~/.local/bin` first; call `$HAETTY_PROXY_DIR/haetty` there (P2b; the shell integration fixes it from PI on).
- `script -c CMD` runs `$SHELL -c CMD`: dash drops `BASH_FUNC_…%%` variables (haetty's bash hook) and a zsh `$SHELL` reads haetty's `.zshenv` first in a non-interactive shell; test shells as the tab's own program (PI).
- `haetty …` with a non-zero exit prints its line on stderr: an e2e check of that line captures `2>&1` (P2b).
- An e2e run from a Claude Code session inside haetty inherits `HAETTY_PROXIED=1`: the dev proxy passes `claude` through; unset it and `HAETTY_PROXY_DIR`, `HAETTY_CLAUDE_SETTINGS`, `HAETTY_SESSION_ID` (P3c).
- Claude Code 2.1.329's folder-trust question selects "No, exit" first: a script answers Down, Enter (P3c).

## 6. Next tasks

### 6.0 Open defects and tasks (2026-11-07)

- **kramers 2026-11-09 (0.2.0, `63822de`)**, [docs/checks/2026-11-09-kramers.md](checks/2026-11-09-kramers.md).
  Open failures: 7 (window size on Wayland at 125 %, still in 0.2.0; §6.1 item 32). Fixed in K1, re-check in K4:
  12 (the red `?` stayed after the answer). Other observations, not counted as failures: a tab's own name was lost on detach and reattach (fixed in K3b);
  a daemon killed with SIGKILL was a zombie of its window (fixed in K3a); test instances wrote their per-session settings into the shared `$XDG_RUNTIME_DIR/haetty/` (fixed in K3a; flagged for review); a tab started inside Claude Code inherited
  `CLAUDE_CODE_CHILD_SESSION` and the messaging socket and token, so transcript saving was off (fixed in K2 and K2b: the child
  markers are not passed on, docs/history/2026-11-09-k2-child-markers.md); the worker mark stayed `–` after a restart though its last report was stored (fixed in K3a); a `WARN` for the empty left button side (cosmetic).
  Pending: 2 (not applicable), 17 (reboot), 25 (X11, 200 %).

- Engine, F5, Xfce `decorations = auto`, upstream drafts (on hold since PH): moved to [history/2026-11-17-ph-harness.md](history/2026-11-17-ph-harness.md), "Moved from HANDOFF".
- The open list of 2026-11-07 (E5, 79 vtdiff FAILs, the 2-hour soak, SGR 58 %) is kept in
  `docs/history/2026-11-09-g0-handoff-trim.md`.
- The S4 soak may have shared an X server with `crashrun.sh` (T4); its
  figures are kept as recorded.
- **A7 (kramers 2026-11-06): on Wayland at a fractional scale the first window was about 1/scale of the requested
  grid** (`--cols 80 --rows 24` gave 64x19 at 125 %). Fix in V7 (DESIGN §3.10), passing on X11 (`scale.sh`).
  **Open risk:** the Wayland waiting path has not run on a compositor yet (WSLg needs an idle desktop; §6.1 item 32).
- winit#4746 workaround to remove once winit has the fix (§4).
- A test daemon can outlive an abnormally ended test run: a `threads` test daemon ran 12 h with its folder gone
  (found in V5); 10 daemons of the `ime` e2e runs of 2026-11-16 16:05 still ran on 2026-11-17 (stopped by PID).
  Tests should stop their daemon in a guard that runs on panic too.

### 6.1 Manual checks (by a person; regrouped 2026-11-05 after the rename)

Re-check on 0.3.1 (kramers 2026-11-09): 7, 12 and 25 (X11, 200 %); K2 (a dev GUI tab started from Claude Code, with
the inherited markers); the K3 items (tab names, the zombie, the settings directory, a resumed report, the `CCTERM_*` names).

**Results so far** (newest first; files in [`docs/checks/`](checks/)):

| Date, machine, build | Pass | Fail | Pending | File |
|---|---|---|---|---|
| 2026-11-06 kramers (ThinkPad T480, Ubuntu 26.04 GNOME Wayland, lid closed, one monitor at 125 %), 0.1.1 | 3, 9, 10 | **7** (window size at a fractional scale: §6.0) | 1 (automatic part passes), 2 (n/a: no ccterm), 4–6, 8 (copying works), 11–17 | [2026-11-06-kramers.md](checks/2026-11-06-kramers.md) |
| 2026-11-09 kramers (ThinkPad T480, Ubuntu 26.04 GNOME Wayland, lid closed, one monitor at 125 %), 0.2.0 (`63822de`) | 1, 3–6, 8–11, 13–17 (17 except the reboot), 25 (Wayland), 26–28 | **7**, **12** | 2 (not applicable), 17 (reboot), 25 (X11, 200 %); not run: 18–24, 29, 31 | [checks/2026-11-09-kramers.md](checks/2026-11-09-kramers.md) |

What automation could not show: real keyboards, IMEs, displays, desktops,
and a logged-in Claude Code (the tests use a fake key, so Claude Code never
answers, edits or asks for permission). "Real account" = Claude Code logged
in. Commands below assume `haetty` is on `PATH` (or run it by its path);
for a build beside your stable one use `HAETTY_INSTANCE=dev haetty …`.

#### A. Linux, no real account needed (a shell, `vim`/`less`, or Claude Code's start screen)

1. [kramers 2026-11-09: pass] **Name and icon.** `haetty install-desktop`, then on GNOME/KDE: the
   launcher shows "haetty" with the new icon (window with an orange bar,
   grey `❯_`); a started window is matched to it in the dock (X11 and
   Wayland session: `WM_CLASS`/`app_id` = `io.github.dhwan0112.haetty`);
   the icon at the smallest sizes (top bar, Alt+Tab, a 16/24 px panel) —
   `❯`, cursor and orange bar still recognisable.
2. [kramers 2026-11-09: not applicable (no ccTerm settings on kramers)] **Migration from ccterm** (only if you used ccterm): the first `haetty`
   start shows "settings and tab sets copied from ccterm"; your
   `~/.config/ccterm/config` settings apply; the palette's "Open the
   previous run's tabs" offers your old tabs; the ccterm files are still
   there. With a `ccterm` window still open: the hint says a ccterm daemon
   is running, and that window keeps working.
3. [kramers 2026-11-09: pass] **Old variable names**: `CCTERM_LOG=info haetty` still logs, with one
   line asking to rename it to `HAETTY_LOG`.
4. [kramers 2026-11-09: pass] **Hangul IME**: ibus or fcitx5 under native Wayland
   (`zwp_text_input_v3`) and under X11 (XIM): composition inline at the
   cursor, commit on Space/Enter, Hangul typed into the palette (Ctrl+Shift+P).
5. [kramers 2026-11-09: pass] **Keys and tabs** with a real keyboard: Ctrl+Shift+T/W (W twice),
   Ctrl+Tab / Ctrl+Shift+Tab, Alt+1..9, Ctrl+Shift+P, Ctrl+Shift+V,
   Shift+Insert; the tab bar and hints legible on your display.
6. [kramers 2026-11-09: pass] **Mouse**: a wheel notch = one step in a full-screen program; touchpad
   scrolling smooth enough.
7. [kramers 2026-11-09: FAIL, still in 0.2.0 (open, §6.0)] **Window**: drag the edge to resize (padding while dragging); HiDPI
   150–200 % on Wayland and moving between monitors of different scale;
   dark title bar on GNOME/KDE X11 (`_GTK_THEME_VARIANT`) and the dark
   adwaita decorations on Wayland.
8. [kramers 2026-11-09: pass] **Clipboard** with a real clipboard manager on X11 and Wayland: copy in
   the terminal, paste elsewhere; paste a large text (bracketed paste).
9. [kramers 2026-11-09: pass] **Fonts**: `--font` with a few families; Hangul next to Latin (size and
   baseline), the symbols ⏸ ⚠ ✔ ● (judged from frame dumps only so far).
10. [kramers 2026-11-09: pass] **Reattach** without Claude Code: open two tabs with shells, `kill -9`
    the haetty window (or let it crash), start `haetty` again from the
    launcher — both shells come back as they were (same history), nothing
    restarted; a second window killed → palette "Attach detached sessions"
    takes its tabs.

25. [kramers 2026-11-09: pass on Wayland; X11 and 200 % pending] **Integrated title bar on GNOME** (Wayland session, `decorations`
    unset = auto): one colour from the title row to the bottom (no `#303030`
    bar); the window buttons where GNOME puts them (Ubuntu: minimize,
    maximize, close on the right; change it in Tweaks → Window Titlebars and
    restart haetty); drag empty title space to move, double-click to
    maximize/restore, drag each edge and corner to resize — also at 200 %
    scale; buttons dim, brighter under the pointer, no colour on ×; the
    window's corners and shadow (mutter's, not haetty's: record what you
    see); a right click on the title row does nothing. X11 session: the
    system title bar again (auto = native), dark.

26. [kramers 2026-11-09: pass] **Tab features with a real Claude Code** (stage S2): in a git
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

27. [kramers 2026-11-09: pass] **Orchestrator with a real Claude Code** (stage S3; follow README
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

28. [kramers 2026-11-09: pass] **Daemon crash with real Claude Code tabs** (stage S4): with two Claude
    Code tabs in a dev instance, end the dev daemon by its PID (`haetty
    status --json` → `daemon_pid`); the window stays with the hint, no
    `claude` process of those tabs is left (`ps` / Task Manager); start haetty
    again — both conversations resume. On Windows also check that no
    `node`/`claude` or `OpenConsole` process of those tabs stays.

29. [kramers 2026-11-09: not run] **Windows GUI end-to-end runs** (`tabs.ps1`, `orchestrator.ps1`; need
    a desktop nobody uses): at night with item 30's `-GuiE2E` (`RESULT
    GUI-TABS` / `GUI-ORCHESTRATOR`), or by hand (`pwsh -ExecutionPolicy
    Bypass -File scripts\e2e\windows\tabs.ps1 -Exe <haetty.exe>`). Expect
    GUI-TABS FAIL for `63822de` (the Hangul rename fix is `6a67b22`);
    `main` passed 14/14 and 17/17 (V6).

30. **Night runs.** Windows PC: `tools/overnight-soak.ps1` from a PowerShell window (the command, the rules and
    the result file: `docs/dev-commands.md`, "Windows night run"). kramers: the short soak is in the candidate check;
    the one-hour soak is a stage's own check (`docs/dev-commands.md`, "Short soak", "One-hour soak"; P1c).

31. [kramers 2026-11-09: not run] **Observing a tab** (V3): palette "Observing: allow or stop for this
    tab" — a dim `◦` appears after the tab's number (grey, also on the
    active tab; no colour) and goes again with the same entry; `haetty
    screen --json <id>` (id from `haetty status`) shows that tab and refuses
    another one; `haetty events --follow` prints its events while you work
    in it (never what you type). Linux and Windows.

32. [kramers 2026-11-09: open: the 2026-11-09 run was on 0.2.0 and failed (see 7)] **A7 again on kramers** (fixed after `b442fff`): Wayland, lid closed,
    the external monitor at 125 % (then 100 % and 200 %):
    - `haetty --cols 80 --rows 24 bash -c 'sleep 3; tput cols; tput lines;
      sleep 2'` prints 80 and 24;
    - with `HAETTY_LOG=debug` the log has `scale 1.25: keeping the 80x24
      grid` and `opening tabs at a 80x24 grid`, and no `grid 64x19`;
    - moving the window between monitors of different scales keeps 80x24;
      Windows likewise at 125 %/150 %.
33. [new, PS 2026-11-16] **Settings button and panel** (DESIGN §2.4): with a real mouse, the shell tab's ⚙ appears
    near the bottom-right corner and goes when you type; a Claude Code tab's ⚙ is in its status row; a click, Ctrl+,
    and the palette open the panel; at 150–200 % the click hits the ⚙ cell; a font family typed with the Hangul IME is
    applied or refused with a reason; your config keeps its comments (`config.bak` beside it). Windows: the same.
34. [new, PU 2026-11-16] **Releases and updates** (SELF-HOSTING "Releases"): the minisign key and `haetty.pub` commit;
    a tag, `tools/release.sh` in your terminal, the draft checked and published; an installed haetty updates from the
    panel (Restart now brings the tabs back, Roll back returns). Windows: the Windows check list.
35. [new, PH 2026-11-17] **Agents in real work**: a Claude Code session that starts five or more subagents (some
    nested, some in the background): the sidebar's Agents follow (parent, model, tool, `◌`/`●`, `✓` and STATUS),
    a click opens an agent's record, Ctrl+Shift+A lists every tab's agents; idle CPU and keys stay quick while they
    run; `claude` typed in your zsh tab becomes a Claude Code tab (from PI also when your `.zshrc` puts `~/.local/bin`
    first on PATH: the shell integration). Windows: `claude` in cmd, PowerShell
    and Git Bash tabs (the Windows check list).
36. [new, PI 2026-11-20] **Shell integration and the sidebar's keyboard**: in your own zsh, `claude` and `haetty` typed
    in a new tab are haetty's (`whence -p claude` → the runtime folder's `bin`); your prompt and startup files behave
    as before; Ctrl+Shift+E, ↑/↓, Enter, Esc in the docked sidebar; nothing typed meanwhile reaches the program.

#### B. Linux with a real account (Claude Code logged in)

11. [kramers 2026-11-09: pass] **Ctrl+C twice** right after Claude Code starts, a few times (rare
    misses were seen only within the first seconds, §3).
12. [kramers 2026-11-09: FAIL; fixed in K1, re-check in K4] **Attention marks**: in a background tab, a permission prompt shows the
    red `?` *already when the dialog appears* (the hook is synchronous; no
    noticeable delay before the dialog); Stop shows the amber dot;
    selecting the tab clears amber; answering clears `?`.
13. [kramers 2026-11-09: pass] **Unfocused window**: when Claude Code finishes or asks, the window gets
    the urgency hint and a desktop notification (`notify-send`; off with
    `notifications = false`), with no session text in it.
14. [kramers 2026-11-09: pass] **Status row and panel**: context %, 5h/7d limits, cost match Claude
    Code's `/status`; "esc to interrupt" while it works; Ctrl+Shift+S panel
    (reset times, subagents with durations, last edit).
15. [kramers 2026-11-09: pass] **Edits and diff**: after an Edit or Write by Claude Code, "N files
    changed · Ctrl+Shift+D"; Ctrl+Shift+D shows exactly that edit (side by
    side at ≥ 110 columns); a multi-MB Write shows no hook error, with and
    without the daemon running.
16. [kramers 2026-11-09: pass] **Permission prompt and the palette**: while Claude Code asks, the
    palette's "Paste" is refused with a hint; Ctrl+Shift+V typed by you
    goes through.
17. [kramers 2026-11-09: pass; the reboot case pending] **Resume and reattach**: kill the window during a long Claude Code turn
    and start haetty again → the same conversation continues in its tab
    (not restarted, no second `--resume`); after a reboot (daemon gone) the
    tabs reopen and each Claude Code tab resumes its conversation once.
37. [new, P3c 2026-11-20] **haetty as the editor** (`ide-integration = true`, new tabs): an Edit or Write Claude Code
    asks about opens the diff overlay; `a` makes Claude Code write it, `d` rejects it, Esc leaves the question in the
    terminal; answering in the terminal closes the overlay.

#### C. Windows 11 (and 10 where noted)

Items 18–23 (name and icon, migration, the Korean IME, keys and HiDPI, a real account, the status line without
Git Bash) moved for stage PW to [history/2026-11-19-pm.md](history/2026-11-19-pm.md), "Moved from HANDOFF".

24. [kramers 2026-11-09: not run (Windows)] **Theme** (Linux and Windows): IBM 5153 CGA on your display — text
    `#cecece` on `#141414`, colour 3 brown, blue dark; the title bar
    (Windows 11), padding, tab bar and status row in the same greys; the
    waiting mark brown, the question mark red. `theme = haetty classic`
    brings the old look back; a Ghostty theme file in the themes folder
    works by its name (for a `HAETTY_INSTANCE=dev` build the folder is
    `~/.config/haetty-dev/themes`, Windows `%APPDATA%\haetty-dev\themes`);
    `minimum-contrast = 3` makes dark blue text white.
    In Claude Code, `/theme` → "Dark mode (ANSI colors only)": its screen
    then uses the theme's colours.

## 7. Where the old sections went (2026-11-07, V2)

The table of the old HANDOFF sections and where their text is now: [history/handoff-sections.md](history/handoff-sections.md)
(moved in G0, 2026-11-09). §6.2 Known gaps and §6.3 Later milestones: [history/2026-11-09-handoff-6-2-6-3.md](history/2026-11-09-handoff-6-2-6-3.md).
