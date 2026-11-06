#!/usr/bin/env bash
# Checks every commit of a range on its own (CLAUDE.md: every commit passes the gate alone).
#
#   HAETTY_TEST_TMP=/tmp/haetty-test tools/check-range.sh haetty-v0.1.1..HEAD
#   HAETTY_TEST_TMP=/tmp/haetty-test tools/check-range.sh origin/main..HEAD     # before a push
#
# For each commit of the range (oldest first) that changes `haetty/`, a temporary detached worktree in the test
# run directory runs the quick gate:
#   fmt      cargo fmt --all -- --check
#   clippy   cargo clippy --all-targets, Linux and x86_64-pc-windows-gnu, with every .rs file touched first so
#            cached warnings cannot hide (one run per target, judged on its own output)
#   test     cargo test (FAILED, panicked or a compile error fails it)
# A commit whose code is the same as the last one checked — it changes only Markdown or other docs under
# haetty/ — gets that commit's result ("same code as …") instead of a second run.
# Prints one line per commit and a summary; exits with the number of failing commits (at most 125).
# Options: --no-windows (skip the Windows clippy), --no-test (fmt and clippy only), --keep (keep worktrees).
# Builds use $HAETTY_CHECK_TARGET, else $CARGO_TARGET_DIR, else <repo>/haetty/target — the usual one, so the
# dependencies (libghostty-vt: Zig, a Ghostty checkout) are not built again; per commit only what changed is.
# Zig must be on PATH (CLAUDE.md, Environment); ~/.local/bin is tried when it is not. Writes only in the test run directory and that target
# directory; never changes a branch, the index or history.
set -u
here=$(cd "$(dirname "$0")" && pwd)
. "$here/../scripts/test-guard.sh"
haetty_guard_init HAETTY_TEST_TMP || exit 3
windows=1 test=1 keep=0 range=
for a in "$@"; do
  case "$a" in
    --no-windows) windows=0 ;;
    --no-test) test=0 ;;
    --keep) keep=1 ;;
    -h|--help) sed -n '2,20p' "$0"; exit 0 ;;
    *) range=$a ;;
  esac
done
[ -n "$range" ] || { echo "usage: check-range.sh [--no-windows] [--no-test] [--keep] RANGE" >&2; exit 2; }
top=$(git -C "$here" rev-parse --show-toplevel) || exit 2
export CARGO_TARGET_DIR=${HAETTY_CHECK_TARGET:-${CARGO_TARGET_DIR:-$top/haetty/target}}
command -v zig > /dev/null || PATH=$HOME/.local/bin:$PATH
command -v zig > /dev/null || { echo "check-range.sh: zig is not on PATH (libghostty-vt-sys needs it)" >&2; exit 2; }
command -v cargo > /dev/null || { echo "check-range.sh: cargo is not on PATH" >&2; exit 2; }
[ $windows = 1 ] && ! rustup target list --installed 2>/dev/null | grep -qx x86_64-pc-windows-gnu && {
  echo "note: x86_64-pc-windows-gnu not installed: Windows clippy skipped"; windows=0; }
commits=$(git -C "$top" rev-list --reverse "$range" -- haetty) || exit 2
[ -n "$commits" ] || { echo "no commit in $range changes haetty/"; exit 0; }
# The code of a commit: haetty/ without Markdown and docs/ (they cannot change a gate result).
code_of() { git -C "$top" ls-tree -r "$1" -- haetty | grep -vE $'\t''haetty/(docs/|.*\.md$)' | sha1sum | cut -c1-12; }
# Where the warnings or errors are (file:line), else the first message.
where() {
  local w; w=$(echo "$1" | grep -E '^ *--> ' | sort -u | head -3 | sed 's/^ *--> //' | paste -sd' ')
  [ -n "$w" ] && echo "$w" || echo "$1" | grep -m1 -E '^(warning|error)' | cut -c1-120
}
fails=0 n=0 last_code= last_result= last_commit=
for c in $commits; do
  n=$((n + 1))
  short=$(git -C "$top" rev-parse --short=7 "$c")
  subj=$(git -C "$top" log -1 --format=%s "$c" | cut -c1-70)
  code=$(code_of "$c")
  if [ "$code" = "$last_code" ]; then
    result="$last_result (same code as $last_commit)"
  else
    wt=$HAETTY_RUN/check-$short
    haetty_guard "$wt"
    git -C "$top" worktree add -q --detach "$wt" "$c" || { echo "$short  ERROR worktree"; exit 2; }
    cd "$wt/haetty" || exit 2
    why=
    cargo fmt --all -- --check > "$wt.fmt.log" 2>&1 || why="fmt: $(grep -m1 '^Diff in' "$wt.fmt.log" | cut -c1-90)"
    find crates -name '*.rs' -exec touch {} +
    out=$(cargo clippy --all-targets 2>&1)
    if echo "$out" | grep -qE '^(warning|error)'; then
      why="${why:+$why; }clippy linux: $(where "$out")"
    fi
    if [ $windows = 1 ]; then
      out=$(cargo clippy --all-targets --target x86_64-pc-windows-gnu 2>&1)
      if echo "$out" | grep -qE '^(warning|error)'; then
        why="${why:+$why; }clippy windows: $(where "$out")"
      fi
    fi
    if [ $test = 1 ]; then
      out=$(cargo test 2>&1)
      if echo "$out" | grep -qE 'FAILED|panicked|^error(\[|:)' || ! echo "$out" | grep -q '^test result'; then
        why="${why:+$why; }test: $(echo "$out" | grep -E '^test .* FAILED|^error' | head -3 | paste -sd' ' | cut -c1-160)"
      fi
    fi
    cd "$top" || exit 2
    [ $keep = 1 ] || git -C "$top" worktree remove --force "$wt"
    if [ -z "$why" ]; then result=PASS; else result="FAIL — $why"; fi
    last_code=$code last_result=${result%% (same code*} last_commit=$short
  fi
  case "$result" in FAIL*) fails=$((fails + 1)) ;; esac
  printf '%s  %-72s %s\n' "$short" "$subj" "$result"
done
git -C "$top" worktree prune
echo "TOTAL check-range $range: $n commit(s) changing haetty/, $fails failing"
[ $fails -gt 125 ] && fails=125
exit $fails
