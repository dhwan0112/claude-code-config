#!/usr/bin/env bash
# Inside xvfb.sh: the GUI when its daemon dies (stage S4). Two tabs (a shell, a fake claude with a conversation and a
# transcript); SIGKILL to the daemon by PID; then a fresh start restores the tabs. Isolation: $HAETTY_RUN only.
. "$(dirname "$0")/lib.sh"
OUT=${1:-}
W=$HAETTY_RUN/crashrun; haetty_guard $W; mkdir -p $W/rt $W/xrt $W/bin $W/home $W/claude $W/proj; chmod 700 $W/rt $W/xrt
export HAETTY_INSTANCE=dev HAETTY_SOCKET=$W/rt/d.sock HAETTY_STATE_DIR=$W/state HAETTY_CONFIG_DIR=$W/cfg XDG_RUNTIME_DIR=$W/xrt
export HAETTY_LOG=info HAETTY_LOG_FILE=$W/gui.log HAETTY_DUMP_FRAME=$W/f.ppm CLAUDE_CONFIG_DIR=$W/claude HOME=$W/home
cat > $W/bin/claude <<EOF
#!/bin/sh
echo "\$\$ \$*" >> $W/claude-starts.log
exec sleep 600
EOF
chmod +x $W/bin/claude
P=0 F=0; ok() { P=$((P+1)); echo "PASS $1"; }; bad() { F=$((F+1)); echo "FAIL $1: $2"; }
J() { HAETTY_LOG=warn $BIN status --json; }
key() { xdotool key --clearmodifiers "$@"; sleep 0.6; }
SID=cccccccc-0000-4000-8000-000000000003
cd $W/proj
setsid $BIN --cols 90 --rows 20 --restore-tabs false $W/bin/claude > /dev/null 2>&1 < /dev/null & GP=$!
GW=$(timeout 10 xdotool search --sync --onlyvisible --pid $GP | head -1); sleep 2; xdotool windowfocus --sync $GW; sleep 0.3
key ctrl+shift+t; sleep 1
A=$(J | python3 -Ic 'import json,sys; print(json.load(sys.stdin)["sessions"][0]["id"])')
echo "{\"session_id\":\"$SID\"}" | HAETTY_SESSION_ID=$A $BIN statusline
PK=$W/claude/projects/$(printf %s "$W/proj" | sed 's/[^A-Za-z0-9]/-/g'); mkdir -p $PK; echo '{}' > $PK/$SID.jsonl
key alt+1; sleep 0.5
DP=$(J | python3 -Ic 'import json,sys; print(json.load(sys.stdin)["daemon_pid"])')
PIDS=$(J | python3 -Ic 'import json,sys; print(" ".join(str(s["pid"]) for s in json.load(sys.stdin)["sessions"]))')
cp $W/state/tabs/last.tsv $W/last-before.tsv
echo "  daemon $DP, programs $PIDS; last: $(grep -vc '^#' $W/last-before.tsv) tabs"
kill -KILL $DP; sleep 2
kill -0 $GP 2>/dev/null && ok "the window stays after the daemon died" || bad "window" "gone"
grep -q "the daemon (pid $DP) is gone" $W/gui.log && ok "logged: the daemon is gone" || bad "log" "$(tail -3 $W/gui.log)"
cmp -s $W/last-before.tsv $W/state/tabs/last.tsv && ok "tab set last unchanged (2 tabs)" || bad "last" "$(cat $W/state/tabs/last.tsv)"
[ ! -s $W/state/tabs/closed.tsv ] && ok "nothing went to the closed list" || bad "closed" "$(cat $W/state/tabs/closed.tsv)"
alive=0; for p in $PIDS; do [ -d /proc/$p ] && ! grep -q ') Z' /proc/$p/stat && alive=$((alive+1)); done
[ $alive = 0 ] && ok "no orphan programs ($PIDS)" || bad "orphans" "$alive alive"
[ -n "$OUT" ] && sleep 0.5 && pnmtopng $W/f.ppm > "$OUT/s4-daemon-gone.png"
sleep 4; key alt+2; sleep 0.5
[ -n "$OUT" ] && pnmtopng $W/f.ppm > "$OUT/s4-daemon-gone-later.png"
grep -q "daemon stopped" /dev/null; ok "(frames: s4-daemon-gone*.png; the hint stays after 4 s)"
kill $GP 2>/dev/null; wait $GP 2>/dev/null
cmp -s $W/last-before.tsv $W/state/tabs/last.tsv && ok "closing the window keeps last" || bad "last after close" "$(cat $W/state/tabs/last.tsv)"
# A fresh start: a new daemon, last → previous, both tabs back, the conversation resumed.
setsid $BIN --cols 90 --rows 20 $W/bin/claude > /dev/null 2>&1 < /dev/null & GP=$!
timeout 10 xdotool search --sync --onlyvisible --pid $GP > /dev/null; sleep 2.5
n=$(J | python3 -Ic 'import json,sys; print(sum(1 for s in json.load(sys.stdin)["sessions"] if s["state"]=="running"))')
[ "$n" = 3 ] && ok "restart: the requested tab and both restored ($n sessions)" || bad "restore" "$n"
grep -q -- "--resume $SID" $W/claude-starts.log && ok "the conversation resumed (--resume $SID)" || bad "resume" "$(cat $W/claude-starts.log)"
kill $GP 2>/dev/null; wait $GP 2>/dev/null
haetty_stop_daemon "$BIN"
echo "TOTAL crashrun: $P passed, $F failed"
