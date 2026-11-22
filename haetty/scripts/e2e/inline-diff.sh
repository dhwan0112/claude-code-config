#!/usr/bin/env bash
# Inside xvfb.sh: Claude Code's edit blocks redrawn side by side (stage PD; DESIGN.md §2.5). A fake claude
# (inline-claude.py) prints blocks in Claude Code 2.1.330's shape and reports the edits through `haetty hook`. With a
# docked sidebar the block is redrawn while the program area is at least inline-diff-min-cols; `haetty screen` still
# reads Claude Code's own rows; a block whose hook comes later is redrawn when it comes; the mark turns a block back; a
# drag over the left side copies the line before; unified (the palette) redraws nothing; a narrow window neither. The
# program gets 0 bytes and the redraw resizes nothing. Frame dumps (docs/look/pd-real-*) when a PNG folder is given.
# Writes only in $HAETTY_RUN. usage: inline-diff.sh [PNG-DIR]
. "$(dirname "$0")/lib.sh"
OUT=${1:-}
W=$HAETTY_RUN/inlinerun; haetty_guard $W; rm -rf $W; mkdir -p $W/rt $W/xrt $W/bin $W/home $W/claude $W/state $W/cfg $W/work
chmod 700 $W/rt $W/xrt
export HAETTY_INSTANCE=dev HAETTY_SOCKET=$W/rt/d.sock HAETTY_STATE_DIR=$W/state HAETTY_CONFIG_DIR=$W/cfg XDG_RUNTIME_DIR=$W/xrt
export HAETTY_LOG=debug HAETTY_LOG_FILE=$W/gui.log HAETTY_DUMP_FRAME=$W/f.ppm CLAUDE_CONFIG_DIR=$W/claude HOME=$W/home
export PATH=$W/bin:$PATH SHELL=/bin/sh ENV= PS1='$ '
P=0 F=0; ok() { P=$((P+1)); echo "PASS $1"; }; bad() { F=$((F+1)); echo "FAIL $1: $2"; }
printf 'restore-tabs = false\nobserve-new-tabs = true\n' > $W/cfg/config
printf '"""calc"""\n\nimport math\n\nSCALE = 10\n\n\ndef add(a, b):\n    # add two numbers\n    return a + b\n' > $W/work/a.py
cat > $W/bin/claude <<EOS
#!/bin/sh
exec python3 -I $E2E/inline-claude.py $BIN $W $W/work/a.py
EOS
chmod +x $W/bin/claude
key() { xdotool key --clearmodifiers "$@"; sleep 0.4; }
typ() { xdotool type --delay 15 "$1"; sleep 0.2; key Return; }
shot() { [ -n "$OUT" ] && sleep 0.8 && pnmtopng $W/f.ppm > "$OUT/pd-real-$1.png" 2> /dev/null; }
clip() { xclip -o -selection clipboard 2>/dev/null; }
cells() { CW=$(( (WIDTH - 20) / $1 )); CH=$(( (HEIGHT - 12) / $2 )); OX=$(( (WIDTH - $1*CW) / 2 )); OY=$(( (HEIGHT - $2*CH) / 2 )); }
cx() { echo $(( OX + $1*CW + CW/2 )); }; cy() { echo $(( OY + $1*CH + CH/2 )); }
to() { xdotool mousemove --window $GW $(cx $1) $(cy $2); sleep 0.15; }
resizes() { grep -c "resize session" $W/gui.log; }
redrawn() { grep -o "inline diff: [0-9]* block(s) redrawn" $W/gui.log | tail -1 | grep -o "[0-9]*"; }
until_redrawn() { for i in $(seq 1 40); do [ "$(redrawn)" = "$1" ] && return 0; sleep 0.1; done; return 1; }
sid() { HAETTY_LOG=warn $BIN status --json | python3 -Ic "import json,sys; print(json.load(sys.stdin)['sessions'][-1]['id'])"; }
cols() { HAETTY_LOG=warn $BIN status --json | python3 -Ic "import json,sys; print(json.load(sys.stdin)['sessions'][-1]['cols'])"; }
row_of() { HAETTY_LOG=warn $BIN screen --json "$(sid)" | python3 -Ic "import json,sys; l=json.load(sys.stdin)['lines']; print(next(i for i,t in enumerate(l) if sys.argv[1] in t))" "$1"; }
grid_to() { xdotool windowsize --sync $GW $(( $1 * CW + 20 )) $HEIGHT; sleep 1.2; eval $(xdotool getwindowgeometry --shell $GW); cells $1 40; }
setsid $BIN --cols 160 --rows 40 --restore-tabs false > /dev/null 2>&1 < /dev/null & GP=$!
GW=$(timeout 10 xdotool search --sync --onlyvisible --pid $GP | head -1); sleep 2
for i in 1 2 3 4 5 6 7 8 9 10; do xdotool windowfocus --sync $GW 2>/dev/null; [ "$(xdotool getwindowfocus)" = "$GW" ] && break; sleep 0.3; done
eval $(xdotool getwindowgeometry --shell $GW); cells 160 40
cd $W/work
typ "claude"; sleep 2
grep -q "fake claude ready" <(HAETTY_LOG=warn $BIN screen "$(sid)") && ok "the fake claude runs in a Claude Code tab" || bad "start" "$(HAETTY_LOG=warn $BIN screen "$(sid)" | tail -3)"
c0=$(cols); r0=$(resizes)
[ "$c0" -ge 110 ] && ok "a docked sidebar leaves $c0 columns of program area" || bad "area" "$c0"
touch $W/step1; sleep 1.5
until_redrawn 1 && ok "the block matching the edit is redrawn side by side (docked sidebar)" || bad "redraw" "$(redrawn)"
shot sidebar
HAETTY_LOG=warn $BIN screen "$(sid)" | grep -q -- "8 -def add(a, b):" && ok "haetty screen reads Claude Code's own rows" || bad "screen" "the original rows are not in haetty screen"
touch $W/step2; sleep 1.5
[ "$(redrawn)" = 1 ] && ok "a block whose edit is not reported yet is left as drawn" || bad "late hook first" "$(redrawn)"
touch $W/step3
until_redrawn 2 && ok "it is redrawn once its hook comes" || bad "late hook" "$(redrawn)"
shot split
[ "$(cols)" = "$c0" ] && [ "$(resizes)" = "$r0" ] && ok "the redraw resized nothing ($c0 columns, 0 resize lines)" || bad "resize" "$(cols) $(resizes) vs $r0"
y=$(row_of "-    # add two numbers")
to 12 $y; xdotool mousedown 1; sleep 0.15; to 30 $y; xdotool mouseup 1; sleep 0.5; key ctrl+shift+c; sleep 0.3
[ "$(clip)" = "    # add two numbers" ] && ok "a drag over the left side copies the line before, whole" || bad "copy" "$(clip | head -c 80)"
h=$(row_of "Added 1 line"); to 113 $h; xdotool click 1; sleep 0.8
until_redrawn 1 && ok "a click on the mark turns that block back" || bad "mark" "$(redrawn)"
shot back
key ctrl+shift+p; xdotool type --delay 20 "inline diff"; sleep 0.4; key Return; sleep 0.8
until_redrawn 0 && ok "unified (palette): nothing redrawn" || bad "unified" "$(redrawn)"
grep -q "^inline-diff = unified" $W/cfg/config && ok "the palette's choice is saved" || bad "saved" "$(cat $W/cfg/config)"
key ctrl+shift+p; xdotool type --delay 20 "inline diff"; sleep 0.4; key Return; sleep 0.8
until_redrawn 1 && ok "split again" || bad "split again" "$(redrawn)"
grid_to 150
[ "$(cols)" -lt 110 ] && until_redrawn 0 && ok "the docked sidebar leaves $(cols) columns: as drawn" || bad "sidebar narrow" "$(cols) $(redrawn)"
grid_to 120
[ "$(cols)" -ge 110 ] && until_redrawn 1 && ok "no docked sidebar at 120: $(cols) columns, redrawn" || bad "undocked" "$(cols) $(redrawn)"
grid_to 100
until_redrawn 0 && ok "a narrow window ($(cols) columns): as drawn" || bad "narrow" "$(cols) $(redrawn)"
shot narrow
kill -0 $GP 2>/dev/null && ok "the window outlived every resize" || bad "window" "the GUI process ended"
[ "$(cat $W/input)" = 0 ] && ok "the program got 0 bytes" || bad "input" "$(cat $W/input) bytes"
grep -q "add_numbers\|# add two numbers" $W/gui.log && bad "logs" "file text in the GUI log" || ok "no file text in the GUI log"
touch $W/stop; sleep 0.5
kill $GP 2>/dev/null; sleep 0.5; haetty_stop_daemon "$BIN"
echo "TOTAL inline-diff.sh: $P passed, $F failed"
[ $F = 0 ]
