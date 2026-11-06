#!/usr/bin/env bash
# Which Zig optimize mode libghostty-vt-sys builds with, per cargo profile — from the zig command itself: a `zig`
# shim first on PATH logs its arguments and runs the real zig. Each profile builds libghostty-vt-sys afresh in its
# own target directory under $HAETTY_RUN (the Ghostty checkout is reused through GHOSTTY_SOURCE_DIR: no network).
#   HAETTY_TEST_TMP=/tmp/haetty-test scripts/zig-mode.sh [PROFILE...]   (default: dev test release dist)
# Takes a few minutes per profile (Zig builds Ghostty's libghostty-vt each time).
set -u
REPO=$(cd "$(dirname "$0")/.." && pwd)
. "$REPO/scripts/test-guard.sh"; haetty_guard_init HAETTY_TEST_TMP || exit 3
. "$HOME/.cargo/env" 2>/dev/null
command -v zig > /dev/null || PATH=$HOME/.local/bin:$PATH
REAL=$(command -v zig) || { echo "no zig"; exit 2; }
SHIM=$HAETTY_RUN/zigshim; haetty_guard "$SHIM"; mkdir -p "$SHIM"
LOG=$HAETTY_RUN/zig-calls.log
cat > "$SHIM/zig" <<EOF
#!/bin/sh
[ "\$1" = build ] && echo "\$CARGO_PKG_NAME PROFILE=\$PROFILE DEBUG=\$DEBUG OPT_LEVEL=\$OPT_LEVEL: zig \$*" >> "$LOG"
exec "$REAL" "\$@"
EOF
chmod +x "$SHIM/zig"
src=$(ls -d "$REPO"/target/*/build/libghostty-vt-sys-*/out/ghostty-src 2>/dev/null | head -1)
[ -n "$src" ] && export GHOSTTY_SOURCE_DIR=$src
cd "$REPO" || exit 2
for p in "${@:-dev test release dist}"; do
  for prof in $p; do
    T=$HAETTY_RUN/target-$prof; haetty_guard "$T"
    : > "$LOG.$prof"
    case $prof in
      dev) args="build -p haetty-core" ;;
      test) args="test -p haetty-core --no-run" ;;
      release) args="build --release -p haetty-core" ;;
      *) args="build --profile $prof -p haetty-core" ;;
    esac
    n0=$(wc -l < "$LOG" 2>/dev/null || echo 0)
    PATH=$SHIM:$PATH CARGO_TARGET_DIR=$T cargo $args > "$HAETTY_RUN/build-$prof.log" 2>&1 || { echo "$prof: build failed"; tail -5 "$HAETTY_RUN/build-$prof.log"; }
    line=$(tail -n +$((n0 + 1)) "$LOG" | grep -o -- '-Doptimize=[A-Za-z]*' | head -1)
    env=$(tail -n +$((n0 + 1)) "$LOG" | head -1 | grep -o 'PROFILE=[a-z]* DEBUG=[a-z]* OPT_LEVEL=[0-9sz]*')
    echo "cargo $args → zig build $line   (build script env: $env)"
    rm -rf "$T"
  done
done
echo "full zig command lines: $LOG"
