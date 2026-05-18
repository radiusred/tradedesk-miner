//! [`Registry`] — versioned `(id, version)` scan catalogue.
//!
//! Pattern analog: `cache.rs:519-534` ([`crate::cache::BarCache`]) — a fielded
//! struct with a `#[must_use]` constructor and a single-method facade. The
//! inner field is a `BTreeMap<(String, u32), Box<dyn Scan>>` (per CONTEXT
//! line 204 — the only Phase 3 map type) so iteration order is deterministic
//! and lexicographic on the `(id, version)` key (OUT-03).
//!
//! Pattern analog (BTreeMap discipline): `findings/mod.rs:101-103`
//! — every map in a `Serialize` path is `BTreeMap` (NEVER `HashMap`).
//! `Registry::scans` is NOT in a `Serialize` path, but it IS in the iteration
//! order that `miner scans` emits as JSONL — same determinism requirement.
//!
//! Wave 0 scaffold: signature only. Plan 02 fills bodies.

#![allow(dead_code, unused_variables)]

use std::collections::BTreeMap;

use super::Scan;
// `LjungBoxScan` is referenced by `bootstrap()` once Plan 03-02 wires the body.
// Wave 0 omits the `use` to avoid an unused-import warning on the scaffold;
// the doc-comment on `bootstrap()` mentions the fully-qualified path so the
// follow-on plan can find it via grep.

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
    /// Construct an empty registry. Plan 02 fills the body.
    #[must_use]
    pub fn new() -> Self {
        unimplemented!("Plan 02 (03-02-PLAN) wires Registry::new")
    }

    /// Register a scan. Plan 02 fills the body.
    pub fn register(&mut self, scan: Box<dyn Scan>) {
        unimplemented!("Plan 02 (03-02-PLAN) wires Registry::register")
    }

    /// Look up a scan by `(id, version)`. Plan 02 fills the body.
    #[must_use]
    pub fn get(&self, id: &str, version: u32) -> Option<&dyn Scan> {
        unimplemented!("Plan 02 (03-02-PLAN) wires Registry::get")
    }

    /// Iterate registered scans in `(id, version)` lexicographic order
    /// (BTreeMap iteration order — OUT-03). Plan 02 fills the body.
    pub fn iter(&self) -> impl Iterator<Item = &dyn Scan> + '_ {
        // Compile-only stub — Plan 02 wires the real iterator.
        self.scans.values().map(|boxed| &**boxed as &dyn Scan)
    }
}

impl Default for Registry {
    fn default() -> Self {
        unimplemented!("Plan 02 (03-02-PLAN) wires Registry::default")
    }
}

/// Construct the production registry with every Phase 3+ scan registered.
///
/// Pattern: explicit `bootstrap()` factory (D3-16) — rejected `inventory`'s
/// compile-time magic. Phase 4 plans extend this with one line per scan.
///
/// Wave 0 scaffold: signature only. Plan 02 fills the body.
#[must_use]
pub fn bootstrap() -> Registry {
    unimplemented!(
        "Plan 02 (03-02-PLAN) wires bootstrap(); will: \
         let mut r = Registry::new(); r.register(Box::new(LjungBoxScan)); r"
    )
}
