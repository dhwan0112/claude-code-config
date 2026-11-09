#!/usr/bin/env bash
# Self-test of tools/dev-run.sh with a fake HOME, XDG directories and runtime directory inside the test run (the
# real stable instance is never involved): the checks print both instances and pass; every override or config
# line that points at the stable instance is refused; a real start (under Xvfb) brings up a dev daemon on the
# dev socket and nothing at the stable one; the dev daemon is then stopped by its PID.
#   HAETTY_TEST_TMP=/tmp/haetty-test tools/test-dev-run.sh      (uses the release build in $CARGO_TARGET_DIR)
here=$(cd "$(dirname "$0")" && pwd)
. "$here/../scripts/test-guard.sh"; haetty_guard_init HAETTY_TEST_TMP || exit 3
F=$HAETTY_RUN/dev-run; haetty_guard "$F"
mkdir -p "$F/home" "$F/rt"; chmod 700 "$F/rt"
for v in $(env | grep -oE '^(HAETTY_[A-Z_]*|XDG_[A-Z_]*)='); do unset "${v%=}"; done
export HOME=$F/home XDG_RUNTIME_DIR=$F/rt
D=$here/dev-run.sh
pass=0 fail=0
ok() { if [ "$1" = 1 ]; then pass=$((pass + 1)); echo "PASS $2"; else fail=$((fail + 1)); echo "FAIL $2"; fi; }
refused() { local out; out=$(env "$@" "$D" --no-build --check 2>&1); [ $? = 3 ] && echo "$out" | grep -q REFUSED && echo 1 || echo 0; }

out=$("$D" --no-build --check 2>&1); rc=$?
ok "$([ $rc = 0 ] && echo 1 || echo 0)" "checks pass with nothing shared (exit $rc)"
ok "$(echo "$out" | grep -q "socket .*$F/rt/haetty-dev/daemon.sock .*$F/rt/haetty/daemon.sock" && echo 1 || echo 0)" "socket: dev and stable both shown, different"
ok "$(echo "$out" | grep -q "state .*$F/home/.local/state/haetty-dev " && echo 1 || echo 0)" "state: haetty-dev"
ok "$(echo "$out" | grep -q 'app id .*haetty.dev ' && echo "$out" | grep -q 'instance dev' && echo 1 || echo 0)" "app id .dev, the build reports instance dev"
ok "$(refused HAETTY_STATE_DIR=$F/home/.local/state/haetty)" "refused: HAETTY_STATE_DIR = the stable state directory"
ok "$(refused HAETTY_CONFIG_DIR=$F/home/.config/haetty/sub)" "refused: HAETTY_CONFIG_DIR inside the stable config directory"
ok "$(refused HAETTY_LOG_FILE=$F/home/.local/state/ccterm/gui.log)" "refused: HAETTY_LOG_FILE in the former name's state directory"
mkdir -p "$F/home/.config/haetty-dev"
echo "theme = $F/home/.config/haetty/themes/mine" > "$F/home/.config/haetty-dev/config"
ok "$(refused)" "refused: the dev config names a stable path"
echo "# theme = $F/home/.config/haetty/themes/mine" > "$F/home/.config/haetty-dev/config"
out=$("$D" --no-build --check 2>&1); ok "$([ $? = 0 ] && echo 1 || echo 0)" "a commented-out stable path is not refused"
rm -f "$F/home/.config/haetty-dev/config"

# A real start: the dev window under Xvfb, the shell in its tab.
if [ -n "${DISPLAY:-}" ]; then
  out=$(SHELL=/bin/sh "$D" --no-build 2>&1); rc=$?
  echo "$out" | tail -1
  ok "$([ $rc = 0 ] && [ -S "$F/rt/haetty-dev/daemon.sock" ] && echo 1 || echo 0)" "started: dev daemon socket at $F/rt/haetty-dev/daemon.sock"
  ok "$([ ! -e "$F/rt/haetty/daemon.sock" ] && echo 1 || echo 0)" "nothing at the stable socket"
  bin=${CARGO_TARGET_DIR:-$here/../target}/release/haetty
  dp=$(HAETTY_INSTANCE=dev "$bin" status --json | python3 -c 'import json,sys; print(json.load(sys.stdin)["daemon_pid"])')
  gp=$(echo "$out" | grep -oE 'dev window pid [0-9]+' | grep -oE '[0-9]+$')
  ok "$([ -n "$dp" ] && echo 1 || echo 0)" "dev daemon pid $dp from status --json (dev instance)"
  [ -n "$gp" ] && kill "$gp" 2> /dev/null; [ -n "$dp" ] && kill "$dp" 2> /dev/null; sleep 1

  # From a stable tab (its normal use): a (fake) stable haetty runs dev-run --check as its tab's program, with
  # exactly the environment a stable session passes on (HAETTY_SESSION_ID, an empty HAETTY_INSTANCE, …).
  (env -u HAETTY_INSTANCE setsid "$bin" sh -c "\"$D\" --no-build --check > '$F/from-tab.out' 2>&1; echo rc=\$? >> '$F/from-tab.out'; env | grep -E '^HAETTY_' > '$F/from-tab.env'; sleep 60"      > "$F/stable-gui.log" 2>&1 < /dev/null &)
  # wait for the script's exit line (a line that starts with rc=), with haetty wait instead of a sleep loop
  "$bin" wait file "$F/from-tab.out" --contains "$(printf '\nrc=')" --timeout 30 > /dev/null 2>&1
  ok "$(grep -q '^rc=0' "$F/from-tab.out" 2> /dev/null && echo 1 || echo 0)" "from a stable tab: checks pass (tab env: $(paste -sd' ' "$F/from-tab.env" 2> /dev/null | cut -c1-80))"
  ok "$([ -S "$F/rt/haetty/daemon.sock" ] && grep -q "$F/rt/haetty-dev/daemon.sock" "$F/from-tab.out" && echo 1 || echo 0)" "the fake stable daemon is at the stable socket; dev-run chose the dev one"
  sp=$(env -u HAETTY_INSTANCE "$bin" status --json | python3 -c 'import json,sys; print(json.load(sys.stdin)["daemon_pid"])')
  sg=$(ps -eo pid=,args= | awk -v b="$bin" '$2 == b && $3 == "sh" { print $1 }')
  [ -n "$sg" ] && kill $sg 2> /dev/null; [ -n "$sp" ] && kill "$sp" 2> /dev/null; sleep 1
else
  echo "SKIP real start (no DISPLAY: run under scripts/e2e/xvfb.sh)"
fi
echo "TOTAL dev-run: $pass passed, $fail failed"
[ $fail = 0 ]
