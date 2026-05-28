# Future MCP + HTTP wrappers (deferred to v2)

miner's v1 transport surface is the CLI. The MCP and HTTP server wrappers — originally scoped for Phase 6 — have been deferred to v2.

Rationale: miner's primary consumer (the RadiusRed Quant agent) drives miner via `subprocess.Popen` + JSONL on stdout without any shape gap. Running a 24x7 server for a single-operator tool adds operational surface (deploy, monitor, restart, secret rotation, body-size caps, rate limits, mTLS) that is not justified for v1. The placeholder crates `crates/miner-mcp/` and `crates/miner-http/` stay in the workspace so a v2 contributor knows exactly where the implementations land; each is currently twelve lines emitting one `tracing::info!` message pointing at this doc.

This doc captures the planned design at the architectural-sketch level only — what each wrapper would expose, why the shapes were chosen, planned crate dependencies with risk notes, and the pointers a v2 contributor needs to pick up the work. The implementation details (route table with HTTP status codes, request / response JSON examples, MCP tool-schema fragments, cancellation propagation diagrams, content-negotiation rules, rmcp-vs-fallback decision tree) belong in the v2 plan-phase's RESEARCH.md and CONTEXT.md, not here.

The two reclassified requirements `PLAT-v2-07` (MCP) and `PLAT-v2-08` (HTTP) — see `.planning/REQUIREMENTS.md` and `.planning/STATE.md` Deferred Items — are the tracking handles for the v2 implementation work. Both inherit the contracts locked by v1: byte-identical-JSONL parity against `miner scan` / `miner sweep`, sync-core + async-edges discipline (FOUND-04), and the locked `Finding` envelope vocabulary documented in [findings_envelope.md](findings_envelope.md).

## What MCP would expose

The MCP wrapper would mirror miner's CLI surface as typed MCP tools:

- **One MCP tool per registered scan** — `stats.autocorr.ljung_box@1`, `stats.stationarity.adf@1`, `cross.cointegration.engle_granger@1`, ... Each tool's parameter schema derives directly from the scan's `Scan::params_schema()` JSON Schema — no duplicated source of truth. The tool result is the streamed JSONL of `Finding` envelopes the underlying `engine::run_one` call produces, framed as MCP streaming tool-result chunks.
- **Meta-tools for discovery and preflight:**
  - `list_scans` — mirrors `miner scans`; emits the catalogue as a single MCP tool result (one entry per registered scan with `scan_id`, `arity`, `param_schema`, `finding_fields`).
  - `list_symbols` — enumerates the `(symbol, side)` pairs available in the configured `MINER_CACHE_ROOT` by walking the reader directory layout.
  - `probe` — preflight a parameter set without executing; equivalent to `miner scan <id> --dry-run` but reachable from MCP without invoking the streaming engine.
- **Transports:** stdio (for local-agent attachment in the LSP-style spawning model) and streamable-HTTP / SSE (for remote agents).
- **Open design question for the v2 plan-phase:** one-tool-per-scan vs a single generic `scan` tool that takes `scan_id@version` as a parameter. The former gives strong typing per tool and lets MCP clients render scan-specific parameter UIs; the latter shrinks the tool count from ~23 to ~5 (one per CLI subcommand) and avoids registry / MCP tool-list synchronisation. v1 has no opinion; the v2 plan-phase decides.

Deep design: `.planning/research/ARCHITECTURE.md` §8 (Wrapper Crates).

## What HTTP would expose

The HTTP wrapper would expose miner's CLI shape over REST + streaming:

- **`GET /v1/scans`** — catalogue introspection; returns the same JSONL stream as `miner scans` but as a JSON array body (or NDJSON when the client signals streaming via `Accept: application/x-ndjson`).
- **`GET /v1/symbols`** — instrument-side inventory; same shape as the MCP `list_symbols` tool.
- **`POST /v1/scan`** — single scan; request body is the resolved `ScanRequest` JSON; response body streams `Finding` envelopes as NDJSON or SSE per content negotiation.
- **`POST /v1/sweep`** — sweep manifest; request body is either the TOML manifest (preferred; `Content-Type: application/toml`) or the parsed `SweepManifest` JSON; response streams findings and the closing `SweepSummary` envelope.
- **Content negotiation:** NDJSON by default; SSE (`text/event-stream`) when the client prefers a framed event stream. Open design question for v2 — both are reasonable; NDJSON matches the CLI byte-for-byte and is preferred for v2.
- **Out of v1 sketch scope:** authentication strategy (bearer / mTLS / OAuth), body-size cap, rate limits, bind address, TLS termination. The v2 plan-phase owns these — they are deployment-posture decisions, not engine-shape decisions.
- **Response framing:** every streaming response endpoint emits one Finding envelope per NDJSON line or one Finding envelope per SSE `data:` field. The byte payload is byte-identical to the corresponding `miner scan` stdout for the same inputs — the HTTP wrapper is a framing, not a transformation.
- **Cancellation:** HTTP client disconnect translates to engine cancellation via the same `Cancelled` flag the CLI's `ctrlc` handler sets. The v2 plan-phase decides the exact propagation path (the `axum` connection-tracker mechanism or `tower::timeout::Timeout` middleware are both plausible carriers).

Deep design: same source.

## Crate choices

Sourced from `.planning/research/STACK.md`.

- **MCP — `rmcp`.** The Rust SDK published by `modelcontextprotocol/rust-sdk` is the canonical pick. STACK.md confidence is MEDIUM and the row is explicitly marked **VERIFY**: the SDK version, transport support (stdio + streamable-HTTP), streaming tool-result chunks, and tokio-version compatibility all need re-confirming when the v2 plan-phase begins. The risk is that the SDK is the youngest dependency in miner's stack and may have shifted shape between v1 docs-freeze and v2 implementation kickoff.
  - **Fallback if `rmcp` does not fit:** a hand-rolled JSON-RPC-over-stdio binary built directly against `serde_json`. STACK.md estimates ~500 LOC. The MCP wire protocol is well-defined and stable; hand-rolling it loses streaming-tool-result ergonomics but otherwise produces a working MCP server. This fallback does NOT affect the HTTP wrapper — the two wrappers are independent crates.
- **HTTP — `axum` over `tokio` + `tower`.** Confidence HIGH in STACK.md. `axum` 0.7+ on `tokio` 1.40+ with `tower-http` middleware (`Trace` for request logging, `Compression` for gzip / br, `Timeout` for upstream cancellation propagation). Streaming response bodies via `axum::body::Body::from_stream` over a `tokio_stream::Stream` of `Result<Bytes, Error>` chunks bridges cleanly into NDJSON or SSE framing.
- **Async bridge — `tokio::task::spawn_blocking`.** This is the canonical pattern and a non-negotiable invariant: `miner-core` MUST stay sync + rayon. The wrappers offload every `engine::run_one` / `sweep::run_sweep` invocation into the blocking pool via `tokio::task::spawn_blocking`, channel findings out as the engine emits them, and feed the channel into the response stream. FOUND-04 (`cargo tree -p miner-core --edges normal,build` shows zero async deps) is the CI gate that pins this — it must remain green after v2 lands.
- **Logging — `tracing` + `tracing-subscriber`.** Already in the workspace; the wrappers inherit miner's structured-log discipline (stderr-only, structured spans for request / scan / job). No new logging-stack choice to make.
- **Serialisation — `serde` + `serde_json`.** Reuse miner-core's `Finding` types verbatim; the wrappers re-serialise via `serde_json::to_writer` to the response body. No JSON-encoder swap (e.g. `simd-json`) until profiling shows encoding is hot.

## Why deferred

Three short reasons:

- **The CLI is enough for v1.** miner's primary consumer (the Quant agent) drives miner via `subprocess.Popen` + line-by-line JSONL parsing. There is no shape gap between what the CLI emits and what the MCP / HTTP wrappers would emit — the wrappers would only re-frame the same byte stream.
- **Servers add operational surface a single-operator tool doesn't need.** Deployment, monitoring, restart semantics, secret rotation, TLS, body-size caps, rate limits, abuse posture — none of these problems exist for a CLI that runs on the same host as its caller. Postponing them until there is a genuine remote-consumer requirement is the cheaper position.
- **`rmcp` is the highest-risk single dependency in the stack.** Better to let the v2 plan-phase re-research the SDK against its then-current release than lock the choice now and discover a surprise (transport shape change, streaming-result API redesign, MSRV bump) on v2 day one.
- **No transport drift in v1.** Because the wrappers never shipped in v1, no consumer has been written against a wrapper API that might shift in v2. The locked v1 surface is the CLI alone; the wrappers are a clean v2 greenfield with no migration debt to repay.

## How to pick this up (v2 contributor)

The v2 plan-phase should read these in order before reopening the wrapper implementations:

1. `.planning/phases/06-mcp-http-wrappers/06-CONTEXT.md` — the discuss-phase output that captured the deferral rationale, the original wrapper-scope notes, and the locked surface decisions (D6-01 through D6-09). Start here.
2. `.planning/research/ARCHITECTURE.md` §8 (Wrapper Crates) — the layered design for the wrappers, including the `tokio::task::spawn_blocking` bridge sketch and the per-wrapper crate boundaries.
3. `.planning/research/STACK.md` — the `rmcp` row (with the VERIFY risk note), the `axum` row, and the `tower` / `tower-http` rows. Re-validate the version pins against the then-current releases.
4. `crates/miner-mcp/src/main.rs` and `crates/miner-http/src/main.rs` — the placeholder anchor points. Each is twelve lines today emitting one `tracing::info!` line referencing this doc; v2 replaces the body and adds the wrapper-crate dependencies to the per-crate `Cargo.toml`.
5. `./architecture.md` (the public-audience system map) — re-read the "Sync core + async edges" section. The async-edges discipline is the contract the v2 implementation must preserve.
6. The Phase 6 sign-off SUMMARY at `.planning/phases/06-mcp-http-wrappers/06-03-SUMMARY.md` — captures the v1 docs-only invariants the v2 implementation must NOT break (zero new dependencies in `miner-core` / `miner-cli`; the `cargo tree -p miner-core --edges normal,build` gate stays green; the locked `Finding` envelope stays additive-only).

CI invariants for the v2 wrapper implementation:

- `cargo tree -p miner-core --edges normal,build` MUST continue to return empty for `tokio` / `axum` / `hyper` / `rmcp` / `tower`. New deps land in `miner-mcp` and `miner-http`, NOT in `miner-core`.
- `cargo clippy --workspace --all-targets -- -D warnings` (CI gate 2) MUST stay green with the new wrapper code.
- Wrapper integration tests must include a byte-identical-JSONL parity check against `miner scan` stdout for at least one representative scan per family.

The byte-identical-JSONL parity expectation: every Finding emitted by an MCP `tool/call` response or an HTTP `POST /v1/scan` response body MUST be byte-identical to the corresponding `miner scan` stdout for the same inputs. The CLI is the wire-format anchor; the wrappers are framings, not transformations. The v2 plan-phase should ship parity tests (one per wrapper) that diff wrapper-output JSONL against `miner scan` stdout for a representative scan from each family (ANOM / CROSS / SEAS).

## Out of scope for this sketch

Explicitly NOT part of this design sketch (per D6-03; tracked for the v2 plan-phase). The bullets below are pointers, not contracts — the v2 RESEARCH.md and CONTEXT.md will turn each into a concrete decision:

- Route table with HTTP status codes (which paths return `404` vs `400` vs `422`, error-body shape, retry-after semantics).
- Request / response JSON examples (the v1 docs/findings_envelope.md already covers the response shape; only the wrapper-specific request shape is missing).
- MCP tool-schema fragments (the typed `inputSchema` / `outputSchema` per tool; mechanically generated from the registry but worth a v2 sample).
- Cancellation propagation diagrams (how MCP `notifications/cancelled` and HTTP client-disconnect translate into engine `Cancelled` flag setting).
- Content-negotiation decision tree (NDJSON vs SSE vs JSON-array; default-vs-honour-Accept policy).
- `rmcp` vs hand-rolled JSON-RPC decision tree (the v2 plan-phase re-runs `gsd-research` on `rmcp` against the then-current release and decides).
- Deployment posture artefacts (Dockerfile, helm chart, systemd unit). Out of v2 sketch scope; a follow-up operational-readiness phase owns these.

Tracked for v2 milestone planning. The v1 docs/ folder is the binding context for the v2 wrapper implementation; nothing in this sketch is normative beyond the contracts already locked in v1 (envelope shape, byte-identical-JSONL parity, FOUND-04 async-edges discipline).

---

## License

Licensed under the Apache License, Version 2.0.
See: https://www.apache.org/licenses/LICENSE-2.0

Copyright 2026 [Radius Red Ltd.](https://github.com/radiusred) | [Contact](mailto:opensource@radiusred.uk)
