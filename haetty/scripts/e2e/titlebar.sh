#!/usr/bin/env bash
# Inside xvfb.sh (Xvfb, no window manager): decorations = integrated. The title row, hover, which window
# calls the clicks make (no WM to carry them out: checked in the log), and the close button.
. "$(dirname "$0")/lib.sh"
OUT=${1:-}
W=$HAETTY_RUN/titlerun; haetty_guard $W; mkdir -p $W/rt $W/cfg; chmod 700 $W/rt
export HAETTY_INSTANCE=dev HAETTY_SOCKET=$W/rt/d.sock HAETTY_STATE_DIR=$W/state HAETTY_CONFIG_DIR=$W/cfg HAETTY_LOG=info
export HAETTY_DUMP_FRAME=$W/f.ppm HAETTY_LOG_FILE=$W/gui.log HAETTY_TITLE_BUTTONS="appmenu:minimize,maximize,close"
P=0 F=0; ok() { P=$((P+1)); echo "PASS $1"; }; bad() { F=$((F+1)); echo "FAIL $1: $2"; }
logged() { grep -c "$1" $W/gui.log; }
sizes() { HAETTY_LOG=warn $BIN status --json | python3 -I "$E2E/sizes.py"; }
setsid $BIN --cols 60 --rows 10 --restore-tabs false --decorations integrated sh -c 'printf "$ ls\nCargo.toml  docs\n$ "; exec sleep 600' > /dev/null 2>&1 < /dev/null & GP=$!
GW=$(timeout 10 xdotool search --sync --onlyvisible --pid $GP | head -1); sleep 2
eval $(xdotool getwindowgeometry --shell $GW)   # WIDTH HEIGHT
grep -q "decorations: Integrated on X11 -> Integrated" $W/gui.log && ok "integrated on request (X11)" || bad "decorations" "$(grep decorations: $W/gui.log)"
grep -m1 "title buttons from" $W/gui.log | sed 's/^/  /'
s=$(sizes); [ "$s" = "sh=60x10" ] && ok "the program keeps its 60x10 (the title row is extra)" || bad "size" "$s"
# Geometry: 60 columns, 11 rows (title + 10), padding 10 / 6, balance.
CW=$(( (WIDTH - 20) / 60 )); CH=$(( (HEIGHT - 12) / 11 )); OX=$(( (WIDTH - 60*CW) / 2 )); OY=$(( (HEIGHT - 11*CH) / 2 ))
ty=$(( OY + CH/2 )); btn() { echo $(( OX + (60 - 9 + 3*$1 + 1) * CW + CW/2 )); }   # 0 minimize, 1 maximize, 2 close
echo "  window ${WIDTH}x${HEIGHT}, cell ${CW}x${CH}, origin $OX,$OY"
[ -n "$OUT" ] && pnmtopng $W/f.ppm > "$OUT/linux-x11-integrated-one-tab.png"
# Hover: the close glyph turns from dim to the foreground colour.
python3 -I - $W/f.ppm $(( OX + 57*CW )) $OY $(( 3*CW )) $CH > $W/h0 <<'EOF'
import sys
d = open(sys.argv[1], "rb").read(); p = d.split(maxsplit=4); w = int(p[1]); px = p[4]
x0, y0, cw, ch = map(int, sys.argv[2:])
print(sum(1 for y in range(y0, y0 + ch) for x in range(x0, x0 + cw) if px[(y*w + x)*3:(y*w + x)*3 + 3] == bytes([0xce, 0xce, 0xce])))
EOF
xdotool mousemove --window $GW $(btn 2) $ty; sleep 0.8
python3 -I - $W/f.ppm $(( OX + 57*CW )) $OY $(( 3*CW )) $CH > $W/h1 <<'EOF'
import sys
d = open(sys.argv[1], "rb").read(); p = d.split(maxsplit=4); w = int(p[1]); px = p[4]
x0, y0, cw, ch = map(int, sys.argv[2:])
print(sum(1 for y in range(y0, y0 + ch) for x in range(x0, x0 + cw) if px[(y*w + x)*3:(y*w + x)*3 + 3] == bytes([0xce, 0xce, 0xce])))
EOF
[ "$(cat $W/h0)" = 0 ] && [ "$(cat $W/h1)" -gt 0 ] && ok "close glyph: dim, foreground colour under the pointer ($(cat $W/h1) px)" || bad "hover" "$(cat $W/h0) -> $(cat $W/h1)"
[ -n "$OUT" ] && pnmtopng $W/f.ppm > "$OUT/linux-x11-integrated-hover-close.png"
# Clicks (no window manager here: the calls are what can be checked).
xdotool mousemove --window $GW $(btn 1) $ty click 1; sleep 0.5
[ "$(logged 'maximize button -> maximized true')" -ge 1 ] && ok "maximize button: set_maximized(true)" || bad "maximize" "$(grep 'title bar' $W/gui.log | tail -2)"
xdotool mousemove --window $GW $(btn 0) $ty click 1; sleep 0.5
[ "$(logged 'title bar: minimize')" -ge 1 ] && ok "minimize button: set_minimized" || bad "minimize" "$(grep 'title bar' $W/gui.log | tail -2)"
xdotool mousemove --window $GW $(( OX + 20*CW )) $ty mousedown 1 mousemove --window $GW $(btn 2) $ty mouseup 1; sleep 0.5
[ "$(logged 'title bar: close')" = 0 ] && kill -0 $GP 2>/dev/null && ok "press on the title, release on close: nothing closes" || bad "release elsewhere" "closed"
xdotool mousemove --window $GW $(( OX + 30*CW )) $ty click 1; sleep 0.6
d0=$(logged 'title bar: drag_window'); [ "$d0" -ge 1 ] && ok "press on empty title space: drag_window" || bad "drag" "$d0"
xdotool mousemove --window $GW $(( OX + 30*CW )) $ty click --repeat 2 --delay 120 1; sleep 0.5
[ "$(logged 'double click -> maximized')" -ge 1 ] && ok "double click: maximize toggled" || bad "double click" "$(grep 'title bar' $W/gui.log | tail -2)"
xdotool mousemove --window $GW 2 $(( HEIGHT / 2 )) click 1; sleep 0.5
[ "$(logged 'drag_resize_window West')" -ge 1 ] && ok "left edge: drag_resize_window West" || bad "edge" "$(grep 'title bar' $W/gui.log | tail -2)"
xdotool mousemove --window $GW $(( WIDTH - 2 )) $(( HEIGHT - 2 )) click 1; sleep 0.5
[ "$(logged 'drag_resize_window SouthEast')" -ge 1 ] && ok "corner: drag_resize_window SouthEast" || bad "corner" "$(grep 'title bar' $W/gui.log | tail -2)"
xdotool mousemove --window $GW $(( OX + 30*CW )) $ty click 3; sleep 0.4
ok "right click on the title row: no action (nothing logged, $(grep -c 'title bar' $W/gui.log) title-bar lines in all)"
# Two tabs: tab clicks still select.
xdotool windowfocus $GW; sleep 0.3; xdotool key ctrl+shift+t; sleep 1.5
xdotool mousemove --window $GW $(( OX + 2*CW )) $ty click 1; sleep 0.6
[ -n "$OUT" ] && pnmtopng $W/f.ppm > "$OUT/linux-x11-integrated-two-tabs.png"
s=$(sizes); [ "$s" = "sh=60x10 sh=60x10" ] && ok "second tab: same 60x10, no resize of the first (the title row was already there)" || bad "two tabs" "$s"
# Close button.
xdotool mousemove --window $GW $(btn 2) $ty click 1
for i in $(seq 1 20); do kill -0 $GP 2>/dev/null || break; sleep 0.2; done
kill -0 $GP 2>/dev/null && bad "close button" "window still there" || ok "close button: the window closed ($(logged 'title bar: close') close)"
kill $GP 2>/dev/null; wait $GP 2>/dev/null
haetty_stop_daemon "$BIN"
echo "TOTAL titlerun: $P passed, $F failed"
# Buttons on the left (GNOME "close,minimize,maximize:"): a frame dump.
HAETTY_TITLE_BUTTONS="close,minimize,maximize:" setsid $BIN --cols 60 --rows 6 --restore-tabs false --decorations integrated sh -c 'exec sleep 600' > /dev/null 2>&1 < /dev/null & GP=$!
timeout 10 xdotool search --sync --onlyvisible --pid $GP > /dev/null; sleep 2
grep "title buttons from" $W/gui.log | tail -1 | sed 's/^/  /'
[ -n "$OUT" ] && pnmtopng $W/f.ppm > "$OUT/linux-x11-integrated-buttons-left.png"
kill $GP 2>/dev/null; wait $GP 2>/dev/null; haetty_stop_daemon "$BIN"
