#!/usr/bin/env bash
# The full gate (CLAUDE.md): fmt, both targets build, clippy clean on both, all tests; then stripped sizes and
# linkage. Run in the haetty folder of any checkout; tests need HAETTY_TEST_TMP (default /tmp/haetty-test).
# Prints "GATE fail=0" when everything passed (and "GATE STEP FAILED: …" for each step that did not).
export HAETTY_TEST_TMP=${HAETTY_TEST_TMP:-/tmp/haetty-test}
cd "$(dirname "$0")/.." || exit 2
. "$HOME/.cargo/env" 2>/dev/null
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$PWD/target}
set -o pipefail
fail=0
mkdir -p "$HAETTY_TEST_TMP"
step() { echo "== $*"; }
step "fmt check"; cargo fmt --all -- --check > "$HAETTY_TEST_TMP/gate-fmt.log" 2>&1 || { echo "FMT DIFF"; head -40 "$HAETTY_TEST_TMP/gate-fmt.log"; fail=1; echo "GATE STEP FAILED: fmt"; }
step "build linux release"; cargo build --release 2>&1 | tail -3 || { fail=1; echo "GATE STEP FAILED: build linux"; }
step "build windows release"; cargo build --release --target x86_64-pc-windows-gnu 2>&1 | tail -3 || { fail=1; echo "GATE STEP FAILED: build windows"; }
# One clippy run per target, judged on its own output (a second run could replay or not replay cached
# warnings). Our crates' sources are touched first so every warning is reported, not only fresh ones.
find crates -name '*.rs' -exec touch {} +
step "clippy linux"; cout=$(cargo clippy --all-targets 2>&1); echo "$cout" | grep -E "^(warning|error)" | sort | uniq -c; echo "$cout" | grep -qE "^(warning|error)" && { fail=1; echo "GATE STEP FAILED: clippy linux"; }
step "clippy windows"; cout=$(cargo clippy --all-targets --target x86_64-pc-windows-gnu 2>&1); echo "$cout" | grep -E "^(warning|error)" | sort | uniq -c; echo "$cout" | grep -qE "^(warning|error)" && { fail=1; echo "GATE STEP FAILED: clippy windows"; }
# A failing test prints FAILED / panicked; a compile error "error[...]" or "error:".
step "test"; tout=$(cargo test 2>&1); echo "$tout" | grep -E "^test result|FAILED|panicked|^error"; if echo "$tout" | grep -qE 'FAILED|panicked|^error(\[|:)' || ! echo "$tout" | grep -q "^test result"; then fail=1; echo "GATE STEP FAILED: test"; fi
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
