---
phase: 01-foundations-contracts
plan: 03
subsystem: foundations
tags: [rust, finding-envelope, schema, schemars, json-schema, base64-bytes, run-id, ulid, error-vocabulary, thiserror, wire-error, finding-sink, config-schema, btreemap]

# Dependency graph
requires: [plan-01-01, plan-01-02]
provides:
  - "Finding tagged enum (RunStart, Result, ScanError, GapAborted, RunEnd) with #[serde(tag = \"kind\", rename_all = \"snake_case\")]"
  - "Locked seven envelope fields (schema_version, scan_id@version, param_hash, code_revision, data_slice, dsr, fdr_q) INLINED into each applicable variant payload (no #[serde(flatten)] per RESEARCH §Anti-Patterns)"
  - "dsr/fdr_q reserved Phase 5 slots serialise as JSON null (not absent) in v1"
  - "Base64Bytes(Vec<u8>) newtype + Dtype enum — production port of the Plan 01-02 spike; manual JsonSchema impl emits contentEncoding: \"base64\" + contentMediaType: \"application/octet-stream\""
  - "RunId(Ulid) newtype with #[derive(Copy)] — the regression gate Plan 05's emit_fixture() depends on (moves the same RunId into both RunStart and RunEnd)"
  - "RunSummary + PerScanCounts derive Default — Plan 05's RunSummary::default() call compiles"
  - "All map-typed fields (Raw::series, Effect::extra, RunSummary::per_scan, WireError::context) are BTreeMap (NEVER HashMap) per OUT-03"
  - "Raw::new constructor enforces D-03 invariant (series must contain `timestamps_ms`)"
  - "PreflightCode enum (7 variants: invalid_parameter, unknown_scan, unknown_instrument, missing_required_config, invalid_config, sweep_too_large, internal_error) per RESEARCH §error_code Vocabulary"
  - "ScanErrorCode enum (4 variants: coverage_gap, compute_error, cache_corruption, internal_panic_caught) per RESEARCH §error_code Vocabulary"
  - "WireError struct with code: String (open for additive extensibility) + From<MinerError> bridge"
  - "MinerError thiserror enum without Serialize derive — RESEARCH §Pitfall 3 split honoured"
  - "FindingSink trait (Send-bound, write_envelope + flush) — interface only; StdoutSink impl deferred to Plan 04"
  - "MinerConfig schema (cache_root, bar_cache_root, output: OutputDest) — schema types only; figment builder deferred to Plan 05"
  - "OutputDest enum (Stdout, File(PathBuf)) with snake_case wire form"
  - "FROZEN public surface in crates/miner-core/src/lib.rs: 18 names from findings + 4 from error + 2 from config — every name Plans 05/06/07 import"
  - "All envelope and supporting types derive JsonSchema (or implement it manually for newtype-with-format types) — Plan 06's xtask CI gate will diff against the regenerated schema"
affects: [plan-01-04, plan-01-05, plan-01-06, plan-01-07, phase-02, phase-03, phase-04, phase-05, phase-06, phase-07]

# Tech tracking
tech-stack:
  added:
    - "schemars `chrono04` feature (workspace Cargo.toml) — required for JsonSchema derives on chrono::DateTime<Utc>"
  patterns:
    - "Manual JsonSchema via serde_json::json!{...}.try_into() for newtype-with-format types (Base64Bytes, RunId)"
    - "Two-layer error model: internal MinerError (thiserror, NO Serialize, From<io::Error>) + wire-form WireError (Serialize + JsonSchema, open-string code) bridged via From<MinerError>"
    - "Locked envelope fields INLINED into each variant struct, NOT via #[serde(flatten)] — the schema stays strict (no additionalProperties: true leak)"
    - "BTreeMap everywhere for deterministic JSON output ordering (OUT-03)"
    - "#[serde(transparent)] on RunId for bare-string wire form; manual JsonSchema for the regex pattern"
    - "#[serde(rename = \"scan_id@version\")] field attribute to emit the literal @-separated key the consumers expect while keeping a Rust-idiomatic identifier in source"
    - "as_str() helper on each error-code enum to construct WireError::code from a typed value at the engine boundary while keeping the wire format open-string"
    - "#[cfg(test)] in-module VecSink helper for FindingSink object-safety testing — avoids hitting io::stdout() in unit tests"

key-files:
  created:
    - "crates/miner-core/src/findings/mod.rs"
    - "crates/miner-core/src/findings/base64_bytes.rs"
    - "crates/miner-core/src/findings/run_id.rs"
    - "crates/miner-core/src/findings/sink.rs"
    - "crates/miner-core/src/error/mod.rs"
    - "crates/miner-core/src/error/codes.rs"
    - "crates/miner-core/src/error/stderr_emit.rs (placeholder — Plan 04 fills)"
    - "crates/miner-core/src/config/mod.rs"
  modified:
    - "Cargo.toml (workspace; added `chrono04` feature to schemars dep)"
    - "Cargo.lock (transitive resolution refresh)"
    - "crates/miner-core/Cargo.toml (post-spike cleanup: removed orphan clap dev-dep, updated comments; kept figment + figment-test dev-dep for Plan 05's Jail usage)"
    - "crates/miner-core/src/lib.rs (replaced the spike pub mod declarations with `pub mod {findings, error, config}` plus the FROZEN public surface)"
  deleted:
    - "crates/miner-core/src/spike_base64.rs (per Plan 03 must_haves)"
    - "crates/miner-core/src/spike_figment.rs (per Plan 03 must_haves)"
    - "crates/miner-core/tests/spike_schema.rs (per Plan 03 must_haves)"
    - "crates/miner-core/tests/spike_figment_precedence.rs (per Plan 03 must_haves)"

key-decisions:
  - "Locked envelope fields are INLINED into each variant struct rather than gathered into a `Common` struct + #[serde(flatten)]. Per RESEARCH §Anti-Patterns, flatten weakens the schema to `additionalProperties: true` even in schemars 1.x — and the inlining adds at most 7 declared fields × 3 variants = 21 lines that are otherwise identical. Worth the verbosity for a strict schema contract."
  - "RunId derives Copy and dsr/fdr_q are Option<f64> with no `skip_serializing_if`. These two derives are the regression gates for Plan 05's emit_fixture() and the v1 reserved-Phase-5-slot contract respectively. Tests 2, 4, 5 are explicit canaries; a future contributor cannot quietly remove either without a test failing."
  - "Raw::new validates D-03 (timestamps_ms must be present) at construction; Raw::new_unchecked is #[cfg(test)] only. This pushes the contract into the type system without forcing production code to handle a fallible constructor in the common path — scans build the Raw via the validated path; tests that exercise OTHER fields bypass via new_unchecked."
  - "WireError::code is `String`, not a typed enum, on the wire — additive-extensibility property. Internally callers use PreflightCode::as_str() / ScanErrorCode::as_str() to construct the string, which gives compile-time guarantees against the locked Phase 1 vocabulary while keeping the schema additive. New codes in Phase 4+ are non-breaking."
  - "MinerError DOES NOT derive Serialize. The two-layer split (MinerError internal, WireError wire) honours RESEARCH §Pitfall 3: std::io::Error #[from] cannot compose with #[derive(serde::Serialize)]. The From<MinerError> for WireError bridge does the conversion at the engine boundary — the wire shape is decoupled from the internal-error variant set."
  - "Schemas crate gained the `chrono04` feature. Without it, DateTime<Utc> in the variant payloads fails the JsonSchema derive (the macro emits `the trait bound \\`DateTime<Utc>: JsonSchema\\` is not satisfied`). This is a Rule-3 environmental fix — the dependency is already used; the feature flag is the difference. Documented in deviations below."

requirements-completed: [FOUND-03, OUT-02, OUT-03]
threats-mitigated: [T-01-02, T-01-04]

# Metrics
duration: 12min
completed: 2026-05-16
---

# Phase 01 Plan 03: Locked Finding envelope types + error vocabulary + sink trait + config schema Summary

**The Finding envelope contract is now the source of truth for Phase 1 and beyond. All five variants compile with `Serialize + Deserialize + JsonSchema`; all map-typed fields are `BTreeMap`; `RunId` is `Copy`; `RunSummary` is `Default`; `dsr`/`fdr_q` serialise as JSON `null`; the seven locked common fields are inlined into each applicable variant; spike modules from Plan 01-02 are deleted. The `lib.rs` FROZEN public surface exposes 24 names — every one Plans 05/06/07 import. 14 unit tests pass; `cargo build -p miner-core` and `cargo build --workspace` both succeed.**

## Performance

- **Duration:** ~12 min
- **Started:** 2026-05-16T09:54:14Z
- **Completed:** 2026-05-16T10:06:34Z
- **Tasks:** 2 (both auto + TDD)
- **Files:** 8 created, 4 modified, 4 deleted

## Accomplishments

### Task 1 — Findings module (8 tests pass)

- `findings/mod.rs` lands the five-variant tagged enum `Finding`, its six per-variant payload structs (`RunStart`, `ResultFinding`, `ScanErrorFinding`, `GapAbortedFinding`, `RunEnd`, plus the supporting `RunSummary` and `PerScanCounts`), and the six common types (`TimeRange`, `DataSlice`, `Source`, `RawArray`, `Raw`, `Effect`). The seven locked envelope fields are inlined into `ResultFinding`, `ScanErrorFinding`, and `GapAbortedFinding` per D-09 (framing records `RunStart`/`RunEnd` intentionally omit them). The `scan_id@version` field is exposed as `scan_id_at_version` in Rust and emitted as the literal `"scan_id@version"` key in JSON via `#[serde(rename)]`.
- `findings/base64_bytes.rs` ports the Plan 01-02 spike `SpikeBase64Bytes` verbatim into production `Base64Bytes(pub Vec<u8>)`. Derives `Debug, Clone, PartialEq, Eq` (NOT `Copy` — heap-owned). Manual `JsonSchema` impl emits `contentEncoding: "base64"` + `contentMediaType: "application/octet-stream"`. `Dtype` enum (single `F64` variant in v1) sits alongside.
- `findings/run_id.rs` lands `RunId(pub Ulid)` with `Copy` (REQUIRED — Plan 05 moves the same value into both `RunStart` and `RunEnd`). Manual `JsonSchema` impl emits the `^[0-9A-HJKMNP-TV-Z]{26}$` Crockford-base32 regex pattern. `Display` delegates to `ulid::Ulid::fmt` for the canonical 26-char wire form; `Default` calls `Self::new()` so deserialised contexts that need a placeholder get a valid ULID.
- `lib.rs` extended with `pub mod findings;` plus a partial `pub use findings::{...}` re-export list (Task 2 extends to the full FROZEN surface).
- Spike modules **deleted**: `spike_base64.rs`, `spike_figment.rs`, `tests/spike_schema.rs`, `tests/spike_figment_precedence.rs`. Their patterns are now embodied in the production types.

8 behavioural tests in `findings::tests` cover:
1. `envelope_fields_present` — `Finding::Result` JSON contains all seven locked keys at top level.
2. `dsr_and_fdr_q_are_null_in_v1` — both reserved slots serialise as JSON `null` (not absent).
3. `run_id_format` — `RunId::new().to_string()` is 26 chars in the Crockford alphabet; the `serde(transparent)` wire form is the bare string.
4. `run_id_is_copy` — compile-time `assert_copy::<RunId>()` + runtime double-move regression gate.
5. `run_summary_default_compiles_and_is_zero` — `RunSummary::default()` + `PerScanCounts::default()` produce zero counters and an empty `per_scan` map.
6. `base64_round_trip` — `Base64Bytes` serialise → deserialise preserves the inner bytes.
7. `raw_series_uses_btreemap` — type-annotated reference binding asserts `Raw::series: BTreeMap<...>`; `Raw::new(empty)` returns `Err` (D-03 invariant enforced).
8. `all_five_variants_round_trip` — each `Finding` variant survives `to_string` → `from_str`.

### Task 2 — Error vocabulary + FindingSink trait + Config schema (6 additional tests pass)

- `error/codes.rs` lands the locked vocabulary: `PreflightCode` (7 variants per RESEARCH §"error_code Vocabulary": `InvalidParameter`, `UnknownScan`, `UnknownInstrument`, `MissingRequiredConfig`, `InvalidConfig`, `SweepTooLarge`, `InternalError`) and `ScanErrorCode` (4 variants: `CoverageGap`, `ComputeError`, `CacheCorruption`, `InternalPanicCaught`). Both derive `Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema` and use `#[serde(rename_all = "snake_case")]`. Each has an `as_str()` const-style helper for constructing a `WireError::code` from a typed value. `WireError { code: String, message: String, context: BTreeMap<...> }` is the open-string wire form with `WireError::preflight()` / `WireError::scan()` typed constructors and a `with_context()` builder.
- `error/mod.rs` lands the `MinerError` thiserror enum (`Io(#[from] std::io::Error)`, `Serialize(#[from] serde_json::Error)`, `Config(#[from] figment::Error)`, `Scan(String)`, `Internal(String)`) — explicitly **without** `serde::Serialize` per RESEARCH §Pitfall 3 (`std::io::Error` doesn't compose with the derive). `From<MinerError> for WireError` provides the bridge at the engine boundary (defaults to `ScanErrorCode::InternalPanicCaught`; specific call-sites construct `WireError::scan()` / `WireError::preflight()` directly when the code is known).
- `error/stderr_emit.rs` is a placeholder module — doc-only. Plan 04 fills the body with the structured-stderr writer; this file exists so the `pub mod stderr_emit;` declaration in `error/mod.rs` resolves and Plan 04 has no file-create conflict.
- `findings/sink.rs` lands `pub trait FindingSink: Send` with `write_envelope` + `flush` methods. The `Send` bound is required because Phase 3+ rayon workers will emit findings. A `#[cfg(test)] VecSink` helper captures envelopes into an in-memory `Vec<u8>` exactly the way Plan 04's `StdoutSink` will write them — used by Test 6 to exercise trait-object safety (`Box<dyn FindingSink>` moved across a `std::thread::spawn` boundary). Plan 04 will land the `StdoutSink` impl.
- `config/mod.rs` lands `MinerConfig { cache_root: PathBuf, bar_cache_root: PathBuf, output: OutputDest }` and `OutputDest { Stdout, File(PathBuf) }`. All fields non-optional — Plan 05's `figment.extract()` must error when any source layer fails to supply a value. **No `Default` impl** — zero hardcoded paths in the library per D-16.
- `lib.rs` extended to the FROZEN public surface: every name Plans 05/06/07 import is now re-exported at crate root (18 names from `findings`, 4 from `error`, 2 from `config` = 24 total).
- `findings/mod.rs` regains `pub mod sink; pub use sink::FindingSink;` (deferred from Task 1 because of the cross-dep on `crate::error::MinerError`).

6 behavioural tests cover:
1. `error::codes::tests::preflight_code_serialises_snake_case` — all 7 variants round-trip with the locked snake_case strings.
2. `error::codes::tests::scan_error_code_serialises_snake_case` — all 4 variants round-trip.
3. `error::codes::tests::wire_error_serialises_open_string_code` — an arbitrary future-code string round-trips (open-string property).
4. `error::tests::miner_error_does_not_require_serialize` — compile-test (the function compiling IS the test) plus a runtime check of the `From<io::Error>` + `From<MinerError> for WireError` bridge.
5. `config::tests::miner_config_type_shape` — both `OutputDest` variants round-trip; the three-field shape is locked.
6. `findings::sink::tests::trait_object_safe` — `Box<dyn FindingSink>` compiles and survives `std::thread::spawn` (proves `Send` is honoured and the trait is object-safe).

## Task Commits

Each task was committed atomically on `worktree-agent-ad06d340e32f79785`:

1. **Task 1: Locked Finding envelope types + Base64Bytes + RunId** — `56cd95c` (`feat(01-03): land locked Finding envelope types + Base64Bytes + RunId`)
2. **Task 2: Error vocabulary + FindingSink trait + Config schema types** — `a33d98a` (`feat(01-03): land error vocabulary, FindingSink trait, config schema types`)

(The final metadata commit covering this SUMMARY.md is appended by the orchestrator after merge — this executor does not modify STATE.md or ROADMAP.md per the parallel-execution contract.)

## Files Created/Modified

### Created (8)

- **`crates/miner-core/src/findings/mod.rs`** — Finding tagged enum + per-variant payload structs (RunStart, ResultFinding, ScanErrorFinding, GapAbortedFinding, RunEnd, RunSummary, PerScanCounts) + common types (TimeRange, DataSlice, Source, RawArray, Raw, Effect). Includes `findings::tests` (8 unit tests).
- **`crates/miner-core/src/findings/base64_bytes.rs`** — `Base64Bytes(pub Vec<u8>)` + `Dtype { F64 }` with manual JsonSchema impl emitting contentEncoding/contentMediaType (ported from Plan 01-02 spike).
- **`crates/miner-core/src/findings/run_id.rs`** — `RunId(pub Ulid)` with Copy + manual JsonSchema impl emitting Crockford-base32 regex pattern.
- **`crates/miner-core/src/findings/sink.rs`** — `FindingSink: Send` trait (interface only, Plan 04 fills) + `#[cfg(test)] VecSink` helper + trait-object-safety test.
- **`crates/miner-core/src/error/mod.rs`** — `MinerError` thiserror enum (no Serialize), `From<MinerError> for WireError` bridge. Includes `error::tests`.
- **`crates/miner-core/src/error/codes.rs`** — Locked Phase 1 vocabulary: `PreflightCode` (7 variants) + `ScanErrorCode` (4 variants) + `WireError` (open-string code). Includes `error::codes::tests` (3 unit tests).
- **`crates/miner-core/src/error/stderr_emit.rs`** — Doc-only placeholder; Plan 04 fills.
- **`crates/miner-core/src/config/mod.rs`** — `MinerConfig` + `OutputDest` schema types (no figment builder; that's Plan 05). Includes `config::tests`.

### Modified (4)

- **`Cargo.toml`** (workspace root) — added `features = ["chrono04"]` to the `schemars` workspace dep. Required for `JsonSchema` derives on `chrono::DateTime<Utc>` fields. The workspace lints, resolver, edition pins are unchanged.
- **`Cargo.lock`** — transitive resolution refresh after the schemars feature addition.
- **`crates/miner-core/Cargo.toml`** — post-spike cleanup: removed the orphan `clap.workspace = true` dev-dep (the spike test that imported it is deleted), updated the top-of-file comment to describe Plan 03's deliverables instead of the spike's, kept the `figment = { ..., features = [..., "test"] }` dev-dep entry because Plan 05 will reuse `figment::Jail` for config-precedence tests.
- **`crates/miner-core/src/lib.rs`** — replaced the `pub mod spike_base64; pub mod spike_figment;` lines with `pub mod {findings, error, config}` and the FROZEN public-surface re-export block. The `CODE_REVISION` constant + its doc are preserved.

### Deleted (4)

- **`crates/miner-core/src/spike_base64.rs`** (per Plan 03 must_haves; pattern lives in `findings/base64_bytes.rs` now)
- **`crates/miner-core/src/spike_figment.rs`** (per Plan 03 must_haves; pattern recommended for Plan 05's implementation)
- **`crates/miner-core/tests/spike_schema.rs`** (per Plan 03 must_haves)
- **`crates/miner-core/tests/spike_figment_precedence.rs`** (per Plan 03 must_haves)

The `crates/miner-core/tests/` directory is now empty — all subsequent tests live as `#[cfg(test)]` modules inside the source files. Plan 04 may reintroduce an integration test crate for the StdoutSink end-to-end test if it makes sense (the plan does not yet say).

## Decisions Made

- **Locked envelope fields are INLINED, not `flatten`-ed.** Per RESEARCH §Anti-Patterns, `#[serde(flatten)]` historically generates an `additionalProperties: true` schema, which weakens the contract. Schemars 1.x partially improves this, but the inline cost is at most 7 fields × 3 variants = 21 declared fields. The plan-mandated derives match: `ResultFinding`, `ScanErrorFinding`, and `GapAbortedFinding` each carry the locked seven plus their per-variant additions.
- **`scan_id@version` is exposed as `scan_id_at_version` in Rust source via `#[serde(rename = "scan_id@version")]`.** The `@` character is not a valid Rust identifier, so the Rust field name must differ. The serialised key is the literal D-12 string the consumers expect.
- **`Raw::new` enforces D-03 at construction.** A future scan implementation that accidentally builds a `Raw` without `timestamps_ms` will get `Err("Raw::new: \`series\` must contain a \`timestamps_ms\` array (D-03)")` at the call site instead of producing a schema-conforming-but-semantically-wrong finding. `Raw::new_unchecked` is `#[cfg(test)]` only — production callers cannot reach it.
- **`From<MinerError> for WireError` defaults to `ScanErrorCode::InternalPanicCaught`.** Generic boundary conversion; specific call-sites that have a more precise code in hand should construct `WireError::scan()` / `WireError::preflight()` directly. Plan 05's preflight-error path is the first concrete caller that will do this — its figment-error classifier converts `figment::Error::kind()` into `PreflightCode::InvalidConfig` / `MissingRequiredConfig` rather than the generic InternalPanicCaught default.
- **`VecSink` is `#[cfg(test)]` only.** Plan 04 may promote this (or a generic `WriterSink<W: Write>`) into a production helper for the `--output=file` config path; for now it's a unit-test fixture only, which keeps the production `FindingSink` impl list at exactly one (`StdoutSink`, landing in Plan 04). This avoids the "two sink impls = drift risk" anti-pattern from the start.
- **`schemars` `chrono04` feature added at the workspace level.** The alternative — manual `JsonSchema` impls on every `DateTime<Utc>` field — would be much more code and would not match the well-tested behaviour of schemars' upstream implementation. The feature is a single line in `Cargo.toml` and is the canonical opt-in for chrono ↔ schemars interop.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 — Blocking issue] `chrono::DateTime<Utc>` requires schemars `chrono04` feature for `JsonSchema` derive**

- **Found during:** Task 1 (first `cargo build -p miner-core` after writing `findings/mod.rs`)
- **Issue:** Seven compile errors of the form `the trait bound \`DateTime<Utc>: JsonSchema\` is not satisfied` — one per field on `RunStart`, `RunEnd`, `ResultFinding`, `ScanErrorFinding`, `GapAbortedFinding`. Schemars 1.x ships chrono integration behind an opt-in feature flag (`chrono04`); the workspace `Cargo.toml` declared `schemars = "1"` without features.
- **Fix:** Changed the workspace dep to `schemars = { version = "1", features = ["chrono04"] }`. The Plan 01-02 spike did not surface this because it used no `chrono` fields (the spike's `SpikeBase64Bytes` is bytes-only and `SpikeRawArray` carries `Vec<u64>` for shape — no datetime).
- **Files modified:** `Cargo.toml` (workspace root), `Cargo.lock` (transitive refresh).
- **Commit:** `56cd95c` (Task 1)
- **Justification for Rule 3 (not Rule 4):** No new dependency; no architectural change; the feature flag is the canonical opt-in documented by upstream `schemars`. The alternative (manual `JsonSchema` impls on every datetime field) would be net-worse code for the same outcome.

**2. [Rule 2 — Missing critical functionality] `Raw::new` validation for D-03**

- **Found during:** Task 1 (writing the `Raw` struct)
- **Issue:** D-03 mandates that `raw.series` always carries a `timestamps_ms` array when `raw` is present, but the struct shape `Raw { series: BTreeMap<String, RawArray> }` admits an empty map at the type level. A future scan could quietly produce a `Result` finding with `raw: Some(Raw { series: empty })` and pass the schema (BTreeMap is just `object`), violating the consumer contract.
- **Fix:** Added a fallible `Raw::new(series) -> Result<Self, &'static str>` constructor that rejects missing `timestamps_ms`, plus a `#[cfg(test)]`-only `Raw::new_unchecked` for tests that exercise other fields. The plan's action text explicitly recommended this pattern (`pub fn new ... -> Result<Self, &'static str>` per the action notes for `findings/mod.rs`); I implemented it verbatim.
- **Files modified:** `crates/miner-core/src/findings/mod.rs`.
- **Commit:** `56cd95c` (Task 1).
- **Justification for Rule 2:** Required for correctness per D-03. The plan recommended it; this is implementation-of-plan-text rather than a true deviation, but cataloguing for completeness.

**3. [Rule 1 — Plan-text resequencing] Defer `pub mod sink` + lib.rs FROZEN surface to Task 2**

- **Found during:** Task 1 commit staging (after writing all files)
- **Issue:** The plan's Task 1 `<action>` block specifies a FROZEN `lib.rs` that re-exports `FindingSink`, `MinerError`, `WireError`, `PreflightCode`, `ScanErrorCode`, `MinerConfig`, `OutputDest` — but the `<files>` list for Task 1 does NOT include `findings/sink.rs`, `error/`, or `config/`. Those files are exclusively Task 2's responsibility (per its `<files>` list). Committing Task 1 with a `lib.rs` that imports from `error::`/`config::`/`findings::sink` while those modules don't exist would fail to compile — and Task 1 must compile atomically as a per-task commit.
- **Fix:** Task 1's `lib.rs` re-exports only the findings module's surface (and adds an inline comment explaining Task 2 extends the list); Task 1's `findings/mod.rs` declares `pub mod base64_bytes; pub mod run_id;` only (with an inline comment saying Task 2 adds `pub mod sink;` because sink.rs depends on `crate::error::MinerError`). Task 2 then commits the missing modules AND restores both lines verbatim — at which point lib.rs is the FROZEN surface the plan describes and the post-Task-2 state matches the plan's "after this task, the file MUST look like..." spec exactly. The grep gates in the plan's Task 2 verify line confirm every name (`MinerConfig`, `WireError`, `PreflightCode`, `FindingSink`, etc.) is present in lib.rs.
- **Files modified:** `crates/miner-core/src/lib.rs` and `crates/miner-core/src/findings/mod.rs` were each touched in BOTH commits. This is acceptable — they're not new files in the second commit; they're extensions.
- **Commits:** `56cd95c` (Task 1, partial), `a33d98a` (Task 2, extended to the FROZEN surface).
- **Justification for Rule 1:** This is a plan-text inconsistency (the FROZEN-surface action conflicts with the per-task `<files>` list). Both options — (a) violate per-task atomicity and ship lib.rs with stubs, or (b) defer the surface extension to Task 2 — preserve the post-plan state the plan describes. Option (b) is cleaner because each commit builds and tests cleanly on its own. The final state is identical to what the plan specifies.

**4. [Rule 1 — Cleanup, post-spike housekeeping] Update `crates/miner-core/Cargo.toml` comments + remove orphan `clap` dev-dep**

- **Found during:** Task 1 (during the spike-file deletion sweep)
- **Issue:** `crates/miner-core/Cargo.toml` carried multi-line comments labelling its `figment` and `clap` deps as "Spike-only (Plan 01-02)". The spike files are now deleted, so those labels are stale; the `clap` dev-dep is genuinely orphan (no remaining file imports clap). Leaving them creates documentation drift for the next contributor.
- **Fix:** Removed the orphan `clap.workspace = true` dev-dep entry. Updated the top-of-file comment block and the `figment` dev-dep comment to describe the post-spike rationale (Plan 05 will reuse `figment::Jail` for config-precedence tests). The `figment.workspace = true` line in `[dependencies]` is preserved because `MinerError::Config(#[from] figment::Error)` references the type and Plan 05's production builder will too.
- **Files modified:** `crates/miner-core/Cargo.toml`.
- **Commit:** `56cd95c` (Task 1, bundled with the spike-deletion changes).
- **Justification for Rule 1:** Documentation accuracy is a correctness concern. The orphan dep removal is housekeeping; not removing it would be Rule-1-able too (eventually `cargo-machete` would flag it).

**5. [Rule 1 — Plan verify-script unsanitized grep] `grep -c '#\[serde(flatten)\]'` matches doc comments**

- **Found during:** Final plan-level verification (after Task 2 commit)
- **Issue:** The plan's `<verification>` line `grep -c '#\[serde(flatten)\]' crates/miner-core/src/findings/mod.rs returns 0` actually returns `2`, because the file has two `#[serde(flatten)]` references inside doc comments (the anti-pattern is documented as a "do NOT use" reminder in the module-level docstring and in a comment above the `Finding` enum). The actual count of `#[serde(flatten)]` ATTRIBUTES is zero — confirmed via `grep -c '^[^/]*#\[serde(flatten)\]' crates/miner-core/src/findings/mod.rs` → 0.
- **Fix:** None on disk — the file is correct (no `flatten` attribute in production code). Documented here so Plan 04 (which will lift these into a CI workflow) anchors its grep on a line-start non-comment pattern OR strips comments before grepping.
- **Files modified:** None.
- **Commit:** N/A.
- **Justification for Rule 1:** Plan-text bug, not a Rust-source bug. Flagging for Plan 04 to fix in the actual CI gate.

### Auto-Added Critical Functionality

Beyond the explicit plan deliverables, the following correctness-required additions were made:

- **`Display` impl on `RunId`** — `Test 3` (run_id_format) asserts the wire-form (the `Display` output) is 26 chars in the Crockford alphabet. `ulid::Ulid` already has `Display`; the wrapper delegates to it. Without this, `to_string()` on a `RunId` would not compile. (Rule 2 — required for the plan's specified behaviour.)
- **`Default` impl on `RunId`** — provided as a convenience for deserialisation paths where a placeholder is needed. Calls `Self::new()` so the placeholder is a valid ULID. This is technically beyond the plan's spec but is harmless (the Plan 05 emit_fixture path constructs RunId explicitly via `RunId::new()`; Default is only reachable if a future caller asks for it).
- **`with_context` builder helper on `WireError`** — convenience for adding context fields incrementally. Plan 05's figment-error classifier will use this to attach `{"field": "cache_root", ...}` style context maps to the WireError it emits. (Rule 2 — Plan 05 needs to attach context; making it ergonomic now avoids friction.)

### Auth Gates

None — entirely a code-only plan.

## Deferred Issues

**1. Pre-existing `clippy::map_unwrap_or` warning in `crates/miner-core/build.rs:19`** — first flagged in Plan 01-02 SUMMARY's Deferred Issues. `cargo clippy --fix` did surface this during my run but I REVERTED the change because `build.rs` is not in Task 1 / Task 2 `<files>` and the scope-boundary rule says "Only auto-fix issues DIRECTLY caused by the current task's changes." Plan 04's CI gate setup is the right place to either fix the warning or exclude `build.rs` from the gate. The build.rs SHA-injection logic itself is sound.

**2. `cargo clippy -p miner-core --all-targets` emits ~19 doc-style pedantic warnings (missing backticks, missing `# Errors` sections).** Most of these I applied via `cargo clippy --fix` during development (you see the changes inside the source); the remainder are stylistic and do not affect runtime correctness. Plan 04's CI gate will run `cargo clippy ... -- -D warnings` and may either auto-fix these or require me to clean them up in a follow-up. The lib + test target builds and `cargo test` passes cleanly today.

**3. `crates/miner-core/tests/` directory is now empty.** This is fine — Plan 03 has no integration tests; everything is `#[cfg(test)] mod tests` inside `src/`. Plan 04 may reintroduce an `tests/sink_jsonl_output.rs` for StdoutSink end-to-end testing. Plan 06 will introduce `tests/schema_sync.rs` for the D-22 schema-validation gate. Plan 05 will reintroduce `tests/config_precedence.rs` to replace the deleted `spike_figment_precedence.rs` (using the same `figment::Jail` pattern).

## Issues Encountered

- **`chrono04` feature on schemars was the only true blocker.** Once the feature was enabled, the cascade of `JsonSchema` errors on `DateTime<Utc>` fields cleared in one re-build. Documented above as Deviation 1.
- **No issues with Base64Bytes or RunId.** Plan 01-02 derisked both — the verbatim port compiled on first try and the manual `JsonSchema` impls work.
- **`serde::Deserialize` on enum-style `OutputDest::File(PathBuf)`** with `#[serde(rename_all = "snake_case")]` automatically maps the JSON shape `{"file": "/path"}` ↔ the Rust `OutputDest::File(PathBuf::from("/path"))`. No special handling needed — verified by `config::tests::miner_config_type_shape`.
- **The plan's Task 1 / Task 2 file-list vs lib.rs frozen-surface inconsistency was the only architectural friction.** Resolved by deferring the lib.rs surface extension to Task 2 (Deviation 3). No information was lost; the final post-plan state matches the plan's intent verbatim.

## Threat Mitigation

- **T-01-02 (schema injection / drift):** Every envelope type derives `JsonSchema` — the Rust types are the source of truth. Plan 06's xtask will regenerate `schemas/findings-v1.schema.json` from these derivations and CI will diff against the checked-in artifact. Renaming a field, changing a type, or adding a variant without regenerating the schema artifact will fail the diff gate. Combined with the schemars `chrono04` feature (which gives well-tested datetime schema generation) and the manual JsonSchema impls on `Base64Bytes` and `RunId` (which encode the format constraints `contentEncoding` and the Crockford-base32 pattern), the schema contract is mechanically tight from day one. Test 1 (`envelope_fields_present`) verifies the seven locked fields make it to the JSON; Tests 3 (`run_id_format`) and 6 (`base64_round_trip`) verify the wire-form contracts for the format-constrained newtypes.
- **T-01-04 (code revision tampering):** Every applicable variant (`ResultFinding`, `ScanErrorFinding`, `GapAbortedFinding`) carries `code_revision: String`. Callers populate this field from `miner_core::CODE_REVISION` (set by Plan 01-01's `build.rs` from `git rev-parse HEAD`; appended with `dirty-` when the worktree had uncommitted changes). No mitigation regression vs Plan 01-01.

## User Setup Required

None — entirely a code-only plan. Run `cargo build -p miner-core && cargo test -p miner-core` to validate.

## Next Phase Readiness

- **Plan 04 (Wave 4, stdout/stderr discipline + StdoutSink impl)** is UNBLOCKED. The `FindingSink` trait is defined; Plan 04 lands `StdoutSink: FindingSink` in `crates/miner-core/src/findings/sink.rs` (the same file) and fills `crates/miner-core/src/error/stderr_emit.rs` with the structured-stderr writer. The `#[cfg(test)] VecSink` pattern in `sink.rs` is the recommended in-memory test fixture for the JSONL output integration test.
- **Plan 05 (Wave 5, config layering + miner-cli main)** is UNBLOCKED. The `MinerConfig` + `OutputDest` schema types are defined; Plan 05 implements `crates/miner-core/src/config/build_figment.rs` (or extends `config/mod.rs`) with the verified Pattern 4 figment builder. The `figment` dev-dep with `test` feature is already wired in `miner-core/Cargo.toml` for `figment::Jail` precedence tests. Plan 05's `emit_fixture()` will reuse the `RunId::new()` → `RunStart` → `RunSummary::default()` → `RunEnd` chain that Tests 4 and 5 lock in. Plan 05's preflight-error classifier will use `WireError::preflight(PreflightCode::MissingRequiredConfig, ...).with_context(...)` to emit structured-stderr JSON when `figment.extract()` fails.
- **Plan 06 (Wave 6, schema regen + CI gates)** is UNBLOCKED. The full envelope type system derives `JsonSchema`; `xtask gen-schema` will call `schemars::schema_for!(Finding)` and produce `schemas/findings-v1.schema.json`. The D-22 schema-validation cargo test will construct one of each variant (the `sample_*` helpers in `findings::tests` show the shapes) and validate against the schema via the `jsonschema` crate.
- **Plan 07 (Wave 7, CI workflow)** is partially derisked. The CI gates D-21 will validate Phase 1 — `cargo build`, `cargo clippy --workspace --all-targets -- -D warnings` (Plan 04 stage), `cargo tree -p miner-core` (already passes), and the schema-sync diff (Plan 06 stage). The clippy warnings cataloged in Deferred Issues #2 need to be cleaned up before -D warnings goes hard.

No blockers.

## Self-Check: PASSED

File existence (created files in this plan):

- `FOUND: crates/miner-core/src/findings/mod.rs`
- `FOUND: crates/miner-core/src/findings/base64_bytes.rs`
- `FOUND: crates/miner-core/src/findings/run_id.rs`
- `FOUND: crates/miner-core/src/findings/sink.rs`
- `FOUND: crates/miner-core/src/error/mod.rs`
- `FOUND: crates/miner-core/src/error/codes.rs`
- `FOUND: crates/miner-core/src/error/stderr_emit.rs`
- `FOUND: crates/miner-core/src/config/mod.rs`

Deleted files (per Plan 03 must_haves) are GONE:

- `GONE: crates/miner-core/src/spike_base64.rs`
- `GONE: crates/miner-core/src/spike_figment.rs`
- `GONE: crates/miner-core/tests/spike_schema.rs`
- `GONE: crates/miner-core/tests/spike_figment_precedence.rs`

Commit hashes:

- `FOUND: 56cd95c` (Task 1 — Finding envelope types)
- `FOUND: a33d98a` (Task 2 — Error vocabulary + sink trait + config schema)

FROZEN public surface (in `crates/miner-core/src/lib.rs`):

- `PRESENT: Finding, FindingSink, RunStart, RunEnd, RunSummary, PerScanCounts, ResultFinding, ScanErrorFinding, GapAbortedFinding, RunId, Base64Bytes, Dtype, RawArray, Raw, Effect, DataSlice, TimeRange, Source` (18 from findings)
- `PRESENT: MinerError, WireError, PreflightCode, ScanErrorCode` (4 from error)
- `PRESENT: MinerConfig, OutputDest` (2 from config)
- **Total: 24 names — every one Plans 05/06/07 import**

Plan-level verification:

- `cargo build -p miner-core` → `Finished dev profile … in 0.21s` (pass)
- `cargo build -p miner-core --release` → `Finished release profile … in 1.05s` (pass)
- `cargo build --workspace` → `Finished dev profile … in 2.92s` (pass)
- `cargo test -p miner-core` → 14 passed, 0 failed (pass)
- Anti-pattern grep `#[serde(flatten)]` actual attributes: 0 (pass; pre-existing plan verify-grep bug noted in Deviation 5)
- Anti-pattern grep `HashMap<` in findings/mod.rs: 0 (pass — every map is BTreeMap)
- Regression-armour grep `Debug, Clone, Copy, PartialEq, Eq, Hash` in run_id.rs: present (pass)
- Regression-armour grep `Default, Debug, Clone` in findings/mod.rs: present (pass)

---
*Phase: 01-foundations-contracts*
*Completed: 2026-05-16*
