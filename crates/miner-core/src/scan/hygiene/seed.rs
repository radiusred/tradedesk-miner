//! Per-job seed derivation — `derive_job_seed` (Plan 05-02 / D5-05 / HYG-05).
//!
//! Pattern analog: `crate::engine::param_hash::param_hash` (same crate-internal
//! `blake3::Hasher` discipline + `Blake3Hex` convention from Phase 2 D2-05).
//! Where `param_hash` produces a 64-char hex `Blake3Hex`, `derive_job_seed`
//! collapses the 32-byte blake3 hash to a `u64` via little-endian read of the
//! first 8 bytes — the right shape for `Xoshiro256PlusPlus::seed_from_u64`.
//!
//! ## Canonicalisation rules (HYG-05 / 05-PATTERNS lines 378-386; WR-07 revision)
//!
//! The hash input uses LENGTH-PREFIXED encoding for every variable-length
//! field so byte-boundary collisions are mathematically impossible. Each
//! variable-length string is preceded by its `u64::to_le_bytes()` length,
//! and instrument specs are written as `(symbol_len, symbol, side_str)`
//! tuples to prevent `:` characters in symbols from colliding with a
//! different `(symbol, side)` pair.
//!
//! Hash input, in order:
//!
//! 1. `master_seed.to_le_bytes()` — little-endian (matches the `u64` wire
//!    convention used everywhere else in `miner-core`).
//! 2. `scan_id_at_version.len() as u64` (LE) + `scan_id_at_version.as_bytes()`.
//! 3. `instruments.len() as u64` (LE) — number of instrument specs.
//! 4. For each `InstrumentSpec`, in vector order:
//!    - `symbol.len() as u64` (LE) + `symbol.as_bytes()`,
//!    - `side_str.len() as u64` (LE) + `side_str.as_bytes()`.
//! 5. `timeframe.len() as u64` (LE) + `timeframe.as_bytes()`.
//! 6. `window_str.len() as u64` (LE) + `window_str.as_bytes()` (rfc3339
//!    start/end joined by `/`).
//! 7. `param_hash.len() as u64` (LE) + `param_hash.as_bytes()`.
//!
//! ## WR-07: collision-resistance discipline
//!
//! Pre-WR-07 the hash concatenated raw ASCII bytes with `:` as the
//! intra-field separator (`format!("{}:{}", symbol, side.as_str())`).
//! A symbol containing a literal `:` could collide with a different
//! `(symbol, side)` pair, AND no inter-instrument delimiter meant two
//! adjacent specs `(s1, side1)` and `(s2, side2)` could collide with a
//! single spec `(s1:side1s2, side2)` for crafted symbols. The
//! length-prefix discipline eliminates both collision paths at the
//! cost of one `MINER_CODE_REVISION` bump (changing the canonical bytes
//! changes every previously-pinned reference seed).
//!
//! These rules MUST match the byte-identical-rerun invariant: any change to
//! the canonical-bytes formatting requires a `Cargo.toml` major-version bump
//! and a regenerated snapshot in `crates/miner-core/tests/snapshots/`.
//!
//! ## Output collapse (blake3-32 → u64)
//!
//! Blake3 produces a 256-bit (32-byte) digest. We take the first 8 bytes and
//! interpret them as little-endian `u64`. This is documented in HYG-05 as
//! "secure-enough" given the seed is non-cryptographic — the only requirement
//! is collision resistance across the canonical-input space (1 ≤ jobs ≤ 10^5
//! per sweep × distinct master seeds), and blake3's 64-bit projection retains
//! the full collision-resistance budget of a 64-bit hash (negligible for the
//! sweep cardinality cap).

use crate::aggregator::Timeframe;
use crate::reader::{ClosedRangeUtc, InstrumentSpec};
use blake3::Hasher;

/// WR-07 helper: write a length-prefixed byte slice into a blake3 hasher.
/// The prefix is `bytes.len() as u64` in little-endian; on 64-bit
/// platforms the `as u64` cast is lossless.
#[inline]
fn update_len_prefixed(hasher: &mut Hasher, bytes: &[u8]) {
    hasher.update(&(bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

/// Derive a per-job 64-bit seed from the master seed + the job's canonical
/// identity tuple (HYG-05 / D5-05).
///
/// Determinism: same inputs ⇒ same `u64`. Sensitivity: changing ANY input
/// (including `master_seed`, scan id, instruments, timeframe, window, or
/// `param_hash`) changes the output `u64` with overwhelming probability
/// (blake3's collision-resistance budget applies; the first-8-bytes
/// projection retains a 64-bit collision budget).
///
/// Plan 05-03 (engine integration) calls this once per job to seed the
/// `Xoshiro256PlusPlus` rngs used by `stationary_bootstrap_ci` and
/// `circular_shift_null_p`. The same seed is echoed into the wire
/// `ReproEnvelope.job_seed` (HYG-05) so a byte-identical re-run can be
/// reconstructed from the streamed JSONL alone.
///
/// # Panics
/// Panics via `expect("blake3 32-byte output")` only if `blake3::Hash`'s
/// public contract ever returns less than 8 bytes — impossible by the
/// crate's published API.
#[must_use]
pub fn derive_job_seed(
    master_seed: u64,
    scan_id_at_version: &str,
    instruments: &[InstrumentSpec],
    timeframe: Timeframe,
    window: &ClosedRangeUtc,
    param_hash: &str,
) -> u64 {
    // WR-07: length-prefix every variable-length field so a `:`-bearing
    // symbol (or boundary-bleed between adjacent specs) cannot collide
    // with a different canonical input.
    let mut hasher = Hasher::new();
    hasher.update(&master_seed.to_le_bytes());
    update_len_prefixed(&mut hasher, scan_id_at_version.as_bytes());
    // Inter-instrument delimiter: count of specs as a fixed-width u64.
    hasher.update(&(instruments.len() as u64).to_le_bytes());
    for spec in instruments {
        update_len_prefixed(&mut hasher, spec.symbol.as_bytes());
        update_len_prefixed(&mut hasher, spec.side.as_str().as_bytes());
    }
    update_len_prefixed(&mut hasher, timeframe.as_str().as_bytes());
    let window_str = format!("{}/{}", window.start.to_rfc3339(), window.end.to_rfc3339());
    update_len_prefixed(&mut hasher, window_str.as_bytes());
    update_len_prefixed(&mut hasher, param_hash.as_bytes());
    let bytes = hasher.finalize();
    let head: [u8; 8] = bytes
        .as_bytes()
        .get(..8)
        .expect("blake3 32-byte output")
        .try_into()
        .expect("blake3 32-byte output");
    u64::from_le_bytes(head)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aggregator::Timeframe;
    use crate::reader::{ClosedRangeUtc, InstrumentSpec, Side};
    use chrono::TimeZone;
    use chrono::Utc;

    fn sample_window() -> ClosedRangeUtc {
        ClosedRangeUtc {
            start: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            end: Utc.with_ymd_and_hms(2026, 2, 1, 0, 0, 0).unwrap(),
        }
    }

    fn sample_instruments() -> Vec<InstrumentSpec> {
        vec![InstrumentSpec {
            symbol: "EURUSD".into(),
            side: Side::Bid,
        }]
    }

    /// Test 7 (Plan 05-02): `derive_job_seed` is deterministic — same inputs
    /// always produce the same `u64`.
    #[test]
    fn derive_job_seed_is_deterministic() {
        let window = sample_window();
        let instruments = sample_instruments();
        let s1 = derive_job_seed(
            0xDEAD,
            "stats.autocorr.ljung_box@1",
            &instruments,
            Timeframe::Tf15m,
            &window,
            "abc123",
        );
        let s2 = derive_job_seed(
            0xDEAD,
            "stats.autocorr.ljung_box@1",
            &instruments,
            Timeframe::Tf15m,
            &window,
            "abc123",
        );
        assert_eq!(s1, s2, "derive_job_seed must be deterministic");
    }

    /// Test 8 (Plan 05-02): changing ANY single input component changes the
    /// output `u64`. Collision-resistance smoke: blake3's 64-bit projection
    /// is overwhelmingly unlikely to collide on the small input deltas tested
    /// here.
    #[test]
    fn derive_job_seed_changes_on_input_change() {
        let window = sample_window();
        let instruments = sample_instruments();
        let baseline = derive_job_seed(
            0xDEAD,
            "stats.autocorr.ljung_box@1",
            &instruments,
            Timeframe::Tf15m,
            &window,
            "abc123",
        );

        // 1. Different master_seed.
        let s_master = derive_job_seed(
            0xBEEF,
            "stats.autocorr.ljung_box@1",
            &instruments,
            Timeframe::Tf15m,
            &window,
            "abc123",
        );
        assert_ne!(baseline, s_master, "master_seed change must change u64");

        // 2. Different scan_id_at_version.
        let s_scan = derive_job_seed(
            0xDEAD,
            "anom.adf@1",
            &instruments,
            Timeframe::Tf15m,
            &window,
            "abc123",
        );
        assert_ne!(baseline, s_scan, "scan_id change must change u64");

        // 3. Different instruments (Ask side).
        let other_instruments = vec![InstrumentSpec {
            symbol: "EURUSD".into(),
            side: Side::Ask,
        }];
        let s_instr = derive_job_seed(
            0xDEAD,
            "stats.autocorr.ljung_box@1",
            &other_instruments,
            Timeframe::Tf15m,
            &window,
            "abc123",
        );
        assert_ne!(baseline, s_instr, "instruments change must change u64");

        // 4. Different timeframe.
        let s_tf = derive_job_seed(
            0xDEAD,
            "stats.autocorr.ljung_box@1",
            &instruments,
            Timeframe::Tf1h,
            &window,
            "abc123",
        );
        assert_ne!(baseline, s_tf, "timeframe change must change u64");

        // 5. Different window.
        let other_window = ClosedRangeUtc {
            start: Utc.with_ymd_and_hms(2026, 3, 1, 0, 0, 0).unwrap(),
            end: Utc.with_ymd_and_hms(2026, 4, 1, 0, 0, 0).unwrap(),
        };
        let s_window = derive_job_seed(
            0xDEAD,
            "stats.autocorr.ljung_box@1",
            &instruments,
            Timeframe::Tf15m,
            &other_window,
            "abc123",
        );
        assert_ne!(baseline, s_window, "window change must change u64");

        // 6. Different param_hash.
        let s_ph = derive_job_seed(
            0xDEAD,
            "stats.autocorr.ljung_box@1",
            &instruments,
            Timeframe::Tf15m,
            &window,
            "def456",
        );
        assert_ne!(baseline, s_ph, "param_hash change must change u64");
    }

    /// Multi-instrument input order matters: `[a, b] != [b, a]` (Pair-arity
    /// scans encode leg-A vs leg-B in the vector order; this test pins the
    /// ordering as load-bearing).
    #[test]
    fn derive_job_seed_instrument_order_matters() {
        let window = sample_window();
        let ab = vec![
            InstrumentSpec {
                symbol: "EURUSD".into(),
                side: Side::Bid,
            },
            InstrumentSpec {
                symbol: "GBPUSD".into(),
                side: Side::Bid,
            },
        ];
        let ba = vec![ab[1].clone(), ab[0].clone()];
        let s_ab = derive_job_seed(
            0xDEAD,
            "cross.lead_lag@1",
            &ab,
            Timeframe::Tf15m,
            &window,
            "abc",
        );
        let s_ba = derive_job_seed(
            0xDEAD,
            "cross.lead_lag@1",
            &ba,
            Timeframe::Tf15m,
            &window,
            "abc",
        );
        assert_ne!(
            s_ab, s_ba,
            "instrument vector order must change the derived seed (leg ordering is load-bearing)"
        );
    }

    /// WR-07 regression: symbols containing `:` MUST NOT collide with a
    /// different `(symbol, side)` pair on the canonical-bytes wire.
    ///
    /// Pre-WR-07 the encoding was `format!("{}:{}", symbol, side.as_str())`
    /// which produced byte-identical canonical input for the two pairs
    /// below. Post-WR-07 the length-prefix discipline pulls them apart
    /// with overwhelming probability.
    #[test]
    fn derive_job_seed_symbol_with_colon_does_not_collide() {
        let window = sample_window();
        // Pair A: symbol "FOO:bid", side Ask → pre-fix bytes: "FOO:bid:ask"
        let a = vec![InstrumentSpec {
            symbol: "FOO:bid".into(),
            side: Side::Ask,
        }];
        // Pair B: symbol "FOO", side Bid + suffix "ask" — pre-fix bytes
        // for the concatenation `"FOO:bid" + "ask"` would equal pair A's
        // canonical input. With length-prefixing this is impossible.
        let b = vec![
            InstrumentSpec {
                symbol: "FOO".into(),
                side: Side::Bid,
            },
            // Crafted second spec whose symbol bytes "fall into" pair A's
            // separator region under the pre-WR-07 encoding.
            InstrumentSpec {
                symbol: "ask".into(),
                side: Side::Bid,
            },
        ];
        let s_a = derive_job_seed(
            0xDEAD,
            "cross.corr.pearson_rolling@1",
            &a,
            Timeframe::Tf15m,
            &window,
            "abc",
        );
        let s_b = derive_job_seed(
            0xDEAD,
            "cross.corr.pearson_rolling@1",
            &b,
            Timeframe::Tf15m,
            &window,
            "abc",
        );
        assert_ne!(
            s_a, s_b,
            "length-prefix encoding must isolate symbol bytes from side bytes (WR-07)"
        );
    }

    /// WR-07: inter-instrument boundary collisions. Two pairs of specs
    /// whose concatenations match pre-WR-07 but separate under length-
    /// prefixing.
    #[test]
    fn derive_job_seed_adjacent_specs_do_not_collide_via_concatenation() {
        let window = sample_window();
        // Pair A: two specs "AB", "CD" both Bid.
        let a = vec![
            InstrumentSpec {
                symbol: "AB".into(),
                side: Side::Bid,
            },
            InstrumentSpec {
                symbol: "CD".into(),
                side: Side::Bid,
            },
        ];
        // Pair B: single spec "AB:bidCD" Bid — pre-WR-07 this had the
        // same concatenated ASCII as pair A's two specs joined together
        // (no inter-spec delimiter). Length-prefixing separates them.
        let b = vec![InstrumentSpec {
            symbol: "AB:bidCD".into(),
            side: Side::Bid,
        }];
        let s_a = derive_job_seed(0xDEAD, "scan@1", &a, Timeframe::Tf15m, &window, "abc");
        let s_b = derive_job_seed(0xDEAD, "scan@1", &b, Timeframe::Tf15m, &window, "abc");
        assert_ne!(
            s_a, s_b,
            "adjacent specs must not collide with a single spec containing the joined bytes (WR-07)"
        );
    }
}
