//! CROSS (cross-instrument / two-leg) scan family namespace.
//!
//! Houses the 4 CROSS scans rolled out by Plan 04-07: rolling Pearson/Spearman
//! correlation, rolling OLS regression, lead-lag CCF, and Engle-Granger
//! cointegration. Every CROSS scan declares `ScanArity::Pair` (D4-02) and
//! consumes [`crate::scan::primitives::time_alignment::inner_join`] to
//! align two `BarFrame`s on common `ts_open_utc`. The CROSS-01 inner-join
//! primitive + the D4-04 manifest-intersection helper both live in
//! `primitives::time_alignment` (PATTERNS.md Pattern I home decision).
//!
//! [`register_cross_scans`] is the SOLE registration path for this family
//! (Pattern E / Plan 04-02 contract): Plan 04-07 appends
//! `r.register(Box::new(<NewScan>));` lines INSIDE this function —
//! alphabetical by scan-id — and never touches `registry::bootstrap`.

use super::Registry;

pub mod corr_rolling;
pub mod lead_lag;
pub mod ols_rolling;

pub use corr_rolling::{PearsonRollingScan, SpearmanRollingScan};
pub use lead_lag::LeadLagCcfScan;
pub use ols_rolling::OlsRollingScan;

/// Register every CROSS scan into the supplied [`Registry`]. Plan 04-07
/// appended three `r.register(...)` lines (alphabetical by scan-id):
/// `cross.corr.pearson_rolling`, `cross.corr.spearman_rolling`, and
/// `cross.ols.rolling`. Plan 04-08 (Wave 4) appends two more, alphabetical
/// by scan-id: `cross.lead_lag.ccf` (after `cross.corr.*` and after
/// `cross.ols.rolling`). Plan 04-08 Task 2 will also append
/// `cross.cointegration.engle_granger` (sorts BEFORE the corr_rolling
/// pair). Plans never touch `registry::bootstrap`.
pub fn register_cross_scans(r: &mut Registry) {
    r.register(Box::new(PearsonRollingScan));
    r.register(Box::new(SpearmanRollingScan));
    r.register(Box::new(OlsRollingScan));
    r.register(Box::new(LeadLagCcfScan));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Plan 04-07 — `register_cross_scans` registers three Pair-arity
    /// rolling scans (Pearson + Spearman correlation, and OLS regression).
    /// Plan 04-08 Task 1 adds CROSS-04 `cross.lead_lag.ccf`. Subsequent
    /// Phase-4 plans extend this helper with further scans.
    #[test]
    fn register_cross_scans_includes_pearson_spearman_ols_and_lead_lag() {
        let mut r = Registry::new();
        let before = r.scans.len();
        register_cross_scans(&mut r);
        let added = r.scans.len() - before;
        assert!(added >= 4, "expected >= 4 CROSS scans registered; got {added}");
        assert!(
            r.get("cross.corr.pearson_rolling", 1).is_some(),
            "cross.corr.pearson_rolling@1 must be registered"
        );
        assert!(
            r.get("cross.corr.spearman_rolling", 1).is_some(),
            "cross.corr.spearman_rolling@1 must be registered"
        );
        assert!(
            r.get("cross.ols.rolling", 1).is_some(),
            "cross.ols.rolling@1 must be registered"
        );
        assert!(
            r.get("cross.lead_lag.ccf", 1).is_some(),
            "cross.lead_lag.ccf@1 must be registered"
        );
    }
}
