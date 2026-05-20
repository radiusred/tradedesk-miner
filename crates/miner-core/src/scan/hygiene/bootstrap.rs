//! Bootstrap kernels — stub for Plan 05-02 Task 2 (Task 1 RED placeholder).
//!
//! Task 2 fills the bodies. This stub exists so `hygiene/mod.rs` compiles
//! while Task 1's `effect_size` + `seed` kernels go through RED → GREEN. The
//! public signatures are pinned to the contract from `05-02-PLAN.md
//! <interfaces>`.

/// Politis-Romano (1994) stationary-bootstrap CI — Task 2 GREEN body.
#[allow(unused_variables)]
pub fn stationary_bootstrap_ci<F>(
    values: &[f64],
    stat: F,
    n_resamples: u32,
    mean_block_len: f64,
    seed: u64,
    ci_level: f64,
) -> [f64; 2]
where
    F: Fn(&[f64]) -> f64,
{
    let _ = (values, stat, n_resamples, mean_block_len, seed, ci_level);
    unimplemented!("Plan 05-02 Task 2 GREEN fills this body")
}

/// Politis-Romano fixed-block bootstrap CI — Task 2 GREEN body.
#[allow(unused_variables)]
pub fn block_bootstrap_ci<F>(
    values: &[f64],
    stat: F,
    n_resamples: u32,
    block_len: usize,
    seed: u64,
    ci_level: f64,
) -> [f64; 2]
where
    F: Fn(&[f64]) -> f64,
{
    let _ = (values, stat, n_resamples, block_len, seed, ci_level);
    unimplemented!("Plan 05-02 Task 2 GREEN fills this body")
}

/// Politis-White / Patton-Politis-White automatic block-length selector —
/// Task 2 GREEN body.
#[allow(unused_variables)]
#[must_use]
pub fn block_length_pwppw(values: &[f64]) -> f64 {
    let _ = values;
    unimplemented!("Plan 05-02 Task 2 GREEN fills this body")
}
