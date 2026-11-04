# engine-compare

Replays a byte stream through libghostty-vt and alacritty_terminal and prints
throughput and the final screen. Not part of the workspace.

```sh
python3 make_stress.py
cargo build --release --features ghostty --bin bench-ghostty
cargo build --release --features alacritty --bin bench-alacritty
target/release/bench-ghostty stress.bin 120 36 3
target/release/bench-alacritty stress.bin 120 36 3
```

A real Claude Code stream: record one with
`ccterm-headless --raw-log cc.rawlog ... -- claude` (no resize steps), then
replay it at the same size and compare with the screen saved by `save FILE`.
