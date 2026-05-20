//! Per-scan stat-closure + input-series dispatch table for the post-`Scan::run`
//! hygiene-kernel invocation (Plan 05-03 / D5-04 / HYG-03 + HYG-04).
//!
//! ## Purpose
//!
//! The `hygiene::bootstrap::stationary_bootstrap_ci` and
//! `hygiene::null::circular_shift_null_p` kernels (Plan 05-02) operate on a
//! plain `&[f64]` series and take a `stat: impl Fn(&[f64]) -> f64` closure.
//! Each per-scan opt-in (per the D5-04 matrix) needs to expose TWO things:
//!
//! 1. **Input series** — the `Vec<f64>` over which resampling happens
//!    (typically `log_returns(&ctx.bars.close)` for ANOM scans; some scans
//!    resample over `ctx.bars.close` directly; SEAS scans resample over
//!    `log_returns` with timestamps captured at closure-construction time).
//! 2. **Stat closure** — a recipe that, given a resampled slice, recomputes
//!    the SAME scalar statistic the scan's `Effect.value` carries.
//!
//! This module centralises both into `fn`-returning dispatch helpers keyed
//! on `scan_id@version`. Scans whose row in the D5-04 matrix says
//! `supports_bootstrap == false` AND every `supports_null_method == false`
//! return `None` from `input_series_for` (defensive — preflight already
//! rejects unsupported requests via `validate_hygiene_support`, but the
//! belt-and-braces `None` keeps the engine call site explicit).
//!
//! ## Scope (Plan 05-03 continuation 2)
//!
//! All 19 Phase 4 opt-in scans are wired here:
//!
//! **Single-arity (ANOM + SEAS):**
//! - `stats.autocorr.ljung_box@1` — input: log returns; stat: Q-stat at
//!   max lag.
//! - `stats.autocorr.ljung_box_sq@1` — input: squared log returns; stat:
//!   Q-stat at max lag.
//! - `stats.summary.welford@1` — input: log returns; stat: mean.
//! - `stats.vol.rolling@1` — input: log returns; stat: rolling std at
//!   last window.
//! - `stats.stationarity.adf@1` — input: closes (LEVELS); stat: ADF τ
//!   statistic with same params as the scan body.
//! - `stats.stationarity.kpss@1` — input: closes (LEVELS); stat: KPSS
//!   statistic.
//! - `stats.variance_ratio.lo_mackinlay@1` — input: log returns; stat:
//!   VR at max(`k_values`).
//! - `stats.heteroskedasticity.arch_lm@1` — input: log returns; stat:
//!   LM statistic at the requested lag.
//! - `stats.normality.jarque_bera@1` — input: log returns; stat: JB
//!   statistic.
//! - `seas.bucket.hour_of_day@1`, `seas.bucket.day_of_week@1`,
//!   `seas.bucket.session@1`, `seas.bucket.eom_som@1`,
//!   `seas.event.pre_post_window@1` — input: log returns; stat:
//!   `max_abs_finite(t_stats)` recomputed against pre-snapshotted bucket
//!   keys. Bucket assignments come from `ts_open_utc` which DOES NOT
//!   change under resampling.
//!
//! **Pair-arity (CROSS):**
//! - `cross.corr.pearson_rolling@1` — input: aligned (`returns_a`, `returns_b`);
//!   stat: rolling Pearson at last window.
//! - `cross.corr.spearman_rolling@1` — input: aligned (`returns_a`, `returns_b`);
//!   stat: rolling Spearman at last window.
//! - `cross.ols.rolling@1` — input: aligned (`returns_a`, `returns_b`);
//!   stat: rolling OLS β at last window.
//! - `cross.lead_lag.ccf@1` — input: aligned (`returns_a`, `returns_b`);
//!   stat: `argmax_lag` (signed integer encoded as f64).
//! - `cross.cointegration.engle_granger@1` — input: aligned (`close_a`,
//!   `close_b`) (LEVELS); stat: hedge-ratio β.
//!
//! ## Joint per-leg resampling (Pair-arity)
//!
//! Pair-arity scans need a JOINT resample — both legs must be resampled
//! with the SAME index sequence so the correlation / regression
//! statistic is computed against a coherent paired sample. The kernel
//! signature `stat: Fn(&[f64]) -> f64` operates on a single slice; we
//! handle joint resampling by replicating the kernel's RNG-driven index
//! sequence in [`pair_bootstrap_ci`] / [`pair_circular_shift_null_p`]
//! which take both legs and apply the same indices to each. Both helpers
//! are byte-for-byte mirrors of the Plan 05-02 kernels (same
//! `Xoshiro256PlusPlus::seed_from_u64`, same `gen_range`, same
//! sequential summation) so the byte-identical-rerun invariant under
//! `master_seed` extends to Pair-arity scans.
//!
//! ## Cancel polling discipline
//!
//! Same as Plan 05-02 — the dispatch closure does NOT poll cancel
//! mid-resample. Cancel polling happens at outer-engine cadence between
//! scan-body and hygiene-body (RESEARCH Pitfall 7); see
//! `apply_hygiene_mutations` in `engine/mod.rs`.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use rand::Rng;
use rand::SeedableRng;
use rand_xoshiro::Xoshiro256PlusPlus;

use crate::aggregator::BarFrame;
use crate::scan::ScanRequest;
use crate::scan::primitives::returns::log_returns;
use crate::scan::primitives::time_alignment::inner_join;

/// Statistic closure type for Single-arity scans. The hygiene kernels accept
/// any `Fn(&[f64]) -> f64`; this alias keeps engine call sites concise.
pub(crate) type StatClosure = Box<dyn Fn(&[f64]) -> f64 + Send + Sync>;

/// Statistic closure type for Pair-arity scans — receives both legs of an
/// aligned series and returns the joint scalar statistic.
pub(crate) type PairStatClosure = Box<dyn Fn(&[f64], &[f64]) -> f64 + Send + Sync>;

// ---------------------------------------------------------------------------
// Single-arity dispatch
// ---------------------------------------------------------------------------

/// Per-scan resample input for Single-arity scans.
///
/// Returns the `Vec<f64>` over which the hygiene kernels resample.
/// `None` ⇒ the scan is not wired into the Single-arity hygiene-dispatch
/// table (the engine treats this as "leave `effect.ci95` / `effect.p_value`
/// untouched").
///
/// Most scans resample over `log_returns(close)`. The unit-root scans
/// (`ADF`, `KPSS`) and the cointegration LEVELS-pair scan input is the
/// raw close series. The Ljung-Box-on-squared-returns input is the
/// squared log-returns. SEAS scans use raw log-returns and snapshot the
/// bucket-key vector (derived from `ts_open_utc`) at closure-construction
/// time — bucket assignments do NOT change under resampling, so the
/// dispatch closure replays the bucket recipe against the resampled
/// returns.
pub(crate) fn input_series_for(scan_id_at_version: &str, bars: &BarFrame) -> Option<Vec<f64>> {
    let close = &bars.close;
    match scan_id_at_version {
        // Log returns: requires at least 2 closes (yields n - 1 returns).
        "stats.autocorr.ljung_box@1"
        | "stats.summary.welford@1"
        | "stats.vol.rolling@1"
        | "stats.variance_ratio.lo_mackinlay@1"
        | "stats.heteroskedasticity.arch_lm@1"
        | "stats.normality.jarque_bera@1"
        | "seas.bucket.hour_of_day@1"
        | "seas.bucket.day_of_week@1"
        | "seas.bucket.session@1"
        | "seas.bucket.eom_som@1"
        | "seas.event.pre_post_window@1" => {
            if close.len() < 2 {
                return None;
            }
            Some(log_returns(close))
        }
        // Squared log returns (Ljung-Box on volatility-clustering).
        "stats.autocorr.ljung_box_sq@1" => {
            if close.len() < 2 {
                return None;
            }
            let returns = log_returns(close);
            Some(crate::scan::anom::ljung_box_sq::kernel::square_returns(&returns))
        }
        // LEVELS series (unit-root tests). ADF/KPSS need a few observations.
        "stats.stationarity.adf@1" | "stats.stationarity.kpss@1" => {
            if close.len() < 4 {
                return None;
            }
            Some(close.clone())
        }
        _ => None,
    }
}

/// Per-scan statistic closure for Single-arity scans.
///
/// Returns a `Box<dyn Fn(&[f64]) -> f64>` that recomputes the scalar
/// scan-output statistic over a resampled slice. The bootstrap kernel
/// passes resampled slices through this closure to build the empirical
/// statistic distribution; the null kernel passes circularly-shifted
/// slices through the SAME closure to build the empirical null
/// distribution.
///
/// `bars` is borrowed so the closure can capture timestamp-derived state
/// (e.g., SEAS bucket keys derived from `ts_open_utc`) at construction
/// time; the resample loop uses ONLY the per-resample slice, so the
/// closure-captured state stays constant across the resample sequence.
///
/// `req` is borrowed so the closure can resolve scan parameters at
/// construction time.
///
/// `None` ⇒ the scan is not wired into the dispatch table (mirror of
/// `input_series_for`'s `None`).
#[allow(clippy::too_many_lines)]
pub(crate) fn stat_closure_for(
    scan_id_at_version: &str,
    req: &ScanRequest,
    bars: &BarFrame,
) -> Option<StatClosure> {
    match scan_id_at_version {
        "stats.autocorr.ljung_box@1" => Some(make_ljungbox_closure(req)),
        "stats.autocorr.ljung_box_sq@1" => Some(make_ljungbox_sq_closure(req)),
        "stats.summary.welford@1" => Some(make_welford_mean_closure()),
        "stats.vol.rolling@1" => make_vol_rolling_closure(req),
        "stats.variance_ratio.lo_mackinlay@1" => Some(make_variance_ratio_closure(req)),
        "stats.heteroskedasticity.arch_lm@1" => Some(make_arch_lm_closure(req)),
        "stats.normality.jarque_bera@1" => Some(make_jarque_bera_closure()),
        "stats.stationarity.adf@1" => Some(make_adf_closure(req)),
        "stats.stationarity.kpss@1" => Some(make_kpss_closure(req)),
        "seas.bucket.hour_of_day@1" => make_seas_hour_of_day_closure(req, bars),
        "seas.bucket.day_of_week@1" => make_seas_day_of_week_closure(req, bars),
        "seas.bucket.session@1" => make_seas_session_closure(req, bars),
        "seas.bucket.eom_som@1" => make_seas_eom_som_closure(req, bars),
        "seas.event.pre_post_window@1" => make_seas_event_window_closure(req, bars),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Pair-arity dispatch
// ---------------------------------------------------------------------------

/// Per-scan resample inputs for Pair-arity scans.
///
/// Returns `(Vec<f64>, Vec<f64>)` — the two parallel inputs the joint
/// bootstrap / null resamples over. For correlation / OLS / lead-lag the
/// inputs are aligned log-returns of each leg; for Engle-Granger the inputs
/// are aligned closes (LEVELS) of each leg.
///
/// `None` ⇒ the scan is not wired into the Pair-arity hygiene-dispatch
/// table.
pub(crate) fn pair_input_series_for(
    scan_id_at_version: &str,
    bars_a: &BarFrame,
    bars_b: &BarFrame,
) -> Option<(Vec<f64>, Vec<f64>)> {
    let aligned = inner_join(bars_a, bars_b);
    if aligned.timestamps_ms.is_empty() {
        return None;
    }
    match scan_id_at_version {
        // Aligned log returns: requires at least 2 aligned bars to produce
        // a useful returns pair (n - 1 entries each).
        "cross.corr.pearson_rolling@1"
        | "cross.corr.spearman_rolling@1"
        | "cross.ols.rolling@1"
        | "cross.lead_lag.ccf@1" => {
            if aligned.close_a.len() < 2 {
                return None;
            }
            let ra = log_returns(&aligned.close_a);
            let rb = log_returns(&aligned.close_b);
            if ra.is_empty() || rb.is_empty() {
                return None;
            }
            Some((ra, rb))
        }
        // Aligned LEVELS (Engle-Granger needs raw closes).
        "cross.cointegration.engle_granger@1" => {
            if aligned.close_a.len() < 4 {
                return None;
            }
            Some((aligned.close_a, aligned.close_b))
        }
        _ => None,
    }
}

/// Per-scan Pair-arity statistic closure.
///
/// Returns a `Box<dyn Fn(&[f64], &[f64]) -> f64>` that recomputes the joint
/// scan-output statistic over a resampled pair of aligned slices. Used by
/// [`pair_bootstrap_ci`] and [`pair_circular_shift_null_p`] — both keep
/// leg-A and leg-B resamples lock-step (same index sequence per resample
/// iteration / same circular-shift offset) so the joint relationship is
/// preserved.
pub(crate) fn pair_stat_closure_for(
    scan_id_at_version: &str,
    req: &ScanRequest,
) -> Option<PairStatClosure> {
    match scan_id_at_version {
        "cross.corr.pearson_rolling@1" => make_pearson_rolling_closure(req),
        "cross.corr.spearman_rolling@1" => make_spearman_rolling_closure(req),
        "cross.ols.rolling@1" => make_ols_rolling_closure(req),
        "cross.lead_lag.ccf@1" => Some(make_lead_lag_closure(req)),
        "cross.cointegration.engle_granger@1" => Some(make_engle_granger_closure()),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Joint resample helpers (Pair-arity bootstrap + null)
//
// These mirror the Plan 05-02 kernels byte-for-byte (same RNG, same
// algorithm structure) but operate on a paired `(values_a, values_b)`
// input so the joint statistic is computed against a coherent shuffle.
// Kernels are NOT modified — the byte-identical contract under
// `master_seed` is preserved by replicating the kernel's RNG state
// machine here.
// ---------------------------------------------------------------------------

/// Stationary bootstrap CI for a JOINT (Pair-arity) scalar statistic.
///
/// Mirrors [`crate::scan::hygiene::bootstrap::stationary_bootstrap_ci`]
/// byte-for-byte but emits paired resamples by sampling the SAME index
/// `idx` from BOTH legs each iteration. Same `Xoshiro256PlusPlus` RNG +
/// same `gen_range` + same sort order ⇒ byte-identical CIs under fixed
/// `seed`.
#[must_use]
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "n_resamples is u32; floor/ceil casts bounded by n_resamples << 2^31"
)]
pub(crate) fn pair_stationary_bootstrap_ci<F>(
    values_a: &[f64],
    values_b: &[f64],
    stat: F,
    n_resamples: u32,
    mean_block_len: f64,
    seed: u64,
    ci_level: f64,
) -> [f64; 2]
where
    F: Fn(&[f64], &[f64]) -> f64,
{
    debug_assert_eq!(values_a.len(), values_b.len(), "pair_stationary_bootstrap_ci: pair length mismatch");
    let n = values_a.len();
    if n < 2 || n_resamples == 0 {
        return [f64::NAN, f64::NAN];
    }
    let mut rng = Xoshiro256PlusPlus::seed_from_u64(seed);
    let p_continue = if mean_block_len > 1.0 { 1.0 / mean_block_len } else { 1.0 };

    let mut boot_stats: Vec<f64> = Vec::with_capacity(n_resamples as usize);
    let mut buf_a: Vec<f64> = Vec::with_capacity(n);
    let mut buf_b: Vec<f64> = Vec::with_capacity(n);

    for _ in 0..n_resamples {
        buf_a.clear();
        buf_b.clear();
        let mut idx = rng.gen_range(0..n);
        while buf_a.len() < n {
            buf_a.push(values_a[idx]);
            buf_b.push(values_b[idx]);
            if rng.r#gen::<f64>() < p_continue {
                idx = rng.gen_range(0..n);
            } else {
                idx = (idx + 1) % n;
            }
        }
        boot_stats.push(stat(&buf_a, &buf_b));
    }

    boot_stats.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let alpha_half = (1.0 - ci_level) / 2.0;
    let n_resamples_f = f64::from(n_resamples);
    let lo_idx = (n_resamples_f * alpha_half).floor() as usize;
    let hi_raw = (n_resamples_f * (1.0 - alpha_half)).ceil() as usize;
    let hi_idx = hi_raw.saturating_sub(1).min(boot_stats.len() - 1);
    [boot_stats[lo_idx], boot_stats[hi_idx]]
}

/// Fixed-block bootstrap CI for a JOINT (Pair-arity) scalar statistic.
///
/// Mirrors [`crate::scan::hygiene::bootstrap::block_bootstrap_ci`]
/// byte-for-byte but paired.
#[must_use]
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "n_resamples is u32; floor/ceil casts bounded by n_resamples << 2^31"
)]
pub(crate) fn pair_block_bootstrap_ci<F>(
    values_a: &[f64],
    values_b: &[f64],
    stat: F,
    n_resamples: u32,
    block_len: usize,
    seed: u64,
    ci_level: f64,
) -> [f64; 2]
where
    F: Fn(&[f64], &[f64]) -> f64,
{
    debug_assert_eq!(values_a.len(), values_b.len(), "pair_block_bootstrap_ci: pair length mismatch");
    let n = values_a.len();
    if n < 2 || n_resamples == 0 || block_len == 0 {
        return [f64::NAN, f64::NAN];
    }
    let mut rng = Xoshiro256PlusPlus::seed_from_u64(seed);
    let mut boot_stats: Vec<f64> = Vec::with_capacity(n_resamples as usize);
    let mut buf_a: Vec<f64> = Vec::with_capacity(n);
    let mut buf_b: Vec<f64> = Vec::with_capacity(n);

    for _ in 0..n_resamples {
        buf_a.clear();
        buf_b.clear();
        let mut idx = rng.gen_range(0..n);
        let mut steps_in_block: usize = 0;
        while buf_a.len() < n {
            if steps_in_block >= block_len {
                idx = rng.gen_range(0..n);
                steps_in_block = 0;
            }
            buf_a.push(values_a[idx]);
            buf_b.push(values_b[idx]);
            idx = (idx + 1) % n;
            steps_in_block += 1;
        }
        boot_stats.push(stat(&buf_a, &buf_b));
    }

    boot_stats.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let alpha_half = (1.0 - ci_level) / 2.0;
    let n_resamples_f = f64::from(n_resamples);
    let lo_idx = (n_resamples_f * alpha_half).floor() as usize;
    let hi_raw = (n_resamples_f * (1.0 - alpha_half)).ceil() as usize;
    let hi_idx = hi_raw.saturating_sub(1).min(boot_stats.len() - 1);
    [boot_stats[lo_idx], boot_stats[hi_idx]]
}

/// Circular-shift surrogate null p-value for a JOINT (Pair-arity) scalar
/// statistic.
///
/// Mirrors [`crate::scan::hygiene::null::circular_shift_null_p`]
/// byte-for-byte but shifts ONLY leg B by a uniform offset in `[1, n)`;
/// leg A stays fixed. This destroys the leg-A↔leg-B temporal pairing
/// while preserving each leg's marginal distribution — the canonical
/// null for cross-correlation / cross-regression statistics
/// (Theiler 1992 surrogate-data convention; RESEARCH §1.5).
#[must_use]
#[allow(
    clippy::cast_precision_loss,
    reason = "n_resamples and more_extreme are u32 counts; f64 conversion exact for inputs < 2^53"
)]
pub(crate) fn pair_circular_shift_null_p<F>(
    values_a: &[f64],
    values_b: &[f64],
    observed_stat: f64,
    stat: F,
    n_resamples: u32,
    seed: u64,
) -> f64
where
    F: Fn(&[f64], &[f64]) -> f64,
{
    debug_assert_eq!(values_a.len(), values_b.len(), "pair_circular_shift_null_p: pair length mismatch");
    let n = values_a.len();
    if n < 2 || n_resamples == 0 {
        return f64::NAN;
    }
    let mut rng = Xoshiro256PlusPlus::seed_from_u64(seed);
    let mut surrogate_b: Vec<f64> = vec![0.0; n];
    let mut more_extreme: u32 = 0;
    let obs_abs = observed_stat.abs();
    for _ in 0..n_resamples {
        let offset = rng.gen_range(1..n);
        for (i, slot) in surrogate_b.iter_mut().enumerate().take(n) {
            *slot = values_b[(i + offset) % n];
        }
        let surr_stat = stat(values_a, &surrogate_b);
        if surr_stat.abs() >= obs_abs {
            more_extreme += 1;
        }
    }
    f64::from(more_extreme) / f64::from(n_resamples)
}

// ---------------------------------------------------------------------------
// ANOM closures (Single-arity)
// ---------------------------------------------------------------------------

/// D3-03 default: `min(10, n / 5)`. Mirrors `LjungBoxScan::default_lags`.
#[inline]
fn default_ljungbox_lags(n: usize) -> usize {
    (n / 5).min(10)
}

fn make_ljungbox_closure(req: &ScanRequest) -> StatClosure {
    let lags_override: Option<usize> = req
        .resolved_params
        .get("lags")
        .and_then(serde_json::Value::as_i64)
        .and_then(|v| usize::try_from(v).ok());
    Box::new(move |slice: &[f64]| {
        let n = slice.len();
        if n < 2 {
            return 0.0;
        }
        let lags = lags_override.unwrap_or_else(|| default_ljungbox_lags(n));
        if lags < 1 || lags >= n {
            return 0.0;
        }
        let acf = crate::scan::ljung_box::kernel::biased_acf(slice, lags);
        let (q_stats, _) = crate::scan::ljung_box::kernel::ljung_box_q_and_p(n, &acf, lags);
        q_stats[lags - 1]
    })
}

fn make_ljungbox_sq_closure(req: &ScanRequest) -> StatClosure {
    // Mirrors LjungBoxScan but applied to ALREADY-SQUARED returns (the
    // input series builder squares the log returns once, so the closure
    // doesn't double-square).
    make_ljungbox_closure(req)
}

fn make_welford_mean_closure() -> StatClosure {
    Box::new(move |slice: &[f64]| {
        if slice.is_empty() {
            return 0.0;
        }
        #[allow(
            clippy::cast_precision_loss,
            reason = "len fits in f64 mantissa for any realistic OHLCV series"
        )]
        let n = slice.len() as f64;
        slice.iter().copied().sum::<f64>() / n
    })
}

/// `VolRollingScan` — last-window rolling std (`Effect.value = last_window_vol`).
/// Returns `None` when the `window` parameter is missing or invalid (the scan
/// body itself would error in that case, so the dispatch stays consistent).
fn make_vol_rolling_closure(req: &ScanRequest) -> Option<StatClosure> {
    let window_i64 = req.resolved_params.get("window").and_then(serde_json::Value::as_i64)?;
    if window_i64 < 2 {
        return None;
    }
    let window = usize::try_from(window_i64).ok()?;
    Some(Box::new(move |slice: &[f64]| {
        if slice.len() < window || window < 2 {
            return 0.0;
        }
        let vols = crate::scan::anom::vol::kernel::rolling_std(slice, window);
        *vols.last().unwrap_or(&0.0)
    }))
}

fn make_variance_ratio_closure(req: &ScanRequest) -> StatClosure {
    // VR(k) at the LAST k of the user grid (default [2, 4, 8, 16] → max k=16).
    let k_values: Vec<usize> = match req
        .resolved_params
        .get("k_values")
        .and_then(serde_json::Value::as_array)
    {
        Some(arr) => arr
            .iter()
            .filter_map(serde_json::Value::as_i64)
            .filter_map(|v| usize::try_from(v).ok())
            .collect(),
        None => vec![2_usize, 4, 8, 16],
    };
    let robust: bool = req
        .resolved_params
        .get("robust")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true);
    let last_k = k_values.last().copied().unwrap_or(2);
    Box::new(move |slice: &[f64]| {
        let n = slice.len();
        if n < 4 || last_k < 2 || last_k > n / 2 {
            return 0.0;
        }
        match crate::scan::anom::variance_ratio::kernel::variance_ratio(slice, last_k, robust) {
            Ok(r) => r.vr,
            Err(_) => 0.0,
        }
    })
}

fn make_arch_lm_closure(req: &ScanRequest) -> StatClosure {
    // Lag must match the scan body resolution. The scan defaults to lag=12.
    let lag_override = req
        .resolved_params
        .get("lag")
        .and_then(serde_json::Value::as_i64)
        .and_then(|v| usize::try_from(v).ok());
    Box::new(move |slice: &[f64]| {
        let n = slice.len();
        let lag = lag_override.unwrap_or(12);
        if lag == 0 || n < 4 || lag > n / 3 {
            return 0.0;
        }
        match crate::scan::anom::arch_lm::kernel::arch_lm_test(slice, lag) {
            Ok(r) => r.lm,
            Err(_) => 0.0,
        }
    })
}

fn make_jarque_bera_closure() -> StatClosure {
    Box::new(move |slice: &[f64]| {
        if slice.len() < 4 {
            return 0.0;
        }
        match crate::scan::anom::jarque_bera::kernel::jarque_bera(slice) {
            Ok(r) => r.statistic,
            Err(_) => 0.0,
        }
    })
}

fn make_adf_closure(req: &ScanRequest) -> StatClosure {
    use crate::scan::anom::adf::kernel::{AutoLagVariant, RegressionVariant};
    let regression = match req.resolved_params.get("regression").and_then(serde_json::Value::as_str) {
        Some("nc") => RegressionVariant::Nc,
        Some("ct") => RegressionVariant::Ct,
        Some("ctt") => RegressionVariant::Ctt,
        _ => RegressionVariant::C,
    };
    let autolag = match req.resolved_params.get("autolag").and_then(serde_json::Value::as_str) {
        Some("BIC") => AutoLagVariant::Bic,
        Some("None") => AutoLagVariant::None,
        _ => AutoLagVariant::Aic,
    };
    let max_lag_override = req
        .resolved_params
        .get("max_lag")
        .and_then(serde_json::Value::as_i64)
        .and_then(|v| usize::try_from(v).ok());
    Box::new(move |slice: &[f64]| {
        let n = slice.len();
        if n < 4 {
            return 0.0;
        }
        let max_lag = max_lag_override.unwrap_or_else(|| {
            // statsmodels default: int(12 * (n/100)^0.25).
            #[allow(
                clippy::cast_precision_loss,
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "n bounded by realistic bar count; result << 100"
            )]
            {
                let nf = n as f64;
                let v = 12.0 * (nf / 100.0).powf(0.25);
                v.floor().max(0.0) as usize
            }
        });
        if max_lag >= n {
            return 0.0;
        }
        match crate::scan::anom::adf::kernel::adfuller(slice, max_lag, regression, autolag) {
            Ok(r) => r.statistic,
            Err(_) => 0.0,
        }
    })
}

fn make_kpss_closure(req: &ScanRequest) -> StatClosure {
    use crate::scan::anom::kpss::kernel::{KpssRegression, NlagsParam};
    let regression = match req.resolved_params.get("regression").and_then(serde_json::Value::as_str) {
        Some("ct") => KpssRegression::Ct,
        _ => KpssRegression::C,
    };
    let nlags = match req.resolved_params.get("nlags") {
        // String form (including "auto") falls through to the default.
        Some(v) if v.is_string() => NlagsParam::Auto,
        Some(v) if v.is_i64() => {
            let raw = v.as_i64().unwrap_or(0).max(0);
            usize::try_from(raw).map_or(NlagsParam::Auto, NlagsParam::Manual)
        }
        _ => NlagsParam::Auto,
    };
    Box::new(move |slice: &[f64]| {
        let n = slice.len();
        if n < 4 {
            return 0.0;
        }
        match crate::scan::anom::kpss::kernel::kpss_statistic(slice, regression, nlags) {
            Ok(r) => r.statistic,
            Err(_) => 0.0,
        }
    })
}

// ---------------------------------------------------------------------------
// SEAS closures (Single-arity, ts_open_utc-dependent buckets)
//
// Each SEAS closure snapshots the bucket-key vector (derived from
// `ts_open_utc[1..]`) at construction time. Resampling shuffles the
// returns; bucket assignments stay fixed; the closure recomputes
// `bucket_stats(resampled, bucket_keys, num_buckets, min_obs)` and folds
// over t_stats with `max_abs_finite`.
// ---------------------------------------------------------------------------

const DEFAULT_SEAS_MIN_OBS: usize = 5;
const NUM_HOUR_BUCKETS: usize = 24;
const NUM_DAY_BUCKETS: usize = 7;

fn resolve_min_obs(req: &ScanRequest, default: usize) -> usize {
    req.resolved_params
        .get("min_obs_per_bucket")
        .and_then(serde_json::Value::as_i64)
        .and_then(|v| usize::try_from(v.max(1)).ok())
        .unwrap_or(default)
}

fn make_seas_hour_of_day_closure(req: &ScanRequest, bars: &BarFrame) -> Option<StatClosure> {
    if bars.ts_open_utc.len() < 2 {
        return None;
    }
    let ts_for_returns: Vec<_> = bars.ts_open_utc.iter().skip(1).copied().collect();
    let bucket_keys = crate::scan::seas::hour_of_day::kernel::hour_keys(&ts_for_returns);
    let min_obs = resolve_min_obs(req, DEFAULT_SEAS_MIN_OBS);
    Some(Box::new(move |slice: &[f64]| {
        if slice.len() != bucket_keys.len() {
            return 0.0;
        }
        let r = crate::scan::seas::bucketing::bucket_stats(slice, &bucket_keys, NUM_HOUR_BUCKETS, min_obs);
        crate::scan::seas::bucketing::max_abs_finite(&r.t_stats)
    }))
}

fn make_seas_day_of_week_closure(req: &ScanRequest, bars: &BarFrame) -> Option<StatClosure> {
    if bars.ts_open_utc.len() < 2 {
        return None;
    }
    let ts_for_returns: Vec<_> = bars.ts_open_utc.iter().skip(1).copied().collect();
    let bucket_keys = crate::scan::seas::day_of_week::kernel::weekday_keys(&ts_for_returns);
    let min_obs = resolve_min_obs(req, DEFAULT_SEAS_MIN_OBS);
    Some(Box::new(move |slice: &[f64]| {
        if slice.len() != bucket_keys.len() {
            return 0.0;
        }
        let r = crate::scan::seas::bucketing::bucket_stats(slice, &bucket_keys, NUM_DAY_BUCKETS, min_obs);
        crate::scan::seas::bucketing::max_abs_finite(&r.t_stats)
    }))
}

fn make_seas_session_closure(req: &ScanRequest, bars: &BarFrame) -> Option<StatClosure> {
    use chrono::Timelike;
    if bars.ts_open_utc.len() < 2 {
        return None;
    }
    // Sessions: the scan body permits a user-supplied override via the
    // `sessions` param, but the default is FX_MAJOR_DEFAULTS. Wiring the
    // override would require duplicating `resolve_sessions` from the scan
    // body. The dispatch closure uses the default FX-major sessions —
    // documented in 05-03-cont2 SUMMARY as a Phase 7 refinement hook
    // (custom sessions are not yet supported on the hygiene path).
    let sessions = crate::scan::seas::session::kernel::FX_MAJOR_DEFAULTS;
    let num_buckets = sessions.len();
    let ts_for_returns: Vec<_> = bars.ts_open_utc.iter().skip(1).copied().collect();

    // Pre-compute per-return per-bucket membership as a flat Vec<Vec<usize>>
    // — one Vec per return listing the bucket indices it belongs to.
    let mut membership: Vec<Vec<usize>> = Vec::with_capacity(ts_for_returns.len());
    for ts in &ts_for_returns {
        let hour = ts.hour();
        let mut buckets = Vec::with_capacity(2);
        for (b, sess) in sessions.iter().enumerate() {
            if crate::scan::seas::session::kernel::hour_in_session(hour, sess.start_utc_h, sess.end_utc_h) {
                buckets.push(b);
            }
        }
        membership.push(buckets);
    }
    let min_obs = resolve_min_obs(req, DEFAULT_SEAS_MIN_OBS);
    Some(Box::new(move |slice: &[f64]| {
        if slice.len() != membership.len() {
            return 0.0;
        }
        let mut per_bucket: Vec<Vec<f64>> = (0..num_buckets).map(|_| Vec::new()).collect();
        for (i, buckets) in membership.iter().enumerate() {
            for &b in buckets {
                per_bucket[b].push(slice[i]);
            }
        }
        let r = crate::scan::seas::bucketing::bucket_stats_from_groups(&mut per_bucket, min_obs);
        crate::scan::seas::bucketing::max_abs_finite(&r.t_stats)
    }))
}

fn make_seas_eom_som_closure(req: &ScanRequest, bars: &BarFrame) -> Option<StatClosure> {
    if bars.ts_open_utc.len() < 2 {
        return None;
    }
    let cutoff_n = req
        .resolved_params
        .get("cutoff_n")
        .and_then(serde_json::Value::as_i64)
        .and_then(|v| usize::try_from(v.max(1)).ok())
        .unwrap_or(3);
    let num_buckets = 2 * cutoff_n;
    let ts_for_returns: Vec<_> = bars.ts_open_utc.iter().skip(1).copied().collect();
    let calendar = crate::calendar::Calendar::fx_major();
    // Pre-compute (return_index → bucket) assignments. Returns whose
    // timestamps don't fall in an EOM/SOM window are filtered out.
    let mut filtered_indices: Vec<usize> = Vec::with_capacity(ts_for_returns.len());
    let mut bucket_keys: Vec<usize> = Vec::with_capacity(ts_for_returns.len());
    for (i, ts) in ts_for_returns.iter().enumerate() {
        if let Some(b) = crate::scan::seas::eom_som::kernel::trading_day_of_month_bucket(*ts, cutoff_n, &calendar) {
            if b < num_buckets {
                filtered_indices.push(i);
                bucket_keys.push(b);
            }
        }
    }
    let min_obs = resolve_min_obs(req, DEFAULT_SEAS_MIN_OBS);
    Some(Box::new(move |slice: &[f64]| {
        if slice.len() != ts_for_returns.len() {
            return 0.0;
        }
        // Gather only the returns whose timestamps land in an EOM/SOM bucket.
        let values: Vec<f64> = filtered_indices.iter().map(|&i| slice[i]).collect();
        let r = crate::scan::seas::bucketing::bucket_stats(&values, &bucket_keys, num_buckets, min_obs);
        crate::scan::seas::bucketing::max_abs_finite(&r.t_stats)
    }))
}

fn make_seas_event_window_closure(req: &ScanRequest, bars: &BarFrame) -> Option<StatClosure> {
    if bars.ts_open_utc.len() < 2 {
        return None;
    }
    let pre_bars = req
        .resolved_params
        .get("pre_window_bars")
        .and_then(serde_json::Value::as_i64)
        .and_then(|v| usize::try_from(v.max(1)).ok())
        .unwrap_or(5);
    let post_bars = req
        .resolved_params
        .get("post_window_bars")
        .and_then(serde_json::Value::as_i64)
        .and_then(|v| usize::try_from(v.max(1)).ok())
        .unwrap_or(5);
    let event_timestamps_ms: Vec<i64> = req
        .resolved_params
        .get("event_timestamps_ms")
        .and_then(serde_json::Value::as_array)
        .map(|arr| arr.iter().filter_map(serde_json::Value::as_i64).collect())
        .unwrap_or_default();
    let timestamps_ms: Vec<i64> = bars
        .ts_open_utc
        .iter()
        .skip(1)
        .map(chrono::DateTime::timestamp_millis)
        .collect();
    Some(Box::new(move |slice: &[f64]| {
        if slice.len() != timestamps_ms.len() {
            return 0.0;
        }
        let r = crate::scan::seas::event_window::kernel::event_window_stats(
            slice,
            &timestamps_ms,
            &event_timestamps_ms,
            pre_bars,
            post_bars,
        );
        if r.event_count == 0 {
            return 0.0;
        }
        #[allow(
            clippy::cast_precision_loss,
            reason = "event_count <= MAX_EVENT_TIMESTAMPS (1e5); fits f64 mantissa"
        )]
        let denom = r.event_count as f64;
        r.post_means.iter().copied().sum::<f64>() / denom
    }))
}

// ---------------------------------------------------------------------------
// CROSS closures (Pair-arity)
// ---------------------------------------------------------------------------

fn resolve_window(req: &ScanRequest) -> Option<usize> {
    req.resolved_params
        .get("window")
        .and_then(serde_json::Value::as_i64)
        .and_then(|v| usize::try_from(v).ok())
        .filter(|w| *w >= 2)
}

fn make_pearson_rolling_closure(req: &ScanRequest) -> Option<PairStatClosure> {
    let window = resolve_window(req)?;
    Some(Box::new(move |a: &[f64], b: &[f64]| {
        if a.len() != b.len() || a.len() < window {
            return 0.0;
        }
        let values = crate::scan::cross::corr_rolling::kernel::rolling_pearson(a, b, window);
        match values.last() {
            Some(v) if v.is_finite() => *v,
            _ => 0.0,
        }
    }))
}

fn make_spearman_rolling_closure(req: &ScanRequest) -> Option<PairStatClosure> {
    let window = resolve_window(req)?;
    Some(Box::new(move |a: &[f64], b: &[f64]| {
        if a.len() != b.len() || a.len() < window {
            return 0.0;
        }
        let values = crate::scan::cross::corr_rolling::kernel::rolling_spearman(a, b, window);
        match values.last() {
            Some(v) if v.is_finite() => *v,
            _ => 0.0,
        }
    }))
}

fn make_ols_rolling_closure(req: &ScanRequest) -> Option<PairStatClosure> {
    let window = resolve_window(req)?;
    if window < 3 {
        return None;
    }
    Some(Box::new(move |a: &[f64], b: &[f64]| {
        // OLS contract: y = leg_a, x = leg_b (regressand = a, regressor = b).
        if a.len() != b.len() || a.len() < window {
            return 0.0;
        }
        let r = crate::scan::cross::ols_rolling::kernel::rolling_ols(a, b, window);
        match r.betas.last() {
            Some(v) if v.is_finite() => *v,
            _ => 0.0,
        }
    }))
}

fn make_lead_lag_closure(req: &ScanRequest) -> PairStatClosure {
    let max_lag = req
        .resolved_params
        .get("max_lag")
        .and_then(serde_json::Value::as_i64)
        .and_then(|v| usize::try_from(v).ok())
        .unwrap_or(20);
    Box::new(move |a: &[f64], b: &[f64]| {
        if a.len() != b.len() || a.len() < 2 || max_lag == 0 || max_lag >= a.len() / 2 {
            return 0.0;
        }
        let r = crate::scan::cross::lead_lag::kernel::lead_lag_ccf(a, b, max_lag);
        // Effect.value = argmax_lag (signed integer encoded as f64).
        #[allow(
            clippy::cast_precision_loss,
            reason = "argmax_lag fits in i64; converts losslessly to f64 for realistic lags"
        )]
        let v = r.argmax_lag as f64;
        v
    })
}

fn make_engle_granger_closure() -> PairStatClosure {
    use crate::scan::cross::engle_granger::kernel::AdfRegression;
    Box::new(move |a: &[f64], b: &[f64]| {
        if a.len() != b.len() || a.len() < 4 {
            return 0.0;
        }
        let r = crate::scan::cross::engle_granger::kernel::engle_granger(a, b, AdfRegression::Constant);
        if r.hedge_ratio_beta.is_finite() {
            r.hedge_ratio_beta
        } else {
            0.0
        }
    })
}

// ---------------------------------------------------------------------------
// Cancel-aware bridge into kernel calls
//
// `apply_hygiene_mutations` in `engine/mod.rs` polls `cancel` before
// invoking the kernels; the kernels themselves are uninterruptible
// (RESEARCH Pitfall 7). The cancel argument is passed through here for
// future use (e.g., a longer-running pair scan that wants to abort
// mid-iteration); v1 doesn't poll inside the loops.
// ---------------------------------------------------------------------------

// Re-exported for ergonomics — the engine uses `Arc<AtomicBool>` as the
// cancel handle.
#[allow(dead_code)]
pub(crate) type CancelFlag = Arc<AtomicBool>;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aggregator::Timeframe;
    use crate::engine::gap_policy::GapPolicyKind;
    use crate::engine::param_hash;
    use crate::findings::TimeRange;
    use crate::reader::{Blake3Hex, ClosedRangeUtc, InstrumentSpec, Side};
    use chrono::{DateTime, Duration, TimeZone, Utc};

    fn make_req(scan_id: &str, params: &serde_json::Value) -> ScanRequest {
        let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2024, 1, 30, 0, 0, 0).unwrap();
        ScanRequest {
            scan_id: scan_id.into(),
            version: 1,
            instruments: vec![InstrumentSpec {
                symbol: "EURUSD".into(),
                side: Side::Bid,
            }],
            timeframe: Timeframe::Tf15m,
            window: ClosedRangeUtc { start, end },
            sub_range: TimeRange {
                start_utc: start,
                end_utc: end,
            },
            gap_policy: GapPolicyKind::ContinuousOnly,
            resolved_params: params.clone(),
            param_hash: param_hash::param_hash(params).expect("hash ok"),
            dry_run: false,
            master_seed: None,
            job_seed: None,
            bootstrap_method: None,
            bootstrap_n: None,
            null_method: None,
            null_n: None,
            #[cfg(any(test, feature = "test-internal"))]
            sleep_after_first_finding_ms: None,
        }
    }

    fn blake3_hex_zero() -> Blake3Hex {
        Blake3Hex::from_hex_bytes(&[b'0'; 64])
    }

    #[allow(clippy::cast_possible_truncation)]
    fn lcg_bar_frame(n: usize, seed: u64, start_hour: u32) -> BarFrame {
        let start = Utc.with_ymd_and_hms(2024, 1, 1, start_hour, 0, 0).unwrap();
        let mut s = seed as u32;
        let mut closes = Vec::with_capacity(n);
        for _ in 0..n {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let frac = f64::from(s) / f64::from(u32::MAX);
            closes.push(1.0 + frac);
        }
        let ts_open: Vec<DateTime<Utc>> = (0..n)
            .map(|i| start + Duration::minutes(15 * i64::try_from(i).unwrap()))
            .collect();
        let ts_close: Vec<DateTime<Utc>> = ts_open.iter().map(|t| *t + Duration::minutes(15)).collect();
        BarFrame {
            source_id: "dukascopy".into(),
            symbol: "EURUSD".into(),
            side: Side::Bid,
            tf: Timeframe::Tf15m,
            ts_open_utc: ts_open,
            ts_close_utc: ts_close,
            open: closes.clone(),
            high: closes.iter().map(|c| c + 0.001).collect(),
            low: closes.iter().map(|c| c - 0.001).collect(),
            close: closes,
            tick_volume: vec![1.0; n],
        }
    }

    /// `input_series_for` returns `None` for an unwired scan id.
    #[test]
    fn input_series_returns_none_for_unwired_scan() {
        let bars = lcg_bar_frame(10, 1, 0);
        assert!(input_series_for("does.not.exist@1", &bars).is_none());
        // Pair-arity scan IDs go through pair_input_series_for, not single-arity.
        assert!(input_series_for("cross.lead_lag.ccf@1", &bars).is_none());
    }

    /// `LjungBox` + Welford share the log-returns input series.
    #[test]
    fn input_series_returns_log_returns_for_wired_anom_scans() {
        let bars = lcg_bar_frame(4, 2, 0);
        let lb = input_series_for("stats.autocorr.ljung_box@1", &bars).expect("Some");
        let wf = input_series_for("stats.summary.welford@1", &bars).expect("Some");
        assert_eq!(lb.len(), 3);
        assert_eq!(wf.len(), 3);
        assert_eq!(lb, wf);
    }

    /// ADF / KPSS input is the LEVELS series (closes), not log returns.
    #[test]
    fn input_series_returns_closes_for_unit_root_scans() {
        let bars = lcg_bar_frame(10, 3, 0);
        let adf = input_series_for("stats.stationarity.adf@1", &bars).expect("Some");
        let kpss = input_series_for("stats.stationarity.kpss@1", &bars).expect("Some");
        assert_eq!(adf.len(), bars.close.len());
        assert_eq!(kpss.len(), bars.close.len());
        // Bit-identical to the bar close series.
        assert_eq!(adf[0].to_bits(), bars.close[0].to_bits());
    }

    /// `LjungBoxSq` returns squared returns (positive everywhere).
    #[test]
    fn input_series_returns_squared_returns_for_ljungbox_sq() {
        let bars = lcg_bar_frame(5, 4, 0);
        let sq = input_series_for("stats.autocorr.ljung_box_sq@1", &bars).expect("Some");
        assert_eq!(sq.len(), 4); // n-1
        for v in &sq {
            assert!(*v >= 0.0, "squared returns must be >= 0; got {v}");
        }
    }

    /// `stat_closure_for` returns `None` for unwired scans.
    #[test]
    fn stat_closure_returns_none_for_unwired_scan() {
        let bars = lcg_bar_frame(5, 5, 0);
        let req = make_req("does.not.exist", &serde_json::json!({}));
        assert!(stat_closure_for("does.not.exist@1", &req, &bars).is_none());
    }

    /// `VolRollingScan` closure: last-window std of constant input = 0.
    #[test]
    fn vol_rolling_closure_last_window_constant_zero() {
        let bars = lcg_bar_frame(10, 6, 0);
        let req = make_req("stats.vol.rolling", &serde_json::json!({"window": 3}));
        let closure = stat_closure_for("stats.vol.rolling@1", &req, &bars).expect("Some");
        let constant = vec![1.0_f64; 10];
        assert_eq!(closure(&constant).to_bits(), 0.0_f64.to_bits());
    }

    /// Welford closure: arithmetic mean.
    #[test]
    fn welford_closure_returns_mean() {
        let bars = lcg_bar_frame(10, 7, 0);
        let req = make_req("stats.summary.welford", &serde_json::json!({}));
        let closure = stat_closure_for("stats.summary.welford@1", &req, &bars).expect("Some");
        let series = vec![1.0_f64, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(closure(&series).to_bits(), 3.0_f64.to_bits());
    }

    /// Pair-arity dispatch returns `None` for unwired scans.
    #[test]
    fn pair_dispatch_returns_none_for_unwired() {
        let bars_a = lcg_bar_frame(10, 8, 0);
        let bars_b = lcg_bar_frame(10, 9, 0);
        assert!(pair_input_series_for("nope@1", &bars_a, &bars_b).is_none());
    }

    /// Pair-arity Pearson rolling: aligned returns are length n-1.
    #[test]
    fn pair_input_series_pearson_rolling_returns_aligned_returns() {
        let bars_a = lcg_bar_frame(10, 10, 0);
        let bars_b = lcg_bar_frame(10, 11, 0);
        let (ra, rb) = pair_input_series_for("cross.corr.pearson_rolling@1", &bars_a, &bars_b).expect("Some");
        // n_a = n_b = 10 aligned → 9 returns per leg.
        assert_eq!(ra.len(), 9);
        assert_eq!(rb.len(), 9);
    }

    /// Engle-Granger input is LEVELS (aligned closes), not returns.
    #[test]
    fn pair_input_series_engle_granger_returns_aligned_levels() {
        let bars_a = lcg_bar_frame(10, 12, 0);
        let bars_b = lcg_bar_frame(10, 13, 0);
        let (ca, cb) = pair_input_series_for("cross.cointegration.engle_granger@1", &bars_a, &bars_b).expect("Some");
        // 10 aligned closes → 10 entries per leg.
        assert_eq!(ca.len(), 10);
        assert_eq!(cb.len(), 10);
        // Bit-identical to bar closes.
        assert_eq!(ca[0].to_bits(), bars_a.close[0].to_bits());
        assert_eq!(cb[0].to_bits(), bars_b.close[0].to_bits());
    }

    /// `pair_stationary_bootstrap_ci` byte-identical for fixed seed.
    #[test]
    fn pair_stationary_bootstrap_ci_deterministic_for_seed() {
        let a: Vec<f64> = (0..50).map(|i| f64::from(i) * 0.01).collect();
        let b: Vec<f64> = (0..50).map(|i| f64::from(i) * 0.02).collect();
        // Joint stat: simple Pearson correlation between paired slices.
        let stat = |xa: &[f64], xb: &[f64]| -> f64 {
            let n = xa.len();
            if n < 2 { return 0.0; }
            #[allow(clippy::cast_precision_loss)]
            let nf = n as f64;
            let ma = xa.iter().sum::<f64>() / nf;
            let mb = xb.iter().sum::<f64>() / nf;
            let mut num = 0.0;
            let mut da = 0.0;
            let mut db = 0.0;
            for i in 0..n {
                let xx = xa[i] - ma;
                let yy = xb[i] - mb;
                num += xx * yy;
                da += xx * xx;
                db += yy * yy;
            }
            if da == 0.0 || db == 0.0 { return 0.0; }
            num / (da.sqrt() * db.sqrt())
        };
        let ci_a = pair_stationary_bootstrap_ci(&a, &b, stat, 100, 5.0, 0xBEEF, 0.95);
        let ci_b = pair_stationary_bootstrap_ci(&a, &b, stat, 100, 5.0, 0xBEEF, 0.95);
        assert_eq!(ci_a[0].to_bits(), ci_b[0].to_bits());
        assert_eq!(ci_a[1].to_bits(), ci_b[1].to_bits());
        assert!(ci_a[0] <= ci_a[1]);
    }

    /// `pair_circular_shift_null_p` byte-identical for fixed seed.
    #[test]
    fn pair_circular_shift_null_p_deterministic_for_seed() {
        let a: Vec<f64> = (0..50).map(|i| f64::from(i) * 0.01).collect();
        let b: Vec<f64> = a.iter().rev().copied().collect();
        let stat = |xa: &[f64], xb: &[f64]| -> f64 {
            let n = xa.len();
            if n < 2 { return 0.0; }
            #[allow(clippy::cast_precision_loss)]
            let nf = n as f64;
            xa.iter().zip(xb.iter()).map(|(x, y)| x * y).sum::<f64>() / nf
        };
        let observed = stat(&a, &b);
        let p1 = pair_circular_shift_null_p(&a, &b, observed, stat, 100, 0xCAFE);
        let p2 = pair_circular_shift_null_p(&a, &b, observed, stat, 100, 0xCAFE);
        assert_eq!(p1.to_bits(), p2.to_bits());
        assert!((0.0..=1.0).contains(&p1));
    }

    /// SEAS hour-of-day closure: 96 bars at 15m → 95 returns → 24 buckets.
    /// Closure recomputes `max_abs_finite(t_stats)` over the resampled series.
    #[test]
    fn seas_hour_of_day_closure_returns_finite_scalar() {
        let bars = lcg_bar_frame(96, 14, 0);
        let req = make_req("seas.bucket.hour_of_day", &serde_json::json!({}));
        let closure = stat_closure_for("seas.bucket.hour_of_day@1", &req, &bars).expect("Some");
        // Use the actual returns series as the "resample" — confirms the
        // closure produces a finite scalar consistent with the scan's
        // Effect.value formula.
        let returns = log_returns(&bars.close);
        let v = closure(&returns);
        assert!(v.is_finite(), "max_abs_t_stat must be finite; got {v}");
        assert!(v >= 0.0, "abs is always >= 0; got {v}");
    }

    /// `pair_stat_closure_for` returns `None` for unwired pair scans.
    #[test]
    fn pair_stat_closure_returns_none_for_unwired() {
        let req = make_req("nope", &serde_json::json!({}));
        assert!(pair_stat_closure_for("nope@1", &req).is_none());
    }

    // Tag-only: surface `blake3_hex_zero` so it's not flagged unused.
    #[allow(dead_code)]
    fn _suppress_unused() {
        let _ = blake3_hex_zero();
    }
}
