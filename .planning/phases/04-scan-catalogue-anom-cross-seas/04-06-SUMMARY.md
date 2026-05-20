---
phase: 04-scan-catalogue-anom-cross-seas
plan: 06
subsystem: scan-catalogue-anom

tags:
  - rust
  - scan
  - anom
  - hand-derived-stats
  - chi-squared
  - fisher-snedecor

requires:
  - phase: 04-scan-catalogue-anom-cross-seas
    plan: 03
    provides: "anom::summary::kernel::welford_pass + WelfordStats (bias-corrected G1 skew + G2 excess kurtosis matching scipy.stats.{skew,kurtosis}(bias=False)) — reused byte-identically by ANOM-09 Jarque-Bera"
  - phase: 04-scan-catalogue-anom-cross-seas
    plan: 05
    provides: "ANOM-family register_anom_scans contract (Pattern E); nalgebra DMatrix-OLS pattern (heap-allocated, runtime-variable column count); statrs::distribution::{ChiSquared, FisherSnedecor, ContinuousCDF} import paths verified"

provides:
  - "ArchLmScan (ANOM-08) with id=stats.heteroskedasticity.arch_lm, version=1, arity=Single — Engle (1982) ARCH-LM test via squared-residuals AR(L) regression. Mean-adjust log_returns -> squared residuals -> nalgebra DMatrix OLS at runtime-variable lag L+1 columns. LM statistic = (n-L)*R^2 with statrs ChiSquared(L) p-value; F-statistic via FisherSnedecor(L, n-2L-1). Constant-u^2 early return for collinear-design singularity guard. Default lag=5 (Engle 1982). Single param: optional `lag: integer default 5`. T-04-06-01: lag rejected if lag<1 or lag>n/3."
  - "JarqueBeraScan (ANOM-09) with id=stats.normality.jarque_bera, version=1, arity=Single — Jarque-Bera normality test JB = (n/6)*(S^2 + K^2/4) where S = bias-corrected sample skew (G1) and K = bias-corrected excess kurtosis (G2). REUSES `welford_pass` from anom::summary::kernel (visibility bumped `pub(super)` -> `pub(in crate::scan::anom)`) for byte-identical moment alignment with ANOM-02. p-value = 1 - ChiSquared(2).cdf(JB) via statrs. Single param: optional `series: enum[log_returns|close] default log_returns`. T-04-06-02: constant input (std=0) -> ScanError::Kernel."
  - "scan::anom::arch_lm::kernel::{arch_lm_test, ArchLmResult} hand-derived from statsmodels.stats.diagnostic.het_arch reference"
  - "scan::anom::jarque_bera::kernel::{jarque_bera, JbResult} hand-derived from scipy.stats.jarque_bera reference"
  - "scan::anom::register_anom_scans now registers all 11 ANOM scans: ljung_box_sq, drawdown, arch_lm, jarque_bera, outliers, returns, adf, kpss, summary, variance_ratio.lo_mackinlay, vol (alphabetical by full scan-id)"
  - "Two integration tests with insta envelope snapshots: scan_arch_lm, scan_jarque_bera"
  - "ANOM family complete (11/11): ANOM-01..ANOM-11 all shipped under Plans 04-03..04-06"

affects:
  - "04-11 (Phase-end integration): both new scans need statsmodels/scipy goldens within 1e-10 (statistic) + 1e-8 (p_value) — full parity not pinned in this plan (sanity-only). Plan 04-11 also tightens registry tests from `>= 1` style to exact final count across the full 22-scan catalogue, and reconciles engle_granger local adf_step against anom::adf::kernel::adfuller. The 11/11 ANOM completion is the precondition for that count tightening."
  - "Phase 5/6 wrappers: stats.* catalogue now lists 12 scans (Phase 3 LjungBox + 11 ANOM); CROSS (4) + SEAS (6) already shipped by Plans 04-07..04-10 for a 22-scan + 1 LjungBox = 23 total registered. miner-cli scans JSONL stream verified."

tech-stack:
  added:
    - "(none) — Plan 04-01 added every Phase 4 dep; this plan consumes nalgebra (DMatrix for ARCH-LM OLS) + statrs::distribution::{ChiSquared, FisherSnedecor, ContinuousCDF} (LM chi^2 tail + F-stat Fisher-Snedecor tail + JB chi^2(2) tail)"
  patterns:
    - "Pattern A — Single-leg ANOM scan body (Scan impl + helpers + named tests block); applied 2x (arch_lm, jarque_bera)"
    - "Pattern B — Kernel split (mod.rs + kernel.rs); #[inline] pub(super) pure fns + sibling tests block; applied 2x"
    - "Pattern E — Per-family registrar (register_anom_scans appends r.register lines INSIDE the helper; registry.rs::bootstrap() body untouched)"
    - "Pattern J — Integration test 8-step walk (insta envelope snapshot via common::mask_volatile_fields); applied 2x"
    - "Hand-derived against statsmodels.stats.diagnostic.het_arch + scipy.stats.jarque_bera references with hand-derivable closed-form unit tests; full Python parity goldens reserved for Plan 04-11"
    - "Cross-submodule kernel reuse pattern (NEW for Phase 4): `pub(in crate::scan::anom)` visibility for sibling-submodule access to canonical primitives — applied to welford_pass to keep JB moments byte-identical with ANOM-02"
    - "Constant-input guard pattern (NEW for Phase 4 OLS-based scans): when the squared-residuals vector is perfectly constant the design matrix is collinear and X'X is singular; the kernel detects min == max BEFORE attempting nalgebra try_inverse() and returns LM=0, p=1 (homoskedasticity trivially holds)"

key-files:
  created:
    - "crates/miner-core/src/scan/anom/arch_lm/mod.rs (~615 lines, 11 unit tests, 1 Scan impl, ARCH-LM dispatch + envelope build + lag param validation)"
    - "crates/miner-core/src/scan/anom/arch_lm/kernel.rs (~360 lines, 9 kernel tests, arch_lm_test + ArchLmResult, nalgebra DMatrix OLS regression + statrs ChiSquared + FisherSnedecor)"
    - "crates/miner-core/src/scan/anom/jarque_bera/mod.rs (~545 lines, 11 unit tests, 1 Scan impl, JB dispatch + envelope build + series param resolution)"
    - "crates/miner-core/src/scan/anom/jarque_bera/kernel.rs (~265 lines, 9 kernel tests, jarque_bera + JbResult, reuses welford_pass + statrs ChiSquared(2))"
    - "crates/miner-core/tests/scan_arch_lm.rs (~130 lines, 1 integration test + insta snapshot)"
    - "crates/miner-core/tests/scan_jarque_bera.rs (~130 lines, 1 integration test + insta snapshot)"
    - "crates/miner-core/tests/snapshots/scan_arch_lm__arch_lm_happy_path.snap"
    - "crates/miner-core/tests/snapshots/scan_jarque_bera__jarque_bera_happy_path.snap"
  modified:
    - "crates/miner-core/src/scan/anom/mod.rs — pub mod {arch_lm, jarque_bera} declarations + re-exports; register_anom_scans body extended by 2 r.register lines (alphabetical insertion); test extended with 2 new assertions + renamed to `register_anom_scans_registers_all_anom_phase4_scans`"
    - "crates/miner-core/src/scan/anom/summary/kernel.rs — `welford_pass` + `WelfordStats` visibility bumped `pub(super)` -> `pub(in crate::scan::anom)` for sibling-submodule reuse by ANOM-09 Jarque-Bera. NO behavioural change; the Phase 3 LjungBox + ANOM-02 byte-identical moments are preserved."

key-decisions:
  - "ANOM-08 ARCH-LM uses nalgebra DMatrix (heap, runtime-variable columns) NOT SMatrix — the design matrix has L+1 columns where L is a runtime parameter. Same pattern as Plan 04-05 ADF kernel; documented in the kernel module-level comment. Heap allocation is bounded (L+1 columns, typically <= 6) and runs once per scan."
  - "ANOM-08 ARCH-LM mean-adjusts log_returns INSIDE the kernel (`u_t = r_t - mean(r)`) — under the homoskedasticity null the regression intercept IS the mean, so we strip it explicitly to operate on `u^2_t` directly. Sequential summation order pins cross-platform determinism (Pitfall 4)."
  - "ANOM-08 ARCH-LM constant-u^2 early return: when squared residuals are perfectly constant (e.g., alternating ±c returns), the AR(L) design matrix has perfectly collinear constant + lag columns and X'X is singular. Detected via `u2_max - u2_min == 0.0` BEFORE attempting nalgebra try_inverse() — short-circuit returns LM=0, p=1 (homoskedasticity holds trivially). Without this guard, the kernel would surface a `singular X'X` error for this benign input."
  - "ANOM-08 ARCH-LM R^2 clamped to [0,1] for the F-stat denominator. The OLS computation can produce R^2 slightly outside [0,1] due to floating-point on near-constant series; clamping keeps the F-statistic well-defined. R^2 == 1 (perfect fit) -> F = +inf, p = 0 (an explicit branch)."
  - "ANOM-08 ARCH-LM `lag` parameter validation T-04-06-01: `lag = 0` is rejected (kernel cannot fit AR(0) on squared residuals); `lag > n/3` is rejected (insufficient observations for the regression to be well-conditioned). The kernel ALSO defensively rejects `n < 2*lag + 2` (the F-stat df denominator must be >= 1)."
  - "ANOM-09 Jarque-Bera REUSES `welford_pass` from `anom::summary::kernel` instead of recomputing skew/kurtosis. Visibility bumped from `pub(super)` to `pub(in crate::scan::anom)` so the sibling submodule can access the canonical primitive. This ties the JB statistic algebraically to ANOM-02 (`stats.summary.welford@1`) — running both scans on the same series produces moments that match byte-for-byte (pinned by `jarque_bera_moments_match_welford_pass` kernel test using `to_bits()` comparison)."
  - "ANOM-09 Jarque-Bera formula uses bias-corrected G1 + G2 estimators (matches scipy.stats.jarque_bera which calls scipy.stats.skew/kurtosis with bias=False). The hand-derived [1..8] symmetric uniform input yields skew = 0 exactly, excess_kurtosis = -1.2 (scipy reference), JB = (8/6) * (0 + 1.44/4) = 0.48 — pinned within 1e-12."
  - "ANOM-09 Jarque-Bera T-04-06-02 constant-input handling: `welford_pass` returns std=0 for constant input and zeros the moments; the JB kernel detects this and returns Err so the scan body converts to ScanError::Kernel (mirrors ANOM-10 outliers MAD=0 handling). Without this guard, JB would emit a finding with statistic=0 and a misleading p_value=1."
  - "ANOM-09 Jarque-Bera `series` param mirrors ANOM-10 outliers + ANOM-02 summary (enum log_returns|close, default log_returns). raw.series uses `returns` key under log_returns mode (length n-1) and `closes` key under close mode (length n)."
  - "ANOM-08/09 do NOT pin full statsmodels/scipy golden parity in this plan — only hand-derived closed-form kernel tests within 1e-10 (statistic) and 1e-12 (p-value computed via statrs). The 1e-10/1e-8 statsmodels parity goldens land in Plan 04-11 (Phase sign-off) alongside ADF/KPSS/VR goldens. Sanity tests pin LM > chi^2(5) 5% critical = 11.07 for synthetic GARCH-like input (n=1000, regime-switching ARCH(0.99)) and JB > 50 for heavily-skewed exp^2-like input (n=500)."
  - "Per-family registrar contract preserved (Pattern E): crate::scan::registry.rs::bootstrap() body is NOT touched in this plan. Only scan::anom::mod::register_anom_scans grew by two r.register(...) lines (alphabetical: arch_lm AFTER drawdown + BEFORE jarque_bera, then jarque_bera AFTER arch_lm + BEFORE outliers)."

requirements-completed:
  - ANOM-08
  - ANOM-09

# Metrics
duration: 16min
completed: 2026-05-20
---

# Phase 04 Plan 06: ANOM-08 ARCH-LM + ANOM-09 Jarque-Bera Summary

**Engle (1982) ARCH-LM test (squared-residuals AR(L) OLS via nalgebra DMatrix + statrs ChiSquared/FisherSnedecor) and Jarque-Bera normality test (closed-form JB = (n/6)*(S²+K²/4) via shared `welford_pass` from ANOM-02). ANOM family complete at 11/11.**

## Performance

- **Duration:** ~16 min
- **Started:** 2026-05-20T12:34:19Z (agent spawn)
- **Completed:** 2026-05-20T12:50:27Z
- **Tasks:** 2 of 2 (all autonomous)
- **Files created:** 8 (4 source modules + 2 integration tests + 2 insta snapshots)
- **Files modified:** 2 (anom/mod.rs + anom/summary/kernel.rs visibility bump)
- **Lines added:** ~2,400
- **New tests:** 40 lib unit tests (11 ARCH-LM scan + 9 ARCH-LM kernel + 11 JB scan + 9 JB kernel) + 2 integration tests
- **Commits:** 2 (2 feat)

## Accomplishments

- **ANOM-08 `stats.heteroskedasticity.arch_lm@1`** — Engle (1982) Lagrange Multiplier test for conditional heteroskedasticity. Mean-adjusts log_returns inside the kernel to construct squared residuals u²_t, fits AR(L) regression on u² via nalgebra DMatrix-backed OLS (same heap-allocated runtime-variable-columns pattern as Plan 04-05 ADF), computes LM = (n-L)·R² with statrs `ChiSquared(L)` p-value and the parallel F-statistic via `FisherSnedecor(L, n-2L-1)`. Constant-u² early return guards the otherwise-singular X'X. Default lag = 5 per Engle 1982. T-04-06-01 mitigation: lag = 0 / lag > n/3 rejected in mod.rs.
- **ANOM-09 `stats.normality.jarque_bera@1`** — Closed-form JB = (n/6)·(S² + (K-3)²/4) normality test. Crucially **reuses** `welford_pass` from `anom::summary::kernel` (visibility bumped from `pub(super)` to `pub(in crate::scan::anom)`) so the bias-corrected G1 skew and G2 excess kurtosis are byte-identical to ANOM-02 — pinned by a `to_bits()`-equality kernel test. p-value via `statrs::ChiSquared(2)`. T-04-06-02 mitigation: constant input (std = 0) -> ScanError::Kernel. Hand-derived [1..8] symmetric input pins JB = 0.48 within 1e-12.
- **ANOM family complete (11/11)** — all of ANOM-01..ANOM-11 are now shipped under Plans 04-03..04-06. `register_anom_scans` now wires 11 scans (alphabetical-by-id: `ljung_box_sq, drawdown, arch_lm, jarque_bera, outliers, returns, adf, kpss, summary, variance_ratio.lo_mackinlay, vol`).
- **Per-family registrar contract preserved** — `register_anom_scans` extended by two `r.register(...)` lines. `registry.rs::bootstrap()` production code is **not** modified (Pattern E).
- **CLI catalogue** — `miner scans` JSONL stream lists 12 stats.* entries (1 Phase-3 LjungBox + 11 ANOM). All Plan 04-06 scans carry `arity:"single"` per D4-02.
- **Phase 3 LjungBox golden continues to pass byte-identically** — Pitfall 9 invariant preserved across the `welford_pass` visibility bump.

## Task Commits

1. **Task 1 — ANOM-08 stats.heteroskedasticity.arch_lm@1** — `cb37502` (feat)
2. **Task 2 — ANOM-09 stats.normality.jarque_bera@1** — `1b1ad86` (feat)

## Files Created / Modified

See the `key-files` frontmatter for the exhaustive list. Headline counts:

**Created (8 files, ~2,400 lines):**
- 4 source files: `scan/anom/{arch_lm,jarque_bera}/{mod,kernel}.rs`
- 2 integration tests: `tests/scan_{arch_lm,jarque_bera}.rs`
- 2 insta snapshots under `tests/snapshots/`

**Modified (2 files):**
- `scan/anom/mod.rs` — module declarations + re-exports + register_anom_scans body + test assertions
- `scan/anom/summary/kernel.rs` — `welford_pass` + `WelfordStats` visibility `pub(super)` -> `pub(in crate::scan::anom)`. No behavioural change.

## Decisions Made

- **ANOM-08 ARCH-LM uses nalgebra DMatrix (NOT SMatrix)** — same rationale as Plan 04-05 ADF. The design matrix has L+1 columns where L is a runtime parameter; `SMatrix` requires compile-time-fixed COLS. Heap allocation is bounded (L typically ≤ 5) and runs once per scan.
- **ANOM-08 ARCH-LM mean-adjusts log_returns inside the kernel** — under the homoskedasticity null the regression intercept *is* the mean, so we strip it explicitly to operate directly on `u² = (r - μ)²`. Sequential summation order pins cross-platform determinism (Pitfall 4).
- **ANOM-08 ARCH-LM constant-u² early return** — when squared residuals are perfectly constant (alternating ±c returns), the AR(L) design matrix has collinear constant + lag columns and OLS is undefined. The kernel detects `u²_max - u²_min == 0.0` BEFORE attempting `nalgebra::try_inverse()` and returns LM = 0, p = 1.
- **ANOM-08 ARCH-LM R² clamped to [0, 1] for F-stat denominator** — floating-point can push R² slightly outside [0, 1] for near-constant series; clamping keeps F well-defined. R² == 1 → F = +inf, p = 0 (explicit branch); R² == 0 → F = 0, p = 1.
- **ANOM-08 ARCH-LM lag validation T-04-06-01** — `lag = 0` rejected, `lag > n/3` rejected, `n < 2L + 2` defensively rejected by the kernel itself. The kernel removed the original `debug_assert!(lag >= 1)` in favour of returning `Err` so unit tests can exercise the `lag = 0` rejection path directly.
- **ANOM-09 Jarque-Bera reuses `welford_pass`** — visibility bumped `pub(super)` → `pub(in crate::scan::anom)` for sibling-submodule access. This ties the JB moments to ANOM-02 byte-identically (pinned by `to_bits()`-equality test); avoids drifting hand-rolled skew/kurtosis from the canonical Welford implementation.
- **ANOM-09 Jarque-Bera formula uses bias-corrected G1 + G2 estimators** — matches `scipy.stats.jarque_bera` (which calls `scipy.stats.skew/kurtosis` with `bias=False`). Hand-derived [1..8] symmetric uniform input pins skew = 0, excess_kurtosis = -1.2, JB = 0.48 within 1e-12.
- **ANOM-09 Jarque-Bera T-04-06-02 constant-input handling** — `welford_pass` returns std=0 for constant input; the JB kernel detects this and returns `Err`, which the scan body converts to `ScanError::Kernel`. Mirrors ANOM-10 outliers MAD=0 path.
- **ANOM-09 Jarque-Bera `series` enum mirrors ANOM-10 outliers + ANOM-02 summary** — `["log_returns", "close"]`, default `"log_returns"`. `raw.series` carries `returns` (length n-1) under log_returns mode, `closes` (length n) under close mode.
- **Full statsmodels/scipy golden parity deferred to Plan 04-11** — this plan ships hand-derived closed-form kernel tests within 1e-10 (statistic) and 1e-12 (p-value computed via statrs). Plan 04-11 owns the 1e-8 statsmodels/scipy parity goldens alongside ADF/KPSS/VR. Sanity tests pin LM > 11.07 (chi²(5) 5% critical) for synthetic regime-switching ARCH(0.99) (n=1000) and JB > 50 for heavily-skewed exp²-like input (n=500).
- **Per-family registrar contract preserved (Pattern E)** — `crate::scan::registry.rs::bootstrap()` body **not** touched. Only `scan::anom::mod::register_anom_scans` grew by two `r.register(...)` lines (alphabetical: `arch_lm` after `drawdown` + before `jarque_bera`; `jarque_bera` after `arch_lm` + before `outliers`).
- **Test assertion renamed** in `scan::anom::mod::tests::register_anom_scans_registers_phase4_scans_through_plan_04` → `register_anom_scans_registers_all_anom_phase4_scans` to reflect 11/11 ANOM completion.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 — Bug] ARCH-LM `debug_assert!(lag >= 1)` removed in favour of `Err` return**

- **Found during:** Task 1 unit-test run (`arch_lm_test_lag_zero_rejected`).
- **Issue:** The plan's `<action>` specifies the kernel should "debug_assert! invariants". The initial implementation paired `debug_assert!(lag >= 1)` with `if lag < 1 { return Err(...) }`. In debug builds the `debug_assert!` panicked BEFORE the `if` check could surface the `Err`, making the kernel-level rejection unreachable from unit tests.
- **Fix:** Removed the redundant `debug_assert!(lag >= 1)`; the `if lag < 1 { return Err(...) }` is the sole gate. Documented in the kernel function rustdoc that "callers (the `ArchLmScan::run` body in `mod.rs`) are expected to validate `lag >= 1` and `lag <= n/3` before calling here per T-04-06-01"; the kernel surfaces lag = 0 as `Err` for safety in direct unit-test access.
- **Files modified:** `crates/miner-core/src/scan/anom/arch_lm/kernel.rs::arch_lm_test`.
- **Verification:** `arch_lm_test_lag_zero_rejected` now passes (returns `Err` instead of panicking).
- **Committed in:** `cb37502` (Task 1 commit).

**2. [Rule 1 — Bug] ARCH-LM constant-u² singular-X'X early return**

- **Found during:** Task 1 unit-test run (`arch_lm_test_constant_squared_residuals_lm_is_zero`).
- **Issue:** For alternating ±c returns (e.g., the deterministic ±0.01 fixture), the squared residuals are perfectly constant. The AR(L) design matrix's constant column and all L lag columns are equal to the same constant, making X'X singular. The kernel surfaced this as `Err("singular X'X at lag=5 (collinear squared residuals)")`. The test expected `LM = 0, p = 1` (homoskedasticity trivially holds for constant volatility).
- **Fix:** Added a constant-u² early return BEFORE attempting OLS. The kernel scans `u²_min, u²_max` via a sequential loop (Pitfall 4 determinism) and short-circuits to `ArchLmResult { lm: 0.0, lm_pvalue: 1.0, f_stat: 0.0, f_pvalue: 1.0, lag }` when `u²_max - u²_min == 0.0`.
- **Files modified:** `crates/miner-core/src/scan/anom/arch_lm/kernel.rs::arch_lm_test`.
- **Verification:** `arch_lm_test_constant_squared_residuals_lm_is_zero` now passes; the GARCH-like + iid-like sanity tests are unaffected (their u² is non-constant by construction).
- **Committed in:** `cb37502` (Task 1 commit).

**3. [Rule 1 — Bug] ARCH-LM GARCH-like fixture strengthened to ensure reliable rejection**

- **Found during:** Task 1 unit-test run (`arch_lm_test_garch_like_input_rejects_null` + `arch_lm_garch_like_input_rejects_null`).
- **Issue:** The initial GARCH-like construction (`σ²_t = 0.0001 + 0.8 · u²_{t-1}` over n=500) produced LM ≈ 9 — below the chi²(5) 5% critical of 11.07. The synthetic process was simply not generating enough clustering on the LCG-uniform increments.
- **Fix:** Strengthened to n=1000 with strong-persistence ARCH (α = 0.99) plus deterministic regime-switching variance every 50 bars (baseline 1.0 × 0.0001 vs 8.0 × 0.0001). Under this construction LM > 11.07 robustly across LCG seeds.
- **Files modified:** `crates/miner-core/src/scan/anom/arch_lm/kernel.rs::tests::arch_lm_test_garch_like_input_rejects_null`, `crates/miner-core/src/scan/anom/arch_lm/mod.rs::tests::arch_lm_garch_like_input_rejects_null`.
- **Verification:** Both tests now pass; LM well above 11.07 with p < 0.05.
- **Committed in:** `cb37502` (Task 1 commit).

**4. [Rule 3 — Blocking] `welford_pass` visibility bumped for ANOM-09 sibling-submodule access**

- **Found during:** Task 2 design.
- **Issue:** The plan's `<action>` for ANOM-09 explicitly specifies "the kernel reuses welford_pass from anom::summary::kernel where applicable to keep skew/excess_kurtosis matching ANOM-02 exactly". `welford_pass` was declared `pub(super)` in `anom::summary::kernel`, meaning it was only visible WITHIN `anom::summary`. The sibling submodule `anom::jarque_bera` could not access it.
- **Fix:** Bumped visibility from `pub(super)` to `pub(in crate::scan::anom)` on BOTH `welford_pass` and the `WelfordStats` return type. This is the minimum-surface visibility that allows sibling-submodule access without exposing the primitive to the wider crate or to consumers. Documented in the kernel's existing module-level pattern comment.
- **Files modified:** `crates/miner-core/src/scan/anom/summary/kernel.rs` (2 visibility specifiers).
- **Verification:** ANOM-02 summary unit tests + all kernel tests + the Phase 3 LjungBox byte-identical golden continue to pass. The `jarque_bera_moments_match_welford_pass` test pins the new sibling-access path via `to_bits()`-equality on skew + excess_kurtosis.
- **Committed in:** `1b1ad86` (Task 2 commit).

---

**Total deviations:** 4 (3 Rule-1 bug fixes during Task 1 + 1 Rule-3 visibility bump during Task 2). No scope creep — every fix was a required adaptation to ship the plan's user-locked behavior. No new clippy errors introduced (existing 3 errors in `drawdown/kernel.rs::tests` LN_2 literals pre-exist and are out of scope).

## Issues Encountered

- **`debug_assert!` interaction with `if return Err`** — debug-mode panic preceded the `Err`-return surface, making the `lag = 0` rejection path unreachable from unit tests. Resolved by removing the `debug_assert!` (Deviation 1).
- **Constant-u² singular X'X** — perfectly constant squared residuals (alternating ±c returns) produce a collinear design matrix; nalgebra `try_inverse()` returns `None`. Resolved by an early-return guard (Deviation 2).
- **Weak GARCH-like synthetic process** — initial α = 0.8 / n = 500 fixture did not reliably push LM above the chi²(5) 5% critical. Strengthened to α = 0.99 / n = 1000 with regime switching (Deviation 3).
- **`welford_pass` visibility** — `pub(super)` blocked sibling-submodule reuse. Resolved by visibility bump (Deviation 4).
- **`INSTA_UPDATE=auto` does NOT auto-accept new snapshots** — same gotcha as Plans 04-03/04-04/04-05. Applied `INSTA_UPDATE=always` once per integration test.

## User Setup Required

None — no new external dependencies, env vars, secrets, or service configuration. Integration tests run against deterministic synthetic fixtures; no Python golden regeneration is needed for this plan (Plan 04-11 owns goldens regen).

## Next Plan Readiness

**Plan 04-11 (Phase sign-off) unblocked.** All Phase 4 ANOM/CROSS/SEAS scans are now shipped: ANOM 11/11 (Plans 04-03..04-06), CROSS 4/4 (Plans 04-07..04-08), SEAS 6/6 (Plans 04-09..04-10). The remaining work for Phase 4 is:

1. **Goldens for ANOM-05..09** — generate statsmodels/scipy reference outputs and pin Rust outputs within 1e-10 (statistic) / 1e-8 (p-value) per RESEARCH §Section 2. ARCH-LM + JB join the existing ADF/KPSS/VR golden queue.
2. **Reconcile engle_granger local adf_step** against `scan::anom::adf::kernel::adfuller` — either prove numeric equivalence (and route engle_granger through the canonical kernel) OR keep the local copy with a referencing comment to scan::anom::adf::kernel::adfuller.
3. **Registry test tightening** — tighten the assertions in `scan::anom::mod::tests` and `scan::registry::tests` from `>= 1` style to exact final count (11 ANOM + 4 CROSS + 6 SEAS + 1 Phase-3 LjungBox = 22 registered scans).
4. **Phase-end smoke / facade-test additions** — verify the full 22-scan catalogue is invocable end-to-end via `miner-cli scans`, `miner-cli run`, and engine facade.

**No blockers.** Both Plan 04-06 scans land cleanly; full workspace test suite is green; Phase 3 LjungBox golden continues byte-identically; `miner scans` lists 12 stats.* entries with the new arch_lm + jarque_bera scans carrying `arity:"single"`.

## Threat Flags

No new security-relevant surface introduced beyond what the plan's `<threat_model>` block anticipates:

- **T-04-06-01** (ARCH-LM lag DOS) — addressed: `arch_lm::mod::resolve_lag` rejects `lag < 1` or `lag > n/3` before kernel invocation. Kernel defensively re-checks lag and `n >= 2*lag + 2`.
- **T-04-06-02** (Jarque-Bera constant-input) — addressed: kernel returns `Err` on `std = 0`; mod.rs converts to `ScanError::Kernel`. Mirrors ANOM-10 outliers MAD=0 path.
- **T-04-06-03** (statrs CDF accuracy) — addressed by deferring to statrs's well-tested `ChiSquared` + `FisherSnedecor` CDFs. Hand-derived kernel tests pin the p-value against `1 - ChiSquared(L).cdf(LM)` byte-identically. Plan 04-11 tightens to 1e-8 against statsmodels/scipy goldens.
- **T-04-06-SC** (no new packages) — confirmed: workspace `Cargo.toml` untouched. All deps already audited in Plan 04-01.

## Self-Check: PASSED

Verified:

- [x] `crates/miner-core/src/scan/anom/{arch_lm,jarque_bera}/{mod,kernel}.rs` all exist (4 files; `git status` confirmed).
- [x] `pub struct ArchLmScan` present in `arch_lm/mod.rs` (1 match).
- [x] `pub struct JarqueBeraScan` present in `jarque_bera/mod.rs` (1 match).
- [x] `SCAN_ID = "stats.heteroskedasticity.arch_lm"` + `SCAN_ID = "stats.normality.jarque_bera"` literals present.
- [x] `fn arch_lm_test` present in `arch_lm/kernel.rs` (1 match); `fn jarque_bera` present in `jarque_bera/kernel.rs` (1 match).
- [x] `nalgebra::DMatrix` + `statrs::distribution::ChiSquared` + `statrs::distribution::FisherSnedecor` imports present in `arch_lm/kernel.rs`.
- [x] `statrs::distribution::ChiSquared` import + `welford_pass` reuse present in `jarque_bera/kernel.rs`.
- [x] `r.register(Box::new(ArchLmScan));` + `r.register(Box::new(JarqueBeraScan));` present inside `register_anom_scans` body (alphabetical: arch_lm after drawdown, jarque_bera after arch_lm).
- [x] `git diff cb78ae7..HEAD -- crates/miner-core/src/scan/registry.rs` shows ZERO changes (Pattern E preserved).
- [x] 2 commits present in `git log --oneline cb78ae7..HEAD`: `cb37502` (Task 1 ARCH-LM), `1b1ad86` (Task 2 JB).
- [x] `crates/miner-core/tests/scan_{arch_lm,jarque_bera}.rs` both exist + their `tests/snapshots/scan_*__*_happy_path.snap` files exist.
- [x] `cargo build -p miner-core --all-targets` exits 0.
- [x] `cargo test -p miner-core --lib -- scan::anom::arch_lm scan::anom::jarque_bera` — 40 tests, all green.
- [x] `cargo test -p miner-core --test scan_arch_lm --test scan_jarque_bera --test scan_ljung_box` — all 3 integration tests pass (Phase 3 LjungBox golden byte-identical, Pitfall 9 preserved).
- [x] `cargo test --workspace` — full workspace test run produces zero failures.
- [x] `MINER_CACHE_ROOT=/tmp/foo MINER_BAR_CACHE_ROOT=/tmp/bar MINER_OUTPUT=stdout cargo run -p miner-cli --quiet -- scans 2>/dev/null | grep '"scan_id":"stats\\.heteroskedasticity\\.arch_lm"'` returns one line with `arity:"single"`; same for `stats.normality.jarque_bera`.
- [x] `miner-cli scans | grep -E '"scan_id":"stats\\.' | grep -c '"scan_id"'` returns 12 (1 Phase-3 LjungBox + 11 ANOM = 12 stats.* registered scans).
- [x] `registry.rs untouched — registration via register_anom_scans` (Pattern E; the only Plan 04-06 production diff is the 2 r.register lines + mod declarations in `scan/anom/mod.rs` + visibility bumps in `scan/anom/summary/kernel.rs`).

---

*Phase: 04-scan-catalogue-anom-cross-seas*
*Completed: 2026-05-20*
