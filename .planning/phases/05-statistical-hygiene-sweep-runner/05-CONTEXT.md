# Phase 5: Statistical Hygiene & Sweep Runner — Context

**Gathered:** 2026-05-20
**Status:** Ready for planning

<domain>
## Phase Boundary

Phase 5 adds the statistical-hygiene layer that the Quant agent needs to read findings without re-running them, plus the sweep-manifest runner that turns the per-invocation facade (Phase 3) into a batched fan-out (rayon-parallel) over user-supplied scan × instrument(s) × timeframe × window × param-grid jobs.

What Phase 5 delivers:

1. **TOML sweep manifest + `miner sweep <manifest.toml>` subcommand** (OP-04, success criterion #1). One file describes a multi-scan job-set; miner fans it out via rayon and streams one `Finding::Result` per `(scan × instrument(s) × timeframe × window × param-point)`. Single-shot `miner scan` (Phase 3 / D3-18) is unchanged.
2. **Sweep dry-run with `--dry-run`** (success criterion #5). Reuses or extends `Finding::DryRun` (D3-21) to emit one record per planned job + an aggregate `estimated_job_count`. Plan-phase decides whether dry-run emits per-job records or a single sweep-level record.
3. **Effect size per finding (HYG-01, success criterion #2).** Every scan that emits a p-value also emits an effect-size scalar paired with a `kind` discriminant (`cohens_d` / `hedges_g` / `cliffs_delta` / `vr_minus_one` / scan-specific). The slot is a new structured field on `Effect` (D5-03 below).
4. **End-of-sweep BH-FDR adjustment (HYG-02, success criterion #2).** Sweep emits one `Finding::SweepSummary` envelope variant at end-of-run carrying per-family q-values; per-finding `ResultFinding.fdr_q` stays `null` during streaming (D5-02 below) so the JSONL stream is single-pass.
5. **Block / stationary (Politis-Romano) bootstrap CIs (HYG-03).** Caller-opt-in via `--bootstrap stationary --bootstrap-n N` (and a sweep-manifest `[hygiene]` block); populates the existing reserved-null `Effect.ci95: Option<[f64;2]>` slot. Per-scan declared support via a new `Scan::supports_bootstrap()` trait method (D5-04).
6. **Phase-scrambled / circular-shift null distributions (HYG-04).** Caller-opt-in via `--null phase_scramble|circular_shift --null-n N`; populates the existing `Effect.p_value` slot (replacing the analytic p-value when caller opts in). Per-scan declared support via `Scan::supports_null_method(NullMethod)` (D5-04).
7. **Bit-for-bit reproducible bootstrap + null (HYG-05, success criterion #4).** New `ResultFinding.repro: Option<ReproEnvelope>` field captures the master seed, derived per-job seed, resample counts, and method names so a re-run with the same envelope produces byte-identical bootstrap / null outputs (D5-05).
8. **Two new `PreflightCode` variants** — `HygieneNotSupported` (scan rejected `--bootstrap` / `--null`) and `SweepTooLarge` (estimated job count exceeds a configurable cap). Additive to the existing `PreflightCode` enum (Phase 1 contract).
9. **Sweep cancellation (SIGINT) preserves every already-streamed finding** (OP-06 reaffirmed). Rayon worker pool inherits Phase 3's `Arc<AtomicBool>` cancel flag pattern (D3-22). End-of-sweep `Finding::SweepSummary` is NOT emitted on SIGINT-mid-sweep — exit code 130 takes precedence.

What Phase 5 does NOT deliver (deferred):

- **Deflated Sharpe Ratio (DSR)** — REQUIREMENTS.md HYG-v2-01. `ResultFinding.dsr` stays reserved-null in v1; populating it lives in v2.
- **"Top-N interesting findings" summary** — HYG-v2-02. Outside Phase 5 scope.
- **Memoised per-sweep intermediates / in-memory arena** — HYG-v2-03. Phase 5 fans out per-job without inter-job caching.
- **Side-channel raw-array storage (URI / file reference)** — HYG-v2-04. Phase 5 inlines raw arrays per Phase 1 contract.
- **PyO3 bindings** — PLAT-v2-01. Phase 5 stays binary-only (CLI now; MCP/HTTP in Phase 6).
- **MCP + HTTP parity for sweep / bootstrap / null** — Phase 6. Phase 5 commits the contract; Phase 6 mirrors it across wrappers.
- **Bench harness / flamegraph / golden regression suite for the sweep path** — Phase 7.

The user is not a Rust practitioner; the four user-locked-by-default decisions below are Claude's-discretion pragmatic defaults explicitly delegated during discuss-phase. Plan-phase research must confirm them against statsmodels + Politis-Romano (1994) reference behaviour before implementation begins.

</domain>

<decisions>
## Implementation Decisions

### Pragmatic defaults — user explicitly delegated, ready for plan-phase research to confirm

The user accepted pragmatic defaults on all four gray areas during discuss-phase. The defaults below are documented clearly enough that downstream agents (researcher, planner) can act on them. Plan-phase research must validate each against the cited reference behaviour and may refine.

---

### D5-01: Sweep manifest is a TOML file with `[[jobs]]` array; each job expands cartesian across axes

**Default shape:**

```toml
# Top-level applies to every job unless overridden.
[sweep]
# Optional master seed for bootstrap / null reproducibility (HYG-05).
# Defaults to blake3-derived from (manifest_hash, ulid-run-id) when omitted.
seed = 0xDEADBEEF
# Maximum estimated jobs; rejects with PreflightCode::SweepTooLarge above this.
max_jobs = 100000

# Optional hygiene defaults applied to every job unless overridden per-job.
[hygiene]
bootstrap = "stationary"        # or "block"; default off (None).
bootstrap_n = 1000              # default 0 (off).
null = "phase_scramble"         # or "circular_shift"; default off (None).
null_n = 1000                   # default 0 (off).

[[jobs]]
scan = "stats.autocorr.ljung_box@1"
instruments = ["EURUSD:bid", "GBPUSD:bid"]   # Single-arity: flat string array;
                                             # each element becomes one job.
timeframes = ["15m", "1h"]
windows = ["2024-01-01:2024-06-30"]
gap_policy = "continuous_only"               # optional override of CLI default.
[jobs.params]
lags = [5, 10, 20]                            # arrays expand cartesian.

[[jobs]]
scan = "cross.lead_lag.ccf@1"
instruments = [["EURUSD:bid", "GBPUSD:bid"], ["EURUSD:bid", "USDJPY:bid"]]
                                             # Pair-arity: nested array of 2-tuples.
                                             # Each inner array becomes one job.
timeframes = ["15m"]
windows = ["2024-01-01:2024-06-30"]
[jobs.params]
max_lag = [10, 20]
[jobs.hygiene]                                # per-job override of [hygiene] block.
null = "phase_scramble"
null_n = 5000
```

**Expansion semantics:** within each `[[jobs]]` block, the cartesian product of (instruments × timeframes × windows × every `params.<key>` array) defines the job set. Multiple `[[jobs]]` blocks accumulate (each fans out independently). Per-job override of `[hygiene]` and `[sweep]` defaults is supported via nested `[jobs.hygiene]` / `[jobs.sweep]` blocks.

**Why pragmatic:** cartesian product is the natural fanout shape; per-job blocks let a single manifest mix Single-arity (ANOM/SEAS) and Pair-arity (CROSS) scans; the syntax mirrors well-known CI matrix configs and Hydra. Zip/named-axes is more flexible but harder to author and to dry-run-estimate. Defer to v2 if a use case demands it.

**Validation:** preflight rejects with `PreflightCode::InvalidParameter` if:
- A Single-arity scan receives Pair-arity `instruments` (nested array).
- A Pair-arity scan receives Single-arity `instruments` (flat array).
- Estimated job count > `[sweep].max_jobs` (default 100,000) — emits `PreflightCode::SweepTooLarge` with the estimate.
- `bootstrap_n` or `null_n` is set but the scan returns `supports_bootstrap() == false` / `supports_null_method(...) == false` — emits `PreflightCode::HygieneNotSupported`.

**Job-graph deterministic ordering:** jobs emit in (1) `[[jobs]]` block declaration order; (2) within a block, cartesian iteration order across axes (instruments → timeframes → windows → params alphabetical). Sweep dry-run echoes this exact order.

---

### D5-02: BH-FDR adjustment scopes per `scan_id@version`; q-values land in an end-of-sweep `Finding::SweepSummary` envelope, NOT in per-finding `fdr_q`

**Default:** miner runs the sweep, streams each finding with `ResultFinding.fdr_q = null` (the Phase 1 reserved slot stays unchanged during the streaming pass), then emits ONE `Finding::SweepSummary` envelope at end-of-sweep carrying per-family q-values:

```jsonl
{"kind":"run_start", ...}
{"kind":"result", "scan_id@version":"stats.autocorr.ljung_box@1", ..., "fdr_q":null, ...}
{"kind":"result", "scan_id@version":"stats.autocorr.ljung_box@1", ..., "fdr_q":null, ...}
{"kind":"result", "scan_id@version":"cross.lead_lag.ccf@1", ..., "fdr_q":null, ...}
...
{"kind":"sweep_summary", "run_id":"<ulid>", "fdr_by_family": {
    "stats.autocorr.ljung_box@1": {
        "method": "benjamini_hochberg",
        "alpha": 0.05,
        "per_finding": [
            {"finding_index": 0, "raw_p": 0.012, "q_value": 0.024},
            {"finding_index": 1, "raw_p": 0.500, "q_value": 0.500}
        ]
    },
    "cross.lead_lag.ccf@1": {...}
}}
{"kind":"run_end", ...}
```

**FDR family scope default: per `scan_id@version`** (e.g., all `stats.autocorr.ljung_box@1` findings in the sweep form one family). Different scan types test different hypotheses — pooling them under one BH-FDR is statistically wrong. Caller can override via sweep manifest `[fdr].family = "scan_id" | "scan_family" | "all" | "none"`:
- `"scan_id"` (default) — per `scan_id@version`.
- `"scan_family"` — group by family prefix (`stats.*` / `cross.*` / `seas.*`).
- `"all"` — single sweep-wide family.
- `"none"` — emit `SweepSummary` with raw p-values but no q-values.

**Per-finding `finding_index`** is the position in the streaming JSONL output (zero-indexed across all `Result` envelopes for that scan_id). Consumers join q-values to findings via `(scan_id_at_version, finding_index)`.

**Why pragmatic:** streaming-friendly (no rewind); per-scan_id is the safe-by-default scientific choice (unrelated hypotheses); `SweepSummary` as an additive `Finding` variant follows the Phase 3 D3-21 `Finding::DryRun` precedent (schema-additive, single new `kind` tag); the reserved `fdr_q: null` slot in `ResultFinding` stays as a v2 hook for streaming-FDR variants if performance demands it.

**Single-shot `miner scan` (Phase 3 facade)** does NOT emit `SweepSummary` — there's no family to adjust over. Both `ResultFinding.fdr_q` and the absence of `SweepSummary` are documented in the README quickstart.

---

### D5-03: Effect size lives in a new typed `Effect.effect_size: Option<EffectSize { kind: String, value: f64 }>` field — NOT parallel scalars, NOT inside `effect.extra`

**Default schema-additive change to `Effect`:**

```rust
pub struct Effect {
    pub metric: String,
    pub value: f64,
    pub p_value: Option<f64>,
    pub n: Option<u64>,
    pub ci95: Option<[f64; 2]>,
    pub effect_size: Option<EffectSize>,   // NEW (Phase 5 / D5-03)
    pub extra: BTreeMap<String, RawArray>,
}

pub struct EffectSize {
    pub kind: String,    // e.g., "cohens_d", "hedges_g", "cliffs_delta", "vr_minus_one"
    pub value: f64,
}
```

**Per-scan canonical `kind` defaults (plan-phase research confirms):**

| Scan family | Canonical effect-size `kind` | Reference |
|------|-----------------------------|-----------|
| `stats.summary.welford@1` | `"cohens_d_vs_zero"` (mean / std) | scipy.stats.ttest_1samp magnitude |
| `stats.vol.rolling@1` | `"vol_ratio_to_baseline"` | last-window vol / baseline-window vol |
| `stats.autocorr.ljung_box*@1` | `"acf_lag_max_abs"` | max abs lag autocorrelation |
| `stats.stationarity.adf@1` / `kpss@1` | `"tau_signed"` (test stat itself) | statsmodels convention |
| `stats.variance_ratio.lo_mackinlay@1` | `"vr_minus_one"` | VR(k) - 1 at max k |
| `stats.heteroskedasticity.arch_lm@1` | `"lm_per_lag"` (LM / lag) | normalised LM |
| `stats.normality.jarque_bera@1` | `"jb_per_n"` (JB / n) | normalised JB |
| `stats.outliers.z_and_mad@1` | `"outlier_rate"` | fraction of obs flagged |
| `stats.drawdown.profile@1` | `"max_dd_pct"` | already in `effect.value` |
| `cross.corr.pearson_rolling@1` | `"r_last_window"` | last window's r |
| `cross.corr.spearman_rolling@1` | `"rho_last_window"` | last window's ρ |
| `cross.ols.rolling@1` | `"beta_last_window"` | last window's β |
| `cross.lead_lag.ccf@1` | `"argmax_ccf_value"` | argmax ρ̂ at argmax lag |
| `cross.cointegration.engle_granger@1` | `"hedge_ratio"` | β from y = α + β·x |
| `seas.bucket.*` | `"max_abs_t_stat"` (already in `effect.value`) | per-bucket t-stat magnitude |
| `seas.test.anova_kruskal@1` | `"omega_squared"` | ANOVA effect-size (1 - SS_res/SS_tot) |
| `seas.event.pre_post_window@1` | `"post_minus_pre_mean"` | post-event mean shift |

Plan-phase research pins exact `kind` strings against scipy / statsmodels conventions before implementation; the table above is the starting position.

**Why pragmatic:** typed struct keeps `kind` + `value` bound together (impossible to have one without the other — `effect.extra` cannot enforce this); a single field on `Effect` aligns with the existing `value` / `p_value` / `n` / `ci95` siblings; downstream Quant agent pattern-matches on `kind` for scan-appropriate interpretation. Parallel-scalar alternative (`effect.effect_size: Option<f64>` + `effect.effect_size_kind: Option<String>`) decouples the two fields and invites mismatched-pair bugs. Stashing it in `effect.extra` makes it not a first-class envelope output, violating HYG-01.

**Schema impact:** schema-additive — `Option<EffectSize>` serialises to `null` when absent; schemars 1.x emits an additive `oneOf` for the new struct type. Plan-phase research must regenerate `schemas/findings-v1.schema.json` and confirm zero `schema_version` bump (matches the Phase 3 D3-10 / D3-21 / Phase 4 D4-01 / D4-03 precedent).

---

### D5-04: Bootstrap + null are caller-opt-in via universal CLI / manifest flags; per-scan declared support via new `Scan` trait methods

**Default CLI surface (extends Phase 3 `ScanArgs`):**

```
miner scan <scan_id@version> --instrument SYMBOL:side --timeframe 15m --window <range> \
  [--bootstrap stationary|block] [--bootstrap-n 1000] \
  [--null phase_scramble|circular_shift] [--null-n 1000] \
  [--seed 0xDEADBEEF]
```

**Default sweep manifest surface (D5-01 `[hygiene]` block):** same four knobs (`bootstrap`, `bootstrap_n`, `null`, `null_n`) at the sweep level + per-job override. `--seed` lives in `[sweep].seed`.

**Default values:** bootstrap and null are BOTH OFF. The caller MUST opt in explicitly because both are O(N) extra resamples and the typical sweep is large enough that defaulting them on would 10-100× the wall-clock without informed consent.

**Per-scan declared support — new `Scan` trait methods:**

```rust
pub trait Scan: Send + Sync {
    // ... existing methods (id, version, arity, param_schema, finding_fields, run) ...

    /// Whether this scan can produce a bootstrap CI on its primary statistic.
    /// Default false — only scans whose statistic is a smooth function of an
    /// autocorrelated series benefit (most do, but discrete-output scans like
    /// outlier-count and event-window may not).
    fn supports_bootstrap(&self) -> bool { false }

    /// Whether this scan can produce a p-value under a given null method.
    /// Default false — only scans that already emit `p_value` benefit. Phase
    /// scramble is for autocorr / cointegration / lead-lag families; circular
    /// shift is broader. Plan-phase pins per-scan defaults.
    fn supports_null_method(&self, m: NullMethod) -> bool { false }
}

pub enum NullMethod { PhaseScramble, CircularShift }
```

**Preflight rejection:** if the caller passes `--bootstrap` to a scan whose `supports_bootstrap() == false`, preflight emits a structured error with the new `PreflightCode::HygieneNotSupported` variant + a message naming which scan rejected which method. Same for `--null`.

**Implementation locations (plan-phase decides exact module layout):**
- New `miner_core::scan::hygiene` module hosting `stationary_bootstrap_ci`, `block_bootstrap_ci`, `phase_scramble_null_p`, `circular_shift_null_p` kernels. Pure-Rust, deterministic given seed + input.
- Each scan opt-in via `supports_*` returning `true`; the engine's `run_one` body invokes the hygiene kernel after the scan's main `run()` body has emitted its base `effect` block.
- The hygiene kernel reads `req.bootstrap_method` / `req.null_method` / `req.master_seed` / per-job derived seed, then populates `effect.ci95` and/or replaces `effect.p_value`.

**Why pragmatic:** universal flags scale across the catalogue (don't multiply per-scan); per-scan declared support catches user error at preflight rather than mid-stream; structured `PreflightCode::HygieneNotSupported` matches the Phase 4 D4-02 `WrongInstrumentArity` precedent.

---

### D5-05: Bit-for-bit reproducibility via a new `ResultFinding.repro: Option<ReproEnvelope>` field with derived per-job seeds

**Default schema-additive field on `ResultFinding`:**

```rust
pub struct ResultFinding {
    // ... existing locked envelope fields + per-variant fields ...
    pub repro: Option<ReproEnvelope>,   // NEW (Phase 5 / D5-05)
}

pub struct ReproEnvelope {
    /// User-supplied master seed (--seed flag or [sweep].seed in manifest).
    /// When absent, derived as blake3(manifest_hash || run_id) and echoed here.
    pub master_seed: u64,
    /// Per-job seed derived deterministically from
    /// (master_seed, scan_id_at_version, instrument(s), timeframe, window,
    /// param_hash) so each job is independently reproducible.
    pub job_seed: u64,
    /// `bootstrap_method` + `bootstrap_n` echoed (None when bootstrap not run).
    pub bootstrap: Option<BootstrapSpec>,
    /// `null_method` + `null_n` echoed (None when null not run).
    pub null: Option<NullSpec>,
}

pub struct BootstrapSpec { pub method: String, pub n: u32 }   // method: "stationary" | "block"
pub struct NullSpec { pub method: String, pub n: u32 }        // method: "phase_scramble" | "circular_shift"
```

**Population rule:** `repro` is `Some(_)` when EITHER bootstrap or null was run; `None` when neither was requested (the typical Phase 4 single-shot scan). The `Effect.p_value` value depends on whether `repro.null` is `Some(_)` — if so, p-value is the empirical null-rank p; if not, the analytic p (Phase 4 behaviour).

**Per-job seed derivation:**
```
job_seed = blake3(
    master_seed.to_le_bytes() ||
    scan_id_at_version.as_bytes() ||
    instruments_canonical.as_bytes() ||
    timeframe.as_bytes() ||
    window_canonical.as_bytes() ||
    param_hash.as_bytes()
).first_8_bytes_as_u64()
```

This makes any single job reproducible from the envelope alone — re-run `miner scan` with `--seed <master_seed>` + the same request and the bootstrap / null samples will be byte-identical.

**RNG choice:** `rand::rngs::SmallRng::seed_from_u64(job_seed)` — fast, deterministic, plenty good for resampling. NOT a CSPRNG (overkill); NOT thread-local rngs (non-deterministic by construction). Plan-phase research confirms `SmallRng` reproduces across Rust toolchain versions (or pins an alternative deterministic RNG crate).

**Why pragmatic:** master seed + derived per-job seeds give a single user knob with deterministic propagation; echoing the seeds in the finding envelope makes re-runs auditable without external state; blake3 derivation matches the existing `param_hash` / `Blake3Hex` convention from Phase 2.

---

### Carry-forward from Phase 1-4 (not re-asked, listed for downstream agent reference)

- **`ScanRequest.instruments: Vec<InstrumentSpec>` + `Scan::arity()`** (Phase 4 D4-01/02) — sweep fanout produces these per-job; Pair-arity scans naturally consume length-2 vectors.
- **`Scan` trait stays object-safe** (Phase 3 `tests::scan_trait_object_safe` regression gate). The new `supports_bootstrap()` and `supports_null_method()` methods are dyn-safe (take `&self`, return `bool`).
- **`BTreeMap` discipline + byte-identical re-run (D3-23)** — sweep parallelism MUST preserve. Rayon `par_iter` over the job vector + a `Mutex<Box<dyn FindingSink>>` does NOT preserve order; the engine fans out, collects per-job buffered output, then drains in deterministic (manifest-order) sequence to the sink. Plan-phase confirms this against the existing Phase 3 `StdoutSink` performance.
- **Stdout = findings, stderr = logs** (D-15, D-19) — clippy `disallowed_macros` gate covers Phase 5 modules automatically.
- **`miner-core` stays sync + rayon + std-only** (FOUND-04) — no tokio anywhere in Phase 5.
- **`SIGINT`-safe shutdown** (D3-22) — sweep cancellation polls `ctx.cancel.load(Ordering::Relaxed)` between jobs and between resamples inside the bootstrap / null kernels. Exit code 130 takes precedence over 0/2.
- **Four-tier exit codes** (D3-24) — Phase 5 inherits unchanged. Mid-sweep `Finding::ScanError` triggers exit 2 same as single-shot.
- **Schema-additive discipline** — Plan-phase research regenerates `schemas/findings-v1.schema.json` after the D5-02 / D5-03 / D5-05 additions and confirms no `schema_version` bump (additive optional fields + additive enum variant).
- **statsmodels reference pinning** (Phase 4 04-11) — the new `seas.test.anova_kruskal@1` effect-size kind `"omega_squared"` follows the same goldens discipline; Plan-phase pins the version.

### Claude's Discretion (plan-phase + research own these)

These decisions can be resolved from Phase 3-4 patterns + Politis-Romano (1994) + statsmodels reference behaviour + plan-phase research without further user input:

- **Block-length default for stationary / block bootstrap** — Politis-White (2004) automatic block-length selector vs fixed default (e.g., `block_len = ceil(n^(1/3))`). Plan-phase picks one defensible default and documents.
- **Per-scan `supports_bootstrap()` / `supports_null_method()` defaults** — plan-phase produces the per-scan table (the D5-03 effect-size table is the starting point; bootstrap and null-method support should be a similar matrix).
- **Phase scramble exact algorithm** — Theiler et al. (1992) IAAFT vs simple phase randomisation; plan-phase picks IAAFT-with-rank-correction as the default.
- **Circular shift random-offset distribution** — uniform vs uniform-with-min-shift; plan-phase pins.
- **`SweepSummary` envelope schema** — exact field names, ordering, whether to include raw p-value alongside q-value. Plan-phase finalises.
- **Sweep dry-run output shape** — one `Finding::DryRun` per planned job vs one sweep-level `Finding::SweepDryRun` aggregate. Plan-phase picks based on consumer ergonomics; recommend the latter for sweep (per-job DryRun is verbose; sweep agent wants the count + a sample).
- **Sweep result emission ordering** — strictly deterministic (preserve manifest job order; rayon collects to buffer, then drains in order) vs completion-order with a final `SweepSummary` containing the order map. Plan-phase recommends deterministic-order for byte-identical re-run compliance.
- **`rand::SmallRng` cross-version stability** — confirm against `rand` crate documentation that `SmallRng::seed_from_u64` is stable across patch versions; if not, pin a specific RNG (e.g., `rand_xoshiro::Xoshiro256PlusPlus`) and document.
- **`PreflightCode::SweepTooLarge` default cap** — `[sweep].max_jobs` default of 100,000. Plan-phase picks based on memory budget (each job's BarFrame load).
- **`[hygiene]` block override resolution order** — per-job > sweep-level > CLI default. Plan-phase confirms.
- **Sweep `--dry-run` interaction with `--bootstrap` / `--null`** — dry-run echoes the requested hygiene config but does NOT execute the bootstrap / null kernels (the dry-run finding's `repro.bootstrap` / `.null` carry the spec only). Plan-phase pins.
- **Bootstrap CI confidence level** — default 95% (matches the existing `Effect.ci95` field name). Plan-phase confirms whether to expose `--ci-level 0.99` etc. or pin at 95%.

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Project-level (always relevant)
- `.planning/PROJECT.md` — Scope, constraints, out-of-scope list (no persistent results store, no DSL, no chart patterns). Phase 5 implements the "TOML sweep manifest" + "statistical hygiene" requirements in Active. The Out-of-Scope "Persistent findings store" reaffirms the streaming-and-stateless property — the new `SweepSummary` envelope is emitted to stdout once and never persisted by miner.
- `.planning/REQUIREMENTS.md` — OP-04 (TOML sweep manifest), HYG-01..05 (effect sizes, BH-FDR, block bootstrap, phase-scrambled nulls, bit-for-bit reproducibility). Phase 5 owns ALL of them. Also: HYG-v2-01 (DSR) stays deferred; the `ResultFinding.dsr` reserved-null slot is NOT populated in v1.
- `.planning/ROADMAP.md` §"Phase 5: Statistical Hygiene & Sweep Runner" — Goal statement, depends-on (Phase 4), five enumerated success criteria. Especially: criterion #4 (bit-for-bit reproducibility via seed echoed in the repro envelope) and criterion #5 (`--dry-run` job graph + estimated count).
- `.planning/STATE.md` — Locked decisions list. Phase 5 introduces THREE additive envelope-shape changes (D5-02 `Finding::SweepSummary` variant, D5-03 `Effect.effect_size` field, D5-05 `ResultFinding.repro` field) and TWO additive `PreflightCode` variants (`HygieneNotSupported`, `SweepTooLarge`). Schema-sync CI gate (FOUND-03 / Gate 4) MUST classify all five as `schema_version`-non-bumping.

### Phase-level prior CONTEXTs (every one is required reading)
- `.planning/phases/01-foundations-contracts/01-CONTEXT.md` — D-01..D-24 envelope and infra contracts. Phase 5 specifically depends on: D-03 (`raw.series.timestamps_ms` mandatory on every Raw), D-04 (input/output split), D-07 (four-tier exit codes), D-09/D-10/D-11 (RunStart/RunEnd framing — Phase 5 emits `SweepSummary` BETWEEN the last `Result` and `RunEnd`), D-15 (clippy `disallowed_macros`), D-19 (single sanctioned `FindingSink` writer).
- `.planning/phases/02-reader-aggregator-derived-bar-cache/02-CONTEXT.md` — D2-01..D2-21. Phase 5 consumes the BarCache + GapDetector verbatim; sweep fans out across `BarCache::get_or_build` calls (per-job parallelism). The `Blake3Hex` convention from D2-05 is the basis for the per-job seed derivation in D5-05.
- `.planning/phases/03-scan-engine-facade-cli/03-CONTEXT.md` — D3-01..D3-24. THE primary contract Phase 5 extends. Specifically: D3-14 (Scan trait — Phase 5 adds `supports_bootstrap()` + `supports_null_method()`), D3-18 (single-shot per invocation — Phase 5 ADDS the `miner sweep` subcommand without changing `miner scan`), D3-21 (`Finding::DryRun` — Phase 5 may extend or add a sweep-level variant per plan-phase decision), D3-22 (SIGINT polling — Phase 5 extends polling into hygiene kernels), D3-23 (byte-identical re-run — sweep MUST preserve this property), D3-24 (four-tier exit codes — Phase 5 inherits unchanged).
- `.planning/phases/04-scan-catalogue-anom-cross-seas/04-CONTEXT.md` — D4-01..D4-09. Phase 5 consumes the 22 scans Phase 4 shipped; each scan family's canonical effect-size `kind` (D5-03 table) follows Phase 4's per-scan statsmodels reference convention.
- `.planning/phases/04-scan-catalogue-anom-cross-seas/04-12-PLAN.md` + `04-13-SUMMARY.md` — Plan 04-12 closed CR-01 (Pair-arity engine dispatch); Plan 04-13 closed the CI Gate 2 (clippy::pedantic workspace cleanup). Phase 5 lands on a green-CI baseline.

### Live artifacts to be EXTENDED (NOT replaced) in Phase 5
- `./schemas/findings-v1.schema.json` — Phase 5 introduces THREE additive envelope changes:
  1. `Finding::SweepSummary(SweepSummaryFinding)` variant added to the existing `#[serde(tag = "kind")]` enum (D5-02) — additive to `oneOf`, expected schema-clean.
  2. `Effect.effect_size: Option<EffectSize>` field added (D5-03) — additive optional field on an existing type, schema-clean (`null` when absent).
  3. `ResultFinding.repro: Option<ReproEnvelope>` field added (D5-05) — additive optional field, schema-clean.
- `./crates/miner-core/src/findings/mod.rs` — `Effect` gains `effect_size: Option<EffectSize>`; `ResultFinding` gains `repro: Option<ReproEnvelope>`; `Finding` enum gains `SweepSummary(SweepSummaryFinding)` variant; new structs `EffectSize`, `ReproEnvelope`, `BootstrapSpec`, `NullSpec`, `SweepSummaryFinding`, `FdrFamilySummary`, `FindingFdrEntry` added.
- `./crates/miner-core/src/scan/mod.rs` — `Scan` trait gains `supports_bootstrap()` + `supports_null_method(NullMethod)` methods (default false). `NullMethod` enum added.
- `./crates/miner-core/src/scan/hygiene/` — NEW module with `bootstrap.rs` (stationary + block bootstrap CI kernels), `null.rs` (phase scramble + circular shift kernels), `fdr.rs` (BH-FDR adjustment), `seed.rs` (per-job seed derivation).
- `./crates/miner-core/src/engine/mod.rs` — `run_one_with_registry` extended to invoke hygiene kernels after the scan's base `run()`; sweep entry point `run_sweep(manifest: SweepManifest, ...)` added.
- `./crates/miner-core/src/error/codes.rs` — `PreflightCode` enum gains `HygieneNotSupported` + `SweepTooLarge` variants (additive to the open-string wire form, no schema bump).
- `./crates/miner-cli/src/cli.rs` — `Command` enum gains `Sweep(SweepArgs)`. `ScanArgs` extends with `--bootstrap`, `--bootstrap-n`, `--null`, `--null-n`, `--seed` (universal hygiene flags).
- `./crates/miner-cli/src/sweep_args.rs` — NEW. `SweepArgs` (clap derive) carries `manifest: PathBuf`, `--dry-run`, optional overrides.
- `./crates/miner-cli/src/sweep_manifest.rs` — NEW. TOML deserialiser for the D5-01 manifest shape; figment-or-serde based, plan decides.
- `./README.md` — Phase 5 plan-phase extends the Quickstart with one `miner sweep example.toml` invocation showing the typical Quant-agent workflow.

### External references (read during plan-phase, not bundled in repo)
- **Politis & Romano (1994) "The Stationary Bootstrap"** — JASA 89(428), 1303-1313. Reference for HYG-03 stationary bootstrap kernel.
- **Politis & White (2004) "Automatic Block-Length Selection for the Dependent Bootstrap"** — Econometric Reviews 23(1), 53-70. Reference for the automatic block-length selector (one of two options for the default).
- **Theiler et al. (1992) "Testing for Nonlinearity in Time Series: the Method of Surrogate Data"** — Physica D 58. Reference for IAAFT phase-scramble null.
- **Benjamini & Hochberg (1995) "Controlling the False Discovery Rate"** — JRSS B 57(1), 289-300. Reference for HYG-02 BH-FDR.
- `scipy.stats` + `statsmodels.stats.diagnostic` + `statsmodels.tsa.stattools` — Effect-size kind conventions per-scan; plan-phase pins versions in `tests/REFERENCE-VERSIONS.md` (extends the Phase 4 file).
- `rand` crate `docs.rs/rand` — `SmallRng` cross-version stability; plan-phase confirms or pins an alternative deterministic RNG.
- `toml` crate `docs.rs/toml` — sweep manifest deserialisation; standard choice for the workspace.

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets (from Phase 1-4, every one consumed by Phase 5)
- **`miner_core::findings::{Effect, ResultFinding, Finding, RunStart, RunEnd, RunSummary}`** — Phase 5 adds optional fields (`Effect.effect_size`, `ResultFinding.repro`) and one new `Finding` variant (`SweepSummary`). Existing variants unchanged.
- **`miner_core::scan::{Scan, ScanCtx, ScanRequest, ScanArity, ScanError, Registry, bootstrap}`** — Phase 5 extends `Scan` with two default-false methods and adds `NullMethod` enum.
- **`miner_core::engine::{run_one, run_one_with_registry, RunOutcome, GapDispatch, GapPolicyKind, framing, gap_policy, param_hash, preflight}`** — Phase 5 ADDS a `run_sweep` entry point; `run_one` is extended to invoke hygiene kernels post-scan-body; existing single-shot path unchanged for callers that don't request hygiene.
- **`miner_core::aggregator::{aggregate, AggParams, BarFrame, Timeframe}` + `miner_core::cache::BarCache`** — Phase 5 sweep fans out across `BarCache::get_or_build` calls (one per `(symbol, side, timeframe)` triple); the cache's two-axis invalidation handles per-job determinism naturally.
- **`miner_core::reader::{Reader, Side, InstrumentSpec, ClosedRangeUtc, Blake3Hex}`** — `InstrumentSpec` is what sweep manifest jobs deserialise into. `Blake3Hex` is the basis for per-job seed derivation.
- **`miner_core::calendar::Calendar` + `miner_core::gap::{GapDetector, GapManifest}`** — Phase 5 fans out across them per-job; no changes to the gap-policy contract.
- **`miner_core::error::{MinerError, PreflightCode, WireError, stderr_emit}`** — Phase 5 adds TWO new `PreflightCode` variants (`HygieneNotSupported`, `SweepTooLarge`).
- **`miner_core::findings::{FindingSink, StdoutSink, FileSink}` + `clippy.toml` disallowed-macros gate** — Phase 5's sweep modules all write through `FindingSink`. The rayon-parallel sweep buffers per-job outputs into per-thread `Vec<Finding>` then drains in deterministic order to the shared sink (under a `Mutex<&mut dyn FindingSink>` for the drain).
- **All 22 Phase 4 scans** — each one is opted into bootstrap and/or null methods by overriding the new default-false `supports_bootstrap()` + `supports_null_method()` returns; the per-scan opt-in table lives in plan-phase.

### Established Patterns (carry forward unchanged)
- **One-way dependency direction**: `miner-cli|mcp|http → miner-reader-dukascopy → miner-core`. Phase 5 introduces zero new edges. All hygiene kernels + sweep runner + manifest deserialiser live inside `miner-core`.
- **`BTreeMap` discipline**: every map on the Serialize path is `BTreeMap` (NEVER `HashMap`). Sweep's `fdr_by_family` is `BTreeMap<String, FdrFamilySummary>`; manifest's `per_job.params` deserialises to `BTreeMap<String, serde_json::Value>` to preserve param-key ordering for `param_hash`.
- **Hand-rolled `param_schema` JSON Schema fragments** (D3-14) — Phase 5 sweep doesn't introduce new per-scan `param_schema` calls; the sweep manifest's per-job `params` map flows through the existing scan-level `param_schema()` validation at preflight.
- **Pure-kernel module pattern** (LjungBoxScan + kernel.rs split) — Phase 5 hygiene kernels follow the same shape: `hygiene/bootstrap.rs` is pure functions on `&[f64]` + `seed: u64`, no IO, no serde, no Reader.
- **Test-fixture discipline**: synthetic deterministic fixtures in `crates/miner-core/tests/fixtures/`; golden bootstrap / null outputs checked in for at least one representative scan per family (plan-phase picks; recommend reusing the Phase 4 ANOM-02 / CROSS-05 / SEAS-01 goldens triple).
- **Look-ahead-safety + shuffled-future regression** (Phase 3 D3-09, Phase 4 D4-extension) — every Phase 5 bootstrap / null kernel is causal (no future bars in any resample); the existing `shuffled_future_regression.rs` test pattern extends.
- **SIGINT cancel polling** (D3-22) — bootstrap inner loops poll `ctx.cancel.load(Ordering::Relaxed)` between resamples (cheap with a polling cadence of every N=64 resamples, similar to Plan 04-10's `CANCEL_POLL_CADENCE = 4096` for SEAS scans).

### Integration Points (where Phase 5 code connects to existing system)
- **`Scan` trait methods `supports_bootstrap()` + `supports_null_method()`** — NEW default-false methods; object-safety regression gate (`scan_trait_object_safe` test) MUST continue to pass after the addition.
- **`PreflightCode` enum** — TWO new variants added.
- **`miner scans` JSONL output** — each scan's line gains `supports_bootstrap` + `supports_null_method` booleans + a list of supported null methods. Schema update is additive.
- **`Finding` enum** — ONE new variant (`SweepSummary`); existing six unchanged.
- **`Effect` struct** — ONE new optional field (`effect_size`); existing fields unchanged.
- **`ResultFinding` struct** — ONE new optional field (`repro`); existing fields unchanged.

### New Phase 5 module layout (planning input — Plan can revise)
- `crates/miner-core/src/scan/hygiene/mod.rs` — re-exports of the four sub-modules.
- `crates/miner-core/src/scan/hygiene/bootstrap.rs` — `stationary_bootstrap_ci(values: &[f64], stat: impl Fn(&[f64]) -> f64, n_resamples: u32, block_param: f64, seed: u64) -> [f64; 2]` (and `block_bootstrap_ci` sibling).
- `crates/miner-core/src/scan/hygiene/null.rs` — `phase_scramble_null_p(...)`, `circular_shift_null_p(...)`.
- `crates/miner-core/src/scan/hygiene/fdr.rs` — `bh_fdr(p_values: &[f64], alpha: f64) -> Vec<f64>` (returns adjusted q-values in input order).
- `crates/miner-core/src/scan/hygiene/seed.rs` — `derive_job_seed(master_seed, scan_id_at_version, instruments, timeframe, window, param_hash) -> u64`.
- `crates/miner-core/src/sweep/mod.rs` — `SweepManifest` deserialiser + `run_sweep` entry point.
- `crates/miner-core/src/sweep/manifest.rs` — TOML schema types (`SweepManifest`, `JobBlock`, `HygieneBlock`, `SweepConfig`).
- `crates/miner-core/src/sweep/job_graph.rs` — cartesian expansion + `Vec<ResolvedJob>` produced from the manifest.
- `crates/miner-core/src/sweep/executor.rs` — rayon-parallel job execution + deterministic-order drain to sink.
- `crates/miner-core/tests/sweep_smoke.rs` — end-to-end sweep with 2 scans × 2 instruments × 1 timeframe × 1 window × 1 param-grid.
- `crates/miner-core/tests/bootstrap_seed_reproducibility.rs` — re-run sweep twice with same seed, assert byte-identical bootstrap samples.
- `crates/miner-core/tests/bh_fdr_kernel.rs` — golden kernel test against statsmodels `multipletests(..., method='fdr_bh')`.
- `crates/miner-core/tests/sweep_dry_run.rs` — `--dry-run` emits sweep dry-run record with planned job count.
- `crates/miner-core/tests/sigint_mid_sweep.rs` — SIGINT mid-sweep preserves streamed findings, no `SweepSummary` emitted, exit code 130.

### Reusable Phase 3-4 scan-implementation patterns
- **Plan 04-12 / D5-04 parallel:** the new `supports_*` methods follow the Phase 4 `arity()` precedent — declared trait method, queried at preflight, validation error emitted via `PreflightCode::WrongInstrumentArity`-style structured rejection.
- **Plan 04-11 / D5-02 parallel:** end-of-sweep `SweepSummary` follows the Phase 4 D4-03 `DataSlice.sources: Vec<Source>` schema-additivity playbook — additive change, schemars regen, schema-sync diff inspection BEFORE committing the API.
- **Plan 04-05 / D5-04 parallel:** sequential summation (not rayon) for AIC lag selection in ADF — block-bootstrap resamples MUST be sequential within a single bootstrap call (the master seed sets the resample sequence; parallel resamples would scramble it). Plan-phase pins the sequential discipline for bootstrap inner loops.

### No new workspace dependencies expected
Every Phase 5 kernel should land on the existing Phase 1-4 stack:
- `statrs` (distributions + CDFs for BH-FDR + p-value transforms) — already a dep.
- `ndarray` + `ndarray-stats` (vectorised resampling kernels — may not be needed; plan-phase decides between `ndarray` and pure-`Vec<f64>` per-kernel) — already deps.
- `nalgebra` (small fixed-size linear algebra for circulant matrix in phase-scramble) — already a dep.
- `rand` + `rand_xoshiro` (deterministic RNGs for resampling) — NEW dep. `rand` is in CLAUDE.md's recommended stack; pin to `rand = "0.8"` + `rand_xoshiro = "0.6"` (or whichever provides `Xoshiro256PlusPlus`). Plan-phase confirms cross-version stability against `SmallRng` first; falls back to `Xoshiro256PlusPlus` if `SmallRng` is not stable.
- `toml` (sweep manifest deserialisation) — NEW dep. Pin `toml = "0.8"` (workspace already uses `figment` for config; `figment-toml` could be reused but the sweep manifest needs the raw TOML deserialiser without figment's profile/env layering — Plan-phase picks).
- `chrono` (sweep window parsing — already a dep).
- pure-std for everything else.

</code_context>

<specifics>
## Specific Ideas

User decisions in this discussion:

The user explicitly delegated all four gray areas to Claude's-discretion pragmatic defaults during discuss-phase ("I'll accept pragmatic defaults on all of the above. Let's move to planning"). The defaults captured in D5-01 through D5-05 above carry the discussion forward to plan-phase, where research confirms or refines them against the cited literature (Politis-Romano 1994, Politis-White 2004, Theiler et al. 1992, Benjamini-Hochberg 1995, statsmodels / scipy reference behaviour).

Recurring user themes (carried forward from prior phases):
- **The Quant agent is THE consumer.** Phase 5's envelope additions (`SweepSummary` envelope, `Effect.effect_size`, `ResultFinding.repro`) must be self-describing — the agent reads any finding without per-scan knowledge of which bootstrap algo or null method was used. The `kind` field on `EffectSize` and the echoed `method` strings on `BootstrapSpec` / `NullSpec` make this possible.
- **Agent-operability across CLI / MCP / HTTP is non-negotiable.** Phase 5 commits the universal hygiene flags + sweep contract; Phase 6 mirrors them across MCP tools and HTTP endpoints with byte-identical JSONL.
- **Determinism is a hard property.** Same `(master_seed, scan_id, instruments, timeframe, window, param_hash, bootstrap_method, bootstrap_n, null_method, null_n)` MUST produce byte-identical JSONL (modulo `run_id` + clock fields). The new per-job seed derivation makes this auditable per-finding.
- **No silent scans over gapped data.** Sweep inherits the Phase 3 + 4 gap-policy contract verbatim; each per-job invocation runs the same `--gap-policy=strict|continuous_only` dispatch as single-shot.

</specifics>

<deferred>
## Deferred Ideas

Items that came up during discussion or are explicitly outside Phase 5 scope:

- **Deflated Sharpe Ratio (DSR)** — REQUIREMENTS.md HYG-v2-01. `ResultFinding.dsr` stays reserved-null in v1. v2 hook is the existing nullable field on `ResultFinding`.
- **"Top-N interesting findings" sweep summary** — HYG-v2-02. Outside Phase 5 scope; `SweepSummary` carries q-values + raw p-values but does NOT rank findings or compute effect-size × q composite scores.
- **Memoised per-sweep intermediates / in-memory arena** — HYG-v2-03. Phase 5 re-loads BarFrames per job via the existing BarCache (which IS persistent across jobs via Arrow IPC). In-memory cross-job arena is v2.
- **Side-channel raw-array storage** (URI / file reference instead of inline base64) — HYG-v2-04. Phase 5 inlines raw arrays per Phase 1 contract; if findings push into MB-scale, v2 introduces the side-channel.
- **PyO3 bindings** — PLAT-v2-01. Phase 5 stays binary-only.
- **GARCH / EGARCH / Hamilton MS-AR / Bayesian online change-point / wavelet seasonality** — PLAT-v2-03..PLAT-v2-06. All v2.
- **Johansen cointegration (basket scans)** — SCAN-v2-01. The `ScanArity::Many(min, max)` enum extension stays deferred until a v2 basket scan demands it.
- **Granger causality, Hurst / R-S / DFA, PELT change-point, correlation-breakdown, basket divergence z-score, Anderson-Darling** — SCAN-v2-02..SCAN-v2-07. All v2.
- **Sweep manifest zip / aligned-axis expansion** (alternative to cartesian) — defer to v2 if a use case demands it. Phase 5 ships cartesian-only (D5-01).
- **Streaming BH-FDR variant** (compute q-values during the streaming pass without an end-of-sweep summary) — defer. The reserved `ResultFinding.fdr_q` slot remains a v2 hook.
- **MCP + HTTP parity for sweep / bootstrap / null** — Phase 6. Phase 5 commits the contract; Phase 6 mirrors.
- **Bench harness for sweep wall-clock** + **flamegraph profiling of bootstrap inner loops** — Phase 7.
- **README cookbook for the Quant agent's typical sweep recipes** — Phase 7 README hardening.
- **Caller-supplied custom RNG seed sequences** (beyond the master-seed → derived-per-job model) — defer. The blake3-derived per-job seed is deterministic and sufficient.

</deferred>

<open_questions>
## Open Questions for Research / Plan-Phase

Plan-phase research must confirm or override every "Claude's Discretion" decision in `<decisions>`. The blocking-for-the-plan items are:

1. **Schema-additive guarantee for D5-02 + D5-03 + D5-05 envelope additions.** Regen `schemas/findings-v1.schema.json` with the three new fields and one new `Finding` variant; inspect the diff against the Phase 4 schema. Plan-phase MUST NOT commit the API changes until the schema-sync diff is `schema_version`-non-bumping. Fallback if schemars produces a non-additive diff: per-field gated emission (always-present-as-`null` for the new fields) — same pattern used for the Phase 1 `dsr` / `fdr_q` reserved slots.

2. **Block-length default for stationary / block bootstrap.** Pick one of (a) Politis-White (2004) automatic selector with a hard floor (e.g., `max(2, ceil(n^(1/3)))`), (b) fixed default `block_param = ceil(n^(1/3))`. Recommend (a) for correctness on autocorrelated series at the cost of one extra O(n log n) pass to estimate the autocorrelation length. Plan-phase research pins.

3. **Per-scan `supports_bootstrap()` + `supports_null_method()` defaults table.** Plan-phase produces a per-scan matrix (the D5-03 effect-size table is the starting point). Default candidates:
   - All ANOM scans on autocorrelated series (ANOM-02..ANOM-11) → `supports_bootstrap() == true`.
   - All ANOM autocorrelation/stationarity/variance-ratio scans (ANOM-04..ANOM-08) → `supports_null_method(PhaseScramble) == true`.
   - All CROSS scans (CROSS-02..CROSS-05) → bootstrap + circular_shift null both true.
   - All SEAS bucket scans (SEAS-01..SEAS-04) → bootstrap true; null methods false (bucket effects aren't a phase-scrambled hypothesis).
   - SEAS-05 ANOVA / Kruskal-Wallis → both false (already produces p-values from F + KW analytically).
   - SEAS-06 event window → bootstrap true; null methods false.
   Plan-phase finalises against the literature.

4. **Phase scramble exact algorithm — IAAFT vs simple phase randomisation.** Recommend IAAFT (Iterative Amplitude Adjusted Fourier Transform; Theiler et al. 1992) because it preserves both the marginal distribution AND the power spectrum — crucial for heavy-tailed financial return series. Plan-phase pins the iteration count (typical: 10) and convergence criterion.

5. **`rand::SmallRng` cross-version stability.** `rand` crate's `SmallRng` is documented as "may change between minor versions". For HYG-05 bit-for-bit reproducibility across miner releases, plan-phase MUST either (a) pin `rand` patch-version + add a regression test, OR (b) switch to `rand_xoshiro::Xoshiro256PlusPlus` which IS guaranteed stable. Recommend (b) for the explicit stability contract.

6. **`SweepSummary` envelope schema final shape.** Plan-phase finalises field names, ordering, optional inclusion of raw p-values alongside q-values, BTreeMap key ordering. Recommend: include both raw `p_value` AND adjusted `q_value` for every finding so the consumer can re-run BH-FDR with a different alpha without re-running miner.

7. **Sweep dry-run emission shape.** Recommend ONE sweep-level `Finding::SweepDryRun` aggregate (NOT one `Finding::DryRun` per planned job — that's verbose for large sweeps). Plan-phase decides whether to add a new variant or extend the existing `DryRunFinding` with a `planned_job_count: u64` field. Recommend the latter (less envelope churn).

8. **Sweep result emission ordering.** Pin DETERMINISTIC-order (manifest job order) over completion-order. Implementation: rayon `par_iter` over the resolved job vector, each worker writes its findings into a per-job `Vec<Finding>` buffer; after all workers complete, the main thread drains the buffers in manifest order to the shared `FindingSink`. Trades some peak memory (one job's buffered findings × N_workers in flight) for byte-identical re-run compliance.

9. **`PreflightCode::SweepTooLarge` default cap.** Default `[sweep].max_jobs = 100,000`. Plan-phase picks based on memory budget — at ~1 KB per finding + a few MB per BarFrame load, 100K jobs is roughly 100 MB findings + 28 instruments × 6 years of cache loads. May need to tune lower if cache load is the bottleneck.

10. **TOML deserialisation crate.** `toml = "0.8"` vs `figment` + `figment-toml`. Recommend bare `toml` for the sweep manifest because figment's profile / env-layering doesn't apply here (the manifest is the single source of truth). Plan-phase confirms `toml` crate compatibility with `serde::Deserialize` on the new manifest schema types.

11. **Sweep + bootstrap + null performance envelope.** Plan-phase research estimates wall-clock for the typical Quant-agent workflow (e.g., 28 instruments × 3 timeframes × 6 years × 11 ANOM scans × 1000 bootstrap resamples × 1000 null resamples). If the projected time is unacceptable, plan-phase may need to revisit the per-job bootstrap discipline (e.g., share bootstrap samples across scans on the same input series).

12. **`hygiene/bootstrap.rs` kernel signature.** Decide between (a) generic over a stat closure: `fn bootstrap_ci<F: Fn(&[f64]) -> f64>(...)`, (b) typed: `fn bootstrap_ci(values: &[f64], stat_kind: StatKind, ...)`. Plan-phase recommends (a) for flexibility; the stat closure is constructed at the scan's call site (each scan knows its own `effect.value` formula).

</open_questions>

---

*Phase 5 context complete. 5 user-locked-by-default decisions captured (D5-01 sweep manifest cartesian shape, D5-02 `SweepSummary` envelope + per-scan_id BH-FDR scope, D5-03 typed `EffectSize` field, D5-04 universal opt-in flags + per-scan declared support, D5-05 `ReproEnvelope` with derived per-job seeds). 12 open questions for plan-phase research. 7 Phase 5 requirements (OP-04, HYG-01..05) mapped to module layout and envelope shape. Ready for `/gsd-plan-phase 5`.*
