#!/usr/bin/env bash
# New ccx tab from a key or mouse binding of a ccx tmux server (tmux.conf: C-a C, C-a S, the "+" and
# "safe" buttons). Usage: newtab.sh SOCKET DIR [--safe] [--once] [--from-option]
#   SOCKET  tmux -L name of the server (#{b:socket_path})
#   DIR     folder for the new tab; "~" and "~/..." are expanded. A missing folder only shows a message.
#   --from-option  read DIR from the server option @ccx_newdir instead (set by the C-a C prompt, so a
#                  typed path never goes through a shell command line)
#   --safe  open the tab with ccx --safe (no bypass default)
#   --once  ignore repeated clicks: one launch at a time, and none within 1.5 s of the last one
set -u
sock=${1:-}; dir=${2:-}; shift 2 2>/dev/null || exit 0
[[ "$sock" =~ ^[A-Za-z0-9_-]+$ ]] || exit 0
safe=(); once=0
for a in "$@"; do
  case $a in
    --safe) safe=(--safe) ;;
    --once) once=1 ;;
    --from-option) dir=$(tmux -L "$sock" show-options -sv @ccx_newdir 2>/dev/null); dir=${dir//\\\$/\$} ;;
  esac
done
msg() { tmux -L "$sock" display-message -- "$1" 2>/dev/null; }
case $dir in "~") dir=$HOME ;; "~/"*) dir=$HOME/${dir#"~/"} ;; esac
if [ -z "$dir" ] || [ ! -d "$dir" ]; then msg "ccx: no such folder: ${dir:-(empty)}"; exit 0; fi
if [ "$once" = 1 ]; then
  lock="$HOME/.cache/cc-panel/.newtab-$sock.lock"
  mkdir -p "${lock%/*}"
  exec 9>"$lock"
  flock -n 9 || exit 0
  now=$(date +%s%N); last=$(cat "$lock.t" 2>/dev/null || echo 0)
  [[ "$last" =~ ^[0-9]+$ ]] || last=0
  (( now - last < 1500000000 )) && exit 0
  echo "$now" > "$lock.t"
fi
cd "$dir" || { msg "ccx: cannot enter $dir"; exit 0; }
ccx=$(tmux -L "$sock" show-options -sv @ccx_bin 2>/dev/null); [ -x "$ccx" ] || ccx="$HOME/.local/bin/ccx"
CCX_SAFE= CCX_SOCKET=$sock CCX_NO_ATTACH=1 CCX_SELECT=1 "$ccx" "${safe[@]}" >/dev/null 2>&1 \
  || msg "ccx: could not open a tab in $dir"
exit 0
