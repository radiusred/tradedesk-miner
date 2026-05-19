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

/// Register every ANOM scan into the supplied [`Registry`]. Empty in Plan
/// 04-02; subsequent plans (04-03..04-06) append `r.register(...)` lines
/// here alphabetical by scan-id. Plans never modify the central
/// `registry::bootstrap` body.
#[allow(
    clippy::needless_pass_by_ref_mut,
    reason = "the &mut Registry is the API contract for Plans 04-03..04-06; the body is intentionally empty in Plan 04-02"
)]
pub fn register_anom_scans(r: &mut Registry) {
    // Suppress unused-variable warning in the empty-body case. Plans
    // 04-03..04-06 will delete this `let _` once they append the first
    // `r.register(...)` call.
    let _ = r;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Plan 04-02 Task 1b — Behavior Test 1: `register_anom_scans` is a
    /// no-op in the Wave-2 baseline (Plans 04-03..04-06 will populate it).
    /// The contract: calling the helper does NOT add any scan to the
    /// registry. Plan 04-11 tightens this with a full count assertion.
    #[test]
    fn register_anom_scans_is_noop_initially() {
        let mut r = Registry::new();
        let before = r.scans.len();
        register_anom_scans(&mut r);
        assert_eq!(
            r.scans.len(),
            before,
            "Plan 04-02 ships ZERO ANOM registrations; Plans 04-03..04-06 populate"
        );
    }
}
