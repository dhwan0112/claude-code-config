#!/usr/bin/env bash
# Left pane of a ccx window: run Claude Code, then clean up this window's panel files and close the window.
D="$HOME/.cache/cc-panel"
# Claude Code (2.1.318) caps its colours at 256 when TMUX is set in its environment, whatever COLORTERM says, so the
# logo and the "bypass permissions on" line come out pale inside tmux. CLAUDE_CODE_TMUX_TRUECOLOR lifts that cap; ccx's
# tmux.conf declares RGB for every outer terminal (terminal-features *:RGB), so 24-bit colour reaches the terminal.
# Run ccx with CLAUDE_CODE_TMUX_TRUECOLOR= (empty) to keep the 256-colour output.
export CLAUDE_CODE_TMUX_TRUECOLOR="${CLAUDE_CODE_TMUX_TRUECOLOR-1}"
# Renderer (CCX_TUI from ccx: fullscreen | default | user). fullscreen: the prompt stays at the bottom and the mouse wheel scrolls the
# conversation inside Claude Code (tmux hands the wheel to the pane while its alternate screen is on). The "tui" key in ccx's --settings
# is not enough on its own: after two fullscreen starts that did not finish, Claude Code keeps the classic renderer for that version
# (fullscreenAutoDisabled in ~/.claude.json) and ignores the setting; CLAUDE_CODE_NO_FLICKER=1 is the override it still honours, and
# such a session is not counted. default: classic renderer, even if the environment carries CLAUDE_CODE_NO_FLICKER=1. user: untouched.
case "${CCX_TUI:-fullscreen}" in
  fullscreen) export CLAUDE_CODE_NO_FLICKER=1 ;;
  default) export CLAUDE_CODE_NO_FLICKER=0 ;;
esac
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
