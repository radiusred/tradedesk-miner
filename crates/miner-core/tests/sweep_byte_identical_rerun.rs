//! Plan 05-04 Task 3 — sweep_byte_identical_rerun integration test.
//!
//! HYG-05 end-to-end reproducibility regression: run the SAME sweep
//! manifest with `[sweep].seed = 0xDEAD` TWICE; mask volatile envelope
//! fields (`run_id`, `produced_at_utc`, `ended_at_utc`,
//! `wall_clock_ms`); assert the masked envelope sequences are
//! byte-identical (per-envelope `serde_json::Value` equality).
//!
//! Two variants exercised:
//!
//! 1. **No hygiene** — sweep without `[hygiene]`. Exercises the
//!    cartesian expansion + rayon-fanout + buffered-drain
//!    determinism (RESEARCH Pattern 4).
//! 2. **Hygiene ON** — sweep with `[hygiene]` driving
//!    `bootstrap = "stationary"` + `null = "circular_shift"` on a
//!    scan that opts into both. Exercises the per-job seeded
//!    bootstrap + null pipeline (Plan 05-03 continuation 2 dispatch).

#![allow(clippy::doc_lazy_continuation, clippy::doc_markdown)]

mod common;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use chrono::NaiveDate;

use miner_core::cache::BarCache;
use miner_core::config::{MinerConfig, OutputDest};
use miner_core::reader::Side;
use miner_core::sweep::manifest::parse_manifest_str;
use miner_core::sweep::{SweepOptions, run_sweep};
use miner_reader_dukascopy::DukascopyReader;

use common::{BufferSink, synthetic_cache::SyntheticCache};

fn build_synthetic_cache() -> SyntheticCache {
    let day = NaiveDate::from_ymd_opt(2024, 6, 12).expect("valid date");
    SyntheticCache::new()
        .with_deterministic_day("EURUSD", Side::Bid, day, 0x1234_5678)
        .with_deterministic_day("GBPUSD", Side::Bid, day, 0x9ABC_DEF0)
}

fn run_sweep_once(manifest_toml: &str) -> Vec<u8> {
    let cache = build_synthetic_cache();
    let cfg = MinerConfig {
        cache_root: cache.cache_root().to_path_buf(),
        bar_cache_root: cache.bar_cache_root().to_path_buf(),
        output: OutputDest::Stdout,
    };
    let reader = DukascopyReader::new(cache.cache_root());
    let bar_cache = BarCache::new(cache.bar_cache_root());
    let manifest = parse_manifest_str(manifest_toml).expect("manifest parses");

    let mut sink = BufferSink::new();
    run_sweep(
        manifest,
        SweepOptions::default(),
        &cfg,
        &reader,
        &bar_cache,
        &mut sink,
        Arc::new(AtomicBool::new(false)),
    )
    .expect("sweep ok");
    sink.0
}

/// Variant 1 (HYG-05 baseline): no hygiene block. Byte-identical
/// reruns of the same manifest with the same `[sweep].seed` must
/// produce byte-identical masked JSONL.
#[test]
fn sweep_byte_identical_rerun_no_hygiene() {
    let manifest = r#"
        [sweep]
        seed = 57005

        [[jobs]]
        scan = "stats.autocorr.ljung_box@1"
        instruments = ["EURUSD:bid", "GBPUSD:bid"]
        timeframes = ["15m"]
        windows = ["2024-06-12:2024-06-13"]
        params = { lags = 5 }

        [[jobs]]
        scan = "stats.autocorr.ljung_box_sq@1"
        instruments = ["EURUSD:bid", "GBPUSD:bid"]
        timeframes = ["15m"]
        windows = ["2024-06-12:2024-06-13"]
        params = { lags = 5 }
    "#;

    let bytes_a = run_sweep_once(manifest);
    let bytes_b = run_sweep_once(manifest);

    let masked_a = common::parse_and_mask_jsonl(&bytes_a);
    let masked_b = common::parse_and_mask_jsonl(&bytes_b);

    assert_eq!(
        masked_a.len(),
        masked_b.len(),
        "envelope counts must match across reruns (got {} vs {})",
        masked_a.len(),
        masked_b.len(),
    );
    assert_eq!(
        masked_a, masked_b,
        "HYG-05: masked envelopes from two sweep runs (same manifest, same seed) must be byte-identical.\n\
         Run A: {}\nRun B: {}",
        serde_json::to_string_pretty(&masked_a).unwrap_or_default(),
        serde_json::to_string_pretty(&masked_b).unwrap_or_default(),
    );
}

/// Variant 2 (HYG-05 with hygiene ON): both bootstrap and null
/// methods active. Byte-identical reruns must still hold — the
/// per-job derive_job_seed (HYG-05) gives the bootstrap + null
/// kernels the SAME PRNG sequence each rerun, so the resampled
/// ci95 + p_value populated by the Plan 05-03 continuation
/// dispatch table must be bit-identical too.
#[test]
fn sweep_byte_identical_rerun_with_hygiene_on() {
    let manifest = r#"
        [sweep]
        seed = 57005

        [hygiene]
        bootstrap = "stationary"
        bootstrap_n = 50
        null = "circular_shift"
        null_n = 50

        [[jobs]]
        scan = "stats.autocorr.ljung_box@1"
        instruments = ["EURUSD:bid", "GBPUSD:bid"]
        timeframes = ["15m"]
        windows = ["2024-06-12:2024-06-13"]
        params = { lags = 5 }
    "#;

    let bytes_a = run_sweep_once(manifest);
    let bytes_b = run_sweep_once(manifest);

    let masked_a = common::parse_and_mask_jsonl(&bytes_a);
    let masked_b = common::parse_and_mask_jsonl(&bytes_b);

    assert_eq!(
        masked_a.len(),
        masked_b.len(),
        "envelope counts must match across reruns under hygiene (got {} vs {})",
        masked_a.len(),
        masked_b.len(),
    );
    assert_eq!(
        masked_a, masked_b,
        "HYG-05 + hygiene-on: masked envelopes from two sweep runs (same manifest, same seed, hygiene active) must be byte-identical.\n\
         Run A: {}\nRun B: {}",
        serde_json::to_string_pretty(&masked_a).unwrap_or_default(),
        serde_json::to_string_pretty(&masked_b).unwrap_or_default(),
    );
}
