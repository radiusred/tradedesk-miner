# Pitfalls Research

**Domain:** Rust quant data-mining engine (statistical scans over cached OHLCV bars)
**Researched:** 2026-05-15
**Confidence:** MEDIUM-HIGH (synthesis of well-documented quant-mining and Rust-engineering canon; web verification of MCP/crate currency was not available this session — flagged inline where it matters)

This document catalogues the failure modes that kill projects of this shape. It is opinionated. For every pitfall: warning signs (early detection), prevention (concrete), severity, and the roadmap phase that should own prevention.

**Severity legend:**
- **CRITICAL** — if missed, the project's findings are silently wrong or the project is structurally unfixable without rewriting
- **HIGH** — significantly degrades correctness, performance, or operability; expensive to retrofit
- **MEDIUM** — meaningful pain, but recoverable with targeted refactor
- **LOW** — minor friction, easy to fix when noticed

**Phase shorthand** (matches the typical greenfield roadmap shape):
- **P0 Foundations** — repo, types, errors, IO, error model, logging convention, findings schema
- **P1 Reader + Aggregator** — Dukascopy reader, deterministic 1m → 15m/1h/1d aggregation, derived-bar cache
- **P2 Scan Engine** — single-scan API, statistics primitives, parallel orchestration
- **P3 Scan Catalogue** — concrete scans (volatility, mean-reversion, autocorrelation, lead-lag, seasonality, cointegration)
- **P4 Wrappers** — CLI, MCP, HTTP surfaces
- **P5 Hardening** — multiple-testing controls, reproducibility audit, benchmarks, docs
- **Ongoing** — dependency hygiene, schema versioning, regression suite

---

## Critical Pitfalls

### Pitfall 1: Look-ahead bias in scan windows

**What goes wrong:**
A scan computes a statistic at bar `t` using information that wasn't yet available at `t`. The classic shapes: (a) using `close[t]` to label or filter a "signal at `t`" that a live trader would have only at `t+1` open, (b) computing rolling z-scores or means using a window that includes the bar being evaluated when the strategy interpretation is "act on prior info", (c) standardizing across the full historical series before slicing — leaking future moments into past windows, (d) gap-fill or forward-fill that propagates information backwards, (e) cross-instrument lead-lag where the "lag" candidate's bar timestamp is not strictly less than the "lead" candidate's.

**Why it happens:**
OHLCV gives you the future-in-a-row by construction. It is trivially easy to write `df.rolling(20).mean()` (or its Rust analogue) and have the window be inclusive of `t`. Standardization helpers (z-score, min-max) almost always default to whole-series statistics. Resampling 1m → 15m places the 15m bar's timestamp at the **start** of the interval in some conventions and at the **end** in others — if scans assume one and the aggregator does the other, every signal silently sees its own future.

**How to avoid:**
- Make the bar-timestamp convention a first-class, documented decision in P1 (recommend: **timestamp = interval START**, bar covers `[t, t+resolution)`, the bar is only fully "known" at `t+resolution`). Encode this in a `BarTimestamping` enum so it is unmissable.
- All scan primitives that produce a value "as of `t`" must use windows over bars whose **close time** `<= t`. Build a `WindowView` abstraction that enforces this — windows yield only fully-closed prior bars.
- Forbid in-place series-wide standardization in scan code. Provide `rolling_zscore(window, lookback)` primitives instead.
- For cross-instrument scans, require explicit `lag_bars >= 1` (and document that `lag=0` means "contemporaneous, not predictive").
- Write a "look-ahead detector" test: shuffle future bars to random noise after a cutoff point — every scan's statistic up to the cutoff must be **identical** to the unshuffled run. If it changes, you have leakage.

**Warning signs:**
- A scan reports unbelievable effect sizes (Sharpe > 3, hit-rates > 65% on naive setups, t-stats > 8).
- Effect strength is monotone with how recent the bar is.
- Findings disappear when the scan is re-run with `series[:-1]` instead of `series` (off-by-one in the window means the future-leak only mattered for the last bar).
- 15m/1h aggregates of identical 1m data produce wildly different findings.

**Phase to address:** **P1 (timestamp convention)** + **P2 (window primitives + leakage test)**. Severity: **CRITICAL**.

---

### Pitfall 2: Multiple-testing / data-snooping with unreported sweep cardinality

**What goes wrong:**
Sweep mode runs thousands of (instrument × timeframe × parameter × window) combinations and emits findings ranked by p-value or effect size. Any reasonable threshold (p < 0.05, t-stat > 3) will yield "significant" findings purely by chance at this sweep cardinality. The quant agent — or worse, a human — promotes noise to hypothesis. The Quant agent has no way to know whether `p=0.001` came from one targeted test or one of fifty thousand.

**Why it happens:**
Mining is exactly the activity classical statistics warns against. The default mental model from one-off backtesting ("p<0.05 is good") is wrong at sweep scale. It is socially uncomfortable to emit a finding annotated "you ran 47,000 tests, expect ~2,350 false positives at p<0.05."

**How to avoke:**
- **Every finding must carry the sweep context that produced it**: `tests_in_sweep`, `family_id` (the parameter grid it came from), `selection_rule` (e.g. "top-k by t-stat", "all p < 0.01"). Lock this in the findings envelope at P0 so it can never be omitted retroactively.
- Provide **at least two** built-in multiplicity corrections: Bonferroni (conservative) and Benjamini-Hochberg FDR (less conservative, more useful at sweep scale). Emit both an unadjusted and adjusted p-value or expected-FDR.
- For Sharpe-style effect sizes, implement **Deflated Sharpe Ratio** (Bailey & López de Prado) which adjusts for the number of trials and the variance of trial Sharpes. This is the de facto standard for quant mining and is far more honest than raw Sharpe.
- Distinguish **query mode** (caller specified one test, no deflation needed at miner level) from **sweep mode** (miner knows the cardinality and must annotate it). Make it impossible to run sweep mode without the deflation/adjustment metadata being attached.
- Cap the volume of "significant" findings emitted from a sweep by FDR target, not by raw p-value, so a sweep that finds nothing real emits nothing.

**Warning signs:**
- Sweep mode emits "significant" findings at roughly the rate you'd expect from pure noise at the chosen alpha (alpha × N).
- Replaying the same sweep on a shuffled-returns null dataset still produces "findings" at similar rates.
- The quant agent cannot reproduce a finding when it re-tests in isolation.

**Phase to address:** **P0 (findings envelope must accommodate adjustment fields)** + **P5 (DSR / FDR implementations and the noise-replay regression test)**. Severity: **CRITICAL**.

---

### Pitfall 3: Survivorship bias from cache contents

**What goes wrong:**
The 28-instrument working set is a list of instruments that *still exist and are still tracked*. Any historical scan implicitly conditions on "instrument survived to today and stayed in the cache." Cross-sectional scans (correlation breakdowns, basket divergence) and any volatility/drift scan on the survivor set will overstate stability and understate tail risk. Extreme cases: crypto pairs delisted by Dukascopy, exotic FX pairs dropped, instruments whose ticker symbol changed.

**Why it happens:**
The cache layout `<cache-root>/<SYMBOL>/...` has no concept of "instrument was tradeable from date X to date Y". Symbols that no longer exist simply have no recent files. Symbols that have been renamed appear as two separate, partial-history instruments. Scans iterate the directory listing and silently get whatever survived.

**How to avoid:**
- Reader trait should expose an `instrument_lifecycle(symbol) -> { first_bar, last_bar, gaps }` introspection. Surface in every finding.
- Findings must include `coverage_window` and `bar_count` for every instrument used — not just the requested scan window. A scan that requested 2020-2026 on an instrument with data 2023-2026 should self-flag (`coverage_mismatch=true`).
- For cross-sectional scans, the scan API must accept a survivorship-aware universe (e.g. "instruments with full coverage in `[from, to]`") and reject mixed-coverage universes unless explicit.
- Document this limitation prominently in the README and in MCP tool descriptions. Forex majors are mostly fine; crypto, exotic crosses, and indices are not.

**Warning signs:**
- A scan over "all instruments 2020-2026" runs against fewer instruments than requested with no warning.
- Crypto pairs in particular show absurdly favourable risk metrics.
- Adding a delisted instrument's historical CSVs back into the cache materially changes findings.

**Phase to address:** **P1 (lifecycle introspection on reader)** + **P3 (universe-construction utilities for cross-sectional scans)**. Severity: **HIGH** (CRITICAL for cross-sectional / basket scans).

---

### Pitfall 4: Stdout-as-findings collision with logs, progress, and crate chatter

**What goes wrong:**
The miner streams findings as JSON/JSONL to stdout. Anything else written to stdout — a `println!` left from debugging, a progress bar, a crate that helpfully logs to stdout, a `dbg!` macro, an unhandled panic message — silently corrupts the findings stream and may break the agent's JSON parser. MCP stdio transport has the same constraint by spec: stdout is the protocol channel.

**Why it happens:**
Rust's `println!` is the path of least resistance. `indicatif` defaults to stderr (good) but several logging crates and example snippets write to stdout. Worse, a Rust panic prints to stderr by default but a `process::exit(1)` after a `println!("error: ...")` writes the message to stdout.

**How to avoid:**
- **P0 hard rule, codified in CI**: `println!` and `print!` are forbidden outside of `src/bin/*` findings emission and a single `findings::emit` module. Enforce via a `clippy::disallowed_macros` lint config.
- All logging goes through `tracing` configured with a `tracing_subscriber` writer set to `std::io::stderr`. No default subscriber — the binary must initialise it explicitly so embedders can override.
- Progress reporting (if any) is opt-in, defaults off in non-TTY, and writes to stderr.
- The findings emitter is the **only** place writing to stdout. It writes one JSON object per line, flushes after each, and the writer is `BufWriter<Stdout>` with explicit flush on each finding (so a panic mid-sweep loses at most the in-flight object, not buffered findings).
- Add a CI test: pipe stdout through `jq -c .` for a fixed dataset. Every line must parse. Re-run with `RUST_BACKTRACE=1` to check panics route to stderr.
- For MCP, the same discipline applies but stricter — MCP uses stdout for the JSON-RPC envelope; any rogue stdout byte breaks the agent's transport.

**Warning signs:**
- Intermittent JSON parse failures on the consumer side.
- Findings stream "stalls" then bursts (a buffer fill from log noise).
- MCP client disconnects mid-call with parse errors.

**Phase to address:** **P0 (logging convention, lint rules)** + **P4 (MCP wrapper must inherit the same discipline)**. Severity: **CRITICAL** (project is unusable by agents if violated).

---

### Pitfall 5: Async runtime contamination of CPU-bound scan code

**What goes wrong:**
The HTTP and MCP wrappers are I/O-bound and use Tokio. Someone touches the scan engine to "fix a warning" and adds `async fn` or a `.await` somewhere in the scan path. Now the scan engine either (a) requires a Tokio runtime to be present (breaks CLI determinism, breaks Rayon parallelism interactions), or (b) blocks the Tokio reactor on CPU-bound work, starving wrapper I/O. Worse: a scan that internally `tokio::spawn`s yields back to the runtime mid-statistic, and parallel reductions become non-deterministic across runs because task scheduling order leaks into floating-point summation order.

**Why it happens:**
Rust's async ergonomics make it the default reach for "this function might be slow." Library authors get pressure to be "async-friendly". The async/sync split is invisible at API design time and painful to undo later.

**How to avoid:**
- **The scan engine crate is pure-sync.** No `tokio`, no `futures`, no `async-trait` in `tradedesk-miner-core`. This is a build-time guarantee: don't list async crates as dependencies.
- Parallelism inside the engine is **Rayon only**. Rayon thread pools are CPU-affined and deterministic given a fixed thread count and reduction strategy.
- Wrappers (CLI, MCP, HTTP) are separate crates that depend on core. They invoke core via `tokio::task::spawn_blocking` so the scan runs on a blocking-thread, not the async reactor.
- Make the scan engine `Send` but **not** require `Send` futures. The scan API returns iterators or channels (`std::sync::mpsc` or `crossbeam`), not `Stream`s. Wrappers adapt to async at their edge.
- Workspace structure: `crates/miner-core` (sync), `crates/miner-cli`, `crates/miner-mcp`, `crates/miner-http`. The `Cargo.toml` of `miner-core` must not pull tokio transitively. Add a CI check.

**Warning signs:**
- Scan throughput drops sharply when invoked over HTTP/MCP versus CLI.
- HTTP request handling stalls during large sweeps.
- Determinism tests pass under CLI but fail under HTTP (different reduction order).
- `tokio` shows up in `cargo tree -p miner-core`.

**Phase to address:** **P0 (workspace layout, dependency rules)** + **P2 (Rayon-based orchestration)** + **P4 (wrapper bridges via spawn_blocking)**. Severity: **CRITICAL** for performance, **HIGH** for determinism.

---

### Pitfall 6: Findings envelope schema breakage between versions

**What goes wrong:**
The first version emits `{ "scan": "lead_lag", "stat": 2.3, ... }`. v0.2 renames `stat` to `t_statistic`. v0.3 adds a required `selection_rule` field. The Quant agent has cached findings, in-flight prompts, and downstream code keyed by the old shape. Every change becomes a silent data-corruption event.

**Why it happens:**
Findings shape feels like an implementation detail early on. Renaming a field is a one-line change in Rust; the consequences are invisible from the producer side.

**How to avoid:**
- Lock the envelope **before** P3. Every finding object has, at minimum:
  - `schema_version` (semver string, hardcoded constant per release)
  - `scan_id` (stable identifier for the scan kind, never renamed)
  - `inputs` (struct: instruments, timeframe, window `[from, to]`, parameters)
  - `effect` (struct: statistic, p_value, n_observations, confidence_interval if applicable)
  - `multiplicity` (struct: tests_in_sweep, family_id, selection_rule, adjusted_p_value, dsr if Sharpe-flavoured)
  - `raw_arrays` (named arrays the quant agent needs to re-test independently)
  - `coverage` (struct: bar_count_per_instrument, coverage_window, gaps_count)
  - `provenance` (struct: miner_version, reader, cache_root_hash, scan_started_at, scan_duration_ms, rng_seed)
- Use a JSON Schema file checked into the repo. CI validates emitted findings against it.
- Schema changes follow semver: additive only on minor, breaking on major. Provide a migration tool for major bumps that re-shapes old findings forward.
- Internal Rust types are versioned modules (`v1::Finding`, `v2::Finding`) — serializers are explicit, not derived from a single struct.
- Snapshot-test the JSON output of a canonical scan run on each release.

**Warning signs:**
- Field name changes during code review without a version bump discussion.
- Optional fields proliferate (sign that the schema is being abused for "kind-of-extends").
- Consumers report "the new version returns less data" (silent field removal).

**Phase to address:** **P0 (envelope locked, JSON Schema written)** + **Ongoing (snapshot tests, semver discipline)**. Severity: **CRITICAL** (recoverable but expensive once consumers have built on a shape).

---

### Pitfall 7: Naive 1m → higher-resolution aggregation

**What goes wrong:**
"15m bar = open of first 1m, close of last 1m, max of highs, min of lows, sum of volumes" sounds obvious but several edge cases will silently produce wrong bars:

1. **Partial-bar contamination**: the source CSV starts at 01:07 (not 01:00) on Sunday session open. A 15m bar labelled 01:00 is built from 8 minutes of data, not 15. The bar's volatility is artificially compressed; volume looks low; the OHLC may even have a degenerate shape (open=high=low=close).
2. **Holiday/weekend gaps**: a 1h bar built across a gap concatenates Friday 21:55 with Sunday 22:00. The "close-to-close return" of that bar is an over-the-weekend return mislabelled as one hour.
3. **DST boundaries**: spring-forward and fall-back days have 23 or 25 hours. Hour-of-day scans see two "01:00 bars" on fall-back and silently double-count.
4. **Bar alignment**: 1h bars aligned to UTC midnight versus to NY session open versus to local exchange — every choice produces different bars from the same 1m data.
5. **Side handling**: bid and ask have independent 1m bars in the Dukascopy cache. Naively averaging `(bid + ask) / 2` minute-by-minute then aggregating is **not** the same as aggregating bid and ask separately then averaging. The "mid bar" from naive averaging has the wrong high/low because the mid's high is **not** `(bid.high + ask.high) / 2` (those highs happen at different sub-minute times).
6. **Volume**: Dukascopy publishes tick-count volume; summing across 15 minutes is fine arithmetically but the metric is "ticks Dukascopy saw", not "contracts" or "notional".

**Why it happens:**
The pseudo-code looks identical to the correct code. Failures are silent and bar-aligned to particular weekends/holidays — invisible unless you specifically test those days.

**How to avoid:**
- Define a `BarAggregator` trait with explicit semantics: `alignment` (UTC start-of-period only for v1; document the choice), `gap_policy` (drop incomplete leading/trailing bars, never silently emit partials), `dst_policy` (work entirely in UTC; reject scans that operate on local time without an explicit timezone parameter).
- A 15m bar is emitted **only** if it contains exactly 15 underlying 1m bars (or whatever the contract says — e.g. "at least N% coverage"). The default must be strict; a tolerant mode exists but requires explicit opt-in and records the coverage ratio on the bar.
- Bid and ask aggregate **independently**. A "mid" derivation is a separate, named operation downstream of aggregation, and it carries a flag warning that mid-OHLC is an approximation.
- Weekend/holiday gap detection is part of the aggregator output: produce a `Vec<Gap>` alongside the bar stream. Scans can choose to skip bars adjacent to gaps.
- Aggregation determinism: given the same 1m input, the output bars must be **byte-identical** across runs. Test this with a fixture.
- Snapshot-test edge dates: DST spring forward / fall back, year boundary, Christmas Day, instrument's first/last day in the cache.

**Warning signs:**
- 15m bars at session open look anomalous in every scan.
- Hour-of-day seasonality shows bizarre spikes at 00:00 and 01:00 on autumn weekends.
- 1h volatility on a Sunday open bar is 10× normal (weekend gap concatenation).
- Re-running aggregation produces slightly different bars (non-determinism — usually parallel HashMap iteration order).

**Phase to address:** **P1 (aggregator design + edge-case fixtures)**. Severity: **CRITICAL** (every downstream finding is built on these bars).

---

### Pitfall 8: Bid/ask handled as "just use mid" or "just use one side"

**What goes wrong:**
The Dukascopy cache stores bid and ask separately for a reason: the spread is real, and for many scans it dominates the effect. Common failures:

- Mean-reversion scans on mid prices look profitable until you realize every entry pays half the spread and every exit pays the other half. A 10-pip mean-reversion edge with 1.5-pip average spread is structurally unprofitable; the scan shouldn't be flagging it.
- Lead-lag and cointegration use one side (usually bid) for both instruments, ignoring that the executable trade is buy-at-ask, sell-at-bid. Asymmetric spread regimes (news, low liquidity hours) destroy the lag relationship in execution but not in the data.
- Volatility scans on mid prices systematically underestimate realised cost-of-trading volatility.
- Cross-instrument scans use bid for both — but during a USD shock, EURUSD bid and GBPUSD bid widen asymmetrically; ask spreads change differently. The "correlation breakdown" finding may just be a spread-widening event.

**Why it happens:**
Mid is the convenient single number. Every textbook formula assumes "price". The cost of spread is invisible until execution.

**How to avoid:**
- Every scan declares its price convention: `PriceSide::Bid | Ask | Mid | BidAskAware`. Default is **not** Mid — make the caller specify.
- For directional scans (mean-reversion, momentum), require `BidAskAware`: longs use ask for entry / bid for exit, shorts the opposite. The scan reports both gross (mid-mid) and net (bid-ask aware) effect statistics.
- Findings include the **average spread over the test window** as part of `coverage`. The quant agent can sanity-check whether the edge survives the spread.
- Document explicitly: cross-instrument scans use the same side for both legs (typically bid-to-bid), and the limitation is called out.
- For seasonality scans on spread itself (a perfectly reasonable scan), bid and ask must be loaded and the scan operates on `ask - bid`.

**Warning signs:**
- Mean-reversion findings with effect sizes smaller than the typical spread.
- Effect sizes that vanish when the scan is re-run with realistic transaction-cost adjustment.
- Cross-instrument findings concentrated in the Sunday-open or news-window hours (spread artifacts).

**Phase to address:** **P3 (scan catalogue; default conventions per scan kind)**. Severity: **HIGH** (findings are misleading rather than nonexistent).

---

### Pitfall 9: Timezone / DST / session-boundary handling on intraday bars

**What goes wrong:**
- Day-of-week scans bucketed by local time on UTC-stored bars produce wrong buckets twice a year (DST shift).
- "End-of-month" seasonality on local-calendar months when bars are UTC produces month-end findings concentrated on a different bar each year depending on how UTC midnight aligns with local midnight.
- "Session open" defined as "22:00 UTC" works for most of the year and is wrong by an hour for half of it. Conversely, defining it as "17:00 New York" requires loading a TZ database.
- Crypto runs 24/7; FX has a weekend gap; indices have local exchange hours; metals partially follow London/NY. A single "session model" doesn't apply.
- Bar timestamps with mixed conventions (some sources put 15m bars at start, others at end) cause time-of-day scans to be off by `resolution - 1m`.

**Why it happens:**
The cache stores UTC, which is the right choice, but "interesting" boundaries (NY close, end-of-month, day-of-week as humans live it) are local. Naively bucketing in UTC produces results that don't match what a human trader would describe.

**How to avoid:**
- Internal representation: UTC always, instants only, no naive datetimes. Use `chrono::DateTime<Utc>` or `jiff::Timestamp` (jiff is the more modern choice if its API matches; **verify currency** — confidence MEDIUM, web verification not available).
- Time-of-day / day-of-week scans accept a `Tz` parameter (IANA name) and do the conversion at bucketing time. Default to UTC, document the choice.
- Provide a `SessionModel` enum per instrument class: `Forex` (weekend gap 21:00 Friday UTC to 21:00 Sunday UTC, ish), `Crypto` (continuous), `Index` (per-exchange hours), `Unknown` (no session assumptions). Findings include which session model was assumed.
- DST handling: convert UTC → local for bucketing, accept that on DST shift days a local "01:00" hour may be skipped (spring) or doubled (fall) — emit a flag rather than silently doubling.
- End-of-month: define explicitly — last trading day of calendar month in TZ X, or last N bars of month. Both are reasonable; the choice is documented and per-scan.

**Warning signs:**
- Day-of-week seasonality with strong "Sunday" effect (probably the weekend-gap bar misbucketed).
- Time-of-day findings that shift by exactly one hour around DST dates.
- End-of-month findings that disappear in years where month-end fell on a weekend.

**Phase to address:** **P1 (time model, session models)** + **P3 (seasonality scans use them correctly)**. Severity: **HIGH**.

---

### Pitfall 10: Non-determinism in parallel reductions and RNG

**What goes wrong:**
Two runs of the same scan over the same data produce different findings — usually slightly different floating-point statistics on the order of 1e-12, occasionally meaningfully different rankings near a significance threshold. Causes:

1. **Parallel floating-point sums**: `a + b + c + d` evaluated as `(a+b) + (c+d)` vs `((a+b)+c)+d` produces different rounding. Rayon's `par_iter().sum()` uses tree-reduction with non-deterministic ordering across runs.
2. **HashMap iteration order**: collecting findings into a `HashMap<Symbol, Result>` and serializing iteration order gives different JSONL line ordering per run.
3. **RNG**: any scan that bootstraps, permutation-tests, or sub-samples needs a seeded RNG. Default `thread_rng()` is per-thread and non-reproducible across runs.
4. **File-system order**: `read_dir()` order is OS-dependent. Iterating instruments in directory order changes per filesystem.

This breaks diff-based testing, breaks the Quant agent's caching, breaks anyone trying to reproduce a finding.

**Why it happens:**
Defaults in Rust's stdlib and Rayon optimize for performance, not determinism. The cost is invisible until someone tries to reproduce.

**How to avoid:**
- Findings emitted in a **deterministic order**: sort by `(scan_id, instrument(s), timeframe, parameters_canonical_form)` before emission. The emit step is sequential even if computation is parallel.
- Use `BTreeMap` instead of `HashMap` where iteration order matters. Use `IndexMap` if insertion order is what's needed.
- Parallel reductions of floating-point statistics: define a canonical order (sorted by chunk index) and reduce in that order, **or** explicitly document and accept the bit-noise variation and quantize the emitted statistic to a defined precision (e.g. `f64` rounded to 12 significant figures). The former is preferred; the latter is acceptable for non-significance-threshold-adjacent statistics.
- RNG: every scan that uses randomness takes a `seed: u64` parameter. Default: derive from a hash of `(scan_id, inputs, miner_version)` so the same scan twice gives the same seed. The seed is recorded in `provenance.rng_seed` in every finding.
- Directory iteration: collect entries, sort by symbol, iterate.
- Regression test: a "golden" findings file. CI runs the scan, diffs the output JSONL byte-for-byte against the golden. Any drift fails CI.

**Warning signs:**
- `git diff` on the golden findings file shows only floating-point noise — but on every commit.
- Findings file ordering changes between runs.
- A finding "vanishes" intermittently because its p-value crossed the threshold one way one run and the other way the next.

**Phase to address:** **P2 (deterministic orchestration)** + **P5 (golden-file regression test)**. Severity: **HIGH** (CRITICAL if downstream consumers diff findings).

---

### Pitfall 11: Hardcoded paths, environment assumptions, and RadiusRed-specific defaults

**What goes wrong:**
The library compiles with `const CACHE_ROOT: &str = "/opt/tradedesk/marketdata"` because that's the dev machine. An open-source user clones the repo, runs the CLI, gets a panic. Or worse, the path is read from `$TRADEDESK_CACHE_ROOT` env var with no fallback — the CLI works for the maintainer and is silently broken for everyone else.

Other shapes: assuming the bid/ask suffix is exactly `_bid.csv.zst`, assuming month folders are 00-indexed (the Dukascopy convention, weird to outsiders), assuming UTC always, assuming the instrument symbol shape (`EURUSD` vs `EUR/USD`).

**Why it happens:**
Defaults that match the maintainer's environment get baked in because tests pass on the maintainer's machine.

**How to avoid:**
- **Zero hardcoded paths** anywhere in the library crate. The reader trait takes a `cache_root: PathBuf` and a configuration struct.
- CLI surfaces config in this priority order: command-line flag > env var > config file (`~/.config/tradedesk-miner/config.toml` on Linux, platform-appropriate elsewhere — use `directories` crate) > error with a clear message. Never a silent default.
- The Dukascopy reader is a concrete impl named `DukascopyZstdReader`, documented to expect the `tradedesk-dukascopy` cache layout. Document the exact layout in the README; document that the 00-indexed months are a Dukascopy quirk, not a miner choice.
- Reader trait permits other layouts. Ship at least one second reader (even if just an in-memory test reader) so the trait isn't accidentally fitted to Dukascopy.
- Integration tests run against a tiny fixture cache checked into the repo, not against `/opt/tradedesk/marketdata`.
- README "Quick start" works on a fresh checkout with the fixture cache. If it doesn't, the project isn't open-source-friendly yet.

**Warning signs:**
- "Works on my machine" issues.
- A `TODO: make this configurable` comment near a path string.
- The README requires a Dukascopy download before any example works.

**Phase to address:** **P0 (config model, env/flag/file precedence)** + **P1 (reader trait, fixture cache)**. Severity: **HIGH** (blocks open-source adoption).

---

### Pitfall 12: Unclear MCP tool descriptions and unvalidated HTTP inputs

**What goes wrong:**
An MCP tool described as `"run a scan"` with parameters `{ "scan": "string", "params": "object" }` is unusable by an LLM agent. The agent will guess scan names, guess parameter shapes, guess timeframe strings, and the miner will either reject calls (frustrating) or accept malformed calls and produce nonsense findings (worse).

HTTP version: an endpoint that accepts unbounded `instruments: Vec<String>` and `lookback_bars: u64` is a DoS vector when invoked by an over-eager agent — a scan over 28 instruments × 6 years × 1m bars × a parameter grid is a multi-hour operation. Without validation/limits, a single careless call locks the miner.

**Why it happens:**
MCP descriptions are written like docstrings for humans, not affordances for LLMs. HTTP APIs intended for "trusted internal use" skip validation because the consumer is friendly.

**How to avoid:**
- **MCP tool descriptions are written for an LLM.** Per tool: one paragraph of purpose, one explicit enumeration of allowed parameter values, one example call, one example response shape. Constrain parameters with JSON Schema `enum`s where possible (`timeframe: enum("15m","1h","1d")` not `string`). Avoid free-form `params: object` — define per-scan tools (`tool: scan_lead_lag`, `tool: scan_mean_reversion`) with concrete typed parameters.
- The MCP tool catalogue is a build artefact derived from the same Rust types the scan engine uses — no hand-maintained duplicate. (This forces the type system to enforce description completeness.)
- HTTP API mirrors the MCP catalogue: same parameter shapes, same enums, validated with `serde` + a `validator` crate or hand-written guards.
- Hard limits on every quantitative parameter: max instruments per call, max bar count, max sweep cardinality, max parameter grid size. Default limits should be generous for interactive use, refusable past a threshold with a clear error message ("would scan 4.2B bars, limit 100M — narrow window or request batch mode").
- Long-running scans get an explicit batch mode that returns a job id rather than blocking the HTTP/MCP call. (Or: stream findings as they arrive; consumers can cancel.)
- Validation errors are structured (machine-parseable) so the agent can self-correct: `{ "error": "invalid_parameter", "parameter": "timeframe", "got": "1m", "allowed": ["15m","1h","1d"] }`.

**Warning signs:**
- LLM agent makes the same kind of malformed call repeatedly even with examples.
- MCP tool descriptions exceed 200 words (a sign of poor parameter constraints — the description is trying to compensate).
- Single agent call ties up the miner for hours.

**Phase to address:** **P4 (wrappers, validation, MCP tool design)**. Severity: **HIGH**.

---

## Major (but non-critical) Pitfalls

### Pitfall 13: Stationarity assumptions and regime change

**What goes wrong:**
Cointegration tests (Engle-Granger, Johansen) and many mean-reversion scans assume the underlying series is stationary or that the relationship is. Six years of data (2020-2026) spans COVID, ZIRP/post-ZIRP, post-COVID inflation, multiple regime shifts in FX correlation structure, crypto's repeated regime breaks. A cointegration coefficient that holds on average over the full window may not hold in any sub-window — and the average is the misleading number.

**Why it happens:**
Single-window cointegration tests are textbook. Rolling cointegration / regime-aware variants are harder to implement and harder to interpret.

**How to avoid:**
- For any "is X cointegrated with Y" scan, also produce a rolling test (e.g. 6-month windows stepped monthly) and emit `pct_windows_cointegrated` alongside the full-window result. A finding where the full-window test passes but only 30% of sub-windows pass is reported as such.
- Provide explicit "regime segmentation" inputs to mean-reversion scans (e.g. break the series at named event dates or by detected volatility regime). Document the segmentation method used.
- Findings should include the test windows used so the quant agent can decide whether to trust the aggregate.
- Document: "miner reports statistical relationships in historical data. Stability of those relationships across regimes is the consumer's concern."

**Warning signs:** Cointegration findings that are "strongly significant" over 6 years but no quant strategy ever survives out-of-sample. Effect sizes that swing wildly between sub-windows.

**Phase to address:** **P3 (scan catalogue design)**. Severity: **MEDIUM** (limitation rather than wrongness; honest reporting mitigates).

---

### Pitfall 14: Dukascopy tick-derived volume is noisy and not "volume"

**What goes wrong:**
Dukascopy's volume column is **tick count** — number of price updates Dukascopy's aggregator received in the minute. It is not contract volume, not notional, not a market-wide figure (Dukascopy is a retail FX broker, not an exchange). Treating it as a true volume signal produces findings that reflect Dukascopy's connectivity, not market activity. Worse: tick volume correlates with price volatility (more ticks during fast moves), so a "volume predicts return volatility" finding may just be tautology.

**Why it happens:**
The column is labelled `volume`. Standard volume-based indicators (OBV, VWAP) are obvious to apply. The provenance of the number is not on the bar.

**How to avoid:**
- Reader should expose volume as `tick_count` in the bar struct, not `volume`. The name forces the consumer to think about what it means.
- Scans that use volume should accept a `volume_semantics: TickCount | Notional | Unknown` parameter; for the Dukascopy reader, default is `TickCount`. Volume-based statistical findings should annotate this.
- For FX, prefer scans that don't depend on volume at all. Volume-conditioned scans should be marked experimental.
- Document the limitation prominently. The README should be explicit: "Dukascopy tick-volume is not exchange volume. Findings using it should be interpreted with care."

**Warning signs:** Volume-based findings that disappear when re-tested on exchange-sourced data. "Volume spikes predict moves" findings with absurdly strong t-stats (this is the volatility-tautology).

**Phase to address:** **P1 (reader API naming)** + **P3 (scan documentation)**. Severity: **MEDIUM**.

---

### Pitfall 15: Parallel I/O thrashing on spinning disks; serial Zstd as the bottleneck

**What goes wrong:**
The naive parallelisation strategy is "fan out over instruments, read CSVs in parallel". On an SSD this works. On a spinning disk (still common for archival quant data), N parallel reads cause head seeking and the aggregate throughput drops below single-threaded read speed. Conversely, even on SSD, single-threaded Zstd decompression of large CSVs is CPU-bound and becomes the bottleneck — the disk is idle while one core decompresses.

The wrong-but-common configuration: read parallelism = scan parallelism = number of CPU cores. Either disk thrashes or decompression cores idle.

**Why it happens:**
Rayon's default thread pool size matches CPU count. Tying I/O parallelism to it is the default mental model.

**How to avoid:**
- Separate I/O pool from compute pool. Two Rayon thread pools (or a small I/O pool + the default compute pool):
  - I/O pool size: 2-4 (configurable, defaults sensible per platform). Just enough to keep the queue full without thrashing.
  - Compute pool size: `num_cpus::get()`.
- Pipeline stages: I/O → decompress → parse → aggregate → scan. Decompress and parse can be on the compute pool (CPU-bound). I/O is gated.
- Benchmark on both SSD and HDD-like throughput (`fio` or synthetic). Don't tune to one.
- The aggregated-bar cache is the real performance win — once a (symbol, timeframe, side) is aggregated, scans hit the small derived cache, not the 1m CSV. Make sure the cache is invalidated correctly when source CSVs change (mtime + size hash on the source files).
- Consider memory-mapping the aggregated cache files (which are small) but not the source CSVs (compressed, so mmap doesn't help).

**Warning signs:** Scan throughput doesn't scale with core count; `iostat` shows disk pegged at low MB/s with high queue depth; `top` shows one core at 100% (Zstd) while others idle.

**Phase to address:** **P1 (reader architecture, derived-bar cache)** + **P2 (orchestration pools)** + **P5 (benchmarks)**. Severity: **MEDIUM** (performance, not correctness).

---

### Pitfall 16: Memory blowup from slurping all bars at once

**What goes wrong:**
"Load all 1m bars for all 28 instruments for 6 years into a `Vec<Bar>`" is ~28 × 6 × 365 × 1440 × bid+ask × ~64 bytes/bar ≈ 11 GB in memory before aggregation. Fine on a 64 GB workstation, fatal on a CI runner or a modest VM. Worse, sweep mode that holds the raw bars in memory while iterating parameter grids never releases them.

**Why it happens:**
Idiomatic Rust APIs return `Vec<T>` everywhere. Streaming/iterator APIs require more thought.

**How to avoid:**
- Reader API returns `impl Iterator<Item = Result<Bar>>` (or a typed `BarStream`), not `Vec<Bar>`. Callers consume bars as a stream.
- The aggregator is a state machine consuming a 1m bar stream and emitting an aggregated bar stream. Constant memory per (symbol, side).
- The derived-bar cache stores aggregated bars on disk (small — 15m bars over 6 years per symbol per side ≈ 200k bars × 64 bytes ≈ 12 MB). Scans operate on the derived cache and can hold those in memory.
- Sweep mode reuses loaded derived-cache bars across parameter combinations — load once, sweep N parameter values over the in-memory array. **Do not** reload per parameter.
- Document the memory model. Provide a `MemoryBudget` knob if scans need to operate on more than the derived cache.

**Warning signs:** OOM on the dev machine running a "small" scan; memory usage scales linearly with scan window size rather than being roughly constant; `cargo test` runs out of memory in CI.

**Phase to address:** **P1 (reader is streaming)** + **P2 (scan engine consumes streams)**. Severity: **MEDIUM**.

---

### Pitfall 17: Hidden allocations in tight loops

**What goes wrong:**
A scan that allocates a `Vec<f64>` per bar, or a `String` per finding, or a `HashMap` per parameter combination, will spend most of its CPU in the allocator. Rust makes allocations easy to write and easy to miss. Common shapes:

- `format!()` inside a loop to build a log/debug string.
- `.collect::<Vec<_>>()` materialising an intermediate that could be a chained iterator.
- Cloning a `BarSlice` (small but per-bar) instead of borrowing.
- Re-allocating a rolling-window buffer per call rather than reusing one.

**Why it happens:**
Allocations are invisible. The code that says `vec.clone()` doesn't look slow.

**How to avoid:**
- Profile under `cargo flamegraph` or `samply` before optimising blindly; let the profiler point at real hot spots.
- For known-hot paths (rolling windows, statistic accumulators), use stack-allocated arrays or pre-allocated reusable buffers. `smallvec` for typically-small dynamic-size containers.
- Track allocations in benchmarks with `dhat-rs` or `jemalloc` stats. A scan over a fixed dataset should have a roughly fixed allocation count regardless of input size after warm-up.
- Lints: `clippy::needless_collect`, `clippy::redundant_clone`. Run on CI.
- For finding emission, reuse a single `serde_json::Value` or pre-sized `String` buffer; flush per-finding to stdout.

**Warning signs:** Scan throughput degrades over time (allocator fragmentation); flamegraphs show `alloc::vec` or `malloc` as dominant; benchmarks have high variance run-to-run.

**Phase to address:** **P2 (statistic primitives)** + **P5 (profiling pass before v1)**. Severity: **MEDIUM**.

---

### Pitfall 18: Float comparison and precision on long-window statistics

**What goes wrong:**
A 6-year 1m series is ~3M data points. Naive sum-of-squares variance over 3M values loses precision badly — Welford's online algorithm is the standard answer, but the naive `(sum_sq / n) - (sum / n)^2` formulation is what most quick implementations reach for. Same for sample covariance, correlation, regression slopes. Errors of 1e-6 in a t-statistic might cross significance thresholds for marginal findings.

Additional traps:
- `f64::EPSILON` is per-step, not cumulative — long sums accumulate well above EPSILON.
- Direct equality comparison (`a == b`) on computed floats is almost always wrong.
- NaN propagation through statistics silently produces NaN findings rather than failing loudly.

**Why it happens:**
The naive formulas are in every statistics 101 textbook.

**How to avoid:**
- Use Welford's algorithm (or Chan's parallel variant for parallel reductions) for variance/std/covariance/correlation. The `statrs` crate (verify currency — confidence MEDIUM, web verification not available) may provide these; if not, hand-roll.
- For correlation/regression, prefer numerically-stable updating formulas over batch sums.
- Define an `Approximately` comparison helper with absolute and relative tolerance for tests. Never `==` on floats.
- NaN handling is explicit: scans choose to skip NaN bars (and report `nan_count` in coverage) or to fail. Never silently produce NaN findings.
- For statistics that go into significance thresholds, ensure the chosen algorithm has known error bounds at the input size (3M samples is the design point).

**Warning signs:** Variance computed two ways differs in the 4th significant figure; rerunning a scan gives different floats but the "stats are roughly the same" handwave; NaN appears in findings.

**Phase to address:** **P2 (statistics primitives)**. Severity: **MEDIUM** (HIGH if findings near thresholds).

---

### Pitfall 19: Crate version churn and unmaintained dependencies

**What goes wrong:**
The Rust ecosystem moves fast and some categories (date/time, async runtimes, CSV parsing, statistics) have crates that have been overtaken or abandoned. Picking the wrong crate in P0 means a painful migration later or living with security advisories.

Specific areas where my training-data confidence is MEDIUM and currency matters:
- **Date/time**: `chrono` is the long-standing default; `jiff` is newer and may be the better choice for new projects. **Verify which is current** before committing.
- **CSV**: `csv` crate is stable; performance-critical alternatives (`arrow-csv`, custom SIMD parsers) may be relevant.
- **Compression**: `zstd` crate wraps libzstd; `ruzstd` is pure-Rust. **Verify** which has the best parallel-decode story.
- **Statistics**: `statrs` for distributions; specific algorithms (Welford, ADF, KPSS for stationarity, Johansen for cointegration) may need to be hand-rolled — there is no comprehensive numerical stats crate that matches scipy/statsmodels.
- **Parallelism**: Rayon is the default and stable.
- **MCP**: the official MCP Rust SDK exists; **verify** crate name, version, maturity, and stdio-transport correctness. Confidence LOW without web verification.
- **HTTP**: `axum` is the mainstream choice on top of `tokio`.

**Why it happens:**
"Whatever the first README recommends" is the default selection criterion.

**How to avoid:**
- Before locking each major dependency in P0, **explicitly verify** (web search or crates.io) that it is actively maintained, has recent releases (last 6 months), and matches the project's needs. Note the verification date.
- Prefer crates with multiple maintainers, > 1M downloads, and a clear changelog.
- For numerics that don't have a mature crate (cointegration tests, FDR), expect to hand-roll and test against scipy/statsmodels golden outputs.
- Avoid the bleeding edge unless the alternative is clearly worse. v1.x stable is preferable to v0.x even if v0.x looks more modern.
- `cargo audit` and `cargo deny` in CI.
- Keep direct dependencies minimal. Every transitive dep is a future migration.

**Warning signs:** `cargo audit` flags advisories; a key crate's last release is > 18 months old; the crate's GitHub has stale issues from a year ago; build breaks on a `cargo update`.

**Phase to address:** **P0 (initial selection)** + **Ongoing (audit, periodic review)**. Severity: **MEDIUM**.

---

## Technical Debt Patterns

| Shortcut | Immediate Benefit | Long-term Cost | When Acceptable |
|----------|-------------------|----------------|-----------------|
| Skip multiplicity correction in v1 ("we'll add it later") | Faster v1 ship | Every finding emitted before correction lands is misleading; quant agent builds bad heuristics on raw p-values | **Never** — the schema must accommodate adjustment fields from day one even if values are null in v1 |
| Hardcode UTC; defer timezone handling | Faster aggregator | Every seasonality scan emits subtly wrong findings until fixed; retrofitting timezone-aware bucketing is invasive | Acceptable if seasonality scans are P3+ and time-of-day scans default to UTC with a clear "TZ support coming" notice |
| Mid-only price handling in v1 | Half the scan code | Mean-reversion findings are wrong; spread-aware retrofit touches every scan | Acceptable only if scans default to "execution cost not modelled" warning in every finding |
| Single-threaded scan engine in v1 | Simpler code | Throughput is 1/N — the project's *core value* | **Never** — parallelism is what justifies Rust; ship with Rayon from day one |
| Store findings in a HashMap before emitting | Simpler accumulation | Non-deterministic ordering; breaks diff testing | Acceptable if a sort-before-emit step is added before any consumer relies on order |
| Skip the JSON Schema for findings | One less artifact to maintain | Consumers can't validate; schema drifts; "what fields exist" becomes oral tradition | **Never** — write the schema before P3, generate types from it where possible |
| Ship MCP wrapper without per-scan tools (one `run_scan` tool with `params: object`) | Reflects only the catalog growing | LLM agents make malformed calls; descriptions can't constrain | **Never** — per-scan tools or per-family tools, with typed parameters |
| Defer survivorship-aware universe construction | Cross-sectional scans ship sooner | Findings overstate cross-asset effects systematically | Acceptable for single-instrument scans; **never** for cross-sectional |
| Use `thread_rng()` for the one bootstrap scan | One fewer parameter | Findings non-reproducible; no `provenance.rng_seed` | **Never** — seeded RNG with default-derived-from-inputs is roughly the same code size |
| Skip the derived-bar cache, recompute each time | Skip a P1 deliverable | Every sweep is 10-100× slower; agent UX is bad | Acceptable for v0; **never** for v1 |

## Integration Gotchas

| Integration | Common Mistake | Correct Approach |
|-------------|----------------|------------------|
| `tradedesk-dukascopy` cache | Assuming month folders are 1-indexed | Document 00-indexed quirk; reader handles it; tests cover December boundary |
| MCP stdio transport | Mixing logs into stdout; using async-trait that leaks tokio into core | Logs to stderr only; MCP wrapper is a separate crate that bridges sync core to async transport via `spawn_blocking` |
| HTTP API (Axum/similar) | No request size limits; no rate limits; no auth on internal endpoints | Tower middleware for limits; even "internal" needs basic auth or network-level isolation; document the trust model |
| Quant agent (remote consumer) | Findings shape evolves between versions; consumer breaks silently | `schema_version` on every finding; JSON Schema in repo; semver discipline |
| `cargo` workspace | Async dep transitively pulled into `miner-core` via a "convenient" sub-crate | CI check on `cargo tree -p miner-core` rejecting `tokio`/`futures` |
| Cache invalidation | Source CSVs updated, derived-bar cache stale, scan returns old data silently | Derived cache keyed by `(symbol, timeframe, side, source_mtime_or_hash)`; mismatch triggers rebuild |
| OS file system | Case-sensitivity (Linux/macOS-APFS differ); path separators; long paths on Windows | Use `Path`/`PathBuf` only; test on at least Linux + macOS in CI; document Windows support level |

## Performance Traps

| Trap | Symptoms | Prevention | When It Breaks |
|------|----------|------------|----------------|
| Reloading 1m bars per parameter sweep | Throughput scales with N (params) × K (bars) | Aggregate once, sweep over derived bars | At ~10 parameter combinations on full history |
| Parallel I/O on HDD | Disk pegged, aggregate throughput < single-thread | Bounded I/O pool (2-4 threads) separate from compute pool | Always on HDD; rarely on SSD |
| Serial Zstd decompression | Single core at 100%, others idle | Decompress on the compute pool; one file at a time per I/O thread but parallel decompression of buffered files | Around 5-10 instruments in parallel scan |
| HashMap iteration in hot path | Non-deterministic order; slow vs BTreeMap on small N | BTreeMap for ordered iteration; FxHashMap for performance on large N; benchmark | Hot when N > 1000 |
| Allocation in `format!`/`to_string` per bar | Throughput degrades; allocator pressure visible in flamegraph | Pre-allocated buffers; structured logging that doesn't format unless emitted | At ~100k bars/sec target |
| String-keyed parameter map | Repeated hashing; allocations on key lookup | Enum or struct typed parameters; string-keyed only at the wrapper boundary | At sweep cardinality > 10k |
| No derived-bar cache | Every scan re-aggregates from 1m | Persistent derived cache, content-addressed by source state | Immediately on second sweep |
| Findings buffered without flush | Crash mid-sweep loses all output; downstream consumer thinks scan is hung | Flush after every finding; line-buffered stdout | On crash or long scans |

## Security Mistakes

| Mistake | Risk | Prevention |
|---------|------|------------|
| HTTP API with no auth on the assumption it's internal | An LLM agent leaked into the wrong network calls scans, exfiltrates the cache contents via findings | Bearer-token auth from day one (even simple shared-secret); network-level isolation; document trust model |
| Unbounded request parameters in HTTP/MCP | DoS via a scan with 10⁹ parameter combinations | Hard upper limits on instruments, bars, parameter grid size; reject before scheduling |
| Logging includes file paths in findings | Reveals server filesystem layout to a remote agent (which may be sandboxed or hostile) | Strip absolute paths from logged messages; never include cache_root verbatim in findings (use `cache_root_hash`) |
| Reader follows symlinks out of cache root | A malicious cache layout exfiltrates arbitrary files | Canonicalize and verify all paths are within `cache_root`; reject symlinks crossing the boundary |
| CSV parser vulnerable to malformed input | Crafted CSV crashes miner or causes OOM | Use a well-maintained CSV crate with bounded line/field limits; fuzz the reader |
| Zstd decompression bomb (compression ratio attack) | Tiny `.zst` file expands to GBs, OOMs the process | Bounded decompress (cap output size per file); reject suspiciously high ratios |
| Findings include raw arrays of potentially-licensed data | Source data licensing (Dukascopy ToS) violated when raw arrays are emitted to third parties | Document the licensing implications; provide an opt-out for raw-array emission for distributing findings publicly |

## UX Pitfalls (agent-operability)

| Pitfall | User Impact | Better Approach |
|---------|-------------|-----------------|
| Single `run_scan` MCP tool with free-form `params` | LLM agents guess parameter shapes; high failure rate | Per-scan-kind tools with typed parameters; LLM picks the tool by name |
| Vague tool descriptions ("runs a scan") | Agent can't decide which tool to call | Description includes purpose, when to use vs alternatives, example invocation |
| Errors as plain strings | Agent can't programmatically distinguish errors; tries again identically | Structured errors with codes (`invalid_parameter`, `coverage_gap`, `sweep_too_large`) and machine-readable fields |
| Findings buried in noise (one finding per page of logs) | Agent context window fills with logs not findings | Logs to stderr; stdout is findings only; findings are dense, no whitespace, JSONL |
| No streaming on long scans | Agent waits 30 minutes with no feedback, hits its own timeout | Emit findings as they arrive; emit a `progress` envelope periodically (also as JSONL with a `kind` discriminant) |
| No way to cancel a sweep | Agent that started a too-large sweep is stuck | Cancellation token in the scan API; HTTP endpoint to abort by job id |
| Findings reference internal Rust paths or types | Agent shown enum discriminants like `BarSide::Bid` it doesn't have context for | Findings use string enums (`"side": "bid"`) and document allowed values in the schema |
| No examples in tool descriptions | Agent invents call shapes | Every tool description includes at least one realistic example call |

## "Looks Done But Isn't" Checklist

- [ ] **Aggregator:** Often missing — DST edge-case test, partial-bar handling at session boundaries, byte-identical re-run determinism. Verify: run on a fixture covering at least one DST transition and one weekend gap; diff outputs across two runs.
- [ ] **Findings emission:** Often missing — line-flushing after each emit, deterministic ordering, schema_version field. Verify: pipe through `jq -c .`, run twice and `diff`.
- [ ] **MCP wrapper:** Often missing — stderr-only logging, typed per-scan tools, structured error responses. Verify: connect with `claude-code` or a manual JSON-RPC client and confirm no stdout pollution outside protocol envelope.
- [ ] **Reader:** Often missing — survivorship coverage flag, lifecycle introspection, symlink rejection. Verify: integration test with a fixture cache containing a deliberately-truncated instrument.
- [ ] **Sweep mode:** Often missing — `tests_in_sweep` populated, adjusted p-values, FDR/DSR fields. Verify: run a known-noise dataset; sweep mode should emit ~zero findings at the configured FDR threshold.
- [ ] **Determinism:** Often missing — RNG seed in provenance, sorted iteration of directories, golden-file regression test. Verify: same scan twice → byte-identical findings.
- [ ] **Configuration:** Often missing — no hardcoded paths, clear flag/env/file precedence, error messages naming the missing config. Verify: clone repo to a fresh user, run the README's quickstart, see if it works.
- [ ] **Async hygiene:** Often missing — `miner-core` has no async deps. Verify: `cargo tree -p miner-core | grep -E '(tokio|async)'` returns empty.
- [ ] **Time handling:** Often missing — UTC-only internally, explicit TZ at bucketing boundaries, session models. Verify: time-of-day scan run with two different TZ parameters gives sensibly-shifted buckets.
- [ ] **Bid/ask:** Often missing — every scan declares its price convention; spread is in coverage. Verify: pick a scan, change `side: bid` to `side: ask`, confirm finding changes (not silently identical).
- [ ] **Cache invalidation:** Often missing — derived-bar cache rebuilds when source changes. Verify: touch a source CSV, run scan, confirm rebuild log; revert, re-run, confirm cache hit.
- [ ] **Documentation:** Often missing — Dukascopy quirks (00-indexed months, tick-volume not contract-volume), data licensing implications. Verify: README has a "data source caveats" section.

## Recovery Strategies

| Pitfall | Recovery Cost | Recovery Steps |
|---------|---------------|----------------|
| Look-ahead bias discovered after findings shipped | **HIGH** | Add the look-ahead detector test, run it against every existing scan, identify affected scans, mark all findings from affected versions as invalid, increment major schema version, re-emit findings. |
| Multiple-testing un-accounted for | **MEDIUM** | Add multiplicity metadata to schema (additive minor bump), backfill `tests_in_sweep` for past sweeps if recoverable, communicate to consumers; future sweeps emit adjusted values. |
| Stdout polluted by stray println | **LOW** | Add CI lint (`clippy::disallowed_macros`), grep codebase, route remaining writes through `tracing` to stderr. |
| Async leaked into core | **HIGH** | Workspace restructure: extract async pieces to wrapper crates; refactor scan API to sync iterators/channels; wrappers re-add async at the edge. Painful but mechanical. |
| Aggregator non-deterministic | **MEDIUM** | Identify source (HashMap, parallel reduce, thread-RNG); replace with ordered alternatives; add golden-file test; rebuild derived cache. |
| Survivorship-biased universe used in cross-sectional scan | **MEDIUM** | Add coverage check + `coverage_mismatch` flag; re-run affected scans with explicit universe construction; flag old findings as `coverage_unknown`. |
| Schema breaking change shipped without bump | **HIGH** | Tag the prior release, emit a "deprecated/broken" advisory, ship a migration script, semver major bump for the next release, communicate widely. |
| Hardcoded path in `main` | **LOW** | Move to config; add flag/env/file precedence; CI test on a fresh checkout. |
| Volume findings shown to be tautology | **LOW** | Mark volume-based scans experimental; emit warning in findings; consider removing if no legitimate use case. |
| Float precision causes flaky threshold crossing | **MEDIUM** | Switch affected statistics to Welford/Chan; quantize emitted values to defined precision; rerun scans; update golden files. |

## Pitfall-to-Phase Mapping

| Pitfall | Severity | Prevention Phase | Verification |
|---------|----------|------------------|--------------|
| 1. Look-ahead bias | CRITICAL | P1 (timestamp convention) + P2 (window primitives) | Shuffled-future regression test passes |
| 2. Multiple-testing | CRITICAL | P0 (schema fields) + P5 (DSR/FDR impl) | Noise-replay sweep emits ~0 findings at target FDR |
| 3. Survivorship bias | HIGH/CRIT | P1 (lifecycle introspection) + P3 (universe utils) | Findings include coverage; cross-sectional rejects mixed coverage |
| 4. Stdout collision | CRITICAL | P0 (logging convention) + P4 (wrappers) | `jq -c .` on every line passes; `clippy::disallowed_macros` in CI |
| 5. Async contamination | CRITICAL | P0 (workspace) + P2 (Rayon) + P4 (bridges) | `cargo tree -p miner-core` shows no tokio/async; throughput stable CLI vs HTTP |
| 6. Schema breakage | CRITICAL | P0 (envelope + JSON Schema) + Ongoing | Snapshot test per release; semver enforced |
| 7. Naive aggregation | CRITICAL | P1 | DST + weekend + holiday fixture tests pass; deterministic re-run |
| 8. Bid/ask mishandled | HIGH | P3 (per-scan conventions) | Mean-reversion findings change between `bid`/`ask`/`mid` settings |
| 9. Timezone / DST | HIGH | P1 (time model) + P3 (seasonality scans) | Time-of-day scan with two TZs gives consistent rotation; DST-day fixture passes |
| 10. Non-determinism | HIGH | P2 (orchestration) + P5 (golden-file CI) | Two scan runs → byte-identical JSONL |
| 11. Hardcoded paths | HIGH | P0 (config) + P1 (reader + fixture) | Fresh-clone quickstart works |
| 12. MCP / HTTP UX | HIGH | P4 (wrappers) | Manual LLM-agent walkthrough; structured errors; per-scan tools |
| 13. Stationarity | MEDIUM | P3 (rolling tests + reporting) | Cointegration scan emits rolling-window breakdown |
| 14. Tick volume | MEDIUM | P1 (reader naming) + P3 (scan docs) | Bar field named `tick_count`; README caveat present |
| 15. I/O thrashing | MEDIUM | P1 + P2 (pool separation) + P5 (bench) | Benchmark on SSD and HDD-like throughput |
| 16. Memory blowup | MEDIUM | P1 (streaming reader) + P2 (stream scans) | Memory profile flat across long scans |
| 17. Hidden allocations | MEDIUM | P2 + P5 (profiling) | Flamegraph shows alloc < 5% of hot path |
| 18. Float precision | MEDIUM | P2 (Welford/Chan primitives) | Statistic computed two ways agrees within tolerance |
| 19. Crate churn | MEDIUM | P0 (selection) + Ongoing (audit) | `cargo audit` clean; dep review at each minor release |

## Sources & Confidence Notes

- **Multiple testing / DSR / FDR**: Bailey & López de Prado, "The Deflated Sharpe Ratio" (J. of Portfolio Management, 2014); Harvey, Liu & Zhu, "...and the Cross-Section of Expected Returns" (RFS, 2016) — canonical works on backtest overfitting. Confidence HIGH.
- **Look-ahead bias / survivorship**: textbook quant-finance pitfalls, e.g. López de Prado, *Advances in Financial Machine Learning* (Wiley, 2018). Confidence HIGH.
- **Welford / Chan parallel variance**: Chan, Golub & LeVeque, "Algorithms for Computing the Sample Variance" (1983); widely-implemented. Confidence HIGH.
- **MCP stdio transport (stdout = protocol)**: model-context-protocol specification (Anthropic). Stdout-as-channel convention is canonical for stdio transport. Confidence HIGH on the principle; **MEDIUM** on current SDK/crate specifics — web verification unavailable this session, **flag for verification when adding the MCP dep in P0 or P4**.
- **Dukascopy data quirks (00-indexed months, tick volume = tick count, bid/ask separate)**: cross-referenced with PROJECT.md and `tradedesk-dukascopy` cache layout described therein. Confidence HIGH (project-internal).
- **Rust async/sync separation, Rayon for CPU-bound**: ecosystem canon (rayon docs, tokio docs on `spawn_blocking`). Confidence HIGH.
- **Specific crate currency** (`jiff` vs `chrono`, current MCP SDK crate names, `ruzstd` vs `zstd`): Confidence **MEDIUM** without web verification. Flagged for explicit re-check in P0 dep-selection.

---
*Pitfalls research for: Rust quant data-mining engine, statistical scans over OHLCV cache, agent-operable*
*Researched: 2026-05-15*
