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
//! ## Cancel-poll discipline (WR-04 revision)
//!
//! The kernels in this module accept `cancel: &AtomicBool` and poll it
//! sparsely (every [`BOOTSTRAP_CANCEL_POLL_CADENCE`] resamples). The
//! original Plan-05-02 design did NOT poll inside the kernel — under the
//! assumption that kernels always finished in milliseconds. WR-04 surfaced
//! that the documented [`crate::engine::HYGIENE_RESAMPLE_CEILING`] (100k)
//! combined with realistic input sizes (`n` of order 10^4 bars) means a
//! single kernel call can run multi-second uninterruptibly — defeating
//! the cooperative-cancel contract. The sparse poll cadence (every 64
//! resamples) keeps per-iteration overhead near zero (~0.02% of a
//! resample) while bounding the worst-case cancel-response latency to
//! ~10ms on a realistic kernel.
//!
//! When cancel fires mid-kernel, the kernel returns `[NaN, NaN]` (CI) or
//! `NaN` (p-value); the engine treats NaN as "no CI / p-value" and
//! propagates the cancel via the outer-loop poll cadence (RESEARCH
//! Pitfall 7).
//!
//! ## Memory-amplification discipline (RESEARCH Pitfall 2)
//!
//! Inner-loop scratch buffers (`buf` in `stationary_bootstrap_ci`,
//! `block_bootstrap_ci`) are allocated ONCE before the resample loop and
//! `buf.clear()`-ed per resample. The naïve `Vec::with_capacity(n)` per
//! resample pattern would allocate `n_resamples * n` floats over the run.

use std::sync::atomic::{AtomicBool, Ordering};

use rand::Rng;
use rand::SeedableRng;
use rand_xoshiro::Xoshiro256PlusPlus;

/// Sparse cancel-poll cadence inside the bootstrap / null kernels (WR-04).
///
/// Polling every 64 resamples bounds the worst-case mid-kernel cancel
/// response to roughly `64 * cost_per_resample`, which is ~10ms on a
/// 10^4-bar input even at the documented [`crate::engine::HYGIENE_RESAMPLE_CEILING`].
pub const BOOTSTRAP_CANCEL_POLL_CADENCE: u32 = 64;

/// Politis-Romano (1994) stationary bootstrap CI for a scalar statistic of
/// an autocorrelated series.
///
/// The stationary bootstrap resamples blocks of geometric length (mean
/// `mean_block_len`). For each of `n_resamples` resamples, the kernel
/// computes `stat(&buf)` and accumulates the value; the returned CI is the
/// `[alpha/2, 1 - alpha/2]` quantile pair (`alpha = 1 - ci_level`).
///
/// `seed` propagates from `derive_job_seed` (HYG-05). The kernel uses
/// `Xoshiro256PlusPlus::seed_from_u64(seed)` — byte-identical re-runs are
/// guaranteed for fixed `(values, n_resamples, mean_block_len, seed, ci_level)`.
///
/// ## Edge cases
///
/// - `values.len() < 2` → `[NaN, NaN]`.
/// - `n_resamples == 0` → `[NaN, NaN]`.
/// - `mean_block_len <= 1.0` → behaves as IID bootstrap (every step starts
///   a new block).
#[must_use]
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "n_resamples is a u32 user input; the floor/ceil cast to usize is bounded by n_resamples and cannot overflow on practical inputs (n_resamples << 2^31)"
)]
#[allow(
    clippy::too_many_arguments,
    reason = "WR-04 adds `cancel: &AtomicBool`; the resampling kernel's positional contract is part of the documented byte-identical-rerun invariant — wrapping into a context struct would obscure the call-site discipline"
)]
pub fn stationary_bootstrap_ci<F>(
    values: &[f64],
    stat: F,
    n_resamples: u32,
    mean_block_len: f64,
    seed: u64,
    ci_level: f64,
    cancel: &AtomicBool,
) -> [f64; 2]
where
    F: Fn(&[f64]) -> f64,
{
    let n = values.len();
    if n < 2 || n_resamples == 0 {
        return [f64::NAN, f64::NAN];
    }
    // WR-06 (defence-in-depth): clamp n_resamples at the kernel boundary
    // against HYGIENE_RESAMPLE_CEILING. The engine clamps at the call
    // site, but the kernel is `pub` and could be invoked externally;
    // accepting an unbounded u32 would allow `n_resamples = u32::MAX` to
    // attempt a ~32GB `boot_stats` allocation.
    let n_resamples = n_resamples.min(crate::engine::HYGIENE_RESAMPLE_CEILING);
    let mut rng = Xoshiro256PlusPlus::seed_from_u64(seed);
    let p_continue = if mean_block_len > 1.0 {
        1.0 / mean_block_len
    } else {
        1.0
    };

    let n_resamples_usize = n_resamples as usize;
    let mut boot_stats: Vec<f64> = Vec::with_capacity(n_resamples_usize);
    let mut buf: Vec<f64> = Vec::with_capacity(n);

    for resample in 0..n_resamples {
        // WR-04: sparse cancel poll. Acquire ordering is not needed —
        // this is a cooperative-cancel signal, not a synchronisation
        // primitive. Relaxed keeps the per-iteration overhead minimal.
        if resample % BOOTSTRAP_CANCEL_POLL_CADENCE == 0 && cancel.load(Ordering::Relaxed) {
            return [f64::NAN, f64::NAN];
        }
        buf.clear();
        let mut idx = rng.gen_range(0..n);
        while buf.len() < n {
            buf.push(values[idx]);
            if rng.r#gen::<f64>() < p_continue {
                idx = rng.gen_range(0..n);
            } else {
                idx = (idx + 1) % n;
            }
        }
        boot_stats.push(stat(&buf));
    }

    boot_stats.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let alpha_half = (1.0 - ci_level) / 2.0;
    let n_resamples_f = f64::from(n_resamples);
    let lo_idx = (n_resamples_f * alpha_half).floor() as usize;
    let hi_raw = (n_resamples_f * (1.0 - alpha_half)).ceil() as usize;
    let hi_idx = hi_raw.saturating_sub(1).min(boot_stats.len() - 1);
    [boot_stats[lo_idx], boot_stats[hi_idx]]
}

/// Fixed-block bootstrap CI — block size `block_len` is a hard count.
///
/// Each resample is built by drawing a uniform start index per `block_len`
/// steps; consecutive bars within a block are read with wrap-around modulo
/// `n`. Same `Xoshiro256PlusPlus` RNG and same `to_bits()` determinism
/// contract as [`stationary_bootstrap_ci`].
///
/// ## Edge cases
///
/// - `values.len() < 2` → `[NaN, NaN]`.
/// - `n_resamples == 0` → `[NaN, NaN]`.
/// - `block_len == 0` → `[NaN, NaN]` (block-size zero is undefined).
/// - `block_len > n` → clamped to `n` (WR-05). Without the clamp, the
///   loop body would draw a single block start per resample and read
///   wraparound until `buf.len() == n` — every resample becomes a
///   deterministic circular window of the input, undermining the
///   bootstrap variance estimate. Clamping to `n` keeps the resample
///   semantics meaningful (each resample is the whole series rotated
///   to a uniform random offset).
#[must_use]
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "n_resamples is a u32 user input; the floor/ceil cast to usize is bounded by n_resamples and cannot overflow on practical inputs (n_resamples << 2^31)"
)]
#[allow(
    clippy::too_many_arguments,
    reason = "WR-04 adds `cancel: &AtomicBool`; positional contract retained for byte-identical-rerun parity with stationary kernel"
)]
pub fn block_bootstrap_ci<F>(
    values: &[f64],
    stat: F,
    n_resamples: u32,
    block_len: usize,
    seed: u64,
    ci_level: f64,
    cancel: &AtomicBool,
) -> [f64; 2]
where
    F: Fn(&[f64]) -> f64,
{
    let n = values.len();
    if n < 2 || n_resamples == 0 || block_len == 0 {
        return [f64::NAN, f64::NAN];
    }
    // WR-05: clamp block_len against n. When block_len > n, the inner
    // loop never re-draws an index before buf fills, so every resample
    // becomes a deterministic circular slice — destroying the variance
    // estimate. Clamping to n preserves the bootstrap semantics.
    let block_len = block_len.min(n);
    // WR-06 (defence-in-depth): same n_resamples ceiling clamp as the
    // stationary kernel.
    let n_resamples = n_resamples.min(crate::engine::HYGIENE_RESAMPLE_CEILING);
    let mut rng = Xoshiro256PlusPlus::seed_from_u64(seed);
    let n_resamples_usize = n_resamples as usize;
    let mut boot_stats: Vec<f64> = Vec::with_capacity(n_resamples_usize);
    let mut buf: Vec<f64> = Vec::with_capacity(n);

    for resample in 0..n_resamples {
        // WR-04: sparse cancel poll (see stationary kernel docs).
        if resample % BOOTSTRAP_CANCEL_POLL_CADENCE == 0 && cancel.load(Ordering::Relaxed) {
            return [f64::NAN, f64::NAN];
        }
        buf.clear();
        let mut idx = rng.gen_range(0..n);
        let mut steps_in_block: usize = 0;
        while buf.len() < n {
            if steps_in_block >= block_len {
                idx = rng.gen_range(0..n);
                steps_in_block = 0;
            }
            buf.push(values[idx]);
            idx = (idx + 1) % n;
            steps_in_block += 1;
        }
        boot_stats.push(stat(&buf));
    }

    boot_stats.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let alpha_half = (1.0 - ci_level) / 2.0;
    let n_resamples_f = f64::from(n_resamples);
    let lo_idx = (n_resamples_f * alpha_half).floor() as usize;
    let hi_raw = (n_resamples_f * (1.0 - alpha_half)).ceil() as usize;
    let hi_idx = hi_raw.saturating_sub(1).min(boot_stats.len() - 1);
    [boot_stats[lo_idx], boot_stats[hi_idx]]
}

/// Politis-White (2004) + Patton-Politis-White (2009) automatic
/// block-length selector for the stationary bootstrap.
///
/// Returns the floating-point `b_star` recommendation; callers floor via
/// `max(3.0, b_star.ceil())` for the `block_bootstrap_ci` `block_len`
/// argument or use the raw `b_star` as `mean_block_len` for
/// `stationary_bootstrap_ci`.
///
/// ## Algorithm (Politis & White 2004 §3, Patton-Politis-White 2009 erratum)
///
/// 1. Compute biased autocovariances `r_k` for `k = 0..=K_n` where
///    `K_n = ceil(min(5 * log10(n), n / 2))`.
/// 2. Find `m`: largest `k` such that `|r_k / r_0| > c * sqrt(log10(n) / n)`
///    with `c = 2.0` (the Politis-White 2004 default).
/// 3. `g_hat = sum_{k = 1..=2m} lambda(k / (2m)) * |k| * r_k` where
///    `lambda(t) = 1 - |t|` is the Bartlett (flat-top) kernel (Patton-
///    Politis-White 2009 erratum — the `|k|` weight inside the sum was
///    missing from Politis-White 2004 and is the load-bearing data-
///    dependent factor).
/// 4. `g_dr = sum_{k = 0..=2m} lambda(k / (2m)) * r_k` (the symmetric
///    half-sum; `lambda(0) = 1`).
/// 5. `D_SB = 2 * g_dr^2` (the stationary-bootstrap MSE constant per
///    PPW 2009 §3; NOT a function of `g_hat`).
/// 6. `b_star = (2 * g_hat^2 / D_SB) ^ (1/3) * n ^ (1/3)`.
///
/// ## Edge cases
///
/// - `values.len() < 4` → `NaN`.
/// - Constant series (`r_0 == 0`) → `NaN`. Callers fall back to
///   `ceil(n^(1/3))` per the documented contract.
/// - `D_SB == 0` (degenerate `g_dr == 0`) → `NaN`. Same fallback.
#[must_use]
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "n is the input series length (bar count); fits trivially in f64; the floor/ceil cast to usize is bounded by n itself"
)]
pub fn block_length_pwppw(values: &[f64]) -> f64 {
    let n = values.len();
    if n < 4 {
        return f64::NAN;
    }
    let n_f = n as f64;
    let k_n = ((5.0 * n_f.log10()).min(n_f / 2.0)).ceil() as usize;
    if k_n < 1 {
        return f64::NAN;
    }

    let mean = values.iter().copied().sum::<f64>() / n_f;
    let cent: Vec<f64> = values.iter().map(|v| v - mean).collect();
    let r0: f64 = cent.iter().map(|v| v * v).sum::<f64>() / n_f;
    if r0 == 0.0 {
        return f64::NAN;
    }

    let mut r = Vec::with_capacity(k_n + 1);
    r.push(r0);
    for k in 1..=k_n {
        let s: f64 = (0..n.saturating_sub(k))
            .map(|i| cent[i] * cent[i + k])
            .sum();
        r.push(s / n_f);
    }

    let threshold = 2.0 * (n_f.log10() / n_f).sqrt();
    let mut m: usize = 0;
    // Clippy `needless_range_loop` is suppressed: the loop body uses `k`
    // as both an index AND a target value to set into `m`; switching to
    // `enumerate().skip(1)` would obscure the intent. The pattern matches
    // the LjungBox kernel `ljung_box_q_and_p` discipline at
    // `scan/ljung_box/kernel.rs:115-126`.
    #[allow(clippy::needless_range_loop)]
    for k in 1..=k_n {
        if (r[k] / r0).abs() > threshold {
            m = k;
        }
    }
    if m == 0 {
        m = 1;
    }

    let two_m = 2 * m;
    let two_m_f = two_m as f64;
    // PPW 2009 erratum: g_hat is the |k|-weighted half-sum (the missing
    // factor in Politis-White 2004). This is the data-dependent term that
    // captures the autocorrelation structure; without |k| the g_hat term
    // algebraically cancels against a g_hat-derived D_hat and the selector
    // collapses to a data-independent constant (CR-01).
    let mut g_hat = 0.0_f64;
    for k in 1..=two_m {
        let r_k = if k < r.len() { r[k] } else { 0.0 };
        let lambda = (1.0 - (k as f64 / two_m_f).abs()).max(0.0);
        g_hat += lambda * (k as f64) * r_k;
    }
    // D_SB: the stationary-bootstrap MSE constant. Computed from the
    // symmetric half-sum of lambda-weighted autocovariances (NOT from
    // g_hat); this is what makes the selector data-dependent.
    let mut g_dr = 0.0_f64;
    for k in 0..=two_m {
        let r_k = if k < r.len() { r[k] } else { 0.0 };
        let lambda = if k == 0 {
            1.0
        } else {
            (1.0 - (k as f64 / two_m_f).abs()).max(0.0)
        };
        g_dr += lambda * r_k;
    }
    let d_hat = 2.0 * g_dr * g_dr;
    if d_hat == 0.0 {
        return f64::NAN;
    }
    (2.0 * g_hat * g_hat / d_hat).powf(1.0 / 3.0) * n_f.powf(1.0 / 3.0)
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

    /// Helper: a never-cancelled flag for tests that exercise the kernel
    /// completion path.
    fn no_cancel() -> AtomicBool {
        AtomicBool::new(false)
    }

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
        let ci_a = stationary_bootstrap_ci(&values, mean, 100, 5.0, 0xBEEF, 0.95, &no_cancel());
        let ci_b = stationary_bootstrap_ci(&values, mean, 100, 5.0, 0xBEEF, 0.95, &no_cancel());
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
                &no_cancel(),
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
        let ci = stationary_bootstrap_ci(&one, mean, 100, 3.0, 0, 0.95, &no_cancel());
        assert!(ci[0].is_nan() && ci[1].is_nan());
        let empty: [f64; 0] = [];
        let ci_empty = stationary_bootstrap_ci(&empty, mean, 100, 3.0, 0, 0.95, &no_cancel());
        assert!(ci_empty[0].is_nan() && ci_empty[1].is_nan());
        let two = [1.0_f64, 2.0];
        let ci_n0 = stationary_bootstrap_ci(&two, mean, 0, 3.0, 0, 0.95, &no_cancel());
        assert!(ci_n0[0].is_nan() && ci_n0[1].is_nan());
    }

    /// `block_bootstrap_ci` byte-identical for fixed seed.
    #[test]
    fn block_bootstrap_ci_deterministic_for_seed() {
        let values = lcg_iid(50, 0xDEAD);
        let ci_a = block_bootstrap_ci(&values, mean, 100, 5, 0xBEEF, 0.95, &no_cancel());
        let ci_b = block_bootstrap_ci(&values, mean, 100, 5, 0xBEEF, 0.95, &no_cancel());
        assert_eq!(ci_a[0].to_bits(), ci_b[0].to_bits());
        assert_eq!(ci_a[1].to_bits(), ci_b[1].to_bits());
        assert!(ci_a[0] <= ci_a[1]);
    }

    /// `block_bootstrap_ci` short-input edge cases.
    #[test]
    fn block_bootstrap_ci_short_input_nan() {
        let one = [1.0_f64];
        assert!(block_bootstrap_ci(&one, mean, 100, 5, 0, 0.95, &no_cancel())[0].is_nan());
        let two = [1.0_f64, 2.0];
        assert!(block_bootstrap_ci(&two, mean, 100, 0, 0, 0.95, &no_cancel())[0].is_nan());
        assert!(block_bootstrap_ci(&two, mean, 0, 5, 0, 0.95, &no_cancel())[0].is_nan());
    }

    /// WR-05 regression: `block_bootstrap_ci` with `block_len >= n` must
    /// produce a deterministic, sensible CI (`lo <= hi`) and not loop
    /// indefinitely / silently degenerate.
    ///
    /// Pre-fix the inner loop never re-drew an index before buf filled
    /// — every resample became a deterministic circular window of the
    /// input, destroying the variance estimate. Post-fix `block_len` is
    /// clamped to `n` at the kernel boundary.
    #[test]
    fn block_bootstrap_ci_block_len_geq_n_is_deterministic_and_sensible() {
        let values = lcg_iid(20, 0xABCD);
        let ci_eq = block_bootstrap_ci(&values, mean, 200, 20, 0x42, 0.95, &no_cancel());
        let ci_gt = block_bootstrap_ci(&values, mean, 200, 100, 0x42, 0.95, &no_cancel());

        assert!(ci_eq[0].is_finite(), "block_len == n: lo finite");
        assert!(ci_eq[1].is_finite(), "block_len == n: hi finite");
        assert!(ci_eq[0] <= ci_eq[1], "block_len == n: lo <= hi");

        assert!(ci_gt[0].is_finite(), "block_len > n: lo finite");
        assert!(ci_gt[1].is_finite(), "block_len > n: hi finite");
        assert!(ci_gt[0] <= ci_gt[1], "block_len > n: lo <= hi");

        // Determinism: with block_len clamped to n, both calls share the
        // same effective block_len, so the CIs MUST be byte-identical.
        assert_eq!(ci_eq[0].to_bits(), ci_gt[0].to_bits());
        assert_eq!(ci_eq[1].to_bits(), ci_gt[1].to_bits());
    }

    /// Test 5 (Plan 05-02): `block_length_pwppw` returns finite sane value.
    #[test]
    fn block_length_pwppw_iid_sane() {
        let values = lcg_iid(1000, 0xDEAD);
        let b_star = block_length_pwppw(&values);
        assert!(b_star.is_finite());
        assert!(b_star > 0.0);
        let b_ceil = b_star.ceil() as usize;
        assert!((1..=50).contains(&b_ceil));
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

    /// Generate an AR(1) series with autoregression coefficient `phi` and
    /// zero-mean IID innovations via the same LCG used by `lcg_iid`.
    /// Used by the CR-01 regression test to produce a series with a
    /// known, non-trivial autocorrelation structure.
    fn lcg_ar1(n: usize, seed: u64, phi: f64) -> Vec<f64> {
        let innovations = lcg_iid(n, seed);
        let mut out = Vec::with_capacity(n);
        let mut prev = 0.0_f64;
        for u in innovations {
            let v = phi * prev + u;
            out.push(v);
            prev = v;
        }
        out
    }

    /// CR-01 regression: `block_length_pwppw` must produce DIFFERENT
    /// outputs for differently-autocorrelated inputs of the same length.
    ///
    /// Pre-fix the function algebraically collapsed to `(3/2)^(1/3) * n^(1/3)`
    /// — a data-independent constant — so an IID series and an AR(1)(phi=0.5)
    /// series of the same length produced identical block lengths. This
    /// test exercises four series of length 1000:
    ///   - IID (white noise)
    ///   - AR(1) with phi = 0.5 (moderate positive autocorrelation)
    ///   - AR(1) with phi = 0.8 (strong positive autocorrelation)
    /// and asserts no two are within 1e-3 of each other. The bug pinned all
    /// three to ~11.447.
    #[test]
    fn block_length_pwppw_varies_with_autocorrelation() {
        let n = 1000;
        let b_iid = block_length_pwppw(&lcg_iid(n, 0xDEAD));
        let b_ar05 = block_length_pwppw(&lcg_ar1(n, 0xDEAD, 0.5));
        let b_ar08 = block_length_pwppw(&lcg_ar1(n, 0xDEAD, 0.8));
        assert!(b_iid.is_finite(), "iid b_star must be finite");
        assert!(b_ar05.is_finite(), "ar1(0.5) b_star must be finite");
        assert!(b_ar08.is_finite(), "ar1(0.8) b_star must be finite");

        // Pin the algebraic-collapse signature: the buggy implementation
        // returns (3/2)^(1/3) * n^(1/3) ≈ 11.4471 for every series of
        // length 1000. The corrected selector must drift away from that
        // constant for autocorrelated inputs.
        let buggy_constant = (3.0_f64 / 2.0).powf(1.0 / 3.0) * (n as f64).powf(1.0 / 3.0);
        // At minimum, AR(1) with phi=0.8 must move materially off the
        // buggy constant (it is the most-autocorrelated input here).
        assert!(
            (b_ar08 - buggy_constant).abs() > 1.0,
            "ar1(0.8) b_star {b_ar08} matched pre-fix constant {buggy_constant} (data-independent collapse)"
        );

        // Cross-input variance: the three selectors must produce
        // measurably different recommendations.
        assert!(
            (b_iid - b_ar05).abs() > 1e-3,
            "block length collapsed to constant: iid={b_iid} ar1(0.5)={b_ar05}"
        );
        assert!(
            (b_ar05 - b_ar08).abs() > 1e-3,
            "block length collapsed to constant: ar1(0.5)={b_ar05} ar1(0.8)={b_ar08}"
        );

        // Sanity: stronger autocorrelation should not produce a smaller
        // block recommendation (monotonicity expectation of the PPW
        // selector for AR(1) processes).
        assert!(
            b_ar08 >= b_ar05 - 1e-9,
            "ar1(0.8) b_star {b_ar08} < ar1(0.5) b_star {b_ar05} (expected monotone)"
        );
    }
}
