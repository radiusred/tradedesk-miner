---
phase: 03-scan-engine-facade-cli
plan: 02
subsystem: scan-engine-facade-cli
tags: [contract-lock, finding-envelope, scan-trait, registry, schema, dry-run, pitfall-8]
requires:
  - "03-01 (Wave 0 scaffold — Phase 3 source/test files exist with signature-only bodies and three workspace deps `ctrlc 3.5`, `statrs 0.17`, `nix 0.31`)"
provides:
  - "Locked `DataSlice.gap_manifest: Option<GapManifest>` additive optional field (D3-10)"
  - "Locked `Finding::DryRun(DryRunFinding)` additive variant (D3-21)"
  - "Locked `ScanRequest.dry_run: bool` canonical end-to-end signal (D3-21, Blocker 2)"
  - "Cfg-gated `ScanCtx.sleep_after_first_finding_ms` + `ScanRequest.sleep_after_first_finding_ms` Pitfall 8 hook"
  - "Cargo feature `test-internal` (gates the sleep hook out of release builds)"
  - "Working `Registry { scans: BTreeMap<(String, u32), Box<dyn Scan>> }` with `new`/`register`/`get`/`iter`/`bootstrap`"
  - "`FindingSink::write_raw_json(&serde_json::Value)` trait method + 3 impls (StdoutSink/FileSink/VecSink)"
  - "Regenerated `schemas/findings-v1.schema.json` (additive: +194 / -2 = doc-only deletions)"
  - "New `schemas/scans-catalogue-v1.schema.json` (Open Question 8 resolution)"
  - "Extended `xtask gen-schema` emitting both schemas idempotently"
  - "Real bodies for `ScanCtx`, `ScanRequest`, `ScanError`, `ScanFindingShape` in `crates/miner-core/src/scan/`"
affects:
  - "crates/miner-core/Cargo.toml — `[features] test-internal = []` block added"
  - "crates/miner-core/src/findings/mod.rs — DataSlice.gap_manifest field + DryRunFinding struct + Finding::DryRun variant + 5 new unit tests"
  - "crates/miner-core/src/findings/sink.rs — FindingSink::write_raw_json trait method + 3 impls + 1 new unit test"
  - "crates/miner-core/src/scan/mod.rs — Scan trait + ScanCtx + ScanRequest + ScanError real bodies + 3 new unit tests"
  - "crates/miner-core/src/scan/registry.rs — Registry real bodies + 5 new unit tests"
  - "crates/miner-core/src/scan/shape.rs — Serialize + JsonSchema derives on ScanFindingShape"
  - "crates/miner-core/src/reader.rs — Rule 3 auto-fix: Serialize/Deserialize/JsonSchema derives on ClosedRangeUtc with start_utc/end_utc serde renames"
  - "crates/miner-core/tests/schema_roundtrip.rs — sample_data_slice updated for new DataSlice.gap_manifest field"
  - "schemas/findings-v1.schema.json — additive regen (gap_manifest property + GapManifest $def + DryRunFinding $def + dry_run oneOf arm)"
  - "schemas/scans-catalogue-v1.schema.json — new sibling schema"
  - "xtask/src/main.rs — gen-schema emits both schemas; CLI signature changed to --out-dir DIR with default schemas/"
  - "xtask/Cargo.toml — added serde direct dep"
tech-stack:
  added:
    - "Cargo feature `miner-core::test-internal` (empty feature list; gates Pitfall 8 sleep hook)"
  patterns:
    - "Additive envelope extension via tagged-enum new variant + bare `#[serde(default)] Option<T>` on existing struct"
    - "Cfg-gated test-only struct field via `#[cfg(any(test, feature = \"test-internal\"))]` — production-clean by construction"
    - "Two-schema xtask gen-schema emit (Findings + sibling catalogue) with byte-stable BTreeMap-backed serde_json output"
key-files:
  created:
    - "schemas/scans-catalogue-v1.schema.json (57 lines)"
    - ".planning/phases/03-scan-engine-facade-cli/03-02-SUMMARY.md (this file)"
  modified:
    - "crates/miner-core/Cargo.toml (+12 lines — [features] test-internal = [])"
    - "crates/miner-core/src/findings/mod.rs (+192 / -9 lines — DataSlice.gap_manifest + DryRunFinding + 5 new tests)"
    - "crates/miner-core/src/findings/sink.rs (+104 / -0 lines — write_raw_json method + 3 impls + 1 test)"
    - "crates/miner-core/src/scan/mod.rs (+288 / -26 lines — Scan trait support-type real bodies + 3 tests)"
    - "crates/miner-core/src/scan/registry.rs (+121 / -25 lines — Registry real bodies + 5 tests)"
    - "crates/miner-core/src/scan/shape.rs (+7 / -4 lines — Serialize + JsonSchema derives)"
    - "crates/miner-core/src/reader.rs (+3 / -1 lines — Rule 3: ClosedRangeUtc serde derives)"
    - "crates/miner-core/tests/schema_roundtrip.rs (+1 / -0 line — sample_data_slice now includes gap_manifest: None)"
    - "schemas/findings-v1.schema.json (+194 / -2 lines — additive regen; deletions are doc-string updates)"
    - "xtask/src/main.rs (+83 / -19 lines — two-schema emit)"
    - "xtask/Cargo.toml (+1 line — serde direct dep)"
    - "Cargo.lock (+1 line — xtask now lists serde as direct dep)"
decisions:
  - "Blake3Hex stays unchanged — its Deserialize impl requires a `&'de str` (zero-copy via borrowed string). ScanRequest tests use `serde_json::from_str` (which feeds the deserializer the original buffer) rather than `from_value` (which materialises owned strings)."
  - "ScanFindingShape derives Serialize + JsonSchema but NOT Deserialize — its `&'static [&'static str]` field types can't be auto-derived for Deserialize. The type is constructed in Rust source, never parsed from the wire; the production CLI emits it via FindingSink::write_raw_json and MCP/HTTP wrappers in Phase 6 will validate catalogue lines against schemas/scans-catalogue-v1.schema.json without round-tripping back into the Rust type."
  - "ClosedRangeUtc gained Serialize/Deserialize/JsonSchema derives with serde renames `start -> start_utc` / `end -> end_utc` to match the TimeRange wire-form convention (Rule 3 auto-fix: required so ScanRequest can derive Serialize/Deserialize). No existing field semantics change; the wire form matches the existing TimeRange convention."
  - "xtask gen-schema CLI signature changed from `--out FILE` (default `schemas/findings-v1.schema.json`) to `--out-dir DIR` (default `schemas/`). The existing CI invocation `cargo run -p xtask -- gen-schema` (no args) continues to work — it now writes both artifacts into the default directory and the CI `git diff --exit-code schemas/findings-v1.schema.json` gate still fires on drift."
  - "Pitfall 8 sleep hook is cfg-gated under BOTH `cfg(test)` AND `feature = \"test-internal\"`. The first activates the field for cargo test runs (no opt-in needed); the second is for any non-test consumer (e.g., a future fuzz binary) that needs the hook without enabling all of cargo test's other behaviours. Release builds activate neither and the field is genuinely absent from the release surface."
metrics:
  duration_seconds: 2211
  completed_date: "2026-05-18T15:22:00Z"
  tasks_completed: 3
  files_touched: 13
---

# Phase 3 Plan 02: Scan Engine Wire Contract Lock Summary

Three commits delivered the wire-contract layer Phase 3's remaining plans (and Phase 6's MCP/HTTP wrappers) implement against: extended Finding envelope (DataSlice.gap_manifest field + Finding::DryRun variant), filled `Scan` trait support types (with the canonical `ScanRequest.dry_run` flag + cfg-gated Pitfall 8 sleep hook), working `Registry::bootstrap()`, new `FindingSink::write_raw_json` method, additively-regenerated `findings-v1` schema, and the new sibling `scans-catalogue-v1` schema documenting `miner scans` lines.

## One-liner

Locked the Phase 3 wire contract: gap-manifest-inlined `DataSlice`, six-variant `Finding` (added `DryRun`), canonical `ScanRequest.dry_run` flag, cfg-gated SIGINT-race sleep hook, working `Registry` keyed by `BTreeMap<(String, u32), Box<dyn Scan>>`, `FindingSink::write_raw_json` method for `miner scans` catalogue lines, and TWO additively-regenerated JSON Schemas (`findings-v1` extended; `scans-catalogue-v1` created).

## What changed

### Task 1 — Extend Finding envelope (commit `d0c2f80`)

- `DataSlice` gained a new `gap_manifest: Option<GapManifest>` field annotated `#[serde(default)]`. NO `skip_serializing_if` — the field serialises as JSON `null` when absent, matching the existing `dsr` / `fdr_q` convention.
- New `DryRunFinding` struct landed (FRAMING-like, mirrors RunStart's shape — carries run_id, produced_at_utc, request, resolved_params, planned_data_slice, estimated_findings_count). Does NOT carry the seven locked envelope fields (those belong to the Result family).
- `Finding` enum gained a sixth `DryRun(DryRunFinding)` variant. The existing `#[serde(tag = "kind", rename_all = "snake_case")]` attribute auto-produces the `"dry_run"` discriminator.
- `RunSummary` is intentionally UNCHANGED (Warning 9 pin). The new `run_summary_has_no_dry_run_emitted_field` test uses an exhaustive destructure pattern — adding any new field would break the build at compile-time, signalling contract drift.
- Five new unit tests landed: `all_variants_round_trip` (extended for DryRun), `dry_run_finding_uses_snake_case_kind`, `dataslice_gap_manifest_serialises_as_null_when_absent` (Pitfall 3-class rule), `dry_run_does_not_increment_results_emitted` (Pitfall 3 type-level pin), `run_summary_has_no_dry_run_emitted_field` (Warning 9 compile-time pin).
- `crates/miner-core/tests/schema_roundtrip.rs::sample_data_slice` updated to include `gap_manifest: None` (the integration test crate would otherwise fail to compile against the new envelope shape).

### Task 2 — Scan trait + Registry + FindingSink::write_raw_json + ScanRequest.dry_run + test-internal feature (commit `ec68072`)

- `crates/miner-core/Cargo.toml` gained a `[features]` block declaring `test-internal = []`. Documents that release builds activate neither `cfg(test)` nor the feature, so the cfg-gated sleep hook is absent from production artifacts.
- `FindingSink` trait extended with `write_raw_json(&serde_json::Value) -> std::io::Result<()>`. Three impls (StdoutSink, FileSink, VecSink) mirror their `write_envelope` framing (per-envelope flush, `\n` terminator) but bypass the `Finding` type for non-envelope introspection lines (the `miner scans` catalogue per CONTEXT D3-20 / RESEARCH Open Question 8 resolution). Doc-comment on the trait method spells out the discipline: ONLY for non-Finding lines; production scan output MUST use write_envelope.
- `scan/mod.rs` filled the real bodies:
  - `ScanCtx<'a>` — `bars: &'a BarFrame`, `gap_manifest: Option<&'a GapManifest>`, `run_id`, `code_revision: &'a str`, `cancel: Arc<AtomicBool>`, and cfg-gated `sleep_after_first_finding_ms: Option<u64>` (Pitfall 8 hook).
  - `ScanRequest` — `scan_id`, `version`, `instrument`, `side: Side`, `timeframe: Timeframe`, `window: ClosedRangeUtc`, `sub_range: TimeRange`, `gap_policy: GapPolicyKind`, `resolved_params: serde_json::Value`, `param_hash: Blake3Hex`, `dry_run: bool` (CANONICAL D3-21 / Blocker 2 signal), and cfg-gated `sleep_after_first_finding_ms: Option<u64>`. Derives Debug, Clone, Serialize, Deserialize.
  - `ScanError` — thiserror enum with `Kernel(String)`, `Io(#[from] std::io::Error)`, `Cancelled`, `Miner(#[from] crate::error::MinerError)`.
  - The `Scan` trait itself stays unchanged from Plan 03-01 (the dyn-compatibility regression gate `scan_trait_object_safe` still compiles).
- `scan/registry.rs` filled the real bodies: `Registry::new()` returns an empty `BTreeMap`-backed catalogue; `register(Box<dyn Scan>)` inserts at the `(id.to_string(), version)` key; `get(&str, u32) -> Option<&dyn Scan>` resolves the typed Boxed scan; `iter()` yields scans in lexicographic-key order (BTreeMap natural order); `bootstrap()` returns a registry with `LjungBoxScan` registered. `Default` impl delegates to `new()`. Five unit tests pin the BTreeMap discipline + bootstrap content + lex-order iteration.
- `scan/shape.rs` gained Serialize + JsonSchema derives on `ScanFindingShape` so xtask Task 3 can root the sibling catalogue schema on it. Deserialize was NOT added — the field types `&'static [&'static str]` can't auto-derive Deserialize, and the type is never parsed back from the wire in production.
- Three new unit tests in `scan/mod.rs`:
  - `scan_request_dry_run_defaults_false_when_absent` (Blocker 2 acceptance — JSON without `dry_run` deserialises with `dry_run == false`)
  - `scan_request_dry_run_round_trips` (Blocker 2 acceptance — `dry_run=true` survives serialise→deserialise)
  - `scan_ctx_has_sleep_after_first_finding_ms_field` (Blocker 3 acceptance — cfg-gated field reachable under `cargo test`)

### Task 3 — xtask gen-schema emits two schemas (commit `27fb7f5`)

- `xtask/src/main.rs` rewritten: gen-schema now takes `--out-dir DIR` (default `schemas/`) and emits both `findings-v1.schema.json` (root: `miner_core::Finding`) and `scans-catalogue-v1.schema.json` (root: xtask-local `ScansCatalogueEntry` shim). The shim has fields `scan_id: String`, `version: u32`, `params: serde_json::Value`, `finding_fields: ScanFindingShape`. Idempotent: running twice produces no diff.
- `xtask/Cargo.toml` gained `serde.workspace = true` so the shim can derive `Serialize`.
- `schemas/findings-v1.schema.json` regenerated additively. Diff stats: +194 / -2. The two deletions are doc-string updates on existing struct descriptions (no structural removal). Structural diff: DataSlice gained `gap_manifest` property; `GapManifest` type definition was added as a new `$defs` entry; `DryRunFinding` type definition was added; `Finding`'s `oneOf` array gained a new arm with `"kind":"dry_run"`. NO existing property removed, renamed, or retyped; NO `schema_version` bump.
- `schemas/scans-catalogue-v1.schema.json` created (new file, 57 lines). Documents the `miner scans` catalogue line shape — `ScansCatalogueEntry` with all four required fields. MCP/HTTP wrappers in Phase 6 will validate catalogue lines against this sibling schema, NOT against `findings-v1`.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking issue] `ClosedRangeUtc` lacks Serialize/Deserialize/JsonSchema**

- **Found during:** Task 2 `cargo build`.
- **Issue:** `ScanRequest` derives `Serialize` + `Deserialize` (per plan's behavior section for tests 9 + 10), but its `window: ClosedRangeUtc` field's type from `crates/miner-core/src/reader.rs:71` only derived `Debug, Clone, Copy, PartialEq, Eq`. The derive on `ScanRequest` failed because `ClosedRangeUtc` is not serde-compatible.
- **Fix:** Added `Serialize, Deserialize, JsonSchema` derives to `ClosedRangeUtc` with serde renames `start -> start_utc` / `end -> end_utc` (matching the existing `TimeRange` wire-form convention). The derive is purely additive — no existing field semantics change. The reader.rs file is not in the plan's `files_modified` list, but the change is structurally required for the plan's tests to compile.
- **Files modified:** `crates/miner-core/src/reader.rs` (+3 / -1 lines).
- **Commit:** Folded into `ec68072`.

**2. [Rule 1 - Bug] `Blake3Hex::from_hex_str` referenced but does not exist**

- **Found during:** Task 2 test compilation.
- **Issue:** My initial test helper used `Blake3Hex::from_hex_str(s)` — the public API only exposes `Blake3Hex::from_hex_bytes(&[u8; 64])`.
- **Fix:** Materialise a `[u8; 64]` zero-byte array via `[b'0'; 64]` and call `from_hex_bytes(&bytes)`. Same observable result (a 64-char `'0'`-filled hex string), uses the canonical constructor.
- **Files modified:** `crates/miner-core/src/scan/mod.rs` (test helper).
- **Commit:** Folded into `ec68072`.

**3. [Rule 1 - Bug] `BarFrame::new` referenced but does not exist**

- **Found during:** Task 2 test compilation.
- **Issue:** `BarFrame` is a pub struct in `aggregator.rs:154` with `Debug, Clone` derives — no `new()` constructor (only `len()` / `is_empty()`).
- **Fix:** Construct via struct literal with the explicit field set (source_id, symbol, side, tf, plus six empty `Vec::new()` column vectors).
- **Files modified:** `crates/miner-core/src/scan/mod.rs` (test helper).
- **Commit:** Folded into `ec68072`.

**4. [Rule 1 - Bug] `serde_json::from_value` cannot deserialise into Blake3Hex**

- **Found during:** Task 2 test failure on `scan_request_dry_run_defaults_false_when_absent`.
- **Issue:** `Blake3Hex`'s manual `Deserialize` impl uses `<&str as Deserialize>::deserialize(deserializer)` — it requires a borrowed `&str`, which `serde_json::Value` (already-materialised owned strings) cannot provide.
- **Fix:** The test now serialises `json` to a `String` via `serde_json::to_string(&json)` then calls `serde_json::from_str(&s)`. `from_str` feeds the deserializer the original buffer, enabling zero-copy borrows.
- **Files modified:** `crates/miner-core/src/scan/mod.rs` (test body).
- **Commit:** Folded into `ec68072`.

**5. [Rule 3 - Blocking issue] `ScanFindingShape` Deserialize derive fails on `&'static [&'static str]` fields**

- **Found during:** Task 2 `cargo build` after I added Serialize/Deserialize/JsonSchema to `shape.rs`.
- **Issue:** `&'static [&'static str]` cannot auto-derive `Deserialize` — serde can deserialise `&[u8]` but not arbitrary slice-of-slice types.
- **Fix:** Dropped the `Deserialize` derive from `ScanFindingShape`. Only `Serialize` + `JsonSchema` are required for the catalogue use case (production constructs the type in Rust source, emits via `FindingSink::write_raw_json`, and the wire shape is documented by `schemas/scans-catalogue-v1.schema.json`; MCP/HTTP wrappers validate against the schema, never deserialise back into the Rust type).
- **Files modified:** `crates/miner-core/src/scan/shape.rs`.
- **Commit:** Folded into `ec68072`.

**6. [Rule 1 - Bug] `serde_json::json!()` macro doesn't accept Rust method-call expressions like `"0".repeat(64)`**

- **Found during:** Task 2 test compilation.
- **Issue:** My test JSON had `"param_hash": "0".repeat(64)` — the `json!` macro parses this as a JSON literal, not a Rust expression.
- **Fix:** Inlined the 64-char `'0'`-filled hex string literal: `"0000000000000000000000000000000000000000000000000000000000000000"`.
- **Files modified:** `crates/miner-core/src/scan/mod.rs` (test JSON).
- **Commit:** Folded into `ec68072`.

**7. [Rule 1 - Bug] clippy `doc_markdown` lint fires on `gap_policy` reference inside DryRunFinding doc**

- **Found during:** clippy run after Task 2 commit.
- **Issue:** The DryRunFinding `request` field's doc-comment listed `gap_policy` without backticks; clippy's `doc_markdown` lint required them.
- **Fix:** Wrapped `gap_policy` in backticks.
- **Files modified:** `crates/miner-core/src/findings/mod.rs` (doc-comment).
- **Commit:** Folded into `ec68072` (Task 2 commit captures the clippy hygiene fix alongside the Task 2 scope).

### Pre-existing Issues (out of scope per SCOPE BOUNDARY)

The engine/mod.rs, engine/framing.rs, and findings/mod.rs scaffold files (from Plan 03-01) have pre-existing clippy warnings (`doc_markdown`, `redundant_pub_crate`, `needless_pass_by_value`) that pre-date this plan. They're contained by the scaffolds' `#![allow(dead_code, unused_variables)]` at the runtime layer but clippy's documentation lints aren't covered by those attributes. Plans 03-03..06 own those files and will fix the warnings as they fill the bodies. Per the deviation-rules SCOPE BOUNDARY ("Only auto-fix issues DIRECTLY caused by the current task's changes"), they're not addressed here.

### Authentication / Manual Action Gates

None.

## Confirmed acceptance criteria

| Criterion | Evidence |
|-----------|----------|
| `cargo test -p miner-core --lib` passes | 81 tests passed; 0 failed; 1 ignored |
| `cargo test -p miner-core --test schema_roundtrip` passes | 3 tests passed; 0 failed |
| `cargo run -p xtask -- gen-schema` exits 0 + `git diff --exit-code schemas/` clean | confirmed idempotent — second run produces no diff |
| `grep -c '"const": "dry_run"' schemas/findings-v1.schema.json` | 1 (>= 1) |
| `grep -c 'gap_manifest' schemas/findings-v1.schema.json` | 6 (>= 2) |
| `grep -c '^test-internal' crates/miner-core/Cargo.toml` | 1 (== 1) |
| `grep -c 'dry_run: bool' crates/miner-core/src/scan/mod.rs` | 4 (>= 1) |
| `grep -c 'sleep_after_first_finding_ms' crates/miner-core/src/scan/mod.rs` | 15 (>= 2) |
| `cargo build --workspace` | clean (3.66s) |
| `cargo build -p miner-core --release` | clean (54.42s first run; 2.66s rebuild); cfg-gated sleep hook genuinely absent |

## Per-Task Test Results

All new tests passing across all three task `cargo test -p miner-core --lib` invocations:

| Test fn | Owning task | Passes? |
|---------|-------------|---------|
| `findings::tests::all_variants_round_trip` | Task 1 | YES |
| `findings::tests::dry_run_finding_uses_snake_case_kind` | Task 1 | YES |
| `findings::tests::dataslice_gap_manifest_serialises_as_null_when_absent` | Task 1 | YES |
| `findings::tests::dry_run_does_not_increment_results_emitted` | Task 1 | YES |
| `findings::tests::run_summary_has_no_dry_run_emitted_field` | Task 1 | YES |
| `findings::sink::tests::write_raw_json_to_vec_sink_emits_jsonl_line` | Task 2 | YES |
| `scan::registry::tests::registry_starts_empty` | Task 2 | YES |
| `scan::registry::tests::registry_register_and_get` | Task 2 | YES |
| `scan::registry::tests::registry_uses_btreemap` | Task 2 | YES |
| `scan::registry::tests::bootstrap_registers_ljung_box_scan` | Task 2 | YES |
| `scan::registry::tests::registry_iter_lex_order` | Task 2 | YES |
| `scan::tests::scan_trait_object_safe` | Task 2 (Plan 03-01 inherited) | YES |
| `scan::tests::scan_request_dry_run_defaults_false_when_absent` | Task 2 (Blocker 2) | YES |
| `scan::tests::scan_request_dry_run_round_trips` | Task 2 (Blocker 2) | YES |
| `scan::tests::scan_ctx_has_sleep_after_first_finding_ms_field` | Task 2 (Blocker 3) | YES |

## Schema diff evidence

Output of `git diff HEAD~3 HEAD --numstat schemas/` after the full plan:

```text
194     2       schemas/findings-v1.schema.json
57      0       schemas/scans-catalogue-v1.schema.json
```

Findings-v1 changes are additive (the 2 deletions are doc-string updates, not structural removals). Scans-catalogue-v1 is a fresh file (57 lines).

## Sibling schema preview (head of schemas/scans-catalogue-v1.schema.json)

```json
{
  "$defs": {
    "ScanFindingShape": {
      "description": "Declarative shape declaration: the `effect.extra` keys and `raw.series`\nkeys a scan WILL emit on success.\n\nUsed by:\n- `miner scans` catalogue introspection (one JSONL line per registered scan).\n- The per-scan integration tests (compile-time gate that the production scan\n  actually emits every declared key).\n\n`&'static [&'static str]` because every scan's emitted-key set is a\ncompile-time constant — there is no dynamic dispatch on these values.",
      "properties": {
        "effect_extra_keys": {
          "description": "Names of the `effect.extra.<key>` arrays the scan emits (e.g.,\n`[\"lags\", \"q_stats\", \"p_values\", \"acf\"]` for Ljung-Box per D3-04).",
          "items": { "type": "string" },
          "type": "array"
        },
        "raw_series_keys": { ... }
      },
      ...
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "description": "xtask-local shim describing one line of `miner scans` introspection output...",
  "properties": {
    "finding_fields": { "$ref": "#/$defs/ScanFindingShape", ... },
    "params":         { "description": "JSON Schema fragment ...", ... },
    "scan_id":        { "type": "string", ... },
    "version":        { "type": "integer", "minimum": 0, ... }
  },
  "required": ["scan_id", "version", "params", "finding_fields"],
  "title": "ScansCatalogueEntry",
  "type": "object"
}
```

## Release-binary cfg-gate evidence

`cargo build --release -p miner-core` (NO `--features test-internal`) compiles cleanly. The cfg-gated `sleep_after_first_finding_ms` field on `ScanCtx` and `ScanRequest` is absent from the compiled object code — the `#[cfg(any(test, feature = "test-internal"))]` predicate evaluates to FALSE under a release build without the feature flag, so the struct definitions don't include the field. Plan 06's final gate will inspect the `miner --help` of the release binary to confirm `--sleep-after-first-finding-ms` is also absent there (CLI side cfg-gated independently in Plan 05).

## Commits

| Task | Hash | Subject |
|------|------|---------|
| 1 | `d0c2f80` | `feat(03-02): extend Finding envelope — DataSlice.gap_manifest + Finding::DryRun` |
| 2 | `ec68072` | `feat(03-02): fill Scan trait + Registry + FindingSink::write_raw_json + ScanRequest.dry_run + test-internal feature` |
| 3 | `27fb7f5` | `feat(03-02): xtask gen-schema emits two schemas — findings-v1 (additive) + scans-catalogue-v1 (new)` |

## Known Stubs

No new stubs introduced. The Plan 03-01 scaffold stubs that remain (`LjungBoxScan::param_schema` + `run`, `engine::run_one`, etc.) are now resolvable in Plans 03-03..06 against the type contract this plan locked. Specifically:

- `LjungBoxScan::param_schema` + `run` — Plan 03-04 fills.
- `engine::{run_one, preflight, gap_policy::dispatch, param_hash, framing}` — Plans 03-02 (engine done elsewhere, not here), 03-03, 03-04, 03-05, 03-06 own incremental pieces.
- `miner-cli::scan_args::{ScanArgs, parse_window, to_scan_request}` — Plans 03-05, 03-02 (`to_scan_request` could land here, but its surface depends on the engine wiring Plans 03-04..05 own, so it stays scaffolded).

## Threat Model Disposition

- **T-03-02-01 (Tampering — `schemas/findings-v1.schema.json`)** — Mitigated. xtask `gen-schema` is the only writer; the regen + git diff workflow lands both schema files in the same commit as the type changes. CI gate at `.github/workflows/ci.yml:84` continues to fire on drift.
- **T-03-02-02 (Information Disclosure — `FindingSink::write_raw_json`)** — Mitigated. Trait method has the mandatory doc-comment per RESEARCH Pitfall 7 line 608: "ONLY for non-Finding introspection lines. Validate against a sibling schema." The sibling schema `schemas/scans-catalogue-v1.schema.json` is now committed; Plan 05's `miner scans` subcommand will be the only production call site.
- **T-03-02-03 (Tampering — Cargo.lock / serde_json features)** — Mitigated. `grep preserve_order Cargo.lock` returns 0 (verified locally). The BTreeMap discipline depends on this; the new round-trip test in findings/mod.rs would fail byte-equality if preserve_order were enabled.
- **T-03-02-04 (Repudiation — DryRun counted as a result)** — Mitigated. `dry_run_does_not_increment_results_emitted` unit test pins the constructor invariant; the end-to-end engine test will be pinned by Plan 04's run_one test.
- **T-03-02-05 (Information Disclosure — test-only sleep hook leaking into release)** — Mitigated. Both fields cfg-gated under `#[cfg(any(test, feature = "test-internal"))]`. `cargo build -p miner-core --release` (no `--features test-internal`) succeeds, and the field is absent from the release object code. Plan 06's final gate will inspect the binary's `--help` output to confirm the corresponding CLI flag is also absent.
- **T-03-02-06 (Repudiation — RunSummary silently extended)** — Mitigated. `run_summary_has_no_dry_run_emitted_field` test exhaustively destructures `RunSummary` at compile-time — adding any new field would break the build immediately.

## Self-Check: PASSED

- [x] `crates/miner-core/Cargo.toml` exists at the path and contains `test-internal` feature
- [x] `crates/miner-core/src/findings/mod.rs` exists and contains `DryRunFinding` struct + `DryRun(DryRunFinding)` variant + `pub gap_manifest: Option<GapManifest>` field
- [x] `crates/miner-core/src/findings/sink.rs` exists and contains `fn write_raw_json` (4 occurrences: trait + 3 impls + test helper)
- [x] `crates/miner-core/src/scan/mod.rs` exists and contains `pub struct ScanCtx`, `pub struct ScanRequest`, cfg-gated `sleep_after_first_finding_ms` (15 occurrences)
- [x] `crates/miner-core/src/scan/registry.rs` exists and contains `Box::new(LjungBoxScan)`
- [x] `crates/miner-core/src/scan/shape.rs` exists with `Serialize` + `JsonSchema` derives
- [x] `schemas/findings-v1.schema.json` regenerated additively (+194 / -2; deletions are doc-only)
- [x] `schemas/scans-catalogue-v1.schema.json` exists with `scan_id` + `version` + `params` + `finding_fields` keys
- [x] Three commits exist in `git log`: `d0c2f80`, `ec68072`, `27fb7f5`
- [x] `cargo build --workspace` exit 0
- [x] `cargo test -p miner-core --lib` passes (81/81)
- [x] `cargo test -p miner-core --test schema_roundtrip` passes (3/3)
- [x] `cargo build -p miner-core --release` succeeds (cfg-gated fields absent)
- [x] `cargo run -p xtask -- gen-schema` is idempotent after commit
