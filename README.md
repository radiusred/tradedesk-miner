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

Phase 1 (Foundations & Contracts), Phase 2 (reader / aggregator / cache),
Phase 3 (scan engine + facade + CLI), and Phase 4 (the v1 scan catalogue)
complete. Phase 4 ships 22 registered scans across three families — 11
single-instrument anomaly tests (ANOM), 5 two-instrument cross scans
(CROSS), and 6 seasonality scans (SEAS) — every one callable via
`miner scan <id>@<version>` with identical envelope shape across families
(single discriminant by `scan_id`, scan-specific extras in
`effect.extra`). The Phase 1 contract surface — locked `Finding` JSON
Schema, single sink writer, config precedence, CI gates — is FROZEN;
later phases add hygiene + wrappers on top without re-litigating the
envelope.

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

## Running a Scan (Phase 3)

The `miner scan` subcommand executes one scan invocation end-to-end and
streams `RunStart` → per-finding envelopes (`Result` / `ScanError` /
`GapAborted` / `DryRun`) → `RunEnd` as JSONL on stdout.

1. **Discover registered scans.** `miner scans` emits one JSONL line per
   registered scan with `scan_id`, `version`, `params` (JSON Schema), and
   `finding_fields`.

   ```sh
   MINER_CACHE_ROOT=/tmp/cache \
   MINER_BAR_CACHE_ROOT=/tmp/bar \
   MINER_OUTPUT=stdout \
   cargo run -p miner-cli -- scans | jq .
   ```

   Phase 3 ships exactly one scan (`stats.autocorr.ljung_box@1`). Lines
   validate against `schemas/scans-catalogue-v1.schema.json`.

2. **Run the Ljung-Box demo scan** against a populated cache (synthesised by
   `tradedesk-dukascopy` or seeded for tests):

   ```sh
   MINER_CACHE_ROOT=/path/to/cache \
   MINER_BAR_CACHE_ROOT=/tmp/bar \
   MINER_OUTPUT=stdout \
   cargo run -p miner-cli -- scan stats.autocorr.ljung_box@1 \
       --instrument EURUSD --side bid --timeframe 15m \
       --window 2024-01-01:2024-12-31
   ```

   Exit code routing (D3-24): `0` clean run, `1` preflight rejection
   (unknown scan / invalid params), `2` at least one mid-stream
   `ScanError`, `130` SIGINT — all already-streamed findings persist on
   stdout (D-19 per-envelope flush).

3. **Dry-run** prints the resolved request + planned `data_slice` + an
   estimated findings count and exits 0 WITHOUT touching the bars:

   ```sh
   cargo run -p miner-cli -- scan stats.autocorr.ljung_box@1 \
       --instrument EURUSD --timeframe 15m \
       --window 2024-01-01:2024-01-02 \
       --dry-run
   ```

   The stream is `[run_start, dry_run, run_end]` — no `Result` envelope is
   emitted; `RunSummary.results_emitted == 0`.

4. **Gap policy.** `--gap-policy strict` (the high-correctness choice)
   emits one `Finding::GapAborted` carrying the full Phase-2 gap manifest
   on any data hole, then `RunEnd` — no `Result`. `--gap-policy
   continuous_only` (the default) partitions the requested window into
   maximal gap-free sub-ranges and emits one `Result` per sub-range with
   the manifest inlined into `data_slice.gap_manifest`.

## Phase 4 Scan Catalogue (ANOM / CROSS / SEAS)

Phase 4 lands 22 v1 scans plus the ANOM-04 squared-returns variant in the
shared registry. Run `cargo run -p miner-cli -- scans | jq '.'` to list
every catalogue entry with its `arity` (`"single"` / `"pair"`),
`param_schema`, and the `effect.extra` / `raw.series` keys it will
emit on success.

Below are three representative invocations — one per scan family —
showing the `miner scan` command line and the expected JSONL fragment.
Per-scan parameter docs are reachable via
`miner scans | jq '.[] | select(.scan_id == "<id>")'`.

1. **ANOM: Welford summary statistics** (`stats.summary.welford@1`).

   Computes mean / std / skew / excess-kurtosis / IQR / min / max of bar
   log-returns via the bias-corrected G1 / G2 Welford pass. Single-leg
   (one `--instrument`).

   ```sh
   cargo run -p miner-cli -- scan stats.summary.welford@1 \
       --instrument EURUSD:bid --timeframe 15m \
       --window 2024-01-01:2024-12-31
   ```

   Expected first `Result` line (truncated):

   ```json
   {"kind":"result","scan_id_at_version":"stats.summary.welford@1",
    "effect":{"metric":"summary_welford_mean","value":-1.23e-6,
              "extra":{"excess_kurtosis":...,"iqr":...,"max":...,
                       "min":...,"n":...,"skew":...,"std":...}},
    "data_slice":{"sources":[{"symbol":"EURUSD",...}]},...}
   ```

2. **CROSS: Engle-Granger cointegration** (`cross.cointegration.engle_granger@1`).

   Two-step Engle-Granger pairs-trading diagnostic — OLS hedge ratio,
   ADF on residuals, OU half-life. Pair arity (two `--instrument` flags;
   leg order = regressand `a`, regressor `b` per D4-09).

   ```sh
   cargo run -p miner-cli -- scan cross.cointegration.engle_granger@1 \
       --instrument EURUSD:bid --instrument GBPUSD:bid \
       --timeframe 15m \
       --window 2024-01-01:2024-12-31
   ```

   Expected first `Result` line (truncated):

   ```json
   {"kind":"result","scan_id_at_version":"cross.cointegration.engle_granger@1",
    "effect":{"metric":"engle_granger_hedge_ratio","value":0.873,
              "p_value":0.041,
              "extra":{"adf_stat":...,"hedge_ratio_alpha":...,
                       "ou_half_life":...,"residual_std":...,
                       "residuals":...}},
    "data_slice":{"sources":[
        {"symbol":"EURUSD",...},{"symbol":"GBPUSD",...}]},...}
   ```

3. **SEAS: Hour-of-day return profile** (`seas.bucket.hour_of_day@1`).

   Buckets log-returns by UTC bar-close hour, emits 24 parallel arrays
   (means / stds / counts / t-stats / IQRs) plus the max-abs t-stat
   headline.

   ```sh
   cargo run -p miner-cli -- scan seas.bucket.hour_of_day@1 \
       --instrument EURUSD:bid --timeframe 15m \
       --window 2024-01-01:2024-12-31
   ```

   Expected first `Result` line (truncated):

   ```json
   {"kind":"result","scan_id_at_version":"seas.bucket.hour_of_day@1",
    "effect":{"metric":"hour_of_day_max_abs_t_stat","value":2.14,
              "n":671,
              "extra":{"buckets":[0,1,...,23],"counts":...,"iqrs":...,
                       "means":...,"stds":...,"t_stats":...}},
    "data_slice":{"sources":[{"symbol":"EURUSD",...}]},...}
   ```

Per ROADMAP Phase 4 Success Criterion #5, statsmodels / scipy / pandas
golden fixtures are checked in for one representative scan from each
family under `crates/miner-core/tests/goldens/`. The pinned reference
versions live at `crates/miner-core/tests/goldens/REFERENCE-VERSIONS.md`
and the regen recipe is bundled with each `generate_<scan>.py` script.

## Quickstart — Sweep with Hygiene (Phase 5)

Phase 5 lands the **sweep runner** and **statistical hygiene** layer (OP-04
+ HYG-01..05). One TOML manifest declares a cartesian grid of
`scan × instruments × timeframes × windows × params` jobs; `miner sweep`
fans those jobs out in parallel, applies per-job bootstrap CIs +
null-distribution p-values, runs Benjamini-Hochberg FDR over the
streamed p-values per `[fdr].family` scope, and emits one
`Finding::SweepSummary` envelope between the last `Finding::Result` and
`Finding::RunEnd`. Every run is byte-identical-reproducible from a
single `[sweep].seed` (HYG-05).

The canonical Quant-agent workflow:

1. **Author a TOML manifest.** Save as `example.toml`:

   ```toml
   [sweep]
   seed = 0xDEADBEEF
   max_jobs = 100

   [hygiene]
   bootstrap = "stationary"
   bootstrap_n = 500
   null = "circular_shift"
   null_n = 500

   [fdr]
   family = "scan_id"
   alpha = 0.05

   [[jobs]]
   scan = "stats.autocorr.ljung_box@1"
   instruments = ["EURUSD:bid", "GBPUSD:bid"]
   timeframes = ["15m"]
   windows = ["2024-01-01:2024-06-30"]
   params = { lags = [10] }
   ```

2. **Run the sweep:**

   ```sh
   MINER_CACHE_ROOT=/path/to/cache \
   MINER_BAR_CACHE_ROOT=/tmp/bar \
   MINER_OUTPUT=stdout \
   cargo run -p miner-cli -- sweep example.toml
   ```

   Expected stdout structure (newline-delimited JSON):

   ```text
   {"kind":"run_start", ...}
   {"kind":"result", "scan_id_at_version":"stats.autocorr.ljung_box@1",
    "effect":{"metric":"ljung_box_q_stat","value":...,"p_value":0.043,
              "ci95":{"low":...,"high":...}},
    "repro":{"master_seed":3735928559,"job_seed":...,
             "bootstrap":{"method":"stationary","n":500},
             "null":{"method":"circular_shift","n":500}}, ...}
   {"kind":"result", "scan_id_at_version":"stats.autocorr.ljung_box@1", ...}
   {"kind":"sweep_summary",
    "fdr_by_family":{"stats.autocorr.ljung_box@1":{
        "method":"benjamini_hochberg","alpha":0.05,
        "per_finding":[{"finding_index":0,"raw_p":...,"q_value":...},
                       {"finding_index":1,"raw_p":...,"q_value":...}]}},
    "totals":{"jobs_run":2,"results_emitted":2,"scan_errors":0,"gap_aborted":0}}
   {"kind":"run_end", ...}
   ```

3. **Dry-run** previews the cartesian-expanded job graph without
   touching scan bodies:

   ```sh
   cargo run -p miner-cli -- sweep example.toml --dry-run
   ```

   Emits one `DryRunFinding` with `planned_job_count = 2`; no `Result`
   or `SweepSummary` envelopes follow. Exit 0.

4. **Single-shot equivalent** — `miner scan` accepts the same universal
   `--bootstrap` / `--bootstrap-n` / `--null` / `--null-n` / `--seed`
   hygiene flags as the sweep manifest, so the same statistical
   hygiene contract is reachable interactively:

   ```sh
   cargo run -p miner-cli -- scan stats.autocorr.ljung_box@1 \
       --instrument EURUSD:bid --timeframe 15m \
       --window 2024-01-01:2024-06-30 \
       --bootstrap stationary --bootstrap-n 500 \
       --null circular_shift --null-n 500 \
       --seed 0xDEADBEEF
   ```

5. **SIGINT during a sweep** preserves every already-streamed
   `Result` envelope; the `SweepSummary` is intentionally suppressed
   (the BH-FDR computation requires the full set of p-values).
   Exit code 130 per D3-24.

The committed `schemas/sweep-manifest-v1.schema.json` (regenerable via
`cargo run -p xtask -- gen-schema`) documents the typed manifest grammar
so MCP/HTTP wrappers in Phase 6 can validate manifests at the
wire boundary without coupling to `miner-core`'s internal types.

### References

- Politis & Romano (1994), *The Stationary Bootstrap*. JASA 89(428).
- Theiler, Eubank, Longtin, Galdrikian, Farmer (1992), *Testing for
  nonlinearity in time series: the method of surrogate data*. Physica D 58.
- Benjamini & Hochberg (1995), *Controlling the False Discovery Rate*.
  JRSS-B 57(1).

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


## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup, quality gates,
and PR expectations.

## License

Licensed under the Apache License, Version 2.0.
See: [https://www.apache.org/licenses/LICENSE-2.0](https://www.apache.org/licenses/LICENSE-2.0)

Copyright 2026 [Radius Red Ltd.](https://github.com/radiusred) |
[Contact](mailto:opensource@radiusred.uk)
