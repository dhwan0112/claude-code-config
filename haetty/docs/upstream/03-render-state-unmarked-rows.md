# Draft: render state does not mark rows changed by a later grapheme join

*Draft for an "Issue Triage" discussion. Not sent. See README.md first.*

## Summary

`ghostty_render_state_update` copies only the rows the terminal marked
dirty. Two changes leave their row unmarked, so a render state that is
updated incrementally keeps showing the old row until something else
changes it:

1. A combining mark, variation selector or ZWJ that joins a cell already
   written, outside mode 2027, when it arrives in a later
   `ghostty_terminal_vt_write` call than its base character.
2. `printCell` clearing the previous row's trailing `spacer_head` back to
   `narrow`, when a wide character or its tail in the first two columns is
   overwritten.

## Versions

- libghostty-vt (Rust crate) 0.2.2, built from Ghostty
  `a887df42c56f6de86c0fe6da9c4eeca37931e083`.
- Linux x86_64, Zig 0.15.2.
- On the pin `e035beb9437118b803830a5864ea2f4dd7094354` (2026-11-09) the two code paths
  still look the same (read in the source, then run; see below):
  - Terminal.zig, the non-2027 zero-width path ends in
    `appendGrapheme(prev, c)` without `cursorMarkDirty` (line 2182 at the pin).
    The mode 2027 path has one (line 2123).
  - Terminal.zig `printCell` sets `head_cell.wide = .narrow` on the row
    above without marking that row.

## Reproduction

Terminal 20×4, one render state kept across updates:

1. `ghostty_terminal_vt_write(t, "cafe")`
2. `ghostty_render_state_update(rs, t)`, then read row 0: `cafe`
3. `ghostty_terminal_vt_write(t, "\xcc\x81")` (U+0301 COMBINING ACUTE ACCENT)
4. `ghostty_render_state_update(rs, t)`, then read row 0 again

- **Expected:** row 0 is dirty (`GHOSTTY_RENDER_STATE_ROW_DATA_DIRTY`) and
  reads `café` (the last cell holds `e` + U+0301).
- **Actual:** row 0 is not dirty (`ROW_DATA_DIRTY` = 0), is not copied, and still reads
  `cafe`. A new render state updated from the same terminal reads `café`. The same happens
  for the VS16 cases and the split write; see the run below.

The same happens with:

- U+2764 then U+FE0F (VS16) in a separate write;
- U+263A then U+FE0F;
- the three bytes of U+FE0F split across two writes.

A ZWJ followed by a wide character in the next write is not affected, because
the wide character marks the row.

For case 2 (spacer head), at 80 columns:

1. Write `ESC [2;1H` followed by 79 × `w`. Row 2 is full except for column 80.
2. Write U+1F600 in its own call. It does not fit, so column 80 of row 2
   becomes a `spacer_head` and the emoji goes to row 3.
3. Update the render state.
4. Write `ESC [3;1H x` in its own call. This overwrites the emoji's first
   cell, and `printCell` turns row 2's `spacer_head` back into `narrow`.
5. Update the render state again.

- **Expected:** row 2 column 80 is narrow.
- **Actual:** the render state still has `spacer_head` there; a fresh one
  has `narrow`. This draws the same, but the cell differs.

Both cases are inputs in haetty's `vtdiff --refresh` (`joiners`).

## Run on the pin (2026-11-09)

`repro/03-repro.c` on `e035beb` (linked against the install folder that `tools/ghostty-ref/build.sh` builds), output
with trailing spaces trimmed:

```
control: cafe + U+0301 in one write            kept: dirty=1 "café"   fresh: "café"   same
cafe, then U+0301 in a later read              kept: dirty=0 "cafe"   fresh: "café"   STALE
U+2764, then U+FE0F in a later read            kept: dirty=0 "❤"      fresh: "❤️"     STALE
U+263A, then U+FE0F in a later read            kept: dirty=0 "☺"      fresh: "☺️"     STALE
U+2764, then U+FE0F split over two writes      kept: dirty=0 "❤"      fresh: "❤️"     STALE
spacer head cleared by a write (row 1, col 80) kept: spacer_head      fresh: narrow   STALE
```

The kept state is marked consumed after each read (`ghostty_render_state_clean`), as a frame renderer does, so its
dirty flag belongs to the update under test. The proposed fix (a) is absent on the pin: line 2182 still calls
`appendGrapheme` without `cursorMarkDirty`. The spacer case shows the cause (b) still present.

## Effect

In an application that updates the render state once per frame (as
recommended), a combining mark or emoji presentation selector that arrives in
a separate read stays invisible until the row changes again. Terminal programs
writing through a pty hit this whenever a read boundary falls between the
base and the mark.

haetty works around it by building a new render state for every snapshot.
Over 212 886 snapshots of fuzzed and recorded Claude Code output, the
incremental state was stale in 1 429 snapshots; a fresh one never was.

## Possible fix

Call `cursorMarkDirty()` before `appendGrapheme` on the non-2027 path as
well, and mark the previous row dirty where `printCell` changes its
`spacer_head`.
