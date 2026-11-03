# Claude Code worker baseline

- Date: 2026-11-03 16:47 +08
- Source host: Pauli
- Claude Code version on source host: 2.1.317
- Minimum Claude Code version: 2.1.271 (`omitClaudeMd` in subagent frontmatter)
- Mirrors the OpenCode baseline in ~/Dropbox/opencode-config/current (orchestrator + generic workers).

## Agents (copy to ~/.claude/agents/)

- 2a3e3274234aa8138ac0f5a84479ee48  worker-deep.md
- 8ac1b4a70821f64f5df3505cdce1c51c  worker-fast.md
- be286868fad178a55767e2b804d9bfdc  worker-readonly.md

| Agent | model | effort | tools | preloaded skills | omitClaudeMd |
|---|---|---|---|---|---|
| worker-fast | haiku | high | Read, Write, Edit, Glob, Grep, Bash | verify, lammpkeeper | false |
| worker-deep | sonnet | medium | Read, Write, Edit, Glob, Grep, Bash | tdd, verify, lammpkeeper | false |
| worker-readonly | sonnet | low | Read, Grep, Glob | lammpkeeper | true |

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

## Not in this folder (apply by hand)

- `~/.claude/settings.json`: add to `permissions.ask`: `Bash(git push *)`, `Bash(git -C * push *)`, `Bash(gh pr merge *)`. Keep the other keys.
- Delegation rules for the main session: `<lammpkeeper repo>/.claude/rules/delegation.md` (the "Delegation to workers" section of the OpenCode AGENTS.md, adapted: Explore, SendMessage agent ID, no background wording). Add that path to the repo's `.git/info/exclude`. Do not create a CLAUDE.md there: it stops Claude Code from loading the repo's AGENTS.md.

## Apply on another computer

1. Check `claude --version` (2.1.271 or later).
2. Back up `~/.claude/agents` and `~/.claude/skills` if they exist. Leave `~/.claude/skills/synced/` alone (claude.ai account skills).
3. Copy `agents/` and `skills/` from this folder into `~/.claude/`. Copy, do not symlink.
4. Apply the two hand edits above.
5. Compare md5 of the agent files with the values above.

## Notes

- Claude Code applies changes to `~/.claude/settings.json` (documented) and `~/.claude/skills` (observed on 2.1.317) to running sessions without a restart. Check for running `claude` processes before you change them.
- On the source host (2.1.317, with `advisorModel: "opus"` in user settings, the likely source), subagents got an `advisor` tool (Opus) that is not in their `tools` list. Workers that have `Bash` did not get `Glob`/`Grep`.
- Auto-compaction default for 1M models is about 967K tokens. Not changed here; `autoCompactWindow` (settings) or `CLAUDE_CODE_AUTO_COMPACT_WINDOW` can lower it.
