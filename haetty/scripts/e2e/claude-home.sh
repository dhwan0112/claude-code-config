# Sourced: isolated Claude Code environment (no login, onboarding done, project trusted).
# Strips the parent's Claude Code markers so the child is not treated as a child session.
for v in $(env | grep -oE '^(CLAUDE_CODE_[A-Z_]*|CLAUDECODE|CLAUDE_[A-Z_]*SESSION[A-Z_]*)='); do unset "${v%=}"; done
[ -n "${HAETTY_RUN:-}" ] || { echo "claude-home.sh: no HAETTY_RUN (scripts/test-guard.sh first)" >&2; exit 3; }
export CCHOME=$HAETTY_RUN/cchome
export CCPROJ=$HAETTY_RUN/ccproj
haetty_guard "$CCHOME"; haetty_guard "$CCPROJ"
mkdir -p "$CCHOME/.claude" "$CCPROJ"
python3 -I "$(dirname "${BASH_SOURCE[0]}")/mkcfg.py" "$CCHOME/.claude.json" "$CCPROJ" > /dev/null
cp "$CCHOME/.claude.json" "$CCHOME/.claude/.claude.json"
export HOME="$CCHOME"
export ANTHROPIC_API_KEY=sk-ant-api03-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
export CLAUDE_CODE_NO_FLICKER=1
export CLAUDE=${CLAUDE:-$(command -v claude)}
