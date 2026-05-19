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

/// Register every CROSS scan into the supplied [`Registry`]. Empty in Plan
/// 04-02; Plan 04-07 appends `r.register(...)` lines here.
#[allow(
    clippy::needless_pass_by_ref_mut,
    reason = "the &mut Registry is the API contract for Plan 04-07; the body is intentionally empty in Plan 04-02"
)]
pub fn register_cross_scans(r: &mut Registry) {
    let _ = r;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Plan 04-02 Task 1b — Behavior Test 2 (sibling for cross). Empty body
    /// adds zero entries; Plan 04-07 populates the four CROSS scans.
    #[test]
    fn register_cross_scans_is_noop_initially() {
        let mut r = Registry::new();
        let before = r.scans.len();
        register_cross_scans(&mut r);
        assert_eq!(
            r.scans.len(),
            before,
            "Plan 04-02 ships ZERO CROSS registrations; Plan 04-07 populates"
        );
    }
}
