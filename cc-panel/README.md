# cc-panel: Claude Code workspace in tmux (ccx)

`ccx [claude arguments]` opens Claude Code on a separate tmux server (`tmux -L cc`):

- left: Claude Code with the classic renderer (no built-in diff panel)
- right top: context and token panel (from the status line input)
- right bottom: side-by-side diff (delta)
- top bar: tabs, one tmux window per Claude Code session

Nothing here changes `~/.claude/settings.json`, `~/.claude/statusline.sh` or `~/.tmux.conf`.
ccx passes its hooks and status line to Claude Code with `--settings`, so only ccx sessions run them.

## Files

| File | Role |
|---|---|
| `~/.local/bin/ccx` | Starts or reuses session `cc` on the `-L cc` server, adds a tab, splits it into three panes, gives each pane `CC_PANEL_ID` |
| `ccx_settings.py` | Builds the `--settings` JSON (classic renderer, status line, hooks) and merges a user `--settings` value |
| `left.sh` | Left pane: runs `claude`, then removes this tab's cache files and closes the tab |
| `statusline-tee.sh` | Status line: saves the input JSON to `~/.cache/cc-panel/$CC_PANEL_ID.json` (atomic), then runs `~/.claude/statusline.sh` |
| `hook.sh` | `pre`: snapshot before Write/Edit/NotebookEdit (skip over 2 MB, keep 20). `post`: record the last edited file and who edited it. `substart`/`substop`: running subagents. Never prints, always exits 0 |
| `panel.py` | Right-top pane renderer (standard library only) |
| `diffview.py` | Right-bottom pane renderer (standard library + `delta`) |
| `tmux.conf` | Config for the `-L cc` server only |

Runtime data: `~/.cache/cc-panel/` (`<id>.json`, `<id>.last`, `<id>.agents`, `<id>/snap/`, `<id>.settings.json`).

## Use

- Start: `ccx` (extra arguments go to `claude`, for example `ccx --model sonnet`, `ccx -c`).
- Inside another tmux server, ccx adds the tab and prints `TMUX= tmux -L cc attach -t cc`.
- Prefix is `C-a`; press `C-a C-a` to send a literal `C-a` to the program.
- `C-a c` new tab (Claude Code in the current pane's directory) · `C-a n` / `C-a p` / `C-a <number>` switch tabs · `C-a z` zoom the current pane · mouse drag on borders resizes panes.
- Diff pane (click it or `C-a` arrow keys to focus): `m` or `Tab` cycles views: last edit (default; snapshot before the edit vs now) → vs HEAD → whole working tree (includes changes made by Bash) · `j`/`k` line · `Space`/`b` page · `g`/`G` top/bottom · `r` refresh.
- `--settings` given to ccx is merged, not replaced: Claude Code applies only the last `--settings` flag, so ccx passes one merged value. Your keys win, except `hooks` (both sets run) and `statusLine` (ccx keeps its own and prints a warning).

## Requirements

`tmux` 3.2+, `python3`, `jq`, `git`, `delta` 0.20.1 (`~/.local/bin/delta`, x86_64 musl release, sha256 69dcb4043f55c29118f408c6efd30115e0a4e0a6709253343b8c8fa814ab4a31).

## Notes

- Closing a tab (Claude Code exits) removes that tab's cache files. Killing the whole `cc` server skips that step; at its next start ccx deletes the files of every tab that is no longer open (windows are tagged with `@ccx_id`) unless they changed in the last 10 minutes.
- The PreToolUse snapshot hook runs synchronously; PostToolUse runs with `async: true`. Measured Edit tool time on 2026-11-03 (2-3 samples each): 73-102 ms in ccx, 43-53 ms in a plain session.
- Edits outside the working directory still ask for permission in Claude Code; choose the "ask again next time" option to avoid changing user settings.
