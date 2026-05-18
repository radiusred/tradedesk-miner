//! Deterministic AR(1) bar-frame builder for Plan 03-06 integration tests.
//!
//! Mirrors the analog in `crates/miner-core/src/scan/ljung_box/mod.rs`'s
//! unit tests; we duplicate the logic here because integration tests under
//! `tests/` cannot reach into a sibling crate's `#[cfg(test)] mod tests`
//! sub-module.
//!
//! For the byte-exact statsmodels golden test we read the `close` array
//! verbatim from `tests/fixtures/ljung_box_golden.json` (the Python script
//! emits the LE-f64-packed AR(1) input alongside the expected output — see
//! `crates/miner-core/tests/fixtures/generate_golden.py`). The function below
//! is the fallback path for tests that just want a deterministic AR(1)-shaped
//! series (no statsmodels parity required).
//!
//! Reference function name: `ar1_bar_frame_seeded(n, seed, start_ts, tf_minutes)`
//! — chosen to match the plan's `<action>` step 3 spec.

#![allow(dead_code)]

use chrono::{DateTime, Duration, Utc};
use miner_core::aggregator::{BarFrame, Timeframe};
use miner_core::reader::Side;

/// Build a deterministic AR(1)-shaped `BarFrame` with `n` bars starting at
/// `start_ts` spaced by `tf_minutes`. The LCG implementation is identical to
/// the per-day LCG in the synthetic-cache builder so callers can pre-compute
/// expected closes if needed.
#[must_use]
pub fn ar1_bar_frame_seeded(
    n: usize,
    seed: u64,
    start_ts: DateTime<Utc>,
    tf_minutes: u32,
) -> BarFrame {
    #[allow(clippy::cast_possible_truncation)]
    let mut s = seed as u32;
    let mut closes = Vec::with_capacity(n);
    for _ in 0..n {
        s = s
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        let frac = f64::from(s) / f64::from(u32::MAX);
        closes.push(1.0 + frac);
    }
    let step_minutes = i64::from(tf_minutes);
    let ts_open: Vec<DateTime<Utc>> = (0..n)
        .map(|i| {
            let i_i64 = i64::try_from(i).expect("fits in i64");
            start_ts + Duration::minutes(step_minutes * i_i64)
        })
        .collect();
    let ts_close: Vec<DateTime<Utc>> = ts_open
        .iter()
        .map(|t| *t + Duration::minutes(step_minutes))
        .collect();
    let opens = closes.clone();
    let highs: Vec<f64> = closes.iter().map(|c| c + 0.001).collect();
    let lows: Vec<f64> = closes.iter().map(|c| c - 0.001).collect();
    let vols = vec![1.0; n];
    BarFrame {
        source_id: "dukascopy".into(),
        symbol: "EURUSD".into(),
        side: Side::Bid,
        tf: Timeframe::Tf15m,
        ts_open_utc: ts_open,
        ts_close_utc: ts_close,
        open: opens,
        high: highs,
        low: lows,
        close: closes,
        tick_volume: vols,
    }
}
