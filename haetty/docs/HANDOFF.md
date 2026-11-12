# haetty handoff

The current state, the rules in short, open defects and tasks, the person's checks and the pitfalls. History — dated
progress records, past measurements, solved defects, milestone 1's record — is in [`docs/history/`](history/) (§7 maps
the old sections). The code lives in `haetty/` of the `dhwan0112/claude-code-config` repository. Read `CLAUDE.md`,
`docs/DESIGN.md` and `docs/SELF-HOSTING.md` with this file. New progress records go to the newest file in
`docs/history/`, not here.

**Start with [NORTH-STAR.md](NORTH-STAR.md)** (the four points every stage serves) and "Next session" below.

## 1. Current state

- **Version** 0.3.1 (workspace; candidates F5 `b1262d2` = 0.3.0 and K4 `1163a0a` = 0.3.1, neither tagged: the Windows night run's focus check). Tags: `haetty-v0.1.0` = `4d75fcd`,
  `haetty-v0.1.1` = `d5ba833`, **`haetty-v0.2.0` = `63822de`** (tagged
  2026-11-08 after the overnight soak: LINUX PASS, WINDOWS PASS;
  check-range `haetty-v0.1.1..63822de` 0 failing; record in
  [history](history/2026-11-06-07-stages-t-u-v.md)).
- **Stable (installed by the user):** kramers has **v0.2.0 (`63822de`)**, installed 2026-11-07 with
  `tools/install-stable.sh 63822de` (checked 2026-11-09: [docs/checks/2026-11-09-kramers.md](checks/2026-11-09-kramers.md)).
  This line changes only when the user says so.
- **After `63822de`, untagged:** V3's observe-only interface (protocol 8), V6's palette fix, the native engine (E1–E5,
  F0–F4), K1–K4, #1 and #2 (CHANGELOG).
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

Stages:
- **P0 done (2026-11-12):** NORTH-STAR and STAGES; this list; #1's Xvfb regression test (`scripts/e2e/remap.sh`),
  #2's parsing test (already in `cd2e0e7`); DESIGN §2 button wording (approved); OPERATING's issue procedure.
- **P1 next: unblock releases.** New tag rule (Linux candidate checks + kramers' Linux night PASS + check-range 0
  failing; the Windows night no longer blocks, CHANGELOG says "Windows: verified / unverified / known problems"); fix the
  flaky `closed_sessions_leave_no_threads` (`tests/threads.rs`; failed 2 of 3 at `780b991`, a 30 s timeout) without
  retries; K1 follow-up (protected, approved scope in STAGES.md); v0.3.2 candidate; prepare the first Linux night run
  (scheduling, lid closed without sleep), scheduled when the person says "예약해"; tag `haetty-v0.3.2` (pre-approved)
  when the rule holds; §6.1 re-check list after the person installs it.
- P2: working inside haetty — coordinating session + workers, two small stages, `haetty tokens` comparison; a key to
  the longest-waiting worker (after the person installs v0.3.2).
- P3: sidebar and side-by-side diff overlay; IDE-integration research and prototype (off by default). DESIGN §2
  change approved for STAGES.md's scope; frame dumps in `docs/look/`.
- P4: session browser, fork, names, `haetty sessions --json`.
- P5: connections screen, `haetty mcp` (stdio, Observe only), `haetty mcp install`.
- P6: SSH and Tailscale through the system OpenSSH; isolated test sshd.
- P7: remote daemon design (docs only). P8: delegation design (docs only, the person decides).
- P9: tidy-up — token tools in dev scripts; Windows result hand-over and `NOT-RUN (foreground)`; the auto-mode deny path.

**On hold** (they do not move a point directly): the differences left to the newest Ghostty (F2), switching the
default engine, splitting the big files (H2), checking the default engine's slow memory growth.

Candidates: v0.3.0 (`b1262d2`) and v0.3.1 (`1163a0a`) are not tagged — each held by the Windows night run's focus
check (`checks/windows-2026-11-10`, `checks/windows-2026-11-11`: one Windows PASS in three runs of `1163a0a`; Linux PASS
in all). Under P1's rule they are superseded by v0.3.2. `main` after `1163a0a` adds #1 and #2 (CHANGELOG
"Unreleased").

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
- Unix socket paths are limited to 108 bytes: keep `HAETTY_TEST_TMP` short (the gate refuses > 32 characters).
- Ubuntu 26.04 has uutils coreutils: its `cat` stops after the first read from a raw terminal (use python3 in fixtures); kramers' `/tmp` is a tmpfs (RAM). `echo "$out" | grep -q` under `pipefail` fails when grep matches early (SIGPIPE): read a here-string (the gate did this until 2026-11-10).

Windows, WSL and the dev machine:
- WSLg windows get focus only when the Windows desktop allows it: check the cursor in a frame dump first.
- `SetForegroundWindow` is refused while the user is active: attach to the foreground thread's input, check the foreground process before every send, never type blind; GUI runs only on an idle desktop.
- Windows PowerShell 5 reads BOM-less UTF-8 as ANSI: use `pwsh` or a BOM.
- WSLg's X11 frame is white/grey and ignores `_GTK_THEME_VARIANT`: not haetty's padding.
- DWM colour attributes are set-only and `PrintWindow` misses the border: check with a real screen capture.
- `cargo build --target <triple>` writes `target/<triple>/`: plain `target/release/haetty` may be stale.
- From Git Bash, `wsl.exe -- bash -lc` expands `$VARS` on Windows and MSYS rewrites paths: `MSYS_NO_PATHCONV=1`, script files; WSL `/tmp` is cleared when the VM stops.
- `wsl -- bash -c` goes through the login shell (zsh), which expands first: run script files.
- Xvfb without sudo: an extracted root, `unshare -rm` overlay for `xkbcomp` and a tmpfs X socket dir (`scripts/e2e/README.md`).
- PowerShell 7 expands a leading `~` in native command arguments to the Windows home (U4).
- Scripts on `\\wsl.localhost` are unsigned: a new window needs `-ExecutionPolicy Bypass` (U5b).
- PowerShell function names are case-insensitive: a function `Git` calling `git` recursed forever (T5).
- From Git Bash, `printf > /mnt/c/...` can fail silently: write Windows files through their Windows path.
- Build directories other than `target/` (the gate's `target-native`, worktrees' `target-*`) reached the Dropbox mirror until E4; `tools/mirror-dropbox.sh` now skips `target-*/`. Files Dropbox holds are removed from Windows (`rmdir /s /q`), not through `/mnt/c`.
- `heavy.ps1` from Git Bash (E3): `-Label … -- pwsh …` printed pwsh's help; dot-sourcing it in `pwsh -Command` left `Enter-HaettyHeavy` undefined. Not yet understood.
- `tools/check-range.sh` builds into the usual target dir, and our crates' artifacts are named the same in every checkout: a build of another worktree into it meanwhile got stale or missing crates (E5). Give other builds their own `CARGO_TARGET_DIR` while it runs.
- Git Bash turns a `\\wsl.localhost\…` argument into `\wsl.localhost\…`: put such paths inside a PowerShell script file (E5).
- PowerShell resolves aliases before functions: a function `R` is shadowed by `r` (Invoke-History) (E5).
- Claude Code may stop idle background shells when memory runs low (seen on Windows too, against its docs, U3): keep long runs outside Claude Code.
- A Task Scheduler night run (2026-11-10) stopped at its first focus check; cause open. Unattended trial 2026-11-07 passed (`docs/history/2026-11-10-windows-handover.md` §2).

## 6. Next tasks

### 6.0 Open defects and tasks (2026-11-07)

- **kramers 2026-11-09 (0.2.0, `63822de`)**, [docs/checks/2026-11-09-kramers.md](checks/2026-11-09-kramers.md).
  Open failures: 7 (window size on Wayland at 125 %, still in 0.2.0; §6.1 item 32). Fixed in K1, re-check in K4:
  12 (the red `?` stayed after the answer). Other observations, not counted as failures: a tab's own name was lost on detach and reattach (fixed in K3b);
  a daemon killed with SIGKILL was a zombie of its window (fixed in K3a); test instances wrote their per-session settings into the shared `$XDG_RUNTIME_DIR/haetty/` (fixed in K3a; flagged for review); a tab started inside Claude Code inherited
  `CLAUDE_CODE_CHILD_SESSION` and the messaging socket and token, so transcript saving was off (fixed in K2 and K2b: the child
  markers are not passed on, docs/history/2026-11-09-k2-child-markers.md); the worker mark stayed `–` after a restart though its last report was stored (fixed in K3a); a `WARN` for the empty left button side (cosmetic).
  Pending: 2 (not applicable), 17 (reboot), 25 (X11, 200 %).

- The default engine stays libghostty. Open for the person (DESIGN §10.15, the transition table): the 744 vtdiff
  FAILs of ghostty-main (`docs/vtdiff/2026-11-09-f2.md`); the native engine's key-to-frame latency (+0.13 ms at the
  median); SGR throughput at the 80 % threshold; no native-only installer option; no Windows soak with libghostty.
- F5 (`b1262d2`): the cause of the Windows FAIL of 2026-11-10 (focus check) is open; see "Next session".
- **For the person: `decorations = auto` on Xfce.** `auto` picks `native` on every X11; DESIGN §3.12's reason (the
  window manager colours the bar, mutter follows `_GTK_THEME_VARIANT=dark`) does not hold on Xfce: xfwm4 4.18 does
  not read that property, so its bar keeps xfwm4's theme colours. `integrated` there would change the default look
  on Xfce — the person's choice (P0 record, history/2026-11-12-p0-north-star.md).
- Upstream drafts: `docs/upstream/README.md` (01 and 02 fixed at the pin; 03 still present; none sent).
- The open list of 2026-11-07 (E5, 79 vtdiff FAILs, the 2-hour soak, SGR 58 %) is kept in
  `docs/history/2026-11-09-g0-handoff-trim.md`.
- The Windows soak v2 (`scripts/e2e/windows/soak.ps1`) has never run end to
  end, and the Windows GUI e2e (item 29) ran once, 13/14: its first full
  runs are on an idle desktop.
- `tools/dev-run.ps1`: the build and a real start are not yet run on
  Windows (U5).
- Memory: the `.wslconfig` proposal (`memory=8GB`) waits for the user's
  decision; Claude Code reaped background shells on Windows although its
  docs say it does not (U3).
- The S4 soak may have shared an X server with `crashrun.sh` (T4); its
  figures are kept as recorded.
- **A7 (kramers 2026-11-06): on Wayland at a fractional scale the first
  window was about 1/scale of the requested grid** (`--cols 80 --rows 24`
  gave 64x19 at 125 %). Fix in V7 (DESIGN §3.10), passing on X11 (`scale.sh`).
  **Open risk:** the Wayland waiting path has not run on a compositor yet
  (WSLg needs an idle desktop; kramers §6.1 item 32).
- winit#4746 workaround to remove once winit has the fix (§4).
- A test daemon can outlive an abnormally ended test run: a `threads`
  test daemon ran 12 h with its folder gone (found in V5). Tests should
  stop their daemon in a guard that runs on panic too.

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
    the result file: `docs/dev-commands.md`, "Windows night run"). kramers: `tools/overnight-soak-linux.sh night`
    (`docs/dev-commands.md`, "Linux night run"; first run with the next candidate).

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

#### C. Windows 11 (and 10 where noted)

18. [kramers 2026-11-09: not run (Windows)] **Name and icon**: `haetty.exe` in Explorer (file icon, Properties),
    pinned to Start and the taskbar, in Alt+Tab, in the title bar — the new
    icon at every size; the window title "haetty"; `haetty install-desktop`
    says nothing is needed. Pin haetty to the taskbar, then start a copy of
    `haetty.exe` from another folder: its window joins the pinned button
    (same AppUserModelID); a `HAETTY_INSTANCE=dev` window gets its own.
19. [kramers 2026-11-09: not run (Windows)] **Migration and the old daemon**: with a ccterm installation, the first
    start copies `%APPDATA%\ccterm\config` and the tab sets
    (`%LOCALAPPDATA%\ccterm\tabs`); with `ccterm.exe` still open, haetty runs
    beside it (separate pipes) and says so; the ccterm window keeps working.
20. [kramers 2026-11-09: not run (Windows)] **Microsoft Korean IME** in `haetty.exe` running Claude Code:
    composition inline and underlined at the cursor, no caret at the start
    of the syllable, Hanja candidate window next to the cursor, commit on
    Space/Enter, Enter right after a syllable submits once; Ctrl+C while
    composing.
21. [kramers 2026-11-09: not run (Windows)] **Keys, mouse, clipboard, fonts, HiDPI** as in 5–9 (Ctrl+V also pastes
    on Windows); dark title bar on Windows 10 (Windows 11 colours do not
    apply there).
22. [kramers 2026-11-09: not run (Windows)] **With a real account**: 11–17 on Windows (taskbar flash instead of a
    notification for 13; the permission dialog after the synchronous hook,
    ~70 ms on the test machine).
23. [kramers 2026-11-09: not run (Windows)] **Status line on a machine without Git Bash** (Claude Code then runs it
    through PowerShell; could not be produced here — Claude Code finds Git
    Bash at its default install location even with Git off `PATH`): with
    haetty in a folder with spaces (e.g. `C:\Program Files\haetty`) or in a
    user folder with a space, the status row at the bottom of a Claude Code
    tab fills (`$ 0.00` …) within a few seconds. The injected command is the
    8.3 short path (checked to run in PowerShell 5.1 and 7 directly).
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

### 6.2 Known gaps, 6.3 Later milestones

Moved to [docs/history/2026-11-09-handoff-6-2-6-3.md](history/2026-11-09-handoff-6-2-6-3.md).

## 7. Where the old sections went (2026-11-07, V2)

The table of the old HANDOFF sections and where their text is now: [history/handoff-sections.md](history/handoff-sections.md)
(moved in G0, 2026-11-09).
