---
phase: 05-statistical-hygiene-sweep-runner
reviewed: 2026-05-21T00:00:00Z
depth: standard
files_reviewed: 24
files_reviewed_list:
  - crates/miner-core/src/scan/hygiene/bootstrap.rs
  - crates/miner-core/src/scan/hygiene/effect_size.rs
  - crates/miner-core/src/scan/hygiene/fdr.rs
  - crates/miner-core/src/scan/hygiene/mod.rs
  - crates/miner-core/src/scan/hygiene/null.rs
  - crates/miner-core/src/scan/hygiene/seed.rs
  - crates/miner-core/src/engine/hygiene_buffering_sink.rs
  - crates/miner-core/src/engine/hygiene_dispatch.rs
  - crates/miner-core/src/engine/mod.rs
  - crates/miner-core/src/engine/preflight.rs
  - crates/miner-core/src/scan/mod.rs
  - crates/miner-core/src/findings/mod.rs
  - crates/miner-core/src/error/codes.rs
  - crates/miner-core/src/sweep/executor.rs
  - crates/miner-core/src/sweep/job_graph.rs
  - crates/miner-core/src/sweep/manifest.rs
  - crates/miner-core/src/sweep/mod.rs
  - crates/miner-core/src/lib.rs
  - crates/miner-cli/src/cli.rs
  - crates/miner-cli/src/main.rs
  - crates/miner-cli/src/scan_args.rs
  - crates/miner-cli/src/sweep_args.rs
  - xtask/src/main.rs
  - crates/miner-core/src/scan/seas/bucketing.rs
findings:
  critical: 3
  warning: 9
  info: 4
  total: 16
status: issues_found
---

# Phase 5: Code Review Report

**Reviewed:** 2026-05-21
**Depth:** standard
**Files Reviewed:** 24
**Status:** issues_found

## Summary

The Phase 5 implementation (hand-rolled hygiene kernels, engine buffering-sink wiring, sweep runner with TOML manifest deserialisation, CLI surface, and xtask schema generation) is broadly well-structured, with good documentation, deterministic-iteration discipline, and consistent use of typed errors. However, adversarial review surfaces:

1. A **mathematically incorrect** Politis-White / Patton-Politis-White block-length selector that collapses to a data-independent constant, defeating the purpose of automatic block-length selection.
2. An **empirical-p formula** that can produce exactly `p = 0`, breaching the conventional `(B+1)/(N+1)` discipline and leaking infinite-confidence claims into the downstream BH-FDR step.
3. A **silent NaN-contamination path** into `bh_fdr` aggregation: NaN p-values from analytic kernels (or surrogates) sort under `partial_cmp.unwrap_or(Equal)`, producing arbitrary q-values without any diagnostic.

Plus six other Warning-tier issues (signed-vs-two-sided statistic mismatch, unvalidated `alpha`, swallowed per-job preflight errors, multi-second uninterruptible kernel windows, etc.) and four Info items.

## Critical Issues

### CR-01: `block_length_pwppw` is data-independent — algebraically collapses to a constant

**File:** `crates/miner-core/src/scan/hygiene/bootstrap.rs:247-257`
**Issue:** The function is documented as computing the Politis-White (2004) + Patton-Politis-White (2009) automatic block-length selector for the stationary bootstrap. The published formula is

```
b_opt = (2 * G_hat^2 / D_hat)^(1/3) * n^(1/3)
```

where `G_hat` and `D_hat` are distinct quantities estimated from the data. However the implementation sets

```rust
let d_hat = 4.0 / 3.0 * g_hat * g_hat;       // line 253
(2.0 * g_hat * g_hat / d_hat).powf(1.0 / 3.0) * n_f.powf(1.0 / 3.0)
```

which algebraically reduces to `(2 * 3 / 4)^(1/3) * n^(1/3) = (3/2)^(1/3) * n^(1/3) ≈ 1.1447 * n^(1/3)`. The `g_hat` term — and therefore the entire autocorrelation structure of the input — cancels out. For any series length `n`, the function always returns the same value modulo `n^(1/3)`.

The unit test `block_length_pwppw_iid_sane` (line 396-403) only asserts the output is in `1..=50` for `n = 1000`, which the constant `1.1447 * 10 ≈ 11.45` trivially satisfies — so the test is not detecting the bug.

The correct PW (2004) form is `G_hat = sum_k lambda(k/(2m)) * |k| * R(k)` (note the `|k|` factor, missing in the code) and `D_SB = 2 * (sum_k lambda(k/(2m)) * R(k))^2` (a separate quantity from `G_hat`). The current code conflates the two.

Downstream impact: the engine uses `block_length_pwppw` to size the block length for both `stationary_bootstrap_ci` and `block_bootstrap_ci` (`engine/mod.rs:1160-1182`, `1300-1322`). With a data-independent block length, the resulting CIs are systematically miscalibrated on highly autocorrelated series (the published consumer use case for this selector). The user-facing `Effect.ci95` ships an incorrect coverage probability without any diagnostic.

**Fix:** Reimplement against the published PPW (2009) formula. A minimal correction:
```rust
// Compute G_hat with the missing |k| factor.
let mut g_hat = 0.0_f64;
for k in 1..=two_m {
    let r_k = if k < r.len() { r[k] } else { 0.0 };
    let lambda = (1.0 - k as f64 / two_m_f).max(0.0);
    g_hat += lambda * (k as f64) * r_k;
}
// Compute D_SB separately (NOT in terms of G_hat).
let mut g_dr = 0.0_f64;
for k in 0..=two_m {
    let r_k = if k < r.len() { r[k] } else { 0.0 };
    let lambda = if k == 0 { 1.0 } else { (1.0 - k as f64 / two_m_f).max(0.0) };
    g_dr += lambda * r_k;
}
let d_hat = 2.0 * g_dr * g_dr;  // D_SB
if d_hat == 0.0 { return f64::NAN; }
(2.0 * g_hat * g_hat / d_hat).powf(1.0 / 3.0) * n_f.powf(1.0 / 3.0)
```
Add a regression test pinning the output for a known-autocorrelated AR(1) series (e.g., `phi = 0.5`) where the expected block length differs measurably from `1.1447 * n^(1/3)`.

### CR-02: Empirical p-value can be exactly `0.0` — breaches `(B+1)/(N+1)` convention

**File:** `crates/miner-core/src/scan/hygiene/null.rs:76`
**File:** `crates/miner-core/src/engine/hygiene_dispatch.rs:459`
**Issue:** Both `circular_shift_null_p` and `pair_circular_shift_null_p` compute the empirical p-value as

```rust
f64::from(more_extreme) / f64::from(n_resamples)
```

When zero surrogates exceed the observed statistic, this returns exactly `0.0`. A p-value of `0.0` is mathematically untenable (it implies infinite log-odds against the null). The textbook surrogate-data correction (Davison & Hinkley 1997 §4.2, Theiler & Prichard 1996 — referenced in the module's own doc comments) is `p = (1 + more_extreme) / (1 + n_resamples)`, which floors `p` at `1/(n+1)` and avoids the singularity.

Downstream impact: `p = 0.0` flows through `apply_hygiene_mutations` (`engine/mod.rs:1243`) — the `(0.0..=1.0).contains(&p)` check ACCEPTS zero — and lands in `effect.p_value`. The sweep runner's `bh_fdr` then sees `p = 0.0` and produces `q = 0.0` for that finding, regardless of how many other tests were performed. Multiple-testing inflation is severely understated.

The test `circular_shift_null_p_uniform_under_null` (line 113-138) only checks the mean p-value is near 0.5 across 50 trials — it does NOT pin the floor behavior.

**Fix:** Replace both call sites with the conventional `(1 + B) / (1 + N)` form:
```rust
(f64::from(more_extreme) + 1.0) / (f64::from(n_resamples) + 1.0)
```
Add a regression test: for an observed statistic strictly more extreme than every surrogate, `p` should equal `1.0/(n_resamples + 1.0)`, NOT `0.0`. Document the convention in the module-level doc comment so future maintainers don't "fix" it back to the naive form.

### CR-03: `bh_fdr` silently corrupts q-values when input p-vector contains NaN

**File:** `crates/miner-core/src/scan/hygiene/fdr.rs:50`
**File:** `crates/miner-core/src/sweep/executor.rs:311`
**Issue:** `bh_fdr` sorts the input p-vector via `partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal)`. For NaN entries `partial_cmp` returns `None`, and the fallback `Equal` puts NaN values in arbitrary positions in the sorted vector — making both the rank assignment AND the running-min step-up walk produce nonsensical q-values for ALL entries, not just the NaN ones.

NaN p-values can reach `bh_fdr` legitimately through two paths:
1. An analytic scan kernel that returns NaN for `effect.p_value` on a degenerate input (e.g., constant-variance bucket → t-stat NaN → p-value NaN). The executor's drain loop captures these via `if let Some(p) = r.effect.p_value` (`executor.rs:311`) without testing `p.is_nan()`.
2. The hygiene engine's `apply_hygiene_mutations` mutator at `engine/mod.rs:1243` rejects NaN replacement (`if p.is_finite() && (0.0..=1.0).contains(&p)`) — but does NOT clear the pre-existing `effect.p_value`. So an analytic-NaN survives and feeds `bh_fdr`.

Downstream impact: a single NaN p-value silently invalidates the q-value computation for every other finding in its family. The `Finding::SweepSummary.fdr_by_family` is shipped with arbitrary q-values; the consumer has no way to detect this corruption.

**Fix:** Two complementary changes:
1. In `bh_fdr` reject (or filter) NaN inputs explicitly:
```rust
debug_assert!(p_values.iter().all(|p| p.is_finite() || p.is_nan()), ...);
let mut indexed: Vec<(usize, f64)> = p_values
    .iter()
    .copied()
    .enumerate()
    .filter(|(_, p)| !p.is_nan())
    .collect();
// Emit NaN q-values for NaN p inputs after sort/walk completes.
```
2. In `executor.rs:311`, skip NaN p-values from family aggregation (and document the decision in `SweepSummaryFinding.fdr_by_family` consumer notes):
```rust
if let Some(p) = r.effect.p_value {
    if !p.is_nan() {
        // ... existing family-key + entries.push logic
    }
}
```
Add a `bh_fdr_with_nan_p_preserves_finite_q_values` test.

## Warnings

### WR-01: Two-sided absolute-value comparison incorrect for one-sided test statistics

**File:** `crates/miner-core/src/scan/hygiene/null.rs:72`
**File:** `crates/miner-core/src/engine/hygiene_dispatch.rs:455`
**Issue:** `circular_shift_null_p` and `pair_circular_shift_null_p` count surrogates whose `abs()` is `>= observed.abs()` — a two-sided comparison. Several scan kernels wired into the dispatch table compute one-sided statistics:
- ADF `τ` (signed; large-negative is rejection; `engine/hygiene_dispatch.rs:614-641`)
- KPSS (one-sided chi-square-like; large positive is rejection)
- ARCH-LM (chi-square-like; large positive)
- Jarque-Bera (chi-square-like; large positive)
- Variance ratio (signed deviation from 1; `vr_minus_one` is symmetric, OK)
- Ljung-Box Q (one-sided chi-square)

For one-sided statistics whose null distribution is asymmetric (chi-square-like), `|surr| >= |obs|` underweights surrogate draws whose statistic is large in the conventional rejection direction, biasing the empirical p-value downward.

**Fix:** Either parameterise the kernel with a `tail: Tail::OneSidedPos | OneSidedNeg | TwoSided` enum (preferred — surfaces the choice at the dispatch site), or document that the universal two-sided rule sacrifices statistical exactness for surface uniformity (acceptable for v1 if explicit). Add per-scan tail-direction metadata in `hygiene_dispatch::stat_closure_for` so the kernel does the right thing per opt-in.

### WR-02: Per-job preflight `Err` is silently swallowed in the rayon fanout

**File:** `crates/miner-core/src/sweep/executor.rs:269`
**Issue:** Each rayon worker calls
```rust
let _ = run_one_with_registry(&scan_req, cfg, reader, &mut job_sink, ..., registry);
```
The returned `Result<RunOutcome, MinerError>` is discarded. `run_one_with_registry` returns `Err(MinerError::Preflight(_))` for unknown-scan, arity-mismatch, and hygiene-not-supported preflight failures — and CRITICALLY emits NO envelope before returning (see `engine/mod.rs:260-282`). So for any per-job preflight rejection at the engine layer, the `JobSink.buf` is empty AND nothing else surfaces to the user.

The sweep's own `manifest::validate` is intended to catch every preflight error upfront, but defence-in-depth fails here: any drift between `validate`'s checks and `run_one_with_registry`'s checks (e.g., a future-added preflight gate) becomes silent data loss in the sweep output.

**Fix:** Capture the per-job `MinerError::Preflight` and emit a synthetic `Finding::ScanError` envelope into the `JobSink.buf`:
```rust
if let Err(MinerError::Preflight(w)) = run_one_with_registry(...) {
    let synth = build_synthetic_scan_error(&scan_req, w);
    let _ = job_sink.write_envelope(&synth);
}
```
Add a regression test injecting a registry mismatch between `manifest::validate` and the per-job dispatch.

### WR-03: `[fdr].alpha` is not validated; out-of-range values silently flow to the wire

**File:** `crates/miner-core/src/sweep/manifest.rs:109`
**Issue:** `FdrConfig.alpha: f64` has only a `#[serde(default = "default_alpha")]` and no bounds validation. A user can supply `alpha = -1.0`, `alpha = 100.0`, or `alpha = NaN` in the TOML; the value passes through to `FdrFamilySummary.alpha` on the wire, and `bh_fdr`'s `debug_assert!((0.0..=1.0).contains(&alpha))` only fires under `cfg(debug_assertions)`. In release builds the bad alpha silently rides into consumer pipelines.

The `manifest::validate` function (lines 209-323) does not check `manifest.fdr.alpha`.

**Fix:** Add a validation step early in `manifest::validate`:
```rust
if !(0.0..=1.0).contains(&manifest.fdr.alpha) || manifest.fdr.alpha.is_nan() {
    return Err(MinerError::Preflight(WireError::preflight(
        PreflightCode::InvalidParameter,
        format!("[fdr].alpha must be in [0, 1]; got {}", manifest.fdr.alpha),
    )));
}
```
Mirror for `[hygiene].bootstrap_n` and `[hygiene].null_n` against the documented `HYGIENE_RESAMPLE_CEILING = 100_000` so over-cap requests reject at preflight rather than getting silently clamped at runtime.

### WR-04: Bootstrap kernels can run uninterruptibly for seconds; cancel discipline is too coarse

**File:** `crates/miner-core/src/scan/hygiene/bootstrap.rs:84-96` (and twins)
**Issue:** The module-level doc claims the kernels "always finish in milliseconds." For the documented `HYGIENE_RESAMPLE_CEILING = 100_000` and realistic input sizes (`n` of order 10⁴ bars), the inner loop is `100_000 * 10_000 = 10⁹` iterations per kernel call — practically multi-second on commodity hardware. The doc also explicitly rejects per-iteration cancel-flag polling.

With cancel-polling only happening BETWEEN kernel calls (engine outer loop), a SIGINT delivered mid-kernel produces a multi-second-long lag before the user sees a response. For interactive agent operation this defeats the cooperative-cancellation contract Plan 06 SIGINT integration tests presumably exercise.

**Fix:** Either (a) drop the ceiling to a value where worst-case kernel latency stays under ~100 ms (e.g., 10⁴), or (b) add a sparse cancel poll inside the resample loop — e.g., every 64 resamples — as
```rust
for resample in 0..n_resamples {
    if resample % 64 == 0 && cancel.load(Ordering::Relaxed) {
        // return NaN CI; engine treats as "no CI"
        return [f64::NAN, f64::NAN];
    }
    // ... existing body
}
```
This requires passing `&AtomicBool` into the kernel. The current "no cancel inside kernel" rationale (RESEARCH Pitfall 7) is reasonable for the millisecond-class kernels it was designed for, but doesn't survive the 100k cap.

### WR-05: `pair_block_bootstrap_ci` `n_resamples == 0` test exists but `block_len > n` is untested

**File:** `crates/miner-core/src/scan/hygiene/bootstrap.rs:149-158`
**Issue:** When `block_len > n`, the loop body never re-draws an index (`steps_in_block` never reaches `block_len` before `buf.len() >= n`). The resulting "resample" is a single circular window of the input — every bootstrap iteration produces a deterministic block selection at offset `idx`, undermining the bootstrap variance estimate. No edge-case test pins the behavior. The engine clamps `block_len >= 3` (`engine/mod.rs:1179`) but does not clamp the upper bound against `n`.

**Fix:** Clamp `block_len` at the kernel entry: `let block_len = block_len.min(n).max(1);`. Add an edge-case test asserting deterministic, sensible CI output (and `lo <= hi`) when `block_len == n` and `block_len > n`.

### WR-06: `n_resamples` saturating cast to `usize` is implicit and undocumented

**File:** `crates/miner-core/src/scan/hygiene/bootstrap.rs:80,142`
**File:** `crates/miner-core/src/engine/hygiene_dispatch.rs:326,384`
**Issue:** `let n_resamples_usize = n_resamples as usize;` performs an implicit u32→usize cast that is lossless on 64-bit targets but a silent truncation on 32-bit targets. The workspace targets 64-bit per the project's stated platform (`Linux x86_64`), but the `#[allow(clippy::cast_possible_truncation)]` attribute documents only the f64 percentile-index cast, not this one.

Combined with the missing `HYGIENE_RESAMPLE_CEILING` enforcement at the kernel boundary (only the engine clamps it; the kernel is `pub` and could be called externally), an internal-caller bug could supply `u32::MAX` and the kernel would attempt to allocate ~32GB for `boot_stats`.

**Fix:** Move the `HYGIENE_RESAMPLE_CEILING` enforcement into the kernel itself (defence-in-depth):
```rust
let n_resamples = n_resamples.min(HYGIENE_RESAMPLE_CEILING);
```
or document the precondition as `# Panics` / `# Errors` in the public doc-comment.

### WR-07: `derive_job_seed` instrument-spec serialisation is ambiguous

**File:** `crates/miner-core/src/scan/hygiene/seed.rs:71-73`
**Issue:** Instrument specs are hashed as `format!("{}:{}", spec.symbol, spec.side.as_str())`. The symbol field has no validation — a symbol containing a literal `:` character (e.g., `"FOO:BAR"`) would collide with a different `(symbol, side)` pair on the canonical-bytes wire. No delimiter is inserted between successive instruments either, so `[("EUR", Bid), ("USD", Ask)]` hashes the same bytes as `[("EUR:bidUSD", Ask)]` (no, actually not quite — but the principle stands for symbols containing `:`).

The `InstrumentSpec::from_str` parser in `reader::Side` likely rejects `:` in symbols, but the seed derivation does not — defence-in-depth fails.

**Fix:** Use a length-prefixed or null-byte-delimited encoding that cannot collide:
```rust
for spec in instruments {
    hasher.update(&(spec.symbol.len() as u64).to_le_bytes());
    hasher.update(spec.symbol.as_bytes());
    hasher.update(spec.side.as_str().as_bytes());
}
```
Update the module-level doc comment to lock the new canonicalisation. Bump the schema's `MINER_CODE_REVISION` so previously-pinned reference seeds don't silently change.

### WR-08: Cartesian expansion uses `usize` total reserve without saturating against `usize::MAX`

**File:** `crates/miner-core/src/sweep/job_graph.rs:432-433`
**Issue:** `cartesian_params` computes `let total: usize = choices.iter().map(Vec::len).product();` and then `out.reserve(total)`. The `product()` is unchecked — for a manifest with several axes of length 100, `total` can overflow `usize::MAX` on 64-bit (very large; unlikely) or 32-bit. Overflow wraps silently to a small value; `reserve` undersizes the Vec, the subsequent push loop reallocates many times.

Mitigated upstream by `estimated_job_count` + `SweepTooLarge` gate, but `params_cartesian_size` uses `saturating_mul`, while `cartesian_params` uses unchecked `product`. The two functions disagree on overflow handling; only the saturating one feeds the gate.

**Fix:** Mirror the saturating discipline in `cartesian_params`:
```rust
let total: usize = choices.iter().fold(1_usize, |acc, c| acc.saturating_mul(c.len()));
```
and add an upper sanity bound (e.g., abort if `total > 10_000_000`) since `cartesian_params` is also invoked by callers that bypass the sweep manifest's `max_jobs` gate.

### WR-09: SIGINT during sweep buffer-drain silently drops still-buffered Results

**File:** `crates/miner-core/src/sweep/executor.rs:339-342`
**Issue:** The cancel poll between the drain loop and `SweepSummary` emission (`if cancel.load(Ordering::Relaxed)`) correctly skips the `SweepSummary`. But: the drain loop itself (`executor.rs:303-331`) does NOT poll cancel. If SIGINT lands during the drain, the loop processes every buffered finding from every job before exiting — for a long sweep with large per-job buffers, this delays user-visible cancel response by tens of seconds or more.

Worse: if a network/disk sink fails mid-drain (`sink.write_envelope(&finding)?`), the `?` propagates the error, leaving subsequent buffered findings unwritten and the `RunEnd` envelope un-emitted — the JSONL stream is truncated mid-record.

**Fix:** Add a cancel poll at the top of the per-finding inner loop, and emit `RunEnd` even on sink-write error (mirroring the `engine::run_one_with_registry` framing-close discipline):
```rust
for (_idx, findings) in buffered {
    for finding in findings {
        if cancel.load(Ordering::Relaxed) { break 'outer; }
        // ... sink.write_envelope (with structured error handling)
    }
}
```

## Info

### IN-01: Redundant `(n_a + n_b) < 3` check in `cohens_d`

**File:** `crates/miner-core/src/scan/hygiene/effect_size.rs:66`
**Issue:** The guard `if n_a < 2 || n_b < 2 || (n_a + n_b) < 3` — the third clause is unreachable since `n_a >= 2 && n_b >= 2 ⇒ n_a + n_b >= 4`. Dead condition.
**Fix:** Drop the third clause; the comment "pooled df = n_a + n_b - 2 must be >= 1" is already implied by the first two.

### IN-02: `clamp_resample_n` rewrites `0` to default but does not warn

**File:** `crates/miner-core/src/engine/mod.rs:1054-1058`
**Issue:** `let raw = if raw == 0 { HYGIENE_RESAMPLE_DEFAULT } else { raw };` silently substitutes `1000` for any caller passing `Some(0)`. The CLI's `--bootstrap-n 0` documentation says "0 leaves bootstrap_n as None"; the manifest `[hygiene].bootstrap_n = 0` is treated identically (`job_graph.rs:139-143`). The Option-to-clamped translation is consistent, but a user who deliberately writes `bootstrap_n = 0` (intending "no bootstrap") gets `1000` resamples instead — counter-intuitive.
**Fix:** Either reject `Some(0)` at preflight (and require the user to omit the flag), or emit a `tracing::warn!` when the clamp fires.

### IN-03: `JobSink::write_raw_json` silently no-ops

**File:** `crates/miner-core/src/sweep/executor.rs:117-121`
**Issue:** The per-job `JobSink` returns `Ok(())` from `write_raw_json` without preserving the input value. If a future scan body decides to bypass typed envelopes via `write_raw_json` (as `miner scans` does for catalogue lines), the data is lost without any signal.
**Fix:** Either return `Err(...)` with a "raw-json bypass not supported inside a sweep" message, or buffer the raw JSON alongside the typed envelopes (more invasive).

### IN-04: `block_length_pwppw` accepts `m == 0` by silently coercing to `1`

**File:** `crates/miner-core/src/scan/hygiene/bootstrap.rs:241-243`
**Issue:** When no autocorrelation lag exceeds the threshold, `m` stays at 0; the code clamps `if m == 0 { m = 1; }`. Combined with CR-01 above, the clamp masks the degenerate "no significant autocorrelation" signal — the function returns a finite `b_star` that the engine then applies as a block length, even though the data has no detectable autocorrelation structure and an IID bootstrap would be more appropriate.
**Fix:** Return `NaN` (or a sentinel) when `m == 0` so callers can fall back to IID bootstrapping rather than the constant `1.1447 * n^(1/3)`.

---

_Reviewed: 2026-05-21T00:00:00Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
