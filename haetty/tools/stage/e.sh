# Sourced: toolchain env inside WSL; working dir = the current stage's worktree (~/m4/current-wt, a path to
# its haetty/ folder) or the main checkout. One cargo target directory for all (the main one), so a worktree
# reuses the built dependencies (libghostty-vt).
. "$HOME/.cargo/env"
export PATH="$HOME/.local/bin:$HOME/.cargo/bin:/usr/local/bin:/usr/bin:/bin"
export CARGO_TARGET_DIR="$HOME/projects/claude-code-config/haetty/target"
HAETTY_PROJ=${HAETTY_PROJ:-$(cat "$HOME/m4/current-wt" 2>/dev/null)}
HAETTY_PROJ=${HAETTY_PROJ:-$HOME/projects/claude-code-config/haetty}
export HAETTY_PROJ
cd "$HAETTY_PROJ"
