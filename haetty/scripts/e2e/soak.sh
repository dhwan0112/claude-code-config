#!/usr/bin/env bash
# Soak v2 (T4), Linux, inside xvfb.sh: one GUI window for DUR seconds (default 3600) with four tabs — a burst of 500
# coloured lines every 10 s, a line every 0.2 s, two idle — and, spread over the run, the operations a person does:
#   200 x open a tab and close it (Ctrl+Shift+T, Ctrl+Shift+W twice)
#    50 x detach a tab and attach it again (palette)
#    20 x make a task (palette "New task"), let its worker end, close its tab, remove its worktree (palette)
#    10 x change the theme (config file + palette "Reload config")
# Every 300 s, between operations (no tab of an operation open): the GUI's and the daemon's threads, file
# descriptors and anonymous memory, the daemon's descendant processes, the sessions, the task worktrees left.
#   HAETTY_TEST_TMP=/tmp/haetty-test scripts/e2e/xvfb.sh bash scripts/e2e/soak.sh OUT.csv [DUR]
# Verdict (written before the first run; "no growth"):
#   threads, fds: the last sample equals the first one (t=0, the four tabs open), GUI and daemon (± 0);
#   descendants of the daemon and sessions: the last sample equals the first one; task worktrees: 0 at the end;
#   anonymous memory: GUI last <= GUI at 15 min + 4 MB; daemon last <= daemon when the scrollback is full + 4 MB.
#   "Full" (E5c): the first sample at which the largest session scrollback (`status --json` "scrollback", the
#   burst tab's) has stopped growing (no more than 1 % over the sample before); there must be at least 15 min of
#   samples after it, else the check FAILS (not judged is not passed). How long filling takes depends on the
#   engine (libghostty ~40 min; haetty's engine keeps more rows in the same bytes). A build without the field
#   (protocol < 11) is judged at 45 min, as before E5c, and says so. The slow tab keeps adding ~30 KB a minute,
#   within its limit;
#   and every operation done (counts printed), the window and the daemon alive throughout.
. "$(dirname "$0")/lib.sh"
OUT=$1; DUR=${2:-3600}
W=$HAETTY_RUN/soak; haetty_guard $W; mkdir -p $W/rt $W/state/tabs $W/cfg $W/bin $W/claude; chmod 700 $W/rt
export HAETTY_INSTANCE=dev HAETTY_SOCKET=$W/rt/d.sock HAETTY_STATE_DIR=$W/state HAETTY_CONFIG_DIR=$W/cfg
export HAETTY_LOG=info HAETTY_LOG_FILE=$W/gui.log CLAUDE_CONFIG_DIR=$W/claude
export GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null
R=$W/proj; mkdir -p $R; G="git -C $R -c user.name=t -c user.email=t@example.invalid -c commit.gpgsign=false"
$G init -q -b main; echo one > $R/a.txt; $G add .; $G commit -qm one
cat > $W/tab.sh <<'EOF'
#!/bin/sh
case $1 in
0) while :; do i=0; while [ $i -lt 500 ]; do printf '\033[3%dm burst line %d %s\033[0m\n' $((i % 8)) $i "$(date +%s%N)"; i=$((i+1)); done; sleep 10; done;;
1) while :; do echo "tick $(date +%T.%N)"; sleep 0.2; done;;
*) exec sleep 100000;;
esac
EOF
chmod +x $W/tab.sh
# The worker: Claude Code's name, prints a line and ends after 3 s.
printf '#!/bin/sh\necho "worker in $PWD"\nsleep 3\n' > $W/bin/claude; chmod +x $W/bin/claude
theme() { printf 'theme = %s\nworker-command = %s\n' "$1" "$W/bin/claude" > $W/cfg/config; }
theme "IBM 5153 CGA"
for m in 0 1 2; do printf 'tab%s\t%s\t/bin/sh\t%s\x1f%s\t\t0\t0\n' $m $R $W/tab.sh $m; done > $W/state/tabs/last.tsv
cd $R
setsid $BIN --cols 120 --rows 36 $W/tab.sh 3 > /dev/null 2>&1 < /dev/null & GP=$!
GW=$(timeout 15 xdotool search --sync --onlyvisible --pid $GP | head -1); sleep 3
J() { HAETTY_LOG=warn $BIN status --json; }
DP=$(J | python3 -Ic 'import json,sys; print(json.load(sys.stdin)["daemon_pid"])')
focus() { for i in 1 2 3 4 5; do xdotool windowfocus --sync $GW 2>/dev/null; [ "$(xdotool getwindowfocus 2>/dev/null)" = "$GW" ] && return; sleep 0.2; done; }
key() { focus; xdotool key --clearmodifiers "$@"; sleep ${KW:-0.5}; }
pal() { key ctrl+shift+p; xdotool type --delay 15 "$1"; sleep 0.2; key Return; }
nsess() { J | python3 -Ic 'import json,sys; print(sum(1 for s in json.load(sys.stdin)["sessions"] if s["state"]=="running"))'; }
desc() { # descendants of the daemon
  local n=0 todo=$1 p c
  while [ -n "$todo" ]; do set -- $todo; p=$1; shift; todo="$*"
    for c in $(cat /proc/$p/task/*/children 2>/dev/null); do n=$((n+1)); todo="$todo $c"; done
  done; echo $n; }
m() { echo "$(ls /proc/$1/task | wc -l),$(ls /proc/$1/fd | wc -l),$(awk '/^RssAnon/{print $2}' /proc/$1/status)"; }
wts() { echo $(( $($G worktree list | wc -l) - 1 + $(ls $W/proj-tasks 2>/dev/null | wc -l) )); }
# The largest scrollback of a session in rows (-1: the build does not report it).
sbmax() { J | python3 -Ic 'import json,sys; v=[s["scrollback"] for s in json.load(sys.stdin)["sessions"] if "scrollback" in s]; print(max(v) if v else -1)'; }
echo "t_s,gui_threads,gui_fds,gui_anon_kb,daemon_threads,daemon_fds,daemon_anon_kb,daemon_descendants,sessions,worktrees,tabs_done,detach_done,tasks_done,themes_done,scrollback_rows" > $OUT
sample() { echo "$1,$(m $GP),$(m $DP),$(desc $DP),$(nsess),$(wts),$tabs,$dets,$tasks,$themes,$(sbmax)" >> $OUT; }
tabs=0 dets=0 tasks=0 themes=0 fails=0
op_tab() { key ctrl+shift+t; sleep 0.8; KW=0.3 key ctrl+shift+w; KW=0.8 key ctrl+shift+w; tabs=$((tabs+1)); }
op_detach() { key alt+4; pal "detach"; sleep 0.5; pal "attach detached"; sleep 0.8; dets=$((dets+1)); }
op_task() {
  key alt+1; pal "new task"; xdotool type --delay 15 "soak task $tasks"; key Return; xdotool type "d"; key Return; KW=2 key Return
  sleep 4   # the worker ends; its tab stays, marked exited
  key ctrl+shift+w; sleep 0.5; pal "remove a task"; key Return; sleep 1.5   # the list, then the task
  [ "$(wts)" = 0 ] || { fails=$((fails+1)); echo "task $tasks: $(wts) worktree(s) left" >&2; }
  tasks=$((tasks+1)); }
op_theme() { if [ $((themes % 2)) = 0 ]; then theme "haetty classic"; else theme "IBM 5153 CGA"; fi; pal "reload config"; themes=$((themes+1)); }
# 280 operations in a fixed interleaving, evenly over the run, a sample every 300 s.
# A shorter DUR (a trial) scales the counts down. The operations take ~910 s of the hour themselves.
plan=$(python3 -Ic "import random; r=random.Random(7); f=min(1.0, $DUR/3600); n=lambda c: max(1, round(c*f)); ops=['tab']*n(200)+['detach']*n(50)+['task']*n(20)+['theme']*n(10); r.shuffle(ops); print(' '.join(ops))")
nops=$(echo $plan | wc -w)
gap=$(python3 -Ic "print(max(0.0, ($DUR - 120 - 910 * $nops / 280) / $nops))")
t0=$(date +%s); next=0
sample 0; next=300
for op in $plan; do
  now=$(( $(date +%s) - t0 ))
  if [ $now -ge $next ]; then sample $now; next=$((next + 300)); fi
  kill -0 $GP 2>/dev/null && kill -0 $DP 2>/dev/null || { echo "DIED at $now s" >&2; break; }
  op_$op
  sleep $gap
done
while [ $(( $(date +%s) - t0 )) -lt $DUR ]; do
  now=$(( $(date +%s) - t0 )); if [ $now -ge $next ]; then sample $now; next=$((next + 300)); fi; sleep 5
done
sleep 3; sample $(( $(date +%s) - t0 ))
alive=$(kill -0 $GP 2>/dev/null && kill -0 $DP 2>/dev/null && echo yes || echo no)
kill $GP 2>/dev/null; wait $GP 2>/dev/null; haetty_stop_daemon "$BIN"
echo "operations: tabs $tabs/200, detach $dets/50, tasks $tasks/20, themes $themes/10; task cleanup failures $fails; alive at the end: $alive"
python3 -I - "$OUT" <<'PY'
import csv, sys
rows = [{k: int(v) for k, v in r.items()} for r in csv.DictReader(open(sys.argv[1]))]
first, last = rows[0], rows[-1]
def at(t): return min(rows, key=lambda r: abs(r["t_s"] - t))
ok = True
def check(name, cond, detail):
    global ok; ok &= cond; print(("PASS " if cond else "FAIL ") + name + ": " + detail)
for k in ["gui_threads", "gui_fds", "daemon_threads", "daemon_fds", "daemon_descendants", "sessions"]:
    check(k, last[k] == first[k], f"{first[k]} -> {last[k]}")
check("worktrees", last["worktrees"] == 0, str(last["worktrees"]))
g15 = at(900)
check("gui_anon", last["gui_anon_kb"] <= g15["gui_anon_kb"] + 4096, f"{g15['gui_anon_kb']} kB at {g15['t_s']} s -> {last['gui_anon_kb']} kB")
sb = [r.get("scrollback_rows", -1) for r in rows]
if min(sb) < 0:
    d = at(2700)
    check("daemon_anon", last["daemon_anon_kb"] <= d["daemon_anon_kb"] + 4096, f"{d['daemon_anon_kb']} kB at {d['t_s']} s -> {last['daemon_anon_kb']} kB (at 45 min: this build reports no scrollback)")
else:
    full = next((r for p, r in zip(rows, rows[1:]) if r["scrollback_rows"] > 0 and r["scrollback_rows"] <= p["scrollback_rows"] * 1.01), None)
    if full is None or last["t_s"] - full["t_s"] < 900:
        when = "never" if full is None else f"only at {full['t_s']} s"
        check("daemon_anon", False, f"scrollback full {when} ({sb[-1]} rows at the end): less than 15 min after it to judge")
    else:
        check("daemon_anon", last["daemon_anon_kb"] <= full["daemon_anon_kb"] + 4096, f"{full['daemon_anon_kb']} kB when the scrollback was full ({full['scrollback_rows']} rows, {full['t_s']} s) -> {last['daemon_anon_kb']} kB")
print("VERDICT", "PASS" if ok else "FAIL")
PY
