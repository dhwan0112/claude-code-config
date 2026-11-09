# haetty design: milestones, external integration and the observe-only interface, the orchestrator (sections 5, 6, 8)

> Part of the design. Index: [`../DESIGN.md`](../DESIGN.md) (section
> numbers are the same there). Text below is moved verbatim from the former `docs/DESIGN.md`.

## 5. Milestones

1. GUI, one window one session (`docs/history/2026-11-04-05-milestone-1.md`), with this document's theme,
   padding and decoration rules.
2. Daemon split, IPC, tabs, saved/restored tab sets, `install-desktop`.
3. Claude Code adapter: hook/status-line subcommands, status panel, diff
   pane, attention states.
4. Orchestrator: tasks, worker tabs, worktrees, worker reports.

## 6. External integration (very long term): Rakazo

Rakazo stays unmodified (no fork). Integration goes through surfaces it
already has:
1. CLI with stable JSON output (`<bin> status --json`, `<bin> run --task ... --json`),
   callable by Rakazo bots over SSH.
2. An MCP adapter (separate feature or binary, not in the main executable)
   speaking to the daemon socket, registered in Rakazo as an HTTPS MCP server
   reachable only over Tailscale, token-protected, read-only by default.
3. Optional push of events into Rakazo chats through its client API, only once
   that API is stable.

Constraints on current work: the daemon API is a typed, versioned
command/event schema shared by GUI, CLI and MCP; sessions, workers and tasks
have durable ids; orchestrator events go to an append-only log readable from a
cursor. Remote parties never approve Claude Code permission prompts; approvals
stay with a human.

### 6.1 Observe-only interface (first step, 2026-11-07; no network)

The local base the later adapters (CLI over SSH, MCP) stand on. Written with
the code (stage V3); the role table change is a protected-area change,
approved for this scope.

- **Consent per session, off by default.** A session is observable only
  after the person allowed it: palette "Observing: allow or stop for this
  tab", or `observe-new-tabs = true` for new tabs. The flag lives in the
  daemon (`SessionInfo.observable`, `SetObservable`, Control only), so it
  survives detach and reattach; tab sets do not store it (a restored tab
  is a new session and asks again).
- **Display.** One dim glyph `◦` after the tab number (`ui::tab_bar`), in
  the dim grey on every tab, active or not — never an accent colour
  (§2.2); no hint stays on screen. With nothing observable the screen is
  unchanged (the reference frames do not move).
- **What an observer sees.**
  - `GetScreen` (`haetty screen [--json] SESSION`): the visible screen
    text and the state — title, folder, program, size, cursor, and for
    Claude Code tabs the attention mark, busy, turns, name, model and the
    report's STATUS. Denied for a session that is not observable. The text
    travels as `Secret` (never logged).
  - `ReadEvents { cursor, follow }` (`haetty events [--follow] [--json]
    [--since CURSOR]`): `events.log` lines after the cursor, only events of
    a session written while it was observable. The daemon keeps per session
    the `seq` ranges of consent (from the "observe on" line to the "observe
    off" line). Nothing from before consent, after it was withdrawn, or from
    an earlier daemon run (those ranges are not kept across a restart; the
    sequence and the cursor continue). Daemon-wide lines (start, stop,
    friction) are never shown. With `follow` each new visible event is sent
    as it is written. The log holds derived fields only (§3.8): no typed
    text, prompts, tool input or screen contents.
- **Roles.** `ReadEvents` and `GetScreen` are Observe's alone (Control and
  Report are denied); `SetObservable` is Control's; every input or session
  message from Observe stays denied (§3.7, unchanged), and the permission
  rule for Program input is untouched.
- **Client type.** `haetty_proto::ObserverClient` is the observe-only
  client of §3.7's rule for remote adapters: it says `Hello` as Observe and
  can only build `ReadEvents`, `GetScreen` and `ListSessions`. It has no
  method taking a `ClientMsg`, no session handle and no input. A unit test
  checks its source for exactly those message names.
  `haetty events` / `haetty screen` use it. The older `haetty status
  --screen` (local scripts and tests) still reads any session through the
  full client's Observe `Attach`; adapters must not use it.
- Protocol version 8. Tests: `crates/haetty-gui/tests/observe.rs` (real
  daemon and CLI).

## 8. Orchestrator (milestone 4, first step)

(Numbered 8 as asked when it was commissioned; 7 is unused.) Written
2026-11-06 before the code (stage S3). Scope: one task → one worker; no
automatic decomposition, no automatic merging, no MCP / Rakazo (§6).

### 8.1 Task and worker

- A task: id (`t1`, `t2`, … per instance), title, description (one line
  in the first step), repository (the git top level), base branch. Created
  in the palette ("New task": title, description, then repository and base,
  prefilled from the active tab's folder and its current branch).
- haetty makes the worktree with git commands only (`gitview.rs`'s
  time-limited runner): `git worktree add -b task/<id>-<slug> <path>
  <base>`, path `<repository's parent>/<repository>-tasks/<id>-<slug>` —
  beside the repository, never inside it, so the repository's content and
  ignore files are not touched. Refused when the branch or the path exists,
  the base is not a commit, or the folder is not a repository.
- The worker is Claude Code in a new tab in that worktree. Program: config
  `worker-command` (default `claude`); arguments: none of haetty's beyond
  the task prompt — no permission flags, so the permission mode is what
  the person's own Claude Code settings say (bypass included or not).
- The task text reaches Claude Code as its positional prompt argument
  (`claude "<prompt>"`), not as typed input: Claude Code itself submits it
  after its own start-up dialogs (folder trust, the one-time bypass
  acceptance), which only the person answers. Hooks from settings can run
  before the trust dialog is answered (Claude Code permissions docs, "What
  runs before you trust a folder"), so a hook report is not a sign that
  input would be safe. The prompt is not stored as the tab's arguments: tab
  sets, the closed list and re-attach strip it, so restoring a worker
  resumes its conversation without submitting the task again.
- The prompt asks for ccx's worker report at the end of the final message
  (`agents/worker-*.md`): `STATUS: verified | blocked | needs_review`,
  `ARTIFACT:`, `EVIDENCE:` (each check actually run), `BLOCKERS:`,
  `NEXT_ACTION:`.
- Tasks are files `tasks/<id>.task` in the state directory (key = value,
  escaped); the worker tab is the tab whose folder is the task's worktree.
- Cleanup only on the person's command (palette "Remove a task's
  worktree"): `git worktree remove <path>` without `--force` (a worktree
  with changes or untracked files is refused, with git's reason), then the
  task file goes; the branch stays (its commits are the result). Never
  `git branch -D`.

### 8.2 Worker reports and states

- Source: the Stop hook's `last_assistant_message` (Claude Code hooks
  docs: the transcript "may not yet include the current turn's most recent
  messages when a hook fires"), parsed in the daemon (`haetty-cc`, not in
  the hook process: `report.rs`'s rules are unchanged). Fallback when that
  field is missing: the transcript (`transcript_path` only if it is a
  `.jsonl` file in a `projects` folder; its last 256 KiB), the last
  main-chain (`isSidechain` false) assistant text whose timestamp is not
  before the turn's UserPromptSubmit. The report is cleared at
  UserPromptSubmit, so it always belongs to the latest turn.
- Grammar (ccx's `hook.sh`): `STATUS` on its own line, markdown marks
  (`#`, `*`, `>`, `-`, spaces) allowed around the key, value one of
  `verified`, `blocked`, `needs_review` (any case); the last such line
  counts. The other four keys take the rest of their line and the lines up
  to the next key, at most 2 000 characters each.
- States, never guessed:
  - running — the worker is between UserPromptSubmit and Stop (`busy`);
  - verified — STATUS verified with an EVIDENCE that is not empty or
    "none" (verified without evidence is needs review: it cannot be
    checked);
  - blocked, needs review — as reported;
  - no report — anything else: no turn finished yet, a final message
    without a valid STATUS, the program exited without one.
- Report texts are carried as `Secret` (never in logs, debug output or
  the event log); the event log gets the state only.
- Wire: `ClaudeInfo` and `SessionInfo` get `turns` (Stops seen), `report`
  and (`SessionInfo`) `busy`; protocol version 7.

### 8.3 Display

- Worker tab name: a state glyph and the task title (an own name replaces
  the title, not the glyph): `…` running, `✓` verified, `✗` blocked, `◇`
  needs review, `–` no report. Grey like the rest of the name: the existing
  attention marks (amber "turn finished", red `?` question) already call
  the person; a second colour channel would compete with them (§2.2).
- Status panel: a WORKERS section — one line per task (glyph, id, title,
  state) and, for the active worker, its BLOCKERS / NEXT_ACTION first
  lines.
- `haetty status --json`: `workers`, one object per task (id, title,
  repository, base, branch, worktree, session, state and the report's five
  fields).

### 8.4 Safety boundary (protected area, approved for this scope)

- Everything the orchestrator sends to a worker is `Origin::Program`;
  the function that sends (`haetty_cc::orch::send`) has no origin
  parameter, and the daemon refuses Program input while the session waits
  for a permission answer (§3.7, unchanged).
- The orchestrator sends only when the worker is ready: at least one turn
  finished (`turns` > 0: past every start-up dialog), not busy (no tool
  call can be asking), no question waiting. Otherwise it holds and says
  why. It sends a message as one paste plus one Enter, never key by key.
- The orchestrator never answers permission prompts or dialogs, never
  edits Claude Code's settings or trust records, and adds no permission
  flags; bypass follows the person's settings.
- A cargo integration test (`tests/orchestrator.rs`, real daemon, a fake
  `claude` recording every byte it reads) checks: before any turn → held,
  0 bytes; a permission question → held and a raw Program paste refused, 0
  bytes; after the answer and a Stop → delivered; busy → held; and that
  the orchestrator source contains no `Origin::Human`.
