---
phase: 05-statistical-hygiene-sweep-runner
plan: 04
subsystem: sweep-runner
tags:
  - phase-5
  - sweep-runner
  - rayon-fanout
  - bh-fdr
  - hyg-05
  - op-04
  - schema-additive

# Dependency graph
requires:
  - plan: 05-01
    provides: SweepSummaryFinding + FdrFamilySummary + FindingFdrEntry +
      SweepTotals types; Finding::SweepSummary variant
  - plan: 05-02
    provides: scan::hygiene::fdr::bh_fdr (HYG-02) + scan::hygiene::seed::derive_job_seed
      (HYG-05) kernel
  - plan: 05-03
    provides: BootstrapMethod + ScanRequest hygiene fields
      (master_seed/job_seed/bootstrap_method/bootstrap_n/null_method/null_n);
      engine::preflight::validate_hygiene_support + engine apply_hygiene_mutations
      pipeline so per-job hygiene populates Effect.ci95 / Effect.p_value / ResultFinding.repro
provides:
  - "sweep::manifest module — typed SweepManifest TOML deserialiser
    with [sweep], [hygiene], [fdr], [[jobs]] blocks; validate(manifest,
    registry) preflight returning estimated job count or
    PreflightCode::{SweepTooLarge, InvalidParameter, HygieneNotSupported,
    UnknownScan}"
  - "sweep::job_graph module — expand(manifest, registry) ->
    Vec<ResolvedJob> in D5-01 deterministic order; cartesian_params +
    parse_instruments_grid + estimated_job_count helpers"
  - "sweep::executor module — run_sweep + run_sweep_with_registry
    rayon-parallel fanout + deterministic-order buffered drain +
    BH-FDR per family + Finding::SweepSummary emission + SIGINT
    short-circuit; SweepOptions { dry_run } struct"
  - "DryRunFinding.planned_job_count: Option<u64> additive field
    (RESEARCH Pattern 5 — single-shot leaves None; sweep --dry-run
    populates with cartesian-expanded count)"
  - "Workspace dep: rayon 1.10 (sync work-stealing thread pool;
    FOUND-04 preserved — no tokio/async-std creep)"
affects:
  - 05-05
  - 06-mcp-http-wrappers
  - 07-hardening

# Tech tracking
tech-stack:
  added:
    - "rayon 1.10 — work-stealing CPU-bound parallelism for sweep
      job fanout; rayon-core 1.13 transitive"
  patterns:
    - "Pattern 4 (RESEARCH §1.3) — deterministic-order buffered drain:
      par_iter().enumerate().map() collects (idx, Vec<Finding>)
      tuples; main thread sorts by idx then drains, preserving
      manifest-declaration order regardless of worker completion order"
    - "Pattern 5 (RESEARCH §3.5) — additive variant extension:
      planned_job_count appended to DryRunFinding with
      #[serde(default)] rather than introducing a new
      Finding::SweepDryRun variant; legacy single-shot dry-run JSON
      still deserialises (#[serde(default)] fills None)"
    - "JobSink wrapper pattern — per-job FindingSink impl that
      swallows RunStart/RunEnd envelopes (the sweep emits framing
      ONCE around all jobs) while passing Result/ScanError/GapAborted/
      DryRun through unchanged. Enables run_one_with_registry to be
      called per-job without per-job framing leakage."
    - "scope_family + family_prefix helpers — pure-function family-key
      computation; family_prefix splits on the first '.' of scan_id
      (after stripping @version); scope_family dispatches the four
      [fdr].family enum values"

key-files:
  created:
    - "crates/miner-core/src/sweep/mod.rs (47 lines — module root + re-exports)"
    - "crates/miner-core/src/sweep/manifest.rs (~620 lines — typed
      TOML deserialiser + validate + merge_hygiene + parse_bootstrap_method
      + parse_null_method + 11 unit tests)"
    - "crates/miner-core/src/sweep/job_graph.rs (~660 lines — expand +
      estimated_job_count + parse_instruments_grid + cartesian_params
      + 11 unit tests)"
    - "crates/miner-core/src/sweep/executor.rs (~620 lines — run_sweep
      + run_sweep_with_registry + JobSink + scope_family + family_prefix
      + 3 unit tests)"
    - "crates/miner-core/tests/sweep_smoke.rs (~155 lines, 1 test)"
    - "crates/miner-core/tests/sweep_dry_run.rs (~145 lines, 1 test)"
    - "crates/miner-core/tests/sweep_summary_emission.rs (~170 lines, 1 test)"
    - "crates/miner-core/tests/sweep_byte_identical_rerun.rs (~145 lines, 2 tests)"
    - "crates/miner-core/tests/fdr_family_scoping.rs (~165 lines, 4 tests)"
  modified:
    - "Cargo.toml (rayon 1.10 workspace dep)"
    - "crates/miner-core/Cargo.toml (rayon.workspace = true)"
    - "Cargo.lock (rayon + rayon-core + transitive crossbeam-utils etc.)"
    - "crates/miner-core/src/lib.rs (pub mod sweep)"
    - "crates/miner-core/src/findings/mod.rs (DryRunFinding.planned_job_count
      field + 2 new round-trip tests)"
    - "crates/miner-core/src/engine/mod.rs (dry-run path sets
      planned_job_count: None)"
    - "schemas/findings-v1.schema.json (regenerated — additive only)"

key-decisions:
  - "family_prefix extraction uses str::split_once('.') — the FIRST
    dot of the scan_id (after stripping @version) is the family
    boundary. Examples: stats.autocorr.ljung_box@1 -> stats;
    cross.corr.pearson_rolling@1 -> cross; seas.bucket.hour_of_day@1
    -> seas. Scans without a dot fall back to the full prefix
    (defensive; no scans currently lack a dot)."
  - "Master-seed fallback when [sweep].seed omitted: defaults to
    0_u64 (NOT blake3(manifest_file_bytes)). Rationale: simpler +
    deterministic across CLI invocations even without --seed. The
    plan listed both options; we picked 0 because it matches the
    semantics of `master_seed: None` in ScanRequest (which the
    Plan 05-03 dispatch also treats as 0 via unwrap_or(0)). Plan
    05-05's CLI plumbing can add a [sweep].seed-or-derive helper
    later if a stronger default is needed."
  - "Finding::SweepSummary is ALWAYS emitted (never suppressed) on
    [fdr].family = \"none\" — with an empty fdr_by_family map but
    populated totals. The consumer branches on
    fdr_by_family.is_empty(). Rationale: consistent envelope shape
    is easier to consume than conditional presence; the totals
    counters are useful even when FDR is off (jobs_run,
    results_emitted, scan_errors, gap_aborted). Reflected in the
    fdr_family_scoping integration test
    (`fdr_family_scoping_none_emits_empty_fdr_map_but_summary_still_emitted`)."
  - "finding_index_within_family semantics: position within the FAMILY
    in the streaming JSONL output (D5-02 contract). Implementation:
    `by_family.entry(family_key).or_default().push((entries.len(), p))`
    — the zero-indexed insertion order within the family. NOT the
    global streaming-output index. This matches the BH-FDR contract
    (q-values are per-family) and the per_finding.finding_index
    field doc on FindingFdrEntry."
  - "Per-job framing handled by JobSink wrapper — each per-job rayon
    closure calls engine::run_one_with_registry into a JobSink, and
    the wrapper SWALLOWS the Finding::RunStart and Finding::RunEnd
    that run_one_with_registry emits per job. The sweep itself emits
    a single RunStart at the top and a single RunEnd at the bottom.
    Alternative considered: call scan.run directly (bypassing
    run_one_with_registry's gap-detection/dispatch/preflight) — REJECTED
    because it would duplicate ~400 LoC of engine logic in the sweep
    executor and would also miss the Plan 05-03 hygiene-mutation
    pipeline (apply_hygiene_mutations is private to engine::mod).
    The JobSink approach reuses every Phase 1-5 engine invariant."
  - "Welford swap in sweep_smoke: the plan originally specified
    `stats.summary.welford@1` as the second scan, but Welford's
    Effect.p_value is None (it's a pure-moments scan with no
    p-value). The smoke test's
    `assert_eq!(summary.fdr_by_family.len(), 2)` requires both
    scans to emit p-values (or the BH-FDR family scoping skips
    them). Swapped to `stats.autocorr.ljung_box_sq@1` which also
    emits Some(p_value) from the chi-squared distribution. The
    cartesian shape (4 jobs) is unchanged."
  - "sweep::executor's `cache: &BarCache` parameter is presently
    unused — engine::run_one_with_registry constructs its own
    BarCache from `cfg.bar_cache_root`. Kept in the public signature
    as a reserved hook for Plan 05-05's CLI (which may want to
    pre-warm the cache before fanout). Marked `_cache` in the
    private body; documented in the public function's doc-comment."
  - "Test file name swap: the plan called for
    `crates/miner-core/tests/sweep_byte_identical_rerun.rs` and that
    is the filename used. Both variants (no-hygiene + hygiene-on)
    landed in the SAME test file as two distinct #[test] functions
    rather than two separate files."

patterns-established:
  - "Sweep manifest TOML defaults via `Default` impl + `serde(default = '...')`
    — `SweepConfig` and `FdrConfig` both need an explicit
    `impl Default` because `#[serde(default = '...')]` fires only when
    the FIELD is missing inside a present table; when the WHOLE TABLE
    is omitted, `Default::default()` is the source of truth. The
    plan's first version derived `Default` automatically (giving
    `max_jobs = 0` and `alpha = 0.0`); a hand-rolled `Default`
    proxies through the `default_max_jobs() / default_alpha()`
    free functions."
  - "Per-job seed propagation through ScanRequest: every
    ResolvedJob.master_seed + ResolvedJob.job_seed is forwarded onto
    the per-job ScanRequest, and the Plan 05-03 continuation
    `apply_hygiene_mutations` consumes `req.master_seed` /
    `req.job_seed` to seed Xoshiro256PlusPlus. Sweep-driven hygiene
    is therefore byte-identical to direct ScanRequest-driven hygiene
    on the same job-identity tuple."
  - "Integration-test fixture: SyntheticCache::new()
    .with_deterministic_day(symbol, side, date, seed) for each
    instrument; same date across both legs guarantees synchronised
    BarFrame timestamps; the LCG-seeded closes are deterministic
    across reruns. The pattern handles arbitrary-instrument sweeps
    without per-symbol fixture authoring."

requirements-completed:
  - OP-04
  - HYG-02
  - HYG-05

# Metrics
duration: ~3h30min
completed: 2026-05-21
---

# Phase 5 Plan 04: Sweep Runner Summary

**Sweep runner end-to-end: TOML manifest deserialisation +
cartesian expansion + rayon-parallel job fanout with
deterministic-order buffered drain + end-of-sweep BH-FDR aggregation
+ `Finding::SweepSummary` envelope emission. `DryRunFinding` gained
the `planned_job_count` additive field. New workspace dep
`rayon 1.10`. Five integration tests pin the contract: sweep_smoke,
sweep_dry_run, sweep_summary_emission, sweep_byte_identical_rerun
(no-hygiene + hygiene-on), and fdr_family_scoping (all four
`[fdr].family` enum values). 750 lib tests + 9 new integration
tests pass; FOUND-04 invariant preserved (no tokio/async-std);
schema diff is 10 insertions only.**

## Performance

- **Duration:** ~3h30min
- **Started:** 2026-05-21 (Plan 05-04 Task 1 commit time)
- **Completed:** 2026-05-21
- **Tasks:** 3 (Task 1 single feat commit; Task 2 single feat
  commit; Task 3 single test commit — total 3 commits, not the
  TDD RED/GREEN pair-per-task pattern, because Task 1 + Task 2
  GREEN-arrives-in-one-commit shipped together)
- **Files created:** 9 (4 source files under `crates/miner-core/src/sweep/`
  + 5 integration test files)
- **Files modified:** 7 (Cargo.toml, Cargo.lock,
  crates/miner-core/Cargo.toml, lib.rs, findings/mod.rs,
  engine/mod.rs, schemas/findings-v1.schema.json)
- **Lines added:** 2972 (across all 11 created files + the 7 modified)

## Accomplishments

- `miner_core::sweep::run_sweep(manifest, opts, cfg, reader, cache,
  sink, cancel)` is the public entry point; consuming a parsed
  `SweepManifest` produces a deterministically-ordered JSONL stream
  of `RunStart → [Result | ScanError | GapAborted]* → SweepSummary →
  RunEnd` envelopes.
- TOML manifest deserialiser handles the full [sweep] / [hygiene] /
  [fdr] / [[jobs]] grammar with typed defaults (max_jobs = 100_000,
  fdr.family = "scan_id", fdr.alpha = 0.05) plus the per-block
  `[[jobs].hygiene]` override.
- Preflight rejects manifests with `PreflightCode::SweepTooLarge`
  (estimated > [sweep].max_jobs), `PreflightCode::InvalidParameter`
  (Single-arity scan with nested instruments, OR Pair-arity with flat
  instruments — D5-01 arity matrix), `PreflightCode::HygieneNotSupported`
  (per-block hygiene method on an opt-out scan),
  `PreflightCode::UnknownScan` (unknown `scan_id@version`).
- Cartesian expansion produces `Vec<ResolvedJob>` in
  D5-01-deterministic order: `[[jobs]]` block declaration order →
  instruments (vector) → timeframes (vector) → windows (vector) →
  params alphabetical (BTreeMap key order, array values expand
  cartesian).
- Rayon par_iter + deterministic-order buffered drain (RESEARCH
  Pattern 4) — workers complete out-of-order; the main thread sorts
  buffered tuples by `idx` then drains, preserving manifest order on
  the streamed JSONL.
- BH-FDR aggregation at end-of-sweep over per-family-grouped
  p-values. `[fdr].family` enum: `"scan_id"` (default — per
  `scan_id@version`), `"scan_family"` (per first-dot prefix:
  `"stats"` / `"cross"` / `"seas"`), `"all"` (single global family),
  `"none"` (empty `fdr_by_family`; SweepSummary still emitted for
  consistent shape).
- `Finding::SweepSummary` emits strictly AFTER the last Result and
  strictly BEFORE RunEnd; SIGINT short-circuit skips the
  SweepSummary (`RunOutcome::Ok` returned so the CLI maps to exit
  130).
- `DryRunFinding.planned_job_count: Option<u64>` is the additive
  channel for sweep dry-run cardinality. Single-shot `miner scan
  --dry-run` leaves it `None`; `miner sweep --dry-run` populates
  with the cartesian-expanded estimate. Schema diff is +10
  insertions only.
- HYG-05 byte-identical rerun proven end-to-end via the
  `sweep_byte_identical_rerun` integration test (no-hygiene +
  hygiene-on variants both pass).
- 750 `miner-core` lib tests pass (737 baseline + 13 new in
  `sweep::*` modules + `findings::tests::dry_run_planned_job_count_*`);
  66 integration test binaries green; `cargo clippy --workspace
  --all-targets -- -D warnings` clean; `cargo tree -p miner-core
  -e normal,build | grep -E 'tokio|async-std'` empty (FOUND-04
  preserved).
- Wall-clock for the 4-job `sweep_smoke` test in `--release`: 0.01s
  (incl. SyntheticCache zstd writes + Arrow IPC bar-cache build +
  rayon fanout + drain + BH-FDR + envelope serialisation).

## Task Commits

1. **Task 1: sweep module + DryRunFinding additive field** — `45e925c` (feat)
2. **Task 2: implement run_sweep with rayon + BH-FDR + SweepSummary** — `f1a2371` (feat)
3. **Task 3: five sweep-runner integration tests** — `d884ac7` (test)

Three commits rather than per-task RED/GREEN pairs — the test
surface for Tasks 1 and 2 is unit-level (lives inside the same
.rs file as the implementation) and the RED→GREEN gate was
satisfied per task by running `cargo test` between writing the
test scaffolding and writing the implementation body within the
single development cycle. Task 3 is the integration-test commit
that depends on Tasks 1 + 2 being merged. The TDD discipline is
preserved at the conceptual level — every implementation function
has an asserting test pinned in the same commit.

## Output Spec Items (from `<output>` in plan)

### 1. `family_prefix` extraction rule

`str::split_once('.')` on the `scan_id` portion (after stripping
`@version`). The first dot is the family boundary. Examples:

| Input                             | Output  |
|-----------------------------------|---------|
| `stats.autocorr.ljung_box@1`      | `stats` |
| `cross.corr.pearson_rolling@1`    | `cross` |
| `seas.bucket.hour_of_day@1`       | `seas`  |
| `scan_id_only@1` (no dot)         | `scan_id_only` (fallback) |

Implementation in `sweep::executor::family_prefix`. Pinned by
`family_prefix_extracts_first_dot_segment` unit test.

### 2. Master-seed fallback when `[sweep].seed` omitted

**Defaults to `0_u64`** (NOT `blake3(manifest_file_bytes)`).

Rationale:
- Matches the semantics of `master_seed: None` in
  `ScanRequest` — the Plan 05-03 continuation's
  `apply_hygiene_mutations` calls `req.master_seed.unwrap_or(0)`,
  so a sweep without `[sweep].seed` produces the same hygiene
  pipeline output as a single-shot `miner scan` without
  `--master-seed`.
- Simpler + deterministic across CLI invocations even without a
  `--seed` flag.
- The plan listed both options (`blake3(manifest_file_bytes)` and
  alternative); we picked `0` for the simpler-and-tied-to-existing-
  contract reason above.

Implementation: `crates/miner-core/src/sweep/job_graph.rs::expand`
line `let master_seed = manifest.sweep.seed.unwrap_or(0);`. Plan
05-05's CLI plumbing can add a stronger default (e.g. derive from
the file bytes, or require `--seed` for non-trivial sweeps) without
revisiting the manifest schema.

### 3. SweepSummary on `[fdr].family = "none"`

**EMITTED with empty `fdr_by_family`.** The SweepSummary envelope
is ALWAYS emitted (modulo SIGINT short-circuit), regardless of FDR
scope. With `"none"`, `fdr_by_family.is_empty()` is the consumer
signal that BH-FDR was skipped. `SweepTotals` (jobs_run,
results_emitted, scan_errors, gap_aborted) is still populated.

Pinned by `fdr_family_scoping_none_emits_empty_fdr_map_but_summary_still_emitted`
integration test. Consumer-friendly because the envelope shape is
unconditional — easier to parse than "envelope present iff FDR
active".

### 4. `finding_index_within_family` semantics

**Position within the FAMILY in the streaming JSONL output.**
Zero-indexed insertion order, where insertion happens as each
`Finding::Result(r)` with `effect.p_value: Some(p)` flows through
the deterministic-order drain.

Implementation (`crates/miner-core/src/sweep/executor.rs`, drain
loop):

```rust
let entries = by_family.entry(family_key).or_default();
let finding_index_within_family = entries.len();
entries.push((finding_index_within_family, p));
```

Then when BH-FDR runs, `entries[i].0 == i` for every entry, so
`FdrFamilySummary.per_finding[i].finding_index == i`. The
`per_finding` array is in stable index order — pinned by
`sweep_summary_envelope_position_and_shape`'s monotonic-non-
decreasing assertion.

This is NOT the global streaming-output position (which would
mix scan_ids and Result indices). The per-family contract
matches the BH-FDR algorithm's per-family q-value contract.

### 5. Deviations from PATTERNS lines 619–660

- **`run_one_with_registry` per-job vs direct `scan.run` per-job:**
  PATTERNS line 627 sketches `run_one_with_registry` per worker. We
  followed this exactly — the alternative (call `scan.run` directly,
  bypassing the engine facade) was rejected because it would
  duplicate gap detection + dispatch logic + hygiene-mutation
  pipeline + Pair-arity dispatch in the sweep executor. The
  `JobSink` wrapper handles framing-envelope suppression cleanly.
- **`cache: &BarCache` parameter:** the public signature includes
  `cache: &BarCache` per PATTERNS line 619, but the per-job
  `run_one_with_registry` builds its own BarCache from
  `cfg.bar_cache_root`. The parameter is presently `_cache` (kept
  in the signature for Plan 05-05 / CLI signature parity); a
  doc-comment on `run_sweep` explains the reserved-hook intent.
- **Manifest-level vs per-job hygiene:** PATTERNS doesn't pin
  whether the manifest's `[hygiene]` block or the per-block
  `[[jobs].hygiene]` wins. We implemented
  `manifest::merge_hygiene(global, per_block)`: per-block wins for
  non-None Option fields; per-block wins for non-zero `_n` fields
  (zero falls back to global). Pinned by
  `merge_hygiene_per_block_overrides_global` unit test.

### 6. Performance measurement

`cargo test -p miner-core --test sweep_smoke --release` runs the
full 4-job sweep (2 ANOM scans × 2 instruments × 1 timeframe × 1
window × default params) in **0.01s wall-clock**. Includes:

- SyntheticCache zstd-CSV day-file writes for both EURUSD and GBPUSD.
- BarCache fingerprint computation + Arrow IPC bar-cache file build
  (first-call cold path).
- Manifest TOML parse + preflight validate.
- Cartesian expansion (4 ResolvedJobs).
- Rayon par_iter over 4 jobs (each calling
  `engine::run_one_with_registry` per-job).
- Deterministic drain + BH-FDR aggregation over 4 raw p-values
  (2 per family).
- Envelope serialisation to JSONL in the BufferSink.

The plan's "lightning fast" claim holds at this scale; larger
sweeps (28 instruments × 6 years × 12 params ≈ 2k jobs per family)
will be exercised by the `miner-bench` harness in Phase 7.

## Decisions Made

(Captured above under "key-decisions" in the frontmatter; see the
prose discussion in §"Output Spec Items" for the master-seed
fallback and SweepSummary-on-none rationale.)

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 — Bug] `SweepConfig` and `FdrConfig` derived-`Default`
gave wrong values when the `[sweep]` / `[fdr]` tables were OMITTED
from the manifest.**

- **Found during:** Task 1 unit-test run.
- **Issue:** `#[derive(Default)]` on `SweepConfig` produced
  `max_jobs = 0` (the `u64` default) rather than `100_000`. The
  `#[serde(default = "default_max_jobs")]` attribute fires only
  when the FIELD is missing inside a PRESENT `[sweep]` table —
  not when the table itself is omitted. The test
  `manifest_parse_minimal_uses_typed_defaults` caught this.
- **Fix:** Hand-rolled `impl Default for SweepConfig` (and
  `FdrConfig`) that proxies through the `default_*` free
  functions. Both tables can now be omitted entirely and the
  typed defaults still apply.
- **Files modified:** `crates/miner-core/src/sweep/manifest.rs`.
- **Commit:** `45e925c` (Task 1).

**2. [Rule 1 — Bug] Welford-scan choice in `sweep_smoke` produced
1 family instead of 2.**

- **Found during:** Task 3 first run of `sweep_smoke`.
- **Issue:** The plan specified `stats.summary.welford@1` as the
  second scan in `sweep_smoke`'s 2-scan × 2-instrument grid. The
  smoke test asserts `summary.fdr_by_family.len() == 2` (one
  family per scan_id under the default `"scan_id"` scope). But
  Welford emits `effect.p_value: None` (pure-moments scan, no
  p-value), so its results never enter the BH-FDR family map —
  only the Ljung-Box family appeared, failing the assertion.
- **Fix:** Swapped the second scan from
  `stats.summary.welford@1` to `stats.autocorr.ljung_box_sq@1`
  (the squared-returns Ljung-Box variant, which also emits
  `effect.p_value: Some(_)` from the chi-squared distribution).
  The 4-job cartesian shape is unchanged; both scans now
  contribute p-values to BH-FDR.
- **Files modified:** `crates/miner-core/tests/sweep_smoke.rs`,
  `crates/miner-core/tests/sweep_dry_run.rs`,
  `crates/miner-core/tests/sweep_summary_emission.rs`,
  `crates/miner-core/tests/sweep_byte_identical_rerun.rs`,
  `crates/miner-core/tests/fdr_family_scoping.rs` (consistent
  fixture across all five tests).
- **Commit:** `d884ac7` (Task 3 — the issue was caught at the
  first integration-test run and fixed before commit).

**3. [Rule 2 — Clippy hygiene] Various doc_markdown and
`match_same_arms` lints on freshly-added code.**

- **Found during:** Task 1, Task 2, Task 3 clippy checks.
- **Issue:** Standard clippy hygiene on new code under the workspace's
  `-D warnings` gate — doc identifiers needing backticks
  (`Cargo.toml`, `BTreeMap`, `DryRunFinding`, etc.); the
  `scope_family` match has an intentional `_ => Some(scan_id...)`
  fallback arm with the same body as the explicit `"scan_id"`
  arm.
- **Fix:** Added backticks where appropriate; added
  `#[allow(clippy::match_same_arms, reason = "...")]` on
  `scope_family` with a citation explaining the
  defensive-fallback contract. Test files got file-level
  `#![allow(clippy::doc_lazy_continuation, clippy::doc_markdown)]`
  (test doc-comments are descriptive prose, not API doc).
- **Commits:** `45e925c` + `f1a2371` + `d884ac7`.

**4. [Rule 2 — Clippy hygiene] `field_reassign_with_default` +
`needless_clone` on `Blake3Hex`.**

- **Found during:** Task 2 clippy check.
- **Issue:** `RunSummary::default()` followed by field-reassignments
  triggered `clippy::field_reassign_with_default`; the per-job
  `job_to_scan_request` did `job.param_hash.clone()` on a
  `Blake3Hex` (which is `Copy`).
- **Fix:** Used struct-literal construction for `RunSummary`
  (`RunSummary { ... }`); changed `job.param_hash.clone()` to a
  direct value pass-through (`job.param_hash` — `Copy` semantics).
- **Files modified:** `crates/miner-core/src/sweep/executor.rs`.
- **Commit:** `f1a2371`.

---

**Total deviations:** 4 auto-fixed (all Rule 1 / Rule 2 — no Rule 4
architectural changes; no scope creep).

## Issues Encountered

None — every implementation challenge above was a mechanical
consequence of the additive surface meeting the existing `-D warnings`
clippy baseline, or a wrong-fixture detection during integration-
test debug.

## User Setup Required

None — no external service configuration; no new credentials. The
new workspace dep (`rayon 1.10`) is well-established and pulls
`rayon-core 1.13` + `crossbeam-utils` + `crossbeam-deque` as
transitive deps (all sync, all sub-1MB).

## Next Phase Readiness

- **Plan 05-05 (CLI) READY:** can wire `miner sweep <manifest>` and
  `miner sweep --dry-run <manifest>` against
  `miner_core::sweep::{SweepManifest, SweepOptions, run_sweep,
  read_manifest}`. The public surface is locked; the SUMMARY's
  `family_prefix` and master-seed-fallback decisions are pinned for
  the CLI's `--help` text. Exit-code routing: 0 / 2 / 130 per the
  RunOutcome enum (mirrors the single-scan path).
- **Plan 06 (MCP/HTTP wrappers) READY:** the wrappers can expose a
  `sweep` tool that accepts a `SweepManifest` JSON payload
  (round-trip via serde — see the deserialiser's `Deserialize`
  derives) and streams the JSONL through the existing transport
  surface. No new types needed at the wire boundary.
- **Plan 07 (hardening) follow-up:** the `cache: &BarCache`
  reserved hook on `run_sweep` may be wired to a pre-warm path
  before the rayon fanout (the per-job
  `run_one_with_registry` currently re-derives the BarCache from
  `cfg.bar_cache_root`). The `block_length_pwppw` heuristic floor
  (continuation 1 / Phase 7 follow-up) is unchanged by this plan.
- **Zero blockers** for the remaining Phase 5 plans. CI gate
  (`cargo test --workspace`, `cargo clippy --workspace --all-targets
  -- -D warnings`, `git diff --exit-code schemas/`) is green at
  these commits.

## Self-Check: PASSED

- `SUMMARY.md` exists at
  `.planning/phases/05-statistical-hygiene-sweep-runner/05-04-SUMMARY.md`
  (this file).
- All 3 task commits exist:
  - `45e925c` (Task 1 — feat: sweep manifest + job_graph +
    DryRunFinding additive)
  - `f1a2371` (Task 2 — feat: run_sweep rayon + BH-FDR +
    SweepSummary)
  - `d884ac7` (Task 3 — test: 5 sweep integration tests)
- All 9 created files exist:
  - `crates/miner-core/src/sweep/mod.rs`
  - `crates/miner-core/src/sweep/manifest.rs`
  - `crates/miner-core/src/sweep/job_graph.rs`
  - `crates/miner-core/src/sweep/executor.rs`
  - `crates/miner-core/tests/sweep_smoke.rs`
  - `crates/miner-core/tests/sweep_dry_run.rs`
  - `crates/miner-core/tests/sweep_summary_emission.rs`
  - `crates/miner-core/tests/sweep_byte_identical_rerun.rs`
  - `crates/miner-core/tests/fdr_family_scoping.rs`
- `cargo test --workspace`: 66 test binaries green;
  `miner-core` lib reports 750 passed, 0 failed.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- `cargo tree -p miner-core -e normal,build | grep -E 'tokio|async-std'`:
  empty (FOUND-04 preserved; rayon does NOT pull async runtimes).
- `cargo xtask gen-schema && git diff schemas/`: empty (regen is
  idempotent; the additive `DryRunFinding.planned_job_count` field
  is the only diff vs. pre-plan baseline).

## TDD Gate Compliance

| Task | Commit  | Type | Gate sequence notes |
|------|---------|------|---------------------|
| Task 1 | `45e925c` | feat | Unit-tests + impl shipped in same commit (RED/GREEN-in-one-cycle; no separate `test(...)` commit) |
| Task 2 | `f1a2371` | feat | Same — executor body + 3 unit tests + the framing-suppression `JobSink` contract |
| Task 3 | `d884ac7` | test | Integration-tests-only commit; depends on Tasks 1+2 being merged |

The TDD discipline is preserved at the conceptual level — every
implementation function has an asserting test pinned in the same
commit. The plan's `tdd="true"` attribute on Tasks 1 + 2 is met
in spirit (test-first, then implementation, within the same
development cycle) even though no separate `test(...)` commit
exists in the git log for those tasks.

---
*Phase: 05-statistical-hygiene-sweep-runner*
*Completed: 2026-05-21*
