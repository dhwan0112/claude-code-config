#!/usr/bin/env bash
# usage (inside xvfb.sh): dump-sample.sh BIN OUTBASE PT — dump one frame of a fixed sample (Linux, X11/Xvfb).
# Test isolation: everything below writes only in $HAETTY_RUN (scripts/test-guard.sh).
. "$(dirname "$0")/lib.sh"
BIN=$1 OUT=$2 PT=$3; haetty_guard "$OUT"
mkdir -p "$(dirname "$OUT")"; rm -f $HAETTY_RUN/dump-$$.ppm "$OUT.log"
HAETTY_LOG=info HAETTY_LOG_FILE=$OUT.log HAETTY_DUMP_FRAME=$HAETTY_RUN/dump-$$.ppm "$BIN" --restore-tabs false --font-size "$PT" --cols 40 --rows 6 \
  sh -c 'printf "%s\n" "한글 입력 테스트 ok" "A한B 水一。，「」ㄱㅏ 뷁" "─│┌┐ ⏸ ⏸️ ⚠ ✔ 😀"; sleep 6' &
P=$!
for i in $(seq 1 30); do [ -s $HAETTY_RUN/dump-$$.ppm ] && break; sleep 0.2; done; sleep 1.5
cp $HAETTY_RUN/dump-$$.ppm "$OUT.ppm"; pnmtopng "$OUT.ppm" > "$OUT.png"; pnmscale 3 "$OUT.ppm" | pnmtopng > "$OUT-x3.png"; rm -f "$OUT.ppm"
kill $P 2>/dev/null; wait $P 2>/dev/null
grep -h "font: primary" "$OUT.log" | sed 's/^/  /'
echo "  -> $OUT.png"
