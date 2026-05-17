//! Trading calendar — placeholder stub.
//!
//! Plan 02-01 ships only the type skeleton so `Reader::trading_calendar()` has a return
//! type the workspace can compile against. Plan 02-02 replaces this with the FX-major
//! default + `is_open_at` predicate (D2-06 / D2-07 / D2-08).
//!
//! The type is exposed through `miner_core::Calendar` already so downstream plans can
//! reference `miner_core::Calendar` from day one without churn when Plan 02-02 lands.

/// Trading calendar. Plan 02-02 fills the real fields (`weekly_open_utc`,
/// `weekly_close_utc`, `yearly_holidays`) and the `is_open_at` predicate.
///
/// `Clone` (NOT `Copy`) because Plan 02-02's real shape carries a `Vec` of yearly
/// holidays — so `Reader::trading_calendar(&self) -> Calendar` can return-by-value
/// via `.clone()` without changing the trait once the real shape lands.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Calendar {
    /// Private placeholder field to keep the struct shape stable across Plan 02-02.
    /// Plan 02-02 replaces this with concrete fields per the D2-08 shape.
    _placeholder: (),
}

impl Calendar {
    /// Construct an empty placeholder calendar. Plan 02-02 replaces this with
    /// `Calendar::fx_major()` returning the FX-major default per D2-07.
    #[must_use]
    pub fn new() -> Self {
        Self { _placeholder: () }
    }
}
