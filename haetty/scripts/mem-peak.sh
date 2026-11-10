#!/usr/bin/env bash
# usage: scripts/mem-peak.sh LABEL CMD [ARGS] — runs CMD (in a heavy-job slot) and reports its memory peaks:
#   tree    the summed RSS of CMD and all its descendants (the job itself; shared pages counted per process,
#           so an upper bound)
#   system  MemTotal - MemAvailable (what cannot be given back: page cache does not count), its value
#           before the job and its peak during it
# sampled every second; one line "MEMPEAK LABEL: …" at the end, and the samples in $HAETTY_MEMPEAK_LOG if set.
[ -n "${HAETTY_HEAVY_SLOT:-}" ] || exec "$(dirname "$0")/../tools/heavy.sh" --label "mem-peak.sh $1" -- bash "$0" "$@"   # at most two heavy jobs
set -u
label=$1; shift
used() { awk '/^MemTotal:/ { t = $2 } /^MemAvailable:/ { a = $2 } END { print int((t - a) / 1024) }' /proc/meminfo; }
tree_rss() {   # kB summed over $1 and its descendants
  ps -eo pid=,ppid=,rss= | awk -v root="$1" '
    { p[$1] = $2; r[$1] = $3 }
    END { for (x in p) { y = x; while (y != "" && y != root && y > 1) y = p[y]; if (y == root) s += r[x] }; print int(s / 1024) }'
}
base=$(used) t0=$(date +%s)
"$@" & job=$!
peak_tree=0 peak_sys=$base
while kill -0 $job 2> /dev/null; do
  tr=$(tree_rss $job); sy=$(used)
  [ "$tr" -gt "$peak_tree" ] && peak_tree=$tr
  [ "$sy" -gt "$peak_sys" ] && peak_sys=$sy
  [ -n "${HAETTY_MEMPEAK_LOG:-}" ] && echo "$(date +%T) $label tree=${tr}MB system=${sy}MB" >> "$HAETTY_MEMPEAK_LOG"
  sleep 1
done
wait $job; rc=$?
echo "MEMPEAK $label: tree peak ${peak_tree} MB; system ${base} -> peak ${peak_sys} MB (+$((peak_sys - base))); $(( $(date +%s) - t0 )) s; exit $rc"
exit $rc
