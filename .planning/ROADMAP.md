# Roadmap: tradedesk-miner

## Overview

`tradedesk-miner` is built strictly layer-by-layer because every downstream phase consumes the correctness contracts established earlier. Phase 1 locks the workspace shape, the `Finding` envelope JSON schema, the stdout/findings vs stderr/logs discipline, and the config precedence model — none of these can be safely retrofitted. Phase 2 lands the reader, aggregator, derived-bar cache, and gap manifest, which together are the correctness foundation for every finding. Phase 3 brings the scan engine, facade, look-ahead-safe windowing, gap-policy enforcement, and the CLI wrapper that validates the facade before MCP/HTTP commit to it. Phase 4 rolls out the v1 scan catalogue (ANOM, CROSS, SEAS) against the locked facade. Phase 5 adds the statistical-hygiene layer (effect sizes, bootstrap, phase-scrambled nulls, BH-FDR) and the sweep-manifest runner. Phase 6 ships the MCP and HTTP wrappers in parallel — flagged for `rmcp` re-research at phase start. Phase 7 hardens the system with golden-file regression tests, the noise-replay sweep test, flamegraph profiling, the bench harness, and the README + data-source caveats.

## Phases

**Phase Numbering:**
- Integer phases (1, 2, 3): Planned milestone work
- Decimal phases (2.1, 2.2): Urgent insertions (marked with INSERTED)

Decimal phases appear between their surrounding integers in numeric order.

- [ ] **Phase 1: Foundations & Contracts** - Workspace, types, locked findings envelope, stdout/stderr discipline, config precedence
- [ ] **Phase 2: Reader, Aggregator & Derived-Bar Cache** - Dukascopy reader, deterministic aggregator, derived-bar cache, gap manifest
- [ ] **Phase 3: Scan Engine, Facade & CLI** - Scan trait/registry, facade, look-ahead-safe windowing, gap-policy enforcement, CLI wrapper
- [ ] **Phase 4: Scan Catalogue (ANOM, CROSS, SEAS)** - All v1 statistical, cross-instrument, and seasonality scans
- [ ] **Phase 5: Statistical Hygiene & Sweep Runner** - Effect sizes, bootstrap, phase-scramble nulls, BH-FDR, sweep manifest
- [ ] **Phase 6: MCP & HTTP Wrappers** - rmcp-based MCP server and axum-based HTTP server with parity to CLI
- [ ] **Phase 7: Hardening, Benchmarks & Reproducibility** - Golden-file tests, noise-replay, flamegraph, bench harness, README caveats

## Phase Details

### Phase 1: Foundations & Contracts
**Goal**: User can build a Rust workspace whose core library produces a locked `Finding` envelope schema, streams findings to stdout, routes logs to stderr (CI-enforced), and reads config via flag > env > file precedence with zero hardcoded paths.
**Depends on**: Nothing (first phase)
**Requirements**: FOUND-01, FOUND-02, FOUND-03, FOUND-04, FOUND-05, OUT-01, OUT-02, OUT-03
**Success Criteria** (what must be TRUE):
  1. User can `cargo build --workspace` and produce `miner-core` (library), `miner-reader-dukascopy`, `miner-cli`, `miner-mcp`, `miner-http`, and `miner-bench` binary/library crates with the correct one-way dependency direction enforced.
  2. User can emit a `Finding` envelope as JSONL on stdout and confirm it contains `schema_version`, `scan@version`, `param_hash`, `code_revision`, `data_slice`, plus reserved-but-null `dsr` and `fdr_q` fields, validated against a checked-in JSON Schema file.
  3. User can run `cargo clippy --workspace` in CI and have it reject any `println!` / `eprintln!` outside the single findings sink and the logging adapter (`clippy::disallowed_macros`).
  4. User can verify `cargo tree -p miner-core` produces zero `tokio` / `async` transitive dependencies (CI-enforced gate).
  5. User can override the cache root, derived-bar-cache root, and output destination via CLI flag, then env var, then config file, with a clear error message when none resolve and zero hardcoded paths in the library.
**Plans**: 7 plans

Plans:
- [ ] 01-01-PLAN.md — Workspace skeleton: virtual manifest, rust-toolchain, 6 member crates + xtask, build.rs for code_revision injection
- [ ] 01-02-PLAN.md — Wave 0 spikes: schemars 1.x base64-with-shape pattern + figment+clap CLI-wins precedence
- [ ] 01-03-PLAN.md — Finding envelope types (5 variants, RawArray, Base64Bytes, RunId, error_code enums), FindingSink trait, MinerConfig schema
- [ ] 01-04-PLAN.md — StdoutSink + stderr_emit writer modules + workspace clippy.toml disallowed-macros gate
- [ ] 01-05-PLAN.md — miner-core::config figment builder + miner-cli clap+tracing wiring + emit-fixture subcommand
- [ ] 01-06-PLAN.md — xtask gen-schema subcommand + committed schemas/findings-v1.schema.json + GitHub Actions CI with 4 mandatory gates
- [ ] 01-07-PLAN.md — Integration tests (schema_roundtrip, config_precedence, cli_streams) + README Quickstart + Phase 1 sign-off ritual
**UI hint**: No

### Phase 2: Reader, Aggregator & Derived-Bar Cache
**Goal**: User can read the existing tradedesk-dukascopy zstd-CSV cache through a pluggable `Reader` trait, materialise deterministic UTC-aligned higher-timeframe bars from the aggregator, serve them from a versioned derived-bar cache, and obtain a structured gap manifest before any scan runs.
**Depends on**: Phase 1
**Requirements**: CACHE-01, CACHE-02, CACHE-03, CACHE-04, CACHE-05, CACHE-06, CACHE-07, CACHE-08
**Success Criteria** (what must be TRUE):
  1. User can point the Dukascopy reader at an existing `<root>/<SYMBOL>/<YYYY>/<MM 00-indexed>/<DD>_<bid|ask>.csv.zst` cache and read 1-minute bars without any modification, re-download, or layout change, with the 00-indexed month quirk encapsulated and boundary-tested.
  2. User can request bars at 15m, 1h, or 1d resolution via the aggregator and receive deterministic, close-aligned, UTC OHLC bars that omit (never interpolate) gaps and process bid/ask sides independently, with `volume` exposed as `tick_count`.
  3. User can re-run any aggregation and have miner serve bars from the on-disk derived-bar cache keyed by `(source_id, symbol, side, timeframe)` with `aggregator_version` plus per-day source fingerprint invalidation.
  4. User can request a gap report for any `(symbol, side, range)` and receive a structured gap manifest enumerating `(start, end, reason)` tuples for missing daily files, zero-byte/corrupt files, and intra-day bar holes against the instrument's trading calendar.
  5. User can run aggregator edge-case fixture tests (DST spring-forward, DST fall-back, weekend gap, holiday, instrument first/last cache day, partial-bar session open) and observe byte-identical re-run determinism.
**Plans**: TBD
**UI hint**: No

### Phase 3: Scan Engine, Facade & CLI
**Goal**: User can register and invoke a versioned scan through a single facade, run one end-to-end scan via the CLI with look-ahead-safe windowing, and choose a `strict` or `continuous_only` gap policy that miner enforces before producing findings.
**Depends on**: Phase 2
**Requirements**: OP-01, OP-05, OP-06, OP-07, OP-08, OUT-04
**Success Criteria** (what must be TRUE):
  1. User can run `miner scan <name@version> --instrument ... --timeframe ... --window ...` from the CLI and receive findings as NDJSON on stdout for one fully-implemented end-to-end scan with resolved parameters (including defaults) echoed into every finding.
  2. User can introspect the scan catalogue via `miner scans` and receive each scan's name, version, parameter schema, and finding fields; unknown scan names and invalid parameters are rejected at the boundary with structured errors.
  3. User can choose `--gap-policy strict` and have miner abort the scan with a single error record containing the gap manifest, or `--gap-policy continuous_only` and have miner partition the window into maximal gap-free sub-ranges, emit one finding per sub-range, and attach the gap manifest — never silently producing findings on ranges containing holes.
  4. User can dry-run a planned scan invocation and see the resolved job (instruments, timeframes, windows, parameter expansion) and `data_slice` summary before committing.
  5. User can interrupt a long-running scan (SIGINT) and keep every finding already streamed to stdout, with a clean shutdown of the rayon worker pool.
  6. User can run the same scan twice on the same data and produce byte-identical JSONL output (sorted emission, BTreeMap, seeded RNG from inputs); the shuffled-future regression test passes (statistics before cutpoint unchanged when post-cutpoint bars are shuffled).
**Plans**: TBD
**UI hint**: No

### Phase 4: Scan Catalogue (ANOM, CROSS, SEAS)
**Goal**: User can invoke every v1 statistical-anomaly, cross-instrument, and seasonality scan through the facade, with each scan emitting a `Finding` envelope carrying its canonical effect statistic, sample size, raw arrays, and reproducibility metadata.
**Depends on**: Phase 3
**Requirements**: ANOM-01, ANOM-02, ANOM-03, ANOM-04, ANOM-05, ANOM-06, ANOM-07, ANOM-08, ANOM-09, ANOM-10, ANOM-11, CROSS-01, CROSS-02, CROSS-03, CROSS-04, CROSS-05, SEAS-01, SEAS-02, SEAS-03, SEAS-04, SEAS-05, SEAS-06
**Success Criteria** (what must be TRUE):
  1. User can run every single-series anomaly scan (return primitives, summary stats, rolling vol + vol-of-vol, Ljung-Box on returns and squared returns, ADF, KPSS, Lo-MacKinlay variance ratio, ARCH-LM, Jarque-Bera, outlier/extreme-return detection, drawdown profile) and receive findings whose statistics match scipy/statsmodels reference outputs within documented tolerances.
  2. User can run every cross-instrument scan (time-alignment primitive on common timestamps, rolling Pearson + Spearman, rolling OLS beta/alpha/R²/residual-std, lead-lag CCF with argmax-lag, Engle-Granger two-step cointegration with OU half-life) and receive findings carrying both legs' provenance and the joined `data_slice`.
  3. User can run every seasonality scan (hour-of-day, day-of-week, trading-session Asia/London/NY/overlap with configurable boundaries, end-of-month / start-of-month by trading-day-of-month, ANOVA / Kruskal-Wallis bucket-comparison, caller-supplied event-time windows) with FX-major session defaults and per-bucket mean/std/count/t-stat-vs-0/bootstrap CIs.
  4. User can invoke any scan through the same CLI facade and observe consistent `Finding` envelope shape across all 22 scans (single discriminant by `scan_id`, scan-specific extras in a documented `effect.extra` object).
  5. User can compare scan output against checked-in golden fixtures for at least one representative ANOM, one CROSS, and one SEAS scan and observe byte-identical JSONL.
**Plans**: TBD
**UI hint**: No

### Phase 5: Statistical Hygiene & Sweep Runner
**Goal**: User can submit a TOML sweep manifest, have miner fan it out in parallel, and receive findings carrying effect sizes alongside p-values, block-bootstrap CIs on autocorrelated series, phase-scrambled null distributions, a deterministic RNG seed, and a sweep-summary record with Benjamini-Hochberg FDR-adjusted q-values.
**Depends on**: Phase 4
**Requirements**: OP-04, HYG-01, HYG-02, HYG-03, HYG-04, HYG-05
**Success Criteria** (what must be TRUE):
  1. User can submit a TOML sweep manifest describing scans × instruments × timeframes × windows × parameter grids and have miner fan it out in parallel via rayon, emitting one finding per (scan × instrument(s) × timeframe × window × param-point).
  2. User can read every reported finding with a scan-appropriate effect size (Cohen's d / Hedges' g / Cliff's delta / VR-minus-one / etc.) alongside its p-value, and an end-of-sweep summary record containing Benjamini-Hochberg-adjusted q-values per finding family.
  3. User can opt into block / stationary (Politis-Romano) bootstrap confidence intervals on any scan that produces a statistic over autocorrelated series, and into phase-scrambled / circular-shift null distributions as a first-class option for any scan producing a p-value.
  4. User can reproduce any bootstrap or permutation result bit-for-bit by re-running the same scan with the seed echoed in the finding's reproducibility envelope.
  5. User can dry-run a sweep manifest with `--dry-run` and see the planned job graph plus the estimated job count before committing.
**Plans**: TBD
**UI hint**: No

### Phase 6: MCP & HTTP Wrappers
**Goal**: User can invoke every scan and meta-tool (`list_scans`, `list_symbols`, `probe`) through both the MCP server and the HTTP API with the same parameter shape and byte-identical findings JSON as the CLI.
**Depends on**: Phase 5
**Requirements**: OP-02, OP-03
**Success Criteria** (what must be TRUE):
  1. User can connect an MCP client over stdio to `miner-mcp` and invoke every scan as a typed MCP tool (one tool per scan, parameter schema derived from the registry) plus `list_scans` / `list_symbols` / `probe` meta-tools; the JSON-bytes-per-finding match CLI output exactly.
  2. User can POST to `/v1/scan` or `/v1/sweep` on `miner-http` and receive findings as content-negotiated NDJSON or SSE, with `/v1/scans` and `/v1/symbols` returning the same catalogue the CLI and MCP expose.
  3. User can verify both wrappers route logs to stderr exclusively (no stdout / response-body pollution outside the protocol payload) and rejected requests return structured machine-parseable errors (`invalid_parameter`, `coverage_gap`, `sweep_too_large`).
  4. User can confirm both wrappers bridge to `miner-core` via `tokio::task::spawn_blocking` and that `cargo tree -p miner-core` still shows zero async dependencies.
  5. User can interrupt an HTTP sweep (client disconnect) or MCP sweep (client cancellation) and miner keeps every finding already streamed without leaking worker threads.
**Plans**: TBD
**UI hint**: No

> **Research flag:** Before planning this phase, re-run `gsd-research` on the `rmcp` crate (verify crate name, version, stdio + streamable-HTTP transport, streaming tool-result chunk support, tokio version compatibility). If any check fails, commit to the hand-rolled JSON-RPC-over-stdio fallback before plan-phase begins.

### Phase 7: Hardening, Benchmarks & Reproducibility
**Goal**: User can run golden-file regression tests, the noise-replay sweep test, the bench harness, and `cargo audit` / `cargo deny` against a clean v1 with documented data-source caveats and a README quickstart that works on a fresh checkout.
**Depends on**: Phase 6
**Requirements**: (no new v1 REQ-IDs; closes verification debt for FOUND-02, FOUND-03, FOUND-04, CACHE-04, OUT-03, HYG-02, HYG-05)
**Success Criteria** (what must be TRUE):
  1. User can run the full golden-file regression suite (one representative scan per family, plus the locked findings-envelope snapshot) and observe byte-identical JSONL across runs.
  2. User can run the noise-replay sweep regression test (the same sweep on a shuffled-returns null dataset) and observe near-zero findings at the configured FDR threshold, proving multiple-testing controls work.
  3. User can run `miner-bench` and `hyperfine` recipes against the dev sample (28 instruments × 3 timeframes × 6 years) and obtain reproducible wall-clock numbers documented in the README; flamegraph / samply profiling shows allocations below 5% of hot path.
  4. User can clone the repo, follow the README quickstart against the checked-in fixture cache, and produce at least one finding without any external download or hardcoded path.
  5. User can read README sections covering Dukascopy data-source caveats (00-indexed months, tick-count volume, bid/ask independence, weekend gaps, data licensing implications) and `cargo audit` / `cargo deny` runs clean in CI.
**Plans**: TBD
**UI hint**: No

## Progress

**Execution Order:**
Phases execute in numeric order: 1 → 2 → 3 → 4 → 5 → 6 → 7

| Phase | Plans Complete | Status | Completed |
|-------|----------------|--------|-----------|
| 1. Foundations & Contracts | 0/7 | Planned | - |
| 2. Reader, Aggregator & Derived-Bar Cache | 0/TBD | Not started | - |
| 3. Scan Engine, Facade & CLI | 0/TBD | Not started | - |
| 4. Scan Catalogue (ANOM, CROSS, SEAS) | 0/TBD | Not started | - |
| 5. Statistical Hygiene & Sweep Runner | 0/TBD | Not started | - |
| 6. MCP & HTTP Wrappers | 0/TBD | Not started | - |
| 7. Hardening, Benchmarks & Reproducibility | 0/TBD | Not started | - |
