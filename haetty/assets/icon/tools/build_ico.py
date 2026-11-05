#!/usr/bin/env python3
"""Pack PNG files into one Windows .ico (Python standard library only).

Usage (from assets/icon/):
    python3 tools/build_ico.py
        -> reads png/haetty-{16,20,24,32,40,48,64,256}.png, writes haetty.ico
    python3 tools/build_ico.py OUT.ico IN1.png IN2.png ...
        -> explicit output and inputs

Each entry is stored as the PNG file itself (PNG-compressed icon entries are
valid since Windows Vista). Layout: ICONDIR (6 bytes), one ICONDIRENTRY
(16 bytes) per image, then the image data. Width/height bytes of 0 mean 256.
"""

import os
import struct
import sys

sys.dont_write_bytecode = True  # keep tools/ free of __pycache__
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from pngio import png_size  # noqa: E402

DEFAULT_SIZES = (16, 20, 24, 32, 40, 48, 64, 256)


def build_ico(out_path, png_paths):
    images = []
    for p in png_paths:
        with open(p, "rb") as f:
            data = f.read()
        w, h, depth, colour = png_size(data)
        if w != h or w > 256:
            raise ValueError("%s: icon images must be square and at most 256 px" % p)
        if depth != 8 or colour != 6:
            raise ValueError("%s: expected 8-bit RGBA PNG" % p)
        images.append((w, data))
    images.sort(key=lambda t: t[0])

    header = struct.pack("<HHH", 0, 1, len(images))  # reserved, type 1 = icon, count
    offset = 6 + 16 * len(images)
    entries = b""
    blobs = b""
    for size, data in images:
        b = 0 if size == 256 else size
        # width, height, colour count, reserved, planes, bit count, bytes, offset
        entries += struct.pack("<BBBBHHII", b, b, 0, 0, 1, 32, len(data), offset)
        blobs += data
        offset += len(data)
    with open(out_path, "wb") as f:
        f.write(header + entries + blobs)


def main():
    if len(sys.argv) == 1:
        build_ico("haetty.ico", ["png/haetty-%d.png" % s for s in DEFAULT_SIZES])
    elif len(sys.argv) >= 3:
        build_ico(sys.argv[1], sys.argv[2:])
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main()
