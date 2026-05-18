//! Phase 3 integration-test fixtures — synthetic cache + AR(1) bar frame helpers.
//!
//! Used by the miner-cli integration tests (`scan_subcommand_smoke.rs`,
//! `scans_catalogue.rs`, `sigint_preserves_stream.rs`). Plan 03-06 fills these
//! out so each consumer test gets a typed `SyntheticCache` and a deterministic
//! bar-frame builder without reimplementing the plumbing per file.
//!
//! Pattern analog: `miner-core/tests/full_determinism.rs::write_synthetic_day`
//! — uses the sibling crate's PUBLIC `day_csv_zst` API; no internal poking.

#![allow(dead_code, unused_imports, clippy::cast_precision_loss)]

pub mod ar1_seed;
pub mod statsmodels_golden;

use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, NaiveDate, Utc};
use miner_core::Side;
use miner_reader_dukascopy::day_csv_zst;
use tempfile::TempDir;

/// Per-test synthetic Dukascopy cache + bar-cache directory pair.
///
/// Tests construct via `SyntheticCache::new().with_deterministic_day(...)`
/// and pass `cache_root()` / `bar_cache_root()` to the spawned `miner`
/// subprocess via `MINER_CACHE_ROOT` / `MINER_BAR_CACHE_ROOT` env vars.
pub struct SyntheticCache {
    tempdir: TempDir,
    cache_root: PathBuf,
    bar_cache_root: PathBuf,
}

impl SyntheticCache {
    #[must_use]
    pub fn new() -> Self {
        let tempdir = TempDir::new().expect("tempdir");
        let cache_root = tempdir.path().join("source");
        let bar_cache_root = tempdir.path().join("bar-cache");
        std::fs::create_dir_all(&cache_root).expect("mkdir source");
        std::fs::create_dir_all(&bar_cache_root).expect("mkdir bar-cache");
        Self {
            tempdir,
            cache_root,
            bar_cache_root,
        }
    }

    #[must_use]
    pub fn cache_root(&self) -> &Path {
        &self.cache_root
    }

    #[must_use]
    pub fn bar_cache_root(&self) -> &Path {
        &self.bar_cache_root
    }

    /// Keep the `TempDir` alive for the lifetime of the cache reference.
    #[must_use]
    pub fn tempdir(&self) -> &TempDir {
        &self.tempdir
    }

    /// Write a full UTC day of 1-minute synthetic bars (1440 bars) at the
    /// canonical Dukascopy path layout. Deterministic LCG-seeded prices so
    /// the aggregator's monotonicity invariants hold trivially.
    pub fn with_deterministic_day(
        self,
        symbol: &str,
        side: Side,
        date: NaiveDate,
        seed: u32,
    ) -> Self {
        let day_start: DateTime<Utc> = date.and_hms_opt(0, 0, 0).expect("00:00:00 valid").and_utc();
        let mut s = seed;
        let mut csv = String::with_capacity(1440 * 64 + 32);
        csv.push_str("timestamp,open,high,low,close,volume\n");
        for i in 0..1440_i64 {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let frac = f64::from(s) / f64::from(u32::MAX);
            let close = 1.0 + frac * 0.01;
            let ts = day_start + Duration::minutes(i);
            csv.push_str(&format!(
                "{},{},{},{},{},{}\n",
                ts.format("%Y-%m-%d %H:%M:%S%:z"),
                close - 0.0001,
                close + 0.0001,
                close - 0.0002,
                close,
                (i + 1) as f64,
            ));
        }
        write_csv_zst_day(&self.cache_root, symbol, date, side, &csv);
        self
    }
}

impl Default for SyntheticCache {
    fn default() -> Self {
        Self::new()
    }
}

fn write_csv_zst_day(root: &Path, symbol: &str, date: NaiveDate, side: Side, csv: &str) {
    let path = day_csv_zst(root, symbol, date, side);
    std::fs::create_dir_all(path.parent().expect("absolute path")).expect("mkdir -p");
    let file = std::fs::File::create(&path).expect("create day file");
    let mut encoder = zstd::stream::write::Encoder::new(file, 3).expect("zstd encoder");
    encoder
        .write_all(csv.as_bytes())
        .expect("write csv into zstd encoder");
    encoder.finish().expect("zstd finish");
}

/// Recursively mask the volatile envelope fields (`run_id`,
/// `started_at_utc`, `produced_at_utc`, `ended_at_utc`, `wall_clock_ms`).
///
/// Mirrors `crates/miner-cli/tests/cli_streams.rs::mask_volatile_fields`
/// (whose `mod` block is not reachable from sibling integration tests —
/// Cargo compiles each test file as a separate crate). Keep the masked-key
/// list in sync with `cli_streams::mask_volatile_fields`.
pub fn mask_volatile_fields(v: &mut serde_json::Value) {
    if let serde_json::Value::Object(map) = v {
        for key in [
            "run_id",
            "started_at_utc",
            "produced_at_utc",
            "ended_at_utc",
        ] {
            if map.contains_key(key) {
                map.insert(
                    key.to_string(),
                    serde_json::Value::String(format!("<masked_{key}>")),
                );
            }
        }
        if map.contains_key("wall_clock_ms") {
            map.insert("wall_clock_ms".to_string(), serde_json::Value::from(0i64));
        }
        for (_, child) in map.iter_mut() {
            mask_volatile_fields(child);
        }
    } else if let serde_json::Value::Array(arr) = v {
        for child in arr.iter_mut() {
            mask_volatile_fields(child);
        }
    }
}
