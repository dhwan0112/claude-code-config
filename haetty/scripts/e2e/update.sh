#!/usr/bin/env bash
# Inside xvfb.sh: the in-app update end to end (stage PU, DESIGN.md §2.4, §3.17), in an isolated HOME. Two test builds
# of this tree (`--features update-test`: they trust the TEST-ONLY key and CA) report 0.90.0 and 0.90.1; a local HTTPS
# release server (`fake_release_server`) serves signed releases. Form (a): 0.90.0 installed as a version folder and
# link → the panel's Download and install → Restart now → 0.90.1 runs with the tabs back → Roll back → 0.90.0 again.
# A new version that ends at once (0.90.2) is rolled back by the restart's watch. Form (b): a single executable
# replaced in place. The real ~/.local/opt/haetty, ~/.local/bin/haetty and the stable haetty processes are compared
# before and after. usage: update.sh
. "$(dirname "$0")/lib.sh"
REAL_HOME=$HAETTY_REAL_HOME
real_state() { stat -c '%n %Y' "$REAL_HOME/.local/opt/haetty" "$REAL_HOME/.local/bin/haetty" 2>/dev/null; ps -eo pid,lstart,args | grep -F "$REAL_HOME/.local/opt/haetty" | grep -v grep; }
REAL0=$(real_state)
W=$HAETTY_RUN/updaterun; haetty_guard $W; rm -rf $W; mkdir -p $W/rt $W/xrt $W/state $W/cfg $W/srv $W/home
chmod 700 $W/rt $W/xrt
P=0 F=0; ok() { P=$((P+1)); echo "PASS $1"; }; bad() { F=$((F+1)); echo "FAIL $1: $2"; }
T=${CARGO_TARGET_DIR:-$REPO/target}-updtest
A=aaaaaaaaaaaa0000000000000000000000000000 B=bbbbbbbbbbbb0000000000000000000000000000 C=cccccccccccc0000000000000000000000000000
build() { # VERSION COMMIT OUT
  ( cd "$REPO" && HAETTY_BUILD_VERSION=$1 HAETTY_BUILD_COMMIT=$2 CARGO_TARGET_DIR=$T \
      cargo build -q --release -p haetty-gui --features update-test 2>&1 | tail -3 ) && cp "$T/release/haetty" "$3"
}
build 0.90.0 $A $W/haetty-0.90.0 && build 0.90.1 $B $W/haetty-0.90.1 || { echo "FAIL build"; exit 1; }
( cd "$REPO" && cargo build -q --example fake_release_server -p haetty-update 2>&1 | tail -3 )
SRVBIN=${CARGO_TARGET_DIR:-$REPO/target}/debug/examples/fake_release_server
# 0.90.2 ends at once, after a correct --version (the install's own check passes).
printf '#!/bin/sh\n[ "$1" = --version ] && { echo "haetty 0.90.2 (commit cccccccccccc, built 2026-11-20, instance stable)"; exit 0; }\nexit 3\n' > $W/haetty-0.90.2
chmod 0755 $W/haetty-0.90.2
release() { # VERSION COMMIT BINARY: the server's release
  local d=$W/pack; rm -rf $d; mkdir -p $d; cp "$3" $d/haetty; echo MIT > $d/LICENSE
  ( cd $d && tar --format=ustar -czf $W/srv/haetty-$1-x86_64-linux.tar.gz haetty LICENSE )
  find $W/srv -name 'haetty-*' ! -name "haetty-$1-*" -delete
  printf '### Fixed\n- release %s\n' "$1" > $W/notes.md
  $W/haetty-0.90.1 release-manifest --version $1 --commit $2 --built 2026-11-20T00:00:00Z --protocol 12 --notes $W/notes.md \
    --asset "x86_64-linux=$W/srv/haetty-$1-x86_64-linux.tar.gz:haetty,LICENSE" > $W/srv/manifest.json
  touch $W/srv/reload; sleep 0.6
}
release 0.90.1 $B $W/haetty-0.90.1
$SRVBIN $W/srv > $W/srv.log 2>&1 & SP=$!
for i in $(seq 50); do [ -s $W/srv/port ] && break; sleep 0.1; done
PORT=$(cat $W/srv/port)
# The window's world: HOME, the instance's folders, the server, the test CA; no HAETTY_INSTANCE (form a needs the
# stable instance's rules), so every folder is pointed into $W.
export HOME=$W/home XDG_RUNTIME_DIR=$W/xrt HAETTY_SOCKET=$W/rt/d.sock HAETTY_STATE_DIR=$W/state HAETTY_CONFIG_DIR=$W/cfg
export HAETTY_LOG=debug HAETTY_LOG_FILE=$W/gui.log HAETTY_UPDATE_TEST=1 HAETTY_UPDATE_TEST_FIRST_MS=1500
export HAETTY_UPDATE_TEST_ROOT=$REPO/crates/haetty-update/tests/TEST-ONLY-ca.pem
unset HAETTY_INSTANCE
echo "update-url = https://localhost:$PORT/r" > $W/cfg/config
OPT=$HOME/.local/opt/haetty LINK=$HOME/.local/bin/haetty
$W/haetty-0.90.0 install-layout put --opt $OPT --name 0.90.0-aaaaaaaaaaaa --commit $A -- $W/haetty-0.90.0 > /dev/null
mv $OPT/0.90.0-aaaaaaaaaaaa/haetty-0.90.0 $OPT/0.90.0-aaaaaaaaaaaa/haetty 2>/dev/null || {
  chmod u+w $OPT/0.90.0-aaaaaaaaaaaa; mv $OPT/0.90.0-aaaaaaaaaaaa/haetty-0.90.0 $OPT/0.90.0-aaaaaaaaaaaa/haetty; chmod 0555 $OPT/0.90.0-aaaaaaaaaaaa; }
$W/haetty-0.90.0 install-layout activate 0.90.0-aaaaaaaaaaaa --opt $OPT > /dev/null
active() { basename "$(dirname "$(readlink $LINK)")"; }
[ "$(active)" = 0.90.0-aaaaaaaaaaaa ] && ok "0.90.0 installed as a version folder and link" || bad "setup" "$(active)"
J() { HAETTY_LOG=warn $LINK status --json 2>/dev/null; }
key() { xdotool key --clearmodifiers "$@"; sleep 0.4; }
pal() { key ctrl+shift+p; xdotool type --delay 30 "$1"; sleep 0.3; key Return; }
# The window a restart started: its PID from the log line ("restart: … started (pid N)"), never by name.
gui_pid() { grep -oE 'started \(pid [0-9]+\)' $W/gui.log | tail -1 | grep -oE '[0-9]+' || true; }
win() { GW=$(timeout 15 xdotool search --sync --onlyvisible --pid $1 | head -1); sleep 1.5; xdotool windowfocus --sync $GW 2>/dev/null; sleep 0.3; }
waitlog() { for i in $(seq ${2:-100}); do grep -q "$1" $W/gui.log && return 0; sleep 0.2; done; return 1; }
updates_row() { pal "updates"; for _ in $(seq $1); do key Down; done; }

# --- Form (a) ------------------------------------------------------------------------------------------------
# An interactive shell: a tab that a restart restores and that stays open. A fake claude started in it with arguments
# comes back after the restart as `claude ARGS --resume ID` (the proxy's saved arguments).
SID=4a1b2c3d-5e6f-4a7b-8c9d-0e1f2a3b4c5d
mkdir -p $W/bin $W/work; WK=$(echo "$W/work" | sed 's/[^A-Za-z0-9]/-/g')
mkdir -p $HOME/.claude/projects/$WK; echo '{"type":"user"}' > $HOME/.claude/projects/$WK/$SID.jsonl
cat > $W/bin/claude <<EOS
#!/bin/sh
printf '%s\n' "\$@" > $W/claude-args.\$\$
printf '{"session_id":"$SID","transcript_path":"$HOME/.claude/projects/$WK/$SID.jsonl","cwd":"$W/work"}' | $LINK hook SessionStart
exec sleep 600
EOS
chmod +x $W/bin/claude
export PATH=$W/bin:$PATH
cd $W/work
setsid $LINK --cols 90 --rows 20 sh > /dev/null 2>&1 < /dev/null & GP=$!
win $GP
xdotool type --delay 20 "claude 'a prompt' --model haiku --add-dir '/w/a b'"; key Return; sleep 2
waitlog "update check: 0.90.1 available" && ok "the check found 0.90.1 (one request to the test server)" || bad "check" "$(grep 'update check' $W/gui.log | tail -1)"
updates_row 3; key Return   # Download and install
waitlog "update install: 0.90.1-bbbbbbbbbbbb installed" 150 && ok "downloaded, verified, installed beside 0.90.0" || bad "install" "$(grep 'update install' $W/gui.log | tail -1)"
[ "$(active)" = 0.90.1-bbbbbbbbbbbb ] && [ -x $OPT/0.90.0-aaaaaaaaaaaa/haetty ] && ok "the link points at 0.90.1; 0.90.0 stays" || bad "link" "$(active)"
[ "$(stat -c %a $OPT/0.90.1-bbbbbbbbbbbb)" = 555 ] && ok "the new folder is read-only" || bad "read-only" "$(stat -c %a $OPT/0.90.1-bbbbbbbbbbbb)"
kill -0 $GP && ok "installing changed nothing running (0.90.0's window still up)" || bad "running" "window gone"
key Escape; updates_row 3; key Return; key Return   # Restart now, twice
wait $GP 2>/dev/null
waitlog "restart: .* started" 150 && ok "Restart now: the window closed, the daemon ended, the new version started" || bad "restart" "$(grep restart: $W/gui.log | tail -2)"
NP=$(gui_pid); sleep 3
V=$($LINK --version); case "$V" in *"0.90.1 (commit bbbbbbbbbbbb"*) ok "0.90.1 runs: $V" ;; *) bad "version" "$V" ;; esac
J | grep -q '"sessions":\[{' && ok "the tab is back after the restart" || bad "tabs" "$(J | head -c 300)"
for i in $(seq 1 30); do grep -lx -- "--resume" $W/claude-args.* > /dev/null 2>&1 && break; sleep 0.3; done
R=$(grep -lx -- "--resume" $W/claude-args.* 2>/dev/null | head -1)
[ -n "$R" ] && [ "$(grep -v -x -e --settings -e '/.*settings.*json' $R)" = "$(printf '%s\n' --model haiku --add-dir '/w/a b' --resume $SID)" ] && ok "the restart resumed claude with its arguments" || bad "claude args" "$(cat $W/claude-args.* 2>/dev/null | tr '\n' '|')"
sleep 21
[ ! -f $W/state/update/pending ] && ok "the new version stayed up 20 s: confirmed" || bad "confirm" "$(cat $W/state/update/pending)"
# Roll back from the panel; the server already has 0.90.2, which ends at once.
release 0.90.2 $C $W/haetty-0.90.2
NP=$(gui_pid); win $NP
updates_row 1; key Return; key Return
for i in $(seq 150); do [ "$(gui_pid)" != "$NP" ] && break; sleep 0.2; done
[ "$(active)" = 0.90.0-aaaaaaaaaaaa ] && ok "Roll back: the link points at 0.90.0 again" || bad "rollback" "$(active)"
NP=$(gui_pid); sleep 3
case "$($LINK --version)" in *"0.90.0 (commit aaaaaaaaaaaa"*) ok "0.90.0 runs again" ;; *) bad "version after rollback" "$($LINK --version)" ;; esac
win $NP
waitlog "update check: 0.90.2 available" 100 && ok "0.90.2 found" || bad "check 0.90.2" "$(grep 'update check' $W/gui.log | tail -1)"
updates_row 3; key Return
waitlog "update install: 0.90.2-cccccccccccc installed" 150 || bad "install 0.90.2" "$(grep 'update install' $W/gui.log | tail -1)"
key Escape; updates_row 3; key Return; key Return
waitlog "restart: the new version ended" 250 && ok "0.90.2 ended at its start: noticed within 20 s" || bad "watch" "$(grep restart: $W/gui.log | tail -2)"
for i in $(seq 100); do [ "$(active)" = 0.90.0-aaaaaaaaaaaa ] && break; sleep 0.2; done
[ "$(active)" = 0.90.0-aaaaaaaaaaaa ] && ok "rolled back to 0.90.0 by itself" || bad "auto rollback" "$(active)"
sleep 3; NP=$(gui_pid)
grep -q "rolled back to 0.90.0" $W/state/update/note 2>/dev/null || grep -q "the new version ended" $W/gui.log && ok "the reason is kept for the panel" || bad "note" "-"
[ -n "$NP" ] && kill $NP 2>/dev/null; sleep 1; haetty_stop_daemon "$LINK"
# --- Form (b) ------------------------------------------------------------------------------------------------
release 0.90.1 $B $W/haetty-0.90.1
mkdir -p $W/single; cp $W/haetty-0.90.0 $W/single/haetty
rm -rf $W/state/update
setsid $W/single/haetty --cols 90 --rows 20 sh -c 'exec sleep 600' > /dev/null 2>&1 < /dev/null & GP=$!
win $GP
waitlog "update check: 0.90.1 available" 100 || true
updates_row 3; key Return
waitlog "update install: single installed" 150 && ok "(b) the single executable replaced" || bad "(b) install" "$(grep 'update install' $W/gui.log | tail -1)"
case "$($W/single/haetty --version)" in *bbbbbbbbbbbb*) ok "(b) the file is 0.90.1, the old one beside it" ;; *) bad "(b) version" "$($W/single/haetty --version)" ;; esac
[ -f $W/single/haetty.old ] || bad "(b) old" "no haetty.old"
kill $GP 2>/dev/null; wait $GP 2>/dev/null; haetty_stop_daemon "$W/single/haetty"
kill $SP 2>/dev/null
# Nothing real touched.
[ "$(real_state)" = "$REAL0" ] && ok "the real ~/.local/opt/haetty, ~/.local/bin/haetty and stable processes are as before" || bad "real" "changed"
echo "TOTAL update.sh: $P passed, $F failed"
