#!/usr/bin/env bash
# C-a d: collapse the diff pane to its minimum height (6 rows) or expand it back.
# Usage: togglediff.sh SOCKET WINDOW_ID   (#{b:socket_path}, #{window_id})
# The height before collapsing is kept in the window option @ccx_diffh; without one (pane already small
# when first toggled) the pane expands to 22% of the window height, at least 12 rows.
set -u
MIN=6
sock=${1:-}; win=${2:-}
[[ "$sock" =~ ^[A-Za-z0-9_-]+$ ]] && [[ "$win" =~ ^@[0-9]+$ ]] || exit 0
msg() { tmux -L "$sock" display-message -t "$win" -- "$1" 2>/dev/null; }
read -r wh zoomed < <(tmux -L "$sock" display-message -p -t "$win" '#{window_height} #{window_zoomed_flag}' 2>/dev/null)
[[ "${wh:-}" =~ ^[0-9]+$ ]] || exit 0
if [ "${zoomed:-0}" = 1 ]; then msg "ccx: unzoom first (C-a z)"; exit 0; fi
pane=""; dh=""
while IFS=' ' read -r role pid h; do
  [ "$role" = diff ] && { pane=$pid; dh=$h; }
done < <(tmux -L "$sock" list-panes -t "$win" -F '#{@ccx_role} #{pane_id} #{pane_height}' 2>/dev/null)
if [ -z "$pane" ]; then msg "ccx: no diff pane in this tab"; exit 0; fi
if [ "$dh" -le "$MIN" ]; then
  want=$(tmux -L "$sock" show-options -wqv -t "$win" @ccx_diffh 2>/dev/null)
  [[ "$want" =~ ^[0-9]+$ ]] && [ "$want" -gt "$MIN" ] || { want=$(( (wh * 22 + 50) / 100 )); (( want < 12 )) && want=12; }
  (( want > wh - 8 )) && want=$(( wh - 8 > MIN ? wh - 8 : MIN ))
  tmux -L "$sock" resize-pane -t "$pane" -y "$want" 2>/dev/null
else
  tmux -L "$sock" set-option -w -t "$win" @ccx_diffh "$dh" 2>/dev/null
  tmux -L "$sock" resize-pane -t "$pane" -y "$MIN" 2>/dev/null
fi
exit 0
