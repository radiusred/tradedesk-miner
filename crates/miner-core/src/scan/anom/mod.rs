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

pub mod returns;

pub use returns::ReturnsProfileScan;

/// Register every ANOM scan into the supplied [`Registry`]. Plan 04-03
/// (this commit) appends `ReturnsProfileScan` (ANOM-01). Subsequent plans
/// (04-04..04-06) append further `r.register(...)` lines here alphabetical
/// by scan-id. Plans never modify the central `registry::bootstrap` body.
pub fn register_anom_scans(r: &mut Registry) {
    // Plan 04-03 — ANOM-01 stats.returns.profile@1.
    r.register(Box::new(ReturnsProfileScan));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Plan 04-03 — `register_anom_scans` now registers ANOM-01
    /// `stats.returns.profile@1`. Subsequent ANOM plans append further
    /// lines; Plan 04-11 tightens this to a full count assertion across
    /// all ANOM scans.
    #[test]
    fn register_anom_scans_registers_returns_profile() {
        let mut r = Registry::new();
        let before = r.scans.len();
        register_anom_scans(&mut r);
        assert_eq!(
            r.scans.len(),
            before + 1,
            "Plan 04-03 ships ANOM-01 (stats.returns.profile@1)"
        );
        let scan = r.get("stats.returns.profile", 1).expect("registered");
        assert_eq!(scan.id(), "stats.returns.profile");
        assert_eq!(scan.version(), 1);
    }
}
