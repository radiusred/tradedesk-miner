---
phase: 01-foundations-contracts
plan: 07
subsystem: foundations
tags: [rust, integration-tests, schema-validation, config-precedence, subprocess, jsonl, out-03, found-02, found-03, found-05, threat-mitigation-t-01-02, threat-mitigation-t-01-03, phase-1-sign-off]

# Dependency graph
requires: [plan-01-01, plan-01-02, plan-01-03, plan-01-04, plan-01-05, plan-01-06]
provides:
  - "`crates/miner-core/tests/schema_roundtrip.rs` — runtime schema-validation integration test (D-22). Loads the COMMITTED `schemas/findings-v1.schema.json` via `jsonschema::validator_for`, constructs one instance of each `Finding` variant (`RunStart`, `Result` with `raw`, `Result` without `raw`, `ScanError`, `GapAborted`, `RunEnd`) using the public re-export surface, and asserts every instance validates. Also asserts `dsr` and `fdr_q` serialise as JSON `null` (OUT-02) and that the `Base64Bytes` `contentEncoding` path is exercised via a populated `raw.series`."
  - "`crates/miner-core/tests/config_precedence.rs` — production-grade FOUND-05 integration test against `miner_core::{MinerConfig, CliOverrides, build_figment}` from the public re-export surface. Four tests (`cli_wins_over_env_and_toml`, `env_wins_when_cli_omitted`, `toml_wins_when_only_source`, `missing_required_yields_err`) cover all three required fields (`cache_root`, `bar_cache_root`, `output`) with `#[serial_test::serial]` annotations on every env-touching test. Uses `figment::Jail` instead of raw `std::env::set_var` (Rule 3 deviation — see below)."
  - "`crates/miner-cli/tests/cli_streams.rs` — end-to-end CLI integration test spawning the actual built `miner` binary via `assert_cmd::Command::cargo_bin(\"miner\")`. Seven tests: happy-path stdout shape (2 JSONL lines, `run_start` + `run_end`), stderr-only tracing (T-01-03 regression gate), shared `run_id` across envelopes, schema validation against the committed schema (FOUND-03, T-01-02 regression gate), preflight `missing_required_config` path (D-06/D-07), preflight `invalid_config` path (Plan 05 `classify_figment_error` regression gate), and the **OUT-03 masked twice-run byte-identity test** that closes envelope determinism fully (NOT partially)."
  - "`README.md` — Phase 1 Quickstart. Five-step walk-through from clone → `cargo build` → emit-fixture (sets the three required MINER_* env vars inline) → `jq` schema inspection → `cargo test` / `cargo clippy`. Includes Status, License, 'What Phase 1 Delivers' summary tied to the FOUND-01..05 / OUT-01..03 contract surface, Architecture pointer, Roadmap pointer."
  - "Phase 1 sign-off ritual completed locally — seven gates (build, clippy `-D warnings`, tokio-free `miner-core` regex, schema-sync diff, `cargo test`, `cargo fmt --check`, emit-fixture smoke) all exit 0 against this commit tree."
affects: [phase-02, phase-03, phase-04, phase-05, phase-06, phase-07]

# Tech tracking
tech-stack:
  added:
    - "`serial_test = \"3\"` (miner-core dev-dep) — serialises `MINER_*` env-var-mutating tests across the integration-test binary's parallel runner. miner-cli already had it from Plan 05; added to miner-core for the new `config_precedence` integration test."
    - "`jsonschema.workspace` + `serde_json.workspace` (miner-cli dev-deps) — workspace-pinned 0.46 + 1 respectively, used by `cli_streams.rs` for schema validation of each emitted envelope and for JSON parsing of stdout/stderr lines."
  patterns:
    - "Pattern — Integration tests live in `crates/<crate>/tests/` and exercise the public re-export surface (`miner_core::*`). Internal unit tests live alongside the source in `src/.../mod.rs::tests`. The two layers test the same invariants from different angles: unit tests catch internal-API regressions; integration tests prove the FROZEN public surface from `lib.rs` is sufficient to exercise every Phase 1 contract end-to-end. This is the layer-of-tests playbook every later phase will follow."
    - "Pattern — `figment::Jail` for env-isolated integration tests. Plan 07's `config_precedence.rs` and Plan 05's in-crate tests both use `Jail::expect_with(|jail| { jail.set_env(..); jail.create_file(..); ... })`. Jail is a process-wide RAII lock that figment provides behind its `test` feature; it snapshots env on enter and restores on drop. Required because the workspace lints set `unsafe_code = \"forbid\"` (at a level `#![allow]` cannot override) and Rust 2024 made `std::env::set_var` `unsafe`. Jail wraps the unsafe internally."
    - "Pattern — `assert_cmd::Command::cargo_bin(\"miner\")` for end-to-end subprocess tests. Spawns the actual built binary, captures stdout / stderr / exit status as separate buffers, and lets tests assert against the real OS process boundary. Plan 07 Task 2 uses `env_clear()` + selective `env(\"PATH\", ...)` to prevent the developer's `MINER_*` shell vars from leaking into the subprocess, and `current_dir(tempdir)` + `env(\"XDG_CONFIG_HOME\", tempdir)` for preflight-failure tests so neither CWD nor XDG can supply a fallback config."
    - "Pattern — Volatile-field masking for byte-determinism gates. `cli_streams::emit_fixture_byte_identical_when_volatile_fields_masked` is the canonical shape: run a binary TWICE, capture stdout, parse each line as JSON, recursively mask the four KNOWN-volatile fields (`run_id`, `started_at_utc`, `ended_at_utc`, `wall_clock_ms`) to fixed sentinel values, re-serialise with `serde_json::to_string` (compact form), assert byte-equality. The recursion handles nested objects (the `request` echo on RunStart is the only nested structure in Phase 1, but recursion is cheap insurance for future shape changes). This is the OUT-03 envelope-determinism contract regression gate."
    - "Pattern — clippy-friendly doc-comments in test files. The workspace `[lints]` inherit `pedantic = warn` which includes `doc-markdown` and `doc-lazy-continuation`. Bullet-list paragraphs in module-level `//!` doctrings need a blank `//!` line between the lead-in and the bullets; type-name mentions need backticks (`` `BTreeMap` ``, `` `HashMap` ``). Discovered while running `cargo clippy -p <crate> --all-targets -- -D warnings` against the new test files."

key-files:
  created:
    - "`crates/miner-core/tests/schema_roundtrip.rs` — 232 lines / 3 tests. Closes D-22 runtime schema validation for every Finding variant."
    - "`crates/miner-core/tests/config_precedence.rs` — 161 lines / 4 tests. Closes FOUND-05 production-grade precedence integration coverage."
    - "`crates/miner-cli/tests/cli_streams.rs` — 374 lines / 7 tests. Closes FOUND-02 (subprocess stdout/stderr split), OUT-01 (NDJSON shape), OUT-02 (per-line schema validation), OUT-03 (masked twice-run byte-identity — FULL closure)."
    - "`README.md` — 86 lines. Phase 1 Quickstart, Status, License, What-Phase-1-Delivers summary, Architecture pointer, Roadmap pointer."
  modified:
    - "`crates/miner-core/Cargo.toml` — added `serial_test = \"3\"` to `[dev-dependencies]` (documented why `tempfile` was contemplated but omitted — `figment::Jail` provides its own scratch directory)."
    - "`crates/miner-cli/Cargo.toml` — added `jsonschema.workspace = true` + `serde_json.workspace = true` to `[dev-dependencies]` for the cli_streams test."
    - "`Cargo.lock` — `serial_test 3.4.0` was already in the graph (miner-cli dev-dep from Plan 05); no new transitive deps."

key-decisions:
  - "**Rule 3 deviation — `figment::Jail` in lieu of raw `std::env::set_var` in `config_precedence.rs`.** The Plan 07 plan body explicitly proposed `std::env::set_var` / `remove_var` plus `#[serial_test::serial]`. The workspace lints set `unsafe_code = \"forbid\"` — a level that `#![allow(unsafe_code)]` cannot override — and Rust 2024 made env mutation `unsafe`. `figment::Jail` is the documented testing helper (already enabled via the `test` feature in miner-core's dev-deps) that wraps the unsafe internally and provides the same env-isolation semantics. Functionally identical regression coverage; preserves the `forbid(unsafe_code)` workspace invariant. Documented in the file's module-doc; the `#[serial_test::serial]` annotation stays as a belt-and-brace against Jail's already-process-wide lock."
  - "**OUT-03 closure is FULL, not partial.** The masked twice-run byte-identity test in `cli_streams.rs::emit_fixture_byte_identical_when_volatile_fields_masked` masks ONLY the four known-volatile fields (`run_id`, `started_at_utc`, `ended_at_utc`, `wall_clock_ms`) and asserts the remaining bytes match across two runs. Every other envelope field — key ordering, scalar values, nested map ordering inside `summary`/`per_scan`/`data_slice`, the `request` echo on RunStart — must be byte-stable for the test to pass. If it ever fails, the BTreeMap discipline somewhere collapsed (a HashMap snuck in, `serde_json/preserve_order` got unification-enabled, schemars insertion-order regressed). Plan 07 explicitly forbids 'OUT-03 partial' — this is the envelope-determinism contract closed."
  - "**Schema path resolution via `env!(\"CARGO_MANIFEST_DIR\")`.** Both `schema_roundtrip.rs` and `cli_streams.rs` resolve the schema artifact relative to the crate's manifest dir + `../../schemas/findings-v1.schema.json`. Robust against `cargo test` being run from anywhere; `env!` macro is compile-time and panic-free."
  - "**Preflight-failure tests scope env via `env_clear()` + selective `env(\"PATH\", ...)` + `env(\"XDG_CONFIG_HOME\", tempdir)` + `env(\"HOME\", tempdir)`.** Without isolating XDG_CONFIG_HOME and HOME, the `directories::ProjectDirs` lookup in `resolve_toml_path` would find the developer's real `~/.config/miner/miner.toml` if one existed, and the preflight test would mis-fire. Pointing both at the empty tempdir guarantees the CWD/XDG/explicit fallbacks all return None and the figment merge sees no sources at all (the `missing_required_config` path) or only the explicit `--config` file (the `invalid_config` path)."

patterns-established:
  - "Pattern — Integration tests cross-cut the public re-export surface, NOT internal modules. Plan 07 tests are the proof that `lib.rs`'s FROZEN re-exports (`pub use findings::{..}`, `pub use error::{..}`, `pub use config::{..}`) are sufficient to exercise every Phase 1 contract. A future phase that needs a new public name will see the integration test fail to compile — visible signal in code review."
  - "Pattern — Three-layer schema/discipline defence. Plan 04 banned `println!`/`eprintln!` at the workspace clippy.toml level (mechanical rejection). Plan 06 added the CI schema-sync diff (drift catch in PR). Plan 07 adds the runtime schema validation per variant (drift catch in `cargo test`). Adding a field to a Rust type without regenerating the schema OR changing it incompatibly fails BOTH the local test and the CI diff — the regression gates compound, they don't substitute."
  - "Pattern — Subprocess tests assert against the real OS process boundary. Unit tests can pass while the binary leaks stdout/stderr in subtle ways (a panic message escaping the panic-hook, a third-party crate `println!`ing, an init-before-tracing-subscriber print). `cli_streams.rs` runs the binary and asserts on `process::Command::output()` — catches anything no unit test can."

requirements-completed: [FOUND-01, FOUND-02, FOUND-03, FOUND-04, FOUND-05, OUT-01, OUT-02, OUT-03]
threats-mitigated: [T-01-02, T-01-03]

# Metrics
duration: 20min
completed: 2026-05-17
---

# Phase 01 Plan 07: Phase 1 Sign-Off Summary

**FOUND-02, FOUND-03, FOUND-05, OUT-01, OUT-02 and OUT-03 (FULL closure, not partial) all
land in this plan as automated integration tests. `cargo test --workspace` now exercises
the locked envelope schema against every `Finding` variant at runtime (D-22), proves the
CLI > env > TOML > error precedence works through the public re-export surface, spawns
the actual `miner` binary via `assert_cmd` and asserts stdout/stderr split + per-line
schema validation + masked twice-run byte-identity. README.md ships the Phase 1
Quickstart. All seven sign-off gates exit 0 locally. Phase 1 is complete.**

## Performance

- **Duration:** ~20 min
- **Started:** 2026-05-17T16:10:00Z (worktree-agent spawned)
- **Completed:** 2026-05-17T16:29Z (last task commit)
- **Tasks:** 3 (two `auto`, one `checkpoint:human-verify` — Task 3's automation side completed; the user-facing verification step is queued via this SUMMARY)
- **Files:** 4 created, 3 modified (excluding `Cargo.lock`)
- **Tests added:** 14 (3 schema_roundtrip + 4 config_precedence + 7 cli_streams)
- **Total workspace tests:** 43 (up from 29 in Plan 06)

## Accomplishments

### Task 1 — `schema_roundtrip` + `config_precedence` integration tests

- `crates/miner-core/tests/schema_roundtrip.rs` (3 tests):
  - `all_kinds_validate`: constructs one instance of each of the five Finding variants
    PLUS the "Result with raw" / "Result without raw" pair (six total instances).
    Loads the committed `schemas/findings-v1.schema.json` via `jsonschema::validator_for`
    and asserts every instance validates with zero errors. The schema path is resolved
    via `env!("CARGO_MANIFEST_DIR")` + `../../schemas/`.
  - `dsr_and_fdr_q_present_as_null_in_v1`: serialises a Result finding, parses the JSON,
    and asserts `dsr` and `fdr_q` are **present** in the top-level object map and **each
    is `Value::Null`** — NOT absent. Uses `obj.contains_key()` to distinguish presence
    from `Value::Null` from `serde_json::Value::Index`'s "missing key returns Null"
    behaviour.
  - `raw_array_content_encoding_path_works`: builds a Result with a populated
    `raw.series` (two arrays — `timestamps_ms` per D-03 and `returns`), validates, and
    additionally asserts the produced JSON for `raw.series.timestamps_ms.data` is a
    `string` (the base64-encoded form, exercising the `Base64Bytes` `contentEncoding`
    schema fragment).
- `crates/miner-core/tests/config_precedence.rs` (4 tests):
  - `cli_wins_over_env_and_toml`: all three layers populated for each of the three
    fields; CLI must win. Uses `OutputDest::File(PathBuf)` for `output` to exercise
    the non-default enum variant.
  - `env_wins_when_cli_omitted`: TOML + env present, CLI defaults; env must win.
    Verifies that fields NOT covered by env survive from TOML.
  - `toml_wins_when_only_source`: TOML only; TOML value flows through.
  - `missing_required_yields_err`: no source; `figment.extract::<MinerConfig>()` errors
    and the message mentions `cache_root` or "missing".
- All four `config_precedence` tests are `#[serial_test::serial]` and use
  `figment::Jail::expect_with(|jail| { ... })` for env isolation (see Rule 3 deviation
  below).
- Added `serial_test = "3"` to `miner-core/Cargo.toml [dev-dependencies]`.

### Task 2 — `cli_streams` subprocess integration test (OUT-03 full closure)

- `crates/miner-cli/tests/cli_streams.rs` (7 tests):
  - `emit_fixture_writes_two_jsonl_lines_to_stdout`: spawn `miner emit-fixture`,
    exit 0, exactly 2 newlines on stdout, both lines parse as JSON, first
    `kind: run_start`, second `kind: run_end`.
  - `emit_fixture_writes_tracing_to_stderr_not_stdout`: stderr contains
    `"emitting fixture"`; stdout does NOT (T-01-03 regression gate).
  - `emit_fixture_run_ids_match_across_envelopes`: 26-char ULID shared across both
    envelopes (regression gate for the `RunId: Copy` reuse pattern from Plan 03).
  - `emit_fixture_validates_against_committed_schema`: loads the committed schema,
    validates each stdout line, all errors empty.
  - `preflight_missing_config_emits_wireerror_to_stderr_exit_1`: `env_clear()` +
    empty tempdir CWD + tempdir XDG + tempdir HOME → exit 1, stdout empty, stderr
    JSON line `code: "missing_required_config"` (D-06, D-07).
  - `preflight_invalid_toml_emits_invalid_config_code`: write a TOML with
    `cache_root = 42` (integer for path) → exit 1, stdout empty, stderr JSON line
    `code: "invalid_config"` (NOT `"missing_required_config"`). BLOCKER regression
    gate for Plan 05's `classify_figment_error` mapper.
  - **`emit_fixture_byte_identical_when_volatile_fields_masked` (OUT-03 FULL
    closure)**: run emit-fixture twice; for each stdout buffer, parse each line as
    JSON, recursively mask `run_id` / `started_at_utc` / `ended_at_utc` to sentinel
    strings + `wall_clock_ms` to `0`, re-serialise with `serde_json::to_string`
    (compact form), assert byte-equality between the two masked vectors. Pass means
    every non-volatile envelope field is already deterministic.
- Added `jsonschema.workspace = true` + `serde_json.workspace = true` to
  `miner-cli/Cargo.toml [dev-dependencies]`.
- `cargo test -p miner-cli --test cli_streams` exits 0; all 7 tests pass first try.

### Task 3 — README Quickstart + four-gate local CI walk-through

- `README.md` (86 lines) at workspace root. Sections:
  - Title + 1-paragraph description + pipeline diagram.
  - Status: Phase 1 complete; Phase 2 in progress.
  - License: Apache-2.0 — pointers to `LICENSE` and `NOTICE`.
  - **Quickstart**: 5 steps. Prerequisites (Rust 1.85+, git) → clone + `cargo build`
    → emit-fixture (sets the three MINER_* env vars inline) → `jq` schema inspection
    → `cargo test` / `cargo clippy` sanity. Each block uses `sh` fenced syntax for
    syntax highlighting.
  - **What Phase 1 Delivers**: 6 bullets tied to FOUND-01..05 + OUT-01..03.
  - Architecture pointer (`.planning/research/ARCHITECTURE.md`).
  - Roadmap pointer (`.planning/ROADMAP.md`).
- **Four-gate (plus three-extra) local sign-off ritual** completed locally against this
  commit tree:
  1. **Gate 1 (build)**: `cargo build --workspace --all-targets` → exit 0.
  2. **Gate 2 (clippy)**: `cargo clippy --workspace --all-targets -- -D warnings` → exit 0.
  3. **Gate 3 (tokio-free miner-core)**: the exact PROHIBITED regex from RESEARCH
     §Tokio-Free Gate run against `cargo tree -p miner-core --edges normal,build --prefix none`
     → "ok: miner-core has zero async-runtime dependencies".
  4. **Gate 4 (schema-sync)**: `cargo run -p xtask -- gen-schema` regenerates
     `schemas/findings-v1.schema.json`; `git diff --exit-code schemas/findings-v1.schema.json`
     → exit 0 (no diff).
  5. **Gate 5 (tests)**: `cargo test --workspace --no-fail-fast` → 43 tests pass
     (29 miner-core unit + 4 config_precedence + 3 schema_roundtrip + 7 cli_streams;
     doc-tests + wrapper unit-tests all 0-pass clean).
  6. **Gate 6 (fmt)**: `cargo fmt --all -- --check` → exit 0 (after a fmt cleanup
     pass that collapsed multi-line expressions in the new test files; see commit
     `4a37752`).
  7. **Gate 7 (emit-fixture smoke)**: `cargo run -p miner-cli -- emit-fixture` with
     `MINER_CACHE_ROOT=/tmp/cache MINER_BAR_CACHE_ROOT=/tmp/bar MINER_OUTPUT=stdout`
     → exit 0; stdout contains 2 NDJSON lines (`run_start` then `run_end`, same
     `run_id`, valid against schema); stderr has the tracing log line.

## Task Commits

Each task was committed atomically. The READMEad sign-off commit (Task 3) also
contains the rustfmt cleanup on Tasks 1 + 2 — fmt was discovered failing during the
sign-off ritual; the cleanup is whitespace-only, no semantic change.

1. **Task 1** — `test(01-07): add schema_roundtrip + config_precedence integration tests`
   → commit `68a86ec`.
2. **Task 2** — `test(01-07): add cli_streams subprocess integration test (OUT-03 full closure)`
   → commit `52ca7f8`.
3. **Task 3** — `docs(01-07): add README quickstart + Phase 1 sign-off (fmt cleanup)`
   → commit `4a37752`.

The plan-metadata commit (this SUMMARY.md) lands separately as the final commit of the plan.

## Files Created / Modified

**Created:**
- `crates/miner-core/tests/schema_roundtrip.rs` (232 lines)
- `crates/miner-core/tests/config_precedence.rs` (161 lines)
- `crates/miner-cli/tests/cli_streams.rs` (374 lines)
- `README.md` (86 lines)

**Modified:**
- `crates/miner-core/Cargo.toml` (added `serial_test = "3"` dev-dep + docstring)
- `crates/miner-cli/Cargo.toml` (added `jsonschema.workspace` + `serde_json.workspace`
  dev-deps + docstring)
- `Cargo.lock` (no new transitive deps; `serial_test 3.4.0` already present)

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 — Blocking issue] `figment::Jail` substituted for raw `std::env::set_var` in `config_precedence.rs`**

- **Found during:** Task 1, first compile.
- **Issue:** Plan 07 body said `Use std::env::set_var/remove_var ... but EVERY env-touching test MUST be annotated #[serial_test::serial]`. The workspace lint config has `unsafe_code = "forbid"` at the `[workspace.lints.rust]` level. `forbid` cannot be downgraded by `#![allow(unsafe_code)]`. Rust 2024 made `std::env::set_var` `unsafe` to call. Result: the test couldn't compile.
- **Fix:** Used `figment::Jail::expect_with(|jail| { jail.set_env(..); jail.create_file(..); ... })` instead. Jail wraps the env mutation in its own unsafe block inside the figment crate (behind its `test` feature, which is already enabled in miner-core's dev-deps). Functionally equivalent: Jail is a process-wide RAII lock that snapshots env on enter and restores on drop; `#[serial_test::serial]` remains as belt-and-brace.
- **Files modified:** `crates/miner-core/tests/config_precedence.rs` (use `figment::Jail`), `crates/miner-core/Cargo.toml` (omit unused `tempfile` dev-dep, document why).
- **Commit:** `68a86ec`.
- **Justification:** The plan's intent was env-isolated precedence testing; the choice between `std::env::set_var` and `figment::Jail` is a how-to detail, not a what. Jail preserves the `forbid(unsafe_code)` workspace invariant (which is itself a security-hygiene contract worth more than the plan's specific recipe). Same regression coverage; same parallel-test-safety. Documented in the file's module doc-comment.

**2. [Rule 3 — Blocking issue] rustfmt cleanup on tests/schema_roundtrip.rs and tests/cli_streams.rs**

- **Found during:** Task 3 Gate 6 sign-off.
- **Issue:** A handful of expressions in the new test files exceeded rustfmt's max-width budget by single characters. `cargo fmt --all -- --check` failed with a multi-file diff.
- **Fix:** Ran `cargo fmt --all`. Whitespace-only changes; collapsed long lines into multi-line form. Re-ran all gates afterwards — clippy, tests, fmt-check, emit-fixture all exit 0.
- **Files modified:** `crates/miner-core/tests/schema_roundtrip.rs`, `crates/miner-cli/tests/cli_streams.rs`.
- **Commit:** Bundled into `4a37752` (the Task 3 README commit) with explicit fmt-cleanup mention.
- **Justification:** Pure formatting; no logic change. Gate 6 is mandatory; running fmt was the only way to clean it.

**3. [Rule 2 — Critical functionality] Preflight tests isolate `HOME` and `XDG_CONFIG_HOME`**

- **Found during:** Task 2 design.
- **Issue:** The CLI's `resolve_toml_path` uses `directories::ProjectDirs::from("", "", "miner").config_dir().join("miner.toml")`. If the test environment leaks `HOME` (typical) the developer's real `~/.config/miner/miner.toml` could supply config values, masking the preflight failure.
- **Fix:** Both preflight tests in `cli_streams.rs` set `XDG_CONFIG_HOME` AND `HOME` to the same empty tempdir via `env(...)` on the `assert_cmd::Command`. Combined with `env_clear()` + selective `PATH` re-injection, the subprocess sees a completely sanitised env where NO `miner.toml` can be discovered from any platform-default location.
- **Files modified:** `crates/miner-cli/tests/cli_streams.rs` (preflight tests).
- **Commit:** `52ca7f8`.
- **Justification:** Without the isolation, the preflight tests are flaky against the developer's actual machine. The XDG/HOME isolation is correctness functionality the plan body did not explicitly mandate but is required for the tests to be a real regression gate.

No architectural deviations (no Rule 4 events). No auth gates. Plan 07's intent —
codify Phase 1's contracts as automated regression tests + ship a Quickstart — held
exactly; only the implementation recipe for env isolation was adjusted to respect the
workspace's `forbid(unsafe_code)` invariant.

## Phase 1 Decision Audit Trail (D-01..D-24)

> Per the Plan 07 `<output>` requirement, every CONTEXT.md decision and its closing plan.

| ID | Decision | Closed by |
|----|----------|-----------|
| **D-01** | Raw arrays always base64 LE-f64 | Plan 03 (`Base64Bytes` newtype + serde Serialize/Deserialize) |
| **D-02** | Self-contained array objects `{data, shape, dtype}` | Plan 03 (`RawArray` struct in `findings/mod.rs`) |
| **D-03** | Every raw payload includes `timestamps_ms` | Plan 03 (`Raw::new` constructor enforces invariant) |
| **D-04** | Clean input/output split: `raw.series` (inputs) vs `effect.extra` (outputs) | Plan 03 (`Raw`, `Effect.extra` both `BTreeMap<String, RawArray>`) |
| **D-05** | Mid-stream errors → `kind: scan_error`, sweep continues | Plan 03 (`ScanErrorFinding` variant + `ScanErrorCode` enum) |
| **D-06** | Pre-flight errors → stderr WireError JSON; stdout empty | Plan 03 (`WireError`) + Plan 04 (`stderr_emit::emit_to_stderr`) + Plan 05 (CLI classifier) + **Plan 07 (subprocess regression gate)** |
| **D-07** | Three-tier exit codes (0/1/2); Phase 1 closes the 0 and 1 paths | Plan 05 (`emit_fixture` exits 0; preflight exits 1) + **Plan 07 (cli_streams subprocess assertion)** |
| **D-08** | Gap-policy outputs are findings, not errors | Plan 03 (`GapAbortedFinding` variant) — payload finalised in Phase 2 |
| **D-09** | Always emit `run_start` + `run_end` framing records | Plan 03 (`RunStart` / `RunEnd` variants; framing records omit the seven locked envelope fields) + Plan 05 (emit_fixture) |
| **D-10** | `run_id` is an always-unique time-prefixed ULID | Plan 03 (`RunId(Ulid)` newtype with `Copy`) |
| **D-11** | Rich framing payloads (`run_start` carries `request`; `run_end` carries `RunSummary`) | Plan 03 (struct definitions) + Plan 05 (`emit_fixture` populates them) |
| **D-12** | Publish `schemas/findings-v1.schema.json` as checked-in artifact | Plan 06 (xtask gen-schema writes it; CI sync-gate diffs it) |
| **D-13** | Schema derived from Rust types via schemars | Plan 06 (xtask uses `schemars::schema_for!(Finding)`) + Plan 03 (all envelope structs derive `JsonSchema`) |
| **D-14** | One schema file per major `schema_version` | Plan 06 (file naming convention established: `findings-v1.schema.json`) |
| **D-15** | stdout discipline mechanism: `clippy::disallowed_macros` + sanctioned writers | Plan 04 (`clippy.toml` + `StdoutSink` + `stderr_emit`) — surgical-correction note: sanctioned writers do NOT carry `#[allow]` because they use `io::Write` directly, not banned macros |
| **D-16** | Config crate (figment) + TOML format + `MINER_*` env + CLI > env > TOML precedence | Plan 03 (`MinerConfig` type + `OutputDest` enum) + Plan 05 (`build_figment`, `CliOverrides`, XDG/CWD `resolve_toml_path`) + **Plan 07 (production-grade integration test for precedence)** |
| **D-17** | Time crate: chrono 0.4+, UTC internally | Plan 01 (workspace dep) + Plan 03 (`DateTime<Utc>` on every envelope timestamp) |
| **D-18** | Error model: thiserror in core, anyhow in wrappers | Plan 03 (`MinerError`, `WireError`, `PreflightCode`, `ScanErrorCode`) |
| **D-19** | Stdout writer pattern: `StdoutSink` is the only writer to io::stdout | Plan 04 (`StdoutSink` + `FindingSink` trait) + **Plan 07 (subprocess test asserts stdout has ONLY JSON, never tracing)** |
| **D-20** | Workspace conventions: edition 2024, MSRV 1.85, resolver 3 (surgical correction from CONTEXT.md "resolver=2") | Plan 01 (`Cargo.toml` + `rust-toolchain.toml`) |
| **D-21** | CI provider: GitHub Actions; four mandatory gates | Plan 06 (`.github/workflows/ci.yml` with build/clippy/tokio-tree/schema-sync) + **Plan 07 (seven-gate local sign-off ritual proves all gates exit 0)** |
| **D-22** | Schema validation in CI: cargo test loads `schemas/findings-v1.schema.json` and validates one of each variant | **Plan 07 (`crates/miner-core/tests/schema_roundtrip.rs`)** |
| **D-23** | `miner-bench` scaffolding (empty crate in Phase 1) | Plan 01 (empty `crates/miner-bench`) |
| **D-24** | ULID crate: `ulid` for `run_id` generation | Plan 01 (workspace dep) + Plan 03 (`RunId(Ulid)` newtype) |

## Phase 1 Requirement Audit Trail (FOUND-01..05, OUT-01..03)

| ID | Requirement (one-line) | Closed by |
|----|------------------------|-----------|
| **FOUND-01** | Cargo workspace with `miner-core` + 3 wrapper bins | Plan 01 (seven-crate workspace established) |
| **FOUND-02** | stdout=findings, stderr=logs; CI-enforced via clippy | Plan 04 (`clippy.toml`, `StdoutSink`, `stderr_emit`) + Plan 05 (CLI wires tracing→stderr) + Plan 06 (CI clippy gate) + **Plan 07 (subprocess regression gate)** |
| **FOUND-03** | Locked Finding envelope JSON schema with reproducibility fields | Plan 03 (envelope types) + Plan 06 (xtask gen-schema + committed artifact + CI diff gate) + **Plan 07 (runtime schema validation per variant — D-22)** |
| **FOUND-04** | Scan engine pure sync + rayon; async only in wrapper edges | Plan 01 (workspace deps preclude tokio in miner-core) + Plan 06 (CI tokio-tree grep gate) |
| **FOUND-05** | Config precedence: CLI > env > TOML > error; zero hardcoded paths in library | Plan 03 (`MinerConfig` type) + Plan 05 (`build_figment` + library-no-hardcoded-paths grep gate) + **Plan 07 (production-grade integration test covering all three required fields and the four precedence cases)** |
| **OUT-01** | NDJSON / JSONL on stdout | Plan 04 (`StdoutSink::write_envelope` writes one JSON + `\n` per envelope with per-envelope flush) + Plan 05 (emit_fixture demonstrates) + **Plan 07 (subprocess test asserts exactly 2 lines, both valid JSON, both parse)** |
| **OUT-02** | Every finding carries the seven locked envelope fields + reserved-but-null DSR/FDR-q | Plan 03 (struct definitions with `dsr: Option<f64>` / `fdr_q: Option<f64>` and no `skip_serializing_if`) + **Plan 07 (`dsr_and_fdr_q_present_as_null_in_v1` regression gate)** |
| **OUT-03** | Deterministic output ordering (Phase 1 scope: envelope determinism) | Plan 01 (`serde_json` pin without `preserve_order`) + Plan 03 (`BTreeMap` discipline throughout) + Plan 06 (xtask determinism pipeline; twice-run cmp) + **Plan 07 (`emit_fixture_byte_identical_when_volatile_fields_masked` — FULL closure for envelope determinism; scan-content determinism is correctly scoped to Phase 3+)** |

## Threat Mitigation Audit

| Threat | Category | Mitigation |
|--------|----------|-----------|
| **T-01-02** | Tampering / Integrity — schema drift | Plan 06 CI sync gate (regenerate + diff) + **Plan 07 runtime validation (`schema_roundtrip.rs` + `cli_streams.rs` per-line schema check)**. Two-layer defence: a contributor who changes a Rust type without regenerating the schema fails BOTH the CI diff and the local `cargo test`. |
| **T-01-03** | Tampering — stdout/stderr discipline | Plan 04 workspace `clippy::disallowed_macros` (mechanical rejection of banned macros) + Plan 04 sanctioned-writer pattern (`StdoutSink` + `stderr_emit`) + **Plan 07 `cli_streams::emit_fixture_writes_tracing_to_stderr_not_stdout` (regression gate at the OS process boundary)**. |

## Verification — Phase 1 Sign-Off Ritual

All seven gates exit 0 against the post-`4a37752` tree:

```text
Gate 1 build         : cargo build --workspace --all-targets        → exit 0
Gate 2 clippy        : cargo clippy --workspace --all-targets -- -D warnings → exit 0
Gate 3 tokio-free    : cargo tree -p miner-core --edges normal,build → "ok: miner-core has zero async-runtime dependencies"
Gate 4 schema-sync   : cargo run -p xtask -- gen-schema && git diff --exit-code schemas/findings-v1.schema.json → exit 0
Gate 5 tests         : cargo test --workspace --no-fail-fast → 43 passed; 0 failed
Gate 6 fmt           : cargo fmt --all -- --check → exit 0
Gate 7 emit-fixture  : MINER_CACHE_ROOT=/tmp/cache MINER_BAR_CACHE_ROOT=/tmp/bar MINER_OUTPUT=stdout cargo run -p miner-cli -- emit-fixture → exit 0; 2 NDJSON lines on stdout (run_start + run_end sharing run_id); tracing line on stderr
```

The plan's `checkpoint:human-verify` (Task 3) is structurally a developer-experience
gate on the README's readability — the underlying automation is complete and green.
The orchestrator should surface this SUMMARY to the user for confirmation that the
README copy is acceptable and Phase 1 is declared complete.

## Self-Check: PASSED

All claims verified.

**Files created exist:**

- `.planning/phases/01-foundations-contracts/01-07-PLAN.md` (read)
- `crates/miner-core/tests/schema_roundtrip.rs` (created in commit `68a86ec`)
- `crates/miner-core/tests/config_precedence.rs` (created in commit `68a86ec`)
- `crates/miner-cli/tests/cli_streams.rs` (created in commit `52ca7f8`)
- `README.md` (created in commit `4a37752`)

**Commits exist:**

- `68a86ec` — Task 1 schema_roundtrip + config_precedence
- `52ca7f8` — Task 2 cli_streams subprocess test (OUT-03 full closure)
- `4a37752` — Task 3 README + fmt cleanup

**Tests run green:**

- `cargo test --workspace --no-fail-fast` → 43 tests pass (0 failed)
- `cargo clippy --workspace --all-targets -- -D warnings` → exit 0
- `cargo fmt --all -- --check` → exit 0
- All seven sign-off gates green (see Verification section)
