//! `stats.meanrev.ou_halflife@1` kernel — single-leg OU half-life scan.
//!
//! Thin wrapper over the shared
//! [`crate::scan::primitives::ar1::ou_ar1_fit`] AR(1) / Ornstein-Uhlenbeck
//! mean-reversion primitive (RAD-3627). This kernel only selects the basis the
//! AR(1) is fitted on — the price LEVEL series (default) or its log RETURNS —
//! and forwards to the primitive. The regression and the `half_life`/`λ`/
//! sentinel math are NOT duplicated here (D4-06 / 04-PATTERNS.md Pitfall 9).
//!
//! See the primitive for the method and the `f64::INFINITY` half-life sentinel
//! convention (returned when the series is not mean-reverting, `φ >= 1`).

use crate::scan::primitives::ar1::{Ar1Fit, ou_ar1_fit};
use crate::scan::primitives::returns::log_returns;

/// Basis the AR(1) is fitted on, selected by the `on` param.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeriesBasis {
    /// `"level"` — fit on the raw close-price level series (the v1 default).
    Level,
    /// `"returns"` — fit on the log-returns of the close series
    /// (`r_t = ln(c_t / c_{t-1})`, length `n - 1`).
    Returns,
}

impl SeriesBasis {
    /// Stable wire label for the `on` param (echoed into `effect.extra`).
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            SeriesBasis::Level => "level",
            SeriesBasis::Returns => "returns",
        }
    }
}

/// Fit the OU / AR(1) mean-reversion model on the chosen basis of `closes`.
///
/// - [`SeriesBasis::Level`] fits directly on `closes`.
/// - [`SeriesBasis::Returns`] fits on `log_returns(closes)` (length `n - 1`).
///
/// Returns the shared [`Ar1Fit`] (ρ, φ, DF t-stat, λ, half-life, nobs); the
/// `half_life` is `f64::INFINITY` when the basis series is not mean-reverting.
#[inline]
#[must_use]
pub fn ou_halflife(closes: &[f64], on: SeriesBasis) -> Ar1Fit {
    match on {
        SeriesBasis::Level => ou_ar1_fit(closes),
        SeriesBasis::Returns => {
            let rets = log_returns(closes);
            ou_ar1_fit(&rets)
        }
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn basis_label_round_trip() {
        assert_eq!(SeriesBasis::Level.label(), "level");
        assert_eq!(SeriesBasis::Returns.label(), "returns");
    }

    /// Level basis on an exact AR(1) decay-to-mean (φ = 0.5) ⇒ half-life 1.0.
    #[test]
    fn level_basis_recovers_known_half_life() {
        let phi = 0.5_f64;
        let series: Vec<f64> = (0..40).map(|t| 1.5 + phi.powi(t) * 0.5).collect();
        let fit = ou_halflife(&series, SeriesBasis::Level);
        assert!(
            (fit.half_life - 1.0).abs() < 1e-6,
            "half_life = {} expected ~1.0",
            fit.half_life
        );
    }

    /// Returns basis fits on n-1 log returns; an i.i.d.-ish level series has
    /// white-noise returns (strongly anti-persistent) — the call must not
    /// panic and produces a defined fit (finite or INFINITY sentinel).
    #[test]
    fn returns_basis_runs_on_n_minus_one() {
        let series: Vec<f64> = (0..40).map(|t| 1.0 + 0.01 * f64::from(t)).collect();
        let fit = ou_halflife(&series, SeriesBasis::Returns);
        // nobs = (n-1 returns) - 1 = n - 2.
        assert_eq!(fit.nobs, series.len() - 2);
        assert!(fit.half_life.is_finite() || fit.half_life.is_infinite());
    }
}
