# Sweep manifest reference

`miner sweep <manifest.toml>` accepts a TOML file describing scans x instruments x timeframes x windows x parameter grids and fans them out in parallel via `rayon`. This doc is the field-by-field reference for the TOML grammar that `miner_core::sweep::manifest::read_manifest` parses.

The sweep runner emits the same locked `Finding` envelopes as `miner scan` — see [findings_envelope.md](findings_envelope.md). The only sweep-specific envelope is `Finding::SweepSummary`, emitted exactly once at the end of every sweep run.

## Overview

- **Cartesian fanout:** every `[[jobs]]` block expands into one `ResolvedJob` per `(scan x instrument-spec x timeframe x window x param-point)`.
- **Hygiene + FDR opt-in:** `[hygiene]` and `[fdr]` are optional global blocks; per-`[[jobs]]` `[jobs.hygiene]` overrides shallow-merge over the global block via `merge_hygiene`.
- **Deterministic seed propagation:** `[sweep].seed` is the master seed; per-job seeds derive via blake3 over the resolved (scan, instrument, timeframe, window, params) tuple. Byte-identical findings across re-runs.
- **Dry-run support:** `--dry-run` emits a single `Finding::DryRun` envelope carrying the resolved-job graph plus `planned_job_count`; no scan executes.
- **End-of-sweep summary:** a single `Finding::SweepSummary` is emitted before `RunEnd`, carrying `SweepTotals` plus per-family `FdrFamilySummary` entries with Benjamini-Hochberg-adjusted q-values.
- **Preflight rejection:** a `SweepTooLarge` preflight error fires if the cartesian expansion would exceed `[sweep].max_jobs` (default 100_000); the sweep does not start. The `T-05-04-V5-SIZE` threat-model mitigation.

## Basic Usage

The shipped manifest at [`docs/examples/sample_sweep.toml`](examples/sample_sweep.toml) — tested against the EURUSD/GBPUSD `:bid` Jan-2024 cache — runs clean (0 hygiene-induced `scan_error` envelopes) under `miner sweep`.

The canonical 2-job manifest used by the smoke test at `crates/miner-core/tests/sweep_smoke.rs` lines 58-75:

```toml
[sweep]
seed = 305419896

[[jobs]]
scan = "stats.autocorr.ljung_box@1"
instruments = ["EURUSD:bid", "GBPUSD:bid"]
timeframes = ["15m"]
windows = ["2024-06-12:2024-06-13"]
params = { lags = 5 }

[[jobs]]
scan = "stats.autocorr.ljung_box_sq@1"
instruments = ["EURUSD:bid", "GBPUSD:bid"]
timeframes = ["15m"]
windows = ["2024-06-12:2024-06-13"]
params = { lags = 5 }
```

This expands to: 2 `[[jobs]]` blocks x 2 instruments x 1 timeframe x 1 window x 1 param-point = 4 `ResolvedJob`s. Output: 4 `Finding::Result` envelopes + 1 `Finding::SweepSummary` + bracketing `RunStart` / `RunEnd`.

A canonical sweep with optional hygiene and FDR blocks:

```toml
[sweep]
seed = 0xDEADBEEF
max_jobs = 1000

[hygiene]
bootstrap = "stationary"
bootstrap_n = 1000
null = "circular_shift"
null_n = 1000

[fdr]
family = "scan_id"
alpha = 0.05

[[jobs]]
scan = "stats.autocorr.ljung_box@1"
instruments = ["EURUSD:bid"]
timeframes = ["15m", "1h"]
windows = ["2024-06-12:2024-06-30"]
params = { lags = [5, 10] }
```

The `params = { lags = [5, 10] }` syntax declares a fanout axis: the block expands to 1 instrument x 2 timeframes x 1 window x 2 param-points = 4 jobs.

## [sweep] block

The top-level `[sweep]` block (`SweepConfig` in `crates/miner-core/src/sweep/manifest.rs` line 67) carries:

- `seed: Option<u64>` — master seed; propagates to every `ResolvedJob`'s `ReproEnvelope.master_seed`. Hex literals accepted (`seed = 0xDEADBEEF`). `None` allows a per-run randomly-drawn seed (engine determines).
- `max_jobs: u64` — cardinality ceiling (default `100_000`). The cartesian expansion is rejected with `PreflightCode::SweepTooLarge` if `estimated_job_count > max_jobs`. This is the `T-05-04-V5-SIZE` DOS mitigation: a manifest declaring 10^9 jobs MUST NOT materialise `Vec<ResolvedJob>`.

When the `[sweep]` table is omitted entirely, `SweepConfig::default()` provides `seed = None` and `max_jobs = 100_000`.

## [[jobs]] block

Each `[[jobs]]` block (`JobBlock` at line 141) declares one fanout axis-set. Required keys:

- `scan: String` — full `scan_id@version` string (e.g. `"stats.autocorr.ljung_box@1"`). Cross-link to [scan_catalogue.md](scan_catalogue.md) for the inventory of valid IDs.
- `instruments: serde_json::Value` — string array form depends on the scan's arity:
  - **Single-arity** (ANOM / SEAS): flat array of strings — `["EURUSD:bid", "GBPUSD:bid"]` declares two single-leg jobs.
  - **Pair-arity** (CROSS): nested 2-array — `[["EURUSD:bid", "GBPUSD:bid"]]` declares one two-leg job. Each inner array MUST be exactly length 2.
- `timeframes: Vec<String>` — list of `"15m"` / `"1h"` / `"1d"` etc. Each becomes a fanout axis.
- `windows: Vec<String>` — list of ISO-date ranges in `"YYYY-MM-DD:YYYY-MM-DD"` form (closed-closed UTC). Each becomes a fanout axis.

Optional keys:

- `gap_policy: Option<String>` — `"strict"` or `"continuous_only"`; overrides the run-level default.
- `params: BTreeMap<String, serde_json::Value>` — inline-table `{ key = value, ... }` or expanded `[jobs.params]` block. Each TOML array param becomes a fanout axis (e.g. `params = { lags = [5, 10, 20] }` triples the job count for this block).
- `hygiene: Option<HygieneBlock>` — per-job override of the global `[hygiene]` block.

The arity-vs-instruments-shape check is enforced at preflight (`PreflightCode::InvalidParameter` on mismatch).

## [hygiene] block

The optional global `[hygiene]` block (`HygieneBlock` at line 94) plus per-block `[jobs.hygiene]` override:

- `bootstrap: Option<String>` — wire-form bootstrap method. v1 values: `"stationary"` (Politis-Romano stationary bootstrap) and `"block"` (fixed-block bootstrap). Mapped to the typed `BootstrapMethod` enum via `parse_bootstrap_method`.
- `bootstrap_n: u32` — number of bootstrap resamples to draw. Capped at the engine's `HYGIENE_RESAMPLE_CEILING`; values over the ceiling are rejected at preflight rather than silently clamped.
- `null: Option<String>` — wire-form null-distribution method. v1 values: `"circular_shift"` and `"phase_scramble"`. Mapped to typed `NullMethod`.
- `null_n: u32` — number of null draws. Same ceiling rule as `bootstrap_n`.

Only scans whose `Scan::supports_bootstrap()` returns true accept `bootstrap`; same gate for `null` via `Scan::supports_null_method()`. Unsupported requests are rejected at preflight with `PreflightCode::HygieneNotSupported`.

`merge_hygiene` semantics: when both global and per-block specify a method, per-block wins. For the `_n` fields, the per-block value wins when non-zero; a zero per-block value inherits the global. See `crates/miner-core/src/sweep/manifest.rs` lines 426-445 for the canonical implementation.

## [fdr] block

The optional global `[fdr]` block (`FdrConfig` at line 105):

- `family: String` — FDR-scope discriminator. Default `"scan_id"` (one BH family per `scan_id@version`); v1 also accepts `"scan_family"` (one BH family per scan-family prefix) and `"none"` (suppress per-family BH; emit empty `fdr_by_family`).
- `alpha: f64` — FDR control level. Default `0.05`. Rejected at preflight if outside `[0, 1]` or NaN.

The `[fdr].family` enum is intentionally open-string (NOT a sealed enum) so v2 can add new families (e.g. `"scan_family_and_timeframe"`) additively without a schema break.

## Per-job hygiene override

Per-block `[jobs.hygiene]` shallow-merges over the global `[hygiene]` block. Useful when one scan in the sweep needs different resample counts:

```toml
[hygiene]
bootstrap = "stationary"
bootstrap_n = 500
null = "circular_shift"
null_n = 500

[[jobs]]
scan = "stats.autocorr.ljung_box@1"
instruments = ["EURUSD:bid"]
timeframes = ["15m"]
windows = ["2024-06-12:2024-06-30"]
#Inherits the global hygiene block verbatim.

[[jobs]]
scan = "cross.cointegration.engle_granger@1"
instruments = [["EURUSD:bid", "GBPUSD:bid"]]
timeframes = ["1h"]
windows = ["2024-06-12:2024-06-30"]

[jobs.hygiene]
#Override one knob; inherit the rest from global.
bootstrap_n = 2000
```

The second job runs with `bootstrap = "stationary"` + `bootstrap_n = 2000` + `null = "circular_shift"` + `null_n = 500`. The shallow-merge rule: per-block fields with non-default values win; absent / default per-block fields fall through to the global.

The per-block hygiene also has to clear the `Scan::supports_bootstrap()` + `Scan::supports_null_method()` gates. A `[jobs.hygiene]` block requesting an unsupported method is rejected at preflight with `PreflightCode::HygieneNotSupported`, scoped to the offending block index in the error message (`[[jobs[N]]]` where N is the block index).

## Per-block gap_policy

The optional `gap_policy: Option<String>` field on `[[jobs]]` lets a single block opt into a stricter / more lenient policy than the run-level default:

- `"strict"` — abort on any gap; emit a single `Finding::GapAborted` for the slice; produce zero `Result` envelopes.
- `"continuous_only"` — partition the requested window into maximal gap-free sub-ranges; emit one `Result` per sub-range with the inline `gap_manifest` carried on `data_slice.gap_manifest`.

Omitting `gap_policy` on a block falls through to the run-level default (set by `--gap-policy` on the CLI). Mixing policies across blocks within a sweep is supported — useful when some scans require continuous data and others tolerate gaps.

## Resolved job graph + planned_job_count

Under `--dry-run`, the sweep runner emits exactly one `Finding::DryRun` envelope carrying the planned `data_slice`, `estimated_findings_count`, and the sweep-specific `planned_job_count` (the count after full cartesian expansion). No scan kernel executes.

A `miner sweep --dry-run` invocation against the basic-usage manifest above produces:

```jsonl
{"kind":"run_start", ...}
{"kind":"dry_run", "planned_job_count": 4, "planned_data_slice": {...}, ...}
{"kind":"run_end", ...}
```

Note: a single-shot `miner scan --dry-run` invocation leaves `planned_job_count = null` (the field is `Option<u64>` with `#[serde(default)]` per the additive Plan 05-04 change — see `findings/mod.rs` lines 449-450). The `null`-vs-`Some(_)` distinction lets consumers detect whether a dry-run came from a sweep or a single-shot.

## SweepSummary + BH-FDR scoping

The `Finding::SweepSummary` envelope is emitted once at sweep-end carrying:

- `totals: SweepTotals { jobs_run, results_emitted, scan_errors, gap_aborted }` — run-level aggregates.
- `fdr_by_family: BTreeMap<String, FdrFamilySummary>` — one entry per family. Keyed by `scan_id@version` under the default `[fdr].family = "scan_id"` scoping, or by the scan-family string when configured. `BTreeMap` (NEVER `HashMap`) for alphabetic key ordering — `OUT-03`.

Each `FdrFamilySummary { method, alpha, per_finding }` carries the BH-adjusted q-values per finding in stable index order — see [findings_envelope.md](findings_envelope.md) for the per-finding `(p_value, q_value)` join contract.

The default `family = "scan_id"` scope means: every `cross.corr.pearson_rolling@1` finding shares one BH family with every other `cross.corr.pearson_rolling@1` finding in the sweep, regardless of which `instruments` / `timeframes` / `windows` / params produced it. Switching to `"scan_family"` widens the family to all `cross.*` (or `stats.*`, or `seas.*`) findings.

## SweepTooLarge preflight rejection

If the cartesian expansion would exceed `[sweep].max_jobs`, the sweep aborts at preflight with `PreflightCode::SweepTooLarge` before any job runs. A single `WireError` envelope is emitted on stderr with the context keys `estimated_job_count` and `max_jobs`, and the process exits with code 1.

Example: a manifest with `max_jobs = 4` declaring 2 instruments x 2 timeframes x 1 window x 2 param-points = 8 jobs is rejected:

```
{"code":"sweep_too_large","message":"sweep would expand to 8 jobs; exceeds [sweep].max_jobs = 4","context":{"estimated_job_count":8,"max_jobs":4}}
```

`max_jobs` is a tunable knob — bump it explicitly when a real workload needs more. The default 100_000 ceiling is the `T-05-04-V5-SIZE` DOS mitigation; it is NOT a hard architectural limit.

## TOML parse and validation errors

Two failure paths produce a single `WireError` on stderr (no `RunStart` on stdout, exit code 1):

1. **TOML syntax error** — the bytes do not parse as valid TOML. `read_manifest` returns `MinerError::Preflight(WireError::preflight(InvalidParameter, "TOML parse error: ..."))` carrying the underlying `toml` crate diagnostic. Defence-in-depth: the `toml = "0.8"` crate enforces a 256-level nesting ceiling so a deeply-nested attack manifest cannot DOS the parser (T-05-04-V5-DEEP).

2. **Schema mismatch** — the bytes parse as TOML but fail the typed-`SweepManifest` shape (e.g. `[sweep].max_jobs` declared as a string instead of an integer). Same error path with the `serde` diagnostic threaded through.

3. **Validation failure** — the manifest parses cleanly but violates a preflight invariant. Specific `PreflightCode` returns by category:
   - `unknown_scan` — `[[jobs]].scan` does not resolve in the registry.
   - `invalid_parameter` — arity mismatch (Single-arity scan with nested `instruments`, or Pair-arity scan with flat `instruments`), out-of-range `[fdr].alpha`, or NaN alpha.
   - `hygiene_not_supported` — the merged per-block hygiene requested a method the scan rejects.
   - `sweep_too_large` — cartesian expansion exceeds `[sweep].max_jobs`.

Every preflight diagnostic carries structured context keys (e.g. `block_index`, `scan_id`, `estimated_job_count`, `max_jobs`) so consumers can route errors without parsing the message string.

## References

The statistical hygiene and FDR machinery `miner sweep` exposes draws on three primary sources:

- Politis & Romano (1994), *The Stationary Bootstrap*. JASA 89(428) — the `bootstrap = "stationary"` method's geometric block-length sampler.
- Theiler, Eubank, Longtin, Galdrikian, Farmer (1992), *Testing for nonlinearity in time series: the method of surrogate data*. Physica D 58 — the `null = "circular_shift"` and IAAFT surrogate constructions.
- Benjamini & Hochberg (1995), *Controlling the False Discovery Rate*. JRSS-B 57(1) — the BH step-up procedure applied per `[fdr].family` scope.

## See Also

- [findings_envelope.md](findings_envelope.md) — the `Finding::SweepSummary` + `DryRunFinding` shapes
- [scan_catalogue.md](scan_catalogue.md) — valid `scan_id@version` strings + per-scan params
- [architecture.md](architecture.md) — sweep runner in the data-flow context

---

## License

Licensed under the Apache License, Version 2.0.
See: https://www.apache.org/licenses/LICENSE-2.0

Copyright 2026 [Radius Red Ltd.](https://www.radiusred.uk) | [Contact](mailto:opensource@radiusred.uk)
