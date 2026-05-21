//! Phase 5 sweep runner — TOML-manifest fanout over
//! `(scan × instrument(s) × timeframe × window × params)` cartesian
//! expansion (Plan 05-04 / D5-01 / OP-04).
//!
//! Pattern analog: `crate::engine::run_one` — a single-method facade
//! returning a value with a multi-line algorithm doc. `sweep::run_sweep`
//! follows the same shape, layered ABOVE
//! `engine::run_one_with_registry`:
//!
//! 1. Parse + validate manifest (preflight rejects → exit 1).
//! 2. Expand cartesian → `Vec<ResolvedJob>`.
//! 3. `rayon::par_iter` over jobs into per-job buffers.
//! 4. Drain buffers in manifest-deterministic order to the shared sink.
//! 5. BH-FDR per family; emit `Finding::SweepSummary` between the last
//!    `Finding::Result` and `Finding::RunEnd`.
//!
//! ## Module decomposition
//!
//! - [`manifest`] — `SweepManifest` typed TOML deserialiser + preflight.
//! - [`job_graph`] — cartesian expansion + `ResolvedJob` struct.
//! - [`executor`] — rayon-parallel job execution + deterministic-order
//!   drain + BH-FDR aggregation + `SweepSummary` emission.
//!
//! The runner is sync + std + rayon (FOUND-04). No tokio, no async-std.
//!
//! ## D5-01 iteration order (pinned by `job_graph::expand`)
//!
//! 1. `[[jobs]]` block declaration order.
//! 2. Within a block: instruments (vector order) → timeframes (vector
//!    order) → windows (vector order) → params (alphabetic key order;
//!    array values expand cartesian, also in alphabetic key order).
//!
//! ## HYG-05 byte-identical-rerun
//!
//! Every `ResolvedJob` carries a deterministic `job_seed: u64` derived
//! via `hygiene::seed::derive_job_seed` from
//! `(master_seed, scan_id_at_version, instruments, timeframe, window,
//! param_hash)`. Re-running the same manifest with the same
//! `[sweep].seed` produces byte-identical JSONL (modulo masked
//! volatile fields — `run_id`, `produced_at_utc`).

pub mod executor;
pub mod job_graph;
pub mod manifest;

pub use executor::{SweepOptions, run_sweep};
pub use job_graph::ResolvedJob;
pub use manifest::{SweepManifest, read_manifest};
