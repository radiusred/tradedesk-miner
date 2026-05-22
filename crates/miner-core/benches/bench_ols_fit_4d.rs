// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: 2026 The tradedesk-miner authors

//! Plan 07-06 Task 2 — criterion microbench for the 4-dim OLS fit hot kernel
//! (CROSS hot path used by `engle_granger::kernel::fit_ols_intercept_slope`
//! at `crates/miner-core/src/scan/cross/engle_granger/kernel.rs:185` — a
//! 2-dim variant; we exercise the 4-dim generalisation here because that
//! is the dimensionality CROSS-03 rolling-OLS regression uses).
//!
//! The production OLS helper is `pub(crate)`; the bench inlines a faithful
//! copy of the normal-equations path (`X^T X` inverted, multiplied by
//! `X^T y`) using `nalgebra::DMatrix` / `DVector` — exactly the same crate
//! and primitives the production kernel uses. The algorithm is the standard
//! textbook OLS; correctness is pinned by `engle_granger/kernel.rs::tests`
//! and `cross_ols_rolling.rs` integration tests.
//!
//! Input: 10 000 rows × 4 columns (intercept + 3 regressors) of synthetic
//! data derived from LCG closes (PATTERNS Pattern C). The design matrix
//! and response vector are built ONCE outside the timed loop; each
//! iteration re-runs the OLS solve.
//!
//! Reports to `target/criterion/ols_fit_4d_n10000/index.html`.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use nalgebra::{DMatrix, DVector};

/// Canonical Numerical Recipes LCG (PATTERNS Pattern C).
#[allow(clippy::cast_possible_truncation)]
fn lcg_closes(n: usize, seed: u64) -> Vec<f64> {
    let mut s = seed as u32;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let frac = f64::from(s) / f64::from(u32::MAX);
        out.push(1.0 + frac);
    }
    out
}

/// 4-dim OLS via nalgebra normal equations. Mirror of the production
/// `engle_granger::kernel::fit_ols_intercept_slope` pattern (line 185)
/// extended to a 4-column design matrix (intercept + 3 regressors).
/// Returns the 4-element coefficient vector `[β0, β1, β2, β3]`.
//
// Suppresses `clippy::similar_names`: `xtx`/`xty` are the canonical
// matrix-algebra short names for `Xᵀ·X` and `Xᵀ·y`; renaming either would
// hide the standard OLS notation.
#[allow(clippy::similar_names)]
fn ols_fit_4d(design: &DMatrix<f64>, y: &DVector<f64>) -> [f64; 4] {
    let x_transpose = design.transpose();
    let xtx = &x_transpose * design;
    let xty = &x_transpose * y;
    let xtx_inv = xtx.try_inverse().expect("non-singular for synthetic LCG inputs");
    let coeffs = xtx_inv * xty;
    [
        coeffs[(0, 0)],
        coeffs[(1, 0)],
        coeffs[(2, 0)],
        coeffs[(3, 0)],
    ]
}

fn bench_ols_fit_4d(c: &mut Criterion) {
    let n = 10_000usize;
    let x1 = lcg_closes(n, 0xCAFE);
    let x2 = lcg_closes(n, 0xBEEF);
    let x3 = lcg_closes(n, 0xF00D);
    let y_vec = lcg_closes(n, 0xDEAD);

    // Column-major DMatrix construction: [intercept || x1 || x2 || x3].
    let mut design_data: Vec<f64> = Vec::with_capacity(n * 4);
    design_data.extend(std::iter::repeat(1.0_f64).take(n));
    design_data.extend_from_slice(&x1);
    design_data.extend_from_slice(&x2);
    design_data.extend_from_slice(&x3);
    let design = DMatrix::<f64>::from_iterator(n, 4, design_data);
    let y = DVector::<f64>::from_iterator(n, y_vec.iter().copied());

    c.bench_function("ols_fit_4d_n10000", |b| {
        b.iter(|| {
            let coeffs = ols_fit_4d(black_box(&design), black_box(&y));
            black_box(coeffs);
        });
    });
}

criterion_group!(benches, bench_ols_fit_4d);
criterion_main!(benches);
