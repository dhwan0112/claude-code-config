# Full gate: both targets build, clippy clean on both, tests, stripped sizes.
. "$(dirname "$0")/e.sh"
set -o pipefail
fail=0
step() { echo "== $*"; }
step "fmt check"; cargo fmt --all -- --check > /tmp/fmt.log 2>&1 || { echo "FMT DIFF"; head -40 /tmp/fmt.log; fail=1; echo "GATE STEP FAILED: fmt"; }
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
mkdir -p /tmp/sizes
T=$CARGO_TARGET_DIR; for b in $T/release/haetty-headless $T/release/haetty $T/x86_64-pc-windows-gnu/release/haetty-headless.exe $T/x86_64-pc-windows-gnu/release/haetty.exe; do
  [ -f "$b" ] || continue
  out=/tmp/sizes/$(echo "$b" | tr / _)
  case "$b" in *.exe) x86_64-w64-mingw32-strip -o "$out" "$b";; *) strip -o "$out" "$b";; esac
  printf "%-60s %10d bytes  (%s)\n" "$b" "$(stat -c %s "$out")" "$(numfmt --to=iec-i --suffix=B "$(stat -c %s "$out")")"
done
echo "GATE fail=$fail"
echo "== linkage"
echo "linux NEEDED:"; readelf -d $CARGO_TARGET_DIR/release/haetty 2>/dev/null | grep NEEDED | sed 's/.*Shared library: /  /'
echo "windows DLLs:"; x86_64-w64-mingw32-objdump -p $CARGO_TARGET_DIR/x86_64-pc-windows-gnu/release/haetty.exe 2>/dev/null | grep 'DLL Name' | sed 's/.*DLL Name: /  /' | sort | tr '\n' ' '; echo
