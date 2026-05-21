//! tradedesk-miner core library.
//!
//! Phase 1 (Plan 03) lands the locked `Finding` envelope types, the error code
//! vocabulary, the `FindingSink` trait interface, and the config schema types.
//! Plans 04 (sink + `stderr_emit` implementations) and 05 (figment builder) build
//! on top.

// Plan 04-13: Test fixtures and golden-comparison assertions legitimately use
// patterns that clippy::pedantic flags. These allows scope to cfg(test) only —
// production code stays under the full pedantic bar.
//
// - `float_cmp`: golden tests assert exact-bit f64 equality after deterministic
//   kernel runs (Phase 3 D3-23 byte-identical-rerun contract).
// - `cast_*`: synthetic fixture generators cast usize indices to f64/i64 to
//   produce deterministic OHLCV bar data; sample sizes are bounded.
// - `cast_possible_wrap`: synthetic timestamp generation from usize loop indices.
#![cfg_attr(
    test,
    allow(
        // Numeric casts — synthetic fixture generators index over usize and
        // convert to f64 / i64 for deterministic bar data.
        clippy::float_cmp,
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        clippy::cast_sign_loss,
        clippy::cast_lossless,
        // Test ergonomics — assertions and synthetic harness loops.
        clippy::comparison_to_empty,
        clippy::useless_conversion,
        clippy::unnecessary_fallible_conversions,
        clippy::needless_range_loop,
        clippy::manual_memcpy,
        clippy::similar_names,
        clippy::many_single_char_names,
        clippy::doc_lazy_continuation,
        clippy::len_zero,
    )
)]

pub mod aggregator;
pub mod cache;
pub mod calendar;
pub mod config;
pub mod engine;
pub mod error;
pub mod findings;
pub mod gap;
pub mod reader;
pub mod scan;
// Phase 5 (Plan 05-04 / OP-04): sweep runner — TOML manifest fanout +
// rayon-parallel job execution + deterministic-order drain + BH-FDR
// aggregation + Finding::SweepSummary emission.
pub mod sweep;

/// Git SHA of the source revision that produced this build; `dirty-<sha>` when the tree
/// had uncommitted changes; `"unknown"` when git was unavailable (e.g., tarball builds).
///
/// Wired into every `Finding` envelope's `code_revision` field (Plan 03+); mitigates
/// threat T-01-04 (a deployed binary cannot lie about which source revision built it).
pub const CODE_REVISION: &str = env!("MINER_CODE_REVISION");

// =============================================================================
// FROZEN public surface — every downstream plan (05, 06, 07) imports from here.
//
// Adding a name to this list is a backwards-compatible change; removing one is a
// Phase 1 contract break. Re-ordering for readability is fine.
// =============================================================================

pub use findings::{
    Base64Bytes, DataSlice, Dtype, Effect, Finding, FindingSink, GapAbortedFinding, PerScanCounts,
    Raw, RawArray, ResultFinding, RunEnd, RunId, RunStart, RunSummary, ScanErrorFinding, Source,
    TimeRange,
};

pub use error::{MinerError, PreflightCode, ScanErrorCode, WireError};

pub use config::{CliOverrides, MinerConfig, OutputDest, build_figment};

// Phase 2 (Plan 02-01) extensions:
pub use calendar::Calendar;
pub use reader::{Blake3Hex, ClosedRangeUtc, RawBar, Reader, Side};

// Phase 2 (Plan 02-02) extensions:
pub use aggregator::{
    AGGREGATOR_VERSION, AggParams, AggregateError, BarFrame, Timeframe, aggregate,
};

// Phase 2 (Plan 02-04) extensions:
pub use gap::{GapDetector, GapManifest, GapReason, GapSpan};

// Phase 2 (Plan 02-05) extensions:
pub use cache::{
    ARROW_SCHEMA_VERSION, BarCache, CacheError, FingerprintSidecar, build_arrow_schema,
};

// Phase 3 (scan-engine-facade-cli) extensions:
pub use engine::{GapDispatch, GapPolicyKind, RunOutcome, run_one};
pub use findings::DryRunFinding;
pub use scan::ljung_box::LjungBoxScan;
pub use scan::{Registry, Scan, ScanCtx, ScanError, ScanFindingShape, ScanRequest, bootstrap};
