#!/usr/bin/env python3
"""Joins two vtdiff outcome files (`haetty-vtdiff --outcomes FILE`, one per reference) and prints the side-by-side
report: per reference the verdicts, and how the two references' results relate.

usage: vtdiff-sides.py MAIN.tsv OLD.tsv

Each run is one (input, cutting) pair. The classes, with main = ghostty-main and old = ghostty-0.2.2:
- agree: both identical, or both differ from haetty-vt.
- 0.2.2 defect: haetty-vt equals main on the run (main identical) and 0.2.2 differs: the difference is 0.2.2's.
- native follows 0.2.2: haetty-vt equals 0.2.2 on the run (0.2.2 identical) and main differs: a difference to fix
  (or to explain) under main.
- other: both differ from haetty-vt.
"""
import collections
import re
import sys


def load(path):
    rows = {}
    with open(path, encoding="utf-8") as f:
        header = f.readline().rstrip("\n").split("\t")
        for line in f:
            cols = line.rstrip("\n").split("\t")
            r = dict(zip(header, cols))
            key = (r["input"], r["category"], r["splits"])
            rows[key] = r
    return rows


def verdict_class(v):
    if v == "Identical":
        return "identical"
    if v.startswith("Known"):
        return "known"
    return "fail"


def main():
    if len(sys.argv) != 3:
        print(__doc__, file=sys.stderr)
        sys.exit(2)
    main_rows = load(sys.argv[1])
    old_rows = load(sys.argv[2])
    keys = sorted(set(main_rows) & set(old_rows))
    only = len(main_rows) + len(old_rows) - 2 * len(keys)

    def summary(rows, label):
        c = collections.Counter(verdict_class(r["verdict"]) for r in rows.values())
        fail_known_ok = sum(1 for r in rows.values() if r["verdict"].startswith("Fail(Some"))
        print(f"| {label} | {len(rows)} | {c['identical']} | {c['known']} | {c['fail']} | {fail_known_ok} |")

    print("## Per reference\n")
    print("| reference | runs | identical | KNOWN | FAIL | of the FAIL, KNOWN if approved |")
    print("|---|---|---|---|---|---|")
    summary(main_rows, "ghostty-main")
    summary(old_rows, "ghostty-0.2.2")
    if only:
        print(f"\n({only} runs present in one file only)")

    print("\n## How the references relate (runs present in both)\n")
    classes = collections.Counter()
    by_category = collections.defaultdict(collections.Counter)
    for k in keys:
        m = main_rows[k]
        o = old_rows[k]
        m_id = m["verdict"] == "Identical"
        o_id = o["verdict"] == "Identical"
        if m_id and o_id:
            cls = "agree: both identical"
        elif m_id and not o_id:
            cls = "0.2.2 defect (haetty-vt = main, 0.2.2 differs)"
        elif o_id and not m_id:
            cls = "native follows 0.2.2 (haetty-vt = 0.2.2, main differs)"
        else:
            cls = "other (both differ from haetty-vt)"
        classes[cls] += 1
        by_category[cls][m["category"]] += 1
    print("| class | runs | categories |")
    print("|---|---|---|")
    for cls, n in sorted(classes.items(), key=lambda kv: -kv[1]):
        cats = ", ".join(f"{c} {k}" for c, k in by_category[cls].most_common())
        print(f"| {cls} | {n} | {cats} |")

    print("\n## Fields of the ghostty-main FAILs\n")
    fields = collections.Counter()
    for r in main_rows.values():
        if verdict_class(r["verdict"]) == "fail":
            for f in filter(None, r["fields"].split(",")):
                fields[f] += 1
    for f, n in fields.most_common():
        print(f"- {f}: {n}")


if __name__ == "__main__":
    main()
