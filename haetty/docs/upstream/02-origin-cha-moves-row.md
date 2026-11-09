# Draft: CHA, HPA and HPR under origin mode move the cursor's row

*Fixed on Ghostty `main` by `82df0be` ("terminal: keep CHA, VPA, HPR and VPR origin-relative"),
an ancestor of the pin `e035beb` (checked 2026-11-09): `setCursorCol` and `cursorColRelative`
now take the row from the new `cursorPosForSet`, which subtracts the region's top under origin
mode; VPA and VPR do the same. Re-run on `e035beb`: the `B` is on the row of `A`, not four rows
lower (`repro/show.c`). Kept for the record; do not send. See README.md.*

## Versions

libghostty-vt 0.2.2, Ghostty `a887df42c56f6de86c0fe6da9c4eeca37931e083`.

Re-run on the pin `e035beb9437118b803830a5864ea2f4dd7094354` (2026-11-09): fixed. See the header.

## Reproduction (80×24)

```
\e[5;10r \e[?6h \e[3;1H A \e[20G B
```

- **Expected:** `B` in column 20 of the same row as `A` (screen row 7).
- **Actual:** `B` four rows lower (screen row 11).
  - `stream_terminal.zig` maps CHA to
    `setCursorPos(cursor.y + 1, value)`.
  - Under origin mode `setCursorPos` adds the region's top to that absolute
    row.
  - HPA (`CSI n \``) and HPR (`CSI n a`) take the same path.
  - So do VPA and VPR the other way round: with left/right margins, the
    column moves right by the left margin.

## Basis

- ECMA-48 §8.3.9 CHA: moves "to character position n in the active line".
- ECMA-48 §8.3.57 HPA: moves "to character position n in the active line".
- ECMA-48 §8.3.59 HPR: moves "by n character positions … in the direction
  of the character progression".
- None of the three changes the line.
- xterm ctlseqs: CHA and HPA "[column] (default = [row,1])"; HPR
  "[columns] (default = [row,col+1])".
