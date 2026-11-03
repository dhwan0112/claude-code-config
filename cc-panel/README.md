# cc-panel: Claude Code workspace in tmux (ccx)

`ccx [claude arguments]` opens Claude Code on a separate tmux server (`tmux -L cc`). Each tab (tmux window) has three panes.

Layout `sidebar` (default):

- left top: Claude Code with the classic renderer (no built-in diff panel)
- left bottom: diff (delta), height `CCX_DIFF` (default `40%`)
- right, full height: context and token panel, width `CCX_SIDEBAR` (default `30%`, at least 36 columns)

Layout `split` (`CCX_LAYOUT=split ccx`): left Claude Code · right top panel (14 rows) · right bottom diff.

`CCX_SIDEBAR` and `CCX_DIFF` take a percentage (`35%`) or a cell count (`50`). The layout applies to tabs opened after the change; open tabs keep theirs.
The top bar shows the tabs.

Nothing here changes `~/.claude/settings.json`, `~/.claude/statusline.sh` or `~/.tmux.conf`.
ccx passes its hooks and status line to Claude Code with `--settings`, so only ccx sessions run them.

## Files

| File | Role |
|---|---|
| `~/.local/bin/ccx` | Starts or reuses session `cc` on the `-L cc` server (`CCX_SOCKET` to use another socket), adds a tab, splits it into three panes in the chosen layout, gives each pane `CC_PANEL_ID` |
| `ccx_settings.py` | Builds the `--settings` JSON (classic renderer, `autoCompactWindow` 180000, status line, hooks) and merges a user `--settings` value |
| `left.sh` | Left pane: runs `claude`, then removes this tab's cache files and closes the tab |
| `statusline-tee.sh` | Status line: saves the input JSON to `~/.cache/cc-panel/$CC_PANEL_ID.json` (atomic), then runs `~/.claude/statusline.sh` |
| `hook.sh` | `pre`: snapshot before Write/Edit/NotebookEdit (skip over 2 MB, keep 20). `post` (async): record the last edited file and who edited it, only if this edit is newer than the recorded one (edit time = when its `pre` hook ran; compare-and-replace under `flock`). `substart`/`substop`: running subagents. Never prints, always exits 0 |
| `panel.py` | Panel renderer (standard library only). Below 56 columns it switches to short labels and values, for example `7d 16% · 11-05 01:00` |
| `diffview.py` | Diff renderer (standard library + `delta`). Side by side when the pane is at least 110 columns wide (`CCX_SBS_MIN`), otherwise top/bottom with a note in the header; it switches back when the pane gets wider (resize or `C-a z`) |
| `tmux.conf` | Config for the `-L cc` server only |

Runtime data: `~/.cache/cc-panel/` (`<id>.json`, `<id>.last`, `<id>.agents`, `<id>/snap/`, `<id>.settings.json`).

## Use

- Start: `ccx` (extra arguments go to `claude`, for example `ccx --model sonnet`, `ccx -c`).
- Inside another tmux server, ccx adds the tab and prints `TMUX= tmux -L cc attach -t cc`.
- Prefix is `C-a`; press `C-a C-a` to send a literal `C-a` to the program.
- `C-a c` new tab (Claude Code in the current pane's directory) · `C-a n` / `C-a p` / `C-a <number>` switch tabs · `C-a z` zoom the current pane.
- Pane size: `C-a` then arrow keys (press the arrow again within 600 ms to repeat, no prefix needed), or drag borders with the mouse.
- Pane focus: `C-a h`/`j`/`k`/`l`, `C-a o`, or click. (Arrow keys resize instead of moving focus; `C-a l` no longer jumps to the last window.)
- Diff pane (focus it first): `m` or `Tab` cycles views: last edit (default; snapshot before the edit vs now) → vs HEAD → whole working tree (includes changes made by Bash) · `j`/`k` line · `Space`/`b` page · `g`/`G` top/bottom · `r` refresh.
- Auto-compaction: ccx sessions compact at about 180k tokens (`autoCompactWindow: 180000` in ccx's `--settings`; `/autocompact` in a ccx tab shows "180k tokens (from settings)"). Pass `--settings '{"autoCompactWindow": 300000}'` to use another value; other sessions are not affected.
- `--settings` given to ccx is merged, not replaced: Claude Code applies only the last `--settings` flag, so ccx passes one merged value. Your keys win, except `hooks` (both sets run) and `statusLine` (ccx keeps its own and prints a warning).

## Requirements

`tmux` 3.2+, `python3`, `jq`, `git`, `delta` 0.20.1 (`~/.local/bin/delta`, x86_64 musl release, sha256 69dcb4043f55c29118f408c6efd30115e0a4e0a6709253343b8c8fa814ab4a31).

## Notes

- Closing a tab (Claude Code exits) removes that tab's cache files. Killing the whole server, or closing a window with `C-a &`, skips that step; at its next start ccx deletes the files of every tab that is no longer open on any of your tmux servers (windows are tagged with `@ccx_id`) unless they changed in the last 10 minutes.
- The default diff view compares the snapshot taken just before the last edit with the file as it is now, so later changes to the same file (by another session, Bash or an editor) show up in it too. Use the other views to tell them apart.
- The PreToolUse snapshot hook runs synchronously; PostToolUse runs with `async: true`. Measured Edit tool time on 2026-11-03 (2-3 samples each): 73-102 ms in ccx, 43-53 ms in a plain session.
- Edits outside the working directory still ask for permission in Claude Code; choose the "ask again next time" option to avoid changing user settings.
