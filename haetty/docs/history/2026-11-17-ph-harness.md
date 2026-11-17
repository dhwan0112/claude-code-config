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
