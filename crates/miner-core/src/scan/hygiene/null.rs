//! Null-distribution kernels — stub for Plan 05-02 Task 2 (Task 1 RED
//! placeholder).
//!
//! Task 2 fills the body. This stub exists so `hygiene/mod.rs` compiles
//! while Task 1's effect_size + seed kernels go through RED → GREEN. The
//! public signature is pinned to the contract from `05-02-PLAN.md
//! <interfaces>`.

/// Circular-shift empirical null p-value — Task 2 GREEN body.
#[allow(unused_variables)]
pub fn circular_shift_null_p<F>(
    values: &[f64],
    observed_stat: f64,
    stat: F,
    n_resamples: u32,
    seed: u64,
) -> f64
where
    F: Fn(&[f64]) -> f64,
{
    let _ = (values, observed_stat, stat, n_resamples, seed);
    unimplemented!("Plan 05-02 Task 2 GREEN fills this body")
}
