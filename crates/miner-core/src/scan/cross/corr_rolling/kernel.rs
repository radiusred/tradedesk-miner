//! Rolling Pearson + Spearman correlation kernels (CROSS-02).
//!
//! Both kernels iterate aligned return slices in `O(n * window)` per-window
//! sweeps (naive but transparent — RESEARCH.md §1.3 picks this over
//! "optimised" Welford-style updates because correctness beats cycles for
//! the discovery layer; benchmarks in Plan 04-11 will measure).
//!
//! - [`rolling_pearson`] — per-window Pearson r over `(a, b)` slices.
//! - [`rolling_spearman`] — same shape, but ranks both vectors per window
//!   (scipy.stats.spearmanr default `method = "average"` tie-correction) and
//!   then runs the Pearson kernel on the ranks.
//! - [`rank_with_ties`] — average-rank tie convention. Returns a `Vec<f64>`
//!   the size of the input; rank values land in `[1.0, n]`. Tied groups are
//!   assigned the mean of their would-be sequential ranks.
//!
//! Zero-variance windows produce NaN from the Pearson kernel; the calling
//! `mod.rs::run` detects NaN and surfaces `ScanError::Kernel(_)` so the wire
//! form never carries an undefined correlation.

/// Per-window Pearson correlation coefficient over two aligned return slices.
///
/// For each window position `i in 0..=n-window` computes:
///
/// ```text
///   r_i = Σ((a_t - mean_a) * (b_t - mean_b)) /
///         sqrt(Σ(a_t - mean_a)^2 * Σ(b_t - mean_b)^2)
/// ```
///
/// over `t in [i, i+window)`. Returns a `Vec<f64>` of length `n - window + 1`
/// (empty when `n < window`).
///
/// Numerical convention: two-pass mean + covariance (sum then divide); the
/// scan layer is correctness-first and the windows are small (3..=512) so
/// the extra pass cost is irrelevant.
///
/// Zero-variance branch: when EITHER leg has zero sample variance over a
/// window the denominator is 0 and the result is `f64::NAN`. The caller
/// (`corr_rolling::run`) detects and converts to `ScanError::Kernel`.
#[inline]
#[must_use]
pub(super) fn rolling_pearson(a: &[f64], b: &[f64], window: usize) -> Vec<f64> {
    debug_assert_eq!(a.len(), b.len(), "rolling_pearson: a.len() must equal b.len()");
    debug_assert!(window >= 2, "rolling_pearson: window must be >= 2");
    let n = a.len();
    if n < window {
        return Vec::new();
    }
    let count = n - window + 1;
    let mut out = Vec::with_capacity(count);
    // window is bounded by aligned_n; realistic values << 2^52 so the
    // cast is lossless in practice. Documented per CLAUDE.md cast policy.
    #[allow(clippy::cast_precision_loss, reason = "window <= aligned_n << 2^52")]
    let w_f = window as f64;
    for i in 0..count {
        // Pass 1 — means.
        let mut sum_a = 0.0_f64;
        let mut sum_b = 0.0_f64;
        for t in i..i + window {
            sum_a += a[t];
            sum_b += b[t];
        }
        let mean_a = sum_a / w_f;
        let mean_b = sum_b / w_f;
        // Pass 2 — covariance + variances.
        let mut cov = 0.0_f64;
        let mut var_a = 0.0_f64;
        let mut var_b = 0.0_f64;
        for t in i..i + window {
            let da = a[t] - mean_a;
            let db = b[t] - mean_b;
            cov += da * db;
            var_a += da * da;
            var_b += db * db;
        }
        let denom = (var_a * var_b).sqrt();
        let r = if denom == 0.0 {
            f64::NAN
        } else {
            cov / denom
        };
        out.push(r);
    }
    out
}

/// Per-window Spearman correlation coefficient. For each window position
/// rank both legs via [`rank_with_ties`] (scipy.stats.spearmanr default
/// `method = "average"` tie correction) then runs the Pearson kernel on
/// the ranks.
///
/// Returns a `Vec<f64>` of length `n - window + 1` (empty when `n < window`).
#[inline]
#[must_use]
pub(super) fn rolling_spearman(a: &[f64], b: &[f64], window: usize) -> Vec<f64> {
    debug_assert_eq!(a.len(), b.len(), "rolling_spearman: a.len() must equal b.len()");
    debug_assert!(window >= 2, "rolling_spearman: window must be >= 2");
    let n = a.len();
    if n < window {
        return Vec::new();
    }
    let count = n - window + 1;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let ranks_a = rank_with_ties(&a[i..i + window]);
        let ranks_b = rank_with_ties(&b[i..i + window]);
        // One-window Pearson on the ranks. Inline the small computation
        // instead of allocating intermediate slices.
        #[allow(clippy::cast_precision_loss, reason = "window <= aligned_n << 2^52")]
        let w_f = window as f64;
        let mut sum_a = 0.0_f64;
        let mut sum_b = 0.0_f64;
        for t in 0..window {
            sum_a += ranks_a[t];
            sum_b += ranks_b[t];
        }
        let mean_a = sum_a / w_f;
        let mean_b = sum_b / w_f;
        let mut cov = 0.0_f64;
        let mut var_a = 0.0_f64;
        let mut var_b = 0.0_f64;
        for t in 0..window {
            let da = ranks_a[t] - mean_a;
            let db = ranks_b[t] - mean_b;
            cov += da * db;
            var_a += da * da;
            var_b += db * db;
        }
        let denom = (var_a * var_b).sqrt();
        let r = if denom == 0.0 {
            f64::NAN
        } else {
            cov / denom
        };
        out.push(r);
    }
    out
}

/// Assign average ranks to a slice, scipy.stats.spearmanr's default
/// `method = "average"` convention. Tied groups receive the mean of their
/// would-be sequential ranks (1-indexed).
///
/// Algorithm:
/// 1. Build index permutation sorted by value (stable, ascending).
/// 2. Walk the sorted index; identify tied runs by equal `values[idx]`.
/// 3. For each tied run spanning sequential ranks `[r0, r0+1, ..., r0+k-1]`
///    (1-indexed), assign each element the average rank `(2*r0 + k - 1) / 2`.
///
/// Returns a `Vec<f64>` of length `values.len()` (empty input -> empty Vec).
///
/// Tolerance note: ties on f64 use bitwise equality on the f64 value (NOT
/// `==` floating-point equality) — `total_cmp` for the sort and bit-pattern
/// equality for the tie test. This mirrors scipy's behaviour which treats
/// any pair of equal floats as a tie.
#[inline]
fn rank_with_ties(values: &[f64]) -> Vec<f64> {
    let n = values.len();
    if n == 0 {
        return Vec::new();
    }
    // Index permutation sorted by value (ascending, stable on the index for
    // equal values — required for the tie-detection walk below).
    let mut indices: Vec<usize> = (0..n).collect();
    indices.sort_by(|&i, &j| values[i].total_cmp(&values[j]));
    let mut ranks = vec![0.0_f64; n];
    let mut i = 0;
    while i < n {
        // Identify the tied run [i, j) where values[indices[i..j]] are
        // bitwise-equal.
        let pivot_bits = values[indices[i]].to_bits();
        let mut j = i + 1;
        while j < n && values[indices[j]].to_bits() == pivot_bits {
            j += 1;
        }
        // Sequential 1-indexed ranks for this run are [i+1, i+2, ..., j].
        // Their mean is (i + 1 + j) / 2. n is bounded by the window size
        // (max 2^52 in practice), so the cast is lossless.
        #[allow(
            clippy::cast_precision_loss,
            reason = "i + 1 + j <= 2*window <= aligned_n << 2^52"
        )]
        let avg_rank = ((i + 1 + j) as f64) / 2.0;
        for k in i..j {
            ranks[indices[k]] = avg_rank;
        }
        i = j;
    }
    ranks
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    const TOL: f64 = 1e-12;

    fn approx_eq(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    /// Hand-derived: window=3 over the sequences
    ///   a = [1, 2, 3, 4, 5]
    ///   b = [2, 4, 6, 8, 10]  (b = 2a)
    /// Each window has perfect linear relation -> Pearson r = 1.0 for all
    /// three windows.
    #[test]
    fn rolling_pearson_perfect_linear() {
        let a = [1.0, 2.0, 3.0, 4.0, 5.0];
        let b = [2.0, 4.0, 6.0, 8.0, 10.0];
        let r = rolling_pearson(&a, &b, 3);
        assert_eq!(r.len(), 3, "n=5, window=3 -> 3 outputs");
        for (i, v) in r.iter().enumerate() {
            assert!(approx_eq(*v, 1.0, TOL), "window[{i}]: r={v}");
        }
    }

    /// Hand-derived: window=3 over inverted relation -> Pearson r = -1.0.
    #[test]
    fn rolling_pearson_perfect_negative() {
        let a = [1.0, 2.0, 3.0, 4.0, 5.0];
        let b = [10.0, 8.0, 6.0, 4.0, 2.0]; // b = -2a + 12
        let r = rolling_pearson(&a, &b, 3);
        assert_eq!(r.len(), 3);
        for v in &r {
            assert!(approx_eq(*v, -1.0, TOL), "expected -1.0; got {v}");
        }
    }

    #[test]
    fn rolling_pearson_short_input_returns_empty() {
        let a = [1.0, 2.0];
        let b = [3.0, 4.0];
        let r = rolling_pearson(&a, &b, 3);
        assert!(r.is_empty(), "n < window -> empty output");
    }

    #[test]
    fn rolling_pearson_zero_variance_window_produces_nan() {
        // First window has constant `a` -> var_a == 0 -> denom 0 -> NaN.
        let a = [1.0, 1.0, 1.0, 2.0, 3.0];
        let b = [1.0, 2.0, 3.0, 4.0, 5.0];
        let r = rolling_pearson(&a, &b, 3);
        assert_eq!(r.len(), 3);
        assert!(r[0].is_nan(), "zero-variance window -> NaN");
        // Later windows have non-zero variance -> finite values.
        assert!(r[2].is_finite());
    }

    /// Hand-derived: window=4 over a = [1, 2, 3, 4], b = [4, 3, 2, 1].
    /// mean_a = 2.5, mean_b = 2.5; deviations da = [-1.5, -0.5, 0.5, 1.5],
    /// db = [1.5, 0.5, -0.5, -1.5]; cov = -5; var_a = var_b = 5; r = -1.
    #[test]
    fn rolling_pearson_hand_derived_window4() {
        let a = [1.0, 2.0, 3.0, 4.0];
        let b = [4.0, 3.0, 2.0, 1.0];
        let r = rolling_pearson(&a, &b, 4);
        assert_eq!(r.len(), 1);
        assert!(approx_eq(r[0], -1.0, TOL), "got {}", r[0]);
    }

    #[test]
    fn rank_with_ties_no_ties() {
        let v = [3.0, 1.0, 4.0, 1.5, 9.0];
        let r = rank_with_ties(&v);
        // Sorted order: 1.0(idx1), 1.5(idx3), 3.0(idx0), 4.0(idx2), 9.0(idx4)
        // Ranks: idx1=1, idx3=2, idx0=3, idx2=4, idx4=5
        assert_eq!(r, vec![3.0, 1.0, 4.0, 2.0, 5.0]);
    }

    #[test]
    fn rank_with_ties_with_ties_average() {
        // Values: [10, 20, 20, 30] — the two 20s are tied at sequential
        // ranks 2 and 3 -> both get rank (2 + 3)/2 = 2.5.
        let v = [10.0, 20.0, 20.0, 30.0];
        let r = rank_with_ties(&v);
        assert_eq!(r, vec![1.0, 2.5, 2.5, 4.0]);
    }

    #[test]
    fn rank_with_ties_triple_tie() {
        // Values: [1, 5, 5, 5, 10] — three 5s at ranks 2, 3, 4 -> all get 3.0.
        let v = [1.0, 5.0, 5.0, 5.0, 10.0];
        let r = rank_with_ties(&v);
        assert_eq!(r, vec![1.0, 3.0, 3.0, 3.0, 5.0]);
    }

    #[test]
    fn rank_with_ties_empty_input() {
        let r = rank_with_ties(&[]);
        assert!(r.is_empty());
    }

    /// Hand-derived: monotone series in both legs -> Spearman r = 1.0 for
    /// every window. Verifies the rank pipeline is wired correctly.
    #[test]
    fn rolling_spearman_perfect_monotone() {
        let a = [1.0, 2.0, 3.0, 4.0, 5.0];
        let b = [10.0, 20.0, 30.0, 40.0, 50.0];
        let r = rolling_spearman(&a, &b, 3);
        assert_eq!(r.len(), 3);
        for v in &r {
            assert!(approx_eq(*v, 1.0, TOL));
        }
    }

    /// Spearman with ties: leg b has a tied pair inside the window. Verify
    /// the kernel returns a finite (non-NaN) value — the rank machinery
    /// must replace exact tied values with their average rank so the
    /// resulting Pearson on ranks has non-zero variance.
    #[test]
    fn rolling_spearman_handles_ties() {
        // a strictly increasing; b has a tied pair within the first window.
        let a = [1.0, 2.0, 3.0, 4.0, 5.0];
        let b = [10.0, 20.0, 20.0, 40.0, 50.0];
        let r = rolling_spearman(&a, &b, 3);
        assert_eq!(r.len(), 3);
        // Each window has at least one non-tied pair so variance > 0 -> finite.
        for v in &r {
            assert!(v.is_finite(), "Spearman with ties must be finite; got {v}");
        }
    }

    #[test]
    fn rolling_spearman_inverted_monotone() {
        // a ascending, b descending -> Spearman = -1.
        let a = [1.0, 2.0, 3.0, 4.0, 5.0];
        let b = [50.0, 40.0, 30.0, 20.0, 10.0];
        let r = rolling_spearman(&a, &b, 3);
        for v in &r {
            assert!(approx_eq(*v, -1.0, TOL));
        }
    }
}
