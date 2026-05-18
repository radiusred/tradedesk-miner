---
phase: 03-scan-engine-facade-cli
plan: 03
subsystem: scan-engine-facade-cli
tags: [engine, param-hash, framing, preflight, gap-policy, pure-functions, blocker-2]
requires:
  - "03-01 (Wave 0 scaffold — engine sub-module files with signature-only bodies)"
  - "03-02 (wire contract lock — ScanRequest.dry_run, Registry::bootstrap, GapManifest, ClosedRangeUtc serde)"
provides:
  - "`engine::param_hash::param_hash(&Value) -> Result<Blake3Hex, serde_json::Error>` — byte-stable blake3 over resolved-params (D3-13 / Pitfall 6)"
  - "`engine::framing::build_run_start(&ScanRequest, RunId, DateTime<Utc>, &str) -> Finding` — pure RunStart builder with verbatim dry_run echo (D3-21 / Blocker 2 closure)"
  - "`engine::framing::build_run_end(RunId, DateTime<Utc>, DateTime<Utc>, RunSummary) -> Finding` — pure RunEnd builder with wall_clock_ms computed from caller-supplied timestamps (D3-23)"
  - "`engine::preflight::resolve_scan_id_at_version(&str) -> Result<(String, u32), WireError>` + `resolve_scan(&str, &Registry) -> Result<&dyn Scan, WireError>` — boundary scan resolver (D3-17, UnknownScan / InvalidParameter)"
  - "`engine::preflight::parse_params_kv(&[String]) -> Result<Value, WireError>` — KEY=VAL parser with A9 typed-fallback (i64 -> f64 -> bool -> string); rejects malformed + duplicates"
  - "`engine::preflight::parse_iso_utc_window(&str) -> Result<ClosedRangeUtc, WireError>` — ISO 8601 half-open UTC window parser with strict-Z enforcement (D3-07 / A3)"
  - "`engine::preflight::classify_param_error(&serde_json::Error) -> PreflightCode` — always InvalidParameter (symmetric with classify_figment_error)"
  - "`engine::gap_policy::dispatch(&GapManifest, ClosedRangeUtc, GapPolicyKind) -> GapDispatch` — pure strict/continuous_only dispatcher (D3-08..D3-12)"
  - "`engine::{GapDispatch, GapPolicyKind}` re-exports from engine::mod for Plan 04 import ergonomics"
affects:
  - "crates/miner-core/src/engine/param_hash.rs — body filled; module-doc + 4 unit tests"
  - "crates/miner-core/src/engine/framing.rs — bodies filled; module-doc + 4 unit tests"
  - "crates/miner-core/src/engine/preflight.rs — bodies filled; module-doc + 20 unit tests"
  - "crates/miner-core/src/engine/gap_policy.rs — dispatch() body filled; 8 unit tests + 1 proptest"
  - "crates/miner-core/src/engine/mod.rs — added `pub use gap_policy::{GapDispatch, GapPolicyKind}`; cleared pre-existing clippy doc-markdown + needless-pass-by-value warnings on the scaffolded run_one stub"
  - "crates/miner-core/proptest-regressions/engine/gap_policy.txt — new regression-seed file (proptest convention; documents the hour-24 generator bug caught + fixed during this plan)"
tech-stack:
  added: []
  patterns:
    - "Pure-function dispatch (no clock reads, no IO, no allocations beyond returned Vec) — gap_policy::dispatch + param_hash::param_hash"
    - "Boundary-pure helpers (preflight + framing) — only IO is passed-in clock value (D3-23 clock-isolation)"
    - "A9 typed-fallback for CLI KEY=VAL params: serde_json::from_str RHS; on failure, treat as string. Production callers never have to quote `lags=20`."
    - "A3 strict-Z UTC enforcement for ISO 8601 datetimes in --window: explicit Z suffix required, +HH:MM rejected"
    - "Pitfall 1 byte-stability gate via BTreeMap-backed serde_json::Map (no preserve_order feature enabled)"
    - "Pitfall 6 separation pinned by function signature: param_hash(&Value) takes ONLY resolved-params, never RunStart.request"
    - "Blocker 2 closure: dry_run is ALWAYS present in RunStart.request (NEVER omitted via skip_serializing_if), mirroring the dsr/fdr_q null-but-present discipline"
key-files:
  modified:
    - "crates/miner-core/src/engine/param_hash.rs (155 lines — +134 / -39 vs Wave 0 scaffold)"
    - "crates/miner-core/src/engine/framing.rs (363 lines — +344 / -47 vs Wave 0 scaffold)"
    - "crates/miner-core/src/engine/preflight.rs (467 lines — +477 / -45 vs Wave 0 scaffold)"
    - "crates/miner-core/src/engine/gap_policy.rs (485 lines — +408 / -10 vs Wave 0 scaffold)"
    - "crates/miner-core/src/engine/mod.rs (134 lines — +18 / -8 vs Wave 0 scaffold)"
  created:
    - "crates/miner-core/proptest-regressions/engine/gap_policy.txt (7 lines — proptest regression-seed file capturing the hour-24 generator boundary)"
decisions:
  - "Doc-comment phrasings rewritten to avoid the literal substring `Utc::now` and `preserve_order` so the plan's `grep -c` acceptance gates fire on actual code matches, not on doc-references. The semantic invariants (no wall-clock reads inside framing builders; no insertion-order feature on serde_json) are preserved verbatim in the rewritten docs."
  - "The `run_one` stub in engine/mod.rs gained a scoped `#[allow(clippy::needless_pass_by_value)]` on the `cancel: Arc<AtomicBool>` argument because the unimplemented body cannot yet consume it. Plan 04 fills run_one, will consume cancel by passing it into ScanCtx, and will remove the allow."
  - "The proptest generator clamps `req_end_h` to `<= 24` and maps hour 24 to the next-day midnight (chrono rejects literal hour-24). This was discovered during proptest run; the test now sweeps the full [0, 24] hour range without panicking on chrono boundary validation."
  - "`engine/mod.rs` re-exports both `GapDispatch` and `GapPolicyKind` so Plan 04 can import them via `miner_core::engine::*` without the inner-module path. The re-export is additive — it does not break any of the explicit `use crate::engine::gap_policy::GapPolicyKind` paths already in the codebase (e.g., `scan/mod.rs:43`)."
metrics:
  duration_seconds: 1380
  completed_date: "2026-05-18T16:50:00Z"
  tasks_completed: 3
  files_touched: 6
---

# Phase 3 Plan 03: Engine Sub-Modules Summary

Three commits delivered every engine sub-module Plan 04 needs to compose into `run_one`: the canonical `param_hash` over resolved params (D3-13 + Pitfall 6 separation), the `RunStart` / `RunEnd` pure framing builders (clock-isolation per D3-23, with the dry_run echo locked in — Blocker 2 closure for D3-21), the four preflight mappers (scan_id resolver, KEY=VAL params parser, ISO 8601 window parser, error classifier — D3-07, D3-19, OP-08), and the gap-policy dispatch (strict vs continuous_only partitioning per D3-08/D3-10/D3-11/D3-12).

## One-liner

Five engine sub-modules filled with pure / boundary-pure bodies: `param_hash` is byte-stable blake3 over BTreeMap-backed resolved-params; `framing` builders echo `req.dry_run` verbatim into `RunStart.request` and accept caller-supplied timestamps (no implicit wall-clock reads); `preflight` resolves scans + parses KEY=VAL with A9 typed-fallback + parses ISO 8601 windows with A3 strict-Z; `gap_policy::dispatch` is pure and never silently emits findings over a hole (proptest pin).

## What changed

### Task 1 — `param_hash` + framing builders with dry_run echo (commit `93c2aec`)

**`engine/param_hash.rs` (155 lines, 4 unit tests):**

Mirrors the dukascopy reader's `fingerprint_day` idiom (`crates/miner-reader-dukascopy/src/reader.rs:123-141`):

```rust
let bytes = serde_json::to_vec(resolved)?;
let hash = blake3::hash(&bytes);
let hex = hash.to_hex();
let bytes64: [u8; 64] = hex.as_bytes().try_into()
    .expect("blake3 hex is always 64 chars");
Ok(Blake3Hex::from_hex_bytes(&bytes64))
```

Pitfall 6 separation pinned in the doc-comment AND structurally enforced by the function signature: `param_hash(resolved: &serde_json::Value)` takes ONLY the resolved-params value, never `RunStart.request` (which contains run_id + timestamps). Determinism falls out of Phase 1's `BTreeMap`-only discipline (no `preserve_order` feature).

Unit tests:
- `param_hash_is_byte_stable` — two calls with identical input produce identical hex
- `param_hash_differs_on_different_input` — `{lags:20}` != `{lags:21}` (sanity)
- `param_hash_btreemap_order_invariant` — two Maps with the same keys built in reverse insertion order hash identically (Pitfall 1 gate)
- `param_hash_returns_64_hex_chars` — Blake3Hex contract (64 lowercase ASCII hex chars)

**`engine/framing.rs` (363 lines, 4 unit tests):**

`build_run_start(req, run_id, started, code_revision) -> Finding` composes the request-echo Value with exactly these BTreeMap-backed keys:

| Key                 | Source                                                                                 |
| ------------------- | -------------------------------------------------------------------------------------- |
| `scan_id@version`   | `format!("{}@{}", req.scan_id, req.version)`                                           |
| `instrument`        | `req.instrument.clone()`                                                               |
| `side`              | `req.side.as_str()` (wire form "bid" / "ask")                                          |
| `timeframe`         | `req.timeframe.as_str()` (wire form "15m" / "1h" / "1d")                               |
| `window`            | object with `start`/`end` as RFC 3339 strings, `SecondsFormat::Secs`, Z suffix         |
| `gap_policy`        | `req.gap_policy.as_str()` (wire form "strict" / "continuous_only")                     |
| `resolved_params`   | `req.resolved_params.clone()`                                                          |
| `dry_run`           | `Value::Bool(req.dry_run)` — **ALWAYS present, never omitted (Blocker 2 / D3-21)**     |

Forbidden inside the request Value: `run_id`, `param_hash`, `sub_range`, any timestamp. Those live on the typed `RunStart` struct (Pitfall 6) or per-finding (Pitfall 4).

`build_run_end(run_id, started, ended, summary) -> Finding` computes `wall_clock_ms = ended.signed_duration_since(started).num_milliseconds()`. NEITHER builder reads the wall-clock; both timestamps are caller-supplied (D3-23 clock-isolation gate — the literal substring `Utc::now` does not appear anywhere in the file).

Unit tests:
- `build_run_start_carries_inputs_verbatim` — every echo field matches verbatim; forbidden fields (`run_id`, `param_hash`, `sub_range`) are absent from the Value
- `build_run_end_carries_inputs_verbatim` — `wall_clock_ms == ended - started` in ms, run_id + summary pass through
- `build_run_start_clock_isolation` — two calls with different `started` produce different `started_at_utc` AND identical request Values (byte-equal `serde_json::to_string`), proving no implicit clock read
- `build_run_start_request_carries_dry_run` (Blocker 2 closure) — `dry_run=true` -> `Value::Bool(true)`; `dry_run=false` -> `Value::Bool(false)` (NOT `None`); serialised form contains the literal `"dry_run":false`

### Task 2 — Preflight helpers (commit `22c7a65`)

**`engine/preflight.rs` (467 lines, 20 unit tests):**

Four public helpers Plan 04's `run_one` chains together at the facade boundary:

1. `resolve_scan_id_at_version(&str) -> Result<(String, u32), WireError>` — splits on FIRST `'@'`; rejects malformed or non-u32 version with `InvalidParameter` + context `scan_id_at_version`.

2. `resolve_scan(&str, &Registry) -> Result<&dyn Scan, WireError>` — chains the splitter + `Registry::get`; both unknown id AND unknown version return `UnknownScan` (D3-17 — `BTreeMap::get` returns `None` for both cases).

3. `parse_params_kv(&[String]) -> Result<Value, WireError>` — A9 typed-fallback parser:

   ```text
   parse_params_kv(["lags=20"])   -> Ok({"lags": 20})        (i64 inferred)
   parse_params_kv(["lags=3.14"]) -> Ok({"lags": 3.14})      (f64 inferred)
   parse_params_kv(["lags=true"]) -> Ok({"lags": true})      (bool inferred)
   parse_params_kv(["lags=abc"])  -> Ok({"lags": "abc"})     (string fallback)
   parse_params_kv(["bad-no-eq"]) -> Err(InvalidParameter + context.param="bad-no-eq")
   parse_params_kv(["k=v1","k=v2"]) -> Err(InvalidParameter + context.key="k")
   ```

   Splits each item on the FIRST `'='` (so values may contain `=`). The RHS is fed to `serde_json::from_str`; on parse failure, the value is wrapped as a `Value::String`. BTreeMap-backed serde_json::Map keeps iteration deterministic (OUT-03 / Pitfall 1 — no `preserve_order` feature enabled anywhere in the workspace).

4. `parse_iso_utc_window(&str) -> Result<ClosedRangeUtc, WireError>` — splits `"START:END"` into two ISO 8601 sides; accepts date-only (`YYYY-MM-DD` -> midnight UTC) and full RFC 3339 with strict `Z` suffix (A3); rejects `+HH:MM` offsets, invalid sides, empty windows (`start >= end`).

5. `classify_param_error(&serde_json::Error) -> PreflightCode` — always returns `InvalidParameter` (one-liner symmetric with `classify_figment_error` at `main.rs:112`).

Unit tests cover every documented behaviour (3 for `resolve_scan_id_at_version`, 3 for `resolve_scan`, 7 for `parse_params_kv` incl. integer/float/bool/string fallback + multi-key + malformed + duplicate, 6 for `parse_iso_utc_window` incl. date-only / full-datetime / A3 strict-Z rejection / invalid RHS / garbage / empty window, 1 for `classify_param_error`).

### Task 3 — Gap-policy dispatch (commit `ea7d801`)

**`engine/gap_policy.rs` (485 lines, 8 unit tests + 1 proptest):**

`dispatch(&GapManifest, ClosedRangeUtc, GapPolicyKind) -> GapDispatch` is the pure function Plan 04's `run_one` calls before deciding whether to emit `Finding::GapAborted` or iterate `Scan::run` over a vector of sub-ranges.

Algorithm:

1. **Empty manifest** → `SubRanges([requested])` for both policies (D3-12 zero-gap fast path).
2. **Strict + non-empty** → `Aborted(manifest.clone())` (D3-11).
3. **ContinuousOnly + gaps** → sweep gaps, emit maximal gap-free sub-ranges inside `requested`. Each gap is clamped to `[requested.start, requested.end)` before consideration (gaps entirely outside the requested range are ignored). If clamped gaps cover the whole range, return `SubRanges([])`.

Example dispatch invocation showing the partition shape:

```text
// Requested [t(0), t(6)), gaps at [t(1), t(2)) and [t(3), t(4)),
// policy = ContinuousOnly:
dispatch(&manifest, requested, ContinuousOnly)
    -> SubRanges([
        TimeRange { start: t(0), end: t(1) },
        TimeRange { start: t(2), end: t(3) },
        TimeRange { start: t(4), end: t(6) },
    ])
```

Unit tests (8 explicit + 1 proptest):
- `strict_with_gaps_aborts` — D3-11
- `strict_zero_gaps_passes_through` — D3-12 strict fast path
- `continuous_only_partitions_around_gaps` — D3-10 happy path
- `continuous_only_zero_gaps_fast_path` — D3-12 continuous_only fast path
- `continuous_only_gap_at_boundary` — gap at start of requested -> sub-range starts after the gap
- `continuous_only_gap_consumes_whole_range` -> `SubRanges([])`
- `continuous_only_multiple_gaps` -> N+1 sub-ranges for N gaps
- `continuous_only_gap_outside_requested` — gap fully after requested is ignored
- `never_silently_emits_on_hole_proptest` — proptest: under any random sorted non-overlapping gap manifest, Strict + non-empty always returns Aborted; ContinuousOnly's returned sub-ranges never overlap a gap and stay within the requested range (OUT-04 / SC-3e invariant)

**`engine/mod.rs`:** re-exports `GapDispatch + GapPolicyKind` for Plan 04 import ergonomics (`use miner_core::engine::*`). Also cleared the pre-existing clippy doc-markdown + needless-pass-by-value warnings on the still-unimplemented `run_one` scaffold that Plan 02 deferred to this plan; the `needless_pass_by_value` allow is scoped to the stub and Plan 04 removes it once `run_one` consumes `cancel`.

## Example: serialised `Finding::RunStart` with `dry_run=true`

Constructed deterministically from the test fixture `sample_request(true)` with a hardcoded `started = 2026-05-18T14:00:00Z`, `code_revision = "abc1234"`, and a placeholder `run_id`:

```json
{
  "kind": "run_start",
  "run_id": "<ulid>",
  "started_at_utc": "2026-05-18T14:00:00Z",
  "miner_version": "0.0.0",
  "code_revision": "abc1234",
  "request": {
    "scan_id@version": "stats.autocorr.ljung_box@1",
    "instrument": "EURUSD",
    "side": "bid",
    "timeframe": "15m",
    "window": {
      "start": "2026-01-01T00:00:00Z",
      "end": "2026-02-01T00:00:00Z"
    },
    "gap_policy": "continuous_only",
    "resolved_params": { "lags": 20 },
    "dry_run": true
  }
}
```

> Note: BTreeMap iteration order in the `request` object is lexicographic on keys, so the on-the-wire ordering of `dry_run`, `gap_policy`, `instrument`, etc. follows the alphabet (not the insertion order shown above). The semantic content is byte-stable: same `ScanRequest` -> same JSON bytes. The shape above is for illustration; the unit test `build_run_start_request_carries_dry_run` is the authoritative byte check.

Critical: `dry_run` is present even when `false` (the `dsr`/`fdr_q` null-but-present discipline), so downstream consumers (Plan 04 run_one branching, Plan 06 dry_run.rs + scan_ljung_box.rs integration tests, Phase 6 MCP/HTTP wrappers) can structurally rely on the field's presence in `RunStart.request`.

## Example: `parse_params_kv` typed-fallback in action

```text
parse_params_kv(&["lags=20".into(), "alpha=true".into(), "tag=foo-bar".into()])
    -> Ok(Value::Object({"alpha": Bool(true), "lags": Number(20), "tag": String("foo-bar")}))
```

Note the iteration order is lex-by-key (BTreeMap-backed `serde_json::Map`), so the wire JSON is `{"alpha":true,"lags":20,"tag":"foo-bar"}` regardless of CLI argument order.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 — Blocking issue] `let`-chain syntax not stabilised in Rust 1.85**

- **Found during:** Task 2 `cargo build` after writing the `split_window` helper.
- **Issue:** Rust 1.85.1 (the workspace's pinned toolchain via `rust-toolchain.toml`) does not yet stabilise `if let ... && let ... {}` chains (RFC 53667). The initial implementation used a 3-clause let-chain.
- **Fix:** Rewrote the helper as nested `if let { if cond { if let ... }}` blocks. Same observable behaviour; same control flow; no semantic change. The code is slightly more vertical but compiles cleanly under stable.
- **Files modified:** `crates/miner-core/src/engine/preflight.rs` (the `split_window` function).
- **Commit:** Folded into `22c7a65`.

**2. [Rule 1 — Bug] `dyn Scan` lacks `Debug`, breaking `Result<&dyn Scan, _>::expect_err`**

- **Found during:** Task 2 `cargo test` after writing the `resolve_scan_rejects_*` tests.
- **Issue:** `Result::expect_err` requires `T: Debug` for the formatter; the `Scan` trait doesn't carry the `Debug` super-bound (per the locked dyn-safety regression gate at `scan/mod.rs:294` — adding `Debug` would change the trait's vtable shape).
- **Fix:** Switched the two tests from `.expect_err()` to an explicit `match { Ok(_) => panic!(...), Err(err) => assert_eq!(err.code, ...) }`. Same coverage; works with non-Debug trait objects.
- **Files modified:** `crates/miner-core/src/engine/preflight.rs` (test bodies).
- **Commit:** Folded into `22c7a65`.

**3. [Rule 1 — Bug] proptest generator panics on chrono hour 24**

- **Found during:** Task 3 first proptest run.
- **Issue:** `Utc.with_ymd_and_hms(2024, 1, 1, 24, 0, 0)` panics — chrono rejects literal hour-24 ("No such local time"). The proptest generator allowed `req_start_h + req_end_offset` to reach 24, which is the natural way to express a full-day requested range. The proptest auto-saved the regression seed (`req_start_h=4, req_end_offset=20`).
- **Fix:** Rewrote the `t(h: u32) -> DateTime<Utc>` test helper to wrap to the next day for `h >= 24` (`2024-01-02 (h-24):00:00`). The proptest now sweeps the full `[0, 24]` hour range without panicking. Committed the proptest-regressions seed file (`crates/miner-core/proptest-regressions/engine/gap_policy.txt`) per the proptest convention — future runs replay this seed first as a fast regression check.
- **Files modified:** `crates/miner-core/src/engine/gap_policy.rs` (test helper), `crates/miner-core/proptest-regressions/engine/gap_policy.txt` (new).
- **Commit:** Folded into `ea7d801`.

**4. [Rule 3 — Blocking issue / SCOPE BOUNDARY] Pre-existing clippy warnings on `engine/mod.rs` (run_one scaffold, deferred by Plan 02)**

- **Found during:** Task 1 clippy run.
- **Issue:** Plan 02's SUMMARY noted these explicitly: `engine/mod.rs`'s `run_one` scaffold had pre-existing `doc_markdown` + `needless_pass_by_value` warnings that fall outside Plan 02's scope; Plan 02 left them for Plans 03-03..06 to clear as they fill bodies. Task 1's clippy invocation tripped these on `mod.rs` (a file Task 3 touches anyway for the re-export).
- **Fix:** Wrapped `RunOutcome / GapAborted / DryRun / Result / ScanError` in backticks within the doc-comments; added a scoped `#[allow(clippy::needless_pass_by_value)]` to the module's existing `#![allow(...)]` line, with a comment noting Plan 04 will remove it when `run_one` consumes the `cancel` arg.
- **Files modified:** `crates/miner-core/src/engine/mod.rs` (doc-comments + `#![allow]` attribute).
- **Commit:** Folded into `93c2aec`.

**5. [Rule 3 — Blocking issue] `Utc::now` / `preserve_order` literal-substring grep gates**

- **Found during:** Acceptance criteria audit after Task 1 and Task 2.
- **Issue:** The plan's acceptance criteria specify `grep -c 'Utc::now' framing.rs returns 0` and `grep -F 'preserve_order' preflight.rs returns no matches`. These grep commands catch ANY literal occurrence — including legitimate doc-comments that explain the invariants ("we do NOT call Utc::now() here"). My initial doc-comments matched the literals.
- **Fix:** Rewrote the doc-comments to express the same semantic invariants using synonyms ("wall-clock", "insertion-order preservation feature"). The rules are still documented; the grep gates now fire only on actual code matches.
- **Files modified:** `crates/miner-core/src/engine/framing.rs` (module doc + 2 fn docs + 1 test comment), `crates/miner-core/src/engine/preflight.rs` (1 fn doc + 1 const comment).
- **Commits:** Folded into `93c2aec` (framing) and `22c7a65` (preflight).

### Authentication / Manual Action Gates

None.

### Pre-existing Issues (out of scope per SCOPE BOUNDARY)

The integration-test scaffold files `tests/gap_policy.rs`, `tests/scan_facade_determinism.rs`, and `tests/shuffled_future_regression.rs` have pre-existing clippy `doc_markdown` warnings + one `redundant_closure_for_method_calls` warning in `scan/registry.rs:170` that the Plan 02 SUMMARY also noted as deferred. They fail `cargo clippy --tests -- -D warnings` but do NOT affect this plan's verification (which is `cargo test -p miner-core --lib -- engine` + library-only clippy). Plan 06 (which fills the integration-test bodies) is the natural owner. Logged in this SUMMARY rather than `deferred-items.md` because they're all in test scaffolds the next plan re-writes anyway.

## Confirmed acceptance criteria

### Task 1

| Criterion                                                                                                          | Evidence                                                                          |
| ------------------------------------------------------------------------------------------------------------------ | --------------------------------------------------------------------------------- |
| `cargo test -p miner-core --lib -- engine::param_hash::tests` passes                                               | 4 tests passed; 0 failed                                                          |
| `cargo test -p miner-core --lib -- engine::framing::tests` passes                                                  | 4 tests passed; 0 failed                                                          |
| `grep -c 'blake3::hash' crates/miner-core/src/engine/param_hash.rs` returns >= 1                                  | 3                                                                                 |
| `grep -c 'Blake3Hex::from_hex_bytes' crates/miner-core/src/engine/param_hash.rs` returns >= 1                     | 3                                                                                 |
| `grep -c 'Utc::now' crates/miner-core/src/engine/framing.rs` returns 0                                            | 0 (clock-isolation gate held — D3-23)                                              |
| `grep -c '"dry_run"' crates/miner-core/src/engine/framing.rs` returns >= 1                                        | 3 (Blocker 2 — D3-21 echo)                                                         |
| `grep -F 'skip_serializing_if' crates/miner-core/src/engine/framing.rs \| grep -c 'dry_run'` returns 0            | 0 (Blocker 2 — null-but-present rule)                                              |
| `cargo clippy -p miner-core --lib -- -D warnings`                                                                  | clean                                                                              |

### Task 2

| Criterion                                                                                                                       | Evidence                            |
| ------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------- |
| `cargo test -p miner-core --lib -- engine::preflight::tests` passes (>= 14 tests)                                              | 20 tests passed; 0 failed           |
| `grep -c 'PreflightCode::UnknownScan' crates/miner-core/src/engine/preflight.rs` returns >= 1                                  | 1                                   |
| `grep -c 'PreflightCode::InvalidParameter' crates/miner-core/src/engine/preflight.rs` returns >= 4                             | 14                                  |
| `grep -F 'preserve_order' crates/miner-core/src/engine/preflight.rs` returns no matches                                        | 0 (Pitfall 1 gate)                  |
| `cargo clippy -p miner-core --lib -- -D warnings`                                                                               | clean                                |

### Task 3

| Criterion                                                                                                              | Evidence                            |
| ---------------------------------------------------------------------------------------------------------------------- | ----------------------------------- |
| `cargo test -p miner-core --lib -- engine::gap_policy::tests` passes (>= 8 tests)                                     | 9 tests passed (8 explicit + 1 proptest); 0 failed |
| `grep -c 'GapDispatch::Aborted' crates/miner-core/src/engine/gap_policy.rs` returns >= 2                              | 3                                   |
| `grep -c 'GapDispatch::SubRanges' crates/miner-core/src/engine/gap_policy.rs` returns >= 3                            | 10                                  |
| `grep -c 'pub use gap_policy' crates/miner-core/src/engine/mod.rs` returns >= 1                                       | 1                                   |
| `grep -c 'unsafe' crates/miner-core/src/engine/gap_policy.rs` returns 0                                                | 0 (workspace forbid lint)           |
| `cargo clippy -p miner-core --lib -- -D warnings`                                                                      | clean                                |

### Phase-level

| Criterion                                  | Evidence                                              |
| ------------------------------------------ | ----------------------------------------------------- |
| `cargo build --workspace` exit 0           | clean (11.23s)                                        |
| `cargo test --workspace --no-run` exit 0   | clean — all test binaries compile                     |
| `cargo test -p miner-core --lib` exit 0    | **118 passed / 0 failed / 0 ignored** (81 prior + 37 new across Tasks 1-3) |

## Full engine test output

```
running 37 tests
test engine::framing::tests::build_run_end_carries_inputs_verbatim ... ok
test engine::framing::tests::build_run_start_carries_inputs_verbatim ... ok
test engine::framing::tests::build_run_start_clock_isolation ... ok
test engine::framing::tests::build_run_start_request_carries_dry_run ... ok
test engine::param_hash::tests::param_hash_btreemap_order_invariant ... ok
test engine::param_hash::tests::param_hash_differs_on_different_input ... ok
test engine::param_hash::tests::param_hash_is_byte_stable ... ok
test engine::param_hash::tests::param_hash_returns_64_hex_chars ... ok
test engine::preflight::tests::classify_param_error_returns_invalid_parameter ... ok
test engine::preflight::tests::parse_iso_utc_window_date_only ... ok
test engine::preflight::tests::parse_iso_utc_window_full_datetime ... ok
test engine::preflight::tests::parse_iso_utc_window_rejects_empty_window ... ok
test engine::preflight::tests::parse_iso_utc_window_rejects_garbage ... ok
test engine::preflight::tests::parse_iso_utc_window_rejects_invalid_rhs ... ok
test engine::preflight::tests::parse_iso_utc_window_rejects_non_z_offset ... ok
test engine::preflight::tests::parse_params_kv_falls_back_to_string ... ok
test engine::preflight::tests::parse_params_kv_parses_bool ... ok
test engine::preflight::tests::parse_params_kv_parses_float ... ok
test engine::preflight::tests::parse_params_kv_parses_integer ... ok
test engine::preflight::tests::parse_params_kv_parses_multiple ... ok
test engine::preflight::tests::parse_params_kv_rejects_duplicate_key ... ok
test engine::preflight::tests::parse_params_kv_rejects_malformed ... ok
test engine::preflight::tests::resolve_scan_id_at_version_rejects_missing_at ... ok
test engine::preflight::tests::resolve_scan_id_at_version_rejects_non_u32_version ... ok
test engine::preflight::tests::resolve_scan_id_at_version_splits_id_and_version ... ok
test engine::preflight::tests::resolve_scan_rejects_unknown_id ... ok
test engine::preflight::tests::resolve_scan_rejects_unknown_version ... ok
test engine::preflight::tests::resolve_scan_resolves_known_scan ... ok
test engine::gap_policy::tests::continuous_only_gap_at_boundary ... ok
test engine::gap_policy::tests::continuous_only_gap_consumes_whole_range ... ok
test engine::gap_policy::tests::continuous_only_gap_outside_requested ... ok
test engine::gap_policy::tests::continuous_only_multiple_gaps ... ok
test engine::gap_policy::tests::continuous_only_partitions_around_gaps ... ok
test engine::gap_policy::tests::continuous_only_zero_gaps_fast_path ... ok
test engine::gap_policy::tests::strict_with_gaps_aborts ... ok
test engine::gap_policy::tests::strict_zero_gaps_passes_through ... ok
test engine::gap_policy::tests::never_silently_emits_on_hole_proptest ... ok

test result: ok. 37 passed; 0 failed; 0 ignored; 0 measured; 81 filtered out; finished in 0.01s
```

## Commits

| Task | Hash      | Subject                                                                                  |
| ---- | --------- | ---------------------------------------------------------------------------------------- |
| 1    | `93c2aec` | `feat(03-03): param_hash + framing builders with dry_run echo locked`                    |
| 2    | `22c7a65` | `feat(03-03): preflight helpers — scan resolver + KEY=VAL parser + ISO 8601 window parser` |
| 3    | `ea7d801` | `feat(03-03): gap-policy dispatch — strict aborts + continuous_only partitions`          |

## Known Stubs

The only remaining `unimplemented!()` body in the engine module is `engine::run_one` itself (in `engine/mod.rs`). That is Plan 04's responsibility — this plan's contract is the FIVE sub-modules `run_one` will call, all of which now have real bodies + unit-test coverage. Plan 04 will:

1. Fill `run_one` by chaining `preflight::resolve_scan`, `preflight::parse_params_kv`, `param_hash::param_hash`, `framing::build_run_start` / `build_run_end`, `gap_policy::dispatch`.
2. Consume the `cancel: Arc<AtomicBool>` argument inside `ScanCtx` (removing the scoped `clippy::needless_pass_by_value` allow this plan added).

## Threat Model Disposition

- **T-03-03-01 (Tampering — preflight params parser)** — Mitigated. `parse_params_kv` rejects malformed input AND duplicate keys with `InvalidParameter` + context. Tests `parse_params_kv_rejects_malformed` and `parse_params_kv_rejects_duplicate_key` pin both.
- **T-03-03-02 (Tampering — param_hash determinism)** — Mitigated. `param_hash_is_byte_stable` + `param_hash_btreemap_order_invariant` tests pin the contract. The `preserve_order` literal does not appear in `preflight.rs` (Pitfall 1 grep gate).
- **T-03-03-03 (Information Disclosure — param_hash includes run_id)** — Mitigated. Function signature `param_hash(resolved: &serde_json::Value)` takes ONLY the resolved-params value; the Pitfall 6 doc-comment is at the top of `param_hash.rs`. No production caller passes `RunStart.request` to this function.
- **T-03-03-04 (Denial of Service — parse_iso_utc_window infinite loop)** — Mitigated. Function is bounded: single split, two parse calls, one comparison. No loops.
- **T-03-03-05 (Repudiation — gap_policy silently emits over a hole)** — Mitigated. `never_silently_emits_on_hole_proptest` (256 default proptest cases) pins the OUT-04 / SC-3e invariant: Strict + non-empty always returns Aborted; ContinuousOnly's sub-ranges never overlap a clamped gap and always sit within the requested range.
- **T-03-03-06 (Repudiation — RunStart.request omits dry_run)** — Mitigated. `build_run_start_request_carries_dry_run` asserts the field is present for both `true` AND `false`; the `grep -c '"dry_run"' framing.rs >= 1` acceptance gate catches accidental omission; the `skip_serializing_if + dry_run` grep gate forbids absent-when-false drift.

## Self-Check: PASSED

- [x] `cargo build --workspace` exit 0 (11.23s)
- [x] `cargo test --workspace --no-run` exit 0
- [x] `cargo test -p miner-core --lib` passes 118/118 (81 prior + 37 new across Tasks 1-3)
- [x] `cargo clippy -p miner-core --lib -- -D warnings` clean
- [x] All five engine sub-modules have real bodies (param_hash, framing, preflight, gap_policy; engine/mod.rs re-exports)
- [x] Five files in `files_modified` exist at declared paths with declared line counts (`wc -l` confirmed)
- [x] All three commit hashes (`93c2aec`, `22c7a65`, `ea7d801`) exist in `git log` on this worktree branch
- [x] Task 1 grep gates: `blake3::hash >= 1` (=3), `Blake3Hex::from_hex_bytes >= 1` (=3), `Utc::now == 0`, `"dry_run" >= 1` (=3), `skip_serializing_if/dry_run == 0`
- [x] Task 2 grep gates: `UnknownScan >= 1` (=1), `InvalidParameter >= 4` (=14), `preserve_order == 0`
- [x] Task 3 grep gates: `GapDispatch::Aborted >= 2` (=3), `GapDispatch::SubRanges >= 3` (=10), `pub use gap_policy >= 1` (=1), `unsafe == 0`
- [x] Blocker 2 (D3-21 echo) structurally closed: `req.dry_run` is echoed into `RunStart.request` as `Value::Bool`, ALWAYS present (true or false), `serde(skip_serializing_if)` is forbidden — unit test + grep gate both verify
