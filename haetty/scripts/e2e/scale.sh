#!/usr/bin/env bash
# Inside xvfb.sh: the first window at 100, 125, 150 and 200 % (WINIT_X11_SCALE_FACTOR; A7). For each scale,
# --cols 80 --rows 24: the program in the tab sees 80x24 (stty), the daemon has the session at 80x24, and the
# window's pixel size is the one the log says was asked for that grid at that scale.
# Wayland's late scale event (a fractional scale arriving after the surface is shown, the kramers case) cannot be
# made here (no compositor with fractional scaling): HANDOFF §6.1 item 32.
. "$(dirname "$0")/lib.sh"
P=0 F=0; ok() { P=$((P+1)); echo "PASS $1"; }; bad() { F=$((F+1)); echo "FAIL $1: $2"; }
for s in 1 1.25 1.5 2; do
  W=$HAETTY_RUN/scale-$s; haetty_guard $W; mkdir -p $W/rt $W/cfg; chmod 700 $W/rt
  export HAETTY_INSTANCE=dev HAETTY_SOCKET=$W/rt/d.sock HAETTY_STATE_DIR=$W/state HAETTY_CONFIG_DIR=$W/cfg HAETTY_LOG=info
  export HAETTY_LOG_FILE=$W/gui.log
  WINIT_X11_SCALE_FACTOR=$s setsid $BIN --cols 80 --rows 24 --restore-tabs false sh -c "stty size > $W/stty; exec sleep 600" \
    > /dev/null 2>&1 < /dev/null & GP=$!
  GW=$(timeout 10 xdotool search --sync --onlyvisible --pid $GP | head -1); sleep 2
  eval $(xdotool getwindowgeometry --shell $GW)   # WIDTH HEIGHT
  asked=$(grep -m1 -oE 'window: [0-9]+x[0-9]+ px asked for a 80x24 grid' $W/gui.log | grep -oE '[0-9]+x[0-9]+' | head -1)
  [ "$(cat $W/stty 2>/dev/null)" = "24 80" ] && ok "$s: the program sees 80x24" || bad "$s: stty" "$(cat $W/stty 2>/dev/null)"
  grep -q "grid 80x24" $W/gui.log && ok "$s: the session is 80x24" || bad "$s: session grid" "$(grep -m1 -o 'grid [0-9]*x[0-9]*' $W/gui.log)"
  [ "${WIDTH}x${HEIGHT}" = "$asked" ] && ok "$s: window ${WIDTH}x${HEIGHT} px = asked for 80x24" || bad "$s: window" "${WIDTH}x${HEIGHT}, asked $asked"
  [ "$s" = 1 ] && W1=$WIDTH
  [ "$s" = 2 ] && { [ $WIDTH -gt $(( W1 * 19 / 10 )) ] && ok "2: the window is about twice as wide as at 1 ($W1 -> $WIDTH)" || bad "2: width" "$W1 -> $WIDTH"; }
  kill $GP 2> /dev/null; sleep 0.5
  dp=$(HAETTY_LOG=warn $BIN status --json 2> /dev/null | python3 -I -c 'import json,sys; print(json.load(sys.stdin)["daemon_pid"] or "")' 2> /dev/null)
  [ -n "$dp" ] && kill "$dp" 2> /dev/null
done
echo "TOTAL scalerun: $P passed, $F failed"
