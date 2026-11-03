#!/usr/bin/env bash
# C-a w: save the current tab's status panel width and diff height as the ccx defaults
# (~/.config/cc-panel/config: CCX_SIDEBAR, CCX_DIFF as percentages with one decimal; other lines kept).
# Usage: savesize.sh SOCKET WINDOW_ID   (#{b:socket_path}, #{window_id})
# The panes are found by their @ccx_role pane option (panel, diff), set by ccx.
set -u
sock=${1:-}; win=${2:-}
[[ "$sock" =~ ^[A-Za-z0-9_-]+$ ]] && [[ "$win" =~ ^@[0-9]+$ ]] || exit 0
msg() { tmux -L "$sock" display-message -t "$win" -- "$1" 2>/dev/null; }
read -r ww wh zoomed < <(tmux -L "$sock" display-message -p -t "$win" '#{window_width} #{window_height} #{window_zoomed_flag}' 2>/dev/null)
[[ "${ww:-}" =~ ^[0-9]+$ ]] && [[ "${wh:-}" =~ ^[0-9]+$ ]] || exit 0
if [ "${zoomed:-0}" = 1 ]; then msg "ccx: unzoom first (C-a z), then save"; exit 0; fi
pw=""; dh=""
while IFS=' ' read -r role w h; do
  case $role in panel) pw=$w ;; diff) dh=$h ;; esac
done < <(tmux -L "$sock" list-panes -t "$win" -F '#{@ccx_role} #{pane_width} #{pane_height}' 2>/dev/null)
if [ -z "$pw" ] || [ -z "$dh" ]; then msg "ccx: not a ccx tab (no status panel / diff pane found)"; exit 0; fi
pct() { local part=$1 total=$2 t; t=$(( (part * 1000 + total / 2) / total )); printf '%d.%d%%' $(( t / 10 )) $(( t % 10 )); }
side=$(pct "$pw" "$ww"); diff=$(pct "$dh" "$wh")
cfg="${XDG_CONFIG_HOME:-$HOME/.config}/cc-panel/config"
mkdir -p "${cfg%/*}" || { msg "ccx: cannot create ${cfg%/*}"; exit 0; }
tmp="$cfg.tmp.$$"
{
  if [ -f "$cfg" ]; then grep -v -E '^[[:space:]]*(CCX_SIDEBAR|CCX_DIFF)[[:space:]]*=' "$cfg"; else echo "# ccx defaults (written by C-a w; environment variables win)"; fi
  echo "CCX_SIDEBAR=$side"
  echo "CCX_DIFF=$diff"
} > "$tmp" && mv -f -- "$tmp" "$cfg" || { rm -f -- "$tmp"; msg "ccx: cannot write $cfg"; exit 0; }
msg "ccx: saved defaults: status panel $side ($pw cols), diff $diff ($dh rows) → ${cfg/#$HOME/\~}"
exit 0
