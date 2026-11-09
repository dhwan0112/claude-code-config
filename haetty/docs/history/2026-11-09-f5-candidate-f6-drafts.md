# 2026-11-09 — F5 candidate and F6 upstream drafts

F5 (candidate `b1262d2`, merged to main): version 0.3.0, CHANGELOG, vtdiff's XTVERSION follows the crate.
- Full vtdiff against ghostty-main after the change: 2066 rows, 1322 identical, 744 Fail (as F2), corpus 4 of 4.
- Linux: run-all on both engines and on the native-only build pass; native-only unit tests (every crate except
  haetty-vtdiff, no Zig) 185 passed; installer self-test 19/19 (v0.2.0 against the candidate).
- Windows: native-only unit tests 160 passed; native-only build and smoke (`haetty 0.3.0`, native session `ok`);
  installer self-test 18/18; daemon-crash 4 PASS.
- Night run of the candidate: registered for 2026-11-10 02:00 (`haetty-overnight-f5`); the tag waits for it.

F6 (upstream drafts, none sent):
- 01 (TBC 0): fixed upstream by `bed20eb`; the draft's recipe re-run on the pin: fixed.
- 02 (CHA/HPA/HPR under origin mode): fixed upstream by `82df0be`; re-run on the pin: fixed.
- 03 (render state, late grapheme join, spacer head): still present on the pin; re-run with `repro/03-repro.c`:
  5 of 6 cases stale. The draft's source claim holds (Terminal.zig line 2182).
- The drafts' README had the fix hash wrong (`5813c94`, an unrelated commit); corrected.
