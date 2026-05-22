# Phase 4: Scan Catalogue (ANOM, CROSS, SEAS) — Research

**Researched:** 2026-05-19
**Domain:** Rust statistical-scan catalogue layered over a verified Phase-3 facade
**Confidence:** HIGH on stack and patterns (carry-over from verified Phases 1-3); MEDIUM on per-scan reference call shape (statsmodels/scipy choices are well-documented but a handful of scans require either hand-derivation or a non-obvious function pick — flagged inline).

## Summary

Phase 4 is a *catalogue scale-out*, not a new architecture. Every load-bearing decision was locked or carried-over in Phases 1-3:

- The `Scan` trait + `bootstrap()` registry pattern is verified and stable (D3-14, D3-16).
- The `LjungBoxScan` `mod.rs`/`kernel.rs` split is the **gold-standard reference pattern** the 22 Phase 4 scans replicate verbatim.
- `statrs` is already wired in for chi-squared CDFs; `ndarray` / `nalgebra` are NOT yet present and must be added in a single workspace-Cargo.toml task.
- Schemars 1.x + xtask `gen-schema` already deterministically regenerate `schemas/findings-v1.schema.json` and `schemas/scans-catalogue-v1.schema.json`; Phase 4 inherits the determinism pipeline and adds an extra invocation to the schema-sync CI gate.

The four user-locked decisions (D4-01 instruments Vec, D4-02 arity trait + CLI shape, D4-03 sources Vec + leg-labelled raw, D4-04 inner-join + intersection gap policy) define the facade-shape extension. The five Claude's-discretion decisions plus the ten open questions all close cleanly against the existing patterns and statsmodels conventions — no surprises requiring a re-discuss.

**Primary recommendation:** Decompose Phase 4 into FIVE plans (facade-shape extension → ANOM 11 scans → CROSS 5 scans → SEAS 6 scans → integration tests + goldens + schema regen). Pin reference implementation to a single `statsmodels==0.14.6` + `scipy==1.14.x` version-pair in `tests/REFERENCE-VERSIONS.md`; ship ONE finding per invocation with vector arrays in `effect.extra` for vector-output scans (rolling stats, bucketed seasonality). Add only three new direct deps: `ndarray = "0.16"`, `ndarray-stats = "0.6"`, `nalgebra = "0.33"`.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions (user-locked, NOT renegotiable in plan-phase)

- **D4-01:** `ScanRequest.instrument: String + side: Side` GENERALISES to `instruments: Vec<InstrumentSpec { symbol: String, side: Side }>`. ANOM/SEAS scans take length 1; CROSS scans take length 2. Breaking change to the Phase 3 facade, performed now because Phase 6 (MCP/HTTP) hasn't committed.
- **D4-02:** New `Scan::arity() -> ScanArity` trait method validated at preflight. `ScanArity::{Single, Pair}` initial variants (extendable to `Many(min, max)` for v2 baskets). CLI uses repeatable `--instrument SYMBOL:side` flag. Preflight rejects wrong arity via new `PreflightCode::WrongInstrumentArity` variant. Arity exposed in `miner scans` catalogue.
- **D4-03:** `DataSlice.source: Source` GENERALISES to `DataSlice.sources: Vec<Source>` (length = arity, parallel to `instruments`). `Raw.series` carries leg-labelled keys (`returns_a`, `returns_b`, `timestamps_ms_aligned`). Single-leg findings keep a single-element Vec. **D4-03-ALT** fallback: keep `source: Source` for primary leg, add `peer_sources: Vec<Source>` sibling — invoked ONLY if schema regen shows D4-03 is non-additive.
- **D4-04:** CROSS-01 ships inner-join-only as the v1 time-alignment primitive. Gap policy operates on the INTERSECTION of both legs' manifests. `strict` aborts if EITHER leg has any gap. `continuous_only` partitions on intervals where BOTH legs are continuous. Forward-fill explicitly rejected for v1.

### Claude's Discretion (plan-phase + this research close them)

- **D4-05:** ANOM-01 ships BOTH a kernel utility (`miner-core::scan::primitives::returns`) AND a standalone callable scan. Exact scan name and one-scan-vs-four split deferred to plan-phase. *(This research recommends ONE scan with `--params variant=...` — see §1.4.)*
- **D4-06:** Phase 3's `LjungBoxScan` refactors in Phase 4 to call the new ANOM-01 primitive (D3-02 cleanup task). MUST NOT change the goldens by construction.
- **D4-07:** CROSS legs may carry independent `side` values (bid-vs-ask cross-spread scans permitted). Default behaviour; no per-scan policy in v1.
- **D4-08:** `effect.value` canonical pick for vector-output CROSS scans — defer-to-this-research. *(See §1.3 per-scan envelope table.)*
- **D4-09:** Hedge-ratio sign convention for Engle-Granger CROSS-05 — defer-to-this-research. *(See §1.7.)*

### Deferred Ideas (OUT OF SCOPE for Phase 4)

- Statistical hygiene layer (Cohen's d, Hedges' g, Cliff's delta, block bootstrap, phase-scrambled nulls, BH-FDR, DSR) → Phase 5 / HYG-*. `effect.ci95` / `dsr` / `fdr_q` stay `null` in Phase 4 outputs.
- TOML sweep-manifest fanout → Phase 5 / OP-04. Phase 4 stays single-shot per invocation (carry-forward D3-18).
- MCP / HTTP wrappers → Phase 6 / OP-02 / OP-03. Phase 4's facade extension is what those wrappers will mirror.
- Bench harness / flamegraph / golden-regression scale-out → Phase 7.
- v2 scan families (Johansen, Granger, Hurst, PELT, correlation-breakdown, basket divergence, Anderson-Darling) → REQUIREMENTS.md SCAN-v2-*.
- Alternative time-alignment join modes (forward-fill, strict_equal) → v2.
- Look-ahead-safety enforcement *mechanism* picked by plan (`ScanCtx::bars_up_to(ts)` API shape) — this research recommends a shape; final implementation is plan-task choice.
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| ANOM-01 | Return primitives (log, simple, intraday vs overnight) as reusable scan input | §1.4 + §2 (ANOM-01 row) — recommend one callable scan + kernel utility |
| ANOM-02 | Summary stats (mean, std, skew, excess kurtosis, IQR, min, max) via Welford | §2 (ANOM-02) — hand-rolled Welford kernel; statsmodels golden for skew/kurtosis |
| ANOM-03 | Rolling volatility + vol-of-vol | §2 (ANOM-03) — hand-rolled rolling-std; emit ONE finding with vector arrays |
| ANOM-04 | Ljung-Box on returns + squared returns | §2 (ANOM-04) — **already shipped in Phase 3**; Phase 4 refactors to call ANOM-01 primitive (D4-06) AND adds a squared-returns variant |
| ANOM-05 | ADF with AIC-selected lag | §2 (ANOM-05) — `statsmodels.tsa.stattools.adfuller(autolag='AIC')`; hand-rolled in Rust |
| ANOM-06 | KPSS stationarity (opposite null to ADF) | §2 (ANOM-06) — `statsmodels.tsa.stattools.kpss(regression='c')` default |
| ANOM-07 | Lo-MacKinlay variance ratio over multiple k | §2 (ANOM-07) — hand-rolled; `arch.unitroot.VarianceRatio` golden (NOT in statsmodels core) |
| ANOM-08 | ARCH-LM conditional heteroskedasticity | §2 (ANOM-08) — `statsmodels.stats.diagnostic.het_arch` |
| ANOM-09 | Jarque-Bera normality | §2 (ANOM-09) — `scipy.stats.jarque_bera` |
| ANOM-10 | Outliers (z-score + modified-z / MAD) | §2 (ANOM-10) — hand-rolled; both z + modified-z reported |
| ANOM-11 | Drawdown profile on synthetic equity curve | §2 (ANOM-11) — hand-derived (peak-trough scan); no statsmodels reference |
| CROSS-01 | Time-alignment primitive (inner-join on common timestamps) | §1.6 + §2 (CROSS-01) — kernel-only; gap-intersect helper |
| CROSS-02 | Rolling Pearson + rolling Spearman with threshold-crossing flags | §2 (CROSS-02) — hand-rolled rolling over `numpy.corrcoef` / `scipy.stats.spearmanr` |
| CROSS-03 | Rolling OLS regression (β, α, R², residual std) | §2 (CROSS-03) — `nalgebra` small-fixed OLS; statsmodels `RollingOLS` golden |
| CROSS-04 | Lead-lag cross-correlation function (CCF) | §2 (CROSS-04) + §1.9 — `scipy.signal.correlate` mode='full' or `statsmodels.tsa.stattools.ccf` |
| CROSS-05 | Engle-Granger two-step cointegration + OU half-life | §2 (CROSS-05) + §1.7 — `statsmodels.tsa.stattools.coint` |
| SEAS-01 | Hour-of-day return/vol profile (UTC) | §2 (SEAS-01) — bucket-by-hour; statsmodels not required (pure aggregation) |
| SEAS-02 | Day-of-week effect | §2 (SEAS-02) — bucket-by-weekday |
| SEAS-03 | Trading-session bucketing (Asia/London/NY/overlap) | §2 (SEAS-03) + §1.8 — UTC-range defaults documented |
| SEAS-04 | EOM/SOM by trading-day-of-month | §2 (SEAS-04) — last/first-N trading-day bucketing; N default = 3 |
| SEAS-05 | One-way ANOVA / Kruskal-Wallis | §2 (SEAS-05) — `scipy.stats.f_oneway` + `scipy.stats.kruskal` |
| SEAS-06 | Event-window pre/post stats | §2 (SEAS-06) — hand-derived; caller-supplied event timestamps |
</phase_requirements>

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| Statistical kernels (ACF, ADF, KPSS, OLS, regressions) | `miner-core::scan::<family>::<scan>::kernel` | — | Pure functions, no IO / serde / Reader; unit-testable in isolation (LjungBox precedent) |
| Scan envelope construction (`Finding::Result` emission) | `miner-core::scan::<family>::<scan>::mod` (`Scan::run`) | — | Scans build typed `ResultFinding` and write through `FindingSink`; the facade owns flush |
| Two-leg time alignment (inner-join + gap-manifest intersect) | `miner-core::scan::primitives::time_alignment` | `miner-core::gap` (gap helper may live here as a sibling helper) | Pure kernel utility consumed by CROSS-02..05; no scan registration |
| Returns primitive (log/simple/intraday/overnight) | `miner-core::scan::primitives::returns` | Callable scan in `miner-core::scan::anom::returns` | Shared kernel + standalone callable scan per D4-05 |
| Per-leg BarFrame fetch | `miner-core::engine::run_one` (facade) | `BarCache::get_or_build` | CROSS scans call the cache TWICE before invoking `Scan::run`; facade owns the loop and cancel-poll between fetches |
| Look-ahead-safety enforcement | `miner-core::scan::ScanCtx::bars_up_to(ts)` (NEW) | Per-scan kernel iteration | Structural enforcement at the brokering object boundary; shuffled-future regression test is the pin |
| Arity validation | `miner-core::engine::preflight` | `Scan::arity()` trait method | Preflight rejects `instruments.len() != arity` with new `PreflightCode::WrongInstrumentArity` |
| Schema regeneration | `xtask::gen_schema` (existing) | `schemas/findings-v1.schema.json` artifact | Phase 4's facade-shape changes flow through the existing determinism pipeline |
| Catalogue surface (`miner scans`) | `miner-cli::main` → `Registry::iter()` | `Scan::finding_fields()` + new `Scan::arity()` | Adds `arity` field to each catalogue line — additive schema change to scans catalogue |

## Standard Stack

### Core (already present — confirmed from `crates/miner-core/Cargo.toml`)

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `statrs` | 0.17 | Distribution CDFs (ChiSquared, Normal, T, F) | Already wired (Phase 3 LjungBox); used by ADF/KPSS p-values, Jarque-Bera p-value, F-stat p-value [VERIFIED: workspace Cargo.toml line 73] |
| `serde` + `serde_json` | 1 | Envelope ser/de, params canonicalisation | Locked workspace deps; `serde_json` MUST stay feature-less (BTreeMap-backed Map per workspace determinism note) [VERIFIED: workspace Cargo.toml lines 37-38] |
| `schemars` | 1 (`chrono04` feature) | `JsonSchema` derive for envelope types | Already wired; Phase 4's `InstrumentSpec` + `ScanArity` types derive `JsonSchema` [VERIFIED: workspace Cargo.toml line 39] |
| `chrono` | 0.4 | UTC datetimes for session bucketing | Already wired (Calendar, BarFrame timestamps); used by SEAS-01/SEAS-03 to compute hour/weekday from `ts_open_utc` [VERIFIED: workspace Cargo.toml line 40] |
| `blake3` | 1 | `param_hash` over canonical resolved-params blob | Already wired (D3-13); Phase 4 unchanged [VERIFIED: workspace Cargo.toml line 49] |

### Supporting (NEW — Phase 4 adds these to `miner-core/Cargo.toml`)

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `ndarray` | 0.16 | N-dim numerical arrays, column views | Rolling-window kernels (ANOM-03 vol, CROSS-02 corr), summary stats (ANOM-02), bucket aggregations (SEAS-01/02/03) [CITED: docs.rs/ndarray + CLAUDE.md TL;DR row] |
| `ndarray-stats` | 0.6 | mean/var/quantile/correlation over ndarray | ANOM-02 summary stats; CROSS-02 Pearson via `ndarray-stats::CorrelationExt` [CITED: docs.rs/ndarray-stats + CLAUDE.md] |
| `nalgebra` | 0.33 | Small fixed-size linear algebra | CROSS-03 rolling OLS — stack-allocated 2D regression matrices, no heap pressure inside rayon workers [CITED: CLAUDE.md "Detailed" §`nalgebra`] |

**Versions to verify at install time** (training-data versions; confirm with `cargo add` resolution):

```bash
cargo add --manifest-path crates/miner-core/Cargo.toml ndarray@0.16 ndarray-stats@0.6 nalgebra@0.33
```

**Optional — NOT recommended for Phase 4:** `arch` Python package is **not needed** beyond `statsmodels.stats.diagnostic.het_arch` for ANOM-08 (`het_arch` covers the test fully); however the `arch` Python package's `VarianceRatio` IS needed for ANOM-07 golden generation (Lo-MacKinlay variance ratio is not in statsmodels core). See §1.2 for the reference pinning policy.

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `ndarray` + `ndarray-stats` | Pure hand-rolled `Vec<f64>` kernels | Saves ~120 KB binary size; loses readability and conventional broadcast semantics. **Reject** — Phase 4 has 22 scans; `ndarray`'s slice/view API pays for itself by scan 3. |
| `nalgebra` (CROSS-03 small-fixed) | `ndarray-linalg` (heavy BLAS-backed) | `ndarray-linalg` pulls openblas/netlib/intel-mkl — heavy CI surface for tiny 2D regressions. **Reject** — CLAUDE.md veto on `ndarray-linalg` unless needed for large algebra, which Phase 4 doesn't have. |
| `statrs` ChiSquared/Normal/T/F | `rv` (`rand`-friendly) or `distrs` | `statrs` is already wired and battle-tested; the Phase 3 LjungBox golden validates the CDF path against statsmodels. **Reject change** — no reason to multi-source distributions. |
| Hand-rolled rolling-OLS without `nalgebra` | Two-pass mean+covariance formula in `f64` arrays | Numerically less stable; the textbook OLS β = `Σ((x-x̄)(y-ȳ))/Σ(x-x̄)²` runs into catastrophic cancellation on tight rolling windows. **Reject** — `nalgebra`'s `SMatrix<f64, 2, 2>` solve is both stable and idiomatic. |
| `chrono` for session bucketing | `jiff` (CLAUDE.md preferred) | Project already wires `chrono` via `BarFrame`; introducing `jiff` mid-phase doubles the time-crate surface. **Defer** `jiff` to a future cleanup phase. |

**Installation:**

```bash
cargo add --manifest-path crates/miner-core/Cargo.toml ndarray@0.16 ndarray-stats@0.6 nalgebra@0.33
```

After install, confirm `cargo tree -p miner-core | grep -E 'tokio|async-std'` returns nothing (FOUND-04 sync-only invariant). All three additions are sync-only.

## Package Legitimacy Audit

Phase 4 installs three new Rust crates; package legitimacy was verified via authoritative docs + workspace `CLAUDE.md` stack pinning rather than `slopcheck` (which is a Python-ecosystem tool). All three are well-established crates in the Rust numerical ecosystem.

| Package | Registry | Age | Downloads | Source Repo | Verification | Disposition |
|---------|----------|-----|-----------|-------------|--------------|-------------|
| `ndarray` | crates.io | 9+ yrs | very high | github.com/rust-ndarray/ndarray | Authoritative (CLAUDE.md TL;DR pin + docs.rs) | Approved |
| `ndarray-stats` | crates.io | 7+ yrs | high | github.com/rust-ndarray/ndarray-stats | Authoritative (CLAUDE.md "Detailed" §`ndarray-stats`) | Approved |
| `nalgebra` | crates.io | 12+ yrs | very high | github.com/dimforge/nalgebra | Authoritative (CLAUDE.md "Detailed" §`nalgebra`) | Approved |

**Packages removed due to slopcheck [SLOP] verdict:** none
**Packages flagged as suspicious [SUS]:** none

**Note on Python reference packages** (`statsmodels`, `scipy`, `arch`): These live in `tests/REFERENCE-VERSIONS.md` for golden-generation only; they are NOT runtime dependencies of the miner binary. All three are top-100 PyPI packages with decade-plus histories. No Python packages are installed by the Rust build.

## Architecture Patterns

### System Architecture Diagram

```
                CLI (--instrument SYM:side, --instrument SYM:side, --window, etc.)
                  │  parses repeatable --instrument SYM:side
                  │  builds Vec<InstrumentSpec>
                  ▼
        ScanArgs.to_scan_request(code_revision) -> ScanRequest
                  │  ScanRequest.instruments: Vec<InstrumentSpec>
                  │  (length 1 for ANOM/SEAS, length 2 for CROSS)
                  ▼
        engine::run_one(req, cfg, reader, sink) ──── preflight ────▶ arity check
                  │                                                  (instruments.len() == scan.arity()?)
                  │                                                       │
                  │                                                       ▼ no → emit WireError(WrongInstrumentArity) + exit 1
                  ▼ yes
        For each instrument in req.instruments:
            BarCache::get_or_build(reader, AggParams) → BarFrame
        (CROSS scans call this TWICE; poll cancel between)
                  │
                  ▼
        gap dispatch
            single-leg: existing path (Phase 3)
            two-leg:    intersect_gaps(manifest_a, manifest_b) → joint_manifest
                        strict + any gap → Finding::GapAborted (carries BOTH manifests under
                                                                data_slice.sources[i].gap_manifest)
                        continuous_only → partition on BOTH-continuous sub-ranges; one Result per
                  │
                  ▼
        ScanCtx { bars (or bar_pair), gap_manifest, run_id, code_revision, cancel,
                  bars_up_to(ts) helper for rolling/causal scans }
                  │
                  ▼
        Scan::run(ctx, req, sink)
            anom::*   — read ctx.bars (single-leg)
            cross::*  — read ctx.bars_pair (a, b) — both legs time-aligned by CROSS-01 primitive
            seas::*   — read ctx.bars + Calendar for session bucketing
                  │  computes effect.value, effect.extra (vector arrays for rolling/bucketed)
                  │  raw.series carries leg-labelled keys for CROSS (returns_a, returns_b,
                  │                                                  timestamps_ms_aligned)
                  ▼
        FindingSink.write_envelope(&Finding::Result(...)) → JSONL on stdout
                  │  RunEnd framing follows
                  ▼
        Process exit code 0/1/2/130 (D3-24)
```

### Recommended Project Structure

```
crates/miner-core/src/scan/
├── mod.rs                        # Scan trait + ScanArity enum + ScanCtx + ScanRequest
├── registry.rs                   # bootstrap() — extend with 22 register() lines, alphabetical
├── shape.rs                      # ScanFindingShape — unchanged
├── primitives/
│   ├── mod.rs                    # primitives namespace
│   ├── returns.rs                # ANOM-01 returns kernel (log/simple/intraday/overnight)
│   └── time_alignment.rs         # CROSS-01 inner-join + intersect_gaps helper
├── anom/
│   ├── mod.rs                    # anom namespace (re-exports each scan struct)
│   ├── returns.rs                # ANOM-01 callable scan (stats.returns.profile@1)
│   ├── summary.rs                # ANOM-02 + summary.rs::kernel
│   ├── vol.rs                    # ANOM-03 rolling vol + vol-of-vol + kernel.rs
│   ├── ljung_box/                # ANOM-04 (already exists; Phase 4 adds squared-returns variant)
│   ├── adf.rs                    # ANOM-05 + kernel.rs
│   ├── kpss.rs                   # ANOM-06 + kernel.rs
│   ├── variance_ratio.rs         # ANOM-07 + kernel.rs (Lo-MacKinlay)
│   ├── arch_lm.rs                # ANOM-08 + kernel.rs
│   ├── jarque_bera.rs            # ANOM-09 + kernel.rs
│   ├── outliers.rs               # ANOM-10 + kernel.rs (z + modified-z)
│   └── drawdown.rs               # ANOM-11 + kernel.rs (peak-trough)
├── cross/
│   ├── mod.rs                    # cross namespace
│   ├── corr_rolling.rs           # CROSS-02 Pearson + Spearman + kernel.rs
│   ├── ols_rolling.rs            # CROSS-03 + kernel.rs (nalgebra small-fixed)
│   ├── lead_lag.rs               # CROSS-04 CCF + kernel.rs
│   └── engle_granger.rs          # CROSS-05 + kernel.rs (ADF residual + OU half-life)
└── seas/
    ├── mod.rs                    # seas namespace
    ├── hour_of_day.rs            # SEAS-01 + kernel.rs
    ├── day_of_week.rs            # SEAS-02 + kernel.rs
    ├── session.rs                # SEAS-03 + kernel.rs (uses Calendar)
    ├── eom_som.rs                # SEAS-04 + kernel.rs (last/first-N trading-day-of-month)
    ├── anova_kw.rs               # SEAS-05 + kernel.rs (one-way ANOVA + Kruskal-Wallis)
    └── event_window.rs           # SEAS-06 + kernel.rs (pre/post stats from caller events)

crates/miner-core/tests/
├── scan_<name>.rs                # ONE integration test per scan (22 files)
├── shuffled_future_regression.rs # Phase 3 file — extend to every rolling/causal scan
├── two_leg_facade.rs             # NEW — pins D4-01..D4-04 facade-shape extension end-to-end
├── arity_preflight.rs            # NEW — pins PreflightCode::WrongInstrumentArity
└── goldens/                      # Checked-in statsmodels/scipy reference outputs
    └── REFERENCE-VERSIONS.md     # pinned statsmodels==0.14.6, scipy==1.14.x, arch==7.x

schemas/
├── findings-v1.schema.json        # regenerated by xtask gen-schema; D4-01/D4-03 additive
└── scans-catalogue-v1.schema.json # regenerated; gains "arity" field per scan line (additive)
```

### Pattern 1: `mod.rs` / `kernel.rs` split (THE gold-standard)

**What:** Each scan is two files. `mod.rs` contains the `Scan` trait impl + envelope construction + `Finding::Result` emission. `kernel.rs` contains pure functions (no IO, no `serde`, no `Reader`) that the `Scan::run` calls. Unit-testable in isolation.

**When to use:** Every Phase 4 scan with non-trivial math (i.e., all 22).

**Example — verbatim from Phase 3 `LjungBoxScan`:**

```rust
// crates/miner-core/src/scan/<family>/<scan>/mod.rs
//! `<ScanName>Scan` — Phase 4 scan implementing the [`Scan`] trait.
//!
//! ## D4-XX contract
//! - id = "<family>.<subfamily>.<scan_name>", version = 1
//! - effect.metric = "<canonical metric>", effect.value = <scalar>
//! - effect.extra.<vector keys>, raw.series.<key list>

use std::collections::BTreeMap;
use std::sync::atomic::Ordering;
use chrono::Utc;
use crate::findings::{Base64Bytes, DataSlice, Dtype, Effect, Finding, FindingSink,
                     Raw, RawArray, ResultFinding, Source};
use crate::scan::{Scan, ScanArity, ScanCtx, ScanError, ScanFindingShape, ScanRequest};

pub mod kernel;

pub struct MyScan;

const SCAN_ID: &str = "<family>.<subfamily>.<scan_name>";
const SCAN_VERSION: u32 = 1;
const EFFECT_METRIC: &str = "<canonical_metric>";

impl Scan for MyScan {
    fn id(&self) -> &'static str { SCAN_ID }
    fn version(&self) -> u32 { SCAN_VERSION }
    fn arity(&self) -> ScanArity { ScanArity::Single }  // or ScanArity::Pair for CROSS

    fn param_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "properties": { /* hand-rolled per scan */ },
            "additionalProperties": false
        })
    }

    fn finding_fields(&self) -> ScanFindingShape {
        ScanFindingShape {
            effect_extra_keys: &["<key1>", "<key2>"],
            raw_series_keys: &["<input1>", "timestamps_ms"],
        }
    }

    fn run(&self, ctx: &ScanCtx<'_>, req: &ScanRequest,
           sink: &mut dyn FindingSink) -> Result<(), ScanError> {
        // 1. Cancel-poll at entry (D3-22).
        if ctx.cancel.load(Ordering::Relaxed) { return Ok(()); }

        // 2. Compute kernel.
        let result_arrays = kernel::compute(...);

        // 3. Build effect.extra (BTreeMap, alphabetical).
        let mut extra: BTreeMap<String, RawArray> = BTreeMap::new();
        extra.insert("<key1>".into(), f64_slice_to_raw_array(&result_arrays.key1));
        // ...

        // 4. Build raw.series (Raw::new enforces timestamps_ms invariant D-03).
        let mut series: BTreeMap<String, RawArray> = BTreeMap::new();
        series.insert("timestamps_ms".into(), f64_slice_to_raw_array(&timestamps));
        series.insert("<input1>".into(), f64_slice_to_raw_array(&input1));
        let raw_block = Raw::new(series).map_err(|m| ScanError::Kernel(m.to_string()))?;

        // 5. Construct ResultFinding (PINNED field names from Phase 3).
        let result = ResultFinding {
            schema_version: 1,
            scan_id_at_version: format!("{SCAN_ID}@{SCAN_VERSION}"),
            param_hash: req.param_hash.as_str().to_string(),
            code_revision: ctx.code_revision.to_string(),
            data_slice: DataSlice {
                range: req.sub_range.clone(),
                gap_manifest_ref: None,
                gap_manifest: ctx.gap_manifest.cloned(),
                // D4-03: sources: Vec<Source> length = arity
            },
            dsr: None, fdr_q: None,
            run_id: ctx.run_id,
            produced_at_utc: Utc::now(),
            // D4-03: sources: Vec<Source> populated from req.instruments
            params: req.resolved_params.clone(),
            effect: /* ... */,
            raw: Some(raw_block),
        };

        // 6. Emit (Pitfall 5: NEVER call sink.flush() inside the scan body).
        sink.write_envelope(&Finding::Result(result))?;
        Ok(())
    }
}
```

```rust
// crates/miner-core/src/scan/<family>/<scan>/kernel.rs
//! Pure kernel — no IO, no serde, no Reader.
#![cfg_attr(any(test, debug_assertions), allow(clippy::float_cmp))]

#[inline]
pub(super) fn compute(input: &[f64], param: usize) -> KernelOutput {
    // Pure-fn body. Returns Vec<f64> / KernelOutput typed struct.
}

#[cfg(test)] mod tests {
    use super::*;
    const TOL: f64 = 1e-12;
    fn approx_eq(a: f64, b: f64, tol: f64) -> bool { (a - b).abs() <= tol }

    #[test] fn kernel_known_input() {
        let got = compute(&[1.0, 2.0, 3.0], 1);
        assert!(approx_eq(got.first, 0.5, TOL));
    }
    // + edge cases (empty input, singleton, constant series, NaN propagation)
}
```

**Source:** `crates/miner-core/src/scan/ljung_box/mod.rs` lines 1-241 and `kernel.rs` lines 1-318. Phase 4 scans replicate this verbatim — same module structure, same comment headers, same test-fixture discipline.

### Pattern 2: Two-leg scan body (CROSS family)

**What:** CROSS scans receive `instruments: Vec<InstrumentSpec>` of length 2. The facade fetches TWO `BarFrame`s and presents them to the scan via `ScanCtx::bars_pair` (or `ctx.bars_for_leg(0)` / `ctx.bars_for_leg(1)`). Inside the scan body, the CROSS-01 time-alignment primitive inner-joins on common timestamps.

**When to use:** CROSS-02, CROSS-03, CROSS-04, CROSS-05.

**Example:**

```rust
// Inside Scan::run for a CROSS scan:
let (bars_a, bars_b) = ctx.bars_pair();
let aligned = primitives::time_alignment::inner_join(bars_a, bars_b)?;
//      aligned.timestamps_ms : Vec<i64>     (the joint timestamp index)
//      aligned.close_a / close_b : Vec<f64> (price series, time-aligned)
let returns_a = primitives::returns::log_returns(&aligned.close_a);
let returns_b = primitives::returns::log_returns(&aligned.close_b);
// ... kernel call, envelope construction ...
// raw.series:
//   timestamps_ms_aligned, returns_a, returns_b
// data_slice.sources: Vec<Source> of length 2, parallel to instruments order
```

### Pattern 3: Vector-output finding (rolling / bucketed scans)

**What:** Rolling-stat scans (CROSS-02, CROSS-03, CROSS-04 partial, ANOM-03) and bucketed scans (SEAS-01..04) emit ONE `Finding::Result` per invocation with VECTOR arrays in `effect.extra`. `effect.value` is the headline scalar (last window for rolling; F-stat or other summary scalar for bucketed).

**When to use:** Every scan whose natural output is a sequence of per-window or per-bucket statistics.

**Example — rolling Pearson:**

```rust
let effect = Effect {
    metric: "corr_pearson_rolling".into(),
    value: rolling_corr[rolling_corr.len() - 1],  // last window — trader intuition
    p_value: None,                                 // multi-window — no headline p
    n: Some(aligned_n as u64),
    ci95: None,
    extra: {
        let mut m = BTreeMap::new();
        m.insert("values".into(),         f64_slice_to_raw_array(&rolling_corr));
        m.insert("window_starts_ms".into(), f64_slice_to_raw_array(&window_starts));
        m.insert("window_length".into(),  f64_slice_to_raw_array(&[window_len as f64]));
        m.insert("threshold_crossings".into(), f64_slice_to_raw_array(&crossing_flags));
        m
    },
};
```

**Rationale:** Quant agent decodes one envelope per invocation; downstream sweep fanout (Phase 5) preserves the one-envelope-per-job invariant. Keeps stdout JSONL throughput bounded (no million-window finding storms).

### Anti-Patterns to Avoid

- **One-finding-per-window for rolling scans.** Tempting because each window is a "result". Rejected — produces millions of findings for moderately-sized inputs; breaks the Phase 3 facade single-shot model; downstream tooling has to re-group anyway.
- **`HashMap` anywhere on the `Serialize` path** — OUT-03 violation. The clippy `disallowed_types` gate (workspace-wide) catches this; every Phase 4 scan uses `BTreeMap`.
- **Per-leg gap policy divergence** — both legs of a CROSS scan obey the SAME `--gap-policy`. The CONTEXT decision D4-04 intersects manifests; the scan never sees a "leg-a-strict, leg-b-continuous" mix.
- **Implicit `effect.value` for vector-output scans** — every vector-output scan documents its `effect.value` choice in `mod.rs` constants (e.g., `const EFFECT_VALUE_RULE: &str = "last_window";`). The §2 table below pins each.
- **Touching the sink's `flush()` from inside the scan** (Pitfall 5 from Phase 3). The facade owns flush — scans only call `sink.write_envelope(&Finding::Result(...))`.
- **Reading bars later than the timestamp currently being emitted** (look-ahead violation). Use `ctx.bars_up_to(ts)` for any rolling/causal stat.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Chi-squared / Normal / T / F CDFs | Polynomial approximations | `statrs::distribution::{ChiSquared, Normal, StudentsT, FisherSnedecor}` + `ContinuousCDF::cdf` | Already wired (Phase 3); statsmodels golden equality requires CDF tails to match within 1e-10 — `statrs` does, hand-rolled won't |
| 2D OLS regression | Manual `Σxy / Σx²` two-pass | `nalgebra::SMatrix<f64, 2, 2>::lu().solve(&b)` | Stack-allocated, numerically stable, no heap; catastrophic cancellation on tight windows kills hand-rolled |
| Rolling Pearson correlation | Welford-style online update | `ndarray::Array1::windows(w).into_iter().map(|window| pearson(...))` | `ndarray-stats::CorrelationExt::pearson_correlation` is the canonical path |
| Spearman rank correlation | Hand-rolled ranks + ties | Pearson over ranks via `ndarray-stats` after `scipy.stats.rankdata`-equivalent | Tie-handling is subtle; rank-with-ties has multiple competing conventions (average / dense / ordinal) — pin one and document |
| ADF / KPSS regression infra | Inline OLS for stationarity tests | Hand-rolled is REQUIRED here (no Rust crate ports these); use `nalgebra` for the inner OLS, `statrs::Normal` for the MacKinnon p-value approximation | No comprehensive Rust stats crate covers ADF/KPSS — flagged in STATE.md "Phase 4 implementation risk" |
| Trading-session boundary picks | Bespoke time-of-day arithmetic | Document the UTC range table in §1.8 + parameterise per scan call | Industry conventions vary by venue; pin defensible defaults and let plan-phase + future v2 extend |
| Modified-z (MAD) for outliers | Naive `mean ± k*std` | Hand-rolled but use the `0.6745 * (x - median) / MAD` formula precisely (Iglewicz & Hoaglin 1993) | Naive z is non-robust to outliers (the thing we're detecting); MAD is the canonical robust alternative |
| Drawdown profile | Custom forward-fill scan | Hand-rolled is the right answer here (no stat-crate helper); use the running-max + peak-trough algorithm | Drawdown is so simple a peak-trough scan over the equity curve suffices; statsmodels has no canonical reference function |
| OU half-life for cointegrating residual (CROSS-05) | None — must hand-roll | `nalgebra` OLS on the AR(1) residual regression: `Δr_t = α + ρ * r_{t-1} + ε_t`; half-life = `-ln(2) / ln(1 + ρ)` | Standard pairs-trading derivation; not in statsmodels as a one-shot function |

**Key insight:** ADF / KPSS / Lo-MacKinlay / Engle-Granger / OU half-life / outliers / drawdown are NOT covered by any comprehensive Rust stats crate. Phase 4 hand-rolls them all, validated against scipy/statsmodels Python golden outputs. This is the "Phase 4 implementation risk" already flagged in STATE.md.

## Common Pitfalls

### Pitfall 1: Per-window finding storm

**What goes wrong:** Plan implements rolling Pearson as "one Finding per window", emits 250k findings on a 2-year 15m run.

**Why it happens:** Each window IS a result; naïve thinking maps "result" to "envelope".

**How to avoid:** Pattern 3 above — ONE finding per invocation with vector arrays in `effect.extra`. Per-window envelopes are a Phase 5 sweep concern (one job → one finding).

**Warning signs:** Integration test produces >100 findings for a single rolling-scan invocation.

### Pitfall 2: NaN propagation into envelope

**What goes wrong:** Zero-variance leg in a CROSS scan → Pearson `0/0` → NaN → `f64.to_le_bytes()` serialises the NaN bit pattern → consumer parses garbage.

**Why it happens:** `f64` NaN is sticky; Rust's `f64 / 0.0` doesn't panic, it silently NaN-propagates.

**How to avoid:** Kernel functions return `Result<KernelOutput, ScanError>`; constant-input / zero-variance branches return `Err(ScanError::Kernel("zero variance in leg b"))`. The scan body converts to `Finding::ScanError` via the engine's existing mapping (D3-24 — exit code 2 for mid-run errors).

**Warning signs:** Any `NaN` literal in a `RawArray.data` Base64 payload. The insta-snapshot diff surfaces this fast.

### Pitfall 3: Look-ahead leakage in rolling stats

**What goes wrong:** Rolling-OLS window at time T reads bars at T+1, T+2, T+3 because the window-end index is computed forward.

**Why it happens:** "Rolling window of size N centred on T" is ambiguous; the implementer picks centred when they should pick trailing.

**How to avoid:** Every rolling scan uses `ctx.bars_up_to(ts)` (NEW API per §1.5) to access bars. The window is ALWAYS trailing: `bars[t-N+1 .. t+1]` for the stat at time `t`. The shuffled-future regression test (Phase 3 `shuffled_future_regression.rs`) extends to every rolling/causal scan.

**Warning signs:** A shuffled-future regression test starts producing different bytes after a refactor.

### Pitfall 4: AIC lag-selection determinism

**What goes wrong:** ADF/KPSS pick a different lag on Linux vs macOS because of floating-point summation order in the AIC selector.

**Why it happens:** AIC is a function of log-likelihood, which involves summing residuals; summation order can drift across platforms if not pinned.

**How to avoid:** Lag selection uses sequential summation (NOT parallel reductions). The kernel iterates lag candidates in ascending order with explicit `for k in 1..=max_lag` and a sequential cumsum accumulator (same shape as Phase 3 LjungBox's Q-stat accumulator, lines 100-131 of `kernel.rs`). Document this in the kernel comment.

**Warning signs:** Golden test passes on one developer's machine and fails on CI.

### Pitfall 5: Two-leg fetch race with SIGINT

**What goes wrong:** User hits `^C` between leg-a fetch and leg-b fetch; the second fetch completes (cache hit; cheap) but the run consumed user time anyway.

**Why it happens:** Cancel is polled inside the scan body but the facade fetches BOTH bar frames before invoking `Scan::run`.

**How to avoid:** The facade polls `cancel.load(Ordering::Relaxed)` between the two `BarCache::get_or_build` calls (open question 6 resolution — recommended yes, see §1.6). If cancel is set after leg-a, return `Ok(())` without invoking the scan; the wrapper exits 130.

**Warning signs:** SIGINT integration test reports >100ms wall time after Ctrl-C in a CROSS scan invocation.

### Pitfall 6: Bid-vs-ask cross-spread legs

**What goes wrong:** A user requests `cross.corr.pearson_rolling --instrument EURUSD:bid --instrument EURUSD:ask` and expects the bid-ask spread series; gets two near-identical correlation curves of ~1.0 with subtle differences only at spread spikes.

**Why it happens:** The contract permits mixed-side legs (D4-07). The user's mental model may not match the math.

**How to avoid:** Document in each CROSS scan's `param_schema()` description that legs are per-instrument-side and the scan does NOT compute spreads (it correlates closes). Spread-specific scans are a v2 family.

**Warning signs:** User-reported "correlation is 0.999 always".

### Pitfall 7: Schemars 1.x `Vec<T>` non-additive surprise

**What goes wrong:** Plan changes `instrument: String` to `instruments: Vec<InstrumentSpec>`; schemars emits the field as `"type":"array"` where the old schema had `"type":"string"`; schema-sync CI gate fails the `git diff --exit-code schemas/` check.

**Why it happens:** Type changes ARE breaking even when the underlying semantics are "1 → N". JSON Schema diff doesn't understand "N-ary generalisation".

**How to avoid:** §1.1 below is the recipe — regen the schema in a scratch task BEFORE committing the field type change; if non-additive, invoke the D4-03-ALT fallback (keep `source: Source`, add `peer_sources: Vec<Source>` sibling).

**Warning signs:** xtask `gen-schema` run produces non-empty diff against the committed artifact in a non-scoped way.

## Runtime State Inventory

Phase 4 is greenfield (no rename / refactor / migration of stored data, services, OS-registered state, secrets, or build artifacts). The only runtime state the phase touches is:

| Category | Items Found | Action Required |
|----------|-------------|------------------|
| Stored data | None — Phase 4 reads existing Arrow IPC derived-bar cache (CACHE-06) and writes findings to stdout only. No persistent results store. | None |
| Live service config | None — miner is a CLI binary; no daemons, no service registry. Phase 4's wrappers (MCP/HTTP) come in Phase 6. | None |
| OS-registered state | None — no scheduled tasks, pm2, launchd, systemd registration. | None |
| Secrets/env vars | None — Phase 4 introduces no new env vars or secrets. Existing `MinerConfig` env precedence (FOUND-05) covers paths only. | None |
| Build artifacts | One: `schemas/findings-v1.schema.json` + `schemas/scans-catalogue-v1.schema.json` regenerate via `cargo xtask gen-schema`. Phase 4 commits the new artifacts in plan-final. | Regen + commit in plan 04-5 |

**Step 2.5 SKIPPED-PER-REASON:** Phase 4 is a catalogue scale-out, not a rename/refactor. No state to migrate.

## Common Pitfalls (continued — Phase-4-specific that the planner must thread into verification)

### Pitfall 8: D4-03 vs D4-03-ALT cliff edge

**What goes wrong:** Plan commits to D4-03 (`sources: Vec<Source>`) without running the schema regen first; CI fails on the merge; team scrambles.

**How to avoid:** Plan task 1 of Plan 1 (facade-shape extension) is "spike the schema regen against a feature-branch with the type change applied; record the diff; decide D4-03 vs D4-03-ALT BEFORE writing any callsite code." This is a single-day spike and pays for itself.

### Pitfall 9: Phase 3 LjungBox golden divergence on ANOM-01 refactor

**What goes wrong:** D4-06's "refactor LjungBox to call ANOM-01 primitive" subtly changes log-returns arithmetic (e.g., the new primitive uses `(close[t] - close[t-1]) / close[t-1]` instead of `ln(close[t] / close[t-1])`).

**How to avoid:** ANOM-01 returns primitive's `log_returns` function MUST be a BYTE-IDENTICAL move of Phase 3's `crates/miner-core/src/scan/ljung_box/kernel.rs::log_returns` function (lines 26-28). Plan task wording: "move, do not rewrite". The existing Phase 3 LjungBox golden test re-runs unchanged and must pass.

## Code Examples

### Resolving per-scan params with defaults (verbatim from Phase 3)

```rust
// Source: crates/miner-core/src/scan/ljung_box/mod.rs:253-274
fn resolve_lags(req: &ScanRequest, n: usize) -> Result<usize, ScanError> {
    let raw = req.resolved_params.get("lags");
    let lags_i64: i64 = match raw {
        Some(v) => v.as_i64()
            .ok_or_else(|| ScanError::Kernel(format!("lags must be an integer; got {v}")))?,
        None => default_lags(n),
    };
    if lags_i64 < 1 { return Err(ScanError::Kernel(format!("lags must be >= 1; got {lags_i64}"))); }
    let lags_us = usize::try_from(lags_i64)
        .map_err(|_| ScanError::Kernel(format!("lags out of range for usize: {lags_i64}")))?;
    if lags_us >= n {
        return Err(ScanError::Kernel(format!("lags must be < n; got lags={lags_us}, n={n}")));
    }
    Ok(lags_us)
}
```

### Sequential summation for AIC lag selection (Pitfall 4 prevention)

```rust
// Pattern (NOT existing code): ADF AIC lag selection
fn select_lag_aic(returns: &[f64], max_lag: usize) -> usize {
    let mut best_aic = f64::INFINITY;
    let mut best_lag = 1usize;
    // Sequential — NOT par_iter — to keep determinism across platforms.
    for k in 1..=max_lag {
        let aic = fit_adf_at_lag(returns, k).aic;
        if aic < best_aic {
            best_aic = aic;
            best_lag = k;
        }
    }
    best_lag
}
```

### Two-leg time alignment via inner-join (CROSS-01 primitive)

```rust
// Pattern (NEW Phase 4): inner-join two BarFrames on common ts_open_utc
pub struct AlignedPair {
    pub timestamps_ms: Vec<i64>,
    pub close_a: Vec<f64>,
    pub close_b: Vec<f64>,
}

pub fn inner_join(a: &BarFrame, b: &BarFrame) -> Result<AlignedPair, ScanError> {
    let mut ia = 0usize;
    let mut ib = 0usize;
    let mut out_ts: Vec<i64> = Vec::new();
    let mut out_a: Vec<f64> = Vec::new();
    let mut out_b: Vec<f64> = Vec::new();
    while ia < a.ts_open_utc.len() && ib < b.ts_open_utc.len() {
        let ta = a.ts_open_utc[ia].timestamp_millis();
        let tb = b.ts_open_utc[ib].timestamp_millis();
        match ta.cmp(&tb) {
            std::cmp::Ordering::Less    => ia += 1,
            std::cmp::Ordering::Greater => ib += 1,
            std::cmp::Ordering::Equal   => {
                out_ts.push(ta);
                out_a.push(a.close[ia]);
                out_b.push(b.close[ib]);
                ia += 1; ib += 1;
            }
        }
    }
    Ok(AlignedPair { timestamps_ms: out_ts, close_a: out_a, close_b: out_b })
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `Option<peer_instrument>` sibling field | `instruments: Vec<InstrumentSpec>` (D4-01) | Phase 4 | Uniform shape for 1-leg + 2-leg; extends to v2 baskets |
| `Source` struct singleton in `DataSlice` | `Vec<Source>` parallel to `instruments` (D4-03) | Phase 4 | Self-describing per-finding leg provenance |
| Inline log-returns inside LjungBox (D3-02) | Reusable `primitives::returns` kernel (D4-05 / D4-06) | Phase 4 | One canonical returns implementation across 22 scans |
| One-finding-per-window for rolling stats (tempting naïve shape) | One finding with vector arrays in `effect.extra` | Phase 4 | Bounded stdout throughput; preserves Phase 3 single-shot facade |

**Deprecated/outdated:**

- Single-instrument `ScanRequest.instrument: String + side: Side` (Phase 3) — replaced by `instruments: Vec<InstrumentSpec>` (D4-01).
- Privileged-primary `DataSlice.source: Source` — replaced by `sources: Vec<Source>` (D4-03) OR (fallback) `source` + `peer_sources: Vec<Source>` (D4-03-ALT).

---

# Section 1: Open-Question Resolutions

The ten open questions from CONTEXT.md §open_questions are closed below. Each resolution has: **Recommendation**, **Rationale**, **Risk**, and **Fallback**.

## 1.1 Schemars 1.x schema-additive verification for D4-01 + D4-03

**Recommendation:** Commit to D4-01 (`instruments: Vec<InstrumentSpec>`) and D4-03 (`sources: Vec<Source>`) ONLY after running `cargo run -p xtask -- gen-schema` against a feature-branch with the type change applied and inspecting the diff. **Schemars 1.x emits `Vec<T>` as `{"type":"array","items":{"$ref":"#/$defs/T"}}` which IS a breaking change from `{"type":"string"}` or `{"$ref":"#/$defs/Source"}`.** The schema-sync CI gate (`git diff --exit-code schemas/`) WILL fail.

**Process to follow** (plan task 04-1, sub-task 1):

```bash
# 1. Apply the field-type change on a scratch branch.
git checkout -b spike/phase4-schema-regen

# 2. Edit src/findings/mod.rs::DataSlice.source -> sources: Vec<Source>
# 3. Edit src/scan/mod.rs::ScanRequest.instrument+side -> instruments: Vec<InstrumentSpec>
# 4. Edit src/findings/mod.rs::Source addition: add InstrumentSpec struct

# 5. Regen schemas.
cargo run -p xtask -- gen-schema

# 6. Diff against committed artifact.
git diff schemas/findings-v1.schema.json | tee /tmp/schema-diff.txt

# 7. Inspect: if the diff is ONLY additive (new fields, new $defs, no
#    REMOVED top-level keys), proceed with D4-01 + D4-03.
#    If the diff REMOVES the `source` field or REMOVES `instrument`+`side`,
#    invoke D4-03-ALT below.
```

**Rationale:** Schemars 1.x derives `JsonSchema` from struct fields one-for-one. A `Vec<T>` field emits `{"type":"array","items":...}`. The OLD `source: Source` field emitted `{"$ref":"#/$defs/Source"}`. Replacing one with the other REMOVES the `source` key from the schema's `properties` map — that is a top-level breaking change regardless of how schema diff tools classify it. The project's CI gate is `git diff --exit-code schemas/`, which treats ANY non-empty diff as failure.

**Verdict (HIGH confidence based on schemars 1.x derive semantics and the existing committed `schemas/findings-v1.schema.json` which uses `"$ref": "#/$defs/Source"` for the singleton field):** D4-01 and D4-03 are NOT additive changes to the JSON Schema artifact. The schema-sync gate will flag them as "schema_version-bumping" candidates UNLESS the plan elects either:

- **(a)** accept the schema regen as a deliberate Phase-4 facade-shape update; commit the new schema; document in CONTEXT/STATE that this is NOT a `schema_version` bump (the `schema_version` field stays `1` because consumers parsing `kind:result` envelopes can still parse Phase-3-shaped outputs through the same code path with one extra Vec layer). The plan-task wording: "additive at the schema-versioning level even though the JSON Schema artifact diffs." This is the recommended path.
- **(b)** Invoke **D4-03-ALT**: keep `source: Source` field for the primary leg, add an additive sibling `peer_sources: Vec<Source>` (default empty Vec). This IS purely additive at the schemars level.

**Risk if wrong:** Picking (a) when the schema-sync CI gate is genuinely strict about field-name removal blocks the merge. Mitigation: run the spike FIRST (sub-task 1 of plan 04-1).

**Fallback (D4-03-ALT):** If the team decides the diff is too disruptive, the alt shape is:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DataSlice {
    pub range: TimeRange,
    pub gap_manifest_ref: Option<String>,
    #[serde(default)]
    pub gap_manifest: Option<GapManifest>,
    pub source: Source,                              // unchanged — primary leg
    #[serde(default)]
    pub peer_sources: Vec<Source>,                   // NEW — additive; empty Vec for single-leg
}

// And on ScanRequest:
pub struct ScanRequest {
    // ...
    pub instrument: String,                          // unchanged — primary leg
    pub side: Side,                                  // unchanged
    #[serde(default)]
    pub peer_instruments: Vec<InstrumentSpec>,       // NEW — additive
    // ...
}
```

This is structurally less clean (asymmetric privileged-primary leg) but is schema-strictly additive. Recommend (a) — accept the diff — unless the plan-phase finds a hard constraint blocking it.

**Source provenance:** [VERIFIED: existing `schemas/findings-v1.schema.json` line 65+ shows `"source": {"$ref": "#/$defs/Source"}`; schemars 1.x derive shape from `crates/miner-core/Cargo.toml` line 29 + xtask/main.rs line 128 schemars `schema_for!` call.] [CITED: docs.rs/schemars/1 — `Vec<T>` field emits `array` type.]

## 1.2 Reference-implementation pinning policy

**Recommendation:** Pin a SINGLE version of statsmodels + scipy + arch for the entire Phase 4 golden suite. Record in `crates/miner-core/tests/goldens/REFERENCE-VERSIONS.md`:

```text
# Reference implementation versions (pinned for Phase 4 golden generation)
# DO NOT change without simultaneously regenerating EVERY golden.

statsmodels==0.14.6        # ANOM-04..09 + CROSS-03 RollingOLS + CROSS-05 coint
                            # already pinned by Phase 3 LjungBox golden — keep
scipy==1.14.1              # ANOM-09 jarque_bera + ANOM-10 zscore + CROSS-04 signal.correlate
                            # + SEAS-05 f_oneway + scipy.stats.kruskal
arch==7.0.0                # ANOM-07 variance_ratio (Lo-MacKinlay) ONLY — not in statsmodels core
numpy==1.26.4              # transitive — pin for byte-stable cumsum order
python==3.11.x             # transitive — interpreter-version reproducibility
```

**Rationale:** Phase 3 already pinned statsmodels 0.14.6 for the LjungBox golden ([CITED: Phase 3 CONTEXT D3-04..D3-05 + LjungBox kernel.rs:12-14]). Keeping the SAME version for the other 21 scans means goldens regenerate in one Python session and tolerance tuning is consistent.

scipy 1.14.x pairs cleanly with statsmodels 0.14.6 (statsmodels declares scipy>=1.8,<2 in setup.py for 0.14.x). Recommend `scipy==1.14.1` specifically — released late 2024, stable, no known issues with the functions ANOM-09 / ANOM-10 / SEAS-05 use.

arch 7.0.0 (released 2024) is needed ONLY for ANOM-07 (Lo-MacKinlay variance ratio) — statsmodels does not implement it. The `arch.unitroot.VarianceRatio` class is the canonical reference. [ASSUMED — confirm exact arch version when generating the ANOM-07 golden; the test reproducibility hinges on this.]

numpy 1.26.4 is the version statsmodels 0.14.6 ships against; pin to prevent silent cumsum-order drift across numpy versions.

**Risk if wrong:** Phase-3 LjungBox golden could regenerate to slightly different bytes if statsmodels minor-bumps before Phase 4 ships. Mitigation: lock-file the Python virtualenv used for golden generation (commit a `crates/miner-core/tests/goldens/python-requirements.lock`).

**Fallback:** Per-scan pinning — list each scan's pinned versions in the per-scan integration test header. More flexible but harder to audit; not recommended.

**Source provenance:** [VERIFIED: existing Phase 3 LjungBox kernel.rs comments and CONTEXT D3-05 confirm statsmodels 0.14.6 pin.] [CITED: pypi.org/project/statsmodels/0.14.6 — declares scipy dependency.] [ASSUMED: scipy 1.14.1 + arch 7.0.0 exact version pin — confirm against PyPI release dates during golden generation.]

## 1.3 `effect.value` canonical pick per scan (all 22)

Full per-scan table — see §2 below. Brief summary of the canonical picks per scan family:

- **Single-shot scans** (most ANOM, all CROSS-05 / SEAS-05): `effect.value` = the canonical scalar (test statistic, hedge ratio, F-stat).
- **Rolling/vector-output scans** (CROSS-02, CROSS-03, CROSS-04, ANOM-03): `effect.value` = **last window value** (trader intuition: "where is the stat right now?"). Full vector in `effect.extra.values`. p_value = None (no headline p; per-window p's in `effect.extra.p_values` if applicable).
- **Bucketed scans** (SEAS-01, SEAS-02, SEAS-03, SEAS-04): `effect.value` = **max absolute t-stat across buckets** (the strongest bucket signal). Full bucket arrays in `effect.extra.{buckets, means, stds, counts, t_stats}`. p_value = None (multi-test correction is Phase 5 / HYG-02's job).
- **Multi-test scans** (SEAS-05 ANOVA + Kruskal-Wallis): emit BOTH stats — `effect.value` = ANOVA F-stat, `effect.p_value` = ANOVA p-value; KW stat + p in `effect.extra.{kw_stat, kw_p_value}`.

See §2 for the full per-scan locked picks.

## 1.4 ANOM-01 returns primitive surface

**Recommendation:** ONE registered scan `stats.returns.profile@1` with `--params variant=log|simple|intraday|overnight` (default `log`). Plus the kernel utility module `crates/miner-core/src/scan/primitives/returns.rs` that BOTH the standalone scan AND all other ANOM/CROSS scans import.

**Rationale:**

- Four separate scans would clutter the `miner scans` catalogue with near-duplicate entries; agent UX for "list me the available scans" suffers.
- One scan + `variant` parameter mirrors the statsmodels convention (one function with kwargs).
- `param_hash` distinguishes the four variants at the envelope level — finding provenance stays intact.
- Quant agent ergonomics: one call site, four variants — agent picks variant via params, doesn't have to memorise four scan IDs.

**Module path:** `crates/miner-core/src/scan/primitives/returns.rs` (kernel) + `crates/miner-core/src/scan/anom/returns.rs` (callable scan). The callable scan's `Scan::id() = "stats.returns.profile"`, version 1.

**Risk if wrong:** Future agent might prefer four discoverable scan IDs over one parametric scan. Mitigation: the variant is in `effect.extra.variant_label` so the agent can grep by variant cleanly anyway. v2 can split into four scans additively if needed.

**Fallback:** Four registered scans `stats.returns.{log,simple,intraday,overnight}@1`. Costs are: 4 vs 1 catalogue lines; 4× the `param_schema()` hand-rolling; slightly easier agent discoverability per variant.

**Source provenance:** [CITED: CONTEXT.md D4-05 — "Plan picks the exact scan name and decides one scan vs four"]

## 1.5 Look-ahead-safety API shape on `ScanCtx`

**Recommendation:** **Shape (a) — `ScanCtx::bars_up_to(ts: DateTime<Utc>) -> &BarFrameView<'_>`** where `BarFrameView<'_>` is a thin column-slice newtype (NOT a `Vec<BarRow>`).

**Rationale:**

- **(a) slice / view:** Zero allocation per call (returns a borrow into the underlying `BarFrame`'s columns). Best ergonomics for rolling kernels that iterate via `bars.close.iter().take_while(...)`. ✓
- **(b) sub-frame (`BarFrame`-typed):** Cleanest semantic ("here is the bar frame at this moment in time") but ALLOCATES on every call inside the hot rolling loop. ✗
- **(c) `Range<usize>`:** Smallest possible return but pushes index arithmetic into every kernel — error-prone, defeats the look-ahead-safety guarantee at the type level. ✗

**Concrete shape:**

```rust
// crates/miner-core/src/scan/mod.rs
impl<'a> ScanCtx<'a> {
    /// Returns a view onto the bar frame's columns truncated at or before `ts`.
    /// Look-ahead-safe: every column slice ends at the largest index `i` where
    /// `bars.ts_open_utc[i] <= ts`.
    pub fn bars_up_to(&self, ts: DateTime<Utc>) -> BarFrameView<'_> {
        let cutoff = self.bars.ts_open_utc.partition_point(|t| *t <= ts);
        BarFrameView {
            source_id: &self.bars.source_id,
            symbol: &self.bars.symbol,
            side: self.bars.side,
            tf: self.bars.tf,
            ts_open_utc: &self.bars.ts_open_utc[..cutoff],
            close: &self.bars.close[..cutoff],
            // ... open / high / low / tick_volume slices ...
        }
    }
}

pub struct BarFrameView<'a> {
    pub source_id: &'a str,
    pub symbol: &'a str,
    pub side: Side,
    pub tf: Timeframe,
    pub ts_open_utc: &'a [DateTime<Utc>],
    pub close: &'a [f64],
    pub open: &'a [f64],
    pub high: &'a [f64],
    pub low: &'a [f64],
    pub tick_volume: &'a [f64],
}
```

The `partition_point` call is O(log n) per `bars_up_to` invocation — cheaper than `binary_search` (no `Ordering` allocation) and the standard idiom for "find first index where predicate fails" since Rust 1.52.

**Risk if wrong:** Hot rolling loops that call `bars_up_to` per window become O(N log N) instead of O(N). Mitigation: the scan body holds the `BarFrameView` once and iterates internally, OR maintains a cursor — both are zero-cost.

**Fallback:** Shape (b) sub-frame with explicit `BarFrame::clone()` if the view approach hits lifetime issues with the existing `ScanCtx<'a>` borrow. Document the per-call allocation cost in the implementation comment.

**Source provenance:** [VERIFIED: `crates/miner-core/src/scan/mod.rs` line 128 already defines `ScanCtx<'a> { bars: &'a BarFrame, ... }` — the borrow lifetime is in place.] [CITED: Rust 1.52 stabilised `slice::partition_point`.]

## 1.6 Two-leg `BarCache::get_or_build` call ordering + cancel polling

**Recommendation:** **Poll cancel BETWEEN the two fetches.** Cheap insurance — `AtomicBool::load(Ordering::Relaxed)` is a single dependent load (~1 ns). Pattern:

```rust
// crates/miner-core/src/engine/mod.rs (or run_one.rs)
let bars_a = cache.get_or_build(reader, AggParams { symbol: &req.instruments[0].symbol,
                                                    side: req.instruments[0].side, tf: req.timeframe })?;
if cancel.load(Ordering::Relaxed) {
    return Ok(RunOutcome::Cancelled);     // facade emits no RunEnd; CLI exits 130
}
let bars_b = cache.get_or_build(reader, AggParams { symbol: &req.instruments[1].symbol,
                                                    side: req.instruments[1].side, tf: req.timeframe })?;
if cancel.load(Ordering::Relaxed) {
    return Ok(RunOutcome::Cancelled);
}
// Proceed to gap-intersect + Scan::run.
```

**Rationale:** A cold cache rebuild of one BarFrame can take seconds (zstd decompress + CSV parse + aggregate + Arrow write). Polling cancel between the two fetches lets the second fetch be skipped if the user already hit Ctrl-C. Cost is one atomic load; benefit is responsive SIGINT under CROSS scans.

**Risk if wrong:** None measurable — `AtomicBool::load(Ordering::Relaxed)` is among the cheapest operations on x86_64.

**Fallback:** Poll only after both fetches complete. Saves one atomic load; loses interruptibility during the second fetch.

**Source provenance:** [VERIFIED: cancel polling pattern from `crates/miner-core/src/scan/ljung_box/mod.rs:112` `if ctx.cancel.load(Ordering::Relaxed) { return Ok(()); }` — same idiom.]

## 1.7 Hedge-ratio sign convention + `effect.value` direction for Engle-Granger CROSS-05

**Recommendation:** `effect.value` = β from the regression `y_t = α + β * x_t + ε_t` where **y = leg_a = req.instruments[0]** and **x = leg_b = req.instruments[1]**. The statsmodels `coint(y, x)` function takes `(y0, y1)` with y0 as the regressand and y1 as the regressor; aligning our leg order with that convention means `coint(close_a, close_b)` produces the β we report.

**Concrete envelope:**

```rust
let effect = Effect {
    metric: "engle_granger_hedge_ratio".into(),
    value: beta,                                // β from a ~ b regression
    p_value: Some(adf_p_value),                 // ADF on residuals
    n: Some(aligned_n as u64),
    ci95: None,
    extra: {
        let mut m = BTreeMap::new();
        m.insert("hedge_ratio_alpha".into(),    f64_slice_to_raw_array(&[alpha]));
        m.insert("adf_stat".into(),             f64_slice_to_raw_array(&[adf_stat]));
        m.insert("ou_half_life".into(),         f64_slice_to_raw_array(&[half_life]));
        m.insert("residual_std".into(),         f64_slice_to_raw_array(&[residual_std]));
        m.insert("residuals".into(),            f64_slice_to_raw_array(&residuals));
        m
    },
};
```

**Rationale:** statsmodels' `coint(y0, y1)` documentation specifies y0 is the dependent variable. Mapping leg order in `req.instruments` to (regressand, regressor) order matches the agent's "first instrument is the one we want to explain" intuition.

**Risk if wrong:** Sign error in agent backtests — β with wrong sign produces a short-when-long signal. Mitigation: integration test pins the convention with a hand-derived golden where the leg ordering is documented.

**Fallback:** None — picking is the right thing; the direction MUST be locked and documented.

**Source provenance:** [CITED: statsmodels.tsa.stattools.coint docstring — `coint(y0, y1, ...)` first argument is regressand. URL: statsmodels.org/dev/generated/statsmodels.tsa.stattools.coint.html]

## 1.8 Session-boundary defaults for SEAS-03

**Recommendation:** Document UTC session boundaries as defaults; parameterise per-call via `--params session_a_start_utc_hour=22 session_a_end_utc_hour=7` etc.

```text
Asia:    22:00–07:00 UTC  (Tokyo open ~00:00 UTC, Sydney earlier; overlap with London at start)
London:  07:00–16:00 UTC  (LSE open ~08:00 BST/07:00 UTC nominal; closes ~16:30 BST/15:30 UTC,
                            rounded up)
NY:      12:00–21:00 UTC  (NYSE open 14:30 UTC nominal; the FX-major convention starts NY at
                            12:00 UTC to capture pre-NYSE liquidity)
overlap: 12:00–16:00 UTC  (intersection of London + NY — the highest-liquidity FX window)
```

**Rationale:** These are FX-major (EUR/USD, GBP/USD, USD/JPY) conventions used by interdealer platforms (ICAP, EBS) and by retail FX desks. They are NOT exchange-equity sessions. Documented in CME's "FX Trading Hours" guidance and matches the convention `tradedesk-dukascopy` calendar already uses for intra-day gap detection.

Sessions overlap (overlap is a slice of London+NY); each session is a parameterisable interval, not an exclusive bucket. SEAS-03 emits four bucket entries (Asia, London, NY, overlap) regardless of overlap with each other.

**Risk if wrong:** Wrong session boundaries produce wrong agent hypotheses. Low risk — these are widely-used defaults; document the source explicitly in the scan's `param_schema().description`.

**Fallback:** Caller-supplied session list via `--params sessions='[{"name":"asia","start_utc_h":22,"end_utc_h":7},...]'`. Plan picks the JSON shape.

**Source provenance:** [CITED: CME Group — "FX Futures Trading Hours" reference. URL: cmegroup.com/markets/fx/trading-hours.html] [CITED: Dukascopy SWFX trading-session conventions match.] [ASSUMED] for exact UTC hour cutoffs — confirm with `tradedesk-dukascopy` calendar source before locking.

## 1.9 CROSS-04 lead-lag CCF lag grid default

**Recommendation:** Default lag grid is **symmetric ±N around zero**, with **N tied to timeframe**:

| Timeframe | Default N | Rationale |
|-----------|-----------|-----------|
| `1m` | 50 | ~50 minutes of lookback — captures intra-hour lead-lag patterns |
| `15m` | 20 | ~5 hours either direction — captures intraday session leads |
| `1h` | 10 | ~10 hours — captures within-day to overnight effects |
| `1d` | 7 | ±1 week — captures weekly cycles |

User-overridable via `--params max_lag=N`. CCF computed via `scipy.signal.correlate(a, b, mode='full')`-equivalent in Rust (hand-rolled or via `ndarray` slice arithmetic). The Rust kernel returns a vector of length `2*N+1`; index `N` corresponds to lag-0; indices `[0..N]` correspond to a-leads-b; indices `[N+1..2*N+1]` correspond to b-leads-a.

**Rationale:** Symmetric grid is standard in signal processing. N scaling by timeframe keeps the time-coverage of the lag window roughly constant across timeframes. Argmax-lag is reported in `effect.value`; full vector in `effect.extra.ccf_values` + lag axis in `effect.extra.lags`.

**Risk if wrong:** Very-high N on 1m data produces a 100-element vector — bounded throughput.

**Fallback:** Single global default (e.g., `N=20`), let user override per call. Less defensible against "the default is wrong for 1m data".

**Source provenance:** [CITED: `scipy.signal.correlate` docstring + `statsmodels.tsa.stattools.ccf` source.]

## 1.10 `Scan::arity()` enum variants

**Recommendation:** Ship `ScanArity::{Single, Pair}` (2 variants) for Phase 4. The additive-extension story for v2: add `Many(min: u8, max: u8)` as a new enum variant when basket scans land. Schemars treats new enum variants as additive (the wire-form `oneOf` gains a new option without invalidating existing parsers).

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ScanArity {
    /// Single-leg scan (ANOM / SEAS family). `instruments.len() == 1`.
    Single,
    /// Two-leg scan (CROSS family). `instruments.len() == 2`.
    Pair,
    // v2: Many(min, max) — basket scans (Johansen etc.)
}
```

**Rationale:** Minimum API surface for Phase 4 (no scan in this phase needs >2 legs). Two variants are unambiguous in CLI rendering. v2 can add `Many` additively because the wire form is `"single"` / `"pair"` strings — adding `{"many": {"min": 3, "max": 5}}` is a new struct-variant case in the `oneOf`.

**Risk if wrong:** v2 basket scans may discover that `Many(min, max)` is the wrong shape (e.g., they need `Many(exact)` or `Many(min)` open-ended). Easy to refine — add variants additively.

**Fallback:** Ship `ScanArity::{Single, Pair, Many(min: u8, max: u8)}` from day one. Costs: more variants to match in preflight (`Many` is currently unused — dead code lint). Recommend NOT pre-shipping `Many` unless plan has a concrete use within Phase 4.

**Source provenance:** [CITED: CONTEXT.md D4-02 — "initial variants Single and Pair; can extend to Many(min, max) if v2 baskets need it"]

---

# Section 2: Per-Scan Reference + Tolerance Table

| scan_id | algorithm | reference call | `effect.value` | `effect.extra` keys | emission | tolerance | look-ahead-safe? |
|---------|-----------|----------------|----------------|---------------------|----------|-----------|------------------|
| `stats.returns.profile@1` (ANOM-01) | log/simple/intraday/overnight returns | hand-rolled per CLAUDE.md `windows(2).map(\|w\| (w[1]/w[0]).ln())` | mean log-return | `returns_vector`, `variant_label`, `mean`, `std`, `n` | single-shot | n/a (no statsmodels reference) | yes |
| `stats.summary.welford@1` (ANOM-02) | mean/std/skew/excess-kurtosis/IQR/min/max via Welford | hand-rolled; golden via `scipy.stats.describe` + `scipy.stats.iqr` | mean | `std`, `skew`, `excess_kurtosis`, `iqr`, `min`, `max`, `n` | single-shot | 1e-10 | yes |
| `stats.vol.rolling@1` (ANOM-03) | rolling std + vol-of-vol | hand-rolled rolling std; golden via `pandas.Series.rolling(W).std(ddof=1)` | last_window_vol | `values`, `vol_of_vol`, `window_starts_ms`, `window_length` | one finding per invocation (vector in extra) | 1e-10 | yes (rolling) |
| `stats.autocorr.ljung_box@1` (ANOM-04 returns) | Ljung-Box on returns | `statsmodels.stats.diagnostic.acorr_ljungbox(returns, lags=L, return_df=True)` | Q at max lag | `lags`, `q_stats`, `p_values`, `acf` | single-shot | 1e-10 (q), 1e-8 (p — chi2 CDF) | yes |
| `stats.autocorr.ljung_box_sq@1` (ANOM-04 sq returns) | Ljung-Box on SQUARED returns | same as above, input = returns² | Q at max lag | same as above + `series_kind="squared_returns"` | single-shot | 1e-10 / 1e-8 | yes |
| `stats.stationarity.adf@1` (ANOM-05) | ADF with AIC lag selection | `statsmodels.tsa.stattools.adfuller(returns, autolag='AIC', regression='c')` | ADF test statistic | `p_value`, `lag_selected`, `crit_values`, `nobs`, `regression` | single-shot | 1e-8 (MacKinnon p approx) | yes |
| `stats.stationarity.kpss@1` (ANOM-06) | KPSS (regression='c' default) | `statsmodels.tsa.stattools.kpss(returns, regression='c', nlags='auto')` | KPSS statistic | `p_value`, `lag_truncation`, `crit_values`, `regression` | single-shot | 1e-8 (interpolated p) | yes |
| `stats.variance_ratio.lo_mackinlay@1` (ANOM-07) | Lo-MacKinlay VR(k) over k grid | `arch.unitroot.VarianceRatio(returns, lags=k, robust=True)` for each k in [2, 4, 8, 16] (default) | VR at max k | `k_values`, `vr_values`, `z_stats`, `p_values` | single-shot (vector of k results in extra) | 1e-8 | yes |
| `stats.heteroskedasticity.arch_lm@1` (ANOM-08) | ARCH-LM | `statsmodels.stats.diagnostic.het_arch(returns, nlags=L)` (default L=5 per Engle 1982) | LM statistic | `p_value`, `lag`, `f_statistic`, `f_p_value` | single-shot | 1e-10 | yes |
| `stats.normality.jarque_bera@1` (ANOM-09) | Jarque-Bera | `scipy.stats.jarque_bera(returns)` | JB statistic | `p_value`, `skew`, `excess_kurtosis`, `n` | single-shot | 1e-10 (chi2 with df=2) | yes |
| `stats.outliers.z_and_mad@1` (ANOM-10) | z-score + modified-z (MAD-based) | `scipy.stats.zscore(returns)` + hand-rolled `0.6745*(x-median)/MAD` per Iglewicz-Hoaglin | count of outliers at z>3 | `z_threshold`, `mad_threshold`, `outlier_indices`, `outlier_values_z`, `outlier_values_modified_z`, `mad`, `median` | single-shot (vector indices in extra) | 1e-12 (pure arithmetic) | yes |
| `stats.drawdown.profile@1` (ANOM-11) | peak-trough scan on cumulative log-returns | hand-rolled; no statsmodels reference | max_drawdown | `equity_curve`, `peaks`, `troughs`, `drawdown_durations_ms`, `time_to_recover_ms`, `dd_distribution_p50_p95_p99` | single-shot | 1e-12 (arithmetic) | yes |
| `cross.align.inner_join@1` (CROSS-01) | inner-join on common timestamps | KERNEL ONLY — not a registered callable scan; primitive consumed by CROSS-02..05 | n/a | n/a | n/a | n/a | yes |
| `cross.corr.pearson_rolling@1` (CROSS-02 Pearson) | rolling Pearson over window W | hand-rolled rolling `numpy.corrcoef` | last_window_corr | `values`, `window_starts_ms`, `window_length`, `threshold_crossings`, `threshold` | one finding per invocation (vector) | 1e-10 | yes (rolling) |
| `cross.corr.spearman_rolling@1` (CROSS-02 Spearman) | rolling Spearman over window W | hand-rolled rolling `scipy.stats.spearmanr` | last_window_corr | same as Pearson + tie-handling note | one finding per invocation (vector) | 1e-8 (rank ties) | yes (rolling) |
| `cross.ols.rolling@1` (CROSS-03) | rolling OLS β / α / R² / residual std | `statsmodels.regression.rolling.RollingOLS(y, sm.add_constant(x), window=W)` golden; in-Rust via `nalgebra::SMatrix<f64,2,2>` per window | last_window_beta | `betas`, `alphas`, `r2s`, `residual_stds`, `window_starts_ms`, `window_length` | one finding per invocation (vectors) | 1e-9 | yes (rolling) |
| `cross.lead_lag.ccf@1` (CROSS-04) | lead-lag cross-correlation function over ±N | `scipy.signal.correlate(a, b, mode='full')` normalised; or `statsmodels.tsa.stattools.ccf` | argmax_lag (integer-valued f64) | `lags`, `ccf_values`, `argmax_lag`, `argmax_value`, `max_lag` | single-shot (vector in extra; phase-scramble null deferred to Phase 5) | 1e-10 | yes |
| `cross.cointegration.engle_granger@1` (CROSS-05) | EG two-step + OU half-life on residual | `statsmodels.tsa.stattools.coint(y=close_a, x=close_b)` + hand-rolled OU AR(1) on residuals | β (hedge ratio, sign per §1.7) | `hedge_ratio_alpha`, `adf_stat`, `ou_half_life`, `residual_std`, `residuals` | single-shot | 1e-8 (coint MacKinnon table); 1e-10 (β, α) | yes |
| `seas.bucket.hour_of_day@1` (SEAS-01) | hour-of-day return + vol profile | hand-rolled bucket-by `ts_open_utc.hour()`; golden via `pandas.groupby(returns.index.hour).agg([mean,std,count])` | max_abs_t_stat | `buckets` (0..23), `means`, `stds`, `counts`, `t_stats`, `iqrs` | single-shot (bucket vectors in extra) | 1e-12 (pure aggregation) | yes |
| `seas.bucket.day_of_week@1` (SEAS-02) | day-of-week effect | hand-rolled bucket-by `ts_open_utc.weekday()`; golden via `pandas.groupby(returns.index.dayofweek).agg(...)` | max_abs_t_stat | `buckets` (0=Mon..6=Sun), `means`, `stds`, `counts`, `t_stats` | single-shot | 1e-12 | yes |
| `seas.bucket.session@1` (SEAS-03) | trading-session bucketing (Asia/London/NY/overlap) | hand-rolled per §1.8 UTC ranges | max_abs_t_stat | `buckets` (string labels), `means`, `stds`, `counts`, `t_stats`, `session_boundaries_utc` | single-shot | 1e-12 | yes |
| `seas.bucket.eom_som@1` (SEAS-04) | EOM/SOM by trading-day-of-month | hand-rolled trading-day-of-month bucket; default N=3 (last 3 + first 3) | max_abs_t_stat | `buckets` ("EOM-3" .. "EOM-1", "SOM-1" .. "SOM-3"), `means`, `stds`, `counts`, `t_stats`, `cutoff_n` | single-shot | 1e-12 | yes |
| `seas.test.anova_kruskal@1` (SEAS-05) | One-way ANOVA + Kruskal-Wallis | `scipy.stats.f_oneway(group_a, group_b, ...)` + `scipy.stats.kruskal(group_a, group_b, ...)` | ANOVA F-stat | `anova_p_value`, `kw_stat`, `kw_p_value`, `group_count`, `total_n` | single-shot | 1e-8 (parametric); 1e-6 (KW rank ties) | yes |
| `seas.event.pre_post_window@1` (SEAS-06) | caller-supplied event-timestamp aligned pre/post stats | hand-rolled; no statsmodels reference (event-study micro implementations vary by paper) | mean_post_event_return | `pre_window_means`, `post_window_means`, `pre_window_stds`, `post_window_stds`, `event_count`, `pre_window_bars`, `post_window_bars` | single-shot (vectors keyed by event index in extra) | 1e-12 (arithmetic) | yes |

**Total registered scans:** 23 (22 from REQUIREMENTS.md + 1 squared-returns variant of LjungBox per ANOM-04). CROSS-01 is the only kernel-only primitive (no registry entry — it's the inner-join utility CROSS-02..05 import).

**Notes on tolerance picks:**

- **1e-10/1e-12 (strict):** Pure-arithmetic kernels (returns, summary stats, drawdown, bucketing, outlier detection). Byte-exact equality is achievable when summation order is pinned.
- **1e-8 (standard):** Statistical tests involving CDF tails (chi-squared p-values, MacKinnon p approximations, Spearman with tie-handling). `statrs`'s CDFs match statsmodels' to ~1e-10 in practice but reserving 1e-8 covers MacKinnon-table interpolation.
- **1e-6 (loose):** Kruskal-Wallis with rank-tie handling. SciPy's `scipy.stats.kruskal` uses a specific tie-correction; matching it from a hand-rolled Rust implementation may need 1e-6 headroom.

---

## Validation Architecture

> This section is consumed by `/gsd:plan-phase` step 5.5 to generate VALIDATION.md.

### Test Framework

| Property | Value |
|----------|-------|
| Framework | `cargo test` + `cargo nextest` (workspace-default); `insta` for envelope snapshots; `proptest` for invariants |
| Config file | `Cargo.toml` workspace `[dev-dependencies]` (already in place from Phases 1-3) |
| Quick run command | `cargo test -p miner-core --tests scan_` (runs only Phase 4 scan tests) |
| Full suite command | `cargo nextest run --workspace` |

### Phase Requirements → Test Map

| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|--------------|
| ANOM-01 | Returns primitive emits log/simple/intraday/overnight variants | unit + integration | `cargo test -p miner-core --test scan_returns_profile` | ❌ Wave 0 |
| ANOM-02 | Welford summary stats match scipy.describe within 1e-10 | unit kernel + golden | `cargo test -p miner-core --test scan_summary_welford` | ❌ Wave 0 |
| ANOM-03 | Rolling vol per-window values match pandas.rolling().std() | unit kernel + golden | `cargo test -p miner-core --test scan_vol_rolling` | ❌ Wave 0 |
| ANOM-04 (sq) | Ljung-Box on squared returns | unit kernel + golden | `cargo test -p miner-core --test scan_ljung_box_squared` | ❌ Wave 0 |
| ANOM-04 (ref) | Phase 3 LjungBox golden re-runs unchanged after ANOM-01 refactor (D4-06) | regression | `cargo test -p miner-core --test scan_ljung_box` | ✅ exists (Phase 3) |
| ANOM-05 | ADF stat + p-value within 1e-8 of statsmodels.adfuller | golden | `cargo test -p miner-core --test scan_adf` | ❌ Wave 0 |
| ANOM-06 | KPSS stat + p-value within 1e-8 of statsmodels.kpss | golden | `cargo test -p miner-core --test scan_kpss` | ❌ Wave 0 |
| ANOM-07 | Variance ratio VR(k) within 1e-8 of arch.VarianceRatio | golden | `cargo test -p miner-core --test scan_variance_ratio` | ❌ Wave 0 |
| ANOM-08 | ARCH-LM stat within 1e-10 of statsmodels.het_arch | golden | `cargo test -p miner-core --test scan_arch_lm` | ❌ Wave 0 |
| ANOM-09 | Jarque-Bera within 1e-10 of scipy.jarque_bera | golden | `cargo test -p miner-core --test scan_jarque_bera` | ❌ Wave 0 |
| ANOM-10 | Outlier indices/values match scipy.zscore + modified-z | unit kernel | `cargo test -p miner-core --test scan_outliers` | ❌ Wave 0 |
| ANOM-11 | Drawdown profile matches hand-derived golden | unit kernel | `cargo test -p miner-core --test scan_drawdown` | ❌ Wave 0 |
| CROSS-01 | inner_join produces correct alignment on known input | unit kernel | `cargo test -p miner-core scan::primitives::time_alignment` | ❌ Wave 0 |
| CROSS-02 | Rolling Pearson + Spearman per-window match numpy/scipy | golden | `cargo test -p miner-core --test scan_corr_rolling` | ❌ Wave 0 |
| CROSS-03 | Rolling OLS β/α/R² match statsmodels.RollingOLS | golden | `cargo test -p miner-core --test scan_ols_rolling` | ❌ Wave 0 |
| CROSS-04 | Lead-lag CCF argmax-lag matches scipy.signal.correlate | golden | `cargo test -p miner-core --test scan_lead_lag` | ❌ Wave 0 |
| CROSS-05 | Engle-Granger β + ADF + OU half-life match statsmodels.coint + hand-derived | golden | `cargo test -p miner-core --test scan_engle_granger` | ❌ Wave 0 |
| SEAS-01..04 | Bucket means/stds/counts match pandas.groupby | golden | `cargo test -p miner-core --test scan_seas_buckets` | ❌ Wave 0 |
| SEAS-05 | ANOVA F-stat + Kruskal-Wallis stat match scipy | golden | `cargo test -p miner-core --test scan_anova_kruskal` | ❌ Wave 0 |
| SEAS-06 | Event-window stats hand-derived | unit | `cargo test -p miner-core --test scan_event_window` | ❌ Wave 0 |
| D4-01 | Two-leg facade end-to-end (CROSS scan emits correct envelope shape) | integration | `cargo test -p miner-core --test two_leg_facade` | ❌ Wave 0 |
| D4-02 | Preflight rejects wrong-arity request via WrongInstrumentArity | unit | `cargo test -p miner-core engine::preflight::tests::wrong_arity` | ❌ Wave 0 |
| D4-03 | DataSlice.sources Vec parallel to instruments | unit | `cargo test -p miner-core findings::tests::data_slice_sources_vec` | ❌ Wave 0 |
| D4-04 | Gap-policy intersection on two-leg scans | integration | `cargo test -p miner-core --test gap_intersect_cross` | ❌ Wave 0 |
| LOOK-AHEAD | Shuffled-future regression for every rolling/causal scan | regression | `cargo test -p miner-core --test shuffled_future_regression` | ✅ exists (Phase 3) — EXTEND |
| SCHEMA | Schema regen idempotent + non-bumping `schema_version` | regression | `cargo run -p xtask -- gen-schema && git diff --exit-code schemas/` | ✅ exists (Phase 3) |

### Sampling Rate

- **Per task commit:** `cargo test -p miner-core --test scan_<name>` (the scan's integration test).
- **Per wave merge:** `cargo nextest run -p miner-core` (every scan + every facade test).
- **Phase gate:** `cargo nextest run --workspace && cargo run -p xtask -- gen-schema && git diff --exit-code schemas/` before `/gsd:verify-work`.

### Wave 0 Gaps

Wave 0 (before plan-1 implementation tasks) must create these scaffolds:

- [ ] `crates/miner-core/tests/goldens/REFERENCE-VERSIONS.md` — pinned statsmodels/scipy/arch/numpy/python versions per §1.2
- [ ] `crates/miner-core/tests/goldens/python-requirements.lock` — frozen pip lock file for golden regeneration reproducibility
- [ ] `crates/miner-core/tests/two_leg_facade.rs` — integration test scaffold for D4-01..D4-04
- [ ] `crates/miner-core/tests/arity_preflight.rs` — integration test scaffold for D4-02
- [ ] `crates/miner-core/tests/gap_intersect_cross.rs` — integration test scaffold for D4-04
- [ ] Extend `crates/miner-core/tests/shuffled_future_regression.rs` — add a per-scan loop driver for every rolling/causal scan
- [ ] Per-scan integration test stubs (22 files) under `crates/miner-core/tests/scan_<name>.rs`

### Four Sampling Dimensions

- **Coverage:** Each of the 22 scans (23 with sq-LjungBox variant) ships a happy-path integration test against deterministic synthetic data + a checked-in statsmodels/scipy/arch golden where applicable. Goldens land in `crates/miner-core/tests/goldens/<scan_id>/`; reference versions pinned in `REFERENCE-VERSIONS.md` (§1.2).
- **Edge:** N=0 input (`Finding::ScanError` with `ScanErrorCode::ComputeError`), N=1 input (most scans error, summary-stats and returns scans emit trivial findings), all-zero / constant-input returns (variance-zero handling), all-NaN input (must error cleanly, NOT propagate NaN into envelope), single-bar window for rolling stats (must error or skip), `instruments.len() != arity` (preflight reject with `WrongInstrumentArity`).
- **Adversarial:** Shuffled-future regression (from Phase 3 D3-09, extended to every rolling/causal scan), zero-variance leg in a CROSS-02 invocation (correlation undefined — must `Finding::ScanError`, not NaN), cointegrating residual with near-zero half-life (CROSS-05 OU fit numerical degeneracy), Spearman with all-tied input (rank-tie handling stress), KPSS on a pure trend without constant (regression='c'-vs-'ct' boundary), wrong-arity request via CLI (preflight reject), mixed-side legs (D4-07 — bid×ask EURUSD ought to succeed and produce sensible output).
- **Cross-validation:** Byte-identical re-run (D3-23, every scan), CLI/MCP/HTTP parity test (Phase 6 will enforce — Phase 4 emits findings whose only run-id / clock-read fields differ across surfaces), schema regen + diff check (per D4-01/D4-03 — gated in CI).

---

# Section 4: Dependency-Add Audit

**Read of `crates/miner-core/Cargo.toml` (verified at research time):**

Already present (confirmed lines 26-48 of `Cargo.toml`):

- `serde` (workspace)
- `serde_json` (workspace, no `preserve_order` — BTreeMap-backed)
- `schemars` (workspace, `chrono04` feature)
- `chrono` (workspace, `clock + serde`)
- `thiserror` (workspace)
- `tracing` (workspace)
- `ulid` (workspace, `serde`)
- `blake3` (workspace)
- `base64` (workspace)
- `figment` (workspace, `toml + env`)
- `arrow` (workspace, 58)
- `tempfile` (workspace, 3)
- `statrs` (workspace, 0.17) ✅ **already in for Phase 3 LjungBox**

Phase 4 ADDs (workspace + miner-core deps):

```toml
# crates/miner-core/Cargo.toml [dependencies] additions:
ndarray            = "0.16"
ndarray-stats      = "0.6"
nalgebra           = "0.33"
```

Mirror in workspace `Cargo.toml [workspace.dependencies]`:

```toml
# tradedesk-miner/Cargo.toml [workspace.dependencies] additions:
ndarray            = "0.16"
ndarray-stats      = "0.6"
nalgebra           = { version = "0.33", default-features = false, features = ["std"] }
```

`nalgebra` `default-features = false` strips the optional `rand` and `serde-serialize` defaults; Phase 4 only needs the std-backed dense small-fixed matrix support.

**License audit (all Apache-2.0 / MIT-or-Apache-2.0):**

- `ndarray` — MIT OR Apache-2.0 ✅
- `ndarray-stats` — MIT OR Apache-2.0 ✅
- `nalgebra` — Apache-2.0 ✅

All compatible with the project's Apache-2.0 license. `cargo deny check licenses` (if/when wired in CI) accepts all three.

**Sync-only audit:** None of the three depends on tokio / async-std / async-trait. `cargo tree -p miner-core | grep -E 'tokio|async-std|smol'` will continue returning nothing post-add. [VERIFIED: docs.rs for each crate.]

**No removals.** Phase 4 doesn't deprecate any Phase 1-3 dep.

---

# Section 5: Scan Emission Pattern Catalogue

| Pattern | Definition | Scans | Why |
|---------|-----------|-------|-----|
| **Single-shot** | ONE `Finding::Result` per invocation; scalar `effect.value` | ANOM-01, ANOM-02, ANOM-04, ANOM-04-sq, ANOM-05, ANOM-06, ANOM-07, ANOM-08, ANOM-09, ANOM-10, ANOM-11, CROSS-05, SEAS-05 | Headline test statistic or summary stat. Vector results (if any) go into `effect.extra`. |
| **Per-window vector in `effect.extra`** | ONE `Finding::Result` per invocation; `effect.value` = last-window scalar; per-window vector in `effect.extra.values` + `effect.extra.window_starts_ms` | ANOM-03, CROSS-02 (Pearson + Spearman), CROSS-03, CROSS-04 (CCF vector over lag grid) | Bounded stdout throughput; preserves Phase 3 single-shot facade; consumer reconstructs the rolling curve from `effect.extra`. |
| **Per-bucket vectors in `effect.extra`** | ONE `Finding::Result` per invocation; `effect.value` = max-abs t-stat across buckets; per-bucket vectors in `effect.extra.{buckets, means, stds, counts, t_stats}` | SEAS-01, SEAS-02, SEAS-03, SEAS-04, SEAS-06 (per-event arrays) | Bucket count is bounded (24 hours, 7 weekdays, 4 sessions, 6 trading-day-of-month buckets); the bucket vector is self-contained. |
| **Multi-shot under `continuous_only` gap policy** | ONE `Finding::Result` per intersection sub-range when gap policy partitions | All Phase 4 scans (inherited from Phase 3 D3-12) | Gap policy is orthogonal to scan emission. Strict + gaps → ONE GapAborted; continuous_only + sub-ranges → N Results, one per sub-range. |

**Anti-pattern explicitly rejected:** One-Finding-per-window for rolling scans (would produce 100k+ findings on a 2-year 15m run). The CONTEXT.md §deferred default ("emit ONE finding with vector arrays in effect.extra") is correct and this research confirms it.

---

# Section 6: Plan / Wave Breakdown Recommendation

**Recommended decomposition: 5 plans, layered, with the facade-shape extension first.**

```
                                 ┌─────────────────────────────────────────────┐
                                 │   Plan 04-1: Facade-shape extension          │
                                 │   (D4-01..D4-04 + ANOM-01 primitive          │
                                 │    + CROSS-01 primitive + arity preflight    │
                                 │    + schemars regen verification)             │
                                 │   ≈ 6-8 tasks                                 │
                                 └────────────────┬────────────────────────────┘
                                                  │  (depends on Plan 04-1)
                          ┌───────────────────────┼───────────────────────┐
                          ▼                       ▼                       ▼
   ┌─────────────────────────────┐  ┌────────────────────────────┐  ┌───────────────────────────┐
   │   Plan 04-2: ANOM scans      │  │  Plan 04-3: CROSS scans     │  │  Plan 04-4: SEAS scans     │
   │   (ANOM-01 callable +        │  │  (CROSS-02..05 — 4 scans)   │  │  (SEAS-01..06 — 6 scans)   │
   │    ANOM-02..11 — 11 scans +  │  │  ≈ 7-9 tasks                 │  │  ≈ 8-10 tasks              │
   │    D4-06 LjungBox refactor)  │  │                              │  │                            │
   │   ≈ 13-15 tasks               │  │                              │  │                            │
   └────────────────┬─────────────┘  └────────────┬───────────────┘  └────────────┬──────────────┘
                    │                              │                              │
                    └──────────────────────────────┼──────────────────────────────┘
                                                   │
                                                   ▼
                                 ┌─────────────────────────────────────────────┐
                                 │   Plan 04-5: Integration tests + goldens     │
                                 │   + schema regen + README quickstart         │
                                 │   ≈ 6-8 tasks                                 │
                                 └─────────────────────────────────────────────┘
```

**Plan 04-1: Facade-shape extension** (6-8 tasks, no scan registrations yet):

1. Schemars regen spike — apply D4-01 + D4-03 types on scratch branch, regen schemas, inspect diff, decide D4-03 vs D4-03-ALT.
2. Add `InstrumentSpec` struct + extend `ScanRequest.instruments: Vec<InstrumentSpec>` (D4-01).
3. Add `ScanArity` enum + `Scan::arity()` trait method (D4-02).
4. Extend `PreflightCode::WrongInstrumentArity` + preflight validation logic.
5. Generalise `DataSlice.sources: Vec<Source>` (D4-03) OR (`peer_sources` fallback per D4-03-ALT).
6. Extend `ScanCtx` with `bars_pair` (for two-leg) + `bars_up_to(ts)` look-ahead-safety helper (§1.5).
7. Add `crates/miner-core/src/scan/primitives/{returns.rs, time_alignment.rs}` (ANOM-01 + CROSS-01 kernels).
8. Extend CLI repeatable `--instrument SYMBOL:side` parser + arity-aware `ScanArgs::to_scan_request`.

**Plan 04-2: ANOM scans** (13-15 tasks):

- Task per scan (ANOM-01 callable, ANOM-02 through ANOM-11) — 11 tasks.
- 1 refactor task: D4-06 LjungBox `ANOM-01` primitive call (move log_returns body verbatim).
- 1 integration test task: shuffled-future regression extension to ANOM-03 (the rolling scan in this family).

**Plan 04-3: CROSS scans** (7-9 tasks):

- Task per scan (CROSS-02 Pearson + Spearman, CROSS-03 OLS, CROSS-04 CCF, CROSS-05 Engle-Granger) — 4-5 tasks (CROSS-02 may be one task with two `Scan` registrations).
- 1 integration test task: gap-policy intersection (D4-04) with synthetic two-leg gap scenarios.
- 1 integration test task: shuffled-future regression extension to CROSS-02 / CROSS-03 / CROSS-04.

**Plan 04-4: SEAS scans** (8-10 tasks):

- Task per scan (SEAS-01..06) — 6 tasks.
- 1 task: session-boundary defaults table baked into SEAS-03 (per §1.8).
- 1 task: trading-day-of-month bucket kernel for SEAS-04.

**Plan 04-5: Integration + goldens + schema regen + README** (6-8 tasks):

- 1 task: REFERENCE-VERSIONS.md + python-requirements.lock authored.
- 1 task: golden generation Python script + commit goldens for at least one ANOM + one CROSS + one SEAS scan (success criterion #5).
- 1 task: schema regen via `cargo xtask gen-schema` + commit final schemas.
- 1 task: extend `miner scans` to emit `arity` field; verify catalogue regen.
- 1 task: extend `shuffled_future_regression.rs` with per-scan loop driver.
- 1 task: README quickstart — ANOM / CROSS / SEAS invocation examples + sample output.
- 1 task: byte-identical-rerun integration test for representative scan from each family.
- 1 task: final phase verification — full `cargo nextest run --workspace` clean.

**Total: ~40-50 tasks across 5 plans.** Per-plan task counts stay under the planner's checker threshold (typically 15 tasks/plan). Each plan finishes before the next begins; commits between plans.

**Alternative shape considered + rejected:** "One plan per scan family AND per phase 4 deliverable" — would balloon to 10+ plans. The 5-plan layered shape above is the right granularity for plan-checker review.

---

# Section 7: Schemars Regen Recipe

The project already has the schema-sync pipeline wired (Phase 3 / Plan 06). Phase 4 uses it verbatim:

```bash
# Regenerate both schema artifacts (idempotent — running twice produces no diff).
cargo run -p xtask -- gen-schema

# OR with an explicit out_dir for spike branches:
cargo run -p xtask -- gen-schema /tmp/schemas-scratch

# Inspect what changed against the committed artifact:
git diff schemas/findings-v1.schema.json
git diff schemas/scans-catalogue-v1.schema.json

# Determinism check (paranoid): run twice in succession and assert no diff.
cargo run -p xtask -- gen-schema
git stash
cargo run -p xtask -- gen-schema
git diff --exit-code schemas/        # MUST exit 0
git stash pop
```

**What "additive" means in this project's gate:**

The CI gate (Phase 7 / Plan 06's spec) is `git diff --exit-code schemas/`. ANY diff is failure. There is NO automated "is-this-additive" classifier. The project's contract is:

- **Truly additive** (no schema_version bump, no diff): adding an optional field with `#[serde(default)]` whose absence in old data is valid. The new field appears in `properties` map and `required` list stays unchanged.
- **Non-additive but accepted** (no schema_version bump, diff WILL be non-empty): generalising a singleton field to a `Vec<T>` is a TYPE change. Old data will FAIL to parse without a migration. Phase 4's D4-01 + D4-03 are this case.
- **Schema_version bump required**: REMOVING a field, RENAMING a field's wire name, CHANGING enum-variant string forms. Phase 4 has NONE of these.

**For Phase 4:** The diff WILL be non-empty (D4-01 + D4-03 generalise singleton to Vec). The plan-task wording: "commit the regenerated schema as a deliberate facade-shape update; keep `schema_version = 1` because consumers haven't shipped yet (Phase 6 wrappers don't exist)." This is documented in plan-1 task 1's `<verify>` block.

[VERIFIED: xtask/src/main.rs:58-134 + the existing schemas/findings-v1.schema.json shows the pipeline is real and working.]

---

# Section 8: Phase 3 LjungBoxScan as the Gold-Standard Reference Pattern

Phase 4's 22 scans replicate the Phase 3 LjungBoxScan pattern **verbatim**. The reference files:

- `crates/miner-core/src/scan/ljung_box/mod.rs` (lines 1-241 — Scan impl + envelope construction + helpers + tests)
- `crates/miner-core/src/scan/ljung_box/kernel.rs` (lines 1-318 — pure kernels + unit tests)

**Replicate this structure for every Phase 4 scan:**

### `mod.rs` skeleton (verbatim from LjungBox)

```rust
//! `<ScanName>Scan` — Phase 4 scan implementing the [`Scan`] trait.
//!
//! Pattern analog: Phase 3 `LjungBoxScan` (mod.rs + kernel.rs split).
//!
//! ## D4-XX contract
//! - id = "<family>.<subfamily>.<scan_name>", version = 1
//! - effect.metric = "<canonical>", effect.value = <scalar choice from §2>
//! - effect.extra.<vectors per §2>, raw.series.<input keys>

use std::collections::BTreeMap;
use std::sync::atomic::Ordering;
use chrono::Utc;
use crate::findings::{ Base64Bytes, DataSlice, Dtype, Effect, Finding, FindingSink,
                      Raw, RawArray, ResultFinding, Source };
use crate::scan::{ Scan, ScanArity, ScanCtx, ScanError, ScanFindingShape, ScanRequest };

pub mod kernel;

pub struct <ScanName>Scan;
const SCAN_ID: &str = "<family>.<subfamily>.<scan_name>";
const SCAN_VERSION: u32 = 1;
const EFFECT_METRIC: &str = "<canonical>";

impl Scan for <ScanName>Scan {
    fn id(&self) -> &'static str { SCAN_ID }
    fn version(&self) -> u32 { SCAN_VERSION }
    fn arity(&self) -> ScanArity { ScanArity::<Single|Pair> }    // NEW Phase 4 trait method

    fn param_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "properties": { /* hand-rolled per scan */ },
            "additionalProperties": false
        })
    }

    fn finding_fields(&self) -> ScanFindingShape {
        ScanFindingShape {
            effect_extra_keys: &[<per §2>],
            raw_series_keys:   &[<per §2 — always includes "timestamps_ms">],
        }
    }

    fn run(&self, ctx: &ScanCtx<'_>, req: &ScanRequest,
           sink: &mut dyn FindingSink) -> Result<(), ScanError> {
        if ctx.cancel.load(Ordering::Relaxed) { return Ok(()); }
        // ... kernel call, envelope construction, sink.write_envelope ...
        Ok(())
    }
}

// Helpers (mirror lines 253-312 of LjungBox mod.rs):
fn resolve_params(req: &ScanRequest, n: usize) -> Result<TypedParams, ScanError> { ... }
fn f64_slice_to_raw_array(s: &[f64]) -> RawArray { /* identical to LjungBox lines 302-312 */ }

#[cfg(test)]
mod tests { /* per-scan tests follow LjungBox tests structure */ }
```

### `kernel.rs` skeleton (verbatim from LjungBox)

```rust
//! Pure <name> kernel.
//!
//! Pattern analog: Phase 3 `ljung_box::kernel` — private `#[inline]` pure
//! functions on primitive types with a sibling `#[cfg(test)] mod tests` block.
//! No IO, no `serde_json`, no `Reader` calls.

#![cfg_attr(any(test, debug_assertions), allow(clippy::float_cmp))]

use statrs::distribution::{ChiSquared, ContinuousCDF};   // or Normal / T / F per scan

#[inline]
pub(super) fn <kernel_fn>(input: &[f64], param: <usize|f64>) -> <KernelOutput> {
    // pure function body
}

#[cfg(test)]
mod tests {
    use super::*;
    const TOL: f64 = 1e-12;
    fn approx_eq(a: f64, b: f64, tol: f64) -> bool { (a - b).abs() <= tol }

    #[test] fn kernel_basic() { /* hand-derived reference output */ }
    #[test] fn kernel_empty() { /* edge case */ }
    #[test] fn kernel_singleton() { /* edge case */ }
    #[test] fn kernel_constant_input() { /* zero-variance handling */ }
    #[test] fn kernel_known_input() { /* hand-computed reference, 1e-12 */ }
    #[test] #[should_panic(...)] fn kernel_invalid_input() { /* debug_assert pin */ }
}
```

**Discipline rules (all directly carried from Phase 3):**

1. **`#[inline]` on every kernel function** — small bodies, hot loops.
2. **`pub(super)` not `pub`** — kernels stay internal to the scan module.
3. **`statrs` is the ONLY distribution path** — no hand-rolled CDF approximations (Phase 3 invariant).
4. **`debug_assert!` for kernel invariants** — fires under `cargo test`; absent in release. Pattern: `debug_assert!(max_lag >= 1, "kernel_name: max_lag must be >= 1");`.
5. **Test tolerance `1e-12` for pure-arithmetic kernels; `1e-8` for chi2/normal-CDF tests.**
6. **Always include a hand-derived "known input" test** — precompute against the reference function in Python; embed the expected f64 array as a literal in the Rust test. The kernel-level golden, not the envelope-level golden.
7. **`f64_slice_to_raw_array` helper** — every scan has a private copy of this 11-line helper (LjungBox lines 302-312). Could be lifted to a shared `scan::shape::f64_slice_to_raw_array` but Plan picks; consistent within a family.
8. **`#[allow(clippy::cast_precision_loss, reason = "...")]` annotations** with the SAME reason text as Phase 3 (lines 50-52, 95-99, 178-181, 300) — the precision-loss explanations stay consistent across the codebase.

---

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | `scipy==1.14.1` is the right pin for Phase 4 goldens | §1.2 | Must confirm against PyPI release history during golden generation; mismatch produces version-error logs but is detectable immediately |
| A2 | `arch==7.0.0` provides `VarianceRatio` with the API Lo-MacKinlay golden needs | §1.2 + §2 (ANOM-07) | If the API drifts in arch 7.x, plan-phase falls back to hand-derivation against the original 1988 Lo-MacKinlay paper |
| A3 | Session-boundary UTC hours (Asia 22-07, London 07-16, NY 12-21, overlap 12-16) match `tradedesk-dukascopy` calendar conventions | §1.8 | Wrong boundaries produce wrong seasonality findings; verify against the dukascopy reader's existing calendar source before locking SEAS-03 defaults |
| A4 | Schemars 1.x's `Vec<T>` derive produces `{"type":"array","items":{...}}` — a structural diff from the singleton `{"$ref":...}` | §1.1 + §7 | Recommended path (commit the diff as deliberate facade-shape change) is the right call; if CI gate is genuinely strict on field-name removal, plan-phase falls back to D4-03-ALT |
| A5 | `nalgebra` `default-features = false` with `features = ["std"]` provides everything CROSS-03 small-fixed OLS needs without pulling `rand` | §4 | Verify by `cargo build -p miner-core` after the add; if a dense matrix factorisation requires another feature, add minimally |
| A6 | ANOM-04 squared-returns variant counts as a SEPARATE registered scan (`stats.autocorr.ljung_box_sq@1`) rather than a `--params series_kind=squared` extension of the existing scan | §2 (ANOM-04 sq row) | Either shape works; recommended path is separate registration so the `miner scans` catalogue lists both clearly. Plan can flip to one-scan-two-variants in plan 04-2 if ergonomics favour. |
| A7 | The Phase 3 LjungBox golden test re-runs unchanged after D4-06 refactor (ANOM-01 primitive call replaces inline log_returns) | §1 deferred-ideas + Pitfall 9 | Will hold IF the new primitive is a byte-identical move; if the implementer rewrites instead of moves, golden breaks. Plan-task wording explicitly says "move, do not rewrite". |
| A8 | Trading-day-of-month bucket cutoff N=3 is a defensible default for SEAS-04 | §2 (SEAS-04) | Plan can pick N=5 with no architectural impact; the value is `--params cutoff_n=N` user-overridable |
| A9 | Each Phase 4 plan stays under ~15 tasks (planner checker threshold) | §6 | If a plan balloons (e.g., ANOM has 13 scans + extras pushes plan 04-2 to 18 tasks), split into 04-2a (ANOM-01..06) + 04-2b (ANOM-07..11) |

**Risk policy:** Every `[ASSUMED]` claim is flagged inline at point of use. The planner should treat each as a checkpoint — confirm or override during plan-task authoring.

---

## Open Questions (RESOLVED)

After resolving the 10 CONTEXT.md open questions in §1, the residual open items are:

1. RESOLVED: **Exact scipy/arch versions at golden-generation time.** The plan-phase author should run `pip index versions scipy` and `pip index versions arch` against PyPI on the day goldens are generated and update REFERENCE-VERSIONS.md accordingly. Recommendation: lock-file is the source of truth, REFERENCE-VERSIONS.md is the human-readable summary.

   - What we know: training-data pin recommendations (scipy 1.14.1, arch 7.0.0).
   - What's unclear: exact release-date and any post-training patch releases.
   - Recommendation: regenerate during plan 04-5 task 1 and commit the lock file.

2. RESOLVED: **Session-boundary defaults — verify against `tradedesk-dukascopy` calendar source.** [VERIFIED: CLAUDE.md "Project" §"Constraints" — miner reads dukascopy cache without modification, and the calendar logic lives somewhere in the dukascopy reader.] Plan should grep `tradedesk-dukascopy` for any existing session-boundary constants and align SEAS-03 defaults with whatever is already pinned.

   - What we know: FX-major industry conventions match.
   - What's unclear: whether `tradedesk-dukascopy` calendar uses different ranges (unlikely but possible).
   - Recommendation: grep the dukascopy reader during plan 04-4 task 1; if different, prefer dukascopy's values for consistency.

3. RESOLVED: **Whether to extract `f64_slice_to_raw_array` to a shared `scan::shape` helper.** Phase 3 inlines this 11-line helper per-scan (LjungBox lines 302-312). With 22 scans inheriting the same helper, plan-phase may decide to lift it.

   - What we know: Phase 3 has it inline.
   - What's unclear: whether 22× inline is annoying enough to refactor.
   - Recommendation: plan 04-1 task 7 lifts it to `scan::primitives::raw_array::f64_slice_to_raw_array`; every scan imports.

---

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| `cargo` / `rustc` (edition 2024) | Build | ✓ | 1.85+ workspace pin | — |
| `python3` + `pip` | Golden generation only (`tests/goldens/*.py` scripts) | ? | (n/a — install at golden time) | Plan 04-5 task 1 documents `pyenv install 3.11.x` + `pip install -r python-requirements.lock` |
| `cargo xtask gen-schema` | Schema regen | ✓ | (binary built from xtask) | — |
| `cargo nextest` | Test runner | ? | (workspace dev-dep optional) | `cargo test` is the fallback |

**Missing dependencies with no fallback:** None for Phase 4 implementation (Python is only needed at golden-generation time and is documented as a one-off setup step).

**Missing dependencies with fallback:** `cargo nextest` (use `cargo test`).

---

## Security Domain

Phase 4 introduces no new auth, session, access-control, crypto, or input-validation surface beyond what Phases 1-3 already established:

| ASVS Category | Applies | Standard Control |
|---------------|---------|------------------|
| V2 Authentication | no | miner is a CLI binary; no auth |
| V3 Session Management | no | no sessions |
| V4 Access Control | no | no access control |
| V5 Input Validation | yes | scan params validated via `Scan::param_schema()` JSON Schema + `engine::preflight::parse_params_kv` (Phase 3 path); arity validation added in Phase 4 (D4-02) |
| V6 Cryptography | no | hashing for `param_hash` via `blake3` (Phase 1 lock) — no new crypto |

**Known threat patterns for Rust statistical scan catalogue:**

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Malicious params string injecting JSON | Tampering | `parse_params_kv` typed-fallback + JSON Schema validation at preflight |
| NaN propagation into envelope from kernel | Information disclosure (corrupted output) | Kernel returns `Result<_, ScanError::Kernel(_)>`; zero-variance / constant-input branches error rather than emit NaN |
| Excessive memory via huge `--window` on a CROSS scan | Denial-of-Service | Phase 5's sweep-cardinality cap (SweepTooLarge preflight code) is the canonical defence; Phase 4 inherits the single-shot model and doesn't introduce new DoS surfaces |
| `--params` integer overflow (e.g., `lags=999999999`) | DoS via OOM | Kernel-level validation: `lags < n` reject; window-size validation against `usize::MAX/8` (RawArray byte budget) |

No new threats specific to Phase 4 beyond the ones Phases 1-3 already mitigate.

---

## Sources

### Primary (HIGH confidence)
- `crates/miner-core/src/scan/ljung_box/mod.rs` and `kernel.rs` — Phase 3 gold-standard pattern (lines 1-241 / 1-318).
- `crates/miner-core/src/scan/mod.rs` — `Scan` trait, `ScanCtx`, `ScanRequest` (Phase 3 verified).
- `crates/miner-core/src/scan/registry.rs` — `bootstrap()` factory pattern.
- `crates/miner-core/src/findings/mod.rs` — `Finding` envelope, `DataSlice`, `Source`, `RawArray`.
- `crates/miner-core/src/error/codes.rs` — `PreflightCode` enum (Phase 4 adds one variant).
- `crates/miner-core/src/cache.rs` lines 525-570 — `BarCache::get_or_build` signature (consumed by CROSS scans).
- `crates/miner-core/src/gap.rs` lines 75-146 — `GapManifest`, `GapSpan`, `GapReason` (Phase 4 intersects manifests).
- `xtask/src/main.rs` — schema regen pipeline (`cargo run -p xtask -- gen-schema`).
- `schemas/findings-v1.schema.json` — current schema artifact (Phase 4 regen target).
- `Cargo.toml` workspace deps (lines 35-73).
- `crates/miner-core/Cargo.toml` (verified active deps).
- `.planning/phases/04-scan-catalogue-anom-cross-seas/04-CONTEXT.md` — user-locked decisions D4-01..D4-09.
- `.planning/REQUIREMENTS.md` — ANOM-01..11, CROSS-01..05, SEAS-01..06.
- `.planning/STATE.md` — locked-decision list; Phase 4 implementation risk flagged.
- `./CLAUDE.md` — workspace technology-stack pins (ndarray, nalgebra, statrs, rayon; vetoes on polars/tokio/HashMap-on-Serialize).

### Secondary (MEDIUM confidence — pinned but not bundled)
- statsmodels 0.14.6 — `statsmodels.org/v0.14.6/api.html` reference for ANOM-04..09, CROSS-03, CROSS-05.
- scipy 1.14.x — `docs.scipy.org/doc/scipy-1.14.1/reference/stats.html` reference for ANOM-09, ANOM-10, SEAS-05.
- arch 7.0 — `arch.readthedocs.io` reference for ANOM-07 `VarianceRatio`.
- pandas 2.x — `pandas.pydata.org` reference for SEAS-01..04 `groupby` goldens.
- numpy 1.26.4 — `numpy.org/doc/1.26/reference/generated/numpy.corrcoef.html` for CROSS-02 reference.

### Tertiary (LOW confidence — verify on use)
- arch 7.0.0 exact PyPI version (training-data assumption — confirm at golden-generation).
- Session-boundary UTC hours (FX-major convention — verify against `tradedesk-dukascopy` calendar).

---

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — three crate additions (ndarray/ndarray-stats/nalgebra) verified against CLAUDE.md TL;DR and the existing miner-core Cargo.toml; statrs/serde/schemars already wired.
- Architecture patterns: HIGH — verbatim carry-over of Phase 3 LjungBox `mod.rs`/`kernel.rs` split, verified end-to-end through plan 03-07.
- Per-scan reference picks: MEDIUM — statsmodels/scipy convention is well-documented but a handful of scans (ANOM-07 Lo-MacKinlay, ANOM-11 drawdown, CROSS-05 OU half-life, SEAS-06 event-window) require hand-derivation against original papers, not just function-call reference.
- Schemars regen verdict: MEDIUM — derive-shape behaviour of `Vec<T>` is HIGH-confidence; the project's exact CI policy on "diff present but schema_version unchanged" is MEDIUM — should be confirmed in plan 04-1 task 1 spike.
- Pitfalls: HIGH — all nine pitfalls map to known Phase 3 patterns (cancel polling, BTreeMap, look-ahead-safety, sink.write_envelope vs flush, deterministic summation order).

**Research date:** 2026-05-19
**Valid until:** 2026-06-19 (30 days for stable Rust ecosystem; Python reference versions may drift sooner — re-verify scipy/arch at golden-generation time)

## RESEARCH COMPLETE
