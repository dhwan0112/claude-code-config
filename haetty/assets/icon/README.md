# haetty application icon

All generated files in this directory are committed, so building haetty needs
no SVG rasterizer or other conversion tool. The tools below are only needed to
regenerate the files after a design change.

## Files

| File | What it is |
| --- | --- |
| `haetty.svg` | The master drawing (256×256 canvas). |
| `png/haetty-N.png` | Renders of `haetty.svg` at N = 16, 20, 24, 32, 40, 48, 64, 128, 256, 512 px (8-bit RGBA, transparent margins). |
| `haetty.ico` | Windows icon with the sizes 16, 20, 24, 32, 40, 48, 64, 256 (PNG-compressed entries). |
| `tools/pngio.py` | Minimal PNG reader/writer (Python standard library only). |
| `tools/build_ico.py` | Packs the PNGs into `haetty.ico`. |
| `tools/magnify.py` | Nearest-neighbour upscaler used for the small-size check. |
| `rgba/haetty-64.rgba` | The 64 px render as raw RGBA (64×64×4 bytes, row-major, no header): the X11 window icon, embedded so haetty needs no PNG decoder at run time. |
| `tools/rgba.py` | Writes that raw RGBA from a PNG. |
| `check/*.png` | Magnified copies of the 16/24/32/48 px renders used to review small sizes. |

The 20 and 40 px renders exist only for the `.ico` (Windows uses them at 125 %
scaling), but they are kept as `png/haetty-20.png` / `png/haetty-40.png` so the
`.ico` can be rebuilt from committed files alone.

No file is hand-adjusted. Every PNG is the unmodified resvg output; the review
of the 16, 24, 32 and 48 px renders (see "Small-size check") found the prompt
`❯`, the underscore cursor and the top bar recognisable at every size.

## Source

The design is the user's own (`haetty-E-grey.svg`). The design file carried a
`<metadata><c2pa:manifest>…</c2pa:manifest></metadata>` element (a
content-credential manifest, about 7.7 KB of base64). For `haetty.svg` only
that `<metadata>` element was removed; every other byte, including the root
`<svg>` tag with its `xmlns:c2pa` declaration and every drawing element,
attribute and value, is identical to the design file. Result: 1434 bytes.

The removal was done with:

```sh
python3 - <<'EOF'
import pathlib
src = pathlib.Path("haetty-E-grey.svg").read_bytes()
a = src.index(b"<metadata>")
b = src.index(b"</metadata>") + len(b"</metadata>")
pathlib.Path("haetty.svg").write_bytes(src[:a] + src[b:])
EOF
```

## Tools and versions

- resvg 0.49.0 (CLI), installed with `cargo install resvg --root ~/m4/tools`
  (cargo 1.99.1, rustc 1.99.1), binary at `~/m4/tools/bin/resvg`.
- Python 3.12.3, standard library only (`struct`, `zlib`).
- Produced on WSL Ubuntu, 2026-11-05.

Byte-identical PNGs need the same resvg version.

## Commands

Run from `assets/icon/`:

```sh
# 1. PNG renders
for n in 16 20 24 32 40 48 64 128 256 512; do
  resvg --width $n --height $n haetty.svg png/haetty-$n.png
done

# 2. Windows icon (reads png/haetty-{16,20,24,32,40,48,64,256}.png)
python3 tools/build_ico.py

# 3. Raw RGBA for the X11 window icon
python3 tools/rgba.py png/haetty-64.png rgba/haetty-64.rgba

# 4. Magnified check images (nearest neighbour; 16 px x16, 24 px x12, 32 px x8, 48 px x6)
python3 tools/magnify.py png/haetty-16.png 16 check/haetty-16-rendered-x16.png
python3 tools/magnify.py png/haetty-24.png 12 check/haetty-24-rendered-x12.png
python3 tools/magnify.py png/haetty-32.png  8 check/haetty-32-rendered-x8.png
python3 tools/magnify.py png/haetty-48.png  6 check/haetty-48-rendered-x6.png
```

`tools/build_ico.py` can also be called as
`python3 tools/build_ico.py OUT.ico IN1.png IN2.png ...`. It writes an ICONDIR
(6 bytes), one 16-byte ICONDIRENTRY per image (sorted by size, planes 1,
32 bpp, width/height byte 0 for the 256 px entry) and then each PNG file
unchanged as the entry data. PNG-compressed entries are valid since Windows
Vista. The entries were checked to decode with the Windows WPF
`IconBitmapDecoder` (all eight frames, 16 to 256 px, Bgra32).

If a small size ever has to be hand-adjusted, store the adjusted file as
`png/haetty-N.png`, keep the plain resvg output as `png/rendered/haetty-N.png`,
skip that size in step 1, and rebuild the `.ico` with step 2.

## Small-size check

Each small render was magnified with nearest-neighbour scaling (pixels copied
as blocks, alpha kept) and inspected:

- 16 px (`check/haetty-16-rendered-x16.png`): the bar is two orange rows,
  bright on the left and fading to the right; the `❯` is a one-pixel diagonal
  staircase, the cursor a four-pixel line on the `❯`'s lower baseline with a
  two-pixel gap. The stroke is about one pixel wide, so the symbols render in a
  mid grey (about #84807C) rather than full #B4B2AF. Recognisable; not adjusted.
- 24 px (`check/haetty-24-rendered-x12.png`): clear `❯` and cursor; the
  cursor covers one full row plus two partial rows. Not adjusted.
- 32 px (`check/haetty-32-rendered-x8.png`) and 48 px
  (`check/haetty-48-rendered-x6.png`): clean, no issues.

## Where haetty uses them

- `crates/haetty-gui/build.rs`: links `haetty.ico` into `haetty.exe` (icon
  resource 1, via mingw's `windres`); the window's small and big icons on
  Windows are loaded from that resource.
- `crates/haetty-gui/src/icon.rs`: embeds `rgba/haetty-64.rgba` (X11 window
  icon), the hicolor PNGs 16–512 and `haetty.svg` for `haetty install-desktop`.
