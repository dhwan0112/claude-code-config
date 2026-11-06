# Sourced by the end-to-end scripts in this folder. Sets E2E (this folder), REPO (the haetty folder), BIN and HL
# (the release `haetty` and `haetty-headless`, or $HAETTY_BIN / $HAETTY_HEADLESS_BIN), PROBE, and test isolation
# (scripts/test-guard.sh: everything is written only in $HAETTY_RUN under $HAETTY_TEST_TMP).
E2E=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
[ -n "${HAETTY_HEAVY_SLOT:-}" ] || exec "$E2E/../../tools/heavy.sh" --label "$(basename "$0") $*" -- bash "$0" "$@"   # at most two heavy jobs
REPO=$(cd "$E2E/../.." && pwd)
. "$REPO/scripts/test-guard.sh"
haetty_guard_init HAETTY_TEST_TMP || exit 3
_t=${CARGO_TARGET_DIR:-$REPO/target}
BIN=${HAETTY_BIN:-$_t/release/haetty}
HL=${HAETTY_HEADLESS_BIN:-$_t/release/haetty-headless}
PROBE=$REPO/crates/haetty-core/tests/helpers/probe.py
[ -x "$BIN" ] || { echo "lib.sh: no $BIN (cargo build --release first)" >&2; exit 3; }
# Claude Code's own markers must not reach programs started here (they would count as child sessions).
for _v in $(env | grep -oE '^(CLAUDE_CODE_[A-Z_]*|CLAUDECODE|CLAUDE_[A-Z_]*SESSION[A-Z_]*)='); do unset "${_v%=}"; done
