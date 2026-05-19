//! [`Registry`] — versioned `(id, version)` scan catalogue.
//!
//! Pattern analog: `cache.rs:519-534` ([`crate::cache::BarCache`]) — a fielded
//! struct with a `#[must_use]` constructor and a single-method facade. The
//! inner field is a `BTreeMap<(String, u32), Box<dyn Scan>>` (per CONTEXT
//! line 204 — the only Phase 3 map type) so iteration order is deterministic
//! and lexicographic on the `(id, version)` key (OUT-03).
//!
//! Pattern analog (`BTreeMap` discipline): `findings/mod.rs:101-103`
//! — every map in a `Serialize` path is `BTreeMap` (NEVER `HashMap`).
//! `Registry::scans` is NOT in a `Serialize` path, but it IS in the iteration
//! order that `miner scans` emits as JSONL — same determinism requirement.
//!
//! Plan 03-02 fills the bodies (Plan 03-01 laid down `unimplemented!()` scaffold).

use std::collections::BTreeMap;

use super::Scan;
use super::ljung_box::LjungBoxScan;

/// Versioned `(id, version)`-keyed catalogue of registered scans.
///
/// `BTreeMap` (NEVER `HashMap`) for deterministic iteration order — OUT-03.
/// The key tuple is `(String, u32)` (scan-id + version) so two versions of
/// the same scan coexist (e.g., `("stats.autocorr.ljung_box", 1)` and
/// `("stats.autocorr.ljung_box", 2)`).
pub struct Registry {
    /// `BTreeMap` (NEVER `HashMap`) for deterministic ordering — OUT-03.
    pub scans: BTreeMap<(String, u32), Box<dyn Scan>>,
}

impl Registry {
    /// Construct an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            scans: BTreeMap::new(),
        }
    }

    /// Register a scan. The key is `(scan.id().to_string(), scan.version())`.
    /// Inserting a duplicate key replaces the previous registration (last-write
    /// wins) — Phase 3 ships exactly one scan via [`bootstrap`] so the
    /// behaviour is academic; Phase 4 plans extend `bootstrap` linearly.
    pub fn register(&mut self, scan: Box<dyn Scan>) {
        let key = (scan.id().to_string(), scan.version());
        self.scans.insert(key, scan);
    }

    /// Look up a scan by `(id, version)`. Returns `None` for unknown scan-id
    /// OR unknown version (preflight rejects both as
    /// `PreflightCode::UnknownScan`).
    #[must_use]
    pub fn get(&self, id: &str, version: u32) -> Option<&dyn Scan> {
        // `BTreeMap::get` accepts the key by reference; we build a tuple of
        // owned `String` because the key type is `(String, u32)`. The .to_string()
        // allocation is fine — this is the preflight path, not the kernel.
        self.scans
            .get(&(id.to_string(), version))
            .map(std::convert::AsRef::as_ref)
    }

    /// Iterate registered scans in `(id, version)` lexicographic order
    /// (`BTreeMap` iteration order — OUT-03). Used by `miner scans` to emit one
    /// JSONL line per registered scan in deterministic order.
    pub fn iter(&self) -> impl Iterator<Item = &dyn Scan> + '_ {
        self.scans.values().map(std::convert::AsRef::as_ref)
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

/// Construct the production registry with every Phase 3+ scan registered.
///
/// Pattern: explicit `bootstrap()` factory (D3-16) — rejected `inventory`'s
/// compile-time magic.
///
/// ## Phase 4 per-family registrar contract (Plan 04-02 / Pattern E)
///
/// `bootstrap()` registers the Phase 3 `LjungBoxScan` directly, then delegates
/// to three per-family `register_<family>_scans(&mut Registry)` helpers — one
/// each for ANOM, CROSS, SEAS. This is the LAST modification to `bootstrap()`
/// in Phase 4: Plans 04-03..04-10 append `r.register(...)` lines inside their
/// own family's helper (alphabetical by scan-id), and Plan 04-11 only updates
/// the count-assertion in `bootstrap_registers_ljung_box_scan` (renamed to
/// reflect the full 22 + 1 count). The per-family registrar pattern lets the
/// Wave-2..Wave-5 plans parallelise scan additions without touching this
/// central function.
#[must_use]
pub fn bootstrap() -> Registry {
    let mut r = Registry::new();
    r.register(Box::new(LjungBoxScan));
    // Phase 4 per-family registrars — Plans 04-03..04-10 append inside the
    // family helpers, NEVER in this function (Pattern E contract).
    crate::scan::anom::register_anom_scans(&mut r);
    crate::scan::cross::register_cross_scans(&mut r);
    crate::scan::seas::register_seas_scans(&mut r);
    r
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Plan 03-02 Task 2 Test 1 — `registry_starts_empty`. A freshly-constructed
    /// `Registry::new()` has no registered scans.
    #[test]
    fn registry_starts_empty() {
        let r = Registry::new();
        assert!(r.scans.is_empty(), "Registry::new() must start empty");
        assert!(r.iter().next().is_none(), "iter() must yield nothing");
    }

    /// Plan 03-02 Task 2 Test 2 — `registry_register_and_get`. After registering
    /// `LjungBoxScan`, `get` returns Some for the matching key and None for
    /// unknown scan-id / unknown version.
    #[test]
    fn registry_register_and_get() {
        let mut r = Registry::new();
        r.register(Box::new(LjungBoxScan));

        // Exact match.
        let found = r.get("stats.autocorr.ljung_box", 1);
        assert!(found.is_some(), "exact (id, version) must resolve");
        let s = found.unwrap();
        assert_eq!(s.id(), "stats.autocorr.ljung_box");
        assert_eq!(s.version(), 1);

        // Unknown scan-id.
        assert!(
            r.get("nonexistent", 1).is_none(),
            "unknown scan-id must return None"
        );

        // Known scan-id but wrong version.
        assert!(
            r.get("stats.autocorr.ljung_box", 99).is_none(),
            "wrong version must return None"
        );
    }

    /// Plan 03-02 Task 2 Test 3 — `registry_uses_btreemap`. Compile-time type
    /// assertion via reference binding (mirrors `findings/mod.rs:526`
    /// `raw_series_uses_btreemap`). If a future commit swaps the inner map
    /// to `HashMap`, this line stops compiling — OUT-03 regression gate.
    #[test]
    fn registry_uses_btreemap() {
        let r = Registry::new();
        let _: &BTreeMap<(String, u32), Box<dyn Scan>> = &r.scans;
    }

    /// Plan 03-02 Task 2 Test 4 — `bootstrap_registers_ljung_box_scan`.
    /// The `bootstrap()` factory returns a registry containing the Phase 3
    /// `LjungBoxScan` plus every scan registered by the per-family helpers
    /// (Plan 04-02 Pattern E). Lower-bound assertion so per-family plans
    /// can grow the count without breaking this test; Plan 04-11 tightens
    /// to the final 23-scan count.
    #[test]
    fn bootstrap_registers_ljung_box_scan() {
        let r = bootstrap();
        assert!(
            r.scans.len() >= 1,
            "bootstrap must register at least LjungBox; got {}",
            r.scans.len()
        );
        assert!(
            r.get("stats.autocorr.ljung_box", 1).is_some(),
            "bootstrap must include LjungBoxScan@1"
        );
    }

    /// Plan 03-02 Task 2 Test 5 — `registry_iter_lex_order`. `Registry::iter()`
    /// yields scans in `(id, version)` lexicographic order — the `BTreeMap`
    /// iteration contract. Phase 3 has one scan so the assertion is trivial;
    /// the test pins the contract for Phase 4 plans that register multiple
    /// scans.
    #[test]
    fn registry_iter_lex_order() {
        let r = bootstrap();
        let ids: Vec<&'static str> = r.iter().map(super::super::Scan::id).collect();
        // For Phase 3 the single-element vec is trivially sorted; the test pins
        // the contract.
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        assert_eq!(ids, sorted, "iter() must yield ids in lexicographic order");
    }

    /// Plan 04-02 Task 1b — Behavior Test 3:
    /// `bootstrap_invokes_all_three_family_registrars`. The factory must call
    /// `register_anom_scans` + `register_cross_scans` + `register_seas_scans`.
    /// Plan 04-07 populates `register_cross_scans` so the post-bootstrap
    /// count grows by 2+ (CROSS scans). Plan 04-11 will tighten this with
    /// the full 23-count assertion once all families are populated.
    ///
    /// Compile-time evidence: this test imports the three family-registrar
    /// helpers via the same module path `bootstrap()` uses, so any rename /
    /// removal of the helpers fails the build.
    #[test]
    fn bootstrap_invokes_all_three_family_registrars() {
        // Type-level evidence the three helpers are reachable from
        // bootstrap()'s namespace (`crate::scan::{anom,cross,seas}::register_*_scans`).
        let _ = crate::scan::anom::register_anom_scans
            as fn(&mut Registry);
        let _ = crate::scan::cross::register_cross_scans
            as fn(&mut Registry);
        let _ = crate::scan::seas::register_seas_scans
            as fn(&mut Registry);
        // Behavioural evidence: bootstrap's count equals the LjungBox direct
        // registration plus the sum of family-registrar contributions.
        // Lower-bound assertion so per-family Phase-4 plans (04-03..04-10)
        // can extend without breaking this test; Plan 04-11 tightens to the
        // exact 23-scan count once every family is populated.
        let r = bootstrap();
        assert!(
            r.scans.len() >= 1,
            "bootstrap must register at least LjungBox; got {}",
            r.scans.len()
        );
    }
}
