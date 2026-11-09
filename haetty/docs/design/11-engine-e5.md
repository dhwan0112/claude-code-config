# haetty design: the terminal engine: E5 (10.10)

> Part of the design. Index: [`../DESIGN.md`](../DESIGN.md) (section
> numbers are the same there). Text below is moved verbatim from the former `docs/DESIGN.md`.

### 10.10 E5: towards no unexplained differences, underlines and links, throughput, soak rule (2026-11-08)

- The user's decisions (E5 instructions): `haetty-v0.2.0` on `63822de`
  (tagged after the overnight soak, §1 of HANDOFF); the VPA/VPR entry
  approved; underline kinds, underline colour and OSC 8 links in both
  engines and the GUI as a DESIGN §2 change (§2.2).
- Tools (vtdiff):
  - `--diverge NAME`: cuts the input before each ESC and asks both engines
    for their state (cursor, DECRQSS, DECRQM, saved cursor, tab stops, a
    print); the first cut whose answers differ is where the states parted,
    minimised under "the answers differ";
  - `--bisect-dump NAME`: the shortest prefix whose dumps differ, minimised;
  - `--probe` takes `VTDIFF_COMPAT=name,…`; patterns take `DECRST N`;
  - a "decor" input category (underlines, links, what keeps or drops a
    link); every dump difference cause is listed.
- haetty-vt behaviours added in E5, as libghostty:
  - E5b: SGR 4:x, 21, 24, 58, 59; OSC 8 (ids, implicit ids, the parser's
    key reading, raw bytes, the whole payload past vte's 16 parameters,
    SGR 0 and DECRC keep the link, a change of screen ends it, a new
    implicit id when the cursor changes page or on a resize); the
    formatter's underline kinds and colours and `<a href>`;
  - E5a: mode 2027 variation-selector bases (Unicode emoji_vs_base,
    `VS_BASE_2027`); CTC (`CSI Ps W`) and DECST8C; a kitty deflate match
    before the output start refused; a wider resize without reflow turns
    the old last column's spacer head into a plain cell; a line feed at
    the bottom of a full-width region moves rows as they are.
- Not as vte: vte 0.15 loses a character when a call completes the UTF-8
  character the last one ended in and goes on to another character and
  the start of a third (`advance_partial_utf8`); the completing bytes go
  to vte on their own first.
- Results (`docs/vtdiff/2026-11-08-e5.md`, 1 033 inputs, 2 066 runs):
  **FAIL 79** (start of E5, decor included: 119). Grammar 56, fuzz2 21,
  decor 2, 0 KNOWN. Dumps on runs with identical screens: 1 985 of 1 987
  in each format (plain, VT, HTML).
- Candidates (`known-candidates.txt`, the user decides):
  no-pending-wrap-without-autowrap (DECAWM off: libghostty sets a pending
  wrap; fixed in Ghostty `42a6564`) and ctc-2-clears (CTC 2 toggles as
  TBC 0). Their switches make the minimal inputs identical; the runs they
  occur in still differ elsewhere, so they count no KNOWN yet.
- Not yet explained (classes, `--bisect` / `--diverge`):
  - libghostty re-exposes rows a row shrink trimmed (blank text, coloured
    background) when the page grows again (line feed, SU, more rows):
    `\e[48;5;1m\e[2J\e[m<resize 80x7>\e[S`. A libghostty defect (new
    lines must be erased); a candidate needs a switch that keeps the
    trimmed rows (not written);
  - fuzz2: left/right margins with a resize, SD with a background and a
    resize, NBSP, combining marks after resizes;
  - grammar (56): random bytes; `--diverge` finds most of them part where
    they show (no hidden state first); their minimal inputs stay long.
- Throughput through a session (haetty-headless, `cat`, 80x24, ~110 MiB
  inputs, 5 alternating runs, median; `e5c-tput.sh`), native / libghostty:

  | | before E5c (`75231c2`) | after (`6c5e2d6`) |
  |---|---|---|
  | plain lines | 25 / 35 MiB/s (71 %) | 40 / 34 (117 %) |
  | long wrapped lines | 38 / 77 (49 %) | 102 / 75 (136 %) |
  | SGR-heavy | 40 / 67 (59 %) | 38 / 65 (58 %) |

  The gain is `print_ascii`: runs of printable ASCII in the ground state
  skip vte and are written a row at a time (haetty-vt alone: wrapped 57
  -> 507 MiB/s). vtdiff unchanged by it.
- Memory: rows going to the scrollback were shrunk in place; with short
  lines that left holes and one session took 37 MB for a 12 MB limit (the
  soak's burst tab; libghostty 12.4 MB). Now copied to their size: 13.2 MB
  (50 000 lines) to 14.4 MB (600 000 lines).
- Soak rule (E5c): daemon memory is judged against the first sample at
  which the largest scrollback stopped growing (`status --json`
  "scrollback", protocol 11), with at least 15 minutes after it, else
  FAIL; builds without the field are judged at 45 minutes as before.
- Native soak, 2 hours (Linux, `soak.sh`, new rule; `6c5e2d6`): every
  operation done (tabs 200, detach 50, tasks 20, themes 10); threads, fds,
  descendants, sessions level; GUI memory 8 036 kB at 15 min -> 8 204 kB.
  **Daemon memory** (kB; the burst tab's scrollback full at 38 678 rows
  from 915 s):

  | t (s) | 0 | 604 | 915 | 1 202 | 2 116 | 3 002 | 4 215 | 5 406 | 6 617 | 7 204 |
  |---|---|---|---|---|---|---|---|---|---|---|
  | daemon | 3 440 | 14 996 | 18 604 | 19 112 | 21 340 | 21 976 | 23 080 | 24 568 | 25 924 | 26 556 |

  - Verdict FAIL: +7 444 kB after the scrollback was full (bar +4 MB).
  - It levels where the burst tab's scrollback fills (about 19 MB at
    15-20 min), then rises by about 0.3 MB per 5 minutes to the end. That
    is the slow tab (5 lines a second, ~230 B a row in haetty-vt, about
    0.35 MB per 5 minutes) filling its own scrollback, which takes about
    3 hours: not full in this run, so not a leak shown, but not judged
    flat either.
  - The same run on `7596e52` (before the scrollback fix, stopped at 45
    min): 46 MB at 15 min, then +1.2 MB per 5 minutes.
  - The rule judges the largest scrollback; libghostty's slow tab costs
    page memory it already has, haetty-vt's costs a row each. Judging
    every session instead would leave libghostty's 1-hour runs unjudged
    too. The user decides between a longer native soak (about 3.5 h), a
    smaller scrollback limit in the soak, or an allowance for rows still
    being added (HANDOFF §6.0).
- Windows, native engine (E5d; a build without libghostty, so every
  session is native): 22 test executables, 158 tests, 0 failed; the smoke
  test 5/5; the daemon crash test 4/4. The GUI e2e and the soak on Windows
  wait for the next overnight run (`overnight-soak.ps1 -Engine native`).
- Linux end to end on `6c5e2d6`: `run-all.sh` 132/132 on either engine
  (links.sh new: 9).
- libghostty after 0.2.2 (E5e): `docs/upstream/libghostty-update-2026-11.md`.
- Before the default can change:
  - the 79 FAIL runs and the user's decision on the two candidates (and
    the trimmed-rows class);
  - SGR-heavy throughput at 58 % (the E4 bar is 50 %);
  - the native overnight run on Windows (GUI e2e, soak);
  - a libghostty release built from a later Ghostty would remove the
    approved entries and move many "as libghostty" behaviours.
