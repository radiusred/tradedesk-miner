# Phase 3: Scan Engine, Facade & CLI — Context

**Gathered:** 2026-05-18
**Status:** Ready for planning

<domain>
## Phase Boundary

Phase 3 turns the locked envelope (Phase 1) and the data layer (Phase 2) into a running scan engine plus the CLI that validates the facade before Phase 6's MCP/HTTP wrappers commit to it. Concretely:

1. **`Scan` trait + scan registry** — versioned `(id, version)` keys; parameter schema + finding-fields introspection per scan; unknown-scan and invalid-parameter rejection at the boundary with the locked `PreflightCode` vocabulary.
2. **Facade** — the single library entry point CLI/MCP/HTTP all call. Owns `RunStart`/`RunEnd` framing emission, `param_hash` computation, run-id assignment, sink dispatch, error classification, exit-code routing.
3. **Look-ahead-safe windowing** — `--window START:END` is the *output* window (the range whose bars the scan emits statistics about); the scan may read earlier bars internally for warm-up. Findings carry the *actual consumed range* in `data_slice.range`, post gap-partitioning. The shuffled-future regression test pins this: stats up to time T must be byte-identical when bars at time >T are shuffled.
4. **Gap-policy enforcement** — `--gap-policy=strict` consumes Phase 2's `GapManifest` and emits one `Finding::GapAborted` per run; `--gap-policy=continuous_only` partitions the requested range into maximal gap-free sub-ranges and emits one `Finding::Result` per sub-range, with the manifest inlined in each finding's `data_slice` (additive `gap_manifest` field — see D3-10).
5. **CLI subcommands** — `miner scan <id@version> --instrument ... --side ... --timeframe ... --window ... --gap-policy ... [--dry-run]` and `miner scans` (catalogue introspection). Both flow through the facade.
6. **One fully-implemented demo scan — Ljung-Box on returns (ANOM-04)** — proves the facade end-to-end with `effect.value` / `effect.p_value` / `effect.n` / `effect.extra.{lags,q_stats,p_values,acf}` / `raw.series.{returns,timestamps_ms}` populated. Validated against statsmodels golden bytes.
7. **SIGINT-safe shutdown** — every finding already written to stdout survives a `^C`; rayon worker pool shuts down cleanly; exit code 130 (128 + SIGINT) per POSIX.
8. **Byte-identical re-runs** — same inputs → same JSONL bytes (modulo `run_id` ULID and timestamps). Sorted emission, BTreeMap throughout, no clock reads in the scan kernel.

What Phase 3 does NOT deliver (belongs in later phases): the other 21 scans (Phase 4), TOML sweep manifest fanout (Phase 5), statistical hygiene layer — bootstrap, BH-FDR, phase-scramble nulls, DSR (Phase 5), MCP / HTTP wrappers (Phase 6), bench harness + flamegraph (Phase 7).

The user is not a Rust practitioner; Rust-ecosystem choices not user-locked below default to the most pragmatic Rust-community-standard pattern. Plan-phase research must confirm or override every Claude's-discretion decision.

</domain>

<decisions>
## Implementation Decisions

### Demo Scan — Ljung-Box on returns (ANOM-04, user-locked)

- **D3-01: Phase 3's fully-implemented end-to-end scan is Ljung-Box on log returns.** `scan_id@version = "stats.autocorr.ljung_box@1"`. Chosen because it exercises every slot of the Phase 1 envelope (`effect.value` headline stat, `effect.p_value`, `effect.n`, multiple arrays in `effect.extra`, paired input arrays in `raw.series`), AND because `statsmodels.stats.diagnostic.acorr_ljungbox` provides a checked-in golden the implementation must byte-match within documented float tolerances. This is the canonical proof that the facade carries Phase 4's hardest envelope shape (test-stat + p-value + scan-derived arrays + raw inputs) end-to-end before Phase 4 scales it to 21 more scans.

- **D3-02: Inline log-returns inside the scan; do NOT pull ANOM-01 forward.** The Phase 3 Ljung-Box scan computes `returns[t] = ln(close[t] / close[t-1])` itself from the `BarFrame.close` column. ANOM-01 (reusable returns primitive) stays in Phase 4. When Phase 4 lands ANOM-01 properly the Ljung-Box scan refactors to call the shared primitive — Phase 3 ships the inline implementation. Avoids scope creep; one extra Phase-4 refactor task is acceptable.

- **D3-03: Lag default is Box-Jenkins `lags = min(10, n / 5)`; user-overridable.** Matches `statsmodels.acorr_ljungbox`'s default exactly so golden-comparison is direct. CLI passes overrides via `--params lags=<int>`. Rejected at the boundary with `PreflightCode::InvalidParameter` when `lags < 1 || lags >= n`. The *resolved* value (post-defaults) is echoed into every finding's `params` block per the Phase 1 contract, and feeds `param_hash` (D3-13).

- **D3-04: Effect shape locked. Per-lag arrays + ACF ship in `effect.extra`.**
  - `effect.metric` = `"ljung_box_q"`
  - `effect.value` = Q-stat at max lag (headline stat)
  - `effect.p_value` = chi-squared p-value at max lag (df = lags)
  - `effect.n` = sample size of the returns series
  - `effect.ci95` = `null` in v1 (Phase 5 adds bootstrap CIs via HYG-03)
  - `effect.extra.lags` = `RawArray` of `[1, 2, …, max_lag]` as `f64`
  - `effect.extra.q_stats` = `RawArray` of per-lag Q-stats as `f64`
  - `effect.extra.p_values` = `RawArray` of per-lag p-values as `f64`
  - `effect.extra.acf` = `RawArray` of sample autocorrelation per lag as `f64`
  - `raw.series.returns` = `RawArray` of the computed log returns
  - `raw.series.timestamps_ms` = `RawArray` of the bars' `ts_open_utc` in epoch milliseconds (D-03 mandatory)
  Matches statsmodels output one-for-one (the quant agent can re-plot, re-test, or re-cut without re-querying the cache).

- **D3-05: Golden fixture is a tiny deterministic synthetic series.** A checked-in `f64` array (e.g., 256 samples drawn from a fixed-seed AR(1) process) with the expected statsmodels `acorr_ljungbox` output bytes (lags, Q-stats, p-values, ACF) baked in. The Phase 3 integration test feeds this through the full facade (CLI → engine → sink → stdout) and `insta`-snapshots the resulting JSONL. Plan-phase pins the tolerance per element (float comparison, not byte equality on the floats themselves; byte equality is on the *finding envelope shape* + the canonical-JSON encoding).

### Windowing Semantics (Claude's Discretion — confirm in plan-phase research)

- **D3-06 (discretion): `--window START:END` is the OUTPUT window, half-open `[START, END)`.** The scan reads bars whose `ts_open_utc` falls in `[START, END)` from the BarCache (which the facade fetches via `BarCache::get_or_build`). The scan MAY read bars earlier than START for lookback warm-up if its statistic requires it; Ljung-Box does not. When a scan IS lookback-bearing (Phase 4 rolling stats), it MUST NOT read bars later than the timestamp it is currently emitting a finding for — this is the look-ahead-safety invariant, enforced structurally by the `ScanCtx::bars_up_to(ts)` API (see D3-15).

- **D3-07 (discretion): CLI window flag accepts ISO 8601 date-or-datetime, half-open, UTC-only.** Forms: `2024-01-01:2024-12-31` (date-only → midnight UTC on each side), `2024-01-01T00:00:00Z:2024-12-31T00:00:00Z` (explicit datetime), or a `--from`/`--to` pair as an alternative form. No timezone suffixes other than `Z`. Parsing failure → `PreflightCode::InvalidParameter` with the structured-error JSON line on stderr. Missing window → `PreflightCode::MissingRequiredConfig` in v1 (Phase 3 does NOT auto-derive a window from the gap manifest's available range; the user must be explicit so the finding's `data_slice.range` has unambiguous provenance).

- **D3-08 (discretion): `data_slice.range` = the actual consumed range, post gap-partitioning.** Under `--gap-policy=strict` with no gaps, this equals the requested window. Under `strict` with gaps, no `Result` finding is emitted — only one `GapAborted` carrying the manifest (D-08 reaffirmed). Under `continuous_only`, each emitted `Result` finding's `data_slice.range` = its sub-range. The `RunStart.request` block carries the user-*requested* window verbatim so the audit trail has both halves (requested vs consumed).

- **D3-09 (discretion): Shuffled-future regression test.** The Phase 3 regression suite ships a test fixture of N bars; the test computes Ljung-Box up to cutpoint T, then shuffles bars at indices >T, recomputes, and asserts the pre-T statistic is byte-identical. This pins the look-ahead-safety invariant for the demo scan. Phase 4 extends this test to every scan with a rolling/causal stat.

### Gap-Policy Emission (Claude's Discretion — confirm in plan-phase research)

- **D3-10 (discretion): `continuous_only` inlines the full gap manifest into every finding's `data_slice`.** Extends `DataSlice` with an additive optional field `gap_manifest: Option<GapManifest>` (and keeps the existing `gap_manifest_ref: Option<String>` for future content-addressed deduplication in Phase 7+). v1 trades some duplicated bytes per finding for self-describing findings — each result is fully decodable without cross-referencing. Plan-phase research must confirm this additive change to `DataSlice` does NOT require bumping `schema_version` (additive optional fields are schemars-clean per the Phase 1 contract; the schema-sync CI gate will diff the new field into `schemas/findings-v1.schema.json`).

- **D3-11 (discretion): `strict` policy emission shape.** ONE `Finding::GapAborted` is emitted per `(symbol, side, requested_window)` invocation, before any potential `Result` finding could be emitted (in practice: under strict + gaps present, NO `Result` is ever emitted — the run aborts after the manifest finding, exit code 0, per D-08). The `GapAborted.gap_manifest` field is populated with the FULL Phase-2 `GapManifest` JSON. `GapAbortedFinding.data_slice.range` = the user-requested window (NOT a sub-range — strict didn't partition).

- **D3-12 (discretion): Zero-gap fast path.** When the gap manifest is empty under `continuous_only`, the run emits one `Result` finding (or zero if the scan computes none on that input) with `data_slice.gap_manifest = Some(GapManifest { gaps: vec![] })`. The empty-gap manifest still ships in `data_slice` so consumers can structurally assume the field is present under `continuous_only`. Under `strict` with zero gaps, NO `GapAborted` is emitted — the scan runs normally and the finding's `data_slice.gap_manifest` is `None` (strict provides no manifest in the success path; consumers read the policy from `RunStart.request.gap_policy`).

### `param_hash` Canonicalization (Claude's Discretion — confirm in plan-phase research)

- **D3-13 (discretion): `param_hash` = lowercase-hex blake3 of `serde_json::to_vec(&resolved_params)`.** Resolved params are the post-defaults `serde_json::Value` (or scan-typed param struct serialised through serde). Since Phase 1 mandates `BTreeMap`-only for maps (D-15 / OUT-03 deterministic ordering) and the resolved-params struct contains no `HashMap`, the resulting JSON is byte-stable across replays. RFC 8785 JCS canonicalisation is deferred unless plan-phase research uncovers a non-BTreeMap path. The hash is a 64-char lowercase-hex string (matches `Blake3Hex` from Phase 2 reader fingerprints).

### Scan Trait + Engine (Claude's Discretion — confirm in plan-phase research)

- **D3-14 (discretion): `Scan` trait shape.** Lives in `miner-core::scan::Scan`:
  ```rust
  pub trait Scan: Send + Sync {
      fn id(&self) -> &'static str;                           // "stats.autocorr.ljung_box"
      fn version(&self) -> u32;                               // 1
      fn param_schema(&self) -> serde_json::Value;            // JSON Schema fragment
      fn finding_fields(&self) -> ScanFindingShape;           // documented effect.extra keys
      fn run(
          &self,
          ctx: &ScanCtx,
          req: &ScanRequest,
          sink: &mut dyn FindingSink,
      ) -> Result<(), ScanError>;
  }
  ```
  `Send + Sync` so the scan can be parked in a static registry shared across rayon workers in Phase 5. `&'static str` for `id` because the scan-id strings are compile-time constants. `param_schema` returns `serde_json::Value` (not a typed schemars derive) because each scan's param shape is different; the Plan can switch to per-scan typed structs deriving `JsonSchema` if it ends up cleaner. `ScanFindingShape` is a tiny declarative struct (`{ effect_extra_keys: &[&str], raw_series_keys: &[&str] }`) consumed by `miner scans` introspection and by the per-scan integration tests.

- **D3-15 (discretion): `ScanCtx` brokering object.** The facade constructs `ScanCtx { cache: &BarCache, gap_detector: &GapDetector, run_id: RunId, code_revision: &str }` and passes it to `Scan::run`. The scan calls `ctx.bars(symbol, side, timeframe, range) -> BarFrame` and `ctx.gap_manifest(symbol, side, range) -> GapManifest` — never touches `Reader` directly. Future look-ahead-safety enforcement (`ctx.bars_up_to(ts)`) goes here when Phase 4's rolling stats need it. Keeps every Scan impl from re-plumbing the data layer.

- **D3-16 (discretion): Static `Registry` constructed by a `bootstrap()` function in `miner-core`.** Pattern:
  ```rust
  pub fn bootstrap() -> Registry {
      let mut r = Registry::new();
      r.register(Box::new(LjungBoxScan));
      // Phase 4 plans extend this with one line per scan.
      r
  }
  ```
  Avoids the `inventory` crate's compile-time-magic that the Rust folks here would have to reason about. Iteration order is registration order, which is the order printed by `miner scans` (also lexicographically stable since `id` strings sort that way). Rejected: `inventory` (magic), `const SCANS: &[&dyn Scan]` (no way to compose across crates if a Phase-7 reader-specific scan ever lands).

- **D3-17 (discretion): scan-id naming convention.** `<family>.<subfamily>.<scan_name>` where family ∈ {`stats`, `cross`, `seas`} mirroring REQUIREMENTS.md (ANOM/CROSS/SEAS). Examples: `stats.autocorr.ljung_box` (Phase 3), `stats.stationarity.adf` (Phase 4), `cross.corr.pearson_rolling` (Phase 4), `seas.bucket.day_of_week` (Phase 4). The `@version` suffix is an integer that bumps on any output-shape change for the same scan. Resolved scan-id-at-version is what `param_hash` is computed alongside and what `miner scans` lists.

### Facade + CLI Shape (Claude's Discretion — confirm in plan-phase research)

- **D3-18 (discretion): Single-shot multi-input scope at v1.** Phase 3's CLI accepts ONE instrument, ONE side, ONE timeframe, ONE window per invocation. `--instrument EURUSD` (not a comma-list). Phase 5's TOML sweep manifest is the only fanout entry point. Rationale: Phase 3's job is to lock the facade contract on the simplest possible request shape; fanout adds permutation-explosion concerns that don't belong here. Matches the singular phrasing in ROADMAP Phase 3 success criterion #1 verbatim (`--instrument ... --timeframe ... --window ...`).

- **D3-19 (discretion): Subcommand surface.**
  ```
  miner scan <scan_id@version> --instrument <SYM> --side <bid|ask> --timeframe <15m|1h|1d> \
      --window <ISO_FROM>:<ISO_TO> [--gap-policy <strict|continuous_only>] [--dry-run] [--params <KEY=VAL>...]
  miner scans   # introspection — emits one JSONL line per registered scan to stdout
  ```
  `--side` defaults to `bid` (the conservative FX default; many strategies trade bid). `--gap-policy` defaults to `continuous_only` (the policy that "does something" by default; `strict` is opt-in for high-correctness runs). `--params` is a repeatable `KEY=VAL` flag for typed scan parameters; values are parsed against the scan's `param_schema()` at the boundary; failure → `PreflightCode::InvalidParameter`.

- **D3-20 (discretion): `miner scans` output.** ONE JSONL line per registered scan, deterministic registration order. Shape:
  ```json
  {"scan_id":"stats.autocorr.ljung_box","version":1,"params":{...JSON Schema...},"finding_fields":{"effect_extra_keys":["lags","q_stats","p_values","acf"],"raw_series_keys":["returns","timestamps_ms"]}}
  ```
  Stays inside the stdout-JSONL discipline so MCP/HTTP wrappers can serve `list_scans` from the same data source byte-identical in Phase 6.

- **D3-21 (discretion): `--dry-run` shape.** Emits ONE new envelope variant `Finding::DryRun(DryRunFinding)` on stdout, then exits 0. Contents: `{kind: "dry_run", run_id, request, resolved_params, planned_data_slice, estimated_findings_count}`. `RunStart`/`RunEnd` framing still wraps it (so the dry-run output is structurally indistinguishable from a normal run except for the envelope kind). Adding the variant is an additive schema change (`Finding::DryRun` slots into the existing `#[serde(tag = "kind")]` enum without breaking existing consumers); plan-phase research must confirm the schema-sync CI gate accepts it without `schema_version` bump.

### SIGINT + Determinism (Claude's Discretion — confirm in plan-phase research)

- **D3-22 (discretion): SIGINT handling in the CLI wrapper only.** `miner-cli` registers a `ctrlc` handler that sets a `std::sync::atomic::AtomicBool` flag. The facade passes a `CancellationToken` (wrapping the flag via `Arc<AtomicBool>`) into `Scan::run` and into the rayon-fanout loop. Scans poll the token between findings (Ljung-Box is single-shot so this is trivially cheap); rayon workers exit cooperatively at the next yield point. `miner-core` itself has NO knowledge of `ctrlc` — the cancellation primitive is a plain `Arc<AtomicBool>` from `std`. Exit code 130 (128 + SIGINT) per POSIX convention so wrapping shells can distinguish "interrupted" from "errored".

- **D3-23 (discretion): Run-level determinism guarantees, run_id excepted.** Same `(scan_id@version, params, instrument, side, timeframe, window, gap_policy, source bars)` → byte-identical JSONL output modulo:
  - `run_id` (always-unique ULID per D-10 — this is correct behavior, not a bug).
  - `started_at_utc`, `produced_at_utc`, `ended_at_utc`, `wall_clock_ms` (clock reads in framing records only, never inside the scan kernel).
  - All other fields are deterministic. The Phase 3 integration test runs the demo scan twice and `insta`-snapshots the JSONL with `run_id` + timestamps redacted, asserting byte-equality on the redacted form.

- **D3-24 (discretion): Three-tier exit code routing in the facade.** Per D-07:
  - `0` — `RunEnd` emitted; at least one (a) result, (b) `gap_aborted`, or (c) dry-run finding was streamed (or zero findings if the scan computes none on the provided slice, which is a valid outcome).
  - `1` — pre-flight rejection (unknown scan, invalid param, missing config, etc.) or catastrophic failure (cache unreadable). Stdout empty; stderr has the structured `WireError` JSON line.
  - `2` — `RunEnd` emitted AND at least one mid-stream `Finding::ScanError` was emitted. Stream may have a mix of `Result` + `ScanError` findings.
  - `130` — SIGINT (overrides 0/2; even if results were emitted, the interrupted exit takes precedence so shells can distinguish).

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Project-level (always relevant)
- `.planning/PROJECT.md` — Scope, constraints, out-of-scope (no DSL/scripting, no chart patterns, no persistent results store). Phase 3 deliverable list under "Active" includes the CLI binary as a thin wrapper.
- `.planning/REQUIREMENTS.md` — OP-01, OP-05 (dry-run), OP-06 (SIGINT preservation), OP-07 (`miner scans` introspection), OP-08 (boundary validation + resolved-params echo), OUT-04 (actual consumed range + gap-manifest reference). Phase 3 owns all six.
- `.planning/ROADMAP.md` §"Phase 3" — Goal statement, depends-on (Phase 2), six enumerated success criteria. Especially: byte-identical re-run + shuffled-future regression test (criterion #6).
- `.planning/STATE.md` — Locked decisions list (sync+rayon core, stdout/findings discipline, schema-version lock, Arrow IPC cache, two-axis cache invalidation). Phase 3 introduces no new locked decisions at this level; it consumes them.

### Phase-level prior CONTEXTs
- `.planning/phases/01-foundations-contracts/01-CONTEXT.md` — D-01..D-24 envelope and infra contracts. Phase 3 specifically depends on: D-04 (input/output split in envelope), D-05 (mid-run errors are findings, sweep continues), D-06 (pre-flight → stderr+exit 1), D-07 (three-tier exit codes — Phase 3 extends to four with SIGINT 130), D-08 (gap-policy outputs are findings), D-09/D-10/D-11 (always-emit `run_start`/`run_end` with ULID + rich summary), D-15 (clippy disallowed-macros lint), D-19 (single sanctioned stdout writer is `FindingSink`).
- `.planning/phases/01-foundations-contracts/01-RESEARCH.md` — `error_code` vocabulary section (Phase 3 implements the dispatch logic that uses this vocabulary).
- `.planning/phases/02-reader-aggregator-derived-bar-cache/02-CONTEXT.md` — D2-01..D2-21 reader/aggregator/cache/gap contracts. Phase 3 consumes: D2-08 (`Calendar` API — `is_open_at` predicate), D2-12 (`Reader` trait), D2-14 (`BarFrame` column shape — the scan kernel input), D2-16/D2-17 (`GapDetector` + `GapManifest` — Phase 3 wraps these into `Finding::GapAborted` and `data_slice.gap_manifest`), D2-19 (bar boundary convention — affects `data_slice.range` arithmetic).
- `.planning/phases/02-reader-aggregator-derived-bar-cache/02-VERIFICATION.md` — Confirms Phase 2's deliverables passed all 5 success criteria including full-determinism integration; Phase 3 can rely on this surface unchanged.

### Project-level docs the user has not directly referenced but downstream agents need
- `./CLAUDE.md` — Stack constraints (Rust 1.85 / edition 2024). §"Technology Stack" lists `clap` v4, `tracing` + `tracing-subscriber`, `rayon`, `thiserror` + `anyhow`. Phase 6 (MCP/HTTP) is where `axum` and `rmcp` enter; Phase 3 stays inside the existing stack.
- `./README.md` — Phase 1 Quickstart section; Phase 3 plan-phase MUST extend it with the `miner scan` + `miner scans` invocation examples (this is the user-facing proof of OP-01 + OP-07).

### Live artifacts to be extended (NOT replaced) in Phase 3
- `./schemas/findings-v1.schema.json` — The locked envelope schema. Phase 3 introduces TWO additive changes:
  1. `DataSlice.gap_manifest: Option<GapManifest>` (D3-10) — additive optional field on an existing type.
  2. `Finding::DryRun(DryRunFinding)` variant (D3-21) — additive enum variant.
  Both MUST be schema-additive (no `schema_version` bump). Plan-phase research must confirm schemars' JSON Schema output for both changes is `oneOf`-additive (existing consumers parse old findings unchanged).
- `./crates/miner-core/src/findings/mod.rs` — Five-variant `Finding` enum becomes six (adding `DryRun`). `DataSlice` gains the optional `gap_manifest` field.
- `./crates/miner-core/src/error/codes.rs` — `PreflightCode` + `ScanErrorCode` vocabularies are already Phase 3-aware (the locked enums include `UnknownScan`, `InvalidParameter`, `MissingRequiredConfig`, `InvalidConfig`, `CoverageGap`, `ComputeError`). Phase 3 wires them into the facade without adding new variants.
- `./crates/miner-core/src/cache.rs` + `./crates/miner-core/src/aggregator.rs` + `./crates/miner-core/src/gap.rs` — Consumed verbatim by `ScanCtx` (D3-15). No changes.
- `./crates/miner-cli/src/cli.rs` + `./crates/miner-cli/src/main.rs` — Extended with the `scan` + `scans` subcommands and the facade-call plumbing. The existing `emit-fixture` subcommand stays as a trivial smoke test.

### External references (read during plan-phase, not bundled in repo)
- `statsmodels.stats.diagnostic.acorr_ljungbox` — Reference implementation for D3-04 / D3-05 golden bytes. Plan-phase research must pin the exact statsmodels version used to generate the golden so the test is reproducible against a specific upstream.
- `ulid` crate `docs.rs/ulid` — Already a Phase 1 dep; Phase 3 reuses for run-id generation.
- `ctrlc` crate `docs.rs/ctrlc` — Plan-phase introduces this in `miner-cli` only (D3-22).

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets from Phase 1 + Phase 2 (every one used by Phase 3)
- **`miner_core::CODE_REVISION`** — git SHA injected by `build.rs`. Phase 3's facade attaches it to every emitted `RunStart` and to every `Result`/`ScanError`/`GapAborted` finding's `code_revision` field.
- **`miner_core::findings::{Finding, ResultFinding, ScanErrorFinding, GapAbortedFinding, RunStart, RunEnd, RunSummary, PerScanCounts, RunId, Source, DataSlice, TimeRange, Effect, Raw, RawArray, Base64Bytes, Dtype}`** — The full envelope vocabulary. Phase 3 constructs each variant inside the facade; does not redefine any.
- **`miner_core::findings::FindingSink` + `StdoutSink` + `FileSink`** — D-19 single-sanctioned-writer pattern. Facade emits findings through `&mut dyn FindingSink`; the CLI constructs the concrete sink via `make_sink(&cfg.output)` (already in `miner-cli/src/main.rs:87`).
- **`miner_core::error::{MinerError, PreflightCode, ScanErrorCode, WireError}` + `error::stderr_emit::emit_to_stderr`** — Pre-flight rejection path. Facade builds `WireError::preflight(code, message)`; CLI's `main()` writes it to stderr and exits 1.
- **`miner_core::config::{MinerConfig, OutputDest, CliOverrides, build_figment}`** — Config precedence is already wired (CLI > env > TOML); Phase 3 does NOT modify `MinerConfig`. The facade accepts a resolved `MinerConfig` reference and reads `cache_root` / `bar_cache_root` from it.
- **`miner_core::reader::{Reader, Side, ClosedRangeUtc, RawBar, Blake3Hex}`** — `Reader` trait + `Side` enum directly drive the CLI's `--side` flag values. `ClosedRangeUtc` is what the facade hands to `BarCache::get_or_build` after parsing `--window`.
- **`miner_core::aggregator::{aggregate, AggParams, BarFrame, Timeframe, AGGREGATOR_VERSION}`** — `BarFrame` is what every Scan reads (via `ScanCtx::bars`). `Timeframe::{Tf15m, Tf1h, Tf1d}` enum drives the `--timeframe` flag.
- **`miner_core::cache::{BarCache, ARROW_SCHEMA_VERSION, build_arrow_schema}`** — The data-layer entry point Phase 3's facade calls.
- **`miner_core::gap::{GapDetector, GapManifest, GapSpan, GapReason}`** — Phase 3 wraps these. `GapManifest` Serializes via the existing schemars `JsonSchema` derive — no envelope adapter needed.
- **`miner_core::calendar::Calendar`** — Used by `GapDetector` internally; Phase 3 does not call directly.
- **`clippy.toml` workspace lint config** — Bans `println!`/`eprintln!`/`dbg!`. Phase 3's facade + scan modules MUST flow tracing through `tracing::*` and findings through `FindingSink::write_envelope`.
- **`miner_cli::cli::{Cli, Command, resolve_toml_path}`** — Existing clap parser; Phase 3 extends the `Command` enum with `Scan(ScanArgs)` and `Scans` variants without touching the global-flag surface.

### Established Patterns (carry forward unchanged)
- **One-way dependency direction**: `miner-cli|mcp|http` → `miner-reader-dukascopy` → `miner-core`. Phase 3's facade lives in `miner-core::engine` (or `miner-core::facade`); the CLI wires `DukascopyReader` into the facade at the binary edge. `miner-core` learns nothing new about Dukascopy.
- **Locked envelope mutation discipline**: `findings-v1.schema.json` is regenerated by `xtask` from Rust types; CI fails the build if checked-in artifact diverges. Phase 3's two additive changes (DataSlice field, DryRun variant) flow through this gate.
- **Determinism via `BTreeMap` everywhere**: any new map types (e.g., the `Registry::scans: BTreeMap<(String, u32), Box<dyn Scan>>`) MUST be `BTreeMap`, never `HashMap`.
- **`Finding`/`WireError` wire-form invariants**: `dsr` and `fdr_q` ALWAYS serialise as JSON `null` in v1 (no `skip_serializing_if`). The new `DataSlice.gap_manifest` field MUST follow the same rule (`null` when absent, not omitted).
- **Test-fixture discipline**: synthetic deterministic fixtures in `crates/<crate>/tests/fixtures/`. Phase 3 ships one Ljung-Box golden fixture + the shuffled-future regression input.

### New Phase 3 Modules (planning input — Plan can revise)
- `crates/miner-core/src/scan/mod.rs` — `Scan` trait, `ScanCtx`, `ScanRequest`, `ScanError`, `ScanFindingShape`.
- `crates/miner-core/src/scan/registry.rs` — `Registry::new()`, `register()`, `get()`, `iter()`. `bootstrap()` helper that wires in the demo scan.
- `crates/miner-core/src/scan/ljung_box.rs` — `LjungBoxScan: Scan` impl.
- `crates/miner-core/src/engine.rs` (or `crates/miner-core/src/facade.rs`) — The single facade entry point: `run(req: ScanRequest, cfg: &MinerConfig, reader: &dyn Reader, sink: &mut dyn FindingSink) -> Result<RunOutcome, FacadeError>`. Owns RunStart/RunEnd framing, gap-policy dispatch, error classification, exit-code routing logic (the exit code itself is the CLI binary's responsibility — the facade returns a `RunOutcome` enum the CLI maps to an exit code).
- `crates/miner-core/src/findings/mod.rs` — Extend with `DryRun(DryRunFinding)` enum variant; extend `DataSlice` with optional `gap_manifest`.
- `crates/miner-cli/src/cli.rs` — Add `Command::Scan(ScanArgs)`, `Command::Scans`. `ScanArgs` carries `--instrument` / `--side` / `--timeframe` / `--window` / `--gap-policy` / `--dry-run` / `--params`.
- `crates/miner-cli/src/main.rs` — Wire SIGINT handler (`ctrlc`), construct `DukascopyReader`, call into the facade.
- `crates/miner-core/tests/scan_ljung_box.rs` + `tests/shuffled_future_regression.rs` + `tests/scan_facade_determinism.rs` — Phase 3 integration tests.

### Dependency Direction (one-way, enforced — unchanged from Phase 2)
```
miner-cli  ──┐
miner-mcp  ──┼──→  miner-reader-dukascopy  ──→  miner-core
miner-http ──┘                                       │
                                                     └── (scan, engine, etc.)
```
Phase 3 introduces NO new edges. Reader stays Dukascopy-specific; `miner-core` knows nothing of Dukascopy.

### New Workspace Dependencies (to be added in Phase 3)
| Crate | Where | Reason |
|-------|-------|--------|
| `ctrlc = "3"` | `miner-cli` only | D3-22 SIGINT handler. Pure-`std` underneath; tiny dep. |
| `serde_json` `RawValue` opt-in feature | `miner-core` | Possible plan-phase optimization for `RunStart.request` if profiling shows the field is hot. Default: skip. |
| `jsonschema = "0.x"` (already a Phase 1 dev-dep) | dev-deps only | Possibly extend to validate `Scan::param_schema()` output against the Phase 3 contract. Plan-phase decides. |

All within CLAUDE.md's recommended stack at HIGH confidence; no surprises.

</code_context>

<specifics>
## Specific Ideas

User decisions in this discussion:
1. **Phase 3's demo scan is Ljung-Box on returns (ANOM-04).** Pinned over summary-stats (insufficient envelope coverage), bar-count baseline (no statistical content), and rolling volatility (spec variants still open). Drives the Phase 3 golden fixture against statsmodels.
2. **Log returns inline inside the scan, no pull-forward of ANOM-01.** Each Phase 4 scan reinvents its own returns until ANOM-01 lands; one Phase-4 refactor pass cleans it up.
3. **Effect.extra carries per-lag stats + ACF.** Matches statsmodels output one-for-one — the canonical Phase 3 envelope-coverage demonstration.
4. **Box-Jenkins default `lags = min(10, n / 5)`, user-overridable.** Same default statsmodels uses → goldens validate without divergence.

Recurring user themes (carried forward from prior phases):
- **The Quant agent is THE consumer.** Phase 3's envelope additions (`DataSlice.gap_manifest`, `Finding::DryRun`) must be self-describing so the agent decodes without per-scan knowledge.
- **Agent-operability across CLI / MCP / HTTP is non-negotiable.** Phase 3's facade is the contract Phase 6 will validate parity against — bytes the CLI emits MUST be byte-identical to what MCP/HTTP will emit in Phase 6.
- **Determinism is a hard property.** Same inputs → same JSONL bytes (modulo run_id + timestamps); the shuffled-future regression pins the look-ahead-safety invariant for the demo scan and every Phase 4 scan inheriting that test.

</specifics>

<deferred>
## Deferred Ideas

Items that came up during discussion or that the user explicitly delegated:

- **ANOM-01 returns primitive** — Phase 4. Phase 3's Ljung-Box scan inlines its own log returns (D3-02). When Phase 4 lands the proper primitive, Ljung-Box refactors.
- **Multi-input fanout on the CLI** — Phase 5's TOML sweep manifest is the only fanout entry. Phase 3 stays single-shot per invocation (D3-18).
- **Bootstrap CIs, phase-scramble nulls, BH-FDR, Deflated Sharpe Ratio** — Phase 5 (HYG-*). Phase 3's `Finding::Result.effect.ci95` and `dsr` / `fdr_q` stay `null`.
- **Content-addressed deduplication of gap manifests across findings** — Phase 7 hardening. v1 inlines the full manifest in each `data_slice.gap_manifest` (D3-10) for simplicity; `gap_manifest_ref` field is reserved for the future dedup path.
- **Look-ahead-safety enforcement via `ScanCtx::bars_up_to(ts)` API** — Surface bare-minimum in Phase 3 (Ljung-Box doesn't need it); fully populate when Phase 4's rolling stats arrive.
- **`Finding::DryRun` rich planning output (e.g., per-instrument bar counts)** — Phase 3 ships the minimum (`{request, resolved_params, planned_data_slice, estimated_findings_count}`); Phase 5's sweep dry-run extends with per-job estimates.
- **`miner scans --schema-version 1`-style filtering** — Defer until a v2 schema exists.
- **`miner scan ... --output-format=cbor`** — Defer; JSONL is the v1 contract.
- **`inventory`-crate registration pattern** — Rejected in D3-16 in favour of explicit `bootstrap()`. Could be revisited if the registry ever splits across crates.
- **`simd-json` for output encoding** — Profile first (CLAUDE.md guidance). Phase 3 uses `serde_json`; flip if Phase 7 benches show JSON encoding hot.

</deferred>

<open_questions>
## Open Questions for Research / Plan-Phase

Plan-phase research must confirm or override every "Claude's Discretion" decision above. The blocking-for-the-plan items are:

1. **Schema-additive guarantee for `DataSlice.gap_manifest` (D3-10) and `Finding::DryRun` (D3-21).** Confirm via the schemars 1.x JSON Schema output that adding an optional struct field and an extra `oneOf` enum variant does NOT bump `schema_version`. Plan-phase regenerates `findings-v1.schema.json` and inspects the diff before committing to the API.
2. **statsmodels golden tolerance for Ljung-Box (D3-05).** Pin the exact statsmodels version used to generate the golden; document tolerances per element (Q-stat, p-value, ACF). Plan-phase decides whether the test compares JSONL bytes (post-canonicalisation) or unpacks the base64 arrays and compares as floats with tolerance.
3. **`Scan::param_schema()` machinery (D3-14).** Either each scan derives a typed `Params` struct via `#[derive(JsonSchema)]` and the trait method returns the generated schema, OR every scan hand-rolls a schema-literal. Plan-phase picks based on schemars 1.x ergonomics in the context of an `&'static Scan` registry.
4. **`ScanCtx::bars(...)` lifetime + borrowing model (D3-15).** The `BarCache::get_or_build` API returns `BarFrame` by value (verify against Phase 2 source). If the cache evolves to return borrowed frames, `ScanCtx` needs revisiting; Phase 3 should commit to the simplest passing shape and let Phase 4 generalise.
5. **`ctrlc` interaction with rayon's thread pool (D3-22).** `ctrlc` installs a signal handler on the main thread; rayon workers must cooperatively poll. Plan-phase should write a tiny spike confirming the cancellation token reaches a rayon worker within ~ms and that no findings are dropped (sink flush ordering vs cancellation).
6. **`Dukascopy` reader path resolution.** Phase 3's CLI must construct a `DukascopyReader` from `MinerConfig.cache_root`. The reader constructor's exact signature (`DukascopyReader::new(cache_root: &Path) -> Result<Self, DukascopyError>`?) is fixed by Phase 2 — plan-phase confirms against `crates/miner-reader-dukascopy/src/lib.rs`.
7. **`miner-cli` test discipline for the SIGINT path.** `cargo test` can't deliver a real SIGINT easily — plan-phase must decide between (a) unit-testing the facade's `CancellationToken` polling separately from the OS signal path, or (b) a small `assert_cmd` integration test that sends SIGINT to the subprocess.

</open_questions>

---

*Phase 3 context complete. 24 decisions captured (4 user-locked + 20 Claude's-discretion within the user-locked framework). All 6 Phase 3 requirements (OP-01, OP-05, OP-06, OP-07, OP-08, OUT-04) mapped to specific modules and behaviours. Ready for `/gsd-plan-phase 3`.*
</content>
</invoke>