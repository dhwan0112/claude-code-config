#!/usr/bin/env bash
# `dist` sizes (what the installers ship) and stripped release sizes of haetty, both targets.
cd "$(dirname "$0")/.." || exit 2
. "$HOME/.cargo/env" 2>/dev/null
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$PWD/target}
T=$(mktemp -d)
for t in x86_64-unknown-linux-gnu x86_64-pc-windows-gnu; do
  cargo build -q --release --target $t -p haetty-gui 2>&1 | tail -2
  cargo build -q --profile dist --target $t -p haetty-gui 2>&1 | tail -2
done
L=$CARGO_TARGET_DIR/x86_64-unknown-linux-gnu; W=$CARGO_TARGET_DIR/x86_64-pc-windows-gnu
strip -o $T/l $L/release/haetty; x86_64-w64-mingw32-strip -o $T/w $W/release/haetty.exe
echo "release+strip: linux $(stat -c %s $T/l)  windows $(stat -c %s $T/w)"
echo "dist:          linux $(stat -c %s $L/dist/haetty)  windows $(stat -c %s $W/dist/haetty.exe)"
rm -rf "$T"
