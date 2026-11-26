#!/usr/bin/env bash
# The scripts README.md and docs/SELF-HOSTING.md tell the person to run (tools/NAME.sh) are executable in git
# (mode 100755): a script the person runs by its path must not fail with "permission denied". Prints each one that is
# not and exits 1; prints "exec bits: N scripts, all executable" otherwise. usage: tools/check-exec.sh
cd "$(dirname "$0")/.." || exit 2
bad=0 n=0
for f in $(grep -ohE '\btools/[A-Za-z0-9_-]+\.sh\b' README.md docs/SELF-HOSTING.md | sort -u); do
  [ -f "$f" ] || continue
  n=$((n + 1))
  mode=$(git ls-files -s -- "$f" | cut -d' ' -f1)
  if [ "$mode" != 100755 ]; then
    echo "not executable in git: $f (mode ${mode:-untracked}); fix: git update-index --chmod=+x $f"
    bad=1
  fi
done
[ $bad = 0 ] && echo "exec bits: $n scripts, all executable"
exit $bad
