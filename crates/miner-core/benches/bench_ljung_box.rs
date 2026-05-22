// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: 2026 The tradedesk-miner authors

//! Plan 07-06 Task 2 — criterion microbench for the Ljung-Box Q-statistic
//! and chi-squared p-value kernel. Mirrors the production kernel at
//! `crates/miner-core/src/scan/ljung_box/kernel.rs::ljung_box_q_and_p`
//! (line 96) and the `biased_acf` helper (line 49).
//!
//! The production kernels are `pub(crate)`; the bench inlines a faithful
//! copy to avoid leaking visibility. Algorithm is byte-identical to the
//! production kernel; correctness is pinned by the kernel's own unit tests
//! in `ljung_box/kernel.rs::tests`.
//!
//! Input: 10 000 synthetic log-returns derived from LCG closes (PATTERNS
//! Pattern C). The bench sweeps lags ∈ {5, 10, 20, 50} as four separate
//! bench functions so criterion reports one timing per lag.
//!
//! Reports to:
//!   - `target/criterion/ljung_box_q_p_n10000_lag5/index.html`
//!   - `target/criterion/ljung_box_q_p_n10000_lag10/index.html`
//!   - `target/criterion/ljung_box_q_p_n10000_lag20/index.html`
//!   - `target/criterion/ljung_box_q_p_n10000_lag50/index.html`

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use statrs::distribution::{ChiSquared, ContinuousCDF};

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

/// Convert closes to log-returns (drops the first element). Mirrors
/// `crates/miner-core/src/scan/primitives/returns.rs::log_returns`.
fn log_returns(closes: &[f64]) -> Vec<f64> {
    if closes.len() < 2 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(closes.len() - 1);
    for i in 1..closes.len() {
        out.push((closes[i] / closes[i - 1]).ln());
    }
    out
}

/// Biased sample autocorrelation up to `max_lag`. Mirror of
/// `crates/miner-core/src/scan/ljung_box/kernel.rs::biased_acf` (line 49).
#[allow(clippy::cast_precision_loss)]
fn biased_acf(x: &[f64], max_lag: usize) -> Vec<f64> {
    let n = x.len();
    let n_f = n as f64;
    let mean = x.iter().copied().sum::<f64>() / n_f;
    let cent: Vec<f64> = x.iter().map(|v| v - mean).collect();
    let denom: f64 = cent.iter().map(|v| v * v).sum();
    let mut out = Vec::with_capacity(max_lag + 1);
    out.push(1.0);
    for k in 1..=max_lag {
        if denom == 0.0 {
            out.push(0.0);
            continue;
        }
        let num: f64 = (0..n.saturating_sub(k))
            .map(|i| cent[i] * cent[i + k])
            .sum();
        out.push(num / denom);
    }
    out
}

/// Ljung-Box Q-statistic + chi-squared p-values. Mirror of
/// `ljung_box/kernel.rs::ljung_box_q_and_p` (line 96). Summation order is
/// sequential cumsum-style — byte-identical with statsmodels' `np.cumsum`.
//
// Suppresses:
// - `clippy::similar_names`: `acc` (running cumsum) and `acf` (input ACF) are the
//   canonical statistics-domain short names; production kernel keeps the same pair.
// - `clippy::needless_range_loop`: `acf[k]` is the index-aware access we want;
//   `enumerate` would obscure the cumsum-style summation order that pins
//   statsmodels' `np.cumsum` equality (see kernel.rs:115 — same allow).
#[allow(clippy::cast_precision_loss, clippy::similar_names, clippy::needless_range_loop)]
fn ljung_box_q_and_p(returns_n: usize, acf: &[f64], max_lag: usize) -> (Vec<f64>, Vec<f64>) {
    debug_assert!(max_lag >= 1);
    debug_assert!(acf.len() > max_lag);
    let n = returns_n as f64;
    let mut q = Vec::with_capacity(max_lag);
    let mut p = Vec::with_capacity(max_lag);
    let mut acc = 0.0_f64;
    for k in 1..=max_lag {
        let k_f = k as f64;
        let denom = n - k_f;
        acc += acf[k] * acf[k] / denom;
        let qk = n * (n + 2.0) * acc;
        q.push(qk);
        let chi = ChiSquared::new(k_f).expect("k >= 1");
        p.push(1.0 - chi.cdf(qk));
    }
    (q, p)
}

fn bench_for_lag(c: &mut Criterion, lag: usize, name: &'static str) {
    let closes = lcg_closes(10_000, 0xCAFE);
    let returns = log_returns(&closes);
    let n = returns.len();
    c.bench_function(name, |b| {
        b.iter(|| {
            let acf = biased_acf(black_box(&returns), black_box(lag));
            let (q, p) = ljung_box_q_and_p(n, black_box(&acf), black_box(lag));
            black_box((q, p));
        });
    });
}

fn bench_ljung_box_lag_5(c: &mut Criterion) {
    bench_for_lag(c, 5, "ljung_box_q_p_n10000_lag5");
}

fn bench_ljung_box_lag_10(c: &mut Criterion) {
    bench_for_lag(c, 10, "ljung_box_q_p_n10000_lag10");
}

fn bench_ljung_box_lag_20(c: &mut Criterion) {
    bench_for_lag(c, 20, "ljung_box_q_p_n10000_lag20");
}

fn bench_ljung_box_lag_50(c: &mut Criterion) {
    bench_for_lag(c, 50, "ljung_box_q_p_n10000_lag50");
}

criterion_group!(
    benches,
    bench_ljung_box_lag_5,
    bench_ljung_box_lag_10,
    bench_ljung_box_lag_20,
    bench_ljung_box_lag_50,
);
criterion_main!(benches);
