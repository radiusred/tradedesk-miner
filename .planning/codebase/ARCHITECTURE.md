<!-- refreshed: 2026-05-25 -->
# Architecture

**Analysis Date:** 2026-05-25

## System Overview

```text
┌──────────────────────────────────────────────────────────────────┐
│                       CLI Entry Point                            │
│  `crates/miner-cli/src/main.rs`  (miner binary)                  │
│  `crates/miner-bench/src/main.rs` (miner-bench binary)           │
└─────────────────┬────────────────────────────┬───────────────────┘
                  │                            │
                  ▼                            ▼
┌─────────────────────────────┐  ┌────────────────────────────────┐
│   Engine Facade             │  │   Sweep Runner                 │
│   `engine::run_one`         │  │   `sweep::run_sweep`           │
│   `engine/mod.rs`           │  │   `sweep/executor.rs`          │
│                             │  │   (rayon par_iter fanout)      │
└──────────┬──────────────────┘  └───────────────┬────────────────┘
           │                                      │
           ▼                                      ▼
┌─────────────────────────────────────────────────────────────────┐
│                    miner-core library                            │
│   `crates/miner-core/src/`                                       │
│                                                                  │
│  ┌──────────┐  ┌──────────┐  ┌─────────────────────────────┐   │
│  │ Scan     │  │ BarCache │  │  Scan Registry               │   │
│  │ Registry │  │ (Arrow   │  │  ANOM / CROSS / SEAS         │   │
│  │ registry │  │  IPC)    │  │  scan/anom, cross, seas      │   │
│  └──────────┘  └────┬─────┘  └─────────────────────────────┘   │
│                     │                                            │
│  ┌──────────────────┴──────────────────┐                        │
│  │   Aggregator  `aggregator.rs`        │                        │
│  │   RawBar → BarFrame (15m/1h/1d)      │                        │
│  └──────────────────┬──────────────────┘                        │
│                     │                                            │
│  ┌──────────────────┴──────────────────┐                        │
│  │   Reader trait  `reader.rs`          │                        │
│  │   (pluggable data source)            │                        │
│  └─────────────────────────────────────┘                        │
└──────────────────────────┬──────────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────────────┐
│   miner-reader-dukascopy   `crates/miner-reader-dukascopy/`      │
│   DukascopyReader: Reader impl                                   │
│   path: <root>/<SYMBOL>/<YYYY>/<MM>/<DD>_<side>.csv.zst          │
└──────────────────────────┬──────────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────────────┐
│   tradedesk-dukascopy cache  (external, read-only)               │
│   .csv.zst files per day per instrument per side                 │
└─────────────────────────────────────────────────────────────────┘

                Output (JSONL) → FindingSink → stdout / file
                Hygiene (bootstrap/null) → scan/hygiene/
                Schema artifacts → schemas/
```

## Component Responsibilities

| Component | Responsibility | File |
|-----------|----------------|------|
| `miner` CLI | Arg parsing, SIGINT handler, sink construction, exit codes | `crates/miner-cli/src/main.rs` |
| `engine::run_one` | Single scan end-to-end: preflight → gap detect → cache → scan → JSONL | `crates/miner-core/src/engine/mod.rs` |
| `sweep::run_sweep` | Sweep TOML fanout: manifest expand → rayon par_iter → BH-FDR → SweepSummary | `crates/miner-core/src/sweep/executor.rs` |
| `Reader` trait | Pluggable data source: `read_1m_bars`, `fingerprint_day`, `enumerate_days` | `crates/miner-core/src/reader.rs` |
| `DukascopyReader` | Concrete Reader for tradedesk-dukascopy zstd CSV cache | `crates/miner-reader-dukascopy/src/reader.rs` |
| `aggregator` | Pure function: `RawBar` stream → columnar `BarFrame` at 15m/1h/1d | `crates/miner-core/src/aggregator.rs` |
| `BarCache` | Derived-bar Arrow IPC cache with blake3 invalidation sidecar | `crates/miner-core/src/cache.rs` |
| `GapDetector` | Scan the source for missing/corrupt days and intra-day holes | `crates/miner-core/src/gap.rs` |
| `Registry` + `bootstrap` | Versioned `(id, version)` → `Box<dyn Scan>` catalogue | `crates/miner-core/src/scan/registry.rs` |
| `Scan` trait | Polymorphic kernel: `run(&ctx, &req, &mut sink)` | `crates/miner-core/src/scan/mod.rs` |
| `ScanCtx` | Per-run broker: `bars`, `bars_pair`, `run_id`, `cancel`, `gap_manifest` | `crates/miner-core/src/scan/mod.rs` |
| ANOM family | 11 single-leg statistical scans | `crates/miner-core/src/scan/anom/` |
| CROSS family | 5 two-leg (Pair arity) scans | `crates/miner-core/src/scan/cross/` |
| SEAS family | 6 single-leg seasonality scans | `crates/miner-core/src/scan/seas/` |
| Hygiene kernels | Bootstrap CI, null p-value, BH-FDR, IAAFT, effect size | `crates/miner-core/src/scan/hygiene/` |
| `FindingSink` | JSONL serialisation + flush contract | `crates/miner-core/src/findings/sink.rs` |
| `Finding` envelope | Tagged-union wire type (6 variants) for all miner output | `crates/miner-core/src/findings/mod.rs` |
| `miner-bench` | Recipe runner: TOML manifest → `run_sweep` → one JSON timing line | `crates/miner-bench/src/main.rs` |
| Criterion benches | Per-kernel microbenchmarks | `crates/miner-core/benches/` |
| `xtask` | Dev-only: `gen-schema` regenerates `schemas/*.schema.json` | `xtask/src/main.rs` |
| `miner-mcp` / `miner-http` | Placeholder stubs, deferred to v2 | `crates/miner-mcp/src/main.rs`, `crates/miner-http/src/main.rs` |

## Pattern Overview

**Overall:** Layered library + thin binary wrappers with pluggable reader trait and JSONL streaming output.

**Key Characteristics:**
- `miner-core` is a sync-only, tokio-free library (FOUND-04 invariant). All compute is via `rayon`; I/O wrappers (CLI, HTTP, MCP) are separate binary crates.
- Every scan is a `Box<dyn Scan>` registered in a `BTreeMap`-keyed `Registry`; new scans plug in via the per-family `register_*_scans` helpers without touching central code.
- All output flows through a single `FindingSink` abstraction — `StdoutSink` is the only type that holds `io::stdout()`. `println!`/`print!`/`eprintln!`/`dbg!` are workspace-banned by `clippy.toml`.
- The derived-bar `BarCache` is the only writable state the miner owns. All source data is read-only via the `Reader` trait.
- Determinism is a first-class invariant: all `Serialize` maps are `BTreeMap`, RNG uses `Xoshiro256PlusPlus` seeded deterministically, `serde_json` insertion-order feature is never enabled.

## Layers

**CLI/Binary Layer:**
- Purpose: Parse args, construct reader, install SIGINT handler, dispatch to engine or sweep, compute exit code
- Location: `crates/miner-cli/src/`, `crates/miner-bench/src/`
- Contains: clap argument types, `DukascopyReader` construction, `FindingSink` construction, `compute_exit_code`
- Depends on: `miner-core`, `miner-reader-dukascopy`
- Used by: end users, CI, the quant agent

**Engine Facade Layer:**
- Purpose: Single entry point for one scan run — preflight, framing, gap dispatch, bar loading, scan dispatch, hygiene pass, JSONL streaming
- Location: `crates/miner-core/src/engine/mod.rs`
- Contains: `run_one`, `run_one_with_registry`, `dispatch_pair_arity_body`, hygiene mutation helpers
- Depends on: `BarCache`, `Reader`, `GapDetector`, `Registry`, `FindingSink`, hygiene kernels
- Used by: `miner-cli`, `sweep::run_sweep`, `miner-bench`

**Sweep Layer:**
- Purpose: TOML manifest cartesian expansion + rayon-parallel job fanout + BH-FDR aggregation
- Location: `crates/miner-core/src/sweep/`
- Contains: `SweepManifest`, `ResolvedJob`, `run_sweep`, `JobSink` wrapper (suppresses per-job `RunStart`/`RunEnd`)
- Depends on: `engine::run_one_with_registry`, rayon, `FindingSink`
- Used by: `miner sweep` subcommand, `miner-bench`

**Scan Kernel Layer:**
- Purpose: Polymorphic statistical kernels implementing `Scan::run`
- Location: `crates/miner-core/src/scan/`
- Contains: `Scan` trait, `ScanCtx`, `ScanRequest`, `ScanError`, 23 registered scan implementations
- Depends on: `BarFrame`, `FindingSink`, hygiene kernels
- Used by: engine facade

**Hygiene Layer:**
- Purpose: Post-scan statistical hygiene — bootstrap CI, null p-value, BH-FDR, seed derivation
- Location: `crates/miner-core/src/scan/hygiene/`
- Contains: `bootstrap.rs`, `null.rs`, `fdr.rs`, `effect_size.rs`, `seed.rs`
- Depends on: `rand_xoshiro`, `realfft`, primitive slice types only (no IO, no serde, no Reader)
- Used by: engine facade (`apply_hygiene_mutations`)

**Cache/Aggregator Layer:**
- Purpose: Materialise and invalidate derived-bar Arrow IPC files; pure aggregation of `RawBar` → `BarFrame`
- Location: `crates/miner-core/src/cache.rs`, `crates/miner-core/src/aggregator.rs`
- Contains: `BarCache::get_or_build`, `FingerprintSidecar`, `aggregate`, `BarFrame`, `Timeframe`
- Depends on: `Reader`, `arrow-rs`, `blake3`, `tempfile`
- Used by: engine facade

**Reader Layer:**
- Purpose: Pluggable data source abstraction; concrete Dukascopy impl reads zstd CSV files
- Location: `crates/miner-core/src/reader.rs` (trait), `crates/miner-reader-dukascopy/src/` (impl)
- Contains: `Reader` trait, `RawBar`, `Side`, `InstrumentSpec`, `DukascopyReader`
- Depends on: `csv`, `zstd`, `walkdir`, `chrono`
- Used by: engine facade, `BarCache`

**Finding/Wire Layer:**
- Purpose: Locked output envelope types and JSONL sink implementations
- Location: `crates/miner-core/src/findings/`
- Contains: `Finding` enum (6 variants), `FindingSink` trait, `StdoutSink`, `FileSink`, `RunId`, `Effect`, `Raw`, `DataSlice`
- Depends on: `serde`, `serde_json`, `schemars`, `ulid`
- Used by: every layer above (all output is typed `Finding`)

## Data Flow

### Single Scan Request (`miner scan`)

1. **CLI parsing** (`crates/miner-cli/src/main.rs:52`) — SIGINT handler installed first (`ctrlc`), then `Cli::parse()`
2. **Config resolution** — `MinerConfig::resolve(toml_path, overrides)` via `figment`
3. **Args → ScanRequest** — `ScanArgs::to_scan_request` in `crates/miner-cli/src/scan_args.rs`
4. **Reader construction** — `DukascopyReader::new(cfg.cache_root)` at the CLI boundary
5. **`engine::run_one`** (`crates/miner-core/src/engine/mod.rs:182`) — 8-step facade:
   - Preflight: `preflight::resolve_scan`, `validate_arity`, `validate_hygiene_support`
   - `RunStart` emitted via `FindingSink::write_envelope`
   - `GapDetector::detect` → `gap_policy::dispatch_at_timeframe`
   - Per sub-range: `BarCache::get_or_build` → `ScanCtx` construction → `scan.run(&ctx, &req, &mut sink)`
   - Hygiene pass (if requested): `apply_hygiene_mutations` → bootstrap CI + null p-value
   - `RunEnd` emitted; sink flushed (single flush point, Pitfall 5)
6. **Exit code** computed from `RunOutcome` + cancel flag (`compute_exit_code`)

### Sweep Request (`miner sweep`)

1. **CLI parsing** → `SweepArgs::to_manifest` → `SweepManifest`
2. **`sweep::run_sweep`** (`crates/miner-core/src/sweep/executor.rs`):
   - `validate` manifest (preflight)
   - `job_graph::expand` → `Vec<ResolvedJob>` (cartesian: scan × instruments × timeframes × windows × params)
   - `rayon::par_iter` over jobs → each worker calls `run_one_with_registry` into a `JobSink` buffer
   - Sequential deterministic-order drain from per-job buffers to the real sink
   - BH-FDR aggregation → `Finding::SweepSummary` emitted
   - `RunEnd` + flush

### Cache Population

1. `BarCache::get_or_build` checks `FingerprintSidecar` for blake3 per-day match
2. On mismatch: `Reader::read_1m_bars` → `aggregator::aggregate` → Arrow IPC write via `write_arrow_to_tempfile` + `persist_arrow_tempfile` (atomic rename)
3. Sidecar written after Arrow file (crash-safe ordering: stale sidecar → rebuild, not silent corruption)

**State Management:**
- No global mutable state. The cancel flag (`Arc<AtomicBool>`) is the only cross-boundary shared mutable value.
- `BarCache` is a path-keyed handle with no in-memory state between requests.
- `Registry` is constructed fresh per invocation via `bootstrap()` — no singleton.

## Key Abstractions

**`Reader` trait (`crates/miner-core/src/reader.rs:295`):**
- Purpose: Decouples miner-core from any specific data format or file layout
- Methods: `read_1m_bars`, `fingerprint_day`, `enumerate_days`, `source_id`, `trading_calendar`
- Invariant: `Send + Sync`; `dyn Reader<Error = E>` object-safe (pinned by compile-time test)
- Current impl: `DukascopyReader` in `crates/miner-reader-dukascopy/`

**`Scan` trait (`crates/miner-core/src/scan/mod.rs:204`):**
- Purpose: Polymorphic statistical kernel; all 23 registered scans share one dispatch path
- Methods: `id()`, `version()`, `arity()`, `param_schema()`, `finding_fields()`, `run()`, `supports_bootstrap()`, `supports_null_method()`
- Invariant: `Send + Sync`; `dyn Scan` object-safe (pinned by compile-time test)
- Arity: `ScanArity::Single` (ANOM/SEAS) or `ScanArity::Pair` (CROSS)

**`FindingSink` trait (`crates/miner-core/src/findings/sink.rs:35`):**
- Purpose: Single serialisation boundary for all miner output
- Methods: `write_envelope(&Finding)`, `write_raw_json(&Value)`, `flush()`
- Invariant: `Send`; per-envelope flush; `StdoutSink` is the sole `io::stdout()` holder

**`BarFrame` (`crates/miner-core/src/aggregator.rs`):**
- Purpose: Columnar in-memory representation of aggregated OHLCV bars
- Fields: `ts_open_utc: Vec<DateTime<Utc>>`, `open/high/low/close/tick_volume: Vec<f64>`, `source_id`, `symbol`, `side`, `tf`
- Used by: `ScanCtx`, all scan kernels, Arrow IPC cache

**`Finding` enum (`crates/miner-core/src/findings/mod.rs`):**
- Purpose: Tagged-union wire type; `#[serde(tag = "kind")]` discriminated
- Variants: `RunStart`, `Result(ResultFinding)`, `ScanError`, `GapAborted`, `RunEnd`, `DryRun`, `SweepSummary`
- All `Serialize` maps within the tree are `BTreeMap` (determinism, OUT-03)

**`ScanCtx` (`crates/miner-core/src/scan/mod.rs:310`):**
- Purpose: Per-run broker passed to `Scan::run`; provides bars, cancel flag, run metadata
- Key method: `bars_up_to(ts)` returns a `BarFrameView` truncated at the cutoff (look-ahead safety)

## Entry Points

**`miner` binary:**
- Location: `crates/miner-cli/src/main.rs`
- Subcommands: `scan <scan_id@version> [args]`, `sweep <manifest.toml>`, `scans`, `emit-fixture`
- Triggers: interactive CLI, CI, quant agent subprocess

**`miner-bench` binary:**
- Location: `crates/miner-bench/src/main.rs`
- Invocation: `miner-bench --recipe <path.toml> [--warmup N] [--runs N]`
- Triggers: `hyperfine` for wall-clock benchmarks, dhat heap profiling (`--features dhat`)

**Criterion benches:**
- Location: `crates/miner-core/benches/`
- Files: `bench_zstd_decompress_1day.rs`, `bench_csv_parse_1day.rs`, `bench_aggregate_1m_to_15m.rs`, `bench_rolling_corr.rs`, `bench_ols_fit_4d.rs`, `bench_ljung_box.rs`
- Triggers: `cargo bench`

**`xtask gen-schema`:**
- Location: `xtask/src/main.rs`
- Triggers: CI schema-sync gate; regenerates `schemas/findings-v1.schema.json`, `schemas/scans-catalogue-v1.schema.json`, `schemas/sweep-manifest-v1.schema.json`

## Scan Catalogue (23 registered scans)

**ANOM family (11, `ScanArity::Single`):**
- `stats.autocorr.ljung_box` — Ljung-Box Q test
- `stats.autocorr.ljung_box_sq` — Squared-returns Ljung-Box
- `stats.drawdown.profile` — Drawdown profile
- `stats.heteroskedasticity.arch_lm` — ARCH-LM test
- `stats.normality.jarque_bera` — Jarque-Bera normality test
- `stats.outliers.z_and_mad` — Z-score + MAD outlier detection
- `stats.returns.profile` — Returns distribution profile
- `stats.stationarity.adf` — Augmented Dickey-Fuller test
- `stats.stationarity.kpss` — KPSS stationarity test
- `stats.summary.welford` — Summary statistics (Welford online algorithm)
- `stats.variance_ratio` — Variance ratio test (Lo-MacKinlay)
- `stats.vol.rolling` — Rolling volatility

**CROSS family (5, `ScanArity::Pair`):**
- `cross.cointegration.engle_granger` — Engle-Granger cointegration test
- `cross.corr.pearson_rolling` — Rolling Pearson correlation
- `cross.corr.spearman_rolling` — Rolling Spearman correlation
- `cross.lead_lag.ccf` — Lead-lag cross-correlation function
- `cross.ols.rolling` — Rolling OLS regression

**SEAS family (6, `ScanArity::Single`):**
- `seas.bucket.day_of_week` — Day-of-week seasonality
- `seas.bucket.eom_som` — End/start of month seasonality
- `seas.bucket.hour_of_day` — Hour-of-day seasonality
- `seas.bucket.session` — Trading session seasonality
- `seas.event.pre_post_window` — Event-window analysis
- `seas.test.anova_kruskal` — ANOVA + Kruskal-Wallis meta-scan

## Architectural Constraints

- **Threading:** CPU compute is `rayon` (work-stealing thread pool). No tokio or async runtime inside `miner-core` (FOUND-04 invariant). Async transports (HTTP/MCP) are deferred to v2.
- **Global state:** No module-level singletons. `Arc<AtomicBool>` cancel flag is the only cross-boundary shared mutable value, constructed per invocation in `main()`.
- **Circular imports:** None by construction — layering enforces a strict direction: CLI → engine → scan kernels → primitives. `miner-core` does NOT depend on `miner-reader-dukascopy`.
- **Stdout discipline:** `println!`/`print!`/`eprintln!`/`dbg!` are workspace-banned in `clippy.toml`. All output goes through `FindingSink`; all tracing goes to stderr.
- **Determinism:** All `Serialize` maps must be `BTreeMap` (never `HashMap`). The `serde_json` `insertion-order` feature is explicitly disabled in `Cargo.toml`. RNG is `Xoshiro256PlusPlus` seeded from deterministic job seeds.
- **Unsafe code:** `unsafe_code = "forbid"` in `[workspace.lints.rust]`. No unsafe anywhere in the workspace.

## Anti-Patterns

### Direct stdout/stderr writes inside library code

**What happens:** Using `println!`, `eprintln!`, `print!`, or `dbg!` anywhere in `miner-core` or scan kernels.
**Why it's wrong:** Pollutes the JSONL stream consumers parse. stdout is the wire; any non-JSONL byte corrupts the output. Tracked as threat T-01-03.
**Do this instead:** Use `tracing::{info,debug,warn,error}!` macros only. Output to `FindingSink::write_envelope` (findings) or `engine::error::stderr_emit::emit_to_stderr` (preflight errors only). See `crates/miner-core/src/findings/sink.rs:13`.

### Using HashMap in serialised types

**What happens:** Using `HashMap<K, V>` in any struct that derives `Serialize` (findings, manifests, summaries).
**Why it's wrong:** HashMap iteration order is non-deterministic; byte-stable output (OUT-03, HYG-05 rerun identity) breaks.
**Do this instead:** Use `BTreeMap<K, V>` in all `Serialize` paths. See `crates/miner-core/src/findings/mod.rs:41`.

### Constructing Reader inside miner-core

**What happens:** Instantiating `DukascopyReader` (or any concrete Reader) inside `miner-core`.
**Why it's wrong:** `miner-core` must remain reader-agnostic. Hard-coding a reader couples the library to a specific cache layout.
**Do this instead:** Construct the reader at the binary edge (CLI boundary). Pass `&dyn Reader` or `&impl Reader` into engine functions. See `crates/miner-cli/src/main.rs:381`.

### Calling `Utc::now()` inside scan kernels

**What happens:** Scan kernels reading wall-clock time directly.
**Why it's wrong:** Violates the clock-isolation invariant (D3-23). `Utc::now()` is read exactly twice per `run_one` invocation (at `RunStart` and `RunEnd`); kernels must not read the clock.
**Do this instead:** Use the `run_id` and `produced_at_utc` from the engine's framing layer only.

### Adding tokio/async dependencies to miner-core

**What happens:** Adding `tokio` or any async runtime to `miner-core`'s dependency tree.
**Why it's wrong:** Violates FOUND-04. CPU-bound rayon workers and async runtimes fight over threads, starving IO workers. `cargo tree -p miner-core | grep -E 'tokio|async-std'` must return nothing.
**Do this instead:** Keep miner-core sync+std+rayon. Async is for transport wrappers (miner-http/miner-mcp) only.

## Error Handling

**Strategy:** `thiserror` enums in library crates; `anyhow` in binary entry points.

**Patterns:**
- `MinerError` (`crates/miner-core/src/error/mod.rs`) — typed library error enum with variants `Io`, `Serialize`, `Config`, `Scan(String)`, `Preflight(WireError)`, `Internal(String)`
- Preflight failures return `Err(MinerError::Preflight(WireError))` — emitted as structured JSON to stderr, never to stdout
- Per-finding scan errors are wrapped as `Finding::ScanError` envelopes and streamed inline; the run continues
- `ScanError` enum (`crates/miner-core/src/scan/mod.rs:626`) — `Kernel(String)`, `Io(std::io::Error)`, `Cancelled`, `Miner(MinerError)`
- Reader and cache errors are wrapped as `MinerError::Scan(format!("reader: {e}"))` / `"cache: {e}"` at the engine boundary
- The engine always emits `RunEnd` before returning, even on scan errors — consumers never see an orphaned `RunStart`

## Cross-Cutting Concerns

**Logging:** `tracing` crate with `tracing-subscriber` → `io::stderr()`. `EnvFilter` respects `RUST_LOG` (default `info`). Spans model scan/instrument nesting under parallelism.

**Validation:** JSON Schema artifacts live in `schemas/`; regenerated by `cargo xtask gen-schema`; diffed in CI. Per-scan `param_schema()` returns a `serde_json::Value` fragment. `jsonschema` crate used for runtime validation.

**Authentication:** None. The miner reads from a local filesystem cache; all path access is mediated by `MinerConfig.cache_root` and `bar_cache_root`.

**Cancellation:** `Arc<AtomicBool>` cancel flag installed by `ctrlc` handler before `Cli::parse()`. Three documented yield sites per `run_one`: `cancel_at_entry`, `cancel_before_subrange`, `cancel_inside_scan_kernel`. SIGINT always maps to exit code 130 regardless of `RunOutcome`.

---

*Architecture analysis: 2026-05-25*
