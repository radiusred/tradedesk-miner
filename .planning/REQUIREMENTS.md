# Requirements: tradedesk-miner

**Defined:** 2026-05-15
**Core Value:** Surface raw statistical candidates from years of OHLCV data fast enough that a quant agent can run wide sweeps and targeted queries interactively, without waiting on Python data pipelines.

## v1 Requirements

Requirements for initial release. Each maps to a roadmap phase.

### Foundation (FOUND)

- [ ] **FOUND-01**: User can build a Rust workspace with `miner-core` library crate and `miner-cli` / `miner-mcp` / `miner-http` thin wrapper binaries
- [ ] **FOUND-02**: User can verify findings stream cleanly on stdout while structured logs route to stderr (enforced by lint in CI)
- [ ] **FOUND-03**: User can rely on a single locked `Finding` envelope JSON schema with `schema_version`, `scan@version`, `param_hash`, `code_revision`, `data_slice` reproducibility fields, and reserved DSR / FDR-q fields (null in v1, present from day 1)
- [ ] **FOUND-04**: User can confirm the scan engine is pure sync + rayon; async is contained inside the HTTP/MCP wrappers via `spawn_blocking`
- [ ] **FOUND-05**: User can configure cache root, derived-bar-cache root, and output destination via CLI flag > env var > config file precedence with no hardcoded paths

### Cache & Aggregation (CACHE)

- [ ] **CACHE-01**: User can read the existing tradedesk-dukascopy zstd-CSV cache (`<root>/<SYMBOL>/<YYYY>/<MM 0-indexed>/<DD>_<bid|ask>.csv.zst`) without modification or re-download
- [ ] **CACHE-02**: User can register additional data sources via a `Reader` trait; the Dukascopy reader is the first implementation
- [ ] **CACHE-03**: User can request bars at caller-specified resolutions (15m / 1h / 1d as the working set); 1-min source is never the analysis grid
- [ ] **CACHE-04**: User can rely on deterministic, close-aligned, UTC bar aggregation that omits (never interpolates) gaps and processes bid/ask sides independently
- [ ] **CACHE-05**: User can trust that miner reports Dukascopy's `volume` field as tick count, not contract volume, and the 00-indexed month layout is documented + boundary-tested
- [ ] **CACHE-06**: User can re-run any aggregation and have miner serve from the semi-permanent on-disk derived-bar cache, keyed by `(source, symbol, side, timeframe)` with `aggregator_version` + per-day source fingerprint invalidation
- [ ] **CACHE-07**: User can rely on miner detecting cache gaps before any scan runs — missing daily files, zero-byte / corrupt files, and intra-day bar holes against the instrument's trading calendar — producing a structured **gap manifest** of `(symbol, side, [(start, end, reason)])` tuples
- [ ] **CACHE-08**: User can choose a per-scan / per-sweep **gap policy**: (a) `strict` — abort the scan, emit the gap manifest as a structured error, produce zero findings; (b) `continuous_only` — partition the requested window into maximal gap-free sub-ranges, run the scan independently on each, emit one finding per sub-range, and attach the gap manifest alongside. Miner MUST NOT silently produce findings on ranges containing holes under any policy.

### Operator Surface (OP)

- [ ] **OP-01**: User can run a single scan by name with parameters from the CLI (`miner scan <name@version> --instrument ... --timeframe ... --window ...`)
- [ ] **OP-02**: User can call any scan through the MCP server with the same parameter shape and identical findings output
- [ ] **OP-03**: User can call any scan through an HTTP API with the same parameter shape and identical findings output
- [ ] **OP-04**: User can submit a TOML sweep manifest (scans × instruments × timeframes × windows × parameter grids) and have miner fan it out in parallel
- [ ] **OP-05**: User can dry-run a sweep manifest and see the planned job graph + estimated job count before committing
- [ ] **OP-06**: User can interrupt an in-progress sweep (SIGINT) and keep every finding already streamed to stdout
- [ ] **OP-07**: User can introspect the scan catalogue via `miner scans` / MCP `list_scans`, returning each scan's name, version, parameters, and finding fields
- [ ] **OP-08**: User can rely on the scan registry rejecting unknown scans, validating parameters at the boundary, and echoing resolved parameters (including defaults) into every finding

### Statistical-Anomaly Scans — single series (ANOM)

- [ ] **ANOM-01**: User can compute return primitives (log, simple, overnight vs intraday split) as a reusable scan input
- [ ] **ANOM-02**: User can run summary statistics per window (mean, std, skew, excess kurtosis, IQR, min, max) via Welford / running-moments
- [ ] **ANOM-03**: User can run rolling volatility and volatility-of-volatility scans
- [ ] **ANOM-04**: User can run the Ljung-Box autocorrelation test on returns and on squared returns, with Q-stat, lags, p-value, and the ACF array
- [ ] **ANOM-05**: User can run the Augmented Dickey-Fuller (ADF) stationarity test with AIC-selected lag, trend specification, test stat, and p-value
- [ ] **ANOM-06**: User can run the KPSS stationarity test (opposite null to ADF) with stat and p-value
- [ ] **ANOM-07**: User can run the Lo-MacKinlay variance ratio test with VR(k) over multiple k, heteroskedasticity-robust z-stat, and p-value
- [ ] **ANOM-08**: User can run the ARCH-LM test for conditional heteroskedasticity with stat and p-value
- [ ] **ANOM-09**: User can run the Jarque-Bera normality test with JB stat, skew, excess kurtosis, p-value
- [ ] **ANOM-10**: User can run outlier / extreme-return detection (z-score, modified-z / MAD) with configurable threshold and surfaced indices + values
- [ ] **ANOM-11**: User can run a drawdown profile (max drawdown, duration, time-to-recover, distribution) on a synthetic equity curve

### Cross-Instrument Scans (CROSS)

- [ ] **CROSS-01**: User can rely on a time-alignment primitive that inner-joins two instruments on common timestamps with configurable behaviour for unequal calendars
- [ ] **CROSS-02**: User can run rolling Pearson and rolling Spearman correlation with summary stats and threshold-crossing flags
- [ ] **CROSS-03**: User can run rolling OLS regression (beta, alpha, R², residual std) of one series against another
- [ ] **CROSS-04**: User can run a lead-lag cross-correlation function (CCF) over a configurable lag grid with argmax-lag and its significance under a phase-scrambled null
- [ ] **CROSS-05**: User can run the Engle-Granger two-step cointegration test for pairs (hedge ratio, residual ADF stat, p-value) and the Ornstein-Uhlenbeck half-life of the residual

### Seasonality Scans (SEAS)

- [ ] **SEAS-01**: User can run an hour-of-day return / volatility profile (UTC) bucketed with mean, std, count, t-stat-vs-0, bootstrap CIs
- [ ] **SEAS-02**: User can run a day-of-week effect scan with the same bucket shape
- [ ] **SEAS-03**: User can run trading-session bucketing (Asia / London / NY / overlap) with configurable session boundaries and FX-major defaults
- [ ] **SEAS-04**: User can run an end-of-month / start-of-month effect scan bucketed by trading-day-of-month
- [ ] **SEAS-05**: User can run a one-way ANOVA / Kruskal-Wallis significance test across any bucketing
- [ ] **SEAS-06**: User can pass caller-supplied event timestamps (news-window placeholder) and receive aligned pre/post return + volatility window stats

### Statistical Hygiene (HYG)

- [ ] **HYG-01**: User can read an effect-size (Cohen's d / Hedges' g / Cliff's delta / scan-appropriate alternative) alongside every reported p-value
- [ ] **HYG-02**: User can rely on Benjamini-Hochberg FDR adjustment at the sweep level, emitted in a sweep-summary record at end-of-sweep
- [ ] **HYG-03**: User can request block / stationary bootstrap (Politis-Romano) confidence intervals on any scan that produces a statistic over autocorrelated series
- [ ] **HYG-04**: User can request phase-scrambled / circular-shift null distributions as a first-class option for any scan that produces a p-value
- [ ] **HYG-05**: User can reproduce any bootstrap / permutation result bit-for-bit by re-running with the seed echoed in the finding's repro envelope

### Output Contract (OUT)

- [ ] **OUT-01**: User can consume findings as newline-delimited JSON (NDJSON / JSONL) on stdout
- [ ] **OUT-02**: User can rely on every finding carrying: schema_version, scan@version, code_revision, instrument(s), side, timeframe, window, params, param_hash, effect (statistic + value + effect_size + p_value + p_value_null + ci_95), raw arrays used, optional notes, reserved DSR + FDR-q fields
- [ ] **OUT-03**: User can rely on deterministic output ordering for any given (scan × params × data slice) so that golden-file diffing works as a regression gate
- [ ] **OUT-04**: User can read, on every finding, the **actual continuous range used** (post-gap-partitioning) in `data_slice` and a reference to the gap manifest for that scan run; under `strict` policy the run emits a single error record containing the gap manifest and no findings

## v2 Requirements

Deferred to a future release. Tracked but not in the v1 roadmap.

### Statistical hygiene & extras (HYG-v2)

- **HYG-v2-01**: Deflated Sharpe Ratio implementation (Bailey & López de Prado) — reference port from published code
- **HYG-v2-02**: Sweep-level "interesting findings" top-N summary ranked by effect size × q-value
- **HYG-v2-03**: Memoised per-sweep intermediates (in-memory arena reused across scans on the same instrument/timeframe/window)
- **HYG-v2-04**: Side-channel raw-array storage (URI / file reference) instead of always inline, for findings whose raw arrays push into MB-scale

### Scans (SCAN-v2)

- **SCAN-v2-01**: Johansen cointegration test for baskets (n ≥ 3) — trace + max-eigenvalue stats, cointegrating vectors
- **SCAN-v2-02**: Granger causality test between two return series — F-test on restricted vs unrestricted VAR
- **SCAN-v2-03**: Hurst exponent / R-S analysis / DFA estimator with confidence interval
- **SCAN-v2-04**: PELT (Killick et al.) change-point detection on returns, volatility, and correlation series
- **SCAN-v2-05**: Correlation-breakdown detection (rolling corr + PELT or CUSUM on the corr series)
- **SCAN-v2-06**: Basket divergence z-score scans with PCA-derived or equal-vol weights
- **SCAN-v2-07**: Anderson-Darling normality test as a complement to Jarque-Bera

### Platform (PLAT-v2)

- **PLAT-v2-01**: PyO3 Python bindings exposing `miner-core` to `tradedesk` and the Quant agent
- **PLAT-v2-02**: Aggregator export — replace `tradedesk`'s Python 1m→Nm aggregation with miner's Rust implementation
- **PLAT-v2-03**: Markov-switching variance / mean models (Hamilton MS-AR)
- **PLAT-v2-04**: GARCH / EGARCH model fitting as a scan
- **PLAT-v2-05**: Bayesian online change-point detection
- **PLAT-v2-06**: Wavelet / spectral seasonality decomposition

## Out of Scope

Explicitly excluded. Documented to prevent scope creep.

| Feature | Reason |
|---------|--------|
| Chart / candlestick pattern matching | Different methodology (technical analysis); user explicitly de-scoped statistical-only. Build a separate tool if ever needed |
| Strategy generation / hypothesis formation | Owned by the Quant agent; coupling discovery to one consumer is wrong |
| Backtesting / PnL simulation | Owned by `tradedesk`; needs execution assumptions miner has no business making |
| Live / streaming market data | Different reliability + latency contract; miner is historical mining only |
| Persistent results database (SQLite / Parquet / DuckDB) | Streaming-and-stateless is a deliberate design property; caller owns persistence |
| GUI / TUI / web dashboard | Agent-operated only; UI doubles surface area for zero benefit to the primary consumer |
| Tick-level scans | 1-minute cache is the finest input; tick microstructure needs different storage + statistics |
| Source-data ingestion / normalisation / re-download | Owned by `tradedesk-dukascopy`; miner is a consumer |
| Caller-supplied DSL / embedded scripting for scans | Sandboxing nightmare; defeats the static registry + reproducibility contract. New scans land via Rust PR |
| Realtime alerting / push notifications | Requires live data + stateful watch loop, both out of scope |
| In-miner PASS/FAIL "significance verdict" | Significance is multi-test- and context-aware; miner cannot honestly render a verdict — agent decides |

## Traceability

Every v1 requirement maps to exactly one phase.

| Requirement | Phase | Status |
|-------------|-------|--------|
| FOUND-01 | Phase 1 | Pending |
| FOUND-02 | Phase 1 | Pending |
| FOUND-03 | Phase 1 | Pending |
| FOUND-04 | Phase 1 | Pending |
| FOUND-05 | Phase 1 | Pending |
| CACHE-01 | Phase 2 | Pending |
| CACHE-02 | Phase 2 | Pending |
| CACHE-03 | Phase 2 | Pending |
| CACHE-04 | Phase 2 | Pending |
| CACHE-05 | Phase 2 | Pending |
| CACHE-06 | Phase 2 | Pending |
| CACHE-07 | Phase 2 | Pending |
| CACHE-08 | Phase 2 | Pending |
| OP-01 | Phase 3 | Pending |
| OP-02 | Phase 6 | Pending |
| OP-03 | Phase 6 | Pending |
| OP-04 | Phase 5 | Pending |
| OP-05 | Phase 3 | Pending |
| OP-06 | Phase 3 | Pending |
| OP-07 | Phase 3 | Pending |
| OP-08 | Phase 3 | Pending |
| ANOM-01 | Phase 4 | Pending |
| ANOM-02 | Phase 4 | Pending |
| ANOM-03 | Phase 4 | Pending |
| ANOM-04 | Phase 4 | Pending |
| ANOM-05 | Phase 4 | Pending |
| ANOM-06 | Phase 4 | Pending |
| ANOM-07 | Phase 4 | Pending |
| ANOM-08 | Phase 4 | Pending |
| ANOM-09 | Phase 4 | Pending |
| ANOM-10 | Phase 4 | Pending |
| ANOM-11 | Phase 4 | Pending |
| CROSS-01 | Phase 4 | Pending |
| CROSS-02 | Phase 4 | Pending |
| CROSS-03 | Phase 4 | Pending |
| CROSS-04 | Phase 4 | Pending |
| CROSS-05 | Phase 4 | Pending |
| SEAS-01 | Phase 4 | Pending |
| SEAS-02 | Phase 4 | Pending |
| SEAS-03 | Phase 4 | Pending |
| SEAS-04 | Phase 4 | Pending |
| SEAS-05 | Phase 4 | Pending |
| SEAS-06 | Phase 4 | Pending |
| HYG-01 | Phase 5 | Pending |
| HYG-02 | Phase 5 | Pending |
| HYG-03 | Phase 5 | Pending |
| HYG-04 | Phase 5 | Pending |
| HYG-05 | Phase 5 | Pending |
| OUT-01 | Phase 1 | Pending |
| OUT-02 | Phase 1 | Pending |
| OUT-03 | Phase 1 | Pending |
| OUT-04 | Phase 3 | Pending |

**Phase 7 (Hardening, Benchmarks & Reproducibility)** carries no new v1 REQ-IDs; it closes verification debt for FOUND-02, FOUND-03, FOUND-04, CACHE-04, OUT-03, HYG-02, and HYG-05 via golden-file regression tests, noise-replay sweep test, flamegraph profiling, the `miner-bench` + hyperfine bench harness, `cargo audit` / `cargo deny` clean runs, and the README data-source caveats section.

**Coverage:**
- v1 requirements: 52 total
- Mapped to phases: 52 ✓
- Unmapped: 0

---
*Requirements defined: 2026-05-15*
*Last updated: 2026-05-15 after roadmap creation (traceability filled in)*
