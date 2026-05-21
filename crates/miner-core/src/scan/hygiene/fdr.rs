//! Benjamini-Hochberg FDR — `bh_fdr` (Plan 05-02 / D5-02 / HYG-02).
//!
//! Pattern analog: `crate::scan::ljung_box::kernel::biased_acf` (~25 LOC
//! pure function with hand-computed unit tests). Hand-rolled per
//! 05-RESEARCH §"Don't Hand-Roll" — the `adjustp` crate is single-author
//! / 430 dl/mo and explicitly rejected.
//!
//! ## Reference: `R::p.adjust(method = "BH")`
//!
//! For the canonical 5-tuple `[0.01, 0.02, 0.03, 0.04, 0.05]` the BH q-values
//! are all exactly 0.05 (every p × n/i hits 0.05 → step-up adjustment is
//! a no-op). This kernel reproduces that result within `1e-12`.

/// Benjamini-Hochberg (1995) step-up FDR adjustment.
///
/// Returns adjusted q-values in INPUT order (same index as `p_values`).
/// Internally sorts a working buffer of `(orig_index, p_value)` pairs; the
/// input slice is NOT mutated.
///
/// `alpha` is the family-wise FDR target; it is NOT used directly in the
/// q-value computation (BH q-values depend only on `p_values`). The
/// parameter is documented for clarity — callers may compare `q < alpha`
/// downstream to reject hypotheses. A `debug_assert!` enforces
/// `alpha in [0, 1]` under `cfg(debug_assertions)`.
///
/// ## Algorithm (Benjamini & Hochberg 1995)
///
/// Let `p_(i)` denote the `i`-th order statistic of `p_values` (1-indexed).
/// The BH-adjusted q-value at rank `i` is the running minimum (from the top
/// rank `n` down to rank 1) of `min(1, p_(k) * n / k)` for `k >= i`. This
/// enforces the step-up monotonicity that makes `q` a non-decreasing
/// function of `p`'s rank.
///
/// ## NaN handling (CR-03)
///
/// NaN p-values are NOT silently included in the sort + step-up walk
/// (the prior `partial_cmp(&b).unwrap_or(Equal)` placed NaNs in
/// arbitrary positions, corrupting q-values for every entry in the
/// family — not just the NaN ones). Instead:
///
/// - NaN entries are filtered OUT of the sort/walk; q-values are
///   computed on the finite-p subset with the BH `n` equal to the
///   COUNT OF FINITE entries (not the full input length).
/// - At the corresponding output index for each NaN input, the q-value
///   is `f64::NAN` (caller can detect and surface).
///
/// The decision (vs early-error) keeps the kernel tolerant of degenerate
/// analytic-p inputs that flow legitimately through the engine boundary
/// (e.g., constant-variance bucket → t-stat NaN) without poisoning the
/// rest of the family. Callers that want strict rejection should filter
/// `p.is_nan()` at their boundary (see `sweep::executor` drain loop).
#[must_use]
#[allow(
    clippy::cast_precision_loss,
    reason = "n is the input vector length (bounded by the engine sweep cap << 2^52); the cast to f64 is exact for any realistic n"
)]
pub fn bh_fdr(p_values: &[f64], alpha: f64) -> Vec<f64> {
    let n = p_values.len();
    if n == 0 {
        return Vec::new();
    }
    debug_assert!(
        (0.0..=1.0).contains(&alpha),
        "alpha out of [0, 1]: {alpha}"
    );

    // Pre-fill output with NaN. Positions corresponding to NaN p-values
    // stay NaN; positions corresponding to finite p-values are overwritten
    // below.
    let mut q = vec![f64::NAN; n];

    // Sort an `(original_index, p_value)` buffer ascending by p, but
    // FILTER NaN entries out of the working set so they cannot end up
    // in arbitrary sort positions (CR-03). The BH step-up walk operates
    // on the finite-p subset only; NaN inputs receive NaN q-values
    // (already pre-filled above).
    let mut indexed: Vec<(usize, f64)> = p_values
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, p)| !p.is_nan())
        .collect();
    if indexed.is_empty() {
        return q;
    }
    indexed.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

    // BH `n` is the COUNT OF FINITE p-values, not the full input length.
    // Including NaN positions in `n` would inflate q-values for every
    // finite entry by the number of NaN inputs.
    let n_finite = indexed.len();
    let n_f = n_finite as f64;
    // Reverse-scan running-min for step-up monotonicity.
    let mut running_min = 1.0_f64;
    for k in (0..n_finite).rev() {
        // 1-indexed rank for the k-th smallest p.
        let i = k + 1;
        let raw_q = (indexed[k].1 * n_f / (i as f64)).min(1.0);
        running_min = running_min.min(raw_q);
        q[indexed[k].0] = running_min;
    }
    q
}

// ---------------------------------------------------------------------------
// Tests (RED — body unimplemented; panics expected until GREEN)
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::cast_lossless, clippy::cast_precision_loss)]
mod tests {
    use super::*;

    const TOL: f64 = 1e-12;

    fn approx_eq(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    /// Test 1 (Plan 05-02): `bh_fdr(&[0.01, 0.02, 0.03, 0.04, 0.05], 0.05)`
    /// returns five values each within 1e-12 of 0.05.
    /// Matches `R::p.adjust(c(0.01, 0.02, 0.03, 0.04, 0.05), method = "BH")`.
    #[test]
    fn bh_fdr_canonical_5() {
        let p = [0.01_f64, 0.02, 0.03, 0.04, 0.05];
        let q = bh_fdr(&p, 0.05);
        assert_eq!(q.len(), 5);
        for (i, qi) in q.iter().enumerate() {
            assert!(
                approx_eq(*qi, 0.05, TOL),
                "q[{i}] = {qi}, expected ~0.05 (diff {})",
                (qi - 0.05).abs()
            );
        }
    }

    /// Test 2 (Plan 05-02): `bh_fdr` preserves rank order. For
    /// `p = [0.001, 0.5, 0.01, 0.04, 0.99]` the smallest-p slot (index 0)
    /// must have the smallest q-value; the largest-p slot (index 4) the
    /// largest.
    #[test]
    fn bh_fdr_preserves_rank_order() {
        let p = [0.001_f64, 0.5, 0.01, 0.04, 0.99];
        let q = bh_fdr(&p, 0.05);
        assert_eq!(q.len(), 5);
        // p rank ascending: indices [0, 2, 3, 1, 4] (p = 0.001, 0.01, 0.04, 0.5, 0.99).
        assert!(q[0] <= q[2], "q[0]={} q[2]={}", q[0], q[2]);
        assert!(q[2] <= q[3], "q[2]={} q[3]={}", q[2], q[3]);
        assert!(q[3] <= q[1], "q[3]={} q[1]={}", q[3], q[1]);
        assert!(q[1] <= q[4], "q[1]={} q[4]={}", q[1], q[4]);
    }

    /// Test 3 (Plan 05-02): `bh_fdr(&[], 0.05)` returns empty Vec.
    #[test]
    fn bh_fdr_empty() {
        let p: [f64; 0] = [];
        let q = bh_fdr(&p, 0.05);
        assert!(q.is_empty());
    }

    /// Test 4 (Plan 05-02): `bh_fdr(&[0.03], 0.05)` returns `vec![0.03]`.
    /// For n = 1 the BH adjustment is the identity (p * n / i = p * 1 / 1 = p).
    #[test]
    fn bh_fdr_single() {
        let q = bh_fdr(&[0.03_f64], 0.05);
        assert_eq!(q.len(), 1);
        assert!(approx_eq(q[0], 0.03, TOL), "q[0] = {} expected 0.03", q[0]);
    }

    /// Test 5 (Plan 05-02): rank-order preservation under property-style
    /// random inputs. For a 20-element random p-vector, the q-values respect
    /// the rank order of the inputs (after deduplicating ties — equal
    /// p-values must produce equal q-values).
    #[test]
    fn bh_fdr_rank_order_property() {
        // Deterministic LCG-seeded vector of length 20 in [0, 1].
        let mut s = 0xDEAD_u32;
        let mut p = Vec::with_capacity(20);
        for _ in 0..20 {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            p.push(f64::from(s) / f64::from(u32::MAX));
        }
        let q = bh_fdr(&p, 0.05);
        // Build the index sort order on p.
        let mut idx: Vec<usize> = (0..p.len()).collect();
        idx.sort_by(|a, b| p[*a].partial_cmp(&p[*b]).unwrap_or(std::cmp::Ordering::Equal));
        // Walk the sorted indices; consecutive q-values must be non-decreasing.
        for w in idx.windows(2) {
            let (i, j) = (w[0], w[1]);
            // Tied p → equal (within TOL) q.
            if approx_eq(p[i], p[j], TOL) {
                assert!(
                    approx_eq(q[i], q[j], TOL),
                    "p[{i}]={} == p[{j}]={} but q[{i}]={} != q[{j}]={}",
                    p[i],
                    p[j],
                    q[i],
                    q[j]
                );
            } else {
                assert!(
                    q[i] <= q[j] + TOL,
                    "rank order violated: p[{i}]={} < p[{j}]={} but q[{i}]={} > q[{j}]={}",
                    p[i],
                    p[j],
                    q[i],
                    q[j]
                );
            }
        }
    }

    /// Test 6 (Plan 05-02): every q-value is in [0, 1].
    #[test]
    fn bh_fdr_q_in_unit_interval() {
        let p = [0.001_f64, 0.01, 0.05, 0.1, 0.5, 0.9, 0.99];
        let q = bh_fdr(&p, 0.05);
        for (i, qi) in q.iter().enumerate() {
            assert!(
                (0.0..=1.0).contains(qi),
                "q[{i}] = {qi} outside [0, 1]"
            );
        }
    }
}
