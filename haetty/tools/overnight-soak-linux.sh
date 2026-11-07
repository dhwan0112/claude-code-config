#!/usr/bin/env bash
# The WSL half of tools/overnight-soak.ps1 (run by it; HAETTY_HEAVY_SLOT is set, the slots are held there).
#   overnight-soak-linux.sh check                        other heavy work running? (prints it; exit 1 if so)
#   overnight-soak-linux.sh build COMMIT WINOUT          a checkout of COMMIT, its release build for Linux and
#                                                        Windows; haetty.exe copied to WINOUT\bin (a Windows path)
#   overnight-soak-linux.sh soak MINUTES XROOT WINOUT    the Linux soak v2 (this checkout's scripts/e2e/soak.sh —
#                                                        the current harness — under Xvfb) with the commit's build;
#                                                        output and CSV to WINOUT;
#                                                        XROOT (HAETTY_XROOT) relative to $HOME unless absolute
#   overnight-soak-linux.sh clean                        removes the checkout and the test files (keeps the target)
# Everything lives in /tmp/haetty-overnight: the checkout (wt), the build (target, kept for the next night),
# the test runs (test).
set -u
here=$(cd "$(dirname "$0")" && pwd)
top=$(git -C "$here" rev-parse --show-toplevel) || exit 2
B=/tmp/haetty-overnight
. "$HOME/.cargo/env" 2> /dev/null
command -v zig > /dev/null || PATH=$HOME/.local/bin:$PATH
case "${1:-}" in
check)
  busy=$(ps -eo pid=,comm=,args= | awk '$2 ~ /^(cargo|rustc|zig|Xvfb|cc1|ld|x86_64-w64-mingw)/ { print "  " $1 " " substr($0, index($0, $3), 100) }')
  slots=$("$here/heavy.sh" --status); n=$?
  [ -z "${HAETTY_HEAVY_SLOT:-}" ] || n=0   # our own holds (overnight-soak.ps1 takes both slots first)
  if [ -n "$busy" ] || [ "$n" != 0 ]; then echo "other heavy work is running:"; [ "$n" = 0 ] || echo "  $slots"; [ -z "$busy" ] || echo "$busy"; exit 1; fi
  echo "no other heavy work"
  ;;
build)
  commit=$2 out=$(wslpath -u "$3")
  git -C "$top" worktree remove --force $B/wt 2> /dev/null; rm -rf $B/wt; mkdir -p $B
  git -C "$top" worktree add -q --detach $B/wt "$commit" || exit 2
  src=$(ls -d "$top"/haetty/target/*/build/libghostty-vt-sys-*/out/ghostty-src 2> /dev/null | head -1)
  [ -n "$src" ] && export GHOSTTY_SOURCE_DIR=$src   # no network
  export CARGO_TARGET_DIR=$B/target
  cd $B/wt/haetty || exit 2
  cargo build -q --release -p haetty-gui -p haetty-headless 2>&1 | tail -3
  cargo build -q --release -p haetty-gui --target x86_64-pc-windows-gnu 2>&1 | tail -3
  mkdir -p "$out/bin" && cp $B/target/x86_64-pc-windows-gnu/release/haetty.exe "$out/bin/haetty.exe" || exit 2
  echo "linux:   $($B/target/release/haetty --version)"
  ;;
soak)
  min=$2 xroot=$3 out=$(wslpath -u "$4")
  case "$xroot" in /*) ;; *) xroot=$HOME/$xroot ;; esac
  cd "$top/haetty" || exit 2
  rm -rf $B/test; mkdir -p $B/test
  export HAETTY_BIN=$B/target/release/haetty HAETTY_HEADLESS_BIN=$B/target/release/haetty-headless HAETTY_TEST_TMP=$B/test
  [ -d "$xroot" ] && export HAETTY_XROOT=$xroot
  scripts/e2e/xvfb.sh bash scripts/e2e/soak.sh "$out/linux.csv" $((min * 60)) 2>&1 | grep -v -e XGetInputFocus -e 'screen size is bogus'
  rm -rf $B/test
  ;;
clean)
  git -C "$top" worktree remove --force $B/wt 2> /dev/null; rm -rf $B/wt $B/test; git -C "$top" worktree prune
  ;;
*) sed -n '2,11p' "$0"; exit 2 ;;
esac
