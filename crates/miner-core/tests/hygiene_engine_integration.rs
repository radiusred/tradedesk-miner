//! Plan 05-03 continuation (`05-03-cont`) — engine hygiene-integration tests.
//!
//! These tests pin the deferred Task 3 wire-contract that Plan 05-03 left
//! unwired: post-`Scan::run` hygiene-kernel invocation in
//! `engine::run_one_with_registry` for bootstrap-CI population, null
//! p-value replacement, and `ReproEnvelope` population — plus the
//! `bootstrap_n` / `null_n` clamp at `100_000` and the byte-identical-rerun
//! invariant under hygiene-on flags.
//!
//! Scope per the continuation directive:
//!
//! - `LjungBox` (`stats.autocorr.ljung_box@1`) — single-arity, supports
//!   bootstrap + both null methods per D5-04. Used as the canonical
//!   hygiene-touched scan for all four tests.
//! - Welford (`stats.summary.welford@1`) — single-arity, supports bootstrap
//!   only. Used for the bootstrap-only-no-null variant.
//! - Outliers (`stats.outliers.z_and_mad@1`) — single-arity, supports
//!   neither bootstrap nor null. Used for the preflight-rejection negative
//!   path that Plan 05-03 already ships, restated here as a regression gate.
//!
//! Pair-arity hygiene (CROSS scans) is INTENTIONALLY out of scope for this
//! continuation — the dispatch table needs a joint-resample design that
//! Phase 7 will own. The Pair-arity preflight rejection still fires correctly
//! via `validate_hygiene_support`; the engine never reaches the in-flight
//! bootstrap/null path for Pair-arity scans because their opt-ins return
//! true but the dispatch closure is intentionally `None` for now.

#![allow(clippy::cast_precision_loss, clippy::too_many_lines)]

mod common;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use chrono::{Duration, NaiveDate};

use miner_core::aggregator::Timeframe;
use miner_core::config::{MinerConfig, OutputDest};
use miner_core::engine::gap_policy::GapPolicyKind;
use miner_core::engine::{param_hash, run_one_with_registry};
use miner_core::findings::{Finding, TimeRange};
use miner_core::reader::{ClosedRangeUtc, InstrumentSpec, Side};
use miner_core::scan::anom::{OutliersZAndMadScan, SummaryWelfordScan};
use miner_core::scan::ljung_box::LjungBoxScan;
use miner_core::scan::{BootstrapMethod, NullMethod, Registry, ScanRequest};
use miner_reader_dukascopy::DukascopyReader;

use common::{BufferSink, parse_findings, synthetic_cache::SyntheticCache};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn make_cfg(cache: &SyntheticCache) -> MinerConfig {
    MinerConfig {
        cache_root: cache.cache_root().to_path_buf(),
        bar_cache_root: cache.bar_cache_root().to_path_buf(),
        output: OutputDest::Stdout,
    }
}

/// Construct a single-instrument `LjungBox` request with explicit lags=5,
/// hygiene flags configurable via callback. The window is one calendar day
/// at 2024-01-02 so the `SyntheticCache` fixture lines up.
fn make_ljungbox_request(
    bootstrap: Option<BootstrapMethod>,
    bootstrap_n: Option<u32>,
    null: Option<NullMethod>,
    null_n: Option<u32>,
    master_seed: Option<u64>,
) -> (ScanRequest, NaiveDate) {
    let day = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap();
    let start = day.and_hms_opt(0, 0, 0).unwrap().and_utc();
    let end = start + Duration::days(1);
    let resolved = serde_json::json!({"lags": 5});
    let param_hash = param_hash::param_hash(&resolved).expect("hash ok");
    let req = ScanRequest {
        scan_id: "stats.autocorr.ljung_box".into(),
        version: 1,
        instruments: vec![InstrumentSpec {
            symbol: "EURUSD".into(),
            side: Side::Bid,
        }],
        timeframe: Timeframe::Tf15m,
        window: ClosedRangeUtc { start, end },
        sub_range: TimeRange {
            start_utc: start,
            end_utc: end,
        },
        gap_policy: GapPolicyKind::ContinuousOnly,
        resolved_params: resolved,
        param_hash,
        dry_run: false,
        master_seed,
        job_seed: None,
        bootstrap_method: bootstrap,
        bootstrap_n,
        null_method: null,
        null_n,
        #[cfg(any(test, feature = "test-internal"))]
        sleep_after_first_finding_ms: None,
    };
    (req, day)
}

/// Construct a single-instrument Welford request with `series=log_returns`.
fn make_welford_request(
    bootstrap: Option<BootstrapMethod>,
    bootstrap_n: Option<u32>,
    master_seed: Option<u64>,
) -> (ScanRequest, NaiveDate) {
    let day = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap();
    let start = day.and_hms_opt(0, 0, 0).unwrap().and_utc();
    let end = start + Duration::days(1);
    let resolved = serde_json::json!({"series": "log_returns"});
    let param_hash = param_hash::param_hash(&resolved).expect("hash ok");
    let req = ScanRequest {
        scan_id: "stats.summary.welford".into(),
        version: 1,
        instruments: vec![InstrumentSpec {
            symbol: "EURUSD".into(),
            side: Side::Bid,
        }],
        timeframe: Timeframe::Tf15m,
        window: ClosedRangeUtc { start, end },
        sub_range: TimeRange {
            start_utc: start,
            end_utc: end,
        },
        gap_policy: GapPolicyKind::ContinuousOnly,
        resolved_params: resolved,
        param_hash,
        dry_run: false,
        master_seed,
        job_seed: None,
        bootstrap_method: bootstrap,
        bootstrap_n,
        null_method: None,
        null_n: None,
        #[cfg(any(test, feature = "test-internal"))]
        sleep_after_first_finding_ms: None,
    };
    (req, day)
}

/// Outliers (default-false on bootstrap + null) for the preflight-rejection
/// regression gate.
fn make_outliers_request(bootstrap: Option<BootstrapMethod>) -> (ScanRequest, NaiveDate) {
    let day = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap();
    let start = day.and_hms_opt(0, 0, 0).unwrap().and_utc();
    let end = start + Duration::days(1);
    let resolved = serde_json::json!({"series": "log_returns"});
    let param_hash = param_hash::param_hash(&resolved).expect("hash ok");
    let req = ScanRequest {
        scan_id: "stats.outliers.z_and_mad".into(),
        version: 1,
        instruments: vec![InstrumentSpec {
            symbol: "EURUSD".into(),
            side: Side::Bid,
        }],
        timeframe: Timeframe::Tf15m,
        window: ClosedRangeUtc { start, end },
        sub_range: TimeRange {
            start_utc: start,
            end_utc: end,
        },
        gap_policy: GapPolicyKind::ContinuousOnly,
        resolved_params: resolved,
        param_hash,
        dry_run: false,
        master_seed: Some(0xDEAD),
        job_seed: None,
        bootstrap_method: bootstrap,
        bootstrap_n: Some(100),
        null_method: None,
        null_n: None,
        #[cfg(any(test, feature = "test-internal"))]
        sleep_after_first_finding_ms: None,
    };
    (req, day)
}

fn run_engine(req: &ScanRequest, day: NaiveDate, register: impl FnOnce(&mut Registry)) -> Vec<u8> {
    let cache = SyntheticCache::new().with_deterministic_day("EURUSD", Side::Bid, day, 0x1234_5678);
    let cfg = make_cfg(&cache);
    let reader = DukascopyReader::new(cache.cache_root());
    let mut registry = Registry::new();
    register(&mut registry);
    let mut sink = BufferSink::new();
    let cancel = Arc::new(AtomicBool::new(false));
    run_one_with_registry(req, &cfg, &reader, &mut sink, cancel, &registry).expect("engine ok");
    sink.0
}

fn first_result(bytes: &[u8]) -> miner_core::findings::ResultFinding {
    let findings: Vec<Finding> = parse_findings(bytes);
    findings
        .iter()
        .find_map(|f| match f {
            Finding::Result(r) => Some(r.clone()),
            _ => None,
        })
        .expect("at least one Finding::Result must be emitted")
}

// ---------------------------------------------------------------------------
// Test 1: bootstrap CI population — LjungBox
// ---------------------------------------------------------------------------

/// Plan 05-03 continuation Test 1 — `bootstrap_ci_populates_effect_ci95`.
///
/// `ScanRequest { bootstrap_method: Some(Stationary), bootstrap_n: Some(100),
/// master_seed: Some(0xDEAD), ... }` against `stats.autocorr.ljung_box@1`
/// produces a `Finding::Result` whose `effect.ci95 = Some([lo, hi])` (finite,
/// `lo <= hi`).
#[test]
fn bootstrap_ci_populates_effect_ci95_on_ljung_box() {
    let (req, day) = make_ljungbox_request(
        Some(BootstrapMethod::Stationary),
        Some(100),
        None,
        None,
        Some(0xDEAD),
    );
    let bytes = run_engine(&req, day, |r| r.register(Box::new(LjungBoxScan)));
    let result = first_result(&bytes);
    let ci = result
        .effect
        .ci95
        .expect("bootstrap was requested + supported — effect.ci95 must be Some([lo, hi])");
    assert!(
        ci[0].is_finite() && ci[1].is_finite(),
        "ci95 must be finite; got [{}, {}]",
        ci[0],
        ci[1]
    );
    assert!(ci[0] <= ci[1], "ci lo must be <= hi; got [{}, {}]", ci[0], ci[1]);
}

// ---------------------------------------------------------------------------
// Test 2: null p-value replacement — LjungBox
// ---------------------------------------------------------------------------

/// Plan 05-03 continuation Test 2 — `null_p_replaces_analytic_p_value`.
///
/// `ScanRequest { null_method: Some(CircularShift), null_n: Some(200),
/// master_seed: Some(0xDEAD), ... }` against `stats.autocorr.ljung_box@1`
/// produces a `Finding::Result` whose `effect.p_value` is the empirical
/// p-value — necessarily in `[0.0, 1.0]`. The analytic chi-squared
/// p-value and the empirical p-value should not be identical except by
/// astronomical coincidence (we assert finite + in-range only to avoid a
/// flaky equality check across seeds).
#[test]
fn null_p_replaces_analytic_p_on_ljung_box() {
    // Baseline: no null requested — capture the analytic chi-squared p-value.
    let (baseline_req, day) = make_ljungbox_request(None, None, None, None, None);
    let baseline_bytes = run_engine(&baseline_req, day, |r| r.register(Box::new(LjungBoxScan)));
    let baseline = first_result(&baseline_bytes);
    let analytic_p = baseline
        .effect
        .p_value
        .expect("LjungBox always emits an analytic p-value");

    // With null requested: empirical p-value replaces the analytic one.
    let (req, day) = make_ljungbox_request(
        None,
        None,
        Some(NullMethod::CircularShift),
        Some(200),
        Some(0xDEAD),
    );
    let bytes = run_engine(&req, day, |r| r.register(Box::new(LjungBoxScan)));
    let result = first_result(&bytes);
    let empirical_p = result
        .effect
        .p_value
        .expect("null was requested + supported — effect.p_value must be Some(empirical_p)");
    assert!(
        (0.0..=1.0).contains(&empirical_p),
        "empirical p must be in [0, 1]; got {empirical_p}"
    );
    assert!(
        empirical_p.is_finite(),
        "empirical p must be finite; got {empirical_p}"
    );
    // The two p-values are computed by entirely different procedures; on a
    // synthetic LCG series the analytic chi-squared and the empirical
    // circular-shift p MUST differ (otherwise the engine silently kept the
    // analytic p and never invoked the null kernel).
    assert!(
        (analytic_p - empirical_p).abs() > 1e-9,
        "empirical p ({empirical_p}) and analytic p ({analytic_p}) must differ — the engine should have replaced one with the other"
    );
}

// ---------------------------------------------------------------------------
// Test 3: ReproEnvelope population — LjungBox
// ---------------------------------------------------------------------------

/// Plan 05-03 continuation Test 3 — `repro_envelope_populated_when_hygiene_runs`.
///
/// For both bootstrap and null cases, the emitted `Finding::Result` carries
/// `repro = Some(ReproEnvelope { master_seed: 0xDEAD, job_seed: <derived>,
/// bootstrap: Some(_) | None, null: Some(_) | None })`.
#[test]
fn repro_envelope_populated_with_bootstrap_and_null() {
    let (req, day) = make_ljungbox_request(
        Some(BootstrapMethod::Stationary),
        Some(100),
        Some(NullMethod::CircularShift),
        Some(100),
        Some(0xDEAD),
    );
    let bytes = run_engine(&req, day, |r| r.register(Box::new(LjungBoxScan)));
    let result = first_result(&bytes);
    let repro = result
        .repro
        .expect("hygiene ran (bootstrap + null) — repro must be Some");
    assert_eq!(repro.master_seed, 0xDEAD);
    assert_ne!(repro.job_seed, 0, "job_seed must be derived (non-zero)");
    let bs = repro
        .bootstrap
        .as_ref()
        .expect("bootstrap requested — repro.bootstrap must be Some");
    assert_eq!(bs.method, "stationary");
    assert_eq!(bs.n, 100);
    let nl = repro.null.as_ref().expect("null requested — repro.null must be Some");
    assert_eq!(nl.method, "circular_shift");
    assert_eq!(nl.n, 100);
}

// ---------------------------------------------------------------------------
// Test 4: ReproEnvelope is None when no hygiene runs
// ---------------------------------------------------------------------------

/// Plan 05-03 continuation Test 4 — `repro_envelope_none_without_hygiene`.
///
/// Without bootstrap or null requested, `repro = None` (the contract Plan
/// 05-01 / D5-05 already pinned in the `LjungBox` kernel; this test gates
/// the engine doesn't accidentally populate it).
#[test]
fn repro_envelope_none_without_hygiene() {
    let (req, day) = make_ljungbox_request(None, None, None, None, None);
    let bytes = run_engine(&req, day, |r| r.register(Box::new(LjungBoxScan)));
    let result = first_result(&bytes);
    assert!(
        result.repro.is_none(),
        "no hygiene requested — repro must stay None"
    );
}

// ---------------------------------------------------------------------------
// Test 5: byte-identical rerun under hygiene-on flags — LjungBox
// ---------------------------------------------------------------------------

/// Plan 05-03 continuation Test 5 — `byte_identical_rerun_under_hygiene`.
///
/// Two consecutive runs with `master_seed = 0xDEAD` + same inputs + same
/// bootstrap+null flags produce byte-identical `effect.ci95`,
/// `effect.p_value`, AND `repro.job_seed`. This is the canonical D5-05
/// invariant on the hygiene path.
#[test]
fn byte_identical_rerun_under_hygiene_on_ljung_box() {
    let (req, day) = make_ljungbox_request(
        Some(BootstrapMethod::Stationary),
        Some(100),
        Some(NullMethod::CircularShift),
        Some(100),
        Some(0xDEAD),
    );
    let bytes_a = run_engine(&req, day, |r| r.register(Box::new(LjungBoxScan)));
    let bytes_b = run_engine(&req, day, |r| r.register(Box::new(LjungBoxScan)));
    let result_a = first_result(&bytes_a);
    let result_b = first_result(&bytes_b);

    // Bit-identity on ci95.
    let ci_a = result_a.effect.ci95.expect("bootstrap ran — ci95 Some");
    let ci_b = result_b.effect.ci95.expect("bootstrap ran — ci95 Some");
    assert_eq!(ci_a[0].to_bits(), ci_b[0].to_bits(), "ci95 lo bit-identity");
    assert_eq!(ci_a[1].to_bits(), ci_b[1].to_bits(), "ci95 hi bit-identity");

    // Bit-identity on p_value.
    let p_a = result_a.effect.p_value.expect("null ran — p_value Some");
    let p_b = result_b.effect.p_value.expect("null ran — p_value Some");
    assert_eq!(p_a.to_bits(), p_b.to_bits(), "p_value bit-identity");

    // job_seed identity.
    let repro_a = result_a.repro.expect("repro Some");
    let repro_b = result_b.repro.expect("repro Some");
    assert_eq!(repro_a.job_seed, repro_b.job_seed, "job_seed identity");
    assert_eq!(repro_a.master_seed, repro_b.master_seed, "master_seed identity");
}

// ---------------------------------------------------------------------------
// Test 6: bootstrap_n / null_n clamp to 100_000
// ---------------------------------------------------------------------------

/// Plan 05-03 continuation Test 6 — `bootstrap_n_clamped_to_ceiling`.
///
/// Per T-05-03-V5 mitigation, `bootstrap_n` and `null_n` are clamped at
/// `100_000`. The engine clamps internally; the `ReproEnvelope.bootstrap.n`
/// / `ReproEnvelope.null.n` echo the clamped value, not the user-supplied
/// value.
///
/// We send `bootstrap_n = 200_000` and `null_n = 200_000` and assert the
/// echoed wire form is `100_000` for both.
#[test]
fn bootstrap_and_null_n_clamped_to_ceiling() {
    let (req, day) = make_ljungbox_request(
        Some(BootstrapMethod::Stationary),
        Some(200_000),
        Some(NullMethod::CircularShift),
        Some(200_000),
        Some(0xDEAD),
    );
    let bytes = run_engine(&req, day, |r| r.register(Box::new(LjungBoxScan)));
    let result = first_result(&bytes);
    let repro = result.repro.expect("hygiene ran — repro Some");
    let bs = repro.bootstrap.expect("bootstrap ran — repro.bootstrap Some");
    assert_eq!(
        bs.n, 100_000,
        "bootstrap_n must be clamped at 100_000 (T-05-03-V5); got {}",
        bs.n
    );
    let nl = repro.null.expect("null ran — repro.null Some");
    assert_eq!(
        nl.n, 100_000,
        "null_n must be clamped at 100_000 (T-05-03-V5); got {}",
        nl.n
    );
}

// ---------------------------------------------------------------------------
// Test 7: Welford bootstrap CI (single-arity, bootstrap-only)
// ---------------------------------------------------------------------------

/// Plan 05-03 continuation Test 7 — `bootstrap_ci_populates_on_welford`.
///
/// Welford supports bootstrap but no null methods per D5-04. Sending
/// bootstrap-only against Welford populates `effect.ci95` and leaves
/// `effect.p_value` unchanged (which for Welford is `None` by construction).
/// `repro.bootstrap` is populated; `repro.null` stays `None`.
#[test]
fn bootstrap_ci_populates_on_welford_with_null_none() {
    let (req, day) =
        make_welford_request(Some(BootstrapMethod::Stationary), Some(100), Some(0xCAFE));
    let bytes = run_engine(&req, day, |r| r.register(Box::new(SummaryWelfordScan)));
    let result = first_result(&bytes);
    let ci = result.effect.ci95.expect("Welford bootstrap — ci95 must be Some");
    assert!(ci[0].is_finite() && ci[1].is_finite());
    assert!(ci[0] <= ci[1]);
    let repro = result.repro.expect("hygiene ran — repro Some");
    assert!(repro.bootstrap.is_some(), "bootstrap requested — Some");
    assert!(repro.null.is_none(), "null not requested — None");
}

// ---------------------------------------------------------------------------
// Test 8: preflight rejection still fires (regression gate for Plan 05-03
// validate_hygiene_support)
// ---------------------------------------------------------------------------

/// Plan 05-03 continuation Test 8 — `preflight_rejects_bootstrap_on_outliers`.
///
/// Outliers has `supports_bootstrap == false`. Requesting bootstrap on
/// outliers still trips `validate_hygiene_support` BEFORE the engine reaches
/// the new hygiene-loop code path. The engine returns
/// `Err(MinerError::Preflight(WireError { code: "hygiene_not_supported" }))`
/// and stdout stays empty.
#[test]
fn preflight_rejects_bootstrap_on_unsupported_scan() {
    let (req, day) = make_outliers_request(Some(BootstrapMethod::Stationary));
    let cache = SyntheticCache::new().with_deterministic_day("EURUSD", Side::Bid, day, 0x1234_5678);
    let cfg = make_cfg(&cache);
    let reader = DukascopyReader::new(cache.cache_root());
    let mut registry = Registry::new();
    registry.register(Box::new(OutliersZAndMadScan));
    let mut sink = BufferSink::new();
    let cancel = Arc::new(AtomicBool::new(false));
    let outcome = run_one_with_registry(&req, &cfg, &reader, &mut sink, cancel, &registry);
    match outcome {
        Err(miner_core::error::MinerError::Preflight(w)) => {
            assert_eq!(w.code, "hygiene_not_supported");
        }
        other => panic!("expected MinerError::Preflight(hygiene_not_supported); got {other:?}"),
    }
    assert!(sink.0.is_empty(), "stdout must stay empty on preflight rejection");
}
