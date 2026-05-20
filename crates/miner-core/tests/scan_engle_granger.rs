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

// ---------------------------------------------------------------------------
// Plan 04-12 (CR-01) — engine-facade variant of the happy-path test.
//
// The direct-ScanCtx test above pins the kernel (CROSS-05's OLS + ADF + OU
// half-life pipeline against a hand-crafted cointegrated fixture). This
// engine-facade variant pins the dispatch wiring: drive the scan through
// `engine::run_one_with_registry` against a SyntheticCache so a future
// regression of the Pair-arity branch trips here too.
// ---------------------------------------------------------------------------

/// CR-01 engine-facade variant of `scan_engle_granger_happy_path`.
#[test]
fn scan_engle_granger_happy_path_via_engine_facade() {
    use chrono::NaiveDate;
    use miner_core::config::{MinerConfig, OutputDest};
    use miner_core::engine::{RunOutcome, run_one_with_registry};
    use miner_core::scan::Registry;
    use miner_reader_dukascopy::DukascopyReader;

    use common::synthetic_cache::SyntheticCache;

    let day = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap();
    // Different seeds per leg so the inputs aren't degenerate; we only
    // assert envelope shape here (the kernel's correctness is pinned by
    // the direct-ScanCtx test above against the hand-crafted cointegrated
    // fixture).
    let cache = SyntheticCache::new()
        .with_deterministic_day("EURUSD", Side::Bid, day, 0x1357_9BDF)
        .with_deterministic_day("GBPUSD", Side::Bid, day, 0x0ACE_F123);
    let cfg = MinerConfig {
        cache_root: cache.cache_root().to_path_buf(),
        bar_cache_root: cache.bar_cache_root().to_path_buf(),
        output: OutputDest::Stdout,
    };
    let reader = DukascopyReader::new(cache.cache_root());
    let mut registry = Registry::new();
    registry.register(Box::new(EngleGrangerScan));

    let start = day.and_hms_opt(0, 0, 0).unwrap().and_utc();
    let end = start + chrono::Duration::days(1);
    let resolved_params = serde_json::json!({"regression": "c"});
    let param_hash = param_hash::param_hash(&resolved_params).expect("hash ok");
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
        window: ClosedRangeUtc { start, end },
        sub_range: TimeRange {
            start_utc: start,
            end_utc: end,
        },
        gap_policy: GapPolicyKind::ContinuousOnly,
        resolved_params,
        param_hash,
        dry_run: false,
        sleep_after_first_finding_ms: None,
    };

    let mut sink = BufferSink::new();
    let outcome = run_one_with_registry(
        &req,
        &cfg,
        &reader,
        &mut sink,
        Arc::new(AtomicBool::new(false)),
        &registry,
    )
    .expect("engine::run_one_with_registry ok");
    assert_eq!(outcome, RunOutcome::Ok);

    let findings = common::parse_findings(&sink.0);
    for f in &findings {
        if let Finding::ScanError(se) = f {
            assert!(
                !se.message.contains("expected Pair arity"),
                "CR-01 regression: {:?}",
                se.message
            );
        }
    }
    let result = findings
        .iter()
        .find_map(|f| match f {
            Finding::Result(r) => Some(r),
            _ => None,
        })
        .expect("Result envelope present");
    assert_eq!(result.scan_id_at_version, "cross.cointegration.engle_granger@1");
    assert_eq!(result.effect.metric, "engle_granger_hedge_ratio");
    assert_eq!(result.data_slice.sources.len(), 2);
    assert_eq!(result.data_slice.sources[0].symbol, "EURUSD");
    assert_eq!(result.data_slice.sources[1].symbol, "GBPUSD");
    assert!(result.effect.value.is_finite());
    assert!(result.effect.p_value.is_some());
}

// ---------------------------------------------------------------------------
// Plan 04-11 Task 1 — golden cross-check (#[ignore]d until pinned regen)
// ---------------------------------------------------------------------------

/// Phase 4 Plan 04-11 Task 1 golden cross-check — CROSS-05.
///
/// Loads `crates/miner-core/tests/goldens/cross.cointegration.engle_granger.jsonl`,
/// builds two BarFrames with the EXACT same LCG recipe the integration
/// test's happy-path uses, runs `EngleGrangerScan::run`, and asserts
/// per-RESEARCH §Section 2 tolerances:
/// - 1e-10 on β (hedge_ratio_beta), α (hedge_ratio_alpha), residuals
/// - 1e-8  on adf_stat + adf_p_value (MacKinnon table approximation)
/// - 1e-10 on residual_std (sample std, pure arithmetic)
///
/// Pattern J Step 1 — provenance gate. The test refuses to run unless
/// `provenance.statsmodels_version == "0.14.6"`. The stub golden checked
/// in at Plan 04-11 has `provenance.statsmodels_version == "STUB"`;
/// this test is `#[ignore]`d until a developer runs the regen recipe
/// documented in `goldens/REFERENCE-VERSIONS.md`.
#[test]
#[ignore = "Phase 4 Plan 04-11: golden is a STUB until pinned Python 3.11 \
            + statsmodels==0.14.6 venv regenerates \
            cross.cointegration.engle_granger.jsonl"]
fn engle_granger_matches_statsmodels_coint_golden() {
    const GOLDEN_JSON: &str = include_str!("goldens/cross.cointegration.engle_granger.jsonl");
    const TOL_BETA: f64 = 1e-10;
    const TOL_ADF: f64 = 1e-8;

    let golden: serde_json::Value = serde_json::from_str(GOLDEN_JSON.trim())
        .expect("cross.cointegration.engle_granger.jsonl must be valid JSON");

    // Pattern J Step 1 — provenance gate.
    let prov = golden["provenance"]["statsmodels_version"].as_str();
    assert_eq!(
        prov,
        Some("0.14.6"),
        "engle_granger golden provenance.statsmodels_version must be \"0.14.6\"; got {prov:?}. \
         Regenerate via `python crates/miner-core/tests/goldens/generate_engle_granger.py > crates/miner-core/tests/goldens/cross.cointegration.engle_granger.jsonl` \
         in a pinned venv per crates/miner-core/tests/goldens/REFERENCE-VERSIONS.md.",
    );

    // Build the EXACT same two legs as `scan_engle_granger_happy_path`.
    let n = 200;
    let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let ts: Vec<chrono::DateTime<Utc>> = (0..n)
        .map(|i| {
            let i_i64 = i64::try_from(i).expect("n fits");
            start + Duration::minutes(15 * i_i64)
        })
        .collect();
    let mut s_b: u32 = 0x1357_9BDF;
    let mut closes_b = Vec::with_capacity(n);
    let mut acc = 1.0_f64;
    for _ in 0..n {
        s_b = s_b.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let dx = (f64::from(s_b) / f64::from(u32::MAX) - 0.5) * 0.01;
        acc += dx;
        closes_b.push(acc);
    }
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
        .expect("engle-granger ok");
    let findings = common::parse_findings(&sink.0);
    let Finding::Result(r) = &findings[0] else {
        panic!("expected Result");
    };

    let exp = &golden["expected"];
    let beta_actual = r.effect.value;
    let beta_expected = exp["hedge_ratio_beta"].as_f64().expect("β f64");
    let alpha_actual = decode_scalar_eg(&r.effect.extra, "hedge_ratio_alpha");
    let alpha_expected = exp["hedge_ratio_alpha"].as_f64().expect("α f64");
    let adf_actual = decode_scalar_eg(&r.effect.extra, "adf_stat");
    let adf_expected = exp["adf_stat"].as_f64().expect("adf_stat f64");
    let adf_p_actual = r.effect.p_value.expect("p_value present");
    let adf_p_expected = exp["adf_p_value"].as_f64().expect("adf_p_value f64");
    let residual_std_actual = decode_scalar_eg(&r.effect.extra, "residual_std");
    let residual_std_expected = exp["residual_std"].as_f64().expect("residual_std f64");

    let assert_within = |label: &str, actual: f64, expected: f64, tol: f64| {
        let diff = (actual - expected).abs();
        assert!(
            diff <= tol,
            "{label}: |{actual} - {expected}| = {diff:.3e} exceeds tolerance {tol:.3e}",
        );
    };
    assert_within("β", beta_actual, beta_expected, TOL_BETA);
    assert_within("α", alpha_actual, alpha_expected, TOL_BETA);
    assert_within("adf_stat", adf_actual, adf_expected, TOL_ADF);
    assert_within("adf_p_value", adf_p_actual, adf_p_expected, TOL_ADF);
    assert_within(
        "residual_std",
        residual_std_actual,
        residual_std_expected,
        TOL_BETA,
    );

    // Element-by-element residuals comparison (length n = 200).
    let residuals_actual = decode_vec_eg(&r.effect.extra, "residuals");
    let residuals_expected: Vec<f64> = exp["residuals"]
        .as_array()
        .expect("residuals array")
        .iter()
        .map(|v| v.as_f64().expect("residual f64"))
        .collect();
    assert_eq!(
        residuals_actual.len(),
        residuals_expected.len(),
        "residuals length mismatch",
    );
    for (i, (act, exp_v)) in residuals_actual
        .iter()
        .zip(residuals_expected.iter())
        .enumerate()
    {
        let diff = (act - exp_v).abs();
        assert!(
            diff <= TOL_BETA,
            "residuals[{i}]: |{act} - {exp_v}| = {diff:.3e} exceeds {TOL_BETA:.3e}",
        );
    }
}

fn decode_scalar_eg(
    extra: &std::collections::BTreeMap<String, miner_core::findings::RawArray>,
    key: &str,
) -> f64 {
    let arr = extra
        .get(key)
        .unwrap_or_else(|| panic!("effect.extra[{key}] present"));
    let bytes = &arr.data.0;
    assert_eq!(bytes.len(), 8, "{key}: expected length-1 f64 array");
    let mut buf = [0u8; 8];
    buf.copy_from_slice(&bytes[..8]);
    f64::from_le_bytes(buf)
}

fn decode_vec_eg(
    extra: &std::collections::BTreeMap<String, miner_core::findings::RawArray>,
    key: &str,
) -> Vec<f64> {
    let arr = extra
        .get(key)
        .unwrap_or_else(|| panic!("effect.extra[{key}] present"));
    let bytes = &arr.data.0;
    assert_eq!(bytes.len() % 8, 0, "{key}: byte length must be multiple of 8");
    let mut out = Vec::with_capacity(bytes.len() / 8);
    for chunk in bytes.chunks_exact(8) {
        let mut buf = [0u8; 8];
        buf.copy_from_slice(chunk);
        out.push(f64::from_le_bytes(buf));
    }
    out
}
