#!/usr/bin/env bash
# The Linux half of the night run. On the Windows check machine tools/overnight-soak.ps1 runs it in WSL
# (HAETTY_HEAVY_SLOT is set, the slots are held there); on a Linux machine `night` runs the Linux part alone.
#   overnight-soak-linux.sh check                        other heavy work running? (prints it; exit 1 if so)
#   overnight-soak-linux.sh build COMMIT WINOUT          a checkout of COMMIT, its release build for Linux and
#                                                        Windows; haetty.exe copied to WINOUT\bin (a Windows path)
#   overnight-soak-linux.sh soak MINUTES XROOT WINOUT [ENGINE]
#                                                        the Linux soak v2 (this checkout's scripts/e2e/soak.sh —
#                                                        the current harness — under Xvfb) with the commit's build;
#                                                        output and CSV to WINOUT;
#                                                        XROOT (HAETTY_XROOT) relative to $HOME unless absolute;
#                                                        ENGINE `native`: every session on haetty's own engine
#                                                        (scripts/e2e/engine-gui.sh, as run-all.sh's E2E_ENGINE)
#   overnight-soak-linux.sh clean                        removes the checkout and the test files (keeps the target)
#   overnight-soak-linux.sh night COMMIT [MINUTES] [ENGINE] [OUT]
#                                                        a Linux machine alone: check, both heavy slots, the Linux
#                                                        build of COMMIT, the soak, clean; the verdict is the first
#                                                        line of OUT/latest.txt ("RESULT LINUX PASS|FAIL|NOT-RUN …",
#                                                        a copy of OUT/result-<stamp>.txt); OUT defaults to
#                                                        $HAETTY_NIGHT_DIR/out. Keep the machine awake around it:
#                                                        systemd-inhibit --what=sleep:idle (docs/dev-commands.md)
# The checkout (wt) and the build (target, kept for the next night) live in $HAETTY_NIGHT_DIR (tools/machine.sh,
# default /tmp/haetty-overnight); the test runs in /tmp/haetty-overnight/test (a short path: Unix sockets).
# WINOUT is a Windows path (converted with wslpath) or a Linux one (used as it is).
set -u
here=$(cd "$(dirname "$0")" && pwd)
top=$(git -C "$here" rev-parse --show-toplevel) || exit 2
. "$here/machine.sh"
B=$HAETTY_NIGHT_DIR T=/tmp/haetty-overnight/test
topath() { case "$1" in [A-Za-z]:[\\/]*|\\\\*) wslpath -u "$1" ;; *) printf '%s\n' "$1" ;; esac; }
case "${1:-}" in
check)
  busy=$(ps -eo pid=,comm=,args= | awk '$2 ~ /^(cargo|rustc|zig|Xvfb|cc1|ld|x86_64-w64-mingw)/ { print "  " $1 " " substr($0, index($0, $3), 100) }')
  slots=$("$here/heavy.sh" --status); n=$?
  [ -z "${HAETTY_HEAVY_SLOT:-}" ] || n=0   # our own holds (overnight-soak.ps1 takes both slots first)
  if [ -n "$busy" ] || [ "$n" != 0 ]; then echo "other heavy work is running:"; [ "$n" = 0 ] || echo "  $slots"; [ -z "$busy" ] || echo "$busy"; exit 1; fi
  echo "no other heavy work"
  ;;
build)
  commit=$2 out=$(topath "$3")
  git -C "$top" worktree remove --force $B/wt 2> /dev/null; rm -rf $B/wt; mkdir -p $B
  git -C "$top" worktree add -q --detach $B/wt "$commit" || exit 2
  echo "commit $commit = $(git -C "$top" rev-parse --short=12 "$commit")"
  src=$(ls -d "$top"/haetty/target/*/build/libghostty-vt-sys-*/out/ghostty-src 2> /dev/null | head -1)
  [ -n "$src" ] && export GHOSTTY_SOURCE_DIR=$src   # no network
  export CARGO_TARGET_DIR=$B/target
  cd $B/wt/haetty || exit 2
  cargo build -q --release -p haetty-gui -p haetty-headless 2>&1 | tail -3
  [ -x $B/target/release/haetty ] || exit 2
  if [ -z "${HAETTY_NIGHT_LINUX_ONLY:-}" ]; then
    cargo build -q --release -p haetty-gui --target x86_64-pc-windows-gnu 2>&1 | tail -3
    mkdir -p "$out/bin" && cp $B/target/x86_64-pc-windows-gnu/release/haetty.exe "$out/bin/haetty.exe" || exit 2
  fi
  echo "linux:   $($B/target/release/haetty --version)"
  ;;
soak)
  min=$2 xroot=$3 out=$(topath "$4") engine=${5:-ghostty}
  case "$xroot" in /*|"") ;; *) xroot=$HOME/$xroot ;; esac
  cd "$top/haetty" || exit 2
  rm -rf $T; mkdir -p $T
  export HAETTY_BIN=$B/target/release/haetty HAETTY_HEADLESS_BIN=$B/target/release/haetty-headless HAETTY_TEST_TMP=$T
  if [ "$engine" != ghostty ]; then
    export E2E_ENGINE=$engine HAETTY_E2E_REAL_BIN=$HAETTY_BIN HAETTY_E2E_REAL_HL=$HAETTY_HEADLESS_BIN
    export HAETTY_BIN=$top/haetty/scripts/e2e/engine-gui.sh HAETTY_HEADLESS_BIN=$top/haetty/scripts/e2e/engine-headless.sh
  fi
  echo "engine: $engine"
  [ -n "$xroot" ] && [ -d "$xroot" ] && export HAETTY_XROOT=$xroot
  scripts/e2e/xvfb.sh bash scripts/e2e/soak.sh "$out/linux.csv" $((min * 60)) 2>&1 | grep -v -e XGetInputFocus -e 'screen size is bogus'
  rm -rf $T
  ;;
clean)
  git -C "$top" worktree remove --force $B/wt 2> /dev/null; rm -rf $B/wt $T; git -C "$top" worktree prune
  ;;
night)
  commit=${2:?night COMMIT [MINUTES] [ENGINE] [OUT]} min=${3:-60} engine=${4:-ghostty} out=${5:-$B/out}
  mkdir -p "$out" || exit 2
  if [ -z "${HAETTY_HEAVY_SLOT:-}" ]; then
    chk=$(bash "$0" check) || { printf 'RESULT LINUX NOT-RUN (other heavy work running)\n%s\n' "$chk" > "$out/latest.txt"; cat "$out/latest.txt"; exit 1; }
    exec "$here/heavy.sh" --label "night run $commit" -- bash "$0" night "$commit" "$min" "$engine" "$out"
  fi
  coproc HOLD { "$here/heavy.sh" --hold "night run $commit (2/2)"; }   # the second slot too: nothing else heavy
  read -r -u "${HOLD[0]}" _
  stamp=$(date +%Y%m%d-%H%M); log=
  ver=$(HAETTY_NIGHT_LINUX_ONLY=1 bash "$0" build "$commit" "$out" 2>&1)
  if [ $? != 0 ]; then lin="NOT-RUN (build failed)"
  else
    rm -f "$out/linux.csv"
    log=$(bash "$0" soak "$min" "${HAETTY_XROOT:-}" "$out" "$engine" 2>&1)
    v=$(printf '%s\n' "$log" | grep '^VERDICT ' | tail -1)
    case "$v" in "VERDICT PASS"*) lin=PASS ;; "") lin="FAIL (no verdict: $(printf '%s\n' "$log" | tail -1))" ;; *) lin=FAIL ;; esac
  fi
  {
    echo "RESULT LINUX $lin"
    echo "commit: $commit; engine: $engine; $min min; machine: $(hostname); started $stamp, ended $(date +%Y%m%d-%H%M)"
    echo "$ver"; echo "harness: $(git -C "$top" log -1 --format='%h %s')"
    echo; echo "== Linux soak v2 (scripts/e2e/soak.sh)"; echo "$log"
    echo "-- linux.csv"; cat "$out/linux.csv" 2> /dev/null || echo "(none)"
  } > "$out/result-$stamp.txt"
  cp "$out/result-$stamp.txt" "$out/latest.txt"
  exec {HOLD[1]}>&-; wait "$HOLD_PID" 2> /dev/null
  bash "$0" clean
  head -2 "$out/latest.txt"
  [ "$lin" = PASS ]
  ;;
*) sed -n '2,22p' "$0"; exit 2 ;;
esac
