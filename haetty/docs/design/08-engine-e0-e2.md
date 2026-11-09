# haetty design: the terminal engine: E0 to E2 (section 10 intro and 10.1-10.6)

> Part of the design. Index: [`../DESIGN.md`](../DESIGN.md) (section
> numbers are the same there). Text below is moved verbatim from the former `docs/DESIGN.md`.

## 10. A terminal engine in Rust (design, stage E0, 2026-11-07)

Goal: an engine of our own in Rust beside libghostty-vt. A build with only
that engine needs no Zig, builds on Windows directly, and is all Rust.
**The default engine stays libghostty-vt** until the differential tests
(§10.5) show no difference from it. Stages: E0 this design, E1 the
differential tool (before the engine), E2 the core engine, E3 encoders and
the rest.

### 10.1 What haetty uses of libghostty-vt today (from the code)

Every use, from `crates/haetty-core` (no other crate calls libghostty-vt;
the GUI and the wire see its key and mouse types through `haetty_core`):

| Area | Where | What |
|---|---|---|
| Parser and screen operations | `engine.rs:341` `vt_write` | all of it: C0/C1, ESC, CSI, OSC, DCS, the screen, cursor, erase, insert and delete, scroll regions (DECSTBM, DECSLRM), origin mode, tab stops, charsets, the alternate screen, saved cursor, wrap state |
| Modes read back | `engine.rs:396, 402, 454, 470, 488, 758–766` | bracketed paste, focus events, colour-scheme reports (2031), synchronized output (2026), alternate screen (47/1047/1049), mouse tracking (any), SGR mouse, DECCKM, cursor visible, kitty keyboard flags |
| Replies to queries | `engine.rs:554–628` (callbacks) and libghostty itself | DA1 (VT220; 132 columns, selective erase, ANSI colour, clipboard), DA2 (VT220, firmware 1), DA3, XTVERSION (`haetty X.Y.Z`), DSR (5, 6), DECRQM, size reports (CSI 14/16/18 t, in-band mode 2048), colour scheme (CSI ? 996 n), OSC 4/10/11/12 queries, kitty keyboard query (CSI ? u) |
| Events | `engine.rs:562–590` | bell, title (OSC 0/2), working directory (OSC 7), clipboard write (OSC 52) |
| Not libghostty | `osc.rs`, `engine.rs:467–490`, the daemon | OSC 9 / 777 / 99 notifications and OSC 9;4 progress (our scanner), the synchronized-output hold timing, frame diffs per row (`haetty-daemon`, `frame_between`) |
| Colours | `engine.rs:532–544`, `theme.rs:97, 121, 149` | default fg/bg, palette 0–15, cursor colour; colour parsing (`#rgb`, `rgb:`, X11 names), xterm's 256-colour palette, `N=COLOR` |
| Scrollback and viewport | `engine.rs:203, 362, 391, 399, 431, 768` | `max_scrollback` in **bytes**, viewport top/bottom/delta, scrollbar (total, offset, length) |
| Resize | `engine.rs:224, 418` | cells and cell pixels; soft-wrapped lines reflow |
| Render state | `engine.rs:752–863` | per cell: grapheme text, resolved fg/bg, bold, italic, faint, blink, inverse, invisible, strikethrough, overline, underline (any kind), selected, wide/spacer. Cursor: visible, position, style (block, bar, underline, hollow), blinking. Title, pwd. The whole grid is copied each time: libghostty's per-row dirty state is **not** used (the daemon diffs rows). |
| Wide characters and graphemes | inside libghostty | East Asian width, emoji presentation, VS15/VS16 (wide only with mode 2027, §4 pitfalls), grapheme clustering (mode 2027) |
| Encoders | `engine.rs:663–750, 402, 457` | keys (`key::Encoder` from the terminal's modes: legacy, modifyOtherKeys, kitty flags), mouse (`mouse::Encoder`: X10, normal, button, any-motion; SGR and SGR-pixels; pixel geometry), paste (bracketed or not, unsafe bytes), focus in/out, colour-scheme report |
| Formatter | `engine.rs:866` | `dump` as plain text, VT or HTML (`haetty-headless`; DESIGN §9 handover candidate) |
| Selection | `engine.rs:843` | only `is_selected` is read; haetty never sets a selection (its own selection is a known gap, HANDOFF §6.2). |
| Hyperlinks | inside libghostty | OSC 8 parsed; nothing reaches the snapshot |
| Types | `types.rs:5–6, 46` | `Key` (about 180 variants, travelling as `u32`), `Mods`, `KittyKeyFlags`, key and mouse `Action`, mouse `Button`, `RgbColor` |

### 10.2 Build it, or take a small crate

Sizes are stripped `dist` builds of a small program using the crate, minus
the same program without it (measured 2026-11-07; Linux / Windows):

| Part | Decision | Size | Why |
|---|---|---|---|
| Parser (DEC/ANSI state machine) | **`vte` 0.15** | +9.5 / +8.7 KB (arrayvec, memchr) | Paul Williams' state machine, as Ghostty's; mature (Alacritty); `no_std`-able. Its limits (parameter count, OSC length) are checked against libghostty's by E1. |
| Screen, cursor, modes, scroll regions, tabs, charsets, alt screen | own | — | the engine itself; no small crate fits our snapshot and byte budget |
| Cell width | **own table, generated from libghostty** | ~10–15 KB (estimate, as unicode-width's tables) | `unicode-width` 0.2 (+13.5 / +12.8 KB) follows UAX 11 as the Rust ecosystem reads it; Ghostty has its own table (emoji presentation, private use, ambiguous). A dev-only generator in the E1 crate asks libghostty for the width of every code point and writes range tables, so widths match by construction; a test lists where `unicode-width` would differ. |
| Grapheme clusters (mode 2027) | **`unicode-segmentation`** to start | +29.5 / +28.7 KB | UAX 29 extended clusters; E1 compares it with libghostty. If they differ (Unicode version, Ghostty's own rules), own break tables generated like the width table. |
| Key, mouse, paste, focus encoders | own | — | small, protocol rules (xterm, kitty); compared byte for byte in E3 |
| Colour parsing, palette | own | X11 names ~10 KB | small; the xterm 256-colour formula; X11 names as a sorted table |
| Formatter | own, later | — | plain text first (`dump`); VT format when §9 needs it |

Budget: libghostty-vt's archive in the Linux `dist` build holds 1.01 MB of
code and 69 KB of data (`size -t`, ReleaseSmall, simdutf and highway
included); the native engine should stay well under that.

### 10.3 Memory

- Baseline (§3.4, libghostty): 11.6–11.8 MB per session with the default
  12 000 000-byte scrollback filled, ~1.1 KB per 120-column row; GUI +
  daemon with one filled session 19.2 MB.
- Target: the same scrollback limit costs no more than with libghostty.
- Cells: 8 bytes each (code point 21 bits, width 2 bits, style id 16 bits,
  flags), as Ghostty packs them. Styles in a per-screen table with
  reference counts. Grapheme extras (combining marks, ZWJ sequences) in a
  side map keyed by cell. Rows: a header (wrap, dirty, length) and their
  cells.
- **`scrollback-limit` stays bytes**: the engine counts what scrollback
  rows hold — 8 bytes per cell kept, the row header, grapheme extras, the
  share of styles — and drops the oldest rows beyond the limit.
  libghostty counts whole pages, so at the same limit the two keep
  different numbers of rows. That is a known difference of accounting, not
  of terminal behaviour: E1 compares scrollback with a limit neither
  reaches, and eviction is tested on its own.
- Trailing blank cells of scrollback rows are not stored (a row keeps its
  used length). Typical Claude Code output then needs less than ~1.1 KB per
  row, so the same bytes keep more history.

### 10.4 The engine boundary

- `haetty-core` gets a synchronous engine trait — feed bytes, take the
  replies and events, resize, snapshot, modes, scroll the viewport, set
  colours and the colour scheme, encode key / mouse / paste / focus, dump —
  with two implementations: `GhosttyEngine` (today's `engine.rs`, behind
  it) and `NativeEngine` (new crate `haetty-vt`, pure Rust). The engine
  thread, the PTY writer queue, the sync-output timing, the OSC scanner and
  the `Snapshot` format stay as they are, around the trait. E1's tool
  drives the same trait without threads.
- The key and mouse types become `haetty-core`'s own, **with libghostty's
  numbering** (the wire carries them as `u32`, §3.6); the ghostty engine
  converts. Then no other crate names libghostty.
- Cargo features of `haetty-core` (and passed through by the binaries):
  `ghostty` (default) and `native`. A build with `--no-default-features
  --features native` has no `libghostty-vt` in its dependency tree, so no
  Zig. The gate gets a step that builds it for both targets with `zig`
  taken off `PATH`. E3 also builds it on Windows itself (`cargo` on Windows,
  no WSL).
- Choice: the setting `engine = ghostty | native` (default `ghostty`) per
  **session**. It travels in `CreateSpec` (protocol 9), so one window can
  hold both for comparison, and an engine that is not compiled in is
  refused with a hint. Decided per session rather than per daemon: the
  daemon reads no config file, and A/B in one window is what E1–E3 need.

### 10.5 Differential tests (E1)

- A dev-only crate `haetty-vtdiff` (`publish = false`), the only place both
  engines are linked together, so the shipped native build never contains
  libghostty. Zig is needed only to build this crate.
- It feeds the same bytes to both engines.
  - Compares at fixed chunk boundaries, at randomly chosen split points (a
    parser state at a chunk edge), and at the end.
  - Compared per cell: text, width, fg, bg, flags; plus cursor (position,
    shape, visible), modes, kitty flags, title, pwd, scrollback line count,
    and the reply bytes.
  - Reports the first difference: input offset, row and column, field, both
    values, the last bytes fed.
- Inputs:
  - the recorded Claude Code corpus (`~/m4/vtcorpus`, `scripts/e2e/README.md`);
  - synthetic load;
  - vttest-like scenarios aimed at hidden state — scroll regions, insert and
    delete, the pending-wrap state at the last column, origin mode, tab
    stops, charsets, saved cursor, the alternate screen;
  - scenarios with resizes;
  - fuzz input (the vtfuzz generators).
- A report: match rate per category, and every remaining difference sorted
  into ours (a bug), interpretation (with the xterm ctlseqs / ECMA-48 / DEC
  STD 070 reference that supports each reading) or undecided.

### 10.6 The core engine (E2, 2026-11-07)

- `crates/haetty-vt` has the following:
  - screen grid and cursor with pending wrap;
  - scroll regions and origin mode;
  - primary and alternate screens (47, 1047, 1049), scrollback;
  - wide characters (a spacer in the last column when one does not fit);
  - combining marks and variation selectors;
  - SGR with the theme palette, OSC 4/10/11/12 and 104/110–112;
  - tab stops, G0–G3 with DEC special graphics (SI/SO, LS2/LS3, SS2/SS3);
  - the modes in `Modes`, including kitty flags per screen;
  - title, pwd, OSC 52 and the bell as events;
  - per-row change flags (`take_dirty`).
- It answers these queries, byte for byte as libghostty does:
  - DA1/2/3, DECID, DSR 5/6, XTVERSION;
  - DECRQM for private modes;
  - CSI 14/16/18 t, mode 2048 size reports;
  - OSC 4/10/11/12 queries (ending with BEL or ST, as asked);
  - kitty `CSI ? u`, `CSI ? 996 n`;
  - the kitty graphics support query (`ESC _ G a=q,i=N`).
- Widths come from `width_table.rs`, generated by
  `haetty-vtdiff --gen-width` from libghostty itself:
  - 375 zero-width ranges;
  - 126 wide ranges;
  - 185 code points a variation selector may follow (after any other, it
    is dropped).
- Behaviours taken over from libghostty, each found by `vtdiff --bisect`
  and checked with `--probe`:
  - Parameters are counted as libghostty counts them: `CSI 5;B` has one
    parameter, `CSI ;B` one, `CSI B` none. A sequence with more
    parameters than it takes is ignored whole (CUU…CUP, ED, EL, IL, DL,
    DCH, SU, SD, ECH, CBT, ICH, HPA/HPR/HPB, REP, VPA/VPR/VPB, TBC, DSR,
    XTWINOPS, DECSCUSR, DECRQM, DECSTBM).
  - An explicit 0 does nothing for IL, DL, DCH, SU, SD, CHT and CBT
    (elsewhere it means 1).
  - TBC needs its parameter (`CSI g` does nothing).
  - `CSI s` saves the cursor only without parameters (with them it would
    be DECSLRM).
  - A pending wrap survives SU, SD, SGR, HT, CBT, ED 3, unknown ED modes,
    DECSC/DECRC, and RI when it scrolls. It is cleared by:
    - CR, LF, BS (which also moves left);
    - cursor moves;
    - EL, ED 0/1/2, ECH, DCH, ICH;
    - RI when it moves up.
  - CUD from above the region's bottom stops at that bottom; CUU from
    below its top stops at that top.
  - SGR 38/48/58 with an unknown kind or too few values drops only the
    38, and the parameters after it count alone. Colour values wrap to a
    byte.
  - RIS clears the scrollback. ED 22 moves the rows down to the last used
    one into the scrollback, then clears the screen.
- Results (`vtdiff --fuzz 5000`, the corpus, 3 synthetic loads, 16
  scenarios, 2 resize scenarios, each cut two ways): 10 045 of 10 046 runs
  identical. The corpus has 0 differences. The random-input test in
  `haetty-vt` (300 rounds of random bytes, resizes down to 1×1, viewport
  scrolls) does not panic.
- One remaining difference, a libghostty bug the native engine does not
  copy. `CSI 0 g` on a column without a tab stop *sets* one in
  libghostty: `Tabstops.unset` toggles the bit (`^=`) instead of clearing
  it. ECMA-48 §8.3.154 (TBC, Ps = 0: "the character tabulation stop at the
  active presentation position is cleared") and xterm's ctlseqs ("Ps = 0
  ⇒ Clear Current Column") agree that it clears. Classified as a
  libghostty interpretation difference against the specification.
- E3 still has to do the following:
  - encoders (keys, mouse, paste, focus);
  - the byte-limited scrollback and the 8-byte cell layout (§10.3);
  - selection;
  - grapheme clustering under mode 2027.
