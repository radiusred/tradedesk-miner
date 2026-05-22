# Phase 5: Statistical Hygiene & Sweep Runner — Pattern Map

**Mapped:** 2026-05-20
**Files analyzed:** 24 new + 7 modified (31 total)
**Analogs found:** 31 / 31 (every Phase 5 file has a Phase 1-4 precedent)

This map tells the planner exactly which existing files each new Phase 5 file should copy from. Every analog is a CURRENT file (Phase 1-4 already shipped) — never a hypothetical or research-only example.

## File Classification

### NEW kernel modules (`crates/miner-core/src/scan/hygiene/`)

| New File | Role | Data Flow | Closest Analog | Match Quality |
|----------|------|-----------|----------------|---------------|
| `scan/hygiene/mod.rs` | module re-export root | scan-time | `crates/miner-core/src/scan/primitives/mod.rs` | exact (same role: lightweight module-doc + pub mod re-exports for sibling kernels) |
| `scan/hygiene/effect_size.rs` | pure-math kernel | scan-time, compute | `crates/miner-core/src/scan/ljung_box/kernel.rs` | exact (same role: `#[inline] pub(crate) fn` over `&[f64]` with sibling `#[cfg(test)] mod tests`) |
| `scan/hygiene/bootstrap.rs` | pure-math kernel + RNG | scan-time, resample | `crates/miner-core/src/scan/anom/adf/kernel.rs` | role-match (hand-rolled deterministic statistical kernel; ADF has the closest "sequential inner loop, no rayon inside" discipline) |
| `scan/hygiene/null.rs` | pure-math kernel + FFT | scan-time, surrogate gen | `crates/miner-core/src/scan/anom/adf/kernel.rs` | role-match (same hand-rolled kernel split + statrs distribution usage) |
| `scan/hygiene/fdr.rs` | pure-math kernel | post-stream batch | `crates/miner-core/src/scan/ljung_box/kernel.rs` | exact (~25 LOC pure function on `&[f64]`; mirrors the LjungBox `biased_acf` + `ljung_box_q_and_p` shape) |
| `scan/hygiene/seed.rs` | hash-derivation helper | preflight, one-shot | `crates/miner-core/src/engine/param_hash.rs` | exact (Blake3Hex hashing of canonical bytes → fixed-size output; same crate, same pattern) |

### NEW sweep runner modules (`crates/miner-core/src/sweep/`)

| New File | Role | Data Flow | Closest Analog | Match Quality |
|----------|------|-----------|----------------|---------------|
| `sweep/mod.rs` | module re-export + entry point | sweep-orchestration | `crates/miner-core/src/engine/mod.rs` (lines 1-50, the doc header + pub mod block) | role-match (top-level orchestration crate root with documented step-by-step algorithm) |
| `sweep/manifest.rs` | TOML deserialiser | config parsing | `crates/miner-core/src/config/` (figment+serde::Deserialize structs) | role-match (typed `#[derive(Deserialize)]` struct tree for an external config file) |
| `sweep/job_graph.rs` | cartesian expansion | pure transform | `crates/miner-core/src/engine/gap_policy.rs` (sub-range partitioning) | role-match (pure function `expand(input) -> Vec<ResolvedItem>`; same shape as `gap_policy::dispatch`) |
| `sweep/executor.rs` | rayon par_iter + buffered drain | parallel orchestration | `crates/miner-core/src/engine/mod.rs::run_one_with_registry` (lines 227-end) | role-match (the engine facade; sweep wraps it with rayon + buffered output) |

### NEW integration tests (`crates/miner-core/tests/`)

| New File | Role | Data Flow | Closest Analog | Match Quality |
|----------|------|-----------|----------------|---------------|
| `tests/sweep_smoke.rs` | end-to-end integration | request → JSONL | `crates/miner-core/tests/scan_ljung_box.rs` | role-match (single-scan end-to-end with masked envelope assertions) |
| `tests/sweep_dry_run.rs` | dry-run integration | request → 3-envelope JSONL | `crates/miner-core/tests/dry_run.rs` | exact (same role: dry-run short-circuit emits exactly N envelopes) |
| `tests/sweep_summary_emission.rs` | end-of-sweep envelope test | request → sweep_summary | `crates/miner-core/tests/scan_seas_anova_kruskal.rs` | role-match (parses captured envelopes, asserts envelope ordering + shape) |
| `tests/sweep_byte_identical_rerun.rs` | bit-exact regression | request × 2 → diff | `crates/miner-core/tests/byte_identical_rerun.rs` | exact (same role: run twice, mask volatile fields, assert byte-equal JSONL) |
| `tests/fdr_family_scoping.rs` | per-family BH-FDR coverage | request → sweep_summary | `crates/miner-core/tests/scan_seas_anova_kruskal.rs` | role-match (parametric envelope-shape coverage across enum variants) |
| `tests/effect_size_emission.rs` | per-finding effect-size pin | request → Result | `crates/miner-core/tests/scan_ljung_box.rs` | role-match (asserts `effect.*` field shape on emitted Result findings) |
| `tests/bootstrap_block_length_golden.rs` | R reference golden (gated) | golden compare | `crates/miner-core/tests/scan_ljung_box.rs` (statsmodels golden) | exact (same role: load golden JSON, run kernel, compare within tolerance; gate behind provenance) |

### NEW CLI surface (`crates/miner-cli/`)

| New File | Role | Data Flow | Closest Analog | Match Quality |
|----------|------|-----------|----------------|---------------|
| `crates/miner-cli/src/sweep_args.rs` | clap-derive Args struct | CLI parsing | `crates/miner-cli/src/scan_args.rs` | exact (same role: clap `#[derive(Args)]` subcommand struct + `to_request()` conversion) |
| `crates/miner-cli/tests/sigint_mid_sweep.rs` | CLI binary SIGINT test | spawn → signal → exit | `crates/miner-cli/tests/sigint_preserves_stream.rs` | exact (same role: spawn binary, deliver SIGINT, assert exit 130 + envelope persistence) |

### MODIFIED files (additive only)

| Modified File | Role | Change Type | Closest "How to extend" Analog | Reason |
|---------------|------|-------------|--------------------------------|--------|
| `crates/miner-core/src/findings/mod.rs` | envelope contract | + 1 enum variant, + 2 Option fields, + 7 new structs | self (lines 168-181 `Effect`, 242-263 `ResultFinding`, 361-370 `Finding` enum) | extend in place; existing tests use exhaustive destructure (test 12) — update accordingly |
| `crates/miner-core/src/scan/mod.rs` | trait contract | + 2 default-false trait methods, + 1 enum | self (lines 120-169 `Scan` trait, 73-105 `ScanArity` enum) | follow Phase 4 D4-02 precedent: add default methods, regression-test object-safety |
| `crates/miner-core/src/engine/mod.rs` | facade entry | + `run_sweep` function, post-scan hygiene hook in `run_one_with_registry` | self (lines 175-190 `run_one`, lines 227+ `run_one_with_registry`) | mirror the `run_one` → `run_one_with_registry` wrapper pattern |
| `crates/miner-core/src/error/codes.rs` | error vocabulary | + 1 `PreflightCode` variant | self (lines 22-44 `PreflightCode` enum, line 41 `SweepTooLarge` already shipped) | append variant + `as_str` arm; extend `preflight_code_serialises_snake_case` cases array |
| `crates/miner-cli/src/scan_args.rs` | CLI struct | + 5 `#[arg]` flags (universal hygiene) | self (lines 56-111 `ScanArgs` struct) | append `#[arg(long)] pub bootstrap: Option<String>` style fields |
| `crates/miner-cli/src/cli.rs` | Command enum | + 1 `Command::Sweep(SweepArgs)` variant | self (lines 56-75 `Command` enum) | append `Sweep(SweepArgs)` arm with doc comment |
| `Cargo.toml` (workspace) | dep manifest | + 3 `[workspace.dependencies]` entries | self (lines 39-72) | append `rand`, `rand_xoshiro`, `toml` in the Phase-bucket comment block |
| `crates/miner-core/Cargo.toml` | crate dep manifest | + 3 deps via workspace inheritance | self | mirror the existing `statrs.workspace = true` style |
| `tests/REFERENCE-VERSIONS.md` | doc | + R 4.x + tseries/stats pins | self (Phase 4 statsmodels block) | append a new "R reference (Phase 5 BH-FDR + bootstrap goldens)" section |

---

## Pattern Assignments

### `scan/hygiene/mod.rs` (module root, scan-time)

**Analog:** `crates/miner-core/src/scan/primitives/mod.rs` (28 lines)

**Module-doc + re-exports pattern** (entire file):
```rust
//! Shared kernel-only primitives consumed by Phase 4 scans.
//!
//! ## Module shape (Plan 04-02 / D4-06 / Pitfall 9)
//!
//! - [`returns`] — log / simple / intraday / overnight return kernels (ANOM-01
//!   surface). `returns::log_returns` is the byte-identical move of the
//!   Phase 3 `ljung_box::kernel::log_returns` body (D4-06; Pitfall 9 — "move,
//!   do not rewrite").
//! - [`time_alignment`] — `inner_join(&BarFrame, &BarFrame) -> AlignedPair`
//!   (CROSS-01) + `intersect_gaps(...)` ...
//! - [`raw_array`] — `f64_slice_to_raw_array(&[f64]) -> RawArray` ...
//!
//! ## Discipline (carried from `04-PATTERNS.md` Pattern B)
//!
//! - Every kernel is `#[inline] pub fn` over primitive slice types.
//! - No IO, no `serde_json`, no `Reader` calls.
//! - `statrs` is the only distribution path...
//! - `debug_assert!` for kernel invariants.

pub mod raw_array;
pub mod returns;
pub mod time_alignment;
```

**Copy this pattern verbatim** — replace the three `pub mod ...` lines with `pub mod effect_size; pub mod bootstrap; pub mod null; pub mod fdr; pub mod seed;`. Re-state the discipline bullets (pure functions, no IO, no serde, `debug_assert!`).

---

### `scan/hygiene/effect_size.rs` (pure-math kernel)

**Analog:** `crates/miner-core/src/scan/ljung_box/kernel.rs` (288 lines)

**Imports pattern** (lines 14-18 of analog):
```rust
#![cfg_attr(any(test, debug_assertions), allow(clippy::float_cmp))]

use statrs::distribution::ChiSquared;
use statrs::distribution::ContinuousCDF;
```
For effect_size.rs, omit the `use statrs::...` lines — Cohen's d / Hedges' g / Cliff's delta / VR-minus-one do not need any distribution CDF.

**Public kernel function pattern** (lines 44-69 of analog — `biased_acf`):
```rust
/// Biased sample autocorrelation up to `max_lag` lags.
///
/// Returns a `Vec<f64>` of length `max_lag + 1` where `acf[0] == 1.0` by
/// construction and `acf[k]` (for `k >= 1`) is the biased ACF estimator at lag
/// `k`...
///
/// ## Constant-series special case
///
/// For a constant series (`denom == 0.0`), the naive formula yields `0.0 / 0.0`
/// for every `k >= 1`. This kernel returns `0.0` at every `k >= 1` instead of
/// `NaN`...
#[inline]
#[allow(
    clippy::cast_precision_loss,
    reason = "n is the returns sample size — bar counts fit trivially in f64's 52-bit mantissa for any realistic OHLCV series (Phase 1 cap << 2^52)"
)]
pub(crate) fn biased_acf(x: &[f64], max_lag: usize) -> Vec<f64> {
    let n = x.len();
    let n_f = n as f64;
    let mean = x.iter().copied().sum::<f64>() / n_f;
    ...
}
```

**Apply to:** each of `cohens_d`, `hedges_g`, `cliffs_delta`, `vr_minus_one`. Document the formula in the doc-comment, mark degenerate cases (NaN return), and add `#[inline]`.

**Test module pattern** (lines 141-217 of analog):
```rust
#[cfg(test)]
#[allow(
    clippy::cast_lossless,
    clippy::needless_range_loop,
)]
mod tests {
    use super::*;

    const TOL: f64 = 1e-12;

    fn approx_eq(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    #[test]
    fn biased_acf_lag0_is_one() {
        let x = [1.0, 2.0, 3.0, 2.5, 2.0];
        let acf = biased_acf(&x, 2);
        assert!(approx_eq(acf[0], 1.0, TOL), "acf[0]={}", acf[0]);
    }

    #[test]
    fn biased_acf_known_input() {
        // Hand-computed reference for x = [1.0, 1.5, 2.0, 1.8, 1.6, 2.2, 2.8, 2.5].
        // Precomputed via reference Python with the same algorithm; pinned here
        // within 1e-12...
        let expected = [
            1.0_f64,
            0.448_340_471_092_077_1,
            ...
        ];
        for (i, (got, want)) in acf.iter().zip(expected.iter()).enumerate() {
            assert!(approx_eq(*got, *want, TOL), "acf[{i}]={} vs expected {}", got, want);
        }
    }
}
```

**Apply to:** each effect-size kernel gets at least one known-answer test (hand-computed or copied from scipy reference) and one degenerate-input test (n=0, n=1, equal groups → NaN).

---

### `scan/hygiene/bootstrap.rs` (pure-math kernel + deterministic RNG)

**Analog:** `crates/miner-core/src/scan/anom/adf/kernel.rs` (the ADF kernel — sequential inner loop, no rayon inside, deterministic given seed)

**Imports pattern** (NEW for Phase 5 — extend the LjungBox kernel imports with rand_xoshiro):
```rust
use rand::SeedableRng;
use rand::Rng;
use rand_xoshiro::Xoshiro256PlusPlus;
```

**Generic-over-stat-closure signature** (RESEARCH §"Pattern 2" — concrete; no current analog uses generics-over-closure, but the Phase 4 ADF kernel's hand-rolled multi-arg signature is the closest):
```rust
/// Politis-Romano (1994) stationary bootstrap CI on a scalar statistic of an
/// autocorrelated series.
///
/// `stat` is the statistic functional being CI'd...
/// `mean_block_len` is the expected block length under the geometric
/// distribution (Politis-White 2004 selector recommended)...
/// `seed` propagates from the per-job derived seed (HYG-05)...
///
/// Returns the percentile CI: `[quantile(boot_stats, alpha/2),
/// quantile(boot_stats, 1 - alpha/2)]`.
///
/// # Errors / edge cases
/// Returns `[NaN, NaN]` when `values.len() < 2` or `n_resamples == 0`.
pub fn stationary_bootstrap_ci<F>(
    values: &[f64],
    stat: F,
    n_resamples: u32,
    mean_block_len: f64,
    seed: u64,
    ci_level: f64,
) -> [f64; 2]
where F: Fn(&[f64]) -> f64
{
    let n = values.len();
    if n < 2 || n_resamples == 0 {
        return [f64::NAN, f64::NAN];
    }
    let mut rng = Xoshiro256PlusPlus::seed_from_u64(seed);
    let p_continue: f64 = 1.0 / mean_block_len;
    let mut boot_stats: Vec<f64> = Vec::with_capacity(n_resamples as usize);
    let mut buf: Vec<f64> = Vec::with_capacity(n);

    for resample in 0..n_resamples {
        // Cancel poll every N=64 resamples — analog: Plan 04-10 SEAS scans use
        // CANCEL_POLL_CADENCE = 4096 for cheap inner loops; bootstrap iterations
        // are heavier so 64 is the recommended cadence (RESEARCH Pitfall 7).
        buf.clear();
        let mut idx = rng.gen_range(0..n);
        while buf.len() < n {
            buf.push(values[idx]);
            if rng.gen::<f64>() < p_continue {
                idx = rng.gen_range(0..n);
            } else {
                idx = (idx + 1) % n;
            }
        }
        boot_stats.push(stat(&buf));
    }
    boot_stats.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let alpha_half = (1.0 - ci_level) / 2.0;
    let lo_idx = ((n_resamples as f64) * alpha_half).floor() as usize;
    let hi_idx = (((n_resamples as f64) * (1.0 - alpha_half)).ceil() as usize)
        .saturating_sub(1)
        .min(boot_stats.len() - 1);
    [boot_stats[lo_idx], boot_stats[hi_idx]]
}
```

**Cancel-poll pattern** — borrow from the LjungBoxScan inner-loop discipline (kernel doesn't directly hold `Arc<AtomicBool>`; the engine wraps invocation). For bootstrap inner loops, the kernel takes an optional `&AtomicBool` for cancel polling — same as the SEAS-anova_kruskal kernel does for `CANCEL_POLL_CADENCE`. **Plan-phase decides:** either the kernel polls (passing the flag) or the engine polls between bootstrap calls (cheaper, smaller surface).

---

### `scan/hygiene/null.rs` (phase-scramble + circular-shift)

**Analog:** `crates/miner-core/src/scan/anom/adf/kernel.rs` (hand-rolled multi-iteration kernel) for IAAFT; `crates/miner-core/src/scan/seas/hour_of_day/kernel.rs` for circular-shift (trivial rotation).

**Imports pattern**:
```rust
use rand::SeedableRng;
use rand::Rng;
use rand_xoshiro::Xoshiro256PlusPlus;
// Optional — only when shipping IAAFT in Phase 5:
// use realfft::{RealFftPlanner, num_complex::Complex};
```

**Public kernel function pattern** (mirrors LjungBox kernel structure):
```rust
/// Circular-shift surrogate-data null distribution p-value.
///
/// Builds `n_resamples` surrogate series by rotating `values` by a uniform
/// offset in `[1, n-1]` (offset 0 is rejected — it's the identity transform).
/// For each surrogate, computes `stat(&shifted)` and tallies the rank of the
/// observed statistic against the surrogate distribution. Returns the
/// two-sided empirical p-value.
///
/// # Edge cases
/// Returns `NaN` when `values.len() < 2` or `n_resamples == 0`.
pub fn circular_shift_null_p<F>(
    values: &[f64],
    observed_stat: f64,
    stat: F,
    n_resamples: u32,
    seed: u64,
) -> f64
where F: Fn(&[f64]) -> f64
{
    let n = values.len();
    if n < 2 || n_resamples == 0 { return f64::NAN; }
    let mut rng = Xoshiro256PlusPlus::seed_from_u64(seed);
    let mut surrogate: Vec<f64> = vec![0.0; n];
    let mut more_extreme = 0u32;
    for _ in 0..n_resamples {
        let offset = rng.gen_range(1..n); // skip 0 = identity
        for i in 0..n {
            surrogate[i] = values[(i + offset) % n];
        }
        let surr_stat = stat(&surrogate);
        if surr_stat.abs() >= observed_stat.abs() {
            more_extreme += 1;
        }
    }
    f64::from(more_extreme) / f64::from(n_resamples)
}
```

**IAAFT** (only if shipped in Phase 5; otherwise defer): follow the same hand-rolled-with-debug_assert discipline as `ljung_box_q_and_p` (kernel.rs lines 87-128). Iterate to convergence (default 10 iterations per Theiler 1992); document the rank-distance convergence criterion in the doc comment.

---

### `scan/hygiene/fdr.rs` (Benjamini-Hochberg)

**Analog:** `crates/miner-core/src/scan/ljung_box/kernel.rs` (the `biased_acf` shape — ~25 LOC pure function with hand-computed test).

**Concrete kernel** (already drafted in RESEARCH lines 310-369; copy verbatim):
```rust
/// Benjamini-Hochberg step-up FDR adjustment (Benjamini & Hochberg 1995).
///
/// Returns adjusted q-values in INPUT ORDER (same index as `p_values`).
/// Internally sorts a working buffer; the input slice is not mutated.
///
/// `alpha` is the family-wise FDR target; not used directly in the q-value
/// computation but documented for clarity (callers may reject q > alpha
/// downstream).
pub fn bh_fdr(p_values: &[f64], alpha: f64) -> Vec<f64> {
    let n = p_values.len();
    if n == 0 { return Vec::new(); }
    debug_assert!((0.0..=1.0).contains(&alpha), "alpha out of range");

    let mut indexed: Vec<(usize, f64)> = p_values.iter().copied().enumerate().collect();
    indexed.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

    // BH step-up: q[(i)] = min(1, p[(i)] * n / (i+1)),
    // then enforce monotone non-increasing from the top via a reverse scan.
    let mut q = vec![0.0f64; n];
    let mut running_min = 1.0f64;
    for k in (0..n).rev() {
        let i = k + 1; // 1-indexed rank
        let raw_q = (indexed[k].1 * n as f64 / i as f64).min(1.0);
        running_min = running_min.min(raw_q);
        q[indexed[k].0] = running_min;
    }
    q
}
```

**Test pattern** (mirrors `ljung_box_q_and_p_known_input`, kernel.rs lines 236-267):
- Canonical 5-tuple `[0.01, 0.02, 0.03, 0.04, 0.05]` → all q ≈ 0.05 (matches R `p.adjust`).
- Monotonicity proptest: q-values respect rank-order of p-values.
- n=0 returns empty Vec.

---

### `scan/hygiene/seed.rs` (per-job seed derivation)

**Analog:** `crates/miner-core/src/engine/param_hash.rs` (Blake3Hex param-hash computation)

**Imports pattern**:
```rust
use blake3::Hasher;
use crate::reader::InstrumentSpec;
use crate::aggregator::Timeframe;
use crate::reader::ClosedRangeUtc;
```

**Hash-derivation function pattern** (mirrors `param_hash::param_hash` — same crate, same Blake3Hex convention from Phase 2 D2-05):
```rust
/// Derive a per-job 64-bit seed from the sweep master seed + the job's
/// canonical identity tuple (HYG-05 / D5-05). The blake3-32 hash collapses to
/// 64 bits via little-endian read of the first 8 bytes...
///
/// Canonicalisation rules (MUST match the byte-identical-rerun invariant):
/// - `master_seed` is written little-endian.
/// - `scan_id_at_version` is its raw `"scan_id@version"` ASCII string.
/// - `instruments` are written in vector order, each as `"SYMBOL:side"`.
/// - `timeframe` is its `as_str()` form (`"15m"` / `"1h"` / `"1d"`).
/// - `window` is its ISO-8601 RFC3339 `start_utc/end_utc` pair separated by `/`.
/// - `param_hash` is the existing Phase 2 Blake3Hex hex string.
pub fn derive_job_seed(
    master_seed: u64,
    scan_id_at_version: &str,
    instruments: &[InstrumentSpec],
    timeframe: Timeframe,
    window: &ClosedRangeUtc,
    param_hash: &str,
) -> u64 {
    let mut h = Hasher::new();
    h.update(&master_seed.to_le_bytes());
    h.update(scan_id_at_version.as_bytes());
    for spec in instruments {
        h.update(format!("{}:{}", spec.symbol, spec.side.as_str()).as_bytes());
    }
    h.update(timeframe.as_str().as_bytes());
    h.update(format!("{}/{}", window.start.to_rfc3339(), window.end.to_rfc3339()).as_bytes());
    h.update(param_hash.as_bytes());
    let bytes = h.finalize();
    u64::from_le_bytes(bytes.as_bytes()[..8].try_into().expect("blake3 32-byte output"))
}
```

**Test pattern**: copy the `param_hash` test structure (deterministic same-inputs-same-output assertion; different-inputs-different-output assertion).

---

### `sweep/mod.rs` (sweep module root + entry point)

**Analog:** `crates/miner-core/src/engine/mod.rs` (lines 1-50 — module doc + pub mod re-exports + the `run_one` entry point signature)

**Module-doc pattern** (lines 1-27 of engine/mod.rs):
```rust
//! Phase 5 sweep runner — TOML-manifest fanout over `(scan × instrument(s) ×
//! timeframe × window × params)` cartesian expansion.
//!
//! Pattern analog: `crate::engine::run_one` — a single-method facade returning
//! a value with a multi-line algorithm doc. `sweep::run_sweep` follows the
//! same shape, layered ABOVE `run_one_with_registry`:
//!
//! 1. Parse + validate manifest (preflight rejects → exit 1).
//! 2. Expand cartesian → Vec<ResolvedJob>.
//! 3. `rayon::par_iter` over jobs into per-job buffers.
//! 4. Drain buffers in manifest-deterministic order to the shared sink.
//! 5. BH-FDR per family; emit `Finding::SweepSummary` between last Result
//!    and `RunEnd`.
//!
//! ## Module decomposition
//!
//! - [`manifest`] — `SweepManifest` typed TOML deserialiser.
//! - [`job_graph`] — cartesian expansion + `ResolvedJob` struct.
//! - [`executor`] — rayon-parallel job execution + deterministic-order drain.
//!
//! The runner is sync + std + rayon (FOUND-04). No tokio, no async-std.

pub mod executor;
pub mod job_graph;
pub mod manifest;

pub use executor::run_sweep;
pub use manifest::SweepManifest;
```

---

### `sweep/manifest.rs` (TOML deserialiser)

**Analog:** `crates/miner-core/src/config/` (figment + serde derives — already in repo for `MinerConfig`).

**Typed struct tree pattern** (concrete shape per RESEARCH §"Code Examples Example 2" lines 786-862):
```rust
use std::collections::BTreeMap;
use serde::Deserialize;
use chrono::{DateTime, Utc};

use crate::error::{MinerError, PreflightCode, WireError};

#[derive(Debug, Clone, Deserialize)]
pub struct SweepManifest {
    #[serde(default)]
    pub sweep: SweepConfig,
    #[serde(default)]
    pub hygiene: HygieneBlock,
    #[serde(default)]
    pub fdr: FdrConfig,
    #[serde(default, rename = "jobs")]
    pub jobs: Vec<JobBlock>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SweepConfig {
    pub seed: Option<u64>,
    #[serde(default = "default_max_jobs")]
    pub max_jobs: u64,
}
fn default_max_jobs() -> u64 { 100_000 }

#[derive(Debug, Clone, Default, Deserialize)]
pub struct HygieneBlock {
    pub bootstrap: Option<String>,
    #[serde(default)]
    pub bootstrap_n: u32,
    pub null: Option<String>,
    #[serde(default)]
    pub null_n: u32,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct FdrConfig {
    #[serde(default = "default_fdr_family")]
    pub family: String,
    #[serde(default = "default_alpha")]
    pub alpha: f64,
}
fn default_fdr_family() -> String { "scan_id".to_string() }
fn default_alpha() -> f64 { 0.05 }

#[derive(Debug, Clone, Deserialize)]
pub struct JobBlock {
    pub scan: String,
    pub instruments: serde_json::Value,  // flat OR nested array
    pub timeframes: Vec<String>,
    pub windows: Vec<String>,
    #[serde(default)]
    pub gap_policy: Option<String>,
    #[serde(default)]
    pub params: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    pub hygiene: Option<HygieneBlock>,
}

pub fn read_manifest(path: &std::path::Path) -> Result<SweepManifest, MinerError> {
    let s = std::fs::read_to_string(path).map_err(MinerError::Io)?;
    let manifest: SweepManifest = toml::from_str(&s)
        .map_err(|e| MinerError::Preflight(WireError::preflight(
            PreflightCode::InvalidParameter,
            format!("TOML parse error: {e}"),
        )))?;
    Ok(manifest)
}
```

**BTreeMap discipline pin** — `params: BTreeMap<String, serde_json::Value>` NEVER `HashMap`. Same OUT-03 rule applied throughout findings/mod.rs.

---

### `sweep/job_graph.rs` (cartesian expansion)

**Analog:** `crates/miner-core/src/engine/gap_policy.rs` (sub-range partitioning — pure function from input → `Vec<Range>`)

**Cartesian expand function pattern**:
```rust
/// A fully-resolved single-job specification produced by `expand()`. Every
/// field is a scalar (no axis-arrays remain after expansion).
#[derive(Debug, Clone)]
pub struct ResolvedJob {
    pub scan_id_at_version: String,
    pub instruments: Vec<InstrumentSpec>,
    pub timeframe: Timeframe,
    pub window: ClosedRangeUtc,
    pub gap_policy: GapPolicyKind,
    pub resolved_params: serde_json::Value,
    pub param_hash: Blake3Hex,
    pub job_seed: u64,
    pub bootstrap: Option<BootstrapSpec>,
    pub null: Option<NullSpec>,
}

/// Expand the manifest into a deterministic-order `Vec<ResolvedJob>`.
///
/// Iteration order (D5-01):
/// 1. `[[jobs]]` block declaration order.
/// 2. Within a block: instruments → timeframes → windows → params alphabetical.
pub fn expand(
    manifest: &SweepManifest,
    registry: &Registry,
) -> Result<Vec<ResolvedJob>, MinerError> {
    let mut jobs = Vec::new();
    for block in &manifest.jobs {
        let (scan_id, version) = preflight::resolve_scan_id_at_version(&block.scan)?;
        let scan = registry.get(&scan_id, version).ok_or_else(|| ...)?;
        let arity = scan.arity();
        // Parse instruments per arity (flat vs nested).
        let instruments_grid: Vec<Vec<InstrumentSpec>> =
            parse_instruments_grid(&block.instruments, arity)?;
        for instruments in instruments_grid {
            for tf_str in &block.timeframes {
                let tf = Timeframe::from_str(tf_str)?;
                for window_str in &block.windows {
                    let window = parse_window(window_str)?;
                    for params in cartesian_params(&block.params) {
                        let param_hash = ...;
                        let job_seed = derive_job_seed(
                            master_seed, &block.scan, &instruments,
                            tf, &window, param_hash.as_str()
                        );
                        jobs.push(ResolvedJob { ... });
                    }
                }
            }
        }
    }
    Ok(jobs)
}
```

---

### `sweep/executor.rs` (rayon par_iter + buffered drain)

**Analog:** `crates/miner-core/src/engine/mod.rs::run_one_with_registry` (lines 227+)

**Imports pattern** (extend run_one_with_registry's imports with rayon):
```rust
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use rayon::prelude::*;
use crate::cache::BarCache;
use crate::config::MinerConfig;
use crate::error::MinerError;
use crate::findings::{Finding, FindingSink, ...};
use crate::engine::run_one_with_registry;
```

**Deterministic-order rayon fanout pattern** (RESEARCH §"Pattern 4" — concrete sketch lines 506-560):
```rust
pub fn run_sweep<R: Reader + Sync>(
    jobs: Vec<ResolvedJob>,
    cfg: &MinerConfig,
    reader: &R,
    cache: &BarCache,
    cancel: Arc<AtomicBool>,
    sink: &mut dyn FindingSink,
) -> Result<RunOutcome, MinerError> {
    // Phase 1: parallel execution into per-job buffers.
    let buffered: Vec<(usize, Vec<Finding>)> = jobs
        .par_iter()
        .enumerate()
        .map(|(idx, job)| {
            if cancel.load(Ordering::Relaxed) { return (idx, Vec::new()); }
            let mut buf = VecSink::new();
            let scan_req = job.to_scan_request();
            let _ = run_one_with_registry(
                &scan_req, cfg, reader, &mut buf,
                Arc::clone(&cancel), registry,
            );
            (idx, buf.into_inner())
        })
        .collect();

    // Phase 2: sequential, manifest-order drain (byte-identical re-run).
    let mut all_p_values_by_family: BTreeMap<String, Vec<(usize, f64)>> = BTreeMap::new();
    for (idx, findings) in &buffered {
        for finding in findings {
            if let Finding::Result(r) = finding {
                if let Some(p) = r.effect.p_value {
                    let family = scope_family(&r.scan_id_at_version, &manifest.fdr);
                    all_p_values_by_family.entry(family).or_default().push((*idx, p));
                }
            }
            sink.write_envelope(finding)?;
        }
    }

    // Phase 3: SIGINT short-circuit — no SweepSummary on cancel.
    if cancel.load(Ordering::Relaxed) {
        return Ok(RunOutcome::Ok);
    }

    // Phase 4: BH-FDR per family + SweepSummary.
    let summary = build_sweep_summary(all_p_values_by_family, manifest.fdr.alpha);
    sink.write_envelope(&Finding::SweepSummary(summary))?;

    Ok(RunOutcome::Ok)
}
```

**Cancel-poll site** — mirrors run_one_with_registry's three documented sites:
1. `cancel_at_entry` — top of `run_sweep` (before the par_iter).
2. `cancel_per_job` — inside each worker (top of the par_iter closure).
3. `cancel_before_summary` — between drain and SweepSummary emission.

Plan-phase pins these.

---

### `findings/mod.rs` (MODIFIED — additive)

**Analog:** self (lines 168-181 `Effect` struct, 242-263 `ResultFinding` struct, 361-370 `Finding` enum)

**Add `EffectSize` struct** (new — schema-additive Option field on existing Effect):
```rust
// Insert AFTER existing `Effect` struct definition (line 181):

/// Effect-size scalar paired with a `kind` discriminant (Phase 5 / D5-03).
///
/// `kind` is an open string ("cohens_d", "hedges_g", "cliffs_delta",
/// "vr_minus_one", or scan-specific) so adding scan-specific kinds is
/// additive and non-breaking. The pair (`kind`, `value`) is the unit consumers
/// pattern-match on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EffectSize {
    pub kind: String,
    pub value: f64,
}
```

**Modify `Effect`** (lines 168-181):
```rust
pub struct Effect {
    pub metric: String,
    pub value: f64,
    #[serde(default)]
    pub p_value: Option<f64>,
    #[serde(default)]
    pub n: Option<u64>,
    #[serde(default)]
    pub ci95: Option<[f64; 2]>,
    /// Phase 5 / D5-03 — typed `(kind, value)` pair (NOT parallel scalars,
    /// NOT inside `extra`). `#[serde(default)]` keeps the change additive.
    #[serde(default)]
    pub effect_size: Option<EffectSize>,
    #[serde(default)]
    pub extra: BTreeMap<String, RawArray>,
}
```

**Add `ReproEnvelope`, `BootstrapSpec`, `NullSpec`** (schema-additive structs, mirror the existing `Source` struct shape lines 109-114):
```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ReproEnvelope {
    pub master_seed: u64,
    pub job_seed: u64,
    #[serde(default)]
    pub bootstrap: Option<BootstrapSpec>,
    #[serde(default)]
    pub null: Option<NullSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct BootstrapSpec {
    pub method: String,  // "stationary" | "block"
    pub n: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct NullSpec {
    pub method: String,  // "phase_scramble" | "circular_shift"
    pub n: u32,
}
```

**Modify `ResultFinding`** (line 242-263 — add `repro: Option<ReproEnvelope>`):
```rust
pub struct ResultFinding {
    // ... existing locked + per-variant fields ...
    pub raw: Option<Raw>,
    /// Phase 5 / D5-05 — bit-for-bit reproducibility envelope. `Some(_)` when
    /// bootstrap OR null was run; `None` when neither. `#[serde(default)]`
    /// keeps the change additive.
    #[serde(default)]
    pub repro: Option<ReproEnvelope>,
}
```

**Add `SweepSummaryFinding` + supports** (mirror DryRunFinding lines 327-345):
```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SweepSummaryFinding {
    pub run_id: RunId,
    pub produced_at_utc: DateTime<Utc>,
    pub fdr_by_family: BTreeMap<String, FdrFamilySummary>,
    pub totals: SweepTotals,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FdrFamilySummary {
    pub method: String,  // "benjamini_hochberg"
    pub alpha: f64,
    pub per_finding: Vec<FindingFdrEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FindingFdrEntry {
    pub finding_index: u64,
    pub raw_p: f64,
    pub q_value: f64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SweepTotals {
    pub jobs_run: u64,
    pub results_emitted: u64,
    pub scan_errors: u64,
    pub gap_aborted: u64,
}
```

**Modify `Finding` enum** (line 361-370 — add SweepSummary variant):
```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Finding {
    RunStart(RunStart),
    Result(ResultFinding),
    ScanError(ScanErrorFinding),
    GapAborted(GapAbortedFinding),
    RunEnd(RunEnd),
    DryRun(DryRunFinding),
    SweepSummary(SweepSummaryFinding),  // NEW (Phase 5 / D5-02)
}
```

**Existing tests to extend** (test 8 lines 622-637 `all_variants_round_trip`):
- Add `Finding::SweepSummary(sample_sweep_summary())` to the array.
- Add `sample_sweep_summary()` fixture.
- Add `effect_size_round_trip` test (Effect → JSON → Effect with effect_size Some/None both round-trip).
- Add `repro_envelope_population_rule` test (Some iff bootstrap/null spec present).

---

### `scan/mod.rs` (MODIFIED — Scan trait extension)

**Analog:** self (lines 120-169 `Scan` trait, lines 73-105 `ScanArity` enum + `as_str`)

**Add `NullMethod` enum** (mirror `ScanArity` lines 73-105):
```rust
/// Null-distribution method declared support — Phase 5 D5-04 trait extension.
///
/// Pattern analog: `ScanArity` — same derives, same
/// `#[serde(rename_all = "snake_case")]`, sibling `as_str` method.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NullMethod {
    PhaseScramble,
    CircularShift,
}

impl NullMethod {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            NullMethod::PhaseScramble => "phase_scramble",
            NullMethod::CircularShift => "circular_shift",
        }
    }
}
```

**Add `supports_*` default-false trait methods** (inside `pub trait Scan: Send + Sync { ... }` — line 120):
```rust
    /// Whether this scan can produce a bootstrap CI on its primary statistic.
    /// Default false — only scans whose statistic is a smooth function of an
    /// autocorrelated series benefit. Plan-phase produces the per-scan opt-in
    /// matrix; the default-false discipline mirrors `ScanArity` (no implicit
    /// behaviour — every opt-in is explicit).
    fn supports_bootstrap(&self) -> bool { false }

    /// Whether this scan can produce a p-value under a given null method.
    /// Default false — only scans that already emit `p_value` benefit. Phase
    /// scramble is for autocorr / cointegration / lead-lag families; circular
    /// shift is broader. Plan-phase pins per-scan defaults.
    fn supports_null_method(&self, _m: NullMethod) -> bool { false }
```

**Object-safety regression test extension** (lines 505-510 — keep `scan_trait_object_safe` as-is; the new methods are `&self`, no generics, no `where Self: Sized` — object-safe by construction).

---

### `engine/mod.rs` (MODIFIED — post-scan hygiene hook + sweep entry)

**Analog:** self (lines 175-190 `run_one` wrapper, lines 227+ `run_one_with_registry`)

**Pattern**: extend `run_one_with_registry`'s sub-range loop to call hygiene kernels AFTER the base scan's `Scan::run` completes. The hygiene kernels read `req.bootstrap` / `req.null` / `req.master_seed` / `req.job_seed`, then mutate the in-flight `Effect` block (populate `effect_size`, `ci95`, replace `p_value`).

**Two extension points:**

1. **Inside `run_one_with_registry` sub-range loop** (~ existing line 380 area): after each scan emits its base `Finding::Result`, call hygiene kernels if `req.bootstrap.is_some()` or `req.null.is_some()`. The hygiene call replaces the original `effect` block via a buffered-write pattern: scan emits into a `VecSink`, engine intercepts the `ResultFinding`, mutates `effect`, then writes through to the real sink.

2. **Add `run_sweep` entry point** at module level (after `run_one_with_registry`):
   ```rust
   /// Phase 5 sweep entry point. Mirrors `run_one`'s wrapper-over-registry
   /// pattern: public `run_sweep` calls `run_sweep_with_registry` with the
   /// bootstrap registry, internal `run_sweep_with_registry` accepts an
   /// injected registry for tests.
   pub fn run_sweep<R: Reader + Sync>(
       jobs: Vec<ResolvedJob>,
       cfg: &MinerConfig,
       reader: &R,
       cache: &BarCache,
       sink: &mut dyn FindingSink,
       cancel: Arc<AtomicBool>,
   ) -> Result<RunOutcome, MinerError> {
       let registry = crate::scan::bootstrap();
       crate::sweep::executor::run_sweep_with_registry(jobs, cfg, reader, cache, sink, cancel, &registry)
   }
   ```

---

### `error/codes.rs` (MODIFIED — additive PreflightCode variant)

**Analog:** self (lines 22-44 `PreflightCode` enum, line 41 `SweepTooLarge` already shipped)

**Add `HygieneNotSupported` variant** (insert AFTER `SweepTooLarge`, line 41):
```rust
    /// Sweep cardinality exceeds a configured upper bound (Phase 5+).
    SweepTooLarge,
    /// Phase 5 / D5-04 — scan rejected `--bootstrap` or `--null` because
    /// it does not opt into the requested method via
    /// `Scan::supports_bootstrap()` / `Scan::supports_null_method()`.
    HygieneNotSupported,
    /// Catastrophic failure unrelated to inputs.
    InternalError,
```

**Extend `as_str` arms** (lines 49-60):
```rust
            PreflightCode::SweepTooLarge => "sweep_too_large",
            PreflightCode::HygieneNotSupported => "hygiene_not_supported",
            PreflightCode::InternalError => "internal_error",
```

**Extend `preflight_code_serialises_snake_case` test cases array** (lines 152-167):
```rust
            (PreflightCode::SweepTooLarge, "sweep_too_large"),
            (PreflightCode::HygieneNotSupported, "hygiene_not_supported"),
            (PreflightCode::InternalError, "internal_error"),
```

---

### `crates/miner-cli/src/sweep_args.rs` (NEW — clap-derive Args struct)

**Analog:** `crates/miner-cli/src/scan_args.rs` (entire file — 568 lines, the closest match by role + clap-derive surface)

**Imports pattern** (lines 41-48 of analog):
```rust
use std::path::PathBuf;
use clap::Args;
use miner_core::error::{PreflightCode, WireError};
use miner_core::sweep::manifest::{read_manifest, SweepManifest};
```

**Clap Args struct pattern** (lines 56-111 of analog):
```rust
/// `miner sweep` subcommand arguments.
///
/// Pattern: `scan_args.rs` (Plan 03-05 / 04-02 — clap `Args` derive + a
/// `to_*` conversion method). Single positional `manifest` path + `--dry-run`
/// + universal hygiene overrides (`--seed`, `--bootstrap`, `--bootstrap-n`,
/// `--null`, `--null-n`) that ALSO live in the scan_args surface so single-
/// shot `miner scan` and `miner sweep` share the contract.
#[derive(Debug, Args)]
pub struct SweepArgs {
    /// Positional path to a TOML sweep manifest.
    pub manifest: PathBuf,

    /// Emit one `Finding::DryRun` envelope with `planned_job_count` and exit 0.
    #[arg(long)]
    pub dry_run: bool,

    /// Master seed override (overrides `[sweep].seed` in the manifest if both
    /// are set).
    #[arg(long)]
    pub seed: Option<u64>,

    /// Override `[hygiene].bootstrap` ("stationary" | "block").
    #[arg(long)]
    pub bootstrap: Option<String>,

    #[arg(long, default_value = "0")]
    pub bootstrap_n: u32,

    /// Override `[hygiene].null` ("phase_scramble" | "circular_shift").
    #[arg(long)]
    pub null: Option<String>,

    #[arg(long, default_value = "0")]
    pub null_n: u32,
}
```

**`to_manifest` conversion** (mirror `to_scan_request` lines 134-200):
```rust
impl SweepArgs {
    /// Load the TOML manifest, apply CLI overrides (seed / hygiene flags
    /// override the manifest's defaults), and validate.
    pub fn to_manifest(&self) -> Result<SweepManifest, WireError> {
        let mut m = read_manifest(&self.manifest)?;
        if let Some(seed) = self.seed { m.sweep.seed = Some(seed); }
        if let Some(b) = &self.bootstrap { m.hygiene.bootstrap = Some(b.clone()); }
        if self.bootstrap_n > 0 { m.hygiene.bootstrap_n = self.bootstrap_n; }
        if let Some(n) = &self.null { m.hygiene.null = Some(n.clone()); }
        if self.null_n > 0 { m.hygiene.null_n = self.null_n; }
        Ok(m)
    }
}
```

---

### `crates/miner-cli/src/cli.rs` (MODIFIED — add Command::Sweep variant)

**Analog:** self (lines 56-75 — `Command` enum)

**Append `Sweep(SweepArgs)` variant** (after `Scans` line 74):
```rust
    /// List every registered scan, one JSONL line per scan ...
    Scans,

    /// Execute a TOML sweep manifest end-to-end (Phase 5 / OP-04).
    ///
    /// Streams `RunStart` → per-job `Result` envelopes → `SweepSummary` →
    /// `RunEnd`. Exit code routing identical to `Scan`: 0 = clean, 1 =
    /// preflight, 2 = mid-stream `ScanError`, 130 = SIGINT.
    Sweep(SweepArgs),
}
```

**Update import** (line 27):
```rust
use crate::scan_args::ScanArgs;
use crate::sweep_args::SweepArgs;
```

**Update `mod` declaration in `main.rs`** (line 43 of main.rs):
```rust
mod cli;
mod scan_args;
mod sweep_args;
```

---

### `crates/miner-cli/src/scan_args.rs` (MODIFIED — universal hygiene flags)

**Analog:** self (lines 56-111 `ScanArgs` struct)

**Append 5 universal `#[arg]` fields** (after `params` line 94):
```rust
    /// Phase 5 / D5-04 — universal hygiene flag.
    /// Bootstrap method: "stationary" | "block". Off by default.
    #[arg(long)]
    pub bootstrap: Option<String>,

    #[arg(long, default_value = "0")]
    pub bootstrap_n: u32,

    /// Phase 5 / D5-04 — universal hygiene flag.
    /// Null method: "phase_scramble" | "circular_shift". Off by default.
    #[arg(long)]
    pub null: Option<String>,

    #[arg(long, default_value = "0")]
    pub null_n: u32,

    /// Phase 5 / D5-05 — master seed for bootstrap + null reproducibility.
    /// When None, derived as `blake3(manifest_hash || run_id)`.
    #[arg(long)]
    pub seed: Option<u64>,
```

**Extend `to_scan_request`** (line 134-) to populate `req.bootstrap`, `req.null`, `req.master_seed` (new `ScanRequest` fields, plan-phase decides additive vs. carried via a side-channel struct).

---

### `crates/miner-core/tests/sweep_smoke.rs` (NEW integration test)

**Analog:** `crates/miner-core/tests/scan_ljung_box.rs` (entire file — golden-test pattern with mod common + BarFrame + ScanCtx fixtures)

**Test scaffolding pattern** (lines 28-44 of analog):
```rust
mod common;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use chrono::{Duration, TimeZone, Utc};

use miner_core::aggregator::{BarFrame, Timeframe};
use miner_core::config::{MinerConfig, OutputDest};
use miner_core::engine::gap_policy::GapPolicyKind;
use miner_core::engine::param_hash;
use miner_core::findings::{Finding, RunId, TimeRange};
use miner_core::reader::{ClosedRangeUtc, Side};
use miner_core::sweep::{SweepManifest, run_sweep};

use common::{BufferSink, synthetic_cache::SyntheticCache};
```

**Test body pattern**: load a small manifest (2 scans × 2 instruments × 1 tf × 1 window × 1 param-grid), call `run_sweep`, parse envelopes, assert: (1) one RunStart at front; (2) N=4 Result envelopes; (3) exactly one SweepSummary between last Result and RunEnd; (4) RunEnd at the end. Mirror the dry_run.rs assertion style (lines 87-137).

---

### `crates/miner-core/tests/sweep_dry_run.rs` (NEW integration test)

**Analog:** `crates/miner-core/tests/dry_run.rs` (entire file — EXACT same role: dry-run envelope count + shape assertion)

**Copy structure verbatim** (lines 36-137 of analog), replacing:
- `run_one(...)` with `run_sweep(...)`.
- `dry_run = true` on `ScanRequest` with `--dry-run` flag on `SweepArgs`.
- Assertion `findings.len() == 3` with assertion that the dry-run envelope's `planned_job_count == manifest.expanded.len()`.
- Banned-counter assertion (line 130 `concat!("\"dry_run_", "emitted\"")`) keeps verbatim.

---

### `crates/miner-core/tests/sweep_byte_identical_rerun.rs` (NEW integration test)

**Analog:** `crates/miner-core/tests/byte_identical_rerun.rs` (entire file — same role: run twice, mask, assert byte-equal)

**Run-twice helper pattern** (lines 112-147 of analog) — copy `run_single_arity_twice` and adapt to call `run_sweep` instead of `Scan::run` directly. The masking helpers in `common::parse_and_mask_jsonl` already mask `run_id` + `produced_at_utc`; Phase 5 may need to mask additional fields if `repro.master_seed` is also volatile (default-derived) — plan-phase pins.

**LCG seed pattern** (lines 73-83 of analog):
```rust
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
```

---

### `crates/miner-core/tests/sweep_summary_emission.rs` (NEW integration test)

**Analog:** `crates/miner-core/tests/scan_seas_anova_kruskal.rs` (lines 33-80) — same parametric envelope-shape assertion role.

**Test body pattern**: run a 2-job sweep, capture envelopes, assert:
- Exactly one `Finding::SweepSummary` envelope is emitted.
- It appears strictly AFTER the last `Finding::Result` and strictly BEFORE `Finding::RunEnd`.
- `fdr_by_family.len() == 2` (per `scan_id@version` default scope).
- Each `FdrFamilySummary.method == "benjamini_hochberg"`.
- `per_finding` is in stable index order.

---

### `crates/miner-core/tests/fdr_family_scoping.rs` (NEW integration test)

**Analog:** `crates/miner-core/tests/scan_seas_anova_kruskal.rs` (parametric coverage style — single test function, multiple assertions across enum variants)

**Parametric coverage pattern**: drive the same 4-job sweep through `[fdr].family = "scan_id" | "scan_family" | "all" | "none"` in turn, assert family-count + per-finding-grouping per variant.

---

### `crates/miner-core/tests/effect_size_emission.rs` (NEW integration test)

**Analog:** `crates/miner-core/tests/scan_ljung_box.rs` (envelope-shape assertion lines 559-602)

**Test body pattern**: run each of the 22 Phase 4 scans (one per scan) and assert `r.effect.effect_size.is_some()`. Per-scan: assert `effect.effect_size.unwrap().kind` matches the canonical kind from CONTEXT.md D5-03 table.

This is essentially a per-scan smoke test array — driver pattern same as the `byte_identical_rerun.rs` per-family iteration.

---

### `crates/miner-core/tests/bootstrap_block_length_golden.rs` (NEW integration test, GATED)

**Analog:** `crates/miner-core/tests/scan_ljung_box.rs` (entire file — same golden-fixture role + provenance gate)

**Provenance gate pattern** (lines 56-70 of analog):
```rust
const GOLDEN_JSON: &str = include_str!("fixtures/bootstrap_block_length_golden.json");

#[test]
#[ignore = "requires R 4.x + tseries::b.star; gated until provenance available"]
fn bootstrap_block_length_matches_r_tseries_golden() {
    let golden: serde_json::Value =
        serde_json::from_str(GOLDEN_JSON).expect("golden JSON valid");
    let prov_r = golden["provenance"]["r_version"].as_str();
    assert_eq!(
        prov_r,
        Some("4.4.0"),
        "golden provenance.r_version must be \"4.4.0\"; regenerate via \
         `Rscript crates/miner-core/tests/fixtures/generate_bootstrap_golden.R`",
    );
    // ... element-by-element comparison within 10% (per RESEARCH §1.7) ...
}
```

The `#[ignore]` attribute keeps the golden out of the default `cargo test` run until R + tseries provenance is set up. Match Phase 4's `tests/REFERENCE-VERSIONS.md` discipline.

---

### `crates/miner-cli/tests/sigint_mid_sweep.rs` (NEW CLI integration test)

**Analog:** `crates/miner-cli/tests/sigint_preserves_stream.rs` (entire file — same role: spawn binary, deliver SIGINT, assert exit 130 + envelope persistence)

**Binary-rebuild + SIGINT pattern** (lines 46-86 of analog `build_with_test_internal_feature`, `target_miner_path`):
```rust
fn build_with_test_internal_feature() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest.parent().unwrap().parent().unwrap();
    let status = Command::new(env!("CARGO"))
        .args(["build", "-p", "miner-cli", "--features", "test-internal", "--bin", "miner"])
        .current_dir(workspace_root)
        .status()
        .expect("cargo build");
    assert!(status.success());
}
```

**Spawn-kill-assert pattern** (lines 89-206 of analog):
```rust
#[test]
#[serial_test::serial]
fn sigint_mid_sweep_preserves_streamed_findings() {
    build_with_test_internal_feature();
    let bin = target_miner_path();
    // ... build a synthetic cache with multiple instruments ...
    // ... write a synthetic manifest TOML to a tempdir ...
    let mut child = Command::new(&bin)
        .env("MINER_CACHE_ROOT", cache.cache_root())
        .args(["sweep", manifest_path.to_str().unwrap(), "--sleep-after-first-finding-ms", "5000"])
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn");
    // ... wait until first Result envelope arrives, then deliver SIGINT ...
    kill(Pid::from_raw(child.id() as i32), Signal::SIGINT).unwrap();
    let status = child.wait().unwrap();
    assert_eq!(status.code(), Some(130), "SIGINT must yield exit 130");
    // ... assert: streamed Result + RunEnd persist; NO SweepSummary emitted (key
    //     differentiator from sweep_summary_emission.rs) ...
}
```

---

## Shared Patterns

### Pattern S1: Module-level documentation header

**Source:** `crates/miner-core/src/scan/ljung_box/mod.rs` lines 1-39

**Apply to:** every new `hygiene/*.rs` and `sweep/*.rs` module.

**Excerpt**:
```rust
//! `LjungBoxScan` — Phase 3 demo scan implementing the [`Scan`] trait.
//!
//! Pattern analog: `aggregator.rs::aggregate` — pure-kernel function calling a
//! `Reader`, returning a typed output. The Ljung-Box scan mirrors this shape
//! but reads from the brokering [`ScanCtx`] ...
//!
//! ## D3-01..D3-05 contract
//!
//! - `id = "stats.autocorr.ljung_box"`, `version = 1` (D3-01, D3-17).
//! - Log returns computed inline from `BarFrame.close` (D3-02; no ANOM-01
//!   pull-forward).
//! ...
//!
//! ## Algorithm (Plan 04 walk per gap.rs:158-186 analog)
//!
//! 1. Cancel-poll at entry (RESEARCH Pattern 4 site 1 — D3-22).
//! 2. Resolve `lags` from `req.resolved_params` ...
```

**Discipline**: every module-level doc cites (1) the closest existing pattern analog by filename + line range; (2) the decision IDs it implements; (3) the numbered algorithm walk if applicable.

---

### Pattern S2: BTreeMap discipline + `#[serde(default)]` on additive Option fields

**Source:** `crates/miner-core/src/findings/mod.rs` lines 129-133 (Raw struct), 168-181 (Effect struct)

**Apply to:** every new struct in `findings/mod.rs` and `sweep/manifest.rs`.

**Excerpt**:
```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Raw {
    /// `BTreeMap` (NEVER `HashMap`) for deterministic ordering — OUT-03.
    pub series: BTreeMap<String, RawArray>,
}

pub struct Effect {
    // ...
    #[serde(default)]
    pub p_value: Option<f64>,
    #[serde(default)]
    pub n: Option<u64>,
    #[serde(default)]
    pub ci95: Option<[f64; 2]>,
    /// `BTreeMap` (NEVER `HashMap`) for deterministic ordering — OUT-03.
    #[serde(default)]
    pub extra: BTreeMap<String, RawArray>,
}
```

**Discipline**:
- Every map on a Serialize path is `BTreeMap<K, V>`, NEVER `HashMap`.
- Every new optional field carries `#[serde(default)]` (keeps schema diff additive — Pitfall 8 in RESEARCH).
- The `dsr` / `fdr_q` rule (line 252-254 of findings/mod.rs) — DO NOT add `#[serde(skip_serializing_if = "Option::is_none")]`; absent fields serialise as JSON `null` (NOT omitted) per OUT-03.

---

### Pattern S3: ScanError thiserror enum + From-conversions

**Source:** `crates/miner-core/src/scan/mod.rs` lines 458-476 (`ScanError` thiserror enum)

**Apply to:** any new error types in `sweep::manifest`, `scan::hygiene::*`.

**Excerpt**:
```rust
#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error("scan kernel error: {0}")]
    Kernel(String),

    #[error("sink io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("scan cancelled")]
    Cancelled,

    #[error(transparent)]
    Miner(#[from] crate::error::MinerError),
}
```

**Discipline**: hygiene kernels return primitive `[f64; 2]` / `f64` / `Vec<f64>` directly (no `Result`); errors surface only at the engine call site as `MinerError::Scan(format!(...))` per Warning 8 (engine/mod.rs lines 13-26 module doc).

---

### Pattern S4: Cfg-gated test-internal hook (test reachability)

**Source:** `crates/miner-cli/src/scan_args.rs` lines 95-110 + `crates/miner-core/src/scan/mod.rs` lines 222-229

**Apply to:** `SweepArgs::sleep_after_first_finding_ms` (if Phase 5 needs SIGINT-during-sweep race control — mirror the existing scan_args.rs hook).

**Excerpt**:
```rust
    /// **Test-only Pitfall 8 hook** (Blocker 1 — Plan 03-06 SIGINT integration
    /// test ingress). When `Some(ms)`, `LjungBoxScan::run` performs a
    /// cancel-aware sleep loop ...
    #[cfg(any(test, feature = "test-internal"))]
    #[arg(long = "sleep-after-first-finding-ms", hide = true)]
    pub sleep_after_first_finding_ms: Option<u64>,
```

**Discipline**: `#[cfg(any(test, feature = "test-internal"))]` ALWAYS — `cfg(test)` for in-process unit tests, `feature = "test-internal"` for integration-test subprocess builds. NEVER reachable in release.

---

### Pattern S5: Cancel-poll cadence in inner loops

**Source:** `crates/miner-core/src/scan/seas/hour_of_day/kernel.rs` (`CANCEL_POLL_CADENCE = 4096`); `crates/miner-core/src/scan/ljung_box/mod.rs` lines 246-256 (cancel-aware sleep loop)

**Apply to:** `bootstrap.rs` inner resample loop (cadence N=64 per RESEARCH Pitfall 7); `null.rs` inner surrogate loop (same cadence).

**Excerpt** (LjungBox cancel-aware sleep, lines 247-256):
```rust
#[cfg(any(test, feature = "test-internal"))]
if let Some(total_ms) = ctx.sleep_after_first_finding_ms {
    let step = std::time::Duration::from_millis(10);
    let mut remaining = std::time::Duration::from_millis(total_ms);
    while !ctx.cancel.load(Ordering::Relaxed) && !remaining.is_zero() {
        let pause = remaining.min(step);
        std::thread::sleep(pause);
        remaining = remaining.saturating_sub(pause);
    }
}
```

**Discipline**: cancel polls between resamples (every N iterations), NEVER inside the inner Rng-call (would slow the kernel materially). Cadence is plan-phase-pinned; recommended N=64 for bootstrap and N=64 for null.

---

### Pattern S6: Test-fixture LCG closes + deterministic BarFrame

**Source:** `crates/miner-core/src/scan/ljung_box/mod.rs` lines 358-397 (`ar1_bar_frame_seeded`); `crates/miner-core/tests/byte_identical_rerun.rs` lines 73-108 (`lcg_closes` + `build_bars`)

**Apply to:** every new integration test in `tests/sweep_*.rs`.

**Excerpt**:
```rust
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

fn build_bars(symbol: &str, n: usize, closes: &[f64]) -> BarFrame {
    let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let ts_open: Vec<chrono::DateTime<Utc>> = (0..n)
        .map(|i| start + Duration::minutes(15 * i64::try_from(i).unwrap()))
        .collect();
    let ts_close: Vec<chrono::DateTime<Utc>> =
        ts_open.iter().map(|t| *t + Duration::minutes(15)).collect();
    BarFrame {
        source_id: "dukascopy".into(),
        symbol: symbol.into(),
        side: Side::Bid,
        tf: Timeframe::Tf15m,
        ts_open_utc: ts_open,
        ts_close_utc: ts_close,
        open: closes.to_vec(),
        high: closes.iter().map(|c| c + 0.001).collect(),
        low: closes.iter().map(|c| c - 0.001).collect(),
        close: closes.to_vec(),
        tick_volume: vec![1.0; n],
    }
}
```

**Discipline**: deterministic LCG (Numerical Recipes constants `a = 1664525, c = 1013904223, m = 2^32`); same seed → same bytes across re-runs.

---

### Pattern S7: `BufferSink` + `parse_findings` integration test plumbing

**Source:** `crates/miner-core/tests/common/` (referenced by every existing `scan_*.rs` integration test)

**Apply to:** every new `tests/sweep_*.rs` integration test.

**Excerpt** (typical usage from `scan_ljung_box.rs` lines 137-141):
```rust
let mut sink = BufferSink::new();
LjungBoxScan.run(&ctx, &req, &mut sink).expect("LjungBoxScan::run ok");
let findings = common::parse_findings(&sink.0);
assert_eq!(findings.len(), 1, "exactly one envelope emitted");
```

**Discipline**: every integration test uses `BufferSink` (in `tests/common/mod.rs`) — never `StdoutSink` (would pollute test output). Always parse via `common::parse_findings(&sink.0)`.

---

### Pattern S8: Insta snapshot + volatile-field masking

**Source:** `crates/miner-core/tests/scan_ljung_box.rs` lines 187-192 (insta snapshot); `crates/miner-core/tests/common/mod.rs` (`mask_volatile_fields`)

**Apply to:** `tests/sweep_summary_emission.rs` for the SweepSummary envelope shape snapshot.

**Excerpt**:
```rust
let mut masked = serde_json::to_value(&findings[0]).expect("envelope -> Value");
common::mask_volatile_fields(&mut masked);
insta::assert_json_snapshot!("sweep_summary_envelope_shape", masked);
```

**Discipline**: mask `run_id` and `produced_at_utc` BEFORE the snapshot assertion. Plan 5 may need additional masking for `repro.master_seed` if it's blake3-derived rather than user-supplied.

---

## No Analog Found

**None.** Every Phase 5 file has a clear Phase 1-4 precedent in the codebase. The "novelty" of Phase 5 is in the *kernel mathematics* (stationary bootstrap, IAAFT, BH-FDR, Cohen's d) — but the *file layout*, *trait extension*, *envelope-additive*, *integration-test*, *clap-derive*, and *cancel-poll* patterns all have exact analogs from Phase 3-4.

---

## Cross-Cutting Analogs to Watch For

| Topic | Authoritative analog file |
|-------|---------------------------|
| `serde(default)` on every new Option field | `findings/mod.rs:88` (`gap_manifest: Option<GapManifest>`) |
| `BTreeMap` keyword search | `findings/mod.rs:101-103, 132, 178-180, 224` |
| `#[serde(rename_all = "snake_case")]` on enums | `scan/mod.rs:74, 460`; `error/codes.rs:21, 67` |
| `JsonSchema` derive on every Serialize struct | `findings/mod.rs` — every struct |
| `Send + Sync` on traits used in rayon | `scan/mod.rs:120` |
| `Arc<AtomicBool>` cancellation handoff | `scan/mod.rs:219-221`; `engine/mod.rs:179-188` |
| Schema-additive regen (`cargo xtask gen-schema`) | `xtask/` crate (referenced by main Cargo.toml workspace members) |
| Clippy `disallowed_macros` (no println/eprintln) | Phase 4 D-15 — `clippy.toml` at workspace root |
| `RunSummary` exhaustive destructure (Warning 9) | `findings/mod.rs:801-818` — pin via `run_summary_has_no_dry_run_emitted_field` test |

## Metadata

**Analog search scope:**
- `crates/miner-core/src/scan/` (all 5 sub-trees: anom, cross, seas, ljung_box, primitives)
- `crates/miner-core/src/engine/` (mod.rs, preflight.rs, gap_policy.rs, param_hash.rs, framing.rs)
- `crates/miner-core/src/findings/mod.rs`
- `crates/miner-core/src/error/codes.rs`
- `crates/miner-core/src/scan/registry.rs`
- `crates/miner-core/tests/` (43 integration tests)
- `crates/miner-cli/src/` (cli.rs, main.rs, scan_args.rs)
- `crates/miner-cli/tests/` (6 integration tests)
- Workspace `Cargo.toml`

**Files scanned for analogs:** 24 (every file directly named in RESEARCH §5.6 "Wave 0 Gaps")
**Pattern extraction date:** 2026-05-20
**Phase 5 plan-phase consumer:** `gsd-planner` — reads this file alongside CONTEXT + RESEARCH to populate per-task Action sections with concrete line-range citations.
