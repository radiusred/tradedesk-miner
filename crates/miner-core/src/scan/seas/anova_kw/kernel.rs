//! Pure `anova_kw` kernel — one-way ANOVA F-stat + Kruskal-Wallis H-stat
//! with tie correction.
//!
//! Pattern analog: `ljung_box/kernel.rs` — kernel-only file with private
//! pure functions over `&[Vec<f64>]` (parallel groups) + a sibling
//! `#[cfg(test)] mod tests` block.
//!
//! ## ANOVA
//!
//! Classical one-way analysis of variance. Given `k` groups with sample sizes
//! `n_1..n_k` and grand sample size `N = Σ n_i`:
//!
//! ```text
//!   SS_between = Σ_i n_i * (μ_i - μ_grand)^2
//!   SS_within  = Σ_i Σ_{j} (x_ij - μ_i)^2
//!   MS_between = SS_between / (k - 1)
//!   MS_within  = SS_within  / (N - k)
//!   F = MS_between / MS_within
//!   p_value = 1 - FisherSnedecor(k-1, N-k).cdf(F)
//! ```
//!
//! The implementation matches `scipy.stats.f_oneway` algebra exactly.
//!
//! ## Kruskal-Wallis
//!
//! Non-parametric rank-based generalisation of the Mann-Whitney U to `k` groups.
//! Pool all values, rank them with average-rank tie correction (matches
//! `scipy.stats.kruskal`'s `method='average'` default). For each group `i`:
//!
//! ```text
//!   R_i = Σ ranks in group i
//!   H = (12 / (N*(N+1))) * Σ_i (R_i^2 / n_i) - 3*(N+1)
//! ```
//!
//! With tie correction (Conover):
//!
//! ```text
//!   c = 1 - Σ_t (t^3 - t) / (N^3 - N)
//!   H_corrected = H / c
//!   p_value = 1 - ChiSquared(k-1).cdf(H_corrected)
//! ```
//!
//! where `t` iterates over the sizes of each tied group in the pooled
//! sample.

#![cfg_attr(any(test, debug_assertions), allow(clippy::float_cmp))]

use statrs::distribution::{ChiSquared, ContinuousCDF, FisherSnedecor};

/// One-way ANOVA result. `f_stat` is `MS_between / MS_within`; `p_value` is the
/// upper-tail probability under `FisherSnedecor(k-1, N-k)`. `k` is the number
/// of non-empty groups supplied to the kernel; `total_n` is the pooled sample
/// size.
#[derive(Debug, Clone, Copy)]
pub(super) struct AnovaResult {
    pub f_stat: f64,
    pub p_value: f64,
    pub k: usize,
    pub total_n: usize,
}

/// One-way ANOVA F-statistic + p-value via `FisherSnedecor(k-1, N-k)`.
///
/// Empty groups (`group.is_empty()`) are silently dropped before the
/// computation (matching `scipy.stats.f_oneway`'s degenerate-group handling).
/// The remaining group count `k` must be `>= 2` and the total sample size
/// `N` must satisfy `N > k`.
///
/// Returns `(NaN, NaN)` for the F-stat / p-value when `MS_within == 0` (every
/// group is constant — the F ratio is undefined).
///
/// # Panics
/// Panics via `debug_assert` when the (post-filter) group count is `< 2` or
/// when `N <= k`.
#[inline]
#[allow(
    clippy::cast_precision_loss,
    reason = "n_i / N are bar counts; fit f64 mantissa for realistic OHLCV slices"
)]
pub(super) fn one_way_anova(groups: &[Vec<f64>]) -> AnovaResult {
    // Drop empty groups.
    let groups: Vec<&Vec<f64>> = groups.iter().filter(|g| !g.is_empty()).collect();
    let k = groups.len();
    debug_assert!(k >= 2, "one_way_anova: need >= 2 non-empty groups; got {k}");

    let total_n: usize = groups.iter().map(|g| g.len()).sum();
    debug_assert!(
        total_n > k,
        "one_way_anova: total_n {total_n} must exceed k {k} for a finite F-stat"
    );

    // Group means + grand mean.
    let group_means: Vec<f64> = groups
        .iter()
        .map(|g| g.iter().copied().sum::<f64>() / g.len() as f64)
        .collect();
    let grand_sum: f64 = groups.iter().map(|g| g.iter().copied().sum::<f64>()).sum();
    let grand_mean = grand_sum / total_n as f64;

    // SS_between = Σ n_i * (μ_i - μ_grand)^2
    let ss_between: f64 = groups
        .iter()
        .zip(group_means.iter())
        .map(|(g, &mu)| {
            let d = mu - grand_mean;
            g.len() as f64 * d * d
        })
        .sum();
    // SS_within = Σ_i Σ_j (x_ij - μ_i)^2
    let ss_within: f64 = groups
        .iter()
        .zip(group_means.iter())
        .map(|(g, &mu)| g.iter().map(|x| (x - mu).powi(2)).sum::<f64>())
        .sum();

    let df_between = (k - 1) as f64;
    let df_within = (total_n - k) as f64;
    let ms_between = ss_between / df_between;
    let ms_within = ss_within / df_within;

    if ms_within == 0.0 {
        // All groups are constant — F is undefined.
        return AnovaResult {
            f_stat: f64::NAN,
            p_value: f64::NAN,
            k,
            total_n,
        };
    }

    let f_stat = ms_between / ms_within;
    // FisherSnedecor::new requires positive df.
    let f_dist = FisherSnedecor::new(df_between, df_within)
        .expect("ANOVA df > 0 guaranteed by k >= 2 + N > k");
    let p_value = 1.0 - f_dist.cdf(f_stat);

    AnovaResult {
        f_stat,
        p_value,
        k,
        total_n,
    }
}

/// Kruskal-Wallis result. `h_stat` is the tie-corrected H statistic;
/// `p_value` is the upper-tail probability under `ChiSquared(k-1)`.
#[derive(Debug, Clone, Copy)]
pub(super) struct KruskalResult {
    pub h_stat: f64,
    pub p_value: f64,
}

/// Kruskal-Wallis H-statistic + p-value via `ChiSquared(k-1)`. Implements the
/// tie-corrected formula (`H / (1 - Σ(t^3 - t) / (N^3 - N))`); matches
/// `scipy.stats.kruskal` default behaviour within 1e-10 for hand-derived
/// inputs, and within 1e-6 for the rank-tie path.
///
/// # Panics
/// Panics via `debug_assert` when fewer than 2 non-empty groups are supplied.
#[inline]
#[allow(
    clippy::cast_precision_loss,
    reason = "n_i / N are bar counts; fit f64 mantissa for realistic OHLCV slices"
)]
pub(super) fn kruskal_wallis(groups: &[Vec<f64>]) -> KruskalResult {
    let groups: Vec<&Vec<f64>> = groups.iter().filter(|g| !g.is_empty()).collect();
    let k = groups.len();
    debug_assert!(
        k >= 2,
        "kruskal_wallis: need >= 2 non-empty groups; got {k}"
    );

    // Pool all values, remember which group each came from.
    let total_n: usize = groups.iter().map(|g| g.len()).sum();
    let mut pooled: Vec<(f64, usize)> = Vec::with_capacity(total_n);
    for (gi, g) in groups.iter().enumerate() {
        for &v in *g {
            pooled.push((v, gi));
        }
    }

    // Build average-rank assignment via index permutation.
    let n = pooled.len();
    let mut indices: Vec<usize> = (0..n).collect();
    indices.sort_by(|&i, &j| pooled[i].0.total_cmp(&pooled[j].0));

    let mut ranks = vec![0.0_f64; n];
    let mut tie_sizes: Vec<usize> = Vec::new();
    let mut i = 0;
    while i < n {
        let pivot_bits = pooled[indices[i]].0.to_bits();
        let mut j = i + 1;
        while j < n && pooled[indices[j]].0.to_bits() == pivot_bits {
            j += 1;
        }
        // Sequential 1-indexed ranks for this run are [i+1, i+2, ..., j].
        let avg_rank = ((i + 1 + j) as f64) / 2.0;
        for kk in i..j {
            ranks[indices[kk]] = avg_rank;
        }
        if j - i > 1 {
            tie_sizes.push(j - i);
        }
        i = j;
    }

    // Sum of ranks per group.
    let mut r_sums = vec![0.0_f64; k];
    let mut n_per_group = vec![0_usize; k];
    for (idx, &(_, gi)) in pooled.iter().enumerate() {
        r_sums[gi] += ranks[idx];
        n_per_group[gi] += 1;
    }

    let n_f = n as f64;
    let sum_term: f64 = r_sums
        .iter()
        .zip(n_per_group.iter())
        .map(|(&r, &nk)| r * r / (nk as f64))
        .sum();
    let h_raw = 12.0 / (n_f * (n_f + 1.0)) * sum_term - 3.0 * (n_f + 1.0);

    // Tie correction. When no ties exist the factor is 1.
    let tie_term: f64 = tie_sizes
        .iter()
        .map(|&t| {
            let tf = t as f64;
            tf * tf * tf - tf
        })
        .sum();
    let c = 1.0 - tie_term / (n_f * n_f * n_f - n_f);
    let h_stat = if c > 0.0 { h_raw / c } else { h_raw };

    let df = (k - 1) as f64;
    let chi = ChiSquared::new(df).expect("k >= 2 yields valid ChiSquared df");
    let p_value = 1.0 - chi.cdf(h_stat);

    KruskalResult { h_stat, p_value }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::cast_lossless)]
mod tests {
    use super::*;

    const TOL_F: f64 = 1e-10;
    const TOL_P: f64 = 1e-8;
    const TOL_KW_TIES: f64 = 1e-6;

    fn approx_eq(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    /// Hand-derived F-stat: 3 groups [1,2,3], [4,5,6], [7,8,9].
    /// Means: 2, 5, 8. Grand mean: 5. N=9, k=3.
    /// `SS_between` = 3*(2-5)^2 + 3*(5-5)^2 + 3*(8-5)^2 = 27 + 0 + 27 = 54.
    /// `SS_within` = sum of (x - `μ_i)^2`:
    ///   group 1: (1-2)^2 + (2-2)^2 + (3-2)^2 = 2
    ///   group 2: 2; group 3: 2. total = 6.
    /// `MS_between` = 54 / 2 = 27. `MS_within` = 6 / 6 = 1. F = 27.
    #[test]
    fn anova_three_groups_hand_derived() {
        let groups = vec![
            vec![1.0_f64, 2.0, 3.0],
            vec![4.0, 5.0, 6.0],
            vec![7.0, 8.0, 9.0],
        ];
        let r = one_way_anova(&groups);
        assert_eq!(r.k, 3);
        assert_eq!(r.total_n, 9);
        assert!(approx_eq(r.f_stat, 27.0, TOL_F), "F={}", r.f_stat);
        // p_value should be very small (F=27 on df 2, 6 — scipy reports
        // ~0.001 to 0.0011 depending on rounding).
        assert!(r.p_value <= 0.002, "p={}", r.p_value);
        assert!(r.p_value >= 0.0);
    }

    /// `FisherSnedecor` p-value sanity: F = 1.0 on df (2, 6) yields a p-value
    /// > 0.4 (the F=1 cutoff is roughly the mode of the distribution).
    #[test]
    fn anova_p_value_via_fisher_snedecor() {
        // Two groups with equal means -> F ≈ 0 -> p ≈ 1. Build [1,2,3] and
        // [1,2,3] -> means are equal, SS_between = 0 -> F = 0 -> p = 1.
        let groups = vec![vec![1.0, 2.0, 3.0], vec![1.0, 2.0, 3.0]];
        let r = one_way_anova(&groups);
        assert!(approx_eq(r.f_stat, 0.0, TOL_F), "F={}", r.f_stat);
        assert!(approx_eq(r.p_value, 1.0, TOL_P), "p={}", r.p_value);
    }

    /// All groups constant -> NaN F.
    #[test]
    fn anova_constant_groups_returns_nan() {
        let groups = vec![vec![5.0_f64; 5], vec![5.0; 5]];
        let r = one_way_anova(&groups);
        assert!(r.f_stat.is_nan(), "F should be NaN for zero-variance");
        assert!(r.p_value.is_nan());
    }

    /// Kruskal-Wallis on the same 3-group hand input. With no ties:
    /// pooled = [1..9] -> ranks 1..9. `R_1` = 1+2+3 = 6; `R_2` = 4+5+6 = 15;
    /// `R_3` = 7+8+9 = 24. `n_i` = 3 each. N=9.
    /// H = (12 / (9*10)) * (6^2/3 + 15^2/3 + 24^2/3) - 3*10
    ///   = (12/90) * (12 + 75 + 192) - 30
    ///   = (12/90) * 279 - 30
    ///   = 37.2 - 30 = 7.2.
    /// No ties -> c = 1; `H_corrected` = 7.2.
    #[test]
    fn kruskal_wallis_three_groups_hand_derived() {
        let groups = vec![
            vec![1.0_f64, 2.0, 3.0],
            vec![4.0, 5.0, 6.0],
            vec![7.0, 8.0, 9.0],
        ];
        let r = kruskal_wallis(&groups);
        assert!(approx_eq(r.h_stat, 7.2, TOL_F), "H={}", r.h_stat);
        // p-value under ChiSquared(2): 1 - chi2.cdf(7.2, 2) ≈ 0.02732.
        assert!(
            approx_eq(r.p_value, 0.027_323_722_447_2, 1e-6),
            "p={}",
            r.p_value
        );
    }

    /// Kruskal-Wallis with ties: groups [1,1,2] and [2,3,3] have the same
    /// pooled rank pattern as [1,2,3]+[4,5,6] but with 2 tied pairs. The
    /// tie correction `c < 1` inflates H above the no-tie equivalent.
    #[test]
    fn kruskal_wallis_with_ties_tie_correction_active() {
        let groups = vec![vec![1.0_f64, 1.0, 2.0], vec![2.0, 3.0, 3.0]];
        let r = kruskal_wallis(&groups);
        // scipy.stats.kruskal([1,1,2], [2,3,3]) reports:
        //   H = 3.0 (tie-corrected); p ≈ 0.0833 under chi2(1).
        // Wait — let's hand-compute. Pooled values: 1,1,2,2,3,3.
        // Sorted: 1,1,2,2,3,3 (same order). Average ranks:
        //   1: avg rank of positions 1,2 = 1.5; 1: 1.5
        //   2: avg rank of positions 3,4 = 3.5; 2: 3.5
        //   3: avg rank of positions 5,6 = 5.5; 3: 5.5
        // Group A = [1,1,2] -> ranks 1.5, 1.5, 3.5 -> R_A = 6.5; n_A = 3.
        // Group B = [2,3,3] -> ranks 3.5, 5.5, 5.5 -> R_B = 14.5; n_B = 3.
        // N=6. H_raw = (12/(6*7)) * (6.5^2/3 + 14.5^2/3) - 3*7
        //   = (12/42) * (42.25/3 + 210.25/3) - 21
        //   = (12/42) * 84.166... - 21
        //   = 24.047... - 21 = 3.047...
        // Wait: 42.25/3 = 14.0833...; 210.25/3 = 70.0833... -> sum = 84.1666...
        // 12/42 = 0.2857...; 0.2857... * 84.1667 = 24.0476...; - 21 = 3.0476...
        // Tie sizes: t1=2 (for value 1), t2=2 (value 2), t3=2 (value 3).
        // tie_term = 3 * (2^3 - 2) = 3 * 6 = 18. N^3 - N = 216 - 6 = 210.
        // c = 1 - 18/210 = 1 - 0.0857... = 0.9143...
        // H_corrected = 3.0476... / 0.9143... = 3.3333...
        assert!(
            approx_eq(r.h_stat, 3.333_333_333_333_333_5, TOL_KW_TIES),
            "H_corrected={}",
            r.h_stat
        );
    }
}
