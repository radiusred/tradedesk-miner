//! tradedesk-miner core library.
//!
//! Phase 1 (Plan 03) lands the locked `Finding` envelope types, the error code
//! vocabulary, the `FindingSink` trait interface, and the config schema types.
//! Plans 04 (sink + `stderr_emit` implementations) and 05 (figment builder) build
//! on top.

pub mod aggregator;
pub mod cache;
pub mod calendar;
pub mod config;
pub mod error;
pub mod findings;
pub mod gap;
pub mod reader;

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
