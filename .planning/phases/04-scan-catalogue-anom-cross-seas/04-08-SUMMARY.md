---
phase: 04-scan-catalogue-anom-cross-seas
plan: 08
subsystem: scan-catalogue-cross

tags:
  - rust
  - scan
  - cross
  - lead-lag
  - cointegration

requires:
  - phase: 04-scan-catalogue-anom-cross-seas
    plan: 02
    provides: "primitives::time_alignment::inner_join (CROSS-01 + D4-04), primitives::returns::log_returns, scan::cross::register_cross_scans helper stub, ScanArity::Pair + Scan::arity() + engine::preflight::validate_arity, engine::gap_policy::dispatch_pair, ScanCtx::bars_pair + bars_up_to(ts)"
  - phase: 04-scan-catalogue-anom-cross-seas
    plan: 07
    provides: "CROSS family conventions (Pattern A + Pattern D layered on two-leg dispatch); register_cross_scans helper already populated with Pearson + Spearman + OLS rolling — Plan 04-08 appends two more in alphabetical-by-id order"

provides:
  - "LeadLagCcfScan (CROSS-04) with id=cross.lead_lag.ccf, version=1, arity=Pair, single-shot cross-correlation function over [-max_lag, +max_lag] grid; effect.value = lag with maximum |CCF| (the argmax-lag), effect.extra = parallel arrays {lags, ccf} of length 2*max_lag+1"
  - "EngleGrangerScan (CROSS-05) with id=cross.cointegration.engle_granger, version=1, arity=Pair, two-step cointegration: (1) OLS β from leg_a ~ leg_b + optional intercept α, (2) ADF on residuals r_t = leg_a - β·leg_b (local self-contained `adf_step` helper inside engle_granger/kernel.rs pending Plan 04-05 ANOM-05 ADF reconciliation in 04-11), (3) OU half-life from AR(1) on residuals τ = -ln(2) / ln(φ); effect.value = adf_t_stat, effect.extra = {beta, alpha, residuals, adf_t_stat, adf_p_value, half_life, n_residuals}"
  - "scan::cross::register_cross_scans now registers all 5 CROSS scans in alphabetical-by-id order: engle_granger -> pearson_rolling -> spearman_rolling -> ols_rolling -> lead_lag.ccf (5 of 5 CROSS catalogue entries shipped)"
  - "Two new integration tests with insta envelope snapshots (scan_lead_lag, scan_engle_granger)"
  - "tests/shuffled_future_regression.rs extended with FOUR new Pair-arity proptests (lead_lag + pearson_rolling + spearman_rolling + ols_rolling) plus shared helpers (`bar_frame_named`, `extract_f64_vec`, `pair_request`, `run_pair_and_extract`) — closes Plan 04-03's deferred Pair-arity proptest coverage"

affects:
  - "04-05 (ANOM-05 ADF kernel): the local `adf_step` helper in cross/engle_granger/kernel.rs SHOULD be replaced by the canonical ANOM-05 kernel in Plan 04-11; until then both coexist (the local copy is documented in the kernel module comment + flagged in this SUMMARY for Plan 04-11)"
  - "04-11 (Phase-end integration): reconciles the local `adf_step` (either keep local copy or route through ANOM-05 ADF after numeric equivalence test); regenerates schemas to capture two new scan ids in `scans-catalogue-v1.schema.json`"
  - "Future Phase 5/6 wrappers: catalogue grows by 2 in this plan; both new entries carry `arity: \"pair\"` per D4-02"

tech-stack:
  added:
    - "(none) — Plan 04-01 already added every Phase 4 dep; this plan consumes nalgebra (OLS β) + statrs (already in tree for ADF distributions) + pure-f64 primitives"
  patterns:
    - "Pattern A — Single-leg/two-leg scan body (Scan impl + helpers + named-tests block); applied 2x"
    - "Pattern B — Kernel split (mod.rs + kernel.rs); #[inline] pub(super) pure fns + sibling tests block; applied 2x"
    - "Pattern C — Two-leg dispatch (consumes inner_join primitive + engine::gap_policy::dispatch_pair); applied 2x"
    - "Pattern D — Vector-output finding (ONE envelope per invocation with arrays in effect.extra); applied to both CROSS-04 (lags + ccf vectors) and CROSS-05 (residuals + summary arrays)"
    - "Pattern E — Per-family registrar (`register_cross_scans` body appended; registry::bootstrap() body untouched)"
    - "Pattern J — Integration test 8-step walk (insta envelope snapshot of masked finding via common::mask_volatile_fields); applied 2x"
    - "Pattern M — Shuffled-future regression proptest extension; FOUR new Pair-arity proptests added in shuffled_future_regression.rs (lead_lag + the three 04-07 rolling scans that Plan 04-03 had deferred per same-wave write-conflict mitigation)"

key-files:
  created:
    - "crates/miner-core/src/scan/cross/lead_lag/mod.rs (733 lines) — LeadLagCcfScan impl with 14 unit tests"
    - "crates/miner-core/src/scan/cross/lead_lag/kernel.rs (366 lines) — `lead_lag_ccf(a, b, max_lag) -> (lags, ccf)` kernel + 6 hand-derived unit tests"
    - "crates/miner-core/src/scan/cross/engle_granger/mod.rs (674 lines) — EngleGrangerScan impl"
    - "crates/miner-core/src/scan/cross/engle_granger/kernel.rs (561 lines) — engle_granger kernel + local `adf_step` helper (Plan 04-11 reconciliation target)"
    - "crates/miner-core/tests/scan_lead_lag.rs (160 lines) — Pattern J integration test"
    - "crates/miner-core/tests/scan_engle_granger.rs (188 lines) — Pattern J integration test"
    - "crates/miner-core/tests/snapshots/scan_lead_lag__scan_lead_lag_happy_path.snap"
    - "crates/miner-core/tests/snapshots/scan_engle_granger__scan_engle_granger_happy_path.snap"
  modified:
    - "crates/miner-core/src/scan/cross/mod.rs — `pub mod {lead_lag,engle_granger};` + re-exports; `register_cross_scans` body now registers all 5 CROSS scans alphabetical by id"
    - "crates/miner-core/tests/shuffled_future_regression.rs — added FOUR Pair-arity proptests + shared helpers (`bar_frame_named`, `extract_f64_vec`, `pair_request`, `run_pair_and_extract`); closes the deferred Pair-arity proptest debt from Plan 04-03"

key-decisions:
  - "CROSS-04 effect.value = argmax-lag (the lag offset with the largest absolute CCF) rather than max |CCF| — preserves the directional information traders care about (positive lag = leg_a leads leg_b)."
  - "CROSS-04 default max_lag = 20 bars per RESEARCH §1.9; the param schema bounds max_lag in `1..=512` to keep the result vector predictable."
  - "CROSS-05 statsmodels.tsa.stattools.coint(y0, y1) ordering: `β_y0_on_y1` (regress y0 on y1 to recover hedge ratio); we follow that convention so consumers can cross-check with the reference impl."
  - "CROSS-05 local `adf_step` helper inside engle_granger/kernel.rs: Plan 04-05 ships the canonical ANOM-05 ADF kernel; for now we ship a self-contained copy so 04-08 can land in Wave 4 without blocking on 04-05 (Wave 5). Plan 04-11 reconciliation: either prove numeric equivalence and route through the canonical kernel, OR keep the local copy with a referencing doc-comment."
  - "CROSS-05 OU half-life degenerate policy: when AR(1) coefficient φ is non-negative (φ >= 0 means no mean-reversion), emit `half_life = +inf` rather than rejecting. Same when φ = 1.0 (random walk). Documented in the kernel doc-comment."
  - "Pair-arity proptest coverage: shuffled_future_regression.rs now contains proptests for lead_lag + pearson_rolling + spearman_rolling + ols_rolling (the three 04-07 scans Plan 04-03 deferred). One thousand seeds each; compares prefix-bounded effects byte-identically across prefix-only and shuffled-tail full-array runs."
  - "Per-family registrar contract preserved: `crate::scan::registry::bootstrap()` body NOT touched. Only edits are inside `register_cross_scans` (Pattern E)."

requirements-completed:
  - CROSS-04
  - CROSS-05

deviations:
  - "Orchestrator-recovery summary: the original Wave-4 worktree dispatch hit a Claude Code worktree base-mismatch (#2924-class) where new worktrees forked at stale main HEAD `dd7c709` instead of the current `2dec2ca`, and the executor protocol's `git reset --hard` was denied by sandbox permission. After user direction, Wave 4 was re-dispatched in sequential mode directly on main (no worktree isolation). Task 1 (`618b789`) and Task 2 (`e8b05c7`) landed via two consecutive sequential-mode executor agents (Task 2 used the same sequential path). This SUMMARY.md was authored by the orchestrator during /gsd-resume-work after the second executor exited cleanly before writing the summary itself. No code modifications beyond the two committed feat() commits."
  - "Pair-arity proptest scope deviation: Plan 04-03 originally deferred Pearson/Spearman/OLS rolling proptests to 04-08 per same-wave write-conflict mitigation. This plan FULFILLS that deferred debt — all four Pair-arity proptests now live alongside the lead_lag proptest in shuffled_future_regression.rs (was: Plan 04-03 ships only the vol_rolling proptest; Plan 04-08 ships the lead_lag proptest)."

verification:
  - "Full `cargo test --workspace` GREEN (592 passed, 0 failed)"
  - "scan_lead_lag_happy_path + scan_engle_granger_happy_path GREEN with masked insta snapshots"
  - "shuffled_future_regression.rs proptest battery (5 scans × 1000 seeds) GREEN — confirms look-ahead-safety for Pair-arity CROSS scans"
  - "registry tests GREEN (`bootstrap_registers_ljung_box_scan`, `bootstrap_invokes_all_three_family_registrars`, `register_cross_scans` count assertion now expects 5 CROSS scans)"
  - "Phase 3 LjungBox golden continues to pass byte-identically (no Phase-3 regression)"
  - "15 registered scans now in `miner scans` JSONL catalogue (1 Phase 3 LjungBox + 6 ANOM + 5 CROSS + 3 SEAS)"

next-steps:
  - "Wave 4 still owes Plan 04-10 (SEAS-04 EOM/SOM + SEAS-05 ANOVA/Kruskal-Wallis + SEAS-06 event-window)"
  - "Wave 5 (04-05): ANOM-05 ADF + ANOM-06 KPSS + ANOM-07 Lo-MacKinlay variance ratio — the heavyweight statsmodels-reference plans"
  - "Wave 6 (04-06): ANOM-08 ARCH-LM + ANOM-09 Jarque-Bera"
  - "Wave 7 (04-11): goldens + schema regen + integration-test cross-checks + 22-requirement traceability table + ADF-reconciliation decision (canonical kernel vs local copy in cross/engle_granger)"
---

# Plan 04-08 — CROSS Wave-4 (lead-lag + Engle-Granger)

## What was built

Two single-shot Pair-arity CROSS scans landed under `crates/miner-core/src/scan/cross/`, completing the CROSS family at 5 of 5.

- **CROSS-04 `cross.lead_lag.ccf@1`** — single-shot cross-correlation function over the `[-max_lag, +max_lag]` lag grid. Consumes the `primitives::time_alignment::inner_join` to align the two legs on common `ts_open_utc`, computes log-returns, and emits parallel `(lags, ccf)` arrays in `effect.extra` with `effect.value = argmax-lag` (the lag offset with the maximum absolute CCF; preserves directional information).
- **CROSS-05 `cross.cointegration.engle_granger@1`** — Engle-Granger two-step cointegration. (1) `nalgebra` OLS regression `leg_a ~ leg_b` recovers hedge ratio β + optional intercept α following the statsmodels `coint(y0, y1)` ordering. (2) ADF on residuals `r_t = leg_a - β·leg_b` via a self-contained local `adf_step` helper inside `engle_granger/kernel.rs`. (3) OU half-life from AR(1) on residuals: `τ = -ln(2) / ln(φ)` with a degenerate-φ policy emitting `+inf` when φ ≥ 1.0. `effect.value = adf_t_stat`; `effect.extra` carries `{beta, alpha, residuals, adf_t_stat, adf_p_value, half_life, n_residuals}`.

Additionally, `tests/shuffled_future_regression.rs` was extended with FOUR new Pair-arity proptests (lead_lag + pearson_rolling + spearman_rolling + ols_rolling) — the three rolling proptests close the deferred-debt path from Plan 04-03.

## Recovery + resume note

This plan was completed across three orchestrator turns:

1. The original Wave-4 parallel dispatch hit a Claude Code worktree base-mismatch — new worktrees forked at stale main HEAD `dd7c709` while expected base was `2dec2ca`, and the executor's protocol-sanctioned `git reset --hard` was denied by sandbox. Per user direction, Wave 4 was re-dispatched in sequential mode directly on main (no worktree isolation).
2. The first sequential executor (commit `618b789`) shipped Task 1 (CROSS-04 + the four Pair-arity proptests) and then hit a missing-cargo-PATH wall mid-plan, exiting cleanly with the suggestion to restart.
3. The second sequential executor (commit `e8b05c7`) shipped Task 2 (CROSS-05 Engle-Granger) with the local `adf_step` helper per the orchestrator's `<notes_on_adf_dependency>` brief, exiting before writing the SUMMARY.
4. During `/gsd-resume-work`, the orchestrator verified both commits compile and test clean (cargo test --workspace: 592 passed, 0 failed), then wrote this SUMMARY.md and updated tracking. **No code modifications beyond the two committed feat() commits.**

## Status

2/2 tasks complete. 22-of-22 phase requirements: CROSS-04, CROSS-05 ticked. Registry now exposes all 5 CROSS scans; Plan 04-11 will reconcile the local `adf_step` against ANOM-05 (Wave 5) once both have landed.
