# Project Research Summary

**Project:** tradedesk-miner
**Domain:** High-performance Rust quantitative data-mining engine (statistical OHLCV scans, agent-operated)
**Researched:** 2026-05-15
**Confidence:** MEDIUM-HIGH (statistical methodology HIGH; Rust architecture HIGH; specific crate versions MEDIUM — verify before committing Cargo.toml)

---

## Executive Summary

`tradedesk-miner` is the discovery layer in the RadiusRed quant pipeline: it reads a zstd-CSV Dukascopy OHLCV cache, fans out embarrassingly-parallel statistical scans over that data, and streams findings as JSONL to whichever surface requested them (CLI, MCP, HTTP). The whole project is a single Rust workspace — a library crate (`miner-core`) that owns all behaviour, a Dukascopy reader crate, and three thin wrapper binaries that translate protocols without duplicating logic. The architectural centrepiece is the `facade::api` — a single entry point (`run_scan / list_scans / probe`) through which every wrapper must pass. This pattern is well-established in the Rust ecosystem and gives strong guarantees about wrapper parity at low maintenance cost.

The recommended build approach is strictly layer-by-layer: foundational types first, then the reader trait and aggregator (the most correctness-critical code), then the findings envelope (lock its schema before any scan is written), then the scan engine with one working scan end-to-end, then the CLI wrapper to validate the facade shape, then the remaining scan catalogue, then MCP and HTTP wrappers in parallel. The aggregated-bar derived cache (Arrow IPC or bincode+zstd, keyed by source+symbol+timeframe+side) is a first-class Phase 1 deliverable — without it, every parameter-grid sweep re-pays zstd decode costs that dominate wall-clock time.

The project carries six CRITICAL pitfalls that must be wired into the correct phase from day one and cannot be safely retrofitted: (1) look-ahead bias in window primitives, (2) stdout/findings discipline (logs polluting the JSONL stream breaks every agent consumer), (3) async contamination of CPU-bound scan code, (4) findings envelope schema stability, (5) naive bar aggregation edge cases (DST, weekend gaps, partial bars), and (6) multiple-testing/data-snooping accounting in sweep mode. All six have specific phase assignments below. Missing any one makes the miner's findings either silently wrong or unusable by agents.

---

## Key Findings

### Recommended Stack

The stack is well-settled with high consensus across all three research files. The core is `ndarray` + `ndarray-stats` for numerical kernels (not Polars — too heavy for column-of-floats rolling math), `rayon` for CPU-bound parallel fanout over instrument×timeframe×parameter grids, `tokio` only at the wrapper edges (HTTP, MCP), and `crossbeam-channel` to bridge rayon scan output back to async transport. CSV parsing is the `csv` crate with `zstd-rs` streaming decode; findings serialization is `serde_json` JSONL. The derived-bar cache is Arrow IPC files via `arrow-rs` (columnar, mmap-friendly, cross-language portable for future Python interop). Error handling follows the standard Rust split: `thiserror` typed errors in the library, `anyhow` in binary entry points.

The one genuinely contested dependency is the MCP server SDK (`rmcp` from `modelcontextprotocol/rust-sdk`). It is the org-blessed choice but version stability and streaming-tool-result support must be verified before committing. The fallback is a hand-rolled JSON-RPC-over-stdio implementation (~500 LOC against `serde_json` alone). This risk does not affect the architecture — wrapper crates are isolated — but it must be resolved at the start of Phase 4.

**Core technologies:**

- `Rust 1.85+ / edition 2024` — language; edition stabilises async-in-trait ergonomics even though core is sync
- `rayon 1.10+` — CPU-parallel fanout over (instrument x timeframe x params); work-stealing handles uneven task duration
- `ndarray 0.16+ / ndarray-stats 0.6+` — rolling kernels and statistical reductions over 1-D OHLCV columns
- `nalgebra 0.33+` — small fixed-size linear algebra (OLS, covariance inversion); stack-allocated, no heap pressure in parallel workers
- `statrs 0.17+` — distributions and p-value functions (t, F, chi-sq, normal CDFs/PDFs); covers ADF, Ljung-Box, ARCH-LM
- `csv 1.3+ / zstd 0.13+` — streaming zstd decode piped into CSV parser via 1 MiB BufReader
- `arrow-rs (arrow + arrow-ipc) 53+` — aggregated-bar cache format; columnar, mmap-readable, Python-portable
- `memmap2 0.9+` — zero-copy reads of derived-bar cache files
- `serde 1.0+ / serde_json 1.0+` — findings serialization; JSONL via to_writer + newline
- `tokio 1.40+` — async runtime for HTTP and MCP wrappers only; never enters miner-core
- `axum 0.7+` — HTTP server with SSE/NDJSON streaming
- `rmcp 0.x` — MCP SDK (VERIFY before use; hand-rolled JSON-RPC is the fallback)
- `clap 4.5+` — CLI parsing with derive macros
- `tracing 0.1.40+` — structured logging to stderr; spans model scan/instrument/timeframe nesting
- `thiserror 1.0+ / anyhow 1.0+` — typed library errors + binary error glue
- `blake3 1+` — cache key hashing for (source, symbol, timeframe, side, aggregator_version)
- `criterion 0.5+` — microbenchmarks per kernel; supplemented by a bespoke miner-bench binary for end-to-end sweep timing
- `proptest 1+ / insta 1+` — aggregation invariant property tests and findings schema snapshot tests

**Versions to verify before committing Cargo.toml:** `rmcp` (crate name, version, transport capabilities), `jiff` vs `chrono` stability, `arrow-rs` major (was 53; likely 54+ by ship), `ndarray-linalg` BLAS backend selection.

### Expected Features

Research confirms a large but well-structured v1 scope. The dependency graph from FEATURES.md is clear: everything flows from the aggregator, so it must be correct before any scan is written. All three research documents agree on scan categories; there are no scope conflicts between FEATURES.md and PROJECT.md.

**Must have (v1 table stakes):**

- Reader trait + Dukascopy zstd-CSV reader — without this, nothing works
- Deterministic 1m to {15m, 1h, 1d} bar aggregator (close-aligned, UTC, gap-honest, bid/ask independent) — most correctness-critical code in the project
- Aggregated-bar disk cache keyed by source+symbol+resolution+side — performance linchpin; all sweeps depend on it
- Versioned findings envelope (JSONL) with effect stats + raw arrays + reproducibility metadata — the agent contract; schema locked before any scan ships
- Scan registry (named + versioned scans, parameter validation, parameter_schema() per scan)
- Parallel execution over (instrument x timeframe x window x param) grids via rayon
- JSONL findings stream on stdout; structured logs on stderr (strict separation, enforced in CI)
- Single-scan query API — CLI, MCP, HTTP wrappers over the same facade
- Sweep-manifest runner (TOML format, dry-run preview, cancellation on SIGINT)
- Statistical-anomaly scans: return primitives, summary stats, rolling vol + vol-of-vol, Ljung-Box, ADF, KPSS, Variance Ratio, ARCH-LM, Jarque-Bera, outlier detection, drawdown profile
- Cross-instrument scans: time-alignment primitive, rolling Pearson + Spearman, rolling beta/OLS, lead-lag CCF, Engle-Granger cointegration + OU half-life, basket divergence (equal-weighted v1)
- Seasonality scans: hour-of-day, day-of-week, session bucketing (Asia/London/NY/overlap), end-of-month, ANOVA/Kruskal-Wallis, event-window placeholder
- Statistical hygiene: effect size alongside every p-value, Benjamini-Hochberg FDR at sweep level, block/stationary bootstrap CIs, phase-scrambled null distributions

**Should have (v1.x — after v1 is in agent's hands):**

- Johansen cointegration for baskets (n >= 3)
- Granger causality test
- Hurst exponent / DFA
- Correlation-breakdown detection (rolling corr + PELT/CUSUM change-point)
- Basket divergence with PCA/equal-vol weights
- PELT/CUSUM change-point on returns and vol series
- Memoised per-sweep intermediates (in-memory arena)
- Side-channel raw-array storage (URI reference vs inline)
- Schema-introspection endpoint (miner scans / list_scans)

**Defer to v2+:**

- Markov-switching variance/mean (Hamilton MS-AR)
- GARCH/EGARCH fitting as a scan
- PyO3 Python bindings
- Tick-level scans
- Wavelet/spectral seasonality decomposition

**Confirmed anti-features (explicitly out of scope):** chart/candlestick pattern matching, strategy generation, backtesting, live market data, persistent findings store, GUI/TUI, source-data ingestion.

**V1 scan-to-architecture cross-check:** All v1 scans fit the PreparedScan::run(inputs: &ScanInputs, sink: &mut dyn FindingSink) interface without architectural concessions. Single-series scans iterate over inputs.bars[(symbol, side, timeframe)] internally. Cross-instrument scans receive the full HashMap<(Symbol, Side, Timeframe), BarSlice> and index into it directly — lead-lag CCF, rolling correlation, and Engle-Granger all need multi-symbol access simultaneously, which the architecture anticipates. The basket divergence equal-weighted v1 variant fits the cross-instrument scan shape as-is. No v1 scan requires a change to the trait surface.

### Architecture Approach

The workspace has one library crate (`miner-core`) and four binary/reader crates. Data flows strictly one way: `reader -> aggregator -> scans -> findings sink`. The three wrapper binaries share zero logic; they translate their respective protocols to and from `facade::api` calls. Findings JSON bytes are produced inside `miner-core` and copied verbatim through every wrapper — wrappers never construct JSON themselves, which enforces identical output semantics across CLI, MCP, and HTTP. The scan engine is pure-sync; rayon handles parallelism; tokio appears only inside wrapper crates and bridges to core via `spawn_blocking`.

**Major components:**

1. `miner-core::reader` — Reader trait (source_id, symbols, available_range, read_day); Send + Sync for shared use across rayon workers; sync-only
2. `miner-reader-dukascopy` — concrete Reader impl; encapsulates 00-indexed month quirk; isolates Dukascopy path layout from core
3. `miner-core::aggregator` — 1m -> target-timeframe resampler + derived-bar cache (Arrow IPC or bincode+zstd); alignment rule, gap/coverage policy, deterministic invalidation via aggregator_version + source fingerprint
4. `miner-core::findings` — Finding envelope, FindingSink trait, JSONL writer; the schema-contract boundary between miner and all consumers
5. `miner-core::scans` — Scan trait (prepare/run split), PreparedScan, ScanRegistry, built-in scan modules (stats/, cross/, seasonality/)
6. `miner-core::facade` — the ONLY entry point for wrappers: run_scan(request, sink), list_scans(), probe(symbol)
7. `miner-cli / miner-mcp / miner-http` — protocol translators only; clap, rmcp, axum; each approximately 200 LOC of adapter code

### Critical Pitfalls

1. **Look-ahead bias in window primitives** — timestamp convention must be locked in P1 (bar timestamp = interval START, bar is "known" at t+resolution); all scan window primitives enforce look-ahead discipline via a WindowView abstraction; add a shuffled-future regression test in P2 (shuffle bars after cutpoint, verify statistics before cutpoint are identical)

2. **Stdout/findings collision with logs** — `println!` and `print!` forbidden everywhere except the findings emitter (enforced via clippy::disallowed_macros in CI from P0); all tracing writes to stderr; MCP stdio transport breaks on any rogue stdout byte; verified by piping stdout through `jq -c .` for a fixed dataset in CI

3. **Async contamination of CPU-bound scan code** — miner-core must have zero async dependencies (`cargo tree -p miner-core | grep tokio` returns empty, checked in CI); wrappers bridge via spawn_blocking; enforced by workspace structure from day one

4. **Findings envelope schema breakage** — envelope schema locked before any scan ships (P0/P1); JSON Schema file checked into repo; CI validates emitted findings against it; schema_version on every finding; semver discipline for changes; snapshot test per release

5. **Naive 1m bar aggregation** — partial bars at session open, weekend/holiday gap concatenation, DST boundary handling, and bid/ask independence all have fixture tests before any scan code is written (P1); deterministic byte-identical re-runs verified

6. **Multiple-testing / data-snooping in sweeps** — findings envelope must carry tests_in_sweep, family_id, selection_rule, and adjusted_p_value fields from P0 even if null in v1; BH-FDR and DSR implementations land in P5; noise-replay sweep test (shuffled returns -> near-zero findings at target FDR) verifies correctness

---

## Implications for Roadmap

All three research files propose phase orderings that are compatible. The reconciled sequence below honours all stated constraints from ARCHITECTURE (11-step build order), FEATURES (MVP definition), and PITFALLS (phase-assignment table).

The decisive constraint from ARCHITECTURE: the facade is the lock-in point. Once a wrapper binary exists against the facade, changing the facade signature is a 3-wrapper refactor. The facade must be right before the CLI ships. This determines the ordering of everything before it.

The decisive constraint from PITFALLS: six CRITICAL pitfalls must be handled in P0/P1 before any scan code is written. They are not polish items; they are correctness foundations that cannot be retrofitted.

---

### Phase 0: Foundations and Contracts

**Rationale:** Everything downstream depends on the types, error model, findings envelope schema, logging convention, and workspace layout being correct and locked. Six CRITICAL pitfalls are partially or fully addressed here. This phase has no user-visible output but is the foundation every other phase builds on.

**Delivers:**
- Cargo workspace skeleton (miner-core, miner-reader-dukascopy, miner-cli, miner-mcp, miner-http, benches/)
- miner-core::types — Bar, Side, Symbol, Timeframe, TimeRange, BarSlice (f64, not generic Bar<T>)
- miner-core::error — MinerError enum (thiserror)
- miner-core::findings — Finding envelope with ALL required fields: schema_version, scan_id, source, params, effect (with effect_size), raw arrays, reproducibility, and sweep context fields (tests_in_sweep, family_id, selection_rule, adjusted_p_value) even if null in v1
- JSON Schema file for the findings envelope, checked in; CI validates emitted output against it
- Snapshot test skeleton (insta) for findings schema stability
- tracing + tracing-subscriber configured to write to stderr; clippy::disallowed_macros banning println!/print! except in the findings emitter — enforced in CI
- Workspace Cargo.toml dependency rules; CI check that `cargo tree -p miner-core` contains no tokio/async
- Config model: flag > env var > config file > clear error; zero hardcoded paths in the library

**Pitfalls addressed:** 4 (stdout discipline), 5 (async contamination — workspace layout), 6 (schema breakage — envelope locked), 2 (multiple-testing — schema fields reserved from day one), 11 (hardcoded paths)

**Stack decisions to lock in Phase 0:** Verify rmcp crate name/version/transport; choose jiff vs chrono; pin arrow-rs major; run `cargo add` for all direct deps and accept resolved versions.

---

### Phase 1: Reader, Aggregator, and Derived-Bar Cache

**Rationale:** The aggregator is the most correctness-critical component — every finding in v1 is built on it. It must be thoroughly tested before any scan code touches its output. The reader trait is the clean boundary for open-source pluggability. The derived-bar cache is a first-class deliverable, not an optimisation, because every sweep depends on it for acceptable wall-clock time.

**Delivers:**
- miner-core::reader — Reader trait, ReaderError; read_day returns Vec<Bar> (sync, no async); Send + Sync
- reader-mem test fixture (in-memory); enables all subsequent testing without disk I/O
- miner-reader-dukascopy — DukascopyZstdReader: zstd::Decoder -> BufReader -> csv::Reader -> Vec<Bar>; 00-indexed month quirk encapsulated; source_id() = "dukascopy-v1"; available_range() cached behind OnceCell; volume field named tick_count explicitly
- miner-core::catalog — symbol discovery and date-range introspection; instrument lifecycle and coverage gap reporting
- miner-core::aggregator — deterministic 1m -> {15m, 1h, 1d} resampler; alignment: timestamp = interval START; OHLC: open=first, high=max, low=min, close=last, volume=sum; gap policy: bar emitted only if >= min_coverage_pct (default 50%) of 1m bars present; bid/ask aggregated independently; no mid derivation
- Derived-bar cache: Arrow IPC files at <bar-cache-root>/<source_id>/<symbol>/<side>/<timeframe>/<YYYY-MM>.bars; invalidation via aggregator_version header + source fingerprint; keys via blake3
- Fixture edge-case tests: DST spring-forward, DST fall-back, weekend gap, holiday, instrument's first/last cache day, partial-bar session open, byte-identical re-run determinism test
- Time model: UTC internally; jiff::Timestamp or chrono::DateTime<Utc> for instants; NaiveDate for disk-file day keys; SessionModel enum per instrument class (Forex, Crypto, Index, Unknown)
- Small fixture cache (a few days, 2-3 instruments) checked into repo; CI runs without Dukascopy download

**Pitfalls addressed:** 1 (timestamp convention — interval START), 7 (naive aggregation — all edge cases tested), 9 (timezone/DST — UTC internally, session models), 3 (survivorship — lifecycle introspection), 14 (tick-volume naming), 15 (I/O pool vs compute pool), 16 (streaming reader)

**Open question (decide during Phase 1 planning):** Arrow IPC vs bincode+zstd for derived-bar cache. Arrow IPC wins if the derived cache files will ever be read by Python (Polars/Pandas); bincode+zstd is simpler and slightly faster for a strictly Rust-only path. Given PROJECT.md's explicit future goal of reusing the aggregator in `tradedesk` (Python), Arrow IPC is the recommended default.

---

### Phase 2: Scan Engine Core (One Scan End-to-End)

**Rationale:** ARCHITECTURE identifies the facade as the lock-in point. Before the facade can be locked, the scan engine trait must be stable. Before the scan trait is stable, at least one complete scan must exist to validate it — and at least one cross-instrument scan requirement should be prototyped to prove the ScanInputs shape handles multi-symbol access correctly. The CLI wrapper ships at the end of this phase to validate the facade before MCP/HTTP are built on top of it.

**Delivers:**
- miner-core::scans — Scan trait (id, family, parameter_schema, prepare -> PreparedScan); PreparedScan trait (requirements, run); ScanRegistry; ScanRequirements; ScanInputs (HashMap<(Symbol, Side, Timeframe), BarSlice>)
- miner-core::facade — Engine struct; run_scan(request, sink), list_scans(), probe(symbol); orchestration via aggregator.materialise_parallel(reqs) + rayon
- Statistical primitives: rolling WindowView enforcing look-ahead discipline (windows yield only fully-closed prior bars); Welford/Chan online variance; rolling covariance/correlation; return computation (log, simple, overnight split)
- Deterministic orchestration: sorted directory iteration, BTreeMap for ordered results, seeded RNG with seed in provenance, sorted findings emission order
- One complete built-in scan exercised end-to-end (e.g. stats.autocorr.ljung_box) plus a prototype of a cross-instrument scan requirement to validate ScanInputs handles multi-symbol input
- miner-cli — thin clap wrapper; subcommands: list-scans, list-symbols, probe, scan, sweep; output NDJSON to stdout; logs to stderr; no scan semantics in the wrapper
- Golden-file regression test: same scan twice -> byte-identical JSONL
- Look-ahead detector test: shuffle bars after cutpoint -> verify statistics before cutpoint are unchanged

**Pitfalls addressed:** 1 (window primitives + leakage test), 5 (async contamination — rayon-only in engine, spawn_blocking in wrappers), 10 (non-determinism — seeded RNG, sorted emission, golden-file CI), 4 (stdout discipline — CLI validates the model end-to-end)

**Design checkpoint:** Treat the facade API shape (particularly ScanInputs and PreparedScan::requirements) as a checkpoint before the CLI ships. The current ARCHITECTURE proposal looks correct, but validate it by prototyping a multi-symbol scan requirement before locking the facade.

---

### Phase 3: Scan Catalogue (All v1 Scans)

**Rationale:** With engine infrastructure proven, the scan catalogue can be built in parallel across contributors if needed. All scans follow the same trait shape; no architectural changes should be needed. This is the widest phase in breadth.

**Delivers:**

Statistical-anomaly scans (single-series):
- Summary stats (mean, std, skew, excess kurtosis, IQR, min, max) per window
- Rolling volatility + vol-of-vol
- Ljung-Box on returns and squared returns (Q-stat, lags, p-value, ACF array)
- ADF stationarity test (AIC-selected lag, trend specification, p-value)
- KPSS stationarity test
- Variance ratio test Lo-MacKinlay (VR(k) for multiple k, heteroskedasticity-robust z-stat)
- ARCH-LM test
- Jarque-Bera normality test
- Outlier/extreme-return detection (z-score, modified-z/MAD, neighbouring-bar context)
- Drawdown profile (max DD, duration, time-to-recover, DD distribution)

Cross-instrument scans:
- Time-alignment primitive (inner-join on common timestamps)
- Rolling Pearson + Spearman correlation
- Rolling beta/OLS (beta, alpha, R-squared, residual std per window)
- Lead-lag CCF over configurable lag grid (CCF array, argmax-lag, phase-scrambled null significance)
- Engle-Granger two-step cointegration (hedge ratio, residual ADF, p-value, OU half-life)
- Basket divergence — equal-weighted mean of N instruments, z-score time series, threshold crossings

Seasonality scans:
- Hour-of-day return/volatility profile (UTC default; TZ parameter for local bucketing)
- Day-of-week effect
- Trading-session bucketing (Asia/London/NY/overlap) with session-conditional stats
- End-of-month / start-of-month effect
- One-way ANOVA / Kruskal-Wallis bucket-comparison test
- Caller-supplied event-time window stats (news-window placeholder)

Statistical hygiene (applied across all scans):
- Effect size alongside every p-value (Cohen's d / Hedges' g / VR-minus-one as appropriate)
- Block/stationary bootstrap CIs callable from any scan (Politis-Romano stationary bootstrap)
- Phase-scrambled/circular-shift null distribution as first-class option per scan
- Bid/ask price convention declared per scan; average spread included in coverage output

Sweep-level hygiene:
- Benjamini-Hochberg FDR adjustment computed at end of sweep, emitted in sweep-summary record
- tests_in_sweep, family_id, selection_rule populated on every finding in sweep mode
- Sweep-summary record: total_findings, fdr_q_value per finding, top-N ranking by |effect_size| x -log10(q)

**Pitfalls addressed:** 2 (multiple-testing — FDR implementations), 3 (survivorship — universe construction utilities for cross-sectional scans), 8 (bid/ask per-scan conventions), 9 (seasonality TZ correctness), 13 (stationarity — rolling sub-window breakdown alongside full-window result)

**Implementation risk:** ADF, KPSS, Engle-Granger, block bootstrap, BH-FDR, and DSR will need to be hand-rolled and validated against scipy/statsmodels golden outputs — there is no comprehensive Rust stats crate covering these. Budget time for implementation + validation for each. This is likely the largest implementation risk in Phase 3.

---

### Phase 4: Wrapper Binaries (MCP and HTTP)

**Rationale:** With the facade locked and validated by the CLI (Phase 2), MCP and HTTP can be built in parallel. Both are purely protocol-translation. The main risk is rmcp SDK status — verify before starting this phase.

**Delivers:**
- miner-mcp — rmcp wrapper (or hand-rolled JSON-RPC-over-stdio if rmcp fails verification); each scan registry entry becomes a typed MCP tool with concrete parameter schema (not free-form params: object); list_scans, list_symbols, probe as meta-tools; stderr-only logging; structured errors with machine-parseable codes
- miner-http — axum wrapper; GET /v1/scans, GET /v1/symbols, POST /v1/scan (SSE or NDJSON, content-negotiated), POST /v1/sweep; Tower middleware for request size limits, timeouts, basic auth; hard upper limits on instruments/bars/grid size with structured rejection errors
- MCP tool descriptions written for LLM agents: purpose, allowed parameter values (enum where possible), one example call, one example response shape
- HTTP auth: bearer-token from day one; trust model documented
- Structured errors: { "error": "invalid_parameter", "parameter": "timeframe", "got": "1m", "allowed": ["15m","1h","1d"] }
- Integration tests: spawn binary, hit it, assert stdout/SSE bytes; jq -c . parse test on every line

**Pitfalls addressed:** 4 (MCP stdout discipline — separate crate, stderr-only logging), 5 (async contamination — spawn_blocking bridge), 12 (MCP tool design — per-scan typed tools, not free-form)

**Prerequisite:** Verify rmcp before Phase 4 planning: (a) crate name and version on crates.io, (b) stdio + streamable-HTTP transport, (c) streaming tool-result chunk support, (d) tokio version compatibility. If any point fails, commit to the hand-rolled JSON-RPC fallback — it does not affect the HTTP wrapper.

---

### Phase 5: Hardening, Benchmarks, and Reproducibility Audit

**Rationale:** This phase delivers the "lightning fast" proof and the statistical correctness guarantees that underpin the tool's value proposition. It also closes out the multiple-testing pitfall with DSR and a noise-replay regression test.

**Delivers:**
- Two-layer benchmark suite: criterion microbenches (decode 1 day, aggregate 1m->15m 1 year, rolling-corr N, OLS fit); miner-bench binary for end-to-end sweep timing (28 instruments x 3 timeframes x 6 years x N params); hyperfine wall-clock numbers for README
- Profiling pass: flamegraph / samply; allocator pressure audit; alloc < 5% of hot path
- Deflated Sharpe Ratio (DSR) implementation (Bailey & Lopez de Prado) for Sharpe-flavoured effect sizes in sweep mode
- Noise-replay sweep regression test: same sweep on shuffled-returns null dataset -> near-zero findings at configured FDR threshold
- Full reproducibility audit: every scan result has provenance.rng_seed; seeded default derived from inputs; verified by re-running each scan
- cargo-llvm-cov coverage gate on statistical kernels
- cargo-deny license + advisory checks in CI
- README: Dukascopy data-source caveats section (00-indexed months, tick-count volume, bid/ask independence, data licensing implications); quick-start works on fresh checkout with fixture cache
- cargo audit clean; dependency review completed

**Pitfalls addressed:** 2 (DSR/FDR + noise-replay test), 10 (golden-file CI), 15 (I/O vs compute pool tuning), 17 (hidden allocations), 18 (float precision verification), 19 (crate audit)

---

### Phase Ordering Rationale

- **Phase 0 before anything:** Six CRITICAL pitfalls have prevention actions in P0. Skipping P0 and retrofitting them later is the most common way greenfield quant projects incur expensive rewrites.
- **Phase 1 before Phase 2:** The aggregator is the correctness foundation. Writing scan code before the aggregator edge cases are proven means every scan has an unknown bug surface.
- **Phase 2 delivers the CLI before Phase 4:** The facade must be validated by one working wrapper before MCP and HTTP are committed to it. CLI is the smallest surface (no async runtime, no SDK risk) and the best validation vehicle.
- **Phases 3 and 4 can overlap once Phase 2's facade is locked:** Additional scans and wrapper binaries are independent once the scan trait and facade are stable.
- **Phase 5 last:** Hardening and the DSR implementation are improvements on a working system. The noise-replay test validates the sweep-mode statistical hygiene put in place in Phase 3.

### Research Flags

**Phases needing specific technical investigation before planning:**

- **Phase 1 — derived-bar cache format:** Arrow IPC vs bincode+zstd open question. Recommend Arrow IPC given PROJECT.md's explicit tradedesk aggregator reuse goal (Python consumer). Decide and document during Phase 1 planning.
- **Phase 2 — facade API shape:** The PreparedScan / ScanInputs design is a checkpoint. Prototype a multi-symbol scan requirement (e.g. lead-lag CCF needing two instruments simultaneously) against the proposed API before locking the facade. The current ARCHITECTURE proposal looks correct but has not been exercised.
- **Phase 4 — rmcp SDK:** Highest-risk dependency. Run `cargo add rmcp`, check crates.io publish date, read GitHub for transport support and streaming results before Phase 4 planning begins. If any doubt: commit to hand-rolled JSON-RPC fallback.
- **Phase 3 — hand-rolled econometric tests:** ADF, KPSS, Engle-Granger, block bootstrap, BH-FDR, DSR need implementation and scipy/statsmodels golden-output validation. Budget is unknown; needs scoping during Phase 3 planning.

**Phases with standard patterns (research-phase not needed):**

- **Phase 0:** Rust workspace setup, tracing/thiserror/anyhow, serde JSON schema — well-documented standard patterns.
- **Phase 2 (CLI wrapper):** clap derive, NDJSON stdout — abundant prior art.
- **Phase 4 (HTTP wrapper):** axum SSE streaming, Tower middleware — well-documented.

---

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| Stack | MEDIUM-HIGH | Core crates (rayon, ndarray, serde, axum, clap, zstd, csv) HIGH — mature, obvious picks. `rmcp` LOW without live verification. `jiff` MEDIUM (newer). Arrow IPC cache MEDIUM (correct reasoning, not externally validated). All versions need re-pinning with `cargo add` before Cargo.toml is committed. |
| Features | HIGH | Statistical methodology (scan names, parameters, test statistics) drawn from canonical econometrics literature — no controversy. Scan scope matches PROJECT.md exactly. Findings schema field inventory is well-motivated; specific schema shape is a reasoned proposal (MEDIUM). |
| Architecture | HIGH | Workspace layout, reader trait, aggregator pipeline, facade pattern, rayon-for-CPU / tokio-for-wrappers split — standard Rust patterns with well-understood trade-offs. FindingSink trait and PreparedScan split are strong design choices. BarSlice ownership in batch sweep defers to benchmarking. |
| Pitfalls | HIGH | Multiple-testing, look-ahead bias, survivorship, stdout discipline — drawn from canonical quant-finance references (Lopez de Prado, Bailey) and Rust ecosystem canon. MCP-specific pitfalls flagged MEDIUM without live SDK verification. |

**Overall confidence:** MEDIUM-HIGH. Architecture and methodology are on solid ground. Primary uncertainties are crate versions (resolved with `cargo add`) and rmcp SDK status (requires a live check before Phase 4).

### Gaps to Address

- **rmcp SDK status (LOW confidence):** Before Phase 4 planning, run `cargo add rmcp` and verify crate name, version, stdio + streamable-HTTP transport, streaming tool-result chunk support, tokio version compatibility. If any fails: commit to hand-rolled JSON-RPC fallback.
- **Arrow IPC vs bincode+zstd for derived-bar cache (MEDIUM confidence):** Decide during Phase 1 planning. Deciding criterion: will the derived-bar cache files ever be read by Python? If yes (likely given tradedesk reuse goal in PROJECT.md): Arrow IPC. If no: bincode+zstd.
- **Hand-rolled econometric test validation strategy (MEDIUM confidence):** ADF, KPSS, Engle-Granger, BH-FDR, DSR, block bootstrap need scipy/statsmodels validation. Effort estimate needs scoping in Phase 3 planning.
- **jiff vs chrono (MEDIUM confidence):** If jiff's API is stable and matches aggregator usage (instants, NaiveDate, IANA TZ lookup for seasonality bucketing), prefer it. If any doubt at Phase 0 kick-off: use chrono — battle-tested, well-known.
- **ndarray-linalg BLAS backend (MEDIUM confidence):** Required for covariance matrix inversion (Johansen, PCA-weight basket divergence). Use openblas-static for portability. Johansen is v1.x so this can be deferred to that phase.

---

## Sources

### Primary (HIGH confidence)

- PROJECT.md (in-repo) — authoritative scope, constraints, out-of-scope items, cache layout, consumer model
- Tsay, R. — Analysis of Financial Time Series (3rd ed.) — returns, volatility models, unit-root tests, cointegration
- Lo, A.W. & MacKinlay, A.C. — A Non-Random Walk Down Wall Street — variance ratio test methodology
- Hamilton, J. — Time Series Analysis — Johansen cointegration, Markov switching, ARCH/GARCH
- Lopez de Prado, M. — Advances in Financial Machine Learning — multiple-testing, bootstrap-on-time-series, FDR, look-ahead bias
- Bailey & Lopez de Prado — The Deflated Sharpe Ratio (J. of Portfolio Management, 2014)
- Politis & Romano — stationary bootstrap (JASA 1994)
- Killick et al. — PELT change-point algorithm (JASA 2012)
- Rust API guidelines — trait-object friendliness, Send + Sync for parallel use
- rayon docs — par_iter model for CPU-bound parallel workloads
- NDJSON / JSON Lines spec — one JSON object per line, newline-delimited

### Secondary (MEDIUM confidence)

- STACK.md, FEATURES.md, ARCHITECTURE.md, PITFALLS.md (in-repo research) — synthesized above; each carries its own confidence notes inline
- Standard Rust workspace patterns (ripgrep, tokio, cargo) — library + multiple-binary split
- MCP protocol spec — stdio transport / stdout-as-channel canonical behaviour
- Engle — ARCH-LM test; Andersen & Bollerslev — realised-volatility methodology
- Lakonishok & Smidt — calendar / end-of-month anomalies

### Tertiary (LOW confidence — verify before use)

- crates.io/crates/rmcp — version, publish date, transport support: must be verified live
- github.com/modelcontextprotocol/rust-sdk — SDK roadmap and streaming-results support: must be verified live
- github.com/BurntSushi/jiff — jiff stability and IANA TZ lookup API: verify before committing
- apache/arrow-rs current major version — was 53 at research time; verify with cargo add

---
*Research completed: 2026-05-15*
*Ready for roadmap: yes*
