# Phase 5: Statistical Hygiene & Sweep Runner — Research

**Researched:** 2026-05-20
**Domain:** Rust statistical-hygiene kernels (effect sizes, block / stationary bootstrap, phase-scrambled and circular-shift nulls, BH-FDR) plus TOML-driven sweep manifest fanout layered over the verified Phase 3-4 facade.
**Confidence:** HIGH on stack and patterns (carry-over from verified Phases 1-4 — every numerical primitive Phase 5 needs is implementable on the existing `statrs` + `ndarray` + `nalgebra` + `blake3` triad with two additive deps); MEDIUM on the precise IAAFT iteration count + block-length selector default (literature pins the algorithm shape but the exact constants are a judgement call) and on `rand` cross-version stability (documented gotcha — see §1.5).

## Summary

Phase 5 is a *kernel scale-out* and a *fanout layer*, not a new architecture. Five envelope-shape extensions (`Finding::SweepSummary` variant, `Effect.effect_size` field, `ResultFinding.repro` field, two `PreflightCode` variants, two `Scan` trait methods) hang off the verified Phase 1-4 contract. The sweep runner is a rayon `par_iter` fanout over a cartesian-expanded TOML manifest with deterministic-order buffering — same `BarCache::get_or_build` per-job loop the Phase 3 single-shot facade already uses, just wrapped in an outer parallel iterator. Every statistical kernel (effect sizes, stationary bootstrap, IAAFT phase-scramble null, BH-FDR) is implementable as pure functions on `&[f64]` + `seed: u64`, mirroring the Phase 4 `mod.rs` + `kernel.rs` split that the 22 shipped scans already use.

Two new direct deps are required: **`rand_xoshiro = "0.6"`** (NOT `SmallRng` — see §1.5; `Xoshiro256PlusPlus` is the documented stable choice) and **`toml = "0.8"`** (bare deserialiser; figment's profile-layering is overkill for the single-source manifest). One *optional* dep (`realfft = "3"`) lands only if IAAFT phase-scramble is in scope at plan time — the trivial circular-shift null does not need FFT and lets Phase 5 ship if IAAFT slips. No off-the-shelf Rust crate exists for block / stationary bootstrap or for effect sizes — both are hand-rolled following the same `kernel.rs` discipline Phase 4 used for ADF, KPSS, ARCH-LM, and Lo-Mackinlay. **BH-FDR**: a third-party `adjustp` crate exists but is low-popularity (430 dl/mo, single-author); hand-rolling the ~25-line algorithm is consistent with the project's Phase 4 hand-roll-statistics discipline and avoids a fragile low-traffic dep — RECOMMEND hand-rolling.

**Primary recommendation:** Decompose Phase 5 into FIVE plans (envelope additions + trait extensions wave 0 → hygiene kernel implementations → sweep manifest deserialiser + job graph → rayon executor + deterministic-order drain + dry-run → integration tests + goldens + schema regen + sign-off). Add only two direct deps: `rand_xoshiro = "0.6"` and `toml = "0.8"`. Pin `Xoshiro256PlusPlus` as the deterministic RNG (NOT `SmallRng` — explicitly non-portable per `rust-random`'s own book). Hand-roll all statistical kernels including BH-FDR. Decision **D5-05** is fully confirmed by the literature on RNG stability; decisions D5-01, D5-02, D5-03, D5-04 close cleanly against statsmodels / Politis-Romano (1994) / Theiler (1992) / Benjamini-Hochberg (1995) references.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions (user-locked-by-default pragmatic defaults — plan-phase research confirms or refines)

The user accepted pragmatic defaults on all four gray areas during discuss-phase ("I'll accept pragmatic defaults on all of the above. Let's move to planning"). The defaults below carry forward; this research confirms each against the cited literature.

- **D5-01:** Sweep manifest is a TOML file with `[[jobs]]` array. Each job block declares `scan`, `instruments` (flat array for Single-arity scans, nested 2-element array for Pair-arity scans), `timeframes`, `windows`, and `[jobs.params]` table. Within each `[[jobs]]` block, cartesian product expansion is the fanout semantics. Multiple `[[jobs]]` blocks accumulate. Top-level `[sweep]` (master seed, max_jobs) and `[hygiene]` (bootstrap/null defaults) blocks apply unless per-job overrides override them. Preflight rejects on (a) arity mismatch, (b) job count > `[sweep].max_jobs`, (c) hygiene flag for unsupported scan. Job-graph deterministic ordering: `[[jobs]]` block declaration order, then cartesian iteration order across axes (instruments → timeframes → windows → params alphabetical).

- **D5-02:** BH-FDR adjustment scopes per `scan_id@version` by default. Q-values land in an end-of-sweep `Finding::SweepSummary` envelope variant, NOT in per-finding `fdr_q` (the Phase 1 reserved slot stays `null` during streaming). `[fdr].family` override accepts `"scan_id"` / `"scan_family"` / `"all"` / `"none"`. Per-finding `finding_index` is the position in the streaming JSONL output (zero-indexed across all `Result` envelopes for that scan_id); consumers join q-values via `(scan_id_at_version, finding_index)`. Single-shot `miner scan` does NOT emit `SweepSummary`.

- **D5-03:** Effect size lives in a new typed `Effect.effect_size: Option<EffectSize { kind: String, value: f64 }>` field — NOT parallel scalars, NOT inside `effect.extra`. Per-scan canonical `kind` values pinned by plan-phase against scipy / statsmodels conventions; the starting-position table in CONTEXT.md `<decisions>` D5-03 enumerates one canonical `kind` per scan in the catalogue. Schema-additive change to `Effect`.

- **D5-04:** Bootstrap + null are caller-opt-in via universal CLI / manifest flags (`--bootstrap stationary|block`, `--bootstrap-n N`, `--null phase_scramble|circular_shift`, `--null-n N`, `--seed N`). Both default OFF. Per-scan declared support via new `Scan::supports_bootstrap()` and `Scan::supports_null_method(NullMethod)` trait methods (defaults `false`). `PreflightCode::HygieneNotSupported` rejects scan + method mismatches at preflight. Hygiene kernels live in `miner_core::scan::hygiene` module.

- **D5-05:** Bit-for-bit reproducibility via a new `ResultFinding.repro: Option<ReproEnvelope>` field with derived per-job seeds. Master seed (user-supplied or blake3-derived from manifest_hash + run_id when omitted) plus per-job seed = `blake3(master_seed || scan_id_at_version || instruments_canonical || timeframe || window_canonical || param_hash)[0..8] as u64`. RNG choice: `rand::rngs::SmallRng::seed_from_u64(job_seed)` per CONTEXT — **plan-phase research must confirm cross-version stability or pin an alternative.** This research closes that open question: **RECOMMEND `rand_xoshiro::Xoshiro256PlusPlus` instead of `SmallRng`** (see §1.5 — `SmallRng` is explicitly NOT portable per the `rust-random` book).

### Claude's Discretion (plan-phase + this research close them)

- Block-length default for stationary / block bootstrap — *(§1.7 recommends Politis-White 2004 with the Patton-Politis-White 2009 correction; floor at `max(3, ceil(n^(1/3)))`)*.
- Per-scan `supports_bootstrap()` / `supports_null_method()` defaults — *(§2 table — full per-scan matrix)*.
- Phase scramble exact algorithm — *(§1.8 recommends IAAFT with 10 iterations / converge-on-rank-distance criterion)*.
- Circular shift random-offset distribution — *(§1.8: uniform on `[1, n-1]`; offset 0 is rejected because it is the identity transform)*.
- `SweepSummary` envelope schema final shape — *(§1.6 + §3.4 finalises field names and ordering)*.
- Sweep dry-run output shape — *(§1.4 recommends extending the existing `DryRunFinding` with `planned_job_count: Option<u64>` rather than introducing a new variant — less envelope churn)*.
- Sweep result emission ordering — *(§1.3 confirms deterministic-order via per-job buffered output + sequential drain)*.
- `rand::SmallRng` cross-version stability — *(§1.5 — **CRITICAL**: `SmallRng` is explicitly non-portable per the upstream Rand Book; pin `Xoshiro256PlusPlus` from `rand_xoshiro` instead)*.
- `PreflightCode::SweepTooLarge` default cap — *(§1.4 recommends `[sweep].max_jobs = 100_000` default with plan-phase memory-budget validation)*.
- `[hygiene]` block override resolution order — *(§1.4 confirms per-job > sweep-level > CLI default)*.
- Sweep `--dry-run` interaction with hygiene flags — *(§1.4 confirms dry-run echoes hygiene config in `repro` spec but does NOT execute resamples)*.
- Bootstrap CI confidence level — *(§1.7 recommends pinning 95% to match existing `Effect.ci95` field name; `--ci-level` exposure deferred to v2)*.
- TOML deserialisation crate — *(§1.2 confirms bare `toml = "0.8"` over `figment`)*.
- `hygiene/bootstrap.rs` kernel signature — *(§1.7 recommends generic-over-stat-closure form; matches Phase 4 hand-rolled kernel conventions)*.

### Deferred Ideas (OUT OF SCOPE for Phase 5)

- **Deflated Sharpe Ratio (DSR)** — REQUIREMENTS.md HYG-v2-01; `ResultFinding.dsr` stays reserved-null in v1.
- **"Top-N interesting findings" summary** — HYG-v2-02.
- **Memoised per-sweep intermediates / in-memory arena** — HYG-v2-03.
- **Side-channel raw-array storage** — HYG-v2-04.
- **PyO3 bindings** — PLAT-v2-01.
- **GARCH / EGARCH / Hamilton MS-AR / Bayesian online change-point / wavelet seasonality** — PLAT-v2-03..06.
- **Johansen cointegration** — SCAN-v2-01.
- **MCP + HTTP parity for sweep / bootstrap / null** — Phase 6.
- **Bench harness for sweep wall-clock + flamegraph profiling of bootstrap inner loops** — Phase 7.
- **Sweep manifest zip / aligned-axis expansion** (alternative to cartesian) — v2 if a use case demands.
- **Streaming BH-FDR variant** (compute q-values during streaming pass) — v2; reserved `ResultFinding.fdr_q` slot remains the hook.
- **Caller-supplied custom RNG seed sequences** beyond master-seed → derived-per-job model — v2.
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| OP-04 | TOML sweep manifest fanout via rayon | §1.1 + §1.2 + §1.3 + §1.4 — manifest deserialiser, cartesian expansion, deterministic-order rayon executor, `--dry-run` shape |
| HYG-01 | Effect-size scalar paired with every p-value | §1.6 + §2 — typed `EffectSize { kind, value }` per-scan table; hand-rolled Cohen's d / Hedges' g / Cliff's delta / VR-minus-one kernels |
| HYG-02 | BH-FDR adjustment at sweep level | §1.6 + §1.9 — hand-rolled `bh_fdr(p_values: &[f64], alpha: f64) -> Vec<f64>` kernel; per-`scan_id@version` family scope default |
| HYG-03 | Block / stationary (Politis-Romano) bootstrap CIs | §1.7 — hand-rolled `stationary_bootstrap_ci(values, stat, n_resamples, mean_block_len, seed) -> [f64; 2]` per Politis-Romano 1994 + Politis-White 2004 + Patton-Politis-White 2009 correction |
| HYG-04 | Phase-scrambled / circular-shift null distributions | §1.8 — hand-rolled IAAFT per Theiler 1992 (FFT via `realfft`) + trivial circular-shift null |
| HYG-05 | Bit-for-bit reproducible bootstrap / null from echoed seed | §1.5 — `Xoshiro256PlusPlus` (NOT `SmallRng`); blake3-derived per-job seed; `ReproEnvelope` echoed in every hygiene-touched finding |
</phase_requirements>

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| TOML sweep manifest deserialisation + validation | `miner-core::sweep::manifest` | `toml` crate + `serde::Deserialize` | Pure-data: TOML → typed struct tree; preflight validators (arity, max_jobs, hygiene support) live as `&SweepManifest -> Result<_, MinerError>` functions |
| Cartesian product expansion → resolved job vector | `miner-core::sweep::job_graph` | — | Pure function `expand(&SweepManifest) -> Vec<ResolvedJob>`; testable in isolation; emits jobs in deterministic order matching D5-01 |
| Sweep executor (rayon par_iter fanout + buffered drain) | `miner-core::sweep::executor` | `rayon` (already wired); `BarCache::get_or_build` per-job | One worker per job; each worker writes to a per-job `Vec<Finding>` buffer; main thread drains in manifest order to the shared `FindingSink` |
| Sweep `--dry-run` (planned job graph + count) | `miner-core::sweep::executor` (short-circuit) | Existing `DryRunFinding` extended with `Option<u64> planned_job_count` | Reuses Phase 3 envelope variant additively; no new envelope variant introduced for dry-run |
| `SweepSummary` finding envelope (end-of-sweep) | `miner-core::findings::{Finding, SweepSummaryFinding, FdrFamilySummary, FindingFdrEntry}` | — | New `Finding::SweepSummary(SweepSummaryFinding)` variant; emitted between last `Result` and `RunEnd`; carries `BTreeMap<String, FdrFamilySummary>` keyed by `scan_id@version` |
| Effect-size kernels (Cohen's d, Hedges' g, Cliff's delta, VR-minus-one, etc.) | `miner-core::scan::hygiene::effect_size` | Per-scan `kind`-mapping in each scan's `Scan::run` body | Pure functions on `&[f64]`; per-scan call site picks the canonical `kind` (D5-03 table) and assembles the `EffectSize { kind, value }` |
| Stationary / block bootstrap kernel | `miner-core::scan::hygiene::bootstrap` | `Xoshiro256PlusPlus` seeded from per-job `u64` | Pure function with generic-over-stat-closure signature: `fn stationary_bootstrap_ci<F: Fn(&[f64]) -> f64 + Sync>(values, stat, n_resamples, mean_block_len, seed, ci_level)`; sequential inner loop (D3-22 cancel polling every N resamples) |
| Phase-scramble + circular-shift null kernels | `miner-core::scan::hygiene::null` | `realfft` (optional, IAAFT only); `Xoshiro256PlusPlus` | IAAFT for phase-scramble (Theiler 1992 + 10-iter convergence); circular shift is `Vec<f64>` rotation by uniform offset `[1, n-1]` |
| BH-FDR adjustment kernel | `miner-core::scan::hygiene::fdr` | — | Pure function `fn bh_fdr(p_values: &[f64], alpha: f64) -> Vec<f64>`; ~25 LOC implementing Benjamini-Hochberg 1995 step-up procedure |
| Per-job seed derivation | `miner-core::scan::hygiene::seed` | `blake3` (already wired) | Pure function `derive_job_seed(master_seed, scan_id_at_version, instruments, timeframe, window, param_hash) -> u64`; same Blake3Hex convention used by Phase 2 `param_hash` |
| `Scan::supports_bootstrap()` + `supports_null_method()` trait extension | `miner-core::scan::Scan` (trait extension) | Per-scan opt-in override | Default-`false` methods preserve object-safety; each scan body opts in by overriding (per-scan defaults table in §2) |
| `Effect.effect_size` field population | Each scan's `Scan::run` body (per-scan kernel boundary) | — | Plan-phase pins the `kind` string per scan; the scan body computes the scalar and assembles the `EffectSize` struct in `ResultFinding.effect.effect_size` |
| Sweep cancellation (SIGINT preserves streamed findings) | `miner-core::sweep::executor` | Existing `Arc<AtomicBool>` cancel flag (D3-22) | Outer rayon loop polls cancel between jobs; inner bootstrap / null loops poll cancel every N=64 resamples; `SweepSummary` NOT emitted on cancel (exit 130 takes precedence) |
| Schema regeneration | `xtask::gen_schema` (existing) | `schemas/findings-v1.schema.json` artifact + new `schemas/sweep-manifest-v1.schema.json` | Phase 5's three envelope additions flow through the existing determinism pipeline; the sweep manifest schema is a new auxiliary artifact |

## Standard Stack

### Core (already present — confirmed from `crates/miner-core/Cargo.toml` and workspace Cargo.toml)

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `statrs` | 0.17 | Distribution CDFs (Normal, ChiSquared, T, F) | Already wired (Phase 3 LjungBox, Phase 4 ADF/KPSS/ARCH-LM/Jarque-Bera); used by Phase 5 for empirical CDF lookup if needed and as the canonical CDF surface [VERIFIED: workspace Cargo.toml line 73] |
| `ndarray` | 0.16 | N-dim numerical arrays, slice views | Already wired (Phase 4); used for windowed series operations inside bootstrap / null kernels [VERIFIED: workspace Cargo.toml line 81] |
| `ndarray-stats` | 0.6 | mean/var/quantile primitives | Already wired (Phase 4 ANOM-02); used for effect-size pooled-std denominators [VERIFIED: workspace Cargo.toml line 82] |
| `nalgebra` | 0.33 | Small fixed-size linear algebra | Already wired (Phase 4 CROSS-03); Phase 5 may use `Vector` for circulant-matrix construction in IAAFT but not required [VERIFIED: workspace Cargo.toml line 83] |
| `blake3` | 1 | Per-job seed derivation | Already wired (Phase 2 param_hash, Phase 3 D3-13); same convention reused for `derive_job_seed` [VERIFIED: workspace Cargo.toml line 49] |
| `serde` + `serde_json` | 1 | TOML manifest → typed struct, JSONL envelope ser/de | Locked workspace deps; `toml` crate deserialiser uses `serde::Deserialize` derives [VERIFIED: workspace Cargo.toml lines 37-38] |
| `schemars` | 1 (with `chrono04` feature) | `JsonSchema` derive for new envelope types | Already wired; Phase 5's `EffectSize`, `ReproEnvelope`, `BootstrapSpec`, `NullSpec`, `SweepSummaryFinding`, `FdrFamilySummary`, `FindingFdrEntry` all derive `JsonSchema` [VERIFIED: workspace Cargo.toml line 39] |
| `rayon` | (existing — implicit via workspace) | Parallel job fanout | CLAUDE.md TL;DR table HIGH-confidence pin; D-21 / FOUND-04 sync-only invariant compatible [CITED: CLAUDE.md TL;DR — Parallelism (CPU) row] |
| `chrono` | 0.4 | UTC datetime parsing for `windows` field | Already wired [VERIFIED: workspace Cargo.toml line 40] |

### Supporting (NEW — Phase 5 adds these to `miner-core/Cargo.toml`)

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `rand_xoshiro` | 0.6 | Deterministic, cross-version-stable RNG | **Bootstrap resampling + null permutation** — `Xoshiro256PlusPlus` seeded from per-job `u64`. **NOT `rand::rngs::SmallRng`** which is explicitly non-portable per the upstream `rust-random` book (§1.5) [CITED: docs.rs/rand_xoshiro + rust-random.github.io/book/crate-reprod.html] |
| `rand` | 0.8 | RNG trait surface (`Rng`, `SeedableRng`) | Standard trait surface that `rand_xoshiro::Xoshiro256PlusPlus` implements; needed for `Rng::gen_range` / sampling APIs [CITED: docs.rs/rand 0.8] |
| `toml` | 0.8 | Sweep manifest deserialisation | Standard Rust TOML deserialiser; used directly (not via figment) — figment's profile/env-layering is overkill for a single-source manifest [CITED: docs.rs/toml + crates.io/crates/toml] |

### Optional (Phase 5 may defer to a follow-up plan if IAAFT slips)

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `realfft` | 3.5 | Real-to-complex FFT for IAAFT phase-scramble null | Only required if Phase 5 ships IAAFT phase-scramble. Circular-shift null (HYG-04 first delivery) does NOT need FFT and is implementable in <30 LOC against `Vec<f64>`. If IAAFT is descoped from Phase 5 → Phase 7, this dep moves with it. Versioning: `realfft = "3"` (current stable line) [CITED: docs.rs/realfft + lib.rs/crates/realfft] |

**Installation:**

```bash
cargo add --manifest-path crates/miner-core/Cargo.toml rand@0.8 rand_xoshiro@0.6 toml@0.8
# Only if shipping IAAFT in Phase 5:
cargo add --manifest-path crates/miner-core/Cargo.toml realfft@3
```

After install, confirm `cargo tree -p miner-core | grep -E 'tokio|async-std'` returns nothing (FOUND-04 sync-only invariant). All four candidates are sync-only.

### Version verification

`cargo` is not on the executing path in this researcher's shell sandbox (`cargo: command not found`), so version pins below are sourced from authoritative docs:

| Crate | Pinned version | Source |
|-------|----------------|--------|
| `rand` | 0.8 (current stable line; 0.9 in development) | docs.rs/rand 0.8 — current Rand Book documents 0.8 as the stable line |
| `rand_xoshiro` | 0.6 | docs.rs/rand_xoshiro latest = 0.6.x |
| `toml` | 0.8 | docs.rs/toml + crates.io/crates/toml — 0.8 is the current major |
| `realfft` | 3.5 | docs.rs/realfft 3.5 (current latest in the 3.x line) |

Plan-phase MUST run `cargo add` to lock the precise patch versions and document them in `Cargo.lock` discipline (no version drift between researcher sandbox and developer machine).

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `rand_xoshiro::Xoshiro256PlusPlus` | `rand::rngs::SmallRng` | **REJECTED.** SmallRng is explicitly excluded from the Rust Rand Book's reproducibility guarantees: "StdRng and SmallRng are deliberately excluded since these types are not portable" and "may make value-breaking changes in any release." HYG-05 demands bit-for-bit reproducibility; using SmallRng would violate the contract on any minor `rand` bump. |
| `rand_xoshiro::Xoshiro256PlusPlus` | `rand_chacha::ChaCha8Rng` | ChaCha is cryptographically secure (overkill for resampling) and slower; both are portable. Pick Xoshiro for speed; ChaCha if cryptographic determinism is ever required (it isn't here). |
| `toml = "0.8"` (bare) | `figment` + `figment-toml` | Figment exists to layer multiple config sources (file → env → CLI). The sweep manifest is a SINGLE source. The layering is unused; the deserialiser is what matters. Bare `toml` is simpler. **CONFIRMS D5-01 + open-question #10.** |
| Hand-rolled BH-FDR | `adjustp` crate (BH + BY + Bonferroni) | `adjustp` v0.1.6 has 430 dl/mo, single author, no transitive backing (dep on `num-traits` only). The actual BH algorithm is ~25 LOC. Phase 4 hand-rolled far more complex statistical kernels (ADF AIC lag selection, KPSS Bartlett kernel, Lo-Mackinlay overlapping VR) — the discipline is established. **RECOMMEND hand-roll** to avoid a low-popularity dep with unclear maintenance commitment. |
| `realfft` | `rustfft` directly | `realfft` is a thin wrapper that exploits Hermitian symmetry of real-valued FFTs for a ~2× speedup. For IAAFT on financial series (typically O(10⁴-10⁶) bars), realfft is the right pick — see lib.rs benchmark notes. |
| `realfft` | Pure Rust DFT (no FFT crate) | A naive DFT is O(n²); financial bar counts (10⁴-10⁶) make this 10²-10⁴× too slow per IAAFT iteration. **Reject.** |
| Block bootstrap | Moving-block bootstrap (Künsch 1989) | Both are valid block-bootstrap variants. Politis-Romano stationary (1994) has a *random* block length (geometric distribution with mean `p`) and is the canonical pick when the user-facing flag is `--bootstrap stationary`. Plain block bootstrap (`--bootstrap block`) uses a FIXED block length — both are documented in Politis-Romano and both ship under D5-04. |
| IAAFT | Simple phase randomisation (Theiler 1992 §3.1) | Simple phase randomisation preserves the power spectrum but distorts the marginal distribution (e.g., heavy-tailed returns become Gaussian-ish). IAAFT iteratively projects to preserve BOTH the power spectrum AND the marginal distribution — crucial for financial return series whose heavy tails are part of the null hypothesis. Default to IAAFT; consider exposing simple phase randomisation as a `--null phase_scramble_simple` v2 escape hatch. |

## Package Legitimacy Audit

Phase 5 installs three Rust crates (plus one optional). Package legitimacy is verified via authoritative docs + CLAUDE.md stack pinning. `slopcheck` is a Python-ecosystem tool and is not directly applicable to Cargo dependencies, mirroring the Phase 4 approach.

| Package | Registry | Age | Downloads | Source Repo | Verification | Disposition |
|---------|----------|-----|-----------|-------------|--------------|-------------|
| `rand` | crates.io | 10+ yrs | very high (top-50 Rust crate) | github.com/rust-random/rand | Authoritative (CLAUDE.md TL;DR + The Rust Rand Book) | Approved |
| `rand_xoshiro` | crates.io | 6+ yrs | high | github.com/rust-random/rngs | Authoritative (rust-random org; companion to `rand`) | Approved |
| `toml` | crates.io | 9+ yrs | very high (top-50 Rust crate; transitively pulled by virtually every project) | github.com/toml-rs/toml | Authoritative (`toml-rs` org maintained alongside `cargo` itself) | Approved |
| `realfft` (optional) | crates.io | 5+ yrs | medium-high | github.com/HEnquist/realfft | Authoritative (HEnquist also maintains `rubato`, well-known audio crate; thin wrapper over `rustfft`) | Approved |
| `adjustp` (CONSIDERED, REJECTED) | crates.io | <2 yrs | low (430/mo) | github.com/noamteyssier/adjustp | Single-author; functional but low traffic — REJECT in favour of hand-roll | N/A — not installed |

**Packages removed due to slopcheck [SLOP] verdict:** none
**Packages flagged as suspicious [SUS]:** none

**Note on Python reference packages** (`statsmodels`, `scipy`, plus the new Phase 5 reference: R's `tseries::tsbootstrap`): These live in `tests/REFERENCE-VERSIONS.md` for golden-generation only; they are NOT runtime dependencies. Continue the Phase 4 stub-fixture-fallback discipline.

## Architecture Patterns

### System Architecture Diagram

```
CLI: miner sweep <manifest.toml> [--dry-run] [--seed N]
                  │
                  ▼
miner-cli::sweep_manifest::read_manifest(path) ──→ toml::from_str → SweepManifest
                  │ (typed deserialiser; serde derives)
                  ▼
miner-core::sweep::manifest::validate(&SweepManifest, &Registry) ──── preflight
   │   ├─ arity match per scan?
   │   ├─ hygiene support per scan (supports_bootstrap / supports_null_method)?
   │   └─ estimated_job_count <= [sweep].max_jobs?
   │
   │ no → WireError(InvalidParameter | HygieneNotSupported | SweepTooLarge) → exit 1
   ▼ yes
miner-core::sweep::job_graph::expand(&SweepManifest) -> Vec<ResolvedJob>
   │ (cartesian: instruments × timeframes × windows × params; deterministic order;
   │  each ResolvedJob carries its own resolved bootstrap_method / null_method /
   │  bootstrap_n / null_n / job_seed inherited from sweep > job overrides)
   ▼
Is --dry-run set?
   yes → emit one Finding::DryRun with planned_job_count = jobs.len() + a sample,
         skip the parallel section, jump to framing-close.
   no  ↓
miner-core::sweep::executor::run_sweep(jobs: Vec<ResolvedJob>, reader, cache, cancel, sink)
   │
   │ rayon::par_iter::<&[ResolvedJob]>:
   │     for each job in parallel:
   │         poll cancel; on cancel → return Vec::new()
   │         load BarFrame via BarCache::get_or_build (Phase 2)
   │         derive job_seed = blake3(master_seed || canonicalise(job))[0..8]
   │         construct ScanRequest + ScanCtx (reuse engine::run_one_with_registry path)
   │         engine::run_one_with_registry(ctx, ...) writes to a per-job Vec<Finding> buffer
   │         (NOT directly to FindingSink — buffering enables deterministic-order drain)
   │         hygiene kernels (bootstrap / null) invoked AFTER scan's base Effect is built,
   │         populating effect.effect_size + (optionally) effect.ci95 + effect.p_value
   │     return Vec<(job_index, Vec<Finding>)>
   │
   ▼
Drain buffers sequentially in manifest order to the shared FindingSink:
   for (job_index, findings) in collected.sorted_by(job_index):
       for finding in findings:
           sink.write_envelope(&finding)
   │
   ▼
Collect all p_values per family (default: per scan_id@version).
miner-core::scan::hygiene::fdr::bh_fdr(&p_values_per_family, alpha = 0.05) -> Vec<f64>
   │
   ▼
Build SweepSummaryFinding { run_id, fdr_by_family: BTreeMap<String, FdrFamilySummary> }
sink.write_envelope(&Finding::SweepSummary(...))
   │
   ▼
RunEnd framing (D-09 carry-forward) → flush → exit 0 / 2 / 130
```

### Recommended Project Structure

(Adds to existing `crates/miner-core/src/` — every other Phase 1-4 path stays unchanged.)

```
crates/miner-core/src/
├── scan/
│   ├── mod.rs                          # MODIFIED: + Scan::supports_bootstrap() / supports_null_method(NullMethod);
│   │                                   #            + NullMethod enum
│   ├── hygiene/                        # NEW
│   │   ├── mod.rs                      # re-exports of effect_size, bootstrap, null, fdr, seed
│   │   ├── effect_size.rs              # cohens_d, hedges_g, cliffs_delta, vr_minus_one, etc.
│   │   ├── bootstrap.rs                # stationary_bootstrap_ci, block_bootstrap_ci
│   │   ├── null.rs                     # phase_scramble_null_p (IAAFT), circular_shift_null_p
│   │   ├── fdr.rs                      # bh_fdr(p_values: &[f64], alpha: f64) -> Vec<f64>
│   │   └── seed.rs                     # derive_job_seed(master_seed, scan_id_at_version,
│   │                                   #                 instruments, timeframe, window, param_hash)
│   ├── anom/                           # MODIFIED: each scan's mod.rs body overrides
│   │                                   # supports_bootstrap() / supports_null_method() per §2 table
│   ├── cross/                          # MODIFIED: same as anom
│   └── seas/                           # MODIFIED: same as anom
│
├── sweep/                              # NEW
│   ├── mod.rs                          # re-exports SweepManifest, run_sweep
│   ├── manifest.rs                     # SweepManifest, JobBlock, HygieneBlock, SweepConfig,
│   │                                   # FdrConfig serde::Deserialize derives
│   ├── job_graph.rs                    # cartesian expansion: ResolvedJob struct + expand() function
│   └── executor.rs                     # rayon::par_iter fanout + deterministic-order drain
│
├── engine/
│   └── mod.rs                          # MODIFIED: run_one_with_registry extended to invoke hygiene
│                                       # kernels after the base scan's Effect is built;
│                                       # NEW run_sweep() entry point
│
├── findings/
│   └── mod.rs                          # MODIFIED: + Effect.effect_size: Option<EffectSize>;
│                                       #            + ResultFinding.repro: Option<ReproEnvelope>;
│                                       #            + Finding::SweepSummary(SweepSummaryFinding) variant;
│                                       #            + new structs EffectSize, ReproEnvelope, BootstrapSpec,
│                                       #              NullSpec, SweepSummaryFinding, FdrFamilySummary,
│                                       #              FindingFdrEntry
│
└── error/
    └── codes.rs                        # MODIFIED: + PreflightCode::HygieneNotSupported variant
                                        # (PreflightCode::SweepTooLarge already shipped in Phase 1)

crates/miner-cli/src/
├── sweep_args.rs                       # NEW: SweepArgs (clap derive) - manifest path, --dry-run,
│                                       # universal hygiene overrides
├── scan_args.rs                        # MODIFIED: + --bootstrap, --bootstrap-n, --null, --null-n, --seed
└── main.rs                             # MODIFIED: Command::Sweep(SweepArgs) variant added;
                                        # handle_sweep_subcommand() dispatch

schemas/                                # Plan-phase regenerates via `cargo xtask gen-schema`
├── findings-v1.schema.json             # MODIFIED (additive)
└── sweep-manifest-v1.schema.json       # NEW (optional companion artifact; mirrors SweepManifest types)
```

### Pattern 1: Hand-rolled statistical kernel split (mod.rs + kernel.rs)

**What:** Each kernel module exposes a public pure function plus a private impl module. The public function is the API consumed by `Scan::run` bodies; the private kernel module is the unit-test surface.

**When to use:** Every Phase 5 hygiene kernel — `effect_size`, `bootstrap`, `null`, `fdr`, `seed`.

**Example:**
```rust
// Source: extension of the Phase 4 LjungBoxScan + ANOM kernel pattern
// (see crates/miner-core/src/scan/ljung_box/kernel.rs and
//  crates/miner-core/src/scan/anom/adf/kernel.rs for the precedent)

// crates/miner-core/src/scan/hygiene/fdr.rs

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

    // (orig_index, p_value) pairs, sorted ascending by p.
    let mut indexed: Vec<(usize, f64)> = p_values.iter().copied().enumerate().collect();
    indexed.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

    // BH step-up: q[(i)] = min(1, p[(i)] * n / (i+1)) , then enforce monotone non-increasing
    // from the top: q_adj[i] = min(q[i], q[i+1], ..., q[n-1]) — implemented by a reverse scan.
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Known-answer test against R's `p.adjust(p, method="BH")` for the
    /// canonical 5-value example: [0.01, 0.02, 0.03, 0.04, 0.05].
    /// R output: [0.05, 0.05, 0.05, 0.05, 0.05].
    #[test]
    fn bh_fdr_canonical_5() {
        let p = [0.01, 0.02, 0.03, 0.04, 0.05];
        let q = bh_fdr(&p, 0.05);
        for v in &q {
            assert!((*v - 0.05).abs() < 1e-12, "expected 0.05, got {v}");
        }
    }

    /// Monotonicity property: q-values respect the same rank-order as p-values.
    /// proptest sweep would tighten this.
    #[test]
    fn bh_fdr_preserves_rank_order() {
        let p = [0.001, 0.5, 0.01, 0.04, 0.99];
        let q = bh_fdr(&p, 0.05);
        // p[0] < p[2] < p[3] < p[1] < p[4]  -> q must respect that order
        assert!(q[0] <= q[2]);
        assert!(q[2] <= q[3]);
        assert!(q[3] <= q[1]);
        assert!(q[1] <= q[4]);
    }
}
```

### Pattern 2: Generic-over-stat-closure bootstrap signature

**What:** The bootstrap kernel takes a `Fn(&[f64]) -> f64` closure so each calling scan provides its own statistic formula at the call site. Avoids per-stat enum dispatch and keeps the bootstrap implementation single-purpose.

**When to use:** `stationary_bootstrap_ci` and `block_bootstrap_ci`.

**Example:**
```rust
// crates/miner-core/src/scan/hygiene/bootstrap.rs
use rand::SeedableRng;
use rand::Rng;
use rand_xoshiro::Xoshiro256PlusPlus;

/// Politis-Romano (1994) stationary bootstrap CI on a scalar statistic of an
/// autocorrelated series.
///
/// `stat` is the statistic functional being CI'd (e.g., correlation, mean,
/// Sharpe ratio). `mean_block_len` is the expected block length under the
/// geometric distribution (Politis-White 2004 selector recommended; see §1.7).
/// `seed` propagates from the per-job derived seed (HYG-05); `ci_level` is the
/// two-sided confidence level (default 0.95; the field name `ci95` pins this
/// at 95% in v1).
///
/// Returns the percentile CI: `[quantile(boot_stats, (1 - ci_level)/2),
/// quantile(boot_stats, 1 - (1 - ci_level)/2)]`.
///
/// # Errors / edge cases
/// Returns `[NaN, NaN]` when `values.len() < 2` (insufficient data).
/// `n_resamples == 0` returns `[NaN, NaN]` as well.
/// Cancel polling: caller (engine::run_one_with_registry) holds the
/// `Arc<AtomicBool>`; this kernel is sync, so the caller wraps invocation in
/// a yield point. Inner-loop cancel polling can be added if profiling shows
/// the kernel runs longer than the SC-5b cancel-yield cadence (typically not
/// — bootstrap is fast).
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
    let p_continue: f64 = 1.0 / mean_block_len; // geometric param
    let mut boot_stats: Vec<f64> = Vec::with_capacity(n_resamples as usize);
    let mut buf: Vec<f64> = Vec::with_capacity(n);

    for _ in 0..n_resamples {
        buf.clear();
        let mut idx = rng.gen_range(0..n);
        while buf.len() < n {
            buf.push(values[idx]);
            // Geometric: with prob `p_continue` start a new block; else extend
            // current block by one (with wrap-around).
            if rng.gen::<f64>() < p_continue {
                idx = rng.gen_range(0..n);
            } else {
                idx = (idx + 1) % n;
            }
        }
        boot_stats.push(stat(&buf));
    }
    // Percentile CI
    boot_stats.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let alpha_half = (1.0 - ci_level) / 2.0;
    let lo_idx = ((n_resamples as f64) * alpha_half).floor() as usize;
    let hi_idx = (((n_resamples as f64) * (1.0 - alpha_half)).ceil() as usize)
        .saturating_sub(1)
        .min(boot_stats.len() - 1);
    [boot_stats[lo_idx], boot_stats[hi_idx]]
}
```

### Pattern 3: Per-job seed derivation via blake3 (HYG-05)

**What:** Each per-job seed is deterministically derived from `(master_seed, scan_id_at_version, instruments_canonical, timeframe, window_canonical, param_hash)` so re-running the same sweep with the same master seed produces byte-identical bootstrap / null samples per job. The derivation reuses the existing Phase 2 `Blake3Hex` convention.

**When to use:** Once per `ResolvedJob` during `job_graph::expand()`.

**Example:**
```rust
// crates/miner-core/src/scan/hygiene/seed.rs
use blake3::Hasher;
use crate::reader::InstrumentSpec;
use crate::aggregator::Timeframe;
use crate::reader::ClosedRangeUtc;

/// Derive a per-job 64-bit seed from the sweep master seed + the job's
/// canonical identity tuple (HYG-05 / D5-05). The blake3-32 hash collapses to
/// 64 bits via little-endian read of the first 8 bytes — sufficient entropy
/// for resampling, deterministic across platforms.
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

### Pattern 4: Deterministic-order rayon fanout

**What:** Spawn parallel workers with `rayon::par_iter`, but each worker writes its output to a per-job `Vec<Finding>` buffer. After all workers complete, the main thread drains the buffers in manifest order to the shared `FindingSink`. Trades some peak memory (a few jobs' worth of buffered output) for the byte-identical-rerun invariant (D3-23).

**When to use:** `sweep::executor::run_sweep` — the OP-04 fanout entry point.

**Example sketch:**
```rust
// crates/miner-core/src/sweep/executor.rs (sketch)
use rayon::prelude::*;

pub fn run_sweep<R: Reader + Sync>(
    jobs: Vec<ResolvedJob>,
    cfg: &MinerConfig,
    reader: &R,
    cache: &BarCache,
    cancel: Arc<AtomicBool>,
    sink: &mut dyn FindingSink,
) -> Result<RunOutcome, MinerError> {
    // Phase 1: parallel execution into per-job buffers.
    let buffered: Vec<(usize, Vec<Finding>, ScanCounts)> = jobs
        .par_iter()
        .enumerate()
        .map(|(idx, job)| {
            // Per-worker cancel poll at job boundary (D3-22 site)
            if cancel.load(Ordering::Relaxed) { return (idx, Vec::new(), ScanCounts::default()); }
            let mut buf = VecSink::new();
            let scan_req = job.to_scan_request();
            // Engine path: run_one_with_registry buffers findings into &mut VecSink instead of stdout
            let outcome = run_one_with_registry(&scan_req, cfg, reader, &mut buf, Arc::clone(&cancel));
            (idx, buf.into_inner(), buf.counts())
        })
        .collect();

    // Phase 2: sequential, manifest-order drain to the real sink (preserves byte-identical re-run).
    let mut had_errors = false;
    let mut all_p_values_by_family: BTreeMap<String, Vec<(usize, f64)>> = BTreeMap::new();
    for (idx, findings, _counts) in &buffered {
        for finding in findings {
            // Capture p-values for end-of-sweep BH-FDR before draining.
            if let Finding::Result(r) = finding {
                if let Some(p) = r.effect.p_value {
                    let family = r.scan_id_at_version.clone();
                    all_p_values_by_family.entry(family).or_default().push((/* per-family index */ ..., p));
                }
            }
            sink.write_envelope(finding)?;
        }
    }

    // Phase 3: SIGINT short-circuit — if cancel was set mid-sweep, skip SweepSummary
    // (exit 130 takes precedence; the streamed findings are preserved).
    if cancel.load(Ordering::Relaxed) {
        return Ok(RunOutcome::Ok); // CLI maps cancel → exit 130
    }

    // Phase 4: BH-FDR per family.
    let summary = build_sweep_summary(all_p_values_by_family, alpha = 0.05);
    sink.write_envelope(&Finding::SweepSummary(summary))?;

    Ok(if had_errors { RunOutcome::HadScanErrors } else { RunOutcome::Ok })
}
```

### Pattern 5: Extending DryRunFinding additively (CONFIRMS open question #7)

**What:** Instead of introducing a new `Finding::SweepDryRun` variant, extend the existing `DryRunFinding` (D3-21) with one new optional field `planned_job_count: Option<u64>`. This keeps the envelope churn to a minimum and matches the Phase 4 D4-03 / D3-10 schema-additive playbook.

**When to use:** Sweep `--dry-run` short-circuit emission.

**Example:**
```rust
// crates/miner-core/src/findings/mod.rs (modified)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DryRunFinding {
    pub run_id: RunId,
    pub produced_at_utc: DateTime<Utc>,
    pub request: serde_json::Value,
    pub resolved_params: serde_json::Value,
    pub planned_data_slice: DataSlice,
    pub estimated_findings_count: u64,
    /// Phase 5 (D5-01): for `miner sweep --dry-run`, this is the cartesian-
    /// expanded job count from the manifest. Single-shot `miner scan
    /// --dry-run` (Phase 3) leaves this as `None` so the schema diff is purely
    /// additive (`#[serde(default)]` is required to keep the existing Phase 3
    /// DryRunFinding wire form unchanged).
    #[serde(default)]
    pub planned_job_count: Option<u64>,
}
```

### Anti-Patterns to Avoid

- **`SmallRng` / `StdRng` for bootstrap or null resampling.** Explicitly excluded from `rust-random`'s reproducibility guarantees ("not portable", "may make value-breaking changes in any release"). HYG-05's bit-for-bit-reproducibility contract is violated on any `rand` minor bump. Always use `rand_xoshiro::Xoshiro256PlusPlus::seed_from_u64`.
- **`tokio` anywhere in `miner-core`.** FOUND-04 invariant. Phase 5's sweep is rayon-only; any async surface lives in Phase 6 wrappers via `spawn_blocking`.
- **`HashMap` in `fdr_by_family` or `per_finding` BH-FDR output.** OUT-03 byte-identical-rerun rule. Use `BTreeMap<String, FdrFamilySummary>` everywhere; `Vec<FindingFdrEntry>` ordered by stable `finding_index`.
- **Parallel inner resample loops inside one bootstrap call.** D3-23 byte-identical-rerun. The master seed sets the resample sequence; parallel inner loops scramble it (Plan 04-05's ADF AIC lag selection has the analogous pattern: sequential inner loop, rayon-parallel outer fanout only).
- **Mixing `criterion` and `divan` benches.** Phase 7 bench harness owns this decision; pick one. Until then, Phase 5 ships no bench code.
- **Streaming BH-FDR.** Tempting but wrong for v1 — the BH step-up needs every p-value in hand to assign ranks. The `ResultFinding.fdr_q` field stays reserved-null per Phase 1 contract; the `SweepSummary` envelope is the home for q-values.
- **Hand-rolling the Patton-Politis-White correction without reading the 2009 erratum first.** The 2004 paper's formula has a documented bias correction that the 2009 paper repaired. A naive read of 2004 ships a slightly biased block-length selector.
- **Phase scramble that doesn't preserve marginal distribution.** Simple phase randomisation (Theiler 1992 §3.1) destroys heavy-tail structure; financial returns have heavy tails as a *feature* not a *nuisance*. IAAFT iteratively projects to preserve BOTH spectrum and marginal — use it.
- **Forgetting `BTreeMap` discipline in `SweepManifest` `params` table.** Cargo's TOML deserialiser emits `toml::Table` (a `BTreeMap<String, Value>` under the hood as of `toml = "0.8"`), but the downstream serde transcode to `serde_json::Value` for `param_hash` calculation must NOT pass through a `HashMap` step — pin via `serde_json::Map`'s BTreeMap-backed default (workspace already pins `serde_json` with NO features list per the Cargo.toml comment).

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| TOML deserialisation | Custom Vec-of-blocks parser | `toml = "0.8"` + `serde::Deserialize` derives | TOML grammar has many edge cases (escapes, dotted keys, array-of-tables); the `toml` crate is maintained alongside Cargo itself |
| Cross-version-stable RNG | Implement xoshiro from scratch | `rand_xoshiro::Xoshiro256PlusPlus` | Already tested against the reference C implementation (Blackman & Vigna); `rust-random` org maintains |
| Real-valued FFT for IAAFT | Implement Cooley-Tukey from scratch | `realfft = "3"` (wraps `rustfft`) | FFT correctness + performance are easy to get wrong (twiddle factors, bit-reversal, in-place vs out-of-place); `realfft` exploits Hermitian symmetry for 2× speedup |
| Blake3 hashing for per-job seed | Re-implement | `blake3 = "1"` (already wired) | Same crate used for `param_hash` since Phase 2 — extend, don't duplicate |
| Distribution CDFs (Normal/T/F/ChiSquared) | Re-implement series expansions | `statrs = "0.17"` (already wired) | Phase 3 LjungBox + Phase 4 ADF/KPSS/ARCH-LM/JarqueBera all use this; do not add a second distribution crate |
| Rayon thread-pool management | Manual `std::thread` + `crossbeam-channel` | `rayon::par_iter` | CLAUDE.md HIGH-confidence pin; work-stealing handles uneven scan-job durations |

**DO hand-roll (matches the project's Phase 4 hand-rolled discipline):**

| Problem | Build it | Why |
|---------|----------|-----|
| Benjamini-Hochberg FDR | ~25 LOC pure function | Algorithm is mechanical (sort, scan, monotone-min); `adjustp` crate has 430 dl/mo single-author — fragile dep for ~one screen of code |
| Effect sizes (Cohen's d, Hedges' g, Cliff's delta, VR-minus-one) | ~10-30 LOC each | No off-the-shelf Rust crate exists; formulas are textbook; matches Phase 4's hand-rolled ANOM/CROSS/SEAS kernel discipline |
| Stationary / block bootstrap | ~50 LOC each | No off-the-shelf Rust crate exists for Politis-Romano stationary bootstrap; R's `tseries::tsbootstrap` is the canonical reference for goldens |
| IAAFT phase-scramble null | ~80 LOC + `realfft` call | No off-the-shelf Rust IAAFT exists; Theiler 1992 pseudo-code maps to ~80 LOC against `realfft` |
| Circular-shift null | ~15 LOC | Trivial rotation of `Vec<f64>` by uniform-random offset |
| Cartesian product expansion of TOML manifest axes | ~40 LOC | A general cartesian-product crate is overkill for the 4-axis (instruments × timeframes × windows × params) loop; explicit `for` loops are clearer than a generic iterator chain |
| BH family bucketing (group p-values by scan_id@version) | ~10 LOC | Simple `BTreeMap` insertion in the drain loop |
| Per-job seed derivation | ~30 LOC | Documented Pattern 3 above |

**Key insight:** Phase 5 is *more* hand-rolled than Phase 4, not less, because the statistical-hygiene ecosystem in Rust is thinner than the basic-stats ecosystem. The project's Phase 4 precedent makes this acceptable — every kernel is small, testable in isolation, and validated against R / scipy / statsmodels goldens.

## Runtime State Inventory

> Phase 5 is greenfield code addition (new modules + additive envelope fields). No rename / refactor / migration is involved.

This section intentionally omits the state-inventory table because no existing runtime state must be reconciled — every Phase 5 addition is purely additive to the Phase 1-4 contracts.

## Common Pitfalls

### Pitfall 1: Output ordering vs reproducibility

**What goes wrong:** A naive `par_iter` over jobs writes findings to the shared sink in completion order — fast jobs finish first regardless of manifest position. Subsequent runs with the same seed produce findings in a *different* order, breaking the D3-23 byte-identical-rerun invariant.

**Why it happens:** Rayon's work-stealing scheduler is non-deterministic by design. Worker thread A may finish job 5 before worker B finishes job 2.

**How to avoid:** Per-job buffered output + sequential manifest-order drain (Pattern 4). Each worker writes its findings into its own `Vec<Finding>`; the main thread drains buffers in `(0..jobs.len())` order. Costs a few MB of peak memory (one job-batch's findings × N workers in flight) for byte-identical-rerun.

**Warning signs:** Two re-runs producing JSONL diffs in `finding_index` or in line ordering; CI `byte_identical_rerun` test failures.

### Pitfall 2: Bootstrap memory amplification

**What goes wrong:** A naive bootstrap allocates `n_resamples` × `n_values` doubles per job. Combined with rayon-parallel job execution (say 8 workers) and large series (n ≈ 10⁵-10⁶ bars, n_resamples = 1000), peak memory can hit 10s of GB.

**Why it happens:** Each bootstrap resample is a new `Vec<f64>` of length n.

**How to avoid:** Reuse a single buffer per bootstrap call (Pattern 2 — `buf.clear()` then `buf.push(...)`). Only the bootstrap *statistic* (a `f64`) is accumulated in `boot_stats: Vec<f64>` of length `n_resamples`. This is what the Pattern 2 code already does.

**Warning signs:** OOM kills on long-history sweeps; flamegraph shows `Vec::with_capacity(n)` allocation hot.

### Pitfall 3: Phase scramble FFT length requirements

**What goes wrong:** `realfft` is FASTEST when the input length has small prime factors (radix-2 / mixed-radix). A series of length 31657 (prime) executes a slow generic DFT in `rustfft` — orders of magnitude slower than 32768.

**Why it happens:** FFT performance follows the n's prime factorisation.

**How to avoid:** For IAAFT, pad the input to the next "nice" length via zero-padding OR truncate to the next power-of-2 below. RECOMMEND padding (preserves more data); document the convention in the algorithm. Plan-phase pins the padding rule.

**Warning signs:** A specific instrument/timeframe combination runs 10× slower than its neighbours.

### Pitfall 4: BH family scoping — too coarse vs too fine

**What goes wrong:** Pool all 22 scans × 28 instruments × 3 timeframes findings (≈1850 findings) under one BH family — p-values from Ljung-Box on returns are pooled with p-values from Engle-Granger cointegration tests. The hypotheses are unrelated; the BH-adjusted q-values are pessimistic to the point of uselessness. Or: scope per `(scan_id, instrument, timeframe)` — too fine, the family contains 1-3 hypotheses and BH provides no multiple-testing protection at all.

**Why it happens:** Family choice is a *scientific* decision, not a technical one.

**How to avoid:** Default to per-`scan_id@version` (D5-02). Expose `[fdr].family = "scan_family" | "all" | "none"` for callers who want a different policy. Document the default in the README quickstart.

**Warning signs:** Quant agent reports "nothing is significant" even on synthetic data with planted signal → family too coarse. Or: "everything is significant" on the noise-replay test → family too fine.

### Pitfall 5: SmallRng vs Xoshiro256PlusPlus

**What goes wrong:** Plan adopts `rand::rngs::SmallRng::seed_from_u64(job_seed)` per the CONTEXT D5-05 default. Bootstrap and null findings are reproducible across runs *of the same binary* but break on `cargo update` of `rand` from `0.8.5` to `0.8.6`.

**Why it happens:** `SmallRng` is explicitly documented as non-portable: "may make value-breaking changes in any release."

**How to avoid:** Use `rand_xoshiro::Xoshiro256PlusPlus::seed_from_u64(job_seed)`. The xoshiro256++ algorithm has a published reference (Blackman & Vigna's C implementation) and `rand_xoshiro` tests against it. Cross-version stable by design.

**Warning signs:** The byte-identical-rerun CI test fails after a workspace `cargo update`.

### Pitfall 6: TOML deserialisation drift

**What goes wrong:** `toml` crate emits `toml::Table` (`BTreeMap` under the hood), but the round-trip into `serde_json::Value` for `param_hash` calculation accidentally passes through a HashMap step (e.g., via a `HashMap<String, serde_json::Value>` intermediate). Two runs of the same TOML produce different `param_hash` values.

**Why it happens:** Workspace's `serde_json` is feature-less specifically so `serde_json::Map` is `BTreeMap`-backed (deterministic key order). Any HashMap-backed intermediate breaks that guarantee.

**How to avoid:** Deserialise straight into typed structs (`JobBlock { params: BTreeMap<String, serde_json::Value>, ... }`), never via an `untyped: HashMap<...>` step. Pin the discipline with a test that re-orders the TOML keys and asserts the resulting `param_hash` is the same.

**Warning signs:** `param_hash` differs between runs on identical TOML; or differs based on TOML key ordering.

### Pitfall 7: Cancel polling cadence inside bootstrap inner loops

**What goes wrong:** A bootstrap with `n_resamples = 100_000` on a large series does not poll the SIGINT cancel flag for 30+ seconds, frustrating the user.

**Why it happens:** D3-22 documents three named cancel-yield sites; bootstrap inner loops aren't one of them.

**How to avoid:** Poll `cancel.load(Ordering::Relaxed)` every N resamples (recommend N = 64, matching Plan 04-10's `CANCEL_POLL_CADENCE = 4096` scale for SEAS scans but tightened because bootstrap iterations are heavier). Plan-phase pins the exact cadence with a microbenchmark.

**Warning signs:** SIGINT during a long bootstrap shows multi-second delay before exit 130.

### Pitfall 8: Schema regen producing non-additive diff

**What goes wrong:** Schemars 1.x is mostly additive on new optional fields and new enum variants, but two edge cases break additivity:
  - Adding a `#[serde(rename_all = "snake_case")]` to a struct that didn't have it before (breaks existing consumers).
  - Adding a field WITHOUT `#[serde(default)]` (becomes a required field in the schema's `required` array).

**Why it happens:** Required-field additions are non-additive by definition.

**How to avoid:** Every Phase 5 new field MUST carry `#[serde(default)]`. Plan-phase regenerates `schemas/findings-v1.schema.json` BEFORE committing any code; the `git diff schemas/` must show only additive changes (new properties, new oneOf variants). The Phase 1 FOUND-03 / CI Gate 4 already enforces this.

**Warning signs:** CI schema diff gate fails on the first Phase 5 commit; consumers report parse failures on Phase 4 fixtures.

### Pitfall 9: Look-ahead leak through bootstrap

**What goes wrong:** A bootstrap resample on a rolling-statistic scan accidentally includes bars from outside the look-ahead-safe window. The scan's `bars_up_to(ts)` discipline is broken at the hygiene layer.

**Why it happens:** The bootstrap kernel takes `&[f64]` — it has no notion of timestamps.

**How to avoid:** Bootstrap kernels operate on the SAME `&[f64]` slice the scan kernel already received from `BarFrameView` (which is pre-truncated to the look-ahead-safe range). Plan-phase pins via a shuffled-future regression test (analogous to Phase 4's `shuffled_future_regression.rs`): bootstrap a rolling-corr scan with the look-ahead-future bars permuted; assert the bootstrap CI is unchanged.

**Warning signs:** Bootstrap CIs differ between two re-runs where the bars *after* the scan's window have been permuted.

## Code Examples

### Example 1: Effect-size kernel (Cohen's d)

```rust
// crates/miner-core/src/scan/hygiene/effect_size.rs

/// Cohen's d effect size between two independent groups (pooled SD).
///
/// `d = (mean_a - mean_b) / s_pooled`
/// where `s_pooled = sqrt(((n_a - 1) * var_a + (n_b - 1) * var_b) / (n_a + n_b - 2))`.
///
/// Returns NaN for `n_a + n_b < 3` (degrees-of-freedom collapse) or for
/// zero pooled variance (degenerate case).
pub fn cohens_d(a: &[f64], b: &[f64]) -> f64 {
    let n_a = a.len();
    let n_b = b.len();
    if n_a < 2 || n_b < 2 || (n_a + n_b) < 3 { return f64::NAN; }

    let mean_a = a.iter().sum::<f64>() / n_a as f64;
    let mean_b = b.iter().sum::<f64>() / n_b as f64;
    let var_a = a.iter().map(|x| (x - mean_a).powi(2)).sum::<f64>() / (n_a - 1) as f64;
    let var_b = b.iter().map(|x| (x - mean_b).powi(2)).sum::<f64>() / (n_b - 1) as f64;
    let denom = ((n_a + n_b - 2) as f64).max(1.0);
    let s_pooled_sq = ((n_a - 1) as f64 * var_a + (n_b - 1) as f64 * var_b) / denom;
    if s_pooled_sq <= 0.0 { return f64::NAN; }
    (mean_a - mean_b) / s_pooled_sq.sqrt()
}

/// Hedges' g — bias-corrected Cohen's d for small samples.
/// `g = d * (1 - 3 / (4 * (n_a + n_b) - 9))`
pub fn hedges_g(a: &[f64], b: &[f64]) -> f64 {
    let d = cohens_d(a, b);
    if !d.is_finite() { return d; }
    let n = (a.len() + b.len()) as f64;
    let correction = 1.0 - 3.0 / (4.0 * n - 9.0);
    d * correction
}

/// Cliff's delta — non-parametric effect size, range [-1, +1].
/// `delta = (count(a_i > b_j) - count(a_i < b_j)) / (n_a * n_b)`
pub fn cliffs_delta(a: &[f64], b: &[f64]) -> f64 {
    let n_a = a.len();
    let n_b = b.len();
    if n_a == 0 || n_b == 0 { return f64::NAN; }
    let mut gt = 0i64;
    let mut lt = 0i64;
    for &x in a {
        for &y in b {
            if x > y { gt += 1; }
            else if x < y { lt += 1; }
        }
    }
    (gt - lt) as f64 / (n_a as f64 * n_b as f64)
}
```

### Example 2: TOML manifest deserialiser

```rust
// crates/miner-core/src/sweep/manifest.rs
use std::collections::BTreeMap;
use serde::Deserialize;
use chrono::{DateTime, Utc};

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
    /// "stationary" | "block" — None means disabled
    pub bootstrap: Option<String>,
    #[serde(default)]
    pub bootstrap_n: u32,
    /// "phase_scramble" | "circular_shift" — None means disabled
    pub null: Option<String>,
    #[serde(default)]
    pub null_n: u32,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct FdrConfig {
    /// "scan_id" (default) | "scan_family" | "all" | "none"
    #[serde(default = "default_fdr_family")]
    pub family: String,
    #[serde(default = "default_alpha")]
    pub alpha: f64,
}
fn default_fdr_family() -> String { "scan_id".to_string() }
fn default_alpha() -> f64 { 0.05 }

#[derive(Debug, Clone, Deserialize)]
pub struct JobBlock {
    pub scan: String,  // "scan_id@version"
    /// Single-arity: Vec<String> like ["EURUSD:bid"];
    /// Pair-arity: Vec<Vec<String>> like [["EURUSD:bid", "GBPUSD:bid"]].
    /// Deserialised as serde_json::Value first, then validated by job_graph::expand.
    pub instruments: serde_json::Value,
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

### Example 3: Sweep summary envelope

```rust
// crates/miner-core/src/findings/mod.rs (additions)

/// New finding variant emitted at end-of-sweep (D5-02).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SweepSummaryFinding {
    pub run_id: RunId,
    pub produced_at_utc: DateTime<Utc>,
    /// BH-FDR families keyed by `scan_id@version` (default scope) or
    /// `scan_family` (e.g. "stats", "cross", "seas") per [fdr].family.
    /// `BTreeMap` for deterministic ordering — OUT-03.
    pub fdr_by_family: BTreeMap<String, FdrFamilySummary>,
    /// Counts (jobs_run, results_emitted, scan_errors, gap_aborted) so consumers
    /// have a single-line digest of the sweep.
    pub totals: SweepTotals,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FdrFamilySummary {
    /// "benjamini_hochberg" — extension hook for v2 (e.g. "benjamini_yekutieli").
    pub method: String,
    pub alpha: f64,
    /// Per-finding ordered by `finding_index` — `Vec` keeps stable ordering.
    pub per_finding: Vec<FindingFdrEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FindingFdrEntry {
    /// Zero-indexed position of this finding within its family in the streaming JSONL output.
    pub finding_index: u64,
    pub raw_p: f64,
    pub q_value: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub struct SweepTotals {
    pub jobs_run: u64,
    pub results_emitted: u64,
    pub scan_errors: u64,
    pub gap_aborted: u64,
}

// Finding enum gains:
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Finding {
    RunStart(RunStart),
    Result(ResultFinding),
    ScanError(ScanErrorFinding),
    GapAborted(GapAbortedFinding),
    RunEnd(RunEnd),
    DryRun(DryRunFinding),
    SweepSummary(SweepSummaryFinding),   // NEW (Phase 5 / D5-02)
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `rand::rngs::SmallRng` for "fast deterministic" RNG | Named-algorithm RNGs (`rand_xoshiro::Xoshiro256PlusPlus`, `rand_chacha::ChaCha8Rng`) for reproducible work | `rand` 0.8 (2022) explicitly documented `SmallRng` as non-portable | Any code requiring cross-version determinism MUST use a named algorithm |
| Implementing BH-FDR ad-hoc per project | (a) Hand-roll ~25 LOC, or (b) `adjustp` crate | `adjustp` 0.1 (2024) | Tiny algorithm; off-the-shelf is fine but adds a low-traffic dep |
| `RustFFT` for real-valued FFT | `realfft` (thin wrapper) | `realfft` 1.0 (2020) | 2× speedup on even-length real input |
| Polars / DataFrame-shaped numerics | `ndarray` for column-of-floats math | Phase 4 (Plan 04-01) | CLAUDE.md veto on Polars in-flight; storage-only (Arrow IPC cache) |
| `tokio::task::spawn_blocking` for parallel CPU work | `rayon::par_iter` | Phase 1 (FOUND-04) | Async runtimes block on CPU work; rayon's work-stealing is the right shape |
| Bare `toml` 0.5/0.6 with manual `toml::Value` traversal | `toml` 0.8 with `serde::Deserialize` derives | `toml` 0.7 (2023) | Cleaner deserialiser surface; matches workspace serde idioms |

**Deprecated/outdated:**
- `SmallRng` for reproducible work: not deprecated as a type, but explicitly documented as non-portable. Use `rand_xoshiro` algorithms instead.
- `async-std`: effectively unmaintained as of 2024+ (CLAUDE.md "What NOT to Use" row). Not relevant to Phase 5 (no async in miner-core), but flagged because some IAAFT examples online use it.
- `rand` 0.7.x with `rand::Rng::gen` ergonomics: `rand` 0.8 changed the trait API; pin `rand = "0.8"` not `rand = "0.7"`.

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | `rand_xoshiro = "0.6"` is the current major; `Xoshiro256PlusPlus` is documented as portable | §1.5 / Standard Stack | Plan-phase must `cargo add` to confirm latest version + cross-reference the crate's `tests/reference.rs` test vectors against Blackman-Vigna's reference output |
| A2 | `toml = "0.8"` is the current major and supports `serde::Deserialize` derives on nested struct trees with `BTreeMap<String, serde_json::Value>` fields | §1.2 / Standard Stack | Low risk — `toml` 0.8 has been stable since 2023; serde support is the crate's primary feature |
| A3 | The `adjustp` crate's BH implementation produces output identical to R's `p.adjust(p, method="BH")` | §1.6 / Don't Hand-Roll | Plan-phase generates BH-FDR goldens against R `stats::p.adjust` for at least 3 corner cases (canonical 5-tuple, ties, length-1) — the hand-rolled implementation MUST match within machine epsilon |
| A4 | IAAFT with 10 iterations converges to a "good enough" surrogate for financial returns (n typically ≥ 10⁴) | §1.8 / Pattern selection | Theiler et al. 1992 §IV.B suggests 5-20 iterations; 10 is the literature's common default. If convergence fails on short series (n < 100), plan-phase must add a max-iteration safety bound + a fallback to simple phase randomisation |
| A5 | Politis-White (2004) automatic block-length selector with Patton-Politis-White (2009) correction produces stable block-length estimates on financial return series (n ≥ 10⁴) | §1.7 / Bootstrap | Plan-phase tests against R `tseries::b.star(returns)` for the 28-instrument sample; the Rust implementation MUST agree within 10% on the selected block length |
| A6 | `realfft` (when used) supports padding to next-nice-length and the slow-prime-length path in pure `rustfft` is acceptable as a fallback if padding cannot be applied | §1.8 / Pitfall 3 | Low risk — `realfft` 3.x supports arbitrary lengths via the underlying `rustfft`; performance degrades but correctness is preserved |
| A7 | Hand-rolling BH-FDR is cheaper than depending on `adjustp` (single-author, low-popularity crate) | §1.6 / Don't Hand-Roll | Low risk — Phase 4 hand-rolled far more complex statistical kernels; the BH algorithm is mechanical |
| A8 | The per-job buffered + sequential drain pattern (Pattern 4) for rayon fanout produces byte-identical output across re-runs even with non-deterministic worker completion order | §1.3 / Pitfall 1 | Plan-phase pins via the existing `byte_identical_rerun` test pattern (Phase 4 Plan 04-11) — re-run same sweep, expect bit-for-bit JSONL match |
| A9 | The `Effect.effect_size: Option<EffectSize>` field addition is schema-additive in schemars 1.x | §1.6 / Pattern 5 | Plan-phase regenerates `schemas/findings-v1.schema.json` and inspects the diff BEFORE committing the API changes (Phase 1 FOUND-03 / CI Gate 4 enforcement) |
| A10 | Finding `Vec<Finding>` buffering inside `par_iter` workers does NOT cause excessive memory pressure for a typical sweep (e.g., 10⁵ findings × 1 KB each = 100 MB peak) | §1.3 / Pitfall 2 | Plan-phase microbenchmarks the worst-case sweep (28 instruments × 3 timeframes × 6 years × 22 scans × 1 param-point = ~11K findings); if memory > 1 GB, the drain cadence is tightened |

**If this table feels long:** it's a side-effect of Phase 5 layering hygiene on top of a tested Phase 4 stack — most assumptions are *constants pins* rather than architectural choices, and the locked decisions in CONTEXT.md cover the architectural risk surface.

## Open Questions

1. **Per-scan default for `supports_null_method(NullMethod)` — IAAFT vs circular-shift for SEAS bucket scans.**
   - What we know: SEAS bucket-effect findings test "is the mean of bucket B different from zero?" — that's not a phase-scrambled hypothesis. Circular shift might still apply (shift the whole series; recompute bucket means; build null distribution).
   - What's unclear: Whether circular-shift adds value on top of the existing analytic per-bucket t-stat (SEAS-01) and ANOVA/Kruskal-Wallis (SEAS-05) p-values.
   - Recommendation: SEAS bucket scans → `supports_null_method(NullMethod) -> false` for both methods in v1. Bootstrap CIs DO apply (per-bucket mean over the bucket's observations — see CROSS-05 ANOVA + bootstrap CIs in scipy.stats reference). Plan-phase pins.

2. **Block-length default constant for `bootstrap = "block"` (FIXED-length variant).**
   - What we know: Politis-Romano stationary uses an *expected* block length. Plain block bootstrap uses a *fixed* length. Common defaults are `ceil(n^(1/3))` (Hall-Horowitz-Jing 1995) or the Politis-White selector applied as a *target* length (round to nearest integer).
   - What's unclear: Whether to expose the block length as a CLI flag (`--bootstrap-block-length N`) or always auto-select.
   - Recommendation: Always auto-select via Politis-White-Patton-Politis-White-2009 (PWPpW); compute the float `b_star` and use `max(3, ceil(b_star))` as both the fixed-length and the mean for stationary. Plan-phase pins; expose `--bootstrap-block-length` only in v2 if a use case demands it.

3. **`SweepSummary` envelope ordering relative to `RunEnd`.**
   - What we know: D5-02 says "between the last Result and `RunEnd`." Phase 3's D-09 framing is RunStart at the top, RunEnd at the bottom.
   - What's unclear: Whether `SweepSummary` is itself a framing-like record (no locked envelope fields) or a content-like record (locked fields present).
   - Recommendation: Framing-like — `SweepSummaryFinding` carries `run_id` + `produced_at_utc` only, no `schema_version` / `scan_id_at_version` / `param_hash` / `code_revision` / `data_slice`. The sweep summary is run-level, not scan-level. Plan-phase pins.

4. **Per-job `param_hash` derivation when the manifest's `params` field carries arrays.**
   - What we know: `param_hash` is computed over canonicalised (post-defaults) params (D3-13). In a sweep, each `ResolvedJob` corresponds to ONE param-point, not a param-array.
   - What's unclear: Whether the canonical `params` for a job is `{lags: 10}` (scalar) or `{lags: [10]}` (array-of-one).
   - Recommendation: Scalar. The job-graph expansion already iterates over the cartesian product, so by the time `ResolvedJob` is built, every param is a scalar. The `param_hash` is computed over the scalar form. Pins the byte-identical-rerun invariant.

5. **Master seed derivation when omitted from manifest.**
   - What we know: D5-05 says `master_seed` defaults to `blake3(manifest_hash || run_id)` when omitted. Echoed in `ReproEnvelope.master_seed`.
   - What's unclear: Whether `manifest_hash` is `blake3(file_bytes)` or `blake3(canonical_serialised_manifest)`. The former is stable to byte-identical re-input; the latter is stable to semantic re-input but breaks on whitespace/comment changes.
   - Recommendation: `blake3(file_bytes)`. Simpler, no canonicalisation step required, breaks only on actual file edits. Plan-phase pins.

6. **Whether `Finding::SweepSummary` increments any counter in `RunSummary`.**
   - What we know: Phase 3 Pitfall 3 / Warning 9 enforces `RunSummary` having exactly four fields (`results_emitted`, `scan_errors`, `gap_aborted`, `per_scan`) — `DryRun` was deliberately NOT given a counter.
   - What's unclear: Whether `SweepSummary` should be counted similarly.
   - Recommendation: NO new counter. `SweepSummary` is run-level metadata, not a scan output. Pin via the Phase 3 Warning 9 regression test pattern (`run_summary_has_no_dry_run_emitted_field`-style exhaustive-destructure test for the four fields).

7. **CI level exposure — pin 95% or expose `--ci-level 0.99`?**
   - What we know: `Effect.ci95: Option<[f64; 2]>` field name pins 95% in v1.
   - What's unclear: Whether v1 should ship `--ci-level` as a CLI flag.
   - Recommendation: Pin 95%. If a use case demands 99%, plan-phase can add `--ci-level` later (the field can stay `ci95` for back-compat; or v2 introduces `ci: Option<{level: f64, low: f64, high: f64}>`). Phase 5 does NOT expose `--ci-level`.

## Environment Availability

> Phase 5 is pure-Rust library + Cargo additions. The CI sandbox is sufficient. No new external runtimes.

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| Rust toolchain | All Phase 5 code | ✓ | 1.85+ (edition 2024) | — |
| `cargo` | Workspace builds | ✓ in CI | latest stable | — |
| `cargo` (researcher sandbox) | Local version verification | ✗ | — | Used `docs.rs` + `crates.io` + WebSearch as source-of-truth (A1, A2 above) |
| Python 3.11 + scipy + statsmodels + R `tseries`/`stats` | Golden generation for BH-FDR + bootstrap CIs (test-only) | Likely ✓ in CI (Plan 04-11 already wires `tests/REFERENCE-VERSIONS.md`) | scipy 1.14.x / statsmodels 0.14.6 / R 4.x | Phase 4 stub-fixture-fallback pattern (gate cross-check tests behind a provenance file) |
| `realfft = "3"` | IAAFT phase-scramble (optional) | Will be installed by `cargo add` | 3.5+ | If `realfft` doesn't resolve, fall back to `rustfft = "6"` directly (~2× slower on real input) |

**Missing dependencies with no fallback:** none — every dep is on crates.io and the alternatives are documented.

**Missing dependencies with fallback:** `realfft` (fallback to `rustfft`). Plan-phase decides whether to ship IAAFT in Phase 5 or defer to Phase 7 (which would push `realfft` out of Phase 5 entirely — circular-shift null is the v1 minimum).

## Validation Architecture

> `workflow.nyquist_validation` defaults to enabled.

### Test Framework

| Property | Value |
|----------|-------|
| Framework | `cargo test` (`#[test]`) + `proptest = "1.11"` (already wired in `miner-core/Cargo.toml` dev-deps) + `insta = "1.47"` (already wired) |
| Config file | None — `cargo test` runs `[lib]` + `tests/*.rs` integration tests directly |
| Quick run command | `cargo test -p miner-core --lib --quiet` (lib unit tests only, ~5-15s) |
| Full suite command | `cargo test --workspace --all-features` (lib + integration + clippy + schema-diff; ~60-180s) |

### Phase Requirements → Test Map

| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|--------------|
| OP-04 | TOML sweep manifest deserialises into typed `SweepManifest` | unit | `cargo test -p miner-core --lib sweep::manifest::tests` | ❌ Wave 0 |
| OP-04 | Cartesian expansion of `[[jobs]]` block produces correct `Vec<ResolvedJob>` length + deterministic order | unit / proptest | `cargo test -p miner-core --lib sweep::job_graph::tests` | ❌ Wave 0 |
| OP-04 | Sweep `--dry-run` emits `Finding::DryRun` with `planned_job_count == jobs.len()` and skips parallel execution | integration | `cargo test -p miner-core --test sweep_dry_run` | ❌ Wave 0 |
| OP-04 | Sweep estimated job count exceeds `[sweep].max_jobs` → `PreflightCode::SweepTooLarge` | unit | `cargo test -p miner-core --lib sweep::manifest::tests::sweep_too_large` | ❌ Wave 0 |
| OP-04 | Sweep runs 2-job × 2-instrument × 1-tf × 1-window × 1-param manifest end-to-end with byte-identical JSONL output | integration | `cargo test -p miner-core --test sweep_smoke` | ❌ Wave 0 |
| OP-04 | SIGINT mid-sweep preserves every already-streamed finding; NO `SweepSummary` emitted; exit code 130 | integration (CLI) | `cargo test -p miner-cli --test sigint_mid_sweep` | ❌ Wave 0 |
| HYG-01 | Cohen's d / Hedges' g / Cliff's delta / VR-minus-one kernels return correct values on known-answer fixtures | unit | `cargo test -p miner-core --lib scan::hygiene::effect_size::tests` | ❌ Wave 0 |
| HYG-01 | Every `Finding::Result` emitted by a sweep carries `effect.effect_size: Some(EffectSize { kind, value })` | integration | `cargo test -p miner-core --test effect_size_emission` | ❌ Wave 0 |
| HYG-01 | `Effect.effect_size` round-trips through serde with `kind` and `value` preserved | unit | `cargo test -p miner-core --lib findings::tests::effect_size_round_trip` | ❌ Wave 0 |
| HYG-02 | `bh_fdr` matches R's `p.adjust(p, method="BH")` for canonical 5-tuple [0.01, 0.02, 0.03, 0.04, 0.05] | unit | `cargo test -p miner-core --lib scan::hygiene::fdr::tests::bh_fdr_canonical_5` | ❌ Wave 0 |
| HYG-02 | `bh_fdr` preserves rank order (q-values are monotone in p-values) | unit / proptest | `cargo test -p miner-core --lib scan::hygiene::fdr::tests::bh_fdr_rank_order_proptest` | ❌ Wave 0 |
| HYG-02 | Sweep emits one `Finding::SweepSummary` envelope with per-family q-values BETWEEN the last `Result` and `RunEnd` | integration | `cargo test -p miner-core --test sweep_summary_emission` | ❌ Wave 0 |
| HYG-02 | `[fdr].family = "scan_id"` (default) groups findings by `scan_id@version`; `"none"` skips q-values | integration | `cargo test -p miner-core --test fdr_family_scoping` | ❌ Wave 0 |
| HYG-03 | `stationary_bootstrap_ci` on iid Gaussian data returns CI containing the true mean ≥ 90% of the time | proptest | `cargo test -p miner-core --lib scan::hygiene::bootstrap::tests::stationary_iid_coverage` | ❌ Wave 0 |
| HYG-03 | `stationary_bootstrap_ci` is deterministic for a fixed seed (re-run produces byte-identical CI) | unit | `cargo test -p miner-core --lib scan::hygiene::bootstrap::tests::deterministic_for_seed` | ❌ Wave 0 |
| HYG-03 | Block-length selector (Politis-White 2004 + 2009 correction) matches R `tseries::b.star` within 10% | golden | `cargo test -p miner-core --test bootstrap_block_length_golden -- --ignored` | ❌ Wave 0 |
| HYG-04 | `circular_shift_null_p` returns a uniform-distributed p-value under the null on synthetic uncorrelated data | proptest | `cargo test -p miner-core --lib scan::hygiene::null::tests::circular_shift_uniform_under_null` | ❌ Wave 0 |
| HYG-04 | IAAFT phase-scramble preserves the original power spectrum to within 1e-6 over 10 iterations | unit | `cargo test -p miner-core --lib scan::hygiene::null::tests::iaaft_preserves_spectrum` | ❌ Wave 0 |
| HYG-04 | IAAFT phase-scramble preserves the original marginal distribution (sorted-values match within machine epsilon) | unit | `cargo test -p miner-core --lib scan::hygiene::null::tests::iaaft_preserves_marginal` | ❌ Wave 0 |
| HYG-05 | Per-job seed derivation is deterministic and stable: `derive_job_seed(...) == derive_job_seed(...)` for identical inputs across runs | unit | `cargo test -p miner-core --lib scan::hygiene::seed::tests::derive_job_seed_deterministic` | ❌ Wave 0 |
| HYG-05 | Re-running the same sweep with the same master seed produces byte-identical JSONL output (modulo `run_id` + clock fields, masked per the Phase 1 OUT-03 convention) | integration | `cargo test -p miner-core --test sweep_byte_identical_rerun` | ❌ Wave 0 |
| HYG-05 | `ResultFinding.repro: Option<ReproEnvelope>` is `Some(_)` iff bootstrap or null was run; `None` otherwise | unit | `cargo test -p miner-core --lib findings::tests::repro_envelope_population_rule` | ❌ Wave 0 |
| HYG-05 | `Xoshiro256PlusPlus::seed_from_u64(seed)` produces a stable sequence (regression test against pinned reference vector) | unit | `cargo test -p miner-core --lib scan::hygiene::bootstrap::tests::xoshiro_reference_vector` | ❌ Wave 0 |
| Cross-cutting | Schema diff is purely additive after Phase 5 envelope changes | CI gate | `git diff --exit-code schemas/findings-v1.schema.json` after `cargo xtask gen-schema` (additive-only inspection) | ❌ Wave 0 |
| Cross-cutting | `cargo tree -p miner-core | grep -E 'tokio|async-std'` returns nothing after Phase 5 deps land | CI gate | manual / `xtask check-no-async` | ✅ existing |
| Cross-cutting | Phase 5 modules respect `clippy::disallowed_macros` (no `println!` / `eprintln!` outside the sink + logging adapter) | CI gate | `cargo clippy --workspace --all-targets -- -D warnings` | ✅ existing |
| Cross-cutting | `Scan::supports_bootstrap()` + `supports_null_method()` are object-safe (`Scan` trait stays dyn-compatible) | compile-only | `cargo test -p miner-core --lib scan::tests::scan_trait_object_safe` | ✅ existing (Phase 3) |

### Sampling Rate

- **Per task commit:** `cargo test -p miner-core --lib --quiet` — covers the kernel unit tests (effect_size, bootstrap, null, fdr, seed, manifest)
- **Per wave merge:** `cargo test --workspace` — adds the integration tests (sweep_smoke, sweep_dry_run, sweep_summary_emission, sweep_byte_identical_rerun, sigint_mid_sweep) and the CLI binary tests
- **Phase gate:** Full suite green before `/gsd:verify-work`, PLUS `cargo xtask gen-schema && git diff --exit-code schemas/` (schema-additive enforcement)

### Wave 0 Gaps

- [ ] `crates/miner-core/src/scan/hygiene/{mod,effect_size,bootstrap,null,fdr,seed}.rs` — new kernel modules (all six files NEW)
- [ ] `crates/miner-core/src/sweep/{mod,manifest,job_graph,executor}.rs` — new sweep runner module (all four files NEW)
- [ ] `crates/miner-core/tests/sweep_smoke.rs` — end-to-end sweep integration test
- [ ] `crates/miner-core/tests/sweep_dry_run.rs` — dry-run integration test
- [ ] `crates/miner-core/tests/sweep_summary_emission.rs` — `Finding::SweepSummary` emission test
- [ ] `crates/miner-core/tests/sweep_byte_identical_rerun.rs` — bit-for-bit reproducibility regression
- [ ] `crates/miner-core/tests/fdr_family_scoping.rs` — `[fdr].family` enum coverage
- [ ] `crates/miner-core/tests/effect_size_emission.rs` — every scan emits a non-null `effect.effect_size`
- [ ] `crates/miner-core/tests/bootstrap_block_length_golden.rs` — R `tseries::b.star` golden (gated #[ignore] until provenance available)
- [ ] `crates/miner-cli/src/sweep_args.rs` — new `SweepArgs` clap-derive struct
- [ ] `crates/miner-cli/tests/sigint_mid_sweep.rs` — CLI binary SIGINT-during-sweep integration test
- [ ] `tests/REFERENCE-VERSIONS.md` — extend with `R 4.x` + `tseries`/`stats` pins for BH-FDR + block-length goldens
- [ ] Workspace `Cargo.toml` — add `rand = "0.8"`, `rand_xoshiro = "0.6"`, `toml = "0.8"` to `[workspace.dependencies]`; optionally `realfft = "3"`
- [ ] `crates/miner-core/Cargo.toml` — pull the four new deps in via workspace inheritance
- [ ] `schemas/sweep-manifest-v1.schema.json` — NEW (optional companion artifact via xtask)

*(None of the existing Phase 1-4 tests need to change; all Wave 0 work is greenfield additions.)*

## Security Domain

> The CONTEXT.md does not surface `security_enforcement`. The project is statically-linked, sync-only, no network I/O in `miner-core`, single-user CLI/MCP/HTTP wrappers. The Phase 5 surface adds (a) a TOML file parser (untrusted file format) and (b) RNG seeding. These are the security-relevant additions.

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | Phase 5 has no authentication surface (single-user CLI; MCP/HTTP auth lives in Phase 6 if at all) |
| V3 Session Management | no | No sessions |
| V4 Access Control | no | No access control inside miner-core; OS file permissions for cache root and manifest path |
| V5 Input Validation | **yes** | TOML manifest is untrusted user input → `serde::Deserialize` + explicit `validate(&SweepManifest)` preflight + bounded recursion (TOML `nested_limit` already enforced by `toml = "0.8"` defaults at 256). Reject paths-with-`..`, oversize values, malformed scan IDs at preflight |
| V6 Cryptography | no | `blake3` is used as a *hash function* for seed derivation (HYG-05) and for `param_hash` (Phase 2) — NOT as a cryptographic MAC or KDF. No keys exchanged. RNG seeding is for resampling, not for cryptographic operations |

### Known Threat Patterns for Phase 5 surface

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Malicious TOML manifest with deep nesting → stack overflow | DoS | `toml = "0.8"` enforces a default 256-level nesting limit; manifest validation aborts with `PreflightCode::InvalidParameter` |
| Sweep manifest declaring 10⁹ jobs → memory exhaustion | DoS | `[sweep].max_jobs` cap (default 100_000) enforced at preflight via `PreflightCode::SweepTooLarge` |
| User-controlled file path injection (manifest path → cache root → IPC file read) | Tampering | The CLI accepts a manifest PATH; `std::fs::read_to_string(path)` is straight POSIX; the manifest content is parsed but does NOT control any subsequent file path beyond `[sweep].max_jobs` and the scan registry — the cache root is set via existing `MinerConfig` precedence (FOUND-05) |
| Bootstrap CI / null p-value tampering via seed prediction | Information Disclosure (statistical noise) | Master seed is echoed in `ReproEnvelope.master_seed`; this is INTENTIONAL (HYG-05 — same seed → same output). No secret material is involved. `Xoshiro256PlusPlus` is NOT cryptographic and predicting future outputs from observed outputs is trivial; this is acceptable because the threat model has no secrecy requirement |
| Wide manifest causing parallel BarCache thrash → disk IO storm | DoS | Pre-existing Phase 2 BarCache concurrency limit applies; `[sweep].max_jobs` and the per-job sequential bar load (via `BarCache::get_or_build`) bound the concurrent disk reads to rayon's worker count |
| Sweep summary BH-FDR family pooling attack | Information manipulation | Caller-controlled `[fdr].family` setting; the worst a malicious manifest can do is set `family = "none"` and not perform FDR adjustment — the per-finding `raw_p` is still emitted, so consumers can re-adjust |

## Sources

### Primary (HIGH confidence)
- **`crates/miner-core/Cargo.toml`** — current workspace dep list; confirms `statrs`, `ndarray`, `nalgebra`, `blake3`, `serde`, `serde_json`, `chrono`, `schemars`, `figment` are wired
- **Workspace `Cargo.toml`** — pinned versions and the canonical determinism note (`serde_json` MUST stay feature-less)
- **`crates/miner-core/src/findings/mod.rs`** — current `Effect` / `ResultFinding` / `Finding` enum shape; the additive Phase 5 changes target this file
- **`crates/miner-core/src/scan/mod.rs`** — current `Scan` trait shape; `supports_bootstrap()` and `supports_null_method()` extension target
- **`crates/miner-core/src/engine/mod.rs`** — `run_one_with_registry` body; the hygiene-kernel invocation point and `run_sweep` entry-point target
- **`crates/miner-core/src/error/codes.rs`** — `PreflightCode` enum; `SweepTooLarge` already shipped (Phase 1), `HygieneNotSupported` is the Phase 5 addition
- **`.planning/phases/05-statistical-hygiene-sweep-runner/05-CONTEXT.md`** — full D5-01..D5-05 + Claude-discretion + open-question list (the contract this research closes against)
- **`.planning/REQUIREMENTS.md`** — HYG-01..05 + OP-04 requirement language
- **`.planning/STATE.md`** — recent decisions (Plan 04-13 clippy::pedantic clean baseline; Plan 04-12 CR-01 closure; Phase 4 carry-over)
- **CLAUDE.md (project root)** — locked technology stack pins (`rand`, `toml`, FFT, FDR are all explicitly considered in the TL;DR table)
- **The Rust Rand Book — Reproducibility chapter** (https://rust-random.github.io/book/crate-reprod.html) — explicit non-portability of `SmallRng` / `StdRng`; named-algorithm RNGs are portable
- **`docs.rs/rand_xoshiro/latest/rand_xoshiro/`** — confirms `Xoshiro256PlusPlus` is the recommended cross-version-stable algorithm

### Secondary (MEDIUM confidence — verified against multiple sources)
- **Politis & Romano (1994) "The Stationary Bootstrap"** — JASA 89(428), 1303-1313 (canonical reference for HYG-03)
- **Politis & White (2004) + Patton-Politis-White (2009) correction** — Econometric Reviews; canonical reference for the automatic block-length selector
- **Theiler et al. (1992) "Testing for Nonlinearity in Time Series: the Method of Surrogate Data"** — Physica D 58; canonical reference for IAAFT phase-scramble
- **Benjamini & Hochberg (1995)** — JRSS B 57(1), 289-300; canonical reference for HYG-02
- **`docs.rs/realfft 3.5`** — confirms `realfft` 3.5 is current; documents the 2× speedup vs raw `rustfft` for real-valued input
- **`docs.rs/toml`** — confirms `toml = "0.8"` supports `serde::Deserialize` derives on nested struct trees with map-typed fields
- **`crates.io/crates/adjustp`** + **`lib.rs/crates/adjustp`** — confirms `adjustp` is real but low-popularity (430 dl/mo, single author, MIT, ~2 years old) — rejected in favour of hand-rolled BH-FDR

### Tertiary (LOW confidence — single source or training-only)
- **The exact iteration count for IAAFT convergence (recommended 10)** — Theiler et al. 1992 §IV.B suggests 5-20; "10" is folklore default. Plan-phase may refine.
- **Block-length floor `max(3, ceil(n^(1/3)))`** — based on Hall-Horowitz-Jing 1995 heuristic; not in any single authoritative source for the floor itself, but the `n^(1/3)` scaling is standard.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — every dep pin is either already wired (`statrs`, `ndarray`, `nalgebra`, `blake3`, `rayon`, `serde`, `chrono`) or backed by an authoritative crates.io / docs.rs lookup (`rand`, `rand_xoshiro`, `toml`, `realfft`)
- Architecture: HIGH — Phase 5 is a pure layering on top of the verified Phase 3-4 facade; every pattern (kernel split, schema-additive envelope extension, rayon-fanout-with-deterministic-drain) has a Phase 1-4 precedent
- Statistical algorithms: HIGH (BH-FDR — mechanical algorithm) / HIGH (effect sizes — textbook formulas) / MEDIUM (stationary bootstrap block-length selector — Politis-White-Patton-Politis-White-2009 is the right reference but the floor constant is a judgment call) / MEDIUM (IAAFT iteration count — literature default but not pinned to a specific value)
- Pitfalls: HIGH — every pitfall listed is either documented in the upstream crate's README (SmallRng portability), called out in CLAUDE.md (Polars veto, rayon-vs-tokio split), or carried over from the Phase 1-4 invariants (BTreeMap discipline, byte-identical-rerun)

**Research date:** 2026-05-20
**Valid until:** 2026-06-20 (30 days; stable stack, mature literature — primary risk is a `rand` or `toml` major bump, both unlikely in the window)
