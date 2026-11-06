#!/usr/bin/env bash
# Heavy jobs — builds, the gate, check-range, end-to-end and soak runs, fuzzing, installer self-tests, WSL builds
# for Windows — run in at most $HAETTY_HEAVY_SLOTS (default 2) slots at a time, Linux and Windows together (this
# machine's memory: two at once, never three). A slot is a lock file held with flock(1).
#
#   tools/heavy.sh [--label TEXT] [--] CMD [ARGS]   run CMD in a slot; waits while all slots are taken
#   tools/heavy.sh --hold TEXT                      take a slot, print "SLOT n WAITED s", keep it until stdin closes
#                                                   (tools/heavy.ps1: Windows jobs hold a slot through WSL)
#   tools/heavy.sh --status                         the slots and who holds them; exit status = slots in use
#
# Inside a slot $HAETTY_HEAVY_SLOT is set, and a nested heavy.sh (run-all.sh → xvfb.sh, c.sh → gate.sh, a Windows
# job's WSL build through WSLENV) runs its command directly: one job, one slot, no deadlock.
# Every start and end goes to $HAETTY_HEAVY_DIR/heavy.log (default /tmp/haetty-heavy): time, slot, seconds
# waited for it, label; a job that has to wait says so on stderr. The command does not inherit the lock: a
# daemon it leaves behind cannot keep a slot. If heavy.sh itself is killed, its slot is free again at once.
set -u
dir=${HAETTY_HEAVY_DIR:-/tmp/haetty-heavy} slots=${HAETTY_HEAVY_SLOTS:-2}
mode=run label=
while [ $# -gt 0 ]; do
  case "$1" in
    --label) label=$2; shift 2 ;;
    --hold) mode=hold label=$2; shift 2 ;;
    --status) mode=status; shift ;;
    --) shift; break ;;
    -h|--help) sed -n '2,16p' "$0"; exit 0 ;;
    *) break ;;
  esac
done
[ -n "$label" ] || label="${*:-?}"
label=$(printf '%s' "$label" | tr '\t\n' '  ' | cut -c1-120)
mkdir -p "$dir" || exit 2
log() { printf '%s\t%s\tslot %s\t%s\tpid %s\t%s\n' "$(date +%FT%T)" "$1" "$2" "$3" "$$" "$label" >> "$dir/heavy.log"; }
holders() {
  local i busy=0 who=
  for i in $(seq 1 "$slots"); do
    if ( exec 8>> "$dir/slot$i.lock"; flock -n 8 ); then who="$who slot$i:free"
    else busy=$((busy + 1)); who="$who slot$i:[$(cut -c1-80 "$dir/slot$i.who" 2>/dev/null)]"; fi
  done
  echo "$busy$who"
}
if [ $mode = status ]; then
  h=$(holders); busy=${h%% *}
  echo "heavy slots ($slots, $dir):${h#"$busy"}"
  exit "$busy"
fi
if [ $mode = run ]; then
  [ $# -gt 0 ] || { echo "usage: heavy.sh [--label TEXT] [--] CMD [ARGS] | --hold TEXT | --status" >&2; exit 2; }
  [ -z "${HAETTY_HEAVY_SLOT:-}" ] || exec "$@"
fi
t0=$(date +%s) said=0 got=
while :; do
  for i in $(seq 1 "$slots"); do
    exec 9>> "$dir/slot$i.lock"
    if flock -n 9; then got=$i; break 2; fi
    exec 9>&-
  done
  if [ $said = 0 ]; then
    h=$(holders); echo "heavy: all $slots slots in use (${h#* }); waiting: $label" >&2; said=1
  fi
  sleep 2
done
waited=$(( $(date +%s) - t0 ))
echo "$$ $(date +%T) $label" > "$dir/slot$got.who"
log start "$got" "waited $waited s"
[ $said = 0 ] || echo "heavy: slot $got after $waited s: $label" >&2
export HAETTY_HEAVY_SLOT=$got
if [ $mode = hold ]; then
  echo "SLOT $got WAITED $waited"
  cat > /dev/null
  rc=0
else
  "$@" 9>&-; rc=$?
fi
log end "$got" "exit $rc after $(( $(date +%s) - t0 - waited )) s"
: > "$dir/slot$got.who"
exit $rc
