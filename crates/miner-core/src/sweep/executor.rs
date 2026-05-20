//! Phase 5 sweep executor — STUB (Plan 05-04 Task 1 RED).
//!
//! The Task 2 commit fills the body with rayon-parallel job execution
//! plus deterministic-order buffered drain plus BH-FDR aggregation
//! plus `Finding::SweepSummary` emission. This stub keeps the module
//! tree buildable for Task 1's tests.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::cache::BarCache;
use crate::config::MinerConfig;
use crate::error::MinerError;
use crate::findings::FindingSink;
use crate::reader::Reader;
use crate::sweep::manifest::SweepManifest;

/// Sweep execution options carried alongside the manifest.
#[derive(Debug, Clone, Default)]
pub struct SweepOptions {
    pub dry_run: bool,
}

/// Public sweep entry point — STUB; Task 2 GREEN replaces with the real
/// rayon-fanout + BH-FDR aggregation + `SweepSummary` emission body.
///
/// # Errors
/// Returns `MinerError::Preflight` if the manifest fails preflight
/// validation (Task 2 GREEN). The stub presently returns
/// `unimplemented!()`.
pub fn run_sweep<R: Reader + Sync>(
    _manifest: SweepManifest,
    _opts: SweepOptions,
    _cfg: &MinerConfig,
    _reader: &R,
    _cache: &BarCache,
    _sink: &mut dyn FindingSink,
    _cancel: Arc<AtomicBool>,
) -> Result<crate::engine::RunOutcome, MinerError> {
    unimplemented!("Plan 05-04 Task 2 will implement run_sweep")
}
