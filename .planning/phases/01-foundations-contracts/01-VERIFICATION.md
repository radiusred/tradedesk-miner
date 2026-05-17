---
phase: 01-foundations-contracts
verified: 2026-05-17T16:30:00Z
re_verified: 2026-05-17T16:45:00Z
status: passed
score: 8/8 must-haves verified
overrides_applied: 0
requirements_verified:
  - FOUND-01: pass
  - FOUND-02: pass
  - FOUND-03: pass
  - FOUND-04: pass
  - FOUND-05: pass
  - OUT-01: pass
  - OUT-02: pass
  - OUT-03: pass
must_haves_verified: 8/8
fmt_gate_resolution: >
  Initial verification flagged a 'cargo fmt --check' failure in
  crates/miner-cli/tests/cli_streams.rs (lines 406-425, rustfmt drift
  from the CR-01 regression-test added at ba6ce97). Resolved by running
  'cargo fmt --all' and committing the result. All four CI gates now
  green: build, clippy -D warnings, tokio-tree grep, schema-sync diff,
  plus fmt --check and test --workspace. Re-verified above.
---

# Phase 1: Foundations & Contracts — Verification Report

**Phase Goal:** Lay the Rust workspace + the locked Finding envelope JSON Schema + the
stream-discipline writers and the figment config layer. Every later phase compiles against
this contract surface.

**Verified:** 2026-05-17T16:30:00Z
**Status:** human_needed (one CI gate failure — formatting only; all contracts verified)
**Re-verification:** No — initial verification

---

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | User can `cargo build --workspace` and produce all six member crates plus xtask | VERIFIED | `cargo build --workspace` exits 0; all 7 members compiled |
| 2 | User can emit a Finding envelope as JSONL with all 5 variants validated against the committed schema | VERIFIED | 8/8 cli_streams tests pass; 3/3 schema_roundtrip tests pass; schema contains all variants |
| 3 | User can run `cargo clippy --workspace` and have it reject `println!`/`eprintln!` outside sanctioned writers | VERIFIED | `cargo clippy --workspace --all-targets -- -D warnings` exits 0; clippy.toml has all 5 disallowed macros |
| 4 | User can verify `cargo tree -p miner-core` produces zero tokio/async transitive dependencies | VERIFIED | tokio-free gate passes; PROHIBITED regex finds no matches |
| 5 | User can override cache root, bar-cache root, and output via CLI flag > env > TOML > error | VERIFIED | 29/29 miner-core unit tests pass including 7 config tests; 4/4 config_precedence integration tests pass |
| 6 | miner-core has zero workspace-internal dependencies (one-way arrow) | VERIFIED | `cargo tree -p miner-core --edges normal --depth 1` shows no other miner-* crates |
| 7 | Workspace uses resolver="3", edition="2024", rust-version="1.85"; toolchain pinned to 1.85 | VERIFIED | Cargo.toml lines 19, 31-33; rust-toolchain.toml channel="1.85" |
| 8 | OUT-03 byte-identity: emit-fixture run twice produces byte-identical output (volatile fields masked) | VERIFIED | `emit_fixture_byte_identical_when_volatile_fields_masked` passes |

**Score:** 8/8 truths verified

---

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `Cargo.toml` | Workspace root, resolver=3, edition=2024, MSRV=1.85, serde_json without preserve_order | VERIFIED | All present; no preserve_order feature in serde_json |
| `rust-toolchain.toml` | channel="1.85", clippy+rustfmt components | VERIFIED | File present with exact content |
| `.cargo/config.toml` | xtask alias | VERIFIED | `xtask = "run --package xtask --"` present |
| `clippy.toml` | disallowed-macros for std::println/print/eprintln/eprint/dbg | VERIFIED | All 5 entries present; clippy clean workspace-wide |
| `crates/miner-core/build.rs` | CODE_REVISION injection; git diff --quiet HEAD for staged changes | VERIFIED | CR-02 fix applied in commit cf55f90; `git diff --quiet HEAD` present |
| `crates/miner-core/src/lib.rs` | FROZEN pub use block; all 18 exported names | VERIFIED | All names present including FindingSink, RunId, Base64Bytes, etc. |
| `crates/miner-core/src/findings/mod.rs` | #[serde(tag="kind")], 5 variants, BTreeMap not HashMap, no #[serde(flatten)] in code | VERIFIED | HashMap only in comments; no flatten in code; BTreeMap<String,RawArray> present |
| `crates/miner-core/src/findings/base64_bytes.rs` | Base64Bytes(Vec<u8>) + manual JsonSchema impl with contentEncoding | VERIFIED | Schema has contentEncoding/contentMediaType |
| `crates/miner-core/src/findings/run_id.rs` | RunId(Ulid) with Copy, Hash, JsonSchema pattern | VERIFIED | #[derive(Debug,Clone,Copy,PartialEq,Eq,Hash,...)] present; ULID pattern in schema |
| `crates/miner-core/src/findings/sink.rs` | StdoutSink (only stdout writer) + FileSink (CR-01 fix) + FindingSink trait | VERIFIED | CR-01 fix in commit ba6ce97; FileSink present; StdoutSink sole stdout opener |
| `crates/miner-core/src/error/codes.rs` | 7 PreflightCode variants, 4 ScanErrorCode variants, WireError with String code | VERIFIED | All variants present with snake_case serialisation |
| `crates/miner-core/src/error/stderr_emit.rs` | write_preflight_error + emit_to_stderr via io::Write (no eprintln!) | VERIFIED | File present; tests pass; no eprintln in production code |
| `crates/miner-core/src/config/mod.rs` | build_figment with CLI-last merge order; CliOverrides with skip_serializing_if; zero hardcoded paths | VERIFIED | All present; library_has_no_hardcoded_paths test enforces it |
| `crates/miner-cli/src/cli.rs` | Cli struct, Command::EmitFixture, resolve_toml_path | VERIFIED | File present; used by cli_streams integration tests |
| `crates/miner-cli/src/main.rs` | tracing init, clap parse, figment build, make_sink dispatch, classify_figment_error | VERIFIED | All present; FileSink dispatch wired after CR-01 fix |
| `schemas/findings-v1.schema.json` | Generated from schemars; all 5 variants; contentEncoding; ULID pattern; byte-deterministic | VERIFIED | `gen-schema` + `git diff --exit-code` passes; twice-run cmp passes |
| `xtask/src/main.rs` | GenSchema subcommand; serde_json::to_value normalisation; #![allow(clippy::disallowed_macros)] | VERIFIED | All present; gen-schema produces correct output |
| `.github/workflows/ci.yml` | 4 mandatory CI gates (build, clippy, tokio-tree, schema-sync) + fmt + test | VERIFIED | All 4 gates present with exact PROHIBITED regex |
| `crates/miner-core/tests/schema_roundtrip.rs` | 3 tests: all_kinds_validate, dsr/fdr_q null, raw contentEncoding | VERIFIED | 3/3 pass |
| `crates/miner-core/tests/config_precedence.rs` | 4 tests covering CLI > env > TOML > Err | VERIFIED | 4/4 pass |
| `crates/miner-cli/tests/cli_streams.rs` | 8 tests including OUT-03 byte-identity and CR-01 file-sink regression gate | VERIFIED | 8/8 pass |
| `README.md` | Quickstart section with emit-fixture instructions | VERIFIED | Quickstart section present with 5-step guide |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| `Cargo.toml` | `crates/*/Cargo.toml` | workspace members glob | VERIFIED | 7 members declared |
| `crates/miner-core/src/lib.rs` | `build.rs MINER_CODE_REVISION` | `env!("MINER_CODE_REVISION")` | VERIFIED | Macro present in lib.rs |
| `.cargo/config.toml` | xtask crate | alias entry | VERIFIED | `xtask = "run --package xtask --"` |
| `crates/miner-cli/src/main.rs` | `miner_core::config::build_figment` | function call after clap parse | VERIFIED | `classify_figment_error` + `MinerConfig::resolve` in main.rs |
| `crates/miner-cli/src/main.rs` | `miner_core::findings::sink::StdoutSink` | `make_sink` dispatch | VERIFIED | CR-01 fix wired; StdoutSink constructed for Stdout dest |
| `crates/miner-cli/src/main.rs` | `miner_core::error::stderr_emit::emit_to_stderr` | preflight error path | VERIFIED | `emit_to_stderr` call present in error branch |
| `xtask/src/main.rs` | `miner_core::findings::Finding` | `schema_for!(Finding)` | VERIFIED | Gen-schema produces correct schema |
| `.github/workflows/ci.yml` | `xtask gen-schema + git diff` | schema sync gate | VERIFIED | Both steps present in workflow |

### Data-Flow Trace (Level 4)

| Artifact | Data Variable | Source | Produces Real Data | Status |
|----------|---------------|--------|--------------------|--------|
| `emit_fixture()` in main.rs | `run_id`, `started_at_utc` | `RunId::new()`, `chrono::Utc::now()` | Yes (ULID + timestamp) | FLOWING |
| `StdoutSink::write_envelope` | `finding` arg | caller-provided Finding struct | Yes | FLOWING |
| `build_figment` | `MinerConfig` | CLI args + env + TOML file | Yes (precedence-tested) | FLOWING |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| emit-fixture produces 2 JSONL lines on stdout | `MINER_CACHE_ROOT=/tmp/cache MINER_BAR_CACHE_ROOT=/tmp/bar MINER_OUTPUT=stdout cargo run -q -p miner-cli -- emit-fixture` | 2 lines, run_start + run_end, shared run_id | PASS |
| Preflight failure exits 1 with WireError JSON on stderr | `cargo run -q -p miner-cli -- emit-fixture` (no config) | exit 1, stdout empty, `{"code":"missing_required_config",...}` on stderr | PASS |
| FileSink writes to file when MINER_OUTPUT is a path | `MINER_OUTPUT=<tmpfile> cargo run -q -p miner-cli -- emit-fixture` | exit 0, stdout empty, 2 envelopes in file | PASS |
| Schema sync (gen-schema then git diff) | `cargo run -q -p xtask -- gen-schema && git diff --exit-code schemas/` | exit 0, no diff | PASS |
| Twice-run determinism | `gen-schema` twice then `cmp` | byte-identical | PASS |
| tokio-free miner-core | tokio-tree grep with PROHIBITED regex | empty output | PASS |

### Probe Execution

No probe scripts declared or present for this phase. Step 7c: SKIPPED.

### Requirements Coverage

| Requirement | Source Plan(s) | Description | Status | Evidence |
|-------------|---------------|-------------|--------|----------|
| FOUND-01 | 01-01 | Workspace with miner-core + wrappers | SATISFIED | 7-crate workspace builds; one-way dep direction enforced |
| FOUND-02 | 01-04, 01-07 | stdout=findings, stderr=logs, CI-enforced | SATISFIED | clippy.toml gates; StdoutSink single writer; cli_streams tests pass |
| FOUND-03 | 01-03, 01-06, 01-07 | Locked Finding envelope JSON Schema | SATISFIED | schemas/findings-v1.schema.json committed; CI schema-sync gate; runtime validation tests |
| FOUND-04 | 01-01, 01-06 | Zero tokio/async in miner-core, CI-enforced | SATISFIED | tokio-tree gate passes locally and in CI workflow |
| FOUND-05 | 01-02, 01-05, 01-07 | CLI > env > TOML > error; no hardcoded paths | SATISFIED | 7 config unit tests + 4 integration tests pass; library_has_no_hardcoded_paths enforced |
| OUT-01 | 01-04, 01-07 | NDJSON on stdout | SATISFIED | StdoutSink writes `\n`-terminated JSON; cli_streams test confirms 2 lines |
| OUT-02 | 01-03, 01-07 | Every finding carries locked envelope fields + dsr/fdr_q as null | SATISFIED | schema_roundtrip dsr/fdr_q test; envelope_fields_present unit test |
| OUT-03 | 01-07 | Deterministic output ordering | SATISFIED | BTreeMap throughout; emit_fixture_byte_identical_when_volatile_fields_masked passes |

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| `crates/miner-cli/tests/cli_streams.rs` | ~406-425 | Unformatted rustfmt diff (long method-chain closure) | WARNING | CI gate `cargo fmt --all -- --check` exits 1; must be fixed before CI is green |
| All crates | N/A | No `version` field declared; all crates are `0.0.0` | INFO | `miner_version` field in every RunStart envelope is `"0.0.0"` (WR-02 from code review, deferred) |
| `crates/miner-core/Cargo.toml` | 34 | `blake3` declared but never used in production code | INFO | Dead dep; WR-03 from code review, deferred |

No `TBD`, `FIXME`, or `XXX` markers found in phase-modified files. No BLOCKER anti-patterns.

---

### Human Verification Required

#### 1. Formatting Gate — `cargo fmt --all -- --check`

**Test:** From repo root, run `cargo fmt --all` then `cargo fmt --all -- --check`.

**Expected:** `cargo fmt --all -- --check` exits 0 with no diff.

**Why human:** The CI workflow gates on `cargo fmt --all -- --check`. This currently exits 1 with a diff in `crates/miner-cli/tests/cli_streams.rs` lines 406-425 (a closure body that rustfmt wants to reformat). Running `cargo fmt --all` and committing the result is a one-command fix but requires an intentional commit. The verifier does not commit per protocol.

The diff is purely cosmetic (rustfmt reformatting a closure), not a logic change. All 8 cli_streams tests pass despite the formatting discrepancy. But CI will fail on this gate until it is committed.

**Resolution:** `cargo fmt --all && git add crates/miner-cli/tests/cli_streams.rs && git commit -m "style(01): apply rustfmt to cli_streams.rs"`

---

### Gaps Summary

No gaps blocking goal achievement. All 8 must-have truths are VERIFIED. All 8 requirements (FOUND-01 through FOUND-05, OUT-01 through OUT-03) are SATISFIED.

The single human verification item is a cosmetic formatting issue that will cause CI to fail (`cargo fmt --all -- --check`) but does not affect the correctness of any Phase 1 contract. It is a one-command fix that requires a commit.

**Deferred warnings from code review (WR-01 through WR-07, IN-01 through IN-06) remain open** but were explicitly deferred by the operator at the time of the review. None of them block the phase goal. The most consequential deferred items are:

- **WR-01**: `OutputDest::File` cannot be expressed via the env/TOML layers (only via CLI); figment deserialization of bare-string paths fails. However, the FileSink dispatch is wired (CR-01 fixed) and the `MINER_OUTPUT=/path` env var is captured by clap's `env = "MINER_OUTPUT"` directive before reaching figment, so users invoking via CLI are unaffected.
- **WR-02**: All crates at version `0.0.0`; `miner_version` in every RunStart envelope is `"0.0.0"`.
- **WR-04**: `dsr`/`fdr_q` are not in the schema's `required` list; the OUT-02 "MUST serialise as null" contract is producer-only.

---

_Verified: 2026-05-17T16:30:00Z_
_Verifier: Claude (gsd-verifier)_
