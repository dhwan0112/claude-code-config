#!/usr/bin/env bash
# Save the open tabs of a ccx server as a tab set that "ccx --restore-tabs [NAME]" opens again.
# Usage: savetabs.sh SOCKET [NAME] [--quiet] [--without ID]
#   SOCKET     tmux -L name of the server (#{b:socket_path})
#   NAME       set name, letters/digits/-/_ (default: last)
#   --quiet    no message (tmux hooks)
#   --without  leave out the tab with this @ccx_id (left.sh: the tab that is closing on purpose)
# File: ${XDG_CONFIG_HOME:-~/.config}/cc-panel/tabs/SOCKET/NAME.tsv, a comment line, then one tab per line in bar order:
#   tab name <TAB> project folder <TAB> session id <TAB> permissions (bypass|safe|user) <TAB> layout (sidebar|split)
# The project folder and session id come from the tab's status JSON (~/.cache/cc-panel/<id>.json: workspace.project_dir,
# session_id). A tab whose Claude Code has not written it yet (the first seconds) is saved with its Claude pane's folder
# and no session id, and is restored as a new session. An empty set is written only with --without (the last tab was
# closed on purpose); nothing is written when the set did not change. Writes are atomic. Run by: C-a W (NAME last, with a message), ccx after it opened a tab, left.sh when
# a tab closes (--without), and the tmux hooks window-renamed / session-window-changed / client-detached (tmux.conf).
set -u
sock=${1:-}; shift 2>/dev/null || exit 0
[[ "$sock" =~ ^[A-Za-z0-9_-]+$ ]] || exit 0
name=last; quiet=0; without=""
while [ $# -gt 0 ]; do
  case $1 in
    --quiet) quiet=1 ;;
    --without) without=${2:-}; shift ;;
    *) name=$1 ;;
  esac
  shift
done
msg() {
  [ "$quiet" = 1 ] && return 0
  tmux -L "$sock" display-message -- "$1" 2>/dev/null
  [ -t 1 ] && echo "$1"
  return 0
}
[[ "$name" =~ ^[A-Za-z0-9_-]+$ ]] || { msg "ccx: tab set name: letters, digits, - and _ only"; exit 0; }
D="$HOME/.cache/cc-panel"
out="${XDG_CONFIG_HOME:-$HOME/.config}/cc-panel/tabs/$sock"
body=""; n=0
while IFS=$'\t' read -r _ wid id perm layout wname; do
  [ -n "$id" ] && [ "$id" != "$without" ] || continue
  [[ "$id" =~ ^[0-9]{14}-[0-9]+-[0-9]+$ ]] || continue
  sid=""; pdir=""
  if [ -f "$D/$id.json" ]; then
    IFS=$'\t' read -r sid pdir < <(jq -r '[(.session_id // ""), (.workspace.project_dir // .cwd // "")]
      | map(tostring | gsub("[\t\n\r]"; " ")) | join("\t")' "$D/$id.json" 2>/dev/null) || true
  fi
  [[ "$sid" =~ ^[0-9a-fA-F-]{36}$ ]] || sid=""
  if [ -z "$pdir" ]; then
    pdir=$(tmux -L "$sock" list-panes -t "$wid" -F '#{@ccx_role}	#{pane_current_path}' 2>/dev/null \
      | awk -F'\t' '$1 == "claude" { print $2; exit }')
  fi
  [ -n "$pdir" ] || continue
  case "$perm" in bypass|safe|user) ;; *) perm=bypass ;; esac
  case "$layout" in sidebar|split) ;; *) layout=sidebar ;; esac
  wname=${wname//[$'\t\n\r']/ }
  body+="$wname	$pdir	$sid	$perm	$layout"$'\n'
  n=$((n + 1))
done < <(tmux -L "$sock" list-windows -t '=cc' \
  -F '#{window_index}	#{window_id}	#{@ccx_id}	#{@ccx_perm}	#{@ccx_layout}	#{window_name}' 2>/dev/null | sort -n)
if [ "$n" -eq 0 ] && [ -z "$without" ]; then msg "ccx: no tabs to save"; exit 0; fi
f="$out/$name.tsv"
if [ -f "$f" ] && [ "$(tail -n +2 "$f" 2>/dev/null)" = "${body%$'\n'}" ]; then
  msg "ccx: tab set '$name' is up to date ($n tab(s)) · ccx --restore-tabs $name"
  exit 0
fi
mkdir -p "$out" 2>/dev/null || exit 0
tmp=$(mktemp "$out/.$name.XXXXXX" 2>/dev/null) || exit 0
if { printf '# ccx tabs "%s" saved %s on %s: %s tab(s). Columns: tab name, project folder, session id, permissions, layout. Open again: ccx --restore-tabs %s\n' \
       "$name" "$(date '+%Y-%m-%d %H:%M %z')" "$(hostname -s 2>/dev/null || echo host)" "$n" "$name"
     printf '%s' "$body"; } > "$tmp" && mv -f -- "$tmp" "$f"; then
  msg "ccx: saved $n tab(s) as '$name' → ${f/#$HOME/\~} · ccx --restore-tabs $name"
else
  rm -f -- "$tmp"
fi
exit 0
