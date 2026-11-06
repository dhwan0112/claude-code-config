#!/usr/bin/env bash
# Every end-to-end check of a release build, Linux (Xvfb): the regression run, DESIGN §2.3 (system and integrated
# decorations), theme change, the integrated title row, tab features, the orchestrator, the GUI after a daemon crash.
#   HAETTY_TEST_TMP=/tmp/haetty-test scripts/e2e/run-all.sh [--no-build]
# Prints each check's FAIL lines and TOTAL; exits with the number of scripts that had a failure (77: no Xvfb).
here=$(cd "$(dirname "$0")" && pwd)
cd "$here/../.." || exit 2
. "$HOME/.cargo/env" 2>/dev/null
command -v zig > /dev/null || PATH=$HOME/.local/bin:$PATH
export HAETTY_TEST_TMP=${HAETTY_TEST_TMP:-/tmp/haetty-test}
[ "${1:-}" = --no-build ] || cargo build -q --release -p haetty-gui -p haetty-headless 2>&1 | tail -3
echo "binary: $(${CARGO_TARGET_DIR:-target}/release/haetty --version)"
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
echo "E2E scripts with a failure: $bad"
exit $bad
