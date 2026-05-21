# Agent integration guide

miner is designed primarily for programmatic consumption from a CLI subprocess. This guide walks an agent through spawning miner, parsing the JSONL stream on stdout, decoding base64 raw arrays, routing on the four-tier exit code, and handling SIGINT cleanly. MCP and HTTP transports are documented separately in [future_mcp_http.md](future_mcp_http.md) — they are deferred to v2; the CLI is the load-bearing v1 surface.

The contract narrated here is locked. `schema_version = 1` is permanent for v1; every change is additive. Anything documented below that has a `BTreeMap` ordering invariant or a `null`-not-omitted convention is contractual — see [findings_envelope.md](findings_envelope.md) for the per-field reference.

## What miner exposes

- `miner scan <scan_id@version>` — run one scan; stream NDJSON `Finding` envelopes to stdout.
- `miner sweep <manifest.toml>` — fan out a cartesian (scan x instrument x timeframe x window x params) grid; stream findings plus a closing `SweepSummary` envelope.
- `miner scans` — JSONL catalogue introspection; one envelope per registered scan with its parameter schema, `arity`, and finding fields.
- `miner emit-fixture` — emit the Phase 1 smoke fixture (one `RunStart` + one `RunEnd`); reserved for golden re-generation and CI smoke tests.
- `--dry-run` — on `scan` or `sweep`, emit a single `DryRunFinding` carrying the resolved request + planned `data_slice` + `estimated_findings_count` (plus `planned_job_count` for sweeps); no scan kernel executes.
- Stdout = findings NDJSON. Stderr = structured `tracing` logs + (preflight only) a single `WireError` envelope. Never mixed (D-15 / D-19; CI-enforced).

## Spawning miner from your agent

The canonical pattern is `subprocess.Popen` with line-by-line stdout iteration:

```python
import subprocess
import json

proc = subprocess.Popen(
    [
        "miner", "scan", "stats.autocorr.ljung_box@1",
        "--instrument", "EURUSD:bid",
        "--timeframe", "15m",
        "--window", "2024-06-12:2024-06-13",
        "--param", "lags=5",
    ],
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    text=True,
)

for line in proc.stdout:
    envelope = json.loads(line)
    # discriminate by envelope["kind"]; see next section
    ...

rc = proc.wait()
```

Notes:

- `text=True` decodes stdout as UTF-8; miner only ever emits valid UTF-8 NDJSON.
- For low-latency streaming, set `bufsize=1` to force line buffering on stdout.
- `stderr=subprocess.PIPE` is recommended so log lines and any preflight `WireError` do not pollute the agent's own stderr; read them in a separate thread if you want real-time log surfacing.
- The `--param` flag may be repeated; each pair is `key=value`. The full canonical invocation form is documented in the README's Quickstart.

## Parsing the JSONL stream

Every line is a tagged `Finding` envelope. Discriminate on the top-level `"kind"` field, which `serde` renders as the snake_case variant name:

```python
for line in proc.stdout:
    envelope = json.loads(line)
    kind = envelope["kind"]
    if kind == "run_start":
        run_id = envelope["run_id"]
    elif kind == "result":
        handle_result(envelope)
    elif kind == "scan_error":
        log_scan_error(envelope)
    elif kind == "gap_aborted":
        record_gap(envelope)
    elif kind == "dry_run":
        record_plan(envelope)
    elif kind == "sweep_summary":
        record_sweep_totals(envelope)
    elif kind == "run_end":
        finalise(envelope)
    else:
        # unknown kind; v1 has exactly the seven above. v2 may add more
        # additively. Forward-compatible consumers log and continue.
        log_unknown_envelope(envelope)
```

Stream order is contractual: exactly one `run_start` opens the stream, zero or more `result` / `scan_error` / `gap_aborted` / `dry_run` envelopes follow in rayon-deterministic order, then for sweeps only exactly one summary envelope (kind sweep_summary), then exactly one `run_end`. See [findings_envelope.md](findings_envelope.md#stream-order) for the full ordering contract.

## Decoding raw arrays

Every `Result` envelope may carry a `raw.series` block of base64-encoded little-endian arrays alongside `effect.extra` (which uses the same `RawArray` shape). The canonical one-liner is `np.frombuffer(base64.b64decode(raw["data"]), dtype="<f8").reshape(raw["shape"])` — wrapped in a small helper:

```python
import base64
import numpy as np

def decode_raw_array(raw_array):
    """Decode one RawArray dict {dtype, shape, data} into a numpy ndarray."""
    return np.frombuffer(base64.b64decode(raw_array["data"]), dtype=raw_array["dtype"]).reshape(tuple(raw_array["shape"]))
```

The `dtype` strings match NumPy's standard little-endian shorthand:

- `"f64"` ⟷ `np.dtype("<f8")` (8-byte IEEE-754 double).
- `"f32"` ⟷ `np.dtype("<f4")`.
- `"i64"` ⟷ `np.dtype("<i8")`.
- `"i32"` ⟷ `np.dtype("<i4")`.
- `"u64"` ⟷ `np.dtype("<u8")`.
- `"u32"` ⟷ `np.dtype("<u4")`.

`shape` is a JSON array of unsigned integers — `[95]` for a flat 95-element series, `[1024, 4]` for a 2-D array. Every `Raw` block carries a `timestamps_ms` array parallel to the other entries (D-03; enforced at construction by `Raw::new` in `crates/miner-core/src/findings/mod.rs`).

A runnable end-to-end decoder lives at [examples/decode_finding.py](examples/decode_finding.py) — read one `Result` line from stdin, decode every `raw.series` array, and reprint a summary. Run it as `miner scan ... | python docs/examples/decode_finding.py`.

## Exit codes

miner follows the four-tier exit-code routing locked by D3-24:

- `0` — clean run; the stream closed with `RunEnd`; the agent should process every envelope received.
- `1` — preflight or kernel error. A `WireError` envelope was emitted on stderr (preflight rejection, before any `RunStart`) OR one or more `Finding::ScanError` envelopes appeared mid-stream. The agent MUST inspect the envelope's `error_code` (mid-stream) or `code` (preflight) field rather than treating exit 1 as opaque. `RunEnd` may still be present if the failure was mid-stream.
- `2` — invalid CLI usage. clap-derive rejected the argument list; stderr carries the usage banner. No envelopes are emitted; the agent should treat this as an integration bug, not a data-error.
- `130` — SIGINT (POSIX convention `128 + 2`). The user (or the parent agent) sent Ctrl-C. Every `Result` envelope already streamed to stdout was flushed at emission time and is valid; the run terminates between envelopes; `RunEnd` may be present for `miner scan`, is suppressed for `miner sweep` (a partial sweep cannot run BH-FDR aggregation — see [findings_envelope.md](findings_envelope.md#sweepsummary-fields)).

A defensive routing pattern:

```python
rc = proc.wait()
if rc == 0:
    finalise_clean(envelopes)
elif rc == 1:
    inspect_error_envelope(envelopes, stderr_text)
elif rc == 2:
    report_integration_bug(proc.args, stderr_text)
elif rc == 130:
    finalise_partial(envelopes)
else:
    report_unexpected(rc, stderr_text)
```

## Error envelope vocabulary

The wire-form `code` (preflight `WireError`) and `error_code` (mid-stream `ScanError`) fields take values from two locked enums under `crates/miner-core/src/error/codes.rs`. The Rust types live in `codes.rs`; `mod.rs` only re-exports them, so a grep against `mod.rs` would miss every code — the source of truth for the literal wire strings is the `as_str()` arms in `codes.rs`.

**Preflight rejections** — single `WireError` on stderr, exit code 1, no `Result` envelope emitted. Nine `PreflightCode` variants:

- `invalid_parameter` (`PreflightCode::InvalidParameter`) — a CLI / sweep-manifest parameter failed type / range / enum validation. Inspect `context` for offending field. Retry only after fixing the request.
- `unknown_scan` (`UnknownScan`) — `[[jobs]].scan` or `miner scan <id>` does not resolve to a registered scan. Run `miner scans` to enumerate available IDs. Do not retry.
- `unknown_instrument` (`UnknownInstrument`) — the requested instrument is not in the source catalogue (the Dukascopy reader's `<root>/<SYMBOL>/...` layout has no matching directory). Verify cache root and symbol spelling; do not retry as-is.
- `wrong_instrument_arity` (`WrongInstrumentArity`) — a Pair-arity (CROSS) scan received one instrument, or a Single-arity (ANOM / SEAS) scan received two. Fix the request shape; do not retry.
- `missing_required_config` (`MissingRequiredConfig`) — a required `MinerConfig` field could not be resolved from any precedence layer (CLI flag > env var > TOML > error). Provide the missing setting and retry.
- `invalid_config` (`InvalidConfig`) — a config file or env value failed parse or type-check. Inspect `context`; fix the config and retry.
- `sweep_too_large` (`SweepTooLarge`) — cartesian expansion exceeds `[sweep].max_jobs` (default `100_000`; see [sweep_manifest.md](sweep_manifest.md#sweeptoolarge-preflight-rejection)). Tighten the manifest or bump `max_jobs` deliberately; do not blindly retry.
- `hygiene_not_supported` (`HygieneNotSupported`) — bootstrap or null-distribution was requested on a scan whose `Scan::supports_bootstrap()` / `supports_null_method()` returned `false`. Remove the hygiene flag for that scan, or move it to one that supports it.
- `internal_error` (`InternalError`) — catastrophic failure unrelated to inputs. Inspect stderr `tracing` log lines for the underlying cause; file a bug. Retry with the same inputs is rarely productive.

**Mid-stream `ScanError`s** — the run continues; only the offending job is lost; exit code is still 1. Four `ScanErrorCode` variants:

- `coverage_gap` (`CoverageGap`) — a coverage check failed mid-run on this slice. Under `--gap-policy=strict` an upstream gap was discovered after preflight (e.g. a corrupt daily file). Inspect `request_context` for the failing range; retry with `--gap-policy continuous_only` or a tighter window if you can tolerate partial coverage.
- `compute_error` (`ComputeError`) — the kernel rejected the inputs (NaN propagation, ill-conditioned regression, insufficient post-window samples, etc.). Inspect `message` for the underlying diagnostic. Retry only after addressing the input pathology (longer window, different params).
- `cache_corruption` (`CacheCorruption`) — the derived-bar cache produced an unreadable Arrow IPC frame. The two-axis invalidation usually heals this on the next run; if not, delete the offending `(symbol, side, timeframe)` cache file and re-run.
- `internal_panic_caught` (`InternalPanicCaught`) — a panic was caught at the scan boundary. Should be vanishingly rare. File a bug with the offending request_context; do not retry as-is.

`GapAborted` is a separate finding kind, NOT a `ScanError`. It is emitted exactly once per scan run under `--gap-policy=strict` when the precomputed gap manifest disallows the requested window. The envelope carries the full `gap_manifest` so an agent can decide whether to widen its window, switch to `continuous_only`, or skip the slice. Exit code remains `0` for a clean `GapAborted` (the run completed; the data just disallowed the requested coverage).

## Catalogue introspection (miner scans)

`miner scans` emits one JSONL line per registered scan; the agent reads it as machine-readable catalogue introspection:

```sh
miner scans | jq -c '.'
```

Each line carries `scan_id`, `version`, the combined `scan_id_at_version` key, the `arity` (`"single"` or `"pair"`), the `param_schema` (JSON Schema for the scan's parameter object), and the `finding_fields` shape (`effect.metric`, the alphabetised list of `effect.extra` keys, the list of `raw.series` keys). For the human-readable per-family overview see [scan_catalogue.md](scan_catalogue.md); the live `miner scans` stream is the source of truth for parameter schemas because it is regenerated from the registry at every miner build.

The recommended discovery flow for an agent that wants to call a previously-unknown scan:

1. Run `miner scans`; cache the JSONL output (it is stable for a given miner binary build).
2. For the target `scan_id`, read `param_schema` to know which parameters are required and what types they accept.
3. Spawn `miner scan <scan_id@version>` with `--param key=value` flags matching the schema.
4. If preflight rejects with `unknown_scan` or `invalid_parameter`, your cached catalogue is stale relative to the binary — re-run `miner scans`.

## Reproducibility

miner is bit-for-bit reproducible by design (HYG-05 / D5-05). The `--seed <u64>` flag (or `[sweep].seed` in a manifest) is the master seed; every `ResultFinding.repro.master_seed` echoes it verbatim. Per-job seeds derive from the master via blake3 over the canonical `(scan_id@version, instruments, timeframe, window, param_hash)` tuple, so identical inputs produce identical per-job seeds and therefore byte-identical findings — the canonical pin is the `derive_job_seed` helper under `crates/miner-core/src/sweep/repro.rs`.

Two consequences for agents:

- **Caching across re-runs is safe.** If the (scan, instruments, timeframe, window, params, seed) tuple is unchanged, the agent can elide a re-run and serve the cached `Result` envelope verbatim. The `code_revision` field on every envelope identifies which miner build produced the finding; bump it on miner upgrade and invalidate the cache.
- **Golden-file diffing is supported (OUT-03).** Streaming JSONL output is stable across re-runs once the four known-volatile fields (`run_id`, `started_at_utc`, `ended_at_utc`, `wall_clock_ms`) are masked. The Phase 1 smoke test `cli_streams::emit_fixture_byte_identical_when_volatile_fields_masked` is the load-bearing pin.

`master_seed` is also the right join key when the agent wants to correlate findings from a single sweep across multiple post-processing stages — every envelope in one run shares the same `master_seed`.

## SIGINT handling

Sending SIGINT to a running miner (D3-22) triggers a graceful shutdown:

- The rayon worker pool drains in flight; no in-progress scan kernel is killed mid-write.
- Every `Result` envelope that reached stdout was flushed at emission time and is preserved.
- The CLI's installed `ctrlc` handler sets a `Cancelled` flag the engine polls between envelopes; the run terminates between envelopes, not within one.
- Exit code is `130` (POSIX `128 + 2`).
- For `miner scan`, `RunEnd` is emitted with whatever counters were observed up to the cancellation.
- For `miner sweep`, `RunEnd` is emitted but **`SweepSummary` is intentionally suppressed** — a partial sweep does not have the full set of p-values needed to run BH-FDR aggregation; emitting a half-computed `fdr_by_family` would be a footgun. Agents that need partial sweep summaries should re-run with a tighter manifest.

Agents that drive miner with a deadline can `proc.send_signal(signal.SIGINT)` and trust that already-streamed envelopes remain valid for the agent's caching tier.

## Reading stderr alongside stdout

stderr carries two distinct kinds of content:

- **Structured `tracing` events** — one log line per event (engine span entries, sweep job lifecycle, gap-policy decisions). Lines may or may not parse as JSON depending on `MINER_LOG_FORMAT` (default `compact`; set to `json` for machine-readable structured-log lines).
- **Single `WireError` envelope on preflight rejection** — a one-shot structured-error line emitted to stderr just before exit when preflight fails. The envelope shape is `{ "code": "<snake_case>", "message": "...", "context": { ... } }` matching the source `WireError` struct in `crates/miner-core/src/error/codes.rs`.

The recommended pattern for agents that want to surface both:

```python
import threading

def drain_stderr(stream, sink):
    for line in stream:
        sink.append(line.rstrip("\n"))

stderr_lines = []
threading.Thread(
    target=drain_stderr, args=(proc.stderr, stderr_lines), daemon=True
).start()
```

When exit code is `1`, scan the drained stderr lines for the last well-formed JSON object — that is the `WireError` envelope.

## Configuration precedence

`MinerConfig` settings resolve via a layered precedence (`crates/miner-core/src/config/`):

1. **CLI flag** — highest precedence; e.g. `--cache-root /tmp/cache`.
2. **Environment variable** — `MINER_CACHE_ROOT`, `MINER_BAR_CACHE_ROOT`, `MINER_OUTPUT`.
3. **TOML config file** — discovered via XDG / CWD lookup; layered with `figment`.
4. **Error** — no default; missing required fields trigger `missing_required_config`.

Agents that drive miner across many users / project roots should prefer explicit CLI flags (precedence 1) for reproducibility — relying on environment variables makes the invocation context-sensitive and harder to cache deterministically.

## Hygiene opt-in

Bootstrap CIs and null-distribution p-values are caller-opt-in per D5-04. Pass `--bootstrap stationary --bootstrap-n 999` (or `block`) to enable bootstrap resampling; pass `--null circular_shift --null-n 999` (or `phase_scramble`) to enable a null-distribution p-value. Only scans whose `Scan::supports_bootstrap()` / `Scan::supports_null_method()` returns true accept these — others reject at preflight with `hygiene_not_supported`. The cost is roughly linear in `n_iter`; defaults are tuned for development latency, production agents typically lift to 999 or 9_999.

In a sweep, hygiene is configured globally in the `[hygiene]` block of the TOML manifest and may be overridden per-job via `[jobs.hygiene]` — see [sweep_manifest.md](sweep_manifest.md#hygiene-block) for the merge semantics. The single-shot `miner scan` flags and the manifest block accept identical wire-form values.

When hygiene runs, the finding's `repro` envelope is populated (`BootstrapSpec { method, n }` / `NullSpec { method, n }` per [findings_envelope.md](findings_envelope.md#reproducibility-envelope)) and `effect.ci95` carries the bootstrap-derived 95% confidence interval. Findings produced without hygiene leave both fields `null` — not absent.

## See Also

- [findings_envelope.md](findings_envelope.md) — locked envelope schema reference (per-field, per-variant)
- [scan_catalogue.md](scan_catalogue.md) — the 23 v1 `scan_id@version` strings and per-scan `effect.extra` keys
- [sweep_manifest.md](sweep_manifest.md) — TOML sweep grammar plus hygiene + FDR blocks
- [future_mcp_http.md](future_mcp_http.md) — deferred MCP + HTTP wrapper design sketch
- [examples/decode_finding.py](examples/decode_finding.py) — runnable base64 raw-array decoder
- [examples/sample_sweep.toml](examples/sample_sweep.toml) — runnable sample sweep manifest
- [../ARCHITECTURE.md](../ARCHITECTURE.md) — system map

---

## License

Licensed under the Apache License, Version 2.0.
See: https://www.apache.org/licenses/LICENSE-2.0

Copyright 2026 [Radius Red Ltd.](https://github.com/radiusred) | [Contact](mailto:opensource@radiusred.uk)
