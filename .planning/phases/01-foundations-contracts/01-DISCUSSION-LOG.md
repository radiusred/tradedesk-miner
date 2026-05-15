# Phase 1: Foundations & Contracts - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-05-15
**Phase:** 1-Foundations & Contracts
**Areas discussed:** Raw-array encoding, Error record semantics, Stream framing records, Schema artifact

---

## Preamble — User's Framing

The user is non-Rust and explicitly delegated all Rust-ecosystem choices to "the most pragmatic and/or popular options Rust community members would recognise." Of the four initially-proposed areas, three were pure ecosystem (stdout-discipline lint mechanism, config crate + format, time crate) and were locked by Claude using community defaults without further discussion. The fourth area — the Finding envelope shape — was split into four contract-level sub-areas the user does have authority over (because the Quant agent has to parse the output). Those four are recorded below.

**Ecosystem defaults locked by Claude (not discussed):**
- Stdout discipline → `clippy::disallowed_macros` + single `findings::sink` module with `#[allow]`
- Config → `figment` crate + TOML file + `MINER_*` env prefix
- Time → `chrono`

---

## Raw-Array Encoding

### Q1 — Encoding mode

| Option | Description | Selected |
|--------|-------------|----------|
| Flag-controlled, base64 default | Default to `f64_le_base64`; `--raw=inline-json` switches to readable arrays; `--no-raw` omits arrays entirely. Best of all worlds — fast for agents, debuggable for humans, terse for triage runs. | |
| Always base64 | Single fast path, no flag complexity. Consumer always decodes the same way. Eyeballing requires a small decode helper. Simpler contract; less ergonomic for ad-hoc human use. | ✓ |
| Always inline JSON | Maximally readable. Big tax on sweep volume; slow parses; not the agent-optimal path. | |
| You decide | Defer to Claude's judgment. | |

**User's choice:** Always base64.
**Notes:** Cleanest contract. A `miner decode-raw` helper subcommand can be added later if eyeballing arrays at the terminal ever becomes painful — deferred.

### Q2 — Shape metadata pattern

| Option | Description | Selected |
|--------|-------------|----------|
| Self-contained objects | `raw.series.returns = { data, shape, dtype }`. Each array carries everything to decode it. Slightly verbose; eliminates the bug where `series` and `shapes` get out of sync. | ✓ |
| Parallel maps | `raw.series.<name> = '<base64>'`, `raw.shapes.<name> = [..]`. Flatter, marginally smaller. Risk: drift between the two maps. | |
| You decide | Pick whichever is more idiomatic for numpy consumers. | |

**User's choice:** Self-contained objects.

### Q3 — Timestamps array

| Option | Description | Selected |
|--------|-------------|----------|
| Always include `timestamps_ms` | Every finding with raw arrays also ships `timestamps_ms` (f64 LE base64, same length as primary series). Lets consumer re-plot / re-test without re-querying. | ✓ |
| Only when scan declares need | Saves bytes; consumer has to know per-scan what's there. | |
| Never — derive from `data_slice` | Doesn't work with gap-filtered bars (timestamps aren't a regular grid). | |

**User's choice:** Always include `timestamps_ms`.
**Notes:** Gap policy means we can't reconstruct timestamps from the range descriptor — they have to be shipped explicitly.

### Q4 — Where do scan-derived arrays go?

| Option | Description | Selected |
|--------|-------------|----------|
| Clean input/output split | `raw.series.*` = ONLY input data the scan consumed. `effect.value` + `effect.extra.*` = scan-derived outputs. Arrays in `effect.extra` use the same `{data, shape, dtype}` shape. | ✓ |
| Everything numeric in `raw` | `raw.series` holds both input (returns) and output (ACF). `effect` carries only headline scalars. Loses input/output distinction. | |
| You decide | | |

**User's choice:** Clean input/output split.

---

## Error Record Semantics

### Q1 — Mid-stream scan failure in a sweep

| Option | Description | Selected |
|--------|-------------|----------|
| Emit `kind: scan_error` and continue | Sweep continues; error record carries scan context (scan_id, params, instrument, timeframe, message, error_code, run_id). Exit 0 if any results emitted, non-zero if 100% failed. | ✓ |
| Abort entire run on first scan error | Cleaner failure semantics; loses already-computed findings. | |
| Configurable via flag | `--on-error=continue|fail-fast`. Surface area; risk of agent picking wrong mode. | |

**User's choice:** Emit `kind: scan_error` finding and continue.

### Q2 — Pre-flight errors

| Option | Description | Selected |
|--------|-------------|----------|
| Abort + structured error to stderr + non-zero exit | Stderr line is JSON like `{ error: 'invalid_parameter', parameter: 'timeframe', got: '1m', allowed: [...] }`. Stdout empty. Fail-fast — agent knows it's a bug, not data. | ✓ |
| Emit single `kind: scan_error` to stdout + exit 0 | Uniform stream-based contract. Mixes 'bug in request' with 'data went wrong'. | |
| You decide | | |

**User's choice:** Abort with structured error to stderr + non-zero exit.

### Q3 — Exit code taxonomy

| Option | Description | Selected |
|--------|-------------|----------|
| Three-tier 0/1/2 | 0 = clean run; 1 = pre-flight/catastrophic; 2 = ran with mid-stream errors. Agent branches without parsing stream. | ✓ |
| Binary 0 / non-zero | 0 = anything emitted; non-zero = pre-flight/catastrophic. Agent has to parse stdout to detect mid-stream errors. | |
| You decide | | |

**User's choice:** Three-tier 0/1/2.

### Q4 — Gap-policy outputs

| Option | Description | Selected |
|--------|-------------|----------|
| Gap-policy outputs are findings | Under `strict`, emit ONE `kind: gap_aborted` record per scan + gap manifest, exit 0 (policy did its job). Under `continuous_only`, gap manifest rides in each finding's `data_slice`. Gaps are data conditions, not errors. | ✓ |
| Gap-strict aborts are `scan_error` records | Under strict, emit `kind: scan_error` with error_code='coverage_gap'. Exit 2. Simpler kind taxonomy; conflates 'data unsuitable' with 'something broke'. | |
| You decide | | |

**User's choice:** Gap-policy outputs are findings, not errors.

---

## Stream Framing Records

### Q1 — Emit `run_start` / `run_end`?

| Option | Description | Selected |
|--------|-------------|----------|
| Always, on every invocation | Every run brackets findings with framing records. Agent detects partial runs (missing run_end) without exit-code parsing. Two-record overhead; `jq 'select(.kind=="result")'` filters them. | ✓ |
| Sweep mode only | Single-scan invocations skip framing. Less noise for ad-hoc CLI use; agent has to know mode. | |
| Never — stream results only | Simplest contract; loses partial-run detection (network drop on HTTP/MCP looks identical to clean end). | |
| You decide | | |

**User's choice:** Always, on every invocation.

### Q2 — `run_id` shape

| Option | Description | Selected |
|--------|-------------|----------|
| Always unique ULID, time-prefixed | Sortable by creation time, globally unique, two replays get different IDs (correct — they're distinct executions). | ✓ |
| Deterministic hash of request | Two replays collide on run_id (cache-style dedup). Conflates 'same logical run' with 'distinct executions'. | |
| You decide | | |

**User's choice:** Always-unique ULID.

### Q3 — Framing payload detail

| Option | Description | Selected |
|--------|-------------|----------|
| Rich — full request echo + per-scan summary | `run_start` carries fully-resolved request; `run_end` carries `wall_clock_ms` + counts + per-scan-id breakdown. Max observability; ~1 KB overhead. | ✓ |
| Minimal | `run_start` = `{run_id, started_at_utc}`; `run_end` = `{run_id, ended_at_utc, wall_clock_ms, results_emitted, scan_errors}`. Smaller; agent tracks request shape itself. | |
| You decide | | |

**User's choice:** Rich.

---

## Schema Artifact

### Q1 — Publish a checked-in schema file?

| Option | Description | Selected |
|--------|-------------|----------|
| Yes — checked into repo, part of public contract | `schemas/findings-v1.schema.json` in repo. Quant agent / tradedesk can validate parser-side. Breaking changes show up as file diffs. | ✓ |
| No — internal validation only | Schema lives only as a Rust validator in CI (insta snapshots). Consumers parse pragmatically. Less coupling, less rigour. | |
| Yes for stable releases only | Pre-1.0 skips published schema; ships from v1.0. Avoids churn early. | |

**User's choice:** Yes — checked-in public-contract artifact.

### Q2 — Schema synchronisation

| Option | Description | Selected |
|--------|-------------|----------|
| Derive from Rust types via `schemars` | Annotate envelope types with `#[derive(JsonSchema)]`; xtask regenerates the .json file; CI fails if checked-in file out of sync. Single source of truth. Rust-community default. | ✓ |
| Hand-write schema, validate Rust output against it in CI | Hand-written file is source of truth; CI emits sample findings and validates via `jsonschema` crate. More maintenance; precise control over docs. | |
| You decide | | |

**User's choice:** Derive via `schemars`.

### Q3 — Versioning

| Option | Description | Selected |
|--------|-------------|----------|
| One file per major version | `findings-v1.schema.json`; v2 lands alongside when schema_version bumps; v1 kept for old-replay validation. Clear migration story. | ✓ |
| Single rolling `findings.schema.json` | Always reflects current schema; old versions live in git history. Consumer parsing old data checks out an older commit. | |

**User's choice:** One file per major version.

---

## Claude's Discretion

The following decisions were locked by Claude using Rust-community default patterns, per the user's explicit preamble:

- **Stdout discipline mechanism** (D-15): `clippy::disallowed_macros` + single `findings::sink` module exemption.
- **Config crate + format** (D-16): `figment` + TOML + `MINER_*` env prefix + `$XDG_CONFIG_HOME/miner/miner.toml` default lookup.
- **Time crate** (D-17): `chrono`.
- **Error model** (D-18): `thiserror` in library, `anyhow` in binaries, `serde::Serialize` on library errors so they embed in findings.
- **Stdout writer pattern** (D-19): `FindingSink` is the only type that calls `Write` against stdout.
- **Workspace conventions** (D-20): edition 2024, MSRV 1.85, pinned via `rust-toolchain.toml`.
- **CI provider + four mandatory gates** (D-21): GitHub Actions; build, clippy with disallowed-macros, `cargo tree -p miner-core` async-free, schema-sync diff check.
- **Schema runtime validation** (D-22): `cargo test` constructs example envelopes per `kind` and validates via the `jsonschema` crate.
- **`miner-bench` scaffolding** (D-23): empty crate in Phase 1; real harness lands in Phase 7.
- **ULID crate** (D-24): the `ulid` crate.

The user is welcome to override any of these during planning if they have a specific preference. None of them shape the agent-facing contract; they're all implementation-mechanism choices.

---

## Deferred Ideas

- `miner decode-raw` CLI subcommand for ad-hoc terminal inspection of base64 raw arrays (revisit if base64-only becomes a debugging-friction problem).
- `kind: progress` records for long-running sweeps (revisit if sweep wall-clock grows enough that an agent needs progress signals; for now SIGINT is the cancellation pattern, lands in Phase 3).
- Deterministic `run_id` from request shape (would conflict with the always-unique decision; if replay/audit ever needs a stable ID, add a separate `request_fingerprint` field rather than overload `run_id`).
- JSON Schema `$id` / `$schema` URL hosting for fetch-by-URL validation (v2 concern; v1 ships as checked-in file).
- Real `miner-bench` harness (Phase 7).

---

*Discussion captured: 2026-05-15*
