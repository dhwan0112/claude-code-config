#!/usr/bin/env python3
"""Nearest-neighbour upscaler for the small-size check images.

Usage: python3 tools/magnify.py IN.png FACTOR OUT.png

Every source pixel becomes a FACTOR x FACTOR block; colours and alpha are
copied unchanged (no smoothing, no background), so the result shows exactly
what the small icon contains.
"""

import os
import sys

sys.dont_write_bytecode = True  # keep tools/ free of __pycache__
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from pngio import read_png, write_png  # noqa: E402


def main():
    if len(sys.argv) != 4:
        sys.exit(__doc__)
    src, factor, dst = sys.argv[1], int(sys.argv[2]), sys.argv[3]
    w, h, rows = read_png(src)
    out = []
    for row in rows:
        big = [px for px in row for _ in range(factor)]
        out.extend([big] * factor)
    write_png(dst, w * factor, h * factor, out)


if __name__ == "__main__":
    main()
