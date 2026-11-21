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

### Added in PQ (2026-11-21)

Approved: DESIGN §2 change (the session browser overlay, the sidebar's Sessions line). A read-only query added to the
protocol.
- Ways to open it: palette "Sessions", one key (chosen from the conflict table), a click on the sidebar's top line.
- One row of the list: name, project (repository:branch), last activity, state (open / detached / closed), model,
  token total, number of agents. Grouped by project, most recent first inside a group. Typing filters.
- What is done here: Enter goes to the open tab or continues a closed conversation in a new tab with `--resume`. `f`
  forks (only if confirmed as Claude Code's official way; otherwise left out and recorded), `r` renames, `p` pins, `h`
  hides, `d` shows that conversation's last change in the diff screen. One conversation is never continued in two
  tabs at once (the current rule).
- Close today's hole: a claude started outside the proxy (another claude first on PATH, an alias, an absolute path) is
  also found through its transcript file and shown in the list as "integration off".
- Workers' and subagents' conversations are shown folded under their parent (the same source as PH's agent table).
- Transcript files are only read. Bodies are not shown (the name and the start of the first user message at most).
  The index is refreshed by file size and modification time; measure opening time and memory on large transcripts
  (hundreds of MB) (budget: first display within 200 ms with 500 conversations, index memory +10 MB at most; if not
  met, the numbers and the reason).
- `haetty sessions [--json]` (Observe; for a session with observing off, only whether it is open).
- Tests: a set of fake transcript files (a large one, broken lines, one in progress, one without a name), the Xvfb e2e
  `sessions.sh`, reference frames.
- Stop when: the transcript format differs between the documentation and measurement so that more than half the list
  cannot be built.
- At the end, a release candidate (0.5.0).

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

### Added in PQ (2026-11-21)

Approved: DESIGN §2 change (the Connections overlay, the sidebar's Tools section). Protected area: up to `haetty mcp`
connecting with the Observe role only and sessions with observing off being invisible. Up to two real claude runs (a
one-sentence task that checks the MCP server is attached, haiku).
- The Connections screen: the MCP servers registered for the current tab's project (scope user / project / local,
  state), the number of skill and subagent definitions, the number of hooks (names only; contents not shown). If a
  state cannot be known, it says "unknown" (no guessing).
- Adding and removing never edit config files directly. Claude Code's official command is shown on screen first, and
  Enter runs it in a new tab.
- `haetty mcp` (stdio, no network, JSON-RPC written by hand; no new dependency). Tools are read-only: session list, the
  screen text of sessions that allow observing, following events, waiting for a condition, the worker table, the
  agent table, part of a saved run log. No tool that sends input, no tool that makes tasks (after P8's decision). Tool
  descriptions short; measure and write down the tokens the tool list takes in Claude Code's context.
- `haetty mcp install` / `uninstall`: show the official registration command, Enter runs it (scope choice).
- The connection with Rakazo (point 4's first step): research, from Rakazo's official docs and repository, whether
  Rakazo as an MCP client can attach `haetty mcp`. If stdio works, write a configuration example in the docs; if it
  only takes HTTPS, write as design only what a bridge needs (local-only address, token, open only inside Tailscale).
  Do not connect to or install anything on Rakazo or pauli. Write in the report, as a sequence of commands, what the
  person must do on Rakazo's side.
- One page "how other tools attach to haetty" (`docs/CONNECT.md`): the command line (status, events, screen, wait,
  workers, agents, sessions), MCP, the event log's format, roles and observe permission.
- Tests: the test attaches to the MCP server as a client and checks every tool (sessions with observing off
  invisible, no input tool, bad requests), fuzzing, the Xvfb e2e `connections.sh`.
- At the end, a release candidate (0.6.0).

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

### Added in PQ (2026-11-21)

Approved: DESIGN §2 change (the Hosts overlay, the host shown in the tab name and sidebar, the drop notice line).
- The principles as in the original: only the system's OpenSSH. No keys, passwords or passphrases handled.
  `~/.ssh/config` only read. Agent forwarding never turned on by haetty. tailscale: read-only queries only.
- The Hosts screen: the hosts of `~/.ssh/config` and the devices of `tailscale status` in one list (the same machine
  merged into one row; online or not, Tailscale SSH or not). Typing filters. Enter connects in a new tab, `c` is a
  connection that runs claude on that host at once (`ssh-host.<name>.command` if set).
- Make clear how much of haetty works in a remote tab: the terminal, text selection and OSC 52 work. A remote claude
  has no integration (hooks, sidebar) — one line in the sidebar, "remote: no integration (P7)". Even if haetty is
  installed on the remote, this stage does not attach to it.
- Environment: how `COLORTERM` and `TERM_PROGRAM` reach the remote shell (prefer ways that need no remote sshd
  setting), the `TERM` fallback on a remote without the terminfo entry. Check 24-bit colour and Hangul width with a
  connection to localhost.
- Drops: the tab stays, "connection lost · Enter to reconnect". When a tab set is restored, one reconnect. ServerAlive
  values follow the person's ssh config; haetty does not override them.
- `haetty ssh HOST`, palette "SSH: connect", settings `ssh-host.<name>.dir` / `.command`.
- The person's machines (reference only; never connected to): kramers and pauli are linked by Tailscale and the Rakazo
  server runs on pauli. Tests only with an isolated sshd (temporary port, temporary keys, temporary known_hosts), and
  tailscale with fake status output. The real connection check is left as an item for the person.
- Tests: `ssh -G` resolution (Include, wildcard exclusion, Match), merging the two sources, drop and reconnect,
  restore, the Xvfb e2e `ssh.sh`.
- At the end, a release candidate (0.7.0).

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

### Added in PQ (2026-11-21): P7 and P8 in one session, docs only

No code is changed. The output is two design documents (`docs/design/`) and a list of what the person decides.
- P7: the original items + the facts checked in P6 (passing the environment, drop detection). The path by which a
  remote claude's integration shows in the local sidebar, the path for reading the remote's agent records, how the
  session browser merges several machines, different protocol versions, remote install (only by the person's
  command), splitting the implementation into stages with each stage's test method, the expected size.
- P8: the original items + the result of P5's Rakazo research (under P8, "Added in PQ").
- At the end, "What the person decides", numbered: each item with the options, the recommendation and the
  implementation stages that would follow the decision. After this session "next stage" goes to P9 (P7's and P8's
  implementation is instructed separately after the person decides).

## P8. Scope of integration (point 4; docs only)

- A design for a "delegate" role, through which another tool (e.g. a Rakazo bot) sends input to a session the person
  chose: delegation switched on per session by the person; its scope (text input only, never an answer to a
  permission prompt, …); a mark on the tab and in the sidebar while delegated; a record; cut off at once. Side by side
  with the observe-only option, with pros and cons. Base option: once switched on, no per-action approval inside its
  scope. A protected area: the person decides.

### Added in PQ (2026-11-21)

Done in the same session as P7 (under P7, "Added in PQ"). The original items + the result of P5's Rakazo research.
The options side by side: (a) observe only (now), (b) a delegate role (switched on per session by the person, text
input only, never an answer to a permission question, shown on the tab and in the sidebar, recorded, cut off at once),
(c) only making tasks allowed (`haetty task new` as an MCP tool). For each option, the risks and, with concrete
examples, what a Rakazo bot would really become able to do.

## P9. Remaining tidy-up (small stages, any order)

- The token tools in the dev scripts: long output the `haetty run` way, polling waits replaced by `haetty wait`.
  Compare the number of screen lines one gate run prints, before and after.
- Windows results and failure kinds: when `overnight-soak.ps1` ends, push a result summary to `checks/windows-<date>`;
  at start check that the haetty window really came to the foreground, else "NOT-RUN (foreground)" (record the
  foreground window's program name, the time without input, the way it was started). Code changed here, Windows
  unverified.
- The auto-mode deny path (K1's open item): look with a real Claude Code, minimally. If the rules must change, propose
  only.

### Added in PQ (2026-11-21)

- Of the original's three items, the Windows night-run item moves to PW (not done here).
- C11 merge helper: `haetty merge TASK` (rebase, fast-forward, the gate in a slot) — on a conflict it stops and says
  so in one line. It never pushes to main.
- P19 queued message to a busy worker: a protected area (§8.4), so not implemented; only a proposal is written
  (delivered when the turn ends, `Origin::Program`, not delivered during a permission wait, cancel, display). If the
  person approves, it is instructed separately.
- Gather into one table what the stages so far wrote as "left as a proposal" (source record, one-line summary, rough
  size, recommendation). Remove from HANDOFF's list of the person's checks (§6.1) the items automatic tests already
  replace.
- Rewrite NORTH-STAR's "Where each point stands" table to the current state.
- Changed by the person (2026-11-21): the P19 proposal above is replaced by stage PT (under PQ), which implements
  the queued message within its approved scope.

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

## PI. Shell integration, sidebar keys, a new 0.4.0 candidate (points 3 and 1; the person's text of 2026-11-20)

PW policy unchanged. One session to the end, no intermediate check-ins. If the session limit hits: push what is done in
the order I1 → I2 → I3 and write the point to continue from into HANDOFF. No tag, signature, release publication or
stable install.

Approved in advance:
- Protected area "the proxy path and what haetty injects": up to I1's shell integration. The content of the observing
  settings haetty injects, the hook output rules, the role checks and the permission-prompt path do not change. A
  commit of its own, "protected area, approved".
- DESIGN §2 change: I2's sidebar focus mark. Code and reference frames in one commit.

I1. Shell integration (closes P1 and P2 of PM's list)
- Problem: the person's .zshrc puts ~/.opencode/bin and ~/.local/bin first on PATH, so `claude` and `haetty` typed in a
  tab are not the proxy folder's.
- Research first (a table in the record): how Ghostty's and kitty's shell integration get into zsh, bash and fish
  (zsh: ZDOTDIR switch, restoring the original and reading the user's .zshenv; bash: passing through environment
  variables and its limits; fish: vendor_conf.d in XDG_DATA_DIRS) and each one's known problems; confirmed in the
  official docs and source, sources named. No code copied (licences: Ghostty MIT, kitty GPL — read kitty's scripts for
  the method only).
- zsh (required): when haetty starts zsh, ZDOTDIR points at an integration folder inside the instance's run folder and
  the original value goes in a separate variable. The folder's .zshenv restores ZDOTDIR, reads the user's .zshenv, then
  sets one precmd hook that puts the proxy folder back first on PATH. At each prompt the hook only checks "is the proxy
  folder first" and moves it if not (no duplicate entries). zsh reads the user's other startup files (.zprofile, .zshrc,
  .zlogin) in their own order. The integration file is built into the executable and written into the run folder at
  start. It is a setting the user's shell reads, not a shell script haetty runs: DESIGN gets one line on its relation
  to "No shell at run time".
- bash (if possible): the simplest method from the research that changes none of the user's files. If none, skip it
  and give the sidebar's hint a line for bash.
- fish and other shells: not done. The sidebar's "without haetty's integration" lines stay.
- Setting: shell-integration = auto | off (default auto); off = today's behaviour. In the settings panel's Claude Code
  group. With claude-integration = false the integration does not step in either.
- No integration when: the user's shell command has arguments (the login-shell choice cannot be changed), the shell is
  not zsh, inside ssh. Worker tabs and `haetty task new` sessions run claude directly as now: not affected.
- The user's shell files are neither read nor changed (only zsh reads them). Test cases: a user who already uses
  ZDOTDIR, no .zshenv, a .zshenv that rewrites the whole PATH, frameworks that remove precmd hooks (oh-my-zsh,
  powerlevel10k's instant prompt).
- Tests: an isolated HOME with per-case .zshenv/.zshrc and a real zsh (fail, not skip, when zsh is missing). With a
  fake claude: "claude typed in a tab goes through the proxy", "which haetty is the running build", "the proxy folder is
  on PATH once", "off does not step in", "the user's ZDOTDIR and startup file order are unchanged (each file read once,
  in the original order)". One more set in the Xvfb e2e sidebar.sh, "the user's .zshrc puts ~/.local/bin first":
  typing claude fills the sidebar's Lead and Agents.
- Start-up time: zsh's time to the first prompt with and without the integration (budget: +5 ms at most).

I2. The sidebar from the keyboard (C14 of PM's list)
- One key that gives the docked sidebar the focus (default Ctrl+Shift+E; update the clash table §3.11). While it has
  the focus: ↑/↓ move between lines, ←/→ fold sections, Enter does the line's action (Files → open the diff, Agents →
  the record, Workers → go to that tab or open a detached worker), Esc gives the focus back to the terminal. Nothing
  reaches the program while the sidebar has the focus.
- The focus mark is only a › before the selected line and the normal text colour for the title. No accent colour.
- tests/permission.rs gets one case: giving the sidebar the focus and back in a tab that waits for a permission
  answer leaves the Ask mark and the input hold as they were (a check, no code change).
- The path match between the sidebar and the diff overlay goes from a tail match to the full path (last line of PM's
  list).

I3. A new 0.4.0 candidate
- 98a9b71 was not tagged and P3b, P2b and this stage came after it. The version stays 0.4.0; CHANGELOG's 0.4.0
  section gets the diff overlay, the coordinator tools (task new, workers, wait worker), the shell integration and the
  sidebar key. The Windows line as it is (unverified, known problems, no shell integration on Windows).
- The stage's last commit gets the full Linux candidate check: gate, run-all, the short soak on both engines (task
  failures 0), the install tool's own test, check-range haetty-v0.3.3..<commit>, and a release.sh <commit> --out DIR
  dry run. Without a display and with the session variables cleared, as in PM. The stable process's PID and start time
  checked before and after.
- Write down which profile the gate's stripped size (Linux haetty 12.9 MB) and the budget table's dist size (≤ 12 MB)
  are measured on, and measure the dist size again. Over budget: report the cause per crate (no reduction work).
- The report names the commands for the person with the exact hash: tag → release.sh → publish the draft →
  install-stable.sh → close every window and start again.

Keep: no command or write to the stable instance, daemon, install folder or config; processes of dev and tests only,
by PID or their own socket. Tests and scripts never start the installed real claude; tests use an isolated HOME and
never read the user's real shell files. No credentials, signing keys or gh auth. No typed text in logs. The terminal
itself (engine, soaks, release tools): defect fixes only. No Windows-only work (one line each in PW's list). No
comments in new code (except external-spec behaviour, SAFETY, public doc comments, script usage headers); the why goes
into the commit body, without a per-file list. The user's git identity, no AI marks; each commit gates on its own;
check-range 0 failing before a push; two heavy jobs at a time.

Stop when: zsh's integration cannot work without changing the user's startup-file order (report the research table
and wait); a protected change beyond the scope above is needed; the candidate check fails and the cause is not found
within an hour.

Report: the research table and the choice, each part's result and evidence, start-up time before and after, the dist
size, the candidate check table and the commands for the person, what is unverified, the lines added to PW's list, the
next stage proposed.

## PQ. All the remaining work: PK → P4 → P5 → P6 → P7·P8 → P9 → PW (the person's text of 2026-11-21)

The person pastes this text once. The session that received it does (a) register it in the repository and (b) finish
the first stage, PK. After that, each time the person says only "next stage", the session does the next one in the
order below.

Before starting
- PX (text selection and copy) was handed to another session separately. If HANDOFF has "In progress: stage px" or
  the branch or worktree `haetty/px` is alive, this session does only (a), in its own worktree, and starts (b) after PX
  is in main. Two sessions never use the same checkout. If PX has not started yet, PX goes to the front of this order
  (its text is not in STAGES, so the session reports and asks the person for it).
- At registration (2026-11-21) PX had not started: no `haetty/px` branch or worktree, no in-progress line. PX was put
  first in HANDOFF's order and (b) was not started. The person's note for PX, sent to the same session just before
  this text: "X5 (the release.sh defect) is done in 5e35b67, so it is skipped. The candidate version is 0.4.1 and it
  includes P3c." PK's three skill texts were written at registration (`skills/*/SKILL.md`, sha256 as below), since
  this text is pasted only once.

(a) Registration
- The "Common rules" of this text and the text of each stage go into `docs/STAGES.md` under "PQ" (in English, without
  changing the meaning; the existing P4–P9 sections are not deleted, only "Added in PQ" is appended under each). The
  "skill texts" at the very end are not copied into STAGES (writing them as files is enough).
- HANDOFF "Next session": the order becomes the order below. The line "Next (proposed): P4 … the P2 way" is deleted.
  At the end of each stage its line moves to "Done"; that is how the next session knows its turn.
- OPERATING gets: "When the person says 'next stage', do the first stage in HANDOFF's order that is not done. A
  proposal to change the order goes into the report; the session does not change the order itself."

Order
1. PK: three working rules (skills) and the rule–enforcement table
2. P4: session browser
3. P5: tool connections screen and `haetty mcp`
4. P6: SSH and Tailscale
5. P7·P8: remote daemon design, delegation scope design (docs only, one session)
6. P9: the remaining tidy-up + a proposal for P19 (a queued message to a busy worker) + C11 (merge helper)
7. PW: Windows catch-up (last)

A release candidate after each of P4, P5 and P6 ("Release" below).

Changes to the order (the person, 2026-11-21, after the text above): "PD, the side-by-side diff inside the chat" goes
after PX and before PK; "PT, messages between sessions" goes after PK and before P4; P9's proposal for P19 is replaced
by PT. The order is now PX → PD → PK → PT → P4 → P5 → P6 → P7·P8 → P9 → PW. PD and PT also end with a release
candidate (the version is the next number at that time).

### Common rules (every stage)

- One stage is one session. No asking for confirmation midway. Where a criterion is written, follow it; where a
  choice is needed, take the option closest to the four points and write it under "Choices and alternatives". Stop only
  in a stage's "Stop when" cases and the ones written here. If the session limit is hit, push what is finished and
  write in HANDOFF where to continue; the stage continues at the next "next stage".
- How the work is done: the stage session does it itself. Research that can be split and independent implementation
  go to Claude Code subagents (haiku or sonnet; subagents do not wait for the person's answer). haetty's
  coordinating session + worker tabs (the P2 way) needs the person to answer every worker's trust and permission
  question, so it is used only in a session where the person has said they are present.
- Research from official documentation and real behaviour, with sources written down. Facts about Claude Code are
  checked against the installed version's documentation, `--help` and the strings of its executable; a feature that
  relies on undocumented behaviour is off by default.
- Approved in advance: the DESIGN §2 changes written in each stage (code and reference frames in one commit, "DESIGN §2
  change, approved") and protected-area changes within the written scope (a separate commit, "protected area,
  approved" and the scope). Beyond that scope: propose and stop. Adding a read-only query to the role table is
  approved in every stage, but only under the same rule as the existing line "Control always, Observe only for
  sessions that allow observing".
- Never changed: hooks and the status line are observe-only (no output, exit 0). haetty, its command line, its MCP
  server and remote clients never answer Claude Code's permission questions (only a key the person pressed in the
  window is an answer). Credentials, OAuth tokens, SSH keys and passwords, the release signing key and gh auth are
  never read, stored or passed on. Always run the claude the user installed, with its arguments unchanged (except the
  observing `--settings`). No typed text, prompts, file contents or selected text in logs.
- Stable: no command to and no write into this machine's stable haetty, its daemon, `~/.local/opt/haetty` or the
  stable config. Installing, replacing, applying updates, tagging, signing and publishing releases are the person's.
  Processes: only dev and test ones, by PID or their own socket (never killed by name).
- Tests: only under an isolated HOME and `HAETTY_TEST_TMP`. Tests never start the installed real claude (only where a
  stage allows "N real runs": inside a dev instance's tab, with haiku). Folders a test made are removed by the test
  (read-only ones included). The person's real `~/.ssh`, `known_hosts`, clipboard, `~/.claude/settings.json` and shell
  config are neither read nor written.
- The terminal itself (engine, soaks, look polishing, release tools): defect fixes only.
- Windows: no Windows-only work before PW. The gate's Windows build and clippy stay. A new feature gets the least that
  builds on Windows (a fallback or "not supported on Windows yet") and one line in HANDOFF's PW list.
- Code: no comments in new or changed code (except external-spec behaviour, SAFETY, public items' doc comments, script
  usage headers). Why it changed goes into the commit body and `docs/history/`, the body without a per-file list. A
  mistake made the second time is blocked by a check, not by text; a tool that would grow bigger than the work itself
  is not built but written as a proposal. (After PK, the three skills are the basis of this line.)
- Commits: the user's git identity, no AI mark and no Co-authored-by. Each commit passes the gate on its own. Before
  a push `tools/check-range.sh origin/main..HEAD` 0 failing. No lines added to known-gate-failures (propose if
  needed). Past history is not rewritten. At most two heavy jobs at a time.
- No new dependency by default. If one is really needed, measure the dist size before and after; over +1 MB for the
  two targets together, do not add it and report. The dist budget is 12 MB Linux, 10 MB Windows.
- The look (DESIGN §2): events do not change the layout. No accent colour on an idle screen. No pop-ups, sounds or
  animations. A new screen uses the same frame and keys as the existing overlays (palette, settings panel, diff):
  ↑/↓, Enter, Esc, / search. Each new screen leaves dumps in `docs/look/` (wide window, narrow window, three themes).
- The report at the end of a stage (short): the points moved, what was done, the evidence (commands and results, dump
  paths), what is unverified, the lines added to PW's list, choices and alternatives, what the person must do (commands
  ready to paste as they are, without placeholders).

### Release (at the end of each of P4, P5, P6)

- Raise the version (0.5.0 after P4, 0.6.0 after P5, 0.7.0 after P6; if PX has already used 0.4.1, from the one after
  it). CHANGELOG gets the stage and the fixes in between, and the Windows line (unverified, known problems).
- The whole Linux candidate check: the gate, run-all, the short soak on both engines (task failures 0), the install
  tool's own test, check-range <previous tag>..<commit>, a `release.sh <commit> --out DIR` dry run. What runs without
  a display and with the session marker variables cleared is as in PM and PI. The stable processes' PID and start time
  checked before and after.
- The report gives the commands for the person with the exact hash (including
  `HAETTY_MINISIGN_KEY=~/.minisign/haetty.key`): tag → release.sh → publish the draft. Installing is done from the
  settings panel's Updates (possible since 0.4.0). That install is the first one from a real release, so the report
  also says where the person looks if it fails (log paths) and gives the manual install command.
- If the candidate check fails and the cause is not found within an hour, no tag commands: report the investigation.

### PD. Claude Code's edit blocks redrawn side by side in place (point 1; the person's text of 2026-11-21)

Background (the person): the "side-by-side diff" asked for from the start was not a separate screen (P3b's overlay)
but the edit block Claude Code draws inside the conversation (red and green, one above the other) seen side by side,
as OpenCode does.

Go through to the end in one session without asking for confirmation midway. PQ's common rules apply.

Approved
- DESIGN §2 change: haetty redraws part of the program's screen (the lines of an edit block). Until now the rule was
  "the program's cells are drawn as they are", so §2 gets this exception and its condition (the matching rule
  below). Code and reference frames in one commit.
- Up to three real claude runs (a dev instance's tab, haiku): to record an edit block's real output together with the
  hook input at that time. In the recordings, prompts and file contents come from a test repository only.
- Protected areas are not touched. Neither the hooks' content nor their output rule changes. Only the edit information
  already received is used (the PreToolUse snapshot and PostToolUse of Edit, Write, MultiEdit, NotebookEdit).

D0. Research and measurement (before code)
- How OpenCode draws an edit inside the conversation side by side: the width threshold, line numbers, long lines
  (cut / wrapped), context lines, new and deleted files, colours. Checked in the official repository and docs; screen
  examples recorded.
- Collect every shape of the edit block Claude Code draws, in the installed version: the normal screen and the
  full-screen renderer (`CLAUDE_CODE_NO_FLICKER=1`); Edit / MultiEdit / Write (new file) / NotebookEdit; the block
  inside a permission question and the block after it ran; a block folded because it is long ("… +N lines (ctrl+o to
  expand)"); the block inside the Ctrl+O transcript view; wrapped long lines; tab characters; Hangul and wide
  characters; a narrow window; the block an `edit()` call of Code Mode leaves. A table of each case's header line,
  line-number column, marker characters, background colours, indentation.
- Whether Claude Code has a side-by-side diff setting for the terminal: checked in the installed version's settings
  documentation, `/config` and the executable's strings. If it has one, this stage becomes "point to that setting in
  the settings panel and the docs", and the implementation below is not done (report and stop).
- Match rate: with the real output and hook input collected above, measure what % of "find the block and match it
  line by line" succeeds. At least 50 edit blocks (the three real runs + saved Claude Code output recordings). Under
  90 %: do not implement; stop with the table of the failed cases.

D1. Finding and matching (the condition that must pass before anything is drawn)
- Block candidates: in the screen (scrollback included), a group of lines under a header line that have the
  line-number column and the background colour.
- Matching: the deleted, inserted and context lines of a candidate must match one of that tab's edit records exactly,
  line by line (wrapped long lines joined before comparing, trailing spaces by the screen's rule). If no edit matches,
  or more than one does, nothing is touched. A block only partly on screen (cut by scrolling, folded) is handled only
  for its visible part, and only when that part matches contiguously.
- Matching uses only the values haetty received through hooks and the screen's cells. Files are not read again.
- Results are cached by a hash of the line contents. If the screen did not change, nothing is computed again. Time per
  frame has a cap (for example 1 ms); over it, that frame is drawn as it was.
- Files that look secret (the existing edit snapshot's exclusion rule) are not handled.

D2. Redrawing
- Draw only inside the range of lines the block took. The number of lines, the terminal size, the bytes the program
  receives and the snapshot's content do not change (resize log 0 lines, program input 0 bytes, checked by e2e).
- Layout: before on the left, after on the right, a dim vertical line in the middle. Line numbers on both sides.
  Changed lines paired on the same row; a line on one side only leaves the other side empty. Context lines the same on
  both sides. The changed part inside a line gets a deeper background (by word; by line only if that is too costly).
- One logical line is one screen row. A line wider than the half width is not wrapped but cut with … at the end. That
  way the rows needed never exceed the original block's rows. Rows left over are blanked with the body background.
- Colours: the theme's diff colours (based on colours 2 and 1, as the overlay uses now) and dim grey. No new colours.
- At the right end of the block's header line one dim mark (for example "⇄"): it means haetty redrew this block.
  Clicking it, or a chosen key over the block, turns only that block back to its original shape.
- Width: only when the program area is at least `inline-diff-min-cols` (default 110) columns. The same threshold when
  a docked sidebar makes it narrower. Below it, as it was.
- A new file (Write) and a full deletion have nothing to split, so they stay as they were.
- A block inside a permission question is redrawn by the same rules, but the question's option lines and the borders
  above and below them are never covered. While redrawn, the permission-wait mark and the input hold stay as they
  are (one case added to `tests/permission.rs`; no code change, a check only).
- Text selection (PX): selecting and copying over a redrawn block gives the text of the side seen on screen (dragging
  on the left gives the before, on the right the after; line numbers and the vertical line are not copied; a cut line
  gives the whole line). If PX is not in main yet, this item is postponed until after PX, with one line written.
- Clicking the block's file name opens P3b's overlay on that file (to see a large change wide).

D3. Setting and turning it off
- `inline-diff = split | unified` (default split: the feature the person asked for). The settings panel's Claude Code
  group, palette "Toggle inline diff layout". With unified nothing is redrawn.
- The README says clearly: this is haetty redrawing on the screen, Claude Code's behaviour and output do not change;
  when matching fails the original shape shows; if Claude Code changes the block's shape, the original shape may show
  for a while.
- The text `haetty screen` and Observe clients read is the original screen (only the window's picture is redrawn).

D4. Tests
- Replay test: replay the real output and hook input collected in D0 as they are, and check for each block "match
  succeeds → redrawn" and "match fails → untouched". The set goes into the repository (test repository content only)
  so it can be run again when the Claude Code version changes. How to build the set is written in the docs.
- Counter-examples: a screen one character different from the edit record, two edits with the same content, a block
  partly visible, a hook that arrives late (drawn as it was first, redrawn after it arrives), a shell tab where another
  program happened to print similar lines (no edit record, so untouched), both engines.
- Renderer: reference frames (normal, in-line highlight, cut long line, Hangul, folded block, inside a permission
  question); the idle-screen grey-only check stays 5/5 (diff colours only while a block is on screen).
- Xvfb e2e `inline-diff.sh`: a fake claude prints blocks of the real shape and the hooks; the side-by-side layout
  checked in the window's frame; the mark clicked back to the original; the unified setting; a narrow window; 0
  resize lines; 0 input bytes.
- Performance: in a long conversation with many edit blocks (scrollback full), key-to-screen latency and idle CPU
  before and after.
- One real claude run (within the approved three): two edits asked for in a dev instance, checked with frame dumps.

D5. Docs and finishing
- DESIGN §2's exception and condition, "as built" in §13, README, CHANGELOG, HANDOFF, NORTH-STAR (point 1's
  "side-by-side diff in the chat" is filled by this stage; P3b is written as "a separate screen").
- `docs/look/pd-*`: the same edit's original shape and side-by-side shape next to each other, in-line highlight, a
  long line, inside a permission question, with a docked sidebar, the narrow case left as it was, three themes.
- At the end, the candidate check and the commands for the person (the version is the next number at that time).
- PW's list: Windows reference frames, the Windows version of `inline-diff.sh`.

Stop when
- Claude Code has an official setting that does the same.
- D0's match rate is under 90 %.
- In the full-screen renderer the block's range of lines cannot be known reliably (report a plan that supports the
  normal screen only, with its limits, and wait).

### PK. Three working rules and the rule–enforcement table

Follow the PK text below as it is (the skill texts and their sha256 were given with it). Added:
- `.ps1` files are not changed; they go into PW's list.
- Item 4's first kind gets the check "did the tests remove by themselves the folders they left in `/tmp` and
  `HAETTY_TEST_TMP` (read-only ones included)". On 2026-11-20 `/tmp` was 80 % full and the gate failed.
- release.sh's executable bit and notes path were finished in 5e35b67. What is left is one thing: the gate checks the
  executable bit of the scripts the user runs directly.
- `agents/worker-readonly.md` has the person's uncommitted change. Do not touch it. If the Dropbox mirror is skipped
  because of it, only mention it in the report.

The PK text (the person's, followed as it is):

Stage PK: put three working rules into the repository and apply them by default (a short session; product code is not
changed).

If another stage (PM etc.) is in progress, start after that session ends. Two sessions never edit CLAUDE.md in the same
checkout at the same time. Go through to the end in one session without asking for confirmation midway. If it does
not finish, do only 1–3 and leave 4–5 as proposals.

Approved scope (approved in advance)
- Making the three new skill folders under the repository's `skills/` and registering them in MANIFEST.md's Skills
  section.
- Copying those three folders under this machine's `~/.claude/skills/` (MANIFEST.md "Apply on another computer": a
  copy, not a symbolic link). Before copying, back up `~/.claude/skills` with the date appended.
  `~/.claude/skills/synced/` is only read, never touched. `~/.claude/settings.json` and `agents/` are not changed.
- Putting the content of 2 and 3 below into `haetty/CLAUDE.md`.

1. Writing the skill files
- Write each of the three blocks of the "skill texts" exactly to these paths. A block is from the line after
  "<<<FILE path" to the line before "FILE>>>"; the two marker lines are not in the file. UTF-8, LF line ends, ending
  with one newline after the last line. Do not edit, translate or re-wrap the content.
  `skills/no-comments/SKILL.md`, `skills/encode-in-structure/SKILL.md`, `skills/change-trail/SKILL.md`
- After writing, check with sha256sum. If it differs from the values below, write again. If it still differs after
  three tries, stop and report which line differs (do not change the content to match the value).
  ```
  081dcf985ee508b32073972fc3a42caf5474c8d46c0b208b9aaa171aa88c7d8b  skills/no-comments/SKILL.md
  c3a9dbb190002010ba13f40d70fbec8c7ce62e0b1405dced68315058e5fc14c0  skills/encode-in-structure/SKILL.md
  50d51aace04a9c065993d952a815453d74dab825fc1dffe697bcab371983c0b0  skills/change-trail/SKILL.md
  ```
  (Done at registration, commit 7eab7a0: all three equal on the first write. PK checks them again.)
- Each folder gets a SOURCE.md in the form of the existing skills: source "the skills the user gave on 2026-11-17
  (.skill files), the same names as the claude.ai account's skills", date and machine; under Local modifications
  "the version refined in the 2026-11-17 review: no-comments 4 places, encode-in-structure 3 places (a proportionality
  section added), change-trail 2 places".
- In MANIFEST.md's Skills section, add the three names under a sub-heading distinct from the existing list (their
  source is not pstack-claude, so they are not mixed into that paragraph). If the sections' habit of md5 lines also
  applies to skills, follow it.
- If `~/.claude/skills/synced/` has a skill with the same name, compare whether the content is the same and only
  report (if the same name is in two places, check with claude's skill list which one is used and write it down). Do
  not delete or change them.
- Before copying into `~/.claude/skills/`, check whether other claude processes are running (MANIFEST's Notes); if so,
  write that in the report and go on with the copy.
- The worker definitions in `agents/` are not changed (per MANIFEST, workers can only use skills preloaded for them).
  Whether to preload no-comments into the workers that may write is only a proposal.

2. Three lines in `haetty/CLAUDE.md` "Working rules"
- No comments in new or changed code (only no-comments' "comments that stay" exceptions). Existing comments are dealt
  with only when that line is changed. No repository-wide clean-up. Comments in protected areas are neither deleted nor
  moved.
- Why it changed goes into the commit body and `docs/history/` (change-trail). No per-file change list in the body.
  "Checked:" and the unverified items are written as now. The decision log's place is the "Choices made" section in
  `docs/history/` (no `docs/decisions.tsv`).
- When the same mistake or instruction comes up twice, make it an enforcement, not text (encode-in-structure). An
  incident that cannot be undone or that touched stable or the real environment is enough once. A tool that would
  grow bigger than the work itself is not built but written in the report as a proposal.
The three skills' check runs once at the end of a stage, on that stage's diff only. CLAUDE.md's line "Code, comments,
docs and commit messages in English" stays as it is.

3. The rule–enforcement table
- Move the rules of CLAUDE.md, HANDOFF §3 and §4, OPERATING and dev-commands one line each into a table: rule /
  enforcement / where. Write the enforcement as it really is (test-guard, check-range, known-gate-failures, the gate,
  deny rules, types, release.sh's session check, none).
- If the table makes CLAUDE.md too long, put it in `docs/RULES.md` and point to it with one line in CLAUDE.md.
- For a rule that already has an enforcement, shorten its prose to one line. The rule's content does not change. When
  moving lines, check with `tools/check-moved-lines.py` that no line went missing.
- Report the number and the list of the "none" rows.

4. Encode only the two kinds that have already happened twice or more (each with the smallest means)
- Tests that touched the real environment, stable or the installed real claude (writing into the real
  `~/.local/share`, trying to connect to the stable daemon, ten test daemons left running, the real Claude Code started
  six times on Windows): as run-all's last check, "no haetty process started by the tests is left" and "the stable
  processes' PID and start time are the same before and after". test-guard checks that when a test script calls
  claude, there is no claude outside the test folder on PATH (`.sh` and `.ps1` both; `.ps1` cannot be run here, so
  marked Windows unverified).
- Stale tests reported as passing (the wizard that grew to seven questions, the soak that counted attempts as
  successes): check that an e2e script ends as a failure when it cannot confirm "the result of what it did", and find
  and fix the places that count only attempts.
- For each, reproduce the past mistake, confirm the check fails, and write the method in the commit body.

5. A comment check applied only to newly added lines
- Put into the gate a script that counts ordinary comments (`//`) on Rust lines added against origin/main. Allowed:
  SAFETY, lines with a link, lines naming a specification or an external program (the list is a table inside the
  script). Doc comments (`///`, `//!`) are not counted.
- Until 2026-12-19 it only lists them as a warning, not a failure. Existing comments are not touched.
- The script's error text names the alternatives: express it with a name or type, move it to the commit body, encode
  it as a check if it is a constraint.

6. Finishing
- A record in `docs/history/`, HANDOFF updated (400 lines or fewer), the repository root's MANIFEST.md updated. Commits
  split by unit of work, each passing the gate. Before a push check-range 0 failing. The Dropbox mirror updated with
  the existing script.
- Report: the sha256 result of the three files, the comparison with synced/, the "none" list of the table, item 4's
  reproduction and how the failure was confirmed, item 5's current number of warnings, what was not done.

Keep
- Product code (`crates/`) is not changed except test code strictly needed for 4 and 5.
- No command to and no write into the stable instance, daemon, install folder or config. Processes: only dev and test
  ones, by PID or their own socket.
- Protected areas are not touched. If it becomes necessary, stop and report.
- Commits: the user's git identity, no AI mark and no Co-authored-by. Past history is not rewritten.

### PT. Messages and hand-over between sessions (point 3; the person's text of 2026-11-21)

Approved (protected area §8.4's delivery path, up to this scope): a queued message through which haetty delivers text
to a session. Only as `Origin::Program`, only after the receiving session's turn has ended, never while it waits for a
permission question. No path that becomes an answer to a permission question is made. DESIGN §2 change (the message
marks on the tab and in the sidebar, the inbox line).
- `haetty send SESSION|WORKER "text" [--file F]`: queued and delivered at the end of the next turn. Cancel
  (`haetty send --cancel ID`), list what is waiting. One line each in the sender's and the receiver's Log (the body is
  not kept in log files; it shows only on screen).
- `haetty ask "question"`: a worker asks the coordinating session or the person and waits for the answer (time limit,
  exit code). The sidebar's Workers line shows "t3 asks" and the question's first line. The answer goes by
  `haetty send`, or the person writes it with Enter on that line.
- `haetty handoff [--to new-tab|new-window|worktree] [--model M]`: has the current session write a hand-over summary
  (what was done, what is left, the files involved, the end condition) and starts a new session with that summary as
  its first prompt. The summary stays as a file that can be opened from the sidebar. The original session and the new
  one show as parent and child in the sidebar. The new session's report is received by the original with
  `wait worker`.
- `send` and `ask` also work between two equal tabs (not only coordinator and worker). The receiving tab sees in its
  own sidebar who sent it. Each tab can turn on "do not receive messages"; the default is receive.
- The window: moving a tab to another window (palette "Move tab to new window", attaching a detached tab to an existing
  window). Two windows share the view of one daemon's sessions.
- Cutting off: "Stop messages to this tab" in the sidebar or palette empties the queue and blocks it in one step.
- Tests: no delivery during a permission wait (0 bytes received by the program), arrival at the turn's end in one
  piece, cancel, do-not-receive, the Observe role cannot send, the Xvfb e2e `messages.sh`, up to two real claude runs
  (haiku: one tab asks another and continues with the answer).
- These tools are not put into `haetty mcp` (P5) (what external tools may send is decided by the person in P8).
- At the end, a release candidate.

The stages P4–P9 and PW: their PQ text is under each section, "Added in PQ".

## PW. Windows catch-up (last)

After all other stages. Inputs: the branches `checks/windows-2026-11-17` (its record's "For kramers": the fixes
branch table, the merge conflicts and their resolution, "Not fixed here"), `windows/2026-11-17-fixes`,
`windows/2026-11-17-ph-fixes`; the Windows check list in `docs/dev-commands.md`; PW's list in HANDOFF "Next session".

### Added in PQ (2026-11-21)

The work of two machines. When the order reaches PW, the kramers session does only W-a below and stops. W-b needs a
text the person gives the Windows PC, so W-a's report lists "what the Windows PC checks" and the person writes it.

W-a (kramers)
- Review `windows/2026-11-17-fixes` and `windows/2026-11-17-ph-fixes` and bring them into main (conflict resolution:
  the "For kramers" table in the `checks/windows-2026-11-17` record; main has changed a lot since, so each commit is
  looked at again). For a commit not taken, write the reason.
- That record's "Not fixed here" items: `layout::list` never returning the staging folder or `current` as a version
  (by type), assertions that depend on the font, the ConPTY files in the instance folder, processes left behind,
  `install-stable.ps1` using install-layout.
- The Windows proxy (protected area, approved): a release build's `haetty.exe` is a GUI-subsystem program, so the
  proxy called by `claude.cmd` comes up without the tab's console. `claude.cmd` calls the console-subsystem
  `haetty-headless.exe claude-proxy` instead (the decision logic shares the one piece of code there is now). If that
  does not work, a small console executable. The AttachConsole way is not taken.
- The lines piled up in HANDOFF's PW list, one by one: make it work on Windows (the shell integration only if there is
  a way that does not change the PowerShell profile), or write "not done on Windows" in the README. The editor
  server's random source, the selection's CLIPBOARD, `haetty.cmd`, the list of reference frames.
- Move to Windows those Unix-only tests that can be moved (priority: permission, daemon, update's case table, ide,
  sessions, mcp).
- In `scripts/e2e/windows/`, write the Windows versions of the Linux e2e scripts made meanwhile (diff, sidebar,
  coordinator, select, sessions, connections, ssh, ide). Build PATH from the test folders only, so the installed real
  claude is never started, and have the script check that itself. They cannot be run here, so marked "written, not
  run".
- At the end, report the list for the Windows PC to check (scripts, order, expected time, what the person looks at by
  hand).
