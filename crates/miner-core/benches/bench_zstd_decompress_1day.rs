// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: 2026 The tradedesk-miner authors

//! Plan 07-06 Task 2 — criterion microbench for the zstd decompression hot
//! kernel against a one-day Dukascopy fixture file.
//!
//! Kernel under test: `zstd::stream::read::Decoder` decompressing the bytes
//! of `tests/fixtures/cache/EURUSD/2024/00/01_bid.csv.zst` (Plan 07-02
//! synthetic fixture; 2024-01-01 EURUSD bid, 1440 1-minute CSV rows).
//!
//! The fixture bytes are read ONCE outside the timed loop; each iteration
//! wraps a fresh `Cursor` over the in-memory buffer so the decoder pipeline
//! starts from scratch without re-hitting the filesystem.
//!
//! Reports to `target/criterion/zstd_decompress_1day_eurusd/index.html` per
//! criterion 0.8's `html_reports` feature. CI does not run criterion benches
//! (D7-03 — variance on shared runners is too high for a useful gate); local
//! `cargo bench -p miner-core --bench bench_zstd_decompress_1day` runs it.

use std::io::{Cursor, Read};
use std::path::PathBuf;

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};

/// Resolve the fixture path relative to this crate's manifest. `cargo bench`
/// sets `CARGO_MANIFEST_DIR` to `crates/miner-core/`, so we walk two levels
/// up to the workspace root and into `tests/fixtures/cache/...`. The
/// 00-indexed month is the Dukascopy on-disk convention (CACHE-05 / January
/// = `00`).
fn fixture_path() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest_dir)
        .join("..")
        .join("..")
        .join("tests")
        .join("fixtures")
        .join("cache")
        .join("EURUSD")
        .join("2024")
        .join("00")
        .join("01_bid.csv.zst")
}

fn bench_zstd_decompress(c: &mut Criterion) {
    let path = fixture_path();
    let compressed = std::fs::read(&path).unwrap_or_else(|e| {
        panic!(
            "fixture missing at {} — run `bash scripts/generate-fixture-cache.sh` (err: {e})",
            path.display()
        )
    });
    let compressed_len = compressed.len();

    c.bench_function("zstd_decompress_1day_eurusd", |b| {
        b.iter(|| {
            // Wrap the in-memory buffer in a fresh Cursor each iteration so
            // the decoder starts at the file head; std::io::copy drives the
            // decode to completion into a discardable Vec<u8>.
            let cursor = Cursor::new(black_box(&compressed[..]));
            let mut decoder = zstd::stream::read::Decoder::new(cursor)
                .expect("zstd decoder construction succeeds on valid fixture");
            let mut sink = Vec::with_capacity(compressed_len * 4);
            decoder
                .read_to_end(&mut sink)
                .expect("zstd decode of fixture completes");
            black_box(sink);
        });
    });
}

criterion_group!(benches, bench_zstd_decompress);
criterion_main!(benches);
