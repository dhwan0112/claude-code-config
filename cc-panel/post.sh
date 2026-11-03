#!/usr/bin/env bash
# PostToolUse hook (async) for Write/Edit/NotebookEdit in ccx sessions opened after 2026-11-03.
# 1. apply_edit.py writes <key>.after: the snapshot with this tool call applied (the file as this edit
#    left it), so the diff pane can show this edit only. 2. hook.sh post records the edit as the latest.
# .after is written before .last changes, so the pane never sees the new edit without it.
# Never prints, always exits 0.
exec >/dev/null 2>&1
[ -n "${CC_PANEL_ID:-}" ] || exit 0
input=$(cat) || exit 0
P="$HOME/.claude/cc-panel"
python3 "$P/apply_edit.py" <<<"$input"
"$P/hook.sh" post <<<"$input"
exit 0
