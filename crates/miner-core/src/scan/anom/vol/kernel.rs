//! Pure rolling-vol kernel for ANOM-03 — `rolling_std`, `vol_of_vol`.
//!
//! Pattern analog: `crates/miner-core/src/scan/ljung_box/kernel.rs` (Phase 3
//! gold-standard / `04-PATTERNS.md` Pattern B).
//!
//! ## Implementation notes
//!
//! - `rolling_std` is naïve O(n*window). The rolling-Welford incremental
//!   variant is numerically tricky and ANOM-03 is NOT on the hot path of
//!   the throughput claims (Phase 7 may revisit if profiling demands it).
//! - Sample standard deviation uses ddof=1 to match
//!   `pandas.Series.rolling(W).std(ddof=1)` (RESEARCH §Section 2 reference).
//! - Constant-input window → std = 0.0 (no NaN).
//! - Sequential summation order pins cross-platform determinism (Pitfall 4).

#![cfg_attr(any(test, debug_assertions), allow(clippy::float_cmp))]

/// Rolling standard deviation over a fixed-size window. Returns a `Vec<f64>`
/// of length `values.len() - window + 1` where `out[i]` is the ddof=1 std
/// of `values[i..i+window]`. Constant-window branch yields `0.0`.
///
/// # Panics
/// Panics via `debug_assert` when:
/// - `window < 2`
/// - `window > values.len()`
#[inline]
#[allow(
    clippy::cast_precision_loss,
    reason = "window is bounded above by values.len() (a bar count); fits trivially in f64's 52-bit mantissa"
)]
pub(super) fn rolling_std(values: &[f64], window: usize) -> Vec<f64> {
    debug_assert!(window >= 2, "rolling_std: window must be >= 2; got {window}");
    debug_assert!(
        window <= values.len(),
        "rolling_std: window ({window}) must be <= values.len() ({})",
        values.len()
    );
    let n_out = values.len() - window + 1;
    let mut out = Vec::with_capacity(n_out);
    let w_f = window as f64;
    for i in 0..n_out {
        let slice = &values[i..i + window];
        // Two-pass mean + sum of squared deviations.
        let sum: f64 = slice.iter().copied().sum();
        let mean = sum / w_f;
        let mut sq_dev = 0.0_f64;
        for v in slice {
            let d = v - mean;
            sq_dev += d * d;
        }
        let var = sq_dev / (w_f - 1.0);
        out.push(var.sqrt());
    }
    out
}

/// Vol-of-vol: rolling std applied to a pre-computed rolling-std series.
/// Returns a `Vec<f64>` of length `rolling_vols.len() - window + 1` (empty
/// when `rolling_vols.len() < window`).
///
/// # Panics
/// Panics via `debug_assert` when `window < 2`.
#[inline]
pub(super) fn vol_of_vol(rolling_vols: &[f64], window: usize) -> Vec<f64> {
    debug_assert!(window >= 2, "vol_of_vol: window must be >= 2; got {window}");
    if rolling_vols.len() < window {
        return Vec::new();
    }
    rolling_std(rolling_vols, window)
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

    // -----------------------------------------------------------------------
    // rolling_std
    // -----------------------------------------------------------------------

    #[test]
    fn rolling_std_constant_input_is_zero() {
        let v = [3.0_f64; 8];
        let out = rolling_std(&v, 3);
        assert_eq!(out.len(), 6);
        for x in out {
            assert!(approx_eq(x, 0.0, TOL), "{x}");
        }
    }

    #[test]
    fn rolling_std_known_input_window_2() {
        // [1, 3, 5, 7, 9] with window=2 -> each window has std (ddof=1) =
        // sqrt(((x1-mean)^2 + (x2-mean)^2) / 1) = |x2-x1|/sqrt(2) (since
        // (x1-mean) = -delta/2, (x2-mean) = +delta/2 -> 2*(delta/2)^2 =
        // delta^2/2, var = delta^2/2 / 1; no — divide by (w-1) = 1 not 2).
        // Wait: mean = (x1+x2)/2; sq_dev = ((x1-x2)/2)^2 + ((x2-x1)/2)^2 =
        // 2 * (delta/2)^2 = delta^2/2. var (ddof=1) = (delta^2/2) / 1.
        // std = delta/sqrt(2). For delta=2: std = sqrt(2).
        let v = [1.0_f64, 3.0, 5.0, 7.0, 9.0];
        let out = rolling_std(&v, 2);
        assert_eq!(out.len(), 4);
        let expected = 2.0_f64.sqrt();
        for (i, x) in out.iter().enumerate() {
            assert!(approx_eq(*x, expected, TOL), "out[{i}]={x}");
        }
    }

    #[test]
    fn rolling_std_length_invariant() {
        let v = (0..10).map(|i| i as f64).collect::<Vec<_>>();
        assert_eq!(rolling_std(&v, 3).len(), 8);
        assert_eq!(rolling_std(&v, 5).len(), 6);
        assert_eq!(rolling_std(&v, 10).len(), 1);
    }

    #[test]
    #[should_panic(expected = "rolling_std: window must be >= 2")]
    fn rolling_std_window_one_panics() {
        let _ = rolling_std(&[1.0_f64, 2.0, 3.0], 1);
    }

    #[test]
    #[should_panic(expected = "rolling_std: window")]
    fn rolling_std_window_too_large_panics() {
        let _ = rolling_std(&[1.0_f64, 2.0], 5);
    }

    // -----------------------------------------------------------------------
    // vol_of_vol
    // -----------------------------------------------------------------------

    #[test]
    fn vol_of_vol_empty_when_input_shorter_than_window() {
        let vols = [0.5_f64, 0.6];
        let out = vol_of_vol(&vols, 3);
        assert!(out.is_empty());
    }

    #[test]
    fn vol_of_vol_known_input() {
        // vols = [0.1, 0.2, 0.3, 0.4]; window = 2.
        // window 0 [0.1, 0.2]: std = 0.1 / sqrt(2)
        // window 1 [0.2, 0.3]: std = 0.1 / sqrt(2)
        // window 2 [0.3, 0.4]: std = 0.1 / sqrt(2)
        let vols = [0.1_f64, 0.2, 0.3, 0.4];
        let out = vol_of_vol(&vols, 2);
        assert_eq!(out.len(), 3);
        let expected = 0.1_f64 / 2.0_f64.sqrt();
        for (i, v) in out.iter().enumerate() {
            assert!(approx_eq(*v, expected, TOL), "vov[{i}]={v}");
        }
    }
}
