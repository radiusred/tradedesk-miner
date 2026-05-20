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
//!
//! RED placeholder: body returns `unimplemented!()` so the Task 3 RED
//! tests panic. Task 3 GREEN fills the body.

/// Benjamini-Hochberg (1995) step-up FDR adjustment — Task 3 GREEN body.
#[must_use]
#[allow(unused_variables)]
pub fn bh_fdr(p_values: &[f64], alpha: f64) -> Vec<f64> {
    // RED: Task 3 GREEN fills this body.
    unimplemented!("Plan 05-02 Task 3 GREEN fills this body")
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
