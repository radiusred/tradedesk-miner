---
phase: 04-scan-catalogue-anom-cross-seas
plan: 12
subsystem: engine-dispatch
gap_closure: true

tags:
  - rust
  - gap-closure
  - engine
  - pair-arity
  - regression-coverage
  - cr-01

requires:
  - phase: 04-scan-catalogue-anom-cross-seas
    plan: 02
    provides: "engine::gap_policy::dispatch_pair helper + two_leg_facade.rs scaffold + validate_arity preflight (the helper that Plan 04-12 finally calls; the scaffold that Plan 04-12 converts to a regression gate)"
  - phase: 04-scan-catalogue-anom-cross-seas
    plan: 07
    provides: "Pair-arity scans registered (cross.corr.pearson_rolling / spearman_rolling / cross.ols.rolling); 04-07-SUMMARY recorded the 'Pair branch wiring deferred to Plan 04-11' note that orphaned dispatch_pair"
  - phase: 04-scan-catalogue-anom-cross-seas
    plan: 08
    provides: "Final two Pair-arity scans registered (cross.lead_lag.ccf, cross.cointegration.engle_granger); the five CROSS scans that this plan unblocks via the facade"
  - phase: 04-scan-catalogue-anom-cross-seas
    plan: 11
    provides: "Phase 4 verification surfaced CR-01 in 04-VERIFICATION.md gaps_found verdict — the trigger for this gap-closure plan"

provides:
  - "engine::run_one_with_registry dispatches Pair-arity scans through a new dispatch_pair_arity_body helper that wraps engine::gap_policy::dispatch_pair (resolves CR-01: all five CROSS scans previously emitted Finding::ScanError 'expected Pair arity (ctx.bars_pair is None)' when invoked via engine::run_one)"
  - "tests/two_leg_facade.rs converted from scaffold to end-to-end CR-01 regression gate via SyntheticCache + DukascopyReader + LeadLagCcfScan"
  - "tests/arity_preflight.rs::correct_arity_pair_scan_passes_arity_preflight tightened from 'any non-arity outcome' to 'must produce Finding::Result' — closes the loose assertion that swallowed CR-01"
  - "tests/byte_identical_rerun.rs::byte_identical_rerun_cross_engle_granger refactored to drive the scan through engine::run_one_with_registry (instead of hand-built ScanCtx { bars_pair: Some(..) }) — byte-identity invariant now covers the engine wiring too"
  - "Four new `<test>_via_engine_facade` happy-path tests appended to scan_corr_rolling.rs / scan_ols_rolling.rs / scan_lead_lag.rs / scan_engle_granger.rs — every CROSS scan now has at least one test that drives the engine path end-to-end"

affects:
  - Phase 5 (HYG-* — picks up the deferred Engle-Granger / KPSS reconciliation; the engine path is now correctly wired for Pair-arity, so any future CROSS scan added to the registry is automatically reachable via run_one)
  - Phase 6 (MCP / HTTP wrappers — both wrappers call engine::run_one directly; the CR-01 fix means Pair-arity CROSS scans work through the wrappers too once they ship)

tech-stack:
  added: []  # No new dependencies; pure refactor + test additions
  patterns:
    - "Per-arity dispatch helper extraction — keep the Single-arity body inline in run_one_with_registry (preserves the algorithm-walk doc comment) and factor the Pair body into a dispatch_pair_arity_body helper (mirrors the Single body verbatim with the per-leg gap-detection + bars_pair-aware ScanCtx construction)"
    - "engine-path regression gate via SyntheticCache — drive integration tests through the production DukascopyReader + BarCache + engine::run_one_with_registry pipeline rather than constructing ScanCtx { bars_pair: Some(..), .. } by hand (the kernel-direct test stays as the kernel pin; the engine-facade variant pins the dispatch wiring)"
    - "Negative CR-01 pin — every engine-facade test asserts NO Finding::ScanError envelope carries the 'expected Pair arity' message; a future regression of the dispatch wiring trips this assertion across nine separate test names"

key-files:
  created:
    - ".planning/phases/04-scan-catalogue-anom-cross-seas/04-12-SUMMARY.md"
  modified:
    - "crates/miner-core/src/engine/mod.rs (run_one_with_registry branches on scan.arity(); new dispatch_pair_arity_body helper; three new dispatch tests — Single path / Pair path / Pair-arity preflight guard)"
    - "crates/miner-core/tests/two_leg_facade.rs (scaffold -> CR-01 regression gate via engine::run_one_with_registry + SyntheticCache + LeadLagCcfScan; two original primitive-shape tests retained as orthogonal pins)"
    - "crates/miner-core/tests/arity_preflight.rs (tightened correct_arity_pair_scan_passes_arity_preflight to require Finding::Result; StubPair body now emits a Result envelope + asserts ctx.bars_pair.is_some() as a CR-01 negative pin)"
    - "crates/miner-core/tests/byte_identical_rerun.rs (byte_identical_rerun_cross_engle_granger refactored to drive engine path via SyntheticCache + run_one_with_registry; byte-identity now covers RunStart + Result + RunEnd masked envelopes; original kernel-direct helpers retained for future use behind #[allow(dead_code)] with rationale)"
    - "crates/miner-core/tests/scan_corr_rolling.rs (+ scan_corr_rolling_pearson_happy_path_via_engine_facade + scan_corr_rolling_spearman_happy_path_via_engine_facade)"
    - "crates/miner-core/tests/scan_ols_rolling.rs (+ scan_ols_rolling_happy_path_via_engine_facade)"
    - "crates/miner-core/tests/scan_lead_lag.rs (+ scan_lead_lag_happy_path_via_engine_facade)"
    - "crates/miner-core/tests/scan_engle_granger.rs (+ scan_engle_granger_happy_path_via_engine_facade)"

key-decisions:
  - "CR-01 root cause + hand-off chain: Plan 04-02 introduced ScanArity + engine::gap_policy::dispatch_pair but left run_one_with_registry hard-coded to single-leg dispatch ('Plan 04-07 will wire the Pair branch via dispatch_pair', per the inline comment). Plan 04-07 deferred to 04-11 with 'will pin engine-facade integration later'. Plan 04-11 picked up the byte-identical-rerun test + goldens but did NOT pick up the dispatch wiring. Net effect: dispatch_pair was orphaned — never called from anywhere in production code, yet every CROSS scan's integration test bypassed the engine by constructing ScanCtx { bars_pair: Some(..), .. } directly, so the kernel correctness was pinned but the facade wiring was structurally absent."
  - "Dispatch refactor strategy — branch-and-return over wrap-in-if: chose to insert `if scan.arity() == ScanArity::Pair { return dispatch_pair_arity_body(...); }` immediately after the Pair-arity preflight, keeping the historical single-leg body intact below. Alternative (wrap the entire Single body in a Single-arity match arm) would have required reflowing the algorithm-walk doc comment (which is referenced by the cancel_at_entry/before_subrange/inside_scan_kernel test names) and risked a larger diff for no behavioural benefit. The branch-and-return shape is byte-identical to the historical Single path for any non-Pair scan."
  - "Pair body parallels Single body verbatim — same 5-arm error handling (reader: leg-a + leg-b, cache: leg-a + leg-b, ScanError::Kernel, ScanError::Io, ScanError::Miner), same cancel-poll cadence (cancel-before-subrange at sub-range loop top, no cancel-inside-kernel because the existing yield site lives in LjungBox not in CROSS scans). The two duplicated reader-error arms (leg a / leg b) are an intentional explicitness — combining them via a match-over-(LegA, LegB) tuple would obscure which leg failed in the surfaced error message."
  - "Joint manifest for ScanCtx.gap_manifest under ContinuousOnly — when the user runs --gap-policy=continuous_only, the engine inlines the JOINT manifest (UNION of leg-a + leg-b gaps via primitives::time_alignment::intersect_gaps) into ScanCtx.gap_manifest. The CROSS scan body then echoes this into Finding::Result.data_slice.gap_manifest (D3-12 parity with the single-leg path). The joint manifest is constructed twice in the Pair body — once for ScanCtx, once internally by dispatch_pair — because the cleanest cleanup (a shared helper returning both the dispatch decision + the joint manifest) was out of scope for this gap-closure plan. Documented for Phase 7 hardening pickup."
  - "Test-coverage strategy — kernel pins + facade pins, no removals: the four existing direct-ScanCtx tests (scan_corr_rolling Pearson + Spearman, scan_ols_rolling, scan_lead_lag, scan_engle_granger) stay as kernel-level pins; the new `<test>_via_engine_facade` siblings pin the dispatch wiring. No existing test was removed — the kernel-direct path remains a valid pin for the kernel correctness, orthogonal to the engine-path coverage."
  - "byte_identical_rerun_cross_engle_granger REFACTORED rather than additive — the previous version constructed ScanCtx { bars_pair: Some(..), .. } directly, which is exactly the kernel-only path Plan 04-12 is moving away from for CR-01-sensitive tests. Refactoring (rather than adding a sibling) is the correct call: the kernel-direct byte-identity invariant is already pinned by the four CROSS direct-ScanCtx integration tests' shape, and routing this specific test through the engine adds genuine new coverage (RunStart + Result + RunEnd byte-identity is a stricter invariant than Result alone)."

deviations: []  # None — the plan was executed exactly as written. Three tasks, three commits, all 796 tests pass workspace-wide.

requirements-completed: []  # CR-01 is a defect, not a numbered requirement. The fix advances Phase 4's SC#4 (consistent Finding envelope shape across all 22 scans) from "kernel-pinned only" to "kernel + facade pinned".

duration: ~40 min
completed: 2026-05-20
---

# Phase 4 Plan 12: Pair-arity Engine Dispatch (CR-01 Gap Closure) Summary

One-liner: Wire `engine::run_one_with_registry` to route Pair-arity scans through the orphaned `gap_policy::dispatch_pair` helper, then tighten nine integration tests so a future regression cannot recur.

## Context

`.planning/phases/04-scan-catalogue-anom-cross-seas/04-REVIEW.md` flagged CR-01 (blocking severity): every Pair-arity CROSS scan emitted `Finding::ScanError(compute_error, "expected Pair arity (ctx.bars_pair is None)")` when invoked via the production `engine::run_one` entry point. The defect was triggered by `engine::run_one_with_registry` hard-coding `bars_pair: None` in its ScanCtx construction; the Pair branch the comment promised ("Plan 04-07 wires it via a new `dispatch_pair` path") never landed.

`.planning/phases/04-scan-catalogue-anom-cross-seas/04-VERIFICATION.md` confirmed the defect via a live binary invocation. Plan 04-12 is the gap-closure plan that resolves CR-01 and tightens the regression coverage so the same hand-off chain cannot orphan another facade-level helper.

## What Shipped

### Task 1 — Engine wiring (commit `fada08e`)

`engine::run_one_with_registry` now branches on `scan.arity()`:

- **Single arity**: existing path, byte-identical to the historical Plan 03-07 body. Single-leg gap detection, single-leg `BarCache::get_or_build`, `ScanCtx { bars_pair: None, .. }`.
- **Pair arity**: new `dispatch_pair_arity_body` helper. Gap-detects leg A AND leg B, dispatches through `gap_policy::dispatch_pair` (UNIONs the per-leg manifests via `intersect_gaps`), loads bars for BOTH legs per sub-range, builds `ScanCtx` with `bars_pair: Some((a, b))`. The body mirrors the Single body's 5-arm error handling (reader: per-leg, cache: per-leg, ScanError::Kernel / Io / Miner) and cancel-poll cadence.

Three new unit tests pin the dispatch contract:

| Test | Pins |
|------|------|
| `run_one_dispatches_single_arity_scan_via_single_leg_path` | Single path unchanged; `data_slice.sources.len() == 1` |
| `run_one_dispatches_pair_arity_scan_via_dispatch_pair` | Pair dispatch reaches kernel; `RunOutcome::Ok`; `data_slice.sources.len() == 2`; NO `Finding::ScanError "expected Pair arity"` envelope |
| `run_one_pair_arity_with_mismatched_instrument_count_rejected_at_preflight` | New Pair branch must NOT bypass `validate_arity` |

The middle test is the engine-level CR-01 regression gate.

### Task 2 — `two_leg_facade.rs` rewrite (commit `65c4f78`)

The Plan 04-02 scaffold (the file's own docstring acknowledged it did not call `engine::run_one`) is now an end-to-end regression gate. The new `two_leg_facade_pair_arity_dispatch_emits_result_envelope` test:

1. Populates both legs (EURUSD + GBPUSD) in a `SyntheticCache`.
2. Registers the real `LeadLagCcfScan` (CROSS-04).
3. Invokes `engine::run_one_with_registry` against the production `DukascopyReader` + `BarCache` pipeline.
4. Asserts `RunOutcome::Ok`, exactly one `Finding::Result`, `data_slice.sources.len() == 2`, and negative-pins that NO `Finding::ScanError "expected Pair arity"` appears.

The two original primitive-shape tests (`inner_join_aligns_two_leg_close_vectors` + `data_slice_sources_vec_is_reachable_for_two_leg_envelopes`) are retained as orthogonal pins.

### Task 3 — Regression-coverage tightening + SUMMARY (this commit)

- **`tests/arity_preflight.rs::correct_arity_pair_scan_passes_arity_preflight`** — tightened from "any non-arity outcome is fine" (lines 210-213 of the pre-fix file — the assertion that swallowed CR-01) to "must produce `Finding::Result`". The `StubPair` body now emits a Result envelope and asserts `ctx.bars_pair.is_some()` as a CR-01 negative pin.
- **`tests/byte_identical_rerun.rs::byte_identical_rerun_cross_engle_granger`** — refactored to drive the scan through `engine::run_one_with_registry` instead of hand-building `ScanCtx { bars_pair: Some(..), .. }`. The byte-identity invariant now covers the FULL envelope chain (RunStart + Result + RunEnd), which is a stricter pin than the previous Result-only check.
- **Four `<test>_via_engine_facade` siblings** appended to the existing CROSS integration tests:
  - `scan_corr_rolling_pearson_happy_path_via_engine_facade`
  - `scan_corr_rolling_spearman_happy_path_via_engine_facade`
  - `scan_ols_rolling_happy_path_via_engine_facade`
  - `scan_lead_lag_happy_path_via_engine_facade`
  - `scan_engle_granger_happy_path_via_engine_facade`

Each engine-facade test drives its scan through `engine::run_one_with_registry` against a `SyntheticCache` and asserts the Finding::Result shape + the CR-01 negative pin.

## Verification

- `cargo test --workspace --no-fail-fast` — **796 passed, 0 failed, 3 ignored** (the 3 ignored are the Plan 04-11 awaiting-regen goldens; unchanged).
- `cargo run -p xtask -- gen-schema && git diff --exit-code schemas/` — exits 0 (no schemars-derived types touched).
- Engine-level CR-01 regression gate (`run_one_dispatches_pair_arity_scan_via_dispatch_pair`) — green.
- End-to-end Pair-arity invocation via `engine::run_one_with_registry` + `LeadLagCcfScan` against a synthetic cache emits `Finding::Result` (NOT `Finding::ScanError`).

## Coverage After Plan 04-12

A future regression of the Pair-arity dispatch wiring (e.g., a "simplification" that drops the arity branch from `run_one_with_registry`) now trips at least NINE separate test names across the workspace:

| File | Test |
|------|------|
| `crates/miner-core/src/engine/mod.rs` (unit) | `run_one_dispatches_pair_arity_scan_via_dispatch_pair` |
| `tests/two_leg_facade.rs` | `two_leg_facade_pair_arity_dispatch_emits_result_envelope` |
| `tests/arity_preflight.rs` | `correct_arity_pair_scan_passes_arity_preflight` |
| `tests/byte_identical_rerun.rs` | `byte_identical_rerun_cross_engle_granger` |
| `tests/scan_corr_rolling.rs` | `scan_corr_rolling_pearson_happy_path_via_engine_facade` |
| `tests/scan_corr_rolling.rs` | `scan_corr_rolling_spearman_happy_path_via_engine_facade` |
| `tests/scan_ols_rolling.rs` | `scan_ols_rolling_happy_path_via_engine_facade` |
| `tests/scan_lead_lag.rs` | `scan_lead_lag_happy_path_via_engine_facade` |
| `tests/scan_engle_granger.rs` | `scan_engle_granger_happy_path_via_engine_facade` |

## Out of Scope (Deferred)

- WR-01..WR-08 from 04-REVIEW.md (effect_extra_keys ordering, event_window doc/impl mismatch, eom_som EOM-precedence, RawArray F64-for-UTF8 lies, etc.) — track in their own gap-closure plans or Phase 7 hardening.
- ADF reconciliation between `cross/engle_granger` and `anom/adf` — Phase 5 / HYG-01.
- Pinned-venv goldens regen — user setup recipe in 04-11-SUMMARY.md §"User Setup Required"; the 3 `#[ignore]`d tests remain ignored until the user runs that recipe.
- `clippy::pedantic` workspace cleanup — Phase 7.
- The joint manifest is constructed twice in `dispatch_pair_arity_body` (once for `ScanCtx`, once internally by `dispatch_pair`). The cleanest cleanup (a shared helper returning both the dispatch decision + the joint manifest) was out of scope for this gap-closure plan — documented for Phase 7 hardening pickup.

## Commits

| # | Hash | Subject |
|---|------|---------|
| 1 | `fada08e` | feat(04-12): wire Pair-arity dispatch into engine::run_one_with_registry |
| 2 | `65c4f78` | test(04-12): convert two_leg_facade.rs into the CR-01 regression gate |
| 3 | (this commit) | test(04-12): tighten arity_preflight + drive byte_identical_rerun + 4 CROSS tests through engine facade |

## Self-Check: PASSED

- `crates/miner-core/src/engine/mod.rs` — FOUND (modified, contains `dispatch_pair_arity_body`)
- `crates/miner-core/tests/two_leg_facade.rs` — FOUND (modified, contains `engine::run_one_with_registry` + `two_leg_facade_pair_arity_dispatch_emits_result_envelope`)
- `crates/miner-core/tests/arity_preflight.rs` — FOUND (modified, `Finding::Result` assertion live)
- `crates/miner-core/tests/byte_identical_rerun.rs` — FOUND (modified, drives engine path)
- `crates/miner-core/tests/scan_corr_rolling.rs` — FOUND (modified, two `_via_engine_facade` tests)
- `crates/miner-core/tests/scan_ols_rolling.rs` — FOUND (modified, one `_via_engine_facade` test)
- `crates/miner-core/tests/scan_lead_lag.rs` — FOUND (modified, one `_via_engine_facade` test)
- `crates/miner-core/tests/scan_engle_granger.rs` — FOUND (modified, one `_via_engine_facade` test)
- Commit `fada08e` — FOUND (Task 1)
- Commit `65c4f78` — FOUND (Task 2)
