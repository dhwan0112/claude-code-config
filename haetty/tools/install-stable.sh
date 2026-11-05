#!/usr/bin/env bash
# Install a stable haetty (Linux; also builds the Windows binary for install-stable.ps1).
#
#   tools/install-stable.sh [--prefix DIR] [--target-dir DIR] REF   build REF (commit or tag) in its own git
#                                                                    worktree with the dist profile, install, activate
#   tools/install-stable.sh [--prefix DIR] --list                    installed versions (* = active)
#   tools/install-stable.sh [--prefix DIR] --activate VERSION        point haetty at an installed version (rollback)
#   tools/install-stable.sh --build-only --target TRIPLE --out DIR REF   only build REF for TRIPLE into DIR
#                                                                    (haetty[.exe], docs, BUILDINFO; used by the .ps1)
#
# Layout (PREFIX defaults to $HOME):
#   PREFIX/.local/opt/haetty/<version>/   haetty, LICENSE, THIRD-PARTY-NOTICES.md, README.md, INSTALLED;
#                                         directory and files read-only (0555); never changed again
#   PREFIX/.local/bin/haetty              symbolic link to the active version's haetty
# <version> = <crate version>-<12-digit commit>. Older versions are kept: rollback = --activate.
# The worktree is temporary; the cargo target directory (default <repo>/haetty/target/stable-install) is a
# cache kept between installs. Nothing running is stopped: a stable haetty that is open keeps its version
# until all its windows are closed and its daemon has exited (CLAUDE.md: the stable instance is never signalled).
#
# Test isolation: when HAETTY_TEST_TMP or HAETTY_RUN is set, every path this script writes (prefix, worktree,
# output) must lie in the test run directory (scripts/test-guard.sh), so a test can never install for real.
# The cargo target directory is the build cache, not an install path, and is not checked.
set -euo pipefail

usage() { sed -n '2,10p' "$0" | sed 's/^# \{0,1\}//' >&2; exit 2; }
die() { echo "install-stable: $*" >&2; exit 1; }

# A non-login shell (wsl --exec, cron) may lack the toolchain on PATH.
command -v cargo >/dev/null 2>&1 || { [ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"; } || true
command -v zig >/dev/null 2>&1 || PATH=$HOME/.local/bin:$PATH
HERE=$(cd "$(dirname "$0")" && pwd)
PROJECT=$(cd "$HERE/.." && pwd)                 # .../haetty
REPO=$(git -C "$PROJECT" rev-parse --show-toplevel) || die "not in a git repository: $PROJECT"
PREFIX=$HOME TARGET_DIR="$PROJECT/target/stable-install" MODE=install REF= VERSION= TRIPLE= OUT=
while [ $# -gt 0 ]; do
  case "$1" in
    --prefix) PREFIX=${2:?}; shift 2 ;;
    --target-dir) TARGET_DIR=${2:?}; shift 2 ;;
    --list) MODE=list; shift ;;
    --activate) MODE=activate; VERSION=${2:?}; shift 2 ;;
    --build-only) MODE=build; shift ;;
    --target) TRIPLE=${2:?}; shift 2 ;;
    --out) OUT=${2:?}; shift 2 ;;
    -h|--help) usage ;;
    -*) die "unknown option $1" ;;
    *) [ -z "$REF" ] || die "one REF only"; REF=$1; shift ;;
  esac
done
case "$PREFIX" in /*) ;; *) die "--prefix must be an absolute path" ;; esac

TESTING=0
if [ -n "${HAETTY_TEST_TMP:-}${HAETTY_RUN:-}" ]; then
  TESTING=1
  # shellcheck source=../scripts/test-guard.sh
  . "$PROJECT/scripts/test-guard.sh"; haetty_guard_init HAETTY_TEST_TMP
fi
guard() { [ "$TESTING" = 0 ] || haetty_guard "$1"; }

OPT=$PREFIX/.local/opt/haetty
BIN=$PREFIX/.local/bin/haetty
# In a test, refuse a real prefix before building anything.
if [ "$MODE" != build ]; then guard "$OPT"; guard "$BIN"; fi

list() {
  local cur; cur=$(readlink "$BIN" 2>/dev/null || true)
  [ -d "$OPT" ] || { echo "nothing installed in $OPT"; return 0; }
  for d in "$OPT"/*/; do
    d=${d%/}; [ -f "$d/INSTALLED" ] || continue
    printf '%s %s  %s\n' "$([ "$cur" = "$d/haetty" ] && echo '*' || echo ' ')" "$(basename "$d")" \
      "$(grep -E '^(ref|installed)=' "$d/INSTALLED" | tr '\n' ' ')"
  done
}

activate() { # VERSION
  local v=$1 target="$OPT/$1/haetty"
  [ -x "$target" ] || die "version $v is not installed (see --list)"
  guard "$BIN"
  mkdir -p "$(dirname "$BIN")"
  if [ -e "$BIN" ] && [ ! -L "$BIN" ]; then
    die "$BIN exists and is not a link (another haetty?); move it away first"
  fi
  local prev; prev=$(readlink "$BIN" 2>/dev/null || true)
  ln -s "$target" "$BIN.new.$$"
  mv -Tf "$BIN.new.$$" "$BIN"         # atomic switch
  echo "active: $v  ($BIN -> $target)"
  [ -n "$prev" ] && [ "$prev" != "$target" ] && echo "previous: $(basename "$(dirname "$prev")")  (roll back: $0 --activate $(basename "$(dirname "$prev")"))"
  return 0
}

# Build REF in a temporary worktree; sets COMMIT, PKGVER, VER, BUILT (path of the binary), WTSRC.
build() { # REF TRIPLE
  local ref=$1 triple=$2
  COMMIT=$(git -C "$REPO" rev-parse --verify --quiet "$ref^{commit}") || die "no such commit or tag: $ref"
  local base=${TMPDIR:-/tmp}
  if [ "$TESTING" = 1 ]; then base=$HAETTY_RUN; fi
  WT=$(mktemp -d "$base/haetty-install.XXXXXX"); guard "$WT"
  trap 'git -C "$REPO" worktree remove --force "$WT/src" >/dev/null 2>&1 || true; rm -rf "$WT"' EXIT
  git -C "$REPO" worktree add --detach --quiet "$WT/src" "$COMMIT"
  WTSRC=$WT/src/haetty
  [ -f "$WTSRC/Cargo.toml" ] || die "$ref has no haetty/ project"
  PKGVER=$(sed -n '/^\[workspace.package\]/,/^\[/s/^version *= *"\([^"]*\)".*/\1/p' "$WTSRC/Cargo.toml" | head -1)
  VER="$PKGVER-${COMMIT:0:12}"
  echo "building $ref = $COMMIT ($VER) for $triple in a worktree; cache $TARGET_DIR"
  ( cd "$WTSRC" && HAETTY_BUILD_COMMIT=$COMMIT CARGO_TARGET_DIR=$TARGET_DIR \
      cargo build --locked --profile dist --target "$triple" -p haetty-gui --bin haetty )
  BUILT=$TARGET_DIR/$triple/dist/haetty
  if [ "${triple#*windows}" != "$triple" ]; then BUILT=$BUILT.exe; fi
  [ -f "$BUILT" ] || die "build produced no $BUILT"
}

install_version() {
  local dest=$OPT/$VER stage=$OPT/.$VER.partial
  guard "$dest"; guard "$stage"
  mkdir -p "$OPT"
  if [ -e "$stage" ]; then chmod -R u+w "$stage"; rm -rf "$stage"; fi
  mkdir "$stage"
  cp "$BUILT" "$stage/haetty"
  for f in LICENSE THIRD-PARTY-NOTICES.md README.md; do if [ -f "$WTSRC/$f" ]; then cp "$WTSRC/$f" "$stage/"; fi; done
  printf 'version=%s\nref=%s\ncommit=%s\ninstalled=%s\nsource=%s\n' "$VER" "$REF" "$COMMIT" \
    "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$REPO" > "$stage/INSTALLED"
  chmod 0555 "$stage"/* "$stage"
  mv -T "$stage" "$dest"
  echo "installed: $dest (read-only)"
}

suggest() {
  local h=$PREFIX
  cat <<EOF

Suggested Claude Code settings (printed only — nothing was changed; put them into your own
~/.claude/settings.json yourself if you want them):

  {
    "permissions": {
      "deny": [
        "Bash(*pkill*haetty*)",
        "Bash(*killall*haetty*)",
        "Edit(/$h/.local/opt/haetty/**)",
        "Edit(/$h/.local/bin/haetty)"
      ],
      "blockWritesOutsideWorkingDirectories": true
    }
  }

- Deny rules apply in every permission mode, bypassPermissions included. The Bash patterns match the
  command text only (not "kill <pid>"); they also refuse e.g. a grep for those words.
- Edit deny rules cover Claude's file tools, ">" redirections and tee — not cp/mv in a shell command.
  The install itself is read-only (0555); a sandbox "denyWrite" for these paths covers shell commands.
- blockWritesOutsideWorkingDirectories (Claude Code 2.1.314+) makes Write/Edit/NotebookEdit refuse
  paths outside the working directories in every mode; it does not limit shell commands, and it also
  removes the EnterWorktree/ExitWorktree tools (worktrees then have to be made with git yourself).
EOF
}

case "$MODE" in
  list) list ;;
  activate) activate "$VERSION" ;;
  build)
    [ -n "$REF" ] && [ -n "$TRIPLE" ] && [ -n "$OUT" ] || die "--build-only needs --target TRIPLE --out DIR REF"
    guard "$OUT"
    build "$REF" "$TRIPLE"
    mkdir -p "$OUT"
    cp "$BUILT" "$OUT/"
    for f in LICENSE THIRD-PARTY-NOTICES.md README.md; do if [ -f "$WTSRC/$f" ]; then cp "$WTSRC/$f" "$OUT/"; fi; done
    printf 'version=%s\nref=%s\ncommit=%s\nsource=%s\n' "$VER" "$REF" "$COMMIT" "$REPO" > "$OUT/BUILDINFO"
    echo "built: $OUT/$(basename "$BUILT")"
    ;;
  install)
    [ -n "$REF" ] || usage
    COMMIT=$(git -C "$REPO" rev-parse --verify --quiet "$REF^{commit}") || die "no such commit or tag: $REF"
    PKGVER=$(git -C "$REPO" show "$COMMIT:haetty/Cargo.toml" | sed -n '/^\[workspace.package\]/,/^\[/s/^version *= *"\([^"]*\)".*/\1/p' | head -1)
    VER="$PKGVER-${COMMIT:0:12}"
    if [ -d "$OPT/$VER" ]; then
      echo "$VER is already installed; activating it"
    else
      build "$REF" x86_64-unknown-linux-gnu
      install_version
    fi
    activate "$VER"
    # Desktop entry and icons, through the link (so they follow updates and rollbacks).
    DATA=${XDG_DATA_HOME:-$HOME/.local/share}
    if [ "$PREFIX" != "$HOME" ]; then DATA=$PREFIX/.local/share; fi
    guard "$DATA"
    env -u HAETTY_INSTANCE -u CCTERM_INSTANCE XDG_DATA_HOME="$DATA" "$BIN" install-desktop
    V=$(env -u HAETTY_INSTANCE -u CCTERM_INSTANCE "$BIN" --version)
    echo "check: $V"
    case "$V" in *"commit ${COMMIT:0:12},"*"instance stable)") echo "check: OK (commit ${COMMIT:0:12}, stable instance)" ;;
      *) echo "check: the installed binary does not report commit ${COMMIT:0:12} (older than --version build info?)" ;; esac
    # A stable daemon that is already running keeps its version (never stopped here).
    if [ -S "${XDG_RUNTIME_DIR:-/nonexistent}/haetty/daemon.sock" ] && [ "$TESTING" = 0 ]; then
      echo "note: a stable haetty daemon is running; open windows keep the old version until all are closed"
    fi
    case ":$PATH:" in *":$(dirname "$BIN"):"*) ;; *) echo "note: $(dirname "$BIN") is not on PATH" ;; esac
    suggest
    ;;
esac
