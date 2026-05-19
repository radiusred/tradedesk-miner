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

pub use corr_rolling::{PearsonRollingScan, SpearmanRollingScan};

/// Register every CROSS scan into the supplied [`Registry`]. Plan 04-07
/// appends three `r.register(...)` lines (alphabetical by scan-id):
/// `cross.corr.pearson_rolling`, `cross.corr.spearman_rolling`, and
/// `cross.ols.rolling`. Plans 04-09/04-10 will append further scans inside
/// this body without touching `registry::bootstrap`.
pub fn register_cross_scans(r: &mut Registry) {
    r.register(Box::new(PearsonRollingScan));
    r.register(Box::new(SpearmanRollingScan));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Plan 04-07 Task 1 — `register_cross_scans` now registers the two
    /// rolling correlation scans (Pearson + Spearman). Plan 04-07 Task 2
    /// adds the third (`cross.ols.rolling`).
    #[test]
    fn register_cross_scans_includes_pearson_and_spearman_rolling() {
        let mut r = Registry::new();
        let before = r.scans.len();
        register_cross_scans(&mut r);
        let added = r.scans.len() - before;
        assert!(added >= 2, "expected >= 2 CROSS scans registered; got {added}");
        assert!(
            r.get("cross.corr.pearson_rolling", 1).is_some(),
            "cross.corr.pearson_rolling@1 must be registered"
        );
        assert!(
            r.get("cross.corr.spearman_rolling", 1).is_some(),
            "cross.corr.spearman_rolling@1 must be registered"
        );
    }
}
