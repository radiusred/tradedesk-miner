// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: 2026 The tradedesk-miner authors

//! Plan 07-06 Task 2 — criterion microbench for the CSV parse hot kernel
//! against one decompressed day of the Dukascopy fixture cache.
//!
//! Kernel under test: `csv::ReaderBuilder::new().has_headers(true).from_reader`
//! plus serde-deserialise into `RawRow` (mirrors
//! `crates/miner-reader-dukascopy/src/reader.rs` production callsite). The
//! zstd decompression happens ONCE outside the timed loop so this bench
//! measures CSV parsing in isolation (decompression is benched separately
//! in `bench_zstd_decompress_1day`).
//!
//! Fixture: `tests/fixtures/cache/EURUSD/2024/00/01_bid.csv.zst` (Plan 07-02;
//! 1440 1-minute rows for 2024-01-01).
//!
//! Reports to `target/criterion/csv_parse_1day_eurusd/index.html`.

use std::io::Read;
use std::path::PathBuf;

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};

/// Mirrors `crates/miner-reader-dukascopy/src/reader.rs::RawRow`. We
/// duplicate the type locally so the bench does not introduce a coupling
/// from `miner-core` to `miner-reader-dukascopy`; the CSV schema (six
/// columns: `timestamp,open,high,low,close,volume`) is byte-pinned by
/// `crates/miner-bench/src/bin/gen-fixtures.rs:85`.
// All fields are populated by `csv::Reader::deserialize` but never inspected
// (the bench measures parse throughput, not field consumption); allow `dead_code`
// at the struct level so the per-field warnings collapse.
#[allow(dead_code)]
#[derive(serde::Deserialize)]
struct RawRow {
    timestamp: String,
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    volume: f64,
}

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

fn decompress_fixture_to_bytes() -> Vec<u8> {
    let path = fixture_path();
    let compressed = std::fs::read(&path).unwrap_or_else(|e| {
        panic!(
            "fixture missing at {} — run `bash scripts/generate-fixture-cache.sh` (err: {e})",
            path.display()
        )
    });
    let mut decoder = zstd::stream::read::Decoder::new(&compressed[..])
        .expect("zstd decoder construction succeeds on valid fixture");
    let mut out = Vec::with_capacity(compressed.len() * 4);
    decoder
        .read_to_end(&mut out)
        .expect("zstd decode of fixture completes");
    out
}

fn bench_csv_parse(c: &mut Criterion) {
    // Decompression happens ONCE outside the timed loop — this bench
    // measures CSV parsing in isolation. Decompression has its own bench
    // (`bench_zstd_decompress_1day`).
    let csv_bytes = decompress_fixture_to_bytes();

    c.bench_function("csv_parse_1day_eurusd", |b| {
        b.iter(|| {
            let mut reader = csv::ReaderBuilder::new()
                .has_headers(true)
                .from_reader(black_box(&csv_bytes[..]));
            let mut rows: Vec<RawRow> = Vec::new();
            for row in reader.deserialize::<RawRow>() {
                rows.push(row.expect("fixture rows are well-formed"));
            }
            // Defeat the optimiser: keep `rows` alive past the timed loop
            // body so the iteration cost is not eliminated.
            black_box(rows);
        });
    });
}

criterion_group!(benches, bench_csv_parse);
criterion_main!(benches);
