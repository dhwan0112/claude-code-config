## Delegation to workers
If you are in plan mode, you may only delegate to worker-readonly or Explore. Otherwise, follow the rules below.

- Do not delegate tasks you could finish yourself in one or two tool calls. Each subagent starts with fresh context and costs several thousand tokens of overhead; delegate only substantial, independent units of work.
- When a task splits into independent units, delegate them to worker-* subagents instead of doing everything yourself.
- Choose the worker by difficulty and permissions: worker-fast for simple scoped tasks, worker-deep for hard ones, worker-readonly for search/analysis.
- Every delegation prompt must begin with:
  ROLE: <what this worker is in this session>
  SCOPE: <files/directories it may touch; everything else is off-limits>
  CONTEXT: <relevant facts, paths, decisions so far>
  DONE WHEN: <concrete completion criteria, including tests to run>
  OUTPUT: <exact format of the report back>
- Delegate exactly what the user asked for. Do not add tasks (extra test suites, refactors, cleanups, additional checks) to a worker's SCOPE or DONE WHEN unless the user asked. If you think more is warranted, propose it in your final reply instead of doing it.
- Workers running in parallel must have non-overlapping SCOPEs. Never let two workers edit the same file at the same time.
- Keep a mental registry of role -> agent ID. For follow-up work in the same role, continue the existing worker by sending it a message with SendMessage to its agent ID instead of spawning a new one.
- Run independent tasks in parallel; wait for all results before integrating.
- You own integration and final review. Do not trust worker reports blindly. Verify with targeted checks: re-run the key tests, read `git diff --stat`, and inspect only the specific hunks that matter, rather than reading entire diffs.
- Shared files (dependency manifests, lockfiles, global config, shared type definitions) are edited only by you, never by workers. If a worker needs a change there, it must report it under NEXT_ACTION instead of making it.
- Treat a worker's STATUS: verified as a claim, not a fact. Before integrating, re-run at least the most important check listed in its EVIDENCE yourself.
- Before any irreversible or external action (git push, merging, publishing, deleting data), stop and ask the user for approval.
- Keep your own context small; every turn re-reads all of it. Do not read large files or print long command outputs yourself. Use `git diff --stat`, `head`, `grep` or targeted line ranges, and redirect long outputs to a file under /tmp. When you need the content of a large file, diff or log, delegate the reading to worker-readonly and ask for a short summary with file:line references.
- On this machine, run this repository's Python code with `<this machine's interpreter: check which python imports numpy and lammpkeeper>`. Pass this interpreter in the CONTEXT of every delegation that runs code.
