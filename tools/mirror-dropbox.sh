#!/usr/bin/env bash
# Mirror this repository into the Dropbox baseline folder for hosts that still read it from Dropbox:
#   repo root (everything but .git and hosts/) -> ~/Dropbox/claude-code-config/current/
#   hosts/                                     -> ~/Dropbox/claude-code-config/hosts/
# Run after a commit. Dropbox history/ snapshots are no longer written: tags in this repository replace them.
# Build output (any target/, e.g. haetty/target, several GB) and the bench's stress.bin are not copied. Files are
# overwritten in place (--inplace): renaming over files Dropbox holds open fails with "Permission denied".
set -eu
R=$(cd "$(dirname "$0")/.." && pwd); DB=${CCC_DROPBOX:-$HOME/Dropbox/claude-code-config}
[ -d "$DB/current" ] || { echo "no Dropbox baseline at $DB" >&2; exit 1; }
X=(--exclude .git --exclude hosts --exclude __pycache__ --exclude target/ --exclude 'haetty/bench/engine-compare/stress.bin')
rsync -rt --inplace --delete "${X[@]}" "$R/" "$DB/current/"
rsync -rt --inplace "$R/hosts/" "$DB/hosts/"
echo "mirrored $(git -C "$R" rev-parse --short HEAD) -> $DB/current ($(diff -rq --exclude .git --exclude hosts --exclude __pycache__ --exclude target --exclude stress.bin "$R" "$DB/current" | wc -l) differences)"
