# Reports for Ghostty (drafts, not sent)

These are draft reports of libghostty-vt behaviours that haetty's differential
tests found (DESIGN.md §10.7 findings 1–3). The user decides whether to send
any of them; nothing here has been posted.

Before sending, read Ghostty's rules (`CONTRIBUTING.md`, `AI_POLICY.md` in
the Ghostty repository):

- Bugs go to an **"Issue Triage" discussion**, with its template filled in
  completely. They do not go to a new issue.
- AI use must be disclosed: the tool, and how much of the work it did.
- A person must review **and edit** AI-written text before it is posted.
- These drafts were written by Claude Code from haetty's tests and
  libghostty's source. Shorten them and put them in your own words.
- Search open and closed issues and discussions first.

Versions:

- haetty uses the `libghostty-vt` crate 0.2.2, which pins Ghostty commit
  `a887df42c56f6de86c0fe6da9c4eeca37931e083` (version 1.3.2-dev).
- The behaviours below were checked against that commit, and their source
  against Ghostty `main` at `5813c9461d5af78b6307a1254d00b1bc190eaccd`
  (2026-11-07). On 2026-11-09 the drafts were re-run on the pinned commit `e035beb` (see `repro/`).

| Draft | Finding | On the pin e035beb (2026-11-09) | Send? |
|---|---|---|---|
| [`01-tbc-0-toggles.md`](01-tbc-0-toggles.md) | TBC 0 toggles the tab stop | fixed by `bed20eb`; re-run on `e035beb`: fixed | no; kept for the record |
| [`02-origin-cha-moves-row.md`](02-origin-cha-moves-row.md) | CHA/HPA/HPR under origin mode move the row | fixed by `82df0be`; re-run on `e035beb`: fixed | no; kept for the record |
| [`03-render-state-unmarked-rows.md`](03-render-state-unmarked-rows.md) | the render state misses rows changed by a late grapheme join or a cleared spacer head | still present: re-run on `e035beb`, 5 of 6 cases stale (`repro/03-repro.c`) | the user's decision |

Findings 1 and 2 also explain why haetty's known-differences list holds them
(`docs/vtdiff/known.md`). With a libghostty-vt release built from a Ghostty
commit after the fixes, both entries would no longer be needed.

See also `libghostty-update-2026-11.md` (E5e): what a libghostty-vt update after 0.2.2 would change (survey, no version change).
