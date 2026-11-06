#!/usr/bin/env bash
# Inside xvfb.sh: does a firehose tab (`yes`) delay the daemon for everyone? `haetty status` latency, idle vs firehose.
. "$(dirname "$0")/lib.sh"
W=$HAETTY_RUN/fh; haetty_guard $W; mkdir -p $W/rt $W/xrt; chmod 700 $W/rt $W/xrt
export HAETTY_INSTANCE=dev HAETTY_SOCKET=$W/rt/d.sock HAETTY_STATE_DIR=$W/state HAETTY_CONFIG_DIR=$W/cfg XDG_RUNTIME_DIR=$W/xrt HAETTY_LOG=warn
lat() { local mx=0 s=0; for i in $(seq 1 20); do t0=$(date +%s%N); $BIN status --json > /dev/null; d=$(( ($(date +%s%N) - t0) / 1000000 )); s=$((s+d)); [ $d -gt $mx ] && mx=$d; sleep 0.1; done; echo "avg $((s/20)) ms, max $mx ms"; }
cd $W
setsid $BIN --cols 120 --rows 40 --restore-tabs false sh -c 'sleep 3; exec yes "firehose line with some text in it"' > /dev/null 2>&1 < /dev/null & GP=$!
timeout 10 xdotool search --sync --onlyvisible --pid $GP > /dev/null; sleep 1
echo "idle:     $(lat)"
sleep 3
echo "firehose: $(lat)"
kill $GP; wait $GP 2>/dev/null; haetty_stop_daemon "$BIN"
