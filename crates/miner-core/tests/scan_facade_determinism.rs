//! Phase 3 integration test — twice-run masked byte-equality (OUT-03 / SC-6a).
//!
//! Runs `engine::run_one` twice in-process against the same `SyntheticCache`
//! + same `ScanRequest`, parses each run's JSONL output, masks the four
//! volatile envelope fields (`run_id`, `started_at_utc`, `produced_at_utc`,
//! `ended_at_utc`) plus the integer `wall_clock_ms`, and asserts the masked
//! bytes are byte-identical across runs.
//!
//! Cheaper than `cli_streams.rs::emit_fixture_byte_identical_*` (no subprocess
//! spawn) while exercising the same envelope-determinism contract.

#![allow(clippy::doc_lazy_continuation)]

mod common;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use chrono::{NaiveDate, TimeZone, Utc};

use miner_core::aggregator::Timeframe;
use miner_core::config::{MinerConfig, OutputDest};
use miner_core::engine::gap_policy::GapPolicyKind;
use miner_core::engine::{param_hash, run_one};
use miner_core::findings::TimeRange;
use miner_core::reader::{ClosedRangeUtc, Side};
use miner_core::scan::ScanRequest;
use miner_reader_dukascopy::DukascopyReader;

use common::{BufferSink, synthetic_cache::SyntheticCache};

#[test]
#[serial_test::serial]
fn twice_run_byte_identical_when_volatile_fields_masked() {
    // Build a synthetic cache with one full day so the engine gets bars to
    // scan (the day is reused across both runs — the cache state is identical).
    let day = NaiveDate::from_ymd_opt(2024, 6, 12).expect("valid date");
    let cache = SyntheticCache::new().with_deterministic_day("EURUSD", Side::Bid, day, 0xDEAD_BEEF);

    let cfg = MinerConfig {
        cache_root: cache.cache_root().to_path_buf(),
        bar_cache_root: cache.bar_cache_root().to_path_buf(),
        output: OutputDest::Stdout,
    };
    let reader = DukascopyReader::new(cache.cache_root());

    let req = sample_request();

    // Run 1.
    let mut sink1 = BufferSink::new();
    let outcome1 = run_one(
        &req,
        &cfg,
        &reader,
        &mut sink1,
        Arc::new(AtomicBool::new(false)),
    )
    .expect("run 1 ok");
    let masked1 = common::parse_and_mask_jsonl(&sink1.0);

    // Run 2 — identical inputs against the same SyntheticCache.
    let mut sink2 = BufferSink::new();
    let outcome2 = run_one(
        &req,
        &cfg,
        &reader,
        &mut sink2,
        Arc::new(AtomicBool::new(false)),
    )
    .expect("run 2 ok");
    let masked2 = common::parse_and_mask_jsonl(&sink2.0);

    assert_eq!(outcome1, outcome2, "RunOutcome must match across runs");
    assert_eq!(
        masked1.len(),
        masked2.len(),
        "envelope counts must match across runs (got {} vs {})",
        masked1.len(),
        masked2.len(),
    );
    assert_eq!(
        masked1,
        masked2,
        "OUT-03 closure: masked envelopes from two run_one invocations differ.\n\
         Run 1: {}\nRun 2: {}",
        serde_json::to_string_pretty(&masked1).unwrap_or_default(),
        serde_json::to_string_pretty(&masked2).unwrap_or_default(),
    );
}

fn sample_request() -> ScanRequest {
    let start = Utc.with_ymd_and_hms(2024, 6, 12, 0, 0, 0).unwrap();
    let end = Utc.with_ymd_and_hms(2024, 6, 13, 0, 0, 0).unwrap();
    let resolved = serde_json::json!({"lags": 5});
    let param_hash = param_hash::param_hash(&resolved).expect("param_hash ok");
    ScanRequest {
        scan_id: "stats.autocorr.ljung_box".into(),
        version: 1,
        instrument: "EURUSD".into(),
        side: Side::Bid,
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
        #[cfg(any(test, feature = "test-internal"))]
        sleep_after_first_finding_ms: None,
    }
}
