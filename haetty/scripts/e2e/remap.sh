#!/usr/bin/env bash
# Inside xvfb.sh: a window that is unmapped and mapped again is painted in full (issue #1, fixed in 70f97a4).
# X11 keeps no contents of an unmapped window; on map the server sends Expose (winit: RedrawRequested), and an
# unchanged screen must still be drawn whole. Xvfb has no window manager, so the window is unmapped and mapped
# directly (what a minimize and restore do to it). The window's own pixels are read from the X server (`import`,
# not haetty's frame dump: the dump is haetty's canvas, which was right all along) and compared with the pixels
# before. usage: remap.sh [PNG-DIR]
. "$(dirname "$0")/lib.sh"
OUT=${1:-}
W=$HAETTY_RUN/remap; haetty_guard $W; rm -rf $W; mkdir -p $W/rt $W/cfg; chmod 700 $W/rt
export HAETTY_INSTANCE=dev HAETTY_SOCKET=$W/rt/d.sock HAETTY_STATE_DIR=$W/state HAETTY_CONFIG_DIR=$W/cfg HAETTY_LOG=debug
export HAETTY_LOG_FILE=$W/gui.log
P=0 F=0; ok() { P=$((P+1)); echo "PASS $1"; }; bad() { F=$((F+1)); echo "FAIL $1: $2"; }
command -v import > /dev/null || { echo "SKIP remap.sh: no import (ImageMagick)"; exit 0; }
# An idle program with text on several rows, so a lost frame cannot pass for an empty one.
cat > $W/prog.sh <<'EOF'
printf 'remap check\r\nline two\r\n\033[7m reverse \033[0m\r\n'
exec sleep 600
EOF
setsid $BIN --cols 50 --rows 8 --restore-tabs false sh $W/prog.sh > /dev/null 2>&1 < /dev/null & GP=$!
GW=$(timeout 10 xdotool search --sync --onlyvisible --pid $GP | head -1); sleep 2
# Count of pixels that differ between two captures (same size), or "size" when the sizes differ.
differ() {
  python3 -I - "$1" "$2" <<'PY'
import sys
def ppm(f):
    d = open(f, "rb").read(); p = d.split(maxsplit=4)
    return (int(p[1]), int(p[2])), p[4]
(a, pa), (b, pb) = ppm(sys.argv[1]), ppm(sys.argv[2])
if a != b:
    print("size")
else:
    print(sum(1 for i in range(0, len(pa), 3) if pa[i:i + 3] != pb[i:i + 3]))
PY
}
cap() { import -window "$GW" -depth 8 "ppm:$1" 2> /dev/null; }
cap $W/before.ppm
# Non-background pixels in the first capture: the frame is really there before the check.
ink=$(python3 -I - $W/before.ppm <<'PY'
import sys
d = open(sys.argv[1], "rb").read(); p = d.split(maxsplit=4); px = p[4]; bg = px[0:3]
print(sum(1 for i in range(0, len(px), 3) if px[i:i + 3] != bg))
PY
)
[ "${ink:-0}" -gt 100 ] && ok "window painted before the unmap ($ink px of text)" || bad "first frame" "${ink:-none} px"
for round in 1 2; do
  xdotool windowunmap --sync $GW; sleep 0.5
  xdotool windowmap --sync $GW; sleep 1
  cap $W/after$round.ppm
  d=$(differ $W/before.ppm $W/after$round.ppm)
  [ "$d" = 0 ] && ok "map $round: the whole window painted again" || bad "map $round" "$d px differ from before"
done
if [ -n "$OUT" ]; then for f in before after1; do pnmtopng $W/$f.ppm > "$OUT/remap-$f.png" 2> /dev/null; done; fi
kill $GP; wait $GP 2>/dev/null; haetty_stop_daemon "$BIN"
echo "TOTAL remap.sh: $P passed, $F failed"
