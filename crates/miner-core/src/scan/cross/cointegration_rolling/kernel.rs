//! Rolling Engle-Granger cointegration kernel + breakdown detection
//! (CROSS-06, RAD-3626).
//!
//! Slides a fixed-size window over two aligned price-LEVEL series and runs the
//! canonical Engle-Granger two-step + OU half-life ONCE per window end. The
//! per-fit math is NOT re-derived here: every window delegates to
//! [`crate::scan::cross::engle_granger::kernel::engle_granger`] (CROSS-05), the
//! factored per-fit kernel. This kernel only owns (1) the window iteration and
//! (2) the **breakdown** detector layered on top of the per-window stats.
//!
//! ## Why rolling
//!
//! Whole-sample Engle-Granger hides regime breaks — a pair that was
//! cointegrated for years can decouple, and the single-shot ADF statistic
//! averages the broken tail back into the stationary history, masking the
//! break. Re-fitting per window surfaces the regime change as a run of
//! windows where the residual ADF crosses from stationary → non-stationary
//! and/or the hedge ratio drifts.
//!
//! ## Breakdown detector
//!
//! Two independent, OR-combined signals evaluated per window `i`:
//!
//! 1. **ADF lost stationarity (hysteresis).** Once the pair has been
//!    *established stationary* at any earlier window (residual ADF
//!    `p <= adf_p_enter`), a later window whose residual ADF `p > adf_p_exit`
//!    trips the flag. The enter/exit hysteresis (`adf_p_enter <= adf_p_exit`)
//!    prevents flapping around a single threshold. A window that is the first
//!    to be stationary cannot itself be a breakdown (nothing established yet).
//!
//! 2. **Beta drift.** `|beta_i|` drifts outside
//!    `median ± beta_band · median`, where `median` is the trailing median of
//!    `|beta|` over the previous [`BETA_MEDIAN_LOOKBACK`] windows (current
//!    window excluded). Requires at least [`MIN_BETA_HISTORY`] prior windows
//!    so the baseline is meaningful, and a non-degenerate baseline
//!    (`median > BETA_MEDIAN_EPS`) so the fractional band is well-defined.
//!
//! The emitted [`BreakdownReason`] records which signal(s) fired; the scan
//! body encodes it as an f64 code (the wire `effect.extra` is single-Dtype
//! F64) per [`BreakdownReason::as_f64`].
//!
//! Determinism: pure arithmetic over the inputs — no RNG, no clock, no
//! allocation-order-dependent output. Two runs over identical inputs produce
//! bit-identical vectors (OUT-03).

#![cfg_attr(any(test, debug_assertions), allow(clippy::float_cmp))]

use crate::scan::cross::engle_granger::kernel::{AdfRegression, engle_granger};
use crate::scan::primitives::robust::median_in_place;

/// Trailing-median lookback (in windows) for the beta-drift baseline. The
/// median is computed over at most this many *prior* windows.
const BETA_MEDIAN_LOOKBACK: usize = 20;

/// Minimum number of prior windows required before the beta-drift signal can
/// fire. Below this the trailing median is too noisy to be a baseline.
const MIN_BETA_HISTORY: usize = 5;

/// A trailing-median magnitude at or below this is treated as degenerate
/// (≈ zero hedge ratio); the fractional beta-drift band is skipped to avoid a
/// divide-by-tiny blow-up.
const BETA_MEDIAN_EPS: f64 = 1e-9;

/// Which signal(s) tripped the breakdown flag for a window. Encoded on the
/// wire as an f64 code (`effect.extra` is single-Dtype F64) via
/// [`BreakdownReason::as_f64`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakdownReason {
    /// No breakdown — the relationship held this window.
    None = 0,
    /// Residual ADF crossed from established-stationary → non-stationary
    /// (`p > adf_p_exit` after a prior window had `p <= adf_p_enter`).
    AdfLostStationarity = 1,
    /// `|beta|` drifted outside the trailing-median band.
    BetaDrift = 2,
    /// Both the ADF and beta-drift signals fired.
    Both = 3,
}

impl BreakdownReason {
    /// Stable integer code (0..=3).
    #[must_use]
    pub fn code(self) -> u8 {
        self as u8
    }

    /// Wire encoding — the f64 the scan body writes into `effect.extra`.
    #[must_use]
    pub fn as_f64(self) -> f64 {
        f64::from(self.code())
    }

    /// Did this reason flag a breakdown (anything other than `None`)?
    #[must_use]
    pub fn is_breakdown(self) -> bool {
        self != BreakdownReason::None
    }

    fn from_signals(adf: bool, beta: bool) -> Self {
        match (adf, beta) {
            (true, true) => BreakdownReason::Both,
            (true, false) => BreakdownReason::AdfLostStationarity,
            (false, true) => BreakdownReason::BetaDrift,
            (false, false) => BreakdownReason::None,
        }
    }
}

/// Breakdown thresholds (all resolved + validated by the scan body).
#[derive(Debug, Clone, Copy)]
pub struct BreakdownThresholds {
    /// Residual ADF p-value AT OR BELOW which a window counts as stationary
    /// (cointegrated). The "established" half of the hysteresis.
    pub adf_p_enter: f64,
    /// Residual ADF p-value ABOVE which an already-established pair counts as
    /// having lost stationarity. Must satisfy `adf_p_enter <= adf_p_exit`.
    pub adf_p_exit: f64,
    /// Fractional band around the trailing median of `|beta|`. `0.25` ⇒ a
    /// window trips beta-drift when `|beta|` leaves `±25 %` of the median.
    pub beta_band: f64,
}

impl Default for BreakdownThresholds {
    fn default() -> Self {
        Self {
            adf_p_enter: 0.05,
            adf_p_exit: 0.10,
            beta_band: 0.25,
        }
    }
}

/// Per-window rolling cointegration output. Every `Vec` has length
/// `n_windows`; index `i` describes the window ending at `window_end_idx[i]`.
pub struct RollingCointegrationResult {
    /// Inclusive aligned-series start index of each window.
    pub window_start_idx: Vec<usize>,
    /// Inclusive aligned-series end index of each window.
    pub window_end_idx: Vec<usize>,
    /// Per-window hedge ratio β (regressand = leg a, regressor = leg b).
    pub betas: Vec<f64>,
    /// Per-window intercept α.
    pub alphas: Vec<f64>,
    /// Per-window residual ADF statistic.
    pub adf_stats: Vec<f64>,
    /// Per-window residual ADF p-value.
    pub adf_p_values: Vec<f64>,
    /// Per-window OU half-life (`f64::INFINITY` sentinel when non-mean-
    /// reverting, matching `engle_granger`).
    pub ou_half_lives: Vec<f64>,
    /// Per-window residual sample std (ddof=1).
    pub residual_stds: Vec<f64>,
    /// Per-window breakdown flag.
    pub breakdown_flags: Vec<bool>,
    /// Per-window breakdown reason.
    pub breakdown_reasons: Vec<BreakdownReason>,
}

impl RollingCointegrationResult {
    /// Number of windows emitted.
    #[must_use]
    pub fn len(&self) -> usize {
        self.betas.len()
    }

    /// True when no windows were produced.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.betas.is_empty()
    }

    /// Count of windows whose breakdown flag tripped.
    #[must_use]
    pub fn breakdown_count(&self) -> usize {
        self.breakdown_flags.iter().filter(|b| **b).count()
    }
}

/// Roll the Engle-Granger per-fit kernel over `(y, x)` price levels.
///
/// `y = leg_a` (regressand), `x = leg_b` (regressor) — same D4-09 convention
/// as the single-shot kernel. Windows are `[start, start + window)` stepping by
/// `step`; the caller guarantees `1 <= window <= y.len()` and `step >= 1`.
///
/// The window iteration is contiguous over the supplied (already inner-joined,
/// gap-removed) series — a window never spans a gap because the engine
/// dispatches per contiguous sub-range (RAD-2397 per-sub-range contract).
#[must_use]
pub(crate) fn rolling_cointegration(
    y: &[f64],
    x: &[f64],
    window: usize,
    step: usize,
    regression: AdfRegression,
    thresholds: BreakdownThresholds,
) -> RollingCointegrationResult {
    debug_assert_eq!(y.len(), x.len(), "rolling_cointegration: len(y) != len(x)");
    debug_assert!(window >= 1, "window must be >= 1");
    debug_assert!(step >= 1, "step must be >= 1");
    let n = y.len();

    // Upper bound on the window count: ceil-free since start increments by step
    // while start + window <= n.
    let n_windows = if window > n {
        0
    } else {
        (n - window) / step + 1
    };

    let mut window_start_idx = Vec::with_capacity(n_windows);
    let mut window_end_idx = Vec::with_capacity(n_windows);
    let mut betas = Vec::with_capacity(n_windows);
    let mut alphas = Vec::with_capacity(n_windows);
    let mut adf_stats = Vec::with_capacity(n_windows);
    let mut adf_p_values = Vec::with_capacity(n_windows);
    let mut ou_half_lives = Vec::with_capacity(n_windows);
    let mut residual_stds = Vec::with_capacity(n_windows);
    let mut breakdown_flags = Vec::with_capacity(n_windows);
    let mut breakdown_reasons = Vec::with_capacity(n_windows);

    // Running breakdown state.
    let mut ever_stationary = false;
    let mut abs_beta_history: Vec<f64> = Vec::with_capacity(n_windows);

    let mut start = 0usize;
    while start + window <= n {
        let end = start + window - 1;
        let fit = engle_granger(
            &y[start..start + window],
            &x[start..start + window],
            regression,
        );

        let beta = fit.hedge_ratio_beta;
        let adf_p = fit.adf_p_value;

        // Signal 1 — ADF stationarity hysteresis.
        let stationary_now = adf_p.is_finite() && adf_p <= thresholds.adf_p_enter;
        let adf_breakdown = ever_stationary && adf_p.is_finite() && adf_p > thresholds.adf_p_exit;

        // Signal 2 — beta drift vs trailing median of |beta| over PRIOR windows.
        let beta_breakdown = beta_drift(beta, &abs_beta_history, thresholds.beta_band);

        let reason = BreakdownReason::from_signals(adf_breakdown, beta_breakdown);

        window_start_idx.push(start);
        window_end_idx.push(end);
        betas.push(beta);
        alphas.push(fit.hedge_ratio_alpha);
        adf_stats.push(fit.adf_stat);
        adf_p_values.push(adf_p);
        ou_half_lives.push(fit.ou_half_life);
        residual_stds.push(fit.residual_std);
        breakdown_flags.push(reason.is_breakdown());
        breakdown_reasons.push(reason);

        // Advance state AFTER recording this window so signals only ever look
        // at strictly-prior history.
        ever_stationary = ever_stationary || stationary_now;
        if beta.is_finite() {
            abs_beta_history.push(beta.abs());
        }

        start += step;
    }

    RollingCointegrationResult {
        window_start_idx,
        window_end_idx,
        betas,
        alphas,
        adf_stats,
        adf_p_values,
        ou_half_lives,
        residual_stds,
        breakdown_flags,
        breakdown_reasons,
    }
}

/// Beta-drift test: is `|beta|` outside `median ± band · median` of the
/// trailing `|beta|` history? Returns false (no signal) until there are at
/// least [`MIN_BETA_HISTORY`] prior windows and a non-degenerate baseline.
fn beta_drift(beta: f64, abs_beta_history: &[f64], band: f64) -> bool {
    if abs_beta_history.len() < MIN_BETA_HISTORY || !beta.is_finite() {
        return false;
    }
    let lookback_start = abs_beta_history.len().saturating_sub(BETA_MEDIAN_LOOKBACK);
    let mut window_hist: Vec<f64> = abs_beta_history[lookback_start..].to_vec();
    let med = median_in_place(&mut window_hist);
    if med.is_nan() || med <= BETA_MEDIAN_EPS {
        return false;
    }
    let lower = med * (1.0 - band);
    let upper = med * (1.0 + band);
    let ab = beta.abs();
    ab < lower || ab > upper
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    fn thresholds() -> BreakdownThresholds {
        BreakdownThresholds::default()
    }

    /// LCG random walk (deterministic) starting at 1.0.
    fn random_walk(n: usize, seed: u32, scale: f64) -> Vec<f64> {
        let mut s = seed;
        let mut acc = 1.0_f64;
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let frac = f64::from(s) / f64::from(u32::MAX) - 0.5;
            acc += frac * scale;
            out.push(acc);
        }
        out
    }

    /// Window count matches the closed-form `(n - window)/step + 1`.
    #[test]
    fn window_count_matches_closed_form() {
        let y = random_walk(100, 1, 0.01);
        let x = random_walk(100, 2, 0.01);
        let res = rolling_cointegration(&y, &x, 40, 5, AdfRegression::Constant, thresholds());
        assert_eq!(res.len(), (100 - 40) / 5 + 1);
        assert_eq!(res.window_start_idx.first().copied(), Some(0));
        assert_eq!(res.window_end_idx.first().copied(), Some(39));
        assert_eq!(res.window_start_idx.last().copied(), Some(60));
        assert_eq!(res.window_end_idx.last().copied(), Some(99));
    }

    /// All output vectors are the same length as `betas`.
    #[test]
    fn all_vectors_equal_length() {
        let y = random_walk(120, 3, 0.01);
        let x = random_walk(120, 4, 0.01);
        let res = rolling_cointegration(&y, &x, 50, 3, AdfRegression::Constant, thresholds());
        let k = res.len();
        assert_eq!(res.window_start_idx.len(), k);
        assert_eq!(res.window_end_idx.len(), k);
        assert_eq!(res.alphas.len(), k);
        assert_eq!(res.adf_stats.len(), k);
        assert_eq!(res.adf_p_values.len(), k);
        assert_eq!(res.ou_half_lives.len(), k);
        assert_eq!(res.residual_stds.len(), k);
        assert_eq!(res.breakdown_flags.len(), k);
        assert_eq!(res.breakdown_reasons.len(), k);
    }

    /// Window larger than the series ⇒ zero windows (no panic).
    #[test]
    fn window_larger_than_series_yields_empty() {
        let y = random_walk(10, 5, 0.01);
        let x = random_walk(10, 6, 0.01);
        let res = rolling_cointegration(&y, &x, 50, 1, AdfRegression::Constant, thresholds());
        assert!(res.is_empty());
        assert_eq!(res.breakdown_count(), 0);
    }

    /// A persistently cointegrated pair (b is a walk, a = b + small stationary
    /// AR(1) noise) never trips the breakdown flag.
    #[test]
    fn persistently_cointegrated_never_breaks_down() {
        let n = 400;
        let b = random_walk(n, 0x1357_9BDF, 0.02);
        // a = b + mean-reverting AR(1) residual.
        let mut se: u32 = 0x0ACE_F123;
        let mut e_prev = 0.0_f64;
        let mut a = Vec::with_capacity(n);
        for &bi in &b {
            se = se.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let noise = (f64::from(se) / f64::from(u32::MAX) - 0.5) * 0.01;
            let e = 0.3_f64 * e_prev + noise;
            a.push(bi + e);
            e_prev = e;
        }
        let res = rolling_cointegration(&a, &b, 60, 10, AdfRegression::Constant, thresholds());
        assert!(!res.is_empty());
        assert_eq!(
            res.breakdown_count(),
            0,
            "persistently cointegrated pair must never trip breakdown; reasons={:?}",
            res.breakdown_reasons
        );
    }

    /// `BreakdownReason` wire codes are stable.
    #[test]
    fn breakdown_reason_codes_stable() {
        assert_eq!(BreakdownReason::None.as_f64(), 0.0);
        assert_eq!(BreakdownReason::AdfLostStationarity.as_f64(), 1.0);
        assert_eq!(BreakdownReason::BetaDrift.as_f64(), 2.0);
        assert_eq!(BreakdownReason::Both.as_f64(), 3.0);
        assert!(!BreakdownReason::None.is_breakdown());
        assert!(BreakdownReason::Both.is_breakdown());
    }

    /// Determinism: identical inputs ⇒ identical betas + breakdown flags.
    #[test]
    fn deterministic_across_runs() {
        let y = random_walk(200, 11, 0.02);
        let x = random_walk(200, 22, 0.02);
        let r1 = rolling_cointegration(&y, &x, 50, 7, AdfRegression::Constant, thresholds());
        let r2 = rolling_cointegration(&y, &x, 50, 7, AdfRegression::Constant, thresholds());
        assert_eq!(r1.betas, r2.betas);
        assert_eq!(r1.adf_p_values, r2.adf_p_values);
        assert_eq!(r1.breakdown_flags, r2.breakdown_flags);
    }
}
