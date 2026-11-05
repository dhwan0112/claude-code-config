"""Raw RGBA (row-major, 4 bytes per pixel, no header) of a PNG, for the X11
window icon: haetty embeds it so it needs no PNG decoder at run time.

    python3 tools/rgba.py png/haetty-64.png rgba/haetty-64.rgba
"""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from pngio import read_png  # noqa: E402


def main():
    src, dst = sys.argv[1], sys.argv[2]
    w, h, rows = read_png(src)
    out = bytearray()
    for row in rows:
        for r, g, b, a in row:
            out += bytes((r, g, b, a))
    assert len(out) == w * h * 4
    os.makedirs(os.path.dirname(dst) or ".", exist_ok=True)
    with open(dst, "wb") as f:
        f.write(out)
    print(f"{dst}: {w}x{h}, {len(out)} bytes")


if __name__ == "__main__":
    main()
