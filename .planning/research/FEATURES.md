# Feature Research

**Domain:** Quant data-mining engine over historical OHLCV cache (Rust, agent-operated)
**Researched:** 2026-05-15
**Confidence:** MEDIUM-HIGH (statistical-finance methodology is well-established in literature — Tsay "Analysis of Financial Time Series", Lo & MacKinlay, Hamilton, López de Prado; tooling assumptions for the agent-operable wrapper are based on the explicit user requirements in PROJECT.md and the milestone context)

**Note on sourcing:** External web/Context7 lookups were unavailable in this run. Algorithm names, parameters, and pitfalls below are drawn from canonical econometrics references and accepted practice. Where a choice is non-obvious (e.g. Johansen vs Engle-Granger as default) the rationale is given so the roadmap author can re-verify before committing.

---

## Domain Framing

The miner is a **discovery layer**, not an analyst. Its job is to take cached OHLCV bars and emit *raw statistical findings* — effect statistics plus the underlying data arrays — that a downstream Quant agent will turn into hypotheses. Three pattern families are in scope:

1. **Statistical anomalies** — single-series properties that deviate from a null (random walk, IID returns, constant variance).
2. **Cross-instrument relationships** — joint behaviour between two or more series.
3. **Time-of-day / seasonality** — calendar-conditional effects.

Two operator surfaces:
- **Query mode** — one named scan, one parameter set, one (or a few) instruments.
- **Sweep mode** — a manifest of scans × instruments × timeframes × windows × parameter grids, fanned out in parallel.

The **output contract** is fixed: every finding carries (a) effect statistics with a confidence/p-value, (b) the raw input arrays used to produce the finding, and (c) reproducibility metadata (scan name + version, parameter hash, data slice, code version, RNG seed if applicable).

---

## Feature Landscape

### Table Stakes (Users Expect These)

A quant agent would consider the tool useless without these.

#### Infrastructure / plumbing

| Feature | Why a quant cares | Complexity | Notes |
|---------|-------------------|------------|-------|
| Pluggable data-reader trait + Dukascopy zstd-CSV reader | Without it, miner can't read the cache the rest of the RadiusRed stack produces | M | Already in PROJECT.md Active list. Trait shape must support lazy day-by-day streaming, not whole-history loads. |
| Deterministic 1m → {15m, 1h, 1d} bar aggregator, close-aligned, UTC | Every downstream scan depends on consistent bars; aggregation drift = silently wrong findings | M | Must handle gaps (weekends, holidays) explicitly; never silently forward-fill. Bid/ask sides aggregated independently. |
| On-disk aggregated-bar cache (source+symbol+resolution+side keyed) | Parameter-grid sweeps re-use the same aggregated bars hundreds of times; recomputing is the #1 wall-clock cost | M | PROJECT.md already calls this out. Cache file format should be column-oriented (Parquet or Arrow IPC) so partial reads are cheap. |
| Bid/ask-aware data plane | FX miners need to scan both sides (and mid). Conflating sides hides spread-driven effects | S | Side is a first-class dimension of the cache key. |
| Window / slice selector (date range, rolling window, in-sample / out-of-sample split) | Every scan needs a "what data did this use" handle for reproducibility | S | Slice must be expressible declaratively in the sweep manifest. |
| Findings stream (JSONL) with effect-stats + raw arrays + repro metadata | The contract with the Quant agent; without it the agent can't re-test | M | Schema must be versioned. Raw arrays should be optionally compressed (zstd) inline or referenced by side-channel file for very large arrays. |
| Reproducibility envelope on every finding | Without scan-version + param-hash + data-slice + code-rev a finding is unauditable | S | Cheap to add at finding-emit time; expensive to retrofit. |
| Stable scan registry (named scans, versioned) | Sweep manifests reference scans by name; renaming or behaviour-changing without a version bump breaks every persisted finding upstream | S | E.g. `volatility.garch_regime@v1`. |
| Parameter validation + echo | A scan that silently coerces or ignores parameters is a debugging nightmare | S | Echo resolved params (including defaults) into the finding. |
| Parallel execution over (instrument × timeframe × window × param) grid | "As fast as good Rust delivers" is the whole point of building this in Rust | M | Rayon / Tokio task scheduler. Bound memory by streaming windows rather than materialising whole grids. |

#### Statistical-anomaly scans (single-series)

| Feature | Why a quant cares | Complexity | Notes |
|---------|-------------------|------------|-------|
| **Return computation primitives** — simple, log, overnight vs intraday, vol-normalised | Every downstream scan operates on returns, not prices; getting this wrong corrupts everything | S | Log returns are the default; simple returns and excess-over-rolling-mean as optional. |
| **Summary stats per window** — mean, std, skew, excess kurtosis, min, max, IQR | Baseline distributional description; consumed both directly and as features by other scans | S | Welford / running moments for streaming windows. |
| **Ljung-Box autocorrelation test** on returns (and squared returns) | Tests whether returns are serially dependent — a direct test of weak-form efficiency violation | S | Squared-returns version detects volatility clustering. Report Q-stat, lags tested, p-value, and the ACF array. |
| **Variance ratio test (Lo–MacKinlay)** | Classical mean-reversion / momentum diagnostic; rejecting random walk in either direction is exactly the signal a quant wants | M | Report VR(k) for multiple k, heteroskedasticity-robust z-stat, p-value. |
| **Augmented Dickey–Fuller (ADF) stationarity test** | Tells the agent whether a series is mean-reverting; gate for whether mean-reversion strategies are even plausible | S | Report test stat, p-value, lag used (AIC-selected), trend specification. |
| **KPSS stationarity test** (complement to ADF) | ADF and KPSS have opposite nulls; running both catches near-unit-root ambiguity | S | Cheap once ADF is in. |
| **Hurst exponent / R/S analysis** | Distinguishes persistent (H > 0.5), mean-reverting (H < 0.5), random (H ≈ 0.5) regimes — a single-number anomaly screen | M | Use rescaled-range or DFA estimator; report H plus a confidence interval. |
| **Rolling volatility + volatility-of-volatility** | Volatility regime detection is one of the user's named scan families | S | Realised vol from squared returns; vol-of-vol as std of rolling vol. |
| **ARCH-LM test** for volatility clustering | Detects conditional heteroskedasticity — the prerequisite for any GARCH-style modelling | S | Engle's LM test. Report stat, p-value. |
| **Return-distribution normality tests** — Jarque-Bera, Anderson-Darling | Heavy-tailed / skewed return windows are themselves anomalies the agent wants to know about | S | Report JB stat, skew, excess kurtosis, p-value. |
| **Outlier / extreme-return detection** — z-score, modified z (MAD), tail-quantile flags | A handful of bars often dominate a window's statistics; the agent needs them surfaced, not hidden | S | Configurable z-threshold; report indices, values, neighbouring-bar context. |
| **Drawdown profile** — max drawdown, drawdown duration, time-to-recover, drawdown distribution | Drawdown shape is a first-class anomaly statistic (asymmetry between gains/losses) | S | Computed from a synthetic equity curve of returns. |

#### Cross-instrument scans

| Feature | Why a quant cares | Complexity | Notes |
|---------|-------------------|------------|-------|
| **Pearson rolling correlation** (and rolling Spearman as a robust variant) | The most basic cross-instrument relationship; the baseline every more advanced cross-scan compares against | S | Report rolling-corr array plus summary stats; flag windows where corr crosses configurable thresholds. |
| **Rolling beta / OLS regression coefficient** of one series vs another | Beta drift over time is itself a tradable signal | S | OLS on returns; report beta, alpha, R², residual std per window. |
| **Lead-lag cross-correlation function (CCF)** with lag grid | Directly named in the milestone context — finds whether one instrument moves ahead of another | M | Report CCF array over a configurable lag range; flag and report argmax-lag plus its significance under a phase-scrambled null. |
| **Engle–Granger two-step cointegration test** for pairs | The standard pairs-trading screen; cheap to compute, well-understood | M | ADF on the OLS residual. Report hedge ratio, residual ADF stat, p-value, residual half-life. |
| **Johansen cointegration test** for baskets (n ≥ 2) | Generalises Engle-Granger to baskets; needed once the agent wants triplets / FX baskets | M-L | Report trace + max-eigenvalue stats, cointegrating vectors, ranks. Use this as the default *only* for n ≥ 3; for pairs, EG is simpler and matches literature. |
| **Cointegration residual half-life** (Ornstein-Uhlenbeck fit) | Tells the agent how fast the spread mean-reverts — the difference between a tradable pair and a too-slow one | S | Once residual is computed, OU half-life is a closed-form AR(1) fit. |
| **Correlation-breakdown detection** | Pairs with stable correlation that suddenly break apart are classic high-information events | M | Rolling-corr + change-point detector (PELT or CUSUM) on the corr series. |
| **Basket divergence scan** — z-score of one instrument vs a weighted basket of peers | The natural generalisation of pairs to "EURUSD vs FX-majors basket" type questions | M | Weights either equal, equal-vol, or principal-component-derived. Report z-score time series, threshold crossings, recovery times. |
| **Granger causality test** between two return series | Formalises "does X help predict Y beyond Y's own past" — the statistical underpinning of lead-lag claims | M | F-test on restricted vs unrestricted VAR; report p-values per lag. |

#### Time-of-day / seasonality scans

| Feature | Why a quant cares | Complexity | Notes |
|---------|-------------------|------------|-------|
| **Hour-of-day return / volatility profile** | The single most-asked seasonality question in FX | S | Bucket returns by UTC hour; report mean, std, count, t-stat vs 0 per bucket. Confidence intervals via bootstrap. |
| **Day-of-week effect** | Classic calendar anomaly; cheap to run, easy for the agent to interpret | S | Same shape as hour-of-day. |
| **Trading-session tags** (Asia / London / NY / overlap) and session-conditional stats | FX behaviour differs sharply by session; binning by raw UTC hour misses session-overlap effects | M | Session boundaries are configurable per instrument; default FX-major sessions provided. |
| **End-of-month / start-of-month effect** | Well-documented anomaly (Lakonishok-Smidt and successors); easy table-stakes inclusion | S | Bucket by trading-day-of-month (1st, last, last-3, etc). |
| **Bucket-comparison significance test** — one-way ANOVA / Kruskal-Wallis across buckets | Without a significance test, hour-of-day means are just noise; need a global "are buckets different at all" stat | S | Report F-stat (or H-stat) and p-value for the bucketed series. |
| **Phase-randomisation / circular-shift null** for seasonality | The right null for "is hour-of-day really different" — preserves autocorrelation that naive permutation destroys | M | Bootstrap p-values via circular shifts; standard fix for autocorrelated seasonality tests. |
| **Calendar-event window placeholder** — caller-provided event timestamps, pre/post return + vol stats | The user explicitly listed news-window placeholders; miner doesn't fetch news but accepts event-time inputs | S | Event-study skeleton: caller passes a `Vec<DateTime<Utc>>`, miner returns aligned-window stats. |

#### Operator surface

| Feature | Why a quant cares | Complexity | Notes |
|---------|-------------------|------------|-------|
| Single-scan query API (CLI / MCP / HTTP) | "Run this one scan with these params" is the interactive-debugging path | S | All three surfaces wrap the same library entry point. |
| Sweep-manifest mode | Batch idea-discovery is the second user mode and cannot reasonably be driven by repeated single calls | M | Manifest is declarative (TOML or YAML): scan name, instrument list, timeframe list, window list, param grid. See "Sweep manifest" notes below. |
| Sweep dry-run / plan output | Sweeps fan out to thousands of jobs; the agent needs a "what would this run" preview before committing | S | Print the job graph + estimated count. |
| Per-scan reproducibility (scan@version + param hash + data slice + code rev) | Without this, a sweep's findings can't be reproduced later when the agent wants to drill into one | S | Already listed under plumbing; restated here because it ties scans to operator UX. |
| Progress + structured logging on stderr (findings on stdout) | The agent runs miner as a subprocess and parses stdout; mixing logs into findings stream is a parser-killer | S | Standard Unix discipline. |
| Cancellation / partial-result emission on signal | Long sweeps must be interruptible without losing the findings already produced | S | Findings are streamed, so SIGINT-mid-sweep already mostly works; document it. |

---

### Differentiators (Competitive Advantage)

Features that elevate miner from "library wrapper" to a genuinely useful discovery engine for an agent.

| Feature | Value Proposition | Complexity | Notes |
|---------|-------------------|------------|-------|
| **Effect-size first, p-value second** finding schema | Most stats libraries report p-values and stop; reporting Cohen's d / Hedges' g / Cliff's delta alongside makes the agent's filtering trivial | S | Tiny code; large pay-off in agent decision quality. |
| **Multiple-testing awareness baked into sweeps** | A sweep that runs 10 000 tests will produce hundreds of "p < 0.05" hits by chance; emitting Benjamini-Hochberg FDR-adjusted q-values per sweep saves the agent from chasing noise | M | Compute on full sweep at end-of-run; emit a "sweep summary" record. The single biggest force-multiplier you can give the Quant agent. |
| **Block-bootstrap / stationary-bootstrap confidence intervals** for autocorrelated series | Plain bootstrap is wrong on time series; offering block-bootstrap CIs out of the box prevents a whole class of agent-side mistakes | M | Politis-Romano stationary bootstrap as default. |
| **Phase-scrambled / circular-shift null distributions** as a first-class option for any scan | Many "anomalies" disappear under a properly autocorrelated null; making this trivial to enable is a real edge | M | Generic wrapper that takes a scan + null-generation strategy. |
| **Half-life and persistence reporting on every mean-reversion scan** | OU half-life turns "is it mean-reverting" into "is it tradable" | S | Already listed under cointegration; broaden it to single-series ADF results too. |
| **Regime / change-point detection** (PELT, CUSUM, Bai-Perron) on returns, vol, or correlation series | Regimes are the right unit of analysis for many FX phenomena; reporting them as bracketed sub-series is high-information output | L | PELT (Killick et al.) is the modern default; offer Bayesian online change-point as a v2 option. |
| **Markov-switching variance / mean model** | The principled regime model; complements change-point detection | L | Hamilton 2-state MS-AR / MS-variance. v2 candidate — relatively heavy to implement well in Rust. |
| **Memoised intermediate computations across a sweep** (e.g. returns array, rolling-mean array reused across scans for same instrument/timeframe/window) | Sweeps repeat identical intermediates dozens of times; a per-sweep arena cuts wall-clock substantially | M | Tied to the aggregator cache but operating one level up — in-memory, per-sweep. |
| **Deterministic RNG with seed in repro envelope** | Every bootstrap / permutation test is reproducible bit-for-bit | S | Standard `rand` seeded from the param hash + scan@version. |
| **Sweep-level "interesting findings" summary record** | At end of sweep, emit one record ranking findings by effect size × |z| (or similar), already FDR-adjusted | S | Makes large sweeps consumable without the agent re-streaming every record. |
| **Streaming, bounded-memory windows** | Allows scans over multi-year 1h or 15m series without materialising in RAM; preserves agent's ability to sweep wide | M | Already implicit in "Rust, fast"; making it explicit and documented is the differentiator. |
| **Schema-versioned findings + machine-readable scan catalogue** | The agent can introspect what scans exist, what params each takes, what fields each finding has — no out-of-band knowledge needed | S | Ship a `miner scans` (or MCP `list_scans`) call that returns the catalogue. |
| **Raw arrays as a side-channel reference** (URI / file path) instead of always inline | Some findings carry MB-scale arrays; inline JSONL becomes unwieldy. Optional side-channel keeps both modes available | M | Caller chooses. Differentiator only if implemented cleanly. |

---

### Anti-Features (Commonly Requested, Often Problematic)

Features that look reasonable but contradict the milestone scope or invite scope creep.

| Feature | Why Requested | Why Problematic | Alternative |
|---------|---------------|-----------------|-------------|
| **Chart / candlestick pattern matching** (head-and-shoulders, flags, doji clusters, engulfing patterns) | "It's a trading tool; trading tools find patterns on charts" | Explicitly out of scope in PROJECT.md. Chart patterns are subjective, low-signal, and live in a different methodology (technical analysis) than the agent's statistical pipeline | If/when needed, build a *separate* TA-pattern tool; do not bolt it onto miner |
| **Strategy generation / hypothesis formation** | "Findings could just be turned into strategies inside miner" | Owned by the Quant agent. Doing it inside miner couples the discovery layer to a specific consumer and prevents the agent from doing its real job (judgement under context the miner doesn't have) | Findings stop at effect-stats + arrays; agent generates hypotheses |
| **Backtesting / PnL simulation** | "Why not score findings by simulated PnL right here?" | Owned by `tradedesk`. A backtest needs execution assumptions, costs, slippage, position sizing — none of which belong in a stats engine | Emit findings; `tradedesk` runs the backtest from a hypothesis |
| **Live / streaming market data** | "Same scans should work on live ticks" | Out of scope (PROJECT.md). Live introduces a totally different reliability and latency contract; building it dilutes the historical-mining mission | Live is `tradedesk`'s domain |
| **Persistent results database** (SQLite / Parquet / DuckDB) | "Surely we want to query past findings?" | Out of scope (PROJECT.md). Statelessness is a deliberate property — caller decides where to put findings | JSONL stream; caller persists |
| **GUI / TUI / web dashboard** | "It would be nice to browse findings visually" | Out of scope. The consumer is an agent, not a human, and adding UI doubles surface area | The Quant agent (or downstream tooling) renders findings if needed |
| **Tick-level scans** | "More resolution = more signal" | Out of scope. 1-minute is the finest cache resolution; tick scans need different storage, different stats (microstructure noise, bid-ask bounce) and different infrastructure | Defer to a future microstructure-specific tool |
| **Source-data ingestion / normalisation / re-download** | "Miner should just fetch what it needs" | Out of scope. `tradedesk-dukascopy` owns the cache; miner is a consumer | Reader trait + Dukascopy implementation, nothing more |
| **Chart-pattern-style "screens" rebranded as anomalies** (e.g. "find all symbols with a recent bullish engulfing") | Looks statistical, is actually TA | Same as chart-pattern matching: subjective definitions, weak nulls | Phrase scans in distributional / statistical terms |
| **ML model training as a scan** ("scan for instruments where an LSTM beats AR(1)") | "Mining could include ML model fitting" | Adds enormous dependency surface (training, serialisation, GPU vs CPU), and the agent already owns model selection upstream | If a model is needed, it belongs in the agent's hypothesis-testing step |
| **In-miner statistical-significance "verdict"** ("PASS / FAIL", "ALPHA / NO ALPHA") | "Just tell me if it's good" | Significance is multi-test-aware, context-aware, and judgement-laden; the miner can't know enough to render a verdict honestly | Report effect size, p-value, FDR-q, raw arrays. Let the agent decide |
| **Caller-supplied arbitrary code / DSL for scans** (Lua, embedded Python, etc.) | "Make scans pluggable from outside" | Massive complexity, sandboxing nightmare, and obviates the static scan registry / repro contract | Scans are first-class Rust modules; new scans land via PR |
| **Realtime alerting / push notifications** | "Tell me when a new anomaly appears" | Requires live data (anti-feature above) and a stateful watch loop | Out of scope; caller polls sweeps if they want this |

---

## Feature Dependencies

```
[Data-reader trait + Dukascopy reader]
        └── feeds ──> [1m → {15m,1h,1d} bar aggregator (deterministic, close-aligned)]
                              └── persisted into ──> [Aggregated-bar disk cache]
                                                              │
                                                              └── feeds ──> [Return primitives]
                                                                                    │
                                            ┌───────────────────────────────────────┼────────────────────────────────────────┐
                                            ▼                                       ▼                                        ▼
                       [Single-series anomaly scans]              [Cross-instrument scans]              [Time-of-day / seasonality scans]
                       (Ljung-Box, ADF, KPSS, VR, Hurst,          (rolling corr, lead-lag CCF,           (hour-of-day, DoW, session, EoM,
                        ARCH-LM, JB, drawdown, outliers,           Engle-Granger, Johansen, OU            event-window, bucket ANOVA)
                        rolling vol)                               half-life, basket divergence,
                                                                   Granger, corr-breakdown)
                                            │                                       │                                        │
                                            └───────────────────────┬───────────────┘                                        │
                                                                    ▼                                                        │
                                                [Findings schema: effect stats + raw arrays + repro envelope]  <─────────────┘
                                                                    │
                                            ┌───────────────────────┴────────────────────────┐
                                            ▼                                                ▼
                                  [Single-scan query API]                       [Sweep-manifest runner]
                                  (CLI / MCP / HTTP, one scan, one              (parallel fan-out, FDR adjustment,
                                   param set)                                    sweep-summary record, cancellation)
                                                                                                │
                                                                                                ▼
                                                                              [JSONL findings stream on stdout]

[Block / stationary bootstrap]  ──enhances──> [every scan that needs CIs]
[Phase-scramble / circular-shift null] ──enhances──> [every scan that needs a p-value on autocorrelated data]
[Change-point / regime detection] ──enhances──> [rolling correlation, rolling vol, lead-lag] (regimised sub-series)
[Side-channel raw-array storage] ──enhances──> [findings schema] (for large arrays)
[Memoised per-sweep intermediates] ──enhances──> [sweep runner] (wall-clock reduction)

[Chart-pattern matching]      ──conflicts──> [the entire statistical mission of the tool]
[Strategy generation]         ──conflicts──> [Quant agent's responsibility]
[Backtesting]                 ──conflicts──> [tradedesk's responsibility]
[Persistent results store]    ──conflicts──> [streaming / stateless contract]
```

### Dependency Notes

- **Everything depends on the aggregator.** A buggy or non-deterministic 1m→Nm aggregator silently corrupts every finding. This is the single most important correctness surface in v1; treat it with the most thorough property-based and golden-file testing.
- **Returns primitives are upstream of all scans.** Mis-specifying log vs simple returns, or mishandling overnight gaps, leaks into every result. One canonical implementation, well-tested.
- **Findings schema is upstream of every output path.** Versioning it from day 1 is mandatory; retrofitting a schema version onto persisted findings later is painful for the Quant agent.
- **Sweep runner depends on scan registry + reproducibility envelope.** A sweep that can't name and version its scans, or can't stamp findings with the data slice used, cannot be re-run or audited.
- **Cross-instrument scans depend on a time-alignment primitive** (aligning two instruments' bar series on common timestamps, handling unequal trading calendars). Cheap once you build it; impossible to skip cleanly.
- **Seasonality scans depend on a timezone / session model.** UTC is the natural internal representation; trading-session tagging is a layer above it. Both required.
- **Phase-randomisation / block-bootstrap nulls enhance many scans** rather than depending on them — they are a generic wrapper. Build them once, plug into many.

---

## MVP Definition

### Launch With (v1)

The minimum that makes the tool useful to the Quant agent for both query and sweep modes.

**Plumbing**
- [ ] Reader trait + Dukascopy zstd-CSV reader
- [ ] Deterministic 1m → {15m, 1h, 1d} aggregator, close-aligned, UTC, bid/ask-aware, gap-honest
- [ ] Aggregated-bar disk cache keyed by source+symbol+resolution+side
- [ ] Window / date-range / rolling-window selector
- [ ] Findings schema (JSON Schema or Rust types) with effect-stats + raw arrays + reproducibility envelope; versioned
- [ ] Scan registry (named + versioned scans)
- [ ] Parallel execution over (instrument × timeframe × window × param) grids
- [ ] JSONL findings stream on stdout; structured logs on stderr
- [ ] Single-scan query API on CLI, MCP, HTTP (thin wrappers over the library)
- [ ] Sweep-manifest runner (TOML/YAML manifest format, dry-run preview)

**Statistical-anomaly scans (single-series)**
- [ ] Return primitives (log, simple, overnight/intraday split)
- [ ] Summary stats (mean, std, skew, excess kurtosis, IQR)
- [ ] Rolling volatility + vol-of-vol
- [ ] Ljung-Box on returns and on squared returns
- [ ] ADF stationarity test
- [ ] KPSS stationarity test
- [ ] Variance ratio test (Lo-MacKinlay)
- [ ] ARCH-LM test
- [ ] Jarque-Bera normality test
- [ ] Outlier / extreme-return detection (z-score, modified-z / MAD)
- [ ] Drawdown profile

**Cross-instrument scans**
- [ ] Time-alignment primitive (inner-join on common timestamps; configurable for unequal calendars)
- [ ] Rolling Pearson + Spearman correlation
- [ ] Rolling beta / OLS regression
- [ ] Lead-lag cross-correlation function (CCF) over a lag grid
- [ ] Engle-Granger two-step cointegration test (pairs) + OU half-life of the residual

**Seasonality scans**
- [ ] Hour-of-day return / volatility profile
- [ ] Day-of-week effect
- [ ] Trading-session bucketing (Asia / London / NY / overlap) and session-conditional stats
- [ ] End-of-month / start-of-month effect
- [ ] One-way ANOVA / Kruskal-Wallis bucket-comparison test
- [ ] Caller-supplied event-time window stats (news-window placeholder)

**Statistical hygiene**
- [ ] Effect size alongside p-value on every scan that produces a p-value
- [ ] Benjamini-Hochberg FDR adjustment at the sweep level, emitted in a sweep-summary record
- [ ] Block / stationary bootstrap for CIs on autocorrelated series (callable from any scan)
- [ ] Phase-scrambled / circular-shift null distribution as a first-class option

**Why these are v1, not v1.x:**
The user explicitly named volatility regimes, mean-reversion, autocorrelation, distribution tests, outliers, rolling correlation, lead-lag, cointegration, basket divergence (deferred — Johansen is v1.x; basket via "divergence of one vs equal-weighted mean of N" is v1), hour-of-day, day-of-week, session, end-of-month, and news-window placeholders. v1 must cover the union of those.

### Add After Validation (v1.x)

Add once v1 is in the agent's hands and concrete gaps are felt.

- [ ] **Johansen cointegration test** (baskets of n ≥ 3) — trigger: agent asks for triplet / FX-cross-basket scans
- [ ] **Granger causality test** — trigger: agent wants a formal lead-lag p-value rather than just CCF
- [ ] **Hurst exponent / DFA** — trigger: agent wants a single-number persistence screen across many instruments
- [ ] **Correlation-breakdown detection** (rolling corr + change-point) — trigger: agent asks "when did this pair stop behaving"
- [ ] **Basket divergence with PCA-derived or equal-vol weights** (beyond the v1 equal-weighted baseline) — trigger: agent wants to scan against synthetic baskets
- [ ] **Anderson-Darling normality test** (complement to Jarque-Bera) — trigger: heavy-tail discrimination matters
- [ ] **PELT / CUSUM change-point detection** on returns and vol series — trigger: regime-bracketing requested
- [ ] **Memoised per-sweep intermediates** (in-memory arena) — trigger: profiling shows redundant intermediates dominate sweep wall-clock
- [ ] **Side-channel raw-array storage** (URI reference instead of inline) — trigger: JSONL findings get large enough to be annoying
- [ ] **Schema-introspection endpoint** (`miner scans` / `list_scans`) — trigger: agent maintainer asks for it

### Future Consideration (v2+)

Defer until the v1 / v1.x tool is being used in anger and the need is concrete.

- [ ] **Markov-switching variance / mean models** (Hamilton MS-AR) — heavy implementation, narrow use-case; defer until change-point detection has proven insufficient
- [ ] **GARCH / EGARCH model fitting** as a scan — useful but agent may prefer to do this itself; defer until clearly demanded
- [ ] **Bayesian online change-point detection** — refinement on PELT; defer until offline change-point is well-used
- [ ] **Wavelet / spectral seasonality decomposition** — interesting but speculative for FX; defer
- [ ] **PyO3 Python bindings** — already deferred in PROJECT.md
- [ ] **Tick-level scans** — already out-of-scope in PROJECT.md
- [ ] **Cross-asset-class regime scans** (e.g. FX vol vs equity vol) — agent can compose these from v1 primitives once it exists

---

## Feature Prioritization Matrix

| Feature | User Value | Implementation Cost | Priority |
|---------|------------|---------------------|----------|
| Aggregator + aggregated-bar cache | HIGH | MEDIUM | P1 |
| Reader trait + Dukascopy reader | HIGH | MEDIUM | P1 |
| Findings schema (versioned, effect+arrays+repro) | HIGH | MEDIUM | P1 |
| Scan registry + parameter validation | HIGH | LOW | P1 |
| Single-scan query API (CLI/MCP/HTTP) | HIGH | LOW | P1 |
| Sweep-manifest runner | HIGH | MEDIUM | P1 |
| Return primitives + summary stats | HIGH | LOW | P1 |
| Rolling volatility + vol-of-vol | HIGH | LOW | P1 |
| Ljung-Box (returns + squared returns) | HIGH | LOW | P1 |
| ADF + KPSS | HIGH | LOW | P1 |
| Variance ratio test | HIGH | LOW | P1 |
| ARCH-LM | HIGH | LOW | P1 |
| Jarque-Bera, outliers, drawdown | MEDIUM | LOW | P1 |
| Time-alignment primitive | HIGH | LOW | P1 |
| Rolling Pearson + Spearman correlation | HIGH | LOW | P1 |
| Rolling beta / OLS | HIGH | LOW | P1 |
| Lead-lag CCF | HIGH | MEDIUM | P1 |
| Engle-Granger + OU half-life | HIGH | MEDIUM | P1 |
| Hour-of-day, DoW, session, EoM | HIGH | LOW | P1 |
| Bucket ANOVA / Kruskal-Wallis | MEDIUM | LOW | P1 |
| Event-window placeholder | MEDIUM | LOW | P1 |
| Effect-size reporting alongside p-value | HIGH | LOW | P1 |
| BH-FDR sweep adjustment | HIGH | MEDIUM | P1 |
| Block / stationary bootstrap | HIGH | MEDIUM | P1 |
| Phase-scramble null | HIGH | MEDIUM | P1 |
| Johansen cointegration | MEDIUM | MEDIUM | P2 |
| Granger causality | MEDIUM | MEDIUM | P2 |
| Hurst / DFA | MEDIUM | MEDIUM | P2 |
| PELT change-point | MEDIUM | HIGH | P2 |
| Correlation-breakdown detection | MEDIUM | MEDIUM | P2 |
| Basket divergence (PCA / equal-vol weights) | MEDIUM | MEDIUM | P2 |
| Memoised per-sweep intermediates | MEDIUM | MEDIUM | P2 |
| Side-channel raw-array storage | LOW-MEDIUM | MEDIUM | P2 |
| Schema-introspection endpoint | MEDIUM | LOW | P2 |
| Markov-switching variance/mean | LOW | HIGH | P3 |
| GARCH fitting as a scan | LOW | HIGH | P3 |
| Bayesian online change-point | LOW | HIGH | P3 |

**Priority key:**
- P1: v1 launch — table-stakes for "the agent can actually use this"
- P2: v1.x — add once v1 is in the agent's hands and gaps are concrete
- P3: v2+ — defer until product-market fit on the discovery loop is proven

---

## Sweep Manifest — Recommended Shape

Not strictly a feature, but a concrete design choice the roadmap should commit to. Recommended manifest format (TOML chosen for ergonomics; YAML acceptable):

```toml
# sweeps/weekly-fx-anomalies.toml
name      = "weekly-fx-anomalies"
version   = "1"
cache_root = "/opt/tradedesk/marketdata"

[[scan]]
id         = "vr-test-mean-reversion"
scan       = "anomaly.variance_ratio@v1"
instruments = ["EURUSD", "GBPUSD", "USDJPY", "AUDUSD"]
timeframes = ["15m", "1h", "1d"]
sides      = ["bid"]

  [scan.window]
  start = "2022-01-01"
  end   = "2026-01-01"
  rolling = { length = "90d", step = "30d" }

  [scan.params]
  k       = [2, 4, 8, 16]
  side    = "bid"
  returns = "log"

  [scan.null]
  type   = "circular_shift"
  reps   = 1000
  seed   = 42

[[scan]]
id         = "lead-lag-fx-majors"
scan       = "cross.lead_lag_ccf@v1"
pairs      = [["EURUSD", "GBPUSD"], ["EURUSD", "USDCHF"]]
timeframes = ["15m", "1h"]
  [scan.params]
  lag_range = { min = -20, max = 20 }
  [scan.window]
  start = "2024-01-01"
  end   = "2026-01-01"
```

Key properties:
- Every scan reference is `name@version` — cannot accidentally drift.
- Every parameter is explicit or defaulted-then-echoed; nothing implicit.
- Windows are declarative (fixed or rolling); the runner expands them.
- The runner emits one findings record per (scan × instrument(s) × timeframe × window × param-point) plus one sweep-summary record at the end.

---

## Findings Schema — Recommended Skeleton

```json
{
  "schema_version": "1",
  "scan": "anomaly.variance_ratio",
  "scan_version": "1",
  "code_revision": "git:abcd123",
  "instrument": "EURUSD",
  "side": "bid",
  "timeframe": "1h",
  "window": { "start": "2024-01-01T00:00:00Z", "end": "2024-04-01T00:00:00Z", "bars": 1560 },
  "params": { "k": 4, "returns": "log" },
  "param_hash": "sha256:...",
  "effect": {
    "statistic": "VR(4)",
    "value": 1.42,
    "effect_size": { "name": "vr_minus_one", "value": 0.42 },
    "p_value": 0.013,
    "p_value_null": "circular_shift_1000_seed42",
    "ci_95": [1.11, 1.74]
  },
  "raw": {
    "returns": [/* ... */],
    "vr_per_k": { "2": 1.18, "4": 1.42, "8": 1.61, "16": 1.55 }
  },
  "notes": []
}
```

End-of-sweep summary record adds:
- `total_findings`
- `fdr_q_value` per finding id, BH-adjusted
- Top-N ranking by `|effect_size| × -log10(q)`

---

## Sources

Web/Context7 lookup tools were not available in this run. The methodology is drawn from canonical references:

- Tsay, R. — *Analysis of Financial Time Series* (3rd ed.) — chapters on returns, volatility models, unit-root tests, cointegration, MS models
- Lo, A. W. & MacKinlay, A. C. — *A Non-Random Walk Down Wall Street* — variance ratio test and mean-reversion methodology
- Hamilton, J. — *Time Series Analysis* — Johansen cointegration, Markov switching, ARCH/GARCH
- López de Prado, M. — *Advances in Financial Machine Learning* — multiple-testing, bootstrap-on-time-series, FDR in financial research
- Politis, D. & Romano, J. — stationary bootstrap (1994 JASA)
- Killick, R. et al. — PELT change-point algorithm (JASA 2012)
- Engle, R. — ARCH-LM test
- Andersen, T. & Bollerslev, T. — realised-volatility methodology
- Lakonishok, J. & Smidt, S. — calendar / end-of-month anomalies

PROJECT.md (in-repo) for scope, constraints, and explicit out-of-scope items.

**Confidence per area:**
- Statistical-anomaly scan menu: **HIGH** — these are textbook, no controversy on names or parameters
- Cross-instrument scan menu: **HIGH** for pairs (EG, CCF), **MEDIUM** for baskets (PCA-weight choice is a defensible default but several alternatives exist)
- Seasonality scan menu: **HIGH** for hour-of-day / DoW / session; **MEDIUM** for the right null-distribution choice (circular-shift recommended, but block-bootstrap is a reasonable alternative)
- Findings-schema choices and sweep-manifest format: **MEDIUM** — these are reasoned proposals, not externally validated; the roadmap author should treat them as starting points
- Anti-feature list: **HIGH** — all anti-features are directly supported by PROJECT.md's Out-of-Scope section or by clear separation of concerns with the Quant agent / `tradedesk`

---
*Feature research for: tradedesk-miner (quant data-mining engine)*
*Researched: 2026-05-15*
