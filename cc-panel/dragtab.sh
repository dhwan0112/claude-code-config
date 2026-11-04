#!/usr/bin/env bash
# Mouse drag on the top bar: move the active tab to the tab under the pointer (they swap places; the dragged tab
# stays active and follows the pointer). Called from the MouseDrag1Status binding in tmux.conf, once per tab the
# pointer crosses. Usage: dragtab.sh SOCKET SESSION_ID ACTIVE_INDEX INDEX_UNDER_POINTER   (#{b:socket_path}, #{session_id}, …)
set -u
sock=${1:-}; sid=${2:-}; a=${3:-}; u=${4:-}
[[ "$sock" =~ ^[A-Za-z0-9_-]+$ ]] && [[ "$sid" =~ ^\$[0-9]+$ ]] && [[ "$a" =~ ^[0-9]+$ ]] && [[ "$u" =~ ^[0-9]+$ ]] || exit 0
[ "$a" != "$u" ] || exit 0
tmux -L "$sock" swap-window -s "$sid:$a" -t "$sid:$u" \; select-window -t "$sid:$u" 2>/dev/null
exit 0
