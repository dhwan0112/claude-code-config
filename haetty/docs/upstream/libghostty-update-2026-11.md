# libghostty-vt after 0.2.2: what an update would change (E5e, survey only)

Surveyed 2026-11-08. haetty pins `libghostty-vt = "=0.2.2"` (Ghostty
`a887df4`, 2026-07-11). Nothing was changed: no version bump, no build
against a newer one.

## Releases

- crates.io: `libghostty-vt` 0.2.2 (2026-09-28) is still the latest; no
  release since. Releases so far: 0.1.0, 0.1.1, 0.2.0, 0.2.1, 0.2.2.
- The repository (`uzaaft/libghostty-rs`) `main` at `84373f8` (2026-11-06)
  has 167 commits after `v0.2.2`; its workspace version still reads 0.2.1.
- `main` builds Ghostty `0b8b998` (2026-10-28). That is **older** than the
  Ghostty `5813c94` (2026-11-06) of `docs/upstream/` and the current Ghostty
  `main` (`27ffca0`, 2026-11-07). So there is no release, nor even an
  unreleased `main`, built from Ghostty after `5813c94`.

## What building it would need

- Zig **0.17.0** (`0b8b998`'s `minimum_zig_version`; this machine has
  0.15.2, which builds `a887df4`). MSRV Rust 1.96 (we have 1.99.1).
- API changes that touch haetty (breaking commits in `v0.2.2..main`):
  - `Terminal::title` / `pwd` return bytes (`fb9378e`): `ghostty.rs`, 4 places;
  - `ClipboardContent::mime` is bytes (`8ea890e`): `on_clipboard_write`;
  - `graphemes_len` returns `u32` (`8363442`): compiles unchanged;
  - `ScrollViewport` is `non_exhaustive` (`75228f9`): we only construct it;
  - not used by haetty: `vt_write_until_ground` (`ec22b13`), the sys hook
    setters now `unsafe` (`a122276`), OSC `CommandType`, kitty graphics
    queries, paste `is_safe`, colour parser input as bytes.

## What it would change in vtdiff and the known lists

Read from Ghostty's history `a887df4..0b8b998` (not run):

- The three approved entries become unnecessary: TBC/CTC `unset` no longer
  toggles (`bed20eb`, 2026-08-18), and CHA, HPA, HPR, VPA, VPR keep the
  other coordinate origin-relative (`82df0be`, 2026-10-18). With them the
  candidate ctc-2-clears goes, and no-pending-wrap-without-autowrap too
  (`42a6564`, 2026-10-11). Removing entries needs the user's approval.
- New differences where haetty-vt copies libghostty 0.2.2 as it is; among
  the terminal changes in that range:
  - `53fb05c` CUB and BS stop at the left margin;
  - `d0c0d40` TBC without a parameter clears the stop at the cursor;
  - `6220a36` code points above 0xFF are printed unmapped in a charset
    (haetty-vt prints a space under DEC graphics, as 0.2.2);
  - `bfd40c8` CSI 2 K resets the row's wrap state;
  - `c706451` a single shift applies to exactly one printed character;
  - `faa346b` printing after a wrap uses the right margin;
  - `f9ab34f`, `afded91`, `e6db5b6`, `b10ba7e`, `6dae79d`: resize and
    reflow fixes (pending wrap, saved cursor, wide characters, graphemes);
  - kitty: deletions as the spec says (`f8b40a0`), placements clipped
    within margins (`90bce0d`), erased history removes placements
    (`9cac2bd`), limits kept across a full reset (`bcbc93a`);
  - the formatter: hyperlinks in VT output (`510b253`), blank cells and
    erased backgrounds restored in VT output (`152bdbc`), trailing blank
    rows kept (`bd1c9cc`), pending wrap restored (`997a2af`, `bae1c01`),
    origin-relative cursor (`23c7a3e`). Dumps would differ until
    `format.rs` follows.
  In all, 234 commits touched the terminal, stream, parser, formatter or
  kitty code in that range.
- Finding 3 (`docs/upstream/03`) is still in Ghostty `main` (`27ffca0`):
  printCell's spacer-head change and the non-2027 appendGrapheme mark no
  row dirty. `eb722cb` (2026-08-28) fixed the same for ECH and DCH
  (splitCellBoundary), not for print.

## Assessment

An update means Zig 0.17, four small source changes, and re-deriving a
good part of haetty-vt's "as libghostty" behaviour from a moving target
(234 terminal-side commits). The spec-related KNOWN entries would
disappear. Until a release built from a Ghostty after `5813c94` exists,
there is little to gain from following `main`.
