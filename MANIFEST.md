# Claude Code worker baseline

- Date: 2026-11-03 20:40 +08
- Source host: Pauli
- Updated: 2026-11-03 23:45 +08 on kramers (advisor env hand edit, templates/delegation.md); files from Pauli unchanged
- Updated: 2026-11-03 23:44 +08 on pauli (merged onto kramers' MANIFEST: `cc-panel/README.md` development note on merging before upload; the advisor env hand edit is now applied on pauli too)
- Updated: 2026-11-04 01:55 +08 on kramers (ccx: status panel 22%/22% defaults, config file, C-a d / C-a w, no own status line in ccx tabs, orchestration section, nested outer window title, diff pane header redraw fix; new files `cc-panel/outer-title.sh`, `cc-panel/savesize.sh`, `cc-panel/togglediff.sh`)
- Updated: 2026-11-04 02:11 +08 on kramers (ccx: key help moved from the tab bar to `keys` rows at the bottom of the status panel; the tab bar shows only the tabs)
- Updated: 2026-11-04 14:50 +08 on kramers (ccx: drag a tab on the top bar to move it (`cc-panel/dragtab.sh`); renaming a tab renames the Claude Code session (`cc-panel/tabrename.sh`, window-renamed hook); `keys` rows one per key)
- Updated: 2026-11-04 15:35 +08 on kramers (ccx: the Claude pane runs with `CLAUDE_CODE_TMUX_TRUECOLOR=1`, so Claude Code draws in 24-bit colour inside tmux instead of 256 colours (`cc-panel/left.sh`); README Colours paragraph)
- Updated: 2026-11-04 15:42 +08 on kramers (ccx: tab attention colours: a tab that is not on screen turns amber when its Claude Code finished a turn or waits at the prompt, red with `?` while a permission prompt or question is pending; `cc-panel/hook.sh attn` from Stop/Notification/UserPromptSubmit/PreToolUse hooks in `cc-panel/ccx_settings.py`, bell options and `window-status-format` in `cc-panel/tmux.conf`)
- Updated: 2026-11-04 15:54 +08 on kramers (baseline moved to git: private repository github.com/dhwan0112/claude-code-config, Dropbox `history/` snapshots imported as tagged commits; Dropbox `current/` and `hosts/` are now a mirror written by `tools/mirror-dropbox.sh`; top-level README.md added)
- Updated: 2026-11-04 16:27 +08 on kramers (ccx: fullscreen renderer by default: the prompt stays at the bottom and the mouse wheel scrolls the conversation inside Claude Code; `CCX_TUI` = fullscreen | default | user (environment or `~/.config/cc-panel/config`) chooses the `tui` key in `cc-panel/ccx_settings.py` and `CLAUDE_CODE_NO_FLICKER` in `cc-panel/left.sh`, the variable because the `tui` setting alone loses to Claude Code's per-version fullscreen auto-off; README Renderer paragraph, Development notes describe the git flow)
- Updated: 2026-11-04 16:51 +08 on kramers (ccx README: Claude Code's own diff panel opens in a fullscreen tab after the first edit at 144+ columns in a git repo, verified; disabling the built-in `diff` mod does not prevent it, close it once and it stays closed)
- Claude Code version on source host: 2.1.318
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

## ccx workspace (copy `cc-panel/` to ~/.claude/cc-panel/, `bin/ccx` to ~/.local/bin/ccx)

Claude Code in a separate tmux server (`tmux -L cc`), tabs on top. Default layout `sidebar`: left top Claude Code (fullscreen renderer by default: prompt fixed at the bottom, the mouse wheel scrolls the conversation inside Claude Code; `CCX_TUI=default` gives the classic renderer, `CCX_TUI=user` leaves the choice to `~/.claude/settings.json`; `left.sh` sets `CLAUDE_CODE_NO_FLICKER` from `CCX_TUI`, because the `tui` setting alone loses to Claude Code's per-version fullscreen auto-off), left bottom diff pane (delta; side by side from 110 columns, top/bottom below), right full-height status panel (22% of the window width, at least 36 columns; the diff pane is 22% of the height, at least 6 rows). `CCX_LAYOUT=split` keeps the earlier layout. Defaults for `CCX_LAYOUT`, `CCX_SIDEBAR`, `CCX_DIFF`, `CCX_TUI` can be put in `~/.config/cc-panel/config` (`KEY=value` lines; environment variables win); `C-a w` writes the current tab's status panel width and diff height there, `C-a d` collapses the diff pane to 6 rows and expands it again, `C-a D` detaches. Pane size: `C-a` + arrow keys (repeatable); pane focus: `C-a h/j/k/l`. In ccx tabs Claude Code draws no status line of its own: the status line command prints nothing and only saves the JSON for the panel and renames the tab (`~/.claude/statusline.sh` is used unchanged outside ccx). The status panel also shows orchestration: a `workers` summary, running subagents (`●` with their description) and the last six finished ones with the `STATUS:` line of their report (`✓` verified, `✗` blocked, `?` needs_review, `-` none), from the Agent PreToolUse/PostToolUse and SubagentStart/SubagentStop hooks ccx passes with `--settings`; weekly limits are listed per window of `rate_limits` (`-` when absent). Hooks, status line and `autoCompactWindow: 180000` are passed with `--settings` by ccx only; `~/.claude/settings.json` is not changed. A user `--settings` value overrides `autoCompactWindow` for that tab. New tabs start in bypass permission mode (`permissions.defaultMode` in ccx's `--settings`) with extra ccx-only ask rules for destructive git/gh commands (`git reset --hard`, `git clean -f`, whole-tree `git checkout`/`git restore`, `git branch -D`, `gh repo delete`, `gh release delete`); `ccx --safe` or `CCX_SAFE=1` leaves bypass out. New tab shortcuts: `C-a c`, `C-a C` (asks for a folder), `C-a S` (safe), and the `+` / `safe` buttons on the top bar. Tab names follow the session name. The cc server sets the terminal title to `ccx · <active tab>`; started inside another tmux, ccx renames that outer window to `ccx · <active tab>` (automatic-rename off for that window only, nothing else on the outer server changes). The default diff view shows only the last edit (`apply_edit.py` rebuilds the result from the snapshot); its header is cut by display width, so wide characters in the path no longer wrap it onto a second row. The tab bar shows only the tabs; a tab can be dragged along the bar to move it, and renaming a tab (`C-a ,`) renames its Claude Code session as well (`tabrename.sh` types `/rename` into the Claude pane; `/rename` inside Claude Code renames the tab as before). The Claude pane runs with `CLAUDE_CODE_TMUX_TRUECOLOR=1` (24-bit colour inside tmux; `left.sh`). Tabs show attention colours: amber when Claude Code in a tab that is not on screen finished a turn or waits at the prompt (cleared when the tab is selected), red with `?` while it waits for a permission answer or a question (`hook.sh attn`, from the Stop/Notification/UserPromptSubmit/PreToolUse hooks; the terminal stays silent). The shortcuts are listed in `keys` rows at the bottom of the status panel (folded first when the pane is short). See `cc-panel/README.md` for use.

- a2dc0b45dac2d3c43994c83fb34f8a42  cc-panel/apply_edit.py
- 26c0327f2fe5dbefcade52a197187176  cc-panel/ccx_settings.py
- b554fba706869d1c317fcebf79fbd8bf  cc-panel/diffview.py
- 5d5c1f24325fe2b63b5918533706665b  cc-panel/dragtab.sh
- c403b90319d1b39177205de23331f799  cc-panel/hook.sh
- 3bc3d8eccca97d00a6a5f5d2bf5e2e3b  cc-panel/left.sh
- 618ce3091f66bc82396f3bb5f558999a  cc-panel/newtab.sh
- 42f30f33b2fd39f94026c5f0a9f9bc82  cc-panel/outer-title.sh
- f8a45b79b8c34e3b7ea1b5391e71e8f8  cc-panel/panel.py
- 1be9d675b84b54e94f2b682c819e7576  cc-panel/post.sh
- 6a6559dc2bf25c647a6ba3d6f00eabf9  cc-panel/README.md
- c068c94a3e2aa4452ed91b90bfa3d9dd  cc-panel/savesize.sh
- 03f458156e7c66fc0c66f6a6f08dee7f  cc-panel/statusline-tee.sh
- 83decc2bf6ccb4eb78d2401bcc1d9107  cc-panel/tabrename.sh
- e05f3972d7dbf702384d724b3bb21cfa  cc-panel/tmux.conf
- 3655f3577c35f6b75f5a0e159b7633ae  cc-panel/togglediff.sh
- 08deca6c289d7ad352c1bb8855eb3c37  bin/ccx

Requires `delta` 0.20.1 in `~/.local/bin` (not in this folder): download `delta-0.20.1-x86_64-unknown-linux-musl.tar.gz` from the dandavison/delta GitHub release and check sha256 69dcb4043f55c29118f408c6efd30115e0a4e0a6709253343b8c8fa814ab4a31 (GitHub asset digest; the project publishes no separate checksum file). Also `tmux` 3.2+, `jq`, `python3`.

## Not in this folder (apply by hand)

- `~/.claude/settings.json`: add to `permissions.ask`: `Bash(git push *)`, `Bash(git -C * push *)`, `Bash(gh pr merge *)`. Keep the other keys.
- `~/.claude/settings.json`: add `"statusLine": {"type": "command", "command": "~/.claude/statusline.sh"}`.
- `~/.claude/settings.json`: add `"env": {"CLAUDE_CODE_DISABLE_ADVISOR_TOOL": "1"}`. This is the only switch that removes the `advisor` tool from worker requests on 2.1.317 (`disallowedTools: advisor` in the agent files does not); it also turns the advisor off for the main session. Verified on kramers 2026-11-03 (see Notes).
- Delegation rules for the main session: copy `templates/delegation.md` (see below) to `<lammpkeeper repo>/.claude/rules/delegation.md` and replace the interpreter placeholder in its last line with the Python that imports numpy and lammpkeeper on that machine (the "Delegation to workers" section of the OpenCode AGENTS.md, adapted: Explore, SendMessage agent ID, no background wording). Add that path to the repo's `.git/info/exclude`. Do not create a CLAUDE.md there: it stops Claude Code from loading the repo's AGENTS.md.

## Templates (copy by hand, then edit)

- 4526714442287a734d094f012a40f28a  templates/delegation.md

Pauli's `.claude/rules/delegation.md` with the interpreter in the last line replaced by a placeholder. On pauli the line names `~/miniconda3/envs/mlff/bin/python`; on kramers `/usr/bin/python3`.

## Apply on another computer

1. Check `claude --version` (2.1.271 or later).
2. Back up `~/.claude/agents` and `~/.claude/skills` if they exist. Leave `~/.claude/skills/synced/` alone (claude.ai account skills).
3. Copy `agents/`, `skills/`, `statusline.sh` and `cc-panel/` from this folder into `~/.claude/`, and `bin/ccx` into `~/.local/bin/`. Copy, do not symlink. Make the scripts executable.
4. Apply the three `settings.json` hand edits above, and copy `templates/delegation.md` into the lammpkeeper repository as described there (edit the interpreter line, add it to `.git/info/exclude`).
5. Compare md5 of the agent files with the values above.

## Notes

- Claude Code applies changes to `~/.claude/settings.json` (documented) and `~/.claude/skills` (observed on 2.1.317) to running sessions without a restart. Check for running `claude` processes before you change them.
- Advisor: with `advisorModel` set in user settings, subagents inherit the `advisor` tool (Opus). The agents set `disallowedTools: advisor`, but on 2.1.317 the tool was still attached to worker-fast and worker-deep requests (observed 2026-11-03). The only documented switch is `CLAUDE_CODE_DISABLE_ADVISOR_TOOL=1`, which also turns the advisor off for the main session. Verified on kramers 2026-11-03: with `"env": {"CLAUDE_CODE_DISABLE_ADVISOR_TOOL": "1"}` in `~/.claude/settings.json` (and the variable absent from the shell), worker-readonly requests carried Glob/Grep/Read and worker-fast requests Bash/Edit/Read/Write, with no `advisor` tool. It is now one of the hand edits above. Workers that have `Bash` did not get `Glob`/`Grep`.
- Auto-compaction default for 1M models is about 967K tokens. User settings are not changed; ccx sessions use `autoCompactWindow: 180000` through `--settings`. Other ways to lower it: `autoCompactWindow` in settings or `CLAUDE_CODE_AUTO_COMPACT_WINDOW`.
- Interpreter paths are machine-specific. Write them in each computer's local, uncommitted rules file (`<repo>/.claude/rules/delegation.md`, excluded through `.git/info/exclude`), not in the agents, skills or committed repository files. On pauli: lammpkeeper uses `~/miniconda3/envs/mlff/bin/python`.
- ccx: Claude Code applies only the last `--settings` flag, so ccx merges a user `--settings` value into its own (user keys win except `hooks`, which are combined, and `statusLine`, which ccx keeps). Closing a tab removes its cache files; if a tab ends without that step (server killed, `C-a &`), ccx removes the files of tabs not open on any of the user's tmux servers at its next start (files changed in the last 10 minutes are kept).
- ccx bypass default: no warning dialog on this machine because `~/.claude/settings.json` has `skipDangerousModePermissionPrompt: true`. On a machine without that key, expect Claude Code's bypass warning dialog when a ccx tab starts.
- ccx PostToolUse hook runs async, so its records can arrive out of order. It replaces `<id>.last` only when the edit is newer (edit time = when the synchronous PreToolUse hook ran), under `flock`. Unit test 2026-11-03: 100 rounds of near-simultaneous main and worker edits, newest kept every time.
- ccx 2026-11-04: `~/.config/cc-panel/config` is read by ccx only (`C-a w` writes it; `C-a c` and the top-bar buttons go through ccx, so they use it too). Running tabs keep the panel/diff code they started with; new hook and status-line code is picked up at the next event because ccx calls the scripts by path. `~/.config/cc-panel` is not part of this folder and is not created unless `C-a w` is used.
- ccx nested in another tmux: ccx renames the outer window only when the outer server is not a ccx server itself (`@ccx_home` unset). Run test launches with `env -u TMUX -u TMUX_PANE` from inside a ccx tab, or the current tab gets renamed (happened on kramers 2026-11-04, restored).
