# usage: c.sh MSGFILE — gate, then commit only the haetty/ tree (no push). The gate is the repository's
# scripts/gate.sh when the checkout has it (stage T2 on), else the session's g.sh.
D="$(dirname "$0")"
. "$D/e.sh"
if [ -f scripts/gate.sh ]; then out=$(bash scripts/gate.sh 2>&1); else out=$(bash "$D/g.sh" 2>&1); fi
echo "$out" | grep -v "^test result: ok. 0 passed" | tail -24
echo "$out" | grep -q "GATE fail=0" || { echo "NOT COMMITTED"; exit 1; }
git add -A . && git commit -q -F "$1" && git log --format='%h %an <%ae> %s' -1
