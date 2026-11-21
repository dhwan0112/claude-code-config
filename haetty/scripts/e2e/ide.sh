#!/usr/bin/env bash
# Inside xvfb.sh: haetty as Claude Code's editor (stage P3c; DESIGN.md §13.9). ide-integration = true; `claude` typed in
# the shell tab goes through the proxy to a fake claude (ide-client.py) that speaks Claude Code's side of the editor
# connection. Each proposal opens the diff overlay by itself over the terminal (the grid never changes): `a` answers
# FILE_SAVED, `d` DIFF_REJECTED, Esc closes it without an answer and it does not open again; Claude Code's close_tab
# then gives TAB_CLOSED. The lock file goes with the tab. Frame dump docs/look/p3c-proposal.png when a PNG folder is
# given. Writes only in $HAETTY_RUN.
# usage: ide.sh [PNG-DIR]
. "$(dirname "$0")/lib.sh"
OUT=${1:-}
W=$HAETTY_RUN/iderun; haetty_guard $W; rm -rf $W; mkdir -p $W/rt $W/xrt $W/bin $W/home $W/claude $W/state $W/cfg $W/work
chmod 700 $W/rt $W/xrt
export HAETTY_INSTANCE=dev HAETTY_SOCKET=$W/rt/d.sock HAETTY_STATE_DIR=$W/state HAETTY_CONFIG_DIR=$W/cfg XDG_RUNTIME_DIR=$W/xrt
export HAETTY_LOG=debug HAETTY_LOG_FILE=$W/gui.log HAETTY_DUMP_FRAME=$W/f.ppm CLAUDE_CONFIG_DIR=$W/claude HOME=$W/home
export PATH=$W/bin:$PATH SHELL=/bin/sh ENV= PS1='$ '
unset $(env | grep -oE '^(CLAUDE_CODE_[A-Z_]*|CLAUDECODE|CLAUDE_[A-Z_]*SESSION[A-Z_]*)=' | tr -d =) 2>/dev/null
unset HAETTY_IDE_PORT HAETTY_PROXIED HAETTY_PROXY_DIR HAETTY_CLAUDE_SETTINGS HAETTY_SESSION_ID
P=0 F=0; ok() { P=$((P+1)); echo "PASS $1"; }; bad() { F=$((F+1)); echo "FAIL $1: $2"; }
printf 'ide-integration = true\nrestore-tabs = false\n' > $W/cfg/config
printf 'one\nfixed\n' > $W/work/a.txt
cat > $W/bin/claude <<EOS
#!/bin/sh
echo "fake claude: port \$CLAUDE_CODE_SSE_PORT"
exec python3 -I $E2E/ide-client.py $W $W/work/a.txt
EOS
chmod +x $W/bin/claude
J() { HAETTY_LOG=warn $BIN status --json; }
sess() { J | python3 -Ic "import json,sys; s=json.load(sys.stdin)['sessions']; print(s[-1]['$1'] if s else '')"; }
key() { xdotool key --clearmodifiers "$@"; sleep 0.5; }
typ() { xdotool type --delay 20 "$1"; sleep 0.2; key Return; }
shot() { [ -n "$OUT" ] && sleep 0.8 && pnmtopng $W/f.ppm > "$OUT/p3c-$1.png" 2> /dev/null; }
frame() { sleep 0.8; cp $W/f.ppm $W/$1.ppm; }
ng() { python3 -I "$E2E/nongrey.py" $W/f.ppm; }
same() { cmp -s $W/$1.ppm $W/$2.ppm; }
grid() { echo "$(sess cols)x$(sess rows)"; }
answer() { for i in $(seq 1 60); do [ -e $W/answer$1 ] && { cat $W/answer$1; return; }; sleep 0.1; done; echo none; }
start() {
  setsid $BIN "$@" > /dev/null 2>&1 < /dev/null & GP=$!
  GW=$(timeout 10 xdotool search --sync --onlyvisible --pid $GP | head -1); sleep 2
  for i in 1 2 3 4 5 6 7 8 9 10; do xdotool windowfocus --sync $GW 2>/dev/null; [ "$(xdotool getwindowfocus)" = "$GW" ] && break; sleep 0.3; done
}
stop() { kill $GP 2>/dev/null; sleep 0.5; haetty_stop_daemon "$BIN"; sleep 0.6; }
cd $W/work

start --cols 110 --rows 30 --restore-tabs false
typ "claude"; sleep 2
grep -q "initialize: 2024-11-05" $W/client.log 2>/dev/null && ok "claude through the proxy connected with the lock's token" || bad "connect" "$(cat $W/client.log 2>&1)"
port=$(ls $W/claude/ide 2>/dev/null | sed -n 's/\.lock$//p')
[ -n "$port" ] && [ "$(stat -c %a $W/claude/ide/$port.lock)" = 600 ] && ok "lock file $port.lock (0600)" || bad "lock" "$(ls -l $W/claude/ide 2>&1)"
g0=$(grid); frame before
touch $W/step1; sleep 1.5; frame proposal
n=$(ng); [ "$n" -gt 0 ] && ok "a proposal opened the overlay by itself ($n diff px)" || bad "open" "$n"
[ "$(grid)" = "$g0" ] && ok "the grid unchanged ($g0)" || bad "grid" "$(grid)"
shot proposal
[ "$(answer 1)" = none ] || bad "no answer before a key" "$(cat $W/answer1)"
key a
[ "$(answer 1)" = "11 FILE_SAVED" ] && ok "a: FILE_SAVED" || bad "a" "$(answer 1)"
frame after1; same before after1 && ok "the overlay closed: the frame before it" || bad "closed after a" "frames differ"
[ "$(cat $W/work/a.txt)" = "$(printf 'one\nfixed')" ] && ok "haetty did not write the file" || bad "file" "$(cat $W/work/a.txt)"
touch $W/step2; sleep 1.5
[ "$(ng)" -gt 0 ] && ok "the next proposal opened" || bad "open 2" "$(ng)"
key d
[ "$(answer 2)" = "12 DIFF_REJECTED" ] && ok "d: DIFF_REJECTED" || bad "d" "$(answer 2)"
touch $W/step3; sleep 1.5
[ "$(ng)" -gt 0 ] && ok "the third proposal opened" || bad "open 3" "$(ng)"
key Escape; sleep 1.5; frame esc
same before esc && ok "Esc: closed, and it does not open again" || bad "esc" "frames differ"
[ "$(answer 3)" = none ] && ok "Esc: no answer" || bad "esc answer" "$(cat $W/answer3)"
touch $W/close
[ "$(answer 3)" = "99 TAB_CLOSED" ] && ok "Claude Code's close_tab: TAB_CLOSED" || bad "close_tab" "$(answer 3)"
kill "$(cat $W/client.pid)"; sleep 0.5; typ "exit"; sleep 1.5
[ -z "$(ls $W/claude/ide 2>/dev/null)" ] && ok "the tab closed: no lock file" || bad "lock gone" "$(ls $W/claude/ide)"
stop
grep -q "two 1\|two 2\|fixed" $W/gui.log && bad "logs" "file text in the GUI log" || ok "no file text in the GUI log"
echo "TOTAL ide.sh: $P passed, $F failed"
[ $F = 0 ]
