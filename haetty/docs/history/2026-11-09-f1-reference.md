# 2026-11-09 — F1: the newest Ghostty as the vtdiff reference

Stage f1 (DESIGN.md section 10.12; the numbers and the record are in docs/vtdiff/2026-11-09-f1.md).

- `tools/ghostty-ref`: the helper (C, pinned Ghostty e035beb, Zig 0.17.0), built with `tools/heavy.sh --label
  ghostty-ref -- bash tools/ghostty-ref/build.sh`: libghostty-vt built in 167 s; helper 2.36 MB stripped, libc only.
- `haetty-vtdiff --ref ghostty-main|ghostty-0.2.2` (default main), `--outcomes FILE`, `refs` in the known lists,
  `tools/vtdiff-sides.py` for the side-by-side join. The 0.2.2 run reproduces E5 (79 FAIL).
- CLAUDE.md: the delegation rule for additions to the known lists.
- Night run of 2026-11-09 (F0 rule, native): PASS on both platforms; see the F0 history file.
- Nothing in the native engine changed. F2 starts from 877 FAIL under ghostty-main.
