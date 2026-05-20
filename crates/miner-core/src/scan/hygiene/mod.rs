//! Phase 5 hygiene kernels — pure-math statistical primitives.
//!
//! Pattern analog: `crate::scan::primitives` (lightweight `pub mod` root +
//! discipline statement). Every kernel here is a `#[inline] pub fn` over
//! primitive slice types (`&[f64]`, scalar params, `seed: u64`). The shape
//! mirrors Phase 4's `scan::anom::adf::kernel` + `scan::ljung_box::kernel` —
//! the 22 production scans already shipped under this discipline.
//!
//! ## Module shape (Plan 05-02 / D5-03, D5-04, D5-05)
//!
//! - [`effect_size`] — `cohens_d`, `hedges_g`, `cliffs_delta`, `vr_minus_one`
//!   (HYG-01: effect-size scalars carried on `Effect.effect_size`).
//! - [`bootstrap`] — `stationary_bootstrap_ci`, `block_bootstrap_ci`,
//!   `block_length_pwppw` (HYG-03: Politis-Romano stationary bootstrap +
//!   Politis-White / Patton-Politis-White block-length selector).
//! - [`null`] — `circular_shift_null_p` (HYG-04: empirical null p-value via
//!   uniform circular rotation; IAAFT phase-scramble defers to Phase 7
//!   under the current Plan 05-02 IAAFT decision).
//! - [`fdr`] — `bh_fdr` (HYG-02: hand-rolled Benjamini-Hochberg step-up,
//!   matches `R::p.adjust(method = "BH")` on the canonical 5-tuple within
//!   1e-12).
//! - [`seed`] — `derive_job_seed` (HYG-05: bit-for-bit reproducible per-job
//!   seed derived from the canonical job-identity tuple via blake3-32 →
//!   first-8-bytes-little-endian-as-u64).
//!
//! ## Discipline (Pattern S1 — copied from `primitives/mod.rs` lines 18-24)
//!
//! - Every kernel is a `pub fn` over primitive slice types — no `&self`, no
//!   trait bounds beyond `Fn(&[f64]) -> f64` closures for the resampling
//!   kernels. No IO. No `serde_json`. No `Reader`. No `AtomicBool` inside the
//!   inner resample loop — cancel polling lives between kernel calls in the
//!   engine, NEVER inside a single `stationary_bootstrap_ci` invocation
//!   (RESEARCH Pitfall 7 — cadence N=64 is implemented in Plan 05-03's
//!   engine glue, not here).
//! - RNG is `rand_xoshiro::Xoshiro256PlusPlus` seeded from `u64` via
//!   `SeedableRng::seed_from_u64` — NEVER `SmallRng` / `StdRng` (those are
//!   explicitly non-portable per the upstream Rand Book; HYG-05's
//!   bit-for-bit reproducibility contract would break).
//! - `debug_assert!` for kernel invariants (matches `LjungBox` kernel
//!   `ljung_box_q_and_p` discipline at `scan/ljung_box/kernel.rs:101-105`).
//!
//! ## Consumer surface
//!
//! Plans 05-03 (engine integration) and 05-04 (sweep runner) consume these
//! functions as opaque pure functions — they get a `f64` / `[f64; 2]` /
//! `Vec<f64>` back per call and do not need to understand the internal
//! resampling logic. The kernels do NOT depend on `Effect` / `ResultFinding`
//! / `ReproEnvelope` — those engine-boundary types live in
//! `crate::findings::mod` and are populated by the engine, not the kernels.

pub mod bootstrap;
pub mod effect_size;
pub mod fdr;
pub mod null;
pub mod seed;
