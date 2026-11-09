cd ~/projects/claude-code-config
echo "attribution: $(git log origin/main..HEAD --format=%B | grep -ci 'co-authored\|claude-session\|generated with')"
# CLAUDE.md: every new commit passes the quick gate on its own before a push.
. ~/.cargo/env
cr=$(cd haetty && HAETTY_TEST_TMP=/tmp/haetty-test tools/check-range.sh origin/main..HEAD 2>&1); rc=$?
echo "$cr" | grep -E "FAIL|KNOWN|NOTE|TOTAL|no commit"
[ $rc = 0 ] || { echo "NOT PUSHED: check-range found failing commits"; exit 1; }
GIT_TERMINAL_PROMPT=0 timeout 120 git push -q origin main 2>&1 | tail -2
git status -sb | head -1
CCC_DROPBOX=/mnt/c/Users/denny/Dropbox/claude-code-config bash tools/mirror-dropbox.sh
