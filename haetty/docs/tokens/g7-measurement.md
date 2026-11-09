# G7: the same small task before and after G0–G6, measured with `haetty tokens`

Stage G7. One run of the same task in each state of the repository. The report has the method, the numbers and the
limits; it has no conversation content (names, paths, sizes and counts only).

## What was compared

- **Before**: commit `3396d5c` (F6). The session-start documents are whole: `docs/DESIGN.md` 135,077 bytes, `CLAUDE.md`
  204 lines. None of the G1–G6 tools exist.
- **After**: commit `6324aac` (G6). `docs/DESIGN.md` is the index, 8,031 bytes; `CLAUDE.md` is 123 lines; the G1–G6 tools
  and the saving mode (off, the default) are in place.
- The G0 estimate of the session-start documents (chars / 4, `docs/history/2026-11-09-g0-handoff-trim.md`): 45,737
  before, 12,296 after.

## Method

- The task, the same text in both runs (`docs/tokens/g7-task.txt` is this text):

  > Add a small percent helper to the haetty-cc crate, with tests.
  > 1. Create crates/haetty-cc/src/percent.rs with `pub fn percent(part: u64, whole: u64) -> f64`: the percentage that
  >    `part` is of `whole`; 0.0 when `whole` is 0.
  > 2. Add `pub mod percent;` to crates/haetty-cc/src/lib.rs, in the module list, keeping the order.
  > 3. Add unit tests in percent.rs: 0 of 0 is 0.0; 1 of 4 is 25.0; 3 of 3 is 100.0.
  > 4. Run `cargo test -p haetty-cc --no-default-features percent -q` from the haetty folder and fix anything that fails.
  >    Do not run the whole test suite. Do not commit anything.
- Each state was extracted with `git archive` into its own folder (`...\g7\before\haetty`, `...\g7\after\haetty`).
- The run: Claude Code 2.1.319 on Windows (the logged-in install), print mode, `--model claude-sonnet-5-5`,
  `--output-format json`, `--permission-mode acceptEdits`, and an allow-list of `Read Edit Write Glob Grep` and
  `Bash(cargo:*) Bash(ls:*) Bash(cat:*)`. Each run held a heavy slot (`tools/heavy.ps1`); the two ran at the same time.
- The measurement: `haetty tokens --top 10 <transcript>` on each transcript, with the same build for both.

## Results

| | before | after |
|---|---:|---:|
| API responses | 3 | 4 |
| tool calls | 4 | 5 (one denied) |
| input | 6 | 8 |
| output | 1,078 | 1,119 |
| cache write | 35,328 | 32,218 |
| cache read | 114,063 | 157,962 |
| sum | 150,475 | 191,307 |
| cache hit | 76.3 % | 83.1 % |
| cache write of the first response | 30,982 | 28,705 |
| turns (run output) | 5 | 6 |
| cost (run output, USD) | 0.1635 | 0.1559 |
| permission denials | none | 1 (PowerShell) |
| test result (the agent's report) | 3 passed | 3 passed |

Largest tool results (bytes): before, `lib.rs` read 488, `cargo test` 188, `lib.rs` edit 186, `percent.rs` write 180;
after, `lib.rs` read 511, `cargo test` 187, `lib.rs` edit 185, `percent.rs` write 179, a PowerShell call 111.

Neither run read `docs/DESIGN.md`, `docs/HANDOFF.md` or any other document: the task did not need them. Neither run used
`haetty run` or `haetty wait`.

## What the numbers show

- **Session start.** The first response's cache write is 2,277 tokens smaller after (−7.3 %). That is the `CLAUDE.md`
  difference (204 → 123 lines), the part of the G0 change this task reached. The `DESIGN.md` change (135 KB → 8 KB)
  would show only in a session that reads it.
- **Totals.** The after sum is higher (191,307 against 150,475). The after run had one more API response: a PowerShell
  call was denied by the allow-list, and the agent then ran the test through Bash. This sample does not show whether
  that was chance or the after state's guidance. It does not show a saving for the task.
- **The G1–G6 tools** did not come into play: the task's commands were short (the largest result was 511 bytes), so
  there was nothing for `haetty run` to shorten and no wait for `haetty wait`.

## Limits

- **One run per state**, as the brief asked. There is no variance estimate, and one run cannot separate the G0–G6
  changes from the agent's path (here, one denied call).
- **The two states are not the same task on the same files**: the repository differs (the after state has the G1–G6
  code and documents). The task text and the allow-list are the same.
- **The totals depend on the number of turns.** Each response re-reads the whole context, so a run with more turns
  reads more even if each turn is cheaper; the per-response figures are not a fair cost either.
- **Windows, not WSL.** The WSL Claude Code is not logged in, so it was not used; no credentials were read or copied.
  The tokens are the API's usage numbers; the attribution to tools is the approximation described in G1.
- **Both runs were started at the same time**, which changes timing, not token counts.
- A saving from G2, G3 or the saving mode needs a task with long output, a wait or a large read. This task had none.

## Reproduce

    git archive 3396d5c haetty | tar -x -C <folder>\before        (and 6324aac into <folder>\after)
    tools\heavy.ps1 -Label g7-before -- pwsh -File g7-run.ps1 -Label before -Dir <folder>\before\haetty -Out <out>
    haetty tokens --top 10 <transcript .jsonl under ~/.claude/projects>
