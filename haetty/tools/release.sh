#!/usr/bin/env bash
# Make a release's assets, sign the manifest and upload a draft (stage PU; docs/SELF-HOSTING.md, "Releases").
#
#   tools/release.sh haetty-vX.Y.Z [--out DIR] [--min-update-from V]   the person, in their own terminal
#   tools/release.sh COMMIT --out DIR                                  a dry run: assets and manifest only
#
# 1. Builds the tag's commit in a clean temporary worktree with the dist profile for x86_64-unknown-linux-gnu and
#    x86_64-pc-windows-gnu (haetty and haetty-headless), SOURCE_DATE_EPOCH = the commit's time.
# 2. Packs haetty-<ver>-x86_64-linux.tar.gz (ustar, owner 0, the commit's time, sorted, `gzip -n`) and
#    haetty-<ver>-x86_64-windows.zip (fixed times, sorted), each with LICENSE and THIRD-PARTY-NOTICES.md: the same
#    commit gives the same bytes (checked in stage PU: docs/history/2026-11-16-pu-update.md).
# 3. Writes manifest.json with the built binary (`haetty release-manifest`, the same code that reads it): version,
#    commit, build date, protocol, the lowest version that may update in-app, each asset's name, size and sha256, the
#    CHANGELOG section of the version.
# 4. Signs manifest.json with YOUR minisign (or rsign2) and YOUR key: the tool asks for the password on the terminal.
#    This script never reads, copies or prints a key, and takes no password. Set HAETTY_MINISIGN_KEY to the secret
#    key's path if it is not the tool's default (~/.minisign/minisign.key).
# 5. `gh release create TAG … --draft`: you look at the draft on GitHub and publish it yourself.
# Inside a Claude Code session (CLAUDECODE / CLAUDE_CODE_* set) steps 4 and 5 are not run: the script stops after
# the manifest and says what to run yourself. A dry run (a commit, not a tag) never signs or uploads.
set -euo pipefail
die() { echo "release: $*" >&2; exit 1; }
HERE=$(cd "$(dirname "$0")" && pwd); PROJECT=$(cd "$HERE/.." && pwd)
REPO=$(git -C "$PROJECT" rev-parse --show-toplevel)
command -v cargo > /dev/null || { [ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"; }
. "$HERE/machine.sh"
REF= OUT= MIN=0.0.0
while [ $# -gt 0 ]; do
  case "$1" in
    --out) OUT=${2:?}; shift 2 ;;
    --min-update-from) MIN=${2:?}; shift 2 ;;
    -h|--help) sed -n '2,25p' "$0"; exit 0 ;;
    -*) die "unknown option $1" ;;
    *) [ -z "$REF" ] || die "one REF only"; REF=$1; shift ;;
  esac
done
[ -n "$REF" ] || die "usage: tools/release.sh haetty-vX.Y.Z [--out DIR] | COMMIT --out DIR"
TAG=
case "$REF" in haetty-v[0-9]*) TAG=$REF ;; esac
COMMIT=$(git -C "$REPO" rev-parse --verify --quiet "$REF^{commit}") || die "no such tag or commit: $REF"
VERSION=$(git -C "$REPO" show "$COMMIT:haetty/Cargo.toml" | sed -n '/^\[workspace.package\]/,/^\[/s/^version *= *"\([^"]*\)".*/\1/p' | head -1)
[ -z "$TAG" ] || [ "${TAG#haetty-v}" = "$VERSION" ] || die "tag $TAG points at a commit whose version is $VERSION"
EPOCH=$(git -C "$REPO" show -s --format=%ct "$COMMIT")
OUT=${OUT:-$PWD/release-$VERSION}
if [ -n "${HAETTY_TEST_TMP:-}${HAETTY_RUN:-}" ]; then . "$PROJECT/scripts/test-guard.sh"; haetty_guard_init HAETTY_TEST_TMP; haetty_guard "$OUT"; fi
mkdir -p "$OUT"
[ -z "$(ls -A "$OUT")" ] || die "$OUT is not empty"
echo "release $VERSION from ${COMMIT:0:12} (${TAG:-dry run}) into $OUT"

WT=$(mktemp -d "${TMPDIR:-/tmp}/haetty-release.XXXXXX")
trap 'git -C "$REPO" worktree remove --force "$WT/src" > /dev/null 2>&1 || true; rm -rf "$WT"' EXIT
git -C "$REPO" worktree add --detach --quiet "$WT/src" "$COMMIT"
SRC=$WT/src/haetty
TARGET_DIR=${HAETTY_RELEASE_TARGET:-$PROJECT/target/release-build}
for triple in x86_64-unknown-linux-gnu x86_64-pc-windows-gnu; do
  echo "building $triple"
  ( cd "$SRC" && HAETTY_BUILD_COMMIT=$COMMIT SOURCE_DATE_EPOCH=$EPOCH CARGO_TARGET_DIR=$TARGET_DIR \
      RUSTFLAGS="--remap-path-prefix=$WT/src=/src --remap-path-prefix=$HOME=/home" \
      nice -n 5 cargo build --locked --profile dist --target "$triple" -p haetty-gui -p haetty-headless --bin haetty --bin haetty-headless )
done
LIN=$TARGET_DIR/x86_64-unknown-linux-gnu/dist WIN=$TARGET_DIR/x86_64-pc-windows-gnu/dist
# A release never trusts the tests' key or CA (`update-test` / `test-support` builds only).
for b in "$LIN/haetty" "$WIN/haetty.exe"; do
  if grep -aqE 'TEST-ONLY|RWRURVNUT05M' "$b"; then die "$b holds the TEST-ONLY key or CA: not a release build"; fi
done
DOCS="LICENSE THIRD-PARTY-NOTICES.md"

# Linux: ustar, owner 0, the commit's time, sorted names, gzip without a name or time.
S=$WT/lin; mkdir -p "$S"
cp "$LIN/haetty" "$LIN/haetty-headless" "$S/"; for f in $DOCS; do cp "$SRC/$f" "$S/"; done
chmod 0755 "$S/haetty" "$S/haetty-headless"; chmod 0644 "$S/LICENSE" "$S/THIRD-PARTY-NOTICES.md"
LTAR=haetty-$VERSION-x86_64-linux.tar.gz
( cd "$S" && tar --format=ustar --owner=0 --group=0 --numeric-owner --mtime="@$EPOCH" --sort=name \
    -cf - haetty haetty-headless $DOCS ) | gzip -n -9 > "$OUT/$LTAR"
# Windows: a zip with fixed times, sorted (Python's zipfile: a build tool here, never at run time).
S=$WT/win; mkdir -p "$S"
cp "$WIN/haetty.exe" "$WIN/haetty-headless.exe" "$S/"; for f in $DOCS; do cp "$SRC/$f" "$S/"; done
WZIP=haetty-$VERSION-x86_64-windows.zip
python3 -I - "$S" "$OUT/$WZIP" "$EPOCH" haetty.exe haetty-headless.exe $DOCS <<'PY'
import sys, time, zipfile
src, out, epoch, names = sys.argv[1], sys.argv[2], int(sys.argv[3]), sorted(sys.argv[4:])
t = time.gmtime(max(epoch, 315532800))[:6]
with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as z:
    for n in names:
        i = zipfile.ZipInfo(n, t)
        i.compress_type = zipfile.ZIP_DEFLATED
        i.external_attr = 0
        i.create_system = 0
        z.writestr(i, open(f"{src}/{n}", "rb").read())
PY

# The CHANGELOG section of this version (from "## VERSION" to the next "## ").
awk -v v="$VERSION" 'index($0, "## " v " ") == 1 || $0 == "## " v {f=1; next} f && /^## / {exit} f' \
  "$SRC/docs/CHANGELOG.md" > "$WT/notes.md"
PROTO=$(sed -n 's/^pub const VERSION: u32 = \([0-9]*\);.*/\1/p' "$SRC/crates/haetty-proto/src/wire.rs")
# The built binary writes the manifest. It must have the subcommand (a commit before PU would take the word for a
# program and open a window), and it runs with no display and an instance of its own, so it can never reach the
# person's running haetty or its daemon.
hb() { env -u DISPLAY -u WAYLAND_DISPLAY HAETTY_INSTANCE=release "$LIN/haetty" "$@"; }
hb --help 2>&1 | grep -q "release-manifest" || die "$REF's haetty has no release-manifest (a commit before stage PU): no release from it"
hb release-manifest --version "$VERSION" --commit "$COMMIT" --built "$(date -u -d "@$EPOCH" +%Y-%m-%dT%H:%M:%SZ)" \
  --protocol "$PROTO" --min-update-from "$MIN" --notes "$WT/notes.md" \
  --asset "x86_64-linux=$OUT/$LTAR:haetty,haetty-headless,LICENSE,THIRD-PARTY-NOTICES.md" \
  --asset "x86_64-windows=$OUT/$WZIP:haetty.exe,haetty-headless.exe,LICENSE,THIRD-PARTY-NOTICES.md" > "$OUT/manifest.json"
( cd "$OUT" && sha256sum "$LTAR" "$WZIP" manifest.json )

cp "$WT/notes.md" "$OUT/notes.md"
claude=$(env | grep -cE '^(CLAUDECODE|CLAUDE_CODE_[A-Z_]*)=' || true)
if [ -z "$TAG" ]; then
  echo "dry run (not a tag): no signature, no upload"
  exit 0
fi
if [ "$claude" != 0 ]; then
  cat <<EOF
stopped before signing: this is a Claude Code session. Signing and the draft are yours; in your own terminal:
  cd $OUT
  minisign -Sm manifest.json${HAETTY_MINISIGN_KEY:+ -s "\$HAETTY_MINISIGN_KEY"}        # asks for your key's password
  gh release create $TAG $LTAR $WZIP manifest.json manifest.json.minisig --draft --title "haetty $VERSION" --notes-file notes.md
EOF
  exit 0
fi
SIGN=minisign; command -v minisign > /dev/null || SIGN=rsign
command -v "$SIGN" > /dev/null || die "neither minisign nor rsign2 is installed (SELF-HOSTING.md, \"Releases\")"
echo "signing manifest.json with $SIGN (it asks for your key's password)"
if [ "$SIGN" = minisign ]; then
  ( cd "$OUT" && minisign -S ${HAETTY_MINISIGN_KEY:+-s "$HAETTY_MINISIGN_KEY"} -m manifest.json )
else
  ( cd "$OUT" && rsign sign ${HAETTY_MINISIGN_KEY:+-s "$HAETTY_MINISIGN_KEY"} manifest.json )
fi
[ -f "$OUT/manifest.json.minisig" ] || die "no manifest.json.minisig"
gh release create "$TAG" "$OUT/$LTAR" "$OUT/$WZIP" "$OUT/manifest.json" "$OUT/manifest.json.minisig" \
  --draft --title "haetty $VERSION" --notes-file "$OUT/notes.md"
echo "draft created: check it on GitHub, then publish it there"
