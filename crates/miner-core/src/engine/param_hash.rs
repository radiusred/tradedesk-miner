//! `param_hash` — blake3 hash of canonical resolved-params JSON (D3-13).
//!
//! Pattern analog: `miner-reader-dukascopy/src/reader.rs::fingerprint_day`
//! (lines 123-141) — the `blake3::hash(&bytes) → hash.to_hex() → bytes.try_into()
//! → Blake3Hex::from_hex_bytes` idiom.
//!
//! ## D3-13 canonicalisation
//!
//! `param_hash = blake3(serde_json::to_vec(&resolved_params))` as lowercase
//! hex (`Blake3Hex` carries the 64-char invariant). Determinism falls out of
//! Phase 1's `BTreeMap`-only rule: the resolved-params object contains no
//! `HashMap`, so `serde_json::to_vec` produces byte-stable bytes across
//! replays.
//!
//! RFC 8785 JCS canonicalisation is deferred (D3-13) unless a non-BTreeMap
//! path surfaces.
//!
//! Wave 0 scaffold: signature only. Plan 03-02 fills the body.

#![allow(dead_code, unused_variables)]

use crate::reader::Blake3Hex;

/// Compute the canonical blake3 hash of the resolved-params object.
///
/// Pattern (Plan 03-02 fills verbatim from 03-RESEARCH §"Code Examples"
/// lines 630-639):
///
/// ```text
/// let bytes = serde_json::to_vec(resolved)?;
/// let hash  = blake3::hash(&bytes);
/// let bytes64: [u8; 64] = hash.to_hex().as_bytes().try_into()
///     .expect("blake3 hex is always 64 chars");
/// Ok(Blake3Hex::from_hex_bytes(&bytes64))
/// ```
///
/// ## Pitfall reminder
///
/// `param_hash`'s input is ONLY the resolved scan params — NEVER the
/// `RunStart.request` value (which contains `run_id` / timestamps). RESEARCH
/// §Pitfall 6 reaffirms.
///
/// # Errors
/// Returns `serde_json::Error` only if the resolved-params value contains a
/// non-serialisable node (should not happen — production callers always pass
/// a `serde_json::Value` returned from `parse_params_kv`).
pub fn param_hash(resolved: &serde_json::Value) -> Result<Blake3Hex, serde_json::Error> {
    unimplemented!(
        "Plan 03-02 wires param_hash per 03-RESEARCH §Code Examples lines 630-639"
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    // Plan 03-02 fills:
    // - `param_hash_is_byte_stable` — two calls with identical input produce
    //   identical hex (mirror `tests/aggregator_determinism.rs:48-100`).
    // - `param_hash_differs_for_different_values` — sanity test.
    //
    // Wave 0 ships the test scaffold with `#[ignore]` so cargo test --list
    // can discover the names per VALIDATION harness contract.
    #[test]
    #[ignore = "Plan 03-02 implements param_hash; Wave 0 scaffold only"]
    fn param_hash_is_byte_stable() {
        unimplemented!("Plan 03-02 wires this test against the real param_hash body")
    }
}
