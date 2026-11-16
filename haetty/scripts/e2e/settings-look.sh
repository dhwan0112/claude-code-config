#!/usr/bin/env bash
# Inside xvfb.sh: screen dumps of the settings button and panel for docs/look/ (stage PS, DESIGN.md §2.4). A fake
# Claude Code tab (no status line yet, then with one), a shell tab before and with the pointer at the corner, the
# panel in each group, a narrow window, and the three built-in themes. usage: settings-look.sh OUTDIR
. "$(dirname "$0")/lib.sh"
OUT=${1:?settings-look.sh OUTDIR}; mkdir -p "$OUT"
W=$HAETTY_RUN/settings-look; haetty_guard $W; rm -rf $W; mkdir -p $W/rt $W/fb $W/state $W/cfg; chmod 700 $W/rt
export HAETTY_INSTANCE=dev HAETTY_SOCKET=$W/rt/d.sock HAETTY_STATE_DIR=$W/state HAETTY_CONFIG_DIR=$W/cfg HAETTY_LOG=debug
export HAETTY_DUMP_FRAME=$W/f.ppm HAETTY_LOG_FILE=$W/gui.log PATH=$W/fb:$PATH
printf '#!/bin/sh\nprintf "\\033[1mClaude Code\\033[0m (fake)\\n\\n> fix the parser\\n"\nexec sleep 600\n' > $W/fb/claude
chmod +x $W/fb/claude
dump() { sleep 1.2; pnmtopng $W/f.ppm > "$OUT/ps-$1.png" 2>/dev/null && echo "  dump ps-$1.png"; }
key() { xdotool key --clearmodifiers "$@"; sleep 0.4; }
start() {
  setsid $BIN --restore-tabs false "$@" > /dev/null 2>&1 < /dev/null & GP=$!
  GW=$(timeout 10 xdotool search --sync --onlyvisible --pid $GP | head -1); sleep 2; xdotool windowfocus $GW; sleep 0.3
  read -r WW WH <<< "$(xdotool getwindowgeometry --shell $GW | awk -F= '/WIDTH|HEIGHT/{print $2}' | paste -sd' ')"
  xdotool mousemove --window $GW $((WW / 2)) $((WH / 3)); sleep 0.3
}
stop() { kill $GP 2>/dev/null; wait $GP 2>/dev/null; haetty_stop_daemon "$BIN"; sleep 0.4; }
status() {
  local cid; cid=$(HAETTY_LOG=warn $BIN status --json | python3 -Ic 'import json,sys; print([s["id"] for s in json.load(sys.stdin)["sessions"] if s["program"].endswith("claude")][0])')
  echo '{"session_id":"s1","context_window":{"used_percentage":12,"context_window_size":200000},"rate_limits":{"five_hour":{"used_percentage":20}},"cost":{"total_cost_usd":0.42}}' \
    | HAETTY_SESSION_ID=$cid $BIN statusline
}

# The Claude Code tab: the button before and with Claude Code's status line.
start --cols 80 --rows 14 claude
dump 1-claude-gear-no-status-yet
status; dump 2-claude-gear-with-status
# The panel in each group.
key ctrl+comma
for g in appearance font window terminal claude-code advanced; do dump "3-panel-$g"; key Tab; done
key Escape; stop
# A shell tab: no button, then the pointer at the corner.
start --cols 80 --rows 14 sh -c 'printf "$ ls\nCargo.toml  README.md  crates  docs\n$ "; exec sleep 600'
dump 4-shell-pointer-away
xdotool mousemove --window $GW $((WW - 3)) $((WH - 3)); dump 5-shell-pointer-at-corner
stop
# A narrow window: the groups on one line.
start --cols 46 --rows 14 claude
key ctrl+comma; dump 6-panel-narrow; key Escape; stop
# The three built-in themes, the panel open on the Window group.
for t in "IBM 5153 CGA" "IBM 5153 CGA (Black)" "haetty classic"; do
  echo "theme = $t" > $W/cfg/config
  start --cols 80 --rows 14 claude
  status; key ctrl+comma; key Tab; key Tab
  dump "7-theme-$(echo "$t" | tr -cd 'a-zA-Z0-9 ' | tr ' ' '-' | tr 'A-Z' 'a-z')"
  key Escape; stop
done
