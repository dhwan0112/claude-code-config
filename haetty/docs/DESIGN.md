# Design: index

The design of haetty, by topic. This file is the index: one line per section, with the file that holds the text.
The text is in `docs/design/`, moved verbatim from the former single file in stage G0 (2026-11-09); section numbers
did not change, so references such as `docs/DESIGN.md §10.11` still point at the right section.

Read this index at the start of a session. Open only the file of the section you need, not all of them.
`docs/HANDOFF.md` holds the state of the code; the design holds the target, the decisions and their measurements.

Protected areas (no change without the person's review; `CLAUDE.md`): §2 (outer design), the reference frames, the
hook output rules, the IPC permission checks, and the path of Claude Code permission prompts (§3.7, §3.13).

| § | Section | In one line | File |
|---|---|---|---|
| 1 | Principles | One executable per platform, no runtime beyond system libraries; Rust except the VT core behind FFI | [01](design/01-principles-and-outer.md) |
| 2 | Outer design | What the user sees and when: nothing on screen without a job; a protected area | [01](design/01-principles-and-outer.md) |
| 2.1 | Layout | The default view is the terminal and one status line; a tab bar only from two tabs | [01](design/01-principles-and-outer.md) |
| 2.2 | Visual rules | Colours from one theme (default IBM 5153 CGA) and the other look rules | [01](design/01-principles-and-outer.md) |
| 2.3 | Checks | Outer-design checks after implementation: layout changes without input, accents on an idle screen | [01](design/01-principles-and-outer.md) |
| 3 | Inner design | Processes, crates, pipeline, socket, protocol, identity, keys, icon, Claude Code, themes, installs | [02](design/02-inner-processes-protocol.md) |
| 3.1 | Processes | GUI, daemon and sessions, and what runs where | [02](design/02-inner-processes-protocol.md) |
| 3.2 | Crates | The crates and each one's responsibility | [02](design/02-inner-processes-protocol.md) |
| 3.3 | Rendering pipeline | From wakeup to drawn cells: snapshot or row diff, layout, rasterizing the changed cells | [02](design/02-inner-processes-protocol.md) |
| 3.4 | Budgets | Latency and memory targets per session, to be measured (`scripts/e2e/measure.sh`) | [02](design/02-inner-processes-protocol.md) |
| 3.5 | Local socket | The local endpoint: a Unix socket under the runtime directory, a named pipe on Windows | [02](design/02-inner-processes-protocol.md) |
| 3.6 | Wire format and session protocol | Frames (u32 length, postcard) and the session protocol's messages | [02](design/02-inner-processes-protocol.md) |
| 3.7 | Roles | The control, observe and report roles, and what each may send (protected) | [02](design/02-inner-processes-protocol.md) |
| 3.8 | Identity, state and event log | Session and tab ids, the saved tab set, the state directory, the event log | [03](design/03-inner-identity-keys-product.md) |
| 3.9 | Product name | The name haetty (decided 2026-11-05) and where it lives in the code | [03](design/03-inner-identity-keys-product.md) |
| 3.10 | Layout rules derived from §2 | The first grid at the monitor's scale; resize rules | [03](design/03-inner-identity-keys-product.md) |
| 3.11 | Keys | Application shortcuts use Ctrl+Shift, Ctrl+Tab and Alt+digit; plain Ctrl+letters go to the program | [03](design/03-inner-identity-keys-product.md) |
| 3.12 | Icon and desktop integration | The icon design (`assets/icon/haetty.svg`) and desktop integration per platform | [03](design/03-inner-identity-keys-product.md) |
| 3.13 | Claude Code integration (milestone 3) | Which tabs are Claude Code tabs; hooks, settings, the status line (protected: permission prompts) | [04](design/04-inner-claude-themes-installs.md) |
| 3.14 | Themes | Ghostty's theme files as the format; the bundled themes | [04](design/04-inner-claude-themes-installs.md) |
| 3.15 | Stable installs | Installing a commit or tag from a worktree; rollback; self-tests | [04](design/04-inner-claude-themes-installs.md) |
| 4 | VT engine decision | The 2026-11-04 decision, from a replay of a real Claude Code session | [05](design/05-vt-engine-decision.md) |
| 5 | Milestones | The milestones, starting with one window and one session | [06](design/06-milestones-external-orchestrator.md) |
| 6 | External integration | Integration through Rakazo's existing surfaces, no fork | [06](design/06-milestones-external-orchestrator.md) |
| 6.1 | Observe-only interface | The read-only base for later adapters: protocol and daemon (stage V3) | [06](design/06-milestones-external-orchestrator.md) |
| 8 | Orchestrator | Milestone 4, first step: one task, one worker, reports and display | [06](design/06-milestones-external-orchestrator.md) |
| 8.1 | Task and worker | Task fields and the worker created for a task | [06](design/06-milestones-external-orchestrator.md) |
| 8.2 | Worker reports and states | Where a report comes from (the Stop hook) and the states | [06](design/06-milestones-external-orchestrator.md) |
| 8.3 | Display | Worker tab names and state glyphs | [06](design/06-milestones-external-orchestrator.md) |
| 8.4 | Safety boundary | What the orchestrator may send, and the one function that sends it (`Origin::Program`) | [06](design/06-milestones-external-orchestrator.md) |
| 9 | Updating the stable instance without closing the tabs | Design only: options (a), (b), (c) | [07](design/07-stable-update.md) |
| 9.1 | (a) Old and new daemon side by side | Both daemons run; new sessions go to the new one | [07](design/07-stable-update.md) |
| 9.2 | (b) Hand the sessions to the new daemon | The old daemon passes its sessions to the new one | [07](design/07-stable-update.md) |
| 9.3 | (c) Close and resume, made smooth | Today's model, with the update as one action | [07](design/07-stable-update.md) |
| 9.4 | Recommendation | (c) now | [07](design/07-stable-update.md) |
| 10 | A terminal engine in Rust | The goal and plan of haetty's own engine beside libghostty-vt | [08](design/08-engine-e0-e2.md) |
| 10.1 | What haetty uses of libghostty-vt today | Every use in the code: the surface the engine must cover | [08](design/08-engine-e0-e2.md) |
| 10.2 | Build it, or take a small crate | The choice, with measured sizes | [08](design/08-engine-e0-e2.md) |
| 10.3 | Memory | Memory per session: the baseline and the targets | [08](design/08-engine-e0-e2.md) |
| 10.4 | The engine boundary | The synchronous engine trait in `haetty-core` | [08](design/08-engine-e0-e2.md) |
| 10.5 | Differential tests (E1) | `haetty-vtdiff`, the dev-only tool that links both engines | [08](design/08-engine-e0-e2.md) |
| 10.6 | The core engine (E2) | The first core engine in `crates/haetty-vt`: grid, cursor, modes | [08](design/08-engine-e0-e2.md) |
| 10.7 | E3: a wider harness | Wider differential tests, the native engine as a session option, encoders, memory | [09](design/09-engine-e3.md) |
| 10.8 | E4a: rows left unmarked | The render-state defect in libghostty and the default engine's fix | [10](design/10-engine-e4.md) |
| 10.9 | E4: KNOWN or FAIL | The default's rule (no unexplained differences), margins, 8-byte cells, kitty graphics | [10](design/10-engine-e4.md) |
| 10.10 | E5: towards no unexplained differences | Approved known differences, underlines and links, throughput | [11](design/11-engine-e5.md) |
| 10.11 | F0: the soak rule | One-hour soak per engine; the night run of 2026-11-09 | [12](design/12-engine-f-stages.md) |
| 10.12 | F1: the newest Ghostty as the reference | `ghostty-main` (`e035beb`) as the vtdiff reference, 0.2.2 side by side | [12](design/12-engine-f-stages.md) |
| 10.13 | F2: following the newest Ghostty | The C0, C1 and DECRQSS changes; 744 FAILs remain | [12](design/12-engine-f-stages.md) |
| 10.14 | F3: SGR-heavy throughput | SGR throughput from 56 % to 82 % of libghostty; the changes | [12](design/12-engine-f-stages.md) |
| 10.15 | F4: native-only readiness | Native-only dist sizes, the Windows build without WSL, latency, the transition table | [12](design/12-engine-f-stages.md) |
