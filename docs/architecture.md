# Architecture

Overview
- tradedesk-miner is a high-performance, agent-operable data-mining engine for historical financial OHLCV data.
- It scans cached Dukascopy bid/ask CSVs and surfaces statistical candidates (anomalies, cross-instrument relationships, seasonality effects) for downstream consumers — primarily the RadiusRed Quant agent.
- The codebase is organised into seven Cargo crates with a strict one-way dependency direction (six runtime crates plus a dev-only `xtask` workspace member):
  - `miner-core` — the sync + rayon library.
    - Owns the locked `Finding` envelope, the scan registry, the engine facade (`engine::run_one`), the sweep runner (`sweep::run_sweep`), the derived-bar cache, and the 23 v1 scans (ANOM / CROSS / SEAS).
    - Pure sync; no `tokio`, no `async fn`.
  - `miner-reader-dukascopy` — the reference implementation of the `Reader` trait against the existing tradedesk-dukascopy zstd-CSV cache layout.
  - `miner-cli` — the thin clap-derive wrapper exposing `miner scan` / `miner sweep` / `miner scans` / `miner emit-fixture`.
  - `miner-mcp` and `miner-http` — placeholder binaries.
    - MCP and HTTP server implementations are deferred to v2 — see `future_mcp_http.md`.
    - The placeholder shells exist so the workspace graph (FOUND-01) is stable and v2 has anchor points.
  - `miner-bench` — the bench harness.
  - `xtask` — dev-only workspace member hosting `cargo run -p xtask -- gen-schema` and similar developer tooling; not part of the runtime artefact set.
- Dependency direction: `miner-cli | miner-mcp | miner-http -> miner-reader-dukascopy -> miner-core`.
- CI gate 3 (`cargo tree -p miner-core --edges normal,build`) enforces this — `miner-core` must show zero `tokio` / `async` transitive dependencies.

Data Flow (high level)
- `Reader::read_day` ingests zstd-CSVs from the tradedesk-dukascopy cache.
  - Path layout: `<root>/<SYMBOL>/<YYYY>/<MM 00-indexed>/<DD>_<bid|ask>.csv.zst`.
  - The 00-indexed month quirk is encapsulated inside the Dukascopy reader and boundary-tested.
- The aggregator deterministically materialises higher-timeframe UTC-aligned bars (15m / 1h / 1d).
  - Bars are served from the on-disk derived-bar cache as Arrow IPC files, keyed by `(source_id, symbol, side, timeframe)`.
  - Two-axis invalidation: `aggregator_version` / `arrow_schema_version` mismatch triggers a full rebuild; a per-day blake3 fingerprint mismatch triggers a day-splice.
- `GapDetector` produces a structured `GapManifest` of `(start, end, reason)` tuples before any scan runs.
- `engine::run_one` is the single facade entry.
  - It runs preflight -> framing -> dry-run -> gap-policy dispatch -> `Scan::run` -> `RunSummary`.
  - Findings are emitted through the `FindingSink` trait.
- Scans are sync + rayon-parallel; multi-job sweeps fan out via `sweep::run_sweep`.
  - `rayon::par_iter` over `ResolvedJob`s with a deterministic-order buffered drain.
  - End-of-sweep BH-FDR aggregation emits a `SweepSummary` finding closing the run.
- `FindingSink` is the single sanctioned writer.
  - Production impls: `StdoutSink` (CLI) and `FileSink` (tests).
  - Stdout = findings JSONL; stderr = `tracing` structured logs.

Sync core + async edges
- `miner-core` is pure sync + rayon. No `tokio`, no `async fn`, no `.await`. This is FOUND-04 and is CI-enforced.
- Async lives only at the wrapper edges.
  - The MCP and HTTP wrappers (designed but not implemented in v1 — see `future_mcp_http.md`) will bridge to `miner-core` via `tokio::task::spawn_blocking`.
  - The scan engine never blocks an async runtime worker.
- Stdout is reserved for findings JSONL. Stderr is reserved for structured logs.
- `clippy::disallowed_macros` rejects `println!` / `eprintln!` outside the single findings sink and the logging adapter (CI gate 2).

Key design decisions
- Locked `Finding` envelope.
  - Seven variants: `RunStart` / `Result` / `ScanError` / `GapAborted` / `RunEnd` / `DryRun` / `SweepSummary`.
  - Every variant carries `schema_version`, `scan@version`, `param_hash`, `code_revision`, `data_slice`, and reserved DSR + FDR-q slots.
  - Schema-additive discipline only; ground truth is `schemas/findings-v1.schema.json` regenerated from the schemars derive on the Rust types.
- Gap-policy enforcement. No silent scans over gapped data.
  - Callers pick `strict` (abort + emit the gap manifest as a structured error, zero findings) or `continuous_only` (partition into maximal gap-free sub-ranges, one finding per sub-range, gap manifest attached).
- Reproducibility envelope.
  - `master_seed` propagates through every `ResolvedJob`.
  - `derive_job_seed` (blake3) makes each scan's RNG state deterministic and byte-identical across re-runs.
- Streaming and stateless.
  - miner emits findings as NDJSON and exits. There is no persistent results store; callers own persistence.
  - The only writable state miner owns is its derived-bar cache.
- Agent-operability.
  - Every miner capability is reachable from the CLI today and from MCP + HTTP in v2 (see `future_mcp_http.md`).
  - The CLI is the load-bearing v1 surface; MCP + HTTP wrappers add transport ergonomics without changing the engine.
- Open-source posture.
  - No hardcoded paths. Cache root + derived-bar-cache root + output destination all configurable via CLI flag > env var > config file precedence.
  - Apache-2.0 licensed.

See also: `../README.md`, `findings_envelope.md`, `scan_catalogue.md`, `sweep_manifest.md`, `agent_integration.md`, and `future_mcp_http.md`.

---

## License

Licensed under the Apache License, Version 2.0.
See: https://www.apache.org/licenses/LICENSE-2.0

Copyright 2026 [Radius Red Ltd.](https://github.com/radiusred) | [Contact](mailto:opensource@radiusred.uk)
