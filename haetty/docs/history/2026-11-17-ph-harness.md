# Stage PH: the harness screen (2026-11-17)

NORTH-STAR points 3 (many jobs) and 1 (an OpenCode-like UI). The person's stage text: [STAGES.md](../STAGES.md), "PH".
From this stage on the terminal itself (engine, soaks, look polish, release tools) gets defect fixes only (HANDOFF,
"On hold"); P3a (sidebar) is part of this stage.

## H0. Measurement (before code)

Two real Claude Code runs with the person's own login (approved: two short runs, haiku, one-sentence tasks that start
subagents), Claude Code **2.1.326**, in a scratch git repository under `/tmp/haetty-test/ph-h0/repo`, inside a tmux
server of its own with a clean environment (`env -i HOME PATH TERM LANG USER`). A first start of run 1 ended at Claude
Code's folder-trust dialog (its default answer is "No, exit"; no model call happened) and was started again.

Hooks: a recorder (`--settings` with every event below, all `async: true`, command
`python3 -I rec.py`, nothing printed, exit 0) that keeps field names, identifiers, enum values and model names and
replaces every other string by `<str:N>` (prompts, commands, file contents, messages, paths) **before** writing. The
records are the test data of the replay tests: `crates/haetty-cc/tests/data/h0-run1.jsonl` (80 events) and
`h0-run2.jsonl` (36), times relative to the first event.

- Run 1: "two Explore subagents at the same time, one in the background, plus a general-purpose subagent that delegates
  to its own Explore subagent", then `/subtask …` (a fork).
- Run 2: "create one task with TaskCreate, then a general-purpose subagent with worktree isolation in the foreground
  that reads README.md and runs a failing `cat`, then mark the task completed". Haiku did not call TaskCreate (it
  searched for the tool and went on) and launched the subagent in the background anyway; `cat no-such-file` came as a
  normal `PostToolUse` (exit code in the response), not `PostToolUseFailure`.

**WorktreeCreate / WorktreeRemove were not registered**: the hooks reference says a WorktreeCreate hook "replaces that
default git behavior" and must print the worktree path, and WorktreeRemove "runs when Claude Code cleans up a worktree
that your WorktreeCreate hook created". Neither is observe-only, so haetty never registers them; worktree isolation is
read from the Agent tool's input instead (below).

### What arrived (run 1, in order; ids shortened)

```
 0.00 SessionStart        source=startup model=claude-haiku-5-5
 0.53 UserPromptSubmit
 5.23 PreToolUse  Agent   (main)  subagent_type=Explore            tool_use_id=…aCQ
 5.73 PreToolUse  Agent   (main)  subagent_type=Explore            tool_use_id=…Vz
 6.68 PreToolUse  Agent   (main)  subagent_type=general-purpose    tool_use_id=…an
 6.97 SubagentStart       agent_id=a454… agent_type=Explore
 7.00 PostToolUse Agent   (main)  status=async_launched agentId=a454… resolvedModel=claude-haiku-5-5
 7.01 SubagentStart a43d… Explore ; 7.04 PostToolUse Agent → a43d…
 7.07 SubagentStart a048… general-purpose ; 7.10 PostToolUse Agent → a048…
 9.48 PreToolUse  Bash    agent_id=a43d… agent_type=Explore
 9.88 Stop                background_tasks=[3 × {id, type: subagent, status: running, description, agent_type}]
10.24 PreToolUse  Agent   agent_id=a048… (the general-purpose agent) subagent_type=Explore
10.59 SubagentStart a1a1… Explore ; 10.61 PostToolUse Agent agent_id=a048… → agentId=a1a1…
11.73 SubagentStop        agent_id=acd6… agent_type="" (no SubagentStart: an internal agent)
14.29 PreToolUse  SubagentHandback agent_id=a454… ; 14.44 SubagentStop a454… Explore
14.49 UserPromptSubmit    (no person: the background agent's completion notification starts a turn)
16.38 SubagentStart a048… again (resumed after its child's notification) … 21.96 SubagentStop a048…
43.13 SubagentStart       agent_id=aa23… agent_type=fork   (/subtask)
47.92 SubagentStop        aa23… fork
71.63 SessionEnd          reason=prompt_input_exit
```

### Findings

| Question | Measured (2.1.326) | Official docs |
|---|---|---|
| `agent_id`, `agent_type` on a subagent's tool hooks | **Yes**: PreToolUse, PostToolUse, PostToolBatch inside a subagent carry both; the main agent's carry neither | Yes ("`agent_id` and `agent_type` tell your script which agent a hook fired in") |
| Parent of a nested agent | **Exact, no time rule needed**: the parent's `PreToolUse`/`PostToolUse` of the Agent tool carries the parent's `agent_id`, and `tool_response.agentId` names the child (no `agent_id` = the main agent). The meta file also has `parentAgentId` | Agent tool response `agentId` documented; no parent field on SubagentStart |
| Order | `SubagentStart` (child) comes ~30 ms **before** the parent's `PostToolUse(Agent)` naming it; the child's first tool hook can come before or after | Not documented |
| Background vs waited | `tool_response.status = "async_launched"` (+ `isAsync: true`) vs `"completed"` (docs); every launch here was background (the default: "a subagent that Claude spawns from your main conversation starts in the background") | Yes (`status`) |
| Running background agents at a turn's end | `Stop.background_tasks[]`: `{id, type: "subagent", status, description, agent_type}` | Yes (Stop input) |
| Fork (`/subtask`) | `agent_type = "fork"` on SubagentStart, tool hooks and SubagentStop | Fork documented; the `"fork"` type value not |
| Worktree isolation | the Agent tool's `tool_input.isolation = "worktree"` | `isolation` frontmatter documented |
| Resumed agents | a second `SubagentStart` with the same `agent_id` (after a child's completion), then a second `SubagentStop` | Yes ("when Claude resumes a subagent") |
| Internal agents | `SubagentStop` with `agent_type: ""` and no `SubagentStart` (5 in run 1) | Yes ("Claude Code also runs internal agents …") |
| Turns without the person | `UserPromptSubmit`/`Stop` pairs for background completion notifications | Partly ("results reach Claude as a completion notification in a later turn") |
| Model of an agent | `tool_response.resolvedModel` of the Agent tool; the meta file's `pickedEffortModel`; `message.model` in the transcript | `resolvedModel` documented |
| Effort | `effort.level` on PreToolUse/PostToolUse/PostToolBatch/Stop/SubagentStop (inside a subagent: the level then in force) and in the status line | Yes (common input `effort`; status line `effort.level`) |
| Session model | `SessionStart.model`, status line `model.{id, display_name}` | Yes |
| Tasks (TaskCreated/Completed) | not seen (the model did not use TaskCreate) | Documented; **not in haetty's injected events** and the stage keeps the injection as it is |
| Failure of an agent | not seen; a failing command is a normal PostToolUse. A tool error would come as `PostToolUseFailure` (already injected) | PostToolUseFailure documented |

**Subagent transcripts** (`~/.claude/projects/{project}/{session}/subagents/agent-{id}.jsonl`, documented):
one JSON object per line; `type` is `user`, `assistant`, `attachment`, `api-request`, `api-request-shape`,
`api-request-blob` (`fork-context-ref` first in a fork's file). Assistant lines carry `message.model`,
`message.usage` (`input_tokens`, `cache_creation_input_tokens`, `cache_read_input_tokens`, `output_tokens`, …),
`effort`, `agentId`, `timestamp`. The last line is a `user` line (the handback's tool result) or the closing
`assistant` line. Each file was last written 21–58 ms **after** its `SubagentStop` (one 38 ms before): a reader must
expect the file to lag the hook, as the docs say of transcripts. **Beside it** (not documented):
`agent-{id}.meta.json` = `{agentType, isBuiltIn, launchedWithDefaults, description, toolUseId, parentAgentId?,
spawnDepth, requestShape: "background", requestNonInteractive, pickedEffortModel | model, isFork?, name?, finished}`.

**Status line JSON** (2.1.326): `model.{id, display_name}`, `effort.level`, `thinking.enabled`, `fast_mode`,
`output_style.name`, `version`, `cost.*`, `context_window.*`, `exceeds_200k_tokens`, `workspace.*`; `agent.name` only
with `--agent`; `worktree.*` only in a worktree session. Nothing about subagents.

**Advisor**: documented (`docs/en/advisor.md`; `--advisor <model>`, setting `advisorModel`, `/advisor`,
`CLAUDE_CODE_DISABLE_ADVISOR_TOOL`). It is a server-side tool: Claude Code shows an "Advising" line in its own
transcript view; in the API the call is a `server_tool_use` block and its tokens are reported in the usage (the API
docs' "Usage and billing"). No hook and no status-line field names it. Not measured (outside the two approved runs)
and **not built**: haetty would have to read the main transcript for it; recorded for later.

### What the screen may show (the H0 table)

Only rows with a source are built. No progress percentage (no source).

| Shown | Source | Official docs |
|---|---|---|
| Agent id, type, running/done | SubagentStart / SubagentStop | yes |
| Parent and depth (nesting) | the parent's PostToolUse(Agent) `agent_id` → `tool_response.agentId` | yes (fields), the linking rule is ours |
| Background (◌ while waiting in the background) | PostToolUse(Agent) `status = async_launched`; Stop `background_tasks` | yes |
| Fork tag | `agent_type = "fork"` | partly (value not documented) |
| Worktree tag | PreToolUse(Agent) `tool_input.isolation = "worktree"` | yes (`isolation`) |
| Model (short name) | PostToolUse(Agent) `tool_response.resolvedModel` | yes |
| Current tool and target | PreToolUse with `agent_id` (tool name; `file_path`/`path`/`pattern` of the input: a file name is shown, never logged) | yes |
| Tool calls count | PostToolUse / PostToolUseFailure with `agent_id` | yes |
| Elapsed time | hook arrival times | — |
| Failure ✗ | PostToolUseFailure of the Agent tool, or a SubagentStop whose agent never started a tool and whose Agent call failed | partly |
| Tokens of a finished agent | transcript `message.usage` (read off the UI thread, after the file catches up) | file documented, fields not |
| Agent record view (prompt first line, tool calls, last message) | transcript file; `last_assistant_message` of SubagentStop for the last message | yes |
| Session model, effort | status line `model.display_name`, `effort.level` | yes |
| Permission wait of one agent | PermissionRequest with `agent_id` (to be checked in the replay; the K1 rules stay as they are) | yes (common fields) |
| Tasks | — (TaskCreated/Completed are not injected; not shown) | — |
| Advisor | — (no hook/status-line source) | — |
| Progress % | — | — |

The stop condition "agent_id missing on subagent tool hooks" does not hold: everything the Agents section needs has a
source.

## H3. ccx (cc-panel) against haetty

Line by line: `cc-panel/README.md`'s `panel.py` sections and `hook.sh`'s Agent-tool and subagent handlers (the ccx of
`claude-code-config` at this stage's start) against haetty after PH. Same / different (why) / missing → what was done.

| ccx | haetty | |
|---|---|---|
| panel `sess` (session name) | sidebar Lead, first line (bold) | same |
| panel `model` | Lead, second line, with effort (`effort.level`) | same (+ effort) |
| panel `ctx` (context %, of size) | Lead `context  34% of 200k` | same |
| panel `limits`: every `rate_limits` window as `name · used% · reset`; `5h` and `7d all` always listed, `-` when absent | Lead: every window present, `5h limit 12% · 11-05 17:33 UTC` | different: an absent window is not listed (nothing without a source, §2.1) |
| panel `cost` | Lead `cost $1.23 (API estimate)` | same |
| panel `cache` | Lead `cache warm · hit 92%` (was in the old status panel; back in H3) | missing → added |
| panel `lines` | Lead `lines +12 -4` (H3) | missing → added |
| panel `upd` | footer `updated 3s ago` | same |
| panel `workers`: summary | Agents title `3 running · 2 done · 1 failed` | same |
| running `●` lines: elapsed + first 40 characters of the description | `● type model tool target  elapsed`; with no tool running, the Agent call's description (H3) | missing description → added (from the Agent tool's `tool_input.description`; shown, never logged) |
| last 6 finished, `✓ verified` / `✗ blocked` / `? needs_review` / `- no STATUS` from SubagentStop's last message | last 6 finished and `+ N earlier`; the STATUS word (`blocked` with ✗) from SubagentStop's `last_assistant_message` (H3), else the call count | missing STATUS → added; `?` stays the permission question's glyph (colour 9), so needs_review is a word, not `?` |
| interactive: Agent returns `async_launched`, the worker stays `●` until SubagentStop | `◌` while a background agent runs, `✓` at its stop | same (glyph differs: background is visible) |
| internal agents (no `agent_type`) not shown | not in the table (stop without start) | same |
| below 44 columns no duration; values wrap; short labels below 56 columns | one line per agent, the detail cut on the left; elapsed always | different: one line per agent keeps the tree readable |
| folding when short: keys, cache, lines, cost, limits first | sections fold by the person (title click, ←/→); the Log takes what is left | different: no automatic folding (nothing changes without input) |
| panel `keys` (ccx shortcuts, one per row) | none: the palette (Ctrl+Shift+P) and hints next to elements | different on purpose (DESIGN §2.2: no resident key list) |
| `hook.sh agentpre` (type, description) | the Agent tool's PreToolUse: isolation, description | same (+ isolation) |
| `hook.sh agentpost` (status from the response text) | PostToolUse of the Agent tool: `status` (async/completed), `agentId`, `resolvedModel`, parent | same (+ parent, model) |
| `hook.sh substart` / `substop` | SubagentStart / SubagentStop: start, end, resume, STATUS | same |
| ccx `--settings` merge (user's keys win, hooks both, statusLine ours) | the same merge (`settings::merge`), also through the proxy | same |
| ccx permission defaults (bypassPermissions unless `--safe`), 39 `ask` rules for destructive git/gh, `tui`, `autoCompactWindow`, `CLAUDE_CODE_NO_FLICKER`, `TMUX_TRUECOLOR` | not injected | left out on purpose: they change Claude Code's behaviour (DESIGN §3.13) |
| diff pane (`diffview.py`, side by side from 110 columns) | the diff view (side by side from 110 columns; P3b: 100) | same |
| tab sets (`savetabs.sh`, closed tabs) | tab sets, closed tabs, and now the conversation of a shell tab (H1) | same (+ shell tabs) |
| `savesize.sh` (panel width per tab) | sidebar width per user (ui state) | different: one width for every tab (no resize on tab switch) |
| tab colours: amber bell (waiting), red `?` (asks) | tab marks (colour 3 / colour 9) | same |

Only haetty: **All agents** (palette, Ctrl+Shift+A): every tab's agents in one list, the tabs waiting for the person
first and the longest-waiting one at the top, each tab's running agents first; Enter goes to that tab. It replaces
P2's "a key to the longest-waiting worker". And `haetty agents [--json] [SESSION]` for observable tabs.

## H1, H2, H4: what was built and checked

| Commit | What | Checked |
|---|---|---|
| `0dca631` | H0: the measurement, the recordings, the stage text | gate |
| `8cc2d91` | H1: the claude proxy (protected area, approved: the same settings, nothing more) | gate; `tests/proxy.rs` 3/3 (arguments, exit code, exec keeps the pid, `--settings` merge 0600, `-p`/subcommand/no terminal/integration off/recursion unchanged, 127); `tests/permission.rs` (mark and hold for a proxied claude, back to a shell) |
| `a962f2d` | H1: resume on restore (`restore-claude`), foreground check | gate; unit tests |
| `e485506` | H2: the agent table, files, effort, `GetClaude`/`GetSessionLog` (protocol 14) | gate; replays of both real runs, out-of-order arrival, another agent's tool end during a question, failed call, resume, `/clear`, 200 limit; the queries' roles |
| `18bb4ba` | H2: the sidebar (DESIGN §2 change, approved), the record view | gate (a first attempt failed clippy on Windows: two Linux-only helpers unused; fixed); unit tests; reference frames |
| `b8c3828` | H2/H3: `haetty agents`, All agents, description and STATUS (protocol 15), lines and cache | gate; unit tests; reference frames |
| `e56f152` | `scripts/e2e/sidebar.sh`, frame dumps, fixes | gate; sidebar.sh 15/15 |

**Measured** (kramers, Xvfb, release build): GUI idle CPU 0.0 % with the sidebar shown and no agents (also with
`sidebar = hide`); 1.2 % with five agents running (the once-a-second redraw of their elapsed times; the damage
tracking redraws only changed rows); the daemon 0.0 %. Stripped sizes (the gate): Linux `haetty` 12 664 440 → 12 898
800 B (+234 360), Windows `haetty.exe` 11 673 600 → 11 912 704 B (+239 104); no new dependency. Key-to-screen latency
with agents running was not measured separately (the daemon's latency lines need a person's keys; §6.1).

**Choices** (the rule "closest to the four points", alternatives noted):
- `◌` is a running agent launched in the background, `●` one Claude Code waits for. In 2.1.326 almost every launch is
  background (H0), so most running agents show `◌`. Alternative: `◌` only for agents listed in a `Stop`'s
  `background_tasks` (after the main turn ended).
- An agent's question closes with its own tool end or stop, or when the session's question mark goes (the K1 rules
  stay the only source of the mark and hold).
- The Log comes from derived event-log lines kept in the daemon's memory (64 per session); no new file.
- The restore types `claude --resume <id>` into the new shell (read by the shell once it is ready) instead of
  starting the shell with `-c`: the shell keeps its own startup files and history.
- The sidebar settings apply at the next start (`sidebar`, width, min columns); the section switches live.
- A docked sidebar takes the Claude Code tab's status row back (§13.3) — so a Claude Code tab's grid is one row taller
  than with the old panel.

**Windows unverified**: the unit and integration tests build for `x86_64-pc-windows-gnu` (`cargo test --no-run`) and clippy passes there (the gate), but they were not run (no Windows or Wine here); the proxy (`claude.cmd`, cmd, PowerShell, Git Bash, Ctrl+C in a batch file), the reference
frames, the record view's paths (`docs/dev-commands.md`, Windows check list; `scripts/e2e/windows/proxy.ps1` written,
not run).

**For the person** (frame dumps `docs/look/ph-1` … `ph-8`): the sidebar's width (42) and threshold (121), the Lead's
order, `◌` versus `●`, the Agents line's parts (model, tags, elapsed), the Log's wording; whether the shell tab's
sidebar should show more than folder, branch and git changes.

## Moved from HANDOFF §6.0 (2026-11-17, PH: the terminal itself is on hold)

- The default engine stays libghostty. Open for the person (DESIGN §10.15, the transition table): the 744 vtdiff
  FAILs of ghostty-main (`docs/vtdiff/2026-11-09-f2.md`); the native engine's key-to-frame latency (+0.13 ms at the
  median); SGR throughput at the 80 % threshold; no native-only installer option; no Windows soak with libghostty.
- F5 (`b1262d2`): the cause of the Windows FAIL of 2026-11-10 (focus check) is open; see "Next session".
- **For the person: `decorations = auto` on Xfce.** `auto` picks `native` on every X11; DESIGN §3.12's reason (the
  window manager colours the bar, mutter follows `_GTK_THEME_VARIANT=dark`) does not hold on Xfce: xfwm4 4.18 does
  not read that property, so its bar keeps xfwm4's theme colours. `integrated` there would change the default look
  on Xfce — the person's choice (P0 record, history/2026-11-12-p0-north-star.md).
- Upstream drafts: `docs/upstream/README.md` (01 and 02 fixed at the pin; 03 still present; none sent).
