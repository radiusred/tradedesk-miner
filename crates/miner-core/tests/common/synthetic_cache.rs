//! Synthetic Dukascopy cache builder for integration tests (Plan 03-06).
//!
//! Mirrors `crates/miner-core/tests/full_determinism.rs::write_synthetic_day`
//! but exposes a typed builder
//! (`SyntheticCache::new` + `with_close_seeded_day` + `with_deterministic_day`
//! + `with_day_holed`) so individual tests don't re-derive the synthetic
//! plumbing. Backed by a `TempDir` so paths clean up automatically.

#![allow(
    dead_code, // each integration test consumes a different subset of helpers.
    clippy::cast_precision_loss, // i64/usize -> f64 for synthetic volume; bounded.
    clippy::doc_lazy_continuation, // module-doc paragraph continuation OK here.
)]

use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, NaiveDate, Utc};
use miner_core::Side;
use miner_reader_dukascopy::day_csv_zst;
use tempfile::TempDir;

/// Synthetic on-disk Dukascopy cache rooted in a per-test `TempDir`.
///
/// Use the builder methods to compose the synthetic days; pass `cache_root()`
/// to `DukascopyReader::new` and `bar_cache_root()` to `BarCache::new`.
pub struct SyntheticCache {
    tmp: TempDir,
    cache_root: PathBuf,
    bar_cache_root: PathBuf,
}

impl SyntheticCache {
    /// Create a new empty synthetic cache. Both source and bar-cache roots
    /// live inside a per-test tempdir.
    #[must_use]
    pub fn new() -> Self {
        let tmp = TempDir::new().expect("tempdir");
        let cache_root = tmp.path().join("source");
        let bar_cache_root = tmp.path().join("bar-cache");
        std::fs::create_dir_all(&cache_root).expect("mkdir source");
        std::fs::create_dir_all(&bar_cache_root).expect("mkdir bar-cache");
        Self {
            tmp,
            cache_root,
            bar_cache_root,
        }
    }

    /// Path to the synthetic Dukascopy source cache root.
    #[must_use]
    pub fn cache_root(&self) -> &Path {
        &self.cache_root
    }

    /// Path to the derived-bar cache root (Arrow IPC files land here).
    #[must_use]
    pub fn bar_cache_root(&self) -> &Path {
        &self.bar_cache_root
    }

    /// Underlying tempdir handle (held so the path stays alive for the
    /// lifetime of the cache).
    #[allow(dead_code)]
    pub fn tempdir(&self) -> &TempDir {
        &self.tmp
    }

    /// Write a full UTC day of 1-minute synthetic bars at the canonical
    /// Dukascopy path layout. `close_seq` MUST have length 1440; OHLC are
    /// derived from `close` with a tiny envelope so the aggregator's
    /// monotonicity invariants hold trivially.
    pub fn with_close_seeded_day(
        self,
        symbol: &str,
        side: Side,
        date: NaiveDate,
        close_seq: &[f64],
    ) -> Self {
        assert_eq!(
            close_seq.len(),
            1440,
            "with_close_seeded_day expects 1440 1-minute closes (got {})",
            close_seq.len()
        );
        let day_start: DateTime<Utc> = date.and_hms_opt(0, 0, 0).expect("00:00:00").and_utc();
        let mut csv = String::with_capacity(1440 * 64 + 32);
        csv.push_str("timestamp,open,high,low,close,volume\n");
        for (i, &c) in close_seq.iter().enumerate() {
            #[allow(clippy::cast_possible_wrap)]
            let ts = day_start + Duration::minutes(i as i64);
            let open = c;
            let high = c + 0.00005;
            let low = c - 0.00005;
            csv.push_str(&format!(
                "{},{},{},{},{},{}\n",
                ts.format("%Y-%m-%d %H:%M:%S%:z"),
                open,
                high,
                low,
                c,
                (i + 1) as f64,
            ));
        }
        write_csv_zst_day(&self.cache_root, symbol, date, side, &csv);
        self
    }

    /// Same shape as [`Self::with_close_seeded_day`] but with a deterministic
    /// price walk (no caller-supplied closes). Useful for tests that don't
    /// care about the bar values, only the cache layout.
    pub fn with_deterministic_day(
        self,
        symbol: &str,
        side: Side,
        date: NaiveDate,
        seed: u32,
    ) -> Self {
        let mut closes = Vec::with_capacity(1440);
        let mut s = seed;
        for _ in 0..1440 {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let frac = f64::from(s) / f64::from(u32::MAX);
            closes.push(1.0 + frac * 0.01);
        }
        self.with_close_seeded_day(symbol, side, date, &closes)
    }

    /// Write a day with `hole_minutes` (relative offsets from midnight)
    /// OMITTED so `GapDetector` flags them as intra-day gaps. `seed` drives
    /// the same LCG as [`Self::with_deterministic_day`].
    pub fn with_day_holed(
        self,
        symbol: &str,
        side: Side,
        date: NaiveDate,
        seed: u32,
        hole_minutes: std::ops::Range<i64>,
    ) -> Self {
        let day_start: DateTime<Utc> = date.and_hms_opt(0, 0, 0).expect("00:00:00").and_utc();
        let mut s = seed;
        let mut csv = String::with_capacity(1440 * 64 + 32);
        csv.push_str("timestamp,open,high,low,close,volume\n");
        for i in 0..1440_i64 {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            if hole_minutes.contains(&i) {
                continue;
            }
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

/// Write one synthetic Dukascopy day file (zstd-encoded CSV bytes at the
/// canonical `<root>/<SYMBOL>/<YYYY>/<MM 00-indexed>/<DD>_<side>.csv.zst`
/// path). Mirrors `full_determinism.rs::write_synthetic_day`.
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
