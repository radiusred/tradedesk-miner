//! Null-distribution kernels — `circular_shift_null_p` (Plan 05-02 / D5-04 /
//! HYG-04) and `iaaft_phase_scramble_null_p` (Plan 07-05 / HYG-02 / HYG-05).
//!
//! Pattern analog: `crate::scan::anom::adf::kernel` (hand-rolled
//! deterministic statistical kernel; sequential inner loop, NO rayon
//! inside). Shares the `Xoshiro256PlusPlus` discipline with
//! [`super::bootstrap`].
//!
//! ## IAAFT phase-scramble
//!
//! `iaaft_phase_scramble_null_p` (Plan 07-05) is a sibling to
//! `circular_shift_null_p`: same positional signature contract for the
//! byte-identical-rerun invariant (HYG-05) plus two extra kernel-tuning
//! parameters (`max_iter` for the inner Theiler 1992 amplitude/phase
//! correction loop, `convergence_tol` for the rank-distance early-exit).
//! The IAAFT surrogate preserves BOTH the marginal distribution and the
//! power spectrum of the input series — making it the gold-standard null
//! for autocorrelation/spectral tests where circular-shift is insufficient
//! (Theiler & Prichard 1996; Schreiber & Schmitz 2000).
//!
//! Backed by `realfft = "3.5"` (real-input FFT plans cached once per
//! kernel call; transitively pulls `rustfft 6.x`). Both crates are
//! sync-only — the FOUND-04 tokio-free invariant for `miner-core` is
//! re-verified by `cargo tree -p miner-core --edges normal,build` in
//! Plan 07-05's acceptance criteria.

use std::sync::atomic::{AtomicBool, Ordering};

use rand::Rng;
use rand::SeedableRng;
use rand_xoshiro::Xoshiro256PlusPlus;
use realfft::RealFftPlanner;
use realfft::num_complex::Complex;

use super::bootstrap::BOOTSTRAP_CANCEL_POLL_CADENCE;

/// Tail-direction selector for surrogate-data null tests (WR-01).
///
/// Different test statistics have different rejection regions under the
/// null. Using a one-size-fits-all two-sided `|x| >= |obs|` comparison
/// (the pre-WR-01 behaviour) biases the empirical p-value downward for
/// statistics whose null distribution is asymmetric (chi-square-like).
///
/// - [`Tail::TwoSided`] — `|surr| >= |obs|`. Default for symmetric
///   statistics (variance ratio, mean, correlation, OLS β).
/// - [`Tail::OneSidedPos`] — `surr >= obs`. Use for chi-square-like
///   one-sided statistics: KPSS, ARCH-LM, Jarque-Bera, Ljung-Box Q.
///   Large positive surrogate values exceed the observed value.
/// - [`Tail::OneSidedNeg`] — `surr <= obs`. Use for signed statistics
///   where large-negative is the rejection direction: ADF τ.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tail {
    TwoSided,
    OneSidedPos,
    OneSidedNeg,
}

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
#[allow(
    clippy::too_many_arguments,
    reason = "WR-04 cancel parameter; positional contract retained for byte-identical-rerun parity"
)]
pub fn circular_shift_null_p<F>(
    values: &[f64],
    observed_stat: f64,
    stat: F,
    n_resamples: u32,
    seed: u64,
    tail: Tail,
    cancel: &AtomicBool,
) -> f64
where
    F: Fn(&[f64]) -> f64,
{
    let n = values.len();
    if n < 2 || n_resamples == 0 {
        return f64::NAN;
    }
    // WR-06 (defence-in-depth): clamp n_resamples at the kernel boundary
    // against HYGIENE_RESAMPLE_CEILING.
    let n_resamples = n_resamples.min(crate::engine::HYGIENE_RESAMPLE_CEILING);
    let mut rng = Xoshiro256PlusPlus::seed_from_u64(seed);
    let mut surrogate: Vec<f64> = vec![0.0; n];
    let mut more_extreme: u32 = 0;
    // WR-01: tail-dependent more-extreme test. Pre-WR-01 used `|surr| >= |obs|`
    // unconditionally; for one-sided test statistics this biases the
    // empirical p-value downward.
    let obs_abs = observed_stat.abs();
    for resample in 0..n_resamples {
        // WR-04: sparse cancel poll. Same cadence as the bootstrap kernels.
        if resample % BOOTSTRAP_CANCEL_POLL_CADENCE == 0 && cancel.load(Ordering::Relaxed) {
            return f64::NAN;
        }
        let offset = rng.gen_range(1..n); // 1..n excludes the identity (offset 0)
        for (i, slot) in surrogate.iter_mut().enumerate().take(n) {
            *slot = values[(i + offset) % n];
        }
        let surr_stat = stat(&surrogate);
        let is_more_extreme = match tail {
            Tail::TwoSided => surr_stat.abs() >= obs_abs,
            Tail::OneSidedPos => surr_stat >= observed_stat,
            Tail::OneSidedNeg => surr_stat <= observed_stat,
        };
        if is_more_extreme {
            more_extreme += 1;
        }
    }
    // CR-02 (1+B)/(1+N) — Davison & Hinkley 1997 §4.2 convention. Floors
    // p at 1/(n_resamples+1) so the empirical p-value is never exactly
    // zero. Do NOT "fix" this back to the naive B/N form.
    (f64::from(more_extreme) + 1.0) / (f64::from(n_resamples) + 1.0)
}

// ---------------------------------------------------------------------------
// IAAFT phase-scramble null kernel (Plan 07-05 / HYG-02 / HYG-05)
// ---------------------------------------------------------------------------

/// Round `n` up to the next 5-smooth integer (an integer whose only prime
/// factors are 2, 3, and 5). Used to pick an FFT-friendly padding length
/// for the IAAFT surrogate generator — rustfft (under realfft) is fastest
/// on highly-composite lengths and pathologically slow on large primes.
///
/// Returns `n` unchanged if `n` is already 5-smooth.
///
/// # Examples
///
/// ```text
/// next_5_smooth(1) == 1        // smallest 5-smooth integer
/// next_5_smooth(1024) == 1024  // already 5-smooth (2^10)
/// next_5_smooth(1009) == 1024  // 1009 is prime, round up
/// next_5_smooth(100_000) == 100_000  // 2^5 * 5^5 * ... is 5-smooth
/// ```
#[must_use]
pub(super) fn next_5_smooth(n: usize) -> usize {
    let n = n.max(1);
    let mut candidate = n;
    loop {
        if is_5_smooth(candidate) {
            return candidate;
        }
        candidate = candidate.saturating_add(1);
        // Defence in depth: extremely large inputs could overflow; the
        // realfft path will reject sizes above usize::MAX/2 anyway. For
        // realistic IAAFT inputs (n << 2^30) this loop terminates within
        // log(n) iterations because 5-smooth integers are dense.
    }
}

#[inline]
fn is_5_smooth(mut n: usize) -> bool {
    if n == 0 {
        return false;
    }
    for &p in &[2_usize, 3, 5] {
        while n % p == 0 {
            n /= p;
        }
    }
    n == 1
}

/// IAAFT (Iterative Amplitude-Adjusted Fourier Transform) phase-scramble
/// surrogate-data null distribution p-value (Theiler 1992; Schreiber &
/// Schmitz 2000).
///
/// Generates `n_resamples` surrogate series that preserve BOTH the
/// marginal distribution AND the power spectrum of `values`, by iterating
/// the two-step Theiler procedure:
///
/// 1. **Phase randomisation** — replace each Fourier-amplitude phase with
///    a uniform-random angle in `[0, 2π)`. The amplitude `|X(f)|` is left
///    unchanged.
/// 2. **Rank shuffle** — replace each inverse-FFT time-domain value with
///    the input value of matching rank (a stable sort + index swap),
///    restoring the exact marginal distribution.
///
/// Steps 1-2 alternate up to `max_iter` times; the inner loop exits early
/// when the rank-distance between consecutive iterations falls below
/// `convergence_tol` (≤ 1 is a sensible IEEE-754 tolerance — for integer-
/// rank distance, `< 1` means "no rank order changed").
///
/// Returns the empirical p-value using the same `(1 + B) / (1 + N)`
/// Davison & Hinkley 1997 §4.2 convention as `circular_shift_null_p` —
/// the floor at `1 / (n_resamples + 1)` prevents the singularity at
/// `p == 0.0` that would propagate as `q == 0.0` through BH-FDR.
///
/// `seed` propagates from `derive_job_seed` (HYG-05). The kernel uses
/// `Xoshiro256PlusPlus::seed_from_u64(seed)` and a STABLE rank-shuffle
/// sort (`sort_by` with index tiebreaker — never an unstable sort) so
/// byte-identical re-runs are guaranteed.
///
/// FFT length is padded to the next 5-smooth integer ≥ `n` via
/// [`next_5_smooth`]; on a prime `n` (e.g. 1009) this avoids the ~10x
/// runtime cost rustfft incurs on large primes (RESEARCH §"Pitfall 7").
///
/// ## Edge cases
///
/// - `values.len() < 4` → `NaN` (IAAFT needs ≥ 4 samples for a meaningful
///   FFT spectrum).
/// - `n_resamples == 0` → `NaN`.
/// - `cancel` flipped before invocation (or every
///   [`BOOTSTRAP_CANCEL_POLL_CADENCE`] resamples) → `NaN` (matches
///   `circular_shift_null_p` cancel semantics).
#[must_use]
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "n_resamples and rank/index counts are bounded by n (the IAAFT input length); all numeric casts are exact for realistic OHLCV slice sizes (n << 2^31)"
)]
#[allow(
    clippy::too_many_arguments,
    reason = "WR-04 cancel parameter + IAAFT tuning parameters (max_iter, convergence_tol); positional contract retained for byte-identical-rerun parity with `circular_shift_null_p`"
)]
#[allow(
    clippy::too_many_lines,
    reason = "linear setup -> resample loop -> inner Theiler iteration walk; splitting hides the documented per-step contract"
)]
pub fn iaaft_phase_scramble_null_p<F>(
    values: &[f64],
    observed_stat: f64,
    stat: F,
    n_resamples: u32,
    seed: u64,
    tail: Tail,
    cancel: &AtomicBool,
    max_iter: u32,
    convergence_tol: f64,
) -> f64
where
    F: Fn(&[f64]) -> f64,
{
    let n = values.len();
    if n < 4 || n_resamples == 0 {
        return f64::NAN;
    }
    // Pre-flight cancel — match circular_shift_null_p's early-exit on
    // pre-set cancel (the resample loop also polls every cadence).
    if cancel.load(Ordering::Relaxed) {
        return f64::NAN;
    }
    // WR-06 (defence-in-depth): clamp n_resamples at the kernel boundary
    // against HYGIENE_RESAMPLE_CEILING.
    let n_resamples = n_resamples.min(crate::engine::HYGIENE_RESAMPLE_CEILING);

    // --- Pre-loop setup (computed once, outside the resample loop) ----------
    let n_padded = next_5_smooth(n);

    // FFT plans — built once, reused across all resamples + iterations.
    let mut planner = RealFftPlanner::<f64>::new();
    let r2c = planner.plan_fft_forward(n_padded);
    let c2r = planner.plan_fft_inverse(n_padded);

    // Target amplitudes |X(f)| — computed once from the zero-padded input.
    let mut input_padded: Vec<f64> = Vec::with_capacity(n_padded);
    input_padded.extend_from_slice(values);
    input_padded.resize(n_padded, 0.0);
    let mut spectrum: Vec<Complex<f64>> = r2c.make_output_vec();
    // The realfft API returns Result<(), FftError>; on a properly-sized
    // input/output vector this never fails. If it does, surrogate
    // generation is impossible — return NaN like other infeasible-input
    // branches.
    if r2c.process(&mut input_padded, &mut spectrum).is_err() {
        return f64::NAN;
    }
    let target_amplitudes: Vec<f64> = spectrum.iter().map(|c| c.norm()).collect();

    // Sorted input values + their inverse rank lookup for the rank-shuffle
    // step. STABLE sort with explicit `(idx, val)` tiebreaker — RESEARCH
    // §"Anti-Patterns" forbids unstable sorts here because they would
    // produce non-deterministic output on tied inputs (e.g. integer-
    // valued series).
    let mut sorted_values: Vec<f64> = values.to_vec();
    // f64 lacks Ord; `partial_cmp.unwrap_or(Equal)` gives a total order
    // over the non-NaN finite inputs we expect. `sort_by` is a stable
    // sort and preserves the original index order on ties — the
    // determinism property we need for HYG-05. (Using an unstable sort
    // here would defeat the byte-identical-rerun contract on tied
    // inputs — see RESEARCH §"Anti-Patterns".)
    sorted_values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    // --- Resample loop ------------------------------------------------------
    let mut rng = Xoshiro256PlusPlus::seed_from_u64(seed);
    // Pre-allocated scratch buffers — RESEARCH §"Pitfall 2" memory
    // amplification: allocate ONCE, reuse across `n_resamples * max_iter`
    // iterations. r2c.make_output_vec() and c2r.make_input_vec() both
    // return `Vec<Complex<f64>>` of length `n_padded/2 + 1` (the half-
    // spectrum of a real-input FFT). r2c.make_input_vec() and
    // c2r.make_output_vec() both return `Vec<f64>` of length `n_padded`.
    let mut spectrum_scratch: Vec<Complex<f64>> = r2c.make_output_vec();
    let mut time_scratch: Vec<f64> = r2c.make_input_vec();
    // c2r's process mutates its complex input — keep a separate buffer
    // so spectrum_scratch survives untouched for the next forward FFT.
    let mut c2r_input: Vec<Complex<f64>> = c2r.make_input_vec();
    debug_assert_eq!(c2r_input.len(), spectrum.len());
    debug_assert_eq!(time_scratch.len(), n_padded);
    let mut prev_ranks: Vec<usize> = vec![0; n];
    let mut curr_ranks: Vec<usize> = vec![0; n];
    // Surrogate output buffer — captured AFTER each rank-shuffle, BEFORE
    // the forward FFT (which mutates time_scratch via realfft's in-place
    // input scratchpad pattern). Holds the most-recent rank-corrected
    // surrogate so the converged result survives the loop tail.
    let mut surrogate_out: Vec<f64> = vec![0.0; n];
    let obs_abs = observed_stat.abs();
    let mut more_extreme: u32 = 0;

    for resample in 0..n_resamples {
        // WR-04: sparse cancel poll. Same cadence as circular_shift_null_p.
        if resample % BOOTSTRAP_CANCEL_POLL_CADENCE == 0 && cancel.load(Ordering::Relaxed) {
            return f64::NAN;
        }

        // Step 1 — random phases. Initial spectrum = target |X(f)| * exp(i*φ).
        for (i, slot) in spectrum_scratch.iter_mut().enumerate() {
            let phi: f64 = rng.gen_range(0.0..std::f64::consts::TAU);
            let (sin_phi, cos_phi) = phi.sin_cos();
            *slot = Complex::new(target_amplitudes[i] * cos_phi, target_amplitudes[i] * sin_phi);
        }
        // Hermitian symmetry is handled automatically by realfft's
        // real-output API (the DC and Nyquist bins must be real-valued).
        spectrum_scratch[0] = Complex::new(spectrum_scratch[0].re, 0.0);
        if n_padded % 2 == 0 {
            let last = spectrum_scratch.len() - 1;
            spectrum_scratch[last] = Complex::new(spectrum_scratch[last].re, 0.0);
        }

        // Iterative correction loop (Theiler 1992 inner).
        prev_ranks.fill(usize::MAX);
        for _iter in 0..max_iter {
            // Step 1a — inverse FFT to time domain. realfft does NOT
            // normalise; divide by n_padded after.
            c2r_input.copy_from_slice(&spectrum_scratch);
            if c2r.process(&mut c2r_input, &mut time_scratch).is_err() {
                return f64::NAN;
            }
            let inv_norm = 1.0 / n_padded as f64;
            for v in &mut time_scratch {
                *v *= inv_norm;
            }

            // Step 1b — rank-shuffle: assign rank of surrogate[i] within
            // surrogate (over the first `n` samples — drop padding), then
            // replace surrogate[i] with sorted_values[rank].
            //
            // Stable rank computation: pair (idx, value), sort_by with
            // explicit index tiebreaker on equal values — the anti-pattern
            // check from Test 6 / threat T-07-05-01.
            let mut idx_val: Vec<(usize, f64)> = time_scratch
                .iter()
                .take(n)
                .copied()
                .enumerate()
                .collect();
            idx_val.sort_by(|a, b| {
                a.1.partial_cmp(&b.1)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| a.0.cmp(&b.0))
            });
            // After sort, idx_val[rank].0 is the original position of the
            // rank-th-smallest value. Invert this so curr_ranks[orig_pos]
            // = rank.
            for (rank, &(orig_idx, _)) in idx_val.iter().enumerate() {
                curr_ranks[orig_idx] = rank;
            }
            // Replace surrogate values with sorted_values[rank] — preserves
            // the input's marginal distribution. Write into both
            // `time_scratch` (input to next forward FFT) AND
            // `surrogate_out` (the output captured here so it survives
            // the forward FFT's in-place input scratchpad pattern).
            for (i, slot) in time_scratch.iter_mut().take(n).enumerate() {
                *slot = sorted_values[curr_ranks[i]];
            }
            surrogate_out.copy_from_slice(&time_scratch[..n]);
            // Zero the padding region so the next forward FFT sees a clean
            // padded series.
            for v in time_scratch.iter_mut().skip(n) {
                *v = 0.0;
            }

            // Convergence test — rank-distance between iterations.
            // For integer rank vectors, distance < 1 means no rank
            // order changed; we allow `convergence_tol ≤ 1` so the same
            // condition covers any IEEE-754 tolerance.
            let rank_distance: f64 = curr_ranks
                .iter()
                .zip(prev_ranks.iter())
                .map(|(&c, &p)| {
                    if p == usize::MAX {
                        // First iteration — force "not converged".
                        f64::INFINITY
                    } else {
                        let d = c as isize - p as isize;
                        (d * d) as f64
                    }
                })
                .sum::<f64>()
                .sqrt();
            prev_ranks.copy_from_slice(&curr_ranks);
            if rank_distance < convergence_tol {
                break;
            }

            // Step 1c — forward FFT (mutates time_scratch), replace
            // amplitudes with target amplitudes (preserves spectrum),
            // keep new phases. Skipped on the convergence break above
            // because the surrogate is already captured in surrogate_out.
            if r2c.process(&mut time_scratch, &mut spectrum_scratch).is_err() {
                return f64::NAN;
            }
            for (i, slot) in spectrum_scratch.iter_mut().enumerate() {
                let current_amp = slot.norm();
                if current_amp > 0.0 {
                    let scale = target_amplitudes[i] / current_amp;
                    *slot = Complex::new(slot.re * scale, slot.im * scale);
                } else {
                    // amplitude-zero bin: preserve real-valued zero
                    *slot = Complex::new(0.0, 0.0);
                }
            }
        }

        // After the iterative loop, surrogate_out holds the surrogate
        // series with marginals matching the input.
        let surrogate: &[f64] = &surrogate_out;
        let surr_stat = stat(surrogate);
        let is_more_extreme = match tail {
            Tail::TwoSided => surr_stat.abs() >= obs_abs,
            Tail::OneSidedPos => surr_stat >= observed_stat,
            Tail::OneSidedNeg => surr_stat <= observed_stat,
        };
        if is_more_extreme {
            more_extreme += 1;
        }
    }

    // CR-02 (1+B)/(1+N) — same Davison & Hinkley 1997 §4.2 floor as
    // `circular_shift_null_p`.
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
    use std::sync::atomic::AtomicBool;

    /// Helper: a never-cancelled flag for tests that exercise the kernel
    /// completion path. The kernel polls this every
    /// [`BOOTSTRAP_CANCEL_POLL_CADENCE`] resamples; tests pass a
    /// fresh-`false` flag so the cancel branch is never taken.
    fn no_cancel() -> AtomicBool {
        AtomicBool::new(false)
    }

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
                Tail::TwoSided,
                &no_cancel(),
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
        let p_a = circular_shift_null_p(
            &values,
            observed,
            mean,
            100,
            0xBEEF,
            Tail::TwoSided,
            &no_cancel(),
        );
        let p_b = circular_shift_null_p(
            &values,
            observed,
            mean,
            100,
            0xBEEF,
            Tail::TwoSided,
            &no_cancel(),
        );
        assert_eq!(p_a.to_bits(), p_b.to_bits(), "p-value bit-identity");
    }

    /// `circular_shift_null_p` short-input edge cases.
    #[test]
    fn circular_shift_null_p_short_input_nan() {
        let one = [1.0_f64];
        assert!(
            circular_shift_null_p(&one, 1.0, mean, 100, 0, Tail::TwoSided, &no_cancel()).is_nan()
        );
        let empty: [f64; 0] = [];
        assert!(
            circular_shift_null_p(&empty, 0.0, mean, 100, 0, Tail::TwoSided, &no_cancel()).is_nan()
        );
        let two = [1.0_f64, 2.0];
        assert!(
            circular_shift_null_p(&two, 1.5, mean, 0, 0, Tail::TwoSided, &no_cancel()).is_nan()
        );
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
        let p = circular_shift_null_p(
            &values,
            observed,
            mean,
            n_resamples,
            0xCAFE,
            Tail::TwoSided,
            &no_cancel(),
        );

        // (1 + 0) / (1 + 99) = 0.01 exactly.
        let expected = 1.0_f64 / (f64::from(n_resamples) + 1.0);
        assert!(p > 0.0, "p must be strictly positive: got {p}");
        assert!(
            (p - expected).abs() < 1e-15,
            "p {p} != expected floor {expected} = 1/(N+1)"
        );
    }

    // -----------------------------------------------------------------------
    // IAAFT tests (Plan 07-05) — eight behaviour tests pinning the kernel
    // contract documented in `iaaft_phase_scramble_null_p`.
    // -----------------------------------------------------------------------

    /// Helper: a small finite series large enough (n >= 4) for the IAAFT
    /// FFT path to be meaningful.
    fn iaaft_test_series() -> Vec<f64> {
        lcg_iid(64, 0xFEED)
    }

    /// Test 1: n < 4 returns NaN.
    #[test]
    fn iaaft_returns_nan_for_short_input() {
        let three = [1.0_f64, 2.0, 3.0];
        let p = iaaft_phase_scramble_null_p(
            &three,
            0.0,
            |x| x.len() as f64,
            100,
            42,
            Tail::OneSidedPos,
            &no_cancel(),
            10,
            1.0,
        );
        assert!(p.is_nan(), "n=3 must return NaN; got {p}");
        let empty: [f64; 0] = [];
        let p2 = iaaft_phase_scramble_null_p(
            &empty,
            0.0,
            |x| x.len() as f64,
            100,
            42,
            Tail::OneSidedPos,
            &no_cancel(),
            10,
            1.0,
        );
        assert!(p2.is_nan(), "empty input must return NaN");
    }

    /// Test 2: `n_resamples` == 0 returns NaN.
    #[test]
    fn iaaft_returns_nan_for_zero_resamples() {
        let s = iaaft_test_series();
        let p = iaaft_phase_scramble_null_p(
            &s,
            0.0,
            mean,
            0,
            42,
            Tail::TwoSided,
            &no_cancel(),
            10,
            1.0,
        );
        assert!(p.is_nan(), "n_resamples=0 must return NaN; got {p}");
    }

    /// Test 3: the empirical-p floor at `1 / (n_resamples + 1)` — Davison
    /// & Hinkley 1997 §4.2 (same as `circular_shift_null_p`).
    #[test]
    fn iaaft_floor_is_one_over_resamples_plus_one() {
        let s = iaaft_test_series();
        let n_resamples = 99_u32;
        let p = iaaft_phase_scramble_null_p(
            &s,
            f64::INFINITY,
            |x| x.iter().sum::<f64>(),
            n_resamples,
            42,
            Tail::OneSidedPos,
            &no_cancel(),
            10,
            1.0,
        );
        let expected = 1.0_f64 / (f64::from(n_resamples) + 1.0);
        assert!(p > 0.0, "p must be strictly positive: got {p}");
        assert!(
            (p - expected).abs() < 1e-15,
            "p {p} != expected floor {expected} = 1/(N+1)"
        );
    }

    /// Test 4: byte-identical p-value across two runs with same seed
    /// (HYG-05 reproducibility contract).
    #[test]
    fn iaaft_byte_identical_across_runs_with_same_seed() {
        let s = iaaft_test_series();
        let observed = mean(&s);
        let p_a = iaaft_phase_scramble_null_p(
            &s,
            observed,
            mean,
            32,
            0xBEEF,
            Tail::TwoSided,
            &no_cancel(),
            10,
            1.0,
        );
        let p_b = iaaft_phase_scramble_null_p(
            &s,
            observed,
            mean,
            32,
            0xBEEF,
            Tail::TwoSided,
            &no_cancel(),
            10,
            1.0,
        );
        assert_eq!(p_a.to_bits(), p_b.to_bits(), "p-value bit-identity");
    }

    /// Test 5: surrogate preserves the marginal distribution of the input
    /// (defining IAAFT property). The stat closure captures the first
    /// surrogate it sees via a `Cell`-backed shared buffer; after the
    /// kernel returns, the test asserts `sort(surrogate) == sort(input)`.
    #[test]
    fn iaaft_marginal_preserved() {
        use std::cell::RefCell;
        let s = iaaft_test_series();
        let captured: RefCell<Option<Vec<f64>>> = RefCell::new(None);
        let stat_with_capture = |x: &[f64]| -> f64 {
            // Capture the first surrogate only.
            let mut slot = captured.borrow_mut();
            if slot.is_none() {
                *slot = Some(x.to_vec());
            }
            mean(x)
        };
        let _ = iaaft_phase_scramble_null_p(
            &s,
            0.0,
            stat_with_capture,
            4,
            0xABCD,
            Tail::TwoSided,
            &no_cancel(),
            10,
            1.0,
        );
        let surrogate = captured.into_inner().expect("surrogate captured");
        // Sorted surrogate must equal sorted input — IAAFT preserves
        // marginal exactly by construction (rank-shuffle step).
        let mut sorted_input = s.clone();
        sorted_input.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mut sorted_surr = surrogate.clone();
        sorted_surr.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert_eq!(sorted_input.len(), sorted_surr.len());
        for (a, b) in sorted_input.iter().zip(sorted_surr.iter()) {
            assert!(
                (a - b).abs() < 1e-12,
                "marginal mismatch: input {a} vs surrogate {b}"
            );
        }
    }

    /// Test 6 (T-07-05-01 mitigation): stable rank-shuffle anti-pattern
    /// check. With tied input values, two runs with the same seed must
    /// produce bit-identical p-values — using an unstable sort (the
    /// anti-pattern) would produce non-deterministic output on ties.
    #[test]
    fn iaaft_uses_stable_rank_shuffle() {
        // Repeated ties: every value appears exactly twice.
        let tied: Vec<f64> = vec![1.0, 1.0, 2.0, 2.0, 3.0, 3.0, 4.0, 4.0];
        let p_a = iaaft_phase_scramble_null_p(
            &tied,
            mean(&tied),
            mean,
            16,
            0xC0DE,
            Tail::TwoSided,
            &no_cancel(),
            10,
            1.0,
        );
        let p_b = iaaft_phase_scramble_null_p(
            &tied,
            mean(&tied),
            mean,
            16,
            0xC0DE,
            Tail::TwoSided,
            &no_cancel(),
            10,
            1.0,
        );
        assert_eq!(
            p_a.to_bits(),
            p_b.to_bits(),
            "stable-sort discipline broken: tied-input p-values diverged ({p_a} vs {p_b})"
        );
    }

    /// Test 7: cancel-before-call aborts resampling. Matches
    /// `circular_shift_null_p`'s cancel behaviour: return NaN.
    #[test]
    fn iaaft_cancel_aborts_resampling() {
        let s = iaaft_test_series();
        let cancel = AtomicBool::new(true);
        let p = iaaft_phase_scramble_null_p(
            &s,
            0.0,
            mean,
            100,
            42,
            Tail::TwoSided,
            &cancel,
            10,
            1.0,
        );
        assert!(p.is_nan(), "cancel-before-call must return NaN; got {p}");
    }

    /// Test 8 (T-07-05-02 mitigation): FFT padding uses 5-smooth lengths.
    /// Direct unit test against the `next_5_smooth` helper:
    /// - 1024 is already 5-smooth (2^10) → unchanged.
    /// - 1009 is prime → rounds up to the next 5-smooth integer 1024.
    /// - `100_000` = 2^5 * 5^5 → already 5-smooth, unchanged.
    #[test]
    fn iaaft_padding_uses_5_smooth_length() {
        assert_eq!(next_5_smooth(1024), 1024, "1024 = 2^10 is 5-smooth");
        assert_eq!(next_5_smooth(1009), 1024, "1009 is prime; next 5-smooth = 1024");
        assert_eq!(
            next_5_smooth(100_000),
            100_000,
            "100_000 = 2^5 * 5^5 is 5-smooth"
        );
        // Sanity-pinning small inputs.
        assert_eq!(next_5_smooth(1), 1);
        assert_eq!(next_5_smooth(7), 8); // 7 is prime; next 5-smooth = 8
        assert_eq!(next_5_smooth(11), 12); // 11 is prime; next 5-smooth = 12
    }
}
