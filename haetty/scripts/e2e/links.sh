#!/usr/bin/env bash
# Inside xvfb.sh: underline kinds and hyperlinks (OSC 8) in the GUI (DESIGN.md §2.2, E5b). A link has no mark of its
# own; under the pointer it is underlined and its target shows on the bottom row; Ctrl+click opens http, https and
# file targets only (HAETTY_LINK_OPENER stands in for the default program), a plain click opens nothing, and the
# target never reaches the log. usage: links.sh [PNG-DIR]
. "$(dirname "$0")/lib.sh"
OUT=${1:-}
W=$HAETTY_RUN/links; haetty_guard $W; rm -rf $W; mkdir -p $W/rt $W/cfg; chmod 700 $W/rt
export HAETTY_INSTANCE=dev HAETTY_SOCKET=$W/rt/d.sock HAETTY_STATE_DIR=$W/state HAETTY_CONFIG_DIR=$W/cfg HAETTY_LOG=debug
export HAETTY_DUMP_FRAME=$W/f.ppm HAETTY_LOG_FILE=$W/gui.log HAETTY_LINK_OPENER=$W/opener.sh
P=0 F=0; ok() { P=$((P+1)); echo "PASS $1"; }; bad() { F=$((F+1)); echo "FAIL $1: $2"; }
printf '#!/bin/sh\nprintf "%%s\\n" "$1" >> %s/opened\n' "$W" > $W/opener.sh; chmod +x $W/opener.sh
cat > $W/prog.sh <<'EOF'
printf '\033]8;;https://example.invalid/real\033\\click here\033]8;;\033\\ plain\r\n'
printf '\033]8;;ssh://host/x\033\\ssh link\033]8;;\033\\\r\n'
printf '\033[4mone\033[0m \033[4:2mtwo\033[0m \033[4:3mthree\033[0m \033[4:4mfour\033[0m \033[4:5mfive\033[0m \033[4:3;58;2;224;64;64mred\033[0m\r\n'
printf '\033]8;id=n;file:///tmp/notes.txt\033\\notes\033]8;;\033\\\r\n'
exec sleep 600
EOF
setsid $BIN --cols 60 --rows 10 --restore-tabs false sh $W/prog.sh > /dev/null 2>&1 < /dev/null & GP=$!
GW=$(timeout 10 xdotool search --sync --onlyvisible --pid $GP | head -1); sleep 2
for i in 1 2 3 4 5 6 7 8 9 10; do xdotool windowfocus --sync $GW 2>/dev/null; [ "$(xdotool getwindowfocus 2>/dev/null)" = "$GW" ] && break; sleep 0.3; done
eval $(xdotool getwindowgeometry --shell $GW)   # WIDTH HEIGHT
CW=$(( (WIDTH - 20) / 60 )); CH=$(( (HEIGHT - 12) / 10 )); OX=$(( (WIDTH - 60*CW) / 2 )); OY=$(( (HEIGHT - 10*CH) / 2 ))
echo "  window ${WIDTH}x${HEIGHT}, cell ${CW}x${CH}, origin $OX,$OY"
cx() { echo $(( OX + $1*CW + CW/2 )); }; cy() { echo $(( OY + $1*CH + CH/2 )); }
# Pixels of cells (col, row, ncols) that are not the background (the frame's corner pixel).
inked() {
  python3 -I - $W/f.ppm $(( OX + $1*CW )) $(( OY + $2*CH )) $(( $3*CW )) $CH <<'PY'
import sys
d = open(sys.argv[1], "rb").read(); p = d.split(maxsplit=4); w = int(p[1]); px = p[4]
x0, y0, cw, ch = map(int, sys.argv[2:]); bg = px[0:3]
print(sum(1 for y in range(y0, y0 + ch) for x in range(x0, x0 + cw) if px[(y*w + x)*3:(y*w + x)*3 + 3] != bg))
PY
}
xdotool mousemove --window $GW $(cx 50) $(cy 6); sleep 0.6
[ -n "$OUT" ] && pnmtopng $W/f.ppm > "$OUT/links-plain.png"
a0=$(inked 0 0 10); b0=$(inked 0 9 60)
[ "$b0" = 0 ] && ok "no link shown away from links (bottom row empty)" || bad "bottom row" "$b0 px"
# Hover "click here": underlined, the target on the bottom row.
xdotool mousemove --window $GW $(cx 3) $(cy 0); sleep 0.6
[ -n "$OUT" ] && pnmtopng $W/f.ppm > "$OUT/links-hover.png"
a1=$(inked 0 0 10); b1=$(inked 0 9 60)
[ "$a1" -gt "$a0" ] && ok "hover underlines the link ($a0 -> $a1 px)" || bad "hover underline" "$a0 -> $a1"
[ "$b1" -gt 0 ] && ok "hover shows the target on the bottom row ($b1 px)" || bad "hover target" "$b1 px"
# A plain click opens nothing.
xdotool click 1; sleep 0.6
[ ! -e $W/opened ] && ok "a plain click opens nothing" || bad "plain click" "$(cat $W/opened)"
# Ctrl+click opens it.
xdotool keydown ctrl click 1 keyup ctrl; sleep 0.8
[ "$(cat $W/opened 2>/dev/null)" = "https://example.invalid/real" ] && ok "Ctrl+click opens the https target" || bad "Ctrl+click" "$(cat $W/opened 2>/dev/null)"
# Ctrl+click on ssh: not opened, a hint says why.
rm -f $W/opened
xdotool mousemove --window $GW $(cx 2) $(cy 1); sleep 0.6
xdotool keydown ctrl click 1 keyup ctrl; sleep 0.8
[ -n "$OUT" ] && pnmtopng $W/f.ppm > "$OUT/links-refused.png"
[ ! -e $W/opened ] && ok "an ssh: link is not opened" || bad "ssh" "opened $(cat $W/opened)"
# file: opens (a link with an explicit id).
xdotool mousemove --window $GW $(cx 2) $(cy 3); sleep 0.6
xdotool keydown ctrl click 1 keyup ctrl; sleep 0.8
[ "$(cat $W/opened 2>/dev/null)" = "file:///tmp/notes.txt" ] && ok "Ctrl+click opens the file target" || bad "file" "$(cat $W/opened 2>/dev/null)"
[ -n "$OUT" ] && pnmtopng $W/f.ppm > "$OUT/links-underlines.png"
# The log has the schemes, never a target.
n=$(grep -c "link: opened" $W/gui.log); leak=$(grep -c "example.invalid\|notes.txt\|host/x" $W/gui.log)
[ "$n" = 2 ] && ok "log: two links opened" || bad "log opened" "$n"
[ "$leak" = 0 ] && ok "log: no target" || bad "log leak" "$leak line(s)"
kill $GP; wait $GP 2>/dev/null; haetty_stop_daemon "$BIN"
echo "TOTAL links.sh: $P passed, $F failed"
