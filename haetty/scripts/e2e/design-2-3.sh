#!/usr/bin/env bash
# Inside xvfb.sh: DESIGN.md §2.3 checks with a fake `claude` (prints nothing, so every non-background pixel is ccterm's
# own chrome or the cursor) and fake hook / status-line JSON through the real `ccterm hook` / `ccterm statusline`.
# Test isolation: everything below writes only in $HAETTY_RUN (scripts/test-guard.sh).
. "$(dirname "$0")/lib.sh"
OUT=$HAETTY_RUN/p5; haetty_guard $OUT; mkdir -p $OUT
W=$HAETTY_RUN/e2e3-w; haetty_guard $W; mkdir -p $W/rt $W/xrt $W/bin; chmod 700 $W/rt $W/xrt
export HAETTY_SOCKET=$W/rt/d.sock HAETTY_STATE_DIR=$W/state XDG_RUNTIME_DIR=$W/xrt HAETTY_LOG=debug
printf '#!/bin/sh\nprintf "\\033[?25l"\nexec sleep 600\n' > $W/bin/claude; chmod +x $W/bin/claude
pass=0; fail=0
ok() { echo "PASS $1"; pass=$((pass+1)); }
bad() { echo "FAIL $1 :: $2"; fail=$((fail+1)); }
shot() { sleep 0.8; cp $W/f.ppm $OUT/$1.ppm; pnmtopng $OUT/$1.ppm > $OUT/$1.png; }
# Pixel analysis: background = most common colour; report non-grey pixels (|r-g|,|g-b| > 12) and the rows that
# differ from the background.
an() { python3 -I - "$OUT/$1.ppm" <<'PY'
import sys, collections
d = open(sys.argv[1], 'rb').read()
parts = d.split(b'\n', 3); w, h = map(int, parts[1].split()); px = parts[3]
cnt = collections.Counter(px[i:i+3] for i in range(0, len(px), 3))
bg = cnt.most_common(1)[0][0]
color = 0; rows = set()
for y in range(h):
    for x in range(w):
        p = px[(y*w+x)*3:(y*w+x)*3+3]
        if p != bg:
            rows.add(y)
            r, g, b = p
            if max(abs(r-g), abs(g-b), abs(r-b)) > 12: color += 1
bands = []
for y in sorted(rows):
    if bands and y == bands[-1][1] + 1: bands[-1][1] = y
    else: bands.append([y, y])
print(f"{w}x{h} nongrey={color} bands={bands}")
PY
}
sizes() { $BIN status --json | python3 -c 'import json,sys; print(" ".join(f"{s["cols"]}x{s["rows"]}" for s in json.load(sys.stdin)["sessions"]))'; }
sid() { $BIN status --json | python3 -c "import json,sys; print(json.load(sys.stdin)['sessions'][$1]['id'])"; }
sl() { echo "$2" | HAETTY_SESSION_ID=$1 $BIN statusline; }
hk() { echo "$3" | HAETTY_SESSION_ID=$1 $BIN hook $2; }

# (c) One tab: only the terminal and the status row.
HAETTY_DUMP_FRAME=$W/f.ppm HAETTY_LOG_FILE=$OUT/gui.log setsid $BIN --cols 80 --rows 20 --restore-tabs false ${E2E_ARGS:-} $W/bin/claude > /dev/null 2>&1 < /dev/null & P=$!
WW=$(timeout 10 xdotool search --sync --onlyvisible --pid $P | head -1); sleep 2
A=$(sid 0)
sl $A '{"session_id":"s1","context_window":{"used_percentage":12,"context_window_size":200000},"rate_limits":{"five_hour":{"used_percentage":20}},"cost":{"total_cost_usd":0.42}}'
shot c-one-tab; R=$(an c-one-tab); echo "  one tab: $R"
nb=$(echo "$R" | grep -o 'bands=.*' | grep -o '\[[0-9]*, [0-9]*\]' | wc -l)
[ "$nb" = "${E2E_BANDS:-1}" ] && ok "one tab: ${E2E_BANDS:-1} band(s) besides the background (status row; with integrated also the title row)" || bad "one tab" "$R"
echo "$R" | grep -q "nongrey=0 " && ok "idle, one tab: no accent colour" || bad "accent, one tab" "$R"

# (b) Idle with two tabs, no attention: no accent colour.
xdotool windowfocus --sync $WW; sleep 0.3; xdotool key ctrl+shift+t; sleep 2
B=$(sid 1)
xdotool key alt+1; sleep 0.5
shot b-two-tabs-idle; R=$(an b-two-tabs-idle); echo "  two tabs idle: $R"
echo "$R" | grep -q "nongrey=0 " && ok "idle, two tabs: no accent colour" || bad "accent, two tabs" "$R"

# (a) Events without input: hooks, status-line changes, a permission question, Stop on the background tab.
S0=$(sizes); L0=$(grep -c "resize\|relayout" $OUT/gui.log)
sl $B '{"session_id":"s2","context_window":{"used_percentage":50}}'
hk $B PermissionRequest '{"session_id":"s2","tool_name":"Bash"}'; sleep 0.5
shot a-ask-mark; R=$(an a-ask-mark); echo "  ask on tab 2 (an event, not idle): $R"
hk $B PostToolUse '{"session_id":"s2","tool_name":"Bash"}'; hk $B Stop '{"session_id":"s2"}'
sl $A '{"session_id":"s1","context_window":{"used_percentage":96}}'; sleep 0.5
shot a-events; R2=$(an a-events); echo "  after events (tab 1 at 96 %: red expected): $R2"
hk $A SubagentStart '{"session_id":"s1","agent_id":"x","agent_type":"Explore"}'; hk $A Stop '{"session_id":"s1"}'; sleep 0.5
S1=$(sizes); L1=$(grep -c "resize\|relayout" $OUT/gui.log)
echo "  session grids before/after events: [$S0] [$S1]; resize log lines $L0 -> $L1"
[ "$S0" = "$S1" ] && [ "$L0" = "$L1" ] && ok "events changed no layout" || bad "layout" "[$S0] [$S1] $L0 $L1"
echo "$R" | grep -qv "nongrey=0 " && ok "the question mark is coloured (check sees colour)" || bad "check sensitivity" "$R"
kill $P; wait $P 2>/dev/null; sleep 0.3
haetty_stop_daemon "$BIN"
echo "TOTAL p5: $pass passed, $fail failed"
