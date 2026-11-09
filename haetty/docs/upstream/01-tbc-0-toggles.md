# Draft: TBC 0 toggles the tab stop instead of clearing it

*Fixed on Ghostty `main` by `bed20eb` ("terminal: clear tabstop bit on unset instead of
toggling"), an ancestor of the pin `e035beb` (checked 2026-11-09). Re-run on `e035beb`: the `X`
lands at the default stop, not in column 5 as before (`repro/show.c`). Kept for the record;
do not send. See README.md.*

## Versions

libghostty-vt 0.2.2, Ghostty `a887df42c56f6de86c0fe6da9c4eeca37931e083`.

Re-run on the pin `e035beb9437118b803830a5864ea2f4dd7094354` (2026-11-09): fixed. See the header.

## Reproduction (80×24)

```
\e[5G \e[0g \r \t X
```

- **Expected:** `X` in column 9, the default stop. Column 5 had no stop and
  TBC 0 leaves it without one.
- **Actual:** `X` in column 5. `Tabstops.unset` XORs the bit
  (`prealloc_stops[i] ^= masks[idx]`), so a column without a stop gains one.

## Basis

- ECMA-48 (5th edition, 1991) §8.3.154 TBC, parameter value 0: "the
  character tabulation stop at the active presentation position is cleared".
- xterm ctlseqs: "CSI Ps g Tab Clear (TBC). … Ps = 0 ⇒ Clear Current Column
  (default)".
