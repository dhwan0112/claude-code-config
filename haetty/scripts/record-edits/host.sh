#!/usr/bin/env bash
# Inside scripts/e2e/xvfb.sh: a dev instance window over a fresh test repository (mkrepo.py) for recording a real
# Claude Code's edit blocks (stage PD's replay set; README.md here). Kept until $W/stop exists; drive.sh drives it.
# usage: host.sh NAME COLS ROWS
. "$(dirname "$0")/../e2e/lib.sh"
N=$1 C=$2 R=$3 HERE=$(cd "$(dirname "$0")" && pwd)
W=$HAETTY_RUN/$N; haetty_guard $W; rm -rf $W; mkdir -p $W/rt $W/xrt $W/state $W/cfg $W/repo
chmod 700 $W/rt $W/xrt
export HAETTY_INSTANCE=dev HAETTY_SOCKET=$W/rt/d.sock HAETTY_STATE_DIR=$W/state HAETTY_CONFIG_DIR=$W/cfg XDG_RUNTIME_DIR=$W/xrt
export HAETTY_LOG=info HAETTY_LOG_FILE=$W/gui.log HAETTY_DUMP_FRAME=$W/f.ppm SHELL=/bin/sh ENV= PS1='$ '
unset CLAUDE_CONFIG_DIR
printf 'restore-tabs = false\nobserve-new-tabs = true\n' > $W/cfg/config
python3 -I $HERE/mkrepo.py $W/repo $W/hooks.jsonl
G="git -C $W/repo -c user.name=t -c user.email=t@example.invalid -c commit.gpgsign=false"
$G init -q -b main; $G add -A; $G commit -qm base
cd $W/repo
setsid $BIN --cols $C --rows $R --restore-tabs false > /dev/null 2>&1 < /dev/null & GP=$!
GW=$(timeout 10 xdotool search --sync --onlyvisible --pid $GP | head -1); sleep 2
xdotool windowfocus --sync $GW 2>/dev/null
cat > $HAETTY_TEST_TMP/record-$N.env <<EOS
export DISPLAY=$DISPLAY HAETTY_INSTANCE=dev HAETTY_SOCKET=$W/rt/d.sock HAETTY_STATE_DIR=$W/state HAETTY_CONFIG_DIR=$W/cfg XDG_RUNTIME_DIR=$W/xrt
W=$W GW=$GW GP=$GP BIN=$BIN
EOS
echo "host $N ready: $W"
while [ ! -e $W/stop ] && kill -0 $GP 2>/dev/null; do sleep 1; done
kill $GP 2>/dev/null; sleep 0.5; haetty_stop_daemon "$BIN"
