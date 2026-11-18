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

## PH. The harness screen (points 3 and 1; the person's instruction of 2026-11-17)

Change of direction: from this stage on, the terminal itself (engine, soaks, look polish, release tools) gets defect
fixes only ("On hold" in HANDOFF). P3a (sidebar) is part of this stage; DESIGN §13.3's decisions stand, the Agents
section grows. One session to the end, no check-ins; if the session limit hits, commit what is done in the order
H1 → H2 → H3 and write the point to continue from into HANDOFF.

Approved in advance:
- DESIGN §2 change: §13.3's sidebar and H2's Agents and Log sections; code and reference frames in one commit.
- Protected "settings haetty injects": H1's proxy injecting the **same** observing settings (hooks, status line) as a
  Claude Code tab. The injected content itself does not grow. Separate commit, "protected area, approved".
- Protocol 12 → 13 or more (ClaudeInfo fields). In the role table: a read-only query on the same line as ListSessions.
- Real Claude Code: for H0, the person's real login (not an isolated HOME), at most two short runs (a one-sentence task
  that starts subagents, model haiku). Every other test replays the recorded hook input.

H0. Measurement (before code; a new file in docs/history/): record the hook input haetty gets in a real run (input text
and file contents removed; field names and formats only): SubagentStart, SubagentStop, PreToolUse / PostToolUse inside
a subagent, TaskCreated, TaskCompleted, WorktreeCreate, WorktreeRemove, Stop. Check where agent_id and agent_type come,
whether a field names the parent agent (else a rule from the order of the Agent tool's PreToolUse and SubagentStart),
whether background runs and forks (/subtask) can be told apart, where model and effort come from. The subagent
transcript `~/.claude/projects/{project}/{sessionId}/subagents/agent-{agentId}.jsonl`: model, token use, which line
holds the last message, when the file is written (the docs say asynchronously, possibly late). Whether the status-line
JSON has effort, model or agent fields. "advisor" in the official docs (sub-agents, CLI reference, model settings): if
present, its observable signals; if not, "not in the docs", not built. A table: item shown / source (hook, transcript,
status line) / official doc basis. No item without a source; no progress %.

H1. `claude` typed in a shell tab becomes a Claude Code tab (proxy). Every tab haetty opens gets an instance folder
(under the runtime directory) first on PATH, holding a proxy named `claude` (Linux: a link to the haetty executable;
Windows: decide after looking at claude.cmd / claude.exe; no new executable shipped). The proxy removes its folder from
PATH, finds the real claude and execs it with the same arguments plus the Claude Code tab's `--settings` (the merge
rule when the user gave `--settings`), passing the tab's session variables; stdio, exit code and signals pass through.
No injection when: claude-integration = false; non-interactive (-p/--print …); stdout not a terminal; already through
the proxy (recursion); a non-conversation subcommand (claude mcp, update, doctor …, the list from `claude --help`). Real
claude not found: the shell's "command not found" exit code and one line. The tab becomes a Claude Code tab at its
first SessionStart (attention marks, sidebar contents, tab names, permission mark and input hold) and returns to a
shell tab when claude ends — without a terminal size change (docked sidebar stays; narrow: no status row reserved, state
only via the sidebar overlay and the tab bar). Tab sets save the claude session id and folder; a restore opens the shell
and runs `claude --resume <id>` there without asking (setting restore-claude = resume | shell, default resume); this
closes HANDOFF P4's "hand-started claude restored as zsh". If the tab's foreground process is claude without reports:
one sidebar line "claude is running without haetty's integration" with likely reasons (PATH, absolute path, alias);
never read or change the user's shell files. claude inside ssh: out of scope (P6), no line. Tests: a fake claude for
arguments, exit code, signals, recursion, non-interactive pass-through, --settings merge, claude-integration=false; Xvfb
e2e: claude in a shell tab → marks and sidebar → back to shell → resume after restart; one case in tests/permission.rs
that the permission mark and input hold work the same for a proxied claude.

H2. Sidebar (§13.3 1–8 as decided, plus): a Lead line at the top (session name, model and effort when sourced, context
%, 5h/7d limits, cost). Agents section (replaces §13.3's Subagents): header "Agents  2 running · 1 done"; one line per
agent: state glyph (● running, ✓ done, ✗ failed, ◌ waiting in the background), agent_type, short model, elapsed time,
and the current tool and target while running (file names cut on the left), or tool calls and tokens when done (when
sourced); nesting by indent and └; worktree isolation and fork as dim tags; running first, the last 6 finished, the
rest "+ 12 earlier". Click or Enter in the overlay list: the agent's record in a read-only overlay (prompt first line,
tool calls with times, last message, errors; shown on screen, never logged; the file read off the UI thread, tail only;
before the file exists, what the hooks gave). Colours: grey only; only an agent waiting for permission gets a colour-9
?; no blinking; elapsed time redrawn each second for that line only (idle CPU budget). Workers section: orchestrator
tasks plus the number of agents in each worker tab; a click goes to that tab. Log section (bottom, the remaining
height): this tab's recent events one per line (time, who, what): agent start/end, edited files, permission questions
and answers, turn ends, compaction; from the daemon's event log, no new store. Sections fold (title click, ←/→ in the
overlay list), kept in the ui state file. A Sidebar group in the settings panel (sidebar, sidebar-width,
sidebar-min-cols, sections on/off). `haetty agents [--json] [TAB]` (Observe-allowed sessions only; the table on
screen). Data: an agent table in haetty-cc's state machine (agent_id → type, parent, start/end, current tool, call
count, state), limits per session (200 finished agents) and rules for /clear and resume; tool events still never
release the permission mark or input hold (state.rs's tests keep passing). Tests: replay of H0's real hook order
(nesting, three parallel, background, failure, another agent's tool finishing during a permission question, arrivals
out of order); layout (events never change the grid); renderer (idle screen grey only, no accent while agents run);
Xvfb e2e sidebar.sh; idle CPU and key-to-screen latency measured with 5 agents running. Frame dumps docs/look/ph-*:
wide Claude tab (0, 3 and nested agents), narrow overlay, agent record view, shell tab, three themes.

H3. Remake the ccx difference table and fill it: cc-panel/README.md's panel.py sections (sess, model, ctx, workers,
limits, cost, keys …) and hook.sh's agentpre/agentpost/substart/substop, line by line against haetty, in the history
record: same / different (why) / missing. Add the missing ones that do not change Claude Code's behaviour; leave out
those that do (permission defaults, ask rules, tui settings, autoCompactWindow) as "left out on purpose". One thing only
haetty can do: a whole-window view of the agents of all tabs (palette "All agents": grouped by tab, longest-waiting
first, Enter goes there); it replaces P2's "key to the longest-waiting worker".

H4. Docs: README "Claude Code in haetty" (what happens when claude is typed in a shell, what is injected and what not,
how to turn it off, the sidebar sections, haetty agents); DESIGN §2 and §13, NORTH-STAR point 3's row, HANDOFF (≤ 400
lines, next: P2 in real use), CHANGELOG (unreleased), §6.1 person checks (5+ agents in real work, Windows proxy).
Windows: build, clippy, unit tests required; the proxy (claude.cmd resolution order, PowerShell and cmd, Git Bash)
marked unverified, with windows/ scripts and check items. Commits per step, each gated, check-range 0 failing before a
push. No tag.

Keep: no command or write to the stable instance, daemon or install folder; dev and test processes only. Hooks and
status line observe-only (no output, exit 0); the proxy changes nothing of Claude Code's arguments and behaviour but
the added --settings; no path where haetty answers a permission question. No credentials or OAuth tokens read; the
user's shell files and ~/.claude/settings.json untouched; no typed text, prompts or file contents in logs. Commits:
the person's git identity, no AI mark. Two heavy jobs at a time.

Stop when: a protected-area change beyond the scope above is needed; H0 shows the docs and reality differ so that half
of the Agents section cannot be built (report the table and wait); on Windows the proxy cannot work in principle
because of how claude is installed (finish Linux and report).

Report: H0's table, results and evidence per step, the ccx table, the commit list, sizes and idle CPU before and after,
what is unverified, choices and alternatives, what the person decides from the frame dumps.

## PM. 0.4.0 candidate, P2 in real use, P3b (points 3 and 1; the person's revised text of 2026-11-19)

Policy (the person, 2026-11-19): Windows checks and fixes happen once, after all other stages, in **PW** (below).
Until then no stage waits for a Windows result or does Windows-only work; written into HANDOFF "Next session" and
OPERATING. Kept: the gate's `x86_64-pc-windows-gnu` build and clippy (fix what fails). Not done: merging `windows/*`
branches, handling `checks/windows-*`, `#[cfg(windows)]` defect fixes, writing or changing `.ps1` scripts, Windows
reference frames, porting tests to Windows. A new feature that needs Windows gets the least that builds (a fallback or
one line "not supported on Windows yet") and one line in PW's list. The three branches (`checks/windows-2026-11-17`,
`windows/2026-11-17-fixes`, `windows/2026-11-17-ph-fixes`) are neither deleted nor merged: HANDOFF's PW line names
them and where the list "Not fixed here" is. CLAUDE.md's "No platform-only feature without a working counterpart" line
stays, with one line under it: Windows counterparts are checked and fixed in PW (2026-11-19).

One session to the end. If the session limit hits: push what is done in the order M1 → M2 → M3 and write the point to
continue from into HANDOFF. No tag, signature, release publication or stable install.

M1. 0.4.0 candidate (Linux)
- CHANGELOG 0.4.0: PH (the claude proxy in shell tabs, the sidebar, All agents, `haetty agents`). The Windows line as
  it is: "unverified; known problems: the claude proxy in shell tabs does not work in release builds; the panel cannot
  install an update (the fix is on `windows/2026-11-17-fixes`, not merged)". The 0.3.3 section gets the line about
  the update install too.
- After the version 0.4.0 commit, the whole Linux candidate check: the gate, run-all, the short soak on both engines
  (task failures 0), the installer's own test, `update.sh`, `settings.sh`, `sidebar.sh`, `check-range
  haetty-v0.3.3..<commit>`. check-range and the install test run without a display, with the session display variables
  removed, and the stable processes' PIDs and start times are compared before and after.
- `tools/release.sh <commit> --out DIR` as a dry run: check the assets and the manifest (the Windows zip: only that it
  is made).
- HANDOFF's "Stable" line (v0.2.0) and commit `9c9a2ae` (v0.3.2 is stable) disagree: read the install folder's
  `INSTALLED` files (no command to stable) and correct it.
- The report gives the person's commands with exact hashes: tag → `release.sh` → publish the draft →
  `install-stable.sh`. If the candidate check fails and the cause is not found within an hour, stop (no M2).

M2. P2 in real use (in a dev instance, because stable has no PH)
- In a haetty started by `tools/dev-run.sh`: one coordinating session and workers do M3's work for real. Coordinator
  sonnet, workers haiku or sonnet; the work is split over two or more workers.
- Write down every inconvenience: what the sidebar does not show, the hands it takes to get to a worker, missed
  permission questions, wrong displays, the commands the coordinator used to learn the workers' state and their
  tokens. Each with how to reproduce it and a proposed fix. This list sets the order of the next stages.
- Fix in this stage what takes under an hour and is not a protected area.
- `haetty tokens`: "one session" against "coordinator + workers" (a past stage of the same size may serve; write down
  the comparison's limits).
- Each worker's result passes the gate on its own worktree branch, then goes into main one at a time.
- Nobody answers permission questions: not the coordinator, not a worker, not haetty. A worker stopped because no
  person is there stays stopped and is recorded.

M3. P3b diff overlay (M2's work; DESIGN §13.4; the §2 change is already approved)
- All of §13.4's 1–6: drawn as an overlay (terminal size unchanged), side by side (100 columns or more) and top and
  bottom, the sidebar's Files as the file list and a click opens, the keys (n/p, ]/[, j/k, v, m/Tab, r, Esc/q), the
  settings `diff-view`, `diff-split-cols`, `diff-after-edit` (default off), their settings-panel items.
- Close to seeing it inside the chat's flow: right after an edit, Enter or one click on that file's line in the sidebar
  opens it, and closing leaves the terminal exactly as it was.
- Tests: unit (layout, hunk moves, the switch by width), renderer (idle screen grey only; diff colours only while the
  overlay is open), Xvfb e2e `diff.sh`, reference frames and `docs/look/p3b-*` dumps (side by side, top and bottom, a
  narrow window, three themes). Code and reference frames in one commit, "DESIGN §2 change, approved".
- P3c (IDE prototype) is not part of this stage.

Keep: no command or write to the stable instance, daemon, install folder or config; processes of dev and tests only,
by PID or their own socket. Tests and scripts never start the installed claude (M2's real use is the exception, only
inside the dev instance's tabs). Protected areas (hook output rules, the role checks of `Daemon::message` /
`Daemon::input`, the permission-prompt path, the injected settings) are not touched; if one becomes necessary, stop
and report. No credentials, signing keys or gh auth read or changed; no typed text in logs. The terminal itself
(engine, soaks, release tools) gets defect fixes only. New code carries no comments (except behaviour an external
spec forces, SAFETY, doc comments of public items, script usage headers); the why goes into the commit body, without a
per-file list. A mistake made a second time is stopped by a check, but a tool larger than the work itself is only
proposed. Commits: the person's git identity, no AI mark; each passes the gate on its own; check-range 0 failing
before a push; two heavy jobs at a time.

Report: the candidate check table and the person's commands, M2's inconvenience list (fixed and left) and the token
comparison, M3's frame dump list, what is unverified, the Windows items moved to PW, the proposed next stages.

## PW. Windows catch-up (last)

After all other stages. Inputs: the branches `checks/windows-2026-11-17` (its record's "For kramers": the fixes
branch table, the merge conflicts and their resolution, "Not fixed here"), `windows/2026-11-17-fixes`,
`windows/2026-11-17-ph-fixes`; the Windows check list in `docs/dev-commands.md`; PW's list in HANDOFF "Next session".
