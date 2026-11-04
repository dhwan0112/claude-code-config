# pauli: differences from the baseline

- Source host of the baseline (MANIFEST "Source host: Pauli"). Snapshots `history/20261103-Pauli` … `-Pauli-7`.
- Claude Code 2.1.317, tmux 3.4, jq 1.7, python3 3.12.3 (`/usr/bin/python3`, no numpy), git 2.43.0.
- Checked 2026-11-03 23:44 +08: agents, `statusline.sh`, skills, `cc-panel/` and `bin/ccx` installed here match `current/` byte for byte (`~/.claude/skills/synced/` is the claude.ai account's and is not part of the baseline).

## Machine-specific values

- lammpkeeper repository: `~/Projects/lammpkeeper` (capital `P`; kramers uses `~/projects`).
- Interpreter: `~/miniconda3/envs/mlff/bin/python`. The last line of `~/Projects/lammpkeeper/.claude/rules/delegation.md` names it; `templates/delegation.md` is this file with that line replaced by a placeholder. `.git/info/exclude` has `.claude/rules/delegation.md`. No CLAUDE.md in the repository (AGENTS.md is loaded).
- `~/.claude/settings.json` keeps `advisorModel: "opus"` and `skipDangerousModePermissionPrompt: true` (ccx tabs start in bypass without a warning dialog). Hand edits applied: the three `permissions.ask` rules, `statusLine`, and the advisor env entry (below).

## Advisor env entry (2026-11-03 23:43 +08)

- Merged `"env": {"CLAUDE_CODE_DISABLE_ADVISOR_TOOL": "1"}` into `~/.claude/settings.json` (there was no `env` key before; all other keys unchanged). Backup: `~/.claude/backups/pauli-20261103/settings.json.before-advisor-env` (md5 cfa5c7b46fd4cfb24e611bc9f2a81282).
- Running sessions picked it up at once: the main session that made the change lost its `advisor` tool right after the save. This is expected: the entry turns the advisor off for main sessions too.
- Test: `claude -p` in `~/Projects/lammpkeeper`, with the variable removed from the shell (`env -u`). The run delegated to worker-readonly, which reported the first README heading `LammpKeeper (`lkpr`)`. Session 0f0fc5c9-9b5a-4538-81fb-def23b2a894b, subagent `agent-aaca77109fbd064e4`.
  - worker-readonly requests: model claude-sonnet-5-5, effort low, tools Read/Grep/Glob/SubagentHandback. No `advisor` anywhere in the subagent transcript (0 occurrences).
  - main request: claude-opus-5-5, effort high. No `advisor` in its tool list or transcript either.
  - So the settings.json entry alone removes the advisor from worker requests, the same result as on kramers.

## Rollback

1. `cp ~/.claude/backups/pauli-20261103/settings.json.before-advisor-env ~/.claude/settings.json`. Running sessions get the advisor back at once.
2. Baseline: `history/20261103-Pauli-7` is `current/` after this change. Kramers' version before it is `history/20261103-Kramers-1`. The only differences are `cc-panel/README.md` and two MANIFEST lines (the "Updated" line and the README md5).
