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

use rand::Rng;
use rand::SeedableRng;
use rand_xoshiro::Xoshiro256PlusPlus;

/// Circular-shift surrogate-data null distribution p-value.
///
/// Builds `n_resamples` surrogate series by rotating `values` by a uniform
/// offset in `[1, n)` (offset 0 rejected — it is the identity transform).
/// For each surrogate, computes `stat(&shifted)` and tallies the count of
/// surrogates whose absolute statistic equals or exceeds the absolute
/// observed statistic. Returns the two-sided empirical p-value using the
/// textbook `(1 + more_extreme) / (1 + n_resamples)` surrogate-data
/// correction (Davison & Hinkley 1997 §4.2; Theiler & Prichard 1996).
///
/// ## Empirical-p convention — `(1 + B) / (1 + N)` (CR-02)
///
/// The conventional surrogate-data empirical p-value floors `p` at
/// `1 / (n_resamples + 1)` and avoids the mathematically untenable
/// singularity `p == 0.0`. The naive `B / N` form (which the original
/// implementation used) allowed `p == 0.0` when zero surrogates exceeded
/// the observed statistic — that value implies infinite log-odds against
/// the null and propagates as `q == 0.0` through BH-FDR, severely
/// understating multiple-testing inflation. The `(1 + B) / (1 + N)`
/// floor is the published correction.
///
/// `seed` propagates from `derive_job_seed` (HYG-05). The kernel uses
/// `Xoshiro256PlusPlus::seed_from_u64(seed)` — byte-identical re-runs are
/// guaranteed for fixed `(values, observed_stat, n_resamples, seed)`.
///
/// ## Edge cases
///
/// - `values.len() < 2` → `NaN` (no non-trivial rotation available; a
///   1-element series has only the identity offset).
/// - `n_resamples == 0` → `NaN`.
#[must_use]
#[allow(
    clippy::cast_precision_loss,
    reason = "n_resamples and more_extreme are u32 counts; the f64 conversion is exact for inputs < 2^53 (n_resamples << 2^31)"
)]
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
    let n = values.len();
    if n < 2 || n_resamples == 0 {
        return f64::NAN;
    }
    let mut rng = Xoshiro256PlusPlus::seed_from_u64(seed);
    let mut surrogate: Vec<f64> = vec![0.0; n];
    let mut more_extreme: u32 = 0;
    let obs_abs = observed_stat.abs();
    for _ in 0..n_resamples {
        let offset = rng.gen_range(1..n); // 1..n excludes the identity (offset 0)
        for (i, slot) in surrogate.iter_mut().enumerate().take(n) {
            *slot = values[(i + offset) % n];
        }
        let surr_stat = stat(&surrogate);
        if surr_stat.abs() >= obs_abs {
            more_extreme += 1;
        }
    }
    // CR-02 (1+B)/(1+N) — Davison & Hinkley 1997 §4.2 convention. Floors
    // p at 1/(n_resamples+1) so the empirical p-value is never exactly
    // zero. Do NOT "fix" this back to the naive B/N form.
    (f64::from(more_extreme) + 1.0) / (f64::from(n_resamples) + 1.0)
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
    /// 0.5 (loose ±0.2 bound for `n_resamples=200` over 50 seeded trials).
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

    /// CR-02 regression: empirical p MUST floor at `1 / (n_resamples + 1)`
    /// even when zero surrogates exceed the observed statistic.
    ///
    /// Construction: pass an `observed_stat` larger than any conceivable
    /// surrogate-mean of a unit-magnitude series; every rotation produces
    /// the SAME mean (rotation preserves the sum), so the count of more-
    /// extreme surrogates is exactly zero.
    ///
    /// Pre-fix the naive `B / N` form returned `0.0` for this construction.
    /// Post-fix the floor is `1 / (N + 1)`. Asserting `p > 0` AND
    /// `p == 1/(N+1)` pins both invariants.
    #[test]
    fn circular_shift_null_p_floors_at_one_over_n_plus_one() {
        // Series with non-zero sum, so the mean rotation is invariant
        // (all surrogates have the same mean as the observed series).
        let values = vec![1.0_f64; 50];
        // Observed statistic well above the surrogate mean (1.0): nothing
        // can exceed it, more_extreme stays 0.
        let observed = 1000.0;
        let n_resamples = 99_u32;
        let p = circular_shift_null_p(&values, observed, mean, n_resamples, 0xCAFE);

        // (1 + 0) / (1 + 99) = 0.01 exactly.
        let expected = 1.0_f64 / (f64::from(n_resamples) + 1.0);
        assert!(p > 0.0, "p must be strictly positive: got {p}");
        assert!(
            (p - expected).abs() < 1e-15,
            "p {p} != expected floor {expected} = 1/(N+1)"
        );
    }
}
