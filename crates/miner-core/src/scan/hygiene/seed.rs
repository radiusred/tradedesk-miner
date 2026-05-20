//! Per-job seed derivation — `derive_job_seed` (Plan 05-02 / D5-05 / HYG-05).
//!
//! Pattern analog: `crate::engine::param_hash::param_hash` (same crate-internal
//! `blake3::Hasher` discipline + `Blake3Hex` convention from Phase 2 D2-05).
//! Where `param_hash` produces a 64-char hex `Blake3Hex`, `derive_job_seed`
//! collapses the 32-byte blake3 hash to a `u64` via little-endian read of the
//! first 8 bytes — the right shape for `Xoshiro256PlusPlus::seed_from_u64`.
//!
//! ## Canonicalisation rules (HYG-05 / 05-PATTERNS lines 378-386)
//!
//! The hash input is the byte concatenation of, in this order:
//!
//! 1. `master_seed.to_le_bytes()` — little-endian (matches the `u64` wire
//!    convention used everywhere else in `miner-core`).
//! 2. `scan_id_at_version.as_bytes()` — raw `"scan_id@version"` ASCII.
//! 3. For each `InstrumentSpec`, in vector order:
//!    `format!("{}:{}", spec.symbol, spec.side.as_str()).as_bytes()`.
//! 4. `timeframe.as_str().as_bytes()` — `"15m"` / `"1h"` / `"1d"`.
//! 5. `format!("{}/{}", window.start.to_rfc3339(), window.end.to_rfc3339()).as_bytes()`.
//! 6. `param_hash.as_bytes()` — the existing Phase 2 `Blake3Hex` hex string.
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
    let mut hasher = Hasher::new();
    hasher.update(&master_seed.to_le_bytes());
    hasher.update(scan_id_at_version.as_bytes());
    for spec in instruments {
        hasher.update(format!("{}:{}", spec.symbol, spec.side.as_str()).as_bytes());
    }
    hasher.update(timeframe.as_str().as_bytes());
    hasher.update(format!("{}/{}", window.start.to_rfc3339(), window.end.to_rfc3339()).as_bytes());
    hasher.update(param_hash.as_bytes());
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
}
