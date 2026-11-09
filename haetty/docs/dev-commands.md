# Commands, environment and tools (moved from CLAUDE.md)

Moved verbatim from `CLAUDE.md` in stage G0; `CLAUDE.md` keeps the rules and points here.

## Environment

- Rust 1.90+, Zig **0.15.2** on `PATH` (libghostty-vt-sys builds Ghostty's
  libghostty-vt with it; first build takes several minutes).
- Optional: `GHOSTTY_SOURCE_DIR` = a Ghostty checkout at commit
  `a887df42c56f6de86c0fe6da9c4eeca37931e083` to skip the clone.
- Windows cross-build: `rustup target add x86_64-pc-windows-gnu` and
  mingw-w64 (`x86_64-w64-mingw32-gcc`).

## Commands

```sh
cargo build --release
cargo test                                   # core 8+12 (Linux, python3); gui 92 (+1 ignored measurement) + completions, daemoncrash, instances, migrate, observe, orchestrator, permission, threads 1 each; proto 5; cc 17; daemon 1+5 (Unix); testkit 1
cargo clippy --all-targets                   # must stay warning-free
cargo clippy --all-targets --target x86_64-pc-windows-gnu
cargo build --release --target x86_64-pc-windows-gnu
cargo build --profile dist --target <triple>   # size-optimised, stripped
cargo fmt                                    # rustfmt.toml: max_width 120
cargo run --release -p haetty-core --example mem -- 3 67108864 10000   # anon memory per session (Linux)
```

`scripts/gate.sh` is the full gate (fmt, both targets, clippy on both, all
tests, sizes; ends with `GATE fail=0`); `tools/check-range.sh RANGE` runs the
quick gate on every commit of a range (see Commits and reference frames).
Heavy jobs — builds, the gate, check-range, end-to-end and soak runs,
fuzzing, installer self-tests, WSL builds for Windows — run at most **two at
a time**, Linux and Windows together (this machine's memory). The scripts
above take a slot themselves (`tools/heavy.sh`; Windows `tools/heavy.ps1`);
run any other heavy command through `tools/heavy.sh CMD` /
`pwsh -File tools\heavy.ps1 -Label TEXT -- CMD`, and never around it.
`tools/heavy.sh --status` shows who holds the slots.
The native engine (DESIGN.md §10, not the default): `crates/haetty-vt`
(pure Rust), wrapped as `haetty_core::native`, chosen per session with
`engine = native` (config, `--engine`). libghostty-vt is the default
feature `ghostty`; `--no-default-features` builds without it and without
Zig (the gate checks this). `cargo run -p haetty-vtdiff --release --
--corpus ~/m4/vtcorpus --report FILE` compares the engines cell by cell
(dev only); `--encoders` compares their key/mouse/paste/focus bytes,
`--bisect NAME` finds the first differing byte and a minimal input,
`--probe 'BYTES'` shows both engines' state, `--refresh` measures the
default engine's render-state refresh (DESIGN.md §10.8). A differing run is
KNOWN only if an entry of `docs/vtdiff/known-vt-differences.txt` explains it for the reference it names
(`refs`: `ghostty-main` or `ghostty-0.2.2`; pattern in the input, fields covered, identical with the entry's
`haetty_vt::Compat` switch on); everything else is FAIL. The newest Ghostty (`ghostty-main`, the default
reference) is the target. **Adding an entry is delegated to the agent** when both hold: (a) `ghostty-main`
behaves as haetty-vt does on the inputs, so the difference lies in the other reference; and (b) an ECMA-48
clause or an xterm behaviour gives the reason, cited in `basis`. Such an entry goes straight into the list with
`refs`, `basis` and `approved = <date>, delegated: (a) and (b)`, and is listed in the stage's report. If either
condition fails, or the sources disagree, the proposal goes to `docs/vtdiff/known-candidates.txt` (counted FAIL,
shown "KNOWN if approved") and the person decides. **Changing or removing an entry needs the person's
approval** (record the date and their words in `approved`). The reasoning for each entry is in
`docs/vtdiff/known.md`. Generated tables:
`--gen-width`, `--gen-keys`, `tools/gen-grapheme-table.py`,
`cargo run --release -p haetty-vt --example feed` (the engine's speed alone),
`tools/gen-x11-colors.py`.
`scripts/zig-mode.sh` shows libghostty-vt's Zig mode per profile (all
optimized); `scripts/e2e/measure.sh` measures the §3.4 budgets.
End-to-end checks of the real GUI (Linux, Xvfb): `scripts/e2e/run-all.sh`,
`E2E_ENGINE=native scripts/e2e/run-all.sh` for every session on haetty's engine
(`scripts/e2e/README.md`: what each script checks, Xvfb without root, the
VT fuzz corpus); Windows: `scripts/e2e/windows/`.

Stable installs (the user runs them; `docs/DESIGN.md` §3.15, README):
`tools/install-stable.sh REF` / `tools/install-stable.ps1 REF`, rollback
with `--activate` / `-Activate`; self-tests `tools/test-install-stable.sh
REF1 REF2` / `.ps1` (test run directory only). `haetty --version` names the
commit, build date and instance.

`haetty-headless` drives any program without a window (see README), e.g.
`haetty-headless --trace -e "wait-idle 3000 60000" -e modes -- claude`.
Use it to check behaviour against a real Claude Code before and after a change.
`HAETTY_DUMP_FRAME=f.ppm haetty ...` writes every GUI frame to a file, which
is how rendering can be checked without a screen capture.

## Test isolation (full rule)

- Test isolation (enforced in code): tests write and remove files only in
  their run directory — Rust tests through `haetty_testkit::run_dir` /
  `check` / `remove_dir_all` (a dev-dependency); shell scripts source
  `scripts/test-guard.sh` and call `haetty_guard_init HAETTY_TEST_TMP` and
  `haetty_guard PATH`; PowerShell dot-sources `scripts/test-guard.ps1`
  (`Initialize-HaettyGuard`, `Assert-HaettyPath`). An empty base variable or
  a path in the real HOME, `~/.config`, `~/.local`, `~/.claude`, XDG dirs,
  `%APPDATA%`, `%LOCALAPPDATA%` (and, in WSL, the Windows profile) is refused
  before anything is written. Bases: Linux `/tmp/…`; Windows
  `%PUBLIC%\haetty-test` (the Windows temp folder is under `%LOCALAPPDATA%`).
  Claude Code in tests: an isolated home in the run directory and a dummy
  key only.

## Commits and reference frames (full rules)

- Commits and reference frames:
  - Every commit passes the full gate on its own (both targets, clippy 0
    on both, all tests including the reference frames) — no commit relies
    on a later one to become green.
  - Before every push, check each new commit on its own:
    `HAETTY_TEST_TMP=/tmp/haetty-test tools/check-range.sh origin/main..HEAD`
    (Windows: `tools/check-range.ps1 origin/main..HEAD`) must end with
    "0 failing". It builds each commit that changes `haetty/` in a
    temporary worktree and runs fmt, clippy on both targets (sources
    touched, one run per target) and `cargo test`. A failing commit is
    fixed before the push (the branch is not pushed yet, so it may be
    rewritten); pushed history is never rewritten — a commit found failing
    later is recorded in HANDOFF.md instead.
  - `tools/known-gate-failures` lists the pushed commits known to fail on
    their own (stage T1); check-range reports them as KNOWN — only for the
    steps listed — and everything else as FAIL. **Adding, changing or
    removing an entry needs the user's approval for that entry**; never add
    a commit to make a range pass. Tag conditions use check-range "0
    failing" (KNOWN does not count; FAIL does).
  - An approved change of the outer design (DESIGN.md §2) puts the code and
    the updated reference frames into **one** commit whose message says
    "DESIGN §2 change, approved".
  - Only a reference-frame update without an intended change of the look
    (e.g. new fonts on the reference machine, a rendering fix that moves
    pixels) is a commit of its own, whose message says why the frames
    changed.
  - Update with `HAETTY_UPDATE_REFERENCE=1 cargo test reftest` (Linux) and
    the Windows test binary with `HAETTY_REFERENCE_DIR` (Windows); a rerun
    must report every frame identical on both.
