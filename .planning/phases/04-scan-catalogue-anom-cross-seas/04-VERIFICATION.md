---
phase: 04-scan-catalogue-anom-cross-seas
verified: 2026-05-20T17:00:00Z
status: gaps_found
score: 17/22 requirements verified end-to-end (5 CROSS REQs registered but not executable through engine), 4/5 success criteria met (SC#2 + SC#4 partially failing through CLI facade)
overrides_applied: 0
requirements:
  total: 22
  passed: 17
  partial: 0
  failed: 5
success_criteria:
  - {id: SC#1, status: pass, evidence: "11 ANOM scans registered, kernels match scipy/statsmodels conventions per unit + integration tests"}
  - {id: SC#2, status: fail, evidence: "CR-01 — all 5 CROSS scans emit Finding::ScanError via engine::run_one (verified live via miner CLI), data_slice.sources populated only via direct Scan::run bypass tests"}
  - {id: SC#3, status: pass, evidence: "6 SEAS scans registered, per-bucket arrays emitted, integration tests pin shape"}
  - {id: SC#4, status: partial, evidence: "byte_identical_rerun.rs pins envelope shape for ANOM+CROSS+SEAS via direct Scan::run, but the ROADMAP SC#4 wording is 'invoke any scan through the same CLI facade' — CROSS scans return ScanError envelopes through CLI, not Result envelopes"}
  - {id: SC#5, status: partial, evidence: "Three goldens checked in as explicit STUBs with null expected values; three cross-check tests are #[ignore]d behind provenance gates. Byte-identical-vs-scipy claim is not yet provable until a developer regens against pinned venv"}
blocking_issues:
  - "CR-01 (from 04-REVIEW.md): engine::run_one_with_registry hardcodes single-leg dispatch (engine/mod.rs:493-501 passes bars_pair: None unconditionally). All 5 CROSS scans (CROSS-01 through CROSS-05) emit Finding::ScanError envelopes through the production CLI path. dispatch_pair exists in gap_policy.rs:206 but is never called. Promised by 04-02-SUMMARY.md ('Plan 04-07 wires Pair branch into run_one_with_registry'), deferred by 04-07-SUMMARY.md ('deferred to Plan 04-11'), never delivered in 04-11."
gaps:
  - truth: "User can run every cross-instrument scan and receive findings carrying both legs' provenance and the joined data_slice (ROADMAP SC#2)"
    status: failed
    reason: "CR-01: engine::run_one_with_registry only dispatches Single-arity scans. All 5 CROSS scans yield Finding::ScanError envelopes containing the literal message 'expected Pair arity (ctx.bars_pair is None)' through the production CLI / engine path. Verified live via `miner scan cross.lead_lag.ccf@1 --instrument EURUSD:bid --instrument GBPUSD:bid ...`."
    artifacts:
      - path: "crates/miner-core/src/engine/mod.rs"
        issue: "Lines 419-579 (GapDispatch::SubRanges arm) hardcode single-leg dispatch: gap detection only for leg A (line 348), cache load only for leg A (line 444-456), make_scan_ctx called with bars_pair: None (line 493-495)"
      - path: "crates/miner-core/src/engine/gap_policy.rs"
        issue: "dispatch_pair() exists at line 206 but no production code path calls it (only referenced in a comment in engine/mod.rs:490)"
      - path: "crates/miner-core/tests/two_leg_facade.rs"
        issue: "Test file is a scaffold (lines 1-21 explicitly document this); does NOT invoke engine::run_one — only tests the inner_join primitive directly. No end-to-end CROSS engine test exists."
      - path: "crates/miner-core/tests/arity_preflight.rs"
        issue: "correct_arity_pair_scan_passes_arity_preflight (line 152) only verifies the preflight does NOT reject Pair scans; lines 210-213 explicitly say 'Plan 04-07's CROSS dispatch will tighten the assertion to a full RunStart/Result/RunEnd shape' — that tightening never happened"
    missing:
      - "Wire the Pair branch in engine::run_one_with_registry: after preflight::validate_arity, branch on scan.arity() == ScanArity::Pair to a sibling function that calls GapDetector::detect for BOTH legs, invokes engine::gap_policy::dispatch_pair to intersect both manifests, loads two BarFrames via BarCache::get_or_build, and builds ScanCtx with bars_pair: Some((&bars_a, &bars_b))"
      - "Extend crates/miner-core/tests/two_leg_facade.rs to invoke engine::run_one end-to-end with a FakeReader/synthetic-cache carrying two legs; assert resulting Finding::Result envelope's data_slice.sources.len() == 2"
      - "Tighten crates/miner-core/tests/arity_preflight.rs::correct_arity_pair_scan_passes_arity_preflight to require Finding::Result (or at least non-arity Finding::ScanError with a different code) — currently swallows CR-01 by accepting any non-arity outcome"
  - truth: "User can invoke any scan through the same CLI facade and observe consistent Finding envelope shape across all 22 scans (ROADMAP SC#4)"
    status: failed
    reason: "Same root cause as CR-01: 18 of 23 registered scans (LjungBox + 11 ANOM + 6 SEAS) emit Finding::Result envelopes through the CLI; the 5 Pair-arity CROSS scans emit Finding::ScanError envelopes. The envelope shape is consistent only in the sense that all scans emit a tagged Finding variant — but SC#4's intent (user can run any scan and get the documented effect.extra) holds only for 18/23 scans."
    artifacts:
      - path: "crates/miner-core/src/engine/mod.rs"
        issue: "Same single-leg hardcoded dispatch as above"
      - path: "crates/miner-core/tests/byte_identical_rerun.rs"
        issue: "Pins envelope shape via direct Scan::run for ANOM-02 / CROSS-05 / SEAS-01 (bypasses engine for CROSS); cannot detect CR-01 because it constructs ScanCtx { bars_pair: Some(...) } manually (lines 139-148)"
    missing:
      - "After CR-01 is fixed, extend byte_identical_rerun.rs (or add a new test) to drive each representative scan through engine::run_one with a FakeReader so the CLI-facade contract is regression-gated, not just the direct Scan::run boundary"
deferred:
  - truth: "Byte-identical golden fixtures cross-checked against scipy/statsmodels (ROADMAP SC#5)"
    addressed_in: "Phase 7 (hardening), unblocked by a developer with pinned Python 3.11 venv running the regen recipe in tests/goldens/REFERENCE-VERSIONS.md"
    evidence: "Per executor brief: the three #[ignore]d cross-check tests + STUB-marked goldens are an executor-acknowledged deferred work item. The integration tests are already wired (they will run automatically once provenance.scipy_version / statsmodels_version flips from 'STUB' to the pinned values). Phase 7 ROADMAP success criteria include 'full golden-file regression suite' which subsumes this regeneration."
  - truth: "Engle-Granger's local adf_step kernel reconciliation against canonical anom::adf::kernel"
    addressed_in: "Phase 5 / HYG-01"
    evidence: "Per executor brief and 04-11-PHASE-VERIFICATION.md §ADF Reconciliation: the local stub is intentionally deferred to Phase 5 alongside the bootstrap CI pipeline (statsmodels.coint uses a different default lag than adfuller(), so routing through the canonical kernel needs the Phase 5 hygiene work). Documented in engle_granger/kernel.rs file-level doc-comment."
  - truth: "Workspace-wide cargo clippy --all-targets -- -D warnings clean run"
    addressed_in: "Phase 7"
    evidence: "Per executor brief and 04-11-PHASE-VERIFICATION.md §Open Items: ~419 inherited pedantic warnings predate Plan 04-11; Plan 04-11 fixed only the 3 LN_2 lints explicitly in scope. ROADMAP Phase 7 lists 'cargo audit / cargo deny clean runs' which implies the clippy gate lands there."
human_verification:
  - test: "Visual inspection of engle_granger / kpss string_label_to_raw_array wire-form encoding (WR-06)"
    expected: "Consumers decoding raw_arrays per the documented Dtype::F64 contract should not silently mis-interpret string bytes as f64 elements. WR-06 in 04-REVIEW.md notes the dtype lies but no consumer is yet active. A human should decide whether to address before Phase 6 wrappers ship (when external consumers appear)."
    why_human: "WR-06 is a design wart, not a behavioural failure of Phase 4's contract. The 'right fix' (Utf8Bytes Dtype variant vs replacing the trick) is a forward-design decision."
  - test: "End-to-end CLI invocation of one CROSS scan against a real Dukascopy cache once CR-01 is fixed"
    expected: "miner scan cross.cointegration.engle_granger@1 --instrument EURUSD:bid --instrument GBPUSD:bid --timeframe 15m --window 2024-01-01:2024-06-01 returns Finding::RunStart + Finding::Result (with data_slice.sources.len() == 2) + Finding::RunEnd"
    why_human: "Requires a populated dev cache (28 instruments × 6 years) which the verifier doesn't have access to. Automated tests with FakeReader cover the synthetic path."
---

# Phase 4 Verification: Scan Catalogue (ANOM, CROSS, SEAS)

**Phase Goal:** User can invoke every v1 statistical-anomaly, cross-instrument, and seasonality scan through the facade, with each scan emitting a `Finding` envelope carrying its canonical effect statistic, sample size, raw arrays, and reproducibility metadata.

**Verified:** 2026-05-20T17:00:00Z
**Status:** gaps_found
**Re-verification:** No — initial verification

## Verdict

**PARTIAL PASS with one BLOCKER (CR-01).**

Phase 4 successfully ships **23 registered scans** (Phase 3 LjungBox + 22 new). Of those, **17 v1 REQ-IDs (11 ANOM + 6 SEAS) are end-to-end functional** via the CLI facade. The **5 CROSS REQ-IDs are registered, kernel-tested, and reachable from `Scan::run` directly, but cannot execute end-to-end through `engine::run_one_with_registry`** — they emit `Finding::ScanError` envelopes containing the literal message `"expected Pair arity (ctx.bars_pair is None)"` because the engine still hardcodes the single-leg dispatch path from Phase 3.

The handoff promised in `04-02-SUMMARY.md` ("Plan 04-07 wires Pair branch into run_one_with_registry") was deferred by `04-07-SUMMARY.md` line 76 to Plan 04-11, then never delivered — `04-11-SUMMARY.md` lists no engine-wiring work, and the executor's own `04-11-PHASE-VERIFICATION.md` does not mention the gap.

The phase goal ("User can invoke every v1 ... scan through the facade") is **observably broken** for 5 of 23 scans via the production CLI path. Unit and integration tests give a false-positive impression of completeness because **every CROSS scan integration test bypasses `engine::run_one` and constructs `ScanCtx { bars_pair: Some(...) }` manually**.

Two warnings (WR-06 string-bytes-in-F64 dtype lie, and WR-08 engle_granger vs canonical adf_step tail-formula divergence) are downstream design quality concerns that don't block the phase goal but should be tracked by Phase 5/6/7 owners. Per the executor brief, the deliberate STUB goldens (SC#5) and the deferred ADF reconciliation are NOT blockers — they are executor-acknowledged deferred work items.

## Requirements Traceability

22 Phase 4 v1 REQ-IDs:

| REQ-ID  | Description                                                                    | Registered scan_id@version                                | Plan  | Integration test                                          | Status                  |
|---------|--------------------------------------------------------------------------------|-----------------------------------------------------------|-------|-----------------------------------------------------------|-------------------------|
| ANOM-01 | Return primitives (log / simple / intraday / overnight split)                  | `stats.returns.profile@1`                                 | 04-03 | `scan_returns_profile.rs`                                 | VERIFIED                |
| ANOM-02 | Summary stats via Welford (mean / std / skew / kurt / iqr / min / max)         | `stats.summary.welford@1`                                 | 04-03 | `scan_summary_welford.rs`                                 | VERIFIED                |
| ANOM-03 | Rolling volatility + vol-of-vol                                                | `stats.vol.rolling@1`                                     | 04-03 | `scan_vol_rolling.rs`                                     | VERIFIED                |
| ANOM-04 | Ljung-Box autocorrelation on returns + squared returns                         | `stats.autocorr.ljung_box@1` (Phase 3) + `..._sq@1`        | 04-04 | `scan_ljung_box.rs` + `scan_ljung_box_squared.rs`          | VERIFIED                |
| ANOM-05 | Augmented Dickey-Fuller with AIC lag                                            | `stats.stationarity.adf@1`                                | 04-05 | `scan_adf.rs`                                             | VERIFIED                |
| ANOM-06 | KPSS stationarity                                                              | `stats.stationarity.kpss@1`                               | 04-05 | `scan_kpss.rs`                                            | VERIFIED                |
| ANOM-07 | Lo-MacKinlay variance ratio                                                    | `stats.variance_ratio.lo_mackinlay@1`                     | 04-05 | `scan_variance_ratio.rs`                                  | VERIFIED                |
| ANOM-08 | ARCH-LM heteroskedasticity                                                     | `stats.heteroskedasticity.arch_lm@1`                      | 04-06 | `scan_arch_lm.rs`                                         | VERIFIED                |
| ANOM-09 | Jarque-Bera normality                                                          | `stats.normality.jarque_bera@1`                           | 04-06 | `scan_jarque_bera.rs`                                     | VERIFIED                |
| ANOM-10 | Outlier / extreme-return detection (z-score + MAD)                             | `stats.outliers.z_and_mad@1`                              | 04-04 | `scan_outliers.rs`                                        | VERIFIED                |
| ANOM-11 | Drawdown profile                                                               | `stats.drawdown.profile@1`                                | 04-04 | `scan_drawdown.rs`                                        | VERIFIED                |
| CROSS-01| Time-alignment primitive (inner-join + intersect_gaps)                         | kernel-only — `scan::primitives::time_alignment`           | 04-02 | `gap_intersect_cross.rs`, exercised in `scan_corr_rolling.rs` | FAILED (CR-01)      |
| CROSS-02| Rolling Pearson + Spearman correlation                                         | `cross.corr.pearson_rolling@1`, `..spearman_rolling@1`     | 04-07 | `scan_corr_rolling.rs` (bypasses engine)                  | FAILED (CR-01)          |
| CROSS-03| Rolling OLS (β / α / R² / residual std)                                        | `cross.ols.rolling@1`                                     | 04-07 | `scan_ols_rolling.rs` (bypasses engine)                   | FAILED (CR-01)          |
| CROSS-04| Lead-lag CCF                                                                   | `cross.lead_lag.ccf@1`                                    | 04-08 | `scan_lead_lag.rs` (bypasses engine)                      | FAILED (CR-01)          |
| CROSS-05| Engle-Granger cointegration + OU half-life                                     | `cross.cointegration.engle_granger@1`                     | 04-08 | `scan_engle_granger.rs` (bypasses engine)                 | FAILED (CR-01)          |
| SEAS-01 | Hour-of-day profile (UTC)                                                       | `seas.bucket.hour_of_day@1`                               | 04-09 | `scan_seas_hour_of_day.rs`                                | VERIFIED                |
| SEAS-02 | Day-of-week effect                                                             | `seas.bucket.day_of_week@1`                               | 04-09 | `scan_seas_day_of_week.rs`                                | VERIFIED                |
| SEAS-03 | Trading-session bucketing (Asia / London / NY / overlap)                       | `seas.bucket.session@1`                                   | 04-09 | `scan_seas_session.rs`                                    | VERIFIED                |
| SEAS-04 | EOM/SOM by trading-day-of-month                                                | `seas.bucket.eom_som@1`                                   | 04-10 | `scan_seas_eom_som.rs`                                    | VERIFIED (with WR-03)   |
| SEAS-05 | One-way ANOVA + Kruskal-Wallis meta-scan                                       | `seas.test.anova_kruskal@1`                               | 04-10 | `scan_seas_anova_kruskal.rs`                              | VERIFIED                |
| SEAS-06 | Event-window pre/post stats                                                    | `seas.event.pre_post_window@1`                            | 04-10 | `scan_seas_event_window.rs`                               | VERIFIED (with WR-02)   |

**Counts:**
- Registered scans (live `miner scans` output): **23** (Phase 3 LjungBox + 22 Phase 4)
- v1 REQ-IDs end-to-end functional via CLI: **17 / 22** (11 ANOM + 6 SEAS)
- v1 REQ-IDs registered but failing through CLI: **5 / 22** (CROSS-01 through CROSS-05) — CR-01

## ROADMAP Success Criteria

| #    | Criterion (paraphrased)                                                                                              | Status   | Evidence                                                                                                                                                                                                                                                                                                  |
|------|----------------------------------------------------------------------------------------------------------------------|----------|-----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| SC#1 | Every single-series anomaly scan emits findings whose statistics match scipy/statsmodels conventions                  | PASS     | 11 ANOM scans registered (`miner scans` confirms); kernels carry sequential-summation + NaN-conversion-to-ScanError discipline; per-scan integration tests assert against hand-derived expected values + RESEARCH §Section 2 tolerances.                                                                  |
| SC#2 | Every cross-instrument scan emits findings carrying both legs' provenance and the joined data_slice                  | FAIL     | **CR-01:** All 5 CROSS scans are registered (`miner scans` confirms `arity=pair` for all 5) BUT every CROSS scan invocation through `engine::run_one_with_registry` emits `Finding::ScanError { error_code: "compute_error", message: "expected Pair arity (ctx.bars_pair is None)" }`. Live-verified via CLI invocation. |
| SC#3 | Every seasonality scan emits per-bucket mean/std/count/t-stat-vs-0/IQR + FX-major session defaults                   | PASS     | 6 SEAS scans registered; per-scan integration tests pin parallel `buckets`/`means`/`stds`/`counts`/`t_stats`/`iqrs` arrays; `session/mod.rs` declares FX-major default boundaries.                                                                                                                          |
| SC#4 | User can invoke any scan through the same CLI facade with consistent Finding envelope shape across all 22 scans     | PARTIAL  | `byte_identical_rerun.rs` proves envelope shape consistency for one rep per family via direct `Scan::run`. But the SC text says "through the same CLI facade" — and through CLI, CROSS scans emit `Finding::ScanError` envelopes (a different variant of the discriminated Finding enum) rather than `Finding::Result`. 18/23 scans emit Result via CLI; 5 emit ScanError. |
| SC#5 | Byte-identical golden fixtures for one ANOM + one CROSS + one SEAS scan within RESEARCH §Section 2 tolerances        | PARTIAL  | Three golden JSONLs exist (`stats.summary.welford.jsonl`, `cross.cointegration.engle_granger.jsonl`, `seas.bucket.hour_of_day.jsonl`) BUT all three are checked in as explicit STUBs with null `expected.*` values + `provenance.scipy_version: "STUB"` + `_stub_note` regen instructions. Three cross-check integration tests are `#[ignore]`d behind a provenance gate. Per executor brief this is an acknowledged deferred item awaiting a pinned Python 3.11 venv. |

## Blocking Issues

### CR-01 — Pair-arity scans non-functional through `engine::run_one_with_registry` (BLOCKER)

**Reproduction (verified live during this verification):**

```
$ cargo run -q -p miner-cli -- scan "cross.lead_lag.ccf@1" \
    --instrument EURUSD:bid --instrument GBPUSD:bid \
    --timeframe 15m --window 2024-01-01:2024-01-02 \
    --cache-root /tmp --bar-cache-root /tmp \
    --output /tmp/cross_scan.jsonl

# stdout (output JSONL):
{"kind":"run_start", ... "request":{"instruments":[{"side":"bid","symbol":"EURUSD"},{"side":"bid","symbol":"GBPUSD"}], "scan_id@version":"cross.lead_lag.ccf@1", ...}}
{"kind":"scan_error", ... "scan_id@version":"cross.lead_lag.ccf@1", ..., "error_code":"compute_error", "message":"expected Pair arity (ctx.bars_pair is None)", ...}
{"kind":"run_end", ..., "summary":{"results_emitted":0,"scan_errors":1,...}}
```

The error message originates from the runtime arity check inside each CROSS scan's `Scan::run` impl, triggered because `engine::run_one_with_registry` at `crates/miner-core/src/engine/mod.rs:493-495` hardcodes:

```rust
let ctx = make_scan_ctx(
    &bars,
    None,        // <-- bars_pair: None for ALL scans, including Pair-arity
    ctx_gap_manifest,
    ...
);
```

The `dispatch_pair` helper exists at `engine/gap_policy.rs:206` but no engine code calls it (grep confirms only one reference in a comment at `engine/mod.rs:490`).

**Impact:**
- All 5 CROSS scans (CROSS-01 through CROSS-05) are unreachable end-to-end via the production CLI/engine path
- Future Phase 6 (MCP / HTTP wrappers) will inherit this defect — both wrappers route through the same `miner_core::engine::run_one_with_registry` facade
- Phase 5 sweep runner cannot include CROSS scans in any meaningful sweep until CR-01 is fixed
- The "agent-operability" core constraint (PROJECT.md line 41-43: "CLI binary as a thin wrapper", "MCP server as a thin wrapper", "HTTP API as a thin wrapper") is violated for the CROSS family

**Hand-off chain that failed:**
1. `04-02-SUMMARY.md` line 42 promised: "Plan 04-07 wires Pair branch into run_one_with_registry"
2. `04-07-SUMMARY.md` line 76 deferred: "The engine's Pair branch in `run_one_with_registry` is NOT touched ... the full engine-side Pair dispatch is deferred to Plan 04-11."
3. `04-11-SUMMARY.md` contains no engine-wiring work — only goldens, byte_identical_rerun, README, and the sign-off memo
4. `04-11-PHASE-VERIFICATION.md` claims SC#4 is "Complete" but does not mention the engine Pair branch at all

**Tests that should have caught this:**
- `crates/miner-core/tests/two_leg_facade.rs` — its own docstring (lines 1-21) acknowledges it is a scaffold that does NOT invoke `engine::run_one`. The test should have been tightened in 04-07.
- `crates/miner-core/tests/arity_preflight.rs::correct_arity_pair_scan_passes_arity_preflight` (lines 210-213) — explicitly accepts ANY non-arity outcome including `Finding::ScanError`. Should have asserted `Finding::Result`.
- `crates/miner-core/tests/byte_identical_rerun.rs::byte_identical_rerun_cross_engle_granger` — manually constructs `ScanCtx { bars_pair: Some((a, b)), ... }` (lines 139-148), bypassing the engine entirely.

## Additional Findings From Code Review (04-REVIEW.md) — NOT phase-blocking but should be tracked

**WR-01:** `corr_rolling::FINDING_SHAPE.effect_extra_keys` order does not match emitted JSON order. Catalog says plan-order; wire emits BTreeMap alphabetical order. Phase 4 itself ships the contradiction; consumers introspecting `miner scans` get one key order, then receive another on every Finding. **Tracking:** fix before Phase 6 wrappers consume the catalog.

**WR-02 / WR-03 / WR-04 / WR-05:** Code-quality items in event_window doc, eom_som SOM-vs-EOM ordering edge case, and variance_ratio dead-code parameters. None affect phase goal. **Tracking:** ordinary Phase 7 hygiene.

**WR-06:** `string_label_to_raw_array` packs UTF-8 bytes into Dtype::F64 with shape = byte count. Consumers decoding per the documented `RawArray` contract will mis-interpret. Affects ADF + KPSS + ANOM-04 squared variant. **Tracking:** consider before Phase 6 wrappers expose the catalog to external consumers; flagged for human verification above.

**WR-07 / WR-08:** Engle-Granger ignores `_regression` arg + local `adf_step` diverges from canonical ADF kernel's tail formula. **Tracking:** per executor brief these are deliberately deferred to Phase 5 / HYG-01.

**IN-01 through IN-06:** Code-smell items; none affect phase goal.

## Anti-Patterns Found

Scanning files modified across Wave 1-7 plans:

| File | Pattern | Severity | Impact |
|------|---------|----------|--------|
| `engine/mod.rs:493-495` | Hardcoded `bars_pair: None` for ALL scans | BLOCKER | CR-01 above |
| `engine/gap_policy.rs:206` | `dispatch_pair` declared `pub` but unused | BLOCKER | CR-01 above |
| `engine/mod.rs:489-492` | Inline comment claims "the Pair-arity preflight rejection above prevents Pair scans from reaching here" — false: preflight accepts well-formed Pair requests | BLOCKER (inaccurate comment misleads future maintainers) | Part of CR-01 |
| `tests/two_leg_facade.rs:1-21` | Acknowledged scaffold — does NOT invoke engine::run_one | WARNING | Hides CR-01 |
| `tests/arity_preflight.rs:210-213` | Accepts any non-arity outcome, masking CR-01 | WARNING | Hides CR-01 |
| `tests/byte_identical_rerun.rs:139-148` | Manually constructs `ScanCtx { bars_pair: Some(...) }`, bypasses engine | WARNING | Hides CR-01 from SC#4 pin |
| `tests/goldens/*.jsonl` (3 files) | STUB markers + null expected.* values | INFO | Acknowledged executor-deferred per brief; SC#5 partial pass |
| `scan/cross/engle_granger/kernel.rs:213` (`adf_step`) | Local copy of ADF; not lifted from canonical kernel | INFO | Acknowledged executor-deferred per brief |

No `TODO`, `FIXME`, `XXX`, `HACK`, `TBD`, or `PLACEHOLDER` markers were found in scope outside the goldens' `_stub_note` field (which is the documented STUB gate).

## Behavioural Spot-Checks

| Behaviour | Command | Result | Status |
|-----------|---------|--------|--------|
| Workspace builds + tests pass | `cargo test --workspace` | 787 passed, 0 failed, 3 ignored | PASS |
| Registry exposes 23 scans | `miner scans --cache-root /tmp --bar-cache-root /tmp --output ...` | 23 JSONL lines emitted, all expected scan_ids present | PASS |
| All 5 CROSS scans declare `arity: pair` | `jq` on `miner scans` output | All 5 CROSS scans have `"arity":"pair"` | PASS |
| Schemas regen idempotently | `cargo run -p xtask -- gen-schema && git diff --exit-code schemas/` | Clean diff | PASS |
| Single ANOM scan via CLI emits Finding::Result | (implied; tested via byte_identical_rerun + per-scan tests) | — | PASS (via tests) |
| Single CROSS scan via CLI emits Finding::Result | `miner scan cross.lead_lag.ccf@1 --instrument EURUSD:bid --instrument GBPUSD:bid ...` | Emits `Finding::ScanError "expected Pair arity (ctx.bars_pair is None)"` | **FAIL — CR-01** |
| Single SEAS scan via CLI emits Finding::Result | (implied; tested via byte_identical_rerun + per-scan tests) | — | PASS (via tests) |

## Recommendation

**Block phase sign-off. Open a gap-closure plan (Plan 04-12) before declaring Phase 4 complete.**

**Minimum gap-closure scope (Plan 04-12):**

1. **CR-01 fix:** Wire the Pair branch in `engine::run_one_with_registry`. Suggested patch in 04-REVIEW.md:
   - After `preflight::validate_arity`, branch on `scan.arity()` → `ScanArity::Pair` dispatches to a sibling helper that:
     - Runs `GapDetector::detect` for BOTH legs in `req.instruments`
     - Calls `engine::gap_policy::dispatch_pair` (already exists) to intersect the two manifests
     - Loads two `BarFrame`s via `BarCache::get_or_build` (one per leg)
     - Builds `ScanCtx` with `bars_pair: Some((&bars_a, &bars_b))`
2. **Regression test:** Tighten `crates/miner-core/tests/two_leg_facade.rs` to invoke `engine::run_one` end-to-end with a synthetic two-leg reader; assert `Finding::Result` with `data_slice.sources.len() == 2`.
3. **Regression test:** Tighten `crates/miner-core/tests/arity_preflight.rs::correct_arity_pair_scan_passes_arity_preflight` to require `Finding::Result` (not "any non-arity outcome").
4. **Regression test:** Extend `byte_identical_rerun.rs` (or add a new test) to drive at least one CROSS scan through `engine::run_one` so the engine-side Pair contract is regression-gated.

**Estimated scope:** 1-2 days. The `dispatch_pair` helper already exists; the engine wiring is mechanical. The biggest risk is making sure cache invalidation, gap-manifest intersection, and the per-sub-range `ScanCtx` rebuild all work correctly for two legs.

**Not in gap-closure scope (defer to later phases per executor's intent):**

- The three STUB goldens (SC#5): Phase 7 once pinned-venv regen is in CI
- The Engle-Granger local `adf_step` reconciliation: Phase 5 / HYG-01
- The WR-01..WR-08 code-review warnings: track each in its own follow-up (most are Phase 7 hygiene; WR-01 should fix before Phase 6)
- The workspace-wide `clippy::pedantic` cleanup: Phase 7

---

_Verified: 2026-05-20T17:00:00Z_
_Verifier: Claude (gsd-verifier)_
