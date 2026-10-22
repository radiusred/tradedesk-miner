//! M33-R3 — the coverage-only dry run, end to end.
//!
//! `miner scan --dry-run` and `miner sweep --dry-run` list, for each job, the
//! gap-free sub-ranges the gap policy gives, from the cache's timestamps only.
//! The synthetic cache holds bars only while an FX market on New York time is
//! open (two northern-summer weeks, as in `gap_calendar.rs`), so weekends and
//! the Friday close are closed time, not gaps.
//!
//! - A weekend inside the window gives one sub-range; a real hole gives two,
//!   and the hole is listed in `gaps`.
//! - The dry-run sub-ranges are the sub-ranges the real run scans.
//! - `strict` with a hole gives `gap_aborted` and no sub-range.
//! - A window with no cache files gives no sub-range.
//! - A two-leg job's gaps are the union of both legs' gaps.
//! - A sweep dry run lists every job in job-graph order with its own coverage,
//!   and emits no `result`.
//! - Coverage reads the timestamps only: unparseable OHLC and volume fields
//!   give the same coverage as numeric fields.

mod common;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use chrono::{Datelike, NaiveDate, TimeZone, Utc, Weekday};

use miner_core::config::{MinerConfig, OutputDest};
use miner_core::engine::coverage::job_coverage;
use miner_core::engine::gap_policy::GapPolicyKind;
use miner_core::engine::{RunOutcome, param_hash, run_one};
use miner_core::findings::{CoverageStatus, Finding, JobCoverage, TimeRange};
use miner_core::scan::ScanRequest;
use miner_core::sweep::manifest::parse_manifest_str;
use miner_core::sweep::{SweepOptions, run_sweep};
use tradedesk_data::aggregator::Timeframe;
use tradedesk_data::cache::BarCache;
use tradedesk_data::dukascopy::DukascopyReader;
use tradedesk_data::reader::{ClosedRangeUtc, InstrumentSpec, Side};

use common::{BufferSink, parse_findings, synthetic_cache::SyntheticCache};

/// Wednesday 2024-06-19 10:00-12:00 UTC: open, no data.
const HOLE: std::ops::Range<i64> = 600..720;

/// Sunday 2024-06-09 to Friday 2024-06-21 (EDT) for `symbol`: open from Sunday
/// 21:00 UTC to Friday 21:00 UTC, no Saturday file. `hole` adds a hole on
/// Wednesday 2024-06-19.
fn add_summer_weeks(
    cache: SyntheticCache,
    symbol: &str,
    hole: Option<&std::ops::Range<i64>>,
) -> SyntheticCache {
    add_summer_weeks_with(cache, hole, |cache, date, seed, holes| {
        cache.with_day_multi_holed(symbol, Side::Bid, date, seed, holes)
    })
}

/// [`add_summer_weeks`] with a caller-supplied day writer, which names the
/// symbol.
fn add_summer_weeks_with(
    mut cache: SyntheticCache,
    hole: Option<&std::ops::Range<i64>>,
    write_day: impl Fn(SyntheticCache, NaiveDate, u32, &[std::ops::Range<i64>]) -> SyntheticCache,
) -> SyntheticCache {
    let mut date = NaiveDate::from_ymd_opt(2024, 6, 9).unwrap();
    let last = NaiveDate::from_ymd_opt(2024, 6, 21).unwrap();
    let mut seed = 0x5EED_0001_u32;
    while date <= last {
        let mut holes: Vec<std::ops::Range<i64>> = match date.weekday() {
            Weekday::Sat => {
                date = date.succ_opt().unwrap();
                continue;
            }
            Weekday::Sun => std::iter::once(0..1260).collect(),
            Weekday::Fri => std::iter::once(1260..1440).collect(),
            _ => Vec::new(),
        };
        if date == NaiveDate::from_ymd_opt(2024, 6, 19).unwrap() {
            holes.extend(hole.cloned());
        }
        cache = write_day(cache, date, seed, &holes);
        seed = seed.wrapping_add(7);
        date = date.succ_opt().unwrap();
    }
    cache
}

fn window() -> ClosedRangeUtc {
    ClosedRangeUtc {
        start: Utc.with_ymd_and_hms(2024, 6, 10, 0, 0, 0).unwrap(),
        end: Utc.with_ymd_and_hms(2024, 6, 22, 0, 0, 0).unwrap(),
    }
}

fn request(symbols: &[&str], policy: GapPolicyKind, dry_run: bool) -> ScanRequest {
    let resolved = serde_json::json!({"lags": 5});
    let param_hash = param_hash::param_hash(&resolved).expect("param_hash ok");
    let w = window();
    ScanRequest::new(
        "stats.autocorr.ljung_box".into(),
        1,
        symbols
            .iter()
            .map(|s| InstrumentSpec {
                symbol: (*s).into(),
                side: Side::Bid,
            })
            .collect(),
        Timeframe::Tf1h,
        w,
        TimeRange {
            start_utc: w.start,
            end_utc: w.end,
        },
        policy,
        dry_run,
        resolved,
        param_hash,
    )
}

fn config(cache: &SyntheticCache) -> MinerConfig {
    MinerConfig {
        cache_root: cache.cache_root().to_path_buf(),
        bar_cache_root: cache.bar_cache_root().to_path_buf(),
        output: OutputDest::Stdout,
        not_after: None,
    }
}

fn run(cache: &SyntheticCache, req: &ScanRequest) -> (Vec<Finding>, String) {
    let reader = DukascopyReader::new(cache.cache_root());
    let mut sink = BufferSink::new();
    let outcome = run_one(
        req,
        &config(cache),
        &reader,
        &mut sink,
        Arc::new(AtomicBool::new(false)),
    )
    .expect("run_one ok");
    assert_eq!(outcome, RunOutcome::Ok, "stream:\n{}", sink.as_str());
    let text = sink.as_str().to_string();
    (parse_findings(&sink.0), text)
}

/// Run a scan dry run; assert the stream is `run_start`, `dry_run`, `run_end`
/// and return the single coverage entry.
fn dry_run_coverage(cache: &SyntheticCache, policy: GapPolicyKind) -> JobCoverage {
    let (findings, stream) = run(cache, &request(&["EURUSD"], policy, true));
    assert_eq!(findings.len(), 3, "stream:\n{stream}");
    assert!(matches!(findings[0], Finding::RunStart(_)));
    assert!(matches!(findings[2], Finding::RunEnd(_)));
    let Finding::DryRun(dr) = &findings[1] else {
        panic!("expected dry_run at index 1; stream:\n{stream}");
    };
    assert_eq!(dr.coverage.len(), 1, "stream:\n{stream}");
    dr.coverage[0].clone()
}

fn result_ranges(findings: &[Finding]) -> Vec<TimeRange> {
    findings
        .iter()
        .filter_map(|f| match f {
            Finding::Result(r) => Some(r.data_slice.range.clone()),
            _ => None,
        })
        .collect()
}

fn utc(d: u32, h: u32) -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2024, 6, d, h, 0, 0).unwrap()
}

#[test]
#[serial_test::serial]
fn weekend_and_friday_close_give_one_sub_range() {
    let cache = add_summer_weeks(SyntheticCache::new(), "EURUSD", None);
    let entry = dry_run_coverage(&cache, GapPolicyKind::ContinuousOnly);

    assert_eq!(entry.status, CoverageStatus::Covered);
    assert_eq!(entry.job_index, 0);
    assert_eq!(entry.scan_id_at_version, "stats.autocorr.ljung_box@1");
    assert_eq!(entry.gap_policy, "continuous_only");
    assert_eq!(entry.sources.len(), 1);
    assert_eq!(entry.sources[0].symbol, "EURUSD");
    assert_eq!(entry.sources[0].timeframe, "1h");
    assert!(entry.gaps.is_empty(), "{:?}", entry.gaps);
    assert_eq!(
        entry.sub_ranges,
        vec![TimeRange {
            start_utc: window().start,
            end_utc: window().end,
        }]
    );
}

#[test]
#[serial_test::serial]
fn real_hole_splits_and_is_listed() {
    let cache = add_summer_weeks(SyntheticCache::new(), "EURUSD", Some(&HOLE));
    let entry = dry_run_coverage(&cache, GapPolicyKind::ContinuousOnly);

    assert_eq!(entry.status, CoverageStatus::Covered);
    assert_eq!(
        entry.gaps,
        vec![TimeRange {
            start_utc: utc(19, 10),
            end_utc: utc(19, 12),
        }]
    );
    assert_eq!(
        entry.sub_ranges,
        vec![
            TimeRange {
                start_utc: window().start,
                end_utc: utc(19, 10),
            },
            TimeRange {
                start_utc: utc(19, 12),
                end_utc: window().end,
            },
        ]
    );
}

#[test]
#[serial_test::serial]
fn dry_run_sub_ranges_are_the_sub_ranges_the_run_scans() {
    for hole in [None, Some(&HOLE)] {
        let cache = add_summer_weeks(SyntheticCache::new(), "EURUSD", hole);
        let entry = dry_run_coverage(&cache, GapPolicyKind::ContinuousOnly);
        let (findings, stream) = run(
            &cache,
            &request(&["EURUSD"], GapPolicyKind::ContinuousOnly, false),
        );
        assert_eq!(
            entry.sub_ranges,
            result_ranges(&findings),
            "hole {hole:?}; stream:\n{stream}"
        );
    }
}

#[test]
#[serial_test::serial]
fn strict_with_a_hole_is_gap_aborted() {
    let cache = add_summer_weeks(SyntheticCache::new(), "EURUSD", Some(&HOLE));
    let entry = dry_run_coverage(&cache, GapPolicyKind::Strict);
    assert_eq!(entry.status, CoverageStatus::GapAborted);
    assert_eq!(entry.gap_policy, "strict");
    assert!(entry.sub_ranges.is_empty());
    assert_eq!(entry.gaps.len(), 1);

    // Without the hole, strict runs over the whole window.
    let cache = add_summer_weeks(SyntheticCache::new(), "EURUSD", None);
    let entry = dry_run_coverage(&cache, GapPolicyKind::Strict);
    assert_eq!(entry.status, CoverageStatus::Covered);
    assert_eq!(entry.sub_ranges.len(), 1);
}

#[test]
#[serial_test::serial]
fn no_cache_files_give_no_sub_range() {
    let cache = SyntheticCache::new();
    let entry = dry_run_coverage(&cache, GapPolicyKind::ContinuousOnly);
    assert_eq!(entry.status, CoverageStatus::Covered);
    assert!(entry.sub_ranges.is_empty(), "{:?}", entry.sub_ranges);
    assert!(!entry.gaps.is_empty());
}

#[test]
#[serial_test::serial]
fn two_leg_gaps_are_the_union_of_both_legs() {
    let cache = add_summer_weeks(SyntheticCache::new(), "EURUSD", None);
    let cache = add_summer_weeks(cache, "GBPUSD", Some(&HOLE));
    let reader = DukascopyReader::new(cache.cache_root());

    let entry = job_coverage(
        3,
        &request(&["EURUSD", "GBPUSD"], GapPolicyKind::ContinuousOnly, true),
        &reader,
    );
    assert_eq!(entry.job_index, 3);
    assert_eq!(entry.sources.len(), 2);
    assert_eq!(
        entry.gaps,
        vec![TimeRange {
            start_utc: utc(19, 10),
            end_utc: utc(19, 12),
        }]
    );
    assert_eq!(entry.sub_ranges.len(), 2);
}

#[test]
#[serial_test::serial]
fn sweep_dry_run_lists_every_job_in_order() {
    let cache = add_summer_weeks(SyntheticCache::new(), "EURUSD", None);
    let cache = add_summer_weeks(cache, "GBPUSD", Some(&HOLE));
    let reader = DukascopyReader::new(cache.cache_root());
    let bar_cache = BarCache::new(cache.bar_cache_root());

    let manifest = parse_manifest_str(
        r#"
        [[jobs]]
        scan = "stats.autocorr.ljung_box@1"
        instruments = ["EURUSD:bid", "GBPUSD:bid"]
        timeframes = ["1h"]
        windows = ["2024-06-10:2024-06-22"]
        params = { lags = 5 }
    "#,
    )
    .expect("manifest parses");

    let mut sink = BufferSink::new();
    let outcome = run_sweep(
        manifest,
        SweepOptions {
            dry_run: true,
            ..Default::default()
        },
        &config(&cache),
        &reader,
        &bar_cache,
        &mut sink,
        Arc::new(AtomicBool::new(false)),
    )
    .expect("sweep dry run ok");
    assert_eq!(outcome, RunOutcome::Ok);

    let findings = parse_findings(&sink.0);
    let stream = sink.as_str();
    assert_eq!(findings.len(), 3, "stream:\n{stream}");
    let Finding::DryRun(dr) = &findings[1] else {
        panic!("expected dry_run at index 1; stream:\n{stream}");
    };
    assert_eq!(dr.planned_job_count, Some(2));
    assert_eq!(dr.coverage.len(), 2, "stream:\n{stream}");

    let by_symbol: Vec<(&str, usize)> = dr
        .coverage
        .iter()
        .map(|c| (c.sources[0].symbol.as_str(), c.sub_ranges.len()))
        .collect();
    assert_eq!(by_symbol, vec![("EURUSD", 1), ("GBPUSD", 2)]);
    assert_eq!(
        dr.coverage.iter().map(|c| c.job_index).collect::<Vec<_>>(),
        vec![0, 1]
    );
    assert!(
        findings.iter().all(|f| !matches!(f, Finding::Result(_))),
        "stream:\n{stream}"
    );
}

#[test]
#[serial_test::serial]
fn coverage_reads_timestamps_only() {
    // The same minutes (weekends, the Friday close and a real hole omitted),
    // once with unparseable price and volume text, once with numeric fields.
    let text = |price: &'static str, volume: &'static str| {
        add_summer_weeks_with(
            SyntheticCache::new(),
            Some(&HOLE),
            move |cache, date, _seed, holes| {
                cache.with_day_multi_holed_fields("EURUSD", Side::Bid, date, holes, price, volume)
            },
        )
    };
    let unparseable = text("NOT_A_PRICE", "NOT_A_VOLUME");
    let numeric = text("1.0", "1");
    let req = request(&["EURUSD"], GapPolicyKind::ContinuousOnly, true);

    let a = job_coverage(0, &req, &DukascopyReader::new(unparseable.cache_root()));
    let b = job_coverage(0, &req, &DukascopyReader::new(numeric.cache_root()));

    assert_eq!(a.status, CoverageStatus::Covered, "{a:?}");
    assert_eq!(
        a.gaps,
        vec![TimeRange {
            start_utc: utc(19, 10),
            end_utc: utc(19, 12),
        }]
    );
    assert_eq!(
        a.sub_ranges,
        vec![
            TimeRange {
                start_utc: window().start,
                end_utc: utc(19, 10),
            },
            TimeRange {
                start_utc: utc(19, 12),
                end_utc: window().end,
            },
        ]
    );
    assert_eq!(a, b);
}
