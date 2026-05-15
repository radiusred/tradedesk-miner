# Stack Research

**Domain:** High-performance Rust quantitative data-mining engine (OHLCV scans, agent-operated)
**Researched:** 2026-05-15
**Confidence:** MEDIUM (see Research Constraints below)

---

## Research Constraints

> **IMPORTANT — Read this before trusting versions.**
>
> In this research session, WebSearch / WebFetch / Bash were all denied by the
> sandbox, so I could not verify crate versions against `crates.io`, GitHub
> releases, or Context7 at the moment of writing. Recommendations below are
> grounded in training data current to early 2026 and reflect the well-known
> "obvious" choices in the Rust quant/data ecosystem — but **versions are
> "as-of last known" and MUST be re-pinned with `cargo add <crate>` (which
> resolves the live current version) before the first commit of `Cargo.toml`.**
>
> Items marked **(VERIFY)** below are the ones I am least confident about and
> should be checked explicitly. Items marked **(STABLE)** are mature, slow-moving
> crates where the recommendation is robust even if the patch version drifts.
>
> Confidence levels assigned to each recommendation reflect this constraint.

---

## TL;DR — The Stack

| Layer | Pick | Confidence |
|------|------|-----------|
| CSV parsing | `csv` + `csv-core` with `serde` derives | HIGH |
| zstd decompression | `zstd` (zstd-rs, libzstd binding) | HIGH |
| Numerics / arrays | `ndarray` + `ndarray-stats` | HIGH |
| Linear algebra (regression) | `nalgebra` (small fixed) + `ndarray-linalg` (large) | MEDIUM |
| Statistics primitives | `statrs` + custom rolling kernels | HIGH |
| Parallelism (CPU) | `rayon` | HIGH |
| Async (IO + servers) | `tokio` (multi-thread runtime) | HIGH |
| CLI | `clap` v4 (derive) | HIGH |
| HTTP server | `axum` 0.7+ over `tokio` + `tower` | HIGH |
| MCP server | `rmcp` (official-ish modelcontextprotocol/rust-sdk) **(VERIFY)** | MEDIUM |
| Serialization | `serde` + `serde_json` (output), `simd-json` optional | HIGH |
| JSONL streaming | `serde_json::to_writer` + `\n` + `BufWriter` | HIGH |
| On-disk bar cache | **Arrow IPC files** via `arrow-rs` (one file per symbol/resolution/side) | MEDIUM |
| Memory-mapped IO | `memmap2` | HIGH |
| Bench harness | `criterion` (microbench) + `divan` (alt) + bespoke "scan harness" binary | HIGH |
| Logging | `tracing` + `tracing-subscriber` | HIGH |
| Errors | `thiserror` (library) + `anyhow` (binaries) | HIGH |
| Config | `figment` or `config` + `serde` | MEDIUM |
| Time | `jiff` (preferred) **or** `chrono` (fallback) | MEDIUM |
| File listing / globbing | `walkdir` + `globset` | HIGH |
| Hashing (cache keys) | `blake3` | HIGH |
| Test data | `proptest` + `insta` snapshots | HIGH |

---

## Recommended Stack — Detailed

### Core Technologies

| Technology | Version (as of 2026-05) | Purpose | Why Recommended |
|------------|------------------------|---------|-----------------|
| **Rust toolchain** | 1.85+ (2024 edition) | Language | Edition 2024 stabilises `async fn in trait` ergonomics, RPIT, etc.; pick stable, not nightly. |
| **`tokio`** | 1.40+ | Async runtime for HTTP + MCP wrappers | De facto async runtime; multi-thread scheduler isolates IO from `rayon`'s compute pool cleanly. |
| **`rayon`** | 1.10+ | Data-parallel scans | Work-stealing thread pool; ideal for the embarrassingly-parallel fanout (instrument × timeframe × params). `par_iter` over scan grids is the workload's natural shape. |
| **`ndarray`** | 0.16+ | N-dim numerical arrays | The standard for numerical Rust outside of DataFrame-shaped problems. Views, slicing, broadcast — exactly what rolling-window stats need over OHLCV columns. Lighter and more transparent than Polars for kernel work. |
| **`ndarray-stats`** | 0.6+ | Mean/var/quantile/correlation over ndarray | Avoids reinventing standard reductions; correlation matrices, summary stats land here. |
| **`nalgebra`** | 0.33+ | Small fixed-size linear algebra | Best ergonomics for the kinds of regressions you actually run (OLS βs are 2–10 dim); compile-time-sized matrices = no heap. |
| **`statrs`** | 0.17+ | Distributions + statistical tests | T, F, χ², normal CDFs/PDFs needed for p-values on regression / cointegration / Ljung-Box. Mature, no surprises. |
| **`csv`** | 1.3+ | CSV reader | Burntsushi crate; effectively the only choice. `csv-core` if you want SIMD-y zero-alloc parsing on the hot path. |
| **`zstd`** | 0.13+ | Zstd decompression | Bindings to upstream libzstd — fastest path, mature, streaming `Decoder<R: Read>` plugs straight into `csv::ReaderBuilder::from_reader`. |
| **`serde`** | 1.0+ | Ser/de framework | Derives for OHLCV rows, finding output, config files. Universal. |
| **`serde_json`** | 1.0+ | JSON / JSONL output | Stable, fast enough; `to_writer` + newline gives streaming JSONL essentially for free. |
| **`arrow-rs` (`arrow` + `arrow-ipc`)** | 53+ | Aggregated-bar cache format | Columnar in-memory layout matches the access pattern (load one column at a time); IPC file format is stable, zero-copy mmap-friendly, language-portable for free interop with Python (Polars/Pandas) later. |
| **`memmap2`** | 0.9+ | Memory-mapped reads | Bar cache files and large raw inputs benefit from `mmap` + page-cache reuse across parallel scans. |
| **`clap`** | 4.5+ | CLI parsing | Derive macros for the thin CLI wrapper; subcommand model fits "scan", "sweep", "cache", "bench". |
| **`axum`** | 0.7+ | HTTP framework | Tower-based, type-safe extractors, streaming responses (`Sse`, `Body::from_stream`) — fits the JSONL-streaming output model cleanly. |
| **`tower`** / **`tower-http`** | latest | Middleware | Tracing, compression, timeouts — standard middleware layer. |
| **`rmcp`** *(VERIFY)* | 0.x | MCP server SDK | The Rust MCP SDK lives at `modelcontextprotocol/rust-sdk`; the crate name is `rmcp`. It's the closest thing to "official" and the way to expose tools to MCP clients. See **MCP** section below — this is the highest-risk dependency. |
| **`tracing`** | 0.1.40+ | Structured logging + spans | Standard. Spans naturally model scan / instrument / timeframe nesting; emits to stderr so it doesn't pollute the stdout JSONL stream. |
| **`thiserror`** | 1.0+ | Library error enums | Library crate gets typed errors; wrappers convert to `anyhow::Error`. |
| **`anyhow`** | 1.0+ | Binary error glue | The CLI, MCP server, HTTP server use `anyhow` at the edge. |

### Supporting Libraries

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `criterion` | 0.5+ | Microbenchmarks | Per-kernel benches: parse-1-day, decompress-1-day, aggregate-1m→1h, rolling-corr-N. |
| `divan` | 0.1+ | Alternate bench framework | Faster iteration, simpler reports than criterion; use if criterion's HTML reports feel overkill. |
| `walkdir` | 2.5+ | Recursive cache scan | Discover `<root>/<SYMBOL>/<YYYY>/<MM>/<DD>_<side>.csv.zst` files. |
| `globset` | 0.4+ | Pattern filters | Symbol / date-range filters from CLI. |
| `jiff` | 0.1+ / 0.2+ | Timezone-aware datetimes | Newer than chrono, designed by burntsushi, correct timezone semantics. **VERIFY** stability for your toolchain — `chrono` is the safe fallback. |
| `chrono` | 0.4+ | Datetimes (fallback) | If `jiff` feels too new; rock-stable but timezone API is more fiddly. |
| `figment` | 0.10+ | Config from TOML + env + CLI | Layered configuration for cache root, parallelism caps, etc. |
| `parking_lot` | 0.12+ | Faster `Mutex`/`RwLock` | If contention shows up; drop-in replacement for std primitives. |
| `dashmap` | 6+ | Concurrent hashmap | Symbol → AggregatedBarFrame cache index, shared across worker threads. |
| `blake3` | 1+ | Hashing | Cache keys derived from `(source, symbol, resolution, side, version, params)` — fast, deterministic, collision-safe. |
| `bytes` | 1+ | Zero-copy byte buffers | If you do framed protocol work or stream large slices around. |
| `simd-json` | 0.13+ | SIMD JSON encoder | Optional swap-in for `serde_json` if profiling shows JSON encoding is hot at high finding throughput. |
| `flume` / `crossbeam-channel` | latest | MPMC channels | Worker → output formatter pipeline if you want to decouple scan threads from the stdout writer. `crossbeam-channel` is the conservative pick. |
| `tokio-stream` | 0.1+ | Async streams | Bridge sync rayon scan output → axum `Body::from_stream` for HTTP. |
| `serde_jsonlines` | 0.6+ | JSONL helpers | Optional sugar; trivial to inline yourself. |
| `proptest` | 1+ | Property tests | Aggregation invariants (e.g. `aggregate(1m, 15m).len() == raw.len() / 15`, OHLC monotonicity). |
| `insta` | 1+ | Snapshot tests | Finding output schema stability — critical because consumers parse it. |
| `tempfile` | 3+ | Test scratch dirs | Cache tests. |
| `assert_cmd` + `predicates` | latest | CLI integration tests | End-to-end testing the thin CLI wrapper. |

### Development Tools

| Tool | Purpose | Notes |
|------|---------|-------|
| `cargo-nextest` | Faster test runner | Massively faster than `cargo test` once you have integration tests. |
| `cargo-deny` | License + advisory checks | Apache-2.0 hygiene + RustSec advisories in CI. |
| `cargo-machete` | Detect unused deps | Keeps `Cargo.toml` lean as the stack evolves. |
| `cargo-flamegraph` | Flamegraphs from benches | Essential for "lightning fast" claims. |
| `samply` | Sampling profiler | Modern, easy, Firefox-profiler UI; complements flamegraph. |
| `hyperfine` | CLI wall-clock benchmarks | End-to-end "scan X over Y" timings for the README. |
| `cargo-llvm-cov` | Coverage | Statistical kernels deserve coverage. |
| `clippy` / `rustfmt` | Lints + format | CI gate; `clippy::pedantic` selectively on the library crate. |

---

## Installation

Rust uses `cargo add`, not `npm install`. The right way to populate `Cargo.toml` is to let `cargo` resolve the current version:

```bash
# Workspace setup
cargo new --lib tradedesk-miner
cd tradedesk-miner

# Core library deps
cargo add csv zstd ndarray ndarray-stats statrs nalgebra
cargo add serde --features derive
cargo add serde_json
cargo add rayon
cargo add memmap2 walkdir globset
cargo add tracing thiserror
cargo add blake3
cargo add arrow arrow-ipc
cargo add jiff           # or: cargo add chrono --features serde

# Wrapper deps (in separate binary crates or behind features)
cargo add tokio --features full
cargo add axum tower tower-http
cargo add clap --features derive
cargo add anyhow
cargo add tracing-subscriber --features env-filter,fmt
cargo add rmcp           # VERIFY: confirm crate name + version against crates.io

# Dev deps
cargo add --dev criterion --features html_reports
cargo add --dev proptest insta tempfile
cargo add --dev assert_cmd predicates
```

Workspace layout (recommended):

```
tradedesk-miner/
  Cargo.toml                  # workspace
  crates/
    miner-core/               # library — readers, aggregation, scans, findings
    miner-cli/                # binary — thin clap wrapper
    miner-http/               # binary — axum server
    miner-mcp/                # binary — rmcp server
    miner-bench/              # binary — scan harness (separate from criterion microbenches)
  benches/                    # criterion microbenches on miner-core
```

---

## Deep-Dives — The Contested Choices

### Numerics: `ndarray` vs `nalgebra` vs `polars`

**Recommended:** `ndarray` (+ `ndarray-stats`) as the primary, `nalgebra` for small fixed-size linear algebra, **not** `polars` for the hot path.
**Confidence:** HIGH

**Rationale:**

- The miner's hot path is **rolling kernels over 1-D price/volume slices**, not DataFrame operations. Slice an OHLCV column, run a kernel over a window, emit stats — this is exactly `ndarray`'s sweet spot.
- `nalgebra` shines for *small fixed-size* algebra: OLS β for a 2–6 parameter regression, covariance inversion for cointegration tests. Stack-allocated, no heap pressure inside parallel workers.
- `polars` is excellent and tempting, **but** for this workload it imposes:
  - A heavy DataFrame abstraction over what is fundamentally column-of-floats math
  - Expression-graph compilation that doesn't amortise well in tight `par_iter` loops
  - Conflation of "data format" and "compute engine" — you'd end up doing kernels outside Polars anyway and converting back
- Use Polars/Arrow for **cache storage** (see Aggregated-Bar Cache Format below) but **not** as the in-flight computation type.

**When to reconsider:** If you later add SQL-shaped exploratory tooling for analysts directly (`miner-explore` binary), Polars in that crate is fine. The core scan engine stays on `ndarray`.

### Parallelism: `rayon` vs `crossbeam` vs `tokio`

**Recommended:** `rayon` for the scan fanout; `tokio` only inside the wrapper binaries (HTTP, MCP); `crossbeam-channel` for the worker→writer fan-in.
**Confidence:** HIGH

**Rationale:**

- The workload is **CPU-bound, embarrassingly parallel**: scans over instruments × timeframes × parameter grids. `rayon::par_iter` over a flattened task vector is the textbook fit, and its work-stealing scheduler handles uneven task durations (long-history pairs vs short ones) for free.
- `tokio` is the wrong tool for CPU-bound compute — it will block its workers and starve IO. The right pattern:
  - **HTTP / MCP server**: `tokio` runtime accepts requests
  - For each request: spawn the scan into a `rayon::spawn` or `tokio::task::spawn_blocking` that uses `rayon::par_iter` internally
  - Findings stream back via a `crossbeam-channel::unbounded::<Finding>` whose receiver is bridged to a `tokio_stream::wrappers::UnboundedReceiverStream` for axum response bodies
- `crossbeam` (`crossbeam-channel`, `crossbeam-utils`) gives you the building blocks if you ever outgrow Rayon's `par_iter` model (e.g. true pipelining of decompress → parse → aggregate → scan stages). Start with rayon; reach for crossbeam only when profiling demands it.

**When to reconsider:** Never replace rayon for the scan fanout. Replace `tokio::spawn_blocking + rayon` with a custom pipeline only after profiling proves a bottleneck.

### MCP Server SDK

**Recommended:** `rmcp` (the crate from `modelcontextprotocol/rust-sdk`). **(VERIFY)**
**Confidence:** MEDIUM (low on version, high on "this is the one to use")

**Rationale:**

- The MCP organisation publishes official-or-officially-blessed SDKs in multiple languages. The Rust SDK lives at `github.com/modelcontextprotocol/rust-sdk` and publishes the `rmcp` crate. As of early 2026 it is the canonical choice.
- Alternatives (`mcp_rust`, `mcp-server` community forks) exist but lag the spec; **don't pick a community port** when an org-blessed SDK exists.

**What to verify before adopting:**
- Run `cargo add rmcp` and inspect the version printed.
- Check `rmcp`'s GitHub README for transport support — you need at least `stdio` (for local agent use) and `streamable-http` / `sse` (for the remote Paperclip agent).
- Confirm it supports streaming tool results — the miner's outputs are streams of findings, not single JSON blobs, so you need either incremental tool-result chunks or the SDK has to handle a long-running tool that produces multiple notifications.

**Risk mitigation:** Architect `miner-core` so that "produce findings" is a stream-returning function. Each wrapper (CLI/HTTP/MCP) is a 200-line adapter. If `rmcp` turns out to be limiting, swapping in a hand-rolled JSON-RPC stdio loop is a 1-day job, not a refactor.

### Aggregated-Bar Cache Format

**Recommended:** **Apache Arrow IPC files** (one file per `(source, symbol, resolution, side)`), via `arrow-rs`.
**Confidence:** MEDIUM (HIGH on "use a columnar binary format", MEDIUM on "Arrow IPC specifically")

**Rationale:**

- The cache is read **far more than written** (write once per resolution, read many times across scan sweeps). The access pattern is "load all bars for symbol X at resolution Y over date range Z" — i.e. range reads of a few columns. **Columnar wins.**
- **Arrow IPC files** specifically (not Parquet):
  - Zero-copy mmap reads — `arrow-ipc::reader::FileReader` + `memmap2` lets you read columns without decompression overhead.
  - Schema-stable: timestamp + 5 floats + (optional) volume; no schema evolution headaches.
  - Cross-language by default: Polars / Pandas / DuckDB / Python Arrow can read the same files when `tradedesk` eventually consumes them.
  - Self-describing: schema in the file, no out-of-band metadata.
  - Faster than Parquet for this access pattern (Parquet wins on compression, Arrow IPC wins on read speed without decompression).
- **Why not Parquet:** Compression overhead is a tax you pay every read for a write-once cache. The aggregated bars are not large enough (28 symbols × 6 years × 1440/15 ≈ 1.4 M rows per symbol/15m) to justify the squeeze.
- **Why not SQLite/DuckDB:** Row store (SQLite) is wrong shape. DuckDB is a query engine, not a storage primitive; it would re-introduce a heavy dependency for a write-once, range-scan cache.
- **Why not a custom binary format:** Tempting and might be ~10% faster, but you lose tooling interop forever. Arrow IPC is the right "fast and standard" pick.

**File layout proposal:**
```
<cache-root>/_derived/
  v1/
    dukascopy/
      EURUSD/
        15m/
          bid.arrow          # full history, single file, mmap-able
          ask.arrow
        1h/
          bid.arrow
          ask.arrow
        1d/
          bid.arrow
          ask.arrow
```
Plus an `index.json` per `(symbol, resolution, side)` for fast existence/version checks, and a `cache.lock` file for write coordination.

**Cache key hash:** `blake3(source_name || symbol || resolution || side || aggregation_version)` — 64-bit prefix is fine, written to filename or to the index for collision checks.

**When to reconsider:** If the bar cache grows past ~50 GB or read latency dominates, switch to Parquet with zstd compression — you get 5–10× smaller files with modest read overhead, and `arrow-rs` handles both.

### CSV + zstd reader plumbing

**Recommended:** `zstd::Decoder::new(File::open(path)?)` → wrap in `BufReader::with_capacity(1 MiB, _)` → feed to `csv::ReaderBuilder::new().has_headers(false).from_reader(_)` → deserialize rows with `serde` derives on a `RawBar` struct.
**Confidence:** HIGH

**Why this pipeline:**
- `zstd-rs` gives a streaming `Read` decoder — no need to decompress to a temp file or hold the full decompressed bytes in RAM.
- `csv` is line-oriented and works against any `Read`; it handles quote/escape edge cases and integrates with `serde` for zero-boilerplate parsing.
- A 1 MiB `BufReader` between the two amortises syscall and decode chunk sizes.

**Performance notes:**
- For peak throughput on the hot path consider `csv-core` (the byte-level parser under `csv`) to avoid `serde` reflection. Profile first — `serde` derives are usually fine.
- The `zstd` crate has a `zstdmt` feature for multi-threaded *decompression of a single stream*. **Don't enable it** — you already parallelise *across files* with rayon, which is more efficient than parallelising decompression within one file. Single-threaded decode per file × N files in parallel beats multi-threaded decode × 1 file.
- Consider an `--mmap-input` mode (`zstd::stream::read::Decoder::new(mmap_slice)`) — sometimes wins on warm caches, sometimes loses; bench it.

### Bench Harness

**Recommended:** Two-layer benchmarking — `criterion` for kernels, a bespoke `miner-bench` binary for end-to-end scan sweeps.
**Confidence:** HIGH

**Why two layers:**
- **`criterion`** owns the microbenches: `bench_zstd_decompress_1day`, `bench_csv_parse_1day`, `bench_aggregate_1m_to_15m_1symbol_1year`, `bench_rolling_corr_N`, `bench_ols_fit_4d`. These let you regression-test individual kernels; they're statistically rigorous and produce per-commit comparisons via `--save-baseline`.
- **`miner-bench`** is a binary that exercises real scan recipes (e.g. "lead-lag scan, 28 instruments × 3 timeframes × 6 years × 12 params") and prints wall-clock + throughput. This is what underwrites the "lightning fast" pitch in the README. Measured with `hyperfine` for stable wall-clock numbers.
- Keep `criterion` benches in `benches/` (it's the canonical layout), `miner-bench` as its own binary crate.

**Alternatives:**
- `divan` is a strong newer alternative; lighter than criterion, simpler output. Pick it if criterion's HTML reports feel like overkill for the team's workflow. Both are fine; **don't mix them** in one repo.
- `iai-callgrind` for instruction-counting benches (deterministic regression detection, no timing noise) — overkill for v1, consider later.

---

## Alternatives Considered

| Recommended | Alternative | When to Use Alternative |
|-------------|-------------|-------------------------|
| `ndarray` | `polars` | If you add a SQL-style exploratory CLI for analysts directly; not for the scan engine. |
| `ndarray` | `nalgebra` for everything | If your math becomes dominated by small fixed-size matrix ops; you'd still want ndarray for the column views. |
| `csv` crate | `csv-core` directly | If profiling shows `serde` deserialization is hot; switch the inner loop, keep `csv` for everything else. |
| `zstd` (libzstd binding) | `ruzstd` (pure Rust) | If you ever need a `no_std` / pure-Rust build (e.g. WASM). For a server-side binary, the C binding is faster. |
| `arrow-ipc` cache | `parquet` cache | When cache size becomes a constraint (>50 GB on disk) or when network-transfer of cache files matters. |
| `arrow-ipc` cache | `duckdb` | If you decide miner *does* own a query layer over derived data (currently de-scoped per PROJECT.md). |
| `rmcp` | Hand-rolled JSON-RPC over stdio | If `rmcp` lags spec, has unstable APIs, or doesn't support streaming tool results. ~1 day to implement minimal stdio JSON-RPC. |
| `axum` | `actix-web` | If you specifically need actor model; axum is the modern default and integrates better with `tokio`/`tower`. |
| `axum` | `poem` / `salvo` | Smaller communities; pick only if there's a feature you genuinely need. |
| `clap` | `argh` | If you want zero-dep, smallest-binary CLI; lose features. clap is fine. |
| `rayon` | manual `std::thread` + `crossbeam` | If you need true pipelined stages (decompress → parse → aggregate). Profile first. |
| `criterion` | `divan` | Personal preference; both are good. |
| `jiff` | `chrono` | If `jiff` feels too new; chrono is rock-stable and well-known. `time` is the third option — fine, less common in this domain. |
| `tracing` | `log` + `env_logger` | Smaller projects; you'll want spans for scan/instrument nesting once you have parallelism, so just start with tracing. |
| `serde_json` | `simd-json` | When profiling shows JSON encoding is hot at high finding throughput — drop-in-ish replacement. |
| `figment` | `config` crate | Similar feature set; pick one and move on. |
| `blake3` | `xxhash-rust` | If you need 64-bit non-cryptographic hash specifically; blake3 is fine and faster than people expect. |

---

## What NOT to Use

| Avoid | Why | Use Instead |
|-------|-----|-------------|
| `async-std` | Effectively unmaintained as of 2024+; `tokio` won the runtime war. | `tokio` |
| `failure` crate | Long deprecated. | `thiserror` + `anyhow` |
| `error-chain` | Predates the modern error story. | `thiserror` + `anyhow` |
| `chrono` 0.3.x and `time` 0.1.x | Old APIs, security advisories in older versions. | Modern `chrono` 0.4+, `jiff`, or `time` 0.3+ |
| `serde_json` for *every* output if throughput matters | Fine 95% of the time; can become hot at >1M findings/s. | Profile, then `simd-json` if needed. |
| `std::sync::Mutex` under heavy contention | Slower than parking_lot for hot contended locks. | `parking_lot::Mutex` (still drop-in for most code). |
| `tokio` for the scan compute itself | Async runtime + CPU-bound work = blocked workers, starved IO. | `rayon` for compute, `tokio` only at the wrapper edges. |
| `Polars` as the in-flight numerics type | Heavy DataFrame abstraction over what is column-of-floats math; expression graph doesn't amortise in tight parallel loops. | `ndarray` for kernels; Polars (or Arrow) only for storage and exploratory tooling. |
| `csv` with default `String` allocations on the hot path | Allocates per field unnecessarily for high-throughput parses. | Use `csv::ByteRecord` + manual parse, or `csv-core` directly. |
| `zstd` crate with `zstdmt` feature | Multi-threaded *single-file* decompression conflicts with rayon's *across-files* parallelism — worse cache locality. | Default single-threaded decoder + rayon over files. |
| Custom serialization format for findings | Consumers (quant agent, future tradedesk) need a stable parser; bespoke = brittle. | JSON / JSONL via serde; document the schema. |
| `mimalloc` / `jemalloc` reflexively | Allocator swaps help workloads with heavy small allocations; profile first. The default allocator is competitive for this workload. | Default `std` allocator; swap only if profiling proves win. |
| `PyO3` in v1 | Explicitly deferred in PROJECT.md; introduces ABI surface and CI complexity. | Defer until concrete `tradedesk` need. |
| Mixed `criterion` + `divan` in one repo | Confusing dual baselines. | Pick one. |
| `reqwest` in the core library | Library should be IO-agnostic; only the HTTP server wrapper needs HTTP plumbing. | Keep `reqwest`/`hyper` out of `miner-core`. |

---

## Stack Patterns by Variant

**If you want to ship v1 with the smallest dep surface:**
- Drop `simd-json`, `parking_lot`, `dashmap`, `figment` (use plain `serde` + `toml`).
- Skip `jiff`, use `chrono`.
- Skip `divan`, use only `criterion`.
- This trims ~8 crates and a few hundred KB of dep weight without changing the architecture.

**If you want maximum throughput at any complexity cost:**
- `csv-core` instead of `csv` on the hot parsing path.
- `simd-json` for output encoding.
- `mimalloc` global allocator (after benching).
- Add `iai-callgrind` instruction-count benches for regression detection.
- Consider a custom mmap-friendly bar cache format if Arrow IPC reads become a measured bottleneck (unlikely).

**If MCP support proves unstable in `rmcp`:**
- Build `miner-mcp` as a hand-rolled JSON-RPC-over-stdio binary directly against `serde_json`. The MCP spec is well-defined; for a server exposing N tools it's <500 LOC. This is a *very* viable fallback and doesn't compromise the other two wrappers.

**If you eventually add PyO3 bindings (post-v1):**
- Add a `miner-py` crate that depends on `miner-core` + `pyo3` + `maturin`.
- Expose the scan engine API as Python classes; reuse the same finding types via `serde_json` round-trip or `pyo3-arrow` for zero-copy ndarray ↔ NumPy.

**If the bar cache grows huge:**
- Switch from per-file Arrow IPC to a partitioned scheme: `EURUSD/15m/bid/2024.arrow`, `EURUSD/15m/bid/2025.arrow`. Lets you append cheaply and parallelise reads further.
- Add zstd compression to Arrow IPC (`arrow-ipc` supports it) or migrate to Parquet.

---

## Version Compatibility

| Package A | Compatible With | Notes |
|-----------|-----------------|-------|
| `tokio` 1.40+ | `axum` 0.7+, `tower` 0.5+ | Axum 0.7 dropped `axum::body::Body` ergonomics changes vs 0.6 — pin to 0.7+ from day one. |
| `arrow-rs` major versions | `parquet` same major | They release in lockstep; pin both to the same major (e.g. `arrow = "53"`, `parquet = "53"`). |
| `clap` 4 | `clap_complete` 4 | Don't mix `clap` 4 with `clap_complete` 3. |
| `ndarray` 0.16 | `ndarray-stats` 0.6, `ndarray-linalg` 0.16 | Pinned to ndarray's major; ndarray-linalg also pulls a BLAS backend (openblas / netlib / intel-mkl). For portability use `openblas-static`. |
| `serde` 1 | `serde_json` 1 | Effectively universal; just make sure they're both `1.x`. |
| `rmcp` | `tokio` 1.x | **VERIFY** — rmcp's MSRV and tokio requirement need confirming when you add it. |
| Rust edition 2024 | toolchain ≥ 1.85 | Set `edition = "2024"` in every crate's `Cargo.toml`. |
| `criterion` 0.5 | `cargo bench` | Add `[[bench]]` entries with `harness = false`. |

**BLAS backend gotcha:** If you bring in `ndarray-linalg`, choose a backend explicitly via features (`openblas-static`, `openblas-system`, `intel-mkl-static`, `netlib`). `openblas-static` is the most portable; `intel-mkl-static` is fastest on Intel but has licensing constraints. For v1, `openblas-static` keeps the build reproducible across Linux/macOS/Windows.

---

## Sources

> Network access was unavailable for this research pass (WebSearch / WebFetch / Bash
> all sandboxed). Recommendations reflect well-established Rust ecosystem
> conventions current to my training data (early 2026) and the canonical
> "obvious pick" for each layer in the quant / data-engineering Rust space.
>
> **Before committing `Cargo.toml`, the agent (or operator) should:**
> 1. Run `cargo add <crate>` for each dependency and accept the resolved version.
> 2. Visit `crates.io/crates/rmcp` and `github.com/modelcontextprotocol/rust-sdk`
>    to verify the MCP SDK is healthy, supports stdio + streamable-http, and
>    handles streaming tool results.
> 3. Verify `jiff` stability; if any doubt, fall back to `chrono`.
> 4. Verify `arrow-rs` major version pin (current major as of writing was 53;
>    likely 54+ by 2026-05).
> 5. Re-confirm `tokio`, `axum`, `clap`, `serde` majors haven't shifted.
>
> Items flagged **(VERIFY)** inline above are the highest-risk version assumptions.

**Authoritative references to consult:**
- `crates.io` — version + last-publish dates for every crate above
- `docs.rs/<crate>` — live API docs
- `github.com/modelcontextprotocol/rust-sdk` — MCP Rust SDK source & roadmap
- `github.com/rust-ndarray/ndarray` — ndarray + ndarray-stats ecosystem
- `github.com/apache/arrow-rs` — Arrow + Parquet Rust
- `github.com/BurntSushi/jiff` — jiff datetime crate
- `bheisler.github.io/criterion.rs/book/` — criterion user guide
- `tokio.rs` — tokio + axum + tower docs
- `rust-lang.github.io/api-guidelines/` — API conventions for the public library
- `rust-fuzz.github.io/book/` — if fuzzing the CSV/zstd parsers becomes worthwhile

---

*Stack research for: high-performance Rust quantitative scan/mining engine*
*Researched: 2026-05-15*
