#!/usr/bin/env bash
# Keep the outer tmux window named "ccx · <active ccx tab>" when ccx runs nested in another tmux server.
# Usage: outer-title.sh SOCKET   (SOCKET = tmux -L name of the ccx server, #{b:socket_path})
# Run by tmux hooks in tmux.conf (session-window-changed, window-renamed, ...) and by ccx itself.
# The target comes from the ccx server option @ccx_outer: "<outer socket path>\t<outer window id>", set
# by ccx when it ran inside another tmux. Only that window is renamed; nothing else on the outer server
# is touched. Never prints, always exits 0.
exec >/dev/null 2>&1
sock=${1:-}
[[ "$sock" =~ ^[A-Za-z0-9_-]+$ ]] || exit 0
outer=$(tmux -L "$sock" show-options -sqv @ccx_outer) || exit 0  # -q: no "invalid option" message when unset
osock=${outer%%$'\t'*}; owin=${outer#*$'\t'}
[ -S "$osock" ] && [[ "$owin" =~ ^@[0-9]+$ ]] || exit 0
tab=$(tmux -L "$sock" display-message -p -t '=cc:' '#{window_name}') || exit 0  # '=cc:' = the active window of session cc
tmux -S "$osock" rename-window -t "$owin" -- "ccx · ${tab:-cc}"
exit 0
