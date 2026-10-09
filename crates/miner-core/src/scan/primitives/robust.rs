//! Robust order-statistic primitives shared across break-detection scans
//! (RAD-3841).
//!
//! The median is the shared building block of two otherwise-unrelated
//! structural-break detectors:
//!
//! - [`crate::scan::cross::cointegration_rolling`] (CROSS-06, RAD-3626) uses the
//!   trailing median of `|beta|` as the *baseline* its beta-drift breakdown
//!   band is measured against (a robust center that a single drifting window
//!   cannot move).
//! - [`crate::scan::anom::cusum_break`] (RAD-3841) reports the per-segment
//!   median alongside the mean/std as the robust pre/post segment center — the
//!   median is resistant to the very level/vol shift the CUSUM statistic is
//!   detecting, so it characterises a regime better than the mean on the
//!   heavy-tailed return series these scans run over.
//!
//! Factored here (per the RAD-3841 "do not duplicate" reuse contract) so the
//! median is defined ONCE; the cointegration kernel's previously-private copy
//! now delegates to this module with byte-identical behaviour.
//!
//! Discipline (04-PATTERNS.md Pattern B): `#[inline] pub fn` over primitive
//! slice types, no IO, no allocation-order-dependent output, `total_cmp` for a
//! NaN-safe total order.

/// Median of a slice, sorting it in place. Empty slice ⇒ `NaN`. Even length ⇒
/// the mean of the two central order statistics. Total order via
/// [`f64::total_cmp`] so the result is deterministic and NaN-safe.
///
/// This is the byte-identical move of the cointegration kernel's previously
/// private `median_in_place` (RAD-3626) — same algorithm, single home.
#[inline]
#[must_use]
pub fn median_in_place(vals: &mut [f64]) -> f64 {
    let m = vals.len();
    if m == 0 {
        return f64::NAN;
    }
    vals.sort_by(f64::total_cmp);
    if m % 2 == 1 {
        vals[m / 2]
    } else {
        (vals[m / 2 - 1] + vals[m / 2]) / 2.0
    }
}

/// Median of a slice without mutating the caller's data (clones into a scratch
/// buffer, then defers to [`median_in_place`]). Empty slice ⇒ `NaN`.
#[inline]
#[must_use]
pub fn median(vals: &[f64]) -> f64 {
    let mut scratch = vals.to_vec();
    median_in_place(&mut scratch)
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    /// `median_in_place` matches hand-computed values for odd/even lengths and
    /// the empty-slice `NaN` contract (moved from the cointegration kernel).
    #[test]
    fn median_in_place_odd_even() {
        let mut odd = [3.0, 1.0, 2.0];
        assert_eq!(median_in_place(&mut odd), 2.0);
        let mut even = [4.0, 1.0, 3.0, 2.0];
        assert_eq!(median_in_place(&mut even), 2.5);
        let mut empty: [f64; 0] = [];
        assert!(median_in_place(&mut empty).is_nan());
    }

    /// `median` leaves the caller's slice untouched.
    #[test]
    fn median_does_not_mutate_input() {
        let data = [3.0, 1.0, 2.0];
        let m = median(&data);
        assert_eq!(m, 2.0);
        assert_eq!(data, [3.0, 1.0, 2.0], "input must not be reordered");
    }
}
