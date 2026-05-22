# tradedesk-miner

## What This Is

`tradedesk-miner` is a high-performance, agent-operable data-mining engine for historical financial OHLCV data. It scans cached candle data (typically Dukascopy bid/ask CSVs prepared by `tradedesk-dukascopy`) and surfaces statistical anomalies, cross-instrument relationships, and seasonality effects as raw candidate findings. Its primary consumer is the RadiusRed Quant agent, which turns those findings into testable trading-strategy hypotheses that feed back into `tradedesk`.

It is the **discovery layer** in the RadiusRed pipeline:

```
tradedesk-dukascopy (cache)  →  tradedesk-miner (raw findings)
                              →  Quant agent (hypotheses)
                              →  tradedesk (strategies, backtests, live)
```

## Current State

**Shipped v1.0** (2026-05-22) — 7 phases, 50 plans, 124 tasks, 63,741 LOC Rust.

The v1.0 MVP delivers:
- Locked `Finding` JSON envelope schema with CI-enforced stdout=findings/stderr=logs discipline
- Pluggable `Reader` trait + Dukascopy zstd-CSV reader + deterministic aggregator + Arrow IPC derived-bar cache + structured gap manifest
- Scan engine facade with `miner scan` / `miner sweep` / `miner scans` CLI, four-tier exit codes, SIGINT handling
- 22 v1 scans across ANOM (returns, summary, vol-rolling, Ljung-Box, ADF, KPSS, VR, ARCH-LM, JB, outliers, drawdown) + CROSS (Pearson/Spearman rolling, OLS rolling, lead-lag, Engle-Granger) + SEAS (hour/day/session/EOM-SOM/event-window/ANOVA-Kruskal) families, all validated against scipy/statsmodels goldens
- Statistical hygiene layer: effect sizes, block bootstrap CIs, IAAFT phase-scramble nulls, BH-FDR aggregation, deterministic seed propagation
- Reproducible TOML sweep manifest runner with cartesian expansion, rayon parallelism, byte-identical reruns
- Docs and contracts: `docs/agent_integration.md`, `docs/future_mcp_http.md`, scan catalogue, sweep manifest reference, Dukascopy data-source caveats deep ref
- Hardening: real goldens regen pipeline, noise-replay regression test, criterion microbenches, dhat alloc profiling, cargo-audit/deny CI, clone-and-run fixture cache + README quickstart, locked findings-envelope snapshot test

## Next Milestone Goals

(To be defined when starting v1.1 via `/gsd:new-milestone`.) Likely candidates carried forward in **Active** below.

## Core Value

Surface raw statistical candidates from years of OHLCV data fast enough that a quant agent can run wide sweeps and targeted queries interactively, without waiting on Python data pipelines.

## Requirements

### Validated

- ✓ Rust library crate with a stable scan-engine API — v1.0
- ✓ Pluggable data-reader trait with Dukascopy zstd-CSV reader matching the `tradedesk-dukascopy` cache layout — v1.0
- ✓ Caller-specified bar resolution (15m / 1h / 1d) — v1.0
- ✓ Semi-permanent on-disk cache of aggregated bars (Arrow IPC, two-axis invalidation) — v1.0
- ✓ Statistical anomaly scans (vol regimes, mean-reversion, autocorrelation, return distributions) — v1.0
- ✓ Cross-instrument relationship scans (lead-lag, correlations, cointegration) — v1.0
- ✓ Time-of-day / seasonality scans (session, hour-of-day, day-of-week, end-of-month) — v1.0
- ✓ Findings carry effect statistics **plus** raw underlying arrays — v1.0
- ✓ Findings stream to stdout in JSONL; miner does not persist results — v1.0
- ✓ Interactive query mode + batch sweep mode — v1.0
- ✓ CLI binary as thin wrapper over the library — v1.0
- ✓ MCP server interface — designed; implementation deferred to v2 (docs/future_mcp_http.md)
- ✓ HTTP API interface — designed; implementation deferred to v2 (docs/future_mcp_http.md)
- ✓ Open-source friendly defaults: no hardcoded paths, configurable cache root, documented reader trait — v1.0

### Active

<!-- Carried forward to v1.1 / v2. Concrete scope set during /gsd:new-milestone. -->

- [ ] HYG-01: Engle-Granger ADF kernel reconciliation against canonical `scan::anom::adf::kernel::adfuller` (deferred from Plan 04-11; re-enables `engle_granger_matches_statsmodels_coint_golden`)
- [ ] PLAT-v2-07: MCP server implementation (`rmcp` Rust SDK; design locked in `docs/future_mcp_http.md`)
- [ ] PLAT-v2-08: HTTP server implementation (`axum`; design locked in `docs/future_mcp_http.md`)
- [ ] Clippy workspace `-D warnings` cleanup (gen-fixtures.rs format_collect + hygiene_dispatch.rs lints — see Phase 7 `deferred-items.md`)
- [ ] Bench-results capture from reference workstation (populate TBD cells in `docs/bench-results.md`)
- [ ] CHANGELOG.md [1.0.0] entry (fill in once v1.0 release date is final)

### Out of Scope

- Chart/price-structure pattern matching (head-and-shoulders, flags, doji clusters) — miner is statistical, not technical-analysis
- Strategy generation, hypothesis formation, backtesting — Quant agent and `tradedesk` own those
- User interface (web, TUI, desktop) — agent-operated only
- Tick-level scans — 1-minute cache is the finest input
- Python bindings (PyO3) in v1 — deferred until a concrete `tradedesk` integration requires them
- Persistent findings store (SQLite / Parquet / DuckDB) — caller owns persistence
- Re-downloading or normalizing source data — `tradedesk-dukascopy` owns the cache; miner consumes
- Live market data — historical scans only; live is `tradedesk`'s domain
- Hard latency/throughput SLOs — "as fast as good Rust and parallelism deliver"

## Context

**RadiusRed toolset.** `tradedesk-miner` joins:

- [`tradedesk`](https://github.com/radiusred/tradedesk) — Python event-driven trading framework (backtest + live IG execution)
- [`tradedesk-dukascopy`](https://github.com/radiusred/tradedesk-dukascopy) — Python downloader that builds the zstd-CSV OHLCV cache miner reads from

**Existing cache layout** (e.g. `/opt/tradedesk/marketdata`):

```
<cache-root>/<SYMBOL>/<YYYY>/<MM(00-indexed)>/<DD>_<side>.csv.zst
  e.g. EURUSD/2024/00/10_bid.csv.zst  → 2024-01-10 EURUSD bid, 1-min OHLCV
```

Each daily file is a CSV of 1-minute bars: `timestamp,open,high,low,close,volume`, UTC, prices already price-divisor-scaled.

Sample environment for development:
- 28 instruments (FX majors, crosses, indices, metals, crypto, commodities)
- 2020-2026 history, ~7.3 GB compressed
- bid and ask sides stored separately

**Consumer model.** The RadiusRed Quant agent (paperclip agent, remote) does both:
- **Query-driven** scans for hypothesis testing ("scan EURUSD/GBPUSD lead-lag at 15m over 2024")
- **Batch sweeps** to surface unprompted ideas (periodic wide scans)

It expects findings as `effect statistics + raw arrays` so it can re-test independently and decide what's worth promoting to a hypothesis.

**Why Rust.** Mining is embarrassingly parallel over instruments, timeframes, parameter grids, and historical windows. A scan can fan out across 28 instruments × 3 timeframes × dozens of parameter combinations × millions of bars per slice.

**Future tradedesk reuse.** `tradedesk` currently does its own 1-min → resampled-bar aggregation in Python. Miner's aggregator is a candidate to eventually export and reuse from `tradedesk` (via the deferred Python bindings) so a single fast Rust implementation owns aggregation for the whole ecosystem.

## Constraints

- **Language**: Rust preferred (C as a fallback) — needed for raw scan throughput and clean cross-platform binaries
- **Source data**: must read the existing `tradedesk-dukascopy` cache layout without modification or re-download
- **Statelessness for results**: miner streams findings; it must not assume a results database is available
- **Aggregate-bar cache**: the only writable state miner owns is its derived-bar cache, location configurable
- **Agent-operability**: every miner capability must be reachable from CLI, MCP, and HTTP without divergent behavior
- **Open-source posture**: configurable paths, documented reader trait, no RadiusRed-specific assumptions baked in
- **License**: Apache 2.0 (matches `tradedesk` and `tradedesk-dukascopy`)
- **Repo / package name**: `tradedesk-miner` (follows family naming convention)

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Rust, library + thin CLI/MCP/HTTP wrappers | Single core, three surfaces, no logic duplication | ✓ Good (v1.0 ships library + CLI; MCP/HTTP design locked, impl deferred to v2) |
| Statistical mining only; no chart patterns | Quant agent treats findings statistically; chart-pattern matching is a different (noisier) discipline | ✓ Good |
| Findings carry effect stats + raw arrays | Quant agent re-tests independently rather than trusting miner's summary | ✓ Good |
| Streaming output; caller persists | Keeps miner stateless and protocol-agnostic | ✓ Good |
| Pluggable readers from day 1 | Open-source distribution implies users with different cache shapes | ✓ Good (Reader trait shipped; only Dukascopy reader implemented in v1) |
| Native bar resolution = caller-specified (15m / 1h / 1d typical) | Matches how the RadiusRed quant team actually works; 1-min is substrate, not analysis grid | ✓ Good |
| Aggregated bars cached semi-permanently | Aggregation is expensive; recomputation wasteful when scans iterate parameter grids | ✓ Good (Arrow IPC, two-axis invalidation) |
| PyO3 bindings deferred to post-v1 | Speeds v1; bindings land when concrete `tradedesk` integration demands them | — Pending (no v2 driver yet) |
| `tradedesk-miner` + Apache 2.0 | Discoverable in RadiusRed family; license matches siblings | ✓ Good |
| No silent scans over gapped data; caller picks `strict` or `continuous_only` policy | Dukascopy caches legitimately have gaps; silent scans over holes are how nonsense findings get published | ✓ Good (gap_policy enforced in engine) |
| `miner-core` is sync + rayon only; no tokio | Async-runtime-free core keeps the library embeddable; tokio enters only at MCP/HTTP wrappers | ✓ Good (CI-enforced via `cargo tree -p miner-core` grep) |
| Stdout = findings, stderr = logs | Wire-protocol cleanliness for agent consumers | ✓ Good (CI-enforced via clippy disallowed_macros) |
| `Finding` envelope locked from day 1 | Schema retrofitting is painful for downstream consumers | ✓ Good (schema_version field; envelope_snapshot test pins it) |
| Arrow IPC for derived-bar cache (not Parquet, not bincode+zstd) | Read-mostly access pattern + zero-copy + language portability | ✓ Good (one file per (source, symbol, side, timeframe), tempfile-rename crash-safety) |
| Hand-rolled statistical kernels validated against scipy/statsmodels goldens | No comprehensive Rust stats crate covers ADF/KPSS/VR/ARCH-LM/JB/bootstrap/BH-FDR/DSR | ✓ Good (provenance-gated golden tests, pinned scipy 1.14.1 / statsmodels 0.14.6) |
| IAAFT phase-scramble null for HYG-02/HYG-05 | Surrogate that preserves marginal + power spectrum; circular-shift insufficient for autocorr/spectral tests | ✓ Good (Theiler 1992 kernel, realfft 3.5, byte-identical reruns) |
| Phase 6 reshaped CODE → DOCS; MCP/HTTP impl deferred to v2 | rmcp SDK churn + axum integration cost not justified for v1 quant-agent use case (CLI subprocess works) | ✓ Good (OP-02/OP-03 → PLAT-v2-07/PLAT-v2-08; design preserved in docs/future_mcp_http.md) |
| Engle-Granger ADF parity left local; canonical re-route deferred to HYG-01 | Engle-Granger v1 ships with hand-rolled adf_step; full MacKinnon parity costs more than v1 needs | ⚠️ Revisit at HYG-01 (one golden test `#[ignore]`d) |

## Evolution

This document evolves at phase transitions and milestone boundaries.

**After each phase transition** (via `/gsd-transition`):
1. Requirements invalidated? → Move to Out of Scope with reason
2. Requirements validated? → Move to Validated with phase reference
3. New requirements emerged? → Add to Active
4. Decisions to log? → Add to Key Decisions
5. "What This Is" still accurate? → Update if drifted

**After each milestone** (via `/gsd:complete-milestone`):
1. Full review of all sections
2. Core Value check — still the right priority?
3. Audit Out of Scope — reasons still valid?
4. Update Context with current state

---
*Last updated: 2026-05-22 after v1.0 MVP milestone.*
