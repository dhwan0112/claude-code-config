#!/usr/bin/env bash
# Inside xvfb.sh: theme change while running — open tabs (palette reload, focus reload) and reattached tabs.
. "$(dirname "$0")/lib.sh"
W=$HAETTY_RUN/themerun; haetty_guard $W; mkdir -p $W/rt $W/rt2 $W/cfg $W/cfg2; chmod 700 $W/rt $W/rt2
export HAETTY_INSTANCE=dev HAETTY_SOCKET=$W/rt/d.sock HAETTY_STATE_DIR=$W/state HAETTY_CONFIG_DIR=$W/cfg HAETTY_LOG=info
export HAETTY_DUMP_FRAME=$W/f.ppm
conf() { printf 'theme = %s\nrestore-tabs = false\n' "$1" > $W/cfg/config; touch -d "+$2 seconds" $W/cfg/config; }
P=0 F=0
check() { # label, expected bg, expected red (theme colour 1)
  sleep 1.2
  local r; r=$(python3 -I - $W/f.ppm "$2" "$3" <<'EOF'
import sys, collections
d = open(sys.argv[1], "rb").read(); p = d.split(maxsplit=4); w, h = int(p[1]), int(p[2]); px = p[4][:w*h*3]
c = collections.Counter(px[i:i+3].hex() for i in range(0, len(px), 3))
bg = c.most_common(1)[0][0]; red = c.get(sys.argv[3], 0)
print(("OK" if bg == sys.argv[2] and red > 0 else "BAD") + f" bg #{bg} (want #{sys.argv[2]}), #{sys.argv[3]} {red} px")
EOF
)
  case "$r" in OK*) P=$((P+1)); echo "PASS $1: $r";; *) F=$((F+1)); echo "FAIL $1: $r";; esac
}
SCRIPT='printf "\033[31mRED RED RED\033[0m\n"; exec sleep 600'
start_gui() { setsid $BIN --cols 40 --rows 6 "$@" > /dev/null 2>&1 < /dev/null & GP=$!; GW=$(timeout 10 xdotool search --sync --onlyvisible --pid $GP | head -1); xdotool windowfocus $GW; sleep 1.5; }
conf "haetty classic" 0
HAETTY_LOG_FILE=$W/gui1.log start_gui sh -c "$SCRIPT"
xdotool key ctrl+shift+t; sleep 1.5
check "start, haetty classic (tab 2)" 1e1e1e cc6666
# 1. Config changed, applied with the palette's "Reload config".
conf "IBM 5153 CGA" 5
xdotool key ctrl+shift+p; sleep 0.4; xdotool type "reload config"; sleep 0.3; xdotool key Return
check "palette reload -> CGA (tab 2)" 141414 d03333
xdotool key alt+1; check "palette reload -> CGA (tab 1)" 141414 d03333
# 2. Config changed, applied when the window gets focus again (another window had it).
conf "IBM 5153 CGA (Black)" 10
HAETTY_SOCKET=$W/rt2/d.sock HAETTY_CONFIG_DIR=$W/cfg2 HAETTY_STATE_DIR=$W/state2 HAETTY_DUMP_FRAME=$W/other.ppm HAETTY_LOG_FILE=$W/other.log \
  setsid $BIN --cols 20 --rows 3 --restore-tabs false sh -c 'exec sleep 600' > /dev/null 2>&1 < /dev/null & OP=$!
OW=$(timeout 10 xdotool search --sync --onlyvisible --pid $OP | head -1); xdotool windowfocus $OW; sleep 1; xdotool windowfocus $GW
check "focus reload -> CGA Black (tab 1)" 000000 c40000
xdotool key alt+2; check "focus reload -> CGA Black (tab 2)" 000000 c40000
# 3. The window dies; the config changes; a new window reattaches both sessions.
kill -9 $GP; wait $GP 2>/dev/null; sleep 0.5
conf "haetty classic" 15
HAETTY_LOG_FILE=$W/gui2.log start_gui
grep -c "reattaching session" $W/gui2.log | sed 's/^/  reattached sessions: /'
check "reattached, haetty classic (active tab)" 1e1e1e cc6666
xdotool key alt+1; check "reattached, haetty classic (tab 1)" 1e1e1e cc6666
xdotool key alt+2; check "reattached, haetty classic (tab 2)" 1e1e1e cc6666
grep -h "theme:" $W/gui1.log $W/gui2.log | sed 's/^/  log: /'
kill $GP $OP 2>/dev/null; wait $GP $OP 2>/dev/null
haetty_stop_daemon "$BIN"; HAETTY_SOCKET=$W/rt2/d.sock haetty_stop_daemon "$BIN"
echo "TOTAL themerun: $P passed, $F failed"
