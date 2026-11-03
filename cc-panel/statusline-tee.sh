#!/usr/bin/env bash
# Status line command for ccx sessions. It prints nothing: the status panel shows the data instead, so
# Claude Code draws no status line of its own in ccx tabs (an empty, trimmed output is treated as "no status
# line", Claude Code 2.1.317). Without CC_PANEL_ID (not a ccx session) it runs ~/.claude/statusline.sh,
# unchanged. The command still runs every refresh because it is the source of two things:
#   1. the input JSON is saved for the status panel (~/.cache/cc-panel/$CC_PANEL_ID.json, atomic);
#   2. the tab is renamed after the session name (tabs opened by ccx since 2026-11-03 have CCX_TAB_RENAME=1:
#      the tmux window of $TMUX_PANE, on the ccx server in $TMUX only, and only if that window's @ccx_id
#      is this CC_PANEL_ID) when it differs from the name last applied (~/.cache/cc-panel/$CC_PANEL_ID.tabname).
#      An empty session name restores the first name (@ccx_name0). Names: at most 20 characters, characters
#      that tmux could treat specially (control characters, # ; quotes \ $ ` { }) removed.
input=$(cat)
if [ -z "${CC_PANEL_ID:-}" ] || [[ "$CC_PANEL_ID" = *[!A-Za-z0-9_-]* ]]; then
  printf '%s' "$input" | "$HOME/.claude/statusline.sh"
  exit 0
fi
d="$HOME/.cache/cc-panel"
if mkdir -p "$d" 2>/dev/null; then
  tmp=$(mktemp "$d/.$CC_PANEL_ID.json.XXXXXX" 2>/dev/null) \
    && printf '%s\n' "$input" > "$tmp" \
    && mv -f -- "$tmp" "$d/$CC_PANEL_ID.json"
fi
if [ "${CCX_TAB_RENAME:-}" = 1 ] && [ -n "${TMUX:-}" ] && [[ "${TMUX_PANE:-}" =~ ^%[0-9]+$ ]]; then
  want=$(jq -r '(.session_name // "") | tostring
    | gsub("[\u0000-\u001f\u007f#;\"'"'"'\\\\$`{}]"; "") | gsub("^\\s+|\\s+$"; "") | .[0:20] | gsub("\\s+$"; "")' <<<"$input" 2>/dev/null)
  had=$(cat "$d/$CC_PANEL_ID.tabname" 2>/dev/null)
  if [ "$want" != "$had" ]; then
    sock=${TMUX%%,*}
    if [ -S "$sock" ] && [ "$(tmux -S "$sock" display-message -p -t "$TMUX_PANE" '#{@ccx_id}' 2>/dev/null)" = "$CC_PANEL_ID" ]; then
      name=$want
      [ -n "$name" ] || name=$(tmux -S "$sock" show-options -wqv -t "$TMUX_PANE" @ccx_name0 2>/dev/null)
      [ -n "$name" ] && tmux -S "$sock" rename-window -t "$TMUX_PANE" -- "$name" 2>/dev/null \
        && printf '%s' "$want" > "$d/$CC_PANEL_ID.tabname"
    fi
  fi
fi
exit 0
