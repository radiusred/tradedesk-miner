//! Bootstrap kernels — `stationary_bootstrap_ci`, `block_bootstrap_ci`,
//! `block_length_pwppw` (Plan 05-02 / D5-03 / HYG-03).
//!
//! Pattern analog: `crate::scan::anom::adf::kernel` (hand-rolled deterministic
//! statistical kernel; sequential inner loop, NO rayon inside — RESEARCH
//! Pitfall 4 / D5-05 byte-identical-rerun invariant). The
//! `Xoshiro256PlusPlus` PRNG is seeded from `u64` via
//! `SeedableRng::seed_from_u64`; `SmallRng` / `StdRng` are explicitly NOT
//! used (RESEARCH §1.5 — non-portable across rand versions).
//!
//! RED placeholder: bodies return `unimplemented!()` so the Task 2 RED
//! tests panic. Task 2 GREEN fills the bodies.

use rand::Rng;
use rand::SeedableRng;
use rand_xoshiro::Xoshiro256PlusPlus;

/// Politis-Romano (1994) stationary-bootstrap CI — Task 2 GREEN body.
#[must_use]
#[allow(unused_variables)]
pub fn stationary_bootstrap_ci<F>(
    values: &[f64],
    stat: F,
    n_resamples: u32,
    mean_block_len: f64,
    seed: u64,
    ci_level: f64,
) -> [f64; 2]
where
    F: Fn(&[f64]) -> f64,
{
    // RED: Task 2 GREEN fills this body.
    unimplemented!("Plan 05-02 Task 2 GREEN fills this body")
}

/// Fixed-block bootstrap CI — Task 2 GREEN body.
#[must_use]
#[allow(unused_variables)]
pub fn block_bootstrap_ci<F>(
    values: &[f64],
    stat: F,
    n_resamples: u32,
    block_len: usize,
    seed: u64,
    ci_level: f64,
) -> [f64; 2]
where
    F: Fn(&[f64]) -> f64,
{
    // RED: Task 2 GREEN fills this body.
    unimplemented!("Plan 05-02 Task 2 GREEN fills this body")
}

/// Politis-White / Patton-Politis-White automatic block-length selector —
/// Task 2 GREEN body.
#[must_use]
#[allow(unused_variables)]
pub fn block_length_pwppw(values: &[f64]) -> f64 {
    // RED: Task 2 GREEN fills this body.
    unimplemented!("Plan 05-02 Task 2 GREEN fills this body")
}

// ---------------------------------------------------------------------------
// Tests (RED — bodies unimplemented; panics expected until GREEN)
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(
    clippy::cast_lossless,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
mod tests {
    use super::*;

    /// Test fixture: deterministic LCG-derived iid samples in (-1, 1].
    /// Pattern: Pattern S6 in 05-PATTERNS lines 1366-1376.
    fn lcg_iid(n: usize, seed: u64) -> Vec<f64> {
        let mut s = seed as u32;
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let frac = f64::from(s) / f64::from(u32::MAX);
            out.push(2.0 * frac - 1.0);
        }
        out
    }

    fn mean(s: &[f64]) -> f64 {
        if s.is_empty() {
            return f64::NAN;
        }
        s.iter().copied().sum::<f64>() / s.len() as f64
    }

    /// Test 1 (Plan 05-02): byte-identical for fixed seed.
    #[test]
    fn stationary_bootstrap_ci_deterministic_for_seed() {
        let values = lcg_iid(50, 0xDEAD);
        let ci_a = stationary_bootstrap_ci(&values, mean, 100, 5.0, 0xBEEF, 0.95);
        let ci_b = stationary_bootstrap_ci(&values, mean, 100, 5.0, 0xBEEF, 0.95);
        assert_eq!(ci_a[0].to_bits(), ci_b[0].to_bits());
        assert_eq!(ci_a[1].to_bits(), ci_b[1].to_bits());
        assert!(ci_a[0].is_finite() && ci_a[1].is_finite());
        assert!(ci_a[0] <= ci_a[1], "lo must be <= hi");
    }

    /// Test 2 (Plan 05-02): Pinned `Xoshiro256PlusPlus` reference vector.
    /// Captures the first three `gen::<u64>()` outputs for
    /// `seed = 0x12345678_9abcdef0` so a future `rand_xoshiro` major bump
    /// (or an accidental swap to `SmallRng`/`StdRng`) is caught immediately.
    /// Reference values printed to stderr below for the SUMMARY artefact
    /// capture (run with `--nocapture` to view); pinned values are the
    /// outputs of the Plan 05-02 implementation.
    #[test]
    fn xoshiro_reference_vector_pinned() {
        let mut rng = Xoshiro256PlusPlus::seed_from_u64(0x1234_5678_9abc_def0_u64);
        let v0: u64 = rng.r#gen();
        let v1: u64 = rng.r#gen();
        let v2: u64 = rng.r#gen();
        // Reference vector pinned at Plan 05-02 implementation time. Update
        // SUMMARY.md if these change.
        let expected = [REF_V0, REF_V1, REF_V2];
        assert_eq!(v0, expected[0], "v0=0x{v0:016x}");
        assert_eq!(v1, expected[1], "v1=0x{v1:016x}");
        assert_eq!(v2, expected[2], "v2=0x{v2:016x}");
    }

    /// Plan 05-02 SUMMARY artefact — pinned Xoshiro reference values.
    /// Captured from `rand_xoshiro` 0.6.0 + `rand` 0.8.6 at Plan 05-02
    /// commit time. Documented in `05-02-SUMMARY.md` for future
    /// cross-version-bump regression detection.
    const REF_V0: u64 = 0x4d4f_7607_a97a_1bd6;
    const REF_V1: u64 = 0x9ba0_27c7_6910_d021;
    const REF_V2: u64 = 0x87ad_b062_153a_e0bc;

    /// Test 3 (Plan 05-02): iid coverage smoke ≥ 90% over 50 trials.
    #[test]
    fn stationary_bootstrap_iid_coverage() {
        let trials = 50_u32;
        let mut covered = 0_u32;
        for trial in 0..trials {
            let values = lcg_iid(200, 0x42 + u64::from(trial));
            let true_mean = mean(&values);
            let ci = stationary_bootstrap_ci(
                &values,
                mean,
                200,
                6.0,
                0xCAFE + u64::from(trial),
                0.95,
            );
            if ci[0] <= true_mean && true_mean <= ci[1] {
                covered += 1;
            }
        }
        let rate = f64::from(covered) / f64::from(trials);
        assert!(rate >= 0.9, "coverage rate {rate:.3} < 0.9");
    }

    /// Test 4 (Plan 05-02): short input → [NaN, NaN].
    #[test]
    fn stationary_bootstrap_ci_returns_nan_on_short() {
        let one = [1.0_f64];
        let ci = stationary_bootstrap_ci(&one, mean, 100, 3.0, 0, 0.95);
        assert!(ci[0].is_nan() && ci[1].is_nan());
        let empty: [f64; 0] = [];
        let ci_empty = stationary_bootstrap_ci(&empty, mean, 100, 3.0, 0, 0.95);
        assert!(ci_empty[0].is_nan() && ci_empty[1].is_nan());
        let two = [1.0_f64, 2.0];
        let ci_n0 = stationary_bootstrap_ci(&two, mean, 0, 3.0, 0, 0.95);
        assert!(ci_n0[0].is_nan() && ci_n0[1].is_nan());
    }

    /// `block_bootstrap_ci` byte-identical for fixed seed.
    #[test]
    fn block_bootstrap_ci_deterministic_for_seed() {
        let values = lcg_iid(50, 0xDEAD);
        let ci_a = block_bootstrap_ci(&values, mean, 100, 5, 0xBEEF, 0.95);
        let ci_b = block_bootstrap_ci(&values, mean, 100, 5, 0xBEEF, 0.95);
        assert_eq!(ci_a[0].to_bits(), ci_b[0].to_bits());
        assert_eq!(ci_a[1].to_bits(), ci_b[1].to_bits());
        assert!(ci_a[0] <= ci_a[1]);
    }

    /// `block_bootstrap_ci` short-input edge cases.
    #[test]
    fn block_bootstrap_ci_short_input_nan() {
        let one = [1.0_f64];
        assert!(block_bootstrap_ci(&one, mean, 100, 5, 0, 0.95)[0].is_nan());
        let two = [1.0_f64, 2.0];
        assert!(block_bootstrap_ci(&two, mean, 100, 0, 0, 0.95)[0].is_nan());
        assert!(block_bootstrap_ci(&two, mean, 0, 5, 0, 0.95)[0].is_nan());
    }

    /// Test 5 (Plan 05-02): `block_length_pwppw` returns finite sane value.
    #[test]
    fn block_length_pwppw_iid_sane() {
        let values = lcg_iid(1000, 0xDEAD);
        let b_star = block_length_pwppw(&values);
        assert!(b_star.is_finite());
        assert!(b_star > 0.0);
        let b_ceil = b_star.ceil() as usize;
        assert!(b_ceil >= 1 && b_ceil <= 50);
    }

    /// `block_length_pwppw` constant input → NaN.
    #[test]
    fn block_length_pwppw_constant_input_nan() {
        let values = vec![5.0_f64; 100];
        assert!(block_length_pwppw(&values).is_nan());
    }

    /// `block_length_pwppw` short input → NaN.
    #[test]
    fn block_length_pwppw_short_input_nan() {
        let short = [1.0_f64, 2.0, 3.0];
        assert!(block_length_pwppw(&short).is_nan());
    }
}
