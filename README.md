# tradedesk-miner

High-performance, agent-operable data-mining engine for historical financial OHLCV
data. Scans cached candle data (typically Dukascopy bid/ask CSVs prepared by
`tradedesk-dukascopy`) and surfaces statistical anomalies, cross-instrument
relationships, and seasonality effects as raw candidate findings. The primary
consumer is a Quant agent, which turns those findings into testable
trading-strategy hypotheses.

```
tradedesk-dukascopy (cache)  →  tradedesk-miner (raw findings)
                             →  Quant agent (hypotheses)
                             →  tradedesk based app (strategies, backtests, live)
```

## Status

Phase 1 (Foundations & Contracts) complete. Phase 2 (reader / aggregator / cache) in
progress. The Phase 1 contract surface — locked `Finding` JSON Schema, single sink
writer, config precedence, CI gates — is FROZEN; later phases add scans and
wrappers on top without re-litigating the envelope.

## License

Apache-2.0 — see [`LICENSE`](LICENSE) and [`NOTICE`](NOTICE).

## Quickstart

1. **Prerequisites.** Rust 1.85+ stable (`rustup default 1.85`) and git.

2. **Clone and build.**

   ```sh
   git clone https://github.com/radiusred/tradedesk-miner
   cd tradedesk-miner
   cargo build --workspace
   ```

3. **Emit a fixture (the Phase 1 smoke test).**

   ```sh
   MINER_CACHE_ROOT=/tmp/cache \
   MINER_BAR_CACHE_ROOT=/tmp/bar \
   MINER_OUTPUT=stdout \
   cargo run -p miner-cli -- emit-fixture
   ```

   Expected output on stdout: exactly 2 NDJSON lines — one `kind: run_start`, one
   `kind: run_end` — sharing the same `run_id`. Tracing log lines land on stderr.

4. **Inspect the locked envelope schema.**

   ```sh
   jq '.title, (.["$defs"] | keys?)' schemas/findings-v1.schema.json
   ```

5. **Run the test suite and lints.**

   ```sh
   cargo test --workspace
   cargo clippy --workspace --all-targets -- -D warnings
   ```

## What Phase 1 Delivers

- **Six-crate Cargo workspace** — `miner-core` library plus `miner-cli`,
  `miner-mcp`, `miner-http` wrapper binaries, the `miner-reader-dukascopy`
  reader shell, and the `miner-bench` benchmark binary. A seventh `xtask`
  crate hosts dev-only commands like `cargo run -p xtask -- gen-schema`.
- **Locked `Finding` envelope** — five-variant tagged enum (`run_start`,
  `result`, `scan_error`, `gap_aborted`, `run_end`) with frozen common
  fields (`schema_version`, `scan_id@version`, `param_hash`, `code_revision`,
  `data_slice`, reserved-but-null `dsr` and `fdr_q`). The committed
  `schemas/findings-v1.schema.json` is regenerable via
  `cargo run -p xtask -- gen-schema`.
- **Stdout = findings, stderr = logs** — `StdoutSink` is the only writer to
  `io::stdout()` in the workspace; `tracing-subscriber` routes structured
  logs to stderr. CI-enforced via `clippy::disallowed_macros` (the
  workspace `clippy.toml` bans `println!` / `eprintln!` / `dbg!` outside the
  sanctioned writers).
- **Tokio-free `miner-core`** — the scan engine is sync + `rayon` only;
  `tokio` enters only via `spawn_blocking` inside the MCP and HTTP
  wrappers. CI-enforced by a `cargo tree -p miner-core` grep gate.
- **Config precedence** — CLI flag > env var > TOML file > error, no
  hardcoded paths in the library. The CLI is responsible for XDG/CWD
  config-path resolution; the library only knows the figment merge order.
- **Envelope-byte determinism** — `cargo run -p miner-cli -- emit-fixture`
  produces byte-identical output across runs once the four known-volatile
  fields (`run_id`, `started_at_utc`, `ended_at_utc`, `wall_clock_ms`) are
  masked. Regression-tested by `cli_streams::emit_fixture_byte_identical_when_volatile_fields_masked`.

## Architecture

See `.planning/research/ARCHITECTURE.md` for the layered design and
`.planning/phases/01-foundations-contracts/01-RESEARCH.md` for the Phase 1
research artefacts.

## Roadmap

See `.planning/ROADMAP.md`. Plan-by-plan summaries live under
`.planning/phases/<phase>/<phase>-<plan>-SUMMARY.md`.

