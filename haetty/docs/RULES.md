# Rules and what enforces them

One row per rule of `CLAUDE.md`, `docs/HANDOFF.md` §3 and §4, `docs/OPERATING.md` and `docs/dev-commands.md`: the rule, what really stops a violation (`none` when only a person or a session has to remember it), and where the rule and its enforcement live.
The documents stay the source of each rule's wording; this table only says in one line what the rule is and how it is held.
A new rule gets a row here in the same commit; a rule that gains or loses an enforcement changes its row.

Enforcement names: the gate = `scripts/gate.sh`; check-range = `tools/check-range.sh`; known-gate-failures = `tools/known-gate-failures`; test-guard = `scripts/test-guard.sh` / `.ps1` and `haetty_testkit`. "Only when used" means the enforcement runs when the script is run; a plain `git` or shell command beside it is not stopped.

## CLAUDE.md

| # | Rule | Enforcement | Where |
|---|---|---|---|
| 1 | Every change builds for `x86_64-unknown-linux-gnu` and `x86_64-pc-windows-gnu`. | The gate builds both release targets and fails the step; check-range runs clippy (which compiles) on both targets for each commit. Only when run. | CLAUDE.md Hard requirements; HANDOFF §3; `scripts/gate.sh`, `tools/check-range.sh` |
| 2 | Clippy reports 0 warnings on both targets and all tests pass. | The gate: clippy per target judged on its own output, `cargo test` (FAILED, panicked or a compile error fails it). check-range repeats fmt, clippy and test per commit. Only when run. | CLAUDE.md Working rules (several sessions); HANDOFF §3; dev-commands "Commands"; `scripts/gate.sh`, `tools/check-range.sh` |
| 3 | No platform-only feature without a working counterpart or a documented, tested fallback on the other platform. | Partial: the gate compiles the Windows target, so a missing counterpart that breaks the build fails. That a counterpart or tested fallback exists: nothing. | CLAUDE.md Hard requirements; `scripts/gate.sh` |
| 4 | Windows counterparts are checked and fixed in stage PW, after all other stages. | `none` | CLAUDE.md Hard requirements; HANDOFF "Next session" |
| 5 | Lightweight: no Electron or webview, no wgpu, no general UI framework for the terminal itself. | `none` (no dependency deny list; the gate only prints sizes) | CLAUDE.md Hard requirements |
| 6 | Check the binary size impact of every new dependency (stripped release build) and report it. | `none` (the gate prints stripped sizes of the four binaries; no limit, no comparison, no report) | CLAUDE.md Hard requirements; `scripts/gate.sh` (sizes step), `scripts/dist-sizes.sh` |
| 7 | Standalone: one executable per platform, no installer, no runtime dependency beyond system libraries. | `none` (the gate prints the Linux NEEDED list and the Windows DLL list; nothing compares them) | CLAUDE.md Hard requirements; `scripts/gate.sh` (linkage) |
| 8 | Rust only, except libghostty-vt (Zig, FFI in `haetty-core`) and the ConPTY binaries; no Python or shell at run time. | `none` | CLAUDE.md Hard requirements |
| 9 | libghostty-vt objects are `!Send` and live only on the engine thread; frontends use `SessionHandle`, `SessionEvent` and `Snapshot` only. | Types and crate graph: `Terminal` is `!Send` (the compiler rejects moving it), `libghostty-vt` is a dependency of `haetty-core` only (and the dev tool `haetty-vtdiff`), and the gate's native-only step fails if libghostty is in the `haetty-gui` / `haetty-headless` graph without the default feature. | CLAUDE.md Hard requirements; `crates/haetty-core/src/lib.rs`, `crates/*/Cargo.toml`, `scripts/gate.sh` (native-only) |
| 10 | Never read, store or forward Claude credentials or OAuth tokens. | `none` | CLAUDE.md Hard requirements |
| 11 | Logs must not record typed text (nor shown text). | Type: `Secret<T>` in `crates/haetty-proto/src/wire.rs` has a `Debug` that prints only a size, and the wire messages carrying typed or shown text use it. Test: `crates/haetty-daemon/tests/daemon.rs` asserts a pasted `secret-typed-text` is absent from the daemon log. A field not wrapped in `Secret`: nothing. | CLAUDE.md Hard requirements; HANDOFF §3 |
| 12 | Hooks are the same executable (`<bin> hook <event>`): no cache files, no jq, no flock. | `none` | CLAUDE.md Hard requirements |
| 13 | The daemon API is a typed, versioned schema with durable ids and an append-only event log. | Partial: types in `haetty-proto/src/wire.rs`; `crates/haetty-daemon/tests/ipcfuzz.rs` checks that a wrong magic or an old or future version is refused. Durable ids and append-only: no test. | CLAUDE.md Hard requirements; DESIGN §6 |
| 14 | Remote parties (CLI over SSH, MCP) never answer Claude Code permission prompts. | Daemon role checks: an Observe connection gets `Denied` for write, paste, close and set-observable (`crates/haetty-gui/tests/observe.rs` `observe_only_interface`); a Program-origin write is refused while a prompt is open (`crates/haetty-gui/tests/permission.rs`: `program_write_taken`, `turn_end_releases_the_hold_and_a_mouse_press_answers`). | CLAUDE.md Hard requirements; DESIGN §6; `Daemon::message`, `Daemon::input` in `crates/haetty-daemon/src/lib.rs` |
| 15 | Outer design (DESIGN §2): events never change the layout, no accent colour on an idle screen, no sounds, pop-ups or animations. | Partial: `reference_frames` in `crates/haetty-gui/src/reftest.rs` compares rendered frames with `crates/haetty-gui/tests/reference/` in `cargo test`, so a look change fails the gate until the frames are updated. Sounds, pop-ups, animations: nothing. | CLAUDE.md Hard requirements; DESIGN §2 |

## CLAUDE.md, working rules

| # | Rule | Enforcement | Where |
|---|---|---|---|
| 16 | Run new builds only as a separate instance (`HAETTY_INSTANCE=dev`: own daemon, socket, state, config, app id, title prefix). | `tools/dev-run.sh` refuses (exit 3) when any override points at the stable instance, a config names stable paths, the binary is the installed stable one, the build does not report instance dev, or the socket is the stable one; `crates/haetty-gui/tests/instances.rs` `stable_and_dev_instances_do_not_interfere`; self-test `tools/test-dev-run.sh`. Only when `dev-run.sh` is used. | CLAUDE.md Working rules; HANDOFF §3; `tools/dev-run.sh` |
| 17 | Never replace the stable binary; installing it is the person's job; a session never applies or rolls back an update and never signs or publishes a release; promotion needs approval. | Partial: `tools/install-stable.sh` installs read-only (0555) version folders; `tools/release.sh` stops before signing and before `gh release create` when `CLAUDECODE` or `CLAUDE_CODE_*` is set (and never signs on a dry run). A typed `cp`/`mv`, or applying an update from a session: nothing. | CLAUDE.md Working rules; HANDOFF §3; OPERATING Limits; `tools/install-stable.sh`, `tools/release.sh` |
| 18 | Never signal, stop or restart the stable instance or its daemon (nor touch its install). | `none` (`install-stable.sh` only prints suggested deny rules for the person's own settings; they are not in the repository's `.claude/settings.json`) | CLAUDE.md Working rules; HANDOFF §3; OPERATING Limits |
| 19 | Manage dev and test processes only by PID or by their own socket, never by name (no `pkill`, `killall`, `taskkill /IM`, `Stop-Process -Name`, `pkill -f`). | Partial: `haetty_stop_daemon` in `scripts/test-guard.sh` stops by the run's socket; run-all's last step fails when a process the tests started is still running (`scripts/e2e/leftovers.sh`, PK) | CLAUDE.md Working rules; HANDOFF §3; `scripts/test-guard.sh` |
| 20 | A session never creates, uploads to or deletes a GitHub release (drafts included); of `gh release` it uses only `view` and `list`. | Partial: `tools/release.sh` does not reach `gh release create` inside a Claude Code session. No deny rule for `gh release` exists in `.claude/settings.json`; a typed `gh release`: nothing. | CLAUDE.md Working rules; `tools/release.sh` |
| 21 | Several sessions: each works in its own `git worktree` on its own branch, never two sessions in one checkout. | `none` (`stage.sh start` creates a worktree and branch when used; nothing stops a second session in a checkout) | CLAUDE.md Working rules |
| 22 | A branch is merged into `main` only after the full gate, one session at a time; the next session rebases and passes the gate again. | Partial, only when the stage scripts are used: `stage.sh finish` merges with `--ff-only` (fails if `main` moved, so a rebase is needed) after `c.sh` ran the gate and before `push.sh` runs check-range. Re-running the full gate after the rebase: nothing. | CLAUDE.md Working rules; OPERATING "One stage"; `tools/stage/stage.sh`, `tools/stage/c.sh`, `tools/stage/push.sh` |
| 23 | Protected areas (DESIGN §2 and reference frames, hook output rules in `haetty-cc/src/report.rs`, the IPC role checks, the permission-prompt path and injected settings, the attention state and input hold, the shared runtime directory, the stable install) change only after the person's review, and the commit and report flag them. | `none` for the review. Tests pin some of the content: hook exit 0 with empty stdout and stderr (`hook()` in `crates/haetty-gui/tests/permission.rs`), the role checks (`observe.rs`), the frames (`reftest.rs`). | CLAUDE.md Working rules; HANDOFF §3; OPERATING Limits |
| 24 | Keep a recovery path: do not remove or break ccx (tmux) while haetty is not the proven default. | `none` | CLAUDE.md Working rules |
| 25 | Tests write and remove files only in their run directory; the guards refuse an empty base and the real home, config and profile folders. | test-guard: `haetty_guard_init` exits 3 on an empty, relative or real-home base; its exit trap removes the run directory when the script ends well (read-only folders included); run-all's last step (`scripts/e2e/leftovers.sh`) fails on a test folder left in `HAETTY_TEST_TMP` or `/tmp` (PK) | CLAUDE.md Working rules; HANDOFF §3; dev-commands "Test isolation"; `scripts/test-guard.sh`, `crates/haetty-testkit/src/lib.rs` |
| 26 | Claude Code in tests: an isolated home in the run directory and a dummy key only, `CLAUDE_CODE_*` / `CLAUDECODE` / `CLAUDE_*SESSION*` stripped. | Partial: `scripts/e2e/lib.sh` unsets the markers and calls test-guard's `haetty_guard_claude`, which drops every PATH folder outside the run directory that holds a `claude` and refuses a claude outside it (PK); `scripts/e2e/claude-home.sh` makes the isolated home. `.ps1`: not yet (PW) | HANDOFF §3; dev-commands "Test isolation"; `scripts/e2e/lib.sh`, `scripts/e2e/claude-home.sh`, `tools/check-range.sh`, `crates/haetty-core/src/lib.rs` |
| 27 | Every commit passes the full gate on its own; none relies on a later one. | `tools/stage/c.sh` runs the gate and makes no commit unless it prints `GATE fail=0`; check-range then checks each commit on its own. A plain `git commit`: nothing (no git hook). | CLAUDE.md Working rules; HANDOFF §3; dev-commands "Commits"; `tools/stage/c.sh`, `tools/check-range.sh` |
| 28 | Before each push `tools/check-range.sh origin/main..HEAD` must end with "0 failing". | `tools/stage/push.sh` runs it and does not push when it exits non-zero (the exit status is the number of FAIL commits). A plain `git push`: nothing. | CLAUDE.md Working rules; HANDOFF §3; dev-commands "Commits"; `tools/stage/push.sh`, `tools/check-range.sh` |
| 29 | Adding, changing or removing an entry of `tools/known-gate-failures` needs the person's approval, for that entry. | `none` (check-range rejects malformed lines and prints a NOTE when the file changed inside a range; it does not stop it) | CLAUDE.md Working rules; HANDOFF §3; dev-commands "Commits"; `tools/known-gate-failures`, `tools/check-range.sh` |
| 30 | A pushed commit that fails is recorded in HANDOFF, not rewritten; a failing unpushed commit is fixed before the push. | `none` for the record and for not rewriting (`push.sh` pushes without `--force`, so the remote's default refusal of non-fast-forward pushes applies). Unpushed: check-range fails the push. | CLAUDE.md Working rules; dev-commands "Commits"; `tools/stage/push.sh` |
| 31 | An approved change of the outer design (DESIGN §2) is one commit, "DESIGN §2 change, approved", with the updated reference frames. | `none` | CLAUDE.md Working rules; dev-commands "Commits" |
| 32 | A reference-frame update without an intended look change is a commit of its own whose message says why; update with `HAETTY_UPDATE_REFERENCE=1 cargo test reftest`, and a rerun must report every frame identical. | `none` (`reftest.rs` reports identical or differing frames; nothing requires the rerun, the separate commit or the message) | dev-commands "Commits"; `crates/haetty-gui/src/reftest.rs` |
| 33 | No comments in new or changed code (only the `no-comments` skill's exceptions); an existing comment is dealt with only when its line changes; protected-area comments are not deleted or moved. | Partial: `tools/check-comments.sh` in the gate lists ordinary `//` comments on added Rust lines (a warning until 2026-12-19, then a failure; PK); the skill's check at a stage's end | CLAUDE.md Working rules |
| 34 | Why a change was made goes into the commit body and `docs/history/`, with no per-file change list; "Checked:" and what is unverified are written as now. | `none` | CLAUDE.md Working rules |
| 35 | Decisions go into the record's "Choices made" section. | `none` | CLAUDE.md Working rules |
| 36 | The same mistake or instruction twice: make it an enforcement, not text; an incident that cannot be undone or that touched stable or the real environment counts at once. | `none` (this table is where the result is kept) | CLAUDE.md Working rules |
| 37 | A tool that would grow bigger than the work is written in the report as a proposal. | `none` | CLAUDE.md Working rules |
| 38 | The three skills' check runs once at a stage's end, on its diff. | `none` | CLAUDE.md Working rules |
| 39 | Heavy jobs: at most two at a time, Linux and Windows together; any other heavy command goes through `tools/heavy.sh` / `tools/heavy.ps1`. | `tools/heavy.sh`: two `flock` slots, a third job waits; `gate.sh`, `check-range.sh` and the stage scripts re-exec through it; self-test `tools/test-heavy.sh`. A heavy command run by hand around it: nothing. | CLAUDE.md Working rules; HANDOFF §3; OPERATING "The stage scripts"; dev-commands "Commands"; `tools/heavy.sh` |
| 40 | A vtdiff known difference is added directly only when `ghostty-main` agrees with haetty and an ECMA-48 or xterm clause gives the reason; otherwise it goes to `known-candidates.txt` for the person. | Partial: `load_known` in `crates/haetty-vtdiff/src/main.rs` refuses an entry of `known-vt-differences.txt` without a non-empty `approved` field (unit tests at its call sites), and the candidates list is counted FAIL. The two conditions themselves: nothing. | CLAUDE.md Working rules; dev-commands "Commands"; `docs/vtdiff/known-vt-differences.txt` |
| 41 | Changing or removing a vtdiff known-difference entry needs the person's approval. | `none` | CLAUDE.md Working rules; dev-commands "Commands" |

## CLAUDE.md, conventions

| # | Rule | Enforcement | Where |
|---|---|---|---|
| 42 | Code, comments, docs and commit messages in English. | `none` | CLAUDE.md Conventions |
| 43 | Commits carry the user's git identity and no AI attribution or co-author lines. | `none` (`tools/stage/push.sh` prints the count of unpushed commit lines matching co-authored, claude-session or generated with; it does not stop the push) | CLAUDE.md Conventions; HANDOFF §3; `tools/stage/push.sh` |
| 44 | Do not push, tag or publish without asking. | `none` | CLAUDE.md Conventions |
| 45 | A release tag needs the full Linux candidate check, short soak included (`scripts/e2e/soak-short.sh`), and `tools/check-range.sh haetty-v<previous>..<commit>` with 0 failing (KNOWN does not count as failing). | `none` (`tools/release.sh` runs neither; the scripts print `VERDICT` and the failing count, the person or session reads them) | CLAUDE.md Conventions; dev-commands "Commits", "Short soak"; `scripts/e2e/soak-short.sh`, `tools/check-range.sh` |
| 46 | The release's CHANGELOG section says "Windows: verified / unverified / known problems". | `none` | CLAUDE.md Conventions; dev-commands "Windows-only checks" |
| 47 | State what was verified and how (command and observed result); mark what was not verified, especially anything Windows-only. | `none` | CLAUDE.md Conventions |
| 48 | Keep `docs/HANDOFF.md` under 400 lines. | `none` (it has 400 lines now) | CLAUDE.md Conventions; OPERATING "One stage" step 6 |
| 49 | Dated progress records, measurements and solved defects go to the newest file in `docs/history/`, not HANDOFF. | `none` | CLAUDE.md Conventions |
| 50 | A new pitfall gets one line in HANDOFF §4 (details in the record). | `none` | CLAUDE.md Conventions; HANDOFF §4 |
| 51 | When moving text, check with `tools/check-moved-lines.py OLD NEW…` that no line was lost. | `none` (the tool exists; nothing runs it) | CLAUDE.md Conventions; `tools/check-moved-lines.py` |

## HANDOFF §3 (rules not already above)

| # | Rule | Enforcement | Where |
|---|---|---|---|
| 52 | `CCTERM_*` environment names stay readable. | `none` (only the manual check §6.1 item 3; `HAETTY_*` is the documented name in `crates/haetty-proto/src/product.rs`) | HANDOFF §3, §6.1 item 3 |

## HANDOFF §4, backend and ConPTY

| # | Rule | Enforcement | Where |
|---|---|---|---|
| 53 | Never send ConPTY flag 0x8 (`PSEUDOCONSOLE_GLYPH_WIDTH_GRAPHEMES`) to the in-box host. | `none` (`crates/haetty-pty/src/win/mod.rs` sends no width flag by default; Windows-only, the gate only builds it, no test) | HANDOFF §4 |
| 54 | Do not use portable-pty's hard-coded ConPTY flags and fixed DLL for Windows; haetty has its own Windows backend. | `none` (`portable-pty` is still a workspace dependency of `haetty-pty`; nothing forbids using it for ConPTY) | HANDOFF §4; `crates/haetty-pty/src/win/mod.rs` |
| 55 | Do not rely on the in-box conhost for mouse modes, APC or kitty keys; the embedded OpenConsole 1.25 passes kitty flags. | `none` | HANDOFF §4 |
| 56 | Ship `conpty.dll` and `OpenConsole.exe` together (it silently falls back to the system conhost). | Code: `crates/haetty-pty/src/win/mod.rs` writes both files from the vendored bytes into one directory. No test (Windows-only). | HANDOFF §4 |
| 57 | libghostty-vt callbacks run inside `vt_write`: only queue there, never call back into the terminal. | `none` | HANDOFF §4 |
| 58 | OSC 9 / 777 / 99 and OSC 9;4 are not surfaced by libghostty-vt: `haetty-core/src/osc.rs` scans the stream. | Unit tests in `crates/haetty-core/src/osc.rs` (`osc9_notification_bel_and_st`, `split_across_chunks`, `progress`, `ignores_other_osc`, `kitty_99`). | HANDOFF §4 |
| 59 | OSC 10/11 get no reply unless default fg/bg are set on the terminal. | `none` (no test found) | HANDOFF §4 |
| 60 | Wheel: send button 4/5 presses only; a release reads as a second notch. | `none` (`input.rs` tests `wheel_lines_and_pixels` cover only the line count) | HANDOFF §4 |
| 61 | Claude Code caps colours at 256 when `TMUX` is set: remove `TMUX` / `TMUX_PANE` for children. | Test `reports_exit_and_strips_tmux_env` in `crates/haetty-core/tests/session.rs`; code in `Session::spawn`. | HANDOFF §4 |
| 62 | Claude Code's first byte takes seconds on a cold start: wait with `wait-text` or a large `wait-idle`. | `none` | HANDOFF §4 |
| 63 | A background grandchild can hold the PTY: `Exited` comes after a 500 ms grace without EOF. | `none` (`EXIT_GRACE` in `crates/haetty-core/src/engine.rs`; no test of its own found) | HANDOFF §4 |
| 64 | portable-pty's Unix writer blocks on drop: use haetty's own non-blocking reader and writer. | Test `closed_sessions_leave_no_threads` in `crates/haetty-gui/tests/threads.rs` (it fails when a closed session's writer blocks). | HANDOFF §4 |
| 65 | The libghostty-vt crate is pinned `=0.2.2`; upgrade deliberately. | `Cargo.toml` `libghostty-vt = "=0.2.2"` (a build with another version needs an edit of that line). | HANDOFF §4; `Cargo.toml` |

## HANDOFF §4, GUI (winit, fonts, rendering)

| # | Rule | Enforcement | Where |
|---|---|---|---|
| 66 | winit#4746 is not fixed in 0.30.13: keep the `app.rs::preedit_cursor` workaround (Windows); remove it when fixed. | `none` | HANDOFF §4, §6.0 |
| 67 | winit X11 double-counts XTest wheel clicks: do not halve them; run `click` without `--window`. | `none` | HANDOFF §4 |
| 68 | softbuffer's buffer is not a back buffer: keep our own canvas and check `buffer.age()`. | `none` | HANDOFF §4 |
| 69 | A snapshot with another grid size must force a full repaint. | Test `first_frame_and_resize_are_fully_dirty` in `crates/haetty-gui/src/damage.rs`. | HANDOFF §4 |
| 70 | X11 ignored `request_inner_size` right after creation: set `with_inner_size` up front. | `none` (code in `crates/haetty-gui/src/app.rs`; the e2e `scale.sh` is not part of the gate) | HANDOFF §4 |
| 71 | smithay-clipboard stays pinned `=0.7.2` (0.7.3 pulls a second sctk, +89 KB). | `crates/haetty-gui/Cargo.toml` pin `=0.7.2`. | HANDOFF §4 |
| 72 | fontdb without `memmap` has no shared files: the GUI maps font files itself (memmap2). | `none` | HANDOFF §4 |
| 73 | swash's `underline_offset` sign varies: use its absolute value below the baseline. | Code only (`crates/haetty-gui/src/render/paint.rs` uses `.abs()`); reference frames would show a large shift, no test names it. `none` | HANDOFF §4 |
| 74 | Cascadia Mono on Windows 11 is variable: fontdb sees only regular, bold and italic are synthesised. | `none` | HANDOFF §4 |
| 75 | WSL has no Noto Sans Symbols 2 (`⏸` missing); Windows uses Segoe UI Symbol. | `none` | HANDOFF §4 |
| 76 | libghostty-vt's `max_scrollback` is bytes, not lines. | `none` (the field is documented as bytes in `crates/haetty-core/src/engine.rs`; the type is a plain integer) | HANDOFF §4 |
| 77 | glibc keeps freed large buffers: map font files instead of reading them. | `none` (`scripts/e2e/measure.sh` measures memory budgets; it is not part of the gate) | HANDOFF §4 |
| 78 | Debian names it "Noto Sans Symbols2" (no space): list both names; check with `fc-scan`. | Code: both names are in the list in `crates/haetty-gui/src/font.rs`; no test. `none` | HANDOFF §4 |
| 79 | VS15/VS16 stay in the cell text; presentation must come from the text, not the width. | Partial: the `⚠` cell test in `crates/haetty-core/tests/session.rs` asserts `"\u{26a0}\u{fe0e}"` in a cell; other sequences: nothing. | HANDOFF §4 |
| 80 | Hinting can make a larger size 1 px narrower: pixel tests need 1 px tolerance. | `none` | HANDOFF §4 |

## HANDOFF §4, testing and Claude Code

| # | Rule | Enforcement | Where |
|---|---|---|---|
| 81 | Testing Claude Code inside a Claude Code session leaks `CLAUDE_CODE_*` markers: strip them (and `CLAUDECODE`). | Partial: same as #26 (`scripts/e2e/lib.sh`, check-range, `Session::spawn` with `CHILD_MARKERS`). | HANDOFF §4; `scripts/e2e/lib.sh`, `tools/check-range.sh` |
| 82 | With `CLAUDE_CONFIG_DIR`, Claude Code reads `$CLAUDE_CONFIG_DIR/.claude.json`: write it with a JSON library. | `none` | HANDOFF §4 |
| 83 | Fullscreen Claude Code: Ctrl+C with a selection copies; a test that drags must not expect Ctrl+C twice to exit. | `none` | HANDOFF §4 |
| 84 | Claude Code's double Ctrl+C window is 700 to 1500 ms; right after start a second press may be missed: wait a few seconds. | `none` | HANDOFF §4 |
| 85 | Two private Xvfb on the same display number share the abstract X socket across `unshare -rm`: use a free number. | `scripts/e2e/xvfb.sh` picks a display number nobody uses, also as an abstract socket. Only for scripts that go through it. | HANDOFF §4; `scripts/e2e/xvfb.sh` |
| 86 | check-range of commits before `0a23bf0` / K2 needs GNU `cat` and no session markers. | `tools/check-range.sh` puts GNU coreutils first on PATH and unsets the markers. | HANDOFF §4; `tools/check-range.sh` |
| 87 | A FAIL of `tests/ide.rs` in commits `8dd43d1` to `f17926a` is a flake (one run in sixteen): rerun such a FAIL. | `none` (those commits are not in `tools/known-gate-failures`) | HANDOFF §4 |
| 88 | Unix socket paths are limited to 108 bytes: keep `HAETTY_TEST_TMP` short. | The gate refuses a `HAETTY_TEST_TMP` longer than 32 characters (`GATE fail=1`). check-range: nothing. | HANDOFF §4; `scripts/gate.sh` |
| 89 | Never probe an older haetty with a new subcommand (it opens a window and reaches the running daemon); grep `--help`. | `none` | HANDOFF §4 |
| 90 | Do not ask a daemon whether it still runs by connecting (it stays busy, no idle exit): watch its PID. | `none` | HANDOFF §4 |
| 91 | A script typing blind into a palette breaks silently when it gains a step: check the result. | Partial: run-all's `run()` fails a script that ends without a FAIL line but with a non-zero exit or without a TOTAL line of passed checks (PK) | HANDOFF §4 |
| 92 | Ubuntu 26.04's uutils `cat` stops after the first read from a raw terminal: use python3 in fixtures. | `none` for new fixtures; check-range puts GNU `cat` first only for old commits (#86). | HANDOFF §4 |
| 93 | `echo "$out" \| grep -q` under `pipefail` fails when grep matches early: read a here-string. | Code: `scripts/gate.sh` uses here-strings; `tools/check-range.sh` sets no `pipefail`. A new script: nothing. | HANDOFF §4; `scripts/gate.sh` |

## HANDOFF §4, the dev machine

| # | Rule | Enforcement | Where |
|---|---|---|---|
| 94 | `cargo build --target <triple>` writes `target/<triple>/`: plain `target/release/haetty` may be stale. | `none` | HANDOFF §4 |
| 95 | Xvfb without sudo: an extracted root, an `unshare -rm` overlay for `xkbcomp` and a tmpfs X socket directory. | Code: `scripts/e2e/xvfb.sh` does it. Only for scripts that go through it. | HANDOFF §4; `scripts/e2e/README.md`, `scripts/e2e/xvfb.sh` |
| 96 | Build directories other than `target/` must not reach the Dropbox mirror; remove Dropbox-held files from Windows. | `tools/mirror-dropbox.sh` excludes `target/` and `target-*/`. Removal from Windows: nothing. | HANDOFF §4; `tools/mirror-dropbox.sh` |
| 97 | check-range builds into the usual target dir: give other builds their own `CARGO_TARGET_DIR` meanwhile (the same for `tools/stage/e.sh`). | `none` (heavy.sh limits how many jobs run, not which target directory they use) | HANDOFF §4 |
| 98 | Claude Code may stop idle background shells when memory runs low: keep long runs outside Claude Code. | `none` | HANDOFF §4 |
| 99 | Never `mv` over a running daemon's binary: start a new build beside it. | `none` (`install-stable.sh` writes a new version folder and renames the folder, not the binary) | HANDOFF §4; `tools/install-stable.sh` |
| 100 | With `blockReadsOutsideWorkingDirectories`, briefs, test output and frame dumps a worker must open belong inside its worktree. | `none` | HANDOFF §4 |
| 101 | In a tab of a daemon older than PI, call `$HAETTY_PROXY_DIR/haetty` (`haetty` and `claude` are not the proxy's when `.zshrc` puts `~/.local/bin` first). | `none` (fixed for new tabs by the shell integration from PI on) | HANDOFF §4 |
| 102 | `script -c CMD` runs `$SHELL -c CMD`: test shells as the tab's own program. | `none` | HANDOFF §4 |
| 103 | An e2e check of a non-zero `haetty …` exit line captures `2>&1`. | `none` | HANDOFF §4 |
| 104 | An e2e run from a session inside haetty inherits `HAETTY_PROXIED=1` and other tab markers: clear them (a script that does not source `lib.sh` clears them itself). | `scripts/e2e/lib.sh` unsets them; `ide.sh` and `select.sh` unset their own. A script that does neither: nothing. | HANDOFF §4; `scripts/e2e/lib.sh` |
| 105 | Claude Code 2.1.329's folder-trust question selects "No, exit" first: a script answers Down, Enter. | `none` | HANDOFF §4 |

## OPERATING

| # | Rule | Enforcement | Where |
|---|---|---|---|
| 106 | One stage per session. | `none` | OPERATING (top) |
| 107 | Start a session by reading HANDOFF (top block, in-progress line, "Next session"), CLAUDE.md and OPERATING, then only the needed part of DESIGN. | `none` | OPERATING "Start of a session" 1-2 |
| 108 | Check the state before changing anything (`git fetch`, the log, `git worktree list`, the logs the in-progress line names); finish or ask about a stage in progress. | `none` | OPERATING "Start of a session" 3 |
| 109 | On "next stage", do the first not-done stage in HANDOFF's order; a proposal to change the order goes into the report, not into the order. | `none` | OPERATING "Start of a session" 4 |
| 110 | Start a stage with `tools/stage/stage.sh start NAME "remaining work"` (through `heavy.sh`). | `stage.sh start` refuses when `haetty/` on main has changes other than HANDOFF and `.gitignore` ("main checkout not clean"), then gates and pushes the in-progress line. Using the script at all: nothing. | OPERATING "One stage" 1, "Limits"; `tools/stage/stage.sh` |
| 111 | Change files in the stage worktree only; keep changes small. | `none` | OPERATING "One stage" 2 |
| 112 | Check the changed crates on the worktree (fmt, clippy with warnings denied, the covering tests) before the commit. | `none` for the early check; the gate in `c.sh` runs all of it at the commit. | OPERATING "One stage" 3 |
| 113 | Commit with `tools/stage/c.sh MSGFILE` (gate, then commit); a commit message file lives outside the repository. | `c.sh` makes no commit unless `GATE fail=0` and keeps the output in `/tmp/haetty-test/gate-last.log`. The message file's location: nothing. | OPERATING "One stage" 4; `tools/stage/c.sh` |
| 114 | End a stage with `stage.sh finish`: fast-forward main, check-range, push, mirror, remove the worktree. | `finish` refuses a dirty worktree, merges `--ff-only`, then `push.sh` runs check-range and refuses to push on a FAIL; the mirror is skipped while the main checkout has uncommitted files. Using the script at all: nothing. | OPERATING "One stage" 5; `tools/stage/stage.sh`, `tools/stage/push.sh` |
| 115 | HANDOFF at the end: the in-progress line goes, "Next session" says what is done and next. | `none` (`finish` does not edit HANDOFF) | OPERATING "One stage" 6 |
| 116 | An issue is one stage of its own (`stage.sh start issue-N …`). | `none` | OPERATING "An issue" |
| 117 | An issue fix records the reproduction conditions and the environment of the check in the commit message. | `none` | OPERATING "An issue" 1 |
| 118 | An issue fix has a regression test where possible, checked to fail without the fix; where not possible the commit says why. | `none` | OPERATING "An issue" 2 |
| 119 | The issue commit message ends with `Fixes #N`. | `none` | OPERATING "An issue" 3 |
| 120 | One CHANGELOG line under the unreleased version, with `(#N)` and the commit. | `none` | OPERATING "An issue" 4 |
| 121 | What only Windows can show is marked "Windows unverified" and added to the Windows check list; it does not hold the stage. | `none` | OPERATING "An issue" 5; dev-commands "Windows check list" |
| 122 | Windows checks and fixes are done once, in stage PW; until then no stage waits for a Windows result or does Windows-only work (no `windows/*` merges, `checks/windows-*` handling, `#[cfg(windows)]` defect fixes, `.ps1` changes, Windows reference frames or test ports). | `none` (the gate's Windows build and clippy stay and still fail a stage) | OPERATING "Windows: one catch-up stage" |
| 123 | A new feature that needs Windows gets the least that builds (a fallback or "not supported on Windows yet") and one line in PW's list in HANDOFF. | Partial: the gate's Windows build and clippy catch a break; the fallback and the HANDOFF line: nothing. | OPERATING "Windows: one catch-up stage"; `scripts/gate.sh` |
| 124 | Nothing heavy runs while a registered night run is on. | Partial: `tools/overnight-soak-linux.sh` waits for and holds both heavy slots, and `tools/overnight-soak.ps1` checks for other heavy work and holds both slots, so other `heavy.sh` jobs wait. A command run outside `heavy.sh`: nothing. | OPERATING "Limits"; `tools/heavy.sh`, `tools/overnight-soak-linux.sh` |
| 125 | The stable haetty (instance, daemon, `~/.local/opt/haetty`) gets no command from a stage: tests run in dev instances and test run directories only. | Partial: as #16 and #25 (`dev-run.sh` refusals, test-guard). A typed command against stable: nothing (see #18). | OPERATING "Limits"; HANDOFF §3 |
| 126 | `stage.sh start` wants `haetty/` clean on main; files changed elsewhere in the repository are left alone (and hold the Dropbox mirror back). | `stage.sh start` refuses a dirty `haetty/`; `push.sh` skips the mirror with a line while the checkout has uncommitted files. Leaving other files alone: nothing. | OPERATING "Limits"; `tools/stage/stage.sh`, `tools/stage/push.sh` |
| 127 | Real Claude Code runs are few and short (quota); a test that needs one says so in its record. | `none` | OPERATING "Limits" |
| 128 | Build folders and the archived old copy are not read. | Deny rules `Read(./target/**)`, `Read(./target-*/**)`, `Read(~/archive/ccterm-2026-11-09/**)` in `haetty/.claude/settings.json` (the Read tool only; a `cat` or `grep` in Bash is not covered). | OPERATING "Limits"; `.claude/settings.json` |
| 129 | Run the one-hour soak before `finish` of a stage that changes the PTY layer, the daemon's session lifetime or the engine's memory structure (ownership or lifetime changes, not a fix in one function). | `none` (`tools/overnight-soak-linux.sh` runs it and `tools/test-soak-verdict.sh` tests the verdict rule; nothing requires the run) | OPERATING "The one-hour soak"; dev-commands "One-hour soak" |
| 130 | Also run the one-hour soak when a `health_warning` line shows up in a daemon's event log. | `none` | OPERATING "The one-hour soak" |
| 131 | The soak's verdict goes into the stage's history record; a FAIL holds the stage like a failing test. | `none` | OPERATING "The one-hour soak" |

## dev-commands

| # | Rule | Enforcement | Where |
|---|---|---|---|
| 132 | The Windows PC only pulls, runs checks and pushes the result as a document on `checks/windows-<date>`, never to `main` and never code; code changes are made on kramers. | `none` | dev-commands "Windows-only checks" |
| 133 | Stable installs are run by the user: `tools/install-stable.sh REF` / `.ps1`, rollback with `--activate`. | Partial: see #17 (read-only install folders). Who runs it: nothing. | dev-commands "Commands"; `tools/install-stable.sh` |
| 134 | The Windows night run is started from a PowerShell window, not from Claude Code, with nothing else heavy running. | Partial: `tools/overnight-soak.ps1` checks that no other heavy job runs and reports NOT-RUN otherwise. Not from Claude Code: nothing (no session check in the script). | dev-commands "Windows night run"; `tools/overnight-soak.ps1` |
| 135 | Keep the charger in during the one-hour soak on battery a closed lid suspends and the verdict is FAIL or missing. | `none` | dev-commands "One-hour soak" |

## Added by stage PK (2026-11-27)

| # | Rule | Enforcement | Where |
|---|---|---|---|
| 136 | Tests leave the stable instance's processes as they were. | run-all's last step compares the stable processes' PIDs and start times since boot before and after (`scripts/e2e/leftovers.sh`) | CLAUDE.md Working rules; PQ common rules |
| 137 | The scripts the person runs by path are executable. | the gate: `tools/check-exec.sh` (every `tools/*.sh` README.md and SELF-HOSTING.md name, mode 100755 in git) | STAGES PK |

## Rules without an enforcement

Rows whose enforcement is `none` (a person or a session has to remember them; a note in the row says what covers a part, if anything): 86 of 137.

#4, #5, #6, #7, #8, #10, #12, #18, #21, #23, #24, #29, #30, #31, #32, #34, #35, #36, #37, #38, #41, #42, #43, #44, #45, #46, #47, #48, #49, #50, #51, #52, #53, #54, #55, #57, #59, #60, #62, #63, #66, #67, #68, #70, #72, #74, #75, #76, #77, #80, #82, #83, #84, #87, #89, #90, #92, #94, #97, #98, #99, #100, #101, #102, #103, #105, #106, #107, #108, #109, #111, #112, #115, #116, #117, #118, #119, #120, #121, #122, #127, #129, #130, #131, #132, #135
