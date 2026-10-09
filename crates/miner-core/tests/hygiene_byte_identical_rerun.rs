//! Plan 05-03 continuation 2 — byte-identical-rerun tests for hygiene
//! mutation across the three scan families (ANOM, CROSS, SEAS).
//!
//! The original Plan 05-03 continuation only verified byte-identical-rerun
//! for `LjungBox` (single-arity ANOM). This file extends the gate to:
//!
//! - **ANOM (beyond Welford)**: `stats.variance_ratio.lo_mackinlay@1`
//!   resamples log returns and recomputes VR(max k).
//! - **CROSS (Pair-arity, joint resampling)**:
//!   `cross.corr.pearson_rolling@1` uses the new
//!   `pair_stationary_bootstrap_ci` helper — leg A and leg B share the
//!   same resample indices so the joint last-window correlation stays
//!   well-defined under the resample.
//! - **SEAS (`ts_open_utc`-dependent buckets)**:
//!   `seas.bucket.hour_of_day@1` snapshots the per-return bucket keys at
//!   closure-construction time. Resampling shuffles the returns; bucket
//!   assignments stay stable; `max_abs_finite(t_stats)` is recomputed
//!   over the shuffled returns. Two runs with the same `master_seed`
//!   must produce byte-identical CI95.
//!
//! ## Invariant
//!
//! Two `run_one_with_registry` invocations with `master_seed = 0xDEAD` +
//! same inputs + same bootstrap/null flags produce `effect.ci95`,
//! `effect.p_value`, and `repro.job_seed` whose bit patterns match
//! across runs (`f64::to_bits` equality).
//!
//! ## Not in scope
//!
//! IAAFT `PhaseScramble` null still defers to Phase 7 per Plan 05-02
//! SUMMARY — these tests use `NullMethod::CircularShift` only.

#![allow(
    clippy::cast_precision_loss,
    clippy::too_many_lines,
    clippy::too_many_arguments
)]

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
use miner_core::scan::anom::VarianceRatioScan;
use miner_core::scan::cross::PearsonRollingScan;
use miner_core::scan::seas::HourOfDayScan;
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

/// One-day synthetic cache for Single-arity tests.
fn single_arity_cache(seed: u32) -> (SyntheticCache, NaiveDate) {
    let day = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap();
    let cache = SyntheticCache::new().with_deterministic_day("EURUSD", Side::Bid, day, seed);
    (cache, day)
}

/// One-day synthetic cache for Pair-arity tests — two distinct symbols
/// sharing the same day.
fn pair_arity_cache(seed_a: u32, seed_b: u32) -> (SyntheticCache, NaiveDate) {
    let day = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap();
    let cache = SyntheticCache::new()
        .with_deterministic_day("EURUSD", Side::Bid, day, seed_a)
        .with_deterministic_day("GBPUSD", Side::Bid, day, seed_b);
    (cache, day)
}

/// Build a `ScanRequest` for the single-arity ANOM/SEAS path.
fn make_single_request(
    scan_id: &str,
    params: serde_json::Value,
    day: NaiveDate,
    bootstrap: Option<BootstrapMethod>,
    bootstrap_n: Option<u32>,
    null: Option<NullMethod>,
    null_n: Option<u32>,
    master_seed: Option<u64>,
) -> ScanRequest {
    let start = day.and_hms_opt(0, 0, 0).unwrap().and_utc();
    let end = start + Duration::days(1);
    let param_hash = param_hash::param_hash(&params).expect("hash ok");
    ScanRequest {
        scan_id: scan_id.into(),
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
        resolved_params: params,
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
    }
}

/// Build a `ScanRequest` for the Pair-arity CROSS path.
fn make_pair_request(
    scan_id: &str,
    params: serde_json::Value,
    day: NaiveDate,
    bootstrap: Option<BootstrapMethod>,
    bootstrap_n: Option<u32>,
    null: Option<NullMethod>,
    null_n: Option<u32>,
    master_seed: Option<u64>,
) -> ScanRequest {
    let start = day.and_hms_opt(0, 0, 0).unwrap().and_utc();
    let end = start + Duration::days(1);
    let param_hash = param_hash::param_hash(&params).expect("hash ok");
    ScanRequest {
        scan_id: scan_id.into(),
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
        resolved_params: params,
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
    }
}

fn run_engine_single(
    req: &ScanRequest,
    cache: &SyntheticCache,
    register: impl FnOnce(&mut Registry),
) -> Vec<u8> {
    let cfg = make_cfg(cache);
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
// ANOM family — VarianceRatio byte-identical-rerun under hygiene
// ---------------------------------------------------------------------------

/// Two runs of `cross.cointegration.engle_granger` … wait, that's CROSS.
/// This test exercises `stats.variance_ratio.lo_mackinlay@1` — ANOM
/// Single-arity, opts in to bootstrap + both null methods.
#[test]
fn byte_identical_rerun_under_hygiene_on_variance_ratio() {
    let (cache, day) = single_arity_cache(0x1234_5678);
    let req = make_single_request(
        "stats.variance_ratio.lo_mackinlay",
        // Restrict the k grid so the stat is computable on a 24h × 15m
        // window (= 96 returns); k <= 96/2 = 48.
        serde_json::json!({"k_values": [2, 4, 8]}),
        day,
        Some(BootstrapMethod::Stationary),
        Some(100),
        Some(NullMethod::CircularShift),
        Some(100),
        Some(0xDEAD),
    );

    let bytes_a = run_engine_single(&req, &cache, |r| r.register(Box::new(VarianceRatioScan)));
    let bytes_b = run_engine_single(&req, &cache, |r| r.register(Box::new(VarianceRatioScan)));

    let result_a = first_result(&bytes_a);
    let result_b = first_result(&bytes_b);

    let ci_a = result_a.effect.ci95.expect("bootstrap ran — ci95 Some");
    let ci_b = result_b.effect.ci95.expect("bootstrap ran — ci95 Some");
    assert_eq!(
        ci_a[0].to_bits(),
        ci_b[0].to_bits(),
        "VR ci95 lo bit-identity"
    );
    assert_eq!(
        ci_a[1].to_bits(),
        ci_b[1].to_bits(),
        "VR ci95 hi bit-identity"
    );

    let p_a = result_a.effect.p_value.expect("null ran — p_value Some");
    let p_b = result_b.effect.p_value.expect("null ran — p_value Some");
    assert_eq!(p_a.to_bits(), p_b.to_bits(), "VR empirical p bit-identity");

    let repro_a = result_a.repro.expect("repro Some");
    let repro_b = result_b.repro.expect("repro Some");
    assert_eq!(repro_a.job_seed, repro_b.job_seed, "VR job_seed identity");
    assert_eq!(repro_a.master_seed, 0xDEAD);
}

/// ANOM: confirm `effect.ci95` is populated AND finite for `VarianceRatio`
/// — the dispatch table wiring landed; not a no-op.
#[test]
fn bootstrap_ci_populates_for_variance_ratio() {
    let (cache, day) = single_arity_cache(0xABCD);
    let req = make_single_request(
        "stats.variance_ratio.lo_mackinlay",
        serde_json::json!({"k_values": [2, 4]}),
        day,
        Some(BootstrapMethod::Stationary),
        Some(50),
        None,
        None,
        Some(0xBEEF),
    );
    let bytes = run_engine_single(&req, &cache, |r| r.register(Box::new(VarianceRatioScan)));
    let result = first_result(&bytes);
    let ci = result.effect.ci95.expect("VR opt-in ⇒ ci95 must populate");
    assert!(
        ci[0].is_finite() && ci[1].is_finite(),
        "ci95 finite; got [{}, {}]",
        ci[0],
        ci[1]
    );
    assert!(ci[0] <= ci[1]);
}

// ---------------------------------------------------------------------------
// CROSS family — PearsonRolling byte-identical-rerun under joint resampling
// ---------------------------------------------------------------------------

/// Pair-arity byte-identical-rerun on `cross.corr.pearson_rolling@1` —
/// uses the joint `pair_stationary_bootstrap_ci` + `pair_circular_shift_null_p`
/// helpers. Both runs sample the SAME paired indices so the joint
/// correlation stat is reproducible across reruns under fixed `master_seed`.
#[test]
fn byte_identical_rerun_under_hygiene_on_pearson_rolling() {
    let (cache, day) = pair_arity_cache(0x1111, 0x2222);
    let req = make_pair_request(
        "cross.corr.pearson_rolling",
        // 96 aligned bars → 95 returns → window 10 → 86 rolling values.
        serde_json::json!({"window": 10}),
        day,
        Some(BootstrapMethod::Stationary),
        Some(100),
        Some(NullMethod::CircularShift),
        Some(100),
        Some(0xDEAD),
    );
    let bytes_a = run_engine_single(&req, &cache, |r| r.register(Box::new(PearsonRollingScan)));
    let bytes_b = run_engine_single(&req, &cache, |r| r.register(Box::new(PearsonRollingScan)));
    let result_a = first_result(&bytes_a);
    let result_b = first_result(&bytes_b);

    let ci_a = result_a
        .effect
        .ci95
        .expect("pair bootstrap ran — ci95 Some");
    let ci_b = result_b
        .effect
        .ci95
        .expect("pair bootstrap ran — ci95 Some");
    assert_eq!(
        ci_a[0].to_bits(),
        ci_b[0].to_bits(),
        "Pair ci95 lo bit-identity"
    );
    assert_eq!(
        ci_a[1].to_bits(),
        ci_b[1].to_bits(),
        "Pair ci95 hi bit-identity"
    );

    let p_a = result_a
        .effect
        .p_value
        .expect("pair null ran — p_value Some");
    let p_b = result_b
        .effect
        .p_value
        .expect("pair null ran — p_value Some");
    assert_eq!(
        p_a.to_bits(),
        p_b.to_bits(),
        "Pair empirical p bit-identity"
    );

    let repro_a = result_a.repro.expect("repro Some");
    let repro_b = result_b.repro.expect("repro Some");
    assert_eq!(repro_a.job_seed, repro_b.job_seed, "Pair job_seed identity");
    assert_eq!(repro_a.master_seed, 0xDEAD);
}

/// CROSS: bootstrap CI populates for Pearson rolling — confirms the
/// Pair-arity dispatch is wired AND the joint resample produces a finite
/// CI consistent with the scan's `Effect.value` (= last-window Pearson r).
#[test]
fn bootstrap_ci_populates_for_pearson_rolling() {
    let (cache, day) = pair_arity_cache(0x3333, 0x4444);
    let req = make_pair_request(
        "cross.corr.pearson_rolling",
        serde_json::json!({"window": 10}),
        day,
        Some(BootstrapMethod::Stationary),
        Some(50),
        None,
        None,
        Some(0xC0DE),
    );
    let bytes = run_engine_single(&req, &cache, |r| r.register(Box::new(PearsonRollingScan)));
    let result = first_result(&bytes);
    let ci = result
        .effect
        .ci95
        .expect("Pearson rolling opt-in ⇒ ci95 must populate");
    assert!(ci[0].is_finite() && ci[1].is_finite(), "ci95 finite");
    assert!(ci[0] <= ci[1]);
    // Pearson r is in [-1, 1]; the stationary-bootstrap CI must respect
    // this bound (modulo numerical noise — we allow a tiny overshoot).
    assert!(
        ci[0] >= -1.01 && ci[1] <= 1.01,
        "Pearson CI must be near [-1, 1]; got [{}, {}]",
        ci[0],
        ci[1]
    );
    let repro = result.repro.expect("repro Some");
    assert_eq!(repro.master_seed, 0xC0DE);
    assert!(repro.bootstrap.is_some());
    assert!(repro.null.is_none(), "null not requested");
}

// ---------------------------------------------------------------------------
// SEAS family — HourOfDay byte-identical-rerun under bucket-keyed resampling
// ---------------------------------------------------------------------------

/// SEAS byte-identical-rerun on `seas.bucket.hour_of_day@1` — confirms
/// bucket keys are correctly snapshotted at closure-construction time
/// and the bootstrap kernel produces byte-identical CI bounds across
/// reruns under fixed `master_seed`.
///
/// `seas.bucket.hour_of_day@1` opts into bootstrap ONLY per the D5-04
/// matrix; null methods stay false. The bootstrap-only path still
/// exercises the bucket-key snapshot + resample + recompute closure,
/// so the byte-identical-rerun gate carries through.
#[test]
fn byte_identical_rerun_under_hygiene_on_hour_of_day() {
    let (cache, day) = single_arity_cache(0x5555_6666);
    let req = make_single_request(
        "seas.bucket.hour_of_day",
        // 96 15m bars → 95 returns covering 24 hourly buckets ~4 each
        // — well below the default min_obs=5. Lower min_obs to 2 so the
        // t-stats are computable for most buckets.
        serde_json::json!({"min_obs_per_bucket": 2}),
        day,
        Some(BootstrapMethod::Stationary),
        Some(100),
        // SEAS scans do not opt into any null method (D5-04 matrix);
        // preflight would reject `Some(NullMethod::CircularShift)` here
        // with `hygiene_not_supported`. The bootstrap path alone still
        // tests the bucket-keyed dispatch closure under resampling.
        None,
        None,
        Some(0xDEAD),
    );
    let bytes_a = run_engine_single(&req, &cache, |r| r.register(Box::new(HourOfDayScan)));
    let bytes_b = run_engine_single(&req, &cache, |r| r.register(Box::new(HourOfDayScan)));
    let result_a = first_result(&bytes_a);
    let result_b = first_result(&bytes_b);

    let ci_a = result_a
        .effect
        .ci95
        .expect("SEAS bootstrap ran — ci95 Some");
    let ci_b = result_b
        .effect
        .ci95
        .expect("SEAS bootstrap ran — ci95 Some");
    assert_eq!(
        ci_a[0].to_bits(),
        ci_b[0].to_bits(),
        "SEAS ci95 lo bit-identity"
    );
    assert_eq!(
        ci_a[1].to_bits(),
        ci_b[1].to_bits(),
        "SEAS ci95 hi bit-identity"
    );

    let repro_a = result_a.repro.expect("repro Some");
    let repro_b = result_b.repro.expect("repro Some");
    assert_eq!(repro_a.job_seed, repro_b.job_seed);
    // Null was not requested ⇒ repro.null stays None on both runs.
    assert!(repro_a.null.is_none() && repro_b.null.is_none());
}

/// SEAS: bootstrap CI populates for `HourOfDay` — confirms the dispatch
/// table wiring; the closure recomputes `max_abs_finite(t_stats)` over
/// the resampled returns with snapshotted bucket keys.
#[test]
fn bootstrap_ci_populates_for_hour_of_day() {
    let (cache, day) = single_arity_cache(0x7777);
    let req = make_single_request(
        "seas.bucket.hour_of_day",
        serde_json::json!({"min_obs_per_bucket": 2}),
        day,
        Some(BootstrapMethod::Stationary),
        Some(50),
        None,
        None,
        Some(0xF00D),
    );
    let bytes = run_engine_single(&req, &cache, |r| r.register(Box::new(HourOfDayScan)));
    let result = first_result(&bytes);
    let ci = result
        .effect
        .ci95
        .expect("hour_of_day opt-in ⇒ ci95 must populate");
    assert!(ci[0].is_finite() && ci[1].is_finite());
    // max_abs_t_stat >= 0 by construction; CI95 must respect the lower
    // bound (modulo bootstrap variance).
    assert!(
        ci[0] >= -0.01,
        "max_abs lower CI must be near 0; got {}",
        ci[0]
    );
    assert!(ci[1] >= ci[0]);
    let repro = result.repro.expect("repro Some");
    assert_eq!(repro.master_seed, 0xF00D);
    assert_ne!(repro.job_seed, 0);
}

// ---------------------------------------------------------------------------
// Cross-family sanity: differing `master_seed` ⇒ differing CIs
// (Failure mode the byte-identical-rerun gate doesn't catch: a stuck
// dispatch table that returns the same CI for every seed.)
// ---------------------------------------------------------------------------

/// Sanity gate: two runs with DIFFERENT `master_seed` produce DIFFERENT
/// CI bounds on the same input. Pin the per-family dispatch is actually
/// driving the kernel's RNG (not constant-folding to a fixed value).
#[test]
fn differing_master_seed_yields_differing_ci_for_variance_ratio() {
    let (cache, day) = single_arity_cache(0xDEAD_BEEF);
    let mk = |seed: u64| {
        make_single_request(
            "stats.variance_ratio.lo_mackinlay",
            serde_json::json!({"k_values": [2, 4]}),
            day,
            Some(BootstrapMethod::Stationary),
            Some(200),
            None,
            None,
            Some(seed),
        )
    };
    let bytes_a = run_engine_single(&mk(0xAAAA), &cache, |r| {
        r.register(Box::new(VarianceRatioScan));
    });
    let bytes_b = run_engine_single(&mk(0xBBBB), &cache, |r| {
        r.register(Box::new(VarianceRatioScan));
    });
    let ci_a = first_result(&bytes_a).effect.ci95.expect("ci95 Some");
    let ci_b = first_result(&bytes_b).effect.ci95.expect("ci95 Some");
    // Different seeds → at least one of the two CI bounds must differ.
    assert!(
        ci_a[0].to_bits() != ci_b[0].to_bits() || ci_a[1].to_bits() != ci_b[1].to_bits(),
        "differing master_seed must yield differing CI; got [{}, {}] == [{}, {}]",
        ci_a[0],
        ci_a[1],
        ci_b[0],
        ci_b[1]
    );
}
