# tradedesk-miner

## What This Is

`tradedesk-miner` is a high-performance, agent-operable data-mining engine for historical financial OHLCV data. It scans cached candle data (typically Dukascopy bid/ask CSVs prepared by `tradedesk-dukascopy`) and surfaces statistical anomalies, cross-instrument relationships, and seasonality effects as raw candidate findings. Its primary consumer is the RadiusRed Quant agent, which turns those findings into testable trading-strategy hypotheses that feed back into `tradedesk`.

It is the **discovery layer** in the RadiusRed pipeline:

```
tradedesk-dukascopy (cache)  →  tradedesk-miner (raw findings)
                              →  Quant agent (hypotheses)
                              →  tradedesk (strategies, backtests, live)
```

## Core Value

Surface raw statistical candidates from years of OHLCV data fast enough that a quant agent can run wide sweeps and targeted queries interactively, without waiting on Python data pipelines.

## Requirements

### Validated

<!-- Shipped and confirmed valuable. -->

(None yet — ship to validate)

### Active

<!-- Current scope. Building toward these. -->

- [ ] Rust library crate with a stable scan-engine API
- [ ] Pluggable data-reader trait; ships with a Dukascopy zstd-CSV reader matching the `tradedesk-dukascopy` cache layout
- [ ] Caller-specified bar resolution (15m / 1h / 1d as the typical working set; not 1m by default)
- [ ] Semi-permanent on-disk cache of aggregated bars, keyed by source + symbol + resolution + side
- [ ] Statistical anomaly scans (volatility regimes, mean-reversion edges, autocorrelation, unusual return distributions)
- [ ] Cross-instrument relationship scans (lead-lag, correlation breakdowns, cointegration, basket divergence)
- [ ] Time-of-day / seasonality scans (session, hour-of-day, day-of-week, end-of-month)
- [ ] Findings carry effect statistics **plus** raw underlying arrays so the quant agent can re-test or visualize
- [ ] Findings stream to stdout in a stable, structured format (JSON / JSONL); miner does not persist results
- [ ] Both interactive query mode (single scan, one set of parameters) and batch sweep mode (broad pre-defined scan set)
- [ ] CLI binary as a thin wrapper over the library
- [x] MCP server interface — designed; implementation deferred to v2 (see docs/future_mcp_http.md)
- [x] HTTP API interface — designed; implementation deferred to v2 (see docs/future_mcp_http.md)
- [ ] Open-source friendly defaults: no hardcoded paths, cache root configurable, reader pluggability documented

### Out of Scope

<!-- Explicit boundaries. Includes reasoning to prevent re-adding. -->

- Chart/price-structure pattern matching (head-and-shoulders, flags, doji clusters) — explicitly de-scoped: miner is statistical, not technical-analysis
- Strategy generation, hypothesis formation, backtesting — Quant agent and `tradedesk` already own those
- User interface (web, TUI, desktop) — agent-operated only
- Tick-level scans — 1-minute cache is the finest input; tick microstructure can come later if needed
- Python bindings (PyO3) in v1 — deferred until a concrete `tradedesk` integration requires them
- Persistent findings store (SQLite / Parquet / DuckDB) — caller owns persistence; miner streams
- Re-downloading or normalizing source data — `tradedesk-dukascopy` already owns the cache; miner is a consumer
- Live market data — historical scans only; live is `tradedesk`'s domain
- Hard latency/throughput SLOs — "as fast as good Rust and parallelism deliver" is the bar; concrete numbers fall out of benchmarks once v1 lands

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

**Why Rust.** Mining is embarrassingly parallel over instruments, timeframes, parameter grids, and historical windows. A scan can fan out across 28 instruments × 3 timeframes × dozens of parameter combinations × millions of bars per slice. Python's current aggregation/scanning is the bottleneck the agent feels.

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
| Rust, library + thin CLI/MCP/HTTP wrappers | Single core, three surfaces, no logic duplication; standard Rust pattern | — Pending |
| Statistical mining only; no chart patterns | The quant agent treats findings statistically; chart-pattern matching is a different (and noisier) discipline | — Pending |
| Findings carry effect stats + raw arrays | The quant agent re-tests independently rather than trusting miner's summary | — Pending |
| Streaming output; caller persists | Keeps miner stateless and protocol-agnostic; consumers (CLI users, MCP agents, HTTP clients) decide storage | — Pending |
| Pluggable readers from day 1 | Open-source distribution implies users with different cache shapes; trait now is cheaper than a refactor later | — Pending |
| Native bar resolution = caller-specified (15m / 1h / 1d typical) | Matches how the RadiusRed quant team actually works; 1-min cache is a substrate, not the analysis grid | — Pending |
| Aggregated bars cached semi-permanently | Aggregation is expensive; recomputation is wasteful when scans iterate parameter grids | — Pending |
| PyO3 bindings deferred to post-v1 | Speeds v1; binary distribution covers MCP/HTTP/CLI use cases; bindings land when a concrete tradedesk integration demands them | — Pending |
| `tradedesk-miner` + Apache 2.0 | Discoverable in the RadiusRed family; license matches siblings | — Pending |
| No silent scans over gapped data; caller picks `strict` or `continuous_only` policy per run | Dukascopy caches legitimately have gaps (weekends, outages, partial downloads); silent scans over holes are the textbook way to publish nonsense findings. Detection happens at the reader/aggregator boundary; policy is selected per scan invocation | — Pending |

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
*Last updated: 2026-05-15 after initialization*
