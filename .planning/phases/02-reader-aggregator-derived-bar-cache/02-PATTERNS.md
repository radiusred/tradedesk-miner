# Phase 2: Reader, Aggregator & Derived-Bar Cache — Pattern Map

**Mapped:** 2026-05-17
**Files analyzed:** 23 (new + modified)
**Analogs found:** 23 / 23 — every Phase 2 file has at least a role-match analog inside `miner-core`.

> **Phase 1 frozen invariants Phase 2 must mirror** (from CLAUDE.md + Cargo.toml + lib.rs + clippy.toml):
>
> 1. **`unsafe_code = "forbid"`** at the workspace level (`Cargo.toml:54`). This `forbid` level cannot be downgraded by `#[allow]` and forces `BufReader<File>` over `memmap2::Mmap` for Phase 2 — confirmed by 02-RESEARCH §"Memory-mapped reads" line 777. Open question #2 resolves: **defer mmap to a later phase; ship `BufReader<File>` reads.**
> 2. **`disallowed-macros`** in `clippy.toml` bans `println!`, `print!`, `eprintln!`, `eprint!`, `dbg!`. The two sanctioned writers are `StdoutSink` (stdout) and `stderr_emit::write_preflight_error` (stderr). All Phase 2 progress/log output goes through `tracing::{info,debug,warn,error}!`.
> 3. **No `tokio` / no `async-trait` / no async runtime** anywhere in `miner-core` (FOUND-04, D-20). `Reader` is sync; iterator-of-`RawBar`, not `Stream`.
> 4. **`BTreeMap` only** on any `derive(Serialize)` type. Phase 1 enforces this for the envelope; 02-RESEARCH §Pitfall 2 carries the same rule for the sidecar JSON and Arrow IPC schema metadata.
> 5. **`thiserror::Error` enum, no `Serialize` derive** for internal error types (because `#[from] std::io::Error` does not compose with `serde::Serialize` — `error/mod.rs:24-40`). Wire-form for embedding in findings is `WireError` (already defined).
> 6. **`#[serde(rename_all = "snake_case")]`** on every public-API enum (locked Phase 1 idiom, see `Side`, `Timeframe`, `GapReason` shapes in 02-RESEARCH).
> 7. **Snapshot tests via `insta`** and **property tests via `proptest`** are Phase 2's Wave 0 introduction — no Phase 1 analog exists. Pattern source is upstream `insta` / `proptest` docs (cited in RESEARCH); we add a `tests/snapshots/` directory by convention.

---

## File Classification

### NEW CRATE — `miner-reader-dukascopy/` (the placeholder lib.rs is replaced)

| File | Role | Data Flow | Closest Analog | Match Quality |
|------|------|-----------|----------------|---------------|
| `crates/miner-reader-dukascopy/Cargo.toml` | config (crate manifest) | — | `crates/miner-core/Cargo.toml` | exact (manifest shape + workspace inheritance) |
| `crates/miner-reader-dukascopy/src/lib.rs` | new-module (crate root) | streaming (Reader trait impl) | `crates/miner-core/src/lib.rs` (module declaration pattern); `crates/miner-core/src/findings/sink.rs` (trait-impl idiom) | role-match |
| `crates/miner-reader-dukascopy/src/path_layout.rs` | utility (path-construction newtype) | transform (date → path) | `crates/miner-core/src/findings/run_id.rs` (newtype pattern with constructors + invariant assertion) | role-match (newtype enforces invariant) |
| `crates/miner-reader-dukascopy/src/reader.rs` | new-trait-impl (`DukascopyReader: Reader`) | streaming (iterator-of-`RawBar`) | `crates/miner-core/src/findings/sink.rs` (`StdoutSink: FindingSink` trait impl with `BufWriter<...>` and per-line discipline) | role-match |
| `crates/miner-reader-dukascopy/src/error.rs` | new-module (thiserror enum) | — | `crates/miner-core/src/error/mod.rs` (`MinerError` enum) | exact (same thiserror idiom) |
| `crates/miner-reader-dukascopy/tests/fixtures/mod.rs` | test-fixture (synthetic-cache helper) | file-I/O (test write) | `crates/miner-core/tests/schema_roundtrip.rs:39-47` (loads a fixture relative to `CARGO_MANIFEST_DIR`) + `crates/miner-cli/tests/cli_streams.rs:200-265` (`tempfile::TempDir` + `std::fs::write` pattern) | role-match |
| `crates/miner-reader-dukascopy/tests/reader_smoke.rs` | test (integration) | streaming | `crates/miner-core/tests/schema_roundtrip.rs` (integration test against public re-export surface) | exact |

### EXTENDED CRATE — `miner-core/src/` adds 5 new top-level modules

| File | Role | Data Flow | Closest Analog | Match Quality |
|------|------|-----------|----------------|---------------|
| `crates/miner-core/src/reader.rs` (or `reader/mod.rs`) | new-module (Reader trait + `RawBar` + `Side` + `ClosedRangeUtc` + `Blake3Hex`) | streaming | `crates/miner-core/src/findings/sink.rs` (`FindingSink` trait — object-safe, `Send`, associated `Error`) | exact (trait + data-type module) |
| `crates/miner-core/src/aggregate.rs` (or `aggregator/mod.rs`) | new-module (pure function + `Timeframe` + `AggParams` + `BarFrame`) | transform (1m bars → Nm bars) | `crates/miner-core/src/findings/mod.rs` (data-type module with multiple public structs + enum + module-level docs) | role-match |
| `crates/miner-core/src/cache.rs` (or `cache/mod.rs` + `cache/fingerprints.rs`) | new-module (`BarCache`, sidecar I/O, atomic write) | file-I/O (read-mostly + atomic write) | `crates/miner-core/src/findings/sink.rs` `FileSink::create` (atomic open + `BufWriter` + per-call flush) | role-match |
| `crates/miner-core/src/gap.rs` (or `gap/mod.rs`) | new-module (`GapDetector`, `GapManifest`, `GapSpan`, `GapReason`) | request-response (range → manifest) | `crates/miner-core/src/findings/mod.rs` (tagged-enum + `JsonSchema`-deriving public types) | exact (same `#[serde(tag = "kind")]` enum idiom) |
| `crates/miner-core/src/calendar.rs` | new-module (`Calendar` + `fx_major()` + `is_open_at`) | request-response (predicate) | `crates/miner-core/src/findings/run_id.rs` (small data-type module with single public struct + builder + module-level docs) | role-match |
| `crates/miner-core/src/fingerprint.rs` (optional helper, may live under `cache/`) | utility (blake3 helper) | transform (bytes → hex) | `crates/miner-core/src/findings/run_id.rs` (newtype wrapper over an underlying type) | partial-match |
| `crates/miner-core/src/lib.rs` (MODIFIED) | new-module wiring + frozen-surface extension | — | `crates/miner-core/src/lib.rs` (itself — extend `pub mod` declarations + `pub use` re-export block) | exact (modify the very file) |
| `crates/miner-core/Cargo.toml` (MODIFIED) | config | — | `crates/miner-core/Cargo.toml` (itself — add new workspace deps inheriting from workspace `[workspace.dependencies]`) | exact |

### EXTENDED CRATE — `miner-core/tests/` adds 8 integration tests + snapshots/

| File | Role | Data Flow | Closest Analog | Match Quality |
|------|------|-----------|----------------|---------------|
| `crates/miner-core/tests/aggregator_fixtures.rs` | test (fixture-driven unit) | transform | `crates/miner-core/tests/schema_roundtrip.rs` (uses public re-exports; sample constructors at top of file) | exact |
| `crates/miner-core/tests/aggregator_determinism.rs` | test (byte-identity gate) | transform | `crates/miner-cli/tests/cli_streams.rs` Test 6 lines 320-450 (twice-run byte-identity with masking) | role-match |
| `crates/miner-core/tests/dst_spring_forward.rs` | test (fixture) | transform | `crates/miner-core/tests/schema_roundtrip.rs` | role-match |
| `crates/miner-core/tests/dst_fall_back.rs` | test (fixture) | transform | `crates/miner-core/tests/schema_roundtrip.rs` | role-match |
| `crates/miner-core/tests/cache_smoke.rs` | test (cache hit/miss/atomic) | file-I/O | `crates/miner-cli/tests/cli_streams.rs:200-265` (`tempfile::TempDir` + `std::fs::write`) | role-match |
| `crates/miner-core/tests/gap_manifest_snapshot.rs` | test (insta snapshot) | request-response | (no Phase 1 analog — Wave 0 introduction; pattern from `insta` upstream docs) | partial-match |
| `crates/miner-core/tests/arrow_schema_snapshot.rs` | test (insta snapshot) | request-response | (no Phase 1 analog — Wave 0 introduction) | partial-match |
| `crates/miner-core/tests/full_determinism.rs` | test (end-to-end byte-identity) | file-I/O | `crates/miner-cli/tests/cli_streams.rs` Test 6 (twice-run byte-identity) | role-match |
| `crates/miner-core/tests/snapshots/` (directory) | test-fixture (insta `.snap` files) | — | (no Phase 1 analog — Wave 0 introduction) | none |

### MODIFIED CRATE — Workspace `Cargo.toml`

| File | Role | Data Flow | Closest Analog | Match Quality |
|------|------|-----------|----------------|---------------|
| `Cargo.toml` (workspace root) | config | — | `Cargo.toml` itself (lines 35-58) — extend `[workspace.dependencies]` | exact (modify in place) |

### Optionally MODIFIED — Error-code vocabulary

| File | Role | Data Flow | Closest Analog | Match Quality |
|------|------|-----------|----------------|---------------|
| `crates/miner-core/src/error/codes.rs` (MODIFY) | extension (new `PreflightCode` variants) | — | `crates/miner-core/src/error/codes.rs` itself (add `CacheRootNotFound`, `BarCacheNotWritable`, `SourceFileCorrupt` per CONTEXT line 201) | exact (modify in place) |

---

## Pattern Assignments

### `crates/miner-reader-dukascopy/Cargo.toml` (config)

**Analog:** `crates/miner-core/Cargo.toml`

**Manifest header pattern** (lines 18-32):
```toml
[package]
name = "miner-reader-dukascopy"
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[lib]

[dependencies]
miner-core = { path = "../miner-core" }
# Phase 2 new workspace deps (must be added under `[workspace.dependencies]` first)
csv.workspace      = true
zstd.workspace     = true
walkdir.workspace  = true
chrono.workspace   = true
blake3.workspace   = true
thiserror.workspace = true
tracing.workspace  = true
serde.workspace    = true

[dev-dependencies]
tempfile = "3"

[lints]
workspace = true
```

**Why this analog wins:** `miner-core/Cargo.toml` already establishes the workspace-inheritance idiom (`.workspace = true`) and `[lints] workspace = true` invocation. Mirror line-for-line.

---

### `crates/miner-reader-dukascopy/src/lib.rs` (new-module, crate root)

**Analog:** `crates/miner-core/src/lib.rs`

**Module declaration + frozen-surface re-export pattern** (lib.rs lines 8-35):
```rust
//! tradedesk-miner Dukascopy reader.
//!
//! Phase 2 implementation of the [`miner_core::reader::Reader`] trait for the
//! `tradedesk-dukascopy` cache layout: `<root>/<SYMBOL>/<YYYY>/<MM 00-indexed>/<DD>_<bid|ask>.csv.zst`.

pub mod error;
pub mod path_layout;
pub mod reader;

// FROZEN public surface — extended in a backwards-compatible way only.
pub use error::DukascopyError;
pub use path_layout::{DukascopyMonth, day_csv_zst, parse_day_path};
pub use reader::DukascopyReader;
```

**Why this analog wins:** `miner-core/src/lib.rs` lines 19-35 carry the comment header `// FROZEN public surface —` and a `pub use` re-export block. Use the same shape.

---

### `crates/miner-reader-dukascopy/src/path_layout.rs` (utility — newtype + invariant)

**Analog:** `crates/miner-core/src/findings/run_id.rs`

**Newtype-with-invariant pattern** (run_id.rs lines 22-46):
```rust
/// Dukascopy-style zero-indexed month (Jan = 0, Dec = 11). Distinct from `chrono::Month`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DukascopyMonth(u8);  // private inner; range 0..=11

impl DukascopyMonth {
    /// Convert from calendar 1..=12 (panics outside that range — caller bug).
    pub fn from_calendar(month: u8) -> Self {
        assert!((1..=12).contains(&month), "calendar month out of range: {month}");
        Self(month - 1)
    }

    pub fn from_chrono_date(d: chrono::NaiveDate) -> Self {
        Self::from_calendar(d.month() as u8)
    }

    pub fn dir_name(self) -> String {
        format!("{:02}", self.0)
    }
}
```

**Why this analog wins:** `RunId(Ulid)` is the canonical "newtype wraps primitive, public constructors enforce invariant" pattern in this codebase. `DukascopyMonth` follows the same shape exactly — private field, public smart constructors, no `Deref` to inner.

**Tests** are the 5 boundary tests already specified in 02-RESEARCH lines 336-397 (jan_maps_to_00, dec_maps_to_11, two `#[should_panic]` for 0 and 13, full-path round-trip, plus a `proptest!` round-trip).

---

### `crates/miner-reader-dukascopy/src/reader.rs` (new-trait-impl)

**Analog:** `crates/miner-core/src/findings/sink.rs` (the `StdoutSink: FindingSink` and `FileSink: FindingSink` impls)

**Trait-impl + `BufWriter` discipline pattern** (sink.rs lines 71-104, 134-176):
```rust
pub struct DukascopyReader {
    cache_root: std::path::PathBuf,
    // Cache the calendar instance so trading_calendar() doesn't allocate per call.
    calendar: miner_core::calendar::Calendar,
}

impl DukascopyReader {
    /// Construct from a cache root. Does NOT validate that the path exists — preflight does that.
    #[must_use]
    pub fn new(cache_root: impl Into<std::path::PathBuf>) -> Self {
        Self {
            cache_root: cache_root.into(),
            calendar: miner_core::calendar::Calendar::fx_major(),
        }
    }
}

impl miner_core::reader::Reader for DukascopyReader {
    type Error = crate::error::DukascopyError;
    fn source_id(&self) -> &str { "dukascopy" }
    fn trading_calendar(&self) -> miner_core::calendar::Calendar { self.calendar.clone() }
    fn read_1m_bars<'a>(&'a self, symbol: &str, side: miner_core::reader::Side, range: miner_core::reader::ClosedRangeUtc)
        -> Result<Box<dyn Iterator<Item = Result<miner_core::reader::RawBar, Self::Error>> + Send + 'a>, Self::Error>
    {
        // open .csv.zst via BufReader<File> + zstd::stream::read::Decoder + csv::ReaderBuilder
        // (NO mmap — `unsafe_code = "forbid"` workspace-wide; see RESEARCH §"Memory-mapped reads")
        todo!()
    }
    fn fingerprint_day(&self, ...) -> Result<Option<miner_core::reader::Blake3Hex>, Self::Error> { todo!() }
    fn enumerate_days(&self, ...) -> Result<Vec<chrono::NaiveDate>, Self::Error> { todo!() }
}
```

**File-reading inner-loop pattern** (RESEARCH §"Reading a Dukascopy day file end-to-end" lines 1077-1130 plus sink.rs `BufWriter<File>` idiom at line 134):
```rust
let file = std::fs::File::open(&path).map_err(DukascopyError::Io)?;
let reader = std::io::BufReader::with_capacity(1024 * 1024, file);
let mut decoder = zstd::stream::read::Decoder::new(reader).map_err(DukascopyError::Zstd)?;
let mut csv_rdr = csv::ReaderBuilder::new()
    .has_headers(true)
    .from_reader(&mut decoder);
// stream rows; convert each to RawBar, parse timestamp with %:z format spec
```

**Why this analog wins:** `StdoutSink`/`FileSink` are the only existing examples of "construct a stateful object that performs streaming I/O behind a trait." The `BufWriter::with_capacity` + `BufWriter`-then-`Write` discipline at sink.rs:71-104 maps directly onto `BufReader::with_capacity` + `BufReader`-then-`Read` for the reader.

**Forbid `Mmap` here:** Phase 2 must keep `unsafe_code = "forbid"`. See `crates/miner-core/tests/config_precedence.rs` lines 13-20 (the workspace lint precedent for refusing to downgrade `forbid`).

---

### `crates/miner-reader-dukascopy/src/error.rs` (thiserror enum)

**Analog:** `crates/miner-core/src/error/mod.rs`

**Error-enum pattern** (error/mod.rs lines 23-40):
```rust
//! Error model for the Dukascopy reader.
//!
//! Mirrors `miner_core::error::MinerError` (which sets the workspace idiom):
//! - thiserror-derived
//! - NOT `Serialize` (the `Io(#[from] std::io::Error)` variant is incompatible with serde-derive,
//!   per RESEARCH §Pitfall 3 in 01-RESEARCH)
//! - convertible to `miner_core::WireError` via a `From` impl for the engine boundary

#[derive(Debug, thiserror::Error)]
pub enum DukascopyError {
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("zstd decode error: {0}")]
    Zstd(#[from] std::io::Error),   // zstd surfaces errors as io::Error

    #[error("csv parse error: {0}")]
    Csv(#[from] csv::Error),

    #[error("blake3 hex decode error: {0}")]
    HexDecode(String),

    #[error("source file is zero-byte or corrupt: {path}")]
    CorruptSourceFile { path: std::path::PathBuf },

    #[error("path layout violation: {0}")]
    PathLayout(String),
}
```

**Why this analog wins:** This is the exact same `thiserror::Error` derive + `#[from] std::io::Error` + `#[error("...")]` message-form already in `miner-core/src/error/mod.rs`. Match struct-style variants for context-carrying errors and tuple-style for transparent wrappers.

**Critical:** DO NOT add `Serialize` derive on `DukascopyError`. The `#[from] std::io::Error` variant is incompatible (`error/mod.rs:22-24` documents this with reference to RESEARCH §Pitfall 3). For wire emission, convert to `miner_core::WireError::scan(ScanErrorCode::CacheCorruption, ...)` via a `From<DukascopyError> for WireError` impl following `error/mod.rs:42-54`.

---

### `crates/miner-reader-dukascopy/tests/fixtures/mod.rs` (test-fixture helper)

**Analog:** `crates/miner-cli/tests/cli_streams.rs:200-265` (`tempfile::TempDir` + `std::fs::write` inside a `serial_test::serial` integration test)

**Synthetic-cache builder pattern** (cli_streams.rs lines 200-265):
```rust
//! Synthetic Dukascopy-format cache fixture builder. Writes a fresh, tiny `.csv.zst`
//! hierarchy under a `tempfile::TempDir` for integration tests. Not checked-in data —
//! we generate it at test setup time so the fixtures version with the code.

use std::path::Path;

pub struct SyntheticCache {
    pub root: tempfile::TempDir,
}

impl SyntheticCache {
    pub fn new() -> Self {
        let root = tempfile::TempDir::new().expect("tempdir");
        Self { root }
    }

    /// Write one synthetic .csv.zst day file under <root>/<SYMBOL>/<YYYY>/<MM 00-indexed>/<DD>_<side>.csv.zst.
    pub fn write_day(&self, symbol: &str, date: chrono::NaiveDate, side: miner_core::reader::Side, csv_body: &str)
        -> std::path::PathBuf
    {
        // 1. Compose path via path_layout::day_csv_zst (the SEALED constructor)
        // 2. mkdir -p the parent
        // 3. zstd::stream::write::Encoder::new(file, 3).write_all(csv_body.as_bytes())
        // 4. Return absolute path for assertion
        todo!()
    }
}
```

**Why this analog wins:** `cli_streams.rs:200-265` is the only existing example of "create a `TempDir`, write files into it, run the SUT against it." Phase 2's fixture builder is the same pattern with zstd-encoding added.

---

### `crates/miner-reader-dukascopy/tests/reader_smoke.rs` (integration test)

**Analog:** `crates/miner-core/tests/schema_roundtrip.rs`

**Integration-test against public re-export surface pattern** (schema_roundtrip.rs lines 22-47):
```rust
//! Plan body: Phase 2 CACHE-01 reader smoke test.
//!
//! Uses ONLY `miner_reader_dukascopy::*` public re-exports + the synthetic fixture
//! helper. This proves the public crate surface is sufficient to exercise the
//! reader contract end-to-end — matching the Phase 1 precedent of testing through
//! the public re-export layer.

mod fixtures;  // pull in the synthetic-cache helper as a sub-module of this test

use miner_reader_dukascopy::DukascopyReader;
use miner_core::reader::{Reader, Side};

#[test]
fn reads_one_day_in_order() {
    let cache = fixtures::SyntheticCache::new();
    let date = chrono::NaiveDate::from_ymd_opt(2024, 6, 15).unwrap();
    cache.write_day("EURUSD", date, Side::Bid, /* synthetic 1m CSV body */);

    let reader = DukascopyReader::new(cache.root.path());
    let range = /* full-day ClosedRangeUtc */;
    let bars: Vec<_> = reader.read_1m_bars("EURUSD", Side::Bid, range)
        .expect("read_1m_bars ok")
        .collect::<Result<_, _>>()
        .expect("all bars parse");

    // ascending order check
    assert!(bars.windows(2).all(|w| w[0].ts_open_utc < w[1].ts_open_utc));
}
```

**Why this analog wins:** `schema_roundtrip.rs` is the canonical "integration test that uses the public re-export surface and a fixture under `CARGO_MANIFEST_DIR`" pattern. Phase 2's reader_smoke.rs is structurally identical.

---

### `crates/miner-core/src/reader.rs` (new module — the `Reader` trait)

**Analog:** `crates/miner-core/src/findings/sink.rs` (the `FindingSink` trait)

**Object-safe `Send`-bound trait pattern** (sink.rs lines 35-50):
```rust
//! Pluggable data-source abstraction. Phase 2 ships the trait here; concrete impls live in
//! sibling crates (e.g. `miner-reader-dukascopy`). The aggregator + gap detector + cache
//! all consume `&dyn Reader` so a future equity / crypto reader plugs in without changing
//! `miner-core`.

use chrono::{DateTime, NaiveDate, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::calendar::Calendar;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Side { Bid, Ask }

impl Side {
    pub fn as_str(self) -> &'static str { match self { Self::Bid => "bid", Self::Ask => "ask" } }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClosedRangeUtc { pub start: DateTime<Utc>, pub end: DateTime<Utc> }

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RawBar {
    pub ts_open_utc: DateTime<Utc>,
    pub ts_close_utc: DateTime<Utc>,
    pub open: f64, pub high: f64, pub low: f64, pub close: f64,
    pub tick_volume: f64,   // RESEARCH A1: renamed from `tick_count: u32`
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct Blake3Hex(pub [u8; 64]);   // ASCII hex chars — fixed-size, no alloc

pub trait Reader: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;
    fn source_id(&self) -> &str;
    fn trading_calendar(&self) -> Calendar;
    fn read_1m_bars<'a>(&'a self, symbol: &str, side: Side, range: ClosedRangeUtc)
        -> Result<Box<dyn Iterator<Item = Result<RawBar, Self::Error>> + Send + 'a>, Self::Error>;
    fn fingerprint_day(&self, symbol: &str, side: Side, date: NaiveDate)
        -> Result<Option<Blake3Hex>, Self::Error>;
    fn enumerate_days(&self, symbol: &str, side: Side, range: ClosedRangeUtc)
        -> Result<Vec<NaiveDate>, Self::Error>;
}
```

**Object-safety regression-gate test** (mirror sink.rs `trait_object_safe` lines 399-409):
```rust
#[test]
fn reader_trait_object_safe() {
    fn _accept_dyn(_: &dyn crate::reader::Reader<Error = std::io::Error>) {}
    // Compile-time check: any `Reader<Error = E>` can be coerced to `&dyn Reader<Error = E>`.
}
```

**Why this analog wins:** `FindingSink` is the *only* trait in `miner-core` right now, and it sets all the relevant idioms — `Send` bound, associated `Error` type, object-safety regression test. The `Reader` trait at 02-RESEARCH lines 437-481 is structurally identical (one extra `Sync` bound for cross-thread reads via rayon, otherwise the same shape).

---

### `crates/miner-core/src/aggregate.rs` (new module — pure aggregator)

**Analog:** `crates/miner-core/src/findings/mod.rs`

**Public-data-types-module pattern** (findings/mod.rs lines 39-103):
```rust
//! Aggregator — pure function (1m source bars, params) → BarFrame.
//!
//! No IO outside the supplied reader, no clock reads, no env reads. Determinism is the
//! contract (CACHE-04 byte-identity). See module doc comments for the 5 determinism
//! safeguards (RESEARCH §"Determinism contract" lines 526-535).

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::reader::{Reader, Side, ClosedRangeUtc};

/// Bump this on ANY code change that alters output bytes for the same input.
/// Stored in both the Arrow file metadata AND the sidecar JSON; mismatch triggers
/// full rebuild (CONTEXT D2-04).
pub const AGGREGATOR_VERSION: &str = "1.0.0";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Timeframe { Tf15m, Tf1h, Tf1d }

impl Timeframe {
    pub fn duration(self) -> chrono::Duration { /* … */ }
    pub fn as_str(self) -> &'static str { /* … */ }
}

#[derive(Debug, Clone)]
pub struct BarFrame {
    pub source_id: String,
    pub symbol: String,
    pub side: Side,
    pub tf: Timeframe,
    pub ts_open_utc: Vec<DateTime<Utc>>,
    pub ts_close_utc: Vec<DateTime<Utc>>,
    pub open: Vec<f64>, pub high: Vec<f64>, pub low: Vec<f64>, pub close: Vec<f64>,
    pub tick_volume: Vec<f64>,
}

pub struct AggParams<'a> {
    pub symbol: &'a str, pub side: Side, pub tf: Timeframe, pub range: ClosedRangeUtc,
}

pub fn aggregate<R: Reader>(reader: &R, params: AggParams)
    -> Result<BarFrame, AggregateError<R::Error>>
{ /* … */ }
```

**Why this analog wins:** `findings/mod.rs` is the established pattern for "public data-type module with multiple structs + tagged enum + a module-level doc block explaining invariants." Lines 39-44 show the imports order; lines 58-103 show `derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)` plus `#[serde(rename_all = "snake_case")]` on enums — copy verbatim.

**Determinism contract documentation** (mirror findings/mod.rs lines 24-32):
```rust
//! ## Determinism (CACHE-04 byte-identity, OUT-03)
//!
//! 1. NO `HashMap` anywhere in the aggregator or its inputs/outputs. `BTreeMap` only.
//! 2. NO `rayon::par_iter` inside the per-symbol reduction. Single-threaded per quartet.
//! 3. NO `Instant::now()` / `SystemTime::now()` / `Utc::now()` inside this module.
//! 4. f64 sums are sequential and ordered by `ts_open_utc`.
//! 5. Arrow IPC `Schema` constructed from a fixed `Vec<Field>`, metadata keys collected
//!    from a `BTreeMap` to guarantee insertion order.
```

---

### `crates/miner-core/src/cache.rs` + `cache/fingerprints.rs` (new module — cache I/O)

**Analog:** `crates/miner-core/src/findings/sink.rs` (`FileSink::create` for the atomic-open pattern + `BufWriter` discipline)

**Atomic-write pattern** (sink.rs lines 138-159 establishes the `OpenOptions` + `BufWriter` flow; 02-RESEARCH lines 742-755 extends it with `tempfile::NamedTempFile::persist`):
```rust
//! Derived-bar cache. Owns ONE writable directory tree under `bar_cache_root`.
//! Phase 2 ships safe `BufReader<File>` reads (NOT mmap — `unsafe_code = "forbid"`).
//! See RESEARCH §"Memory-mapped reads" lines 762-779 for the deferral rationale.

use std::io::BufWriter;
use std::path::Path;

pub const ARROW_SCHEMA_VERSION: &str = "1.0.0";  // bump on any field shape change

/// Atomic write of an Arrow IPC file.
/// Pattern: tempfile in same parent → sync_all → tempfile::NamedTempFile::persist (atomic rename).
/// If the process crashes mid-write, the tempfile is orphaned and the existing target is unchanged.
fn write_arrow_atomic(target: &Path, schema: &arrow::datatypes::Schema, batches: &[arrow::record_batch::RecordBatch])
    -> Result<(), CacheError>
{
    let parent = target.parent().expect("absolute path");
    std::fs::create_dir_all(parent).map_err(CacheError::Io)?;
    let tmp = tempfile::NamedTempFile::new_in(parent).map_err(CacheError::Io)?;
    {
        let mut writer = arrow::ipc::writer::FileWriter::try_new(tmp.as_file(), schema)
            .map_err(CacheError::Arrow)?;
        for batch in batches { writer.write(batch).map_err(CacheError::Arrow)?; }
        writer.finish().map_err(CacheError::Arrow)?;
    }
    tmp.as_file().sync_all().map_err(CacheError::Io)?;
    tmp.persist(target).map_err(|e| CacheError::Io(e.error))?;
    Ok(())
}
```

**Tracing on cache invalidation** (mirror cli/src/main.rs:48 — single-line `tracing::info!` with structured fields):
```rust
tracing::info!(
    symbol = %params.symbol, side = ?params.side, tf = %params.tf.as_str(),
    old_version = %sidecar_version, new_version = %AGGREGATOR_VERSION,
    "cache invalidated: aggregator_version bumped, rebuilding"
);
```

**Why this analog wins:** `FileSink::create` (sink.rs:144-159) already wraps `OpenOptions::new().create(true).append(true).open(path)` in `BufWriter` and emits `MinerError::Io`. The cache extends this pattern with `tempfile::NamedTempFile::persist` for the atomic-rename. Importantly, `FileSink` writes the file FIRST then flushes — the cache's "Arrow first, sidecar second" ordering (02-RESEARCH line 760) is the same crash-safety principle one layer up.

---

### `crates/miner-core/src/gap.rs` (new module — gap detector + manifest)

**Analog:** `crates/miner-core/src/findings/mod.rs` (the `#[serde(tag = "kind", rename_all = "snake_case")]` tagged-enum idiom)

**Tagged-enum pattern** (findings/mod.rs lines 288-301):
```rust
//! Gap detector + manifest. The manifest is the data Phase 3's gap-policy enforcer
//! wraps into a `GapAbortedFinding` envelope under `--gap-policy=strict`.

use chrono::{DateTime, NaiveDate, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::findings::TimeRange;   // REUSE Phase 1 type
use crate::reader::Side;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GapManifest {
    pub source_id: String,
    pub symbol: String,
    pub side: Side,
    pub queried_range: TimeRange,
    /// Sorted by `start_utc` ascending. Empty when no gaps.
    pub gaps: Vec<GapSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GapSpan {
    pub start_utc: DateTime<Utc>,
    pub end_utc: DateTime<Utc>,
    pub reason: GapReason,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GapReason {
    MissingSourceFile { date: NaiveDate },
    CorruptSourceFile { date: NaiveDate, detail: String },
    IntraDayGap { affected_minutes: u32 },
}
```

**Why this analog wins:** `Finding::RunStart(...) | Result(...) | ScanError(...) | GapAborted(...) | RunEnd(...)` in findings/mod.rs:293-301 is the workspace's canonical tagged-enum-with-snake-case-tag idiom — `GapReason` follows the same recipe. **Critical:** reuse Phase 1's `TimeRange` for `queried_range` (findings/mod.rs:58-62) — do NOT redefine.

---

### `crates/miner-core/src/calendar.rs` (new module — `Calendar`)

**Analog:** `crates/miner-core/src/findings/run_id.rs`

**Small-data-type-module with single public struct + builder + module doc** (run_id.rs lines 1-46):
```rust
//! Trading calendar. FX-major default; per-symbol overrides come via `Reader::trading_calendar()`.
//!
//! Closed-form predicate — `is_open_at` is O(1), no allocation, inlineable. Performance budget:
//! `< 100 ns/call` (RESEARCH §"Trading Calendar" Assumption A4). Verified by Phase 7 criterion bench.

use chrono::{DateTime, Datelike, NaiveTime, Timelike, Utc, Weekday};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Calendar {
    pub weekly_open_utc: (Weekday, NaiveTime),
    pub weekly_close_utc: (Weekday, NaiveTime),
    pub yearly_holidays: Vec<(u32, u32)>,
}

impl Calendar {
    /// The FX-major default — Friday 22:00 UTC → Sunday 22:00 UTC closed,
    /// plus Christmas Day (Dec 25) and New Year's Day (Jan 1) closed each year.
    #[must_use]
    pub fn fx_major() -> Self {
        Self {
            weekly_open_utc: (Weekday::Sun, NaiveTime::from_hms_opt(22, 0, 0).unwrap()),
            weekly_close_utc: (Weekday::Fri, NaiveTime::from_hms_opt(22, 0, 0).unwrap()),
            yearly_holidays: vec![(12, 25), (1, 1)],
        }
    }

    /// Closed-form predicate. O(1), no allocation, inlineable.
    #[inline]
    pub fn is_open_at(&self, ts: DateTime<Utc>) -> bool {
        // see RESEARCH §"Trading Calendar" lines 896-919 for the full predicate body
        for (m, d) in &self.yearly_holidays {
            if ts.month() == *m && ts.day() == *d { return false; }
        }
        let wd = ts.weekday(); let t = ts.time();
        match wd {
            Weekday::Mon | Weekday::Tue | Weekday::Wed | Weekday::Thu => true,
            Weekday::Fri => t < self.weekly_close_utc.1,
            Weekday::Sat => false,
            Weekday::Sun => t >= self.weekly_open_utc.1,
        }
    }
}
```

**Why this analog wins:** `run_id.rs` is the small-self-contained-module template — single public struct, `Default` impl, focused docstring at the top. `Calendar` is the same shape with `fx_major()` instead of `new()`.

**Note:** `Calendar` is NOT `Copy` (contains a `Vec`); it IS `Clone`. Reader's `trading_calendar(&self)` should return `Calendar` by value (`.clone()` internally) or by reference (`&Calendar`). 02-RESEARCH line 446 shows by-value; mirror that for ergonomics.

---

### `crates/miner-core/src/lib.rs` (MODIFY — extend frozen surface)

**Analog:** `crates/miner-core/src/lib.rs` itself

**Module-declaration + frozen-surface extension pattern** (lib.rs lines 8-35):
```rust
// EXISTING (Phase 1) — keep
pub mod config;
pub mod error;
pub mod findings;

// NEW (Phase 2) — add
pub mod aggregate;
pub mod cache;
pub mod calendar;
pub mod gap;
pub mod reader;

pub const CODE_REVISION: &str = env!("MINER_CODE_REVISION");   // unchanged

// =============================================================================
// FROZEN public surface — every downstream plan imports from here.
// =============================================================================

// EXISTING — keep
pub use findings::{ /* unchanged Phase 1 list */ };
pub use error::{ /* unchanged Phase 1 list */ };
pub use config::{ /* unchanged Phase 1 list */ };

// NEW (Phase 2) — extend
pub use aggregate::{AGGREGATOR_VERSION, AggParams, BarFrame, Timeframe, aggregate};
pub use cache::{ARROW_SCHEMA_VERSION, BarCache, CacheError};
pub use calendar::Calendar;
pub use gap::{GapDetector, GapManifest, GapReason, GapSpan};
pub use reader::{Blake3Hex, ClosedRangeUtc, RawBar, Reader, Side};
```

**Why this analog wins:** lib.rs lines 19-35 already establish the "Adding a name to this list is backwards-compatible; removing one is a contract break." comment + `pub use` block. Add Phase 2's names without touching Phase 1's lines.

---

### `crates/miner-core/Cargo.toml` (MODIFY — add Phase 2 deps)

**Analog:** `crates/miner-core/Cargo.toml` itself (lines 26-45)

**Workspace-inherited deps + dev-deps pattern** (existing lines 27-46):
```toml
[dependencies]
serde.workspace      = true
serde_json.workspace = true
schemars.workspace   = true
chrono.workspace     = true
thiserror.workspace  = true
tracing.workspace    = true
ulid.workspace       = true
blake3.workspace     = true
base64.workspace     = true
figment.workspace    = true
# Phase 2 additions:
arrow.workspace      = true   # CACHE-06 cache format
tempfile.workspace   = true   # atomic NamedTempFile::persist pattern

[dev-dependencies]
jsonschema.workspace = true
figment              = { version = "0.10", features = ["toml", "env", "test"] }
serial_test          = "3"
# Phase 2 additions (Wave 0):
proptest             = "1.11"   # aggregator/path-layout property tests
insta                = "1.47"   # gap manifest + Arrow schema snapshots
```

**Workspace `Cargo.toml` MODIFY** — add under `[workspace.dependencies]` (Cargo.toml:35-51):
```toml
# Phase 2 additions:
arrow      = "58"
csv        = "1.4"
zstd       = "0.13"
walkdir    = "2.5"
tempfile   = "3"
# NOTE: proptest + insta are dev-deps; workspace inheritance is optional. Pin them at the
# crate dev-deps level rather than the workspace level to keep the workspace dep list lean.
```

---

### `crates/miner-core/src/error/codes.rs` (OPTIONAL MODIFY — add `PreflightCode` variants)

**Analog:** `crates/miner-core/src/error/codes.rs` itself (lines 22-37)

**Adding `PreflightCode` variants pattern** (codes.rs:22-37):
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PreflightCode {
    // EXISTING (Phase 1) — keep
    InvalidParameter,
    UnknownScan,
    UnknownInstrument,
    MissingRequiredConfig,
    InvalidConfig,
    SweepTooLarge,
    InternalError,
    // NEW (Phase 2) — per CONTEXT line 201
    CacheRootNotFound,
    BarCacheNotWritable,
    SourceFileCorrupt,
}
```

…and extend the matching `as_str()` arm at codes.rs:42-52. The Phase 1 round-trip test at codes.rs:141-161 automatically covers the new variants (the test iterates all variants and asserts each round-trips snake_case).

**Why this analog wins:** Adding variants to a frozen-shape enum is documented as backwards-compatible in codes.rs:11 (`code` is `String` on the wire). Mirror.

---

### `crates/miner-core/tests/aggregator_fixtures.rs` etc. (integration tests)

**Analog:** `crates/miner-core/tests/schema_roundtrip.rs`

**Test-file header + fixture-builder pattern** (schema_roundtrip.rs lines 1-47):
```rust
//! Plan body: Phase 2 CACHE-03 / CACHE-04 aggregator integration tests.
//!
//! Uses ONLY public `miner_core::*` re-exports (FROZEN surface in lib.rs). Synthetic
//! 1m source data is constructed in-test via a `MockReader` that implements `Reader`
//! and returns a hand-built iterator — no filesystem, no zstd, no CSV parse.

use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use miner_core::{
    aggregate, AggParams, BarFrame, Calendar, ClosedRangeUtc, RawBar, Reader, Side, Timeframe,
};

struct MockReader { bars: Vec<RawBar>, calendar: Calendar }
impl Reader for MockReader { /* … sync iterator over self.bars … */ }

#[test]
fn three_timeframes_from_one_day_of_1m() {
    let mock = MockReader { bars: build_24h_1m_bars(), calendar: Calendar::fx_major() };
    for tf in [Timeframe::Tf15m, Timeframe::Tf1h, Timeframe::Tf1d] {
        let frame = aggregate(&mock, AggParams { /* … */ }).unwrap();
        assert_eq!(frame.ts_open_utc.len(), expected_len(tf));
    }
}
```

**Snapshot-test pattern** (Wave 0 introduction, no Phase 1 analog — copy from `insta` upstream docs):
```rust
// crates/miner-core/tests/gap_manifest_snapshot.rs
#[test]
fn gap_manifest_json_shape_pinned() {
    let manifest = /* build a deterministic GapManifest from a fixed input */;
    insta::assert_json_snapshot!(manifest);
}
```

After first run, `cargo insta accept` creates `crates/miner-core/tests/snapshots/gap_manifest_snapshot__gap_manifest_json_shape_pinned.snap`. Commit the `.snap` file. Any future drift fails CI.

**Why this analog wins:** `schema_roundtrip.rs` already shows the "integration test that uses the public re-export surface + a sample builder" idiom. The aggregator tests follow it with `MockReader` standing in for the production `DukascopyReader`.

---

### `crates/miner-core/tests/full_determinism.rs` (byte-identity two-runs)

**Analog:** `crates/miner-cli/tests/cli_streams.rs` Test 6 lines 320-450 (twice-run byte-identity with masking)

**Twice-run byte-identity pattern** (cli_streams.rs:320-450):
```rust
//! Full-pipeline determinism gate: aggregate + write Arrow + write sidecar TWICE
//! from the same synthetic source, then byte-compare the resulting files.

#[test]
fn two_runs_byte_identical() {
    let tmp = tempfile::TempDir::new().unwrap();
    let source = build_synthetic_dukascopy_cache(tmp.path());

    let run1_dir = tmp.path().join("run1");
    run_full_aggregator(&source, &run1_dir);
    let run1_arrow = std::fs::read(run1_dir.join("dukascopy/EURUSD/15m_bid.arrow")).unwrap();
    let run1_sidecar = std::fs::read(run1_dir.join("dukascopy/EURUSD/15m_bid.fingerprints.json")).unwrap();

    let run2_dir = tmp.path().join("run2");
    run_full_aggregator(&source, &run2_dir);
    let run2_arrow = std::fs::read(run2_dir.join("dukascopy/EURUSD/15m_bid.arrow")).unwrap();
    let run2_sidecar = std::fs::read(run2_dir.join("dukascopy/EURUSD/15m_bid.fingerprints.json")).unwrap();

    assert_eq!(run1_arrow, run2_arrow, "Arrow IPC bytes must be byte-identical across runs");
    assert_eq!(run1_sidecar, run2_sidecar, "sidecar JSON must be byte-identical across runs");
}
```

**Why this analog wins:** cli_streams.rs Test 6 is the only existing example of "run the SUT twice, mask the volatile fields, byte-compare." Phase 2's full_determinism.rs does the same but on the Arrow IPC + sidecar JSON files (no masking needed — there should be no volatile fields in the cache output).

---

## Shared Patterns

### Pattern: Error → WireError boundary conversion
**Source:** `crates/miner-core/src/error/mod.rs:42-54`
**Apply to:** `DukascopyError`, `AggregateError`, `CacheError`, `GapDetectorError`

Every new error type Phase 2 introduces must (a) derive `thiserror::Error`, (b) NOT derive `Serialize`, and (c) provide a `From<MyError> for miner_core::WireError` impl so the engine boundary can emit it as a `ScanErrorFinding`.

```rust
impl From<MinerError> for WireError {
    fn from(err: MinerError) -> Self {
        WireError {
            code: ScanErrorCode::InternalPanicCaught.as_str().to_string(),
            message: err.to_string(),
            context: std::collections::BTreeMap::new(),
        }
    }
}
```

The cache error specifically should map to `ScanErrorCode::CacheCorruption` (already defined in codes.rs:65). The reader's `CorruptSourceFile` should also map to `CacheCorruption`. Preflight-time mismatches (cache root not found, bar cache not writable) map to the new `PreflightCode::CacheRootNotFound` etc.

### Pattern: `BTreeMap` discipline for all serialised maps
**Source:** `crates/miner-core/src/findings/mod.rs:101-103, 138-151, 188-195`
**Apply to:** Sidecar JSON `per_day_fingerprint`, Arrow schema metadata, gap manifest, anything with `#[derive(Serialize)]` that contains a map.

```rust
/// `BTreeMap` (NEVER `HashMap`) for deterministic ordering — OUT-03.
pub per_day_fingerprint: std::collections::BTreeMap<chrono::NaiveDate, String>,
```

02-RESEARCH §Pitfall 2 (lines 954-963) carries the same rule. Plan review must grep every Phase 2 module for `HashMap` and reject.

### Pattern: Tracing through `tracing::*!` macros only
**Source:** `crates/miner-cli/src/main.rs:44,48,141`; `crates/miner-core/build.rs:25` exemption
**Apply to:** All Phase 2 progress / observability output.

```rust
tracing::info!(
    symbol = %params.symbol, side = ?params.side, tf = %params.tf.as_str(),
    "cache invalidated: aggregator_version bumped, rebuilding"
);
tracing::debug!(?path, "reading Dukascopy day file");
```

NEVER use `println!`, `eprintln!`, `print!`, `eprint!`, `dbg!` — the workspace `clippy.toml` bans them. The two sanctioned writers (`StdoutSink`, `stderr_emit`) are reserved for envelope + preflight-error emission. The build-script exception (`build.rs:25`) is irrelevant here.

### Pattern: Object-safe trait + `Send` bound + associated `Error`
**Source:** `crates/miner-core/src/findings/sink.rs:35-50` (`FindingSink` trait)
**Apply to:** `Reader` trait (`miner-core/src/reader.rs`)

Trait constraints:
- `Send` (cross-thread for rayon workers — Phase 3+)
- `Sync` ADDITIONAL for `Reader` because multiple workers read concurrently from one `&Reader`
- Associated `type Error: std::error::Error + Send + Sync + 'static`
- Methods return concrete types or `Box<dyn Iterator<...> + Send + 'a>` (NOT `impl Iterator` — see 02-RESEARCH lines 488-490 for the dyn-vs-impl rationale)

Object-safety regression test (mirror sink.rs:399-409):
```rust
#[test]
fn reader_trait_object_safe() {
    fn _accept(_r: &dyn Reader<Error = std::io::Error>) {}
}
```

### Pattern: `#[serde(rename_all = "snake_case")]` on every public enum
**Source:** `crates/miner-core/src/error/codes.rs:21,59`; `crates/miner-core/src/config/mod.rs:58`; `crates/miner-core/src/findings/base64_bytes.rs:74`
**Apply to:** `Side`, `Timeframe`, `GapReason`, any new `PreflightCode`/`ScanErrorCode` variants.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Timeframe { Tf15m, Tf1h, Tf1d }
```

The `Tf15m` / `Tf1h` / `Tf1d` variant names are Rust-idiomatic (can't start with a digit) but serialise as `"tf_15m"` — the user-visible string isn't quite right. **02-RESEARCH line 556 uses `as_str()` to return `"15m" / "1h" / "1d"`** for filesystem paths and Arrow metadata. Default-derived serde rename produces `"tf_15m"`. Pick one. **Recommendation: provide a manual `JsonSchema` impl** (mirror `run_id.rs:48-65`) OR override per-variant rename:
```rust
#[serde(rename_all = "snake_case")]
pub enum Timeframe {
    #[serde(rename = "15m")] Tf15m,
    #[serde(rename = "1h")]  Tf1h,
    #[serde(rename = "1d")]  Tf1d,
}
```

### Pattern: Atomic write via `tempfile::NamedTempFile::persist`
**Source:** `crates/miner-core/src/findings/sink.rs:144-159` (the `OpenOptions` + `BufWriter` flow) + 02-RESEARCH lines 742-755 (the persist extension)
**Apply to:** Both the `.arrow` cache files AND the `.fingerprints.json` sidecars. Arrow file written FIRST, then sidecar (crash-safety: 02-RESEARCH line 760).

```rust
let tmp = tempfile::NamedTempFile::new_in(parent)?;
{ /* write into tmp.as_file() */ }
tmp.as_file().sync_all()?;
tmp.persist(target).map_err(|e| e.error)?;
```

### Pattern: `walkdir` MUST sort
**Source:** 02-RESEARCH §Pitfall 1 (lines 944-953)
**Apply to:** Every `walkdir::WalkDir::new(...)` invocation in `miner-reader-dukascopy` and in cache invalidation walks.

```rust
walkdir::WalkDir::new(root)
    .sort_by_file_name()   // MANDATORY for byte-identity across runs
    .max_depth(3)
```

Consider adding `walkdir::WalkDir::new` to `clippy.toml`'s `disallowed-methods` and providing a `find_source_files(root) -> Vec<PathBuf>` helper that hardcodes the sort.

### Pattern: Integration test against the public re-export surface
**Source:** `crates/miner-core/tests/schema_roundtrip.rs:27-31` + `crates/miner-core/tests/config_precedence.rs:45`
**Apply to:** All Phase 2 `tests/*.rs` files.

```rust
use miner_core::{aggregate, AggParams, BarFrame, Reader, Side, Timeframe};
// NOT: use miner_core::aggregate::aggregate;     ← bypasses the frozen-surface gate
```

The FROZEN public-surface block in `miner-core/src/lib.rs` is the contract; tests use it so the contract stays honest.

### Pattern: `#[serial_test::serial]` for tests that mutate process env
**Source:** `crates/miner-core/tests/config_precedence.rs:81`; `crates/miner-cli/tests/cli_streams.rs:94,127,150`
**Apply to:** Any Phase 2 test that touches `MINER_*` env vars or `std::env::set_current_dir`. Reader/aggregator tests that only use `tempfile::TempDir` paths do NOT need `#[serial_test::serial]`.

---

## No Analog Found

| File | Role | Data Flow | Reason | Pattern Source |
|------|------|-----------|--------|----------------|
| `crates/miner-core/tests/gap_manifest_snapshot.rs` | test (insta snapshot) | request-response | No Phase 1 test uses `insta`; Wave 0 introduction | `insta` upstream docs — `insta::assert_json_snapshot!(value)` + commit `tests/snapshots/*.snap` files |
| `crates/miner-core/tests/arrow_schema_snapshot.rs` | test (insta snapshot) | request-response | Same as above | Same |
| `crates/miner-core/tests/snapshots/` | test-fixture directory | — | Same | `insta` creates it on first `cargo insta accept` run; committed to git |
| `crates/miner-reader-dukascopy/src/path_layout.rs` proptest tests | property tests | transform | No Phase 1 test uses `proptest`; Wave 0 introduction | `proptest` upstream macro — `proptest!` block with `#[test]` and `prop_assert_eq!` (02-RESEARCH lines 387-397 shows the exact recipe) |

**Wave 0 introductions to land alongside the first plan that needs them:**
1. `proptest = "1.11"` dev-dep on `miner-core` AND `miner-reader-dukascopy`.
2. `insta = "1.47"` dev-dep on `miner-core`.
3. `crates/miner-core/tests/snapshots/` directory (will be auto-created by `cargo insta accept`; commit it).
4. Optional: a `.cargo/config.toml` alias `cargo insta-accept` for the executor.

---

## Metadata

**Analog search scope:**
- `crates/miner-core/src/**` (all files)
- `crates/miner-core/tests/**` (2 integration tests)
- `crates/miner-cli/tests/cli_streams.rs` (test-fixture builder patterns)
- `crates/miner-cli/Cargo.toml` + `crates/miner-core/Cargo.toml` (manifest patterns)
- `Cargo.toml` workspace + `clippy.toml` (workspace constraints)

**Files scanned:** 18 source files + 5 manifests + 1 clippy.toml + 1 build.rs + the two phase-1 PHASE/RESEARCH summaries (skimmed for `unsafe_code` / `tracing` precedent).

**Pattern extraction date:** 2026-05-17

**Key constraints carried forward into every Phase 2 plan:**

1. **`unsafe_code = "forbid"` stays.** Phase 2 ships `BufReader<File>` reads. mmap deferred (RESEARCH Open Question #2 → "defer to Phase 7"). Any plan that requires `unsafe` must surface that as a SEPARATE workspace-policy change with explicit user sign-off.
2. **`tracing` is the ONLY progress output mechanism.** No `println!`, `eprintln!`, `dbg!`. The clippy ban is enforced by `cargo clippy -D warnings` in CI.
3. **`BTreeMap` in every `derive(Serialize)` type, every Arrow schema metadata constructor, every sidecar JSON map.** Plan review must grep for `HashMap`.
4. **All public-API enums get `#[serde(rename_all = "snake_case")]`** with explicit per-variant `#[serde(rename = "...")]` overrides where the wire form is not snake_case (e.g., `Timeframe::Tf15m` → `"15m"`).
5. **All new error types are thiserror enums WITHOUT `Serialize` derive,** with a `From<MyError> for miner_core::WireError` impl at the engine boundary.
6. **All `WalkDir::new(...)` invocations MUST chain `.sort_by_file_name()`.** Consider adding the bare form to `clippy.toml`'s `disallowed-methods`.
7. **Integration tests use the public `miner_core::*` re-export surface** — extending lib.rs's FROZEN block is the way to expose new types.
8. **Arrow file is written FIRST, then sidecar** (crash-safety ordering, 02-RESEARCH:760).
9. **`#[serial_test::serial]` only for env-mutating tests;** reader/aggregator/cache tests use isolated `tempfile::TempDir` paths and run in parallel without it.
10. **The 00-indexed month is encapsulated in `DukascopyMonth` (sealed newtype);** `path_layout::day_csv_zst` is the ONLY public path constructor. Round-trip proptest mandatory.

---

## PATTERN MAPPING COMPLETE
