"""Minimal PNG reader/writer (Python standard library only).

Reads 8-bit non-interlaced PNGs of colour type 6 (RGBA) or 2 (RGB),
which covers resvg's output. Writes 8-bit RGBA PNGs.
"""

import struct
import zlib

PNG_SIG = b"\x89PNG\r\n\x1a\n"


def _chunks(data):
    if data[:8] != PNG_SIG:
        raise ValueError("not a PNG file")
    pos = 8
    while pos < len(data):
        (length,) = struct.unpack(">I", data[pos:pos + 4])
        ctype = data[pos + 4:pos + 8]
        body = data[pos + 8:pos + 8 + length]
        yield ctype, body
        pos += 12 + length


def png_size(data):
    """Return (width, height, bit_depth, colour_type) from the IHDR chunk."""
    for ctype, body in _chunks(data):
        if ctype == b"IHDR":
            w, h, depth, ctype_ = struct.unpack(">IIBB", body[:10])
            return w, h, depth, ctype_
    raise ValueError("no IHDR chunk")


def read_png(path):
    """Return (width, height, rows) where rows[y] is a list of (r, g, b, a)."""
    with open(path, "rb") as f:
        data = f.read()
    idat = b""
    for ctype, body in _chunks(data):
        if ctype == b"IHDR":
            w, h, depth, colour, _comp, _filt, interlace = struct.unpack(">IIBBBBB", body)
        elif ctype == b"IDAT":
            idat += body
    if depth != 8 or colour not in (2, 6) or interlace != 0:
        raise ValueError("unsupported PNG format (need 8-bit RGB/RGBA, non-interlaced)")
    bpp = 4 if colour == 6 else 3
    raw = zlib.decompress(idat)
    stride = w * bpp
    prev = bytearray(stride)
    rows = []
    pos = 0
    for _y in range(h):
        ftype = raw[pos]
        line = bytearray(raw[pos + 1:pos + 1 + stride])
        pos += 1 + stride
        for i in range(stride):
            a = line[i - bpp] if i >= bpp else 0
            b = prev[i]
            c = prev[i - bpp] if i >= bpp else 0
            if ftype == 1:
                line[i] = (line[i] + a) & 0xFF
            elif ftype == 2:
                line[i] = (line[i] + b) & 0xFF
            elif ftype == 3:
                line[i] = (line[i] + ((a + b) >> 1)) & 0xFF
            elif ftype == 4:
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                pr = a if (pa <= pb and pa <= pc) else (b if pb <= pc else c)
                line[i] = (line[i] + pr) & 0xFF
            elif ftype != 0:
                raise ValueError("bad filter type %d" % ftype)
        prev = line
        if bpp == 4:
            rows.append([tuple(line[x * 4:x * 4 + 4]) for x in range(w)])
        else:
            rows.append([tuple(line[x * 3:x * 3 + 3]) + (255,) for x in range(w)])
    return w, h, rows


def write_png(path, w, h, rows):
    """Write rows (list of lists of (r, g, b, a)) as an 8-bit RGBA PNG."""
    raw = bytearray()
    for row in rows:
        raw.append(0)
        for px in row:
            raw.extend(px)

    def chunk(ctype, body):
        return (struct.pack(">I", len(body)) + ctype + body
                + struct.pack(">I", zlib.crc32(ctype + body) & 0xFFFFFFFF))

    out = PNG_SIG
    out += chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0))
    out += chunk(b"IDAT", zlib.compress(bytes(raw), 9))
    out += chunk(b"IEND", b"")
    with open(path, "wb") as f:
        f.write(out)
