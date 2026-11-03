#!/usr/bin/env bash
# cc-panel hook. Usage: hook.sh pre|post|substart|substop  (hook JSON on stdin)
# Rules: never print anything, always exit 0, do nothing unless CC_PANEL_ID is set.
exec >/dev/null 2>&1
[ -n "${CC_PANEL_ID:-}" ] || exit 0
case "$CC_PANEL_ID" in *[!A-Za-z0-9_-]*) exit 0 ;; esac

ev=${1:-}
D="$HOME/.cache/cc-panel"
id=$CC_PANEL_ID
input=$(cat) || exit 0
mkdir -p "$D" || exit 0

# One jq call: tool_use_id, file path, agent label, agent_id
IFS=$'\x1f' read -r tuid path agent aid < <(jq -r '[
    (.tool_use_id // ""),
    (.tool_input.file_path // .tool_input.notebook_path // ""),
    (.agent_type // "main"),
    (.agent_id // "")
  ] | join("\u001f")' <<<"$input" 2>/dev/null) || exit 0
key=${tuid//[^A-Za-z0-9_-]/_}

case "$ev" in
  pre)
    [ -n "$key" ] && [ -n "$path" ] || exit 0
    snap="$D/$id/snap"
    mkdir -p "$snap" || exit 0
    if [ -f "$path" ]; then
      size=$(stat -c %s -- "$path" 2>/dev/null || echo 0)
      if [ "$size" -gt 2097152 ]; then
        st=toolarge
      elif cp -p -- "$path" "$snap/$key.snap"; then
        st=copied
      else
        st=error
      fi
    else
      st=new
    fi
    printf '%s\t%s\t%s\t%s\n' "$path" "$st" "$agent" "$EPOCHREALTIME" > "$snap/$key.meta"
    # keep the 20 most recent snapshots for this panel
    ls -1t "$snap"/*.meta 2>/dev/null | tail -n +21 | while IFS= read -r m; do
      rm -f -- "$m" "${m%.meta}.snap"
    done
    ;;
  post)
    [ -n "$path" ] || exit 0
    tmp="$D/.$id.last.$$"
    printf '%s\t%s\t%s\t%s\n' "$path" "$EPOCHREALTIME" "$key" "$agent" > "$tmp" && mv -f -- "$tmp" "$D/$id.last"
    ;;
  substart|substop)
    [ -n "$aid" ] || exit 0
    f="$D/$id.agents"
    (
      flock -w 0.05 9 || exit 0
      touch "$f"
      grep -v -F -- "$aid"$'\t' "$f" > "$f.tmp" 2>/dev/null
      [ "$ev" = substart ] && printf '%s\t%s\n' "$aid" "$agent" >> "$f.tmp"
      mv -f -- "$f.tmp" "$f"
    ) 9>"$f.lock"
    ;;
esac
exit 0
