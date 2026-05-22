// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: 2026 The tradedesk-miner authors

//! Plan 07-06 Task 2 — criterion microbench for the rolling correlation
//! kernels (Pearson + Spearman, CROSS-02). Mirrors the production kernel
//! at `crates/miner-core/src/scan/cross/corr_rolling/kernel.rs`.
//!
//! The production kernel is `pub(crate)`; rather than promote its
//! visibility (and pollute the public surface) or call the full `Scan::run`
//! path (which adds envelope-build overhead unrelated to the kernel timing),
//! the bench inlines a faithful copy of the per-window math. The algorithm
//! is byte-identical to the production kernel; CROSS-02 unit tests in
//! `corr_rolling/kernel.rs::tests` continue to pin the math.
//!
//! Reports to:
//!   - `target/criterion/rolling_corr_pearson_w100_n10000/index.html`
//!   - `target/criterion/rolling_corr_spearman_w100_n10000/index.html`

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};

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

/// Per-window Pearson correlation — mirror of
/// `crates/miner-core/src/scan/cross/corr_rolling/kernel.rs::rolling_pearson`.
#[allow(clippy::cast_precision_loss)]
fn rolling_pearson(a: &[f64], b: &[f64], window: usize) -> Vec<f64> {
    assert_eq!(a.len(), b.len(), "a.len() must equal b.len()");
    assert!(window >= 2, "window must be >= 2");
    let n = a.len();
    if n < window {
        return Vec::new();
    }
    let count = n - window + 1;
    let mut out = Vec::with_capacity(count);
    let w_f = window as f64;
    for i in 0..count {
        let mut sum_a = 0.0_f64;
        let mut sum_b = 0.0_f64;
        for t in i..i + window {
            sum_a += a[t];
            sum_b += b[t];
        }
        let mean_a = sum_a / w_f;
        let mean_b = sum_b / w_f;
        let mut cov = 0.0_f64;
        let mut var_a = 0.0_f64;
        let mut var_b = 0.0_f64;
        for t in i..i + window {
            let da = a[t] - mean_a;
            let db = b[t] - mean_b;
            cov += da * db;
            var_a += da * da;
            var_b += db * db;
        }
        let denom = (var_a * var_b).sqrt();
        let r = if denom == 0.0 { f64::NAN } else { cov / denom };
        out.push(r);
    }
    out
}

/// Average-rank tie-correction (scipy.stats.spearmanr default
/// `method = "average"`). Mirror of
/// `corr_rolling/kernel.rs::rank_with_ties`.
#[allow(clippy::cast_precision_loss)]
fn rank_with_ties(values: &[f64]) -> Vec<f64> {
    let n = values.len();
    if n == 0 {
        return Vec::new();
    }
    let mut indices: Vec<usize> = (0..n).collect();
    indices.sort_by(|&i, &j| values[i].total_cmp(&values[j]));
    let mut ranks = vec![0.0_f64; n];
    let mut i = 0;
    while i < n {
        let pivot_bits = values[indices[i]].to_bits();
        let mut j = i + 1;
        while j < n && values[indices[j]].to_bits() == pivot_bits {
            j += 1;
        }
        let avg_rank = ((i + 1 + j) as f64) / 2.0;
        for k in i..j {
            ranks[indices[k]] = avg_rank;
        }
        i = j;
    }
    ranks
}

/// Per-window Spearman correlation — mirror of
/// `corr_rolling/kernel.rs::rolling_spearman`. Ranks each window then runs
/// the Pearson kernel on the ranks.
#[allow(clippy::cast_precision_loss)]
fn rolling_spearman(a: &[f64], b: &[f64], window: usize) -> Vec<f64> {
    assert_eq!(a.len(), b.len(), "a.len() must equal b.len()");
    assert!(window >= 2, "window must be >= 2");
    let n = a.len();
    if n < window {
        return Vec::new();
    }
    let count = n - window + 1;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let ranks_a = rank_with_ties(&a[i..i + window]);
        let ranks_b = rank_with_ties(&b[i..i + window]);
        let w_f = window as f64;
        let mut sum_a = 0.0_f64;
        let mut sum_b = 0.0_f64;
        for t in 0..window {
            sum_a += ranks_a[t];
            sum_b += ranks_b[t];
        }
        let mean_a = sum_a / w_f;
        let mean_b = sum_b / w_f;
        let mut cov = 0.0_f64;
        let mut var_a = 0.0_f64;
        let mut var_b = 0.0_f64;
        for t in 0..window {
            let da = ranks_a[t] - mean_a;
            let db = ranks_b[t] - mean_b;
            cov += da * db;
            var_a += da * da;
            var_b += db * db;
        }
        let denom = (var_a * var_b).sqrt();
        let r = if denom == 0.0 { f64::NAN } else { cov / denom };
        out.push(r);
    }
    out
}

fn bench_pearson_rolling(c: &mut Criterion) {
    let a = lcg_closes(10_000, 0xCAFE);
    let b = lcg_closes(10_000, 0xBEEF);
    let window = 100;
    c.bench_function("rolling_corr_pearson_w100_n10000", |bb| {
        bb.iter(|| {
            let r = rolling_pearson(black_box(&a), black_box(&b), black_box(window));
            black_box(r);
        });
    });
}

fn bench_spearman_rolling(c: &mut Criterion) {
    let a = lcg_closes(10_000, 0xCAFE);
    let b = lcg_closes(10_000, 0xBEEF);
    let window = 100;
    c.bench_function("rolling_corr_spearman_w100_n10000", |bb| {
        bb.iter(|| {
            let r = rolling_spearman(black_box(&a), black_box(&b), black_box(window));
            black_box(r);
        });
    });
}

criterion_group!(benches, bench_pearson_rolling, bench_spearman_rolling);
criterion_main!(benches);
