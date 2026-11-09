# 2026-11-09 — G0: DESIGN by topic, CLAUDE.md shortened, HANDOFF under 400 lines

Stage g0. The session-start documents (read at the start of a session), before and after:

Before (`docs/DESIGN.md` whole, `CLAUDE.md`, `docs/HANDOFF.md`, `docs/SELF-HOSTING.md`; estimate = characters / 4):

```
file                        lines    chars   words est.tokens
CLAUDE.md                     205    12718    1789       3179
docs/HANDOFF.md               414    28169    4267       7042
docs/DESIGN.md               2117   134376   21419      33594
docs/SELF-HOSTING.md          146     7689    1056       1922
total                                                   45737
```

After (session start: CLAUDE.md, HANDOFF, DESIGN index; SELF-HOSTING unchanged):

| file | lines | chars | est. tokens |
|---|---|---|---|
| CLAUDE.md | 115 | 6927 | 1731 |
| docs/HANDOFF.md | 395 | 26549 | 6637 |
| docs/DESIGN.md | 66 | 8025 | 2006 |
| docs/SELF-HOSTING.md | 146 | 7689 | 1922 |
| total | | | 12296 |

DESIGN.md: 2 117 lines in, 65 index lines out; the 12 topic files in `docs/design/` hold 2 117 lines verbatim
(`tools/check-moved-lines.py`: 1 971 non-blank lines found, 0 missing). CLAUDE.md: 190 non-blank lines checked, 0
missing (the rules stay in CLAUDE.md, the commands, environment and the long rule details in `docs/dev-commands.md`).
HANDOFF: section 7 moved to `docs/history/handoff-sections.md`; the stale first bullet of 6.0 moved below.

## Moved from HANDOFF 6.0 (verbatim, the 2026-11-07 open list)

- E5 (DESIGN §10.10) done; the default engine stays libghostty. Open:
  - vtdiff FAIL 79 runs (grammar 56, fuzz2 21, decor 2); classes in
    §10.10. KNOWN candidates no-pending-wrap-without-autowrap and
    ctc-2-clears wait for the user's decision; the trimmed-rows class
    (libghostty re-exposes rows a shrink trimmed) has no candidate yet
    (needs a switch). VPR past the bottom margin without origin mode
    (`\e[;8r\e[20e`: libghostty to the screen bottom, haetty-vt to the
    margin) is undecided: xterm's behaviour to confirm.
  - Native soak rule: the 2-hour run FAILs (+7.4 MB after the burst tab
    filled) on the slow tab still filling its scrollback (~3 h). The user
    decides: a longer native soak, a smaller scrollback limit in the soak,
    or an allowance for rows still being added.
  - SGR-heavy throughput 58 % of libghostty (E4 bar 50 %).
  - Next overnight (§6.1 item 30): `-Engine native -GuiE2E` (commit HEAD).
  - libghostty-vt after 0.2.2: `docs/upstream/libghostty-update-2026-11.md`.

## Moved from HANDOFF 'Next session' (verbatim, before G0)

## Next session

- F0 done (section 10.11); the night run of 2026-11-09 passed on native (§6.1 item 30) at `bb61e92`.
- F1 done (10.12): ghostty-main is the default reference; 0.2.2 side by side.
- F2 done in part (10.13): ghostty-main FAIL 877 -> 744 (docs/vtdiff/2026-11-09-f2.md).
- F3 done (10.14): SGR-heavy throughput 82 % of libghostty (session), from 56 %.
- F4 done (10.15): native-only builds pass the e2e suite; the Windows build without WSL works; NOT MET: the 744
  vtdiff FAILs, key-to-frame latency (0.13 ms over libghostty), the native-only installer path; SGR is at 80 %.
- F5: the v0.3.0 candidate `b1262d2` is on main (version, CHANGELOG, XTVERSION). Its Linux and Windows checks pass
  (e2e on both engines, native-only build and unit tests, installer self-tests on both platforms, daemon-crash).
  The night run is registered for 2026-11-10 02:00 (native, then libghostty, both with -GuiE2E). The tag
  `haetty-v0.3.0` on `b1262d2` only after both nights PASS; then the F5 record (DESIGN 10.16).
- F6 done: drafts 01 and 02 are fixed upstream (`bed20eb`, `82df0be`; re-run on the pin); 03 is still present (re-run:
  5 of 6 cases stale); `docs/upstream/repro/`; none sent.
- Next: G0 (DESIGN split by topic, CLAUDE.md), then G1-G7 (token saving, the brief of 2026-11-09).
- Leave "Stable" in §1 until the user says the new version is installed.

## Moved from CLAUDE.md intro (verbatim, before G0; it read all of DESIGN at session start)

crate `haetty-gui`) are done. Read `docs/HANDOFF.md` (state, verified facts,
pitfalls, tasks) and `docs/DESIGN.md` (target outer and inner design) once at
the start of a session.
