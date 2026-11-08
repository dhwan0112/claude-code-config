# 2026-11-09 — F2: haetty-vt moved to the newest Ghostty, in part

Stage f2 (DESIGN.md section 10.13; numbers and causes in docs/vtdiff/2026-11-09-f2.md).

- F2a 73afba7: C0 controls 0x10–0x1F do nothing in ground state (ghostty-main FAIL 877 -> 852).
- F2b b0d999b: DCS payload from 0x80, UTF-8 C1 ignored, DECRQSS for SGR/DECSTBM/DECSCUSR/DECSLRM
  (ghostty-main FAIL 852 -> 744).
- Not done: kitty graphics responses, XTWINOPS size reports, other replies, grammar and mutated text.
- The 0.2.2 side is classified only (172 runs are 0.2.2 defects, where haetty agrees with ghostty-main).
- `GHOSTTY_REF_TARGET` in tools/ghostty-ref/build.sh keeps the helper build outside a stage worktree.
