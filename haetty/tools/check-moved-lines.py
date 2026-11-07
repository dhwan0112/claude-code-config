#!/usr/bin/env python3
"""Check that a restructured document lost nothing: every non-blank line of OLD must still exist,
character for character (trailing spaces ignored), as a line of one of the NEW files.

    tools/check-moved-lines.py OLD NEW [NEW ...]
    git show HEAD:haetty/docs/HANDOFF.md > /tmp/old.md
    tools/check-moved-lines.py /tmp/old.md docs/HANDOFF.md docs/history/*.md

Prints the counts — lines checked, found in each file — and every missing line with its line number in OLD;
exits 1 if any is missing. Lines that only mark the progress of a running stage ("> **In progress:** …") are
not required (they are removed on purpose when a stage ends).
"""
import sys
from collections import Counter
from pathlib import Path


def lines(p):
    return [l.rstrip() for l in Path(p).read_text(encoding="utf-8").splitlines()]


def main(argv):
    if len(argv) < 3:
        print(__doc__.strip(), file=sys.stderr)
        return 2
    old, news = argv[1], argv[2:]
    where = {}
    for n in news:
        for l in lines(n):
            if l.strip():
                where.setdefault(l, n)
    checked = 0
    found = Counter()
    missing = []
    for i, l in enumerate(lines(old), 1):
        if not l.strip() or l.startswith("> **In progress:**"):
            continue
        checked += 1
        if l in where:
            found[where[l]] += 1
        else:
            missing.append((i, l))
    print(f"{old}: {checked} non-blank lines checked")
    for n in news:
        print(f"  found in {n}: {found[n]}")
    for i, l in missing:
        print(f"MISSING line {i}: {l[:110]}")
    print(f"TOTAL moved-lines: {checked - len(missing)} found, {len(missing)} missing")
    return 1 if missing else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
