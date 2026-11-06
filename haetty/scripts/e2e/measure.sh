#!/usr/bin/env bash
# DESIGN.md §3.4 budget measurements (Linux; the GUI parts inside xvfb.sh). Built with one profile for all
# binaries — default `dist`, what the installers ship (libghostty-vt in Zig ReleaseSmall); `release` gives
# ReleaseFast. Prints, for that build:
#   - key → frame through the daemon (examples/latency.rs, 300 keys): median, p95, max
#   - throughput: plain lines, long wrapped lines, SGR-heavy output through a session (haetty-headless, `cat`)
#   - anonymous memory per session with the default scrollback filled (examples/mem.rs)
#   - GUI + daemon anonymous memory and idle CPU with 1 and 4 tabs (Claude Code idle when on PATH, else a shell)
#   - GUI draw time per incremental frame while typing (trace log)
#   HAETTY_TEST_TMP=/tmp/haetty-test scripts/e2e/xvfb.sh bash scripts/e2e/measure.sh [dist|release]
. "$(dirname "$0")/lib.sh"
PROF=${1:-dist}
cd "$REPO" || exit 2
. "$HOME/.cargo/env" 2>/dev/null
command -v zig > /dev/null || PATH=$HOME/.local/bin:$PATH
T=${CARGO_TARGET_DIR:-$REPO/target}
case $PROF in release) flag=--release; dir=release ;; *) flag="--profile $PROF"; dir=$PROF ;; esac
cargo build -q $flag -p haetty-gui -p haetty-headless 2>&1 | tail -3
cargo build -q $flag -p haetty-daemon --example latency 2>&1 | tail -3
cargo build -q $flag -p haetty-core --example mem 2>&1 | tail -3
BIN=$T/$dir/haetty HL=$T/$dir/haetty-headless
echo "== build: $($BIN --version), profile $PROF"
W=$HAETTY_RUN/measure; haetty_guard $W; mkdir -p $W/rt $W/data; chmod 700 $W/rt
export HAETTY_INSTANCE=dev HAETTY_SOCKET=$W/rt/d.sock HAETTY_STATE_DIR=$W/state HAETTY_CONFIG_DIR=$W/cfg

echo "== key -> frame through the daemon (120x36, 300 keys)"
$T/$dir/examples/latency 300 2>&1 | tail -2

echo "== throughput through a session (80x24, haetty-headless, cat)"
python3 -Ic "import sys; sys.stdout.buffer.write(b''.join(b'line %d of plain text output\n' % i for i in range(400000)))" > $W/data/lines
python3 -Ic "import sys; sys.stdout.buffer.write((b'x' * 1000 + b'\n') * 12000)" > $W/data/wrapped
python3 -Ic "import sys; sys.stdout.buffer.write(b''.join(b'\x1b[38;2;%d;%d;%dm%d\x1b[0m ' % (i % 256, i * 7 % 256, i * 13 % 256, i) for i in range(1000000)))" > $W/data/sgr
for f in lines wrapped sgr; do
  sz=$(stat -c %s $W/data/$f)
  t0=$(date +%s%N)
  $HL --cols 80 --rows 24 --timeout 300 -e "wait-exit 290000" --dump none -- cat $W/data/$f > /dev/null 2>&1
  t=$(( ($(date +%s%N) - t0) / 1000000 ))
  echo "  $f: $((sz / 1048576)) MiB in $t ms = $(( sz * 1000 / (t > 0 ? t : 1) / 1048576 )) MiB/s"
done

echo "== anonymous memory per session, default scrollback filled (examples/mem.rs, 4 sessions x 12 000 lines)"
$T/$dir/examples/mem 4 12000000 12000 2>&1 | tail -6

anon() { awk '/^RssAnon/{print $2}' /proc/$1/status; }
ticks() { awk '{print $14+$15}' /proc/$1/stat; }
dpid() { HAETTY_LOG=warn $BIN status --json | python3 -Ic 'import json,sys; print(json.load(sys.stdin)["daemon_pid"])'; }
CL=$(command -v claude || true)
# One window: idle 30 s after start, then CPU ticks over 30 s and anonymous memory of the GUI and the daemon.
#   claude N — N Claude Code tabs, idle at the prompt (isolated home, dummy key; skipped without `claude`)
#   filled N — N shell tabs, each with the default scrollback filled (12 000 lines of 119 characters)
gui_case() {
  local kind=$1 tabs=$2 prog
  [ "$kind" = claude ] && [ -z "$CL" ] && { echo "== $kind $tabs: SKIP (no claude on PATH)"; return; }
  echo "== GUI + daemon, $tabs tab(s), 120x36, $( [ "$kind" = claude ] && echo 'Claude Code idle, little scrollback' || echo 'scrollback filled, idle' )"
  (
    if [ "$kind" = claude ]; then . "$E2E/claude-home.sh" && cd "$CCPROJ"; prog=$CL; set --; args=
    else cd "$W"; prog=sh; script='i=0; while [ $i -lt 12000 ]; do printf "%0119d\n" $i; i=$((i+1)); done; exec sleep 600'
      set -- -c "$script"; args="-c$(printf '\x1f')$script"; fi
    mkdir -p $W/state/tabs; : > $W/state/tabs/last.tsv
    for i in $(seq 2 $tabs); do printf 'tab%s\t%s\t%s\t%s\t\t0\t0\n' $i "$PWD" "$prog" "$args" >> $W/state/tabs/last.tsv; done
    HAETTY_LOG=info HAETTY_LOG_FILE=$W/gui-$kind-$tabs.log setsid $BIN --cols 120 --rows 36 "$prog" "$@" > /dev/null 2>&1 < /dev/null & P=$!
    timeout 15 xdotool search --sync --onlyvisible --pid $P > /dev/null; sleep 20
    D=$(dpid)
    g0=$(ticks $P); d0=$(ticks $D); sleep 30; g1=$(ticks $P); d1=$(ticks $D)
    n=$(HAETTY_LOG=warn $BIN status --json | python3 -Ic 'import json,sys; print(len(json.load(sys.stdin)["sessions"]))')
    echo "  sessions $n; GUI $((g1 - g0)) ticks, RssAnon $(anon $P) kB; daemon $((d1 - d0)) ticks, RssAnon $(anon $D) kB, $(ls /proc/$D/task | wc -l) threads"
    kill $P; wait $P 2>/dev/null; haetty_stop_daemon "$BIN"; sleep 0.5
  )
}
gui_case claude 1
gui_case filled 1
gui_case filled 2
gui_case filled 4

echo "== GUI draw per incremental frame while typing (120x36)"
HAETTY_LOG=trace HAETTY_LOG_FILE=$W/draw.log setsid $BIN --cols 120 --rows 36 --restore-tabs false python3 $PROBE echo-input > /dev/null 2>&1 < /dev/null & P=$!
WW=$(timeout 10 xdotool search --sync --onlyvisible --pid $P | head -1); sleep 2; xdotool windowfocus --sync $WW; sleep 0.3
xdotool type --delay 60 'the quick brown fox jumps over the lazy dog 0123456789'; sleep 1
python3 -I - $W/draw.log <<'PY'
import re, sys
v = sorted(int(m.group(1)) for m in re.finditer(r"frame drawn in (\d+) us \(full false\)", open(sys.argv[1]).read()))
if v: print(f"  n={len(v)} median {v[len(v)//2]/1000:.2f} ms p95 {v[len(v)*95//100]/1000:.2f} ms max {v[-1]/1000:.2f} ms")
PY
kill $P 2>/dev/null; wait $P 2>/dev/null; haetty_stop_daemon "$BIN"
