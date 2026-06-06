# External Integrations

**Analysis Date:** 2026-05-25

## External Data Sources

**Dukascopy Bid/Ask CSV Cache:**
- Role: Primary read-only input data source (no network calls at runtime)
- Provider: `tradedesk-dukascopy` (sibling repo; pre-built cache consumed by this project)
- Format: `<cache_root>/<SYMBOL>/<YYYY>/<MM>/<DD>_<bid|ask>.csv.zst` — zstd-compressed CSV, 1-minute OHLCV bars
- Client: `miner-reader-dukascopy` crate (`crates/miner-reader-dukascopy/src/reader.rs`)
- Auth: None — local filesystem read; `cache_root` path supplied via config
- Access pattern: File-at-a-time; `walkdir::WalkDir::new(root).sort_by_file_name()` for deterministic ordering
- No modification of this cache is permitted; the miner is a read-only consumer

## APIs & External Services

**None at runtime.** The miner is a fully offline, local-filesystem tool. It does not make HTTP
calls, connect to databases, or authenticate against any remote service during normal operation.

The three server wrapper crates (`miner-http`, `miner-mcp`) are v1 stubs with no real
implementation — HTTP framework (`axum`, `tower`) and MCP SDK (`rmcp`) are not yet wired.

## Data Storage

**Read-only input:**
- Dukascopy CSV cache — see External Data Sources above
- Connection: `MINER_CACHE_ROOT` env var or `cache_root` TOML field or `--cache-root` CLI flag

**Writable derived-bar cache (the only state miner owns):**
- Format: Apache Arrow IPC files at `<bar_cache_root>/<source_id>/<symbol>/<timeframe>_<side>.arrow`
- Sidecar: `<...>.fingerprints.json` per Arrow file (blake3 per-day fingerprints + schema versions)
- Connection: `MINER_BAR_CACHE_ROOT` env var or `bar_cache_root` TOML field or `--bar-cache-root` CLI flag
- Client: `miner-core::cache` module (`crates/miner-core/src/cache.rs`)
- Crash safety: Two-step atomic write — `write_arrow_to_tempfile` + `persist_arrow_tempfile` via `tempfile::NamedTempFile::persist` (atomic rename)
- Invalidation: Full rebuild on aggregator or schema version mismatch; day-splice on blake3 fingerprint mismatch

**File Storage:**
- Local filesystem only; no object storage

**Caching:**
- No external cache (Redis, Memcached, etc.); the Arrow IPC bar cache is the only derived state

## Authentication & Identity

**None.** No auth provider, no user management, no tokens. The tool is invoked by agents or users
with direct filesystem access to the cache directories.

## Output / Findings Stream

**JSONL stdout stream:**
- All findings emitted as newline-delimited JSON to stdout via `miner_core::findings::sink::StdoutSink`
- Alternatively redirected to a file via `output = { file = "path" }` config or `--output` flag
- Schema: `schemas/findings-v1.schema.json` (committed, CI-validated)
- Consumer: RadiusRed Quant agent reads findings stream and converts to strategy hypotheses
- Stream framing: `RunStart` record → per-finding envelopes → `RunEnd` record

**Stderr diagnostic stream:**
- All structured logging via `tracing` → `tracing_subscriber::fmt().with_writer(stderr)`
- Pre-flight errors emitted via `miner_core::error::stderr_emit` (typed `WireError` structs with `PreflightCode`)
- No `println!`/`eprintln!` anywhere in the codebase (banned by `clippy.toml`)

## Monitoring & Observability

**Error Tracking:** None (no Sentry, Datadog, etc.)

**Logs:**
- `tracing` + `tracing-subscriber` with `env-filter`
- Log level controlled by `RUST_LOG` environment variable
- All log output goes to stderr; stdout is reserved exclusively for findings JSONL

**Metrics:** None at v1

**Profiling (dev-time only):**
- `dhat` heap profiler available behind `--features dhat` on `miner-bench`; writes `dhat-heap.json` to CWD on process exit
- `criterion` microbench HTML reports in `target/criterion/` (local runs only; CI does not run benches)

## CI/CD & Deployment

**Source Control:** GitHub (`radiusred/tradedesk-miner`)

**CI Pipeline:** GitHub Actions (`.github/workflows/ci.yml`)
- Runs on every push and PR (skips `release/*` branch PRs)
- Gate 1: `cargo build --workspace --all-targets`
- Gate 2: `cargo clippy --workspace --all-targets -- -D warnings` (stdout discipline + pedantic lints)
- Gate 3: `cargo fmt --all -- --check`
- Gate 4: `cargo test --workspace --no-fail-fast`
- Gate 5 (tokio-free): `cargo tree -p miner-core --edges normal,build` grepped for prohibited async crates
- Gate 6 (schema-sync): `cargo xtask gen-schema` + `git diff --exit-code schemas/`
- Gate 7: `cargo audit` (RustSec advisory database; `--locked` to avoid MSRV breakage)
- Gate 8: `EmbarkStudios/cargo-deny-action@v2` (license + bans + advisories + sources)

**Release Flow:** Two-workflow chain
1. `prepare-release.yml` (`.github/workflows/prepare-release.yml`) — `workflow_dispatch` only; requires `release-approval` environment reviewer; opens `release/vX.Y.Z` branch + PR; waits for merge; creates draft GitHub Release using conventional commits → semver bump + `git-cliff` release notes
2. `publish.yml` (`.github/workflows/publish.yml`) — fires on `workflow_run` completion of prepare-release (or manual `workflow_dispatch`); cross-compiles `miner` binary for each matrix target; uploads tarballs + SHA256 checksums to draft release; flips draft to published

**Hosting:** GitHub Releases (binary tarballs; not published to crates.io — all crates have `publish = false`)

**Secrets:**
- `RELEASE_APP_PRIVATE_KEY` — GitHub App private key for release automation (repo secret)
- `RELEASE_APP_CLIENT_ID` — GitHub App client ID (repo variable)
- `GITHUB_TOKEN` — standard Actions token for release asset upload

## Webhooks & Callbacks

**Incoming:** None

**Outgoing:** None (offline tool)

## Future Integrations (Stubbed, Not Yet Implemented)

**MCP server (`miner-mcp`):**
- Stub binary exists at `crates/miner-mcp/src/main.rs`; no MCP SDK wired yet
- Planned: `rmcp` crate (official Rust MCP SDK from `modelcontextprotocol/rust-sdk`)
- Transport: stdio (local agent) + streamable-HTTP/SSE (remote Paperclip agent)
- Deferred to v2 (PLAT-v2-07)

**HTTP server (`miner-http`):**
- Stub binary exists at `crates/miner-http/src/main.rs`; no HTTP framework wired yet
- Planned: `axum` 0.7+ over `tokio`, `tower`/`tower-http` middleware, SSE for JSONL streaming
- Deferred to v2 (PLAT-v2-08)

---

*Integration audit: 2026-05-25*
