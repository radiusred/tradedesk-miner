//! SEAS (seasonality / time-of-day / calendar-effect) scan family namespace.
//!
//! Houses the 6 SEAS scans rolled out by Plans 04-08..04-09: hour-of-day,
//! day-of-week, session (uses [`crate::calendar::Calendar`]), end-of-month /
//! start-of-month, ANOVA + Kruskal-Wallis, event-window. Every SEAS scan is
//! single-leg (`ScanArity::Single`).
//!
//! [`register_seas_scans`] is the SOLE registration path for this family
//! (Pattern E / Plan 04-02 contract): Plans 04-08..04-09 append
//! `r.register(Box::new(<NewScan>));` lines INSIDE this function —
//! alphabetical by scan-id — and never touch `registry::bootstrap`.

use super::Registry;

/// Register every SEAS scan into the supplied [`Registry`]. Empty in Plan
/// 04-02; Plans 04-08..04-09 append `r.register(...)` lines here.
#[allow(
    clippy::needless_pass_by_ref_mut,
    reason = "the &mut Registry is the API contract for Plans 04-08..04-09; the body is intentionally empty in Plan 04-02"
)]
pub fn register_seas_scans(r: &mut Registry) {
    let _ = r;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Plan 04-02 Task 1b — Behavior Test 2 (sibling for seas). Empty body
    /// adds zero entries; Plans 04-08..04-09 populate the six SEAS scans.
    #[test]
    fn register_seas_scans_is_noop_initially() {
        let mut r = Registry::new();
        let before = r.scans.len();
        register_seas_scans(&mut r);
        assert_eq!(
            r.scans.len(),
            before,
            "Plan 04-02 ships ZERO SEAS registrations; Plans 04-08..04-09 populate"
        );
    }
}
