//! Pure squared-returns kernel for ANOM-04 (squared variant).
//!
//! Pattern analog: `crates/miner-core/src/scan/ljung_box/kernel.rs` — private
//! `#[inline]` pure functions on `&[f64]` with a sibling `#[cfg(test)] mod
//! tests` block. No IO, no `serde_json`, no `Reader` calls.
//!
//! ## Why squared returns?
//!
//! Ljung-Box on RETURNS tests serial correlation in the level (mean) process.
//! Ljung-Box on SQUARED returns tests serial correlation in the variance
//! process — i.e., GARCH-style volatility clustering. The kernel and Q-stat
//! computation are identical to the level-variant; only the input preprocessing
//! differs (`returns -> returns.iter().map(|r| r*r).collect()`).
//!
//! The Q-stat + p-value computation lives in the sibling Phase 3 kernel
//! `crate::scan::ljung_box::kernel::{biased_acf, ljung_box_q_and_p}` whose
//! visibility was widened from `pub(super)` to `pub(crate)` in this plan so
//! the squared variant can call them.

#![cfg_attr(any(test, debug_assertions), allow(clippy::float_cmp))]

/// Element-wise square of a returns slice. Used to transform a log-returns
/// vector into the squared-returns input required by the Ljung-Box on
/// squared returns (ANOM-04 variant).
///
/// Returns a `Vec<f64>` of the same length as `returns`. Empty input returns
/// an empty `Vec`. Pure / allocation-only function — no IO.
#[inline]
#[must_use]
pub(super) fn square_returns(returns: &[f64]) -> Vec<f64> {
    returns.iter().map(|r| r * r).collect()
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

    #[test]
    fn square_returns_basic() {
        // [1.0, -2.0, 3.0] -> [1.0, 4.0, 9.0].
        let r = square_returns(&[1.0_f64, -2.0, 3.0]);
        assert_eq!(r.len(), 3);
        assert!(approx_eq(r[0], 1.0, TOL));
        assert!(approx_eq(r[1], 4.0, TOL));
        assert!(approx_eq(r[2], 9.0, TOL));
    }

    #[test]
    fn square_returns_empty() {
        let r = square_returns(&[]);
        assert!(r.is_empty());
    }

    #[test]
    fn square_returns_singleton() {
        let r = square_returns(&[2.5_f64]);
        assert_eq!(r.len(), 1);
        assert!(approx_eq(r[0], 6.25, TOL));
    }

    #[test]
    fn square_returns_zeros() {
        // Zeros stay zero; squared returns of a constant-price series are
        // all zeros (since log_returns are zero for constant input).
        let r = square_returns(&[0.0_f64; 5]);
        assert_eq!(r.len(), 5);
        for v in r {
            assert!(approx_eq(v, 0.0, TOL));
        }
    }

    #[test]
    fn square_returns_length_invariant() {
        let xs: Vec<f64> = (1..=10).map(|i| i as f64).collect();
        let r = square_returns(&xs);
        assert_eq!(r.len(), xs.len());
    }

    #[test]
    fn square_returns_sign_invariant() {
        // Squaring drops sign: square(x) == square(-x).
        let pos = square_returns(&[1.0_f64, 2.0, 3.0]);
        let neg = square_returns(&[-1.0_f64, -2.0, -3.0]);
        for (a, b) in pos.iter().zip(neg.iter()) {
            assert!(approx_eq(*a, *b, TOL));
        }
    }
}
