//! statsmodels golden-fixture loader for Plan 03-06 integration tests.
//!
//! The canonical artefact is at
//! `crates/miner-core/tests/fixtures/ljung_box_golden.json` (committed by
//! `generate_golden.py` — Blocker 4 fix per D3-05). This helper loads it via
//! `include_str!` so consumers don't re-derive the path.
//!
//! The provenance contract is asserted at every call site: the JSON MUST
//! cite `statsmodels==0.14.6` in `provenance.statsmodels_version`. If the
//! version mismatches, the loader panics with a regeneration pointer — never
//! silently consume a stale golden.
//!
//! Direction: statsmodels-to-Rust. The Rust code consumes this JSON; the
//! Python script (NEVER the Rust code) is the canonical writer.

#![allow(dead_code)]

/// statsmodels version the committed golden fixture was generated against.
pub const STATSMODELS_REQUIRED: &str = "0.14.6";

/// The committed golden fixture, loaded via `include_str!` so the path
/// resolution is compile-time and the integration tests don't depend on
/// process cwd.
pub const GOLDEN_JSON: &str =
    include_str!("../../../miner-core/tests/fixtures/ljung_box_golden.json");

/// Parse the golden fixture as a `serde_json::Value`, asserting the
/// `provenance.statsmodels_version` field matches [`STATSMODELS_REQUIRED`].
/// Panics with a clear regeneration message on mismatch.
#[must_use]
pub fn load_statsmodels_golden() -> serde_json::Value {
    let v: serde_json::Value = serde_json::from_str(GOLDEN_JSON).expect("golden JSON must parse");
    let prov_version = v["provenance"]["statsmodels_version"].as_str();
    assert_eq!(
        prov_version,
        Some(STATSMODELS_REQUIRED),
        "ljung_box_golden.json provenance.statsmodels_version mismatch — \
         expected {STATSMODELS_REQUIRED:?}; got {prov_version:?}. \
         Regenerate via `python3 crates/miner-core/tests/fixtures/generate_golden.py` \
         after installing `statsmodels=={STATSMODELS_REQUIRED}`.",
    );
    v
}
