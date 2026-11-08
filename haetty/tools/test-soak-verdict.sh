#!/usr/bin/env bash
# The soak verdicts (scripts/e2e/soak-verdict.py, scripts/e2e/windows/soak-verdict.ps1) against the synthetic series
# of scripts/e2e/soak-cases, which both must judge alike. Linux always; Windows when pwsh is there.
set -u
here=$(cd "$(dirname "$0")" && pwd)
top=$(cd "$here/.." && pwd)
cd "$top" || exit 2
rc=0
python3 -I scripts/e2e/soak-cases/check.py || rc=1
if command -v pwsh > /dev/null; then
  pwsh -NoProfile -ExecutionPolicy Bypass -File scripts/e2e/windows/test-soak-verdict.ps1 || rc=1
else
  echo "pwsh not found: the Windows verdict is not tested here"
fi
exit $rc
