#!/usr/bin/env bash
# Inside xvfb.sh, after update.sh built its test binaries: the window's start time and its draw time while typing,
# with the update check off and with the check running against a server that never answers (a TCP listener that
# accepts and stays silent: the TLS handshake hangs until the 30 s timeout). The check runs on the updater's own
# thread, so both numbers should not move (stage PU, U2). usage: update-timing.sh
. "$(dirname "$0")/lib.sh"
W=$HAETTY_RUN/updtiming; haetty_guard $W; rm -rf $W; mkdir -p $W/rt $W/xrt $W/state $W/cfg; chmod 700 $W/rt $W/xrt
BIN=${CARGO_TARGET_DIR:-$REPO/target}-updtest/release/haetty
[ -x "$BIN" ] || { echo "SKIP update-timing.sh: run update.sh first (it builds $BIN)"; exit 0; }
export XDG_RUNTIME_DIR=$W/xrt HAETTY_SOCKET=$W/rt/d.sock HAETTY_STATE_DIR=$W/state HAETTY_CONFIG_DIR=$W/cfg
export HAETTY_LOG=trace HAETTY_UPDATE_TEST=1 HAETTY_UPDATE_TEST_FIRST_MS=0
export HAETTY_UPDATE_TEST_ROOT=$REPO/crates/haetty-update/tests/TEST-ONLY-ca.pem
unset HAETTY_INSTANCE
python3 -I -c "
import socket, time
s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1); s.bind(('127.0.0.1', 0)); s.listen(8)
open('$W/port', 'w').write(str(s.getsockname()[1]))
held = []
while True:
    c, _ = s.accept(); held.append(c)
" & SP=$!
for i in $(seq 50); do [ -s $W/port ] && break; sleep 0.1; done
run() { # SLUG LABEL CONFIG-LINE
  echo "$3" > $W/cfg/config
  local starts=() t0 t1
  for n in 1 2 3 4 5; do
    t0=$(date +%s%N)
    HAETTY_LOG_FILE=$W/$1-$n.log setsid $BIN --cols 80 --rows 24 --restore-tabs false sh -c 'exec cat' > /dev/null 2>&1 < /dev/null & GP=$!
    GW=$(timeout 15 xdotool search --sync --onlyvisible --pid $GP | head -1)
    t1=$(date +%s%N); starts+=($(( (t1 - t0) / 1000000 )))
    sleep 1
    for i in 1 2 3 4 5 6 7 8 9 10; do xdotool windowfocus --sync $GW 2>/dev/null; [ "$(xdotool getwindowfocus)" = "$GW" ] && break; sleep 0.3; done
    [ $n = 1 ] && xdotool type --delay 20 "the quick brown fox jumps over the lazy dog 0123456789 again and again"
    sleep 1; kill $GP; wait $GP 2>/dev/null; haetty_stop_daemon "$BIN"; sleep 0.5
  done
  local draws; draws=$(grep -oE 'frame drawn in [0-9]+ us' $W/$1-1.log | grep -oE '[0-9]+' | sort -n)
  local nd; nd=$(echo "$draws" | grep -c .)
  local med; med=$(echo "$draws" | sed -n "$(( (nd + 1) / 2 ))p")
  local p95; p95=$(echo "$draws" | sed -n "$(( (nd * 95 + 99) / 100 ))p")
  local checking; checking=$(grep -c 'update check' $W/$1-1.log)  # "asking" when it started, the result when done
  printf '%-28s start ms: %s (median %s)  draw us: median %s p95 %s (%s frames)  check lines in log: %s\n' "$2" \
    "${starts[*]}" "$(printf '%s\n' "${starts[@]}" | sort -n | sed -n 3p)" "$med" "$p95" "$nd" "$checking"
}
run off "update-check off" "update-check = false"
run hanging "check hanging (no answer)" "update-url = https://localhost:$(cat $W/port)/r"
kill $SP 2>/dev/null
