#!/usr/bin/env bash
# Every end-to-end check of a release build, Linux (Xvfb): the regression run, DESIGN §2.3 (system and integrated
# decorations), theme change, the integrated title row, tab features, the orchestrator, the GUI after a daemon crash,
# the first window at 100–200 % scale.
#   HAETTY_TEST_TMP=/tmp/haetty-test [E2E_ENGINE=native] scripts/e2e/run-all.sh [--no-build]
# Prints each check's FAIL lines and TOTAL; exits with the number of scripts that had a failure (77: no Xvfb).
[ -n "${HAETTY_HEAVY_SLOT:-}" ] || exec "$(dirname "$0")/../../tools/heavy.sh" --label "run-all.sh $*" -- bash "$0" "$@"   # at most two heavy jobs (tools/heavy.sh)
here=$(cd "$(dirname "$0")" && pwd)
cd "$here/../.." || exit 2
. "$HOME/.cargo/env" 2>/dev/null
command -v zig > /dev/null || PATH=$HOME/.local/bin:$PATH
export HAETTY_TEST_TMP=${HAETTY_TEST_TMP:-/tmp/haetty-test}
[ "${1:-}" = --no-build ] || cargo build -q --release -p haetty-gui -p haetty-headless 2>&1 | tail -3
echo "binary: $(${CARGO_TARGET_DIR:-target}/release/haetty --version)"
# E2E_ENGINE=native: every session on that engine (the GUI and haetty-headless get `--engine`, through
# engine-gui.sh / engine-headless.sh as HAETTY_BIN / HAETTY_HEADLESS_BIN).
if [ -n "${E2E_ENGINE:-}" ]; then
  export E2E_ENGINE HAETTY_E2E_REAL_BIN=${HAETTY_BIN:-${CARGO_TARGET_DIR:-$PWD/target}/release/haetty}
  export HAETTY_E2E_REAL_HL=${HAETTY_HEADLESS_BIN:-${CARGO_TARGET_DIR:-$PWD/target}/release/haetty-headless}
  export HAETTY_BIN=$here/engine-gui.sh HAETTY_HEADLESS_BIN=$here/engine-headless.sh
  echo "engine: $E2E_ENGINE (every session)"
fi
bad=0
run() {
  local out; out=$("$here/xvfb.sh" env "$@" 2>&1); local rc=$?
  [ $rc = 77 ] && { echo "SKIP $*: no Xvfb"; exit 77; }
  echo "$out" | grep -E "^FAIL|^TOTAL|^SKIP"
  echo "$out" | grep -qE "^FAIL" && bad=$((bad + 1))
}
run bash "$here/regression.sh" e2e
run bash "$here/design-2-3.sh"
run E2E_ARGS="--decorations integrated" E2E_BANDS=2 bash "$here/design-2-3.sh"
run bash "$here/themes.sh"
run bash "$here/titlebar.sh"
run bash "$here/tabs.sh"
run bash "$here/orchestrator.sh"
run bash "$here/daemon-crash.sh"
run bash "$here/scale.sh"
echo "E2E scripts with a failure: $bad"
exit $bad
