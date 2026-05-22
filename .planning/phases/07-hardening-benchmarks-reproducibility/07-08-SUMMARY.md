---
phase: 07-hardening-benchmarks-reproducibility
plan: 08
subsystem: infra
tags: [bench, hyperfine, dhat, samply, miner-bench, recipe-runner, profiling, perf]

# Dependency graph
requires:
  - phase: 07-hardening-benchmarks-reproducibility
    provides: "Plan 07-02 fixture cache regenerator (gen-fixtures binary); Plan 07-03 cargo-audit/deny CI gates; Plan 07-06 criterion + dhat workspace deps and [profile.release] debug = 1; Plan 07-07 README ## Data source caveats anchor; Plan 07-01 CONTRIBUTING ## Regenerating goldens anchor"
provides:
  - "miner-bench recipe-runner binary (replaces the Phase 1 placeholder) — reads TOML SweepManifest, drives miner_core::sweep::run_sweep, emits one JSON timing line on stdout"
  - "dhat heap-profiler integration behind a miner-bench-only Cargo feature gate (`--features dhat`) — FOUND-04 preserved (miner-core stays dhat-free + tokio-free)"
  - "benches/recipes/full-sweep.toml — 28 × 3 × 6 × 3-scan-family production-scale sweep recipe (hyperfine target)"
  - "benches/recipes/single-job.toml — single-instrument single-window single-scan recipe (dhat target; fits the fixture cache)"
  - "scripts/run-bench.sh — hyperfine 1.20+ wrapper exporting /tmp/miner-bench.json"
  - "scripts/run-alloc-profile.sh — dhat wrapper writing dhat-heap.json"
  - "docs/bench-results.md — single canonical home for perf numbers per D7-07 (six required sections; TBD placeholders to be populated by future perf-capture PRs)"
  - "README.md ## Performance one-line pointer to docs/bench-results.md"
  - "CONTRIBUTING.md ## Profiling subsection documenting the samply + dhat + hyperfine recipes"
affects: [phase-08, perf-followup, ci-cleanup]

# Tech tracking
tech-stack:
  added:
    - "dhat 0.3.3 (heap profiler; feature-gated to miner-bench, off by default)"
    - "clap 4.5 (workspace dep already present; first consumer in miner-bench)"
    - "toml 0.8 (workspace dep already present; first consumer in miner-bench)"
    - "ctrlc 3.5 (workspace dep already present; first consumer in miner-bench)"
  patterns:
    - "miner-bench recipe-runner pattern: clap-derive Args { recipe, warmup, runs } -> read TOML -> parse_manifest_str -> MinerConfig::resolve(None, default) (env-driven cfg) -> DukascopyReader::new + BarCache::new + CountingSink -> run_sweep -> serde_json::to_writer(stdout) one-line summary"
    - "dhat global allocator behind `#[cfg(feature = \"dhat\")]` — Profiler bound to a `_profiler` local in main() so the drop on Ok-exit triggers the dhat-heap.json write"
    - "Counting FindingSink — in-process tallies Finding::Result + Finding::ScanError counts, discards bytes, used by the bench runner to avoid polluting stdout with the JSONL stream (stdout is reserved for the timing summary line)"
    - "TOML recipe convention: plain SweepManifest TOML, NO bench-wrapper type (RESEARCH Open Question 3) — bench knobs come from CLI args on the binary"
    - "Shell-wrapper convention for scripts/run-*.sh: SPDX header lines 1-2 + `set -euo pipefail` + `REPO_ROOT=$(git rev-parse --show-toplevel) && cd $REPO_ROOT` + tool-availability check + env-var default block + the wrapped invocation"

key-files:
  created:
    - "benches/recipes/full-sweep.toml"
    - "benches/recipes/single-job.toml"
    - "scripts/run-bench.sh"
    - "scripts/run-alloc-profile.sh"
    - "docs/bench-results.md"
    - "docs/bench-results/.gitkeep"
  modified:
    - "crates/miner-bench/Cargo.toml (add clap + toml + ctrlc + dhat optional dep + `dhat = [\"dep:dhat\"]` feature)"
    - "crates/miner-bench/src/main.rs (replace 14-line placeholder with 266-line recipe runner)"
    - "Cargo.lock (dhat 0.3.3 + addr2line/backtrace/gimli/object/etc. transitive deps locked)"
    - ".gitignore (ignore dhat-heap.json — per-run profiling output)"
    - "README.md (add `## Performance` H2 between Data source caveats and Design principles)"
    - "CONTRIBUTING.md (add `## Profiling` H2 between Regenerating goldens and Pull request expectations)"

key-decisions:
  - "dhat feature on miner-bench ONLY (not miner-core) preserves FOUND-04 — async-runtime AND heap-profiler boundaries both live at the wrapper edge, not in the scan engine"
  - "Recipes are plain SweepManifest TOML, not a bench-wrapper type; bench knobs (`--warmup` / `--runs`) come from CLI args on the miner-bench binary so hyperfine sees a `miner-bench --help` surface that's self-documenting"
  - "CountingSink (in-process) over the JSONL stream — bench mode prioritises stdout discipline (one JSON timing line) over per-finding output, which would otherwise dominate the wall-clock measurement with serde + IO overhead"
  - "Single-job recipe targets seas.bucket.hour_of_day@1 on EURUSD:bid for January 2024 — exactly fits the gen-fixtures.rs fixture cache (one symbol-side, one month). The reference flamegraph captures cross.cointegration.engle_granger@1 per RESEARCH Open Question 5 (hottest scan family)"
  - "How-to-reproduce content lives in docs/bench-results.md ## How to reproduce, not CONTRIBUTING.md (per RESEARCH Open Question 4)"
  - "samply (not cargo-flamegraph) is the recommended profiler in CONTRIBUTING.md ## Profiling — modern, simple two-step (cargo build --release + samply record), Firefox profiler UI"

patterns-established:
  - "Pattern: bench-runner CLI binary (clap-derive + tracing-to-stderr + serde_json-to-stdout + ctrlc-before-anything + env-driven MinerConfig — mirrors miner-cli's main() structure but consumes a SweepManifest TOML instead of CLI scan args)"
  - "Pattern: feature-gated global allocator — `#[cfg(feature = \"dhat\")] #[global_allocator] static ALLOC: dhat::Alloc = dhat::Alloc;` + `let _profiler = dhat::Profiler::new_heap();` inside main() guarded by the same cfg"
  - "Pattern: bench wrapper script — REPO_ROOT resolution + tool-availability gate + env-var defaults (MINER_BAR_CACHE_ROOT, MINER_OUTPUT) + explicit error when MINER_CACHE_ROOT is unset (no safe default for a production-shape Dukascopy cache)"
  - "Pattern: docs/bench-results.md TBD-populated tables — Reference workstation, Wall-clock results, Allocation budget tables ship with TBD cells. New perf-capture PRs commit the refresh as a single `chore(07): refresh bench numbers as of <sha>` so the bench evidence and the code under test are clearly separated"

requirements-completed: [FOUND-04]

# Metrics
duration: 16min
completed: 2026-05-22
---

# Phase 07 Plan 08: D7-03 Layers 2 + 3 + samply/dhat Profiling — recipe-runner + bench harness Summary

**Replace miner-bench placeholder with the production recipe-runner binary; wire dhat-rs behind a miner-bench-only `--features dhat` Cargo gate; ship hyperfine + dhat wrapper scripts + canonical `docs/bench-results.md`.**

## Performance

- **Duration:** ~16 minutes
- **Started:** 2026-05-22T10:42:20Z
- **Completed:** 2026-05-22T10:58:35Z
- **Tasks:** 3 / 3 (atomic auto)
- **Files created:** 6 (1 Rust binary replacement — but `src/main.rs` was a 14-line placeholder so it counts as a from-scratch authoring of the recipe runner — 2 TOML recipes, 2 shell scripts, 1 markdown doc, 1 `.gitkeep`)
- **Files modified:** 5 (Cargo.toml, Cargo.lock, .gitignore, README.md, CONTRIBUTING.md)

## Accomplishments

- **miner-bench is now the recipe runner.** Reads `--recipe <toml>`, drives `miner_core::sweep::run_sweep` in-process, emits one JSON timing line on stdout (per Pattern I stdout discipline). Tracing logs go to stderr. SIGINT installs a cooperative cancel flag before the sweep call (Pitfall 2 / D3-22).
- **dhat heap profiler is wired behind `--features dhat`.** `dhat::Alloc` becomes the global allocator only when the feature is enabled; default release builds (cargo install, CI, distribution) use the system allocator. FOUND-04 invariant verified by `cargo tree -p miner-core --edges normal,build` — no async or dhat deps leak into miner-core.
- **Bench recipes ship as plain SweepManifest TOML.** `full-sweep.toml` is the production-scale 28 × 3 × 6 × 3-scan-family target; `single-job.toml` is the smallest sane workload sized for the fixture cache. Both are valid SweepManifest TOML; the single-job recipe was verified end-to-end against the regenerated fixture cache (5 Finding::Result envelopes, 0 errors, ~55ms wall-clock).
- **Wrapper scripts ship and were verified end-to-end.** `scripts/run-bench.sh` wraps hyperfine 1.20+ with `--warmup 3 --runs 5 --export-json /tmp/miner-bench.json`. `scripts/run-alloc-profile.sh` builds with `--features dhat` and runs the single-job recipe — verified to produce a 621 KB `dhat-heap.json` in the repo root with the standard dhat top-level keys (`ftbl`, `pps`, `tg`, `tu`, etc.).
- **docs/bench-results.md is the canonical perf-numbers home.** Six required sections per RESEARCH Open Question 4 + D7-07; TBD-populated tables; Apache-2.0 footer byte-identical to `docs/.license-footer.md`. `docs/bench-results/` directory created with `.gitkeep` so the future flamegraph PNG path is in place.
- **README and CONTRIBUTING gain the right discovery surfaces.** README's `## Performance` H2 (one-line pointer to bench-results.md) lands between `## Data source caveats` (Plan 07-07) and `## Design principles`. CONTRIBUTING's `## Profiling` H2 (samply recipe + dhat / hyperfine wrapper links) lands between `## Regenerating goldens` (Plan 07-01) and `## Pull request expectations`. All previous-plan content is preserved verbatim.

## Task Commits

Each task was committed atomically:

1. **Task 1: Replace miner-bench Cargo.toml + main.rs with the recipe-runner; add dhat feature** — `bc52227` (feat)
2. **Task 2: Author the two TOML recipes + the two shell wrapper scripts** — `b331b01` (feat)
3. **Task 3: Author docs/bench-results.md + extend README ## Performance pointer + extend CONTRIBUTING.md ## Profiling subsection** — `248d296` (docs)

## Files Created/Modified

- `crates/miner-bench/Cargo.toml` — add clap + toml + ctrlc + optional dhat dep + `dhat = ["dep:dhat"]` feature; both `[[bin]]` declarations (`miner-bench` + `gen-fixtures`) coexist.
- `crates/miner-bench/src/main.rs` — replace the Phase 1 14-line placeholder with the 266-line recipe runner (clap-derive Args, env-driven MinerConfig, DukascopyReader + BarCache construction, ctrlc handler, CountingSink, `run_sweep`, JSON timing summary on stdout).
- `Cargo.lock` — dhat 0.3.3 + addr2line / backtrace / gimli / miniz_oxide / mintex / object / parking_lot_core / rustc-demangle / rustc-hash / thousands transitive deps locked.
- `.gitignore` — add `dhat-heap.json` (per-run profiling output emitted by the dhat global allocator's destructor).
- `benches/recipes/full-sweep.toml` — 28 × 3 × 6 × 3-job sweep recipe.
- `benches/recipes/single-job.toml` — single-instrument × single-window × single-scan recipe (dhat target).
- `scripts/run-bench.sh` — hyperfine wrapper.
- `scripts/run-alloc-profile.sh` — dhat wrapper.
- `docs/bench-results.md` — canonical perf-numbers doc with six required sections + Apache-2.0 footer.
- `docs/bench-results/.gitkeep` — placeholder for future flamegraph PNGs.
- `README.md` — add `## Performance` H2 (one-line pointer to docs/bench-results.md).
- `CONTRIBUTING.md` — add `## Profiling` H2 with samply recipe + dhat / hyperfine wrapper links.

## Decisions Made

- **dhat feature on miner-bench ONLY** (not miner-core, not miner-cli). The heap profiler is a wrapper-edge concern, structurally identical to the tokio-at-the-edge convention FOUND-04 already enforces. CI's standard `cargo test --workspace` path never sees dhat; only `bash scripts/run-alloc-profile.sh` (or an explicit `cargo run --release --features dhat -p miner-bench`) does.
- **Recipes are plain SweepManifest TOML.** No `BenchRecipe` wrapper struct; the bench knobs (`--warmup` / `--runs`) live on the miner-bench CLI surface so `miner-bench --help` is the canonical reproduction documentation and hyperfine's own `--warmup N --runs N` flags do the actual repetition externally (per RESEARCH Open Question 3).
- **CountingSink over a JSONL-emitting sink.** The recipe runner needs ONE JSON timing line on stdout — feeding the per-finding JSONL stream there would put hundreds of envelopes between hyperfine's stdout reader and the timing summary. CountingSink tallies `Finding::Result` + `Finding::ScanError` counts in memory and discards the bytes; the summary line at the end carries `total_findings` + `scan_errors` so the hyperfine consumer can spot-check the job did real work.
- **Single-job recipe target is `seas.bucket.hour_of_day@1`** — exactly the smallest scan kernel that fits the fixture cache (24-bucket stats, log-returns vec, bucket-keys vec). The reference flamegraph documented in `docs/bench-results.md ## Reference flamegraph` targets `cross.cointegration.engle_granger@1` instead, per RESEARCH Open Question 5 (the hottest scan family — full ADF + OLS + half-life inner loop) — that's the right thing to flamegraph, but it doesn't fit cleanly into the fixture cache's two-symbol single-month shape, so the `--recipe single-job.toml` invocation is the smoke-test target and the samply recipe is a hand-driven capture against a different recipe.
- **How-to-reproduce lives in docs/bench-results.md** (not CONTRIBUTING.md), per RESEARCH Open Question 4. CONTRIBUTING.md's `## Profiling` subsection is the discovery surface (where to find the tools); docs/bench-results.md `## How to reproduce` is the canonical reproduction recipe (where to run them and what to commit afterwards).
- **`samply` over `cargo-flamegraph`** in CONTRIBUTING.md `## Profiling`. samply is the modern, simpler tool (two-step recipe; output renders directly in the Firefox profiler UI) and matches RESEARCH §"Pattern 8" + Stack guidance.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Add `dhat-heap.json` to .gitignore**

- **Found during:** Task 2 (end-to-end verification of `scripts/run-alloc-profile.sh`).
- **Issue:** Running the alloc-profile script writes `dhat-heap.json` (621 KB) to the repo root via the dhat global allocator's destructor. The repo's `.gitignore` had no entry for it — every contributor who runs the script would see `dhat-heap.json` as an untracked file at the top of `git status`, and the file is per-run profiling output (not source).
- **Fix:** Added `dhat-heap.json` line to `.gitignore` with a comment naming Plan 07-08 and the script that produces it.
- **Files modified:** `.gitignore`.
- **Verification:** `git check-ignore dhat-heap.json` → exits 0, prints `dhat-heap.json`.
- **Committed in:** `b331b01` (Task 2 commit — kept in-scope because the script is the cause and the ignore lives alongside it).

### Documented exceptions (not auto-fixed; out of scope per the boundary rule)

**2. `cargo clippy -p miner-bench --all-targets --all-features -- -D warnings` does NOT exit 0 — pre-existing breakage from Plan 07-02.**

- **Found during:** Task 1 verification.
- **Symptom:** 4 pedantic-tier clippy errors in `crates/miner-bench/src/bin/gen-fixtures.rs` (2× `doc_markdown` on line 8, 1× `cast_precision_loss` on line 98, 1× `format_collect` on lines 193-196). Same errors are present with and without `--all-features` (the dhat feature doesn't affect gen-fixtures.rs); they predate this plan on main HEAD (verified by stashing 07-08's changes — errors persist).
- **Why not auto-fixed:** Per the GSD scope-boundary rule ("Only auto-fix issues DIRECTLY caused by the current task's changes"), these belong to Plan 07-02 / a future cleanup plan, not 07-08. The breakage was already logged under item 3 of `.planning/phases/07-hardening-benchmarks-reproducibility/deferred-items.md` by Plan 07-06; I added item 5 there confirming the breakage is still present at Plan 07-08 time and that 07-08's own `src/main.rs` is clean.
- **Mitigating verification:** The 07-08-scoped clippy invocations DO pass cleanly:
  - `cargo clippy -p miner-bench --bin miner-bench -- -D warnings` → 0
  - `cargo clippy -p miner-bench --bin miner-bench --features dhat -- -D warnings` → 0
  Both build the new `src/main.rs` (without gen-fixtures.rs as a sibling target). The `--bin miner-bench` invocation is the right gate for 07-08's deliverable; the `--all-targets` superset is the right gate for the follow-up cleanup plan.

**3. Two grep-based acceptance criteria in the plan are technically off by one (planning oversight).**

- `grep -c '^name = "miner-bench"$' crates/miner-bench/Cargo.toml` returns 2, not 1 (matches both `[package] name = "miner-bench"` AND `[[bin]] name = "miner-bench"`; the `[package]` match is structurally required).
- `grep -c -- "--features dhat" scripts/run-alloc-profile.sh` returns 2, not 1 (matches the inline doc comment in the script header AND the actual `cargo run --release --features dhat` invocation).
- The SPIRIT of each criterion is met: the `[[bin]]` entry exists and the cargo invocation uses `--features dhat`. The planner's intent was to check for presence, not exclusive presence. Documenting here so the verifier doesn't flag this as a regression.

---

**Total deviations:** 1 auto-fixed (Rule 3 — blocking), 2 documented exceptions.
**Impact on plan:** No scope creep. The .gitignore fix is a one-line correctness improvement; the documented exceptions are planning-grep precision issues that don't affect correctness.

## Issues Encountered

- **The fixture cache wasn't actually committed by Plan 07-02.** Only `tests/fixtures/cache/.gitkeep` is tracked; the `EURUSD/` + `GBPUSD/` synthetic-stub trees and the `SHA256SUMS` file are NOT in git. The `scripts/run-alloc-profile.sh` script handles this gracefully (regenerates via `scripts/generate-fixture-cache.sh` when the sentinel file `tests/fixtures/cache/EURUSD/2024/00/01_bid.csv.zst` is absent), so Plan 07-08's end-to-end verification works on a fresh clone. But the Plan 07-02 SUMMARY claimed the cache bytes ship as tracked artifacts; reality on main HEAD diverges from that claim. Documented in this SUMMARY for the verifier; out of scope for this plan to fix (would be a Plan 07-02 follow-up).

## User Setup Required

None — the bench harness installs cleanly via `cargo install hyperfine@1.20.0 samply@0.13.1` (documented in `CONTRIBUTING.md ## Profiling` and `docs/bench-results.md ## How to reproduce`). The first time a contributor runs `bash scripts/run-alloc-profile.sh`, the script auto-regenerates the fixture cache. No external services, no environment-variable wiring beyond the documented `MINER_CACHE_ROOT` / `MINER_BAR_CACHE_ROOT` / `MINER_OUTPUT` triple.

## Self-Check: PASSED

Verified post-write:
- `crates/miner-bench/Cargo.toml` — FOUND (Task 1).
- `crates/miner-bench/src/main.rs` — FOUND, 266 lines, recipe runner present.
- `benches/recipes/full-sweep.toml` — FOUND (Task 2).
- `benches/recipes/single-job.toml` — FOUND.
- `scripts/run-bench.sh` — FOUND, executable.
- `scripts/run-alloc-profile.sh` — FOUND, executable.
- `docs/bench-results.md` — FOUND with all six required sections + Apache-2.0 footer byte-identical to docs/.license-footer.md.
- `docs/bench-results/.gitkeep` — FOUND.
- Task 1 commit `bc52227` — FOUND in `git log`.
- Task 2 commit `b331b01` — FOUND.
- Task 3 commit `248d296` — FOUND.

## Next Phase Readiness

- Phase 7 is closed by this plan. The bench harness is complete: criterion microbenches (Plan 07-06), recipe runner + hyperfine wrapper (Plan 07-08), dhat allocation profiler (Plan 07-08), samply flamegraph recipe (Plan 07-08), and the canonical perf-numbers doc (`docs/bench-results.md`).
- D7-03 (Layers 1, 2, 3) and D7-07 (perf-numbers location) are both fully closed.
- Follow-up work: A `chore(07): refresh bench numbers as of <sha>` PR should be the first thing a maintainer runs after Phase 7 merges — it populates the TBD cells in `docs/bench-results.md` with real captured numbers from a reference workstation.
- Out-of-band follow-up (separate plan): the workspace-wide `cargo clippy --workspace --all-targets -- -D warnings` gate is broken on main HEAD because of pre-existing pedantic lints in `crates/miner-bench/src/bin/gen-fixtures.rs` (Plan 07-02) and `crates/miner-core/tests/{noise_replay_regression.rs,findings_envelope_snapshot.rs}` (older work). All four items are logged in `.planning/phases/07-hardening-benchmarks-reproducibility/deferred-items.md`. None of these block Phase 7 closure; they block a future tightening of CI.

---
*Phase: 07-hardening-benchmarks-reproducibility*
*Completed: 2026-05-22*
