//! Phase 3 integration-test fixtures — synthetic cache + AR(1) BarFrame builder.
//!
//! Pattern analog: `miner-core/tests/full_determinism.rs` (whole file — inlines a
//! `SyntheticCache` builder using the sibling crate's PUBLIC `day_csv_zst` API,
//! lines 33-50). The Phase 3 integration tests reach into this fixtures module
//! rather than re-implementing the synthetic-cache plumbing per test.
//!
//! Module status: scaffold. Plan 03-06 fills the bodies once the AR(1) data
//! generator + the synthetic-cache builder land.

#![allow(dead_code, unused_imports)]

// Note: this module is referenced from individual integration test files via
// `#[path = "fixtures/mod.rs"]` declarations (Plan 03-06 wires those when it
// fills the test bodies). Cargo treats files in `tests/` as separate
// integration-test crates, so `tests/fixtures/mod.rs` is NOT auto-discovered
// — each consumer test opts in.
//
// Wave 0 scaffold: empty stubs so the file exists and the grep-discovery
// gate at execute-time finds it.

/// Marker type for the per-test synthetic Dukascopy cache.
///
/// Plan 03-06 fills with the real path-builder + on-disk writer that emits
/// `<root>/<SYMBOL>/<YYYY>/<MM>/<DD>_<side>.csv.zst` files in the layout
/// `DukascopyReader` expects.
pub struct SyntheticCache;

/// Build a deterministic AR(1) bar frame for fixture use.
///
/// `n` is the bar count; `seed` is the proptest / golden seed. Plan 03-06
/// fills with a `BarFrame { ts_open_utc, close, ... }` constructor where
/// `close[t] = phi * close[t-1] + epsilon[t]` for `phi = 0.7`, `epsilon[t]`
/// drawn from a fixed-seed Gaussian.
#[must_use]
pub fn build_ar1_bar_frame(n: usize, seed: u64) -> miner_core::aggregator::BarFrame {
    unimplemented!(
        "Plan 03-06 implements build_ar1_bar_frame(n={}, seed={}) per CONTEXT D3-05 — \
         AR(1) phi=0.7, fixed-seed Gaussian innovations",
        n,
        seed
    )
}
