// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: 2026 The tradedesk-miner authors

//! Plan 07-06 Task 2 — criterion microbench for the 1m→15m aggregator hot
//! kernel (`miner_core::aggregator::aggregate`, callsite at
//! `crates/miner-core/src/aggregator.rs:304`).
//!
//! Input shape: 250 synthetic trading days × 1440 1-minute bars = 360 000
//! `RawBar`s, driven through an in-memory `Reader` impl. The bars are
//! built ONCE outside the timed loop; each iteration re-runs the
//! aggregator over the same `Reader`, producing a 15m `BarFrame` of
//! ~24 000 buckets.
//!
//! Bar values come from the canonical Numerical Recipes LCG (PATTERNS
//! Pattern C — `crates/miner-core/tests/byte_identical_rerun.rs:74-83`),
//! producing deterministic synthetic closes in `[1.0, 2.0]`. The constants
//! `1_664_525` + `1_013_904_223` are cross-platform deterministic; do NOT
//! replace with `rand::SmallRng` (07-RESEARCH Anti-Patterns / `SmallRng` /
//! `StdRng` explicitly non-portable).
//!
//! Reports to `target/criterion/aggregate_1m_to_15m_360000_bars/index.html`.

use std::collections::BTreeMap;

use chrono::{DateTime, Duration, NaiveDate, TimeZone, Utc};
use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};

use miner_core::reader::RawBarIter;
use miner_core::{
    AggParams, Blake3Hex, Calendar, ClosedRangeUtc, RawBar, Reader, Side, Timeframe, aggregate,
};

/// Canonical Numerical Recipes LCG (PATTERNS Pattern C). The constants
/// `1_664_525` + `1_013_904_223` are cross-platform deterministic.
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

/// `BTreeMap`-backed Reader fixture — mirrors `MockReader` from
/// `crates/miner-core/tests/aggregator_fixtures.rs` but inlined so the
/// bench has no integration-test cross-link (benches and tests compile
/// as separate units).
struct BenchReader {
    bars: BTreeMap<(String, Side, NaiveDate), Vec<RawBar>>,
    calendar: Calendar,
}

impl BenchReader {
    fn new() -> Self {
        Self {
            bars: BTreeMap::new(),
            calendar: Calendar::fx_major(),
        }
    }
}

impl Reader for BenchReader {
    type Error = std::io::Error;

    fn source_id(&self) -> &'static str {
        "bench"
    }

    fn trading_calendar(&self) -> Calendar {
        self.calendar.clone()
    }

    fn read_1m_bars<'a>(
        &'a self,
        symbol: &str,
        side: Side,
        range: ClosedRangeUtc,
    ) -> Result<RawBarIter<'a, Self::Error>, Self::Error> {
        let start_date = range.start.date_naive();
        let end_date = range.end.date_naive();
        let mut all: Vec<RawBar> = Vec::new();
        for ((sym, sd, date), bars) in &self.bars {
            if sym != symbol || *sd != side {
                continue;
            }
            if *date < start_date || *date > end_date {
                continue;
            }
            for bar in bars {
                if bar.ts_open_utc >= range.start && bar.ts_open_utc < range.end {
                    all.push(*bar);
                }
            }
        }
        Ok(Box::new(all.into_iter().map(Ok)))
    }

    fn fingerprint_day(
        &self,
        symbol: &str,
        side: Side,
        date: NaiveDate,
    ) -> Result<Option<Blake3Hex>, Self::Error> {
        let key = (symbol.to_string(), side, date);
        if self.bars.contains_key(&key) {
            Ok(Some(Blake3Hex::from_hex_bytes(&[b'0'; 64])))
        } else {
            Ok(None)
        }
    }

    fn enumerate_days(
        &self,
        symbol: &str,
        side: Side,
        range: ClosedRangeUtc,
    ) -> Result<Vec<NaiveDate>, Self::Error> {
        let start_date = range.start.date_naive();
        let end_date = range.end.date_naive();
        let mut out: Vec<NaiveDate> = self
            .bars
            .keys()
            .filter(|(s, sd, _)| s == symbol && *sd == side)
            .map(|(_, _, d)| *d)
            .filter(|d| *d >= start_date && *d <= end_date)
            .collect();
        out.sort_unstable();
        Ok(out)
    }
}

/// Build a `BenchReader` carrying `days` consecutive 24-hour blocks of
/// 1-minute bars (1440 bars/day). Closes come from a single LCG sweep
/// across all days; OHLC manufactures small jitter around each close,
/// volume is `(i + 1) as f64` (same scheme as
/// `crates/miner-bench/src/bin/gen-fixtures.rs:85-105`).
#[allow(clippy::cast_precision_loss, clippy::cast_possible_wrap)]
fn build_synthetic_reader(symbol: &str, start: NaiveDate, days: usize) -> BenchReader {
    let n_bars = days * 1440;
    let closes = lcg_closes(n_bars, 0xCAFE);
    let mut reader = BenchReader::new();
    let day_zero_start: DateTime<Utc> =
        Utc.from_utc_datetime(&start.and_hms_opt(0, 0, 0).expect("00:00:00 valid"));
    for d in 0..days {
        let date = start
            .checked_add_signed(Duration::days(d as i64))
            .expect("date in synthetic range");
        let day_start = day_zero_start + Duration::days(d as i64);
        let mut bars: Vec<RawBar> = Vec::with_capacity(1440);
        for i in 0..1440 {
            let ts_open = day_start + Duration::minutes(i as i64);
            let ts_close = ts_open + Duration::minutes(1);
            let c = closes[d * 1440 + i];
            bars.push(RawBar {
                ts_open_utc: ts_open,
                ts_close_utc: ts_close,
                open: c,
                high: c + 0.000_05,
                low: c - 0.000_05,
                close: c,
                tick_volume: (i + 1) as f64,
            });
        }
        reader
            .bars
            .insert((symbol.to_string(), Side::Bid, date), bars);
    }
    reader
}

fn bench_aggregate_1m_to_15m(c: &mut Criterion) {
    // 250 trading days × 1440 1-minute bars = 360 000 bars. Builds ONCE
    // outside the timed loop; each iteration aggregates over the same
    // `Reader` so we measure the aggregator hot path, not the synthetic
    // fixture build cost.
    let symbol = "EURUSD";
    let start = NaiveDate::from_ymd_opt(2024, 1, 1).expect("valid date");
    let reader = build_synthetic_reader(symbol, start, 250);
    let range_start = Utc.from_utc_datetime(&start.and_hms_opt(0, 0, 0).expect("00:00:00 valid"));
    let range_end = range_start + Duration::days(250);
    let range = ClosedRangeUtc {
        start: range_start,
        end: range_end,
    };
    let params = AggParams {
        symbol,
        side: Side::Bid,
        tf: Timeframe::Tf15m,
        range,
    };

    c.bench_function("aggregate_1m_to_15m_360000_bars", |b| {
        b.iter(|| {
            let frame = aggregate(black_box(&reader), black_box(params))
                .expect("aggregate succeeds on aligned range");
            black_box(frame);
        });
    });
}

criterion_group!(benches, bench_aggregate_1m_to_15m);
criterion_main!(benches);
