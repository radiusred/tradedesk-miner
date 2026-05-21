# Findings envelope reference

`miner-core` emits every finding as a tagged `Finding` JSON envelope on stdout — one NDJSON line per envelope. This doc is the human-readable companion to [`../schemas/findings-v1.schema.json`](../schemas/findings-v1.schema.json) — the JSON Schema is the authoritative source of truth (regenerated from the schemars derives on the Rust types under `crates/miner-core/src/findings/`); this doc explains what each field means and why it is shaped the way it is.

The envelope shape is locked. `schema_version = 1` is permanent for v1; every change in the v1 line is additive (new optional fields under `#[serde(default)]`), never breaking. Reserved-but-null slots (`dsr`, `fdr_q`) exist precisely so v2 can populate them without bumping `schema_version`.

## The seven Finding variants

Every line in a miner run is one of seven `Finding::*` arms (`crates/miner-core/src/findings/mod.rs` line 545), discriminated by the top-level `"kind"` field via `#[serde(tag = "kind", rename_all = "snake_case")]`:

- `RunStart` — first line of every run; carries `run_id`, `started_at_utc`, `miner_version`, `code_revision`, and the resolved `request` blob (the fully-defaulted invocation).
- `Result` — the headline payload. One envelope per `(scan x instrument(s) x timeframe x window x param-point)`. Carries all locked envelope fields plus the per-scan `effect` block.
- `ScanError` — mid-run scan failure. Preflight passed but `Scan::run` errored, the kernel rejected inputs, or computation produced an unrecoverable result. The run continues; only the offending job is lost.
- `GapAborted` — strict gap policy fired and aborted before any `Result` was emitted on this slice; carries the full gap manifest so consumers can see what was missing.
- `DryRun` — emitted only under `--dry-run`. Carries the planned `data_slice` plus `estimated_findings_count` and (for `miner sweep --dry-run`) the `planned_job_count`.
- `SweepSummary` — last data line of a `miner sweep` invocation, emitted immediately before `RunEnd`. Carries `SweepTotals` plus per-family `FdrFamilySummary` entries with Benjamini-Hochberg-adjusted q-values.
- `RunEnd` — final line of every run. Carries `ended_at_utc`, `wall_clock_ms`, and the aggregate `RunSummary { results_emitted, scan_errors, gap_aborted, per_scan }`.

Three of the seven (`Result`, `ScanError`, `GapAborted`) carry the locked envelope fields. The four framing-like variants (`RunStart`, `RunEnd`, `DryRun`, `SweepSummary`) intentionally do NOT carry them — framing records identify the run, not a particular scan-on-instrument result.

## Stream order

The order of finding kinds within a single run is contractual:

1. Exactly one `RunStart` opens the stream.
2. Zero or more `Result` / `ScanError` / `GapAborted` envelopes interleave (in `rayon`-deterministic order for sweeps — see `sweep::executor::run_sweep`).
3. For `miner sweep` only, exactly one `SweepSummary` follows the last result-bearing envelope.
4. Exactly one `RunEnd` closes the stream.

`DryRun` envelopes are emitted between `RunStart` and `RunEnd` and never co-occur with result-bearing envelopes — a dry-run never executes a scan kernel. See `crates/miner-core/src/findings/mod.rs` test `dry_run_does_not_increment_results_emitted` for the type-level invariant pin: a `DryRun` envelope MUST NOT touch `RunSummary.results_emitted`.

Consumers MUST NOT assume the absence of `SweepSummary` for a `miner scan` invocation indicates a failure — single-shot scans never emit it.

## Common envelope fields

The three result-bearing variants (`Result`, `ScanError`, `GapAborted`) carry these locked fields inlined into each payload struct (NOT via `#[serde(flatten)]` — see the module doc-comment in `findings/mod.rs` for the anti-pattern rationale):

- `schema_version` (u32; `1` in v1; bumps on a non-additive break).
- `scan_id@version` — serialised key (the Rust field is `scan_id_at_version`, renamed via `#[serde(rename = "scan_id@version")]`). String of the form `"<scan_id>@<u32 version>"`, e.g. `"stats.autocorr.ljung_box@1"`.
- `param_hash` — blake3 hex over the canonical sorted-key resolved-param JSON (post-defaults). Identical params produce identical hashes regardless of TOML / JSON key order.
- `code_revision` — `git describe`-style string injected by `build.rs` at compile time. Format: `"<sha>"` or `"dirty-<sha>"` when the working tree had uncommitted changes at build.
- `data_slice` — the input range the scan actually consumed (see next section).
- `dsr` — reserved for v2 Deflated Sharpe Ratio. Serialises as JSON `null` in v1 (NOT omitted).
- `fdr_q` — reserved for the BH-FDR adjusted q-value populated by the sweep summary aggregator. Serialises as JSON `null` outside a sweep.

Additionally every `Result` envelope carries:

- `run_id` — the parent `RunStart`'s ULID, echoed verbatim so all findings in one run share an identifier.
- `produced_at_utc` — RFC 3339 UTC timestamp.
- `params` — the resolved parameter map (post-defaults). `serde_json::Value` shape — typically `{"key": value, ...}`.
- `instruments` — leg-labelled instrument vector inherited from `ScanRequest.instruments`. Length 1 for ANOM / SEAS Single-arity scans, length 2 for CROSS Pair-arity scans. Each entry is an `InstrumentSpec { symbol, side }` (Phase 4 / D4-01).
- `timeframe` — the bar-resolution string (e.g. `"15m"`, `"1h"`, `"1d"`). Always present alongside `instruments`.
- `effect` — the test statistic + p-value + optional CI + optional effect-size + per-scan extras (see "Effect block" below).
- `raw` — optional inputs the scan consumed (base64-encoded little-endian raw bytes; see "Raw arrays" below).
- `repro` — optional reproducibility envelope (populated when bootstrap / null resampling produced the finding's p-value or CI; see "Reproducibility envelope" below).

A representative `Result` envelope (abridged):

```json
{
  "kind": "result",
  "schema_version": 1,
  "scan_id@version": "stats.autocorr.ljung_box@1",
  "param_hash": "blake3-abc123...",
  "code_revision": "v0.1.0-12-gabc1234",
  "data_slice": { "range": { "start_utc": "2024-06-12T00:00:00Z", "end_utc": "2024-06-13T00:00:00Z" }, "gap_manifest_ref": null, "gap_manifest": null, "sources": [{ "source_id": "dukascopy", "symbol": "EURUSD", "side": "bid", "timeframe": "15m" }] },
  "dsr": null,
  "fdr_q": null,
  "run_id": "01HWQ5MMNJ9CWGR7XGMR4PHZJX",
  "produced_at_utc": "2024-06-13T00:00:01Z",
  "params": { "lags": 5 },
  "effect": { "metric": "ljung_box_q", "value": 12.34, "p_value": 0.0123, "n": 96, "ci95": null, "effect_size": null, "extra": { "...": "..." } },
  "raw": { "series": { "returns": { "data": "...base64...", "shape": [95], "dtype": "f64" }, "timestamps_ms": { "data": "...", "shape": [95], "dtype": "i64" } } },
  "repro": null
}
```

## RunStart / RunEnd framing

The first and last lines of every run are framing records. They identify the run; they are NOT result-bearing.

`RunStart` (`crates/miner-core/src/findings/mod.rs` lines 280-289) carries:

- `run_id` — Crockford-base32 ULID (26 chars, e.g. `"01HWQ5MMNJ9CWGR7XGMR4PHZJX"`). Copied verbatim onto every result-bearing envelope produced by this run.
- `started_at_utc` — RFC 3339 UTC timestamp.
- `miner_version` — semver string from the crate's `Cargo.toml`.
- `code_revision` — same `git describe`-style string injected by `build.rs` as on result-bearing envelopes.
- `request` — the fully-resolved invocation (`scan_id@version`, instrument(s), side, timeframe, window, params with defaults applied, `gap_policy`, `dry_run` flag). `serde_json::Value` shape; the structure mirrors the engine's `ScanRequest`.

`RunEnd` (lines 294-300) carries:

- `run_id` + `ended_at_utc`.
- `wall_clock_ms` — total wall-clock duration of the run.
- `summary: RunSummary { results_emitted, scan_errors, gap_aborted, per_scan }` — aggregate counters across the run. `per_scan: BTreeMap<String, PerScanCounts>` keyed by `scan_id@version`. `PerScanCounts { results, errors, gap_aborted }` is `Copy`-able for use in test fixtures.

Exhaustive destructure of `RunSummary` is asserted by the `run_summary_has_no_dry_run_emitted_field` test (Warning 9 pin): adding a new field to `RunSummary` would break the destructure at compile time, signalling envelope-shape drift before tests even run.

## data_slice

The `data_slice` block (`crates/miner-core/src/findings/mod.rs` lines 80-102) carries everything a consumer needs to know about the input range the scan actually consumed:

- `range: TimeRange { start_utc, end_utc }` — the half-open continuous range used by the scan (post gap-partitioning under `continuous_only` policy). UTC RFC 3339 strings.
- `gap_manifest_ref: Option<String>` — reserved for the content-addressed deduplication path; `null` in v1.
- `gap_manifest: Option<GapManifest>` — the full inline Phase 2 `GapManifest` under `--gap-policy=continuous_only`. `null` on the strict success path and in pre-Phase-3 callers. **Serialises as JSON `null` when absent — NEVER omitted.** Same convention as `dsr` / `fdr_q`.
- `sources: Vec<Source>` — leg-labelled source vector (Phase 4 / D4-03). Length matches the scan's `arity().expected_len()`: 1 for ANOM / SEAS, 2 for CROSS.

Each `Source` is `{ source_id, symbol, side, timeframe }` — e.g. `{ "source_id": "dukascopy", "symbol": "EURUSD", "side": "bid", "timeframe": "15m" }`.

Pair-arity / CROSS scans populate exactly two `Source` entries in `ScanRequest.instruments` order. The inline `gap_manifest` for a two-leg scan is the INTERSECTION of both legs' gaps (D4-04) — the continuous sub-range emitted is the maximal gap-free intersection of the two leg coverages.

## Effect block

The `effect` block (`crates/miner-core/src/findings/mod.rs` lines 174-203) is the headline scan output:

- `metric: String` — short identifier of the test statistic. Examples: `"ljung_box_q"`, `"adf_statistic"`, `"jarque_bera_statistic"`, `"vr_minus_one"`, `"pearson_corr_last"`, `"lead_lag_argmax_lag"`, `"hour_of_day_max_abs_t_stat"`. See [scan_catalogue.md](scan_catalogue.md) for the per-scan `effect.metric` literal.
- `value: f64` — the canonical effect statistic.
- `p_value: Option<f64>` — present on most scans; some emit only a statistic (e.g. variance-ratio reports per-`k` p-values inside `extra` and leaves `p_value = null`).
- `n: Option<u64>` — sample size used.
- `ci95: Option<[f64; 2]>` — `[lower, upper]` confidence interval at 95%. Populated only when the caller opted into bootstrap resampling (HYG-03).
- `effect_size: Option<EffectSize>` — standardised effect-size statistic alongside `value` (HYG-01 / D5-03). Each `EffectSize { kind, value }` carries an open-string `kind` (canonical values: `"cohens_d"`, `"hedges_g"`, `"cliffs_delta"`, `"vr_minus_one"`, plus scan-specific kinds). `null` for scans that do not emit one.
- `extra: BTreeMap<String, RawArray>` — per-scan extras carried as base64-with-shape arrays (same shape as `raw.series` entries). Examples: `{"acf", "lags", "p_values", "q_stats"}` for Ljung-Box; `{"argmax_lag", "argmax_value", "ccf_values", "lags", "max_lag"}` for lead-lag CCF; `{"buckets", "counts", "iqrs", "means", "stds", "t_stats"}` for the seasonality bucketers. See [scan_catalogue.md](scan_catalogue.md) for the per-scan key list.

`BTreeMap` ordering means `effect.extra` keys are alphabetic and stable across re-runs (`OUT-03`).

## Raw arrays

Optional inputs the scan consumed live in `raw.series` (`crates/miner-core/src/findings/mod.rs` lines 129-161). The block follows the base64-with-shape pattern (D-02):

- `Raw { series: BTreeMap<String, RawArray> }` — `BTreeMap` (NEVER `HashMap`) for alphabetic ordering.
- `RawArray { data: Base64Bytes, shape: Vec<u64>, dtype: Dtype }`:
  - `data` — standard base64 (RFC 4648) over the raw little-endian bytes.
  - `shape` — JSON array of unsigned ints, e.g. `[95]` or `[1024, 4]`.
  - `dtype` — one of `"f64"`, `"f32"`, `"i64"`, `"i32"`, `"u64"`, `"u32"` (the `Dtype` enum is open-additive — see `findings/base64_bytes.rs`).

Every `Raw` block MUST carry a `timestamps_ms` key (D-03; enforced at construction by `Raw::new`). The `timestamps_ms` array is i64 little-endian milliseconds-since-Unix-epoch and is parallel to the other arrays in the block.

Canonical Python decode one-liner:

```python
import base64, numpy as np
arr = np.frombuffer(base64.b64decode(raw_array["data"]), dtype="<f8").reshape(raw_array["shape"])
```

A worked end-to-end example lives at `docs/examples/decode_finding.py` (Plan 06-03).

Decode example for a typical Ljung-Box `Result` envelope:

```python
import base64, json, numpy as np
line = json.loads(stdin_line)
assert line["kind"] == "result"
arr = line["raw"]["series"]["returns"]
returns = np.frombuffer(base64.b64decode(arr["data"]), dtype="<f8").reshape(arr["shape"])
ts_arr = line["raw"]["series"]["timestamps_ms"]
timestamps_ms = np.frombuffer(base64.b64decode(ts_arr["data"]), dtype="<i8").reshape(ts_arr["shape"])
assert returns.shape == timestamps_ms.shape
#returns[i] is the log-return at timestamps_ms[i]; reconstruct any per-scan invariant from this pair.
```

The dtype string (`"<f8"` for f64, `"<i8"` for i64, etc.) follows NumPy's standard little-endian shorthand. The shape array is the multi-dimensional shape; flat 1-D arrays appear as `[N]`.

## Reproducibility envelope

When a scan ran under bootstrap or null resampling (HYG-03 / HYG-04), `ResultFinding.repro` is populated with a `ReproEnvelope` (`crates/miner-core/src/findings/mod.rs` lines 254-269):

- `master_seed: u64` — the seed the user passed via `--seed` or the sweep manifest `[sweep].seed`. Echoed verbatim on every finding from one run so consumers can correlate.
- `job_seed: u64` — per-job seed derived deterministically from the master seed plus the canonical job key (`scan_id@version`, instruments, timeframe, window, `param_hash`) via blake3. Equal seeds across re-runs produce byte-identical findings.
- `bootstrap: Option<BootstrapSpec>` — `BootstrapSpec { method, n }` when bootstrap CIs were computed. v1 `method` values: `"stationary"` (Politis-Romano), `"block"`. `n` is the number of resamples drawn.
- `null: Option<NullSpec>` — `NullSpec { method, n }` when null resampling produced the p-value. v1 `method` values: `"phase_scramble"`, `"circular_shift"`. `n` is the number of null draws.

The population rule (enforced by the Plan 05-03 engine integration): `repro = Some(_)` iff bootstrap or null was run for the finding. Closed-form-only findings carry `repro = null`. The envelope serialises as JSON `null` when absent (NOT omitted).

The RNG state in `ReproEnvelope` is NOT cryptographic. `Xoshiro256PlusPlus` is the seeded generator; predicting future outputs from observed outputs is trivial. The threat model has no secrecy requirement for resampling RNG state (`T-05-01-I1`).

## Error envelopes

Two finding kinds carry typed error vocabularies.

`ScanError` carries an open-string `error_code` plus a free-form `message` and a `request_context` JSON blob. The `error_code` is constructed from the typed `ScanErrorCode` enum (`crates/miner-core/src/error/codes.rs`); v1 variants:

- `coverage_gap` — coverage check failed mid-run on this slice.
- `compute_error` — kernel rejected the inputs (NaN propagation, ill-conditioned regression, insufficient post-window samples, etc.).
- `cache_corruption` — the derived-bar cache produced an unreadable frame.
- `internal_panic_caught` — defensive catch-unwind around `Scan::run`; the kernel panicked but the run continues.

Preflight failures are NOT mid-stream `ScanError` envelopes. They are emitted as a single `WireError` on stderr (with the run's `RunStart` already on stdout if framing started) and the process exits with code 1. The `PreflightCode` vocabulary (`crates/miner-core/src/error/codes.rs`) covers:

- `invalid_parameter`, `unknown_scan`, `unknown_instrument`, `wrong_instrument_arity`, `missing_required_config`, `invalid_config`, `sweep_too_large`, `hygiene_not_supported`, `internal_error`.

`GapAborted` is a separate kind, NOT a `ScanError`. It is emitted exactly once per scan run under `--gap-policy=strict` when the gap manifest disallows the requested window (D-08). The `gap_manifest` field carries the full Phase 2 manifest (tuples of `(start, end, reason)` for missing daily files, corrupt files, intra-day holes against the trading calendar).

## Stdout, stderr, and exit codes

The envelope on stdout is one half of miner's contract; stderr is the other.

- **Stdout** carries NDJSON `Finding` envelopes only — one per line, no leading whitespace, no padding, terminated by `\n`. The single sanctioned writer is `FindingSink` (production impls: `StdoutSink` and `FileSink`). Scans never call `println!` or any other stdout writer; `clippy::disallowed_macros` rejects this at build time outside the sink module (D-15, D-19).
- **Stderr** carries structured `tracing` events (instrument / scan / job spans) plus the single `WireError` envelope emitted on preflight rejection. Consumers parse stderr as line-delimited JSON when invoked via subprocess and treat any non-JSON line as a free-form log message.
- **Exit codes** follow the four-tier mapping (D3-24):
  - `0` — run completed (zero or more `Result` / `ScanError` envelopes emitted, `RunEnd` closed the stream).
  - `1` — preflight rejection (a single `WireError` on stderr, no `RunStart` on stdout). Examples: `unknown_scan`, `invalid_parameter`, `sweep_too_large`.
  - `2` — fatal mid-run error (catastrophic engine failure; no further structured envelopes guaranteed).
  - `130` — SIGINT (Ctrl-C). Already-streamed envelopes persist; the run is interrupted between envelopes.

The locked envelope discipline is what makes subprocess invocation work: a consumer reading stdout line-by-line as JSON can correctly classify every line by `"kind"` without out-of-band coordination.

## SweepSummary fields

`SweepSummary` is emitted exactly once at the end of a `miner sweep` invocation, immediately before `RunEnd`. Single-shot `miner scan` runs never emit it.

`SweepSummaryFinding` (`crates/miner-core/src/findings/mod.rs` lines 463-476) carries:

- `run_id` + `produced_at_utc` — matches the surrounding `RunStart` / `RunEnd` for cross-finding correlation.
- `fdr_by_family: BTreeMap<String, FdrFamilySummary>` — one entry per family. Keyed by `scan_id@version` (default `[fdr].family = "scan_id"`) or by `scan_family` per configuration. `BTreeMap` ordering guarantees alphabetic key emission.
- `totals: SweepTotals { jobs_run, results_emitted, scan_errors, gap_aborted }` — run-level aggregates.

Each `FdrFamilySummary { method, alpha, per_finding }` carries:

- `method` — open-string FDR method; `"benjamini_hochberg"` in v1.
- `alpha` — FDR control level (typical: 0.05 or 0.10).
- `per_finding: Vec<FindingFdrEntry { finding_index, raw_p, q_value }>` — one row per finding in the family in stable index order, where `finding_index` is the zero-indexed position of the finding within the family in the streaming JSONL output. `Vec` (NEVER `HashMap`) preserves the index alignment.

Consumers that want q-values on individual `Result` envelopes can post-process: read the streaming JSONL, group `Result` envelopes by their `scan_id@version`, then at end-of-sweep look up each finding's `q_value` in the matching `FdrFamilySummary.per_finding` by index.

## Reserved-but-null fields

`dsr` and `fdr_q` on every `Result` / `ScanError` / `GapAborted` envelope serialise as JSON `null` in v1, NOT as absent fields (`crates/miner-core/src/findings/mod.rs` line 338-342). Why they exist:

- `dsr` — Deflated Sharpe Ratio. Reserved for v2; the slot is locked into the v1 schema so v2 can populate it additively without bumping `schema_version`.
- `fdr_q` — BH-FDR adjusted q-value for a single finding. Populated only by the SweepSummary aggregator on its `per_finding` rows; on the individual `Result` envelope it remains `null` (consumers join by index — see "SweepSummary fields" above). Reserved at the per-finding level so v2 may inline q-values directly on `Result` without a schema bump.

This is the same null-not-omitted convention used by `data_slice.gap_manifest` and `effect.effect_size`. DO NOT treat absent and null as equivalent — `null` is the explicit signal that v1 has no value to report.

### Wire-form summary table

| Field                         | Variants carrying it             | v1 value source                                  |
| ----------------------------- | -------------------------------- | ------------------------------------------------ |
| `schema_version`              | Result, ScanError, GapAborted    | Constant `1`                                     |
| `scan_id@version`             | Result, ScanError, GapAborted    | `Scan::id()` + `Scan::version()`                 |
| `param_hash`                  | Result, ScanError, GapAborted    | blake3 over canonical resolved params            |
| `code_revision`               | Result, ScanError, GapAborted, RunStart | `miner_core::CODE_REVISION` (`build.rs`)  |
| `dsr`                         | Result, ScanError, GapAborted    | Always `null` in v1 (reserved for v2)            |
| `fdr_q`                       | Result, ScanError, GapAborted    | Always `null` on per-finding envelopes in v1     |
| `repro`                       | Result                           | `Some(_)` iff bootstrap or null was run          |
| `effect.ci95`                 | Result                           | `Some([lo, hi])` iff bootstrap was run           |
| `effect.effect_size`          | Result                           | `Some({kind, value})` iff D5-03 emitted one      |

## See Also

- [scan_catalogue.md](scan_catalogue.md) — the 23 v1 scan_ids + per-scan `effect.metric` + `effect.extra` keys.
- [sweep_manifest.md](sweep_manifest.md) — TOML sweep grammar (the source of `SweepSummary`).
- [../ARCHITECTURE.md](../ARCHITECTURE.md) — system map; the locked envelope discipline in context.
- [../schemas/findings-v1.schema.json](../schemas/findings-v1.schema.json) — authoritative JSON Schema (regenerated from the Rust types via `xtask`).

---

## License

Licensed under the Apache License, Version 2.0.
See: https://www.apache.org/licenses/LICENSE-2.0

Copyright 2026 [Radius Red Ltd.](https://github.com/radiusred) | [Contact](mailto:opensource@radiusred.uk)
