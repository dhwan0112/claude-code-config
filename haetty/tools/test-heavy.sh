#!/usr/bin/env bash
# Self-test of tools/heavy.sh (slots in a private directory of the test run, not the real /tmp/haetty-heavy):
# with two slots a third job waits until one ends, and the log says how long; a nested call takes no slot; a
# child left running by a job keeps no slot; a killed heavy.sh frees its slot at once; --status counts.
#   HAETTY_TEST_TMP=/tmp/haetty-test tools/test-heavy.sh
here=$(cd "$(dirname "$0")" && pwd)
. "$here/../scripts/test-guard.sh"; haetty_guard_init HAETTY_TEST_TMP || exit 3
H=$here/heavy.sh
export HAETTY_HEAVY_DIR=$HAETTY_RUN/heavy HAETTY_HEAVY_SLOTS=2
unset HAETTY_HEAVY_SLOT
pass=0 fail=0
ok() { if [ "$1" = 1 ]; then pass=$((pass + 1)); echo "PASS $2"; else fail=$((fail + 1)); echo "FAIL $2"; fi; }
t() { [ "$@" ] && echo 1 || echo 0; }
waited() { grep -P "\tstart\t.*\t$1\$" "$HAETTY_HEAVY_DIR/heavy.log" | tail -1 | grep -oE 'waited [0-9]+' | grep -oE '[0-9]+'; }

# 1. Two jobs hold both slots for 12 s; a third waits for one of them.
"$H" --label one -- sleep 12 & A=$!
"$H" --label two -- sleep 12 & B=$!
sleep 1.5
"$H" --status > /dev/null; ok "$(t $? = 2)" "status: 2 slots in use while two jobs run"
t0=$(date +%s)
"$H" --label three -- true 2> "$HAETTY_RUN/three.err"
el=$(( $(date +%s) - t0 ))
w=$(waited three)
ok "$(t "$el" -ge 8)" "third job started only after a slot was free (${el} s later)"
ok "$(t "${w:-0}" -ge 8)" "log: third job waited ${w:-?} s"
c=$(grep -c 'all 2 slots in use' "$HAETTY_RUN/three.err")
ok "$(t "$c" = 1)" "third job said it was waiting: $(head -1 "$HAETTY_RUN/three.err" | cut -c1-90)"
ok "$(t "$(waited one)" -le 1 -a "$(waited two)" -le 1)" "first two jobs did not wait"
wait $A $B

# 2. Nested: a job inside a slot calls heavy.sh again — runs at once, no new slot.
n0=$(grep -c $'\tstart\t' "$HAETTY_HEAVY_DIR/heavy.log")
out=$("$H" --label outer -- bash -c "\"$H\" --label inner -- bash -c 'echo slot=\$HAETTY_HEAVY_SLOT'")
n1=$(grep -c $'\tstart\t' "$HAETTY_HEAVY_DIR/heavy.log")
ok "$(t "$n1" = $((n0 + 1)) -a "${out#slot=}" != "")" "nested call ran inside the outer slot ($out, $((n1 - n0)) slot taken)"

# 3. A child left running by a job does not keep the slot.
"$H" --label leaver -- bash -c "sleep 30 > /dev/null 2>&1 & echo \$! > '$HAETTY_RUN/leaver.pid'"
"$H" --status > /dev/null; ok "$(t $? = 0)" "a job's leftover child holds no slot"
kill "$(cat "$HAETTY_RUN/leaver.pid")" 2> /dev/null

# 4. A heavy.sh killed with SIGKILL frees its slot.
"$H" --label killed -- sleep 60 & K=$!
sleep 1; "$H" --status > /dev/null; ok "$(t $? = 1)" "status: 1 slot in use"
C=$(pgrep -P $K)   # its sleep: stopped by PID afterwards
kill -9 $K; sleep 0.5
"$H" --status > /dev/null; ok "$(t $? = 0)" "slot free right after its heavy.sh was killed"
[ -n "$C" ] && kill $C 2> /dev/null

# 5. --hold (heavy.ps1's way): the slot is held until stdin closes.
exec 7> >("$H" --hold holder > "$HAETTY_RUN/hold.out")
sleep 1; "$H" --status > /dev/null; ok "$(t $? = 1)" "hold: slot in use ($(cat "$HAETTY_RUN/hold.out"))"
exec 7>&-; sleep 1
"$H" --status > /dev/null; ok "$(t $? = 0)" "hold: slot free once stdin closed"
echo "TOTAL heavy: $pass passed, $fail failed"
[ $fail = 0 ]
