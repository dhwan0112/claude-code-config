#!/usr/bin/env bash
# cc-panel hook. Usage: hook.sh pre|post|substart|substop|agentpre|agentpost  (hook JSON on stdin)
# Rules: never print anything, always exit 0, do nothing unless CC_PANEL_ID is set.
#   pre/post            Write/Edit/NotebookEdit: snapshot before the edit, record of the last edit
#   agentpre/agentpost  PreToolUse/PostToolUse of the Agent tool: orchestration records
#   substart/substop    SubagentStart/SubagentStop: orchestration records
# Orchestration records go to ~/.cache/cc-panel/<id>.orch (read by panel.py), one tab-separated line each:
#   event  time(epoch)  tool_use_id  agent_id  agent_type  description  status
# status is the STATUS: verified|blocked|needs_review line of the worker's report, or "-" when it has none.
# In interactive sessions the Agent tool returns "async_launched" at once, so the report (and its STATUS) arrives
# with SubagentStop (last_assistant_message); PostToolUse then records "-". Internal agents of Claude Code fire
# SubagentStop with an empty agent_type (recorded; panel.py leaves them out).
exec >/dev/null 2>&1
[ -n "${CC_PANEL_ID:-}" ] || exit 0
case "$CC_PANEL_ID" in *[!A-Za-z0-9_-]*) exit 0 ;; esac

ev=${1:-}
D="$HOME/.cache/cc-panel"
id=$CC_PANEL_ID
input=$(cat) || exit 0
mkdir -p "$D" || exit 0

# Append one record to <id>.orch (kept to the last 400 lines) under flock
orch() {
  local f="$D/$id.orch" line
  printf -v line '%s\t%s\t%s\t%s\t%s\t%s\t%s' "$1" "${EPOCHREALTIME%.*}" "$2" "$3" "$4" "$5" "$6"
  (
    flock -w 1 9 || exit 0
    printf '%s\n' "$line" >> "$f"
    if [ "$(wc -l < "$f")" -gt 400 ]; then tail -n 300 "$f" > "$f.tmp" && mv -f -- "$f.tmp" "$f"; fi
  ) 9>"$f.lock"
}
# STATUS: verified|blocked|needs_review on a line of the text (markdown ** and list marks allowed), else "-"
status_of() {
  local s
  s=$(printf '%s\n' "$1" | /usr/bin/grep -o -i -E '^[[:space:]#*>-]*STATUS[*[:space:]]*:[*[:space:]]*(verified|blocked|needs_review)' | tail -n 1)
  s=${s##*[:[:space:]*]}
  printf '%s' "${s:--}" | tr 'A-Z' 'a-z'
}
# Flatten for the TSV record: tabs/newlines to spaces
flat() { printf '%s' "$1" | tr '\t\n\r' '   '; }

case "$ev" in
  agentpre|agentpost|substart|substop)
    IFS=$'\x1f' read -r tuid aid type desc text < <(jq -r '[
        (.tool_use_id // ""),
        (.tool_response.agentId // .agent_id // ""),
        (.tool_input.subagent_type // .tool_response.agentType // .agent_type // ""),
        (.tool_input.description // ""),
        (if .tool_response then (.tool_response | if type == "object" then ((.content // []) | map(select(type == "object" and .type == "text") | .text) | join("\n")) else tostring end)
         else (.last_assistant_message // "") end)
      ] | map(tostring | gsub("[\u001f\t\r]"; " ") | gsub("\n"; "\u001e")) | join("\u001f")' <<<"$input") || exit 0
    text=${text//$'\x1e'/$'\n'}
    tuid=${tuid//[^A-Za-z0-9_-]/_}; aid=${aid//[^A-Za-z0-9_-]/_}
    case "$ev" in
      agentpre)  [ -n "$tuid" ] || exit 0; orch agentpre "$tuid" "" "$(flat "$type")" "$(flat "${desc:0:120}")" "" ;;
      agentpost) [ -n "$tuid" ] || exit 0; orch agentpost "$tuid" "$aid" "$(flat "$type")" "" "$(status_of "$text")" ;;
      substart)  [ -n "$aid" ] || exit 0; orch substart "" "$aid" "$(flat "$type")" "" "" ;;
      substop)   [ -n "$aid" ] || exit 0; orch substop "" "$aid" "$(flat "$type")" "" "$(status_of "$text")" ;;
    esac
    exit 0
    ;;
esac

# pre/post: one jq call: tool_use_id, file path, agent label
IFS=$'\x1f' read -r tuid path agent < <(jq -r '[
    (.tool_use_id // ""),
    (.tool_input.file_path // .tool_input.notebook_path // ""),
    (.agent_type // "main")
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
    # Edit time = when the synchronous PreToolUse hook ran for this tool call (just before the edit).
    # PostToolUse runs async, so posts can arrive out of order; keep only the newest edit.
    ts=""
    [ -n "$key" ] && IFS=$'\t' read -r _ _ _ ts < "$D/$id/snap/$key.meta" 2>/dev/null
    [[ "$ts" =~ ^[0-9]+\.[0-9]{6}$ ]] || ts=$EPOCHREALTIME
    last="$D/$id.last"
    (
      flock -w 2 9 || exit 0
      old=""
      [ -f "$last" ] && IFS=$'\t' read -r _ old _ < "$last"
      if [[ "$old" =~ ^[0-9]+\.[0-9]{6}$ ]] && [ "${ts/./}" -le "${old/./}" ]; then
        exit 0
      fi
      tmp="$D/.$id.last.$$"
      printf '%s\t%s\t%s\t%s\n' "$path" "$ts" "$key" "$agent" > "$tmp" && mv -f -- "$tmp" "$last"
    ) 9>"$last.lock"
    ;;
esac
exit 0
