//! ANOM (anomaly / single-leg statistical) scan family namespace.
//!
//! Houses the 11 ANOM scans rolled out by Plans 04-03..04-06 (returns,
//! summary stats, rolling volatility, squared-returns Ljung-Box, ADF, KPSS,
//! variance ratio, ARCH-LM, Jarque-Bera, outliers, drawdown). Each scan lives
//! in its own submodule with the standard `mod.rs` + `kernel.rs` split
//! (PATTERNS.md Pattern A / B).
//!
//! [`register_anom_scans`] is the SOLE registration path for this family
//! (Pattern E / Plan 04-02 contract): Plans 04-03..04-10 append
//! `r.register(Box::new(<NewScan>));` lines INSIDE this function — alphabetical
//! by scan-id — and never touch `crates/miner-core/src/scan/registry.rs`'s
//! `bootstrap()` body. The per-family registrar pattern keeps the registration
//! diffs scoped and parallelisable across the 22-scan rollout.

use super::Registry;

pub mod adf;
pub mod drawdown;
pub mod kpss;
pub mod ljung_box_sq;
pub mod outliers;
pub mod returns;
pub mod summary;
pub mod vol;

pub use adf::AdfScan;
pub use drawdown::DrawdownProfileScan;
pub use kpss::KpssScan;
pub use ljung_box_sq::LjungBoxSqScan;
pub use outliers::OutliersZAndMadScan;
pub use returns::ReturnsProfileScan;
pub use summary::SummaryWelfordScan;
pub use vol::VolRollingScan;

/// Register every ANOM scan into the supplied [`Registry`]. Plan 04-03
/// (Wave 3) registered ANOM-01 (`stats.returns.profile`), ANOM-02
/// (`stats.summary.welford`), and ANOM-03 (`stats.vol.rolling`). Plan 04-04
/// (Wave 4) added ANOM-04 squared variant (`stats.autocorr.ljung_box_sq`),
/// ANOM-10 (`stats.outliers.z_and_mad`), and ANOM-11
/// (`stats.drawdown.profile`). Subsequent plans (04-05..04-06) append further
/// `r.register(...)` lines here alphabetical by scan-id. Plans never modify
/// the central `registry::bootstrap` body.
pub fn register_anom_scans(r: &mut Registry) {
    // Alphabetical by scan-id:
    //   stats.autocorr.ljung_box_sq   <- Plan 04-04
    //   stats.drawdown.profile        <- Plan 04-04
    //   stats.outliers.z_and_mad      <- Plan 04-04
    //   stats.returns.profile         <- Plan 04-03
    //   stats.stationarity.adf        <- Plan 04-05
    //   stats.stationarity.kpss       <- Plan 04-05
    //   stats.summary.welford         <- Plan 04-03
    //   stats.vol.rolling             <- Plan 04-03
    r.register(Box::new(LjungBoxSqScan));
    r.register(Box::new(DrawdownProfileScan));
    r.register(Box::new(OutliersZAndMadScan));
    r.register(Box::new(ReturnsProfileScan));
    r.register(Box::new(AdfScan));
    r.register(Box::new(KpssScan));
    r.register(Box::new(SummaryWelfordScan));
    r.register(Box::new(VolRollingScan));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `register_anom_scans` registers the ANOM scans rolled out through
    /// Plan 04-04 (ANOM-01, ANOM-02, ANOM-03, ANOM-04 squared, ANOM-10,
    /// ANOM-11). Plan 04-11 tightens this to a full count assertion across
    /// the complete catalogue.
    #[test]
    fn register_anom_scans_registers_phase4_scans_through_plan_04() {
        let mut r = Registry::new();
        register_anom_scans(&mut r);
        assert!(
            r.get("stats.autocorr.ljung_box_sq", 1).is_some(),
            "ANOM-04 squared"
        );
        assert!(
            r.get("stats.drawdown.profile", 1).is_some(),
            "ANOM-11 drawdown"
        );
        assert!(
            r.get("stats.outliers.z_and_mad", 1).is_some(),
            "ANOM-10 outliers"
        );
        assert!(r.get("stats.returns.profile", 1).is_some(), "ANOM-01");
        assert!(r.get("stats.stationarity.adf", 1).is_some(), "ANOM-05");
        assert!(r.get("stats.stationarity.kpss", 1).is_some(), "ANOM-06");
        assert!(r.get("stats.summary.welford", 1).is_some(), "ANOM-02");
        assert!(r.get("stats.vol.rolling", 1).is_some(), "ANOM-03");
    }
}
