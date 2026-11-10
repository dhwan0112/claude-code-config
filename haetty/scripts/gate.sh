#!/usr/bin/env bash
# The full gate (CLAUDE.md): fmt, both targets build, clippy clean on both, all tests; then stripped sizes and
# linkage. Run in the haetty folder of any checkout; tests need HAETTY_TEST_TMP (default /tmp/haetty-test).
# Prints "GATE fail=0" when everything passed (and "GATE STEP FAILED: …" for each step that did not).
[ -n "${HAETTY_HEAVY_SLOT:-}" ] || exec "$(dirname "$0")/../tools/heavy.sh" --label "gate.sh $*" -- bash "$0" "$@"   # at most two heavy jobs (tools/heavy.sh)
export HAETTY_TEST_TMP=${HAETTY_TEST_TMP:-/tmp/haetty-test}
# Tests put Unix sockets under it (<base>/haetty-test-<pid>-<ns>/<test>/rt/haetty/daemon.sock): a long base
# overflows the 108-byte socket path limit and a daemon test fails with "path must be shorter than SUN_LEN".
[ ${#HAETTY_TEST_TMP} -le 32 ] || { echo "gate.sh: HAETTY_TEST_TMP is ${#HAETTY_TEST_TMP} characters; use a short base (at most 32), e.g. /tmp/haetty-test"; echo "GATE fail=1"; exit 1; }
cd "$(dirname "$0")/.." || exit 2
. tools/machine.sh   # the toolchain: cargo, zig 0.15.2 in $HAETTY_ZIG_BIN
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$PWD/target}
set -o pipefail
fail=0
mkdir -p "$HAETTY_TEST_TMP"
step() { echo "== $*"; }
step "fmt check"; cargo fmt --all -- --check > "$HAETTY_TEST_TMP/gate-fmt.log" 2>&1 || { echo "FMT DIFF"; head -40 "$HAETTY_TEST_TMP/gate-fmt.log"; fail=1; echo "GATE STEP FAILED: fmt"; }
step "build linux release"; cargo build --release 2>&1 | tail -3 || { fail=1; echo "GATE STEP FAILED: build linux"; }
step "build windows release"; cargo build --release --target x86_64-pc-windows-gnu 2>&1 | tail -3 || { fail=1; echo "GATE STEP FAILED: build windows"; }
# The native-only build (DESIGN.md §10.4): no libghostty in the graph, and it builds with no Zig on PATH.
# Its own target dir, so the default build's cache stays as it is.
step "build native-only (no zig)"
nozig=$(echo "$PATH" | tr ':' '\n' | grep -vxF "$HAETTY_ZIG_BIN" | paste -sd:)
if PATH=$nozig command -v zig > /dev/null; then echo "zig still on the filtered PATH"; fail=1; echo "GATE STEP FAILED: native-only"
elif cargo tree -q -p haetty-gui -p haetty-headless --no-default-features -e normal 2>/dev/null | grep -q ghostty; then echo "libghostty in the native-only graph"; fail=1; echo "GATE STEP FAILED: native-only"
else PATH=$nozig CARGO_TARGET_DIR=$CARGO_TARGET_DIR-native cargo build --release -p haetty-gui -p haetty-headless --no-default-features 2>&1 | tail -3 || { fail=1; echo "GATE STEP FAILED: native-only"; }; fi
# One clippy run per target, judged on its own output (a second run could replay or not replay cached
# warnings). Our crates' sources are touched first so every warning is reported, not only fresh ones.
find crates -name '*.rs' -exec touch {} +
step "clippy linux"; cout=$(cargo clippy --all-targets 2>&1); echo "$cout" | grep -E "^(warning|error)" | sort | uniq -c; grep -qE "^(warning|error)" <<< "$cout" && { fail=1; echo "GATE STEP FAILED: clippy linux"; }
step "clippy windows"; cout=$(cargo clippy --all-targets --target x86_64-pc-windows-gnu 2>&1); echo "$cout" | grep -E "^(warning|error)" | sort | uniq -c; grep -qE "^(warning|error)" <<< "$cout" && { fail=1; echo "GATE STEP FAILED: clippy windows"; }
# A failing test prints FAILED / panicked; a compile error "error[...]" or "error:". (grep -q reads a
# here-string, not a pipe: under pipefail an early match ends the pipe's echo with SIGPIPE and turns the result.)
step "test"; tout=$(cargo test 2>&1); echo "$tout" | grep -E "^test result|FAILED|panicked|^error"; if grep -qE 'FAILED|panicked|^error(\[|:)' <<< "$tout" || ! grep -q "^test result" <<< "$tout"; then fail=1; echo "GATE STEP FAILED: test"; fi
step "sizes (stripped copies)"
S="$HAETTY_TEST_TMP/gate-sizes"; mkdir -p "$S" "$HAETTY_TEST_TMP"
T=$CARGO_TARGET_DIR; for b in $T/release/haetty-headless $T/release/haetty $T/x86_64-pc-windows-gnu/release/haetty-headless.exe $T/x86_64-pc-windows-gnu/release/haetty.exe; do
  [ -f "$b" ] || continue
  out=$S/$(echo "$b" | tr / _)
  case "$b" in *.exe) x86_64-w64-mingw32-strip -o "$out" "$b";; *) strip -o "$out" "$b";; esac
  printf "%-60s %10d bytes  (%s)\n" "$b" "$(stat -c %s "$out")" "$(numfmt --to=iec-i --suffix=B "$(stat -c %s "$out")")"
done
echo "GATE fail=$fail"
echo "== linkage"
echo "linux NEEDED:"; readelf -d $CARGO_TARGET_DIR/release/haetty 2>/dev/null | grep NEEDED | sed 's/.*Shared library: /  /'
echo "windows DLLs:"; x86_64-w64-mingw32-objdump -p $CARGO_TARGET_DIR/x86_64-pc-windows-gnu/release/haetty.exe 2>/dev/null | grep 'DLL Name' | sed 's/.*DLL Name: /  /' | sort | tr '\n' ' '; echo
