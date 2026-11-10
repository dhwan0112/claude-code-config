#!/usr/bin/env bash
# Builds the helper `ghostty-ref`: the newest Ghostty's libghostty-vt (the pin below), statically linked into a
# line-protocol server that haetty-vtdiff uses as its reference `ghostty-main` (tools/ghostty-ref/README.md).
# Test tooling only: nothing here is part of haetty's build, dist or installers. Re-runnable. Run it through the
# heavy-job wrapper:
#   tools/heavy.sh --label ghostty-ref -- bash tools/ghostty-ref/build.sh
set -euo pipefail

PIN=e035beb9437118b803830a5864ea2f4dd7094354
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
. "$HERE/../machine.sh"   # HAETTY_ZIG_REF, HAETTY_WORK
ZIG=${ZIG:-$HAETTY_ZIG_REF}
SRC=${GHOSTTY_SRC:-$HAETTY_WORK/ghostty-ref/ghostty}   # the pinned checkout; outside the repo and outside /tmp
# Build output. Set GHOSTTY_REF_TARGET to keep it outside the stage worktree (a finished stage removes the
# worktree and everything in it). The default is git-ignored (/target-*/ in haetty/.gitignore).
TARGET=${GHOSTTY_REF_TARGET:-$HERE/../../target-ghostty-ref}
PREFIX=$TARGET/install                             # zig's --prefix: lib/, include/, share/
CACHE=$TARGET/zig-cache
BUILD=$TARGET/build                                # unstripped helper
OUT=$TARGET/ghostty-ref                            # stripped helper: the deliverable

die() { echo "build.sh: $*" >&2; exit 1; }
now() { date +%s.%N; }
secs() { awk -v a="$1" -v b="$2" 'BEGIN { printf "%.1f", b - a }'; }

# The pin: the checkout must be at it, clean. (A linked worktree's .git is a file, so ask git.)
top=$(git -C "$SRC" rev-parse --show-toplevel 2>/dev/null) || die "no git checkout at $SRC (set GHOSTTY_SRC=)"
[ "$top" = "$(cd "$SRC" && pwd -P)" ] || die "$SRC is inside another repository, not its top"
head=$(git -C "$SRC" rev-parse HEAD)
[ "$head" = "$PIN" ] || die "checkout is at $head, not the pin $PIN"
git -C "$SRC" diff --quiet HEAD -- || die "checkout has local changes; refusing to build"
[ -x "$ZIG" ] || die "zig not found at $ZIG (set ZIG=)"
zver=$("$ZIG" version)
[ "$zver" = "0.17.0" ] || die "zig is $zver, the pin needs 0.17.0"
echo "pin: Ghostty $PIN, zig $zver"
mkdir -p "$BUILD"

# 1. libghostty-vt, static and shared, into $PREFIX
t0=$(now)
(
  cd "$SRC"
  "$ZIG" build -Demit-lib-vt=true -Doptimize=ReleaseFast -Dcpu=baseline \
    -Demit-xcframework=false -Dapp-runtime=none \
    --prefix "$PREFIX" --cache-dir "$CACHE"
)
t1=$(now)
echo "libghostty-vt build: $(secs "$t0" "$t1") s"
[ -f "$PREFIX/lib/libghostty-vt.a" ] || die "no static library in $PREFIX/lib"
[ -f "$PREFIX/include/ghostty/vt.h" ] || die "no header in $PREFIX/include"

# 2. the helper, linked to the static archive (named by path: -lghostty-vt would pick the shared object in lib/)
extra=""
pc=$PREFIX/share/pkgconfig/libghostty-vt-static.pc
if [ -f "$pc" ]; then
  extra=$(sed -n 's/^Libs\.private: *//p' "$pc" | tr -s ' ' | sed 's/-lghostty-vt-static//; s/-lghostty-vt//')
fi
echo "extra libraries: ${extra:-none}"
t2=$(now)
# shellcheck disable=SC2086
"$ZIG" cc -std=c11 -O2 -Wall -DGHOSTTY_STATIC -I "$PREFIX/include" \
  "$HERE/ghostty-ref.c" "$PREFIX/lib/libghostty-vt.a" $extra -o "$BUILD/ghostty-ref"
t3=$(now)
echo "helper compile and link: $(secs "$t2" "$t3") s"

# 3. stripped copy
cp "$BUILD/ghostty-ref" "$OUT"
if command -v strip >/dev/null 2>&1; then strip "$OUT"; else "$ZIG" objcopy --strip-all "$OUT" "$OUT.stripped" && mv "$OUT.stripped" "$OUT"; fi
chmod +x "$OUT"
echo "helper: $OUT"
echo "size: $(stat -c %s "$OUT") bytes (stripped; unstripped $(stat -c %s "$BUILD/ghostty-ref") bytes)"
echo "dynamic dependencies:"
ldd "$OUT" | sed 's/^/  /'
