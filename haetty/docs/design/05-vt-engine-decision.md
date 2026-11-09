# haetty design: the VT engine decision (section 4)

> Part of the design. Index: [`../DESIGN.md`](../DESIGN.md) (section
> numbers are the same there). Text below is moved verbatim from the former `docs/DESIGN.md`.

## 4. VT engine decision (measured 2026-11-04)

Replay of a real Claude Code 2.1.318 session (startup, Hangul typing,
Shift+Enter, fullscreen renderer; 2.5 KB of output) at 120x36:
libghostty-vt and alacritty_terminal 0.26 produced identical screens, equal
to the live screen. A 16.9 MB synthetic stream (24-bit colour, Hangul, CJK,
emoji, box drawing, synchronized updates) x3 at 120x36, one vCPU:
libghostty-vt ~31–34 MB/s, alacritty_terminal ~50–73 MB/s; identical final
screens. Building libghostty-vt for `x86_64_v3` instead of `baseline` did
not change throughput measurably. Minimal stripped binaries (engine +
replay): 2.0 MB vs 0.6 MB.

The memory figures of that run (max RSS ~20 MB vs ~47 MB) are void: the
benchmark gave libghostty-vt `max_scrollback = 10000`, which is bytes
(about 680 rows of history), and alacritty_terminal 10 000 lines. They are
no argument for either engine. Re-measured 2026-11-05 with about the same
history in both (`bench/engine-compare`, same stream x3 at 120x36, all
cores, three runs; max RSS includes the 16.9 MB input held in memory):

| Engine | Scrollback | Max RSS | Throughput |
|---|---|---|---|
| libghostty-vt 0.2.2 | 11 000 000 bytes (≈10 200 rows of 120 columns) | 31.5–31.6 MB | 32.5–37.1 MB/s |
| alacritty_terminal 0.26 | 10 000 lines | 47.6 MB | 110–120 MB/s |

Both max RSS figures include the same 16.9 MB input file held in memory;
without it they are roughly 31.6 − 16.9 ≈ 15 MB (libghostty-vt) and
47.6 − 16.9 ≈ 31 MB (alacritty_terminal): with about the same history
libghostty-vt uses about half the memory. This is an approximation — the
subtraction assumes the input buffer is fully resident in both and ignores
other differences (binary size 2.0 vs 0.6 MB, allocator behaviour, ~2 %
more rows kept by libghostty-vt). (The old condition, libghostty-vt with
10 000 bytes: 20.6 MB.) The final
screens of the two replays differ only in which 36 rows the harnesses
print (one row offset); the rows themselves match.

Claude Code's fullscreen renderer emits kilobytes per interaction, so
throughput is not the constraint for this workload; memory, correctness of
widths (emoji presentation, graphemes) and the built-in key/mouse encoders
and query replies are. Decision: keep libghostty-vt as the only non-Rust
component, isolated in `haetty-core`; keep `Snapshot` engine-neutral so an
alacritty_terminal backend can be added and A/B-tested later if the Zig
toolchain becomes a burden. Re-run the comparison on Windows once the GUI
exists.
