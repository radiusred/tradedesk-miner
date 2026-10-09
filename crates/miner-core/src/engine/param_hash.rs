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
//! ## Pitfall 6 — params vs request separation (RESEARCH lines 589-597)
//!
//! `param_hash`'s input is ONLY the resolved scan params (the typed params
//! struct serialised to `serde_json::Value`), NEVER the `RunStart.request`
//! value. The request blob mixes run-level metadata (`scan_id@version`,
//! instrument, window, etc.) with the post-defaults params; mixing the two
//! would make the hash time-dependent because `RunStart.request` is built
//! AFTER `param_hash` is computed by the engine facade. The fix is the
//! function signature itself: `param_hash(resolved: &serde_json::Value)`
//! takes ONLY the resolved-params value, structurally precluding the bug.

use crate::reader::Blake3Hex;

/// Compute the canonical blake3 hash of the resolved-params object.
///
/// Mirrors the dukascopy reader's `fingerprint_day` idiom
/// (`crates/miner-reader-dukascopy/src/reader.rs:123-141`):
/// `serde_json::to_vec → blake3::hash → to_hex → try_into → Blake3Hex::from_hex_bytes`.
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
///
/// # Panics
/// Panics with `"blake3 hex is always 64 chars"` if `blake3::Hash::to_hex`
/// ever produces a non-64-char ASCII-hex output — impossible by the crate's
/// public contract; the `expect` exists as a runtime witness if a future
/// blake3 release ever changes the digest size.
pub fn param_hash(resolved: &serde_json::Value) -> Result<Blake3Hex, serde_json::Error> {
    let bytes = serde_json::to_vec(resolved)?;
    let hash = blake3::hash(&bytes);
    let hex = hash.to_hex();
    // `blake3::Hash::to_hex()` returns a fixed-size 64-char ASCII-hex string;
    // the `try_into` cannot fail by the type's contract. We use `expect` (not
    // `unwrap`) so the panic message points at the invariant if some future
    // blake3 version ever changes the hex output length.
    let bytes64: [u8; 64] = hex
        .as_bytes()
        .try_into()
        .expect("blake3 hex is always 64 chars");
    Ok(Blake3Hex::from_hex_bytes(&bytes64))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Two calls with structurally identical input produce identical hex.
    /// Pattern mirror: `tests/aggregator_determinism.rs:48-100`
    /// `byte_identical_two_runs`.
    #[test]
    fn param_hash_is_byte_stable() {
        let v1 = serde_json::json!({"lags": 20});
        let v2 = serde_json::json!({"lags": 20});
        let h1 = param_hash(&v1).expect("hash v1");
        let h2 = param_hash(&v2).expect("hash v2");
        assert_eq!(
            h1.as_str(),
            h2.as_str(),
            "param_hash must be byte-stable across calls with identical input"
        );
    }

    /// Different params produce different hashes (sanity check —
    /// rules out an accidental constant-return implementation).
    #[test]
    fn param_hash_differs_on_different_input() {
        let v1 = serde_json::json!({"lags": 20});
        let v2 = serde_json::json!({"lags": 21});
        let h1 = param_hash(&v1).expect("hash v1");
        let h2 = param_hash(&v2).expect("hash v2");
        assert_ne!(
            h1.as_str(),
            h2.as_str(),
            "param_hash must change when input changes"
        );
    }

    /// `BTreeMap`-backed `serde_json::Value` produces the same hash regardless
    /// of insertion order (Pitfall 1 — `preserve_order` would break this).
    ///
    /// Two `serde_json::Map`s built in different insertion orders but with
    /// identical key-value pairs MUST hash identically. This is structurally
    /// guaranteed by `serde_json`'s default `BTreeMap`-backed `Map`; the test
    /// pins the invariant so a future transitive `preserve_order` feature
    /// activation surfaces immediately.
    #[test]
    fn param_hash_btreemap_order_invariant() {
        // Build two Maps with the same content but reverse insertion order.
        let mut m1 = serde_json::Map::new();
        m1.insert("alpha".to_string(), serde_json::Value::from(1i64));
        m1.insert("beta".to_string(), serde_json::Value::from(2i64));
        m1.insert("gamma".to_string(), serde_json::Value::from(3i64));

        let mut m2 = serde_json::Map::new();
        m2.insert("gamma".to_string(), serde_json::Value::from(3i64));
        m2.insert("beta".to_string(), serde_json::Value::from(2i64));
        m2.insert("alpha".to_string(), serde_json::Value::from(1i64));

        let v1 = serde_json::Value::Object(m1);
        let v2 = serde_json::Value::Object(m2);

        let h1 = param_hash(&v1).expect("hash v1");
        let h2 = param_hash(&v2).expect("hash v2");
        assert_eq!(
            h1.as_str(),
            h2.as_str(),
            "BTreeMap-backed Maps must produce identical hashes regardless of insertion order"
        );
    }

    /// `Blake3Hex` carries a 64-char lowercase-hex invariant. Confirm by
    /// stringifying the result and asserting length + character class.
    #[test]
    fn param_hash_returns_64_hex_chars() {
        let v = serde_json::json!({"lags": 20});
        let h = param_hash(&v).expect("hash");
        let s = h.as_str();
        assert_eq!(s.len(), 64, "Blake3Hex must be exactly 64 chars; got {s:?}");
        for c in s.chars() {
            assert!(
                c.is_ascii_hexdigit() && !(c.is_ascii_alphabetic() && c.is_ascii_uppercase()),
                "Blake3Hex must contain only lowercase ASCII hex chars; saw {c:?} in {s:?}"
            );
        }
    }
}
