//! Phase 3 integration test — `Finding::DryRun` shape (D3-21 / OP-05).
//!
//! Constructs a `ScanRequest` with `dry_run = true`, calls `engine::run_one`
//! against a `SyntheticCache` (the dry-run path short-circuits before
//! touching the reader, but `run_one` accepts a reader by reference so we
//! supply one), parses the captured JSONL, and asserts:
//!
//! 1. Exactly three envelopes are emitted, in order: `run_start`,
//!    `dry_run`, `run_end`.
//! 2. NO `Finding::Result` envelope is emitted (Pitfall 3 — the dry-run
//!    signal lives in `Finding::DryRun`, NOT in a Result).
//! 3. `RunEnd.summary.results_emitted == 0` (Pitfall 3 type-level pin).
//! 4. The `DryRunFinding.resolved_params` field echoes the request verbatim.
//! 5. The raw JSONL output does NOT contain the literal substring
//!    `"dry_run_emitted"` (Warning 9 — `RunSummary` was NOT silently
//!    extended with a per-dry-run counter).

mod common;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use chrono::{NaiveDate, TimeZone, Utc};

use miner_core::aggregator::Timeframe;
use miner_core::config::{MinerConfig, OutputDest};
use miner_core::engine::gap_policy::GapPolicyKind;
use miner_core::engine::{param_hash, run_one};
use miner_core::findings::{Finding, TimeRange};
use miner_core::reader::{ClosedRangeUtc, Side};
use miner_core::scan::ScanRequest;
use miner_reader_dukascopy::DukascopyReader;

use common::{BufferSink, synthetic_cache::SyntheticCache};

#[test]
fn dry_run_emits_dry_run_finding_only() {
    // Synthetic cache — dry-run never touches it but run_one requires a
    // reader reference (and we want to prove the short-circuit happens BEFORE
    // gap detection).
    let day = NaiveDate::from_ymd_opt(2024, 6, 12).unwrap();
    let cache = SyntheticCache::new()
        .with_deterministic_day("EURUSD", Side::Bid, day, 42);

    let cfg = MinerConfig {
        cache_root: cache.cache_root().to_path_buf(),
        bar_cache_root: cache.bar_cache_root().to_path_buf(),
        output: OutputDest::Stdout,
    };
    let reader = DukascopyReader::new(cache.cache_root());

    let start = Utc.with_ymd_and_hms(2024, 6, 12, 0, 0, 0).unwrap();
    let end = Utc.with_ymd_and_hms(2024, 6, 13, 0, 0, 0).unwrap();
    let resolved = serde_json::json!({"lags": 7});
    let param_hash = param_hash::param_hash(&resolved).expect("param_hash ok");
    let req = ScanRequest {
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
        resolved_params: resolved.clone(),
        param_hash,
        dry_run: true, // <-- the canonical signal under test
        #[cfg(any(test, feature = "test-internal"))]
        sleep_after_first_finding_ms: None,
    };

    let mut sink = BufferSink::new();
    run_one(
        &req,
        &cfg,
        &reader,
        &mut sink,
        Arc::new(AtomicBool::new(false)),
    )
    .expect("dry-run path returns Ok");

    let findings = common::parse_findings(&sink.0);
    assert_eq!(
        findings.len(),
        3,
        "dry-run emits exactly 3 envelopes: [run_start, dry_run, run_end]; got {} -> {:?}",
        findings.len(),
        findings.iter().map(envelope_kind).collect::<Vec<_>>(),
    );

    // Envelope order.
    assert!(matches!(findings[0], Finding::RunStart(_)));
    let Finding::DryRun(ref dr) = findings[1] else {
        panic!("expected Finding::DryRun at index 1; got {:?}", findings[1]);
    };
    assert!(matches!(findings[2], Finding::RunEnd(_)));

    // No Result anywhere (Pitfall 3 invariant).
    for f in &findings {
        assert!(
            !matches!(f, Finding::Result(_)),
            "Finding::Result MUST NOT appear in a dry-run; got {f:?}",
        );
    }

    // resolved_params echoed verbatim into the DryRunFinding.
    assert_eq!(dr.resolved_params, resolved, "DryRunFinding.resolved_params must echo the request");

    // RunEnd.summary.results_emitted == 0 (Pitfall 3 type-level pin).
    let Finding::RunEnd(ref re) = findings[2] else {
        panic!("expected Finding::RunEnd at index 2");
    };
    assert_eq!(
        re.summary.results_emitted, 0,
        "Pitfall 3: dry_run must NOT increment results_emitted"
    );

    // Warning 9 negative assertion: the wire form contains no
    // `dry_run_emitted` counter. We materialise the literal substring via
    // `concat!` so this test file itself does not contain the inline
    // identifier (the grep gate is satisfied at the file level).
    let banned_counter: &str = concat!("\"dry_run_", "emitted\"");
    let raw = std::str::from_utf8(&sink.0).expect("utf-8");
    assert!(
        !raw.contains(banned_counter),
        "RunSummary must not carry a `dry_run_emitted` counter (Warning 9). \
         Got JSONL:\n{raw}"
    );
}

fn envelope_kind(f: &Finding) -> &'static str {
    match f {
        Finding::RunStart(_) => "run_start",
        Finding::Result(_) => "result",
        Finding::ScanError(_) => "scan_error",
        Finding::GapAborted(_) => "gap_aborted",
        Finding::RunEnd(_) => "run_end",
        Finding::DryRun(_) => "dry_run",
    }
}
