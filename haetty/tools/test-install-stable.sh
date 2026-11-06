#!/usr/bin/env bash
# Self-test of install-stable.sh (Linux). Installs two commits into a prefix inside the test run directory —
# never the real ~/.local — and checks layout, permissions, the link, --version, the desktop entry, rollback,
# reinstall, cleanup and that a real prefix is refused.
#   HAETTY_TEST_TMP=/tmp/haetty-test tools/test-install-stable.sh REF1 REF2
set -uo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
. "$HERE/../scripts/test-guard.sh"; haetty_guard_init HAETTY_TEST_TMP
REF1=${1:?REF1}; REF2=${2:?REF2}
INSTALL=$HERE/install-stable.sh
REPO=$(git -C "$HERE" rev-parse --show-toplevel)
P=$HAETTY_RUN/home; haetty_guard "$P"; mkdir -p "$P"
OPT=$P/.local/opt/haetty BIN=$P/.local/bin/haetty
pass=0 fail=0
ok() { pass=$((pass+1)); echo "PASS $1"; }
bad() { fail=$((fail+1)); echo "FAIL $1: $2"; }
real_state() { ls -la "$HOME/.local/opt/haetty" "$HOME/.local/bin/haetty" "$HOME/.local/share/applications/io.github.dhwan0112.haetty.desktop" 2>&1 | md5sum; }
before=$(real_state)
c1=$(git -C "$REPO" rev-parse "$REF1^{commit}"); c2=$(git -C "$REPO" rev-parse "$REF2^{commit}")
v1="0.1.0-${c1:0:12}" v2="0.1.0-${c2:0:12}"

# 1. A real prefix is refused before anything is built or written.
out=$("$INSTALL" --prefix "$HOME" "$REF1" 2>&1); rc=$?
[ $rc -ne 0 ] && echo "$out" | grep -q "refusing" && [ "$(real_state)" = "$before" ] \
  && ok "real prefix refused (rc=$rc), nothing written" || bad "real prefix" "rc=$rc $(echo "$out" | tail -2)"

# 1b. A release tag whose crate version differs is refused before building (a throwaway tag in a clone
#     inside the run directory; the real repository gets no tag).
CL=$HAETTY_RUN/clone; haetty_guard "$CL"
git clone -q "$REPO" "$CL" && git -C "$CL" checkout -q "$(git -C "$REPO" rev-parse HEAD)" 2>/dev/null
have=$(sed -n '/^\[workspace.package\]/,/^\[/s/^version *= *"\([^"]*\)".*/\1/p' "$CL/haetty/Cargo.toml" | head -1)
git -C "$CL" tag haetty-v9.9.9 HEAD
out=$(HAETTY_RUN=$HAETTY_RUN "$CL/haetty/tools/install-stable.sh" --prefix "$P" haetty-v9.9.9 2>&1); rc=$?
[ $rc -ne 0 ] && echo "$out" | grep -q "crate version is $have, not 9.9.9" && ! echo "$out" | grep -q "^building" && [ ! -e "$OPT" ] \
  && ok "tag haetty-v9.9.9 on a $have commit refused before building" || bad "tag/version mismatch" "rc=$rc $(echo "$out" | tail -2)"
# (A matching tag passes the check: REF1 below is the real tag haetty-v0.1.0.)

# 2. First install.
"$INSTALL" --prefix "$P" "$REF1" > "$HAETTY_RUN/install1.log" 2>&1; rc=$?
[ $rc -eq 0 ] && ok "install $REF1 (rc 0)" || bad "install $REF1" "rc=$rc; $(tail -5 "$HAETTY_RUN/install1.log")"
[ "$(stat -c %a "$OPT/$v1")" = 555 ] && ok "version directory 0555" || bad "dir mode" "$(stat -c %a "$OPT/$v1")"
modes=$(stat -c %a "$OPT/$v1"/* | sort -u | tr '\n' ' ')
[ "$modes" = "555 " ] && ok "files 0555 ($(ls "$OPT/$v1" | tr '\n' ' '))" || bad "file modes" "$modes"
touch "$OPT/$v1/x" 2>/dev/null && bad "read-only" "could create a file" || ok "cannot write into the installed version"
[ "$(readlink "$BIN")" = "$OPT/$v1/haetty" ] && ok "link -> $v1" || bad "link" "$(readlink "$BIN")"
ver=$(env -u HAETTY_INSTANCE "$BIN" --version)
case "$ver" in *"commit ${c1:0:12},"*"instance stable)") ok "--version: $ver" ;; *) bad "--version" "$ver" ;; esac
grep -q "check: OK" "$HAETTY_RUN/install1.log" && ok "installer's own check OK" || bad "installer check" "$(grep check: "$HAETTY_RUN/install1.log")"
d=$P/.local/share/applications/io.github.dhwan0112.haetty.desktop
grep -qx "Exec=\"$BIN\"" "$d" && ok "desktop entry Exec= the link" || bad "desktop Exec" "$(grep Exec "$d" 2>&1)"
grep -q '"Edit(/'"$P"'/.local/opt/haetty/\*\*)"' "$HAETTY_RUN/install1.log" && grep -q blockWritesOutsideWorkingDirectories "$HAETTY_RUN/install1.log" \
  && ok "printed deny/blockWrites suggestion with this prefix" || bad "suggestion" "missing"

# 3. Second version: link moves, the first stays.
"$INSTALL" --prefix "$P" "$REF2" > "$HAETTY_RUN/install2.log" 2>&1; rc=$?
[ $rc -eq 0 ] && [ "$(readlink "$BIN")" = "$OPT/$v2/haetty" ] && [ -x "$OPT/$v1/haetty" ] \
  && ok "install $REF2: link -> $v2, $v1 kept" || bad "install $REF2" "rc=$rc $(readlink "$BIN"); $(tail -3 "$HAETTY_RUN/install2.log")"
grep -q "^previous: $v1" "$HAETTY_RUN/install2.log" && ok "installer names the previous version for rollback" || bad "previous" "$(grep previous "$HAETTY_RUN/install2.log")"
lst=$("$INSTALL" --prefix "$P" --list 2>&1)
echo "$lst" | grep -q "^\* $v2" && echo "$lst" | grep -q "^  $v1" \
  && ok "--list marks $v2 active" || bad "--list" "$lst"

# 4. Rollback.
"$INSTALL" --prefix "$P" --activate "$v1" > /dev/null; rc=$?
ver=$(env -u HAETTY_INSTANCE "$BIN" --version 2>&1)
[ $rc -eq 0 ] && [ "$(readlink "$BIN")" = "$OPT/$v1/haetty" ] && case "$ver" in *"${c1:0:12}"*) true ;; *) false ;; esac \
  && ok "--activate $v1: rolled back ($ver)" || bad "rollback" "rc=$rc $(readlink "$BIN") $ver"
"$INSTALL" --prefix "$P" --activate nonexistent > /dev/null 2>&1 && bad "unknown version" "accepted" || ok "unknown version refused"

# 5. Reinstalling an installed version only activates it (no build).
"$INSTALL" --prefix "$P" "$REF2" > "$HAETTY_RUN/install3.log" 2>&1
grep -q "already installed" "$HAETTY_RUN/install3.log" && ! grep -q "^building" "$HAETTY_RUN/install3.log" && [ "$(readlink "$BIN")" = "$OPT/$v2/haetty" ] \
  && ok "reinstall $REF2: activated without building" || bad "reinstall" "$(head -3 "$HAETTY_RUN/install3.log")"

# 6. No worktree left; the real home untouched.
git -C "$REPO" worktree list | grep -q haetty-install && bad "worktrees" "$(git -C "$REPO" worktree list)" || ok "temporary worktrees removed"
[ "$(real_state)" = "$before" ] && ok "real ~/.local/opt/haetty, ~/.local/bin/haetty, desktop entry unchanged" || bad "real home" "changed"

chmod -R u+w "$OPT"   # so the run directory can be removed
echo "TOTAL install-stable: $pass passed, $fail failed"
[ $fail -eq 0 ]
