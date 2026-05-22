// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Radius Red Ltd.

//! Generate the synthetic Dukascopy-shape fixture cache at
//! `tests/fixtures/cache/` from a deterministic seed (Plan 07-02).
//!
//! Output is byte-identical across machines: per-day closes come from the
//! canonical Numerical Recipes LCG (constants 1_664_525 + 1_013_904_223 per
//! PATTERNS Pattern C / `crates/miner-core/tests/byte_identical_rerun.rs:74-83`),
//! and zstd compression uses single-threaded level 3, matching the
//! `tradedesk-dukascopy` producer (`export.py:442`) per RESEARCH Pitfall 4.
//!
//! NEVER call `.multithread(N)` on the zstd encoder — multi-threaded zstd is
//! non-deterministic and would break the SHA256SUMS gate.
//!
//! Layout mirrors `crates/miner-core/tests/common/synthetic_cache.rs` but
//! writes to the repo-tracked `tests/fixtures/cache/` path instead of a
//! per-test `TempDir`. Two symbols (EURUSD, GBPUSD), bid side only, January
//! 2024 weekdays only (weekends absent — Dukascopy cache shape).
//!
//! Path constructor: `miner_reader_dukascopy::day_csv_zst` is the ONLY
//! sanctioned way to build day-file paths (encapsulates the 00-indexed-month
//! quirk per CACHE-05 / T-02-04). Do NOT hand-roll the `<MM 00-indexed>` math.
//!
//! Stdout discipline (PATTERNS Pattern I): the binary emits exactly ONE JSON
//! summary line on stdout via `serde_json::to_writer(io::stdout().lock(), ...)`;
//! all tracing logs go to stderr.

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc, Weekday};
use miner_core::Side;
use miner_reader_dukascopy::day_csv_zst;
use serde::Serialize;
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

/// Canonical Numerical Recipes LCG (PATTERNS Pattern C). The constants
/// `1_664_525` and `1_013_904_223` are non-negotiable — they are the
/// cross-platform deterministic primitive every synthetic-OHLCV pipeline in
/// this repo uses. Do NOT replace with `rand::SmallRng` (RESEARCH
/// Anti-Patterns / SmallRng/StdRng explicitly non-portable).
#[allow(clippy::cast_possible_truncation)]
fn lcg_closes(n: usize, seed: u64) -> Vec<f64> {
    let mut s = seed as u32;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let frac = f64::from(s) / f64::from(u32::MAX);
        // Synthetic close in [1.0, 2.0] — matches PATTERNS Pattern C and
        // mirrors the OHLC range the in-memory `synthetic_cache.rs` uses.
        out.push(1.0 + frac);
    }
    out
}

/// Per-day seed derived from `blake3(format!("{symbol}-{date}"))` truncated
/// to a `u64`. This makes the LCG seed itself deterministic from the
/// `(symbol, date)` pair — re-runs across machines and OS versions cannot
/// drift the seed source.
fn per_day_seed(symbol: &str, date: NaiveDate) -> u64 {
    let key = format!("{symbol}-{date}");
    let hash = blake3::hash(key.as_bytes());
    let bytes = hash.as_bytes();
    u64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ])
}

/// Build a full UTC day of 1440 1-minute CSV rows for `date` using
/// `lcg_closes` seeded by `(symbol, date)`. Header + per-row format are
/// byte-identical to `crates/miner-core/tests/common/synthetic_cache.rs:88-105`.
#[allow(clippy::cast_possible_wrap)]
fn build_day_csv(symbol: &str, date: NaiveDate) -> String {
    let seed = per_day_seed(symbol, date);
    let closes = lcg_closes(1440, seed);
    let day_start: DateTime<Utc> = date
        .and_hms_opt(0, 0, 0)
        .expect("00:00:00 always valid")
        .and_utc();
    let mut csv = String::with_capacity(1440 * 64 + 32);
    csv.push_str("timestamp,open,high,low,close,volume\n");
    for (i, &c) in closes.iter().enumerate() {
        let ts = day_start + Duration::minutes(i as i64);
        let open = c;
        let high = c + 0.000_05;
        let low = c - 0.000_05;
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
    csv
}

/// Compress `csv_body` with single-threaded zstd level 3 (RESEARCH Pitfall 4)
/// and write to `path`. The parent directory is created on demand. NEVER
/// enable `.multithread(N)` — multi-threaded zstd is non-deterministic and
/// would silently desync the SHA256SUMS gate.
fn write_csv_zst(path: &Path, csv_body: &str) -> Result<u64> {
    let parent = path
        .parent()
        .with_context(|| format!("path has no parent: {}", path.display()))?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("create_dir_all({})", parent.display()))?;
    let file = File::create(path).with_context(|| format!("create({})", path.display()))?;
    // Single-threaded zstd level 3 — byte-identical with `tradedesk-dukascopy/export.py:442`.
    let mut encoder =
        zstd::stream::write::Encoder::new(file, 3).context("zstd encoder init")?;
    let mut src = csv_body.as_bytes();
    std::io::copy(&mut src, &mut encoder).context("zstd copy")?;
    encoder.finish().context("zstd finish")?;
    let metadata = std::fs::metadata(path)
        .with_context(|| format!("stat({})", path.display()))?;
    Ok(metadata.len())
}

/// Iterate every day in `[start, end]` (inclusive). Returns dates that fall
/// on a weekday (Mon..=Fri) — matches the Dukascopy on-disk shape where
/// weekend day-files do not exist (D7-01: weekend gaps are intentional).
fn weekday_range(start: NaiveDate, end: NaiveDate) -> Vec<NaiveDate> {
    let mut out = Vec::new();
    let mut d = start;
    while d <= end {
        if !matches!(d.weekday(), Weekday::Sat | Weekday::Sun) {
            out.push(d);
        }
        d = d.succ_opt().expect("next day always defined for in-range dates");
    }
    out
}

/// Compute the repo root from `CARGO_MANIFEST_DIR` (which points at
/// `crates/miner-bench/`). The two `..` components walk up to the workspace
/// root.
fn repo_root() -> Result<PathBuf> {
    let manifest_dir =
        std::env::var("CARGO_MANIFEST_DIR").context("CARGO_MANIFEST_DIR not set")?;
    let root = PathBuf::from(&manifest_dir)
        .join("..")
        .join("..")
        .canonicalize()
        .with_context(|| format!("canonicalize repo root from {manifest_dir}"))?;
    Ok(root)
}

/// Render a `sha256sum`-compatible line: `<64-hex>  <relative-path>\n`. Uses
/// `/`-separated relative paths regardless of host OS so the file is
/// byte-identical across platforms.
fn sha256_line(rel_path: &Path, digest_hex: &str) -> String {
    // Force forward-slash separators for cross-platform byte-identity.
    let rel = rel_path
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/");
    format!("{digest_hex}  {rel}\n")
}

/// Walk `fixture_root` in sorted order and emit a sha256sum-compatible
/// listing for every regular file under it. The sort key is the
/// path-relative-to-fixture-root, lexicographic, ASCII byte order (forced via
/// `WalkDir::sort_by_file_name`) so the output is byte-identical across hosts.
fn write_sha256sums(fixture_root: &Path) -> Result<usize> {
    let mut entries: Vec<(PathBuf, String)> = Vec::new();
    for entry in WalkDir::new(fixture_root)
        .sort_by_file_name()
        .into_iter()
        .filter_map(std::result::Result::ok)
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let file_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        // Skip the SHA256SUMS file itself + the .gitkeep marker.
        if file_name == "SHA256SUMS" || file_name == ".gitkeep" {
            continue;
        }
        let bytes =
            std::fs::read(path).with_context(|| format!("read({})", path.display()))?;
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let digest = hasher.finalize();
        let digest_hex = digest
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let rel = path
            .strip_prefix(fixture_root)
            .with_context(|| format!("strip_prefix({})", path.display()))?
            .to_path_buf();
        entries.push((rel, digest_hex));
    }
    // Re-sort by relative-path string for cross-platform stability — WalkDir's
    // `sort_by_file_name` sorts per-directory; this gives a single global order.
    entries.sort_by(|a, b| {
        let a_s = a
            .0
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        let b_s = b
            .0
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        a_s.cmp(&b_s)
    });
    let sha_path = fixture_root.join("SHA256SUMS");
    let mut out = String::new();
    for (rel, hex) in &entries {
        out.push_str(&sha256_line(rel, hex));
    }
    let mut f =
        File::create(&sha_path).with_context(|| format!("create({})", sha_path.display()))?;
    f.write_all(out.as_bytes())
        .with_context(|| format!("write({})", sha_path.display()))?;
    Ok(entries.len())
}

#[derive(Serialize)]
struct Summary {
    files_written: usize,
    total_bytes: u64,
    sha256sums_path: String,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();

    let repo = repo_root()?;
    let fixture_root = repo.join("tests").join("fixtures").join("cache");
    std::fs::create_dir_all(&fixture_root).with_context(|| {
        format!("create_dir_all({})", fixture_root.display())
    })?;

    // Two symbols, bid side only, January 2024 weekdays only.
    let start = NaiveDate::from_ymd_opt(2024, 1, 1).expect("2024-01-01 valid");
    let end = NaiveDate::from_ymd_opt(2024, 1, 31).expect("2024-01-31 valid");
    let weekdays = weekday_range(start, end);
    tracing::info!(
        weekday_count = weekdays.len(),
        "fixture-generator: trading days for 2024-01"
    );

    let symbols: &[&str] = &["EURUSD", "GBPUSD"];
    let mut files_written = 0usize;
    let mut total_bytes: u64 = 0;
    for symbol in symbols {
        for date in &weekdays {
            let csv = build_day_csv(symbol, *date);
            let path = day_csv_zst(&fixture_root, symbol, *date, Side::Bid);
            let bytes = write_csv_zst(&path, &csv)?;
            files_written += 1;
            total_bytes = total_bytes.saturating_add(bytes);
            tracing::debug!(
                symbol = symbol,
                date = %date,
                path = %path.display(),
                bytes = bytes,
                "wrote day-file"
            );
        }
    }

    let sha_count = write_sha256sums(&fixture_root)?;
    tracing::info!(
        files = files_written,
        sha_entries = sha_count,
        total_bytes = total_bytes,
        "fixture-generator complete"
    );

    let summary = Summary {
        files_written,
        total_bytes,
        sha256sums_path: "tests/fixtures/cache/SHA256SUMS".to_string(),
    };
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    serde_json::to_writer(&mut handle, &summary).context("write summary json")?;
    handle.write_all(b"\n").context("write summary newline")?;
    Ok(())
}
