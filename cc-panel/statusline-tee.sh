#!/usr/bin/env bash
# Status line for ccx sessions: save the input JSON for the panel, then print the normal status line.
# Without CC_PANEL_ID it only runs ~/.claude/statusline.sh, unchanged.
# Tabs opened by ccx since 2026-11-03 also have CCX_TAB_RENAME=1: the tab (tmux window of $TMUX_PANE, on the
# ccx server in $TMUX only, and only if that window's @ccx_id is this CC_PANEL_ID) is renamed to the session
# name when it differs from the name last applied (kept in ~/.cache/cc-panel/$CC_PANEL_ID.tabname).
# An empty session name restores the first name (@ccx_name0). Names: at most 20 characters, characters that
# tmux could treat specially (control characters, # ; quotes \ $ ` { }) removed.
input=$(cat)
if [ -n "${CC_PANEL_ID:-}" ] && [[ "$CC_PANEL_ID" != *[!A-Za-z0-9_-]* ]]; then
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
fi
printf '%s' "$input" | "$HOME/.claude/statusline.sh"
