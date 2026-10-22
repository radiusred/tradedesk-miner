//! M33-R1 — date fence (`not_after`) on the sweep path.
//!
//! Sweep `windows` are documented closed at both ends, while `scan --window`
//! is half-open. The sweep preflight reads every window as closed (the
//! stricter reading), so the fence holds under both. A refused sweep writes
//! nothing to the sink, `--dry-run` included. Synthetic fixtures only.

mod common;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use chrono::{DateTime, NaiveDate, TimeZone, Utc};

use miner_core::config::{MinerConfig, OutputDest};
use miner_core::engine::RunOutcome;
use miner_core::error::MinerError;
use miner_core::findings::Finding;
use miner_core::sweep::manifest::parse_manifest_str;
use miner_core::sweep::{SweepOptions, run_sweep};
use tradedesk_data::cache::BarCache;
use tradedesk_data::dukascopy::DukascopyReader;
use tradedesk_data::reader::Side;

use common::{BufferSink, synthetic_cache::SyntheticCache};

fn fence() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2024, 6, 14, 0, 0, 0).unwrap()
}

fn run(manifest_toml: &str, dry_run: bool) -> (Result<RunOutcome, MinerError>, BufferSink) {
    let day = NaiveDate::from_ymd_opt(2024, 6, 12).expect("valid date");
    let cache = SyntheticCache::new().with_deterministic_day("EURUSD", Side::Bid, day, 42);
    let cfg = MinerConfig {
        cache_root: cache.cache_root().to_path_buf(),
        bar_cache_root: cache.bar_cache_root().to_path_buf(),
        output: OutputDest::Stdout,
        not_after: Some(fence()),
    };
    let reader = DukascopyReader::new(cache.cache_root());
    let bar_cache = BarCache::new(cache.bar_cache_root());
    let manifest = parse_manifest_str(manifest_toml).expect("manifest parses");
    let mut sink = BufferSink::new();
    let res = run_sweep(
        manifest,
        SweepOptions {
            dry_run,
            ..Default::default()
        },
        &cfg,
        &reader,
        &bar_cache,
        &mut sink,
        Arc::new(AtomicBool::new(false)),
    );
    (res, sink)
}

fn ljung_box_manifest(window: &str) -> String {
    format!(
        r#"
        [[jobs]]
        scan = "stats.autocorr.ljung_box@1"
        instruments = ["EURUSD:bid"]
        timeframes = ["15m"]
        windows = ["2024-06-12:2024-06-13", "{window}"]
        params = {{ lags = 5 }}
        "#
    )
}

fn assert_refused(res: Result<RunOutcome, MinerError>, sink: &BufferSink, reached_by: &str) {
    match res {
        Err(MinerError::Preflight(w)) => {
            assert_eq!(w.code, "date_fence", "{w:?}");
            assert_eq!(
                w.context.get("reached_by"),
                Some(&serde_json::json!(reached_by))
            );
            assert_eq!(w.context.get("block"), Some(&serde_json::json!(0)));
            assert_eq!(
                w.context.get("not_after"),
                Some(&serde_json::json!("2024-06-14T00:00:00Z"))
            );
        }
        other => panic!("expected date_fence preflight refusal; got {other:?}"),
    }
    assert!(sink.0.is_empty(), "a refused sweep writes no envelope");
}

/// A date-only end on the fence day is closed: it includes that whole day,
/// so it reaches the fence. Refused for a real run and for `--dry-run`.
#[test]
fn sweep_refuses_closed_window_ending_on_fence_day() {
    for dry_run in [false, true] {
        let (res, sink) = run(&ljung_box_manifest("2024-06-12:2024-06-14"), dry_run);
        assert_refused(res, &sink, "window");
    }
}

/// A closed datetime end on the fence instant uses that instant: refused.
/// The same string as a half-open `scan --window` would be accepted; the
/// sweep reads it the stricter way.
#[test]
fn sweep_refuses_closed_datetime_end_on_fence() {
    let (res, sink) = run(
        &ljung_box_manifest("2024-06-12T00:00:00Z:2024-06-14T00:00:00Z"),
        true,
    );
    assert_refused(res, &sink, "window");
}

/// The day before the fence, closed, ends on the fence: accepted. The
/// sweep `RunStart` states the fence.
#[test]
fn sweep_accepts_window_before_fence_and_states_it() {
    let (res, sink) = run(&ljung_box_manifest("2024-06-12:2024-06-13"), true);
    assert_eq!(res.expect("accepted"), RunOutcome::Ok);
    let findings = common::parse_findings(&sink.0);
    let Some(Finding::RunStart(rs)) = findings.first() else {
        panic!("first envelope must be RunStart");
    };
    assert_eq!(
        rs.request.get("not_after"),
        Some(&serde_json::json!("2024-06-14T00:00:00Z"))
    );
}

/// An event window named in a param grid that reaches the fence is refused,
/// even though the request window is clear of it.
#[test]
fn sweep_refuses_event_window_past_fence() {
    let ms = |d: u32, h: u32| {
        Utc.with_ymd_and_hms(2024, 6, d, h, 0, 0)
            .unwrap()
            .timestamp_millis()
    };
    // Second grid point: an event at 2024-06-13 22:00, 1h bars, 3 post bars
    // -> post window ends 2024-06-14 01:00, past the fence.
    let manifest = format!(
        r#"
        [[jobs]]
        scan = "seas.event.pre_post_window@1"
        instruments = ["EURUSD:bid"]
        timeframes = ["1h"]
        windows = ["2024-06-12:2024-06-13"]
        params = {{ event_timestamps = [[{a}], [{b}]], post_window_bars = 3 }}
        "#,
        a = ms(12, 12),
        b = ms(13, 22),
    );
    let (res, sink) = run(&manifest, false);
    assert_refused(res, &sink, "event_window");
}

/// An event between bar boundaries aligns to the next bar open, as the
/// kernel does. 2024-06-13 22:30 with 1h bars and 2 post bars uses the bars
/// at 23:00 and 2024-06-14 00:00 (on the fence): refused, `--dry-run` too.
/// The same event on the boundary (22:00) uses 22:00 and 23:00: accepted.
#[test]
fn sweep_event_between_bar_boundaries_aligns_up() {
    let manifest = |event: DateTime<Utc>| {
        format!(
            r#"
            [[jobs]]
            scan = "seas.event.pre_post_window@1"
            instruments = ["EURUSD:bid"]
            timeframes = ["1h"]
            windows = ["2024-06-12:2024-06-13"]
            params = {{ event_timestamps = [[{e}]], post_window_bars = 2 }}
            "#,
            e = event.timestamp_millis(),
        )
    };
    let between = Utc.with_ymd_and_hms(2024, 6, 13, 22, 30, 0).unwrap();
    for dry_run in [false, true] {
        let (res, sink) = run(&manifest(between), dry_run);
        assert_refused(res, &sink, "event_window");
    }
    let aligned = Utc.with_ymd_and_hms(2024, 6, 13, 22, 0, 0).unwrap();
    let (res, _sink) = run(&manifest(aligned), true);
    assert_eq!(res.expect("accepted"), RunOutcome::Ok);
}
