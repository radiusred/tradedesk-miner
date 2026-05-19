//! Shared kernel-only primitives consumed by Phase 4 scans.
//!
//! ## Module shape (Plan 04-02 / D4-06 / Pitfall 9)
//!
//! - [`returns`] — log / simple / intraday / overnight return kernels (ANOM-01
//!   surface). `returns::log_returns` is the byte-identical move of the
//!   Phase 3 `ljung_box::kernel::log_returns` body (D4-06; Pitfall 9 — "move,
//!   do not rewrite").
//! - [`time_alignment`] — `inner_join(&BarFrame, &BarFrame) -> AlignedPair`
//!   (CROSS-01) + `intersect_gaps(&GapManifest, &GapManifest) -> GapManifest`
//!   (D4-04 helper; PATTERNS.md Pattern I home decision: co-located with the
//!   inner-join primitive that CROSS-01 owns).
//! - [`raw_array`] — `f64_slice_to_raw_array(&[f64]) -> RawArray`. The helper
//!   currently duplicated inline in `ljung_box/mod.rs` is lifted here once; the
//!   22 Phase 4 scans consume the same single copy.
//!
//! ## Discipline (carried from `04-PATTERNS.md` Pattern B)
//!
//! - Every kernel is `#[inline] pub fn` over primitive slice types.
//! - No IO, no `serde_json`, no `Reader` calls.
//! - `statrs` is the only distribution path (for primitives that need one;
//!   `returns` does not).
//! - `debug_assert!` for kernel invariants.

pub mod raw_array;
pub mod returns;
pub mod time_alignment;
