. "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/../machine.sh"
cd "$HAETTY_REPO"
echo "attribution: $(git log origin/main..HEAD --format=%B | grep -ci 'co-authored\|claude-session\|generated with')"
# CLAUDE.md: every new commit passes the quick gate on its own before a push.
cr=$(cd haetty && HAETTY_TEST_TMP=/tmp/haetty-test tools/check-range.sh origin/main..HEAD 2>&1); rc=$?
echo "$cr" | grep -E "FAIL|KNOWN|NOTE|TOTAL|no commit"
[ $rc = 0 ] || { echo "NOT PUSHED: check-range found failing commits"; exit 1; }
GIT_TERMINAL_PROMPT=0 timeout 120 git push -q origin main 2>&1 | tail -2
git status -sb | head -1
# The mirror copies the working tree: only what is committed goes (a file changed outside haetty/ would go too).
if [ -z "$HAETTY_DROPBOX" ]; then echo "no Dropbox mirror on this machine"
elif [ -n "$(git status --porcelain)" ]; then echo "NOT MIRRORED: the main checkout has uncommitted changes"; git status --porcelain | head -5
else CCC_DROPBOX=$HAETTY_DROPBOX bash tools/mirror-dropbox.sh; fi
