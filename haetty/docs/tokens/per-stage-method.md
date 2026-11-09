# Token use: one long session against a session per stage (stage H4, 2026-11-09)

**Question.** Does a stage cost fewer tokens in its own session (see [../OPERATING.md](../OPERATING.md)) than in one long
session?

**Measure.** `haetty tokens --top 10 PATH` on each session's transcript (read-only; it prints names, sizes and counts,
no conversation content). The numbers that matter: the **cache read** total (every response re-reads the context, so it
grows with the session), the output total, and the number of responses per stage.

**Baseline: one long session.** The whole transcript of this session (the earlier stages F and G, then K0 to K3b and H),
measured at 23:05 on 2026-11-09. The stages in it are not measured one by one.

| measure | value |
|---|---|
| records | 50,703 |
| API responses | 4,445 |
| tool calls | 5,232 |
| input | 9,264 |
| output | 6,699,295 |
| cache write | 18,033,218 |
| cache read | 2,403,136,458 |
| sum of the four | 2,427,878,235 |
| cache hit | 99.3 % |

The largest tool results are reads of big files: a documentation page fetched twice (45 KB each) and a 45 KB source file
read from the old copy before the move to the archive (stage H1).

**Per-stage measurement: pending.** Each stage in its own session, then `haetty tokens` on each transcript; the comparison
is the cache read and the responses per stage, for stages of the same kind (a start, a check, a commit). It needs new
sessions, and each costs quota (the limits are in OPERATING.md), so it is not run yet.

**Limits.** One run of one session. The totals include any subagents' responses. A new session starts with the same
documents, so its first response costs the same as this session's first. The report carries no conversation content.
