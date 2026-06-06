# Technology Stack

**Analysis Date:** 2026-05-25

## Languages

**Primary:**
- Rust 1.85 (edition 2024) - all production code across all workspace crates

**Secondary:**
- None (Python appears only in CI release-workflow inline scripts for version-bump patching)

## Runtime

**Environment:**
- Native binary (no runtime VM); sync computation via `rayon`, async I/O via `tokio` in wrapper stubs only

**Package Manager:**
- Cargo (workspace resolver = "3")
- Lockfile: `Cargo.lock` present and committed (binary crate — correct)

## Workspace Structure

The codebase is a Cargo workspace with seven crates under one version (`1.0.3`). All crates
inherit `version`, `edition`, `rust-version`, and `license` from `[workspace.package]`.

| Crate | Type | Purpose |
|-------|------|---------|
| `miner-core` | `lib` | Core scan engine, types, cache, reader trait — no async, no tokio |
| `miner-reader-dukascopy` | `lib` | Concrete `Reader` impl for Dukascopy zstd-CSV layout |
| `miner-cli` | `bin` (miner) | Thin CLI wrapper: clap, ctrlc, figment config, findings to stdout |
| `miner-mcp` | `bin` (miner-mcp) | MCP server stub — deferred to v2 (PLAT-v2-07) |
| `miner-http` | `bin` (miner-http) | HTTP server stub — deferred to v2 (PLAT-v2-08) |
| `miner-bench` | `bin` (miner-bench, gen-fixtures) | Scan-recipe benchmark runner + fixture generator |
| `xtask` | `bin` (xtask) | Dev-only task runner; `cargo xtask gen-schema` regenerates JSON schemas |

**Key invariant (FOUND-04):** `miner-core` must be async-runtime-free. CI enforces this with
`cargo tree -p miner-core --edges normal,build` grepping for tokio/async-std/smol/async-trait.
Tokio exists in the lockfile only as a transitive dep of test/bench tooling and the wrapper stubs.

## Frameworks

**Core:**
- `rayon` 1.12.0 — CPU-bound parallel scan fanout via `par_iter` over (instrument × timeframe × params) grids
- `arrow` 58.3.0 — Apache Arrow IPC; derived-bar cache format (columnar, mmap-friendly, language-portable)
- `figment` 0.10.19 — Layered config: TOML → `MINER_*` env vars → CLI overrides (CLI wins)

**CLI:**
- `clap` 4.6.1 (derive + env features) — subcommands: `scan`, `scans`, `sweep`, `emit-fixture`

**Async (wrappers only):**
- `tokio` 1.52.3 — present in lockfile as transitive dep; not used by `miner-core` or `miner-cli`
- `tower` / `tower-http` — in lockfile but not wired into any shipping crate yet

**Testing:**
- `criterion` 0.7.0 (html_reports) — microbench harness; six named benches in `miner-core/benches/`
- `proptest` 1.11.0 — property tests for aggregation invariants and path-layout round-trips
- `insta` 1.47.2 (json feature) — snapshot tests for finding output schema stability
- `assert_cmd` 2 + `predicates` 3 — CLI integration tests (spawn real `miner` binary)
- `jsonschema` 0.46.5 — validates stdout JSONL lines against committed schema artifacts in tests
- `serial_test` 3 (file_locks feature on miner-cli) — serialises env-var-mutating tests

**Build/Dev:**
- `xtask` pattern (`cargo xtask gen-schema`) — regenerates `schemas/findings-v1.schema.json`
- `git-cliff` (via `orhun/git-cliff-action@v4` in CI) — conventional commits → release notes

## Key Dependencies

**Critical production:**
- `serde` 1.0.228 (derive) — universal ser/de framework; all OHLCV rows, findings, configs
- `serde_json` 1 — JSONL findings output via `to_writer` + newline; NO `preserve_order` feature (byte-stability requirement documented in root `Cargo.toml`)
- `schemars` 1.2.1 (chrono04) — derive `JsonSchema` on `Finding` types; xtask regenerates `schemas/findings-v1.schema.json`
- `chrono` 0.4 (clock, serde; no default features) — UTC timestamps on all bar data and findings
- `csv` 1.4.0 — CSV parsing of Dukascopy decompressed streams; serde derive integration
- `zstd` 0.13.3 — streaming zstd decompressor; `Decoder<BufReader<File>>` pipeline in reader
- `arrow` + `arrow-ipc` 58.3.0 — Arrow IPC write/read for derived-bar cache; `parquet` 58 also in workspace deps for future use
- `walkdir` 2.5.0 — recursive Dukascopy cache discovery (must chain `.sort_by_file_name()` for determinism)
- `blake3` 1.8.5 — per-day file fingerprints for cache invalidation; param hashes for finding envelopes
- `ulid` 1.2.1 (serde) — `RunId` unique run identifiers on every findings stream

**Statistical computation:**
- `ndarray` 0.16.1 — n-dim numerical arrays for rolling-window kernels over OHLCV columns
- `ndarray-stats` 0.6.0 — mean/var/quantile/correlation primitives over ndarray
- `nalgebra` 0.32.6 (std only, no rand/serde defaults) — small fixed-size linear algebra for rolling OLS
- `statrs` 0.17.1 — `ChiSquared` distribution for Ljung-Box p-values
- `realfft` 3.5.0 — real-input FFT for IAAFT phase-scramble null kernel (Plan 07-05)
- `rand` 0.8.6 — `Rng` + `SeedableRng` trait surface (no thread-local RNGs)
- `rand_xoshiro` 0.6.0 — `Xoshiro256PlusPlus` portable PRNG for bootstrap/null resampling

**Infrastructure:**
- `thiserror` 1 — typed error enums in library crates (`miner-core`, `miner-reader-dukascopy`)
- `anyhow` 1 — error glue in binary crates (`miner-cli`, `miner-bench`, `xtask`)
- `tracing` 0.1.44 + `tracing-subscriber` 0.3 (env-filter, fmt) — structured logging to stderr; `println!`/`eprintln!` banned workspace-wide by `clippy.toml`
- `figment` 0.10.19 (toml, env) — layered config; Jail feature enabled for test isolation
- `ctrlc` 3.5 — SIGINT handler in `miner-cli`; sets `Arc<AtomicBool>` cancellation flag
- `directories` 5.0.1 — XDG/platform config path resolution in CLI
- `base64` 0.22 — `Base64Bytes` serialisation for raw series data in findings
- `toml` 0.8 — TOML deserialiser for sweep manifests (single config source, no layering)
- `tempfile` 3 — atomic `NamedTempFile::persist` for crash-safe Arrow IPC cache writes
- `sha2` 0.10.9 — fixture checksums in `miner-bench/gen-fixtures` binary

**Optional / feature-gated:**
- `dhat` 0.3 — heap profiler; opt-in via `--features dhat` on `miner-bench` only; never leaks to `miner-core`

## Configuration

**Config sources (precedence: CLI > env > TOML > error):**
- TOML file: XDG config dir (`~/.config/miner/config.toml`) or CWD `miner.toml`; path overridable with `--config`
- Environment variables: `MINER_CACHE_ROOT`, `MINER_BAR_CACHE_ROOT`, `MINER_OUTPUT`
- Nested env vars use double-underscore separator (`MINER_TELEMETRY__ENABLED`)
- CLI flags: `--cache-root`, `--bar-cache-root`, `--output`

**Required config fields (all must be present; no defaults in library):**
- `cache_root` — path to read-only `tradedesk-dukascopy` CSV cache
- `bar_cache_root` — path to miner's writable derived-bar Arrow IPC cache
- `output` — `"stdout"` or a file path

**Config types live in `miner-core`; path resolution lives in `miner-cli`.** The library
contains zero hardcoded path literals (FOUND-05 invariant).

## Build Configuration

**Toolchain pin:** `rust-toolchain.toml` pins channel `1.85` (minimal profile) with `clippy` and `rustfmt` components.

**Release profile** (`Cargo.toml [profile.release]`):
- `lto = "thin"` — cross-crate inlining
- `codegen-units = 1` — full optimisation
- `strip = "symbols"` — removes `.symtab`/`.strtab`, preserves `.debug_*` for dhat frame attribution
- `debug = 1` — line tables only; required for `dhat-rs` symbol attribution

**Workspace lint gates:**
- `unsafe_code = "forbid"` — enforced workspace-wide; no `unsafe` anywhere
- `clippy::pedantic = warn` — workspace-wide
- `disallowed-macros` (`clippy.toml`): `println!`, `print!`, `eprintln!`, `eprint!`, `dbg!` all banned; stdout reserved for findings stream (threat T-01-03)

**Schema artifacts:** Three committed JSON Schema files in `schemas/`:
- `schemas/findings-v1.schema.json` — finding envelope contract; regenerated by `cargo xtask gen-schema`; CI diffs it (Gate 4)
- `schemas/scans-catalogue-v1.schema.json` — scan registry schema
- `schemas/sweep-manifest-v1.schema.json` — sweep TOML manifest schema

## Platform Requirements

**Development:**
- Rust 1.85 stable + clippy + rustfmt (pinned via `rust-toolchain.toml`)
- No nightly features used
- `cargo-audit`, `cargo-deny` needed for local security gates

**Production / Distribution targets (v1.0 via GitHub Releases):**
- `x86_64-unknown-linux-gnu` (primary; Quant agent host class)
- `aarch64-unknown-linux-gnu` (Graviton/Ampere agents)
- `aarch64-apple-darwin` (macOS Apple Silicon dev workstations)
- Intel macOS (`x86_64-apple-darwin`) intentionally omitted from publish matrix

---

*Stack analysis: 2026-05-25*
