# Phase 4: Scan Catalogue (ANOM, CROSS, SEAS) — Context

**Gathered:** 2026-05-19
**Status:** Ready for planning

<domain>
## Phase Boundary

Phase 4 rolls out the v1 scan catalogue against the Phase 3 facade — 22 scans implementing every ANOM (11), CROSS (5), and SEAS (6) requirement in REQUIREMENTS.md. Each scan registers in the `bootstrap()` factory (D3-16) following the `<family>.<subfamily>.<scan_name>@<version>` naming convention (D3-17), implements the locked `Scan` trait (D3-14), and emits findings through the locked envelope (Phase 1 contract + Phase 3 additive extensions).

What Phase 4 delivers:

1. **All 22 v1 scans wired into the Registry** — every requirement (ANOM-01..11, CROSS-01..05, SEAS-01..06) ships as a registered `Scan` impl callable via `miner scan <id@version>`, MCP (Phase 6 parity surface), and HTTP (Phase 6 parity surface).
2. **ANOM-01 returns primitive** — the reusable log-returns / simple-returns / intraday-vs-overnight split kernel that Phase 3 inlined (D3-02) and deferred. Phase 4 publishes the proper primitive AND refactors `LjungBoxScan` to call it.
3. **CROSS-01 time-alignment primitive** — the inner-join-on-common-timestamps kernel that CROSS-02..05 consume. Pure-kernel utility (not a directly callable scan).
4. **Two-leg scan request shape** — `ScanRequest.instrument: String + side: Side` GENERALISES to `instruments: Vec<InstrumentSpec { symbol, side }>`. Single-leg ANOM/SEAS scans take length 1; CROSS scans take length 2. This is the breaking facade change Phase 6 will mirror in MCP/HTTP. (D4-01)
5. **Per-scan arity validation** — new `Scan::arity() -> ScanArity { Single, Pair }` trait method validated at preflight via new `PreflightCode::WrongInstrumentArity`. Surfaces in `miner scans` catalogue so MCP/HTTP can render typed parameter shapes. (D4-02)
6. **Two-leg DataSlice + Raw shape** — `DataSlice.source: Source` GENERALISES to `DataSlice.sources: Vec<Source>` (length = scan arity, parallel to `instruments`). Raw.series uses leg-labelled keys (`returns_a`, `returns_b`, `timestamps_ms_aligned`). (D4-03)
7. **CROSS two-leg gap-policy semantics** — inner-join-only v1; gap policy operates on the INTERSECTION of both legs' manifests. `strict` aborts if either leg has gaps; `continuous_only` partitions on intervals where BOTH legs are continuous. (D4-04)
8. **Golden-fixture parity for representative scans** — at least one ANOM, one CROSS, one SEAS golden checked in against pinned scipy/statsmodels reference outputs (success criterion #5).
9. **Look-ahead-safety surface for rolling scans** — Phase 4 must surface the `ScanCtx::bars_up_to(ts)` API that Phase 3 deferred. The shuffled-future regression test (D3-09) extends to every rolling/causal scan.

What Phase 4 does NOT deliver (belongs in later phases):

- **Statistical hygiene layer** — Cohen's d, Hedges' g, Cliff's delta, block bootstrap, phase-scrambled nulls, BH-FDR q-values, Deflated Sharpe Ratio. `effect.ci95`, `dsr`, `fdr_q` stay `null` in v1 per Phase 1 contract (Phase 5 / HYG-*).
- **TOML sweep manifest fanout** — Phase 5 / OP-04. Phase 4 stays single-shot per invocation (carries forward D3-18).
- **MCP / HTTP wrappers** — Phase 6 / OP-02 / OP-03. Phase 4's facade change is what those wrappers will mirror.
- **Bench harness / flamegraph / golden regression suite scale-out** — Phase 7.
- **v2 scan families** — Johansen (basket cointegration), Granger causality, Hurst / R-S / DFA, PELT change-point, correlation-breakdown, basket divergence, Anderson-Darling — all REQUIREMENTS.md v2 (SCAN-v2-*).

The user is not a Rust practitioner; Rust-ecosystem decisions not user-locked below default to the most pragmatic Rust-community-standard pattern and to consistency with Phases 1-3 decisions. Plan-phase research must confirm or override every Claude's-discretion call.

</domain>

<decisions>
## Implementation Decisions

### Two-leg request shape (user-locked)

- **D4-01: `ScanRequest.instrument: String + side: Side` generalises to `instruments: Vec<InstrumentSpec { symbol: String, side: Side }>`.** ANOM/SEAS scans take length 1; CROSS scans take length 2. This is a BREAKING change to the Phase 3 facade and is the right time to make it — Phase 6 hasn't committed yet. Rationale: the alternatives (`Option<peer_instrument>` sibling field, or `peer` inside params) each carry asymmetries (privileged-primary leg vs equal legs, structural-vs-bag-of-strings provenance, MCP/HTTP tool typing). The Vec extends cleanly to a future v2 basket scan (Johansen, 3+ instruments). Single-leg invocations still send a Vec of length 1; the shape is uniform.

- **D4-02: New `Scan::arity() -> ScanArity` trait method validated at preflight; CLI uses repeatable `--instrument SYMBOL:side` flag.** `ScanArity` is a Plan-decided enum (initial variants `Single` and `Pair`; can extend to `Many(min, max)` if v2 baskets need it). Preflight rejects wrong arity with a new `PreflightCode::WrongInstrumentArity` variant. The arity is exposed in `miner scans` catalogue output so MCP/HTTP can render a typed parameter surface in Phase 6. CLI: `miner scan cross.corr.pearson_rolling@1 --instrument EURUSD:bid --instrument GBPUSD:bid --timeframe 15m --window 2024-01-01:2024-12-31`. Repeatable flag, leg order = vector order, `SYMBOL:side` syntax parsed at the CLI boundary (validation reused for both legs).

- **D4-03: `DataSlice.source: Source` generalises to `DataSlice.sources: Vec<Source>` (length = arity, parallel to `ScanRequest.instruments`).** `Raw.series` keys carry leg labels: `returns_a`, `returns_b`, `timestamps_ms_aligned` (the inner-joined timestamp vector after CROSS-01). Self-describing per-finding; consumer reconstructs which leg is which by index. Single-leg findings keep the same single-element Vec shape — no privileged-primary leg, no plural/singular ambiguity. Schema impact is a Vec replacing a struct at one Serialize-path location; plan-phase research MUST confirm schemars 1.x emits a schema-additive change (i.e., no `schema_version` bump). If the change is non-additive, fall back to D4-03-ALT: keep `source: Source` for the primary leg and add `peer_sources: Vec<Source>` (still a clean Vec, just shifted off the primary slot).

- **D4-04: CROSS-01 ships inner-join-only as the v1 time-alignment primitive; gap policy operates on the INTERSECTION of both legs' manifests.** `strict` aborts if EITHER leg has any gap in the requested window (emits one `Finding::GapAborted` carrying both legs' manifests under the per-leg keys of `data_slice.gap_manifest`). `continuous_only` partitions on intervals where BOTH legs are continuous (the intersection of each leg's gap-free sub-ranges), emitting one `Finding::Result` per intersection sub-range. The REQUIREMENTS.md phrase "configurable behaviour for unequal calendars" is satisfied by CROSS-01 being a documented callable primitive — alternative join modes (forward-fill, strict_equal) land in v2 (deferred). Forward-fill is explicitly rejected for v1 because it introduces subtle look-ahead landmines for rolling stats (a forward-filled bar at time T appears to know about T but reflects T-1's price).

### ANOM-01 returns primitive (Claude's Discretion — plan-phase confirms)

- **D4-05 (discretion): ANOM-01 ships BOTH a kernel utility AND a standalone callable scan.** The kernel utility lives in `miner-core::scan::primitives::returns` (or similar) and is called by every ANOM scan that needs log returns + by CROSS-02..05 after the time-alignment primitive joins both legs. The standalone scan registers as `stats.returns.log@1` (or `stats.returns.profile@1`) so the Quant agent can request just-the-returns via the same facade. The scan's `Finding::Result` emits `effect.metric = "returns_log"`, `effect.value = mean log return`, `effect.n = sample size`, `raw.series.returns + raw.series.timestamps_ms`. Plan-phase decides the exact scan name and whether to register one scan per return type (log / simple / intraday / overnight) or one scan with `--params variant=log|simple|...`.

- **D4-06 (discretion): The Phase 3 `LjungBoxScan` refactors in Phase 4 to call the ANOM-01 primitive.** This is the cleanup task Phase 3 deferred (D3-02). A one-line task in the Plan; the refactor MUST NOT change the goldens (the kernel emits byte-identical log returns for the same input, by construction).

### Per-leg side independence + effect.value canonical pick (deferred to plan-phase)

The user explicitly delegated these technical-discretion decisions to plan-phase + statsmodels reference behaviour. Plan-phase research must lock them before Phase 4 implementation begins:

- **D4-07 (open-question, plan-phase decides): Can CROSS legs carry independent `side` values (e.g., `EURUSD-bid × GBPUSD-ask`)?** Default suggestion: yes (the `InstrumentSpec.side` is per-leg already by D4-01's design); typical agent use is same-side both legs but the contract should not preclude bid-vs-ask cross-spread scans. Plan-phase may add a per-scan policy if any specific CROSS scan rejects mixed sides.

- **D4-08 (open-question, plan-phase decides): `effect.value` canonical pick for vector-output CROSS scans (rolling Pearson, rolling Spearman, rolling OLS, lead-lag CCF).** Each emits a vector of per-window statistics. Default suggestion: `effect.value = last_window_value` (most recent rolling stat) matching trader intuition; the full vector goes into `effect.extra.values` + `effect.extra.window_starts_ms`. Engle-Granger CROSS-05 is single-shot so it sets `effect.value = hedge_ratio` and emits `effect.extra.{adf_stat, adf_p_value, ou_half_life, residual_std}`. Plan-phase research must check statsmodels' canonical reporting convention for each scan family and align.

- **D4-09 (open-question, plan-phase decides): Hedge-ratio sign convention for Engle-Granger CROSS-05.** The two-step Engle-Granger regression `y_t = α + β x_t + ε_t` produces a hedge ratio β whose sign depends on which leg is regressor vs regressand. Default suggestion: leg-order in `ScanRequest.instruments` determines regressand (leg `a`) vs regressor (leg `b`); `effect.value = β` from `a ~ b`. Plan-phase research must confirm against statsmodels' coint() ordering and document.

### Carry-forward from Phase 3 (not re-asked)

These decisions carry from prior CONTEXT.md files and require NO re-discussion. Listed here for downstream agent reference:

- **Scan trait + Registry + `bootstrap()` extension pattern** (D3-14, D3-16) — Phase 4 adds 22 `r.register(Box::new(...))` lines, alphabetical by `scan_id`.
- **`scan_id` naming convention** (D3-17) — `<family>.<subfamily>.<scan_name>`; Phase 4 family values are `stats` (ANOM), `cross` (CROSS), `seas` (SEAS).
- **`@version` integer suffix** (D3-17) — every Phase 4 scan starts at `version = 1`.
- **Single-shot per invocation** (D3-18) — fanout is Phase 5.
- **Hand-rolled `param_schema` returning `serde_json::Value`** (D3-14) — Plan may switch to typed `#[derive(JsonSchema)]` per-scan if ergonomics demand it.
- **`effect.ci95` / `dsr` / `fdr_q` stay `null` in v1** — Phase 5 hygiene layer adds them.
- **`raw.series.timestamps_ms` is mandatory** (Phase 1 D-03) — every finding ships its timestamp vector.
- **`param_hash` = blake3-lowercase-hex of `serde_json::to_vec(&resolved_params)`** (D3-13) — `BTreeMap`-only maps, no `HashMap` anywhere on the Serialize path (OUT-03).
- **SIGINT-safe shutdown** (D3-22) — every Phase 4 scan polls `ctx.cancel.load(Ordering::Relaxed)` between findings (cheap for single-shot scans; mandatory inside rolling-stat inner loops).
- **Byte-identical re-run** (D3-23) — every Phase 4 scan is deterministic modulo `run_id` + clock-read fields. Shuffled-future regression test (D3-09) extends to every rolling/causal scan.
- **`miner-core` stays sync + rayon + std-only** (FOUND-04) — no tokio anywhere in Phase 4.
- **Stdout = findings, stderr = logs** (D-15, D-19) — clippy `disallowed_macros` gate covers Phase 4 scan modules automatically.

### Claude's Discretion (plan-phase + research own these)

Decisions Claude can resolve from Phase 3 patterns + statsmodels reference behaviour + Plan-phase research without further user input:

- The 22-scan plan/wave breakdown (one plan per family? per scan? layered kernels first?) — pure organisational decision for `/gsd:plan-phase 4`.
- Hand-rolled JSON Schema vs typed `#[derive(JsonSchema)]` per scan — Phase 3 used hand-rolled (D3-14); Plan may switch if 22 hand-rolled schemas become unwieldy.
- Specific session-boundary defaults for SEAS-03 (Asia / London / NY / overlap UTC ranges) — FX-major conventions are well-documented; Plan picks one defensible source and documents.
- Trading-day-of-month cutoff for SEAS-04 EOM/SOM bucketing — last-N / first-N where N defaults to a Plan-chosen sensible value (typically 3 or 5).
- Default outlier z-threshold for ANOM-10 — Plan picks (3.0 / 4.0 / both, configurable).
- AIC-selected lag specifics for ADF (ANOM-05) — pin against statsmodels default and document.
- ARCH-LM lag default for ANOM-08 — pin against statsmodels default and document.
- Schemars 1.x schema-additive guarantee for the D4-01..D4-03 facade changes — plan-phase research must regenerate `findings-v1.schema.json` and inspect the diff before committing.
- Reference-implementation pinning policy (single statsmodels version for all goldens, or per-scan pins; insta JSONL snapshot with float tolerance per field vs separate float-array comparison file) — plan-phase decision tied to the chosen test discipline.
- ANOM-01 surface details — single scan with `--params variant=log|simple|intraday|overnight` vs four registered scans; Plan picks based on ergonomics and `miner scans` catalogue readability.
- Look-ahead-safety enforcement mechanism — `ScanCtx::bars_up_to(ts)` API shape (slice vs sub-frame vs index range) — Plan picks the simplest passing shape; the shuffled-future regression pins behaviour regardless of shape.

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Project-level (always relevant)
- `.planning/PROJECT.md` — Scope, constraints, out-of-scope list (no chart patterns, no strategy generation, no persistent results store, no DSL). Phase 4 implements the "statistical anomaly", "cross-instrument relationship", and "seasonality" scan families listed in Active Requirements.
- `.planning/REQUIREMENTS.md` — ANOM-01..11, CROSS-01..05, SEAS-01..06 (22 requirements). Phase 4 owns ALL of them. Also: HYG-01..05 stay deferred to Phase 5; OUT-02 envelope spec is unchanged by Phase 4 (the new `DataSlice.sources` Vec extension is the only OUT-* surface area touched).
- `.planning/ROADMAP.md` §"Phase 4: Scan Catalogue (ANOM, CROSS, SEAS)" — Goal statement, depends-on (Phase 3), five enumerated success criteria. Especially: criterion #4 (`consistent Finding envelope shape across all 22 scans, single discriminant by scan_id, scan-specific extras in documented effect.extra object`) and criterion #5 (golden fixtures for one representative ANOM / CROSS / SEAS).
- `.planning/STATE.md` — Locked decisions list (sync+rayon core, stdout/findings discipline, schema-version lock, Arrow IPC cache, two-axis cache invalidation). Phase 4 introduces TWO facade-shape changes (D4-01 instruments Vec, D4-03 sources Vec) that the schema regen MUST classify as additive (or the plan falls back to D4-03-ALT).

### Phase-level prior CONTEXTs (every one is required reading)
- `.planning/phases/01-foundations-contracts/01-CONTEXT.md` — D-01..D-24 envelope and infra contracts. Phase 4 specifically depends on: D-03 (timestamps_ms mandatory on every Raw), D-04 (input/output split), D-05 (mid-run errors are findings), D-07 (three-tier exit codes, extended in Phase 3 to four-tier with SIGINT 130), D-08 (gap-policy outputs are findings), D-09/D-10/D-11 (RunStart/RunEnd framing with ULID), D-15 (clippy `disallowed_macros`), D-19 (single sanctioned `FindingSink` writer).
- `.planning/phases/01-foundations-contracts/01-RESEARCH.md` — `error_code` vocabulary section; Phase 4 introduces ONE new `PreflightCode::WrongInstrumentArity` variant per D4-02.
- `.planning/phases/02-reader-aggregator-derived-bar-cache/02-CONTEXT.md` — D2-01..D2-21 reader / aggregator / cache / gap contracts. Phase 4 consumes: D2-08 (Calendar API for SEAS session bucketing), D2-12 (Reader trait — unchanged), D2-14 (BarFrame columnar shape — the scan kernel input), D2-16/D2-17 (GapDetector + GapManifest — Phase 4's CROSS scans intersect two manifests per D4-04), D2-19 (bar boundary convention — affects rolling-window arithmetic).
- `.planning/phases/02-reader-aggregator-derived-bar-cache/02-VERIFICATION.md` — Confirms Phase 2 surface unchanged.
- `.planning/phases/03-scan-engine-facade-cli/03-CONTEXT.md` — **THE primary contract Phase 4 extends.** D3-01..D3-24. Phase 4 specifically extends: D3-14 (Scan trait — adds `arity()` method per D4-02), D3-17 (scan-id naming — Phase 4 names all 22 scans), D3-18 (single-shot per invocation — unchanged, fanout is Phase 5), D3-21 (`Finding::DryRun` — Phase 4's rolling scans must produce a dry-run estimate that respects look-ahead-safety), D3-22 (SIGINT polling — Phase 4 inner loops poll cancel between findings), D3-23 (byte-identical re-run — Phase 4 every scan obeys this), D3-24 (four-tier exit codes — Phase 4 inherits unchanged).
- `.planning/phases/03-scan-engine-facade-cli/03-VERIFICATION.md` — Confirms Phase 3 facade is verified and stable. Phase 4's facade-shape changes (D4-01..D4-03) are additive extensions to a verified surface.

### Live artifacts to be EXTENDED (NOT replaced) in Phase 4
- `./schemas/findings-v1.schema.json` — Phase 4 introduces THREE additive changes the schema-sync CI gate must classify as `schema_version`-non-bumping:
  1. `ScanRequest.instrument: String + side: Side` → `instruments: Vec<InstrumentSpec>` (D4-01) — STRUCTURAL change, may NOT be cleanly additive; plan-phase research must regen and inspect. Fallback plan: dual-emit (legacy `instrument` field + new `instruments` Vec) during the transition; v2 drops the legacy field.
  2. `DataSlice.source: Source` → `DataSlice.sources: Vec<Source>` (D4-03) — same shape; same fallback plan (D4-03-ALT keeps `source` + adds `peer_sources: Vec<Source>`).
  3. `PreflightCode::WrongInstrumentArity` enum variant addition (D4-02) — additive to an existing `oneOf`-style enum schema, expected schema-clean.
- `./crates/miner-core/src/findings/mod.rs` — `DataSlice` field shape change (D4-03); finding envelope shape stays the 6-variant enum (no new variants — Phase 3 already added `DryRun`; Phase 4 adds no new variants).
- `./crates/miner-core/src/scan/mod.rs` — `Scan` trait gains `arity()` method (D4-02); `ScanRequest` field shape change (D4-01).
- `./crates/miner-core/src/scan/registry.rs` — `bootstrap()` extends with 22 more `register()` lines, alphabetical by scan-id.
- `./crates/miner-core/src/scan/` — 22 new scan modules (one per scan, grouped under `scan/anom/`, `scan/cross/`, `scan/seas/` subdirectories per Plan's preference) + `scan/primitives/` for ANOM-01 returns + CROSS-01 time-alignment.
- `./crates/miner-core/src/error/codes.rs` — `PreflightCode` enum gains `WrongInstrumentArity` variant (D4-02).
- `./crates/miner-cli/src/scan_args.rs` — `ScanArgs::instrument: String` + `side: Side` → repeatable `instruments: Vec<InstrumentSpec>` parsed from `--instrument SYMBOL:side` (D4-02). Backward-compat: a single `--instrument` + `--side` form may remain as a sugar (Plan decides).
- `./README.md` — Phase 4 plan-phase MUST extend the Quickstart with at least one each of an ANOM / CROSS / SEAS scan invocation.

### External references (read during plan-phase, not bundled in repo)
- `scipy.stats` + `statsmodels.tsa.stattools` + `statsmodels.stats.diagnostic` — Reference truth source for ANOM-04..09, CROSS-04..05, SEAS-05. Plan-phase MUST pin exact versions in a Plan-owned `tests/REFERENCE-VERSIONS.md` (or similar) so goldens are reproducible. Phase 3 pinned statsmodels 0.14.6 for Ljung-Box; Phase 4 may pin the same single version for all scans or pin per-scan.
- `arch` Python package — Reference for ANOM-08 (ARCH-LM). Plan-phase research confirms whether `statsmodels.stats.diagnostic.het_arch` covers ANOM-08 fully or the `arch` package is needed for the heteroskedasticity-robust z-stat.
- Box-Jenkins / Lo-MacKinlay / Engle-Granger original papers — Background reading; not strictly required if statsmodels reference matches the documented algorithm. Plan-phase decides whether any scan needs hand-derivation outside statsmodels' implementation.

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets (from Phase 1-3, every one consumed by Phase 4)
- **`miner_core::findings::{Finding, ResultFinding, ScanErrorFinding, GapAbortedFinding, DryRunFinding, RunStart, RunEnd, RunSummary, RunId, Source, DataSlice, TimeRange, Effect, Raw, RawArray, Base64Bytes, Dtype}`** — The full envelope vocabulary. Phase 4 constructs `ResultFinding` 22+ times (one per scan; rolling scans emit one per window or one per invocation depending on the per-scan emission shape Plan picks for that scan).
- **`miner_core::scan::{Scan, ScanCtx, ScanRequest, ScanError, ScanFindingShape, Registry, bootstrap}`** — The Phase 3 trait surface. Phase 4 extends `Scan` with `arity()` (D4-02) and `ScanRequest` with the `instruments: Vec<InstrumentSpec>` field (D4-01).
- **`miner_core::scan::ljung_box::{LjungBoxScan, kernel::{log_returns, biased_acf, ljung_box_q_and_p}}`** — Phase 3 demo scan. Phase 4 refactors `LjungBoxScan` to call the new ANOM-01 returns primitive (D4-06); the `biased_acf` + `ljung_box_q_and_p` kernels stay.
- **`miner_core::engine::{run_one, run_one_with_registry, RunOutcome, GapDispatch, GapPolicyKind, framing, gap_policy, param_hash, preflight}`** — Phase 3 facade. Phase 4 extends preflight to validate `instruments.len() == scan.arity()` (D4-02); extends gap_policy to intersect two legs' manifests under CROSS scans (D4-04); framing stays unchanged.
- **`miner_core::aggregator::{aggregate, AggParams, BarFrame, Timeframe, AGGREGATOR_VERSION}`** — `BarFrame` is the columnar input for every scan kernel. CROSS-01 time-alignment primitive operates on TWO `BarFrame`s and returns the joined view.
- **`miner_core::cache::{BarCache, ARROW_SCHEMA_VERSION, build_arrow_schema}`** — Phase 4 fetches BarFrames via `BarCache::get_or_build`, ONE per instrument leg. CROSS scans call it twice.
- **`miner_core::gap::{GapDetector, GapManifest, GapSpan, GapReason}`** — Phase 4 intersects two manifests for CROSS scans per D4-04. The GapManifest struct stays; the intersection is a new helper function (Plan decides where it lives — `gap::intersect()` or `scan::primitives::time_alignment::intersect_gaps()`).
- **`miner_core::error::{MinerError, PreflightCode, ScanErrorCode, WireError, stderr_emit}`** — Phase 4 adds ONE new `PreflightCode::WrongInstrumentArity` variant (D4-02).
- **`miner_core::reader::{Reader, Side, ClosedRangeUtc, RawBar, Blake3Hex}`** — `Side` enum becomes a field of `InstrumentSpec`. `Reader` trait is unchanged.
- **`miner_core::calendar::Calendar`** — Used by SEAS-03 trading-session bucketing (Asia/London/NY/overlap) and SEAS-04 trading-day-of-month bucketing.
- **`miner_core::findings::{FindingSink, StdoutSink, FileSink}` + `clippy.toml` disallowed-macros gate** — Phase 4's 22 scans all write through `FindingSink`; no `println!` / `eprintln!` anywhere.
- **`miner_cli::scan_args::ScanArgs`** — Phase 4 extends to repeatable `--instrument SYMBOL:side` (D4-02). Plan decides whether the single-instrument legacy form (`--instrument SYMBOL --side SIDE`) survives as sugar.

### Established Patterns (carry forward unchanged)
- **One-way dependency direction**: `miner-cli|mcp|http → miner-reader-dukascopy → miner-core`. Phase 4 introduces zero new edges. All 22 scans + ANOM-01 primitive + CROSS-01 primitive live inside `miner-core`.
- **`BTreeMap` discipline**: every map on the Serialize path is `BTreeMap` (NEVER `HashMap`). Phase 4's `Registry::scans: BTreeMap<(String, u32), Box<dyn Scan>>` already obeys this; new scan params + finding extras follow.
- **Hand-rolled `param_schema` JSON Schema fragments** (D3-14) — Phase 4 follows the LjungBoxScan pattern for each of the 22 scans. If a scan's params are complex (e.g., session-boundary list for SEAS-03), Plan may opt into typed `Params` struct with `#[derive(JsonSchema)]` for that scan only.
- **Pure-kernel module pattern** (LjungBoxScan + kernel.rs split) — every Phase 4 scan with non-trivial math splits into `mod.rs` (`Scan` impl + envelope construction) + `kernel.rs` (pure functions, no IO, no serde, no Reader). Unit-testable in isolation.
- **Test-fixture discipline**: synthetic deterministic fixtures in `crates/<crate>/tests/fixtures/` + checked-in goldens in `crates/miner-core/tests/goldens/` (or similar Plan-decided location). One representative scan per family (ANOM / CROSS / SEAS) gets a full statsmodels golden per success criterion #5.
- **Look-ahead-safety**: every rolling/causal scan inherits the shuffled-future regression test pattern from Phase 3's `shuffled_future_regression.rs`. Plan-phase scales this to a per-scan integration-test file.
- **SIGINT cancel polling** (D3-22): every scan inner loop (rolling-window iteration, lag-grid iteration, bootstrap-iter when Phase 5 lands) polls `ctx.cancel.load(Ordering::Relaxed)` between findings or between expensive inner iterations.

### Integration Points (where Phase 4 code connects to existing system)
- **`Registry::bootstrap()`** — Phase 4 adds 22 `r.register(Box::new(...))` lines (alphabetical by scan_id).
- **`Scan` trait method `arity()`** — NEW method added in `scan/mod.rs`. Object-safety regression gate (`scan_trait_object_safe` test) MUST continue to pass after the addition.
- **`PreflightCode` enum** — One new `WrongInstrumentArity` variant in `error/codes.rs`.
- **`miner scans` JSONL output** — Each scan's line gains an `arity` field (e.g., `"arity":"single"` / `"arity":"pair"`). Schema update on `miner scans` output is itself schema-additive.
- **`ScanCtx::bars(symbol, side, timeframe, range)`** — Existing single-instrument access stays. CROSS scans call it twice (once per leg). `ScanCtx::bars_up_to(ts)` (look-ahead-safety) is the new API for rolling scans — Plan picks the shape.

### New Phase 4 module layout (planning input — Plan can revise)
- `crates/miner-core/src/scan/primitives/returns.rs` — ANOM-01 log/simple/intraday/overnight returns kernel.
- `crates/miner-core/src/scan/primitives/time_alignment.rs` — CROSS-01 inner-join + two-leg gap-manifest intersection kernel.
- `crates/miner-core/src/scan/anom/` — 11 modules (one per ANOM-01..11 scan).
- `crates/miner-core/src/scan/cross/` — 5 modules (one per CROSS-01..05 scan).
- `crates/miner-core/src/scan/seas/` — 6 modules (one per SEAS-01..06 scan).
- `crates/miner-core/tests/scan_<name>.rs` — Per-scan integration tests (golden + shuffled-future where applicable).
- `crates/miner-core/tests/goldens/` (or similar) — Checked-in statsmodels/scipy reference outputs for at least one ANOM / CROSS / SEAS scan.

### No new workspace dependencies expected
Every Phase 4 scan should land on the existing Phase 1-3 stack: `statrs` (distributions + CDFs for p-values), `ndarray` + `ndarray-stats` (rolling kernels — may need to add `ndarray` if not already present; check Cargo.toml), `nalgebra` (small fixed-size OLS for CROSS-03), `chrono` (already a dep, for SEAS session bucketing), pure-std for everything else. Plan-phase confirms `ndarray` is in `miner-core` deps and adds if not; pulls `ndarray-stats` only if its primitives meaningfully shorten the rolling kernels (otherwise hand-roll for transparency).

</code_context>

<specifics>
## Specific Ideas

User decisions in this discussion:

1. **`ScanRequest.instrument + side` generalises to `instruments: Vec<InstrumentSpec>`.** Phase 4 is the right time to break the facade — Phase 6 hasn't committed yet. Vec is uniform across ANOM/SEAS (length 1) and CROSS (length 2), and extends cleanly to v2 basket scans. Chosen over `Option<peer_instrument>` (asymmetric, awkward MCP typing) and `params.peer = "..."` (peer not structurally visible).
2. **Repeatable `--instrument SYMBOL:side` CLI flag + new `Scan::arity()` trait method.** Declarative arity surfaces in `miner scans` catalogue for MCP/HTTP Phase 6 to render typed parameters. Preflight rejects wrong arity via new `PreflightCode::WrongInstrumentArity`. Chosen over comma-list, two named flags (`--instrument-a` / `--instrument-b`), and runtime-only validation inside `Scan::run`.
3. **`DataSlice.sources: Vec<Source>` + leg-labelled `Raw.series` keys (`returns_a`, `returns_b`, `timestamps_ms_aligned`).** Self-describing per-finding; no privileged-primary leg. Schema-additive is a plan-phase research checkpoint (D4-03-ALT fallback: keep `source` + add `peer_sources: Vec<Source>`).
4. **CROSS-01 ships inner-join-only + gap-policy on the intersection of both legs' manifests.** `strict` aborts if either leg has gaps; `continuous_only` partitions on both-legs-continuous intervals. Alternative join modes (forward-fill, strict_equal) deferred to v2 — forward-fill explicitly rejected for v1 due to look-ahead landmines for rolling stats.

Recurring user themes (carried forward from prior phases):
- **The Quant agent is THE consumer.** Phase 4's envelope additions (`DataSlice.sources` Vec, leg-labelled raw arrays, `Scan::arity()` in catalogue output) must be self-describing — the agent decodes any finding without per-scan knowledge.
- **Agent-operability across CLI / MCP / HTTP is non-negotiable.** Phase 4's facade extension (`instruments: Vec`, `arity()` method) is the contract Phase 6's MCP/HTTP wrappers will validate parity against. Byte-identical findings across surfaces stays the hard rule.
- **Determinism is a hard property.** Every Phase 4 scan obeys D3-23 (same inputs → same JSONL bytes modulo run_id + timestamps). Rolling scans inherit the shuffled-future regression discipline from Phase 3.
- **No silent scans over gapped data.** Phase 4 inherits this for two-leg CROSS scans via the intersection gap policy — strict policy aborts if EITHER leg has a gap; continuous_only partitions on intersections only.

</specifics>

<deferred>
## Deferred Ideas

Items that came up during discussion or that the user explicitly delegated:

- **Envelope shape for multi-output scans (rolling vol, summary stats, bucketed scans)** — User declined to discuss explicitly. Plan-phase + Claude resolve based on Phase 3 patterns + statsmodels reference convention. Default heuristics: rolling scans emit ONE finding with vector arrays in `effect.extra.values` + `effect.extra.window_starts_ms`, `effect.value = last_window_value`; summary-stats scans (ANOM-02) emit ONE finding with `effect.value = mean`, all other stats in `effect.extra`; bucketed scans (SEAS-01..04) emit ONE finding with bucket vectors in `effect.extra.{buckets, means, stds, counts, t_stats}`. Plan-phase locks per-scan.
- **Golden-test discipline (statsmodels/scipy pinning + tolerance policy)** — User declined to discuss explicitly. Plan-phase + Claude resolve. Default: single pinned version (statsmodels 0.14.6, scipy 1.x.y) recorded in a Plan-owned `tests/REFERENCE-VERSIONS.md`; insta JSONL snapshot for envelope shape with float tolerance per field (Plan picks 1e-10 / 1e-8 / 1e-6 per scan based on numerical stability).
- **ANOM-01 returns primitive surface** — User declined to discuss explicitly. Default suggestion: standalone callable scan `stats.returns.log@1` (or `stats.returns.profile@1`) + reusable kernel utility. Plan-phase picks the exact name and decides one scan vs four (per variant: log / simple / intraday / overnight).
- **Per-leg side independence for CROSS scans** — User delegated to plan-phase. Default: yes, legs carry independent sides; typical use is same-side but contract does not preclude bid-vs-ask cross-spread scans.
- **`effect.value` canonical pick for vector-output CROSS scans** — User delegated to plan-phase. Defaults proposed in D4-08; plan-phase locks per-scan against statsmodels reporting convention.
- **Hedge-ratio sign convention for Engle-Granger CROSS-05** — User delegated to plan-phase. Default: leg-order in `instruments` determines regressand (`a`) vs regressor (`b`); plan-phase research confirms against statsmodels' `coint()` ordering.
- **Phase 4 plan/wave breakdown** — Organisational decision for `/gsd:plan-phase 4`. Likely shape: layered (kernels first → primitives → ANOM → CROSS → SEAS → integration tests), but Plan owns it.
- **Look-ahead-safety enforcement mechanism (`ScanCtx::bars_up_to(ts)` API shape)** — Plan picks; shuffled-future regression pins behaviour regardless of API shape.
- **Hand-rolled vs typed `#[derive(JsonSchema)]` per-scan param_schema** — Phase 3 used hand-rolled; Plan may switch per-scan if ergonomics demand it.
- **Session-boundary defaults for SEAS-03 (Asia / London / NY UTC ranges)** — Plan picks defensible source and documents.
- **Trading-day-of-month cutoff for SEAS-04 EOM/SOM bucketing** — Plan picks (likely last/first 3 trading days; configurable via params).
- **Default outlier z-threshold for ANOM-10** — Plan picks (likely 3.0 with z + modified-z both reported).
- **AIC lag selection details for ANOM-05 (ADF), ARCH-LM default lag for ANOM-08** — Plan pins against statsmodels defaults.
- **v2 scan families** — Johansen, Granger, Hurst, PELT, correlation-breakdown, basket divergence, Anderson-Darling — REQUIREMENTS.md SCAN-v2-*. Out of Phase 4 scope.
- **Statistical hygiene layer (Cohen's d / Hedges' g / Cliff's delta / block bootstrap / phase-scramble nulls / BH-FDR / DSR)** — Phase 5 / HYG-*. Phase 4 emits `effect.ci95` / `dsr` / `fdr_q` as `null` per Phase 1 contract.

</deferred>

<open_questions>
## Open Questions for Research / Plan-Phase

Plan-phase research must confirm or override every "Claude's Discretion" decision above. The blocking-for-the-plan items are:

1. **Schema-additive guarantee for D4-01 + D4-03 facade changes.** Regen `findings-v1.schema.json` with the `instruments: Vec<InstrumentSpec>` (D4-01) and `sources: Vec<Source>` (D4-03) field-shape changes and inspect the diff against the Phase 3 schema. If schemars emits non-additive output (i.e., the schema-sync CI gate fails), invoke the D4-03-ALT fallback (`peer_sources: Vec<Source>` additive sibling) and document the deviation. Plan-phase MUST not commit the API change until the schema-sync diff is green.
2. **Reference-implementation pinning policy.** Pick one of: (a) single `statsmodels==0.14.6` + `scipy==1.x.y` pin for all goldens, recorded in `tests/REFERENCE-VERSIONS.md`; (b) per-scan pins. Recommend (a) unless a scan requires a version statsmodels does not yet ship in 0.14.6. The Phase 3 LjungBoxScan golden is already on 0.14.6 — keeping it is the path of least resistance.
3. **`effect.value` canonical pick per scan.** For each of the 22 scans, document the headline scalar in `effect.value` and the dictionary keys in `effect.extra`. Plan-phase produces a per-scan envelope-shape table that the integration tests pin via insta snapshots.
4. **ANOM-01 returns primitive surface (callable scan vs kernel-only).** Default suggestion: ship both. Plan-phase decides scan name and whether one scan with `--params variant=...` or four registered scans.
5. **Look-ahead-safety API shape on `ScanCtx`.** `bars_up_to(ts) -> &[BarRow]` vs sub-frame vs index-range. Plan picks the simplest passing shape; the shuffled-future regression test pins behaviour either way.
6. **Two-leg `BarCache::get_or_build` call ordering + cancel polling.** CROSS scans fetch two BarFrames sequentially before computing. Plan-phase decides whether the cancel flag is polled between fetches (cheap insurance) or only after both are ready.
7. **Hedge-ratio sign convention + `effect.value` direction for Engle-Granger.** Statsmodels' `coint()` returns specific ordering; Plan-phase research confirms.
8. **Session-boundary defaults for SEAS-03.** Plan picks defensible UTC ranges for Asia / London / NY / overlap.
9. **CROSS-04 lead-lag CCF lag grid default.** Plan picks (likely ±N lags symmetric around zero, N configurable; default N tied to timeframe).
10. **`Scan::arity()` enum variants.** Plan-phase decides whether to ship `ScanArity::{Single, Pair}` or `ScanArity::{Single, Pair, Many(min, max)}` from day one. Recommend the two-variant minimum — v2 basket scans can extend the enum additively later.

</open_questions>

---

*Phase 4 context complete. 9 decisions captured (4 user-locked: D4-01 instruments Vec, D4-02 arity trait + CLI shape, D4-03 sources Vec + leg-labelled raw, D4-04 inner-join + intersection gap policy; plus 5 Claude's-discretion within the user-locked framework: D4-05/D4-06 ANOM-01 surface, D4-07 per-leg side independence, D4-08 effect.value canonical pick, D4-09 hedge-ratio sign — all marked plan-phase-decides). 22 Phase 4 requirements (ANOM-01..11, CROSS-01..05, SEAS-01..06) mapped to module layout and envelope shape. Ready for `/gsd-plan-phase 4`.*
