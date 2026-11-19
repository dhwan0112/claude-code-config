#!/usr/bin/env bash
# Inside xvfb.sh: the coordinator's commands (stage P2b). A GUI with one shell tab; `haetty task new` from the
# repository makes a task and its worker (a fake `claude` that logs its folder, arguments and environment) without a
# window change; `haetty workers` and `haetty wait worker` follow the hooks a worker sends (running, background, a
# question, a report); a sidebar click opens the detached worker; `observe-workers`; the proxy folder's `haetty`.
# Writes only in $HAETTY_RUN.
# usage: coordinator.sh [PNG-DIR]
. "$(dirname "$0")/lib.sh"
OUT=${1:-}
W=$HAETTY_RUN/coordrun; haetty_guard $W; rm -rf $W; mkdir -p $W/rt $W/xrt $W/bin $W/home $W/claude $W/state $W/cfg
chmod 700 $W/rt $W/xrt
export HAETTY_INSTANCE=dev HAETTY_SOCKET=$W/rt/d.sock HAETTY_STATE_DIR=$W/state HAETTY_CONFIG_DIR=$W/cfg XDG_RUNTIME_DIR=$W/xrt
export HAETTY_LOG=debug HAETTY_LOG_FILE=$W/gui.log HAETTY_DUMP_FRAME=$W/f.ppm CLAUDE_CONFIG_DIR=$W/claude HOME=$W/home
cat > $W/bin/claude <<EOF
#!/bin/sh
pwd >> $W/worker-cwd.log
for a in "\$@"; do printf '[%s]' "\$a"; done >> $W/worker-args.log; echo >> $W/worker-args.log
env | grep -E '^(CLAUDECODE|HAETTY_PROXY_DIR|HAETTY_CLAUDE_SETTINGS|PATH)=' >> $W/worker-env.log
printf '\033[?25l'
stty raw -echo
exec cat > /dev/null
EOF
chmod +x $W/bin/claude
printf 'worker-command = %s\nsidebar = true\n' "$W/bin/claude" > $W/cfg/config
P=0 F=0; ok() { P=$((P+1)); echo "PASS $1"; }; bad() { F=$((F+1)); echo "FAIL $1: $2"; }
J() { HAETTY_LOG=warn $BIN status --json; }
hk() { echo "$3" | HAETTY_SESSION_ID=$1 $BIN hook $2 > /dev/null; }
key() { xdotool key --clearmodifiers "$@"; sleep 0.6; }
pal() { key ctrl+shift+p; xdotool type --delay 30 "$1"; sleep 0.3; key Return; }
shot() { [ -n "$OUT" ] && sleep 0.5 && pnmtopng $W/f.ppm > "$OUT/$1.png"; }
ntabs() { grep -v '^#' $W/state/tabs/last.tsv | grep -c .; }
wk() { HAETTY_LOG=warn $BIN workers | awk -v t=$1 '$1 == t'; }

R=$W/proj; mkdir -p $R; G="git -C $R -c user.name=t -c user.email=t@example.invalid -c commit.gpgsign=false"
export GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null
$G init -q -b main; echo one > $R/a.txt; $G add .; $G commit -qm one
printf '# Fix the parser\n\nHandle CRLF line ends; cargo test passes.\n' > $W/brief.md

cd $R
out=$(HAETTY_LOG=warn $BIN task new --title "Fix the parser" 2>&1)
[ $? = 1 ] && echo "$out" | grep -q "no haetty daemon runs" && [ ! -d $W/proj-tasks ] && ok "no daemon: refused before anything is made" || bad "no daemon" "$out"
setsid $BIN --cols 150 --rows 30 --restore-tabs false sh -c 'exec sleep 600' > /dev/null 2>&1 < /dev/null & GP=$!
GW=$(timeout 10 xdotool search --sync --onlyvisible --pid $GP | head -1); sleep 2; xdotool windowfocus --sync $GW; sleep 0.3
PD=$(find $W/rt $W/xrt -type l -name claude -printf '%h\n' 2>/dev/null | head -1)
[ "$(readlink $PD/haetty)" = "$(readlink $PD/claude)" ] && [ -n "$(readlink $PD/claude)" ] && ok "proxy folder: haetty beside claude, the same executable" || bad "proxy haetty" "$(ls -l $PD 2>&1)"
shot p2b-1-before; cp $W/f.ppm $W/before.ppm
# 1. task new: the task and a detached worker, no window change.
out=$(CLAUDECODE=1 HAETTY_PROXY_DIR=$PD PATH=$PD:$PATH HAETTY_LOG=warn $BIN task new --title "Fix the parser" --brief $W/brief.md --model haiku 2>&1)
WT=$W/proj-tasks/t1-fix-the-parser
echo "$out" | grep -q "^t1 on task/t1-fix-the-parser in $WT: session .*, detached" && ok "task new: one line (id, branch, worktree, session, detached)" || bad "task new" "$out"
[ "$(git -C $WT branch --show-current)" = task/t1-fix-the-parser ] && [ -f $W/state/tasks/t1.task ] && ok "worktree beside the repository, task record" || bad "worktree" "$(ls $W/proj-tasks 2>&1)"
sleep 1
[ "$(tail -1 $W/worker-cwd.log 2>/dev/null)" = "$WT" ] && ok "worker started in the worktree" || bad "worker cwd" "$(cat $W/worker-cwd.log 2>&1)"
head -1 $W/worker-args.log | grep -qF "[--model][haiku]" && grep -qF "Handle CRLF line ends" $W/worker-args.log && grep -qF "STATUS: verified | blocked | needs_review" $W/worker-args.log && ok "worker arguments: --model, the prompt with the brief and the report request" || bad "worker args" "$(head -c 300 $W/worker-args.log)"
if ! grep -qE "^(CLAUDECODE|HAETTY_PROXY_DIR|HAETTY_CLAUDE_SETTINGS)=" $W/worker-env.log && ! grep "^PATH=" $W/worker-env.log | grep -qF "$PD"; then
  ok "worker environment: no CLAUDECODE, no proxy of the coordinator's tab"; else bad "worker env" "$(cat $W/worker-env.log)"; fi
S=$(J | python3 -Ic "import json,sys; print([s['id'] for s in json.load(sys.stdin)['sessions'] if s['cwd'].rstrip('/')=='$WT'][0])")
[ "$(J | python3 -Ic "import json,sys; print([s['attached'] for s in json.load(sys.stdin)['sessions'] if s['id']=='$S'][0])")" = 0 ] && ok "worker session detached" || bad "attached" "$(J)"
sleep 3.5; key alt+1
[ "$(ntabs)" = 1 ] && ok "the window still has one tab" || bad "tabs" "$(ntabs)"
shot p2b-2-detached-worker
# 2. haetty workers.
wk t1 | grep -qE "^t1 +no_report +$S +Fix the parser +\(detached\)$" && ok "workers: t1 no_report, session, title, detached" || bad "workers" "$(HAETTY_LOG=warn $BIN workers)"
HAETTY_LOG=warn $BIN workers --json | python3 -Ic "import json,sys; w=json.load(sys.stdin)[0]; assert (w['id'],w['state'],w['session'],w['detached'])==('t1','no_report','$S',True)" && ok "workers --json" || bad "workers json" "$(HAETTY_LOG=warn $BIN workers --json)"
# 3. wait worker: any change, then a named state.
HAETTY_LOG=warn $BIN wait --timeout 20 worker t1 > $W/w1.out & WP=$!
sleep 1.5; hk $S SessionStart '{"session_id":"cc-1"}'; hk $S UserPromptSubmit '{"session_id":"cc-1"}'
wait $WP; rc=$?
[ $rc = 0 ] && grep -qE "^t1: no_report -> running after [0-9.]+ s$" $W/w1.out && ok "wait worker t1: no_report -> running" || bad "wait change" "$rc $(cat $W/w1.out)"
HAETTY_LOG=warn $BIN wait --timeout 20 worker any --until needs_review,verified > $W/w2.out & WP=$!
sleep 1.5; hk $S Stop '{"session_id":"cc-1","last_assistant_message":"Gate runs.","background_tasks":[{"id":"b1","type":"shell","status":"running","description":"gate"}]}'
sleep 1.5; wk t1 | grep -qE "^t1 +background " && ok "turn ended with a background shell: background" || bad "background" "$(wk t1)"
kill -0 $WP 2>/dev/null && ok "wait --until is not over at a state it does not name" || bad "until early" "$(cat $W/w2.out)"
hk $S UserPromptSubmit '{"session_id":"cc-1"}'; hk $S Stop '{"session_id":"cc-1","last_assistant_message":"STATUS: needs_review\nEVIDENCE: cargo test"}'
wait $WP; rc=$?
[ $rc = 0 ] && grep -qE "^t1 is needs_review after" $W/w2.out && ok "wait worker any --until: t1 is needs_review" || bad "wait until" "$rc $(cat $W/w2.out)"
HAETTY_LOG=warn $BIN wait --timeout 2 worker t1 > $W/w3.out 2>&1; rc=$?
[ $rc = 124 ] && grep -q "timed out after 2 s: worker t1 changes state" $W/w3.out && ok "no change: exit 124" || bad "timeout" "$rc $(cat $W/w3.out)"
HAETTY_LOG=warn $BIN wait --timeout 2 worker t9 > $W/w4.out 2>&1; rc=$?
[ $rc = 1 ] && grep -q "no task t9" $W/w4.out && ok "an unknown task: exit 1" || bad "unknown" "$rc $(cat $W/w4.out)"
hk $S UserPromptSubmit '{"session_id":"cc-1"}'; hk $S PermissionRequest '{"session_id":"cc-1","tool_name":"Bash"}'
wk t1 | grep -qE "^t1 +asks " && ok "a question: asks" || bad "asks" "$(wk t1)"
for t in "Fix the parser" "Handle CRLF"; do grep -q "$t" $W/gui.log $W/state/daemon.log 2>/dev/null && bad "log" "typed text in a log: $t"; done
ok "no title or brief in the logs"
# 4. The sidebar's Workers: the detached worker's line opens it.
sleep 3.5; shot p2b-3-sidebar-asks
xdotool mousemove --window $GW 1100 100 click 1; sleep 1.5
[ "$(ntabs)" = 2 ] && ok "a click on the worker's line in the sidebar opens it: a second tab" || bad "attach" "$(ntabs)"
[ "$(J | python3 -Ic "import json,sys; print([s['attached'] for s in json.load(sys.stdin)['sessions'] if s['id']=='$S'][0])")" = 1 ] && ok "worker session attached" || bad "attached after" "$(J)"
grep -v '^#' $W/state/tabs/last.tsv | awk -F'\t' 'NR==2' | cut -f4 | grep -q "haetty task" && bad "saved args" "the task prompt in the tab set" || ok "tab set stores no task prompt"
shot p2b-4-worker-open
# 5. observe-workers.
printf 'worker-command = %s\nsidebar = true\nobserve-workers = true\n' "$W/bin/claude" > $W/cfg/config
out=$(HAETTY_LOG=warn $BIN task new --title "Second" --base main --json 2>&1)
echo "$out" | python3 -Ic "import json,sys; j=json.load(sys.stdin); assert j['id']=='t2' and j['observable'] is True" && ok "observe-workers: the worker starts observable" || bad "observable" "$out"
S2=$(echo "$out" | python3 -Ic "import json,sys; print(json.load(sys.stdin)['session'])")
HAETTY_LOG=warn $BIN screen $S2 > /dev/null && ok "haetty screen reads the worker" || bad "screen" "$S2"
kill $GP 2>/dev/null; wait $GP 2>/dev/null
haetty_stop_daemon "$BIN"
echo "TOTAL coordrun: $P passed, $F failed"
