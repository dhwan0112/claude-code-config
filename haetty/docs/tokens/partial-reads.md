# Partial reads and big files (stage H2, 2026-11-09)

Reading a whole big file costs its size in input tokens on every response after it: the context is read again each time
(see [per-stage-method.md](per-stage-method.md)). The rules:

1. **Find, then read.** Search first (`Grep` with `output_mode: content`, a small context and a `head_limit`; or
   `grep -n`), then `Read` with `offset` and `limit` around the lines found (150 to 300 lines per read).
2. **No whole reads of big files** (over 1000 lines) unless the task needs all of them. `Read` returns 2000 lines by
   default.
3. **Docs.** `docs/DESIGN.md` is the index: open one file of `docs/design/`. `docs/HANDOFF.md` is read whole (it is short
   by rule: under 400 lines). `docs/history/` is read by section, when a stage names it.
4. **Build folders and the old copy are not read.** `.claude/settings.json` denies `Read` of `target/` and `target-*/`;
   the old `ccterm` copy is archived at `~/archive/ccterm-2026-11-09`, outside the projects.
5. **Logs.** `haetty log` in parts; `haetty wait` for a job, not a loop of reads.
6. **Generated files** (`crates/haetty-vt/src/grapheme_table.rs`, from `tools/gen-grapheme-table.py`): read the header only.

## Big files (lines, 2026-11-09)

| file | lines | plan (each split is a stage of its own; behaviour unchanged; the gate on each) |
|---|---|---|
| `crates/haetty-vt/src/lib.rs` | 4185 | split by part: the parser, the grid and modes, the public API |
| `crates/haetty-vtdiff/src/main.rs` | 2786 | a tool: split into the command, the comparison and the report modules |
| `crates/haetty-gui/src/app.rs` | 2512 | split: tabs, input, the palette and actions, the status and task glue |
| `crates/haetty-vt/src/kitty.rs` | 1387 | split: the protocol parser and the encoder |
| `crates/haetty-gui/src/ui.rs` | 1272 | split: the tab bar, the status row, the panels, the tests |
| `crates/haetty-gui/src/render/paint.rs` | 1197 | split by element: text, cursor, decorations |
| `crates/haetty-proto/src/wire.rs` | 1052 | split: the session and Claude types, keys and mouse, the frames |
| `docs/history/2026-11-06-07-stages-t-u-v.md` | 1139 | history: read by section |
| `README.md` | 1001 | keep: the first 60 lines say what the product is |
| `crates/haetty-vt/src/grapheme_table.rs` | 1706 | generated: read the header only |

The plans are not carried out in stage H; the reading rules above apply now.
