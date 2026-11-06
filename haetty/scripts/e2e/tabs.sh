#!/usr/bin/env bash
# Inside xvfb.sh: stage S2 features end to end with a fake `claude` (logs its arguments) in a git repository.
# Test isolation: writes only in $HAETTY_RUN; Claude Code's folder is CLAUDE_CONFIG_DIR=$W/claude; HOME=$W/home.
. "$(dirname "$0")/lib.sh"
OUT=${1:-}
W=$HAETTY_RUN/tabsrun; haetty_guard $W; mkdir -p $W/rt $W/xrt $W/bin $W/home $W/claude $W/state; chmod 700 $W/rt $W/xrt
export HAETTY_INSTANCE=dev HAETTY_SOCKET=$W/rt/d.sock HAETTY_STATE_DIR=$W/state HAETTY_CONFIG_DIR=$W/cfg XDG_RUNTIME_DIR=$W/xrt
export HAETTY_LOG=debug HAETTY_LOG_FILE=$W/gui.log HAETTY_DUMP_FRAME=$W/f.ppm CLAUDE_CONFIG_DIR=$W/claude HOME=$W/home
unset $(env | grep -oE '^(CLAUDE_CODE_[A-Z_]*|CLAUDECODE|CLAUDE_[A-Z_]*SESSION[A-Z_]*)=' | tr -d =) 2>/dev/null
cat > $W/bin/claude <<EOF
#!/bin/sh
echo "\$\$ \$*" >> $W/claude-starts.log
printf '\033[?25l'
exec sleep 600
EOF
chmod +x $W/bin/claude
P=0 F=0; ok() { P=$((P+1)); echo "PASS $1"; }; bad() { F=$((F+1)); echo "FAIL $1: $2"; }
# Running sessions "id:attached"; the tab set "last" as "name|custom;…"; the session id of tab-set line N.
ids() { HAETTY_LOG=warn $BIN status --json | python3 -Ic 'import json,sys; print(" ".join(s["id"]+":"+str(s["attached"]) for s in json.load(sys.stdin)["sessions"] if s["state"]=="running"))'; }
last() { grep -v '^#' $W/state/tabs/last.tsv 2>/dev/null | cut -f1,7 | tr '\t' '|' | paste -sd';'; }
nlast() { grep -vc '^#' $W/state/tabs/last.tsv; }
sl() { echo "$2" | HAETTY_SESSION_ID=$1 $BIN statusline > /dev/null; }
hk() { echo "$3" | HAETTY_SESSION_ID=$1 $BIN hook $2 > /dev/null; }
key() { xdotool key --clearmodifiers "$@"; sleep 0.6; }
pal() { key ctrl+shift+p; xdotool type --delay 30 "$1"; sleep 0.3; key Return; }
shot() { [ -n "$OUT" ] && sleep 0.5 && pnmtopng $W/f.ppm > "$OUT/$1.png"; }

R=$W/proj; mkdir -p $R; G="git -C $R -c user.name=t -c user.email=t@example.invalid -c commit.gpgsign=false"
export GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null
$G init -q -b main; printf 'one\ntwo\nthree\n' > $R/a.txt; $G add .; $G commit -qm one; $G checkout -qb feat/x

cd $R
setsid $BIN --cols 100 --rows 24 --restore-tabs false $W/bin/claude > /dev/null 2>&1 < /dev/null & GP=$!
GW=$(timeout 10 xdotool search --sync --onlyvisible --pid $GP | head -1); sleep 2; xdotool windowfocus --sync $GW; sleep 0.3
A=$(ids | cut -d: -f1)
# 1. Branch names.
[ "$(last)" = "proj:feat/x|0" ] && ok "Claude Code tab in a repository: named proj:feat/x" || bad "branch name" "$(last)"
$G checkout -qb other; sleep 3.5; sl $A '{"session_id":"aaaaaaaa-0000-4000-8000-000000000001"}'; sleep 0.5
key ctrl+shift+t; B=$(ids | tr ' ' '\n' | grep -v "^$A:" | cut -d: -f1)
[ "$(last)" = "proj:other|0;proj:other|0" ] && ok "branch change followed (3 s): proj:other" || bad "branch follow" "$(last)"
sl $A '{"session_id":"aaaaaaaa-0000-4000-8000-000000000001","session_name":"fix parser"}'; sleep 0.5
key alt+1
[ "$(last)" = "fix parser|0;proj:other|0" ] && ok "Claude Code's session name wins over repo:branch" || bad "session name" "$(last)"
shot s2-1-names
# 2. Rename (shortcut and palette); the name is not logged.
key ctrl+shift+r; xdotool type --delay 30 "my tab 한"; sleep 0.3; shot s2-2-rename-prompt; key Return
[ "$(last)" = "my tab 한|1;proj:other|0" ] && ok "Ctrl+Shift+R renames: saved with custom=1" || bad "rename" "$(last)"
grep -q "my tab" $W/gui.log && bad "log" "the typed name is in the log" || ok "the typed name is not in the debug log"
grep -q "tab 1 renamed (own name)" $W/gui.log && ok "rename logged without the text" || bad "rename log" "$(grep renamed $W/gui.log)"
sl $A '{"session_id":"aaaaaaaa-0000-4000-8000-000000000001","session_name":"other name"}'; sleep 0.5; key alt+2; key alt+1
[ "$(last)" = "my tab 한|1;proj:other|0" ] && ok "own name stays over Claude Code's session name" || bad "custom precedence" "$(last)"
pal "rename"; for i in $(seq 1 12); do xdotool key BackSpace; done; sleep 0.3; key Return
[ "$(last)" = "other name|0;proj:other|0" ] && ok "empty name (palette): the automatic name again" || bad "auto name" "$(last)"
# 3. Diff views on tab 1: an Edit hook pair on a.txt, an untracked file made by "Bash", a credential-like file.
hk $A PreToolUse "{\"session_id\":\"x\",\"tool_name\":\"Edit\",\"tool_input\":{\"file_path\":\"$R/a.txt\",\"old_string\":\"two\",\"new_string\":\"TWO\"}}"
sed -i 's/two/TWO/' $R/a.txt
hk $A PostToolUse "{\"session_id\":\"x\",\"tool_name\":\"Edit\",\"tool_input\":{\"file_path\":\"$R/a.txt\",\"old_string\":\"two\",\"new_string\":\"TWO\"}}"
printf 'made by bash\n' > $R/bash-made.txt; printf 'K=v\n' > $R/.env
sleep 0.5; key ctrl+shift+d; shot s2-3-edit
key m; shot s2-3-head
grep -q "diff view vs HEAD: .* to collect" $W/gui.log && ok "m: vs HEAD ($(grep -o 'diff view vs HEAD.*' $W/gui.log | tail -1))" || bad "head view" "$(grep 'diff' $W/gui.log | tail -3)"
key m; shot s2-3-tree
grep -q "diff view working tree: .* to collect" $W/gui.log && ok "m: working tree ($(grep -o 'diff view working tree.*' $W/gui.log | tail -1))" || bad "tree view" "$(grep diff $W/gui.log | tail -3)"
key r; [ "$(grep -c 'diff view working tree' $W/gui.log)" = 2 ] && ok "r: read again" || bad "refresh" "$(grep -c 'diff view working tree' $W/gui.log)"
key m; shot s2-3-edit-again; key Escape
kill -0 $GP && ok "m back to the last edit; Esc closes" || bad "diff cycle" "window gone"
# 4. Closed tabs: close tab 2 (a conversation with a transcript), reopen it from the palette with --resume.
SB=bbbbbbbb-0000-4000-8000-000000000002
key alt+2; sl $B "{\"session_id\":\"$SB\"}"; sleep 0.5
PK=$W/claude/projects/$(printf %s "$R" | sed 's/[^A-Za-z0-9]/-/g'); mkdir -p $PK; echo '{}' > $PK/$SB.jsonl
key ctrl+shift+w; key ctrl+shift+w; sleep 0.5
c=$(grep -v '^#' $W/state/tabs/closed.tsv | cut -f2,3,5 | tr '\t' '|')
[ "$c" = "$R|$W/bin/claude|$SB" ] && ok "closed tab kept: folder, command, conversation" || bad "closed list" "$c"
pal "reopen"; shot s2-4-closed-list; key Return; sleep 1.5
n=$(ids | wc -w)
grep -q -- "--resume $SB" $W/claude-starts.log && [ "$n" = 2 ] && ok "reopened with --resume $SB" || bad "reopen" "$(ids) / $(tail -1 $W/claude-starts.log)"
[ -z "$(grep -v '^#' $W/state/tabs/closed.tsv)" ] && ok "the reopened tab left the closed list" || bad "closed after reopen" "$(cat $W/state/tabs/closed.tsv)"
# Before the new session reports its id: a second closed entry for the conversation must not resume it again.
printf '# t\nold\t%s\t%s\t\t%s\t0\t0\n' "$R" "$W/bin/claude" "$SB" > $W/state/tabs/closed.tsv
pal "reopen"; key Return; sleep 1
[ "$(ids | wc -w)" = 2 ] && [ "$(grep -c -- "--resume $SB" $W/claude-starts.log)" = 1 ] && ok "the running conversation is not resumed twice" || bad "duplicate resume" "$(ids)"
grep -q "already open; skipped" $W/gui.log && ok "logged: already open; skipped" || bad "dup log" "-"
# 5. Detach tab 1 and attach it again.
key alt+1; pal "detach"; sleep 0.5
[ "$(ids | wc -w)" = 2 ] && [ "$(ids | tr ' ' '\n' | grep -c ':0$')" = 1 ] && [ "$(nlast)" = 1 ] && ok "detach: 1 tab in the window, its session runs unattached" || bad "detach" "$(ids) / $(last)"
pal "detach"; sleep 0.5
[ "$(nlast)" = 1 ] && kill -0 $GP && ok "the only tab is not detached" || bad "detach only tab" "$(last)"
pal "attach detached"; sleep 1.5
[ "$(nlast)" = 2 ] && [ "$(ids | tr ' ' '\n' | grep -c ':0$')" = 0 ] && ok "Attach detached sessions brings it back" || bad "attach" "$(ids)"
echo "  tabs after attach: $(last)"; shot s2-5-attached
kill $GP 2>/dev/null; wait $GP 2>/dev/null
haetty_stop_daemon "$BIN"
echo "TOTAL tabsrun: $P passed, $F failed"
