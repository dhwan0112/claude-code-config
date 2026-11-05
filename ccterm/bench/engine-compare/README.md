# engine-compare

Replays a byte stream through libghostty-vt and alacritty_terminal and prints
throughput and the final screen. Not part of the workspace. libghostty-vt's `max_scrollback` is in bytes:
the defaults (11 MB vs 10 000 lines) keep about the same history at 120
columns (~10 200 vs 10 000 rows).

```sh
python3 make_stress.py
cargo build --release --features ghostty --bin bench-ghostty
cargo build --release --features alacritty --bin bench-alacritty
/usr/bin/time -v target/release/bench-ghostty stress.bin 120 36 3      # 5th arg: scrollback bytes (11000000)
/usr/bin/time -v target/release/bench-alacritty stress.bin 120 36 3    # 5th arg: scrollback lines (10000)
```

A real Claude Code stream: record one with
`ccterm-headless --raw-log cc.rawlog ... -- claude` (no resize steps), then
replay it at the same size and compare with the screen saved by `save FILE`.
