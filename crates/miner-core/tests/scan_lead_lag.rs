//! Phase 4 (Plan 04-08 Task 1) integration test — CROSS-04 lead-lag CCF
//! scan.
//!
//! Drives `LeadLagCcfScan` directly via `Scan::run` against a two-leg
//! deterministic seeded fixture. Pattern J analog: `scan_corr_rolling.rs`,
//! `scan_ols_rolling.rs`.

mod common;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use chrono::{Duration, TimeZone, Utc};

use miner_core::aggregator::{BarFrame, Timeframe};
use miner_core::engine::gap_policy::GapPolicyKind;
use miner_core::engine::param_hash;
use miner_core::findings::{Finding, RunId, TimeRange};
use miner_core::reader::{ClosedRangeUtc, InstrumentSpec, Side};
use miner_core::scan::cross::LeadLagCcfScan;
use miner_core::scan::{Scan, ScanCtx};

use common::BufferSink;

fn lcg_closes(n: usize, seed: u64) -> Vec<f64> {
    #[allow(clippy::cast_possible_truncation)]
    let mut s = seed as u32;
    let mut closes = Vec::with_capacity(n);
    for _ in 0..n {
        s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let frac = f64::from(s) / f64::from(u32::MAX);
        closes.push(1.0 + frac);
    }
    closes
}

fn build_bars(symbol: &str, ts: &[chrono::DateTime<Utc>], closes: &[f64]) -> BarFrame {
    assert_eq!(ts.len(), closes.len());
    BarFrame {
        source_id: "dukascopy".into(),
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

/// Plan 04-08 Task 1 integration test — `scan_lead_lag_happy_path` for the
/// lead-lag CCF scan. The scan emits exactly one `Finding::Result` envelope
/// with Pair-arity shape (data_slice.sources.len() == 2 + leg-labelled
/// raw.series keys + ccf_values / lags vectors of length 2*max_lag+1 in
/// effect.extra).
#[test]
fn scan_lead_lag_happy_path() {
    let n = 100;
    let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let ts: Vec<chrono::DateTime<Utc>> = (0..n)
        .map(|i| {
            let i_i64 = i64::try_from(i).expect("n fits");
            start + Duration::minutes(15 * i_i64)
        })
        .collect();
    let closes_a = lcg_closes(n, 17);
    let closes_b = lcg_closes(n, 29);
    let a = build_bars("EURUSD", &ts, &closes_a);
    let b = build_bars("GBPUSD", &ts, &closes_b);

    let resolved_params = serde_json::json!({"max_lag": 5});
    let param_hash = param_hash::param_hash(&resolved_params).expect("hash ok");
    let window_start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let window_end = Utc.with_ymd_and_hms(2024, 1, 2, 0, 0, 0).unwrap();

    let req = miner_core::scan::ScanRequest {
        scan_id: "cross.lead_lag.ccf".into(),
        version: 1,
        instruments: vec![
            InstrumentSpec {
                symbol: "EURUSD".into(),
                side: Side::Bid,
            },
            InstrumentSpec {
                symbol: "GBPUSD".into(),
                side: Side::Bid,
            },
        ],
        timeframe: Timeframe::Tf15m,
        window: ClosedRangeUtc {
            start: window_start,
            end: window_end,
        },
        sub_range: TimeRange {
            start_utc: window_start,
            end_utc: window_end,
        },
        gap_policy: GapPolicyKind::ContinuousOnly,
        resolved_params,
        param_hash,
        dry_run: false,
        sleep_after_first_finding_ms: None,
    };

    let ctx = ScanCtx {
        bars: &a,
        bars_pair: Some((&a, &b)),
        gap_manifest: None,
        run_id: RunId::new(),
        code_revision: "test-rev",
        cancel: Arc::new(AtomicBool::new(false)),
        sleep_after_first_finding_ms: None,
    };

    let mut sink = BufferSink::new();
    LeadLagCcfScan
        .run(&ctx, &req, &mut sink)
        .expect("lead-lag run ok");

    let findings = common::parse_findings(&sink.0);
    assert_eq!(findings.len(), 1, "exactly one Result envelope");
    let Finding::Result(r) = &findings[0] else {
        panic!("expected Result; got {:?}", findings[0]);
    };

    assert_eq!(r.scan_id_at_version, "cross.lead_lag.ccf@1");
    assert_eq!(r.effect.metric, "lead_lag_argmax_lag");
    // n == returns count = aligned_n - 1 = 99 for n=100 fully overlapping bars.
    assert_eq!(r.effect.n, Some(99));
    // effect.value is integer-valued argmax_lag.
    assert_eq!(r.effect.value, r.effect.value.trunc());

    // D4-03: two-leg sources Vec.
    assert_eq!(r.data_slice.sources.len(), 2);
    assert_eq!(r.data_slice.sources[0].symbol, "EURUSD");
    assert_eq!(r.data_slice.sources[1].symbol, "GBPUSD");

    // raw.series — leg-labelled returns + canonical timestamps key.
    let raw = r.raw.as_ref().expect("raw present");
    assert!(raw.series.contains_key("returns_a"));
    assert!(raw.series.contains_key("returns_b"));
    assert!(raw.series.contains_key("timestamps_ms"));

    // effect.extra carries the documented 5 keys.
    for key in ["argmax_lag", "argmax_value", "ccf_values", "lags", "max_lag"] {
        assert!(
            r.effect.extra.contains_key(key),
            "effect.extra missing {key}"
        );
    }

    // ccf_values + lags have length 2*max_lag + 1 = 11.
    let ccf_bytes = &r.effect.extra["ccf_values"].data.0;
    assert_eq!(ccf_bytes.len(), 11 * 8);
    let lags_bytes = &r.effect.extra["lags"].data.0;
    assert_eq!(lags_bytes.len(), 11 * 8);
}
