# Known differences between libghostty-vt and haetty-vt

vtdiff compares haetty-vt (the native engine) with libghostty-vt (the
reference) cell by cell. A few differences are on purpose: libghostty
departs there from ECMA-48 and xterm, and haetty-vt follows the
specification. Each such difference is one entry below.

- `known-vt-differences.txt` holds the entries the user approved; vtdiff
  counts a run that one of them explains as KNOWN.
- `known-candidates.txt` holds proposals. vtdiff counts their runs as FAIL
  and marks them "KNOWN if approved".
- Adding or changing an entry in the approved list needs the user's
  approval (CLAUDE.md).

A run is KNOWN only if all three hold:

1. an entry's pattern occurs in the input;
2. every field that differed is among the matching entries' fields;
3. the run is identical when haetty-vt is switched to do what libghostty
   does (`haetty_vt::Compat`, used by vtdiff only).

Point 3 keeps an unrelated bug from hiding behind a known pattern. vtdiff's
self-test (`cargo test -p haetty-vtdiff`) adds such a bug and expects FAIL.

## Approved

### tbc-0-clears — TBC 0 clears the tab stop at the cursor

- What happens: `CSI 0 g` at a column without a tab stop.
  - libghostty sets a stop there: its `Tabstops.unset` XORs the bit, so it
    toggles.
  - haetty-vt leaves the column without a stop. Later tabs land
    elsewhere.
- The specification:
  - ECMA-48 (5th edition, 1991) §8.3.154 TBC, parameter value 0: "the
    character tabulation stop at the active presentation position is
    cleared".
  - xterm ctlseqs: "CSI Ps g Tab Clear (TBC). … Ps = 0 ⇒ Clear Current
    Column (default)".
- Approved 2026-11-07, in the user's E4 instructions: "발견 1·2(TBC 0,
  원점 모드의 CHA·HPA·HPR): 새 엔진은 규격(ECMA-48, xterm)을 따른다.
  libghostty 동작을 따라 하지 마. … 발견 1·2는 이번 지시로 승인함".

### origin-column-moves-only-column — CHA, HPA and HPR under origin mode

- What happens: with origin mode set (DECOM, `CSI ? 6 h`) and a scroll
  region not starting at the top, CHA (`CSI n G`), HPA (`CSI n \``) or HPR
  (`CSI n a`).
  - libghostty also moves the cursor down by the region's top. It
    implements them as `setCursorPos(cursor.y + 1, column)`, and under
    origin mode `setCursorPos` adds the region's top to the row.
  - haetty-vt changes only the column.
- The specification:
  - ECMA-48 §8.3.9 CHA: moves "to character position n in the active line".
  - ECMA-48 §8.3.57 HPA: moves "to character position n in the active line".
  - ECMA-48 §8.3.59 HPR: moves "by n character positions … in the direction
    of the character progression".
  - None of the three changes the line.
  - xterm ctlseqs: CHA and HPA "[column] (default = [row,1])"; HPR
    "[columns] (default = [row,col+1])".
- Approved 2026-11-07, in the user's E4 instructions (as above).

### origin-row-moves-only-row — VPA and VPR under origin mode

- What happens: with origin mode set and left/right margins (DECLRMM,
  DECSLRM), VPA (`CSI n d`) or VPR (`CSI n e`).
  - libghostty also moves the cursor right by the left margin. It
    implements both as `setCursorPos(row, cursor.x + 1)`, and under origin
    mode `setCursorPos` adds the left margin to that column.
  - For VPR, libghostty also adds the region's top to the row; that shows
    without margins too.
  - haetty-vt changes only the row.
  - This is the same mechanism as the approved entry above, on the other
    axis.
- The specification:
  - ECMA-48 §8.3.158 VPA: moves "to line position n in the active character
    position".
  - ECMA-48 §8.3.160 VPR: moves "by n line positions … in a direction
    parallel to the line progression".
  - Neither changes the column.
  - xterm ctlseqs: VPA "[row] (default = [1,column])"; VPR "[rows]
    (default = [row+1,column])".
- Found in E4 once margins were in vtdiff's inputs (`\e[?69h\e[3;7s\e[?6h\e[3dX`).
- Ghostty `main` (`5813c94`) routes VPA and VPR through `cursorPosForSet`,
  which would fix it as well (docs/upstream/).
- Approved 2026-11-08, in the user's E5 instructions: "VPA·VPR
  후보(origin-row-moves-only-row)를 known-vt-differences.txt로 옮기는 것을
  승인한다."

## Candidates (not approved; counted FAIL)

### no-pending-wrap-without-autowrap — DECAWM off and the last column

- What happens: with autowrap reset (DECAWM, `CSI ? 7 l`) a character is
  written in the last column; later autowrap is set again and another
  character is written.
  - libghostty sets the pending wrap anyway, so that character goes to
    the next row.
  - haetty-vt sets no pending wrap without autowrap: it replaces the
    character in the last column.
- The specification:
  - DEC STD 070 and the VT510 manual, DECAWM reset: characters received
    when the cursor is at the right margin replace previously displayed
    characters.
  - xterm sets its wrap flag only with wraparound on.
- Found in E5a (fuzz2): `\e[?7l` + a full row + `\e[?7h` + a character.
  With `pending-wrap-without-autowrap` on, haetty-vt matches libghostty.
- Ghostty fixed it after the version we compare with: `42a6564` ("clear
  pending wrap when printing with DECAWM off", 2026-10-11).
- Pattern `DECRST 7` (new in E5a: a reset of DEC private mode 7).

### ctc-2-clears — CTC 2

- What happens: CTC with parameter 2 (`CSI 2 W`) at a column without a
  tab stop.
  - libghostty sets one there: it clears with `Tabstops.unset`, which
    toggles, as in the approved entry tbc-0-clears.
  - haetty-vt leaves the column without a stop.
- The specification: ECMA-48 §8.3.17 CTC, Ps = 2: "the character
  tabulation stop in the active line is cleared at the active
  presentation position".
- Found in E5a with `--diverge` (grammar): haetty-vt had no CTC at all
  until E5a; it now takes 0 (or none), 2 and 5 as libghostty does, and
  DECST8C (`CSI ? 5 W`).
- Ghostty fixed `unset` in `bed20eb` (2026-08-18), as for tbc-0-clears.
