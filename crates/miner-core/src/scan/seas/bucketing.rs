//! Shared bucket-statistics helper for the SEAS family — Plan 04-09 / PATTERNS Pattern D.
//!
//! Given a parallel pair of `(values, bucket_keys)` and a `num_buckets` count,
//! [`bucket_stats`] computes per-bucket mean / std (ddof=1) / count / t-stat / IQR
//! and returns them as parallel vectors of length `num_buckets`. Sparse buckets
//! (count < `min_obs`) yield `NaN` for mean / std / t-stat / IQR (count stays
//! exact).
//!
//! Used by:
//!
//! - [`super::hour_of_day`] (SEAS-01, 24 buckets, key = `ts.hour()`)
//! - [`super::day_of_week`] (SEAS-02, 7 buckets, key = `ts.weekday().num_days_from_monday()`)
//! - SEAS-04 / SEAS-06 in Plan 04-10 (end-of-month / event-window — same shape)
//!
//! ## Discipline (PATTERNS.md Pattern B)
//!
//! - Sequential Welford accumulation per bucket (Pitfall 4 — deterministic
//!   summation order; matches `pandas.groupby(...).std()`'s pairwise reduction
//!   within 1e-12 for the bar counts realistic to this workload).
//! - `debug_assert!` for the `num_buckets >= 1` invariant.
//! - IQR via partial-collection + sort: `O(n + Σ n_b log n_b)`. Cost is
//!   proportional to the bar count which is bounded by `realistic OHLCV slices`
//!   << 2^20 even for multi-year windows.

#![cfg_attr(any(test, debug_assertions), allow(clippy::float_cmp))]

/// Per-bucket statistics returned by [`bucket_stats`].
///
/// Vector indices are bucket keys in `0..num_buckets` — empty buckets are
/// present at their index with `count == 0` and `NaN` for the other fields.
#[derive(Debug, Clone, PartialEq)]
pub struct BucketResult {
    /// Per-bucket sample mean. `NaN` when `count < min_obs`.
    pub means: Vec<f64>,
    /// Per-bucket sample standard deviation (Bessel-corrected, `ddof = 1`).
    /// `NaN` when `count < min_obs` (incl. `count < 2`).
    pub stds: Vec<f64>,
    /// Per-bucket observation count. `0` for empty buckets.
    pub counts: Vec<u64>,
    /// Per-bucket one-sample t-statistic against zero:
    /// `mean / (std / sqrt(count))`. `NaN` when `count < min_obs`,
    /// `count < 2`, or `std == 0` (zero-variance bucket).
    pub t_stats: Vec<f64>,
    /// Per-bucket inter-quartile range (Q3 − Q1) computed via linear-interpolation
    /// quantiles over the sorted bucket values. `NaN` when `count < min_obs` or
    /// `count < 2`.
    pub iqrs: Vec<f64>,
}

/// Compute per-bucket mean / std / count / t-stat / IQR for the supplied
/// parallel `(values, bucket_keys)` pair.
///
/// `bucket_keys[i]` must be in `0..num_buckets`; an out-of-range key triggers a
/// `debug_assert` (under `cfg(debug_assertions)` and `cfg(test)`) and is
/// silently skipped under release builds. Callers (the SEAS-01..SEAS-06 scan
/// bodies) derive the key via `chrono` — by construction the key is in range.
///
/// Sequential Welford accumulation per bucket (Pitfall 4 — deterministic
/// summation order). Time complexity: `O(n + Σ n_b log n_b)` for the
/// IQR-via-sort phase, where `n_b` is bucket-b's observation count.
///
/// # Panics
/// Panics via `debug_assert!` when `num_buckets < 1` or when `values.len()` and
/// `bucket_keys.len()` mismatch.
#[allow(
    clippy::cast_precision_loss,
    reason = "count_u is bounded by values.len(); realistic OHLCV slices fit in f64's 52-bit mantissa"
)]
pub fn bucket_stats(
    values: &[f64],
    bucket_keys: &[usize],
    num_buckets: usize,
    min_obs: usize,
) -> BucketResult {
    debug_assert!(num_buckets >= 1, "bucket_stats: num_buckets must be >= 1");
    debug_assert_eq!(
        values.len(),
        bucket_keys.len(),
        "bucket_stats: values.len() must equal bucket_keys.len()"
    );

    // Per-bucket Welford running-moments accumulators.
    let mut count = vec![0_u64; num_buckets];
    let mut mean = vec![0.0_f64; num_buckets];
    // M2 = sum of squared deviations from mean (Welford's running variance numerator).
    let mut m2 = vec![0.0_f64; num_buckets];
    // Per-bucket value vectors, used for the IQR pass.
    let mut per_bucket: Vec<Vec<f64>> = (0..num_buckets).map(|_| Vec::new()).collect();

    for (i, &v) in values.iter().enumerate() {
        let k = bucket_keys[i];
        debug_assert!(
            k < num_buckets,
            "bucket_stats: bucket_keys[{i}] = {k} out of range 0..{num_buckets}"
        );
        if k >= num_buckets {
            // Silently skip out-of-range keys under release builds.
            continue;
        }
        // Welford increment — sequential per bucket so the summation order is
        // deterministic across runs (Pitfall 4).
        count[k] += 1;
        let n_f = count[k] as f64;
        let delta = v - mean[k];
        mean[k] += delta / n_f;
        let delta2 = v - mean[k];
        m2[k] += delta * delta2;
        per_bucket[k].push(v);
    }

    let mut means = Vec::with_capacity(num_buckets);
    let mut stds = Vec::with_capacity(num_buckets);
    let mut counts = Vec::with_capacity(num_buckets);
    let mut t_stats = Vec::with_capacity(num_buckets);
    let mut iqrs = Vec::with_capacity(num_buckets);

    for k in 0..num_buckets {
        let c = count[k];
        counts.push(c);
        let c_us = c as usize;
        // Sparse buckets — count < max(min_obs, 2). The min_obs == 0 path still
        // returns NaN-stats for count < 2 (Bessel-corrected std needs n >= 2).
        let threshold = min_obs.max(2);
        if c_us < threshold {
            means.push(f64::NAN);
            stds.push(f64::NAN);
            t_stats.push(f64::NAN);
            iqrs.push(f64::NAN);
            continue;
        }
        means.push(mean[k]);
        let c_f = c as f64;
        // ddof = 1 (Bessel-corrected sample std). c >= 2 guaranteed above.
        let var = m2[k] / (c_f - 1.0);
        let s = var.sqrt();
        stds.push(s);
        // t = mean / (std / sqrt(n)). When std == 0 (degenerate constant
        // bucket) emit NaN — the bucket signal is undefined (mean is exact but
        // there's no variance to normalise against).
        if s == 0.0 {
            t_stats.push(f64::NAN);
        } else {
            let se = s / c_f.sqrt();
            t_stats.push(mean[k] / se);
        }
        iqrs.push(compute_iqr(&mut per_bucket[k]));
    }

    BucketResult {
        means,
        stds,
        counts,
        t_stats,
        iqrs,
    }
}

/// Compute the inter-quartile range (Q3 − Q1) for the supplied bucket values
/// using a linear-interpolation quantile estimator (Numpy / pandas default —
/// `method="linear"`, equivalent to `scipy.stats.iqr` default settings).
///
/// Mutates the input vector by sorting it in place — the caller's bucket vector
/// is consumed by this single call so the cost is paid once.
///
/// Returns `NaN` when `values.len() < 2`.
#[allow(
    clippy::cast_precision_loss,
    reason = "values.len() is bounded by the input bar count; realistic OHLCV slices fit in f64's 52-bit mantissa"
)]
fn compute_iqr(values: &mut [f64]) -> f64 {
    let n = values.len();
    if n < 2 {
        return f64::NAN;
    }
    // `sort_by` with `total_cmp` is the IEEE-754-aware ordering — handles NaN /
    // ±0.0 deterministically. The values here come from log-return kernels so
    // NaN entries are not expected in normal flow but defensive ordering keeps
    // determinism.
    values.sort_by(|a, b| a.total_cmp(b));
    let q1 = linear_quantile(values, 0.25);
    let q3 = linear_quantile(values, 0.75);
    q3 - q1
}

/// Linear-interpolation quantile over a pre-sorted slice (numpy default).
///
/// `p` must be in `[0, 1]`. `values` must be non-empty and sorted ascending.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "n is bounded by realistic bar counts; the floor/ceil indices come from the linear-interpolation quantile algorithm and are non-negative integers within slice bounds"
)]
fn linear_quantile(values: &[f64], p: f64) -> f64 {
    let n = values.len();
    debug_assert!(n >= 1, "linear_quantile: values must be non-empty");
    if n == 1 {
        return values[0];
    }
    let h = (n - 1) as f64 * p;
    let lo = h.floor() as usize;
    let hi = h.ceil() as usize;
    if lo == hi {
        return values[lo];
    }
    let frac = h - h.floor();
    values[lo] * (1.0 - frac) + values[hi] * frac
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::cast_lossless)]
mod tests {
    use super::*;

    const TOL: f64 = 1e-12;

    fn approx_eq(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    /// Plan 04-09 Task 1 — hand-derived 3-bucket synthetic input. Bucket 0 has
    /// `[1.0, 2.0, 3.0]`, bucket 1 has `[10.0, 20.0, 30.0]`, bucket 2 is empty.
    /// means: `[2.0, 20.0, NaN]`, counts: `[3, 3, 0]`. ddof=1 stds:
    /// bucket 0 = `sqrt(((1-2)^2 + (2-2)^2 + (3-2)^2)/(3-1))` = `1.0`;
    /// bucket 1 = `10.0` (same shape scaled by 10).
    #[test]
    fn bucket_stats_three_buckets_hand_derived() {
        let values = vec![1.0, 2.0, 3.0, 10.0, 20.0, 30.0];
        let keys = vec![0_usize, 0, 0, 1, 1, 1];
        let r = bucket_stats(&values, &keys, 3, 0);
        // Counts
        assert_eq!(r.counts, vec![3, 3, 0]);
        // Means
        assert!(approx_eq(r.means[0], 2.0, TOL), "mean[0]={}", r.means[0]);
        assert!(approx_eq(r.means[1], 20.0, TOL), "mean[1]={}", r.means[1]);
        assert!(r.means[2].is_nan(), "mean[2] must be NaN (empty bucket)");
        // Stds (Bessel-corrected)
        assert!(approx_eq(r.stds[0], 1.0, TOL), "std[0]={}", r.stds[0]);
        assert!(approx_eq(r.stds[1], 10.0, TOL), "std[1]={}", r.stds[1]);
        assert!(r.stds[2].is_nan(), "std[2] must be NaN");
        // t-stats: bucket 0 = 2 / (1/sqrt(3)) = 2*sqrt(3) ≈ 3.464...
        let expected_t0 = 2.0_f64 * (3.0_f64).sqrt();
        let expected_t1 = 20.0_f64 / (10.0 / (3.0_f64).sqrt());
        assert!(
            approx_eq(r.t_stats[0], expected_t0, TOL),
            "t[0]={}",
            r.t_stats[0]
        );
        assert!(
            approx_eq(r.t_stats[1], expected_t1, TOL),
            "t[1]={}",
            r.t_stats[1]
        );
        assert!(r.t_stats[2].is_nan(), "t[2] must be NaN");
        // IQRs: bucket 0 sorted = [1, 2, 3]; Q1 = 1.5, Q3 = 2.5 -> IQR = 1.0.
        // Bucket 1 = same shape * 10 -> IQR = 10.
        assert!(approx_eq(r.iqrs[0], 1.0, TOL), "iqr[0]={}", r.iqrs[0]);
        assert!(approx_eq(r.iqrs[1], 10.0, TOL), "iqr[1]={}", r.iqrs[1]);
        assert!(r.iqrs[2].is_nan(), "iqr[2] must be NaN");
    }

    /// Plan 04-09 Task 1 — sparse-bucket behaviour. With `min_obs = 5`, a
    /// bucket of count 3 emits NaN for mean / std / t-stat / IQR (count stays
    /// exact).
    #[test]
    fn bucket_stats_min_obs_marks_sparse_buckets_nan() {
        let values = vec![1.0, 2.0, 3.0];
        let keys = vec![0, 0, 0];
        let r = bucket_stats(&values, &keys, 1, 5);
        assert_eq!(r.counts, vec![3]);
        assert!(r.means[0].is_nan(), "mean must be NaN for sparse bucket");
        assert!(r.stds[0].is_nan(), "std must be NaN for sparse bucket");
        assert!(r.t_stats[0].is_nan(), "t_stat must be NaN for sparse bucket");
        assert!(r.iqrs[0].is_nan(), "iqr must be NaN for sparse bucket");
    }

    /// Zero-variance bucket: mean is exact but std == 0 -> t_stat is NaN by
    /// convention (the bucket signal is undefined when no variance exists).
    #[test]
    fn bucket_stats_zero_variance_bucket_yields_nan_t_stat() {
        let values = vec![5.0, 5.0, 5.0];
        let keys = vec![0, 0, 0];
        let r = bucket_stats(&values, &keys, 1, 0);
        assert!(approx_eq(r.means[0], 5.0, TOL));
        assert!(approx_eq(r.stds[0], 0.0, TOL));
        assert!(
            r.t_stats[0].is_nan(),
            "zero-variance bucket -> t_stat must be NaN"
        );
    }

    /// A bucket with `mean == 0` and non-zero std produces `t_stat == 0`
    /// (mathematically: 0 / se = 0).
    #[test]
    fn bucket_stats_zero_mean_bucket_yields_zero_t_stat() {
        let values = vec![-1.0, 0.0, 1.0];
        let keys = vec![0, 0, 0];
        let r = bucket_stats(&values, &keys, 1, 0);
        assert!(approx_eq(r.means[0], 0.0, TOL));
        // std = sqrt(2/2) = 1
        assert!(approx_eq(r.stds[0], 1.0, TOL));
        assert!(approx_eq(r.t_stats[0], 0.0, TOL), "t[0]={}", r.t_stats[0]);
    }

    /// Empty input -> every bucket reports count 0 and NaN stats.
    #[test]
    fn bucket_stats_empty_input() {
        let r = bucket_stats(&[], &[], 4, 0);
        assert_eq!(r.counts, vec![0, 0, 0, 0]);
        for k in 0..4 {
            assert!(r.means[k].is_nan());
            assert!(r.stds[k].is_nan());
            assert!(r.t_stats[k].is_nan());
            assert!(r.iqrs[k].is_nan());
        }
    }

    /// Length invariants — every output vector is exactly `num_buckets`.
    #[test]
    fn bucket_stats_lengths_match_num_buckets() {
        let values = vec![1.0_f64; 100];
        let keys: Vec<usize> = (0..100).map(|i| i % 7).collect();
        let r = bucket_stats(&values, &keys, 7, 0);
        assert_eq!(r.means.len(), 7);
        assert_eq!(r.stds.len(), 7);
        assert_eq!(r.counts.len(), 7);
        assert_eq!(r.t_stats.len(), 7);
        assert_eq!(r.iqrs.len(), 7);
    }

    /// linear_quantile basic — symmetric series, Q1 / Q3 hand-derived.
    #[test]
    fn linear_quantile_basic() {
        let v = [1.0_f64, 2.0, 3.0, 4.0, 5.0];
        assert!(approx_eq(linear_quantile(&v, 0.0), 1.0, TOL));
        assert!(approx_eq(linear_quantile(&v, 0.25), 2.0, TOL));
        assert!(approx_eq(linear_quantile(&v, 0.5), 3.0, TOL));
        assert!(approx_eq(linear_quantile(&v, 0.75), 4.0, TOL));
        assert!(approx_eq(linear_quantile(&v, 1.0), 5.0, TOL));
    }

    /// linear_quantile linear-interp branch — fractional index.
    #[test]
    fn linear_quantile_linear_interpolation() {
        // 4-element series: Q1 index = (4-1)*0.25 = 0.75 -> interp 0.25*v[0] + 0.75*v[1]
        let v = [1.0_f64, 2.0, 3.0, 4.0];
        let q1 = linear_quantile(&v, 0.25);
        // 0.25*1 + 0.75*2 = 0.25 + 1.5 = 1.75
        assert!(approx_eq(q1, 1.75, TOL), "q1={q1}");
        let q3 = linear_quantile(&v, 0.75);
        // 0.75 index = (4-1)*0.75 = 2.25 -> 0.75*v[2] + 0.25*v[3] = 0.75*3 + 0.25*4 = 2.25+1.0 = 3.25
        assert!(approx_eq(q3, 3.25, TOL), "q3={q3}");
    }

    #[test]
    #[should_panic(expected = "num_buckets must be >= 1")]
    fn bucket_stats_zero_buckets_panics_under_debug() {
        let _ = bucket_stats(&[], &[], 0, 0);
    }
}
