# Stages P0–P9 (the person's instruction of 2026-11-12)

The person's full instruction, in English, kept so that a session after `/clear` has every detail. The four points and
the conditions are in [NORTH-STAR.md](NORTH-STAR.md); the state of each stage is in [HANDOFF.md](HANDOFF.md), "Next
session". Approved protected-area scopes are quoted here exactly as given; anything beyond them is proposed, not done.

## How stages run

- One stage per session (`docs/OPERATING.md`). At the end write the next stage into HANDOFF and stop; after `/clear` the
  person says "next stage". A small docs-only stage may be followed by the next one in the same session. A long stage is
  split into sessions (research / implementation / test) with the remaining work in HANDOFF.
- `tools/stage/`, the heavy slots, the gate and check-range, then push and the Dropbox mirror. Commits carry the
  person's git identity, no AI mark. A new dependency: report the dist size change.
- Never touch the stable haetty on this machine, its daemon or `~/.local/opt/haetty`. Tests only in dev instances and
  test run directories. Installing or replacing stable is the person's job. The person's config files: show the change,
  get approval, back up, then edit.
- Protected areas (outer design §2, hook output rules, role and permission checks, the permission-prompt path): only
  what the person approved below, in a separate commit whose message says "protected area, approved" and the scope.
  If the scope must be exceeded: propose and stop.
- Fewer approval round trips: where the criterion is written below, follow it; where a choice is needed, take the
  option closest to the four points and record why. Ask the person only for protected-area changes outside the scope
  and for large choices of the look.
- Windows-only checks do not block: mark "Windows unverified" and add them to the Windows check list in
  `docs/dev-commands.md`.
- Report at the end of each stage, short: the point(s) moved, what was done, the evidence (commands and results,
  frame-dump paths), what stays Windows unverified, what the next stage needs, what the person must do. A stage that
  changed the look always gives frame-dump paths.

## P0. Direction (docs; may continue into the next stage)

- `docs/NORTH-STAR.md`: the points and conditions on one page; pointed to from the top of `CLAUDE.md` and HANDOFF.
- A status table per point (done, not done, next step) drawn from the records, in the same file.
- HANDOFF's stage list becomes P0–P9. Put on hold, with the reason (does not move a point directly): closing the
  differences between the new engine and the newest Ghostty, switching the default engine, splitting big files,
  checking the default engine's slow memory growth.
- The two issue fixes of 2026-11-11: an Xvfb regression test for `70f97a4` (repaint after hide and show), a unit test
  of the setting-string parsing for `cd2e0e7` (Xfce buttons) if there is none. DESIGN §2's button-layout wording
  becomes "follows the desktop's window-button layout setting (GNOME: button-layout, Xfce: xfwm4's button_layout,
  otherwise right)" (protected area, approved with this wording). Record in CHANGELOG and HANDOFF. Check and report
  what the `auto` decorations rule picks on Xfce (X11) and whether its reason is written in DESIGN accordingly.
- OPERATING.md gets an issue procedure: one issue is one stage; record the reproduction conditions and the environment
  checked; a regression test where possible; `Fixes #N` in the commit message; one CHANGELOG line.

## P1. Unblock releases

- New tag rule (CLAUDE.md and HANDOFF): tag when the full Linux candidate check passes, kramers' Linux night run
  PASSes, and check-range reports 0 failing. The Windows night run does not block the tag; CHANGELOG records
  "Windows: verified / unverified / known problems". Reason: the v0.3.1 candidate's Windows night passed once in three
  runs of the same build; twice it stopped at the window focus check (`checks/windows-2026-11-11`) — an environment
  problem was blocking the tag.
- Find and fix the cause of the flaky test check-range caught (`closed_sessions_leave_no_threads`,
  `crates/haetty-gui/tests/threads.rs`, at `780b991`; record on `checks/windows-2026-11-11`). Do not cover it with
  retries; if the cause is the test's timing, make the test deterministic.
- K1 follow-up (protected area, approved for this scope): a Stop or StopFailure that started after the question
  releases the input hold without the person's input. A mouse button press (not motion, not the wheel) counts as the
  person's input and clears the mark. Add both cases to `crates/haetty-gui/tests/permission.rs`. The auto-mode deny
  path stays an open item.
- The v0.3.2 candidate with the above: version, CHANGELOG, every candidate check possible on this machine. Prepare the
  first Linux night run here (`tools/overnight-soak-linux.sh night`): how to schedule it, and keeping the machine awake
  with the lid closed. When the person says "예약해" (schedule it), schedule it for that night. When the new rule holds,
  create `haetty-v0.3.2` and push the tag only (approved in advance).
- HANDOFF §6.1, what to look at again after the person installs it on kramers: item 7 (scale), item 12 (marks and
  hold), inherited variables, tab names kept, the title bar on X11 and Xfce, restore from minimized.

## P2. Working inside haetty (point 3; after the person installs v0.3.2)

- As `docs/SELF-HOSTING.md` and OPERATING.md say: a coordinating session makes each stage an orchestrator task, hands
  it to a worker and reads only its STATUS report. Try it for real with two small stages (one docs change, one code
  change); workers are interactive tabs. Compare with the one-session way by `haetty tokens` (responses, mean context
  per response, total).
- Write down every inconvenience: making a task, noticing a waiting worker, jumping to the worker that waited longest,
  reading reports, cleaning up worktrees. Fix what can be fixed (add a key that goes to the longest-waiting worker);
  leave large ones as input for P3 and later.
- From then on stages may run this way (coordinating session + one worker per stage), so the coordinating session can
  hand on the next stage without the person saying "next stage". But a worker job that changes a protected area is
  reported to the person, with its scope, by the coordinating session before the merge.

## P3. Sidebar and side-by-side diff (point 1; DESIGN §2 change approved for the scope below)

Research first: what OpenCode's sidebar shows and in what order, what happens in a narrow terminal, how it shows diffs
— from the official docs and screenshots.

- Sidebar: from a set window width up, shown on the right from the moment a tab opens (the threshold and the sidebar
  width from the research, changeable in the config). Narrow: hidden, a key shows it as an overlay. A tab's terminal
  size never changes because of an event (only when the person resizes the window).
- Contents, Claude Code tab: session name; context use, cost, usage limits; the files changed in this conversation
  (lines added/removed); connected MCP servers (filled in P5); the workers. Shell tab: working directory, git branch and
  a change summary. SSH tab: the host (P6).
- Look: no boxes or borders, spacing and dim text; section titles one step brighter; accent colours only for state;
  between sidebar and terminal a 1 px low-contrast line. Idle screen: still no accent. The existing status panel and
  status row merge into the sidebar; only when the sidebar is hidden, today's one line.
- Side-by-side diff (overlay): choose a changed file in the sidebar, or a key; drawn over the terminal without resizing
  it. Narrow: top and bottom. Move between files and between hunks; close. The kinds of view (last edit, against
  HEAD, working tree) stay as now. Opening it right after an edit: a setting, off by default. It replaces today's diff
  view.
- IDE integration, research and prototype (optional, off by default): research Claude Code's editor-integration protocol
  (MCP over WebSocket, the lock files in `~/.claude/ide/`, the auth token, the environment variables) from the docs of
  public implementations, separating what the official docs say from what rests on reverse engineering. Only when
  enabled, haetty acts as the editor: when Claude Code proposes an edit, show the overlay diff and take accept or
  reject. 127.0.0.1 only, a new token per session, no heavy dependencies. Protected: accept and reject only by the
  person's keys, consistent with the permission-wait mark and the input-hold rules. README says the protocol is
  unofficial and may break, and that enabling it changes the editing flow; if the connection fails, silently fall back
  to the hook-based diff.
- The reference frames in the same commit ("DESIGN §2 change, approved"). Frame dumps (wide window, narrow window, diff
  overlay, shell tab) in `docs/look/`, paths in the report: the person will look and adjust.

## P4. Session management (point 1)

Research: OpenCode's session features (list, switch, names, child sessions, revert …) and what Claude Code offers
(transcript location and format, resume, the fork option, session names), from official docs and real behaviour.

- Session browser: a list opened from the palette (one overlay). Grouped by project; each row: name, last activity,
  branch, total tokens, state (open, detached, closed). Filter by typing. Choosing one goes to its open tab or resumes it
  in a new tab. No duplicate resume of one conversation (as now).
- Transcripts are read only. No message bodies shown: the name and the start of the first user message at most. Index
  so that large transcripts stay fast; record the measurements.
- Fork (the current conversation as a branch in a new tab); name, pin, hide (recorded in haetty's state folder only).
- `haetty sessions --json` (observe only).

## P5. Connecting tools (points 1 and 4)

Research: how OpenCode attaches MCP servers and other tools; Claude Code's MCP commands and scopes (user, project,
local); how their state can be known.

- A connections screen (overlay) and a sidebar section: the MCP servers registered for the current tab's project, their
  scope, their state as far as it can be known. Adding and removing never edits config files directly: haetty runs
  Claude Code's official commands, showing the command first and running it on Enter.
- haetty's own MCP server: `haetty mcp` (stdio, no network, written by hand). Tools mostly read: session list, the screen
  of sessions that allow observing, following events, waiting for a condition, reading part of a saved run log. No tool
  that sends input (P8, separately).
- `haetty mcp install`: shows and runs the official command that registers haetty in Claude Code (scope choice; removal
  too).
- Protected area (approved for this scope): the MCP server connects with the Observe role only, and sessions whose
  observing is off are invisible.
- One page on how other tools attach to haetty. For tools that only take HTTPS (Rakazo …): only what a bridge would
  need, written down.

## P6. SSH and Tailscale (point 2)

Principles: the connection is the system's OpenSSH. haetty has no SSH implementation, never handles or stores keys,
passwords or passphrases, and only reads `~/.ssh/config`. It never turns on agent forwarding by itself. tailscale: read
only queries.

- Two sources for the host list: `~/.ssh/config` (with `Include`, without wildcard hosts, values resolved with
  `ssh -G`) and `tailscale status --json` (only when installed: names, online, Tailscale SSH where it can be known).
- Palette "SSH: connect" and `haetty ssh HOST`: connect in a new tab; the host in dim text in the tab name and sidebar.
- `COLORTERM=truecolor` and `TERM_PROGRAM=haetty` reach the remote shell (prefer ways that need no remote server
  setting). Check 24-bit colour with a connection to localhost.
- On a drop the tab stays: "connection lost · Enter to reconnect". When a tab set is restored, a connection tab
  reconnects once.
- Per-host settings: `ssh-host.<name>.dir`, `ssh-host.<name>.command`.
- Tests: an isolated test sshd (temporary port, temporary keys). The person's real `~/.ssh` and `known_hosts` are
  neither read nor written by tests.

## P7. Remote daemon design (points 2 and 3; docs only)

- Goal: attach to the haetty daemon on a remote machine over SSH and use its sessions as tabs of the local window;
  sessions survive a drop; the remote Claude Code integration (attention marks, sidebar, tokens, workers) shows as it
  does locally. The session browser lists across machines.
- Channel: the daemon protocol over the stdio of `ssh HOST haetty bridge` as the base option, compared with socket
  forwarding. Version mismatch; a plan to measure latency and traffic; detecting drops and reconnecting. Remote install
  only on the person's command (`haetty remote install HOST`, a build with the new engine only).
- Safety: the input hold during a permission wait is decided on the remote daemon; the Origin travels in the protocol;
  roles only for connections that passed SSH authentication. Note Quil's remote approach
  (github.com/artyomsv/quil) for comparison.
- Write the implementation stages and the test method, then stop. Implementation after the person reads the design.

## P8. Scope of integration (point 4; docs only)

- A design for a "delegate" role, through which another tool (e.g. a Rakazo bot) sends input to a session the person
  chose: delegation switched on per session by the person; its scope (text input only, never an answer to a
  permission prompt, …); a mark on the tab and in the sidebar while delegated; a record; cut off at once. Side by side
  with the observe-only option, with pros and cons. Base option: once switched on, no per-action approval inside its
  scope. A protected area: the person decides.

## P9. Remaining tidy-up (small stages, any order)

- The token tools in the dev scripts: long output the `haetty run` way, polling waits replaced by `haetty wait`.
  Compare the number of screen lines one gate run prints, before and after.
- Windows results and failure kinds: when `overnight-soak.ps1` ends, push a result summary to `checks/windows-<date>`;
  at start check that the haetty window really came to the foreground, else "NOT-RUN (foreground)" (record the
  foreground window's program name, the time without input, the way it was started). Code changed here, Windows
  unverified.
- The auto-mode deny path (K1's open item): look with a real Claude Code, minimally. If the rules must change, propose
  only.
