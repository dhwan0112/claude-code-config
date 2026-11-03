#!/usr/bin/env bash
# Left pane of a ccx window: run Claude Code, then clean up this window's panel files and close the window.
D="$HOME/.cache/cc-panel"
claude --settings "$D/$CC_PANEL_ID.settings.json" "$@"
rc=$?
if [ "$rc" -ne 0 ]; then
  printf '\nclaude exited with status %s. Press Enter to close this tab.' "$rc"
  read -r _
fi
case "$CC_PANEL_ID" in
  ''|*[!A-Za-z0-9_-]*) ;;
  *) rm -rf -- "$D/$CC_PANEL_ID" "$D/$CC_PANEL_ID".* "$D/.$CC_PANEL_ID".* ;;
esac
tmux kill-window -t "$TMUX_PANE" 2>/dev/null
exit "$rc"
