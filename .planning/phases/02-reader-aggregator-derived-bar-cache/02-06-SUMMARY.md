---
phase: 02-reader-aggregator-derived-bar-cache
plan: 06
subsystem: infra
tags: [rust, integration-test, determinism, dyn-compat, public-surface, frozen-block, arrow-ipc, blake3, dukascopy, validation]

# Dependency graph
requires:
  - phase: 02-reader-aggregator-derived-bar-cache (Plans 02-01..02-05)
    provides: "Reader trait + DukascopyReader + path_layout::day_csv_zst (02-01); Calendar::fx_major + aggregate kernel + BarFrame + AGGREGATOR_VERSION (02-02); DST + edge-case integration tests (02-03); GapDetector + GapManifest + insta snapshots scaffolding (02-04); BarCache + FingerprintSidecar + Arrow IPC two-step atomic write API + ARROW_SCHEMA_VERSION + build_arrow_schema + 3 snapshot files (02-05)."
provides:
  - "crates/miner-core/tests/full_determinism.rs — end-to-end pipeline byte-identity test (DukascopyReader → aggregate → BarCache get_or_build × 2; assert byte-identical Arrow IPC + sidecar JSON across runs; iterates all 6 timeframe×side combos)."
  - "crates/miner-core/tests/public_surface_audit.rs — compile-time gate over all 20 Phase 2 FROZEN public names; consumes `use miner_core::*` only (never names internal modules); coerces EmptyReader to `&dyn Reader<…>` to gate dyn-compat at the audit site as well."
  - "crates/miner-reader-dukascopy/tests/reader_trait_object_safety.rs — integration-level dyn-compat regression gate for DukascopyReader (mirrors the inline test in miner-core/src/reader.rs from Plan 02-01); proves both `&dyn Reader<Error = DukascopyError>` and `Box<dyn Reader<…>>` coercion."
  - "crates/miner-core/Cargo.toml [dev-dependencies] — adds `miner-reader-dukascopy` (path dep, dev-only cycle accepted per T-02-21) + `zstd` (workspace) for the full-determinism test's synthetic-cache writer."
  - ".planning/phases/02-reader-aggregator-derived-bar-cache/02-VALIDATION.md — Phase 2 close-out: wave_0_complete=true, status=completed, execution_phase_completed=2026-05-18; all 14 Wave 0 Requirements checkboxes ticked; all 34 Per-Task Verification Map rows updated to ✅ green; Validation Sign-Off Approval re-issued by Plan 02-06 (execution-phase completion)."
  - "Workspace fmt-clean restoration: the 02-03 integration test files (aggregator_edge_cases.rs / dst_fall_back.rs / dst_spring_forward.rs) flagged in deferred-items.md are rustfmt-clean as of this plan."
affects:
  - "Phase 3 (scan engine + facade): inherits the closed Phase 2 surface (Reader, aggregate, BarCache, GapManifest) and the byte-identity guarantee through the FROZEN public re-exports."
  - "Phase 4 (scans): scan implementations import via `use miner_core::*` — every name they need is now audited to be reachable through the FROZEN block."
  - "Phase 6 (MCP/HTTP wrappers): the dyn-compat tests gate that Phase 6's workers can build `Box<dyn Reader<…>>` per-task without hitting trait-object-safety regressions."
  - "Phase 7 (bench harness): the full_determinism test is the regression-detection backbone — any kernel-level perf change must keep this gate green."

# Tech tracking
tech-stack:
  added: []  # No new workspace deps; only dev-dep wiring (miner-reader-dukascopy + zstd) for the determinism test.
  patterns:
    - "Dev-dep cycle (miner-core dev-deps → miner-reader-dukascopy → miner-core) accepted because dev-deps are NOT part of the published API graph (cargo permits dev-dep cycles; T-02-21 disposition)."
    - "Compile-time public-surface audit test: `use miner_core::*` names every FROZEN type; coerces concrete impls to `&dyn Trait<…>` at the audit site. Names are forced to be reachable through the FROZEN block (NOT through internal modules)."
    - "Both-sides dyn-compat regression gate: trait declaration (inline `reader::tests::reader_trait_object_safe` in miner-core) AND every concrete impl (separate `reader_trait_object_safety` integration test in each reader crate). Either side alone is necessary-but-not-sufficient."
    - "End-to-end byte-identity test wraps the REAL reader (DukascopyReader on synthetic on-disk .csv.zst) — NOT a MockReader — so the zstd decoder + CSV parser + blake3 fingerprinter + walkdir + path_layout all participate in the byte-identity assertion."
    - "Inline synthetic Dukascopy cache builder (uses the sibling crate's public `day_csv_zst` API + raw zstd encoder) avoids cross-crate `#[path]`-include fragility while still exercising the production path layout."

key-files:
  created:
    - "crates/miner-core/tests/full_determinism.rs (~225 lines: inline synthetic cache builder + `run_full_pipeline_for(tf, side)` + 2 tests `two_runs_byte_identical` + `two_runs_byte_identical_three_timeframes`)"
    - "crates/miner-core/tests/public_surface_audit.rs (~205 lines: imports all 20 Phase 2 FROZEN names; constructs / type-asserts each; inline EmptyReader stub for `aggregate::<R>` + `&dyn Reader<…>` coercion at the audit site)"
    - "crates/miner-reader-dukascopy/tests/reader_trait_object_safety.rs (~55 lines: single test `dukascopy_reader_is_dyn_compatible` with both `&dyn Reader<…>` and `Box<dyn Reader<…>>` coercions)"
  modified:
    - "crates/miner-core/Cargo.toml (add `miner-reader-dukascopy` path dev-dep + `zstd.workspace = true` dev-dep for the determinism test)"
    - "Cargo.lock (resolver output — workspace deps unchanged at the version level)"
    - "crates/miner-core/tests/aggregator_edge_cases.rs / dst_fall_back.rs / dst_spring_forward.rs (rustfmt: rewrap long assert!/let-binding lines per deferred-items.md scope — pre-existing drift cleared in one atomic commit for Phase 2 fmt-clean close-out)"
    - ".planning/phases/02-reader-aggregator-derived-bar-cache/02-VALIDATION.md (frontmatter close-out + 14 Wave 0 checkboxes ticked + 34 verification-map rows updated to ✅ green + TBD substring purged from prose + Sign-Off + Approval re-issued)"

key-decisions:
  - "Dev-dep cycle (miner-core → miner-reader-dukascopy → miner-core) accepted, documented in commit and threat model T-02-21. Cargo permits dev-dep cycles because dev-deps are not part of the published API graph; the cycle exists ONLY for the full_determinism test that needs the REAL DukascopyReader to exercise the end-to-end pipeline (zstd-CSV parse + blake3 + path layout) — a MockReader would not catch zstd / CSV / walkdir-determinism regressions."
  - "Synthetic Dukascopy cache for the determinism test is INLINED into the test file (uses the sibling crate's public `day_csv_zst` API + raw zstd encoder) rather than #[path]-including `miner-reader-dukascopy/tests/fixtures/mod.rs`. The inlined helper is ~50 LOC; the path-include alternative is fragile (rebuilds depending on a relative test-path that cargo does not guarantee stable)."
  - "Public-surface audit test creates a minimal EmptyReader stub rather than depend on DukascopyReader (which would deepen the dev-dep cycle). EmptyReader's `read_1m_bars` returns `Box::new(std::iter::empty())` — enough to coerce to `&dyn Reader<…>` and to instantiate `aggregate::<EmptyReader>` for the fn-pointer name-resolution gate."
  - "`grep -c 'TBD | TBD' VALIDATION.md` == 0 is satisfied by REWORDING the legend prose that previously said `> All TBD | TBD rows have been replaced...`. The prose now says `> Plan/Wave/Task-ID columns are all populated with real {plan-id}-T{task-num} identifiers; no placeholder rows remain.` — keeps the intent (audit-trail of revision pass 1) without keeping the literal substring."
  - "Pre-existing rustfmt drift in Plan 02-03 integration test files (deferred-items.md) rolled into Task 1's commit instead of a separate commit. Rationale: the workspace fmt gate must be green to mark Phase 2 complete; bundling the fmt fix with the new fmt-clean test keeps the commit count minimal and atomic (one commit per planned task)."

patterns-established:
  - "Phase close-out plan shape (Plan 02-06): (a) end-to-end determinism gate test, (b) compile-time public-surface audit test, (c) integration-level dyn-compat test per concrete impl crate, (d) VALIDATION.md frontmatter update + all wave-0 checkboxes + all verification-map rows green + Sign-Off + Approval re-issued. Reusable template for Phase 3 close-out and beyond."
  - "Forced-naming through FROZEN re-exports: `use miner_core::{Foo, Bar, Baz};` in the audit test, NOT `use miner_core::aggregator::Foo;`. Catches an accidental removal of a `pub use` line at the source of the contract (miner-core/src/lib.rs) instead of in some downstream consumer crate."
  - "Coerce-to-dyn at the audit site too: even though there's a separate `reader_trait_object_safety` test, the audit test also coerces an EmptyReader to `&dyn Reader<…>` so any regression that breaks dyn-compat without breaking the surface still fails the audit (and vice-versa). Defence-in-depth: both gates catch a single class of regression."
  - "Synthetic data with fixed seeds keeps byte-identity tests deterministic: `open = 1.0 + i * 0.0001`, `high = open + 0.00005`, `volume = (i + 1) as f64` etc. — every value is a pure function of `i`. No clock, no random, no env."

requirements-completed:
  - CACHE-04
  - CACHE-06

# Metrics
duration: ~35min
completed: 2026-05-18
---

# Phase 02 Plan 06: Phase 2 Close-out — End-to-end determinism gate + FROZEN public-surface audit + dyn-compat regression Summary

**End-to-end byte-identity gate proves the Reader → aggregate → BarCache pipeline is fully deterministic; FROZEN public-surface audit gates all 20 Phase 2 re-exports; standalone DukascopyReader dyn-compat test seals CACHE-02; VALIDATION.md marks Phase 2 closed. Phase 2 COMPLETE.**

## Performance

- **Duration:** ~35 min
- **Started:** 2026-05-18 (worktree spawn at base `4bca313`)
- **Completed:** 2026-05-18
- **Tasks:** 3 (atomic commits)
- **Files modified:** 8 (3 created — full_determinism.rs / public_surface_audit.rs / reader_trait_object_safety.rs; 5 modified — Cargo.toml + Cargo.lock + 3 02-03 integration tests for fmt + VALIDATION.md)
- **Tests added:** 4 (2 in full_determinism + 1 in public_surface_audit + 1 in reader_trait_object_safety)
- **Total miner-core tests after this plan:** 67 unit + 30+ integration (incl. 2 new full_determinism + 1 new public_surface_audit), all green
- **Total miner-reader-dukascopy tests after this plan:** 9 unit + 5 reader_smoke + 1 new reader_trait_object_safety, all green

## Accomplishments

- **Headline determinism gate** (`tests/full_determinism.rs`): two runs of the REAL pipeline (`DukascopyReader` over synthetic `.csv.zst` cache → `aggregate` → `BarCache::get_or_build` → Arrow IPC + sidecar JSON) MUST produce byte-identical Arrow bytes AND byte-identical sidecar JSON. `two_runs_byte_identical_three_timeframes` extends this to all 6 timeframe × side combinations. Any HashMap iteration leaking, `par_iter` ordering, clock read, `walkdir` non-sort, or `arrow::Schema` constructed from a non-BTreeMap source — any of these break this gate.
- **FROZEN public-surface audit** (`tests/public_surface_audit.rs`): compile-time gate over every Phase 2 type. Imports via `use miner_core::{…}` only (no internal-module reach-through); constructs / type-asserts each name; coerces an EmptyReader to `&dyn Reader<Error = std::io::Error>` to gate dyn-compat at the audit site too. Removing a `pub use` from `lib.rs` fails compilation here.
- **DukascopyReader dyn-compat integration test** (`miner-reader-dukascopy/tests/reader_trait_object_safety.rs`): proves both `&dyn Reader<Error = DukascopyError>` and `Box<dyn Reader<…>>` coercion. Mirrors Plan 02-01's inline `reader::tests::reader_trait_object_safe` from the miner-core side; both gates catch the regression from BOTH sides of the trait/impl seam.
- **Phase 2 VALIDATION.md closed out**: frontmatter `wave_0_complete: true` + `status: completed`; all 14 Wave 0 Requirements ticked; all 34 Per-Task Verification Map rows updated from `⬜ pending` → `✅ green`; legend prose reworded so `grep -c 'TBD | TBD'` returns 0; Sign-Off check-line added for the execution phase; Approval re-issued by Plan 02-06.
- **Workspace fmt-clean restored**: the 02-03 integration test files (aggregator_edge_cases.rs / dst_fall_back.rs / dst_spring_forward.rs) flagged in deferred-items.md are rustfmt-clean now, bundled into Task 1's atomic commit.
- **`cargo tree -p miner-core --edges normal,build | grep tokio/async-std/async-trait` empty** — FOUND-04 gate intact across the cycle.

## Task Commits

Each task was committed atomically on `worktree-agent-ab86dab7f4b6cac13` (base `4bca313`):

1. **Task 1: full-pipeline determinism gate + workspace fmt-clean** — `5b33c1a` (test)
2. **Task 2: reader dyn-compat gate + FROZEN public-surface audit** — `efbfdea` (test)
3. **Task 3: VALIDATION.md close-out (wave_0_complete + all rows green)** — `8de6af4` (docs)

_Plan metadata commit (this SUMMARY) follows._

## Files Created/Modified

### Created

- `crates/miner-core/tests/full_determinism.rs` — end-to-end byte-identity test. `build_synthetic_dukascopy_cache` writes 3 days × 2 sides × 1440 1m bars at the canonical Dukascopy path layout (00-indexed month encapsulation via the sibling crate's public `day_csv_zst`). `run_full_pipeline_for(source, cache, tf, side)` drives `DukascopyReader → BarCache::get_or_build`, reads the resulting `<cache>/dukascopy/EURUSD/<tf>_<side>.arrow` + `<…>.fingerprints.json` bytes. 2 tests: `two_runs_byte_identical` (Tf15m/Bid headline) + `two_runs_byte_identical_three_timeframes` (all 6 tf × side combos). Header comment lists the 5 likely culprits from 02-RESEARCH §"Determinism contract" lines 526-534 for future maintainers.
- `crates/miner-core/tests/public_surface_audit.rs` — single test `phase_2_public_surface_present` that imports all 20 Phase 2 FROZEN names from `miner_core` via `use miner_core::{…}` only. Constructs / type-asserts each. Forces `aggregate::<EmptyReader>` to resolve through `miner_core::aggregate` (not `miner_core::aggregator::aggregate`). Inline EmptyReader stub provides the concrete impl needed to instantiate `aggregate::<R>` + coerce to `&dyn Reader<Error = std::io::Error>`.
- `crates/miner-reader-dukascopy/tests/reader_trait_object_safety.rs` — single test `dukascopy_reader_is_dyn_compatible` proving both `&dyn Reader<Error = DukascopyError>` AND `Box<dyn Reader<…>>` coercion. Pure compile-time gate; no filesystem touches at runtime.

### Modified

- `crates/miner-core/Cargo.toml` — `[dev-dependencies]` adds `miner-reader-dukascopy = { path = "../miner-reader-dukascopy" }` + `zstd.workspace = true`. Cycle (`miner-core` dev-deps → `miner-reader-dukascopy` → `miner-core`) accepted per T-02-21 (cargo permits dev-dep cycles; dev-deps are not part of the published API graph).
- `Cargo.lock` — resolver output; no version drift (the new dev-deps were already in the resolver from sibling-crate inclusion).
- `crates/miner-core/tests/aggregator_edge_cases.rs` / `tests/dst_fall_back.rs` / `tests/dst_spring_forward.rs` — rustfmt rewrap of long `assert!`/`let`-binding lines that exceeded the wrap width. Pre-existing drift from Plans 02-03's task work (per `deferred-items.md`); cleared in Task 1's commit so the workspace fmt gate stays green for Phase 2 close-out. NO behavioural changes.
- `.planning/phases/02-reader-aggregator-derived-bar-cache/02-VALIDATION.md` — frontmatter close-out (`status: completed`, `wave_0_complete: true`, `execution_phase_completed: 2026-05-18`); 14 Wave 0 Requirements ticked; 34 Per-Task Verification Map rows updated to `✅ green`; legend prose reworded; Validation Sign-Off check-line added for the execution phase; Approval re-issued.

## Decisions Made

See `key-decisions` in frontmatter. Five highlights:

- **Dev-dep cycle (miner-core dev-deps → miner-reader-dukascopy → miner-core) accepted.** Cargo permits dev-dep cycles because dev-deps are not part of the published API graph. The cycle exists ONLY so the full_determinism test can exercise the REAL `DukascopyReader` (zstd decoder + CSV parser + blake3 fingerprinter + walkdir + path_layout) — a MockReader would not catch determinism regressions in any of those components. Documented in Task 1's commit message and in threat model T-02-21.
- **Inline synthetic-cache builder inside `tests/full_determinism.rs`** instead of `#[path]`-including the sibling crate's `tests/fixtures/mod.rs`. The inlined helper is ~50 LOC and uses ONLY the sibling crate's PUBLIC `day_csv_zst` API + raw zstd encoder — no internal-module reach-through. The path-include alternative is fragile (depends on a relative test-path that cargo does not guarantee stable across worktrees).
- **EmptyReader stub in `tests/public_surface_audit.rs`** instead of using DukascopyReader (which would deepen the dev-dep cycle). EmptyReader is a single `struct EmptyReader;` with five `impl Reader for EmptyReader` methods returning empty/`None`/empty-vec — enough to coerce to `&dyn Reader<…>` and to instantiate `aggregate::<EmptyReader>` for the fn-pointer name-resolution gate. The audit test doesn't need the reader to DO anything; it needs the trait + impl to COMPILE.
- **TBD-substring purge via prose rewording.** The legend prose at line 82 previously said `> All TBD | TBD rows have been replaced...` — that literal substring still tripped Plan 02-06 Task 3's `grep -c 'TBD | TBD'` verify gate. Reworded to `> Plan/Wave/Task-ID columns are all populated with real {plan-id}-T{task-num} identifiers; no placeholder rows remain.` — same intent (audit trail), no literal substring.
- **Pre-existing rustfmt drift in 02-03 files rolled into Task 1's commit.** Per the scope brief: "If the workspace fmt gate is currently red because of 02-03's pre-existing rustfmt drift, run `cargo fmt --all` and roll the fmt-only changes into one of the 02-06 atomic commits so the phase ends fmt-clean." Bundled into Task 1 (the test file whose addition surfaced the gate failure) rather than a standalone fmt-only commit — keeps the commit count at exactly N per planned task.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Plan-suggested `_accept(&reader)` triggered `clippy::used_underscore_items`**

- **Found during:** Task 2 (initial draft of `tests/reader_trait_object_safety.rs`)
- **Issue:** The plan and the Phase 1 precedent (`findings/sink.rs:399-409`) use `fn _accept(...) {}` + `_accept(&reader)` for object-safety regression tests. Clippy's `pedantic` lint flags this as `used_underscore_items` ("called an underscore-prefixed function").
- **Fix:** Renamed the helper from `_accept` to `accept_reader`. The semantic intent is preserved (a "dummy consumer" function whose body is empty) and the call site reads cleanly. Phase 1's `findings/sink.rs:399-409` test does not call `_accept`; it relies purely on the function declaration + type ascription. Plan 02-06's test goes a bit further by calling the function — hence the clippy regression that didn't surface in Phase 1.
- **Files modified:** `crates/miner-reader-dukascopy/tests/reader_trait_object_safety.rs`
- **Verification:** `cargo clippy -p miner-reader-dukascopy --all-targets -- -D warnings` exits 0.
- **Committed in:** `efbfdea` (Task 2).

**2. [Rule 3 - Blocking] `let _range`/`let _params` triggered `clippy::used_underscore_binding`**

- **Found during:** Task 2 (initial draft of `tests/public_surface_audit.rs`)
- **Issue:** Started the audit test with `_range: ClosedRangeUtc { ... }` + `_params: AggParams { ..., range: _range }` to suppress unused-variable warnings. Clippy `pedantic` then complained about USING the underscore-prefixed bindings (`range: _range`).
- **Fix:** Rewrote with proper named bindings (`range`, `params`, `bid`, `cal`, `tf_15m` etc.) and added a module-level `#[allow(unused_variables)]` on the test function with an explanatory comment ("Audit-test: many bindings exist solely to force a name resolution"). Lifted the doc-friendliness AND solved the clippy gate.
- **Files modified:** `crates/miner-core/tests/public_surface_audit.rs`
- **Verification:** `cargo clippy -p miner-core --all-targets -- -D warnings` exits 0; `cargo test -p miner-core --test public_surface_audit` exits 0.
- **Committed in:** `efbfdea` (Task 2).

**3. [Rule 3 - Blocking] `tf15` / `tf1h` / `tf1d` triggered `clippy::similar_names`**

- **Found during:** Task 2 (audit test naming)
- **Issue:** Three Timeframe bindings named `tf15` / `tf1h` / `tf1d` were too close together for clippy `pedantic`'s `similar_names` lint (4-character bindings, sharing the `tf` prefix).
- **Fix:** Renamed to `tf_15m` / `tf_hourly` / `tf_daily`. More readable AND clippy-clean.
- **Files modified:** `crates/miner-core/tests/public_surface_audit.rs`
- **Verification:** `cargo clippy -p miner-core --all-targets -- -D warnings` exits 0.
- **Committed in:** `efbfdea` (Task 2).

**4. [Rule 1 - Bug] Initial `type_name_of_val(&aggregate_fn).contains("aggregate")` assertion failed at runtime**

- **Found during:** Task 2 (`tests/public_surface_audit.rs` first run)
- **Issue:** I tried to assert the captured fn-pointer's `type_name_of_val` contained the substring `"aggregate"` to confirm the FUNCTION ITEM was reachable. `type_name_of_val` on a function-pointer-typed binding returns the FN-POINTER TYPE (`fn(&miner_core::EmptyReader, ...) -> Result<...>`), NOT the function's path. The substring `"aggregate"` did not appear in the fn-pointer type's `type_name`.
- **Fix:** Replaced with a non-null fn-pointer check: `assert!((aggregate_fn as usize) != 0, "aggregate fn pointer must be non-null")`. The TYPE ASCRIPTION on the binding (`let aggregate_fn: fn(...) -> Result<...> = aggregate::<EmptyReader>;`) is what enforces the compile-time gate — the runtime non-null check just keeps the binding alive through optimisation.
- **Files modified:** `crates/miner-core/tests/public_surface_audit.rs`
- **Verification:** `cargo test -p miner-core --test public_surface_audit phase_2_public_surface_present` exits 0.
- **Committed in:** `efbfdea` (Task 2 — colocated with the audit-test addition).

**5. [Rule 1 - Bug] `GapDetector<EmptyReader>` failed to compile — `GapDetector` is a unit struct, not generic**

- **Found during:** Task 2 (`tests/public_surface_audit.rs` first compile)
- **Issue:** Drafted the audit test assuming `GapDetector` was generic over `R: Reader` (`GapDetector<EmptyReader>` etc.). Actual shape from Plan 02-04 is `pub struct GapDetector;` — a unit struct — with `detect<R: Reader>(reader: &R, ...) -> GapManifest` taking the reader as a method parameter. Plan 02-04 chose this shape so a single `GapDetector` value can drive multiple reader impls.
- **Fix:** Replaced `let _: &dyn Fn(&GapDetector<EmptyReader>) = ...` with `let gap_detector: GapDetector = GapDetector;` — direct construction of the unit struct.
- **Files modified:** `crates/miner-core/tests/public_surface_audit.rs`
- **Verification:** `cargo test -p miner-core --test public_surface_audit` exits 0.
- **Committed in:** `efbfdea` (Task 2).

**6. [Rule 3 - Blocking] `clippy::doc_markdown` on the title `Plan 02-06 / Task 2 — DukascopyReader dyn-compat...`**

- **Found during:** Task 2 (workspace clippy run after creating `reader_trait_object_safety.rs`)
- **Issue:** Clippy `pedantic` requires backticks around code identifiers in doc comments. The header had `DukascopyReader` (a type name) without backticks.
- **Fix:** Backtick the type name: `` `DukascopyReader` ``. Touched a few other identifiers in the same module-doc block while at it.
- **Files modified:** `crates/miner-reader-dukascopy/tests/reader_trait_object_safety.rs`
- **Verification:** `cargo clippy --workspace --all-targets -- -D warnings` exits 0.
- **Committed in:** `efbfdea` (Task 2).

**7. [Rule 3 - Blocking] Pre-existing rustfmt drift in Plan 02-03 integration tests (deferred-items.md)**

- **Found during:** Task 1 verification gates (`cargo fmt --all --check` after adding `tests/full_determinism.rs`)
- **Issue:** `aggregator_edge_cases.rs` / `dst_fall_back.rs` / `dst_spring_forward.rs` had multiple over-wide `assert!(...)` and `let` lines that Plan 02-05 documented in `deferred-items.md` as "not Plan 02-05's concern." Plan 02-06's scope explicitly assigns these to me.
- **Fix:** Ran `cargo fmt --all` which rewrapped the long lines. NO behavioural changes — only whitespace + multi-line wrapping. Bundled into Task 1's atomic commit (the new fmt-clean test was the trigger for noticing the workspace gate failure).
- **Files modified:** `crates/miner-core/tests/aggregator_edge_cases.rs`, `crates/miner-core/tests/dst_fall_back.rs`, `crates/miner-core/tests/dst_spring_forward.rs`
- **Verification:** `cargo fmt --all --check` exits 0; all Plan 02-03 tests still pass (`cargo test -p miner-core --test aggregator_edge_cases / dst_spring_forward / dst_fall_back` all green).
- **Committed in:** `5b33c1a` (Task 1).

---

**Total deviations:** 7 auto-fixed (5 × Rule 3 Blocking lint/fmt, 2 × Rule 1 Bug compile/runtime).
**Impact on plan:** None of the deviations changed the planned behaviour. The largest (Deviation #1: `_accept` → `accept_reader`) is a naming choice that diverges from Phase 1's `findings/sink.rs:399-409` precedent — but only because Phase 2's test CALLS the helper (Phase 1 does not), surfacing a clippy lint that Phase 1 did not trip. Deviation #5 (`GapDetector` shape) is a small drift between the plan's pseudo-code and Plan 02-04's actual implementation; the fix matches the real type. All other deviations are lint compliance.

## Threat Surface Audit

All STRIDE entries from the plan `<threat_model>` are mitigated as planned:

- **T-02-19 (Tampering — end-to-end pipeline non-determinism):** `tests/full_determinism.rs::two_runs_byte_identical` is the catch-all gate. The test runs the FULL pipeline (real DukascopyReader + zstd-CSV parse + blake3 fingerprint + aggregate + Arrow IPC write + sidecar JSON) twice on the same synthetic source and asserts byte-equality on BOTH the Arrow file AND the sidecar JSON. Any HashMap leak, par_iter leak, clock leak, walkdir non-sort, or Arrow-metadata-from-non-BTreeMap-source fails this test. The `_three_timeframes` extension covers all 6 timeframe × side combinations.
- **T-02-20 (Repudiation — public-surface drift):** `tests/public_surface_audit.rs::phase_2_public_surface_present` is the compile-time gate. Imports every Phase 2 type via `use miner_core::{…}` only (no internal-module reach-through). Removing a `pub use` from `lib.rs` breaks this test at compile time, BEFORE downstream consumers are affected. The audit ALSO coerces an EmptyReader to `&dyn Reader<Error = std::io::Error>` at the same site, adding defence-in-depth against dyn-compat regressions.
- **T-02-21 (Information Disclosure — dev-dep cycle):** ACCEPTED per plan. Cargo permits dev-dep cycles because dev-deps are not part of the published API graph. The cycle (`miner-core` dev-deps → `miner-reader-dukascopy` → `miner-core`) exists ONLY so the full_determinism test can exercise the REAL DukascopyReader (not a MockReader). Documented in Task 1's commit message and `Cargo.toml` comment block.

No new threat surface introduced outside the plan's threat model. The audit test specifically does NOT add new public types — it only NAMES the existing FROZEN block.

## Threat Flags

None — Plan 02-06 introduces only test files + a dev-dep wiring + a docs update. No new runtime trust boundaries, no new network endpoints, no new auth paths, no schema changes.

## Known Stubs

None. Every behaviour the plan listed for closure is fully implemented:

- `tests/full_determinism.rs` — two tests, both green; exercise the FULL pipeline (no mocks anywhere in the SUT path).
- `tests/public_surface_audit.rs` — single test naming all 20 FROZEN types; coerces EmptyReader to `&dyn Reader<…>` at the audit site.
- `crates/miner-reader-dukascopy/tests/reader_trait_object_safety.rs` — single test proving both `&dyn` and `Box<dyn>` coercion.
- `VALIDATION.md` — all 14 Wave 0 checkboxes ticked; all 34 verification-map rows green; Sign-Off + Approval re-issued.

## Issues Encountered

None beyond the documented auto-fixes above. No upstream blocker, no environmental issue, no test flake.

## Deferred Items

- The `deferred-items.md` file documents pre-existing fmt drift from Plan 02-03 that Plan 02-06 has now resolved. The file itself is unchanged in this plan (it documents history, not pending work) — Phase 3 can prune it if desired, but the items it lists are all closed as of `5b33c1a`.

## Self-Check: PASSED

- [x] `crates/miner-core/tests/full_determinism.rs` exists with 2 tests `two_runs_byte_identical` + `two_runs_byte_identical_three_timeframes`.
- [x] `crates/miner-core/tests/public_surface_audit.rs` exists with 1 test `phase_2_public_surface_present`.
- [x] `crates/miner-reader-dukascopy/tests/reader_trait_object_safety.rs` exists with 1 test `dukascopy_reader_is_dyn_compatible`.
- [x] `crates/miner-core/Cargo.toml` `[dev-dependencies]` contains `miner-reader-dukascopy = { path = "../miner-reader-dukascopy" }` AND `zstd.workspace = true`.
- [x] `cargo test -p miner-core --test full_determinism` exits 0 (2 tests green).
- [x] `cargo test -p miner-core --test public_surface_audit phase_2_public_surface_present` exits 0.
- [x] `cargo test -p miner-reader-dukascopy --test reader_trait_object_safety dukascopy_reader_is_dyn_compatible` exits 0.
- [x] `cargo test --workspace` green across all 5 prior plans + this plan.
- [x] `cargo clippy --workspace --all-targets -- -D warnings` exits 0.
- [x] `cargo fmt --all --check` exits 0.
- [x] `cargo tree -p miner-core --edges normal,build | grep -E '(tokio|async-std|async-trait)'` empty (FOUND-04 gate intact).
- [x] `grep -c 'pub use aggregator::' crates/miner-core/src/lib.rs` returns 1.
- [x] `grep -c 'pub use aggregate::' crates/miner-core/src/lib.rs` returns 0 (B1 rename intact).
- [x] All 20 Phase 2 FROZEN public names appear in `crates/miner-core/src/lib.rs`'s `pub use` block: Reader, RawBar, Side, ClosedRangeUtc, Blake3Hex, Calendar, Timeframe, AggParams, BarFrame, AggregateError, aggregate, AGGREGATOR_VERSION, BarCache, CacheError, ARROW_SCHEMA_VERSION, FingerprintSidecar, build_arrow_schema, GapDetector, GapManifest, GapSpan, GapReason.
- [x] `grep -c 'TBD | TBD' .planning/phases/02-reader-aggregator-derived-bar-cache/02-VALIDATION.md` returns 0.
- [x] `grep -c 'aggregate::tests' .planning/phases/02-reader-aggregator-derived-bar-cache/02-VALIDATION.md` returns 0.
- [x] `grep -c 'aggregator::tests' .planning/phases/02-reader-aggregator-derived-bar-cache/02-VALIDATION.md` returns 6 (>= 4).
- [x] `grep -c 'nyquist_compliant: true' .planning/phases/02-reader-aggregator-derived-bar-cache/02-VALIDATION.md` returns 2 (frontmatter + sign-off).
- [x] `grep -c 'wave_0_complete: true' .planning/phases/02-reader-aggregator-derived-bar-cache/02-VALIDATION.md` returns 1.
- [x] CACHE-NN Coverage Map (footer of VALIDATION.md) intact: all 8 CACHE-NN requirements CLOSED.
- [x] `grep -c tick_count crates/miner-core/src/ crates/miner-reader-dukascopy/src/ --include='*.rs' -r` returns 0 (entire Phase 2 codebase A1-rename clean).
- [x] All 3 task commits in git log: `5b33c1a`, `efbfdea`, `8de6af4`.

## Phase 2 Close-out

This summary marks Phase 2 COMPLETE. The Phase 2 success criteria from ROADMAP.md / Phase 2 plan frontmatter:

1. **`cargo test --workspace` green across all 5 prior plans + this plan** — ✅
2. **Byte-identical Arrow + sidecar across two pipeline runs (real DukascopyReader)** — ✅ (`full_determinism.rs::two_runs_byte_identical`)
3. **Standalone DukascopyReader dyn-compat integration test** — ✅ (`reader_trait_object_safety.rs::dukascopy_reader_is_dyn_compatible`)
4. **Public surface audit gates the 20-name FROZEN re-export block** — ✅ (`public_surface_audit.rs::phase_2_public_surface_present`)
5. **VALIDATION.md per-task verification map has no TBD rows AND `nyquist_compliant: true`** — ✅
6. **`cargo clippy --workspace --all-targets -- -D warnings` exits 0** — ✅
7. **`cargo tree -p miner-core` shows zero tokio/async-std/async-trait** — ✅
8. **CACHE-NN Coverage Map shows all 8 requirements CLOSED** — ✅
9. **`grep -c tick_count crates/miner-{core,reader-dukascopy}/src --include='*.rs' -r` returns 0** — ✅

## Next Phase Readiness

**Phase 2 complete.** Phase 3 (scan engine + facade) can now start:

- **Reader trait + DukascopyReader** are reachable via `use miner_core::Reader;` + `use miner_reader_dukascopy::DukascopyReader;` — both dyn-compatible.
- **`BarCache::get_or_build`** is the cache-aware entry point Phase 3's scan engine will wrap to materialize bar frames on demand.
- **`GapDetector::detect`** + `GapManifest` are the data shape Phase 3's gap-policy enforcer wraps into a `Finding::GapAborted` envelope under `--gap-policy=strict`.
- **`aggregate(reader, params)`** is the pure-function kernel for any scan that needs raw 15m/1h/1d bars without going through the cache.
- **Calendar::fx_major + `is_open_at`** is the closed-form predicate the scan engine will call to bound queries to open hours.
- **Byte-identity gate** (`full_determinism.rs`) is the regression-detection backbone any Phase 3+ refactor of these components must keep green.

No blockers. No follow-ups requested. The dev-dep cycle accepted in Plan 02-06 is dev-only and does not affect the published API graph; Phase 3 inherits the SAME runtime dependency graph as Phase 2 (one-way: miner-core ← miner-reader-dukascopy ← wrappers).

---
*Phase: 02-reader-aggregator-derived-bar-cache*
*Plan: 06*
*Completed: 2026-05-18*
