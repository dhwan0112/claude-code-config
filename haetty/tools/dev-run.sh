#!/usr/bin/env bash
# Builds this working tree and starts it as the dev instance (HAETTY_INSTANCE=dev) beside the stable one
# (docs/SELF-HOSTING.md). Linux; Windows: tools/dev-run.ps1.
#   tools/dev-run.sh [--debug] [--no-build] [--check] [-- HAETTY-ARGS…]
#     --debug     the dev profile (faster to build) instead of release
#     --no-build  start the last build
#     --check     print and check everything, start nothing
# Every run prints what the dev instance uses next to the stable one — socket, state, config and runtime
# directories, app id, window title prefix — and refuses to start when anything would be shared: an override
# (HAETTY_SOCKET / _STATE_DIR / _CONFIG_DIR / _LOG_FILE, or their old CCTERM_* names) or a path in the dev
# config that points at the stable instance, or a binary that is the installed stable one.
# The build runs in a heavy-job slot (tools/heavy.sh); the window is started detached, its log in the dev state
# directory. Never touches the stable daemon.
set -u
here=$(cd "$(dirname "$0")" && pwd)
proj=$(cd "$here/.." && pwd)
profile=release build=1 check=0
while [ $# -gt 0 ]; do
  case "$1" in
    --debug) profile=debug; shift ;;
    --no-build) build=0; shift ;;
    --check) check=1; shift ;;
    --) shift; break ;;
    -h|--help) sed -n '2,14p' "$0"; exit 0 ;;
    *) break ;;
  esac
done
die() { echo "dev-run: REFUSED: $*" >&2; exit 3; }
# Parent markers out: a dev window started from a stable tab or a Claude Code session is not their child.
for v in $(env | grep -oE '^(HAETTY_SESSION_ID|CCTERM_SESSION_ID|CLAUDE_CODE_[A-Z_]*|CLAUDECODE|CLAUDE_[A-Z_]*SESSION[A-Z_]*)='); do unset "${v%=}"; done
unset CCTERM_INSTANCE
export HAETTY_INSTANCE=dev

uid=$(id -u)
rt_of() { if [ -n "${XDG_RUNTIME_DIR:-}" ]; then echo "$XDG_RUNTIME_DIR/$1"; else echo "${TMPDIR:-/tmp}/$1-$uid"; fi; }
st_base=${XDG_STATE_HOME:-$HOME/.local/state} cf_base=${XDG_CONFIG_HOME:-$HOME/.config}
# The stable instance (and the former name's directories) — nothing of the dev instance may point here.
stable=()
for s in "$(rt_of haetty)" "$st_base/haetty" "$cf_base/haetty" "$(rt_of ccterm)" "$st_base/ccterm" "$cf_base/ccterm"          "$HOME/.local/opt/haetty" "$HOME/.local/bin/haetty"; do
  stable+=("$s"); r=$(realpath -m "$s"); [ "$r" = "$s" ] || stable+=("$r")
done
inside() { local p=${1%/} s; for s in "${stable[@]}"; do case "$p" in "$s"|"$s"/*) return 0 ;; esac; done; return 1; }
for n in SOCKET STATE_DIR CONFIG_DIR LOG_FILE; do
  for pre in HAETTY_ CCTERM_; do
    v=$(printenv "$pre$n") || continue
    [ -n "$v" ] || continue
    inside "$(realpath -m "$v")" && die "$pre$n=$v points at the stable instance"
  done
done
sock=${HAETTY_SOCKET:-${CCTERM_SOCKET:-$(rt_of haetty-dev)/daemon.sock}}
state=${HAETTY_STATE_DIR:-${CCTERM_STATE_DIR:-$st_base/haetty-dev}}
cfg=${HAETTY_CONFIG_DIR:-${CCTERM_CONFIG_DIR:-$cf_base/haetty-dev}}
if [ -f "$cfg/config" ]; then
  for s in "${stable[@]}"; do
    grep -nF -- "$s" "$cfg/config" | grep -vE '^[0-9]+:\s*#' | head -1 | grep -q . && die "$cfg/config names $s (the stable instance)"
  done
fi
target=${CARGO_TARGET_DIR:-$proj/target}
bin=$target/$profile/haetty
inside "$(realpath -m "$bin")" && die "the binary $bin is the installed stable one"

if [ $build = 1 ] && [ $check = 0 ]; then
  args=(-q -p haetty-gui); [ $profile = release ] && args+=(--release)
  (cd "$proj" && "$here/heavy.sh" --label "dev-run build" -- cargo build "${args[@]}") || exit 2
fi
[ -x "$bin" ] || die "no build at $bin (run without --no-build)"
ver=$("$bin" --version)
case "$ver" in *"instance dev"*) ;; *) die "the build does not report the dev instance: $ver" ;; esac
printf '%-18s %-58s %s\n' "" "dev (this run)" "stable (untouched)" \
  "socket" "$sock" "$(rt_of haetty)/daemon.sock" \
  "state" "$state" "$st_base/haetty" \
  "config" "$cfg" "$cf_base/haetty" \
  "app id" "io.github.dhwan0112.haetty.dev" "io.github.dhwan0112.haetty" \
  "window title" "\"[dev] …\"" "\"…\"" \
  "binary" "$bin" "$(command -v haetty 2> /dev/null || echo -)"
echo "version: $ver"
[ "$sock" != "$(rt_of haetty)/daemon.sock" ] || die "same socket as the stable instance"
[ $check = 1 ] && { echo "dev-run: checks passed (--check: not started)"; exit 0; }
mkdir -p "$state"
setsid "$bin" "$@" > "$state/dev-run.log" 2>&1 < /dev/null &
pid=$!
for i in $(seq 1 40); do [ -S "$sock" ] && break; sleep 0.25; done
[ -S "$sock" ] && echo "started: dev window pid $pid, dev daemon on $sock (log $state/dev-run.log)" \
  || { echo "dev-run: the dev daemon socket did not appear; see $state/dev-run.log" >&2; exit 4; }
