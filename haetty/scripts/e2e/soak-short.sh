#!/usr/bin/env bash
# The short soak (P1c, 2026-11-12): part of the candidate check for a tag (docs/SELF-HOSTING.md, "Dev → stable").
# soak.sh's operations and verdict rule (scripts/e2e/soak-verdict.py) in 600 s instead of 3600 s:
#   HAETTY_TEST_TMP=/tmp/haetty-test [E2E_ENGINE=native …] scripts/e2e/soak-short.sh OUT.csv
# The numbers (measured on kramers 2026-11-12, both engines; docs/history/2026-11-12-p1.md, "P1c"):
# - 600 s: soak.sh scales the hour's counts by 600/3600: 33 tab open/close, 8 detach/attach, 3 tasks, 2 theme
#   changes (46 operations, about 150 s of their own time, about 7 s apart; the last ends about 2 min before the end).
# - The verdict needs at least half the run (300 s) after the latest full time, so every tab's scrollback must be
#   full within about 4 min. The slow tab prints 5 rows/s. SOAK_LIMIT 160 000 bytes holds about 740 of its rows on
#   haetty's engine (about 216 bytes a row): full after about 150–185 s (measured: 155 s). libghostty keeps 350–730
#   rows in page steps at this limit (as at 800 000) and is full in the first minute. The burst tab is full in 30 s.
# - Samples every 30 s: the full time to the half minute, about 20 samples.
# - The rule is the hour's: threads, fds, descendants and sessions unchanged; GUI and daemon anonymous memory at most
#   4 MB above their values at the full time (measured: +0.6 MB native, +1.3 MB libghostty).
here=$(cd "$(dirname "$0")" && pwd)
OUT=${1:?soak-short.sh OUT.csv}
export SOAK_LIMIT=160000 SOAK_SAMPLE_S=30
"$here/xvfb.sh" bash "$here/soak.sh" "$OUT" 600 2>&1 | grep -v -e XGetInputFocus -e 'screen size is bogus'
exit "${PIPESTATUS[0]}"
