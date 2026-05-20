//! Null-distribution kernels — `circular_shift_null_p` (Plan 05-02 / D5-04 /
//! HYG-04).
//!
//! Pattern analog: `crate::scan::anom::adf::kernel` (hand-rolled
//! deterministic statistical kernel; sequential inner loop, NO rayon
//! inside). Shares the `Xoshiro256PlusPlus` discipline with
//! [`super::bootstrap`].
//!
//! ## IAAFT phase-scramble — DEFERRED to Phase 7
//!
//! Plan 05-02 ships ONLY `circular_shift_null_p`. The IAAFT (Theiler 1992)
//! phase-scramble null was an optional shipping decision in the plan; per
//! the IAAFT DECISION recorded in `05-02-SUMMARY.md`, IAAFT defers to
//! Phase 7 hardening so this plan adds zero new workspace dependencies
//! (`realfft` stays excluded from the workspace per Plan 05-01's
//! intentional-exclusion comment in `Cargo.toml`). Every Scan impl's
//! `Scan::supports_null_method(NullMethod::PhaseScramble)` will return
//! `false` until Phase 7; user requests for phase-scramble are rejected
//! with `PreflightCode::HygieneNotSupported`.
//!
//! RED placeholder: bodies return `unimplemented!()` so the Task 2 RED
//! tests panic. Task 2 GREEN fills the body.

use rand::Rng;
use rand::SeedableRng;
use rand_xoshiro::Xoshiro256PlusPlus;

/// Circular-shift empirical null p-value — Task 2 GREEN body.
#[must_use]
#[allow(unused_variables)]
pub fn circular_shift_null_p<F>(
    values: &[f64],
    observed_stat: f64,
    stat: F,
    n_resamples: u32,
    seed: u64,
) -> f64
where
    F: Fn(&[f64]) -> f64,
{
    // RED: Task 2 GREEN fills this body.
    unimplemented!("Plan 05-02 Task 2 GREEN fills this body")
}

// ---------------------------------------------------------------------------
// Tests (RED — body unimplemented; panics expected until GREEN)
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

    /// Test 6 (Plan 05-02): under the null the average p-value approaches
    /// 0.5 (loose ±0.2 bound for n_resamples=200 over 50 seeded trials).
    #[test]
    fn circular_shift_null_p_uniform_under_null() {
        let trials = 50_u32;
        let mut total_p = 0.0_f64;
        for trial in 0..trials {
            let values = lcg_iid(100, 0x100 + u64::from(trial));
            let observed = mean(&values);
            let p = circular_shift_null_p(
                &values,
                observed,
                mean,
                200,
                0x200 + u64::from(trial),
            );
            assert!(
                (0.0..=1.0).contains(&p),
                "p-value out of [0, 1]: {p} (trial {trial})"
            );
            total_p += p;
        }
        let avg_p = total_p / f64::from(trials);
        assert!(
            (avg_p - 0.5).abs() <= 0.2,
            "avg_p {avg_p:.3} not within ±0.2 of 0.5"
        );
    }

    /// Test 7 (Plan 05-02): byte-identical p-value for fixed seed.
    #[test]
    fn circular_shift_null_p_deterministic_for_seed() {
        let values = lcg_iid(50, 0xDEAD);
        let observed = mean(&values);
        let p_a = circular_shift_null_p(&values, observed, mean, 100, 0xBEEF);
        let p_b = circular_shift_null_p(&values, observed, mean, 100, 0xBEEF);
        assert_eq!(p_a.to_bits(), p_b.to_bits(), "p-value bit-identity");
    }

    /// `circular_shift_null_p` short-input edge cases.
    #[test]
    fn circular_shift_null_p_short_input_nan() {
        let one = [1.0_f64];
        assert!(circular_shift_null_p(&one, 1.0, mean, 100, 0).is_nan());
        let empty: [f64; 0] = [];
        assert!(circular_shift_null_p(&empty, 0.0, mean, 100, 0).is_nan());
        let two = [1.0_f64, 2.0];
        assert!(circular_shift_null_p(&two, 1.5, mean, 0, 0).is_nan());
    }
}
