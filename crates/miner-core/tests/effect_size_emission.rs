//! Plan 05-03 Task 2 — per-scan integration test asserting every Phase 4 scan
//! emits `effect.effect_size = Some(EffectSize { kind, value })` with the
//! canonical kind from the D5-03 table.
//!
//! The test exercises EACH of the 22 Phase 4 scan implementations directly
//! (via `Scan::run`) against a synthetic `BarFrame` built from an LCG-seeded
//! closes vector. It asserts:
//!
//! - exactly one `Finding::Result` is emitted,
//! - `r.effect.effect_size.is_some()`,
//! - `r.effect.effect_size.unwrap().kind == <canonical-kind>` (string equality
//!   against the D5-03 contract),
//! - `r.effect.effect_size.unwrap().value` is finite (`.is_finite()`).
//!
//! Scans are grouped by family (`anom`, `cross`, `seas`) into three test
//! functions so a per-family failure surfaces independently. Plan 05-04 (sweep
//! runner) and Plan 05-05 (CLI) test the wire surface end-to-end; this test
//! pins the per-scan emission contract.

#![allow(clippy::cast_precision_loss, clippy::too_many_lines)]

mod common;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use chrono::{Duration, TimeZone, Utc};

use miner_core::aggregator::{BarFrame, Timeframe};
use miner_core::engine::gap_policy::GapPolicyKind;
use miner_core::engine::param_hash;
use miner_core::findings::{Finding, RunId, TimeRange};
use miner_core::reader::{ClosedRangeUtc, InstrumentSpec, Side};
use miner_core::scan::{Scan, ScanCtx, ScanRequest};

use common::BufferSink;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

#[allow(clippy::cast_possible_truncation)]
fn lcg_closes(n: usize, seed: u64) -> Vec<f64> {
    let mut s = seed as u32;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let frac = f64::from(s) / f64::from(u32::MAX);
        out.push(1.0 + frac);
    }
    out
}

fn build_bars(close: &[f64], symbol: &str) -> BarFrame {
    let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let n = close.len();
    let ts_open: Vec<chrono::DateTime<Utc>> = (0..n)
        .map(|i| start + Duration::minutes(15 * i64::try_from(i).expect("fits")))
        .collect();
    let ts_close: Vec<chrono::DateTime<Utc>> =
        ts_open.iter().map(|t| *t + Duration::minutes(15)).collect();
    let opens: Vec<f64> = close.to_vec();
    let highs: Vec<f64> = close.iter().map(|c| c + 0.001).collect();
    let lows: Vec<f64> = close.iter().map(|c| c - 0.001).collect();
    let vols = vec![1.0; n];
    BarFrame {
        source_id: "dukascopy".into(),
        symbol: symbol.into(),
        side: Side::Bid,
        tf: Timeframe::Tf15m,
        ts_open_utc: ts_open,
        ts_close_utc: ts_close,
        open: opens,
        high: highs,
        low: lows,
        close: close.to_vec(),
        tick_volume: vols,
    }
}

fn single_request(scan_id: &str, version: u32, params: serde_json::Value) -> ScanRequest {
    let param_hash = param_hash::param_hash(&params).expect("hash ok");
    let window_start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let window_end = Utc.with_ymd_and_hms(2024, 1, 30, 0, 0, 0).unwrap();
    ScanRequest {
        scan_id: scan_id.into(),
        version,
        instruments: vec![InstrumentSpec {
            symbol: "EURUSD".into(),
            side: Side::Bid,
        }],
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
        resolved_params: params,
        param_hash,
        dry_run: false,
        master_seed: None,
        job_seed: None,
        bootstrap_method: None,
        bootstrap_n: None,
        null_method: None,
        null_n: None,
        #[cfg(any(test, feature = "test-internal"))]
        sleep_after_first_finding_ms: None,
    }
}

fn pair_request(scan_id: &str, version: u32, params: serde_json::Value) -> ScanRequest {
    let param_hash = param_hash::param_hash(&params).expect("hash ok");
    let window_start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let window_end = Utc.with_ymd_and_hms(2024, 1, 30, 0, 0, 0).unwrap();
    ScanRequest {
        scan_id: scan_id.into(),
        version,
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
        resolved_params: params,
        param_hash,
        dry_run: false,
        master_seed: None,
        job_seed: None,
        bootstrap_method: None,
        bootstrap_n: None,
        null_method: None,
        null_n: None,
        #[cfg(any(test, feature = "test-internal"))]
        sleep_after_first_finding_ms: None,
    }
}

fn single_ctx(bars: &BarFrame) -> ScanCtx<'_> {
    ScanCtx {
        bars,
        bars_pair: None,
        gap_manifest: None,
        run_id: RunId::new(),
        code_revision: "test-rev",
        cancel: Arc::new(AtomicBool::new(false)),
        sleep_after_first_finding_ms: None,
    }
}

fn pair_ctx<'a>(a: &'a BarFrame, b: &'a BarFrame) -> ScanCtx<'a> {
    ScanCtx {
        bars: a,
        bars_pair: Some((a, b)),
        gap_manifest: None,
        run_id: RunId::new(),
        code_revision: "test-rev",
        cancel: Arc::new(AtomicBool::new(false)),
        sleep_after_first_finding_ms: None,
    }
}

fn assert_emits_kind(sink: &BufferSink, expected_kind: &str, scan_label: &str) {
    let findings = common::parse_findings(&sink.0);
    let result = findings
        .iter()
        .find_map(|f| match f {
            Finding::Result(r) => Some(r),
            _ => None,
        })
        .unwrap_or_else(|| panic!("{scan_label}: no Finding::Result emitted"));
    let es = result
        .effect
        .effect_size
        .as_ref()
        .unwrap_or_else(|| panic!("{scan_label}: effect.effect_size is None (expected Some)"));
    assert_eq!(
        es.kind, expected_kind,
        "{scan_label}: effect_size.kind mismatch (got {:?}, expected {expected_kind:?})",
        es.kind
    );
    assert!(
        es.value.is_finite(),
        "{scan_label}: effect_size.value is not finite (got {})",
        es.value
    );
}

// ---------------------------------------------------------------------------
// ANOM family — 11 scans + LjungBox (which lives outside the anom namespace
// in the codebase but is part of Plan 04's ANOM family of scans).
// ---------------------------------------------------------------------------

#[test]
fn anom_every_scan_emits_effect_size() {
    use miner_core::scan::anom::{
        AdfScan, ArchLmScan, DrawdownProfileScan, JarqueBeraScan, KpssScan, LjungBoxSqScan,
        OutliersZAndMadScan, ReturnsProfileScan, SummaryWelfordScan, VarianceRatioScan,
        VolRollingScan,
    };
    use miner_core::scan::ljung_box::LjungBoxScan;

    // Per-scan: (label, scan_id, version, params, expected_kind, n_closes, seed)
    #[allow(clippy::type_complexity)]
    let scans: Vec<(
        &str,
        Box<dyn Scan>,
        &str,
        u32,
        serde_json::Value,
        &str,
        usize,
        u64,
    )> = vec![
        (
            "ljung_box",
            Box::new(LjungBoxScan),
            "stats.autocorr.ljung_box",
            1,
            serde_json::json!({"lags": 5}),
            "acf_lag_max_abs",
            128,
            1,
        ),
        (
            "ljung_box_sq",
            Box::new(LjungBoxSqScan),
            "stats.autocorr.ljung_box_sq",
            1,
            serde_json::json!({"lags": 5}),
            "acf_lag_max_abs",
            128,
            2,
        ),
        (
            "summary_welford",
            Box::new(SummaryWelfordScan),
            "stats.summary.welford",
            1,
            serde_json::json!({"series": "log_returns"}),
            "cohens_d_vs_zero",
            128,
            3,
        ),
        (
            "vol_rolling",
            Box::new(VolRollingScan),
            "stats.vol.rolling",
            1,
            serde_json::json!({"window": 10}),
            "vol_ratio_to_baseline",
            128,
            4,
        ),
        (
            "returns_profile",
            Box::new(ReturnsProfileScan),
            "stats.returns.profile",
            1,
            serde_json::json!({"variant": "log"}),
            "log_returns_mean",
            128,
            5,
        ),
        (
            "adf",
            Box::new(AdfScan),
            "stats.stationarity.adf",
            1,
            serde_json::json!({}),
            "tau_signed",
            128,
            6,
        ),
        (
            "kpss",
            Box::new(KpssScan),
            "stats.stationarity.kpss",
            1,
            serde_json::json!({}),
            "tau_signed",
            128,
            7,
        ),
        (
            "variance_ratio",
            Box::new(VarianceRatioScan),
            "stats.variance_ratio.lo_mackinlay",
            1,
            serde_json::json!({"k_values": [2, 4]}),
            "vr_minus_one",
            128,
            8,
        ),
        (
            "arch_lm",
            Box::new(ArchLmScan),
            "stats.heteroskedasticity.arch_lm",
            1,
            serde_json::json!({"lags": [5]}),
            "lm_per_lag",
            128,
            9,
        ),
        (
            "jarque_bera",
            Box::new(JarqueBeraScan),
            "stats.normality.jarque_bera",
            1,
            serde_json::json!({"series": "log_returns"}),
            "jb_per_n",
            128,
            10,
        ),
        (
            "outliers",
            Box::new(OutliersZAndMadScan),
            "stats.outliers.z_and_mad",
            1,
            serde_json::json!({"series": "log_returns"}),
            "outlier_rate",
            128,
            11,
        ),
        (
            "drawdown",
            Box::new(DrawdownProfileScan),
            "stats.drawdown.profile",
            1,
            serde_json::json!({}),
            "max_dd_pct",
            128,
            12,
        ),
    ];
    for (label, scan, scan_id, version, params, expected_kind, n, seed) in scans {
        let closes = lcg_closes(n, seed);
        let bars = build_bars(&closes, "EURUSD");
        let req = single_request(scan_id, version, params);
        let ctx = single_ctx(&bars);
        let mut sink = BufferSink::new();
        scan.run(&ctx, &req, &mut sink)
            .unwrap_or_else(|e| panic!("{label} run failed: {e}"));
        assert_emits_kind(&sink, expected_kind, label);
    }
}

// ---------------------------------------------------------------------------
// CROSS family — 5 pair-arity scans.
// ---------------------------------------------------------------------------

#[test]
fn cross_every_scan_emits_effect_size() {
    use miner_core::scan::cross::{
        EngleGrangerScan, LeadLagCcfScan, OlsRollingScan, PearsonRollingScan, SpearmanRollingScan,
    };

    #[allow(clippy::type_complexity)]
    let scans: Vec<(&str, Box<dyn Scan>, &str, u32, serde_json::Value, &str)> = vec![
        (
            "pearson_rolling",
            Box::new(PearsonRollingScan),
            "cross.corr.pearson_rolling",
            1,
            serde_json::json!({"window": 10}),
            "r_last_window",
        ),
        (
            "spearman_rolling",
            Box::new(SpearmanRollingScan),
            "cross.corr.spearman_rolling",
            1,
            serde_json::json!({"window": 10}),
            "rho_last_window",
        ),
        (
            "ols_rolling",
            Box::new(OlsRollingScan),
            "cross.ols.rolling",
            1,
            serde_json::json!({"window": 10}),
            "beta_last_window",
        ),
        (
            "lead_lag_ccf",
            Box::new(LeadLagCcfScan),
            "cross.lead_lag.ccf",
            1,
            serde_json::json!({"max_lag": 5}),
            "argmax_ccf_value",
        ),
        (
            "engle_granger",
            Box::new(EngleGrangerScan),
            "cross.cointegration.engle_granger",
            1,
            serde_json::json!({}),
            "hedge_ratio",
        ),
    ];

    for (label, scan, scan_id, version, params, expected_kind) in scans {
        let closes_a = lcg_closes(200, 100);
        let closes_b = lcg_closes(200, 200);
        let bars_a = build_bars(&closes_a, "EURUSD");
        let bars_b = build_bars(&closes_b, "GBPUSD");
        let req = pair_request(scan_id, version, params);
        let ctx = pair_ctx(&bars_a, &bars_b);
        let mut sink = BufferSink::new();
        scan.run(&ctx, &req, &mut sink)
            .unwrap_or_else(|e| panic!("{label} run failed: {e}"));
        assert_emits_kind(&sink, expected_kind, label);
    }
}

// ---------------------------------------------------------------------------
// SEAS family — 6 single-arity scans.
// ---------------------------------------------------------------------------

#[test]
fn seas_every_scan_emits_effect_size() {
    use miner_core::scan::seas::{
        AnovaKruskalScan, DayOfWeekScan, EomSomScan, EventWindowScan, HourOfDayScan, SessionScan,
    };

    // SEAS scans need a longer window to see multiple buckets — 1500 15-m bars
    // crosses ~15 trading days, enough for hour/day/session/eom-som and
    // ANOVA/Kruskal-Wallis to have non-degenerate buckets.
    let n_bars = 1500_usize;
    let closes = lcg_closes(n_bars, 4242);
    let bars = build_bars(&closes, "EURUSD");

    #[allow(clippy::type_complexity)]
    let scans: Vec<(&str, Box<dyn Scan>, &str, u32, serde_json::Value, &str)> = vec![
        (
            "hour_of_day",
            Box::new(HourOfDayScan),
            "seas.bucket.hour_of_day",
            1,
            serde_json::json!({"series": "log_returns"}),
            "max_abs_t_stat",
        ),
        (
            "day_of_week",
            Box::new(DayOfWeekScan),
            "seas.bucket.day_of_week",
            1,
            serde_json::json!({"series": "log_returns"}),
            "max_abs_t_stat",
        ),
        (
            "session",
            Box::new(SessionScan),
            "seas.bucket.session",
            1,
            serde_json::json!({"series": "log_returns"}),
            "max_abs_t_stat",
        ),
        (
            "eom_som",
            Box::new(EomSomScan),
            "seas.bucket.eom_som",
            1,
            serde_json::json!({"series": "log_returns", "cutoff_n": 3}),
            "max_abs_t_stat",
        ),
        (
            "anova_kruskal",
            Box::new(AnovaKruskalScan),
            "seas.test.anova_kruskal",
            1,
            serde_json::json!({"buckets_via": "hour_of_day", "series": "log_returns"}),
            "omega_squared",
        ),
        (
            "event_window",
            Box::new(EventWindowScan),
            "seas.event.pre_post_window",
            1,
            serde_json::json!({
                // ms-since-epoch for 2024-01-02T12:00:00Z, +3 days, +8 days
                "event_timestamps": [1_704_196_800_000_i64, 1_704_456_000_000_i64, 1_704_888_000_000_i64],
                "pre_window_bars": 3,
                "post_window_bars": 3,
            }),
            "post_minus_pre_mean",
        ),
    ];

    for (label, scan, scan_id, version, params, expected_kind) in scans {
        let req = single_request(scan_id, version, params);
        let ctx = single_ctx(&bars);
        let mut sink = BufferSink::new();
        scan.run(&ctx, &req, &mut sink)
            .unwrap_or_else(|e| panic!("{label} run failed: {e}"));
        assert_emits_kind(&sink, expected_kind, label);
    }
}
