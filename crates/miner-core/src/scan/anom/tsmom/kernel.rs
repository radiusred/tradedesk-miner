//! Pure intraday time-series-momentum (TSMOM) kernel for ANOM-12 — `tsmom`.
//!
//! Pattern analog: [`crate::scan::anom::variance_ratio::kernel`] and the
//! sibling `adf/kernel.rs` / `kpss/kernel.rs` — private `pub(crate)` pure
//! functions over `&[f64]` with a sibling `#[cfg(test)] mod tests` block. No
//! IO, no `serde_json`, no `Reader` calls.
//!
//! ## Reference
//!
//! Time-series momentum (return continuation): Moskowitz, T. J., Ooi, Y. H. &
//! Pedersen, L. H. (2012), "Time Series Momentum", Journal of Financial
//! Economics 104(2), 228-250. The canonical TSMOM signal trades the sign of
//! the trailing-`k` return and holds for the next `k` bars; the continuation
//! coefficient is the slope of next-`k` return regressed on past-`k` return.
//!
//! ## Algorithm (per horizon `k`)
//!
//! 1. **Non-overlapping `k`-blocks.** Partition the return series into
//!    consecutive blocks of `k` bars (dropping the trailing remainder); block
//!    `b` carries `R_b = Σ r_t` over its `k` returns. Non-overlapping blocks
//!    keep the `(past, next)` pairs (near-)independent so the OLS t-stat is
//!    honest — overlapping windows would inflate it via induced
//!    autocorrelation.
//! 2. **Continuation regression.** Form pairs `(R_b, R_{b+1})` and fit OLS
//!    `R_{b+1} = α + β·R_b`. The slope `β` is the continuation coefficient:
//!    `β > 0` ⇒ momentum (the past block predicts the next in the same
//!    direction), `β < 0` ⇒ mean reversion. Under a random walk `β ≈ 0`.
//! 3. **t-stat / p-value.** `t = β / SE(β)`, two-sided p from Student-t with
//!    `df = m - 2` (`m` = number of pairs).
//! 4. **Hit-rate.** Fraction of pairs whose past and next blocks share sign
//!    (`R_b · R_{b+1} > 0`) — the directional accuracy of the signal.
//! 5. **TSMOM mean.** Mean of `sign(R_b)·R_{b+1}` — the average per-block
//!    return to holding the next block in the direction of the past one.
//!
//! Vol-normalisation of the input return series (the `scaling` param) is
//! applied by the caller via [`vol_normalize`] before the per-`k` blocks are
//! formed; a global rescale would leave `β`/`t`/hit-rate unchanged (OLS slope
//! is scale-invariant), so the normalisation is deliberately *time-varying*
//! (ex-ante trailing vol).

#![cfg_attr(any(test, debug_assertions), allow(clippy::float_cmp))]

use statrs::distribution::{ContinuousCDF, StudentsT};

/// Trailing window (in bars) for the ex-ante volatility estimate used by
/// [`vol_normalize`]. 20 bars is a standard intraday lookback.
pub(crate) const VOL_WINDOW: usize = 20;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TsmomResult {
    /// Continuation coefficient: OLS slope of next-block on past-block return.
    pub continuation_coef: f64,
    /// t-statistic of the continuation coefficient.
    pub t_stat: f64,
    /// Two-sided p-value of the continuation coefficient (Student-t, df=m-2).
    pub p_value: f64,
    /// Directional hit-rate: fraction of block pairs sharing sign.
    pub hit_rate: f64,
    /// Mean per-block return to the sign(past)·next TSMOM rule.
    pub tsmom_mean: f64,
    /// Number of `(past, next)` block pairs used.
    pub n_pairs: usize,
}

/// Sample standard deviation (unbiased, ddof=1). Returns `0.0` for `< 2`
/// elements.
#[inline]
#[must_use]
#[allow(clippy::cast_precision_loss, reason = "element count << 2^52")]
pub(crate) fn sample_std(xs: &[f64]) -> f64 {
    let n = xs.len();
    if n < 2 {
        return 0.0;
    }
    let n_f = n as f64;
    let mean = xs.iter().sum::<f64>() / n_f;
    let var = xs.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / (n_f - 1.0);
    var.sqrt()
}

/// Ex-ante (look-ahead-free) volatility normalisation: divide each return by
/// the sample std of the *strictly preceding* returns in a trailing `window`.
///
/// The first one/two bars (fewer than two past observations) fall back to the
/// full-series std — a bounded warmup approximation acceptable for a
/// measurement scan. A constant series (global std `0`) is normalised against
/// a tiny positive floor; the caller's degenerate-variance guard rejects it
/// downstream regardless.
#[inline]
#[must_use]
pub(crate) fn vol_normalize(returns: &[f64], window: usize) -> Vec<f64> {
    let n = returns.len();
    if n == 0 {
        return Vec::new();
    }
    let global = {
        let g = sample_std(returns);
        if g > 0.0 && g.is_finite() {
            g
        } else {
            f64::MIN_POSITIVE
        }
    };
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let lo = i.saturating_sub(window);
        let past = &returns[lo..i];
        let sigma = if past.len() >= 2 {
            let s = sample_std(past);
            if s > 0.0 && s.is_finite() { s } else { global }
        } else {
            global
        };
        out.push(returns[i] / sigma);
    }
    out
}

/// Compute the TSMOM continuation statistics for a return series at horizon
/// `k` using non-overlapping `k`-blocks.
///
/// The caller guarantees `k >= 1` and `returns.len() / k >= 4` (at least three
/// `(past, next)` block pairs so the OLS has `df = m - 2 >= 1`).
#[inline]
#[allow(clippy::cast_precision_loss, reason = "block / pair counts << 2^52")]
#[allow(
    clippy::similar_names,
    reason = "sx/sy/sxx/sxy/dx/dy are the canonical OLS accumulator names"
)]
#[allow(
    clippy::many_single_char_names,
    reason = "n/k/m are canonical OLS/TSMOM names; x/y are the standard regressor/regressand pair"
)]
pub(crate) fn tsmom_continuation(returns: &[f64], k: usize) -> Result<TsmomResult, String> {
    let n = returns.len();
    if k < 1 {
        return Err(format!("tsmom: k must be >= 1; got k={k}"));
    }
    let num_blocks = n / k;
    if num_blocks < 4 {
        return Err(format!(
            "tsmom: k={k} too large for n={n} (need n >= 4*k for >= 3 block pairs)"
        ));
    }

    // Step 1 — non-overlapping block sums (drop the trailing remainder).
    let mut blocks: Vec<f64> = Vec::with_capacity(num_blocks);
    for b in 0..num_blocks {
        let sum: f64 = returns[b * k..(b + 1) * k].iter().sum();
        blocks.push(sum);
    }

    // Step 2 — (past, next) pairs: x = R_b, y = R_{b+1}.
    let m = num_blocks - 1;
    let m_f = m as f64;
    let mut sx = 0.0_f64;
    let mut sy = 0.0_f64;
    for j in 0..m {
        sx += blocks[j];
        sy += blocks[j + 1];
    }
    let xbar = sx / m_f;
    let ybar = sy / m_f;

    let mut sxx = 0.0_f64;
    let mut sxy = 0.0_f64;
    let mut hits = 0usize;
    let mut tsmom_sum = 0.0_f64;
    for j in 0..m {
        let x = blocks[j];
        let y = blocks[j + 1];
        let dx = x - xbar;
        let dy = y - ybar;
        sxx += dx * dx;
        sxy += dx * dy;
        if x * y > 0.0 {
            hits += 1;
        }
        // Directional TSMOM return: hold the next block in the sign of past.
        if x > 0.0 {
            tsmom_sum += y;
        } else if x < 0.0 {
            tsmom_sum -= y;
        }
    }

    if sxx <= 0.0 || !sxx.is_finite() {
        return Err(format!(
            "tsmom: degenerate past-block variance (constant blocks?) at k={k}"
        ));
    }

    // Step 3 — OLS slope + t-stat.
    let beta = sxy / sxx;
    let alpha = ybar - beta * xbar;
    let mut sse = 0.0_f64;
    for j in 0..m {
        let resid = blocks[j + 1] - alpha - beta * blocks[j];
        sse += resid * resid;
    }
    let df = m - 2; // m >= 3 guaranteed (num_blocks >= 4).
    let s2 = sse / df as f64;
    let se_beta = (s2 / sxx).sqrt();
    // se_beta == 0 (perfect fit, beta != 0) -> t = ±inf -> p -> 0; handled in
    // two_sided_p_value.
    let t_stat = beta / se_beta;
    let p_value = two_sided_p_value(t_stat, df);

    Ok(TsmomResult {
        continuation_coef: beta,
        t_stat,
        p_value,
        hit_rate: hits as f64 / m_f,
        tsmom_mean: tsmom_sum / m_f,
        n_pairs: m,
    })
}

/// Two-sided Student-t p-value for a t-statistic with `df` degrees of freedom.
/// A finite `t` runs the standard path (`t == 0` falls out as `p = 1` since
/// `cdf(0) = 0.5`); non-finite `|t|` (perfect fit) ⇒ 0.0; NaN ⇒ NaN.
#[inline]
#[allow(clippy::cast_precision_loss, reason = "df << 2^52")]
fn two_sided_p_value(t: f64, df: usize) -> f64 {
    if !t.is_finite() {
        return if t.is_nan() { f64::NAN } else { 0.0 };
    }
    let dist = StudentsT::new(0.0, 1.0, df as f64).expect("students-t df >= 1");
    let upper_tail = 1.0 - dist.cdf(t.abs());
    (2.0 * upper_tail).clamp(0.0, 1.0)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::cast_lossless)]
mod tests {
    use super::*;

    fn approx_eq(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    /// Deterministic LCG noise in [-0.5, 0.5] — an IID white-noise return
    /// series (random walk in log-price), so continuation `β ≈ 0`.
    #[allow(clippy::cast_possible_truncation)]
    fn lcg_returns(n: usize, seed: u64) -> Vec<f64> {
        let mut s = seed as u32;
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            out.push(f64::from(s) / f64::from(u32::MAX) - 0.5);
        }
        out
    }

    /// AR(1) momentum series `r_t = phi·r_{t-1} + eps_t` with `phi > 0` —
    /// positive serial dependence, so block-to-block continuation `β > 0`.
    #[allow(clippy::cast_possible_truncation)]
    fn ar1_returns(n: usize, seed: u64, phi: f64) -> Vec<f64> {
        let eps = lcg_returns(n, seed);
        let mut out = Vec::with_capacity(n);
        let mut prev = 0.0_f64;
        for &e in &eps {
            let r = phi * prev + e;
            out.push(r);
            prev = r;
        }
        out
    }

    // -- two_sided_p_value -----------------------------------------------

    #[test]
    fn p_value_at_zero_is_unity() {
        assert!(approx_eq(two_sided_p_value(0.0, 10), 1.0, 1e-12));
    }

    #[test]
    fn p_value_large_t_is_tiny() {
        let p = two_sided_p_value(8.0, 30);
        assert!(p < 0.01, "p({}) = {p}", 8.0);
    }

    #[test]
    fn p_value_infinite_t_is_zero() {
        assert!(approx_eq(two_sided_p_value(f64::INFINITY, 10), 0.0, 1e-12));
    }

    #[test]
    fn p_value_nan_in_nan_out() {
        assert!(two_sided_p_value(f64::NAN, 10).is_nan());
    }

    #[test]
    fn p_value_symmetric_in_sign() {
        let a = two_sided_p_value(1.7, 25);
        let b = two_sided_p_value(-1.7, 25);
        assert!(approx_eq(a, b, 1e-12));
    }

    // -- sample_std ------------------------------------------------------

    #[test]
    fn sample_std_known_value() {
        // [2,4,4,4,5,5,7,9] has sample std 2.138... (ddof=1).
        let s = sample_std(&[2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0]);
        assert!(approx_eq(s, 2.138_089_935_299_395, 1e-9), "std = {s}");
    }

    #[test]
    fn sample_std_too_few_is_zero() {
        assert_eq!(sample_std(&[]), 0.0);
        assert_eq!(sample_std(&[3.0]), 0.0);
    }

    // -- vol_normalize ---------------------------------------------------

    #[test]
    fn vol_normalize_length_invariant() {
        let r = lcg_returns(100, 1);
        let v = vol_normalize(&r, VOL_WINDOW);
        assert_eq!(v.len(), r.len());
        assert!(v.iter().all(|x| x.is_finite()));
    }

    #[test]
    fn vol_normalize_empty() {
        assert!(vol_normalize(&[], VOL_WINDOW).is_empty());
    }

    /// Global rescale of the input does NOT change the normalised series'
    /// continuation statistics — but the time-varying normalisation itself is
    /// not a global rescale, so it generally changes them. Here we just pin
    /// that normalising white noise yields a finite, roughly unit-scale
    /// series.
    #[test]
    fn vol_normalize_unit_scale_on_white_noise() {
        let r = lcg_returns(500, 7);
        let v = vol_normalize(&r, VOL_WINDOW);
        let s = sample_std(&v);
        assert!(s > 0.3 && s < 4.0, "normalised std = {s} should be O(1)");
    }

    // -- tsmom_continuation: guards --------------------------------------

    #[test]
    fn tsmom_k_zero_errors() {
        let r = lcg_returns(40, 1);
        assert!(tsmom_continuation(&r, 0).is_err());
    }

    #[test]
    fn tsmom_k_too_large_errors() {
        // n=10, k=3 -> num_blocks=3 < 4 -> error.
        let r = lcg_returns(10, 1);
        assert!(tsmom_continuation(&r, 3).is_err());
    }

    #[test]
    fn tsmom_constant_blocks_errors() {
        let r = vec![0.0_f64; 40];
        assert!(
            tsmom_continuation(&r, 1).is_err(),
            "constant -> sxx=0 -> Err"
        );
    }

    // -- tsmom_continuation: behaviour -----------------------------------

    /// White-noise returns: continuation coefficient ≈ 0 and the directional
    /// hit-rate ≈ 0.5. (Both are robust to the seed — `β` sits ~7σ inside the
    /// tolerance and the hit-rate ~9σ — unlike a `p > 0.05` check which has an
    /// inherent ~5% per-seed false-positive rate under the null.)
    #[test]
    fn tsmom_white_noise_no_continuation() {
        let r = lcg_returns(2000, 42);
        let res = tsmom_continuation(&r, 1).expect("ok");
        assert!(
            res.continuation_coef.abs() < 0.15,
            "white-noise β = {} should be ≈ 0",
            res.continuation_coef
        );
        assert!(
            (res.hit_rate - 0.5).abs() < 0.1,
            "white-noise hit-rate = {} should be ≈ 0.5",
            res.hit_rate
        );
        assert!(
            res.p_value >= 0.0 && res.p_value <= 1.0,
            "p in [0,1]: {}",
            res.p_value
        );
    }

    /// AR(1) positive momentum: continuation coefficient significantly > 0 at
    /// the matching short horizon (k=1).
    #[test]
    fn tsmom_ar1_positive_continuation_significant() {
        let r = ar1_returns(2000, 99, 0.5);
        let res = tsmom_continuation(&r, 1).expect("ok");
        assert!(
            res.continuation_coef > 0.0,
            "AR(1) β = {} should be > 0",
            res.continuation_coef
        );
        assert!(
            res.p_value < 0.01,
            "AR(1) p = {} should be significant",
            res.p_value
        );
        assert!(
            res.hit_rate > 0.5,
            "AR(1) hit-rate = {} should beat a coin flip",
            res.hit_rate
        );
        assert!(res.tsmom_mean > 0.0, "AR(1) TSMOM mean should be positive");
    }

    /// Perfect anti-correlation at the block scale: alternating block signs
    /// yield a negative continuation coefficient (mean reversion).
    #[test]
    fn tsmom_mean_reverting_negative_continuation() {
        // Each bar is a one-element block at k=1; alternating ±1 with a tiny
        // perturbation so blocks are not perfectly collinear.
        let mut r = Vec::new();
        for i in 0..400 {
            let base = if i % 2 == 0 { 1.0 } else { -1.0 };
            let jitter = ((i as f64) * 0.01).sin() * 0.05;
            r.push(base + jitter);
        }
        let res = tsmom_continuation(&r, 1).expect("ok");
        assert!(
            res.continuation_coef < 0.0,
            "alternating β = {} should be < 0 (reversion)",
            res.continuation_coef
        );
    }

    #[test]
    fn tsmom_n_pairs_matches_blocks() {
        let r = lcg_returns(100, 3);
        // k=10 -> num_blocks=10 -> m=9 pairs.
        let res = tsmom_continuation(&r, 10).expect("ok");
        assert_eq!(res.n_pairs, 9);
    }
}
