#!/usr/bin/env bash
# The new look in the real window: a shell tab and a (fake) Claude Code tab. Usage: lookdump.sh TAG OUTDIR [x11|wayland]
# x11 (inside xvfb.sh): dumps of both tabs, the Claude Code tab before and after its first status line, then 20 tab
# switches with the sessions' sizes compared. wayland (WSLg, no input injection): start and status-line dumps only.
. "$(dirname "$0")/lib.sh"
TAG=$1 OUT=$2 MODE=${3:-x11}
W=$HAETTY_RUN/look-$TAG; haetty_guard $W; mkdir -p $W/rt $W/fb $W/state/tabs $W/cfg; chmod 700 $W/rt
export HAETTY_INSTANCE=dev HAETTY_SOCKET=$W/rt/d.sock HAETTY_STATE_DIR=$W/state HAETTY_CONFIG_DIR=$W/cfg HAETTY_LOG=debug
export HAETTY_DUMP_FRAME=$W/f.ppm HAETTY_LOG_FILE=$W/gui.log
printf '#!/bin/sh\nprintf "\\033[1mClaude Code\\033[0m (fake)\\n\\n> fix the parser\\n"\nexec sleep 600\n' > $W/fb/claude; chmod +x $W/fb/claude
printf '# haetty tabs "last"\nClaude Code\t%s\t%s\t\t\t0\n' "$W" "$W/fb/claude" > $W/state/tabs/last.tsv
n=0
dump() { sleep 1.3; n=$((n+1)); pnmtopng $W/f.ppm > "$OUT/$TAG-$n-$1.png" 2>/dev/null && echo "  dump $TAG-$n-$1.png"; }
sizes() { HAETTY_LOG=warn $BIN status --json | python3 -I "$E2E/sizes.py"; }
mkdir -p "$OUT"
setsid $BIN --cols 70 --rows 12 sh -c 'printf "$ ls\nCargo.toml  README.md  crates  docs\n$ "; exec sleep 600' > /dev/null 2>&1 < /dev/null & GP=$!
sleep 3
echo "  sessions: $(sizes)"
if [ "$MODE" = x11 ]; then
  GW=$(timeout 10 xdotool search --sync --onlyvisible --pid $GP | head -1); xdotool windowfocus $GW
  xdotool key alt+1; dump shell-tab
  xdotool key alt+2; dump claude-tab-no-status-yet
else
  dump start
fi
CID=$(HAETTY_LOG=warn $BIN status --json | python3 -c 'import json,sys; print([s["id"] for s in json.load(sys.stdin)["sessions"] if s["program"].endswith("claude")][0])')
echo '{"session_id":"s1","context_window":{"used_percentage":12,"context_window_size":200000},"rate_limits":{"five_hour":{"used_percentage":20}},"cost":{"total_cost_usd":0.42}}' \
  | HAETTY_SESSION_ID=$CID $BIN statusline
dump claude-tab-with-status
if [ "$MODE" = x11 ]; then
  before=$(sizes); r0=$(grep -c "resize" $W/gui.log)
  for i in $(seq 1 10); do xdotool key alt+1; sleep 0.15; xdotool key alt+2; sleep 0.15; done
  xdotool key alt+1; dump shell-tab-after-switching
  after=$(sizes); r1=$(grep -c "resize" $W/gui.log)
  echo "  sizes before 20 switches: $before"
  echo "  sizes after:              $after"
  echo "  resize log lines: $r0 -> $r1"
  [ -n "$before" ] && [ "$before" = "$after" ] && [ "$r0" = "$r1" ] && echo "PASS tab switches resized nothing" || echo "FAIL tab switches resized"
fi
grep -m1 "scale" $W/gui.log | sed 's/^/  log: /'
kill $GP 2>/dev/null; wait $GP 2>/dev/null
haetty_stop_daemon "$BIN"
