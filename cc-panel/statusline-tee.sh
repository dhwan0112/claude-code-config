#!/usr/bin/env bash
# Status line for ccx sessions: save the input JSON for the panel, then print the normal status line.
# Without CC_PANEL_ID it only runs ~/.claude/statusline.sh, unchanged.
input=$(cat)
if [ -n "${CC_PANEL_ID:-}" ] && [[ "$CC_PANEL_ID" != *[!A-Za-z0-9_-]* ]]; then
  d="$HOME/.cache/cc-panel"
  if mkdir -p "$d" 2>/dev/null; then
    tmp=$(mktemp "$d/.$CC_PANEL_ID.json.XXXXXX" 2>/dev/null) \
      && printf '%s\n' "$input" > "$tmp" \
      && mv -f -- "$tmp" "$d/$CC_PANEL_ID.json"
  fi
fi
printf '%s' "$input" | "$HOME/.claude/statusline.sh"
