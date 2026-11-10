# tools/ghostty-ref: the newest Ghostty as the vtdiff reference

haetty-vtdiff compares haetty's engine with a reference engine on the same bytes. Until E5 the reference was
libghostty-vt 0.2.2, linked into the test binary. From F1 the default reference is `ghostty-main`: the newest Ghostty,
pinned below, built as a separate helper program (`ghostty-ref`). `ghostty-0.2.2` stays available
(`haetty-vtdiff --ref ghostty-0.2.2`) for the side-by-side report.

This is test tooling. Nothing here is part of haetty's build, its dist or its installers, and no product code uses it.
It needs Zig 0.17.0 and a checkout of the Ghostty source at the pin. It is a separate program because libghostty-vt 0.2.2
and the newest one both declare the link name `ghostty-vt`: they cannot be linked into one binary.

## Pins

| | |
|---|---|
| Ghostty | `e035beb9437118b803830a5864ea2f4dd7094354` (main, 2026-11-08: "Add NSColorTests for NSColor+Extension (#15139)") |
| Zig | 0.17.0 (ziglang.org), `sha256 1cbe9df9f27e6b78d14ccbca43b6703a404ef79ef1c463de901d7f088d4e2026` for `zig-x86_64-linux-0.17.0.tar.xz`; installed at `~/.local/zig-0.17.0/zig` (`$HAETTY_ZIG_REF`, `tools/machine.sh`) |
| Checkout | `$GHOSTTY_SRC`, default `$HAETTY_WORK/ghostty-ref/ghostty` (`~/m4/ghostty-ref/ghostty`): a clean checkout at the pin, outside the repository and outside /tmp |

Zig 0.15.2 (the one haetty's own builds use, `~/.local/bin/zig`) is not used for this. `build.sh` refuses any other Zig.

## Build

    tools/heavy.sh --label ghostty-ref -- bash tools/ghostty-ref/build.sh

It checks the pin and that the checkout is clean, builds libghostty-vt (static, `-Dcpu=baseline`, ReleaseFast) into
`target-ghostty-ref/install`, links `ghostty-ref.c` against it, and writes the stripped helper to
`target-ghostty-ref/ghostty-ref`. `target-ghostty-ref/` is git-ignored. Set `GHOSTTY_REF_TARGET` to build elsewhere (outside a stage worktree, which
a finished stage removes). haetty-vtdiff looks for the helper in `target-ghostty-ref/`, or at `$HAETTY_GHOSTTY_REF`;
without it, `--ref ghostty-main` stops with the build command.

## Protocol

One request per line on stdin, one answer per request on stdout: a value line or `ok`; `err <text>` when refused.

| request | answer |
|---|---|
| `init COLS ROWS CELL_W CELL_H SCROLLBACK_BYTES` | `ok` (first) |
| `feed HEX` | `ok` |
| `resize COLS ROWS CELL_W CELL_H` | `ok` |
| `replies` | `replies HEX`: the bytes the terminal wrote back since the last call |
| `snap` | the lines below, then `end` |
| `scroll top`, `scroll bottom`, `scroll delta N` | `ok` |
| `sbrows` | `sbrows N`: the rows above the screen |
| `dump plain`, `dump vt`, `dump html` | `dump HEX`: the formatter output (trimmed, whole screen) |
| `quit` | exits |

Lines of `snap`:

- `size COLS ROWS`
- `colors FG BG CURSOR`: RRGGBB, or `-` for none
- `title HEX`, `pwd HEX`
- `modes ALT MOUSE_TRACKING SGR_MOUSE BRACKETED_PASTE FOCUS SYNC_OUTPUT APP_CURSOR CURSOR_VISIBLE KITTY_FLAGS`, 0/1 each
  (the last one a number)
- `scroll TOTAL OFFSET LEN`
- `cursor X Y SHAPE BLINKING`, or `cursor none`. SHAPE: 0 block, 1 bar, 2 underline, 3 hollow block
- `r Y CELL...`, once per row. A cell is `flags:width:fg:bg:ul:ulk:ulc:text:link`:
  - `flags`: haetty's CellFlags bits (`haetty-core` `CellFlags`), with bit 9 for a selected cell
  - `width`: 0 narrow, 1 wide, 2 spacer (head or tail)
  - `fg`, `bg`, `ulc`: RRGGBB or `-`
  - `ul`: 0/1, `ulk`: the underline kind 1 to 5 (single, double, curly, dotted, dashed)
  - `text`: hex of the UTF-8, `-` when empty
  - `link`: hex of the hyperlink URI, `-` when none

`init` gives the terminal what vtdiff's `init()` gives haetty's engine: colours fg `cccccc`, bg `141414`, the built-in
palette, no cursor colour; the XTVERSION answer `haetty(0.3.0)` (the crate version; vtdiff derives its side from `CARGO_PKG_VERSION`); the scrollback limit in bytes as given. The colours and
the XTVERSION answer are constants of `ghostty-ref.c`: change them there and in `init()` together.

## How it matches haetty

- Every snapshot takes a new render state (haetty-core's `snapshot_fresh`).
- The terminal runs on main's defaults, except that nothing is switched on that haetty does not do. Title report
  (CSI 21 t) and the glyph protocol stay off: haetty answers neither.
- Synchronized output (mode 2026) ends on every valid resize, as main does. 0.2.2's C API did the same
  (`ghostty_terminal_resize` disables it). The helper puts nothing back.
- The snapshot follows `haetty-core/src/ghostty.rs` (`GhosttyEngine::snapshot`) field by field.
- Not in the protocol: the encoders (keys, mouse, paste, focus), colour-scheme reports, events and mode queries.
  vtdiff's `--encoders`, `--colors`, `--gen-keys`, `--gen-width` and `--refresh` stay on the 0.2.2 engine in process.

## Refreshing the pin

1. `git -C "$GHOSTTY_SRC" fetch origin && git -C "$GHOSTTY_SRC" checkout <new commit>`.
2. Set `PIN` in `build.sh` and in the table above. Build. If the build fails, the names that moved are in the compiler
   output: fix `ghostty-ref.c`.
3. Run `haetty-vtdiff --ref ghostty-main` on the corpus and record the counts in `docs/vtdiff/`.
4. Check the C API changes (`git diff --stat OLD NEW -- include/`) and the entries of `docs/vtdiff/known-vt-differences.txt`
   that name `ghostty-main` (their reasons may no longer hold).
