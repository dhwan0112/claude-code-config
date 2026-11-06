#!/usr/bin/env bash
# Records Claude Code's terminal output (its TUI only: nothing is submitted, no API call) for the VT fuzz corpus.
# Isolation: HOME, CLAUDE_CONFIG_DIR and the project in $HAETTY_RUN; a fake key; CLAUDE_CODE_* etc. removed.
. "$(dirname "$0")/lib.sh"
CLAUDE=$(readlink -f "${CLAUDE:-$(command -v claude)}")
[ -x "$CLAUDE" ] || { echo "record-claude.sh: no claude (set CLAUDE)"; exit 77; }
OUT=$1
W=$HAETTY_RUN/rec; haetty_guard $W; H=$W/home; P=$W/project; mkdir -p $H/.claude $P
cat > $H/.claude.json <<EOF
{"hasCompletedOnboarding":true,"theme":"dark","customApiKeyResponses":{"approved":["$(printf 'A%.0s' $(seq 1 20))"],"rejected":[]},
 "projects":{"$P":{"hasTrustDialogAccepted":true,"hasCompletedProjectOnboarding":true}}}
EOF
cp $H/.claude.json $H/.claude/.claude.json
for v in $(env | grep -oE '^(CLAUDE_CODE_[A-Z_]*|CLAUDECODE|CLAUDE_[A-Z_]*SESSION[A-Z_]*)=' | tr -d =); do unset $v; done
KEY="sk-ant-api03-$(printf 'A%.0s' $(seq 1 95))"
cd $P
for tui in fullscreen default; do
  rm -f $W/raw-$tui.log
  env HOME=$H CLAUDE_CONFIG_DIR=$H/.claude ANTHROPIC_API_KEY=$KEY $( [ $tui = fullscreen ] && echo CLAUDE_CODE_NO_FLICKER=1 ) \
    $HL --cols 120 --rows 36 --timeout 90 --raw-log $W/raw-$tui.log --cwd $P \
    -e "wait-idle 3000 60000" -e "type 안녕 hello world" -e "key shift+Enter" -e "type second line with 한글 and emoji 🎉" \
    -e "wait-idle 800 5000" -e "type /" -e "wait-idle 800 5000" -e "key Escape" -e "key ctrl+u" -e "type ?" -e "wait-idle 800 5000" \
    -e "key Escape" -e "key ctrl+c" -e "key ctrl+c" -e "wait-exit 15000" --dump none -- $CLAUDE > /dev/null 2>&1
  echo "$tui: $(stat -c %s $W/raw-$tui.log 2>/dev/null) bytes"
  cp $W/raw-$tui.log $OUT/claude-$tui.raw 2>/dev/null
done
# Never keep the fake key or the run path in the corpus (both are replaced by same-length filler).
for f in $OUT/claude-*.raw; do
  python3 -I - "$f" "$KEY" "$W" <<'PY'
import sys
p, key, run = sys.argv[1], sys.argv[2].encode(), sys.argv[3].encode()
b = open(p, "rb").read()
n = b.count(key) + b.count(run)
b = b.replace(key, b"K" * len(key)).replace(run, b"/" + b"r" * (len(run) - 1))
open(p, "wb").write(b)
print(f"{p}: {len(b)} bytes, {n} replacements")
PY
done
