# Architecture Research

**Domain:** Rust quant data-mining engine (library + CLI + MCP + HTTP) over a zstd-CSV OHLCV cache
**Researched:** 2026-05-15
**Confidence:** HIGH for crate layout, reader/aggregator/scan shapes, findings envelope (these are standard Rust patterns with well-known trade-offs); MEDIUM for the specific MCP/HTTP wrapper crate choices (depends on which crates the team standardizes on — see STACK.md)

---

## 1. System Overview

The system is a **single core library** (`miner-core`) flanked by three **thin wrapper binaries** (`miner-cli`, `miner-mcp`, `miner-http`) and a small number of **trait-implementation crates** (`miner-reader-dukascopy`) that the wrappers wire into the core at startup. Everything is one Cargo workspace.

```
┌─────────────────────────────────────────────────────────────────────────┐
│  Wrappers (thin; one binary each — argument parsing + protocol only)    │
│                                                                         │
│   ┌────────────┐    ┌────────────┐    ┌────────────┐                    │
│   │ miner-cli  │    │ miner-mcp  │    │ miner-http │                    │
│   │ (clap)     │    │ (rmcp)     │    │ (axum)     │                    │
│   └─────┬──────┘    └─────┬──────┘    └─────┬──────┘                    │
│         │                 │                 │                           │
│         └─────────────────┼─────────────────┘                           │
│                           │ calls into one shared facade                │
├───────────────────────────┴─────────────────────────────────────────────┤
│  miner-core (library — owns all behaviour)                              │
│                                                                         │
│   ┌──────────────────────────────────────────────────────────────┐      │
│   │  facade::api  — single entry point for wrappers              │      │
│   │     run_scan(request, sink) / list_scans() / probe(symbol)   │      │
│   └──────────────────────────────────────────────────────────────┘      │
│         │                │                  │              │            │
│         ▼                ▼                  ▼              ▼            │
│   ┌─────────┐      ┌──────────┐      ┌───────────┐   ┌──────────┐       │
│   │ scans   │      │ aggreg.  │      │ findings  │   │ catalog  │       │
│   │ engine  │ ←──→ │ pipeline │      │ stream    │   │ (symbols)│       │
│   └────┬────┘      └────┬─────┘      └─────┬─────┘   └────┬─────┘       │
│        │ requests       │ requests         │ writes to    │ reads from  │
│        │ bars           │ 1m bars          │ sink         │ reader      │
│        │                ▼                  │              │             │
│        │           ┌──────────┐            │              │             │
│        └──────────►│  reader  │◄───────────┴──────────────┘             │
│                    │  trait   │                                         │
│                    └─────┬────┘                                         │
└──────────────────────────┼──────────────────────────────────────────────┘
                           │ dyn Reader
              ┌────────────┴─────────────┐
              ▼                          ▼
       ┌──────────────────┐     ┌──────────────────┐
       │ reader-dukascopy │     │ reader-mem (test)│
       │ zstd + CSV       │     │ in-memory fixture│
       └────────┬─────────┘     └──────────────────┘
                │
                ▼
        <cache-root>/<SYMBOL>/<YYYY>/<MM>/<DD>_<side>.csv.zst
                │
                ▼
       ┌──────────────────────────────────────┐
       │  derived-bar cache (miner-owned)     │
       │  <bar-cache-root>/<src>/<sym>/...    │
       └──────────────────────────────────────┘
```

**Cardinal rule:** data flows one way — `reader → aggregator → scans → findings sink`. The facade orchestrates, but no inner layer ever calls "up" toward a wrapper.

---

## 2. Workspace / Crate Layout

Single Cargo workspace. Each crate has one job. This maps directly onto the build order in §10.

```
tradedesk-miner/
├── Cargo.toml                       # [workspace] members + resolver = "2"
├── crates/
│   ├── miner-core/                  # library — types, traits, scan engine, aggregator, facade
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── types/               # Bar, Side, Symbol, Timeframe, TimeRange
│   │   │   ├── reader/              # trait Reader + errors (no impls)
│   │   │   ├── aggregator/          # 1m → Nm; derived-bar cache
│   │   │   ├── catalog/             # symbol discovery (delegates to Reader)
│   │   │   ├── scans/               # trait Scan + registry + built-in scans
│   │   │   │   ├── stats/           # volatility, autocorr, return-dist
│   │   │   │   ├── cross/           # lead-lag, correlation, cointegration
│   │   │   │   └── seasonality/     # hour, day-of-week, end-of-month
│   │   │   ├── findings/            # Finding envelope, sink trait, JSONL writer
│   │   │   ├── facade/              # the public API the wrappers call
│   │   │   └── error.rs             # crate-wide MinerError
│   │   └── tests/                   # integration: reader-mem driving end-to-end
│   │
│   ├── miner-reader-dukascopy/      # one Reader impl. depends on miner-core.
│   │   └── src/lib.rs               # zstd decode + CSV parse + path layout
│   │
│   ├── miner-cli/                   # binary. depends on core + dukascopy reader.
│   │   └── src/main.rs              # clap subcommands → facade calls → stdout JSONL
│   │
│   ├── miner-mcp/                   # binary. depends on core + dukascopy reader.
│   │   └── src/main.rs              # MCP tools → facade calls → MCP responses
│   │
│   └── miner-http/                  # binary. depends on core + dukascopy reader.
│       └── src/main.rs              # axum routes → facade calls → SSE/NDJSON
│
├── xtask/                           # optional: dev tooling (release, fuzz, bench)
└── benches/                         # workspace-wide criterion benches
```

**Why a workspace and not one crate with features:**

- Each wrapper has its own dependency surface (clap, rmcp, axum). Feature-gating those inside a single crate inflates compile times for everyone and makes `cargo check` in the library noisy.
- Workspace gives every wrapper its own `[[bin]]` target, its own `Cargo.lock` entry, and lets `miner-core` stay free of any wrapper-specific dep.
- `miner-reader-dukascopy` is split out so a downstream user wanting a different cache shape can swap it without forking the core. This is the open-source posture from PROJECT.md.

**Why not a feature flag per wrapper inside `miner-core`:** the wrappers do not contribute behaviour; they translate protocols. Putting `feature = "cli"` in core is the wrong abstraction — wrappers are *consumers* of core, not *variants* of it.

**Dependency direction (strictly one-way):**

```
miner-cli ─┐
miner-mcp ─┼─► miner-reader-dukascopy ─► miner-core
miner-http ┘                              ▲
                                          │
                       (any future reader impl)
```

`miner-core` depends on **nothing in the workspace**. That is the invariant.

---

## 3. Module Boundaries Inside `miner-core`

| Module        | Owns                                              | Reads from        | Writes to              | Public to wrappers? |
| ------------- | ------------------------------------------------- | ----------------- | ---------------------- | ------------------- |
| `types`       | Bar, Side, Symbol, Timeframe, TimeRange, BarSlice | —                 | —                      | yes (re-exported)   |
| `reader`      | Reader trait + ReaderError                        | —                 | —                      | yes (for impls)     |
| `catalog`     | Symbol discovery, date-range introspection        | reader            | —                      | yes                 |
| `aggregator`  | 1m → 15m/1h/1d, derived-bar cache                 | reader            | derived-bar cache (fs) | indirectly          |
| `scans`       | Scan trait, registry, built-in scans              | aggregator        | findings sink          | yes (registry only) |
| `findings`    | Finding envelope, FindingSink trait, JSONL writer | scans             | sink (stdout / chan)   | yes                 |
| `facade::api` | run_scan, list_scans, probe                       | all of the above  | —                      | **the only entry**  |
| `error`       | MinerError enum (thiserror)                       | —                 | —                      | yes                 |

The wrappers should `use miner_core::facade::*;` and `use miner_core::findings::*;` and **nothing else** for behaviour. Types are re-exported for ergonomics.

---

## 4. Reader Trait

### Constraints driving the shape

- The 1-minute cache is the only thing that crosses the disk-decode boundary. Everything downstream is in-memory `Vec<Bar>` or columnar slices. Decode is the expensive step and is **embarrassingly parallel by day** — fan-out belongs *outside* the reader, not inside it.
- The reader must yield **whole-day chunks** because that is the natural unit on disk (one `.csv.zst` per day per side). Sub-day streaming is pointless: a daily file is ~1440 rows, decodes in milliseconds, and the consumer (aggregator) needs the whole day anyway to emit aligned higher-timeframe bars.
- The API surface must be small enough that a downstream user with a different cache (Parquet, ClickHouse, S3) can implement it without pain.
- Async buys nothing here: reads are CPU-bound (zstd decode + CSV parse), not I/O-wait-bound. Synchronous + rayon is the right model.

### Sketch

```rust
// crates/miner-core/src/reader/mod.rs
use crate::types::{Bar, Side, Symbol, TimeRange};

pub trait Reader: Send + Sync {
    /// Reader identity, used as part of the derived-bar cache key
    /// (e.g. "dukascopy-v1"). Must be stable across releases.
    fn source_id(&self) -> &str;

    /// Symbols the reader knows about. Cheap; called at startup and by `list`.
    fn symbols(&self) -> Result<Vec<Symbol>, ReaderError>;

    /// Inclusive UTC date range available for (symbol, side).
    /// Returns None if the reader has no data for that pair.
    fn available_range(
        &self,
        symbol: &Symbol,
        side: Side,
    ) -> Result<Option<TimeRange>, ReaderError>;

    /// Yield 1-minute bars for one UTC day, in timestamp order.
    /// Implementations should decode the file once and return all rows;
    /// the aggregator handles cross-day stitching.
    fn read_day(
        &self,
        symbol: &Symbol,
        side: Side,
        day: chrono::NaiveDate,
    ) -> Result<Vec<Bar>, ReaderError>;
}

#[derive(Debug, thiserror::Error)]
pub enum ReaderError {
    #[error("symbol not in source")] UnknownSymbol,
    #[error("no data for {symbol} {side:?} on {day}")] Missing { symbol: Symbol, side: Side, day: chrono::NaiveDate },
    #[error("decode failed: {0}")] Decode(String),
    #[error(transparent)] Io(#[from] std::io::Error),
}
```

### Why these choices

- **`&self`, not `&mut self`:** readers are shared across rayon threads. State (open file handles, caches) lives behind `Mutex` or `OnceCell` *inside the impl* if needed.
- **`Vec<Bar>` per day, not an iterator:** a day is small (~1440 rows ≈ 80 KB), iterator machinery is overhead, and the aggregator wants random access for missing-bar interpolation policy. If a future reader needs to stream multi-day chunks, add `read_range(symbol, side, range) -> Box<dyn Iterator>` as a *default-method extension* without breaking the trait.
- **`Send + Sync`:** required so the engine can hold a single `Arc<dyn Reader>` across rayon workers. This is the dominant cost saving in the design.
- **`chrono::NaiveDate`:** all data is UTC by spec (PROJECT.md). NaiveDate avoids the timezone footgun.
- **`Bar` is a plain struct** (`timestamp: i64 millis utc, open/high/low/close: f64, volume: f64`). Not generic. Generic-Bar is overkill until a second reader proves it.

### Dukascopy reader specifics

Lives in `miner-reader-dukascopy`. Knows:

- Path template `<root>/<SYMBOL>/<YYYY>/<MM 0-indexed>/<DD>_<bid|ask>.csv.zst`.
- The 0-indexed month is a Dukascopy quirk — encapsulated here, never leaked.
- `symbols()` = `read_dir(root)`.
- `available_range()` = directory scan, cached behind `OnceCell` for the duration of the process.
- `read_day()` = `zstd::Decoder` wrapped around `csv::Reader`, push into `Vec<Bar>`.
- `source_id() -> "dukascopy-v1"`.

---

## 5. Aggregator Pipeline

### What it does

Convert 1-minute `Vec<Bar>` from the reader into close-aligned higher-timeframe bars (15m, 1h, 1d), cached on disk so subsequent scans over the same parameter grid pay decode cost once.

### Bar alignment rule (must be deterministic and obvious)

- A 15-minute bar's `timestamp` is the **open** of the window (UTC), aligned to wall-clock boundaries: `00:00, 00:15, 00:30, 00:45, 01:00, …`.
- 1h bars align to the hour. 1d bars align to UTC midnight.
- OHLC: `open = first 1m open`, `high = max(highs)`, `low = min(lows)`, `close = last 1m close`, `volume = sum(volumes)`.
- **Missing-bar policy:** if fewer than `min_coverage_pct` (default 50%) of the 1m bars in a window are present, the higher-timeframe bar is **omitted** (not interpolated). Quant scans must see absence as absence.

### Pipeline shape

```
read_day(D, sym, side)               ┐
read_day(D+1, sym, side)             │  rayon::join across days
... (range)                          ┘
        │
        ▼
  flatten + sort by ts (defensive)
        │
        ▼
  resample(timeframe, alignment, coverage_policy)
        │
        ▼
  Vec<Bar>  (target timeframe)
        │
        ├── return to caller
        └── (optionally) write to derived-bar cache
```

### Derived-bar cache key

The key uniquely identifies "the input parameters that would produce this exact `Vec<Bar>`":

```
<bar-cache-root>/<source_id>/<symbol>/<side>/<timeframe>/<YYYY-MM>.bars.zst
```

- **`source_id`**: from `Reader::source_id()`. Ensures a future swap to a different reader (e.g. polygon) does not collide.
- **`symbol`, `side`, `timeframe`**: obvious cache dimensions.
- **Month-partitioned files**: month is the right chunk size — a year is 12 files, each holding ~30 days of 15m bars (~2880 rows) or 1h bars (~720 rows). Trivial to read, easy to invalidate by month, mmap-friendly.
- **Format**: bincode + zstd. CSV is wrong here — this cache is for the engine, not humans.
- **Header** in each file: `{ source_id, symbol, side, timeframe, aggregator_version, written_at_utc }`. `aggregator_version` is bumped if the alignment rule or coverage policy changes — this is the **invalidation mechanism**.

### Invalidation

Two-layer:

1. **Version field mismatch** in the file header → cache miss → recompute. Bump `aggregator_version` in code when the aggregator's output for the same input changes.
2. **Source-data freshness** check via `Reader::available_range()` and an mtime comparison against the source files for that month. Pluggable: each Reader impl exposes `source_fingerprint(symbol, side, day) -> u64` (default: file mtime hash); the cache stores it; mismatch → recompute.

### Aggregator API

```rust
pub struct AggregatorConfig {
    pub bar_cache_root: std::path::PathBuf,
    pub min_coverage_pct: f32,        // default 0.5
    pub write_cache: bool,            // default true
}

pub struct Aggregator<R: Reader> {
    reader: std::sync::Arc<R>,
    config: AggregatorConfig,
}

impl<R: Reader> Aggregator<R> {
    pub fn bars(
        &self,
        symbol: &Symbol,
        side: Side,
        timeframe: Timeframe,
        range: TimeRange,
    ) -> Result<BarSlice, MinerError>;
}
```

`BarSlice` is a thin wrapper around `Vec<Bar>` carrying provenance (symbol, side, timeframe, range, source_id) so downstream scans can produce reproducibility metadata in findings without re-deriving it.

---

## 6. Scan Engine

### Surface goals

- One trait covers every scan (stats, cross-instrument, seasonality). Registry pattern, not enum dispatch.
- Parameters are typed per-scan but type-erased at the registry boundary (JSON in → typed struct via `serde`).
- Each scan emits zero or more findings. Streaming, not batch.
- Errors during one scan never kill the engine — they become a `Finding` of kind `Error` so the caller sees what failed and continues.

### Trait sketch

```rust
// crates/miner-core/src/scans/mod.rs
pub trait Scan: Send + Sync + 'static {
    /// Stable scan identifier, e.g. "stats.autocorr.lag1", "cross.lead_lag".
    fn id(&self) -> &'static str;

    /// Scan family for filtering ("stats" | "cross" | "seasonality").
    fn family(&self) -> ScanFamily;

    /// JSON schema for parameters (served by the wrappers for tool discovery).
    fn parameter_schema(&self) -> serde_json::Value;

    /// Resolve a `serde_json::Value` into the scan's own typed params,
    /// returning a boxed "prepared" handle. Validation happens here.
    fn prepare(
        &self,
        params: &serde_json::Value,
    ) -> Result<Box<dyn PreparedScan>, ScanError>;
}

pub trait PreparedScan: Send + Sync {
    /// Symbols / timeframes this scan needs. The engine pre-fetches bars
    /// via the aggregator and passes them in.
    fn requirements(&self) -> ScanRequirements;

    /// Execute over the materialised inputs. Emits to a FindingSink.
    fn run(
        &self,
        inputs: &ScanInputs,
        sink: &mut dyn FindingSink,
    ) -> Result<(), ScanError>;
}

pub struct ScanRequirements {
    pub symbols: Vec<Symbol>,            // [] = all from catalog
    pub timeframes: Vec<Timeframe>,
    pub sides: Vec<Side>,                // typically [Bid] or [Bid, Ask]
    pub range: TimeRange,
}

pub struct ScanInputs<'a> {
    pub bars: &'a std::collections::HashMap<(Symbol, Side, Timeframe), BarSlice>,
    pub range: TimeRange,
}
```

### Engine orchestration

```rust
pub struct Engine<R: Reader> {
    aggregator: Aggregator<R>,
    registry: ScanRegistry,
}

impl<R: Reader> Engine<R> {
    pub fn run_scan(
        &self,
        request: ScanRequest,
        sink: &mut dyn FindingSink,
    ) -> Result<RunSummary, MinerError> {
        let scan = self.registry.get(&request.scan_id)?;
        let prepared = scan.prepare(&request.params)?;
        let reqs = prepared.requirements();

        // 1. fan out bar materialisation across (symbol, side, timeframe) on rayon
        let bars = self.aggregator.materialise_parallel(&reqs)?;

        // 2. hand them all to the scan in one shot (most scans need it that way:
        //    cross-instrument scans want every symbol at once; single-symbol
        //    scans can iterate internally with rayon)
        let inputs = ScanInputs { bars: &bars, range: reqs.range };
        prepared.run(&inputs, sink)?;

        Ok(RunSummary { /* counts, timing */ })
    }
}
```

### Registry

`ScanRegistry::default()` is populated at the call site (in `miner-core::facade::Engine::with_default_scans()`). New scans register by adding `registry.add(Box::new(MyScan));`. The registry is `HashMap<&'static str, Box<dyn Scan>>` keyed by `id()`.

### Why this shape

- **`prepare` separated from `run`**: validation errors are reported before any bar materialisation kicks off (fail fast on bad params). The `Box<dyn PreparedScan>` returned can also be re-used in batch sweep mode (same scan, same params, different ranges) without re-validating.
- **Bars passed by reference, not channels**: scans want random access (windowed slices, multi-symbol correlation matrices). A channel-of-bars would force them all into streaming form for no benefit — once a year of 15m bars is in memory (~35K rows × 8 cols × 8 bytes ≈ 2 MB), keeping it there is free.
- **Cross-instrument scans receive a `HashMap<(Symbol, Side, Timeframe), BarSlice>`**: this is the simplest shape that supports lead-lag, correlation matrices, and basket divergence without ad-hoc plumbing.
- **`FindingSink` is `&mut dyn`, not generic**: keeps the trait object-safe and lets one `Engine` run with stdout, file, or in-memory sinks interchangeably.

### Error handling

- `ScanError` (per-scan) is *recoverable*: the engine catches it and emits a `Finding` with `kind = "scan_error"` so the caller's stream is never silently empty.
- `MinerError` (engine-level: reader IO failure, cache corruption) is *fatal*: it aborts the run with an error code.
- All errors implement `thiserror::Error` and `serde::Serialize` for findings-stream embedding.

---

## 7. Findings Envelope

JSON Lines (NDJSON, one finding per line, `\n`-terminated, UTF-8). This format is:

- Trivial to consume from any language (the quant agent is Python).
- Streamable — each line is independently parsable; consumer can persist or render incrementally.
- Already implied by PROJECT.md ("findings stream to stdout in a stable, structured format (JSON / JSONL)").

### Envelope schema

Every finding is one of: `result`, `scan_error`, `run_start`, `run_end`. The discriminant is `kind`.

```json
{
  "kind": "result",
  "schema_version": 1,
  "id": "01HZF9G2K0...",                     // ULID per finding
  "run_id": "01HZF9G09T...",                 // ULID per scan run; ties findings together
  "scan_id": "stats.autocorr.lag1",
  "scan_family": "stats",
  "produced_at_utc": "2026-05-15T14:33:21.412Z",

  "source": {
    "source_id": "dukascopy-v1",
    "symbol": "EURUSD",
    "side": "bid",
    "timeframe": "15m",
    "range": { "start_utc": "2024-01-01T00:00:00Z", "end_utc": "2024-12-31T23:45:00Z" }
  },

  "params": { /* exact params the scan received, after defaults applied */ },

  "effect": {
    "metric": "autocorr_lag1",
    "value": 0.087,
    "p_value": 0.0014,
    "n": 35136,
    "ci95": [0.054, 0.120],
    "extra": { /* scan-specific stats — open object */ }
  },

  "raw": {
    "encoding": "f64_le_base64",             // or "inline_json"
    "series": {
      "returns": "...base64...",             // raw arrays the scan operated on
      "timestamps_ms": "...base64..."
    },
    "shapes": { "returns": [35136], "timestamps_ms": [35136] }
  },

  "reproducibility": {
    "aggregator_version": 1,
    "min_coverage_pct": 0.5,
    "miner_version": "0.1.0",
    "reader_version": "dukascopy-v1@0.1.0"
  }
}
```

### Field-by-field

| Field                     | Type                | Required | Notes                                                                     |
| ------------------------- | ------------------- | -------- | ------------------------------------------------------------------------- |
| `kind`                    | enum string         | yes      | `"result" \| "scan_error" \| "run_start" \| "run_end"`                    |
| `schema_version`          | u32                 | yes      | Currently `1`. Bumped on breaking envelope changes.                       |
| `id`                      | string (ULID)       | yes      | Per finding. Sortable, unique, time-prefixed.                             |
| `run_id`                  | string (ULID)       | yes      | Per scan invocation. Lets consumers group findings.                       |
| `scan_id`                 | string              | yes      | Matches `Scan::id()`.                                                     |
| `scan_family`             | string              | yes      | `"stats" \| "cross" \| "seasonality"`.                                    |
| `produced_at_utc`         | RFC3339 string      | yes      | When the finding was emitted.                                             |
| `source`                  | object              | yes      | What input produced it. See below.                                        |
| `source.source_id`        | string              | yes      | From `Reader::source_id()`.                                               |
| `source.symbol`           | string              | yes      | For cross-instrument scans this is the primary; secondaries go in extras. |
| `source.side`             | `"bid" \| "ask"`    | yes      | —                                                                         |
| `source.timeframe`        | `"15m"\|"1h"\|"1d"` | yes      | —                                                                         |
| `source.range`            | object              | yes      | `start_utc`, `end_utc` (inclusive start, exclusive end).                  |
| `params`                  | object              | yes      | Exact resolved params (defaults applied).                                 |
| `effect`                  | object              | yes      | Required for `kind = "result"`.                                           |
| `effect.metric`           | string              | yes      | Canonical metric name.                                                    |
| `effect.value`            | f64                 | yes      | The headline number.                                                      |
| `effect.p_value`          | f64                 | no       | Where applicable.                                                         |
| `effect.n`                | u64                 | yes      | Sample size.                                                              |
| `effect.ci95`             | `[f64, f64]`        | no       | Bootstrap or analytic.                                                    |
| `effect.extra`            | object              | no       | Open schema for scan-specific extras.                                     |
| `raw`                     | object              | no       | Omittable if caller passes `--no-raw`.                                    |
| `raw.encoding`            | string              | yes¹     | `"f64_le_base64"` (binary) or `"inline_json"` (arrays inline).            |
| `raw.series`              | object              | yes¹     | Map of name → base64 string or inline JSON array.                         |
| `raw.shapes`              | object              | yes¹     | Map of name → shape `[u64,…]`, lets the consumer reshape correctly.       |
| `reproducibility`         | object              | yes      | Versions for exact reproducibility.                                       |
| `error`                   | object              | yes²     | `{ message, scan_id, params }`. Required for `kind = "scan_error"`.       |

¹ Required only if `raw` is present.
² Required only for `scan_error`.

`run_start` / `run_end` envelopes carry `{ kind, run_id, scan_id, started_at_utc / ended_at_utc, summary }`. The summary has counts (`results_emitted`, `errors`) and wall-clock timing.

### Why base64 for raw arrays

JSON-array-of-floats is 3-4× larger than `f64-LE + base64` and ~10× slower to parse. The quant agent in Python decodes with `np.frombuffer(base64.b64decode(s), dtype="<f8").reshape(shape)` in one line. `--no-raw` and `--raw=inline-json` are CLI/MCP options for humans peeking at output.

---

## 8. Wrapper Crates

Each wrapper does three things and nothing else:

1. Parse its protocol's request format.
2. Build a `ScanRequest` (or call `list_scans()` / `probe()`).
3. Pipe the resulting `FindingSink` output back to the protocol.

### `miner-cli`

`clap` derive. Subcommands map 1:1 to facade methods:

```
miner list-scans
miner list-symbols
miner probe --symbol EURUSD
miner scan stats.autocorr.lag1 \
    --symbol EURUSD --side bid --timeframe 15m \
    --start 2024-01-01 --end 2024-12-31 \
    --params-json '{"max_lag": 5}' \
    [--no-raw]
miner sweep <preset-name> [--filter family=stats]
```

Output: NDJSON to stdout. Logs to stderr (`tracing`). No flags that change scan semantics — those go in `--params-json` so they round-trip identically across all three wrappers.

### `miner-mcp`

`rmcp` over stdio (default) or HTTP. Each scan registry entry becomes a tool: `tool/list` enumerates them with their `parameter_schema()`. `tool/call` invokes the facade. Findings stream back as MCP tool-call result chunks (one envelope per chunk). Plus three meta-tools: `list_scans`, `list_symbols`, `probe`.

### `miner-http`

`axum`. Endpoints:

```
GET  /v1/scans                         → JSON array
GET  /v1/symbols                       → JSON array
POST /v1/scan                          → text/event-stream (SSE) of findings
                                         or application/x-ndjson, content-negotiated
POST /v1/sweep                         → same streaming model
```

The streaming response is the *same byte sequence* the CLI writes to stdout — by construction, because all three wrappers funnel through one `FindingSink::write_envelope()` method that does the actual JSON encoding inside `miner-core`. **This is the mechanism that guarantees identical semantics across wrappers.** The wrappers never construct JSON themselves.

---

## 9. Where the Boundaries Are (for benching & testing)

Each component can be exercised independently:

| Component       | Test fixture                                                    | What it proves                                                  |
| --------------- | --------------------------------------------------------------- | --------------------------------------------------------------- |
| `reader` trait  | `reader-mem` impl that returns hand-built `Vec<Bar>`            | Aggregator works without disk; deterministic.                   |
| `aggregator`    | Drive with `reader-mem`; assert exact bar timestamps & OHLC     | Alignment, coverage policy, derived-bar cache round-trip.       |
| `scans`         | Drive with hand-built `BarSlice`s; no aggregator, no reader     | Pure math; property tests on synthetic series.                  |
| `findings`      | `Vec<u8>`-backed sink; parse output back as JSON, assert schema | Envelope shape is stable; schema_version invariants hold.       |
| `facade`        | `reader-mem` + real aggregator + real scans + Vec sink          | End-to-end without disk or protocols.                           |
| Wrappers (each) | Spawn the binary, hit it, assert stdout/SSE bytes               | Protocol translation correctness only.                          |

Bench targets (criterion, in `benches/`):

- `bench_reader_dukascopy_decode_day` — pure decode throughput.
- `bench_aggregator_resample_1m_to_15m_year` — pure aggregation.
- `bench_scan_autocorr_year` — pure scan over pre-loaded bars.
- `bench_facade_run_scan_year` — end-to-end with cold and warm derived-bar cache.

The separation matters because the bottleneck will move as work progresses: it starts in zstd decode, moves to aggregation, then settles in whichever scan is being optimised. Independent benches let you point at the right thing.

---

## 10. Build Order

Strict dependency-respecting order. Each step is independently testable when complete.

```
STEP 1.  miner-core::types                  (no deps)
            Bar, Side, Symbol, Timeframe, TimeRange, BarSlice

STEP 2.  miner-core::reader trait           (deps: types)
            Reader trait, ReaderError
            Plus an internal reader-mem impl in tests/

STEP 3.  miner-core::aggregator             (deps: types, reader)
            Resample function (pure, no I/O)
            Derived-bar cache (read + write)
            BLOCKS on: reader-mem fixture from step 2.

STEP 4.  miner-core::findings               (deps: types)
            Finding envelope, FindingSink trait, JSONL writer
            Independent of reader/aggregator/scans — can be built in parallel
            with step 3 once step 1 is done.

STEP 5.  miner-core::scans (one scan first) (deps: types, aggregator, findings)
            Scan trait, registry, ONE scan (e.g. stats.autocorr.lag1)
            BLOCKS on: aggregator (needs BarSlice), findings (needs sink).

STEP 6.  miner-core::facade                 (deps: all of the above)
            run_scan, list_scans, probe
            BLOCKS on: scans (needs at least one).

STEP 7.  miner-reader-dukascopy             (deps: miner-core::reader)
            Real zstd-CSV reader; first end-to-end run against real cache.
            CAN start in parallel with step 5 (only depends on the trait,
            not on scan code), but isn't useful until facade exists.

STEP 8.  miner-cli                          (deps: facade, dukascopy reader)
            First wrapper. Smallest surface area. Use it to validate the facade
            shape before locking it in by adding MCP / HTTP on top.

STEP 9.  Add remaining built-in scans       (deps: scans trait stable)
            cross.lead_lag, cross.correlation,
            seasonality.hour_of_day, seasonality.day_of_week, etc.

STEP 10. miner-mcp                          (deps: facade)
STEP 11. miner-http                         (deps: facade)
            Parallel; both translate the same facade methods.
```

**Critical observation:** the facade (step 6) is the lock-in point. Once a wrapper exists (step 8), changing the facade signature is a 3-wrapper refactor. Treat step 6 as a design checkpoint — get the facade shape right before step 8.

**What can be parallelised across people:** steps 3 and 4 (aggregator and findings) are independent once step 1 lands. Step 9 (additional scans) can be parallelised across N people once step 5 lands. Steps 10 and 11 are parallel once step 8 ships.

---

## 11. Architectural Patterns

### Pattern 1: Trait-object boundary at every plug point

**What:** `Reader`, `Scan`, `FindingSink` are all `dyn`-friendly. Generic only inside hot loops (e.g. `Aggregator<R: Reader>` for monomorphisation of decode), but the facade holds `Arc<dyn Reader>` because the wrappers' compile times matter more than the facade's marginal dispatch cost.

**When to use:** any plug-point where downstream users will write impls; or where the variant count is bounded but >1 (the three sinks: stdout, file, in-memory).

**Trade-off:** virtual call per bar would be a disaster; virtual call per scan-run is free. Keep `dyn` at coarse granularity.

### Pattern 2: Facade as the single contract

**What:** `miner-core::facade::api` is the only public surface for wrappers. Wrappers may use `types` and `findings` for type names, but call only through facade.

**When to use:** whenever you have multiple frontends over one engine. This is the standard library-+-thin-wrapper pattern.

**Trade-off:** facade becomes a chokepoint to maintain. That is the *point* — it forces wrapper parity.

### Pattern 3: Reader-driven cache keys

**What:** the derived-bar cache key includes `Reader::source_id()`. Switching readers does not silently reuse another reader's cache.

**When to use:** any time multiple data sources can produce the same logical shape (here: 1m OHLCV) with different upstream semantics.

**Trade-off:** small cost (one extra path segment); huge correctness win.

### Pattern 4: Findings as immutable envelopes, not method calls

**What:** scans emit `Finding` values to a `FindingSink`. The sink is what differs (stdout JSONL, SSE, in-memory). Scans never know they are talking to a network.

**When to use:** any streaming-result system that needs to support multiple transports.

**Trade-off:** all output is JSON-shaped, even for the in-memory case. We pay the serialise cost once at the boundary. That is fine — it is amortised by orders-of-magnitude more time in the scan body.

### Pattern 5: Rayon for parallelism, no async in core

**What:** `rayon::par_iter` over `(symbol, side, timeframe)` tuples inside the aggregator and over inputs inside cross-instrument scans. No `tokio` in `miner-core`. The wrappers (mcp, http) bring their own runtimes and call into core via `spawn_blocking`.

**When to use:** CPU-bound workloads. This is one.

**Trade-off:** wrappers cross a thread boundary per request. Negligible compared to per-scan time.

---

## 12. Data Flow

### Single-scan request flow

```
wrapper (CLI/MCP/HTTP)
    │ ScanRequest
    ▼
facade::run_scan
    │
    ▼
ScanRegistry::get(scan_id)  ─►  Scan::prepare(params)  ─►  PreparedScan
    │
    ▼
PreparedScan::requirements()  ─►  ScanRequirements
    │
    ▼
Aggregator::materialise_parallel(reqs)
    │
    │ for each (symbol, side, timeframe) needed:
    │     check derived-bar cache
    │       hit → load
    │       miss →
    │           Reader::read_day(D) over the range (rayon par_iter)
    │           resample to target timeframe
    │           write to derived-bar cache
    │
    ▼
HashMap<(Symbol, Side, Timeframe), BarSlice>
    │
    ▼
PreparedScan::run(inputs, sink)
    │ emits Finding values
    ▼
FindingSink::write_envelope(json_bytes)
    │
    ▼
wrapper protocol (stdout / SSE / MCP chunk)
```

### Batch sweep flow

Same shape, but the wrapper feeds a list of `ScanRequest`s. The engine runs them sequentially (each scan internally parallel via rayon). Sequential at the outer level keeps stdout/SSE output coherent and avoids cache thrash on bar materialisation.

---

## 13. Scaling Considerations

| Scale                                                  | Architecture adjustments                                                                                                              |
| ------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------- |
| Dev sample (28 symbols × 6 years)                      | Default config is fine. Single host. Derived-bar cache fits in <1 GB.                                                                 |
| 100 symbols × 10 years × 15m                           | Still single host. Watch derived-bar cache size; move to mmap-loaded bincode files. Consider per-symbol parallelism in scan dispatch. |
| 500+ symbols, frequent sweeps                          | Pre-warm derived-bar cache as a separate batch step. Add a `--bar-cache-only` mode to the CLI. HTTP wrapper gains a job queue.        |
| Whole-universe scans (1000s of symbols, 1m raw access) | This is out of scope per PROJECT.md (tick is out, 1m is the floor). If forced: move to Parquet-backed bar cache + DuckDB query layer. |

### Scaling priorities (what breaks first → next)

1. **First bottleneck: zstd decode + CSV parse** on cold cache. Mitigation: derived-bar cache eliminates re-paying this cost.
2. **Second bottleneck: cross-instrument scans with O(N²) symbol pairs.** Mitigation: scan-level parallelism (rayon over pairs); optionally a top-K pruning option per scan.
3. **Third bottleneck: findings serialisation for very chatty scans** that emit per-bar findings. Mitigation: per-scan emit thresholds (already a parameter), and the `f64_le_base64` raw encoding.

---

## 14. Anti-Patterns to Avoid

### Anti-Pattern 1: Async everywhere

**What people do:** Make `Reader` async because "it does I/O." Sprinkle `async fn` through the engine.
**Why it's wrong:** zstd decode and CSV parsing are CPU-bound. `tokio::fs::File` does not parallelise CPU work. Async colours the entire API for no throughput gain and adds runtime coupling that breaks the CLI binary's "no tokio dep" benefit.
**Do this instead:** sync `Reader`, rayon for parallelism, `spawn_blocking` at the wrapper boundary when a wrapper happens to be async.

### Anti-Pattern 2: Generic `Bar<T>` over price representation

**What people do:** `pub struct Bar<P: Price> { open: P, … }` to "support fixed-point one day."
**Why it's wrong:** Generic adds noise across hundreds of files, costs nothing until proven necessary, and the existing cache is `f64` anyway. YAGNI.
**Do this instead:** `f64`. If a fixed-point reader appears, introduce conversion at the reader boundary, not the type.

### Anti-Pattern 3: One giant `miner` crate with feature flags

**What people do:** `cargo build --features cli`, `--features mcp`, etc.
**Why it's wrong:** every contributor pays the compile cost of every wrapper's deps; `cargo check` is noisy; doc generation conflates wrapper APIs with library APIs; downstream users of `miner-core` pull in axum.
**Do this instead:** workspace, four binaries, clean dependency direction.

### Anti-Pattern 4: Persisted findings inside the engine

**What people do:** "Just add a SQLite finding store, it's three lines."
**Why it's wrong:** explicit out-of-scope per PROJECT.md; couples the engine to a storage choice; breaks the streaming contract; turns the miner into something it isn't.
**Do this instead:** stream findings; callers pick persistence (or none). If you want a "default findings file", do it in the CLI wrapper as a tee to a file. Core stays clean.

### Anti-Pattern 5: Aggregator that infers higher timeframes lazily during scans

**What people do:** Have the scan engine ask for `(EURUSD, 15m, 2024)`, and resample 1m bars to 15m on the fly inside the scan code path.
**Why it's wrong:** every scan recomputes the same aggregation; the cache exists to avoid this; the bar-alignment rule has to be defined in N places.
**Do this instead:** aggregator is a *materialisation step* that runs before the scan and writes its output through the cache. Scans always consume already-aggregated `BarSlice`s.

### Anti-Pattern 6: Different findings JSON across wrappers

**What people do:** the CLI prints `{"scan_id":..., "value":...}`, the HTTP server returns `{"data": {"scanId":..., "value":...}}`, the MCP server adds a wrapping envelope.
**Why it's wrong:** breaks the agent-operability promise from PROJECT.md ("every miner capability must be reachable from CLI, MCP, and HTTP without divergent behavior").
**Do this instead:** the JSON bytes are produced inside `miner-core` and copied verbatim through every wrapper. Wrappers may add an *outer* envelope appropriate to their protocol (e.g. SSE `data:` prefix), but never alter the inner JSON.

---

## 15. Integration Points

### External services

| Service                              | Integration pattern                                                                                       | Notes                                                                                |
| ------------------------------------ | --------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------ |
| Dukascopy cache (filesystem)         | `miner-reader-dukascopy` reads files directly                                                             | Path layout from PROJECT.md; 0-indexed month is a known quirk encapsulated in reader |
| Derived-bar cache (filesystem)       | `miner-core::aggregator` writes/reads `<bar-cache-root>/...`                                              | Owned and writable by miner; configurable root; bincode+zstd files                   |
| Quant agent (downstream consumer)    | Reads NDJSON from CLI stdout / SSE from HTTP / MCP tool results                                           | Stable schema_version field; envelope spec is the contract                           |

### Internal boundaries

| Boundary                      | Communication              | Notes                                                                                |
| ----------------------------- | -------------------------- | ------------------------------------------------------------------------------------ |
| wrapper ↔ facade              | direct function calls      | wrappers never see internal types beyond `types` + `findings` + `facade`             |
| facade ↔ engine               | direct                     | both inside `miner-core`                                                              |
| engine ↔ aggregator           | direct, owns `Aggregator`  | engine fans out via rayon inside aggregator                                          |
| aggregator ↔ reader           | `Arc<dyn Reader>`          | trait object so the engine carries one reader pointer                                |
| scans ↔ findings              | `&mut dyn FindingSink`     | sink chosen by the caller, scans don't care                                          |
| wrappers ↔ findings transport | `FindingSink` impl per wrapper | CLI: stdout writer; HTTP: SSE writer; MCP: chunk-emit closure                    |

---

## 16. Confidence and Open Questions

**HIGH confidence:**

- Workspace + four binaries layout. This is textbook Rust for this shape.
- Sync reader + rayon. Async would be wrong for CPU-bound decode.
- Findings as NDJSON envelopes through a `FindingSink` trait.
- Derived-bar cache keyed on `source_id`.
- Build order with facade as the lock-in point.

**MEDIUM confidence:**

- Specific crate choices for the wrappers (`clap` is standard; `axum` is standard; `rmcp` is the recommended Rust MCP SDK at time of writing but the ecosystem moves — STACK.md will confirm). The architecture does not depend on these specific choices; any equivalent crate slots in without reshaping.
- Bincode vs Parquet for the derived-bar cache. Bincode is simpler and faster for this use case (single-process reader, append-mostly, well-defined struct). Parquet would matter if external tools needed to read the cache directly. PROJECT.md does not require external access, so bincode wins.

**Open questions for later phases:**

- Whether `Aggregator::materialise_parallel` should return owned `BarSlice`s or share an `Arc` across scans in batch sweep mode. Answer falls out once batch sweep is benchmarked.
- Exact set of effect statistics each scan emits. Defer to scan-implementation phases.
- Whether MCP should expose each scan as a separate tool or one parameterised `scan` tool. Both are valid; lean toward one-tool-per-scan because `tool/list` then naturally enumerates the catalog.

---

## Sources

- PROJECT.md (constraints, scope, the three-wrapper requirement, cache layout)
- Standard Rust workspace patterns as used by ripgrep, tokio, cargo, etc. — library + multiple binaries split across workspace member crates
- Rust API guidelines on trait-object friendliness and `Send + Sync` for parallel use
- The `rayon` data-parallelism model (par_iter over independent work units; no async required for CPU-bound code)
- NDJSON / JSON Lines spec (one JSON object per line, `\n`-delimited) as the de-facto streaming format for tool→tool pipelines
- MCP protocol streaming-tool-result semantics (tool/call returns chunked content)

---

*Architecture research for: tradedesk-miner (Rust quant data-mining engine)*
*Researched: 2026-05-15*
