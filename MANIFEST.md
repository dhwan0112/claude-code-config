# Claude Code worker baseline

- Date: 2026-11-03 18:21 +08
- Source host: Pauli
- Claude Code version on source host: 2.1.317
- Minimum Claude Code version: 2.1.271 (`omitClaudeMd` in subagent frontmatter)
- Mirrors the OpenCode baseline in ~/Dropbox/opencode-config/current (orchestrator + generic workers).

## Agents (copy to ~/.claude/agents/)

- ff6c6a50c7c88d6106e8641554fac8c6  worker-deep.md
- 31e78d71729c2fddadbd8cee6c83911d  worker-fast.md
- a312541685fe8fc04fb0adff5514bfe1  worker-readonly.md

| Agent | model | effort | tools | preloaded skills | omitClaudeMd |
|---|---|---|---|---|---|
| worker-fast | haiku | high | Read, Write, Edit, Glob, Grep, Bash | verify, lammpkeeper | false |
| worker-deep | sonnet | medium | Read, Write, Edit, Glob, Grep, Bash | tdd, verify, lammpkeeper | false |
| worker-readonly | sonnet | low | Read, Grep, Glob | lammpkeeper | true |

Every agent also has `disallowedTools: advisor` (see Notes: on 2.1.317 this does not remove the tool).
`Agent` and `Skill` are left out of every tools list, so workers cannot spawn subagents and cannot load skills other than the preloaded ones.
`verify` and `lammpkeeper` are project skills in the lammpkeeper repository (`.claude/skills/`). They are preloaded only when the session runs inside that repository.

## Skills (copy to ~/.claude/skills/)

Copied from https://github.com/michael-denyer/pstack-claude at commit 5851c89613e2ddc3a6b7f4af02a42363b30ef8d3, identical to the OpenCode baseline.
Each skill folder has a SOURCE.md with its path and local modifications.

- blast-radius
- correct
- create-verification-skill
- maintain-verification-skill
- tdd
- technical-writing
- unslop

Do not install the pstack-claude plugin. Its SessionStart hook turns on the poteto-mode router, which conflicts with the delegation rules.

## Status line (copy to ~/.claude/statusline.sh)

- 403e07d9e8bd8faed10c5b3dbb397079  statusline.sh

Two lines: model · effort | directory | git branch (worktree), then a context bar with % and tokens | 5h / 7d usage | session cost. On a subscription the cost is shown as `~$x API-equiv (subscription)`, an estimate at list price. Needs `jq`. Make it executable (`chmod +x`).

## Not in this folder (apply by hand)

- `~/.claude/settings.json`: add to `permissions.ask`: `Bash(git push *)`, `Bash(git -C * push *)`, `Bash(gh pr merge *)`. Keep the other keys.
- `~/.claude/settings.json`: add `"statusLine": {"type": "command", "command": "~/.claude/statusline.sh"}`.
- Delegation rules for the main session: `<lammpkeeper repo>/.claude/rules/delegation.md` (the "Delegation to workers" section of the OpenCode AGENTS.md, adapted: Explore, SendMessage agent ID, no background wording). Add that path to the repo's `.git/info/exclude`. Do not create a CLAUDE.md there: it stops Claude Code from loading the repo's AGENTS.md.

## Apply on another computer

1. Check `claude --version` (2.1.271 or later).
2. Back up `~/.claude/agents` and `~/.claude/skills` if they exist. Leave `~/.claude/skills/synced/` alone (claude.ai account skills).
3. Copy `agents/`, `skills/` and `statusline.sh` from this folder into `~/.claude/`. Copy, do not symlink.
4. Apply the two hand edits above.
5. Compare md5 of the agent files with the values above.

## Notes

- Claude Code applies changes to `~/.claude/settings.json` (documented) and `~/.claude/skills` (observed on 2.1.317) to running sessions without a restart. Check for running `claude` processes before you change them.
- Advisor: with `advisorModel` set in user settings, subagents inherit the `advisor` tool (Opus). The agents set `disallowedTools: advisor`, but on 2.1.317 the tool was still attached to worker-fast and worker-deep requests (observed 2026-11-03). The only documented switch is `CLAUDE_CODE_DISABLE_ADVISOR_TOOL=1`, which also turns the advisor off for the main session. Workers that have `Bash` did not get `Glob`/`Grep`.
- Auto-compaction default for 1M models is about 967K tokens. Not changed here; `autoCompactWindow` (settings) or `CLAUDE_CODE_AUTO_COMPACT_WINDOW` can lower it.
- Interpreter paths are machine-specific. Write them in each computer's local, uncommitted rules file (`<repo>/.claude/rules/delegation.md`, excluded through `.git/info/exclude`), not in the agents, skills or committed repository files. On pauli: lammpkeeper uses `~/miniconda3/envs/mlff/bin/python`.
