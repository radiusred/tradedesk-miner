//! Benjamini-Hochberg FDR — stub for Plan 05-02 Task 3 (Task 1 RED
//! placeholder).
//!
//! Task 3 fills the body. This stub exists so `hygiene/mod.rs` compiles
//! while Task 1's effect_size + seed kernels go through RED → GREEN. The
//! public signature is pinned to the contract from `05-02-PLAN.md
//! <interfaces>`.

/// Benjamini-Hochberg (1995) step-up FDR adjustment — Task 3 GREEN body.
#[allow(unused_variables)]
#[must_use]
pub fn bh_fdr(p_values: &[f64], alpha: f64) -> Vec<f64> {
    let _ = (p_values, alpha);
    unimplemented!("Plan 05-02 Task 3 GREEN fills this body")
}
