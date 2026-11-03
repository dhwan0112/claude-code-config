# cc-panel: Claude Code workspace in tmux (ccx)

`ccx [claude arguments]` opens Claude Code on a separate tmux server (`tmux -L cc`). Each tab (tmux window) has three panes.

Layout `sidebar` (default):

- left top: Claude Code with the classic renderer (no built-in diff panel)
- left bottom: diff (delta), height `CCX_DIFF` (default `40%`)
- right, full height: context and token panel, width `CCX_SIDEBAR` (default `30%`, at least 36 columns)

Layout `split` (`CCX_LAYOUT=split ccx`): left Claude Code · right top panel (14 rows) · right bottom diff.

`CCX_SIDEBAR` and `CCX_DIFF` take a percentage (`35%`) or a cell count (`50`). The layout applies to tabs opened after the change; open tabs keep theirs.
The top bar shows the tabs.

New tabs start in **bypass permissions** mode (see Permissions below). Nothing here changes `~/.claude/settings.json`, `~/.claude/statusline.sh` or `~/.tmux.conf`.
ccx passes its hooks and status line to Claude Code with `--settings`, so only ccx sessions run them.

## Files

| File | Role |
|---|---|
| `~/.local/bin/ccx` | Starts or reuses session `cc` on the `-L cc` server (`CCX_SOCKET` to use another socket), adds a tab, splits it into three panes in the chosen layout, gives each pane `CC_PANEL_ID` |
| `ccx_settings.py` | Builds the `--settings` JSON (classic renderer, `autoCompactWindow` 180000, status line, hooks, ccx `ask` rules, `defaultMode: bypassPermissions` unless safe) and merges a user `--settings` value |
| `newtab.sh` | Opens a ccx tab on a given tmux socket and folder; used by `C-a C`, `C-a S` and the `+` / `safe` buttons. `--once` drops repeated clicks (lock plus 1.5 s cooldown) |
| `left.sh` | Left pane: runs `claude`, then removes this tab's cache files and closes the tab |
| `statusline-tee.sh` | Status line: saves the input JSON to `~/.cache/cc-panel/$CC_PANEL_ID.json` (atomic), renames the tab after the session name when it changed (tabs started with `CCX_TAB_RENAME=1`), then runs `~/.claude/statusline.sh` |
| `hook.sh` | `pre`: snapshot before Write/Edit/NotebookEdit (skip over 2 MB, keep 20). `post` (async): record the last edited file and who edited it, only if this edit is newer than the recorded one (edit time = when its `pre` hook ran; compare-and-replace under `flock`). `substart`/`substop`: running subagents. Never prints, always exits 0 |
| `post.sh` | Async PostToolUse hook: runs `apply_edit.py`, then `hook.sh post` |
| `apply_edit.py` | Writes `<snap>/<tool_use_id>.after`: the pre-edit snapshot with this tool call applied (Edit: old→new, every occurrence with `replace_all`; Write: the content). Skipped when `old_string` is not in the snapshot, for NotebookEdit, non-UTF-8 files and results over 2 MB. Pruned with its snapshot (last 20) |
| `panel.py` | Panel renderer (standard library only). Below 56 columns it switches to short labels and values, for example `7d 16% · 11-05 01:00` |
| `diffview.py` | Diff renderer (standard library + `delta`). Side by side when the pane is at least 110 columns wide (`CCX_SBS_MIN`), otherwise top/bottom with a note in the header; it switches back when the pane gets wider (resize or `C-a z`) |
| `tmux.conf` | Config for the `-L cc` server only |

Runtime data: `~/.cache/cc-panel/` (`<id>.json`, `<id>.last`, `<id>.agents`, `<id>.tabname`, `<id>/snap/` with `.snap`/`.meta`/`.after`, `<id>.settings.json`).

## Use

- Start: `ccx` (extra arguments go to `claude`, for example `ccx --model sonnet`, `ccx -c`).
- Inside another tmux server, ccx adds the tab and prints `TMUX= tmux -L cc attach -t cc`.
- Prefix is `C-a`; press `C-a C-a` to send a literal `C-a` to the program.
- New tab: `C-a c` (current pane's directory) · `C-a C` (asks for a folder; default is the current pane's, `~` works; a missing folder only shows a message) · `C-a S` (current directory, `--safe`) · click `+` on the top bar (same as `C-a c`) or `safe` next to it (same as `C-a S`). Repeated clicks within 1.5 s open one tab.
- Tab names start as `repo:branch` (or the folder name) and then follow the session name (`/rename`, or the title Claude Code gives the session), up to 20 characters, with `#`, `;`, quotes, backslash, `$`, backtick, braces and control characters removed. An empty session name puts the first name back.
- `C-a n` / `C-a p` / `C-a <number>` switch tabs · `C-a z` zoom the current pane.
- Pane size: `C-a` then arrow keys (press the arrow again within 600 ms to repeat, no prefix needed), or drag borders with the mouse.
- Pane focus: `C-a h`/`j`/`k`/`l`, `C-a o`, or click. (Arrow keys resize instead of moving focus; `C-a l` no longer jumps to the last window.)
- Diff pane (focus it first): `m` or `Tab` cycles views: last edit (default; snapshot before the edit vs that snapshot with the edit applied, so later changes to the file do not show; marked `(current file)` when it falls back to the file as it is now) → vs HEAD → whole working tree (includes changes made by Bash) · `j`/`k` line · `Space`/`b` page · `g`/`G` top/bottom · `r` refresh.
- Auto-compaction: ccx sessions compact at about 180k tokens (`autoCompactWindow: 180000` in ccx's `--settings`; `/autocompact` in a ccx tab shows "180k tokens (from settings)"). Pass `--settings '{"autoCompactWindow": 300000}'` to use another value; other sessions are not affected.
- `--settings` given to ccx is merged, not replaced: Claude Code applies only the last `--settings` flag, so ccx passes one merged value. Your keys win, except `hooks` (both sets run) and `statusLine` (ccx keeps its own and prints a warning).

## Permissions

- Each tab starts in bypass mode: ccx puts `permissions.defaultMode: "bypassPermissions"` in its `--settings` (the documented setting; no extra CLI flag). No warning dialog appears because `~/.claude/settings.json` already has `skipDangerousModePermissionPrompt: true`; without that key, expect Claude Code's bypass warning dialog (not tested here).
- Turn it off for one tab: `ccx --safe` or `CCX_SAFE=1 ccx` (also `C-a S` and the `safe` button). The global `defaultMode` then applies (auto on this machine). `C-a c` and `+` always open bypass tabs, even on a server started with `CCX_SAFE=1`.
- `ccx --permission-mode <mode>` or `ccx --dangerously-skip-permissions` goes to `claude` unchanged and ccx adds no `defaultMode` (a CLI flag beats settings anyway).
- Subagents inherit the mode. Ask rules still prompt in bypass mode, for the main session and for subagents.
- Ask rules: the global ones in `~/.claude/settings.json` (`git push *`, `git -C * push *`, `gh pr merge *`) plus ccx's own, each in a `git X` and a `git * X` form (so `git -C dir X` and `git -c k=v X` match too):
  - `git reset *--hard*`
  - `git clean *-*f*` (`-f`, `-fd`, `-xdf`, `--force`)
  - `git checkout` of the whole tree (`.`, `./`, `:/`), `git checkout -f` / `--force`
  - `git restore` of the whole tree (`.`, `./`, `:/`)
  - `git branch -D` and the `-d`+`-f` forms
  - `git * push*`
  - `gh repo delete`, `gh release delete`, `gh api` with `DELETE`/`delete`
- Not covered (these run without a prompt in bypass mode): commands wrapped in `bash -c`, `sh -c` or `eval`, shell aliases and scripts, `xargs` / `find -exec`, `git clean` with `clean.requireForce=false`, abbreviated options such as `--har`. `/usr/bin/git reset --hard` and `env git reset --hard` did prompt in the test. The patterns also over-match a little (for example `git checkout feature.` or a `git reset` whose message mentions `--hard`); those just ask.

## Requirements

`tmux` 3.2+, `python3`, `jq`, `git`, `delta` 0.20.1 (`~/.local/bin/delta`, x86_64 musl release, sha256 69dcb4043f55c29118f408c6efd30115e0a4e0a6709253343b8c8fa814ab4a31).

## Notes

- Closing a tab (Claude Code exits) removes that tab's cache files. Killing the whole server, or closing a window with `C-a &`, skips that step; at its next start ccx deletes the files of every tab that is no longer open on any of your tmux servers (windows are tagged with `@ccx_id`) unless they changed in the last 10 minutes.
- The default diff view shows only what the last Edit/Write changed. When the edit cannot be rebuilt (NotebookEdit, `old_string` not in the snapshot, non-UTF-8, over 2 MB) it compares with the current file and the header says `(current file)`; later changes by Bash or an editor show up in that case.
- The PreToolUse snapshot hook runs synchronously; PostToolUse runs with `async: true`. Measured Edit tool time on 2026-11-03 (2-3 samples each): 73-102 ms in ccx, 43-53 ms in a plain session.
- Changes to `tmux.conf` apply when the `cc` server starts again (or after `tmux -L cc source-file ~/.claude/cc-panel/tmux.conf`). Open tabs keep running.
- Edits outside the working directory still ask for permission in Claude Code; choose the "ask again next time" option to avoid changing user settings.

## Development notes

- Test in `~/.cache/ccx-testdir` only, with a separate socket (`CCX_SOCKET=ccxt`, `tmux -L ccxt ...`), and kill that server afterwards. The folder is trusted once in `~/.claude.json`; a new folder for every test adds a trust entry each time. Empty the folder after a test but keep it.
- Do not reload the `cc` server or edit `hook.sh` / `left.sh` / `statusline-tee.sh` in place while tabs run: write a new file and `mv` it over the old one.
- Publishing to the baseline (`~/Dropbox/claude-code-config/current`): other computers edit it too. First read the current `MANIFEST.md` there and compare `current/` with your last snapshot in `history/`. Start your staging copy from `current/` and merge your changes into it (MANIFEST text, md5 lines, files such as `templates/`). Then upload it. Never upload a stale staging copy over `current/`.
