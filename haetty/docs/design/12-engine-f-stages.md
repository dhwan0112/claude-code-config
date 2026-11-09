# haetty design: the terminal engine: F0 to F4 (10.11-10.15)

> Part of the design. Index: [`../DESIGN.md`](../DESIGN.md) (section
> numbers are the same there). Text below is moved verbatim from the former `docs/DESIGN.md`.

### 10.11 F0: the soak rule for both engines, one hour each (2026-11-09)

The 2-hour rule of E5c (§10.10) is replaced. A run is judged by how each tab's scrollback behaves, not by its length,
and the same rule runs on both engines.

- **Test-only limit.** The soaks write `scrollback-limit = 800000` into the config they make (`SOAK_LIMIT` in
  `soak.sh`, `$SoakLimit` in `soak.ps1`). The product default (12 MB) is unchanged and nothing else reads that value.
  Calibrated with `examples/mem.rs` (`MEM_SCRIPT`, 120 x 36, 119-character lines) at 800 000 bytes: native 4 761 rows
  for ticks and 2 439 for bursts, libghostty 507 rows for both. The soaks' own output differs from the calibration
  lines, so the one-hour runs below are the numbers that count.
- **The rule.** Per session, from its scrollback rows at each sample:
  - a session is *full* from the first sample from which its rows no longer grow by more than max(100 rows, 1 %) over
    the sample it is at; a session that never grew by more than 100 rows does not count;
  - the *baseline* is the latest full time among the sessions that grew; a session still growing at the last sample
    means no baseline (FAIL);
  - from the baseline on, the window is at least 1 800 s, and at the last sample the GUI's and the daemon's anonymous
    memory (Windows: private working set) is at most 4 MB above its value at the baseline.

  Other checks are unchanged: threads, file descriptors (handles), descendants and sessions as at the first sample;
  task worktrees 0 at the end; every operation done; the window and the daemon alive. A series without the
  `sessions_rows` column (an old script) is not judged: FAIL. The short tabs of the operations never appear in a
  sample, and one-line workers never grow by 100 rows, so neither moves the baseline.
- **Where it is written.** Linux: `scripts/e2e/soak-verdict.py`, run by `soak.sh` at the end. Windows:
  `scripts/e2e/windows/soak-verdict.ps1`, dot-sourced by `soak.ps1`. Both judge the same synthetic series
  (`scripts/e2e/soak-cases`: ten cases, full early, full late, never full, growth after full, legacy column, a closed
  session, one-line workers, a sawtooth, operations short, no growth). `tools/test-soak-verdict.sh` runs both (Windows
  through `pwsh`); all ten agree with `expect.txt` on both platforms.
- **Overnight.** `tools/overnight-soak.ps1` takes `-Commit HEAD` by default (the main checkout's HEAD when the run
  starts; the Linux build's first line prints its hash, and the result header carries it) and `-Engine`. The command
  for the night is §6.1 item 30.

**Results of the two one-hour Linux runs.** The binary of `9c517ca` (haetty 0.2.0), the scripts of this stage. The
samples are in `~/m4/f0-native-1h.csv` and `~/m4/f0-ghostty-1h.csv`.

| | native | libghostty |
|---|---|---|
| verdict | PASS, every check | PASS, every check |
| operations (tabs / detach / tasks / themes) | 200 / 50 / 20 / 10 | 200 / 50 / 20 / 10 |
| burst tab full at | 308 s (2 578 rows) | 309 s (686 rows at the end) |
| slow tab full at | 907 s (3 703 rows) | 309 s (701 rows at the end) |
| baseline; window after it | 907 s; 2 698 s | 309 s; 3 295 s |
| GUI anonymous memory, baseline to end | 8 144 to 8 000 kB | 8 028 to 8 120 kB |
| daemon anonymous memory, baseline to end | 7 028 to 7 500 kB (+472 kB) | 9 092 to 10 300 kB (+1 208 kB) |

- libghostty's row counts do not settle: the page-granular eviction moves both tabs between about 380 and 730 rows, so
  the rule takes the level a session reaches, not one value. Its daemon memory keeps rising after the fill, about
  +0.2 MB per 10 min (+1.2 MB in 55 min, inside the bar). Whether it levels off is what the night run will show.
- Native's daemon memory rose by 0.47 MB in the 45 min after its baseline.
- The Windows soak and its verdict have not had a real run; the night run is their first.
- Unchanged: the default engine (libghostty), the stable instance, the protocol (still 11).

**Night run, 2026-11-09 02:00–04:06 (commit `bb61e92`, the same code as `d02cee8`; `-Engine native -GuiE2E`).**
LINUX PASS (soak, 60 min); WINDOWS PASS (soak, 60 min: the first real run of `soak.ps1` and
`windows/soak-verdict.ps1`); GUI-TABS PASS 14/14; GUI-ORCHESTRATOR PASS 17/17. Linux: baseline 906 s (slow tab,
3 703 rows), window 2 700 s, daemon anonymous memory +548 kB, GUI 8 212 to 8 056 kB. Windows: baseline 1 203 s
(slow tab, 4 761 rows), window 2 404 s, GUI private working set 11 048 to 10 860 kB, daemon 7 812 to 8 100 kB
(+288 kB). Operations 200/50/20/10 on both. Result file: `C:\Users\Public\haetty-overnight\result-20261109-0200.txt`.

### 10.12 F1: the newest Ghostty as the vtdiff reference (2026-11-09)

- **The reference.** `ghostty-main` is Ghostty `e035beb` (main, 2026-11-08), built with Zig 0.17.0 as a helper
  program (`tools/ghostty-ref`, README there: pin, protocol, build, refresh). It is the default reference of
  `haetty-vtdiff`. `ghostty-0.2.2` stays in process for the side-by-side report. The two cannot share one binary:
  both link as `ghostty-vt`. Neither is linked into dist, the gate or the installers.
- **The adapter** (`crates/haetty-vtdiff/src/refs.rs`) follows `haetty-core/src/ghostty.rs` field by field. Title and
  working directory are taken only when they are valid UTF-8, else empty (as `term.title().unwrap_or("")`); cell text
  and hyperlink targets are decoded with replacement (as `cell_link`). Title report and the glyph protocol stay off
  (haetty answers neither). Synchronized output ends on every valid resize, as main and 0.2.2's C API both do.
- **The lists.** Every entry names its reference (`refs`). The five approved entries explain 0.2.2 only. The two
  candidates were approved by the person on 2026-11-09 and moved. CLAUDE.md now delegates additions under both
  conditions (ghostty-main agrees with haetty; an ECMA-48 or xterm clause gives the reason); changes still need the
  person.
- **Results** (`docs/vtdiff/2026-11-09-f1.md`; 1033 inputs × 2 cuttings). With 0.2.2, the run reproduces E5 exactly:
  1987 identical, 79 FAIL, 0 KNOWN. With ghostty-main: 1189 identical, 877 FAIL, 0 KNOWN. Of the 877, 798 are runs
  where haetty agrees with 0.2.2 and main differs, and 79 are E5's remainder. No run has haetty agreeing with main
  while 0.2.2 differs. The recorded corpus is identical under both.
- **What the first differences are** (F2's worklist): a C0 control while a UTF-8 sequence is incomplete (grammar, all
  400: CAN is executed by main, printed as U+0018 by haetty and 0.2.2); DECRQSS for SGR (main answers
  `DCS 1 $ r 0 m ST`; 220 of the fuzz2 first differences); text and replies in fuzz2 and mutated (not yet classified).
- **Not changed in this stage:** the native engine, the default engine (libghostty), the protocol, the stable
  instance. F2 is next.
### 10.13 F2: haetty-vt moved to the newest Ghostty where it had followed 0.2.2 (2026-11-09)

- **Changes** (`docs/vtdiff/2026-11-09-f2.md`): C0 controls 0x10–0x1F do nothing in ground state (F2a); from 0x80 in a
  DCS string the bytes are payload, and a C1 control sent as UTF-8 is ignored (F2b); DECRQSS answered for SGR, DECSTBM,
  DECSCUSR and DECSLRM (F2b). Each is the newest Ghostty's behaviour at e035beb, with its source location in the record.
- **Result**: ghostty-main FAIL 877 -> 744; identical 1189 -> 1322 (64.0 %). The corpus is 4 of 4 under both references.
  ghostty-0.2.2 FAIL 79 -> 883 (its differences are classified, not fixed: 172 runs are 0.2.2 defects, where haetty
  agrees with ghostty-main).
- **Not reached**: the target (no unexplained difference under ghostty-main). The 744 are in kitty graphics responses
  and XTWINOPS size reports (about 200 first differences), other replies, and grammar and mutated text (not yet
  bisected to causes). Next: the kitty graphics pass, then the text causes.
### 10.14 F3: SGR-heavy throughput (2026-11-09)

- **Measure**: `scripts/e2e`'s session harness of E5 (`e5c-tput.sh`, `sgr` input of 112 MiB, five alternating runs per
  engine, median), and `examples/feed sgr` for the engine alone.
- **Result** (session): native 41 -> 57 MiB/s against libghostty 73 -> 69 MiB/s: 56 % -> 82 % of libghostty.
  (Engine alone: 56 -> 92 MiB/s.)
- **What the profile said**: vte alone reads this input at 468 MiB/s, about 15 % of the engine's time, so the cost is
  in haetty's own code: the scanner's byte-by-byte pass over the CSI parameters, and the SGR parser's allocations.
- **Changes** (all keep the output: vtdiff's outcome files are identical to F2's under both references):
  - the SGR parameters are parsed in one pass into two buffers the terminal keeps (no allocation per SGR);
  - a run of CSI parameter bytes (digits, `;`, `:`) is taken in one step, as `byte` would take each of them;
  - the erased form of a style with the default background is id 0, so the style table is not searched for it.

### 10.15 F4: native-only readiness and the transition table (2026-11-09)

- **Measure**: on `12b5546`, the F4 base (no code change in F4). Dist builds with `--profile dist`, sizes as built;
  latency with `examples/latency.rs` (300 keys, three quiet repeats per engine); throughput with `e5c-tput.sh` (five
  alternating runs per engine, medians); the installers with `test-install-stable.sh` and `.ps1` (two commits, isolated
  test folders); the e2e suite with `scripts/e2e/run-all.sh` (libghostty sessions, `E2E_ENGINE=native`, and the
  native-only build with `--no-build`); the Windows MSVC build without WSL, smoke-tested.
- **Sizes** (bytes, dist profile):

  | build | haetty | haetty-headless |
  |---|---|---|
  | Linux, default (libghostty) | 7 091 288 | 2 193 488 |
  | Linux, native-only | 6 001 736 (−15 %) | 1 110 712 (−49 %) |
  | Windows, mingw from WSL, default | 5 596 672 (`.exe`) | 3 361 792 (`.exe`) |
  | Windows, mingw from WSL, native-only | 4 309 504 (−23 %) | 2 074 624 (−38 %) |
  | Windows, MSVC, native-only, no WSL | 4 520 960 | 2 051 584 |

  The Linux native-only binaries contain no libghostty C symbols (`ghostty_` strings: 0). The MSVC build came from a
  copy without `.git`, so its `--version` says `commit unknown`.
- **Latency** (key to frame, 120 x 36, 300 keys): libghostty median 0.94–0.98 ms, p95 1.12–1.25 ms; native median
  1.07–1.08 ms, p95 1.28–1.32 ms. The budget is 16 ms (section 3.4); the gap is 0.13 ms at the median.
- **Throughput** (session, five runs per engine, medians, quiet machine): lines 35 → 41 MiB/s (117 %); wrapped
  78 → 103 MiB/s (132 %); SGR 65 → 52 MiB/s (80.0 %). F3 measured SGR at 82 % (57 vs 69 MiB/s). A first F4 SGR run
  overlapped other heavy jobs (80 %, 54 vs 67 MiB/s) and is not used.
- **Installers**: `test-install-stable.sh` 19 of 19 passed (Linux, commits 3a983af and HEAD); `test-install-stable.ps1`
  18 of 18 (Windows). Both test the default build. `install-stable` has no option for a native-only build: every
  installer build compiles libghostty with Zig.
- **e2e** (`run-all.sh` on 12b5546): libghostty sessions, native sessions (`E2E_ENGINE=native`) and the native-only
  build all pass every script (regression 29, design-2-3 5 and 5, themes 8, titlebar 13, links 9, tabs 20,
  orchestrator 21, daemon-crash 9, scale 13; no failures, no skips).
- **Windows without WSL**: Visual Studio 2022 Build Tools and Rust `stable-x86_64-pc-windows-msvc`;
  `cargo build --profile dist --no-default-features -p haetty-gui -p haetty-headless` builds `haetty.exe` and
  `haetty-headless.exe`. Smoke test: `--version` runs, and a native session prints `ok` with exit 0. The procedure
  is in SELF-HOSTING.md, "Windows build without WSL".
- **A ghostty request on the native-only build** runs on the native engine and logs a warning (`crates/haetty-core/src/
  vt.rs`, `new_engine`). `haetty-headless` prints nothing without `--trace`, so there the fallback shows only in the
  log. Open; not changed in F4.

**Transition table** (status 2026-11-09; the default engine stays libghostty; the switch is the user's decision):

| # | condition | status | evidence |
|---|---|---|---|
| 1 | vtdiff: no unexplained differences from ghostty-main | NOT MET | 744 FAILs after F2 (`docs/vtdiff/2026-11-09-f2.md`) |
| 2 | vtdiff corpus (4 recorded Claude Code outputs) identical under ghostty-main | MET | F1 (`docs/vtdiff/2026-11-09-f1.md`), unchanged by F2 and F3 |
| 3 | e2e suite passes with both engines | MET | `run-all.sh` on 12b5546: libghostty and native sessions |
| 4 | night run, native, both platforms, PASS | MET at `bb61e92` (night 2026-11-09, 10.11); re-run on the v0.3.0 candidate (F5) | |
| 5 | soak, one hour per engine, PASS | MET on Linux (F0, 10.11); Windows native only; Windows libghostty not run yet | |
| 6 | throughput, lines at least 100 % of libghostty | MET (117 %) | |
| 7 | throughput, wrapped at least 100 % | MET (132 %) | |
| 8 | SGR at least 80 % | MET, at the threshold (80.0 %; 82 % at F3) | |
| 9 | key to frame latency at most libghostty | NOT MET (1.07–1.08 vs 0.94–0.98 ms; budget 16 ms) | |
| 10 | installer self-tests pass (default build) | MET (Linux 19/19, Windows 18/18) | |
| 11 | the installer can build the native-only build | NOT MET: no option; the native-only installer path does not exist | |
| 12 | native-only build without libghostty or Zig | MET (`--no-default-features` with Zig off PATH; no `ghostty_` symbols; e2e passes) | |
| 13 | Windows native build without WSL | MET (MSVC; smoke test passed) | |
| 14 | default engine | unchanged: libghostty (instruction) | |

**Open after F4**: the 744 vtdiff FAILs (F2 remainder); the 0.13 ms latency gap; the SGR margin (none beyond the
threshold); no native-only installer option; the headless fallback warning shows only in the log; no Windows
libghostty soak yet.
