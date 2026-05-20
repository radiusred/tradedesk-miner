//! Lead-lag cross-correlation function (CCF) kernel (CROSS-04).
//!
//! Computes the cross-correlation between two equal-length aligned series
//! `a` and `b` over a symmetric ±`max_lag` grid. The element at the centre
//! of the returned vector (index `max_lag`) is the full-sample Pearson
//! correlation between the two series; positive lags `+k` represent the
//! correlation of `a_t` with `b_{t+k}` (a leads b by k bars: compare today's
//! a with b k-bars-in-the-future); negative lags `-k` the inverse
//! (b leads a by k bars).
//!
//! Reference: `scipy.signal.correlate(a, b, mode='full')` (with the
//! normalisation step) or `statsmodels.tsa.stattools.ccf`. Tolerance 1e-10
//! at the per-lag level.
//!
//! The argmax-lag is selected by **absolute value** (the strongest
//! lead/lag signal regardless of sign) — this is the canonical lead-lag
//! signal for pairs-trading inference (RESEARCH.md §Section 2 for
//! `cross.lead_lag.ccf@1`).
//!
//! Per RESEARCH §1.9 the timeframe-conditional recommendation for the lag
//! grid is `{1m → 50, 15m → 20, 1h → 10, 1d → 7}`; this kernel takes
//! `max_lag` as a parameter so callers can dispatch on the timeframe at
//! their option. The scan-level default is a single `20` (see
//! `lead_lag::mod::lead_lag_param_schema`) for simplicity over per-timeframe
//! dispatch; users override via `--params max_lag=N`.

#![cfg_attr(any(test, debug_assertions), allow(clippy::float_cmp))]

/// Per-lag CCF result with the argmax-lag selection (absolute-value).
///
/// `lags` is the symmetric integer grid `[-max_lag, ..., -1, 0, 1, ..., max_lag]`
/// in ascending order, length `2 * max_lag + 1`. `ccf_values[i]` is the
/// per-lag Pearson correlation at `lags[i]`.
///
/// `argmax_lag` is the lag whose `|ccf_values[i]|` is maximal (ties broken
/// by lowest `i` — i.e. most-negative lag — via the `position_max_by`
/// stable-iteration convention). `argmax_value` is the **signed**
/// `ccf_values` entry at that index.
pub struct LeadLagResult {
    pub lags: Vec<i64>,
    pub ccf_values: Vec<f64>,
    pub argmax_lag: i64,
    pub argmax_value: f64,
    pub max_lag: usize,
}

impl LeadLagResult {
    /// Number of per-lag entries = `2 * max_lag + 1`.
    #[must_use]
    pub fn len(&self) -> usize {
        self.ccf_values.len()
    }

    /// True iff no lags were emitted (`max_lag` was zero — invalid; the
    /// kernel never returns this naturally, callers reject `max_lag == 0`
    /// up-stream).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ccf_values.is_empty()
    }
}

/// Cross-correlation function over a symmetric ±`max_lag` grid.
///
/// Computes, for each `lag ∈ [-max_lag..=max_lag]`, the Pearson correlation
/// of `a` and `b` shifted by `lag` (positive lags correlate `a_t` with
/// `b_{t+lag}` — "a leads b by lag bars"; e.g. for `b_t = a_{t-2}` the CCF
/// peaks at `lag = +2`). The result vector is in ascending lag order; the
/// middle element (`ccf_values[max_lag]`) is the full-sample Pearson
/// correlation.
///
/// **Sequential** lag loop (NOT `par_iter`): determinism across runs is the
/// load-bearing property; rayon would produce identical results but the
/// kernel is small enough that the work-stealing overhead is a net loss for
/// typical `max_lag <= 50`. Cancellation is polled from the caller before
/// AND after this function — the kernel itself is uninterruptible.
///
/// Returns a [`LeadLagResult`] carrying the full lag grid + the
/// absolute-value argmax-lag selection.
///
/// # Degenerate cases
///
/// - If `sigma_a == 0.0` OR `sigma_b == 0.0` (zero sample variance in
///   either input), the per-lag correlations are undefined and every
///   `ccf_values` entry is `f64::NAN`. The caller (`lead_lag::mod::run`)
///   detects the NaN and converts to `ScanError::Kernel`.
/// - If `max_lag == 0`, the result has a single entry at lag 0; the
///   caller validates `max_lag >= 1` upstream so the kernel does not
///   special-case it.
/// - If `max_lag >= n`, every shifted correlation has `N_eff <= 0`; the
///   caller validates `max_lag < n / 2` upstream so this is unreachable.
#[inline]
#[must_use]
pub(super) fn lead_lag_ccf(a: &[f64], b: &[f64], max_lag: usize) -> LeadLagResult {
    debug_assert_eq!(a.len(), b.len(), "lead_lag_ccf: a.len() must equal b.len()");
    let n = a.len();
    let lag_count = 2 * max_lag + 1;

    // Full-sample mean and population std (ddof=0). The CCF normalisation
    // by sigma_a * sigma_b uses the full-sample sigmas; per-lag windows
    // re-use these moments (the standard non-windowed CCF definition,
    // matching scipy.signal.correlate after normalisation).
    #[allow(clippy::cast_precision_loss, reason = "n bounded by aligned bars; << 2^52")]
    let n_f = n as f64;
    let mean_a = a.iter().copied().sum::<f64>() / n_f;
    let mean_b = b.iter().copied().sum::<f64>() / n_f;
    let var_a = a.iter().map(|v| (v - mean_a).powi(2)).sum::<f64>() / n_f;
    let var_b = b.iter().map(|v| (v - mean_b).powi(2)).sum::<f64>() / n_f;
    let sigma_a = var_a.sqrt();
    let sigma_b = var_b.sqrt();

    // Build the lag grid: [-max_lag, ..., -1, 0, 1, ..., max_lag].
    let mut lags: Vec<i64> = Vec::with_capacity(lag_count);
    #[allow(clippy::cast_possible_wrap, reason = "max_lag bounded above by n/2 << i64::MAX")]
    let max_lag_i = max_lag as i64;
    for k in -max_lag_i..=max_lag_i {
        lags.push(k);
    }

    let mut ccf_values: Vec<f64> = Vec::with_capacity(lag_count);

    // Degenerate variance branch: produce NaN entries; caller converts to
    // ScanError::Kernel.
    let denom = sigma_a * sigma_b;
    if denom == 0.0 || !denom.is_finite() {
        for _ in 0..lag_count {
            ccf_values.push(f64::NAN);
        }
        return LeadLagResult {
            lags,
            ccf_values,
            argmax_lag: 0,
            argmax_value: f64::NAN,
            max_lag,
        };
    }

    // Per-lag Pearson correlation. The N_eff normalisation is `n - |lag|`
    // (using only the overlapping pairs); matches statsmodels.tsa.stattools.ccf
    // default behaviour. The mean/std come from the FULL sample so the
    // estimator is the classic "unbiased mean + biased lag-N_eff covariance"
    // form rather than per-window re-centring.
    //
    // Sign convention: positive `k` means **a leads b by k bars**, so the
    // CCF at lag +k correlates `a_t` with `b_{t+k}` (i.e. compare today's a
    // with b k-bars-in-the-future); equivalently `b_t` matches `a_{t-k}` if
    // a leads. For the test case `b[t] = a[t-2]` (a leads b by 2) the CCF
    // peaks at k=+2 because `b_{t+2} = a_t`. Negative `k` flips the roles.
    for &k in &lags {
        #[allow(
            clippy::cast_possible_truncation,
            reason = "k is bounded by max_lag which is CLI-validated to <= n/4; fits in usize on every supported target"
        )]
        let abs_k = k.unsigned_abs() as usize;
        #[allow(clippy::cast_precision_loss, reason = "n_eff bounded by n; << 2^52")]
        let n_eff_f = (n - abs_k) as f64;
        let cov = if k >= 0 {
            // k >= 0: a leads b by k. Sum `(a_t - μ_a)(b_{t+k} - μ_b)` over
            // valid t ∈ [0..n-k); equivalently over the overlapping pairs.
            let mut sum = 0.0_f64;
            for t in 0..(n - abs_k) {
                sum += (a[t] - mean_a) * (b[t + abs_k] - mean_b);
            }
            sum / n_eff_f
        } else {
            // k < 0: b leads a by |k|. Sum `(a_{t+|k|} - μ_a)(b_t - μ_b)`
            // over valid t ∈ [0..n-|k|).
            let mut sum = 0.0_f64;
            for t in 0..(n - abs_k) {
                sum += (a[t + abs_k] - mean_a) * (b[t] - mean_b);
            }
            sum / n_eff_f
        };
        ccf_values.push(cov / denom);
    }

    // Argmax by absolute value. Initialize with the first element so the
    // accumulator's |value| is a real comparable quantity (a NEG_INFINITY
    // sentinel breaks the comparison because |NEG_INFINITY| is +INFINITY,
    // which no finite |v| can exceed — every iteration's "greater-than"
    // check then fails and the accumulator never updates, leaving the
    // bogus initial (0, NEG_INFINITY)).
    let (argmax_idx, argmax_value) = ccf_values
        .iter()
        .enumerate()
        .skip(1)
        .fold((0_usize, ccf_values[0]), |acc, (i, v)| {
            if v.abs() > acc.1.abs() { (i, *v) } else { acc }
        });
    let argmax_lag = lags[argmax_idx];

    LeadLagResult {
        lags,
        ccf_values,
        argmax_lag,
        argmax_value,
        max_lag,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    const TOL: f64 = 1e-10;

    fn approx_eq(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    /// Hand-derived: identical inputs -> lag-0 correlation is 1.0; argmax is 0.
    #[test]
    fn lead_lag_identical_series_argmax_zero() {
        let series = [1.0_f64, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        let res = lead_lag_ccf(&series, &series, 3);
        assert_eq!(res.len(), 7); // 2*3 + 1
        // Centre element (index = max_lag) is the lag-0 correlation.
        assert!(
            approx_eq(res.ccf_values[3], 1.0, TOL),
            "lag-0 corr must be 1.0; got {}",
            res.ccf_values[3]
        );
        assert_eq!(res.argmax_lag, 0);
        assert!(approx_eq(res.argmax_value, 1.0, TOL));
        // Lag grid is symmetric and ascending.
        assert_eq!(res.lags, vec![-3, -2, -1, 0, 1, 2, 3]);
    }

    /// Hand-derived: when `b` is `a` shifted forward by 2 bars (so `b_t = a_{t-2}`
    /// for `t >= 2`), the CCF should peak at lag = +2 (a leads b by 2).
    ///
    /// Uses an asymmetric chirp signal (varying frequency) so the CCF has a
    /// UNIQUE absolute maximum at the true shift — a pure sine would alias
    /// (cos(ω*4) ≠ 1 in general but |corr(±k)| = |corr(∓k)| under the
    /// product-to-sum identity, making the argmax ambiguous). The chirp's
    /// non-stationary frequency breaks that symmetry. The prefix [0..2] is
    /// filled with the chirp extrapolated backward so legs share moments.
    #[test]
    fn lead_lag_shifted_series_argmax_matches() {
        let n = 60;
        // Chirp: instantaneous frequency increases with i so the signal is
        // non-stationary and the CCF is unambiguously asymmetric.
        let chirp = |i_f: f64| -> f64 {
            let phase = 0.3_f64 * i_f + 0.005_f64 * i_f * i_f;
            phase.sin()
        };
        let mut a: Vec<f64> = Vec::with_capacity(n);
        for i in 0..n {
            a.push(chirp(i as f64));
        }
        // b_t = a_{t-2} for t >= 2; prefix extrapolates the chirp backward.
        let mut b = vec![0.0_f64; n];
        b[0] = chirp(-2.0);
        b[1] = chirp(-1.0);
        for t in 2..n {
            b[t] = a[t - 2];
        }
        let res = lead_lag_ccf(&a, &b, 5);
        // Argmax lag should be +2 (a leads b by 2).
        assert_eq!(
            res.argmax_lag, 2,
            "expected argmax_lag = +2 for shifted-by-2; got {} (ccf={:?})",
            res.argmax_lag, res.ccf_values
        );
    }

    /// Lag-0 element equals the full-sample Pearson correlation within 1e-10.
    #[test]
    fn lead_lag_zero_lag_equals_pearson() {
        let a = [1.0_f64, 1.1, 0.9, 1.2, 1.05, 0.95, 1.15];
        let b = [2.0_f64, 2.3, 1.8, 2.6, 2.2, 1.9, 2.5];
        let res = lead_lag_ccf(&a, &b, 2);
        // Compute Pearson manually.
        #[allow(clippy::cast_precision_loss)]
        let n_f = a.len() as f64;
        let mean_a = a.iter().sum::<f64>() / n_f;
        let mean_b = b.iter().sum::<f64>() / n_f;
        let var_a = a.iter().map(|v| (v - mean_a).powi(2)).sum::<f64>() / n_f;
        let var_b = b.iter().map(|v| (v - mean_b).powi(2)).sum::<f64>() / n_f;
        let cov = a
            .iter()
            .zip(b.iter())
            .map(|(va, vb)| (va - mean_a) * (vb - mean_b))
            .sum::<f64>()
            / n_f;
        let expected_pearson = cov / (var_a.sqrt() * var_b.sqrt());
        // Centre index is res.max_lag.
        assert!(
            approx_eq(res.ccf_values[res.max_lag], expected_pearson, TOL),
            "lag-0 = {}, expected Pearson = {}",
            res.ccf_values[res.max_lag],
            expected_pearson
        );
    }

    /// Lag grid is the symmetric integer sequence [-`max_lag..=max_lag`] in
    /// ascending order with length 2 * `max_lag` + 1.
    #[test]
    fn lead_lag_grid_symmetric_and_ascending() {
        let a = [1.0_f64, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];
        let res = lead_lag_ccf(&a, &a, 4);
        assert_eq!(res.lags, vec![-4, -3, -2, -1, 0, 1, 2, 3, 4]);
        assert_eq!(res.lags.len(), 9);
        assert_eq!(res.ccf_values.len(), 9);
        // Ascending check.
        for w in res.lags.windows(2) {
            assert!(w[0] < w[1]);
        }
    }

    /// Zero-variance input produces NaN entries — caller detects + converts.
    #[test]
    fn lead_lag_zero_variance_produces_nan() {
        let a = [1.0_f64; 10]; // constant
        let b = [1.0_f64, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];
        let res = lead_lag_ccf(&a, &b, 2);
        for v in &res.ccf_values {
            assert!(v.is_nan(), "expected NaN for zero-variance leg; got {v}");
        }
        assert!(res.argmax_value.is_nan());
    }

    /// Negatively-correlated series argmax via absolute value: a = -b
    /// exactly gives lag-0 correlation = -1, |corr| = 1, `argmax_lag` = 0,
    /// `argmax_value` = -1.
    #[test]
    fn lead_lag_negative_correlation_absolute_argmax() {
        let a = [1.0_f64, 2.0, 3.0, 4.0, 5.0];
        let b: Vec<f64> = a.iter().map(|v| -v).collect();
        let res = lead_lag_ccf(&a, &b, 1);
        assert!(approx_eq(res.ccf_values[res.max_lag], -1.0, TOL));
        assert_eq!(res.argmax_lag, 0);
        assert!(approx_eq(res.argmax_value, -1.0, TOL));
    }

    /// Reverse-shifted: `a_t = b_{t-3}` (b leads a by 3 => `argmax_lag` = -3).
    /// Uses an asymmetric chirp signal (see `lead_lag_shifted_series_argmax_matches`)
    /// so the CCF has a UNIQUE absolute maximum at the true shift.
    #[test]
    fn lead_lag_b_leads_a_argmax_negative() {
        let n = 60;
        let chirp = |i_f: f64| -> f64 {
            let phase = 0.25_f64 * i_f + 0.004_f64 * i_f * i_f;
            phase.cos()
        };
        let mut b: Vec<f64> = Vec::with_capacity(n);
        for i in 0..n {
            b.push(chirp(i as f64));
        }
        // a_t = b_{t-3} for t >= 3; prefix [0..3] extrapolates the chirp backward.
        let mut a = vec![0.0_f64; n];
        a[0] = chirp(-3.0);
        a[1] = chirp(-2.0);
        a[2] = chirp(-1.0);
        for t in 3..n {
            a[t] = b[t - 3];
        }
        let res = lead_lag_ccf(&a, &b, 5);
        assert_eq!(
            res.argmax_lag, -3,
            "expected argmax_lag = -3 for b-leads-a-by-3; got {} (ccf={:?})",
            res.argmax_lag, res.ccf_values
        );
    }
}
