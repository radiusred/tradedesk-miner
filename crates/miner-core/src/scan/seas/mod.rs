//! SEAS (seasonality / time-of-day / calendar-effect) scan family namespace.
//!
//! Houses the 6 SEAS scans rolled out by Plans 04-08..04-10: hour-of-day,
//! day-of-week, session (uses [`crate::calendar::Calendar`]), end-of-month /
//! start-of-month, ANOVA + Kruskal-Wallis, event-window. Every SEAS scan is
//! single-leg (`ScanArity::Single`).
//!
//! [`register_seas_scans`] is the SOLE registration path for this family
//! (Pattern E / Plan 04-02 contract): Plans 04-08..04-10 append
//! `r.register(Box::new(<NewScan>));` lines INSIDE this function —
//! alphabetical by scan-id — and never touch `registry::bootstrap`.
//!
//! ## Shared helper — [`bucketing`]
//!
//! The [`bucketing::bucket_stats`] helper computes per-bucket mean / std /
//! count / t-stat / IQR from parallel `(values, bucket_keys)` slices. SEAS-01
//! through SEAS-04 + SEAS-06 share this single helper; each scan provides its
//! own bucket-key derivation (UTC hour for `hour_of_day`, weekday for
//! `day_of_week`, session-window membership for `session`, etc.).

use super::Registry;

pub mod bucketing;
pub mod day_of_week;
pub mod hour_of_day;
pub mod session;

pub use day_of_week::DayOfWeekScan;
pub use hour_of_day::HourOfDayScan;
pub use session::SessionScan;

/// Register every SEAS scan into the supplied [`Registry`]. Plans 04-09 and
/// 04-10 append `r.register(...)` lines here alphabetical by scan-id.
pub fn register_seas_scans(r: &mut Registry) {
    // Plan 04-09 — SEAS-01, SEAS-02, SEAS-03 (alphabetical by scan-id).
    r.register(Box::new(DayOfWeekScan));
    r.register(Box::new(HourOfDayScan));
    r.register(Box::new(SessionScan));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// After Plan 04-09 the helper registers all 3 bucketed SEAS scans
    /// (day_of_week, hour_of_day, session). Plan 04-10 will bring the count
    /// to 6.
    #[test]
    fn register_seas_scans_registers_three_after_plan_04_09() {
        let mut r = Registry::new();
        let before = r.scans.len();
        register_seas_scans(&mut r);
        assert!(
            r.scans.len() >= before + 3,
            "Plan 04-09 ships 3 SEAS registrations"
        );
        assert!(r.get("seas.bucket.day_of_week", 1).is_some());
        assert!(r.get("seas.bucket.hour_of_day", 1).is_some());
        assert!(r.get("seas.bucket.session", 1).is_some());
    }
}
