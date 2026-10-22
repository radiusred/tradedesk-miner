//! M33-R2 — calendar-aware gap policy, end to end.
//!
//! Drives [`engine::run_one`] through `tradedesk-data`'s gap detector against a
//! synthetic cache that holds bars only while an FX market on New York time is open:
//! two northern-summer weeks, so the market closes each Friday at 21:00 UTC. The
//! detector's fixed UTC calendar still counts Friday 21:00-22:00 UTC as open, so it
//! reports that hour as missing.
//!
//! - Under the `fx_new_york` calendar (the default) the closed hour is not a gap: the
//!   two-week window gives one `Result`, and `strict` runs instead of aborting.
//! - Under `fx_utc` (the gap dispatch before M33-R2) the hour splits the window.
//! - A real hole in open hours still splits the window under `fx_new_york`.
//! - `run_start.request.calendar` states the calendar in force.

mod common;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use chrono::{Datelike, NaiveDate, TimeZone, Utc, Weekday};

use miner_core::config::{MinerConfig, OutputDest};
use miner_core::engine::gap_policy::GapPolicyKind;
use miner_core::engine::{MarketCalendar, RunOutcome, param_hash, run_one};
use miner_core::findings::{Finding, TimeRange};
use miner_core::scan::ScanRequest;
use tradedesk_data::aggregator::Timeframe;
use tradedesk_data::dukascopy::DukascopyReader;
use tradedesk_data::reader::{ClosedRangeUtc, InstrumentSpec, Side};

use common::{BufferSink, parse_findings, synthetic_cache::SyntheticCache};

/// Sunday 2024-06-09 to Friday 2024-06-21 (EDT, UTC-4): the market opens on Sunday at
/// 21:00 UTC and closes on Friday at 21:00 UTC. Saturdays have no file. `extra_hole`
/// adds a hole (minute offsets from midnight) on Wednesday 2024-06-19.
fn summer_fx_cache(extra_hole: Option<&std::ops::Range<i64>>) -> SyntheticCache {
    let mut cache = SyntheticCache::new();
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
            holes.extend(extra_hole.cloned());
        }
        cache = cache.with_day_multi_holed("EURUSD", Side::Bid, date, seed, &holes);
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

fn request(policy: GapPolicyKind, calendar: &str) -> ScanRequest {
    let resolved = serde_json::json!({"lags": 5});
    let param_hash = param_hash::param_hash(&resolved).expect("param_hash ok");
    let w = window();
    ScanRequest::new(
        "stats.autocorr.ljung_box".into(),
        1,
        vec![InstrumentSpec {
            symbol: "EURUSD".into(),
            side: Side::Bid,
        }],
        Timeframe::Tf1h,
        w,
        TimeRange {
            start_utc: w.start,
            end_utc: w.end,
        },
        policy,
        false,
        resolved,
        param_hash,
    )
    .with_calendar(MarketCalendar::preset(calendar).expect("preset"))
}

fn run(cache: &SyntheticCache, req: &ScanRequest) -> (Vec<Finding>, String) {
    let cfg = MinerConfig {
        cache_root: cache.cache_root().to_path_buf(),
        bar_cache_root: cache.bar_cache_root().to_path_buf(),
        output: OutputDest::Stdout,
        not_after: None,
    };
    let reader = DukascopyReader::new(cache.cache_root());
    let mut sink = BufferSink::new();
    let outcome = run_one(
        req,
        &cfg,
        &reader,
        &mut sink,
        Arc::new(AtomicBool::new(false)),
    )
    .expect("run_one ok");
    assert_eq!(outcome, RunOutcome::Ok, "stream:\n{}", sink.as_str());
    let text = sink.as_str().to_string();
    (parse_findings(&sink.0), text)
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

fn count_gap_aborted(findings: &[Finding]) -> usize {
    findings
        .iter()
        .filter(|f| matches!(f, Finding::GapAborted(_)))
        .count()
}

#[test]
#[serial_test::serial]
fn closed_friday_hour_does_not_split_the_window_under_fx_new_york() {
    let cache = summer_fx_cache(None);

    let (findings, stream) = run(
        &cache,
        &request(GapPolicyKind::ContinuousOnly, "fx_new_york"),
    );
    let ranges = result_ranges(&findings);
    assert_eq!(
        ranges.len(),
        1,
        "one Result over two weeks; stream:\n{stream}"
    );

    let (findings, stream) = run(&cache, &request(GapPolicyKind::ContinuousOnly, "fx_utc"));
    assert_eq!(
        result_ranges(&findings).len(),
        2,
        "fx_utc splits at Friday 21:00-22:00 UTC; stream:\n{stream}"
    );

    let (findings, stream) = run(&cache, &request(GapPolicyKind::Strict, "fx_new_york"));
    assert_eq!(count_gap_aborted(&findings), 0, "stream:\n{stream}");
    assert_eq!(result_ranges(&findings).len(), 1, "stream:\n{stream}");

    let (findings, stream) = run(&cache, &request(GapPolicyKind::Strict, "fx_utc"));
    assert_eq!(count_gap_aborted(&findings), 1, "stream:\n{stream}");
}

#[test]
#[serial_test::serial]
fn real_hole_in_open_hours_still_splits_under_fx_new_york() {
    // Wednesday 2024-06-19 10:00-12:00 UTC: open, no data.
    let cache = summer_fx_cache(Some(&(600..720)));
    let (findings, stream) = run(
        &cache,
        &request(GapPolicyKind::ContinuousOnly, "fx_new_york"),
    );
    let ranges = result_ranges(&findings);
    assert_eq!(ranges.len(), 2, "stream:\n{stream}");
    assert_eq!(
        ranges[0].end_utc,
        Utc.with_ymd_and_hms(2024, 6, 19, 10, 0, 0).unwrap()
    );
    assert_eq!(
        ranges[1].start_utc,
        Utc.with_ymd_and_hms(2024, 6, 19, 12, 0, 0).unwrap()
    );
}

#[test]
#[serial_test::serial]
fn run_start_states_the_calendar() {
    let cache = summer_fx_cache(None);
    let (findings, _) = run(
        &cache,
        &request(GapPolicyKind::ContinuousOnly, "fx_new_york"),
    );
    let Finding::RunStart(rs) = &findings[0] else {
        panic!("first envelope must be RunStart");
    };
    let cal = &rs.request["calendar"];
    assert_eq!(cal["time_zone"], "America/New_York");
    assert_eq!(cal["week_open"], "sun 17:00");
    assert_eq!(cal["week_close"], "fri 17:00");
    assert_eq!(cal["trading_day_start"], "17:00");
}
