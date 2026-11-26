#!/usr/bin/env bash
# run-all's first and last step (stage PK): nothing the tests started is left, and the stable instance is as it was.
#   leftovers.sh before STATE-DIR        notes the time, the stable processes and the test folders
#   leftovers.sh after STATE-DIR FAILED  FAIL for a test process still running, a stable process whose PID or start
#                                        time (since boot, not the wall clock) changed, and, when no script FAILED
#                                        (a failed one keeps its run directory), a test folder left in
#                                        $HAETTY_TEST_TMP or a haetty folder left in /tmp
# HAETTY_STABLE_OPT (default ~/.local/opt/haetty) names the stable install folder.
mode=$1 st=$2 failed=${3:-0}
mkdir -p "$st"
base=${HAETTY_TEST_TMP:-/tmp/haetty-test}
opt=${HAETTY_STABLE_OPT:-${HAETTY_REAL_HOME:-$HOME}/.local/opt/haetty}
started() { sed 's/.*) //' "/proc/$1/stat" 2>/dev/null | awk '{print $20}'; }
stable() {
  local p e
  for p in /proc/[0-9]*; do
    e=$(readlink "$p/exe" 2>/dev/null) || continue
    case $e in "$opt"/*) echo "${p#/proc/} $(started "${p#/proc/}") $e" ;; esac
  done | sort
}
folders() { { ls -d "$base"/haetty-run.* 2>/dev/null; ls -d /tmp/haetty* /tmp/ccterm* 2>/dev/null | grep -vx "$base"; } | sort; }
if [ "$mode" = before ]; then
  started $$ > "$st/start"
  stable > "$st/stable"
  folders > "$st/folders"
  exit 0
fi
P=0 F=0; ok() { P=$((P+1)); echo "PASS $1"; }; bad() { F=$((F+1)); echo "FAIL $1: $2"; }
start=$(cat "$st/start")
left=$(for p in /proc/[0-9]*; do
  pid=${p#/proc/}; t=$(started "$pid"); [ -n "$t" ] && [ "$t" -gt "$start" ] || continue
  e=$(readlink "$p/exe" 2>/dev/null) || continue
  cmd=$(tr '\0' ' ' < "$p/cmdline" 2>/dev/null | cut -c1-140)
  case "$e $cmd" in *"$base"*|*/target*/haetty*|*/target*/examples/*|*Xvfb*) echo "$pid $cmd" ;; esac
done)
[ -z "$left" ] && ok "no process the tests started is still running" || bad "left running" "$(echo "$left" | tr '\n' ';')"
now=$(stable)
[ "$now" = "$(cat "$st/stable")" ] && ok "the stable processes have the same PIDs and start times ($(echo "$now" | grep -c .))" \
  || bad "stable" "before: $(tr '\n' ';' < "$st/stable") after: $(echo "$now" | tr '\n' ';')"
new=$(comm -13 "$st/folders" <(folders))
if [ "$failed" = 0 ]; then
  [ -z "$new" ] && ok "the tests removed their folders" || bad "folders left" "$(echo "$new" | tr '\n' ' ')"
else
  echo "INFO $failed script(s) failed: their run directories are kept ($(echo "$new" | grep -c .) new)"
fi
echo "TOTAL leftovers: $P passed, $F failed"
[ $F = 0 ]
