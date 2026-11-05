# Test isolation for shell scripts (Linux, WSL, Git Bash). Source it, then
#
#   haetty_guard_init HAETTY_TEST_TMP    # the NAME of the variable holding the base directory
#   haetty_guard "$path"                 # before writing or removing $path
#
# haetty_guard_init refuses (exit 3) when the variable is unset, empty or not
# an absolute path (a relative base lands in the current folder, e.g. the
# repository; `C:/…` counts only in Git Bash), or
# when the base or the run directory it creates there is, or is inside, the
# real HOME, ~/.config, ~/.local, ~/.claude, $XDG_{CONFIG,DATA,STATE}_HOME,
# %APPDATA% or %LOCALAPPDATA% (Git Bash). It creates the run directory
# $HAETTY_RUN (base/haetty-run.XXXXXX) and exports it (a $HAETTY_RUN already
# inside the base is reused: nested scripts). haetty_guard refuses
# every path outside $HAETTY_RUN. haetty_stop_daemon BIN [SIGNAL] stops the
# daemon of this run's own socket, by the PID `BIN status --json` reports —
# never by process name (CLAUDE.md: the stable instance is never signalled). The real home is the one at the time the
# file is sourced (HAETTY_REAL_HOME, if set, wins), so source it before a
# script points HOME at a test home. Same rules: crates/haetty-testkit,
# scripts/test-guard.ps1.

HAETTY_REAL_HOME=${HAETTY_REAL_HOME:-$HOME}
# WSL: the Windows profile is a real home too.
if [ -n "${WSL_DISTRO_NAME:-}" ] && [ -z "${HAETTY_REAL_WINHOME:-}" ] && command -v cmd.exe >/dev/null; then
  HAETTY_REAL_WINHOME=$(cd /mnt/c 2>/dev/null && cmd.exe /c 'echo %USERPROFILE%' 2>/dev/null | tr -d '\r')
  [ -n "$HAETTY_REAL_WINHOME" ] && HAETTY_REAL_WINHOME=$(wslpath -u "$HAETTY_REAL_WINHOME" 2>/dev/null)
fi

_haetty_norm() { # absolute, normalised, no trailing slash (the path need not exist)
  local p=$1 lead=
  if command -v cygpath >/dev/null 2>&1; then
    # Git Bash: compare Windows paths, lower-cased (C:/users/…). Not `cygpath -u`:
    # it maps the Windows temp folder to /tmp and so hides %LOCALAPPDATA%\Temp.
    p=$(cygpath -m -a "$p" 2>/dev/null) || p=$1
    p=${p,,}
    lead=${p%%/*}; p=${p#*/}
  else
    case "$p" in /*) ;; *) p=$PWD/$p ;; esac
  fi
  # resolve . and .. lexically
  local IFS=/ out=() part
  for part in $p; do
    case "$part" in ''|.) ;; ..) [ ${#out[@]} -gt 0 ] && out=("${out[@]:0:${#out[@]}-1}") ;; *) out+=("$part") ;; esac
  done
  printf '%s' "$lead"; printf '/%s' "${out[@]}"
}

_haetty_protected() {
  local h=$HAETTY_REAL_HOME d
  for d in "$h" "$h/.config" "$h/.local" "$h/.claude" "$XDG_CONFIG_HOME" "$XDG_DATA_HOME" "$XDG_STATE_HOME" "$APPDATA" "$LOCALAPPDATA" "$USERPROFILE" "${HAETTY_REAL_WINHOME:-}"; do
    [ -n "$d" ] && _haetty_norm "$d" && echo
  done
}

_haetty_inside() { # path dir
  local p d; p=$(_haetty_norm "$1"); d=$(_haetty_norm "$2")
  [ "$p" = "$d" ] || case "$p" in "$d"/*) true ;; *) false ;; esac
}

_haetty_refuse() { echo "test-guard: refusing: $*" >&2; exit 3; }

haetty_guard_init() {
  local name=$1 base
  base=${!name}
  [ -n "$base" ] || _haetty_refuse "\$$name is unset or empty"
  case "$base" in
    /*) ;;
    [A-Za-z]:[/\\]*) command -v cygpath >/dev/null 2>&1 || _haetty_refuse "\$$name=$base is not an absolute path here" ;;
    *) _haetty_refuse "\$$name=$base is not an absolute path" ;;
  esac
  local d
  while read -r d; do
    [ -n "$d" ] && _haetty_inside "$base" "$d" && _haetty_refuse "\$$name=$base is (inside) the real $d"
  done < <(_haetty_protected)
  # Nested scripts share the run directory of the outer one.
  if [ -n "${HAETTY_RUN:-}" ] && [ -d "$HAETTY_RUN" ] && _haetty_inside "$HAETTY_RUN" "$base" \
     && [ "$(_haetty_norm "$HAETTY_RUN")" != "$(_haetty_norm "$base")" ]; then
    export HAETTY_RUN; return 0
  fi
  mkdir -p "$base" || _haetty_refuse "cannot create $base"
  HAETTY_RUN=$(mktemp -d "$base/haetty-run.XXXXXX") || _haetty_refuse "mktemp in $base failed"
  [ -n "$HAETTY_RUN" ] && [ -d "$HAETTY_RUN" ] || _haetty_refuse "no run directory"
  export HAETTY_RUN
}

haetty_guard() {
  [ -n "$1" ] || _haetty_refuse "empty path"
  [ -n "$HAETTY_RUN" ] || _haetty_refuse "haetty_guard_init was not called"
  local d
  while read -r d; do
    [ -n "$d" ] && _haetty_inside "$1" "$d" && _haetty_refuse "$1 is (inside) the real $d"
  done < <(_haetty_protected)
  _haetty_inside "$1" "$HAETTY_RUN" || _haetty_refuse "$1 is outside this run's directory $HAETTY_RUN"
}

# Stops the daemon this run started: the PID that `BIN status --json` reports
# for this run's own socket (HAETTY_SOCKET, CCTERM_SOCKET or XDG_RUNTIME_DIR
# inside $HAETTY_RUN; anything else is refused — it could be the stable
# instance's). Never by name: no pkill/killall.
haetty_stop_daemon() { # BIN [SIGNAL]
  local bin=$1 sig=${2:-TERM} sock pid
  [ -n "$HAETTY_RUN" ] || _haetty_refuse "haetty_guard_init was not called"
  sock=${HAETTY_SOCKET:-${CCTERM_SOCKET:-${XDG_RUNTIME_DIR:-}}}
  [ -n "$sock" ] && _haetty_inside "$sock" "$HAETTY_RUN" \
    || _haetty_refuse "haetty_stop_daemon: the socket ($sock) is not this run's — set HAETTY_SOCKET inside $HAETTY_RUN"
  pid=$(HAETTY_LOG=warn CCTERM_LOG=warn "$bin" status --json 2>/dev/null | sed -n 's/.*"daemon_pid":\([0-9][0-9]*\).*/\1/p')
  [ -n "$pid" ] || return 0
  kill -s "$sig" "$pid" 2>/dev/null
}

