![banner](https://i.ibb.co/Kc5C88gp/tradedesk-banner.webp)

# tradedesk-miner

![CI Build](https://github.com/radiusred/tradedesk-miner/actions/workflows/ci.yml/badge.svg)

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

The v1 scan engine ships 23 scans across three families (single-instrument
anomaly, two-instrument cross, seasonality) under a locked `Finding` JSON
envelope, with a parallel sweep runner that applies bootstrap CIs, null
distributions, and Benjamini-Hochberg FDR. MCP and HTTP wrappers are
documented and deferred — see [docs/future_mcp_http.md](docs/future_mcp_http.md).

## Quickstart

Prerequisites: Rust 1.85+ stable (`rustup default 1.85`) and git.

```sh
git clone https://github.com/radiusred/tradedesk-miner
cd tradedesk-miner
./scripts/install-git-hooks.sh   # one-time: wires the cargo fmt + clippy pre-commit gate
cargo build --workspace
cargo test --workspace
```

`install-git-hooks.sh` points `core.hooksPath` at the tracked `.githooks/`
directory so the local pre-commit hook mirrors the CI fmt + clippy gates.
Overrides: `MINER_AUTOFIX=1` lets the hook re-stage fmt fixes and continue;
`MINER_SKIP_CLIPPY=1` skips clippy for fast WIP commits (CI still enforces
the gate); `git commit --no-verify` bypasses everything.

## Example

Run a scan over a populated cache and stream NDJSON `Finding` envelopes to
stdout:

```sh
MINER_CACHE_ROOT=./tests/fixtures/cache \
MINER_BAR_CACHE_ROOT=/tmp/bar \
MINER_OUTPUT=stdout \
cargo run -p miner-cli -- scan seas.bucket.hour_of_day@1 \
    --instrument EURUSD:bid --timeframe 15m \
    --window 2024-01-01:2024-01-31
```

If you cloned the repo, this works as-is — no external download needed.

Truncated `Result` envelope:

```json
{"kind":"result","scan_id@version":"stats.autocorr.ljung_box@1",
 "effect":{"metric":"ljung_box_q_stat","value":33.87,"p_value":0.043,
           "extra":{"lags":10,"acf":[...]}},
 "data_slice":{"sources":[{"symbol":"EURUSD","side":"bid",...}]}, ...}
```

For the full catalogue of 23 scans, the sweep-manifest TOML grammar, the exit
code routing, and a programmatic-consumption walkthrough, see
[docs/agent_integration.md](docs/agent_integration.md),
[docs/scan_catalogue.md](docs/scan_catalogue.md), and
[docs/sweep_manifest.md](docs/sweep_manifest.md).

## Data source caveats

`tradedesk-miner` reads the cache layout `tradedesk-dukascopy` produces. A few
non-obvious conventions matter for interpreting findings:

- Months are **00-indexed** on disk (`2024/00/` = January, `2024/11/` = December).
- The `volume` column is a **tick count**, not lot volume.
- Bid and ask sides are processed independently; spread reconstruction is out of scope.
- Weekend and exchange-holiday gaps are intentional, not missing data; the
  `gap_policy` flag controls how scans treat them.

See [docs/data_sources.md](docs/data_sources.md) for the full reference, including
the data licensing posture.

## Design principles

- **Locked `Finding` envelope.** Seven-variant tagged enum (`run_start`,
  `result`, `scan_error`, `gap_aborted`, `dry_run`, `sweep_summary`,
  `run_end`) with frozen common fields. Schema-additive only; ground truth
  is `schemas/findings-v1.schema.json`.
- **Stdout = findings, stderr = logs.** `StdoutSink` is the only writer to
  `io::stdout()` in the workspace; `tracing` routes structured logs to
  stderr. CI-enforced via `clippy::disallowed_macros`.
- **Tokio-free `miner-core`.** The scan engine is sync + `rayon` only;
  async lives only at the wrapper edges via `spawn_blocking`. CI-enforced
  by a `cargo tree -p miner-core` gate.
- **Config precedence.** CLI flag > env var > TOML file > error. No
  hardcoded paths in the library; the CLI owns config-path resolution.
- **Envelope-byte determinism.** Re-runs with the same seed against the
  same code revision + cache produce byte-identical NDJSON once the four
  volatile fields (`run_id`, timestamps, `wall_clock_ms`) are masked.

## Architecture

See [ARCHITECTURE.md](ARCHITECTURE.md) for the system map.
[`.planning/research/ARCHITECTURE.md`](.planning/research/ARCHITECTURE.md)
carries the deeper layered design rationale.

## Roadmap

See [`.planning/ROADMAP.md`](.planning/ROADMAP.md). Plan-by-plan summaries
live under `.planning/phases/<phase>/<phase>-<plan>-SUMMARY.md`.

## Documentation

Start with:

- [ARCHITECTURE.md](ARCHITECTURE.md)
- [docs/findings_envelope.md](docs/findings_envelope.md)
- [docs/scan_catalogue.md](docs/scan_catalogue.md)
- [docs/sweep_manifest.md](docs/sweep_manifest.md)
- [docs/agent_integration.md](docs/agent_integration.md)
- [docs/future_mcp_http.md](docs/future_mcp_http.md)

Runnable examples live under [docs/examples/](docs/examples/).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup, quality gates,
and PR expectations.

## License

Licensed under the Apache License, Version 2.0.
See: [https://www.apache.org/licenses/LICENSE-2.0](https://www.apache.org/licenses/LICENSE-2.0)

Copyright 2026 [Radius Red Ltd.](https://github.com/radiusred) |
[Contact](mailto:opensource@radiusred.uk)
