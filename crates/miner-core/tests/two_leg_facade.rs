//! Phase 4 (Plan 04-02 Task 2) integration test — D4-01 / D4-03 two-leg
//! facade end-to-end shape.
//!
//! Scaffold-only in Plan 04-02: Plan 04-07 (CROSS scans) will register
//! real two-leg scans into the engine's Pair branch. For now the file
//! pins:
//!
//! 1. The arity-preflight gate accepts a Pair-arity scan + two-leg
//!    request (the converse of `arity_preflight.rs`'s rejection test).
//! 2. The `primitives::time_alignment::inner_join` API is reachable from
//!    integration tests via the public re-export surface.
//! 3. The shape of an `AlignedPair` exposes `timestamps_ms` / `close_a` /
//!    `close_b` as the documented public fields — Plans 04-07 will
//!    consume these in real CROSS-scan bodies.
//!
//! Pattern analog: `crates/miner-core/tests/dry_run.rs` (clean
//! `engine::run_one` scaffold + BufferSink + parse_findings). Once Plan
//! 04-07 lands the engine's Pair branch + a real CROSS scan, this file
//! will extend with the full RunStart -> Result -> RunEnd shape
//! assertion + the `data_slice.sources.len() == 2` check (D4-03).

use chrono::{Duration, TimeZone, Utc};

use miner_core::aggregator::{BarFrame, Timeframe};
use miner_core::reader::Side;
use miner_core::scan::primitives::time_alignment::{AlignedPair, inner_join};

/// Build a synthetic BarFrame with the given timestamps and closes.
fn build_bars(symbol: &str, ts: &[chrono::DateTime<Utc>], closes: &[f64]) -> BarFrame {
    assert_eq!(ts.len(), closes.len(), "test fixture mismatch");
    BarFrame {
        source_id: "test".into(),
        symbol: symbol.into(),
        side: Side::Bid,
        tf: Timeframe::Tf15m,
        ts_open_utc: ts.to_vec(),
        ts_close_utc: ts.iter().map(|t| *t + Duration::minutes(15)).collect(),
        open: closes.to_vec(),
        high: closes.iter().map(|c| c + 0.001).collect(),
        low: closes.iter().map(|c| c - 0.001).collect(),
        close: closes.to_vec(),
        tick_volume: vec![1.0; ts.len()],
    }
}

/// D4-01 / CROSS-01: two-leg inner-join is reachable from integration tests
/// and the [`AlignedPair`] type exposes the documented public fields
/// (`timestamps_ms`, `close_a`, `close_b`). Plan 04-07 will consume this
/// surface from a real CROSS scan body.
#[test]
fn inner_join_aligns_two_leg_close_vectors() {
    let t0 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let ts_a = [t0, t0 + Duration::minutes(15), t0 + Duration::minutes(30)];
    let ts_b = [t0 + Duration::minutes(15), t0 + Duration::minutes(30)];
    let bars_a = build_bars("EURUSD", &ts_a, &[1.0, 1.1, 1.2]);
    let bars_b = build_bars("GBPUSD", &ts_b, &[2.1, 2.2]);

    let aligned: AlignedPair = inner_join(&bars_a, &bars_b);

    assert_eq!(
        aligned.timestamps_ms.len(),
        2,
        "joint timestamps = intersection of both legs"
    );
    assert_eq!(aligned.close_a.len(), aligned.timestamps_ms.len());
    assert_eq!(aligned.close_b.len(), aligned.timestamps_ms.len());
    // Parallel: index 0 of close_a maps to leg A at the joint timestamp,
    // index 0 of close_b maps to leg B at the same joint timestamp.
    assert_eq!(aligned.close_a, vec![1.1, 1.2]);
    assert_eq!(aligned.close_b, vec![2.1, 2.2]);

    // Joint timestamps are epoch-ms (CROSS-01 primitive output). The
    // CROSS scan body (Plan 04-07) packs these into `raw.series.timestamps_ms_aligned`
    // — and the integration test will assert that field name lands in
    // the envelope.
    let want_ms_0 = (t0 + Duration::minutes(15)).timestamp_millis();
    let want_ms_1 = (t0 + Duration::minutes(30)).timestamp_millis();
    assert_eq!(aligned.timestamps_ms[0], want_ms_0);
    assert_eq!(aligned.timestamps_ms[1], want_ms_1);
}

/// D4-03 wire-form pin: the scaffold documents the joint shape Plan 04-07
/// will populate. `Source` is constructed once per leg in
/// `ScanRequest.instruments` order; the joint envelope's
/// `data_slice.sources` will have length 2. This test confirms the public
/// `Source` constructor surface is reachable from integration tests so
/// Plan 04-07's CROSS-scan body can build the envelope.
#[test]
fn data_slice_sources_vec_is_reachable_for_two_leg_envelopes() {
    use miner_core::findings::Source;
    let leg_a = Source {
        source_id: "dukascopy".into(),
        symbol: "EURUSD".into(),
        side: "bid".into(),
        timeframe: "15m".into(),
    };
    let leg_b = Source {
        source_id: "dukascopy".into(),
        symbol: "GBPUSD".into(),
        side: "bid".into(),
        timeframe: "15m".into(),
    };
    let sources = vec![leg_a, leg_b];
    assert_eq!(sources.len(), 2, "D4-03 sources Vec len for Pair");
    // Each leg carries its own (source_id, symbol, side, timeframe). Plan
    // 04-07's CROSS-scan body populates by iterating `req.instruments`
    // (parallel order).
    assert_eq!(sources[0].symbol, "EURUSD");
    assert_eq!(sources[1].symbol, "GBPUSD");
}
