# haetty design: the terminal engine: E4 (10.8-10.9)

> Part of the design. Index: [`../DESIGN.md`](../DESIGN.md) (section
> numbers are the same there). Text below is moved verbatim from the former `docs/DESIGN.md`.

### 10.8 E4a: the default engine redraws rows libghostty leaves unmarked (2026-11-07)

- The defect (§10.7 finding 3):
  - libghostty's render state copies only the rows the terminal marked
    dirty;
  - a combining mark, VS16 or ZWJ that joins a cell already drawn does
    not mark its row;
  - when that character arrives in a later PTY read than its base, the
    daemon's snapshot keeps the old row until something else changes it.
  - Seen through the observer and the GUI frame: `cafe`, then U+0301 one
    second later, stays `cafe` until the cursor leaves the row.
  - `vtdiff --refresh` found a second unmarked change: the spacer at the
    last column (before a wide character that wrapped) turning back into
    an ordinary blank. It draws the same, but the cells differ.
- Fix: `GhosttyEngine` reads each snapshot from a new render state
  (`Refresh::Always` in `haetty-core/src/ghostty.rs`). The engine and the
  bytes fed are unchanged; only the copy into the snapshot changed.
- Choice, measured (`vtdiff --refresh`: a snapshot after every read, reads
  of 4096 bytes and of 1–64 bytes, corpus plus 200 inputs of each
  generator, 212 886 snapshots):

  | Refresh | New render states | Rows copied per snapshot | Time per snapshot | Stale snapshots (runs) |
  |---|---|---|---|---|
  | incremental (before) | 0 % | 4.91 | 86.2 µs | 1429 (240) |
  | after a possible joiner (`Joined`) | 10.7 % | 6.90 | 86.6 µs | 96 (22) |
  | always (chosen) | 100 % | 24.58 | 90.3 µs | 0 (0) |

  - The Claude Code corpus: 24 rows copied per snapshot instead of 4.02,
    84.3 µs instead of 87.8 µs per snapshot (within noise).
  - `Joined` scans the bytes for zero-width characters, and for anything
    above U+00FF under mode 2027. It copies fewer rows, but it misses the
    spacer case (all of its 96 stale snapshots).
  - Through the daemon (dist, `measure.sh`'s cases, two rounds each,
    incremental / joined / always):
    - key → frame median: 0.88–0.93 ms for all three;
    - throughput: within the noise of the 50 ms steps of headless's
      `wait-exit` (plain lines 24–30 MiB/s, SGR 53–67 MiB/s, a VS16/ZWJ
      mix 10–13 MiB/s).
  - GUI frame cost is unchanged: the daemon sends only rows whose
    contents changed (`haetty-proto` frame diff), so copying unchanged
    rows from libghostty sends nothing more.
- Tests:
  - `haetty-core/tests/ghostty_refresh.rs`: seven split cases (combining
    mark, VS16, VS16 on a narrow base, ZWJ, VS16 split inside its bytes,
    mode 2027 combining and ZWJ) equal a fresh render state. A second test
    shows that the incremental state alone leaves four of them stale.
  - `scripts/e2e/regression.sh` 5b:
    - headless: `café` and `❤️` with each joiner in its own read;
    - GUI: the frame dump with U+0301 a second after `cafe` equals, byte
      for byte, the frame where both come in one read.
    - Both checks failed with the incremental state and pass now.
  - vtdiff's reference was already read fresh (§10.7), so its results are
    unchanged. `run-all.sh` passes with the reference frames unchanged.

### 10.9 E4: KNOWN or FAIL, margins, 8-byte cells, kitty graphics, dumps (2026-11-07)

- The rule for switching the default (the user, E4): **no unexplained
  differences**.
  - A run is KNOWN only if an entry in `docs/vtdiff/known-vt-differences.txt`
    explains it. Every other differing run is FAIL.
  - The list holds only what the user approved: findings 1 and 2, approved
    in the E4 instructions.
  - Proposals go to `known-candidates.txt`. The reasoning for each is in
    `docs/vtdiff/known.md`.
  - KNOWN is decided by counterfactual, not by pattern alone (`docs/vtdiff/known.md`):
    1. the pattern occurs in the input;
    2. every field that differed is among the entry's fields;
    3. the run is identical with the entry's `haetty_vt::Compat` switch on,
       which makes haetty-vt do what libghostty does.
  - vtdiff's self-test adds an unrelated difference and expects FAIL.
- Findings 1 and 2 are already fixed on Ghostty `main` (`5813c94`;
  `docs/upstream/`). A libghostty-vt release built from a later commit would
  make both entries unnecessary.
- haetty-vt behaviours added in E4, as libghostty:
  - E4b, part 1:
    - a print over a wide character or its tail in the first two columns
      clears the row above's spacer head;
    - DCH and ECH split cell boundaries and reset the row's wrap;
    - CAN and SUB end an OSC without printing;
    - OSC 7 keeps semicolons;
    - XTSAVE / XTRESTORE;
    - 47/1047/1049 as `switchScreenMode`.
  - E4b, part 2:
    - left and right margins (DECLRMM, DECSLRM) in printing, wrapping, CR,
      CUF, HT, CBT, CUP/CHA/HPA under origin mode, IND, RI, SU/SD, IL/DL,
      ICH/DCH;
    - IL/DL/SU/SD reset wrap flags only on rows that moved.
- UTF-8: Unicode 17 §3.9 Tables 3-8 to 3-11 give the same U+FFFD
  substitution (maximal subparts) in both engines, whole and byte by byte.
  The decoders do not differ. The remaining broken-UTF-8 runs differ in
  other ways (below).
- Cells (E4c): 8 bytes. That is content, a 16-bit style number, width and
  protection.
  - The style table is per terminal and hashed with an FxHash-like hasher.
    The last style and the last erased form are cached.
  - When the table gets large it is rebuilt from the styles still in use,
    which bounds it (test: 200 000 colours).
- Dumps: `format.rs` ports libghostty's formatter (plain, VT, HTML,
  libghostty's HTML page boundaries included). vtdiff compares the dumps
  after every run.
- Kitty graphics: `kitty.rs` ports libghostty's command handling: parsing,
  base64, replies, quiet levels, chunks, ids and numbers, storage limits,
  placements and the cursor after them, deletes, screen events.
  - Images are kept as metadata and never drawn.
  - Files, temporary files and shared memory are never read; they answer
    "unsupported medium", as this libghostty build does.
  - `o=z` uses `miniz_oxide` 0.9.1 with `adler2` 2.0.1 (70.5 KB crate; +76.8
    KB in a stripped release binary).
  - PNG answers as this libghostty build, which has no PNG decoder.
- Results (`docs/vtdiff/2026-11-07-e4.md`: corpus, 200 inputs of each
  generator, 1 664 runs):
  - **FAIL (unexplained): 98 runs.** KNOWN 0, with the two approved entries
    and one candidate.
    - At the start of E4 there were 321, counted the same way. The inputs
      have since widened: margins and kitty commands in fuzz2, and the
      joiners.
    - Corpus 4/4, fuzz 400/400, mutated 400/400, scenarios, resizes,
      synthetic and joiners all identical.
    - fuzz2 362/400, grammar 340/400.
  - What the FAIL runs are (first differing field):
    - grammar, 60 runs: random bytes with broken UTF-8 and C1 controls.
      Their minimal inputs stay long (hundreds of steps), so the cause is
      context, not a single sequence. Undecided.
    - fuzz2, 38 runs: text 17, width 15, background 6. The minimal inputs
      examined involve wide characters at the last column and resizes
      after scrolling. Not each one has been traced.
    - kitty: a deflate match before the output start, placements through a
      reflow, and a huge `r=`.
  - Dumps after each run, on runs with identical screens:
    - plain 99.9 %;
    - VT 88.6 %;
    - HTML 70.2 %;
    - the rest is underline style and colour, OSC 8 hyperlinks (neither
      kept) and ED 22 on the alternate screen.
- Measured (dist, Linux; libghostty / native):

  | | libghostty | native | the user's bar |
  |---|---|---|---|
  | rows kept, 12 MB limit, 119-char lines (20 000 fed) | 11 065 | 12 195 | ≥ libghostty: met |
  | memory per filled session | 12.4 MB | 13.4 MB (1.10 vs 1.12 kB per row) | |
  | throughput, plain lines | 24–30 MiB/s | 18–22 MiB/s (67–81 %) | ≥ 50 %: met |
  | throughput, long wrapped lines | 55 MiB/s | 24–32 MiB/s (44–58 %) | ≥ 50 %: borderline (50 ms steps of the measurement) |
  | throughput, SGR-heavy | 59 MiB/s | 35–38 MiB/s (59–64 %) | ≥ 50 %: met |
  | key → frame median / p95 | 0.91 / 1.18 ms | 0.74 / 0.92 ms | ≤ libghostty: met |

  - The engine alone (`examples/feed.rs`, in-process): lines 34, wrapped
    71, SGR 60 MiB/s.
- Native beside libghostty, end to end, Linux, commit `6f3ceea`
  (`E2E_ENGINE=native scripts/e2e/run-all.sh`).
  - A check in the same setup confirmed which engine each run used. TBC 0
    lands the next tab at column 9 on native and column 5 on libghostty,
    for both GUI and headless sessions.

  | Check (script) | libghostty | native |
  |---|---|---|
  | regression list (`regression.sh`, incl. late joiners) | 29/29 | 29/29 |
  | DESIGN §2.3, system decorations (`design-2-3.sh`) | 5/5 | 5/5 |
  | DESIGN §2.3, integrated decorations | 5/5 | 5/5 |
  | themes (`themes.sh`) | 8/8 | 8/8 |
  | title row (`titlebar.sh`) | 13/13 | 13/13 |
  | tabs (`tabs.sh`) | 20/20 | 20/20 |
  | orchestrator (`orchestrator.sh`) | 21/21 | 21/21 |
  | GUI after a daemon crash (`daemon-crash.sh`) | 9/9 | 9/9 |
  | first window at 100–200 % (`scale.sh`) | 13/13 | 13/13 |
  | total | 123/123 | 123/123 |
- Soak, one hour on native (Linux, `soak.sh` through `E2E_ENGINE`'s
  wrappers, commit `6f3ceea`): VERDICT PASS.
  - Every operation was done: tabs 200/200, detach 50/50, tasks 20/20,
    themes 10/10.
  - These stayed level from the first sample to the last: GUI threads 7,
    GUI fds 17, daemon threads 24, daemon fds 21, descendants 6, sessions
    4.
  - GUI memory: 7 128 kB at 15 min, 7 228 kB at the end.
  - **Daemon memory: 53 400 kB at 45 min, 57 472 kB at the end.** That is
    +4 072 kB against a bar of +4 MB, and it was still rising by about
    1.4 MB per 5 minutes.
  - The bar was set for libghostty's scrollback, which fills in about 40
    minutes. With 8-byte cells the same byte limit holds more rows and
    fills later.
  - A longer native soak has to show where it levels off.
- Before the default can change (no unexplained differences):
  - the 98 FAIL runs: grammar 60, fuzz2 38;
  - kitty: a deflate match before the output start (miniz reads zeros),
    placements through a reflow;
  - underline style and colour (SGR 4:x, 21, 58) and OSC 8 hyperlinks:
    they show in dumps, and in rendering once the GUI draws them. They fit
    the style table (§10.9 cells);
  - the user's decision on the VPA/VPR candidate;
  - wrapped-line throughput just at the bar;
  - Windows: the native engine's e2e and soak have not run there.
