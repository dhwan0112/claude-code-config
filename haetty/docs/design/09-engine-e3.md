# haetty design: the terminal engine: E3 (10.7)

> Part of the design. Index: [`../DESIGN.md`](../DESIGN.md) (section
> numbers are the same there). Text below is moved verbatim from the former `docs/DESIGN.md`.

### 10.7 E3: a wider harness, the engine as an option, encoders, memory (2026-11-07)

- vtdiff got wider before the engine went further:
  - the fuzz test's generators (escape-sequence grammar, Claude-Code-like
    stream, mutations with corpus bytes spliced in;
    `haetty-core/tests/common/vtgen.rs`, shared);
  - a second fuzz mix (private modes, non-ASCII text, SGR colon forms, OSC
    colours, DCS/APC, kitty keys, resizes inside the stream);
  - the scrollback contents compared page by page at the end of each run;
  - `--bisect` minimises inputs with resizes; `--probe` takes
    `<resize CxR>` and `VTDIFF_SPLIT`; `--dump`; `--encoders`;
    `--gen-keys`.
- The reference is read through a fresh render state
  (`GhosttyEngine::snapshot_fresh`). The incremental one misses a row
  whose only change is a combining mark, VS or ZWJ joining a cell already
  drawn: **libghostty does not mark that row dirty** (finding 3 below).
- Behaviours taken over from libghostty, found by vtdiff and checked
  against its source:
  - parsing:
    - C1 bytes inside escape, CSI, DCS and SOS/PM/APC act as their 7-bit
      forms; a raw C1 in ground state is U+FFFD, a UTF-8-encoded one is
      printed;
    - C0 bytes 0x10–0x17, 0x19 and 0x1C–0x1F do nothing in ground state, as the newest Ghostty
      (`src/terminal/stream.zig`, e035beb: "we ignore C0 escapes > 0xF"); 0x18 and 0x1A only cancel an
      escape. libghostty 0.2.2 printed them as cells between characters, and haetty followed it until F2
      (docs/vtdiff/2026-11-09-f2.md);
    - a CSI with a colon outside SGR is void;
    - an APC ends at the ESC of its terminator, or at a C1 byte (and is
      dispatched); an OSC that is not valid UTF-8 is dropped;
  - SGR from the raw parameter bytes:
    - an empty field differs from 0 (`48:5:` is void, `38:2::R:G:B` is
      not);
    - sub-parameters void anything but `4:N` and the colours;
    - a single value left over after `38:5:N` counts as its own SGR;
  - resize as `PageList.resize`:
    - rows before columns when narrowing, after when widening;
    - fewer rows trim blank rows at the bottom (not the cursor's);
    - more rows pull the scrollback down only with the cursor on the last
      row;
    - reflow drops trailing empty cells of hard line ends, writes blank
      lines only before content, and keeps the rows below the cursor;
    - a cursor (or saved cursor) that ends in the scrollback goes to the
      top left;
    - no reflow while autowrap is off; the alternate screen never
      reflows;
    - a resize ends synchronized output;
  - wrap state:
    - IL, DL, SD and RI at the top (and SU on a region not from the
      first row) clear the soft wrap and the wrap-continuation of shifted
      rows, and turn their trailing spacer heads into plain cells;
    - DECALN clears every row's wrap, resets the region and the pen's
      attributes, and fills in the pen's colours;
    - overwriting a wide character in column 0 drops the spacer head of
      the row above;
    - the other half of an overwritten wide character is cleared in the
      pen's background;
  - mode 2027 grapheme clusters:
    - uucode's break rules (UAX #29 with GB9c and the emoji-modifier
      tailoring) on the Unicode 17 classes libghostty builds with
      (`grapheme_table.rs` from `tools/gen-grapheme-table.py`);
    - VS16 widens and VS15 narrows a cluster;
    - a part with width of its own widens it, wrapping to the next row
      when it no longer fits;
    - a zero-width character that starts a cluster is dropped;
  - screens and cursor:
    - the cursor shape goes with the cursor between screens but not
      through DECSC;
    - `?1049l` restores the cursor even off the alternate screen;
    - `?1049h` on it saves and erases;
    - leaving 47 or 1047 keeps the alternate screen's cursor;
    - DECRQM reports 47, 1047 and 1049 apart;
  - the rest:
    - DECRQM knows 3, 4, 8, 40, 45, 67, 1007, 1035, 1036, 1039, 1045 and
      1048, and needs a parameter;
    - DECSCUSR 0 is a steady block;
    - REP repeats the last printed character, as written, through the
      current charset;
    - characters above U+00FF print as a space under DEC special
      graphics;
    - kitty flags above 31 void a push or set, and popping 0 pops none;
    - HPR and VPR with an explicit 0 do nothing;
    - character protection (SPA/EPA ISO, DECSCA DEC) and the erasures
      that respect it;
    - colour specifications as `RGB.parse` (components truncated; `rgbi:`;
      9- and 12-digit hex; X11 names from `tools/gen-x11-colors.py`).
- Our bugs found on the way:
  - the reflow lost the cursor's line when there was scrollback;
  - spacer tails were trimmed as blanks;
  - RIS reset the byte scanner mid-input (E2);
  - the ghostty adapter reported `alt_screen` from mode flags that can
    stay set after 47/1047 leave, not from the screen shown (fixed:
    `active_screen`).
- **libghostty findings** (not copied; for the person to decide):
  1. TBC 0 toggles a tab stop (E2, §10.6).
  2. CHA, HPA and HPR under origin mode move the cursor down by the
     region's top. `setCursorPos` adds the origin offset to the row too.
     ECMA-48 §8.3.9 (CHA: "to character position n *in the active
     line*") and xterm (CHA, HPA move only horizontally) disagree. The
     native engine moves only horizontally.
  3. The render state misses rows changed only by a grapheme joining a
     drawn cell. This affects haetty today: a VS16, ZWJ or combining mark
     that arrives in a later PTY read than its base shows only when the
     row changes again. A fix on our side, outside E3 (the default engine
     is not to change): snapshot from a fresh render state when such a
     character was fed, or always (one extra copy of the visible rows).
  4. Mouse motion is never deduplicated in haetty's use: the adapter sets
     the encoder's options and size before each event, and libghostty
     clears its last cell on both. The native engine matches. Whether the
     GUI should deduplicate is a separate question.
- The engine is an option:
  - haetty-core owns its key and mouse types (`keys.rs`, from
    `vtdiff --gen-keys`, libghostty's numbering; a test round-trips every
    variant) and its colour parsing, so libghostty-vt is an optional
    dependency;
  - feature `ghostty` is the default, forwarded by every crate, and the
    workspace declares the haetty crates without default features;
  - `--no-default-features` builds with no libghostty in the graph and no
    Zig on PATH; the gate checks it ("build native-only (no zig)");
  - on Windows itself (MSVC, Rust 1.93.1, no Zig) it builds in 77 s and
    haetty-headless runs a session on the native engine;
  - `engine = ghostty | native` in the config file, `CreateSpec.engine`
    (protocol 9), `haetty-headless --engine`; a build without libghostty
    runs native with a warning.
- Colour parsing (themes, OSC) equals libghostty's: `vtdiff --colors`
  compares 3219 specifications and 97 palette entries (X11 names in
  several spellings, every hex/`rgb:`/`rgbi:` form, junk, every line of
  the built-in themes): all identical.
- Encoders (`haetty-core/src/encode.rs`): ports of libghostty's key
  (legacy, modifyOtherKeys, kitty), mouse, paste and focus encoders. `vtdiff
  --encoders` compares, byte for byte:
  - every key × 16 modifier sets × press/repeat/release × with and
    without text, under 6 legacy mode sets and all 31 kitty flag sets
    (397 824 cases);
  - mouse: 5 tracking modes × 5 formats × 11 buttons × 5 modifier sets ×
    6 positions (beyond 223 and 2015 columns, outside the window) × a
    press/motion/release sequence, at two widths (82 500 cases);
  - paste (bracketed or not, control bytes, an embedded `ESC [201~`) and
    focus.
  All 480 342 cases are identical.
- Memory (§10.3):
  - a cell is 16 bytes and `Copy`: a `u32` with the character or an index
    into a table of grapheme clusters (each kept once), the style inline,
    the width, the protected bit;
  - the scrollback keeps `scrollback-limit` bytes (a line costs its cells
    and its header);
  - a hard line end goes in without trailing blanks, so short lines cost
    little; the oldest lines leave first.
  This is twice libghostty's 8-byte cell for a full row and less for
  Claude Code's output. 8-byte cells (styles in a table) remain an option
  if the measurements call for it.
- Not in the native engine:
  - kitty graphics beyond the support query (transmit, display, delete:
    libghostty answers them; Claude Code sends only the query);
  - left/right margins (DECLRMM is kept for DECRQM, DECSLRM only homes the
    cursor; fuzz2 leaves mode 69 out);
  - the behaviour of the DECRQM-only modes (DECCOLM, reverse wrap…);
  - `dump` in VT and HTML form (`haetty-headless --dump vt|html`): the
    native engine gives plain text for all three.
- Selection needs nothing from the engine: haetty never sets one in
  libghostty (§10.1; its own selection is a known gap, HANDOFF §6.2).
- Results (`docs/vtdiff/2026-11-07-e3.md`, `--fuzz 200` per kind):
  - 1326 of 1648 runs identical (80.5 %); the corpus has 0 differences;
  - by kind: fuzz 400/400, mutated 394/400, fuzz2 317/400, grammar
    167/400 (random bytes, broken UTF-8), scenarios, resizes and
    synthetic loads all identical;
  - field match rates over checkpoints: 95 % or more each; scrollback
    contents 93 % of runs.
- What still differs, from minimal inputs of 30 differing runs:
  - libghostty findings 1 and 2 (TBC 0, CHA under origin mode);
  - undecided, needing more work on our side:
    - long runs of broken UTF-8 next to a wide character at the last
      column (vte's and libghostty's decoders split invalid sequences
      differently there);
    - SD with large counts followed by a resize;
    - kitty graphics transmit commands;
    - left/right margins (grammar sets mode 69).
- Measurements beside today's (Linux, `dist`, commit 2e2d572,
  `measure.sh`, `ENGINE=native` for the second column):

  | | libghostty (today) | native |
  |---|---|---|
  | key → frame, median / p95 | 0.99 / 1.25 ms | 0.78 / 0.94 ms |
  | throughput, plain lines | 24 MiB/s | 18 MiB/s |
  | throughput, long wrapped lines | 44 MiB/s | 31 MiB/s |
  | throughput, SGR-heavy | 53 MiB/s | 33 MiB/s |
  | memory per session, default scrollback filled (119-char lines) | 12.0 MB, 10 805 rows | 12.9 MB, 6 198 rows |
  | daemon with 4 filled tabs | 50.5 MB | 55.2 MB |
  | GUI draw per incremental frame | 0.14 ms | 0.15 ms |

  dist sizes, default build (both engines) / without libghostty:
  - Linux `haetty`: 6 654 576 / 5 851 648 B;
  - Linux `haetty-headless`: 1 855 424 / 978 968 B;
  - Windows `haetty.exe` (cross): 5 458 944 / 4 174 848 B;
  - Windows `haetty-headless.exe`: 3 244 032 / 1 960 448 B;
  - Windows `haetty.exe` built on Windows (MSVC), without libghostty:
    4 371 968 B.

  The default build carries both engines. New dependency: none besides
  vte 0.15 (E0: +9.5 KB); bitflags 2 was already in the tree.
- Before the default can change (the person's rule: 0 differences):
  - the person decides findings 1 and 2: copy libghostty or keep the
    specification (vtdiff cannot reach 0 with the specification);
  - the undecided differences above;
  - memory: 8-byte cells (styles in a table) to keep at least as many
    rows as libghostty in the same bytes (now 57 % of the rows, 7 % more
    memory);
  - throughput: now 65–75 % of libghostty's;
  - the e2e suite (`run-all.sh`) and a soak with `--engine native`;
  - VT and HTML dumps;
  - finding 3 (stale rows): fixed in the default engine (§10.8).
