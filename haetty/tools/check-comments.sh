#!/usr/bin/env bash
# Ordinary comments (`//`, not `///` or `//!`) on Rust lines added against BASE (default: where HEAD left
# origin/main, so an old commit checked by check-range counts only its own lines), stage PK. A
# comment that names why or what was tried belongs in the commit body, a name or a type, or a check. Allowed: SAFETY,
# a line with a link, a line naming a specification or an external program (ALLOWED below). Until 2026-12-19 it
# warns (exit 0); from then on a counted comment fails (exit 1). usage: tools/check-comments.sh [BASE]
cd "$(dirname "$0")/.." || exit 2
BASE=${1:-$(git merge-base HEAD origin/main 2>/dev/null || echo origin/main)}
ALLOWED='SAFETY|https?://|ECMA-48|xterm|VT[0-9]+|DEC|Unicode|UAX|RFC ?[0-9]+|POSIX|ConPTY|conhost|Win32|libghostty|Ghostty|kitty|WezTerm|OpenCode|Claude Code|winit|softbuffer|swash|fontdb|glibc|X11|Wayland|GNOME|tmux|zsh|bash|minisign'
git rev-parse -q --verify "$BASE" > /dev/null || { echo "check-comments: no $BASE; nothing checked"; exit 0; }
out=$(git diff -U0 --no-color "$BASE" -- '*.rs' | awk -v allowed="$ALLOWED" '
  /^\+\+\+ / { f = substr($0, 7); next }
  /^@@/ { split($3, a, ","); n = substr(a[1], 2) + 0; next }
  /^\+/ {
    line = substr($0, 2); text = line; sub(/^[ \t]+/, "", text)
    c = ""
    if (text ~ /^\/\/[^\/!]/ || text == "//") c = text
    else if (match(line, /[ \t]\/\/[ \t][^\/!]/)) {
      before = substr(line, 1, RSTART); q = gsub(/"/, "\"", before)
      if (q % 2 == 0) c = substr(line, RSTART + 1)
    }
    if (c != "" && c !~ allowed) print f ":" n ": " c
    n++
  }')
count=$(printf '%s' "$out" | grep -c . || true)
if [ "$count" = 0 ]; then echo "comments: 0 on lines added against $BASE"; exit 0; fi
echo "$out"
echo "comments: $count on lines added against $BASE. Instead: say it with a name or a type; put why it changed in the commit body (docs/history/ for the long form); make a constraint a check or a test."
[ "$(date +%Y%m%d)" -lt 20261219 ] && { echo "check-comments: a warning until 2026-12-19"; exit 0; }
exit 1
