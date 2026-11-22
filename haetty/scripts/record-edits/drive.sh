# Sourced from bash: drive a host.sh window. usage: . drive.sh NAME   (then: typ TEXT, key KEYS, scr [ROWS], shot NAME)
. "$HAETTY_TEST_TMP/record-$1.env"
key() { xdotool key --clearmodifiers "$@"; sleep 0.4; }
typ() { xdotool type --delay 20 "$1"; sleep 0.3; key Return; }
sid() { HAETTY_LOG=warn $BIN status --json | python3 -Ic "import json,sys; print(json.load(sys.stdin)['sessions'][-1]['id'])"; }
scr() { HAETTY_LOG=warn $BIN screen "$(sid)" 2>&1 | tail -n "${1:-45}"; }
shot() { sleep 0.6; pnmtopng $W/f.ppm > "$1.png" 2>/dev/null; }
