//! tradedesk-miner core library.
//!
//! Phase 1 (this plan, Wave 3) lands the locked `Finding` envelope types: a tagged enum
//! with five variants, the seven locked common fields, `RawArray`/`Base64Bytes`/`RunId`
//! supporting types, `RunStart`/`RunEnd` framing payloads. Task 2 of Plan 03 adds the
//! error vocabulary, the `FindingSink` trait, and the config schema types — the lib.rs
//! `pub use` block is extended to the FROZEN public surface in that task.

pub mod findings;

/// Git SHA of the source revision that produced this build; `dirty-<sha>` when the tree
/// had uncommitted changes; `"unknown"` when git was unavailable (e.g., tarball builds).
///
/// Wired into every `Finding` envelope's `code_revision` field (Plan 03+); mitigates
/// threat T-01-04 (a deployed binary cannot lie about which source revision built it).
pub const CODE_REVISION: &str = env!("MINER_CODE_REVISION");

// =============================================================================
// Public re-exports — extended to the FROZEN public surface in Task 2 of Plan 03.
//
// Adding a name to this list is a backwards-compatible change; removing one is a
// Phase 1 contract break.
// =============================================================================

pub use findings::{
    Base64Bytes, DataSlice, Dtype, Effect, Finding, GapAbortedFinding, PerScanCounts, Raw,
    RawArray, ResultFinding, RunEnd, RunId, RunStart, RunSummary, ScanErrorFinding, Source,
    TimeRange,
};
