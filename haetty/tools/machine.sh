# Sourced (bash): the values that differ between development machines, in one place. The stage scripts, the gate,
# check-range, the night run and tools/ghostty-ref/build.sh read them from here and nowhere else.
# Order: the environment wins, then tools/machines/<hostname>.env (committed, one file per machine), then the
# defaults below. A machine file sets only what differs, in the same `: "${NAME:=value}"` form.
_hm=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
[ -f "$_hm/machines/$(hostname).env" ] && . "$_hm/machines/$(hostname).env"
: "${HAETTY_REPO:=$HOME/projects/claude-code-config}"      # the main checkout (the repository root, not haetty/)
: "${HAETTY_WTROOT:=$HAETTY_REPO-wt}"                      # stage worktrees: $HAETTY_WTROOT/NAME
: "${HAETTY_WORK:=$HOME/m4}"                               # outside the repository and /tmp: current-wt (the running
                                                           # stage), ghostty-ref/ghostty (the pinned source), vtcorpus
: "${HAETTY_ZIG_BIN:=$HOME/.local/bin}"                    # the folder holding zig 0.15.2 (the gate's native-only
                                                           # step drops exactly this folder from PATH)
: "${HAETTY_ZIG_REF:=$HOME/.local/zig-0.17.0/zig}"         # zig 0.17.0, for tools/ghostty-ref only
: "${HAETTY_DROPBOX=$HOME/Dropbox/claude-code-config}"     # the Dropbox mirror; set empty for a machine without one
: "${HAETTY_NIGHT_DIR:=/tmp/haetty-overnight}"             # the night run's checkout and build (several GB)
export HAETTY_REPO HAETTY_WTROOT HAETTY_WORK HAETTY_ZIG_BIN HAETTY_ZIG_REF HAETTY_DROPBOX HAETTY_NIGHT_DIR
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
case ":$PATH:" in *":$HAETTY_ZIG_BIN:"*) ;; *) PATH=$HAETTY_ZIG_BIN:$PATH ;; esac
export PATH
unset _hm
