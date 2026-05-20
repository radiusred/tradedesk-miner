//! Phase 4 (Plan 04-08 Task 2) integration test — CROSS-05 Engle-Granger
//! two-step cointegration scan.
//!
//! Drives `EngleGrangerScan` directly via `Scan::run` against a two-leg
//! deterministic seeded fixture. Pattern J analog: `scan_lead_lag.rs`,
//! `scan_corr_rolling.rs`, `scan_ols_rolling.rs`.
//!
//! Engle-Granger operates on LEVELS (closes), not returns — the raw.series
//! block carries `{close_a, close_b, timestamps_ms}` (NOT `returns_a/b`).
//! Per D4-09 (RESEARCH.md §1.7): `y = leg_a = req.instruments[0]`,
//! `x = leg_b = req.instruments[1]`. Matches statsmodels.tsa.stattools.coint(y0, y1)
//! where y0 = leg_a and y1 = leg_b — so the reported β is β_y0_on_y1.

mod common;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use chrono::{Duration, TimeZone, Utc};

use miner_core::aggregator::{BarFrame, Timeframe};
use miner_core::engine::gap_policy::GapPolicyKind;
use miner_core::engine::param_hash;
use miner_core::findings::{Finding, RunId, TimeRange};
use miner_core::reader::{ClosedRangeUtc, InstrumentSpec, Side};
use miner_core::scan::cross::EngleGrangerScan;
use miner_core::scan::{Scan, ScanCtx};

use common::BufferSink;

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

/// Plan 04-08 Task 2 integration test — `scan_engle_granger_happy_path`.
///
/// Synthetic cointegrated pair: `close_a = close_b + ε` where ε is a
/// mean-reverting AR(1) residual with φ = 0.3. The scan emits exactly
/// one `Finding::Result` envelope with Pair-arity shape (data_slice.sources.len()
/// == 2 + leg-labelled raw.series keys carrying CLOSES + adf_p_value
/// surfaced through effect.p_value + the documented five effect.extra keys).
#[test]
fn scan_engle_granger_happy_path() {
    let n = 200;
    let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let ts: Vec<chrono::DateTime<Utc>> = (0..n)
        .map(|i| {
            let i_i64 = i64::try_from(i).expect("n fits");
            start + Duration::minutes(15 * i_i64)
        })
        .collect();
    // Build leg b as a deterministic random walk via LCG.
    let mut s_b: u32 = 0x1357_9BDF;
    let mut closes_b = Vec::with_capacity(n);
    let mut acc = 1.0_f64;
    for _ in 0..n {
        s_b = s_b.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let dx = (f64::from(s_b) / f64::from(u32::MAX) - 0.5) * 0.01;
        acc += dx;
        closes_b.push(acc);
    }
    // Build leg a as leg b + stationary AR(1) residual with φ = 0.3.
    let mut s_e: u32 = 0x0ACE_F123;
    let mut closes_a = Vec::with_capacity(n);
    let mut e_prev = 0.0_f64;
    for cb in &closes_b {
        s_e = s_e.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let noise = (f64::from(s_e) / f64::from(u32::MAX) - 0.5) * 0.005;
        let e_t = 0.3_f64 * e_prev + noise;
        closes_a.push(*cb + e_t);
        e_prev = e_t;
    }
    let a = build_bars("EURUSD", &ts, &closes_a);
    let b = build_bars("GBPUSD", &ts, &closes_b);

    let resolved_params = serde_json::json!({"regression": "c"});
    let param_hash = param_hash::param_hash(&resolved_params).expect("hash ok");
    let window_start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let window_end = Utc.with_ymd_and_hms(2024, 1, 5, 0, 0, 0).unwrap();

    let req = miner_core::scan::ScanRequest {
        scan_id: "cross.cointegration.engle_granger".into(),
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
    EngleGrangerScan
        .run(&ctx, &req, &mut sink)
        .expect("engle-granger run ok");

    let findings = common::parse_findings(&sink.0);
    assert_eq!(findings.len(), 1, "exactly one Result envelope");
    let Finding::Result(r) = &findings[0] else {
        panic!("expected Result; got {:?}", findings[0]);
    };

    assert_eq!(r.scan_id_at_version, "cross.cointegration.engle_granger@1");
    assert_eq!(r.effect.metric, "engle_granger_hedge_ratio");
    // n == aligned bars count = 200.
    assert_eq!(r.effect.n, Some(200));
    // β = effect.value. For close_a = close_b + small_residual, β should
    // be close to 1.0.
    assert!(
        (r.effect.value - 1.0).abs() < 0.05,
        "β = {} expected near 1.0 for close_a = close_b + small_residual",
        r.effect.value
    );
    // p_value is present (the headline ADF p-value).
    assert!(r.effect.p_value.is_some(), "p_value must be Some");
    let p_val = r.effect.p_value.expect("p_value present");
    assert!(p_val.is_finite(), "p_value must be finite; got {p_val}");

    // D4-03: two-leg sources Vec.
    assert_eq!(r.data_slice.sources.len(), 2);
    assert_eq!(r.data_slice.sources[0].symbol, "EURUSD");
    assert_eq!(r.data_slice.sources[1].symbol, "GBPUSD");

    // raw.series — leg-labelled CLOSES (NOT returns) + canonical timestamps key.
    let raw = r.raw.as_ref().expect("raw present");
    assert!(raw.series.contains_key("close_a"));
    assert!(raw.series.contains_key("close_b"));
    assert!(raw.series.contains_key("timestamps_ms"));

    // effect.extra carries the documented 5 keys.
    for key in [
        "adf_stat",
        "hedge_ratio_alpha",
        "ou_half_life",
        "residual_std",
        "residuals",
    ] {
        assert!(
            r.effect.extra.contains_key(key),
            "effect.extra missing {key}"
        );
    }

    // residuals vector length == aligned_n.
    let residuals_bytes = &r.effect.extra["residuals"].data.0;
    assert_eq!(residuals_bytes.len(), 200 * 8);
}
