#!/usr/bin/env bash
# window-renamed hook: when the user renames a ccx tab (C-a , or rename-window), rename the Claude Code session to match
# by typing "/rename <name>" into that tab's Claude pane. Renames that came from the status line (session name → tab name,
# statusline-tee.sh) are recognised because the tab then already equals the session name, and ignored.
# Usage: tabrename.sh SOCKET WINDOW_ID   (#{b:socket_path}, #{window_id})
# The name Claude gets is cleaned like statusline-tee.sh cleans session names (control characters and # ; quotes \ $ ` { }
# removed, trimmed, 20 characters), so the tab keeps that name afterwards. Nothing is typed when no claude process of this
# tab is running. If Claude Code is busy, the command is queued like any typed input and applied when the turn ends.
set -u
sock=${1:-}; win=${2:-}
[[ "$sock" =~ ^[A-Za-z0-9_-]+$ ]] && [[ "$win" =~ ^@[0-9]+$ ]] || exit 0
T() { tmux -L "$sock" "$@"; }
id=$(T show-options -wqv -t "$win" @ccx_id 2>/dev/null); [[ "$id" =~ ^[A-Za-z0-9_-]+$ ]] || exit 0
name=$(T display-message -p -t "$win" '#{window_name}' 2>/dev/null) || exit 0
d="$HOME/.cache/cc-panel"
clean() { jq -rn --arg s "$1" '$s | gsub("[\u0000-\u001f\u007f#;\"'"'"'\\\\$`{}]"; "") | gsub("^\\s+|\\s+$"; "") | .[0:20] | gsub("\\s+$"; "")' 2>/dev/null; }
# the tab name the status line would set from the current session name (first name of the tab when the session has none)
sess=$(clean "$(jq -r '(.session_name // "") | tostring' "$d/$id.json" 2>/dev/null)")
[ -n "$sess" ] || sess=$(T show-options -wqv -t "$win" @ccx_name0 2>/dev/null)
[ "$name" != "$sess" ] || exit 0
new=$(clean "$name")
# nothing left after cleaning (blank or only special characters): put the previous name back, no session rename
if [ -z "$new" ]; then [ -n "$sess" ] && T rename-window -t "$win" -- "$sess" 2>/dev/null; exit 0; fi
pane=$(T list-panes -t "$win" -F '#{@ccx_role} #{pane_id}' 2>/dev/null | awk '$1 == "claude" { print $2; exit }')
[ -n "$pane" ] || exit 0
# only when a claude process of this tab is running (CC_PANEL_ID in its environment)
found=0
for pid in $(pgrep -u "$(id -u)" -x claude 2>/dev/null); do
  if tr '\0' '\n' < "/proc/$pid/environ" 2>/dev/null | /usr/bin/grep -qx "CC_PANEL_ID=$id"; then found=1; break; fi
done
[ "$found" = 1 ] || exit 0
T send-keys -t "$pane" -l -- "/rename $new" 2>/dev/null && sleep 0.3 && T send-keys -t "$pane" Enter 2>/dev/null
exit 0
