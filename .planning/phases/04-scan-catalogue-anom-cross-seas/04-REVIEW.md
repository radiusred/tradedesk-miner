---
phase: 04-scan-catalogue-anom-cross-seas
reviewed: 2026-05-20T18:00:00Z
depth: standard
files_reviewed: 95
files_reviewed_list:
  - crates/miner-cli/src/cli.rs
  - crates/miner-cli/src/main.rs
  - crates/miner-cli/src/scan_args.rs
  - crates/miner-cli/tests/cancel_overrides_error_exit_130.rs
  - crates/miner-cli/tests/scans_catalogue.rs
  - crates/miner-cli/tests/scan_subcommand_smoke.rs
  - crates/miner-cli/tests/sigint_preserves_stream.rs
  - crates/miner-core/Cargo.toml
  - crates/miner-core/src/engine/framing.rs
  - crates/miner-core/src/engine/gap_policy.rs
  - crates/miner-core/src/engine/mod.rs
  - crates/miner-core/src/engine/preflight.rs
  - crates/miner-core/src/error/codes.rs
  - crates/miner-core/src/findings/mod.rs
  - crates/miner-core/src/reader.rs
  - crates/miner-core/src/scan/anom/adf/kernel.rs
  - crates/miner-core/src/scan/anom/adf/mod.rs
  - crates/miner-core/src/scan/anom/arch_lm/kernel.rs
  - crates/miner-core/src/scan/anom/arch_lm/mod.rs
  - crates/miner-core/src/scan/anom/drawdown/kernel.rs
  - crates/miner-core/src/scan/anom/drawdown/mod.rs
  - crates/miner-core/src/scan/anom/jarque_bera/mod.rs
  - crates/miner-core/src/scan/anom/jarque_bera/kernel.rs
  - crates/miner-core/src/scan/anom/kpss/kernel.rs
  - crates/miner-core/src/scan/anom/kpss/mod.rs
  - crates/miner-core/src/scan/anom/ljung_box_sq/kernel.rs
  - crates/miner-core/src/scan/anom/ljung_box_sq/mod.rs
  - crates/miner-core/src/scan/anom/mod.rs
  - crates/miner-core/src/scan/anom/outliers/kernel.rs
  - crates/miner-core/src/scan/anom/outliers/mod.rs
  - crates/miner-core/src/scan/anom/returns/kernel.rs
  - crates/miner-core/src/scan/anom/returns/mod.rs
  - crates/miner-core/src/scan/anom/summary/kernel.rs
  - crates/miner-core/src/scan/anom/summary/mod.rs
  - crates/miner-core/src/scan/anom/variance_ratio/kernel.rs
  - crates/miner-core/src/scan/anom/variance_ratio/mod.rs
  - crates/miner-core/src/scan/anom/vol/kernel.rs
  - crates/miner-core/src/scan/anom/vol/mod.rs
  - crates/miner-core/src/scan/cross/corr_rolling/kernel.rs
  - crates/miner-core/src/scan/cross/corr_rolling/mod.rs
  - crates/miner-core/src/scan/cross/engle_granger/kernel.rs
  - crates/miner-core/src/scan/cross/engle_granger/mod.rs
  - crates/miner-core/src/scan/cross/lead_lag/kernel.rs
  - crates/miner-core/src/scan/cross/lead_lag/mod.rs
  - crates/miner-core/src/scan/cross/mod.rs
  - crates/miner-core/src/scan/cross/ols_rolling/kernel.rs
  - crates/miner-core/src/scan/cross/ols_rolling/mod.rs
  - crates/miner-core/src/scan/ljung_box/kernel.rs
  - crates/miner-core/src/scan/ljung_box/mod.rs
  - crates/miner-core/src/scan/mod.rs
  - crates/miner-core/src/scan/primitives/mod.rs
  - crates/miner-core/src/scan/primitives/raw_array.rs
  - crates/miner-core/src/scan/primitives/returns.rs
  - crates/miner-core/src/scan/primitives/time_alignment.rs
  - crates/miner-core/src/scan/registry.rs
  - crates/miner-core/src/scan/seas/anova_kw/kernel.rs
  - crates/miner-core/src/scan/seas/anova_kw/mod.rs
  - crates/miner-core/src/scan/seas/bucketing.rs
  - crates/miner-core/src/scan/seas/day_of_week/kernel.rs
  - crates/miner-core/src/scan/seas/day_of_week/mod.rs
  - crates/miner-core/src/scan/seas/eom_som/kernel.rs
  - crates/miner-core/src/scan/seas/eom_som/mod.rs
  - crates/miner-core/src/scan/seas/event_window/kernel.rs
  - crates/miner-core/src/scan/seas/event_window/mod.rs
  - crates/miner-core/src/scan/seas/hour_of_day/kernel.rs
  - crates/miner-core/src/scan/seas/hour_of_day/mod.rs
  - crates/miner-core/src/scan/seas/mod.rs
  - crates/miner-core/src/scan/seas/session/kernel.rs
  - crates/miner-core/src/scan/seas/session/mod.rs
  - crates/miner-core/tests/arity_preflight.rs
  - crates/miner-core/tests/byte_identical_rerun.rs
  - crates/miner-core/tests/gap_intersect_cross.rs
  - crates/miner-core/tests/scan_adf.rs
  - crates/miner-core/tests/scan_arch_lm.rs
  - crates/miner-core/tests/scan_corr_rolling.rs
  - crates/miner-core/tests/scan_drawdown.rs
  - crates/miner-core/tests/scan_engle_granger.rs
  - crates/miner-core/tests/scan_jarque_bera.rs
  - crates/miner-core/tests/scan_kpss.rs
  - crates/miner-core/tests/scan_ljung_box.rs
  - crates/miner-core/tests/scan_ljung_box_squared.rs
  - crates/miner-core/tests/scan_ols_rolling.rs
  - crates/miner-core/tests/scan_outliers.rs
  - crates/miner-core/tests/scan_returns_profile.rs
  - crates/miner-core/tests/scan_seas_anova_kruskal.rs
  - crates/miner-core/tests/scan_seas_day_of_week.rs
  - crates/miner-core/tests/scan_seas_eom_som.rs
  - crates/miner-core/tests/scan_seas_event_window.rs
  - crates/miner-core/tests/scan_seas_hour_of_day.rs
  - crates/miner-core/tests/scan_seas_session.rs
  - crates/miner-core/tests/scan_summary_welford.rs
  - crates/miner-core/tests/scan_variance_ratio.rs
  - crates/miner-core/tests/scan_vol_rolling.rs
  - crates/miner-core/tests/shuffled_future_regression.rs
  - crates/miner-core/tests/two_leg_facade.rs
  - schemas/findings-v1.schema.json
  - schemas/scans-catalogue-v1.schema.json
  - xtask/src/main.rs
findings:
  critical: 1
  warning: 8
  info: 6
  total: 15
status: issues_found
---

# Phase 4: Code Review Report

**Reviewed:** 2026-05-20
**Depth:** standard
**Files Reviewed:** ~95 source files
**Status:** issues_found

## Summary

Phase 4 ships 22 statistical scans across three families (ANOM, CROSS, SEAS) on top of the Phase-3 registry/engine scaffold. The numerical kernels are generally well-disciplined — sequential summation for determinism, debug_assert guards on shape invariants, NaN-conversion-to-ScanError at the scan/kernel boundary, hand-derivable test inputs with 1e-10..1e-12 tolerances cross-checked against scipy/statsmodels.

The most serious defect is **CR-01: Pair-arity scans cannot execute end-to-end through `engine::run_one`** — the engine's dispatch loop still uses the Phase-3 single-leg path and passes `bars_pair: None` to every scan; all five CROSS scans abort at their runtime arity check, surfacing as a `Finding::ScanError` envelope rather than a real result. Unit tests bypass this by constructing `ScanCtx` directly with `bars_pair: Some(...)`, masking the engine bug. The `dispatch_pair` helper exists in `gap_policy.rs` but is never invoked.

The remaining defects are smaller — wire-form key-order inconsistency between `finding_fields()` and emitted JSON in `corr_rolling`, a contradictory doc-comment in `event_window/kernel.rs`, dead/unused arguments in two helpers, a silent SOM-vs-EOM ordering ambiguity in `eom_som` when `cutoff_n > n_trading/2`, and several minor redundant-guard / dead-code-path code smells.

Per the reviewer brief: the deliberate `cross/engle_granger/kernel.rs::adf_step` stub, the three deliberately-stubbed JSONL goldens, and the `#[ignore]`d integration tests pending pinned-venv regen are NOT flagged. The deliberate Pattern-E discipline keeping `registry::bootstrap` untouched is honoured.

## Critical Issues

### CR-01: Pair-arity scans are non-functional through `engine::run_one`

**File:** `crates/miner-core/src/engine/mod.rs:419-579`
**Issue:** `run_one_with_registry`'s `GapDispatch::SubRanges` arm hardcodes single-leg dispatch:

1. Gap detection (line 348) calls `GapDetector::detect(reader, &leg.symbol, leg.side, req.window)` with `leg = req.instruments.first()` — only leg A's manifest is computed. Leg B never sees gap detection.
2. The cache load (line 444-456) calls `cache.get_or_build(reader, AggParams { symbol: &leg.symbol, side: leg.side, ... })` — only leg A's bars are loaded.
3. `make_scan_ctx` (line 493-501) is called with `bars_pair: None` (the second positional arg is hardcoded `None` at line 495), so every Pair-arity scan that reaches `Scan::run` finds `ctx.bars_pair() == None` and returns `ScanError::Kernel("expected Pair arity (ctx.bars_pair is None)")`.

The inline comment at lines 489-492 claims "the Pair-arity preflight rejection above prevents Pair scans from reaching here," but `preflight::validate_arity` (preflight.rs:122-149) only validates `instruments.len() == expected`. It does NOT short-circuit Pair scans. Any well-formed Pair request (`instruments` len 2 + Pair scan id) passes preflight, reaches the single-leg dispatch, loads only leg A's bars, and emits a `Finding::ScanError` envelope — never a real `Finding::Result`.

Evidence:
- `dispatch_pair` exists at `engine/gap_policy.rs:206` but is never called from `run_one_with_registry`.
- All Pair-scan integration tests under `crates/miner-core/tests/` (e.g. `scan_corr_rolling.rs`, `scan_engle_granger.rs`, `scan_lead_lag.rs`, `scan_ols_rolling.rs`) bypass the engine and call `<Scan>::run(&ctx, ...)` directly with a manually-constructed `ScanCtx { bars_pair: Some((a, b)), ... }`.
- The end-to-end Pair integration test `tests/two_leg_facade.rs` is explicitly a scaffold (line 1-11) and never invokes `engine::run_one`.
- The CLI's `cross` subcommand registers Pair scans (registry.rs:100 calls `register_cross_scans`) and they appear in the catalogue, so they're user-reachable.

**Fix:** Wire the Pair branch end-to-end. The minimum-viable patch:
1. After `preflight::resolve_scan`, branch on `scan.arity()`. For `ScanArity::Pair`, dispatch to a sibling function `run_one_pair_with_registry` that:
   - Runs `GapDetector::detect` for BOTH legs in `req.instruments`.
   - Calls `engine::gap_policy::dispatch_pair` (already exists) to intersect the two manifests.
   - Loads two `BarFrame`s via `BarCache::get_or_build` (one per leg).
   - Builds `ScanCtx` with `bars_pair: Some((&bars_a, &bars_b))` and `bars: &bars_a` (the single-leg-default convention per `scan/mod.rs:198-206`).
2. Update `tests/two_leg_facade.rs` to invoke `engine::run_one` end-to-end with a `FakeReader` carrying two legs, and assert the resulting `Finding::Result` envelope's `data_slice.sources.len() == 2`.

```rust
// engine/mod.rs — after preflight::validate_arity:
let outcome = match scan.arity() {
    ScanArity::Single => run_one_single_leg(...),
    ScanArity::Pair   => run_one_pair_legs(...),  // <- new branch
};
```

This is Critical because the entire CROSS family is non-functional in shipped form via the public API. The unit tests give a false-positive impression of completeness.

## Warnings

### WR-01: `corr_rolling::FINDING_SHAPE.effect_extra_keys` order does not match emitted JSON order

**File:** `crates/miner-core/src/scan/cross/corr_rolling/mod.rs:91-100`
**Issue:** The declared catalogue order is `["values", "window_starts_ms", "window_length", "threshold_crossings", "threshold"]` (plan-order), but emitted JSON via BTreeMap is alphabetical: `["threshold", "threshold_crossings", "values", "window_length", "window_starts_ms"]`. The test `pearson_rolling_result_envelope_shape` (mod.rs:654-663) asserts the alphabetical order — confirming the wire form, but contradicting the catalogue. Consumers introspecting `miner scans` to derive expected key sets will see a key-list-order that disagrees with what they then receive on every emitted Finding. Compare with every other Pattern-A scan in the same phase (e.g. `summary/mod.rs:93-97`, `returns/mod.rs:83-86`, `adf/mod.rs:106-112`) which all declare `effect_extra_keys` alphabetically.

**Fix:** Reorder to match the alphabetical BTreeMap order:
```rust
const FINDING_SHAPE: ScanFindingShape = ScanFindingShape {
    effect_extra_keys: &[
        "threshold",
        "threshold_crossings",
        "values",
        "window_length",
        "window_starts_ms",
    ],
    raw_series_keys: &["returns_a", "returns_b", "timestamps_ms"],
};
```

### WR-02: `event_window/kernel.rs` module doc-comment contradicts implementation

**File:** `crates/miner-core/src/scan/seas/event_window/kernel.rs:14-15` vs `:88`
**Issue:** The module documentation at lines 14-15 states the event's bar index is `timestamps_ms.partition_point(|&t| t <= event) minus one` — i.e. "the most-recent bar whose timestamp is at-or-before the event." The implementation at line 88 uses `partition_point(|&t| t < event_ts)` (strict less-than), with no `minus one` adjustment — returning the index of the first bar AT-OR-AFTER the event. The implementation matches the later lines 18-22 ("the event bar IS the first bar of the post window") but contradicts the earlier semantic paragraph. Either the docs or the implementation needs to change; tests rely on the actual behaviour (line 226-241 `event_between_bars_resolves_to_next_bar`).

**Fix:** Update the lines 14-15 doc-comment to match the implementation:

```rust
//! 1. Resolve its bar index by `timestamps_ms.partition_point(|&t| t < event)`
//!    — i.e. the index of the first bar at-or-after the event timestamp.
```

### WR-03: `eom_som` silently masks SOM buckets when `cutoff_n > n_trading / 2`

**File:** `crates/miner-core/src/scan/seas/eom_som/kernel.rs:80-92`
**Issue:** When `cutoff_n` is large enough to overlap (e.g. `cutoff_n = 15` with a 22-trading-day month), bars in the overlapping middle satisfy BOTH `from_end < cutoff_n` AND `pos < cutoff_n`. The if-else returns EOM bucket first, silently masking the SOM assignment. There is no validation that `cutoff_n <= n_trading / 2`. A user supplying `cutoff_n = 30` on a normal month will get every trading day classified as EOM, never SOM. Not catastrophic, but documented behavior diverges from "first N vs last N trading days" intuition; no doc-comment or test pins this edge case.

**Fix:** Either explicitly document the precedence rule in the module-doc-comment, or validate at the scan-body level that `cutoff_n * 2 <= min_n_trading_in_window` and reject otherwise. At minimum, add a test asserting the overlap behavior so the contract is pinned.

### WR-04: `variance_ratio::robust_variance` takes an unused `centred` parameter

**File:** `crates/miner-core/src/scan/anom/variance_ratio/kernel.rs:154,174`
**Issue:** `robust_variance(centred: &[f64], u_sq: &[f64], ...)` receives `centred` as the first parameter but only reads `u_sq` (the squared residuals). Line 174 `let _ = centred;` is a deliberate discard to silence an unused-variable warning. The parameter is dead weight — every caller has to pass the slice but it's never read.

**Fix:** Remove the `centred` parameter from the signature; update the single call site at line 125:
```rust
fn robust_variance(u_sq: &[f64], sum_u_sq: f64, k: usize, n: usize) -> f64 {
    // ... (drop `centred` parameter and the `let _ = centred;` at line 174)
}
```

### WR-05: `variance_ratio` has unreachable `n < 2` check after `n < 4` guard

**File:** `crates/miner-core/src/scan/anom/variance_ratio/kernel.rs:88-90`
**Issue:** Line 74 already requires `n >= 4`. Line 88 then re-checks `if n < 2 { return Err... }` — unreachable dead code. Indicates the bounds checks were copy-pasted without consolidating.

**Fix:** Delete the unreachable check at lines 88-90.

### WR-06: ADF/KPSS `string_label_to_raw_array` packs UTF-8 bytes into a `Dtype::F64` array with `shape = [N_bytes]`

**File:** `crates/miner-core/src/scan/anom/adf/mod.rs:346-355`, `crates/miner-core/src/scan/anom/kpss/mod.rs:302-311`
**Issue:** The helper packs `label.as_bytes()` into `RawArray { dtype: Dtype::F64, shape: vec![bytes.len() as u64], data: ... }`. The shape value is the byte count, not the f64-element count, and the dtype lies (the bytes are UTF-8, not LE f64). A consumer decoding this per the documented `RawArray` contract (D-01: "data: base64-encoded LE f64 bytes; shape: f64-element count") would read it as `shape[0]/8` (truncated) f64 values, getting garbage. The schema is also misleading — consumers can't tell from the wire form that a particular RawArray is "really a string." The same wire-form trick is used by ANOM-04 squared `series_kind` per the comment chain. The integration tests presumably decode by `str::from_utf8(&data.0)` directly, bypassing the shape/dtype invariant.

**Fix:** Either:
1. Extend `Dtype` to include a `Utf8Bytes` variant and use `shape: vec![bytes.len()]`. Additive enum change.
2. Encode the discriminator as a 1-element F64 array (the pattern `returns/mod.rs:265-277` already uses for `variant_label`). Drop the string-bytes-in-F64 trick entirely.

The current state encodes invalid metadata into the envelope: shape says "N elements" but each element is 1 byte, not 8. Consumers writing a generic decoder will mis-interpret.

### WR-07: `engle_granger::engle_granger` ignores `_regression` argument

**File:** `crates/miner-core/src/scan/cross/engle_granger/kernel.rs:100`
**Issue:** The function signature accepts `_regression: AdfRegression` but the entire body ignores it — the leading underscore acknowledges this. The module-doc-comment promises `"ct"` is "accepted as a parameter but downgraded to 'c' in the local kernel," implying users supplying `regression=ct` get a constant-only ADF as documented. This is the intent — but there's no compile-time linkage between "ct is accepted" and "ct is silently downgraded." A future maintainer might assume the parameter is wired and re-introduce a branch on it without realising the test expects identical output for `c` vs `ct`. Test `engle_granger_invalid_regression_param` (mod.rs:642-651) only checks rejection of `regression="x"`; no test pins `c == ct` output equality.

**Fix:** Either (a) extract the always-Constant behaviour into a private const and pass that constant from the public function (no `_regression` argument), or (b) add an explicit test pinning `engle_granger(y, x, ConstantTrend) == engle_granger(y, x, Constant)` so the "silent downgrade" promise is regression-gated.

### WR-08: `engle_granger`'s mid-plan `mackinnon_p_constant` diverges from the canonical ADF kernel's tail behaviour

**File:** `crates/miner-core/src/scan/cross/engle_granger/kernel.rs:292-298` vs `crates/miner-core/src/scan/anom/adf/kernel.rs:415-429`
**Issue:** The reviewer brief acknowledges this kernel is a stub pending Plan 5 reconciliation, BUT the two implementations now produce DIFFERENT p-values for the same τ in the lower tail, which is a behavioural drift that downstream consumers will see. The canonical ADF uses `0.02 * Φ(τ - c1)` (line 424) with a clamp to [0.0, 0.01] for τ ≤ c1; the engle_granger stub uses `0.01 * exp(τ - c1)` (line 296) with the same clamp. The two produce different small-p-values for the same input. The brief allows the stub to stay until Plan 5, but the two functions should at least share a common tail formula so the reconciliation is "delete the duplicate" rather than "delete one and silently change behaviour."

**Fix:** When reconciling in Plan 5, also harmonise the tail-asymptotic so the engle_granger result is byte-identical to the canonical ADF for the same `regression="c"` config. Until then, mark the divergence explicitly in the engle_granger module-doc-comment (it currently says "accuracy ≈ 1e-3" but doesn't say "the formula form differs from the canonical kernel"). If both formulas are kept, add a test pinning the divergence numerically.

## Info

### IN-01: `jarque_bera/mod.rs` has a redundant N=0 guard before N<2 check

**File:** `crates/miner-core/src/scan/anom/jarque_bera/mod.rs:128-138`
**Issue:** Lines 128-133 check `if n_closes == 0` and return InsufficientData; immediately afterwards lines 134-138 check `if n_closes < 2` which subsumes the first check. The first branch is dead — a zero-length close series would be caught by the second branch with the same `InsufficientData` label.

**Fix:** Delete lines 128-133; keep the `n_closes < 2` check.

### IN-02: `LeadLagResult.argmax_value == NaN` is silently accepted when all CCF entries are NaN

**File:** `crates/miner-core/src/scan/cross/lead_lag/kernel.rs:179-186`
**Issue:** When every `ccf_values` entry is NaN (the explicit degenerate-variance early-return at line 125-136 returns `argmax_value: NaN, argmax_lag: 0` — but the general path's fold at line 179-185 has a different outcome). For an all-NaN ccf produced elsewhere (e.g. a future code path computing ccf from a different source), the fold's `v.abs() > acc.1.abs()` returns false for any NaN comparison; the accumulator stays at `(0, ccf_values[0])` — `argmax_lag = lags[0] = -max_lag`, not 0. The two NaN paths report different `argmax_lag` for what should be equivalent "undefined" cases.

**Fix:** Add an explicit NaN-handling sweep after the fold:
```rust
let argmax_value = ccf_values[argmax_idx];
if argmax_value.is_nan() {
    // Match the early-return convention: argmax_lag = 0 for undefined CCF.
    return LeadLagResult { lags, ccf_values, argmax_lag: 0, argmax_value: f64::NAN, max_lag };
}
```

### IN-03: `engle_granger_perfect_fit_ou_half_life_inf` test accepts both INFINITY and NaN sentinels

**File:** `crates/miner-core/src/scan/cross/engle_granger/mod.rs:524-530`
**Issue:** The test assertion accepts `is_infinite() || is_nan()` but the kernel `ou_half_life_from_residuals` (kernel.rs:334-365) deterministically returns `f64::INFINITY` for every degenerate branch — never NaN. The overly-permissive `|| is_nan()` masks a future regression where the function accidentally returns NaN.

**Fix:** Tighten to `assert!(res.ou_half_life.is_infinite())`. Drop the `|| is_nan()` slack.

### IN-04: `summary/mod.rs::Step 4` has a dead `n_closes < 2` check inside a branch already requiring `n_closes >= 1`

**File:** `crates/miner-core/src/scan/anom/summary/mod.rs:138-142`
**Issue:** Lines 119-123 guard `n_closes == 0`; lines 138-142 in the `LogReturns` arm check `if n_closes < 2`. The N=1 case is allowed past the first guard and rejected by the second only when `series == log_returns`. Functionally correct but the structure obscures intent — the N=1 trivial-result behaviour (lines 166-173) only applies to `series == close`, and that's not visible from the param resolution site.

**Fix:** Comment the asymmetry clearly, or refactor so the per-series N-guard lives next to the series-specific value computation rather than scattered between Step 3 and Step 4.

### IN-05: `vol_rolling::run` calls `kernel::rolling_std(slice_of_window, window)` once per window — O(n*window) work re-done

**File:** `crates/miner-core/src/scan/anom/vol/mod.rs:174-186`
**Issue:** The per-window cancel-poll loop calls `kernel::rolling_std(slice, window)` for each window, where `slice.len() == window`. The kernel returns a 1-element vec containing the single window's std — but it allocates `Vec::with_capacity(1)` every iteration. Out of v1 scope per the review brief (performance), and the trade-off is intentional per the cancel-poll discipline ("cheap insurance"), but worth documenting why the kernel's natural batch interface isn't used directly. A future maintainer might "optimise" by removing the per-window cancel-poll, regressing the cancel-latency contract.

**Fix:** Add an inline comment explaining the per-window kernel call is intentional (cancel-poll boundary, not a perf bug). Better: factor a `rolling_std_single_window(slice) -> f64` helper that doesn't allocate the 1-element Vec.

### IN-06: `string_label_to_raw_array` is duplicated across ADF and KPSS scans

**File:** `crates/miner-core/src/scan/anom/adf/mod.rs:346-355`, `crates/miner-core/src/scan/anom/kpss/mod.rs:302-311`
**Issue:** Both scans inline an identical helper function. Per the Pitfall 9 / D4-06 "move, don't rewrite" discipline that motivated `primitives/raw_array.rs::f64_slice_to_raw_array`, this should also be lifted to `primitives/raw_array.rs` if it's going to stick (see WR-06 — the trick itself is suspect, so the right fix is to remove it; but if it stays, dedupe it).

**Fix:** Decide WR-06 first. If the string-bytes-in-F64 trick is replaced by a proper Dtype variant, this duplication goes away naturally.

---

_Reviewed: 2026-05-20_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
