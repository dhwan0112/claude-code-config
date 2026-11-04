#!/usr/bin/env bash
# Left pane of a ccx window: run Claude Code, then clean up this window's panel files and close the window.
D="$HOME/.cache/cc-panel"
# Claude Code (2.1.318) caps its colours at 256 when TMUX is set in its environment, whatever COLORTERM says, so the
# logo and the "bypass permissions on" line come out pale inside tmux. CLAUDE_CODE_TMUX_TRUECOLOR lifts that cap; ccx's
# tmux.conf declares RGB for every outer terminal (terminal-features *:RGB), so 24-bit colour reaches the terminal.
# Run ccx with CLAUDE_CODE_TMUX_TRUECOLOR= (empty) to keep the 256-colour output.
export CLAUDE_CODE_TMUX_TRUECOLOR="${CLAUDE_CODE_TMUX_TRUECOLOR-1}"
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
