#!/usr/bin/env bash
# Inside xvfb.sh: stage S3 orchestrator end to end (GUI). A fake worker `claude` logs its folder and arguments and
# records every byte it reads. Test isolation: writes only in $HAETTY_RUN; HOME and CLAUDE_CONFIG_DIR inside it.
. "$(dirname "$0")/lib.sh"
OUT=${1:-}
W=$HAETTY_RUN/orchrun; haetty_guard $W; mkdir -p $W/rt $W/xrt $W/bin $W/home $W/claude $W/state $W/cfg; chmod 700 $W/rt $W/xrt
export HAETTY_INSTANCE=dev HAETTY_SOCKET=$W/rt/d.sock HAETTY_STATE_DIR=$W/state HAETTY_CONFIG_DIR=$W/cfg XDG_RUNTIME_DIR=$W/xrt
export HAETTY_LOG=debug HAETTY_LOG_FILE=$W/gui.log HAETTY_DUMP_FRAME=$W/f.ppm CLAUDE_CONFIG_DIR=$W/claude HOME=$W/home
unset $(env | grep -oE '^(CLAUDE_CODE_[A-Z_]*|CLAUDECODE|CLAUDE_[A-Z_]*SESSION[A-Z_]*)=' | tr -d =) 2>/dev/null
cat > $W/bin/claude <<EOF
#!/bin/sh
pwd >> $W/worker-cwd.log
for a in "\$@"; do printf '[%s]' "\$a"; done >> $W/worker-args.log; echo >> $W/worker-args.log
printf '\033[?25l'
stty raw -echo
exec cat >> $W/worker-in.log
EOF
chmod +x $W/bin/claude
echo "worker-command = $W/bin/claude" > $W/cfg/config
P=0 F=0; ok() { P=$((P+1)); echo "PASS $1"; }; bad() { F=$((F+1)); echo "FAIL $1: $2"; }
J() { HAETTY_LOG=warn $BIN status --json; }
wk() { J | python3 -Ic "import json,sys; w=json.load(sys.stdin)['workers']; print(w[0]['$1'] if w else '')"; }
sl() { echo "$2" | HAETTY_SESSION_ID=$1 $BIN statusline > /dev/null; }
hk() { echo "$3" | HAETTY_SESSION_ID=$1 $BIN hook $2 > /dev/null; }
key() { xdotool key --clearmodifiers "$@"; sleep 0.6; }
pal() { key ctrl+shift+p; xdotool type --delay 30 "$1"; sleep 0.3; key Return; }
typ() { xdotool type --delay 30 "$1"; sleep 0.3; }
shot() { [ -n "$OUT" ] && sleep 0.5 && pnmtopng $W/f.ppm > "$OUT/$1.png"; }
lastw() { grep -v '^#' $W/state/tabs/last.tsv | awk -F'\t' 'NR==2'; }

R=$W/proj; mkdir -p $R; G="git -C $R -c user.name=t -c user.email=t@example.invalid -c commit.gpgsign=false"
export GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null
$G init -q -b main; echo one > $R/a.txt; $G add .; $G commit -qm one

cd $R
setsid $BIN --cols 110 --rows 26 --restore-tabs false sh -c 'exec sleep 600' > /dev/null 2>&1 < /dev/null & GP=$!
GW=$(timeout 10 xdotool search --sync --onlyvisible --pid $GP | head -1); sleep 2; xdotool windowfocus --sync $GW; sleep 0.3
# 1. New task through the palette: title, description, the prefilled repository and branch.
pal "new task"; typ "Fix the parser"; key Return; typ "Handle CRLF"; key Return; shot s3-1-new-task-place; key Return; sleep 1.5
WT=$W/proj-tasks/t1-fix-the-parser
[ -d $WT ] && [ "$(git -C $WT branch --show-current)" = task/t1-fix-the-parser ] && ok "worktree beside the repository on task/t1-fix-the-parser" || bad "worktree" "$(ls $W/proj-tasks 2>&1)"
[ -f $W/state/tasks/t1.task ] && ok "task file tasks/t1.task" || bad "task file" "$(ls $W/state/tasks 2>&1)"
[ "$(tail -1 $W/worker-cwd.log)" = "$WT" ] && ok "worker started in the worktree" || bad "worker cwd" "$(cat $W/worker-cwd.log)"
extra=$(grep -oE -- '--[a-z][a-z-]+' $W/worker-args.log | sort -u | paste -sd,)
head -1 $W/worker-args.log | grep -qF "[haetty task t1: Fix the parser" &&! grep -qE -- "--dangerously|--permission-mode|--allow" $W/worker-args.log && [ "$extra" = "--settings" ] && ok "worker arguments: the task prompt, then only haettys own --settings (no permission flags)" || bad "worker args" "$extra"
grep -q "STATUS: verified | blocked | needs_review" $W/worker-args.log && ok "the prompt asks for the report" || bad "prompt" "-"
S=$(J | python3 -Ic "import json,sys; print([s['id'] for s in json.load(sys.stdin)['sessions'] if s['cwd'].rstrip('/')=='$WT'][0])")
[ "$(wk state)" = "no report" ] && [ "$(wk session)" = "$S" ] && ok "status --json: worker t1, its session, no report yet" || bad "status json" "$(J)"
# 2. States from hooks.
hk $S SessionStart '{"session_id":"cc-1"}'; hk $S UserPromptSubmit '{"session_id":"cc-1"}'; sleep 0.5
[ "$(wk state)" = running ] && ok "UserPromptSubmit → running" || bad "running" "$(wk state)"
key alt+1; key alt+2
lastw | cut -f1 | grep -qx '… Fix the parser' && ok "tab name: … Fix the parser" || bad "tab name running" "$(lastw | cut -f1)"
[ -z "$(lastw | cut -f4)" ] && ok "tab set stores no task prompt as arguments" || bad "saved args" "$(lastw | cut -f4 | head -c 80)"
# A new tab next to the worker opens in its worktree (Claude Code reports no folder; the tab's own one counts).
key alt+2; key ctrl+shift+t; sleep 1.5
ncwd=$(J | python3 -Ic "import json,sys; s=json.load(sys.stdin)['sessions']; print(max(s, key=lambda x: x['created_ms'])['cwd'].rstrip('/'))")
[ "$ncwd" = "$WT" ] && ok "a new tab next to the worker opens in its worktree" || bad "new tab folder" "$ncwd"
key ctrl+shift+w; key ctrl+shift+w; sleep 0.8; key alt+2
# 3. A question: the orchestrator holds; the person's keys are not touched.
hk $S PermissionRequest '{"session_id":"cc-1","tool_name":"Bash"}'; sleep 0.4
pal "message"; sleep 0.3
grep -q "palette" /dev/null; sz0=$(stat -c %s $W/worker-in.log 2>/dev/null || echo 0)
[ "$sz0" = 0 ] && ok "question waiting: Message the worker does not open, nothing sent" || bad "held on ask" "$sz0 bytes"
hk $S PostToolUse '{"session_id":"cc-1","tool_name":"Bash"}'
hk $S Stop '{"session_id":"cc-1","last_assistant_message":"Done.\nSTATUS: verified\nARTIFACT: src/parser.rs\nEVIDENCE: cargo test -> 12 passed\nBLOCKERS: none\nNEXT_ACTION: review task/t1-fix-the-parser"}'
sleep 0.6
[ "$(wk state)" = verified ] && [ "$(wk evidence)" = "cargo test -> 12 passed" ] && ok "Stop report → verified, evidence in status --json" || bad "verified" "$(wk state) / $(wk evidence)"
key alt+1; key alt+2; sleep 0.8; ng=$(python3 -I "$E2E/nongrey.py" $W/f.ppm)
[ "$ng" = 0 ] && ok "§2.3: idle worker tab showing ✓ (active, seen): 0 non-grey pixels" || bad "nongrey verified" "$ng"
key ctrl+shift+s; shot s3-2-worker-verified; key ctrl+shift+s
lastw | cut -f1 | grep -qx '✓ Fix the parser' && ok "tab name: ✓ Fix the parser" || bad "tab name verified" "$(lastw | cut -f1)"
# 4. Ready: the message goes as one paste and Enter.
pal "message"; typ "please add tests"; key Return; sleep 0.6
grep -q 'please add tests' $W/worker-in.log && [ "$(tail -c 1 $W/worker-in.log | od -An -c | tr -d ' ')" = '\r' ] && ok "message delivered (text + Enter)" || bad "message" "$(od -c $W/worker-in.log | head -3)"
grep -q "message to worker t1 sent" $W/gui.log && ok "logged without the text" || bad "send log" "-"
for t in "Fix the parser" "Handle CRLF" "please add tests"; do grep -q "$t" $W/gui.log && bad "log" "typed text in the log: $t"; done
ok "no task title, description or message in the debug log"
# 5. Blocked, then cleanup on command only.
hk $S UserPromptSubmit '{"session_id":"cc-1"}'; hk $S Stop '{"session_id":"cc-1","last_assistant_message":"STATUS: blocked\nBLOCKERS: no network"}'; sleep 0.5
[ "$(wk state)" = blocked ] && ok "→ blocked" || bad "blocked" "$(wk state)"
shot s3-3-worker-blocked
pal "remove a task"; key Return; sleep 0.5
[ -d $WT ] && ok "worker open: worktree not removed" || bad "remove while open" "removed"
key ctrl+shift+w; key ctrl+shift+w; sleep 0.5
echo dirty > $WT/new.txt; pal "remove a task"; key Return; sleep 0.8
[ -d $WT ] && [ -f $W/state/tasks/t1.task ] && ok "untracked file in the worktree: refused (no --force)" || bad "dirty remove" "gone"
rm $WT/new.txt; pal "remove a task"; key Return; sleep 0.8
[ ! -d $WT ] && [ ! -f $W/state/tasks/t1.task ] && git -C $R rev-parse -q --verify refs/heads/task/t1-fix-the-parser > /dev/null && ok "removed on command; branch kept" || bad "remove" "$(ls $W/proj-tasks)"
kill $GP 2>/dev/null; wait $GP 2>/dev/null
haetty_stop_daemon "$BIN"
echo "TOTAL orchrun: $P passed, $F failed"
