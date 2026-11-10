# Sourced: the toolchain (tools/machine.sh); working dir = the current stage's worktree ($HAETTY_WORK/current-wt, a
# path to its haetty/ folder) or the main checkout. One cargo target directory for all (the main one), so a worktree
# reuses the built dependencies (libghostty-vt).
. "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/../machine.sh"
export CARGO_TARGET_DIR="$HAETTY_REPO/haetty/target"
HAETTY_PROJ=${HAETTY_PROJ:-$(cat "$HAETTY_WORK/current-wt" 2>/dev/null)}
HAETTY_PROJ=${HAETTY_PROJ:-$HAETTY_REPO/haetty}
export HAETTY_PROJ
cd "$HAETTY_PROJ"
