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
# Prints one line per commit — PASS, KNOWN or FAIL — and a summary; exits with the number of FAIL commits (at
# most 125). KNOWN: a pushed commit listed in tools/known-gate-failures whose failing steps are all steps listed
# there (history is not rewritten); any other failure is FAIL. The list is read from this checkout; a change of it
# inside the range is reported (a new entry needs the user's approval, CLAUDE.md). Tag conditions: "0 failing".
# Options: --no-windows (skip the Windows clippy), --no-test (fmt and clippy only), --keep (keep worktrees).
# Builds use $HAETTY_CHECK_TARGET, else $CARGO_TARGET_DIR, else <repo>/haetty/target — the usual one, so the
# dependencies (libghostty-vt: Zig, a Ghostty checkout) are not built again; per commit only what changed is.
# Zig must be on PATH (CLAUDE.md, Environment); ~/.local/bin is tried when it is not. Writes only in the test run directory and that target
# directory; never changes a branch, the index or history.
set -u
here=$(cd "$(dirname "$0")" && pwd)
[ -n "${HAETTY_HEAVY_SLOT:-}" ] || exec "$(dirname "$0")/heavy.sh" --label "check-range.sh $*" -- bash "$0" "$@"   # at most two heavy jobs (tools/heavy.sh)
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
known_file=$here/known-gate-failures
# Every entry: a full hash, then steps from the four names — a malformed line is an error, never ignored.
if [ -f "$known_file" ]; then
  bad=$(grep -vE '^(#|$)' "$known_file" | awk -F'\t' '$1 !~ /^[0-9a-f]{40}$/ || $2 !~ /^(fmt|clippy-linux|clippy-windows|test)(,(fmt|clippy-linux|clippy-windows|test))*$/ || NF < 5')
  [ -z "$bad" ] || { echo "check-range.sh: malformed line(s) in $known_file:" >&2; echo "$bad" >&2; exit 2; }
fi
known_steps() { [ -f "$known_file" ] && awk -F'\t' -v c="$1" '!/^#/ && $1 == c { print $2 }' "$known_file"; }
known_fix() { awk -F'\t' -v c="$1" '!/^#/ && $1 == c { print substr($4, 1, 7) }' "$known_file"; }
[ -n "$commits" ] || { echo "no commit in $range changes haetty/"; exit 0; }
# The code of a commit: haetty/ without Markdown and docs/ (they cannot change a gate result).
code_of() { git -C "$top" ls-tree -r "$1" -- haetty | grep -vE $'\t''haetty/(docs/|.*\.md$)' | sha1sum | cut -c1-12; }
# Where the warnings or errors are (file:line), else the first message.
where() {
  local w; w=$(echo "$1" | grep -E '^ *--> ' | sort -u | head -3 | sed 's/^ *--> //' | paste -sd' ')
  [ -n "$w" ] && echo "$w" || echo "$1" | grep -m1 -E '^(warning|error)' | cut -c1-120
}
fails=0 knowns=0 n=0 last_code= last_result= last_commit= last_steps=
for c in $commits; do
  n=$((n + 1))
  short=$(git -C "$top" rev-parse --short=7 "$c")
  subj=$(git -C "$top" log -1 --format=%s "$c" | cut -c1-70)
  code=$(code_of "$c")
  if [ "$code" = "$last_code" ]; then
    why=$last_result steps=$last_steps same=" (same code as $last_commit)"
  else
    same=
    wt=$HAETTY_RUN/check-$short
    haetty_guard "$wt"
    git -C "$top" worktree add -q --detach "$wt" "$c" || { echo "$short  ERROR worktree"; exit 2; }
    cd "$wt/haetty" || exit 2
    why= steps=
    cargo fmt --all -- --check > "$wt.fmt.log" 2>&1 || { why="fmt: $(grep -m1 '^Diff in' "$wt.fmt.log" | cut -c1-90)"; steps=fmt; }
    find crates -name '*.rs' -exec touch {} +
    out=$(cargo clippy --all-targets 2>&1)
    if echo "$out" | grep -qE '^(warning|error)'; then
      why="${why:+$why; }clippy linux: $(where "$out")"; steps="$steps clippy-linux"
    fi
    if [ $windows = 1 ]; then
      out=$(cargo clippy --all-targets --target x86_64-pc-windows-gnu 2>&1)
      if echo "$out" | grep -qE '^(warning|error)'; then
        why="${why:+$why; }clippy windows: $(where "$out")"; steps="$steps clippy-windows"
      fi
    fi
    if [ $test = 1 ]; then
      out=$(cargo test 2>&1)
      if echo "$out" | grep -qE 'FAILED|panicked|^error(\[|:)' || ! echo "$out" | grep -q '^test result'; then
        why="${why:+$why; }test: $(echo "$out" | grep -E '^test .* FAILED|^error' | head -3 | paste -sd' ' | cut -c1-160)"
        steps="$steps test"
      fi
    fi
    cd "$top" || exit 2
    [ $keep = 1 ] || git -C "$top" worktree remove --force "$wt"
    last_code=$code last_result=$why last_commit=$short last_steps=$steps
  fi
  listed=$(known_steps "$c")
  if [ -z "$steps" ]; then
    result="PASS$same${listed:+ (listed in known-gate-failures, passes now)}"
  else
    unlisted=
    for st in $steps; do case ",$listed," in *",$st,"*) ;; *) unlisted="$unlisted $st" ;; esac; done
    if [ -n "$listed" ] && [ -z "$unlisted" ]; then
      result="KNOWN — $why$same (known-gate-failures; fixed in $(known_fix "$c"))"; knowns=$((knowns + 1))
    else
      result="FAIL — $why$same${listed:+ (listed for $listed only; also failing:$unlisted)}"; fails=$((fails + 1))
    fi
  fi
  printf '%s  %-72s %s\n' "$short" "$subj" "$result"
done
git -C "$top" worktree prune
changed=$(git -C "$top" log --format=%h "$range" -- haetty/tools/known-gate-failures | paste -sd' ')
[ -z "$changed" ] || echo "NOTE: tools/known-gate-failures changed in this range ($changed): every new entry needs the user's approval (CLAUDE.md)"
echo "TOTAL check-range $range: $n commit(s) changing haetty/, $fails failing, $knowns known"
[ $fails -gt 125 ] && fails=125
exit $fails
