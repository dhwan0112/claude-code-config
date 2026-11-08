#!/usr/bin/env python3
"""Runs the Linux soak verdict (scripts/e2e/soak-verdict.py) on the synthetic series of this folder and compares
each result with expect.txt. Exit 1 on any mismatch. The Windows twin is windows/test-soak-verdict.ps1."""
import importlib.util
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location("soak_verdict", os.path.join(HERE, "..", "soak-verdict.py"))
sv = importlib.util.module_from_spec(spec)
spec.loader.exec_module(sv)

bad = 0
with open(os.path.join(HERE, "expect.txt")) as f:
    for line in f:
        if not line.strip():
            continue
        name, want = line.split()
        cols, rows = sv.load(os.path.join(HERE, "linux", name + ".csv"))
        _, ok = sv.verdict(cols, rows, 3600, "yes")
        got = "PASS" if ok else "FAIL"
        bad += got != want
        print(f"{'ok' if got == want else 'MISMATCH':9} {name:16} want {want} got {got}")
print(f"mismatches: {bad}")
sys.exit(1 if bad else 0)
