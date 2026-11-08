# 2026-11-09 — F3: SGR-heavy throughput

Stage f3 (DESIGN.md section 10.14).

- Session, `e5c-tput.sh sgr` (112 MiB, five runs, median): native 41 -> 57 MiB/s; libghostty 73 -> 69 MiB/s.
  Ratio 56 % -> 82 %.
- Engine alone, `examples/feed sgr`: 56 -> 92 MiB/s. vte alone on the input: 468 MiB/s.
- Changes: SGR parameters parsed in one pass into reused buffers; CSI parameter runs taken in one step; the erased
  form of a default-background style is id 0.
- vtdiff outcome files identical to F2's (ghostty-main and ghostty-0.2.2, 2067 lines each).
