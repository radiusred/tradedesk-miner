---
phase: 04-scan-catalogue-anom-cross-seas
plan: 02
subsystem: engine + cli + primitives

tags:
  - rust
  - engine
  - cli
  - primitives
  - facade
  - registry

requires:
  - phase: 04-scan-catalogue-anom-cross-seas
    plan: 01
    provides: "ScanArity enum + Scan::arity() trait method, InstrumentSpec struct, ScanRequest.instruments Vec, DataSlice.sources Vec, PreflightCode::WrongInstrumentArity, ndarray/ndarray-stats/nalgebra workspace deps"

provides:
  - "scan::primitives::returns::{log_returns, simple_returns, intraday_returns, overnight_returns} kernels (ANOM-01 surface; log_returns is a BYTE-IDENTICAL move of the Phase 3 ljung_box::kernel::log_returns body per D4-06 / Pitfall 9)"
  - "scan::primitives::time_alignment::inner_join(&BarFrame, &BarFrame) -> AlignedPair (CROSS-01 primitive; RESEARCH §1.6 body)"
  - "scan::primitives::time_alignment::intersect_gaps(&GapManifest, &GapManifest) -> GapManifest (D4-04 helper; PATTERNS.md Pattern I home decision: co-located with the inner-join primitive)"
  - "scan::primitives::raw_array::f64_slice_to_raw_array(&[f64]) -> RawArray (lifted from inline ljung_box helper; 22 scans share one copy)"
  - "engine::preflight::validate_arity(scan, &instruments) -> Result<(), WireError> with PreflightCode::WrongInstrumentArity + context (scan_id, expected_arity, supplied_arity)"
  - "engine::run_one_with_registry visibility widened from pub(crate) to pub so integration tests can inject per-test stub registries (Rule 3 deviation — plan needed Pair-scan stubbing)"
  - "engine::gap_policy::dispatch_pair(&manifest_a, &manifest_b, requested, policy) -> GapDispatch — Two-leg dispatch helper calling primitives::time_alignment::intersect_gaps then defers to existing dispatch (PATTERNS.md Pattern I)"
  - "ScanCtx.bars_pair: Option<(&BarFrame, &BarFrame)> + ctx.bars_pair() accessor — Pair-arity borrow surface for CROSS scans (Plan 04-07 will populate the engine's Pair branch)"
  - "ScanCtx::bars_up_to(ts) -> BarFrameView<'a> — look-ahead-safety enforcement API per RESEARCH §1.5 / PATTERNS Pattern L. partition_point(|t| *t <= ts) cutoff (inclusive)"
  - "BarFrameView<'a> — public struct with borrowed slice columns (source_id / symbol / side / tf / ts_open_utc / open / high / low / close / tick_volume) + len() / is_empty() helpers"
  - "scan::{anom, cross, seas}::register_<family>_scans(&mut Registry) — per-family registrar pattern (PATTERNS.md Pattern E). registry::bootstrap() invokes all three; Plans 04-03..04-10 append `r.register(...)` lines INSIDE the family helpers only (registry.rs is locked)"
  - "CLI repeatable `--instrument SYMBOL:side` flag (Pattern K) + scan_args::parse_instrument_spec value-parser"
  - "miner scans catalogue JSONL gains an `arity` field per scan (\"single\" / \"pair\")"
  - "schemas/scans-catalogue-v1.schema.json regenerated with the new `arity` field"
  - "crates/miner-core/tests/goldens/{REFERENCE-VERSIONS.md, python-requirements.lock, .gitkeep} — Phase 4 golden regeneration scaffolding pinning statsmodels 0.14.6, scipy 1.14.1, arch 7.2.0, numpy 1.26.4, python 3.11.x"
  - "Three integration-test scaffolds: tests/{arity_preflight.rs, two_leg_facade.rs, gap_intersect_cross.rs} — Plan 04-07 extends them with real CROSS scan registrations"

affects:
  - "04-03 (ANOM scans): consumes primitives::returns::{log_returns,simple_returns} + register_anom_scans helper"
  - "04-04 (more ANOM scans): same primitives + helper"
  - "04-05 (ANOM-04+ stationarity tests): primitives + helper"
  - "04-06 (ANOM-08+ heteroscedasticity / normality / drawdown): primitives + helper"
  - "04-07 (CROSS scans): primitives::time_alignment::{inner_join, intersect_gaps} + ctx.bars_pair() + ScanArity::Pair + register_cross_scans + engine::gap_policy::dispatch_pair (Plan 04-07 wires Pair branch into run_one_with_registry)"
  - "04-08 (SEAS scans): primitives::returns::{intraday_returns, overnight_returns} + register_seas_scans"
  - "04-09 (more SEAS scans): primitives + register helper"
  - "04-10 (SEAS event-window): primitives + register helper"
  - "04-11 (Phase-end integration): registry::bootstrap() count assertion updated to 23 (LjungBox + 22), full shuffled-future regression proptest over rolling/causal scans pins look-ahead-safety via ctx.bars_up_to()"
  - "Phase 6 MCP/HTTP wrappers: catalogue `arity` field rendered as a typed parameter surface; repeatable --instrument SYMBOL:side flag mirrored on the HTTP request payload"

tech-stack:
  added:
    - "(none) — Phase 4 deps were added in Plan 04-01; this plan only consumes them"
  patterns:
    - "Pattern A — Imports block + helper-lift (ljung_box/mod.rs imports `scan::primitives::returns::log_returns` + `scan::primitives::raw_array::f64_slice_to_raw_array`; the inline copies are deleted)"
    - "Pattern B — Kernel split + #[inline] pub fn discipline (primitives::returns / primitives::time_alignment / primitives::raw_array)"
    - "Pattern E — Per-family registrar pattern (scan::{anom,cross,seas}::register_<family>_scans + registry::bootstrap() invokes all three; Plans 04-03..04-10 append INSIDE family helpers only)"
    - "Pattern H — engine::preflight::validate_arity helper verbatim per PATTERNS.md body"
    - "Pattern I — Two-leg gap intersection home in primitives::time_alignment (CROSS-01 owns it); engine::gap_policy::dispatch_pair sibling defers to existing dispatch"
    - "Pattern K — CLI repeatable --instrument SYMBOL:side flag (clap ArgAction::Append + value_parser = parse_instrument_spec)"
    - "Pattern L — ScanCtx::bars_up_to(ts) -> BarFrameView look-ahead-safety API"

key-files:
  created:
    - "crates/miner-core/src/scan/primitives/mod.rs"
    - "crates/miner-core/src/scan/primitives/returns.rs (192 lines, 13 tests)"
    - "crates/miner-core/src/scan/primitives/time_alignment.rs (296 lines, 10 tests)"
    - "crates/miner-core/src/scan/primitives/raw_array.rs (60 lines, 2 tests)"
    - "crates/miner-core/src/scan/anom/mod.rs (44 lines, 1 test — empty register_anom_scans helper)"
    - "crates/miner-core/src/scan/cross/mod.rs (39 lines, 1 test — empty register_cross_scans helper)"
    - "crates/miner-core/src/scan/seas/mod.rs (39 lines, 1 test — empty register_seas_scans helper)"
    - "crates/miner-core/tests/arity_preflight.rs (180 lines, 2 tests)"
    - "crates/miner-core/tests/two_leg_facade.rs (95 lines, 2 tests)"
    - "crates/miner-core/tests/gap_intersect_cross.rs (130 lines, 4 tests)"
    - "crates/miner-core/tests/goldens/REFERENCE-VERSIONS.md (76 lines)"
    - "crates/miner-core/tests/goldens/python-requirements.lock (33 lines)"
    - "crates/miner-core/tests/goldens/.gitkeep"
  modified:
    - "crates/miner-core/src/scan/mod.rs — pub mod {anom, cross, primitives, seas}; ScanCtx gains bars_pair field + bars_up_to() method + bars_pair() accessor; BarFrameView struct; scan_ctx_bars_up_to_partitions_at_cutoff test"
    - "crates/miner-core/src/scan/ljung_box/mod.rs — imports primitives::returns::log_returns + primitives::raw_array::f64_slice_to_raw_array; inline helper bodies deleted; make_ctx fixture updated for bars_pair"
    - "crates/miner-core/src/scan/ljung_box/kernel.rs — log_returns body moved (Pitfall 9); local tests pointer to primitives module"
    - "crates/miner-core/src/scan/registry.rs — bootstrap() invokes register_{anom,cross,seas}_scans; bootstrap_invokes_all_three_family_registrars regression test"
    - "crates/miner-core/src/engine/preflight.rs — validate_arity helper + 4 unit tests"
    - "crates/miner-core/src/engine/gap_policy.rs — dispatch_pair sibling function"
    - "crates/miner-core/src/engine/mod.rs — run_one_with_registry visibility widened to pub; validate_arity wired between resolve_scan and parse_params; make_scan_ctx extended with bars_pair parameter; single-leg path passes None"
    - "crates/miner-core/tests/scan_ljung_box.rs — ScanCtx fixture updated for bars_pair: None"
    - "crates/miner-core/tests/shuffled_future_regression.rs — same update"
    - "crates/miner-cli/src/scan_args.rs — instruments: Vec<InstrumentSpec> replaces instrument: String + side: String; parse_instrument_spec value-parser; 5 new tests"
    - "crates/miner-cli/src/main.rs — handle_scans_subcommand emits arity field; handle_scan_subcommand_forwards_sleep_hook test updated"
    - "crates/miner-cli/src/cli.rs — scan_args_defaults_per_d3_19 test drops removed --side default"
    - "crates/miner-cli/tests/scans_catalogue.rs — asserts arity field present + value \"single\""
    - "crates/miner-cli/tests/scan_subcommand_smoke.rs — every --instrument EURUSD migrated to EURUSD:bid; invalid_params test swapped to --gap-policy lax"
    - "crates/miner-cli/tests/cancel_overrides_error_exit_130.rs — :bid migration"
    - "crates/miner-cli/tests/sigint_preserves_stream.rs — :bid migration"
    - "xtask/src/main.rs — ScansCatalogueEntry gains `arity: String` field for schema regen"
    - "schemas/scans-catalogue-v1.schema.json — regenerated with `arity` required field"

key-decisions:
  - "D4-06 / Pitfall 9 invariant pinned via byte-exact f64::to_bits() comparison in primitives::returns::tests::log_returns_matches_ljung_box_kernel — not 1e-12 tolerance"
  - "primitives::time_alignment is the home for both inner_join AND intersect_gaps (PATTERNS.md Pattern I option (a) — both helpers owned by CROSS-01)"
  - "intersect_gaps picks the conservative GapReason on overlap (MissingSourceFile > CorruptSourceFile > IntraDayGap per discriminant_ord) — avoids adding a GapReason::EitherLeg variant"
  - "ScanCtx.bars stays a single-frame borrow for Pair scans (points at leg A as a default anchor); CROSS scan bodies MUST access leg B via ctx.bars_pair() — no implicit cast"
  - "BarFrameView is its own struct (not a BarFrame trait) — explicit slice borrows make the look-ahead-safety contract structural; impossible for a kernel to accidentally index past the cutoff"
  - "engine::run_one_with_registry widened from pub(crate) to pub — Rule 3 deviation (plan baseline assumed the function was reachable from integration tests; widening was necessary to land arity_preflight.rs / two_leg_facade.rs / gap_intersect_cross.rs without spawning the CLI binary)"
  - "engine::gap_policy::dispatch_pair is a sibling helper (option (b) in PATTERNS.md Pattern I) — minimal blast radius on Phase 3 tests vs extending the existing dispatch enum"
  - "Per-family registrar contract (PATTERNS.md Pattern E) locks registry::bootstrap() — Plans 04-03..04-10 append `r.register(...)` INSIDE the family's helper only; Plan 04-11 updates the count assertion in registry tests"
  - "CLI `--side` flag REMOVED (no sugar / clean break per CONTEXT.md D4-02). Side travels inside `--instrument SYMBOL:side`"
  - "ScansCatalogueEntry shim in xtask gains `arity: String` (not the typed ScanArity enum) — schemars serialises the wire-form string directly; matches the existing pattern for other shim fields"
  - "primitives::returns::{intraday,overnight}_returns use UTC date partitioning via Datelike::num_days_from_ce() — timezone-neutral; callers responsible for any session-local shifts (SEAS family will compose with calendar.rs)"

requirements-completed:
  - ANOM-01
  - CROSS-01

# Metrics
duration: 22min
completed: 2026-05-19
---

# Phase 04 Plan 02: Engine + CLI + Primitives Wave Summary

**Wave 2 wired the Phase 4 D4-02 / D4-04 / D4-06 facade extensions: returns + time-alignment + raw-array primitives, arity preflight, two-leg gap dispatch, ScanCtx.bars_pair + bars_up_to(ts) look-ahead-safety API, per-family registrar stubs, repeatable `--instrument SYMBOL:side` CLI flag, and the `arity` field on the `miner scans` catalogue. The 22-scan rollout in Waves 3-7 now consumes a stable, self-contained primitives + registrar surface.**

## Performance

- **Duration:** ~22 min
- **Started:** 2026-05-19T23:09:27Z
- **Completed:** 2026-05-19T23:31:28Z
- **Tasks:** 4 of 4 (all autonomous)
- **Files created:** 13
- **Files modified:** 18
- **Lines added (commit diff sum):** ~2,150
- **New tests:** 24 lib unit tests + 8 integration tests (across 3 new scaffold files)

## Accomplishments

- **D4-06 byte-identical lift (Pitfall 9):** `primitives::returns::log_returns` is the BYTE-EXACT move of `ljung_box::kernel::log_returns` (compared via `f64::to_bits()` equality, tolerance 0.0). The Phase 3 statsmodels golden test (`crates/miner-core/tests/scan_ljung_box.rs`) continues to pass byte-identically after LjungBoxScan was refactored to call the primitive — the regression guard is the existing golden, no math regen needed.
- **ANOM-01 primitives kernel surface:** `primitives::returns::{log_returns, simple_returns, intraday_returns, overnight_returns}` — every kernel is `#[inline] #[must_use] pub fn` over `&[f64]` (the timestamp slice for intraday/overnight). The 22 Phase 4 scans share one canonical returns implementation.
- **CROSS-01 primitive:** `primitives::time_alignment::inner_join(&BarFrame, &BarFrame) -> AlignedPair` — two-pointer sweep over `ts_open_utc` per RESEARCH.md §1.6 body, returns an `AlignedPair` with parallel `timestamps_ms` (epoch-ms) / `close_a` / `close_b` vectors.
- **D4-04 manifest intersection:** `primitives::time_alignment::intersect_gaps(&GapManifest, &GapManifest) -> GapManifest` — sweep-and-merge over sorted spans, picks the more conservative `GapReason` on overlap (`MissingSourceFile > CorruptSourceFile > IntraDayGap` per `discriminant_ord`). PATTERNS.md Pattern I home decision: co-located with `inner_join` (CROSS-01 owns both helpers).
- **Helper lift:** `primitives::raw_array::f64_slice_to_raw_array(&[f64]) -> RawArray` — the inline LjungBox helper moved here once; 22 scans share the byte-layout discipline.
- **D4-02 arity preflight:** `engine::preflight::validate_arity(scan, &instruments)` — rejects mismatched arity with `PreflightCode::WrongInstrumentArity` + context (`scan_id`, `expected_arity`, `supplied_arity`). Wired into `run_one_with_registry` between `resolve_scan` and `parse_params` per RESEARCH §1.10 ordering.
- **Per-family registrar contract (Pattern E):** `scan::{anom,cross,seas}::register_<family>_scans(&mut Registry)` — empty helpers in Plan 04-02; Plans 04-03..04-10 append `r.register(...)` lines INSIDE the family helper alphabetical by scan-id. `registry::bootstrap()` invokes all three; this is the LAST modification to `bootstrap()` in Phase 4.
- **ScanCtx two-leg + look-ahead-safety API:** `ScanCtx.bars_pair: Option<(&BarFrame, &BarFrame)>` + `ctx.bars_pair()` accessor; `ctx.bars_up_to(ts) -> BarFrameView<'a>` with `partition_point(|t| *t <= ts)` cutoff (inclusive upper bound). `BarFrameView` is a new public struct with borrowed slice columns + `len()` / `is_empty()`.
- **D4-04 two-leg gap-policy dispatch:** `engine::gap_policy::dispatch_pair(&manifest_a, &manifest_b, requested, policy)` — calls `intersect_gaps` then defers to the existing `dispatch` (PATTERNS.md Pattern I option (b) — sibling helper, minimal Phase 3 blast radius).
- **D4-02 CLI surface (Pattern K):** `--instrument SYMBOL:side` is now a repeatable clap flag with `parse_instrument_spec` value-parser. The legacy `--side bid|ask` flag is REMOVED (clean break per CONTEXT.md D4-02). Single-leg scans pass it once; CROSS scans pass it twice.
- **`miner scans` catalogue arity:** each emitted JSONL line gains an `arity` field ("single" / "pair") via `scan.arity().as_str()`. The `xtask/ScansCatalogueEntry` shim gained the field and `schemas/scans-catalogue-v1.schema.json` was regenerated.
- **Goldens scaffolding:** `crates/miner-core/tests/goldens/REFERENCE-VERSIONS.md` pins statsmodels 0.14.6 (Phase 3 carryover), scipy 1.14.1, arch 7.2.0, numpy 1.26.4, pandas 2.2.3, python 3.11.x; `python-requirements.lock` is the regen input only — never executed by Rust builds or CI.
- **3 integration-test scaffolds:** `tests/arity_preflight.rs` (2 tests), `tests/two_leg_facade.rs` (2 tests), `tests/gap_intersect_cross.rs` (4 tests). Plan 04-07 will extend `two_leg_facade.rs` once the engine's Pair branch is wired.

## Task Commits

1. **Task 1a: Primitives namespace + LjungBoxScan D4-06 refactor** — `eb3f0fb` (feat)
2. **Task 1b: Per-family registrar stubs + goldens scaffolding** — `7b582e9` (feat)
3. **Task 2: Engine arity preflight + bars_pair + bars_up_to + dispatch_pair** — `a7d1860` (feat)
4. **Task 3: CLI repeatable --instrument SYMBOL:side + scans arity field** — `2578577` (feat)

## Files Created / Modified

See the `key-files` frontmatter section above for the exhaustive list. Headline counts:

**Created (13 files, ~1,400 lines):**
- 4 primitives files (mod / returns / time_alignment / raw_array)
- 3 family-namespace stubs (anom / cross / seas)
- 3 integration-test scaffolds (arity_preflight / two_leg_facade / gap_intersect_cross)
- 3 goldens scaffolding files (REFERENCE-VERSIONS.md / python-requirements.lock / .gitkeep)

**Modified (18 files):**
- 5 miner-core src files (scan/mod.rs, scan/ljung_box/{mod.rs,kernel.rs}, scan/registry.rs, engine/{preflight,gap_policy,mod}.rs)
- 2 miner-core test files updated for the new `ScanCtx.bars_pair` field
- 7 miner-cli files (3 src + 4 test files updated for the new `--instrument SYMBOL:side` form)
- 1 xtask src file (ScansCatalogueEntry gains `arity`)
- 1 regenerated schema

## Decisions Made

- **D4-06 byte-identical-move regression guard uses `f64::to_bits()` equality** — tolerance 0.0 (NOT 1e-12). The byte-equality test (`log_returns_matches_ljung_box_kernel`) compares the lifted primitive's output against a local copy of the Phase 3 body via `to_bits()` so any IEEE-754 bit-level drift fails the build.
- **`primitives::time_alignment` is the home for BOTH `inner_join` AND `intersect_gaps`** — PATTERNS.md Pattern I option (a). Co-located with the CROSS-01 surface that consumes both.
- **`intersect_gaps` picks conservative `GapReason` on overlap** — avoids introducing a new `GapReason::EitherLeg` variant; reuses the existing 3-variant enum (`MissingSourceFile > CorruptSourceFile > IntraDayGap` per `discriminant_ord`).
- **`ScanCtx.bars` is still a single-leg borrow under `Pair` arity** — points at leg A as a default anchor. CROSS scan bodies access leg B via `ctx.bars_pair()` explicitly. This keeps Phase 3 ANOM/SEAS scan bodies unchanged.
- **`BarFrameView` is its own public struct, not a trait or method on `BarFrame`** — explicit slice borrows make the look-ahead-safety guarantee structural; impossible for a downstream kernel to bypass the `partition_point` cutoff.
- **`engine::gap_policy::dispatch_pair` is a sibling helper (option b)** — minimal blast radius on Phase 3 tests vs extending the `GapDispatch` enum.
- **CLI `--side` flag REMOVED (no sugar)** — per CONTEXT.md D4-02 "clean break". Side now travels inside `--instrument SYMBOL:side`.
- **`ScansCatalogueEntry.arity` is `String` (not the typed `ScanArity` enum)** — schemars serialises the wire-form string directly via the existing pattern for other shim fields.
- **`primitives::returns::{intraday,overnight}_returns` partition by UTC date** — timezone-neutral; SEAS scans compose with `calendar.rs` for session-local shifts.

## Deviations from Plan

### Rule 3 (Blocking-issue auto-fix) — 1 instance

**1. [Rule 3 - Plan signature mismatch] `engine::run_one_with_registry` widened from `pub(crate)` to `pub`**

- **Found during:** Task 2 (integration-test scaffold authoring).
- **Issue:** The plan's Task 2 acceptance criterion specifies "uses `engine::run_one_with_registry`" from `tests/arity_preflight.rs` / `tests/two_leg_facade.rs` / `tests/gap_intersect_cross.rs`. The function was `pub(crate)` — unreachable from integration tests (which compile as separate crates). The alternatives — (a) running tests as in-crate unit tests inside the engine module's `mod tests` block, or (b) spawning the CLI binary via `assert_cmd` — both diverge from the plan's stated pattern analog (`tests/gap_policy.rs` and `tests/dry_run.rs` both call public engine functions directly).
- **Fix:** Widened the visibility of `run_one_with_registry` from `pub(crate)` to `pub`. The function already accepts a borrowed `&Registry` so its API is suitable for external callers. Documented the visibility change in the doc-comment + added `# Errors` / `# Panics` sections to satisfy clippy's missing-docs lints (which surfaced once the function joined the public API).
- **Files modified:** `crates/miner-core/src/engine/mod.rs` (`pub(crate)` → `pub`).
- **Verification:** All three integration scaffolds compile and pass; existing engine `mod tests` continues to use the same function with no signature change.
- **Committed in:** `a7d1860` (Task 2 commit).

### Rule 3 (Blocking-issue auto-fix) — Test-coverage cascade

**2. [Rule 3 - CLI test cascade after `--side` removal]**

- **Found during:** Task 3 (CLI surface migration).
- **Issue:** The plan's Task 3 acceptance criterion specifies the legacy `--side bid|ask` flag is REMOVED (no sugar / clean break per CONTEXT.md D4-02). This cascades to every CLI integration test that uses the old `--instrument EURUSD --side bid` form: `tests/scan_subcommand_smoke.rs` (5 test invocations), `tests/sigint_preserves_stream.rs`, `tests/cancel_overrides_error_exit_130.rs`, `tests/scans_catalogue.rs`, `src/cli.rs::tests::scan_args_defaults_per_d3_19`, `src/main.rs::tests::handle_scan_subcommand_forwards_sleep_hook_to_scan_request`, and a sub-test that used `--side middle` as the `invalid_parameter` boundary trigger.
- **Fix:** Migrated every `--instrument EURUSD` to `--instrument EURUSD:bid`; removed every `--side bid` / `--side ...` arg. The boundary-error trigger in `invalid_params_emits_wireerror_exit_1` was swapped from `--side middle` to `--gap-policy lax` (same `invalid_parameter` classification path through preflight). The `cli::tests::scan_args_defaults_per_d3_19` assertion on `args.side == "bid"` was dropped (replaced with an `args.instruments` len/symbol assertion).
- **Files modified:** `crates/miner-cli/src/{cli.rs, main.rs}` + 4 test files.
- **Verification:** Full `cargo test --workspace` GREEN.
- **Committed in:** `2578577` (Task 3 commit).

### Plan deviation summary

**Total deviations:** 2 (both Rule 3 — blocking-issue resolutions). No scope creep — every deviation was a required adaptation to land the plan's user-locked D4-02 contract (the `--side` removal + the integration-test scaffold visibility). No new clippy lints introduced.

## Issues Encountered

- **Clippy `missing_docs_in_private_items` cascade after widening `run_one_with_registry`'s visibility** — the function's existing doc-comment did not declare `# Errors` / `# Panics` sections because `pub(crate)` items don't trigger the lint. After the `pub` widening, both sections had to be added. Added them inline with content matching the existing public `run_one` doc-comment style.
- **`BarFrameView<'a>` lifetime-elision lint** — clippy flagged `impl<'a> BarFrameView<'a>` as redundant; switched to `impl BarFrameView<'_>`.
- **The schema regen via `cargo run -p xtask -- gen-schema` requires a clean rebuild after the `ScansCatalogueEntry` field addition** — the regen command rebuilt `xtask` then wrote the new `arity` field into `schemas/scans-catalogue-v1.schema.json` deterministically. The Phase 1 schema idempotency invariant (two consecutive runs produce byte-identical output) holds — verified by running the command twice; the second run produced no diff (`git diff --stat` exits 0).
- **Existing `scan_args::tests::scan_args_invalid_side` test name no longer matches its behaviour** — renamed to `scan_args_invalid_side_in_instrument` and updated to assert that clap rejects `--instrument EURUSD:middle` at parse time (the value-parser path) instead of expecting `to_scan_request` to reject `--side middle` (which no longer exists).

## User Setup Required

None — no new external dependencies, env vars, secrets, or service configuration. The goldens regeneration recipe in `crates/miner-core/tests/goldens/REFERENCE-VERSIONS.md` is documentation-only; Plan 04-03 (the first plan that ships a Python-generated golden) will produce the first real `python-requirements.lock` via `pip freeze`.

## Next Phase Readiness

**Plan 04-03 (Wave 3 — ANOM scans) unblocked.** All four user-locked Phase 4 decisions in 04-CONTEXT.md now have their full facade-shape prerequisites:

- D4-01: `ScanRequest.instruments: Vec<InstrumentSpec>` + CLI `--instrument SYMBOL:side` form ✓ (Plan 04-01 + Plan 04-02)
- D4-02: `Scan::arity()` + `ScanArity { Single, Pair }` + `PreflightCode::WrongInstrumentArity` + `engine::preflight::validate_arity` wired into the facade ✓ (Plan 04-01 + Plan 04-02)
- D4-03: `DataSlice.sources: Vec<Source>` ✓ (Plan 04-01)
- D4-04: `engine::gap_policy::dispatch_pair` + `primitives::time_alignment::intersect_gaps` ✓ (Plan 04-02)

**Plan 04-03 / 04-04 / 04-05 / 04-06 entry points (ANOM scans):**
- Append `r.register(Box::new(<NewScan>))` lines INSIDE `scan::anom::register_anom_scans` alphabetical by scan-id.
- Each scan implements `Scan::arity() -> ScanArity::Single` explicitly (no default).
- Returns kernels: `use crate::scan::primitives::returns::{log_returns, simple_returns, intraday_returns, overnight_returns};`
- Helper: `use crate::scan::primitives::raw_array::f64_slice_to_raw_array;`
- Rolling/causal scans MUST consume `ctx.bars_up_to(ts)` (not raw `ctx.bars` slicing) — Plan 04-11 pins the invariant with a shuffled-future proptest.

**Plan 04-07 (CROSS scans) entry points:**
- Append `r.register(Box::new(<NewScan>))` lines INSIDE `scan::cross::register_cross_scans`.
- Each scan implements `Scan::arity() -> ScanArity::Pair`.
- Scan body: `let (a, b) = ctx.bars_pair().expect("Pair"); let aligned = primitives::time_alignment::inner_join(a, b);`
- The engine's Pair branch in `run_one_with_registry` is NOT yet wired — Plan 04-07 lands it (fetches both legs, computes per-leg manifests, calls `dispatch_pair`, populates `ScanCtx.bars_pair`).

**Plan 04-08 / 04-09 / 04-10 (SEAS scans) entry points:**
- Append inside `scan::seas::register_seas_scans`.
- Single arity; intraday/overnight kernels for date-bucketed surfaces; `Calendar` consumer for session-local scans (SEAS-03).

**No blockers.** The 22-scan rollout has a stable, fully-tested primitives + registrar + CLI surface.

## Threat Flags

(No section emitted — every change was anticipated by the plan's threat model; no NEW security-relevant surface was introduced beyond what the plan's `<threat_model>` block already classifies.)

## Self-Check: PASSED

Verified:
- [x] `crates/miner-core/src/scan/primitives/{mod,returns,time_alignment,raw_array}.rs` exist (`ls` confirms).
- [x] `pub fn log_returns` present in `primitives/returns.rs` (1 match).
- [x] `pub fn log_returns` ABSENT from `ljung_box/kernel.rs` body (grep verified the function literal is gone; the comment-only marker remains).
- [x] `use crate::scan::primitives::returns::log_returns` present in `ljung_box/mod.rs` (1 match).
- [x] `pub fn inner_join` + `pub fn intersect_gaps` present in `primitives/time_alignment.rs` (1 match each).
- [x] `pub fn f64_slice_to_raw_array` present in `primitives/raw_array.rs` (1 match).
- [x] `pub fn validate_arity` present in `engine/preflight.rs` (1 match).
- [x] `validate_arity` call site in `engine/mod.rs::run_one_with_registry` (1 match).
- [x] `pub fn bars_up_to` + `pub struct BarFrameView` present in `scan/mod.rs`.
- [x] `pub fn dispatch_pair` present in `engine/gap_policy.rs`.
- [x] `pub struct StubPair` / `instruments: Vec<InstrumentSpec>` present in `arity_preflight.rs` + `scan_args.rs` (verified by inspection).
- [x] `ArgAction::Append` + `parse_instrument_spec` present in `scan_args.rs`.
- [x] `instruments: Vec<InstrumentSpec>` field present in `scan_args.rs::ScanArgs` (1 match).
- [x] `pub instrument: String` + `pub side: String` ABSENT from `scan_args.rs::ScanArgs`.
- [x] `crates/miner-core/tests/{arity_preflight,two_leg_facade,gap_intersect_cross}.rs` all exist.
- [x] `crates/miner-core/tests/goldens/REFERENCE-VERSIONS.md` exists (76 lines, ≥ 25).
- [x] `crates/miner-core/tests/goldens/REFERENCE-VERSIONS.md` mentions `statsmodels==0.14.6` and `scipy` (grep verified).
- [x] `crates/miner-core/tests/goldens/python-requirements.lock` exists.
- [x] `crates/miner-core/tests/goldens/.gitkeep` exists.
- [x] All 4 commits present in `git log` — `eb3f0fb`, `7b582e9`, `a7d1860`, `2578577`.
- [x] `cargo run -p miner-cli -- scans 2>/dev/null | head -1 | grep -q '"arity"' && echo PASS` → PASS.
- [x] `cargo test --workspace` workspace-wide GREEN (every test result line reads `test result: ok`, zero failures).
- [x] `cargo clippy --workspace --all-targets -- -D warnings` exits with ONLY the 2 pre-existing main-branch errors (`reader.rs:100`, `ljung_box/mod.rs:79`). Zero NEW lints introduced.

---
*Phase: 04-scan-catalogue-anom-cross-seas*
*Completed: 2026-05-19*
