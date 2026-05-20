//! SEAS (seasonality / time-of-day / calendar-effect) scan family namespace.
//!
//! Houses the 6 SEAS scans rolled out by Plans 04-09..04-10: hour-of-day,
//! day-of-week, session (uses [`crate::calendar::Calendar`]), end-of-month /
//! start-of-month, ANOVA + Kruskal-Wallis, event-window. Every SEAS scan is
//! single-leg (`ScanArity::Single`).
//!
//! [`register_seas_scans`] is the SOLE registration path for this family
//! (Pattern E / Plan 04-02 contract): Plans 04-09..04-10 append
//! `r.register(Box::new(<NewScan>));` lines INSIDE this function —
//! alphabetical by scan-id — and never touch `registry::bootstrap`.
//!
//! ## Shared helper — [`bucketing`]
//!
//! The [`bucketing::bucket_stats`] helper computes per-bucket mean / std /
//! count / t-stat / IQR from parallel `(values, bucket_keys)` slices. SEAS-01
//! through SEAS-04 + SEAS-06 share this single helper; each scan provides its
//! own bucket-key derivation (UTC hour for `hour_of_day`, weekday for
//! `day_of_week`, session-window membership for `session`, etc.). SEAS-05
//! (anova_kw) is the only meta-scan — it reuses other SEAS scans' bucket-key
//! kernels via `params.buckets_via`.

use super::Registry;

pub mod bucketing;
pub mod day_of_week;
pub mod eom_som;
pub mod hour_of_day;
pub mod session;

pub use day_of_week::DayOfWeekScan;
pub use eom_som::EomSomScan;
pub use hour_of_day::HourOfDayScan;
pub use session::SessionScan;

/// Register every SEAS scan into the supplied [`Registry`]. Plans 04-09 and
/// 04-10 append `r.register(...)` lines here alphabetical by scan-id.
pub fn register_seas_scans(r: &mut Registry) {
    // Alphabetical by scan-id:
    //   seas.bucket.day_of_week        <- Plan 04-09
    //   seas.bucket.eom_som            <- Plan 04-10
    //   seas.bucket.hour_of_day        <- Plan 04-09
    //   seas.bucket.session            <- Plan 04-09
    r.register(Box::new(DayOfWeekScan));
    r.register(Box::new(EomSomScan));
    r.register(Box::new(HourOfDayScan));
    r.register(Box::new(SessionScan));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Plan 04-10 Task 1 — after EOM/SOM lands the helper registers 4 SEAS
    /// scans. Subsequent tasks (event_window, anova_kw) extend the count.
    #[test]
    fn register_seas_scans_registers_four_after_plan_04_10_task_1() {
        let mut r = Registry::new();
        let before = r.scans.len();
        register_seas_scans(&mut r);
        assert!(
            r.scans.len() >= before + 4,
            "Plan 04-10 Task 1 ships 4 SEAS registrations (3 from 04-09 + 1 eom_som); got {}",
            r.scans.len() - before
        );
        assert!(r.get("seas.bucket.day_of_week", 1).is_some());
        assert!(r.get("seas.bucket.eom_som", 1).is_some());
        assert!(r.get("seas.bucket.hour_of_day", 1).is_some());
        assert!(r.get("seas.bucket.session", 1).is_some());
    }
}
