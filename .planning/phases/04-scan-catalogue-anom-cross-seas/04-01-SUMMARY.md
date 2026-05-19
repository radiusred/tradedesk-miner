---
phase: 04-scan-catalogue-anom-cross-seas
plan: 01
subsystem: facade

tags:
  - rust
  - facade
  - schema
  - scan-trait
  - serde
  - schemars
  - ndarray
  - nalgebra

requires:
  - phase: 03-scan-engine-facade-cli
    provides: "Scan trait, ScanRequest, DataSlice, PreflightCode, BarFrame, engine::run_one facade, LjungBoxScan demo, schemars gen-schema pipeline"

provides:
  - "ScanArity enum (Single, Pair) with snake_case serde + as_str() + expected_len() helpers"
  - "Scan::arity() trait method (no default; every impl declares explicitly)"
  - "InstrumentSpec { symbol, side } struct in reader.rs with FromStr / Display / JsonSchema / Hash"
  - "ScanRequest.instruments: Vec<InstrumentSpec> (D4-01) replacing instrument: String + side: Side"
  - "DataSlice.sources: Vec<Source> (D4-03) — leg-labelled per-finding provenance; replaces source: Source on ResultFinding + GapAbortedFinding"
  - "PreflightCode::WrongInstrumentArity variant (D4-02; wire form: wrong_instrument_arity)"
  - "Three workspace deps: ndarray 0.16, ndarray-stats 0.6, nalgebra 0.33 (std-only) — wired into miner-core, sync-only invariant preserved"
  - "Regenerated schemas/findings-v1.schema.json reflecting the D4-03 structural change"
  - "Phase 4 schema-additive decision recorded in 04-01-SCHEMA-DIFF.md (D4-03 path; D4-03-ALT not needed)"

affects:
  - "04-02 (ANOM scans — depends on Scan::arity, InstrumentSpec, primitives namespace)"
  - "04-03 (CROSS scans — depends on Pair arity, DataSlice.sources Vec, ScanRequest.instruments len 2)"
  - "04-04 (SEAS scans — depends on Single arity)"
  - "04-05 (integration tests / goldens / arity catalogue output)"
  - "Phase 6 MCP/HTTP wrappers — must mirror the new instruments Vec + sources Vec wire form"

tech-stack:
  added:
    - "ndarray 0.16 (n-dim arrays, rolling-window kernels)"
    - "ndarray-stats 0.6 (mean/var/quantile/correlation)"
    - "nalgebra 0.33 (small fixed-size linear algebra; std-only feature set)"
  patterns:
    - "Pattern F (ScanArity enum) — extendable additively for v2 basket scans (Many(min, max))"
    - "Pattern G (PreflightCode variant extension) — new variant + as_str arm + test cases-array row"
    - "Pattern K (InstrumentSpec next to Side in reader.rs; FromStr parses CLI wire form SYMBOL:side)"
    - "D4-03 wire-form: per-finding leg provenance lives on DataSlice.sources Vec (parallel to ScanRequest.instruments order)"

key-files:
  created:
    - ".planning/phases/04-scan-catalogue-anom-cross-seas/04-01-SCHEMA-DIFF.md"
  modified:
    - "Cargo.toml (workspace deps)"
    - "crates/miner-core/Cargo.toml (mirrored deps)"
    - "crates/miner-core/src/reader.rs (InstrumentSpec struct)"
    - "crates/miner-core/src/scan/mod.rs (ScanArity enum, Scan::arity trait method, ScanRequest.instruments Vec)"
    - "crates/miner-core/src/findings/mod.rs (DataSlice.sources Vec; removed source field from ResultFinding/GapAbortedFinding)"
    - "crates/miner-core/src/error/codes.rs (PreflightCode::WrongInstrumentArity)"
    - "crates/miner-core/src/scan/ljung_box/mod.rs (arity() impl; envelope rewire)"
    - "crates/miner-core/src/engine/mod.rs (instruments[0] dispatch; sources Vec construction)"
    - "crates/miner-core/src/engine/framing.rs (RunStart.request.instruments Vec echo)"
    - "crates/miner-cli/src/scan_args.rs (single-leg Vec construction)"
    - "schemas/findings-v1.schema.json (regenerated; D4-03 shape)"
    - "crates/miner-core/tests/snapshots/scan_ljung_box__ljung_box_matches_statsmodels_golden.snap (envelope reshape; math byte-identical)"
    - "crates/miner-core/tests/{scan_ljung_box,scan_facade_determinism,dry_run,shuffled_future_regression,schema_roundtrip}.rs (fixture updates)"

key-decisions:
  - "D4-03 path chosen — full Vec<Source> generalisation on DataSlice; D4-03-ALT fallback (peer_sources sibling) rejected because the schemars diff is purely additive (#[serde(default)] keeps `required` array unchanged)"
  - "D4-01 ScanRequest.instruments Vec — zero schema impact (ScanRequest is not JsonSchema-derived; wire form lives in RunStart.request opaque Value)"
  - "schema_version stays = 1 per 04-RESEARCH §Section 7 — consumers haven't shipped, Phase 6 wrappers don't exist yet"
  - "InstrumentSpec lives in reader.rs (next to Side) per PATTERNS.md Pattern K — co-location with the Side enum it composes"
  - "ScanArity::Single + Pair only (2 variants) — defer Many(min, max) to v2 per 04-RESEARCH §1.10"
  - "Scan::arity() has NO default body — every impl declares explicitly (footgun-avoidance: a silent Single default would mis-classify CROSS scans)"
  - "Engine reads req.instruments[0] post-preflight; Plan 04-02's validate_arity will reject mismatched arity earlier"

patterns-established:
  - "ScanArity enum with snake_case serde + as_str() + expected_len() — analog: GapPolicyKind"
  - "InstrumentSpec::from_str(SYMBOL:side) — analog: Side::from_str / Timeframe::from_str"
  - "Per-finding leg provenance: DataSlice.sources: Vec<Source> parallel to req.instruments order"
  - "Engine constructs `sources: Vec<Source>` by iterating req.instruments — single helper-free closure (3 occurrences: ResultFinding via LjungBox, GapAbortedFinding, emit_scan_error)"

requirements-completed:
  - ANOM-01
  - CROSS-01

# Metrics
duration: 22min
completed: 2026-05-19
---

# Phase 04 Plan 01: Facade-shape Extension Summary

**Phase 4 facade-shape extension landed: Scan::arity() trait method, ScanArity enum, InstrumentSpec struct, ScanRequest.instruments Vec, DataSlice.sources Vec, PreflightCode::WrongInstrumentArity, and ndarray/ndarray-stats/nalgebra workspace deps — D4-01/02/03 in place ahead of the 22-scan rollout.**

## Performance

- **Duration:** 22 min
- **Started:** 2026-05-19T22:30:57Z
- **Completed:** 2026-05-19T22:53:27Z
- **Tasks:** 3 of 3 (all autonomous)
- **Files modified:** 13 source files + 1 schema + 1 snapshot + 1 new memo

## Accomplishments

- **D4-02 trait method:** `Scan::arity() -> ScanArity` (no default body) added between `version` and `param_schema` per RESEARCH §1.10 ordering; LjungBoxScan declares `ScanArity::Single`; object-safety regression gate (`scan_trait_object_safe`) continues to pass.
- **D4-01 instruments Vec:** `ScanRequest.instrument: String + side: Side` generalised to `instruments: Vec<InstrumentSpec>`; the new typed `InstrumentSpec { symbol, side }` struct lives next to `Side` in `reader.rs` (Pattern K) with `Display` + `from_str("SYMBOL:side")` for Plan 04-02's CLI value-parser.
- **D4-03 sources Vec:** `DataSlice` gains `sources: Vec<Source>` with `#[serde(default)]` (additive at the schemars level — the `required` array stays unchanged); the previous singleton `source: Source` field on `ResultFinding` and `GapAbortedFinding` is removed, and per-finding leg provenance now lives on `DataSlice` (self-describing for the Quant agent).
- **PreflightCode extension:** New `WrongInstrumentArity` variant with snake_case wire form `wrong_instrument_arity` (D4-02); the existing 7-variant test case-array gains one row.
- **Workspace deps:** `ndarray = "0.16"`, `ndarray-stats = "0.6"`, `nalgebra = "0.33"` (std-only features) added to the workspace and mirrored into `miner-core`; FOUND-04 sync-only invariant preserved on the lib graph (`cargo tree -p miner-core --edges normal | grep -cE 'tokio|async-std'` returns 0).
- **Schema regen + idempotency:** `schemas/findings-v1.schema.json` regenerated to reflect the D4-03 structural change; two consecutive `cargo run -p xtask -- gen-schema` runs against `/tmp/schemas-idempotency-{1,2}` produce byte-identical output (`diff -r` exits 0). The `schema_roundtrip` integration test passes against the freshly regenerated schema.
- **LjungBox golden math preserved (D3-23):** the Phase 3 statsmodels golden test (`scan_ljung_box`) re-runs with byte-identical `q_stats`, `p_values`, `acf`, `returns`, `timestamps_ms`, `value`, `p_value`, `n`, and `param_hash`. Only the envelope wire shape changed (envelope-shape change is the explicit point of D4-03, not a math regression).

## Task Commits

1. **Task 1: Schemars regen spike + workspace dep additions** — `442ad60` (chore)
2. **Task 2: Scan trait extension + ScanArity + InstrumentSpec + ScanRequest.instruments + DataSlice.sources** — `bed92bb` (feat; TDD task — six new behavior tests shipped alongside the type changes)
3. **Task 3: PreflightCode::WrongInstrumentArity + schemars regen** — `798e2a9` (feat; TDD task — extended test cases-array + new sibling test)

_Note: This plan ships ONLY the trait-and-types extension; the 22 scan registrations (Plans 04-02 / 04-03 / 04-04) and `validate_arity` preflight helper (Plan 04-02) consume this surface._

## Files Created/Modified

**Created**
- `.planning/phases/04-scan-catalogue-anom-cross-seas/04-01-SCHEMA-DIFF.md` — schema-additive memo recording the D4-01 / D4-03 path decision.

**Modified**
- `Cargo.toml` — three new `[workspace.dependencies]` entries.
- `crates/miner-core/Cargo.toml` — three new `[dependencies]` mirrored via `workspace = true`.
- `crates/miner-core/src/reader.rs` — `InstrumentSpec` struct + `instrument_spec_parse_round_trip` test.
- `crates/miner-core/src/scan/mod.rs` — `ScanArity` enum; `Scan::arity()` trait method; `ScanRequest.instruments: Vec<InstrumentSpec>`; constructor signature updated; three new tests (`scan_arity_serialises_snake_case`, `scan_request_instruments_len_one_serialises`); existing `scan_request_dry_run_defaults_false_when_absent` + `sample_scan_request` updated for the new shape.
- `crates/miner-core/src/findings/mod.rs` — `DataSlice.sources: Vec<Source>`; `source: Source` field removed from `ResultFinding` + `GapAbortedFinding`; `data_slice_sources_vec_round_trip` test added; fixtures updated.
- `crates/miner-core/src/error/codes.rs` — `PreflightCode::WrongInstrumentArity`; extended `preflight_code_serialises_snake_case` cases array; new sibling test.
- `crates/miner-core/src/scan/ljung_box/mod.rs` — `arity()` impl; `data_slice.sources` populated from `req.instruments`; `r.source.*` test assertions rewritten to `r.data_slice.sources[0].*`; new `ljung_box_scan_reports_single_arity` test.
- `crates/miner-core/src/engine/mod.rs` — single-leg dispatch reads `req.instruments[0]`; `GapAborted` / `emit_scan_error` / dry-run `planned_data_slice` populate `data_slice.sources`; `FailingIoScan`/`FailingMinerScan` test impls declare `arity()`.
- `crates/miner-core/src/engine/framing.rs` — `RunStart.request` echoes `instruments` JSON array instead of singleton `instrument` + `side`; framing test updated.
- `crates/miner-cli/src/scan_args.rs` — `to_scan_request` builds a single-leg `Vec<InstrumentSpec>` from existing `--instrument` + `--side` flags (Plan 04-02 will introduce the repeatable `--instrument SYMBOL:side` flag per Pattern K); CLI tests updated.
- `crates/miner-core/tests/{scan_ljung_box,scan_facade_determinism,dry_run,shuffled_future_regression,schema_roundtrip}.rs` — `instruments: vec![InstrumentSpec {...}]` fixture updates; D4-03 source removal from `schema_roundtrip` fixtures.
- `crates/miner-core/tests/snapshots/scan_ljung_box__ljung_box_matches_statsmodels_golden.snap` — regenerated to reflect the D4-03 envelope shape; statsmodels math arrays byte-identical.
- `schemas/findings-v1.schema.json` — regenerated; D4-03 `sources` field + removed `source` field from ResultFinding/GapAbortedFinding.

## Decisions Made

- **D4-03 (sources Vec) is fully additive at the schemars level** — discovered during Task 1's spike. The `#[serde(default)]` attribute keeps the `required` array unchanged, so the spike outcome is the strongest possible classification (truly additive, not "non-additive but accepted"). D4-03-ALT (peer_sources sibling) is therefore NOT required and is rejected.
- **D4-01 (instruments Vec) has zero schema impact** — `ScanRequest` is not `JsonSchema`-derived, so the wire-form generalisation flows through `RunStart.request: serde_json::Value` (opaque) and is invisible to schemars.
- **schema_version stays = 1** — per 04-RESEARCH §Section 7 and CONTEXT.md: consumers haven't shipped yet, Phase 6 wrappers don't exist, so the regenerated schema is committed as a deliberate facade-shape update rather than a `schema_version`-bumping breaking change.
- **Scan::arity() has no default body** — explicit declaration per impl prevents a silent `Single` default from mis-classifying CROSS scans.
- **InstrumentSpec lives in reader.rs (next to Side)** — PATTERNS.md Pattern K recommendation; co-location keeps the typed (symbol, side) pair in one place.
- **Engine single-leg dispatch reads `req.instruments[0]`** — `expect("ScanRequest.instruments must be non-empty post-preflight (D4-02)")` documents the invariant; Plan 04-02's `validate_arity` preflight will reject mismatched arity before this point with `PreflightCode::WrongInstrumentArity`.

## Deviations from Plan

### Rule 3 (Blocking-issue auto-fix) — 4 instances

**1. [Rule 3 - Plan baseline incorrect] DataSlice did not have `source: Source` field; field lived on ResultFinding / GapAbortedFinding instead**

- **Found during:** Task 1 (schema regen spike) — the plan's `<interfaces>` block said `DataSlice { source: Source, time_range, gap_manifest, ... }` but the Phase 3 head has `DataSlice { range: TimeRange, gap_manifest_ref, gap_manifest }` with no `source` field; `source: Source` lives directly on `ResultFinding` and `GapAbortedFinding`.
- **Issue:** Implementing D4-03 verbatim against the plan baseline ("replace `DataSlice.source: Source` with `DataSlice.sources: Vec<Source>`") would have left the existing `source` fields on `ResultFinding` and `GapAbortedFinding` orphaned, contradicting D4-03's stated goal of "per-finding leg provenance lives on DataSlice".
- **Fix:** Preserved the spirit of D4-03: ADD `sources: Vec<Source>` to `DataSlice` AND REMOVE `source: Source` from both `ResultFinding` and `GapAbortedFinding`. Documented in 04-01-SCHEMA-DIFF.md "Side Note: Plan baseline vs current code".
- **Files modified:** `crates/miner-core/src/findings/mod.rs`, every callsite of the removed fields.
- **Verification:** `cargo test -p miner-core --test schema_roundtrip` GREEN against regenerated schema; `data_slice_sources_vec_round_trip` GREEN.
- **Committed in:** bed92bb (Task 2 commit).

**2. [Rule 3 - Blocking compile cascade] `ScanRequest::new` constructor signature change cascaded to scan_args.rs (CLI), engine/mod.rs (2 test fixtures), engine/framing.rs (1 test fixture), and 5 integration test files**

- **Found during:** Task 2 (D4-01 ScanRequest field shape change).
- **Issue:** The plan's `<files_modified>` block listed only `scan/mod.rs`, `findings/mod.rs`, `error/codes.rs`, `reader.rs`, plus Cargo.toml + schemas + memo — but the D4-01 ScanRequest change cascades to every callsite that constructs a ScanRequest or asserts `req.instrument` / `req.side` (the CLI builder, the engine sample-request fixtures, the framing builder, plus five integration tests).
- **Fix:** Updated each callsite to use `vec![InstrumentSpec { symbol, side }]`; updated CLI to build the single-leg Vec from existing `--instrument` + `--side` flags. Per PATTERNS.md Pattern K, Plan 04-02 will land the repeatable `--instrument SYMBOL:side` CLI flag.
- **Files modified:** `crates/miner-cli/src/scan_args.rs`, `crates/miner-core/src/engine/{mod.rs,framing.rs}`, 5 integration tests.
- **Verification:** Full `cargo test --workspace` GREEN.
- **Committed in:** bed92bb (Task 2 commit).

**3. [Rule 3 - Plan acceptance criterion incorrect] Plan acceptance criterion said `grep -q '"wrong_instrument_arity"' schemas/findings-v1.schema.json` succeeds; the variant does NOT appear in the schema because `PreflightCode` is not transitively reachable from `Finding`**

- **Found during:** Task 3 (PreflightCode::WrongInstrumentArity + regen).
- **Issue:** `PreflightCode` is emitted to stderr via `WireError`, not via the `Finding` envelope. schemars 1.x only walks types transitively reachable from the root type (`Finding` for findings schema, `ScansCatalogueEntry` for catalogue schema). The acceptance criterion's grep expectation cannot be satisfied without adding `PreflightCode` to either schema root, which is out of scope for Plan 04-01.
- **Fix:** Verified the variant via the explicit unit tests (`preflight_code_serialises_snake_case` 8-row cases array + `preflight_code_as_str_wrong_instrument_arity`) instead of the schema grep. The schemas/ diff is exactly what 04-01-SCHEMA-DIFF.md predicted (additive `sources` Vec on DataSlice; `source` removed from ResultFinding + GapAbortedFinding) and nothing more.
- **Files modified:** none beyond the planned `error/codes.rs` + regenerated schema.
- **Verification:** `cargo test -p miner-core --lib error::codes::tests::preflight_code_serialises_snake_case` GREEN with 8 rows; `cargo test -p miner-core --lib error::codes::tests::preflight_code_as_str_wrong_instrument_arity` GREEN.
- **Committed in:** 798e2a9 (Task 3 commit).

**4. [Rule 3 - Plan verify command needs `--edges normal`] `cargo tree -p miner-core | grep -cE 'tokio|async-std|smol'` returns 11 due to pre-existing dev-dep transitives, not a Phase 4 regression**

- **Found during:** Task 1 (workspace dep add).
- **Issue:** The plan's verify command `cargo tree -p miner-core | grep -cE 'tokio|async-std|smol' | grep -q '^0$'` is incorrect at the pre-Phase 4 baseline too — 11 matches surface via the dev-dep `miner-reader-dukascopy` declared in `crates/miner-core/Cargo.toml:74-78`. The matches are NOT introduced by Phase 4's three new deps (verified by toggling them and re-running).
- **Fix:** Used `cargo tree -p miner-core --edges normal | grep -cE 'tokio|async-std|smol'` which is the CORRECT FOUND-04 contract per `crates/miner-core/Cargo.toml:7-8` ("miner-core is sync + rayon only" — the lib graph, not the dev graph). Post-add value: 0.
- **Files modified:** none.
- **Verification:** `cargo tree -p miner-core --edges normal 2>/dev/null | grep -cE 'tokio|async-std|smol'` returns 0.
- **Committed in:** 442ad60 (Task 1 commit).

---

**Total deviations:** 4 auto-fixed (4 Rule 3 — blocking-issue resolutions; the underlying type-cascade was unavoidable given the user-locked D4-01 / D4-03 decisions, and the plan baseline / verify-command issues were corrected in-line).
**Impact on plan:** All four deviations preserved the spirit of D4-01 / D4-02 / D4-03 (the user-locked decisions in 04-CONTEXT.md) and the SCHEMA-DIFF.md decision (D4-03 path, no D4-03-ALT fallback). No scope creep — the 22 Phase 4 scan registrations, the `validate_arity` preflight helper, and the repeatable CLI `--instrument SYMBOL:side` flag all remain deferred to Plans 04-02 / 04-03 / 04-04 / 04-05 as specified.

## Issues Encountered

- **`schema_roundtrip` integration test failed transiently during Task 2** — the committed `schemas/findings-v1.schema.json` lagged the D4-03 shape change. Resolved as planned in Task 3 (schema regen). No actual problem; just an inter-task gap that the plan's three-task structure explicitly accommodates (the plan's `<acceptance_criteria>` for Task 2 says `cargo test -p miner-core` "produces 0 failing tests including the six new behavior tests" — the six new tests passed; `schema_roundtrip` is not one of the six and only requires Task 3's regen to pass).
- **`Blake3Hex` deserialization requires a borrowed `&str`** — the initial `scan_request_instruments_len_one_serialises` test used `serde_json::from_value` which produces owned `Value`s without borrowable string slices. Rewrote to `serde_json::to_string` -> `serde_json::from_str` (the borrowable path). Workspace-wide pattern: `serde_json::from_str` is required for any `ScanRequest` round-trip test.

## User Setup Required

None — no external service configuration, no new env vars, no secrets. The three new workspace deps (`ndarray`, `ndarray-stats`, `nalgebra`) resolve from crates.io via the normal `cargo build` path; they require no setup beyond a working Rust 1.85 toolchain.

## Next Phase Readiness

**Plan 04-02 unblocked.** All four user-locked decisions in 04-CONTEXT.md (D4-01, D4-02, D4-03, D4-04) have their facade-shape prerequisites in place:

- D4-01: `ScanRequest.instruments: Vec<InstrumentSpec>` ✓
- D4-02: `Scan::arity()` + `ScanArity { Single, Pair }` + `PreflightCode::WrongInstrumentArity` ✓
- D4-03: `DataSlice.sources: Vec<Source>` ✓ (D4-03-ALT fallback NOT needed; full Vec path)
- D4-04: gap-policy intersection helper — DEFERRED to Plans 04-02 / 04-03 (CROSS family wire-up).

**Plan 04-02 entry points:**
- Add `engine::preflight::validate_arity(scan, &instruments) -> Result<(), WireError>` (Pattern H in PATTERNS.md).
- Add `crates/miner-core/src/scan/primitives/{returns.rs, time_alignment.rs}` (ANOM-01 + CROSS-01 kernels per 04-RESEARCH §Section 8).
- Land the repeatable `--instrument SYMBOL:side` CLI flag per Pattern K.
- Register 11 ANOM scans + refactor LjungBox to call the primitives::returns::log_returns kernel (D4-06).

**Plan 04-03 / 04-04 prerequisites:** all in place. Plan 04-03 (CROSS) reads from `req.instruments[0..2]`; Plan 04-04 (SEAS) reads `req.instruments[0]`. The CROSS gap-policy intersection per D4-04 will live in `crates/miner-core/src/scan/primitives/time_alignment.rs::intersect_gaps` per RESEARCH §Pattern I.

**No blockers.** The 22-scan rollout has a clean, fully-tested facade surface to build against.

## Self-Check: PASSED

Verified:
- [x] `crates/miner-core/Cargo.toml` ndarray/ndarray-stats/nalgebra entries present (`grep -E '^ndarray|^nalgebra'`).
- [x] root `Cargo.toml` workspace deps present.
- [x] `.planning/phases/04-scan-catalogue-anom-cross-seas/04-01-SCHEMA-DIFF.md` exists (198 lines, ≥20).
- [x] `pub enum ScanArity` present in scan/mod.rs (1 match).
- [x] `fn arity(&self) -> ScanArity` present in scan/mod.rs (trait method) AND scan/ljung_box/mod.rs (LjungBox impl).
- [x] `pub struct InstrumentSpec` present in reader.rs.
- [x] `pub instruments: Vec<InstrumentSpec>` present in scan/mod.rs (1 match).
- [x] `pub sources: Vec<Source>` present in findings/mod.rs (1 match).
- [x] `pub source: Source` ABSENT from ResultFinding + GapAbortedFinding.
- [x] `WrongInstrumentArity` present in error/codes.rs (5 matches — variant + as_str arm + 3 test references).
- [x] Schemas regenerate idempotently (`diff -r /tmp/schemas-idempotency-{1,2}` exits 0).
- [x] FOUND-04 sync-only invariant preserved on lib graph (0 tokio/async-std/smol via `--edges normal`).
- [x] All workspace tests pass: `cargo test --workspace` GREEN (175 lib + every integration suite).
- [x] Commits exist:
  - `442ad60` (Task 1: deps + SCHEMA-DIFF.md) — `git log --oneline | grep 442ad60` ✓
  - `bed92bb` (Task 2: trait + types) — `git log --oneline | grep bed92bb` ✓
  - `798e2a9` (Task 3: PreflightCode + regen) — `git log --oneline | grep 798e2a9` ✓

---
*Phase: 04-scan-catalogue-anom-cross-seas*
*Completed: 2026-05-19*
