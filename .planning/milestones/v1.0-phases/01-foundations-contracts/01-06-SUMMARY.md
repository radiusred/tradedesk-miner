---
phase: 01-foundations-contracts
plan: 06
subsystem: foundations
tags: [rust, xtask, schemars, json-schema, ci, github-actions, clippy, tokio-tree, schema-sync, determinism, found-03, found-04, threat-mitigation-t-01-02, threat-mitigation-t-01-03]

# Dependency graph
requires: [plan-01-01, plan-01-02, plan-01-03, plan-01-04, plan-01-05]
provides:
  - "`xtask gen-schema` subcommand — clap-driven dispatcher that walks `schemars::schema_for!(Finding)` and writes `schemas/findings-v1.schema.json` through a `serde_json::to_value` → `to_string_pretty` normalisation pipeline guaranteeing byte-deterministic output across invocations (D-13)"
  - "`schemas/findings-v1.schema.json` — committed public contract artifact (15 494 bytes, 564 lines). Top-level `$schema = https://json-schema.org/draft/2020-12/schema`, `title = Finding`, `oneOf` enumerates the five `kind` consts (`run_start`, `result`, `scan_error`, `gap_aborted`, `run_end`). `$defs` contains the 16 supporting types: `Base64Bytes` (with `contentEncoding: base64` + `contentMediaType: application/octet-stream`), `DataSlice`, `Dtype` (enum `[f64]`), `Effect`, `GapAbortedFinding`, `PerScanCounts`, `Raw`, `RawArray`, `ResultFinding`, `RunEnd`, `RunId` (with the Crockford-base32 ULID `^[0-9A-HJKMNP-TV-Z]{26}$` pattern), `RunStart`, `RunSummary`, `ScanErrorFinding`, `Source`, `TimeRange`. (D-12, D-14)"
  - "`.github/workflows/ci.yml` — single `build-and-test` job under `ubuntu-latest` with seven explicit steps: checkout, `dtolnay/rust-toolchain@stable` with `toolchain: \"1.85\"` + `components: clippy, rustfmt`, then the four mandatory D-21 gates plus fmt + test hygiene. Action pins are major-version tags (`@v4`, `@stable`) for Phase 1; SHA-pinning lands in Phase 7 hardening."
  - "**Gate 1 (build):** `cargo build --workspace --all-targets`"
  - "**Gate 2 (clippy / stdout discipline):** `cargo clippy --workspace --all-targets -- -D warnings` — leverages Plan 04's clippy.toml `disallowed-macros` to mechanically reject `println!`/`eprintln!` outside the two sanctioned writers; mitigates T-01-03"
  - "**Gate 3 (tokio-free miner-core):** the exact PROHIBITED regex from RESEARCH §Tokio-Free Gate (`^(tokio|tokio-[^ ]+|async-std|async-std-[^ ]+|smol|smol-[^ ]+|async-trait|async-io|async-channel|async-executor|async-task)$`) run against `cargo tree -p miner-core --edges normal,build --prefix none`; enforces FOUND-04"
  - "**Gate 4 (schema sync):** `cargo run -p xtask -- gen-schema && git diff --exit-code schemas/findings-v1.schema.json` — fails the build if a contributor mutates Rust envelope types without regenerating the schema; mitigates T-01-02"
  - "Twice-run byte-deterministic schema regeneration proven before commit (`cmp` returns 0 between two invocations into separate paths) — the regression armour that makes Gate 4 flake-free across runners and OSes"
affects: [plan-01-07, phase-02, phase-03, phase-04, phase-05, phase-06, phase-07]

# Tech tracking
tech-stack:
  added: []  # no new dependencies; uses workspace clap + anyhow + serde_json + schemars (already in xtask/Cargo.toml since Plan 01-01) and the github-hosted dtolnay/rust-toolchain action
  patterns:
    - "Three-step determinism pipeline for serde_json output: (1) `schemars::schema_for!` produces a `Schema` (stable derive walk via schemars 1.x), (2) round-trip through `serde_json::to_value` to land in the `BTreeMap`-backed `Map` (workspace serde_json pin deliberately omits the `preserve_order` feature), (3) `serde_json::to_string_pretty` emits keys alphabetically. Trailing newline appended on write. Provably byte-stable."
    - "xtask command shape: clap derive `Parser` + `Subcommand` enum with one variant per future command (`GenSchema { out: PathBuf }` is the only one in Phase 1). `out` defaults to `schemas/findings-v1.schema.json` for the common case; `cargo run -p xtask -- gen-schema /tmp/foo.json` enables the second-path determinism cmp without ENV/cwd gymnastics."
    - "Two-tier CI gate naming: gates are named with the exact phrase that grep-able tests check for (`tokio-free miner-core`, `schema sync`). The Task 2 verify block uses `grep -q 'tokio-free miner-core'` and `grep -q 'schema sync'` against `.github/workflows/ci.yml`, so renaming a step would break the gate-presence assertion before CI even runs."
    - "Gate 4 schema-sync regression armour: the determinism pipeline + Task 1's twice-run `cmp` acceptance gate together mean Gate 4 is reliable enough to fail the build only on REAL drift. A flake on this gate signals a real bug (e.g., someone adding `serde_json/preserve_order` to a downstream crate, propagating through unification) — and the comment in the workspace `Cargo.toml` already calls that out."

key-files:
  created:
    - ".github/workflows/ci.yml — 88-line single-job workflow; the four mandatory gates from D-21 plus fmt + test"
    - "schemas/findings-v1.schema.json — 15 494 bytes / 564 lines; the FROZEN v1 envelope contract"
  modified:
    - "xtask/src/main.rs — replaced the Plan 01-01 placeholder with the real clap dispatcher + `gen_schema` function (72 lines incl. doc comments); retains the crate-level `#![allow(clippy::disallowed_macros)]` from Plan 01-01 / Plan 04 (audited xtask exemption — dev-only command runner, never shipped)"
    - "crates/miner-bench/src/main.rs, crates/miner-core/src/{config/mod.rs, error/codes.rs, error/stderr_emit.rs, findings/sink.rs, lib.rs}, crates/miner-http/src/main.rs, crates/miner-mcp/src/main.rs — `cargo fmt --all` applied (Rule 3 deviation; see below). Whitespace/wrapping only, no semantic changes."

key-decisions:
  - "D-13 honoured: the xtask subcommand walks `miner_core::Finding` via `schemars::schema_for!(Finding)`, NOT a build.rs or runtime path. xtask is dev-only — schema regeneration is a manual/CI concern, not a per-build cost. RESEARCH §Schema Derivation Strategy explicitly favoured this approach over build.rs (the build.rs alternative would have run schemars on every compile, slowing the dev loop)."
  - "Determinism pipeline implemented as the THREE compounding guarantees the plan calls out, not just one: (1) `serde_json` pin (workspace `Cargo.toml`), (2) schemars 1.x stable derive walk (Plan 01-02 spike A1), (3) `to_value → to_string_pretty` normalisation in xtask. The third is the BELT-AND-BRACES — even if (1) or (2) silently regress (e.g., a downstream crate enables `serde_json/preserve_order` and feature unification propagates), the explicit `Value` round-trip collapses the output back to BTreeMap order. Twice-run `cmp` proves the pipeline holds end-to-end before the schema is committed."
  - "Action toolchain pin: `dtolnay/rust-toolchain@stable` with `toolchain: \"1.85\"` (as a YAML-quoted string so the parser doesn't elide the trailing `.0`, e.g., `toolchain: 1.85` numerically would be `1.85` but `1.10` would silently become `1.1`). Components are `clippy, rustfmt` because Gate 2 needs clippy and the fmt hygiene step needs rustfmt. This is consistent with `rust-toolchain.toml` (channel `1.85`, components `[clippy, rustfmt]`, profile `minimal`); the explicit Action input wins if there is any drift but in practice both layers agree."
  - "`gen_schema` takes `&PathBuf` (not `PathBuf` by value) per clippy::needless-pass-by-value pedantic lint. The signature change is internal-only (private function in xtask/src/main.rs) so it has zero contract surface."
  - "Action SHA-pinning EXPLICITLY DEFERRED to Phase 7 hardening (called out in the workflow file's leading comment and in the plan body). Phase 1 uses `actions/checkout@v4` and `dtolnay/rust-toolchain@stable` as major-version tags. The threat model section already classifies this as accepted risk for Phase 1."

patterns-established:
  - "Pattern — Byte-deterministic generated artifact: a generator binary (xtask) writes a committed file; CI diffs the regenerated file against the committed copy. Robustness requires the generator to be deterministic — guaranteed here by an explicit `serde_json::Value` round-trip, NOT by trusting the upstream library's internal map ordering. The pattern generalises to any future generated artifact in this repo (e.g., a CLI help-text snapshot, a generated config skeleton)."
  - "Pattern — xtask as the home for dev-only commands: the matklad xtask pattern owns commands that don't belong in `build.rs` (because they shouldn't run every build) and don't belong in a product binary (because they're not user-facing). Phase 1 establishes `gen-schema`; future plans add `verify-schema`, `release`, etc. The `#![allow(clippy::disallowed_macros)]` exemption is scoped to xtask only."
  - "Pattern — CI step names that double as grep tags: workflow step names are stable identifiers that grep-able verification can lock onto. The Task 2 verify block leans on this (`grep -q 'tokio-free miner-core' .github/workflows/ci.yml`). Renaming a step is a contract change in this design."

requirements-completed: [FOUND-03, FOUND-04]
threats-mitigated: [T-01-02, T-01-03]

# Metrics
duration: 22min
completed: 2026-05-17
---

# Phase 01 Plan 06: xtask gen-schema + 4-gate CI Summary

**FOUND-03 (locked envelope JSON schema with single source of truth via schemars) and FOUND-04 (CI-enforced async-runtime-free miner-core) both land in this plan. `cargo run -p xtask -- gen-schema` now produces a byte-deterministic `schemas/findings-v1.schema.json` (proven by twice-run `cmp` against a separate path); `.github/workflows/ci.yml` runs the four mandatory D-21 gates — build, clippy with `-D warnings`, tokio-tree grep against miner-core, and schema-sync diff — plus fmt + test hygiene; T-01-02 (schema drift) and T-01-03 (stdout pollution) are now mechanically enforced on every PR.**

## Performance

- **Duration:** 22 min
- **Started:** 2026-05-17T14:45:00Z
- **Completed:** 2026-05-17T15:07:00Z
- **Tasks:** 2 (both type=auto, no-tdd)
- **Files:** 2 created, 9 modified
- **Schema size:** 15 494 bytes / 564 lines

## Accomplishments

### Task 1 — `xtask gen-schema` + committed schema artifact

- Replaced the Plan 01-01 placeholder in `xtask/src/main.rs` with a clap `Parser`/`Subcommand` dispatcher and the real `gen_schema(&PathBuf) -> anyhow::Result<()>` function. Retained the `#![allow(clippy::disallowed_macros)]` exemption (xtask is dev-only; eprintln-for-feedback is the audited exception).
- Implemented the THREE-step determinism pipeline: `schemars::schema_for!(Finding)` → `serde_json::to_value(&schema)?` (lands in `BTreeMap`-backed `Map`) → `serde_json::to_string_pretty(&value)?` (alphabetic output) + trailing `\n`. The intermediate `Value` round-trip is the explicit guarantor — even if schemars' internal map happens to be insertion-ordered, the round-trip collapses it.
- Generated and committed `schemas/findings-v1.schema.json` (15 494 bytes). `$schema = https://json-schema.org/draft/2020-12/schema` (schemars 1.x default), `title = Finding`, top-level `oneOf` of length 5 (one per variant). `$defs` contains 16 supporting types: `Base64Bytes` (with `contentEncoding: base64`), `DataSlice`, `Dtype` (`enum: [f64]`), `Effect`, `GapAbortedFinding`, `PerScanCounts`, `Raw`, `RawArray`, `ResultFinding`, `RunEnd`, `RunId` (with the Crockford-base32 ULID pattern `^[0-9A-HJKMNP-TV-Z]{26}$`), `RunStart`, `RunSummary`, `ScanErrorFinding`, `Source`, `TimeRange`.
- Twice-run byte-determinism gate passed BEFORE commit: `cargo run -p xtask -- gen-schema` into the canonical path, then `cargo run -p xtask -- gen-schema /tmp/findings-v1-second.schema.json`, then `cmp schemas/findings-v1.schema.json /tmp/findings-v1-second.schema.json` returned exit 0. This is the Plan 06 BLOCKER-class regression armour that lets Gate 4 be reliable across runners.
- Python-level schema content verification passed: all 5 variant `kind` consts present (`run_start`, `result`, `scan_error`, `gap_aborted`, `run_end`); `contentEncoding` present (Base64Bytes derived correctly); ULID pattern `[0-9A-HJKMNP-TV-Z]` + `{26}` present (RunId derived correctly); all 16 expected `$defs` entries present.

### Task 2 — `.github/workflows/ci.yml` with four mandatory D-21 gates

- Created `.github/workflows/ci.yml` (88 lines) as a single `build-and-test` job under `ubuntu-latest`. Seven explicit steps after checkout + toolchain install: Gate 1 build, Gate 2 clippy, fmt hygiene, test hygiene, Gate 3 tokio-tree, Gate 4 schema-sync.
- Gate 3's PROHIBITED regex is the exact string from RESEARCH §Tokio-Free Gate: `^(tokio|tokio-[^ ]+|async-std|async-std-[^ ]+|smol|smol-[^ ]+|async-trait|async-io|async-channel|async-executor|async-task)$`. `--edges normal,build` deliberately excludes dev edges so tests in the wrapper crates can use a tokio runtime to exercise the bridge code without tripping the gate.
- Gate 4 runs `cargo run -p xtask -- gen-schema` and asserts `git diff --exit-code schemas/findings-v1.schema.json` — fails the build the instant a Rust type changes without a corresponding schema regen. Robustness backed by Task 1's twice-run determinism armour.
- All four gates + fmt + tests exit 0 locally on the post-commit tree (verified via the Task 2 verify block + an explicit replay of all five command invocations).
- Action pins are major-version tags (`@v4`, `@stable`) — Phase 7 hardening pass will pin SHAs. Toolchain is `dtolnay/rust-toolchain@stable` with `toolchain: "1.85"` (YAML string, defensive against numeric `1.10` → `1.1` elision) and `components: clippy, rustfmt`. Consistent with the workspace `rust-toolchain.toml`.

## Task Commits

Each task was committed atomically:

1. **Task 1: xtask gen-schema + committed schema artifact + twice-run determinism gate** — `b460648` (feat)
2. **Task 2: GitHub Actions CI workflow with four mandatory D-21 gates** — `58272a9` (feat)

_Plan-metadata commit (the SUMMARY itself) lands separately after this file is written._

## Files Created/Modified

**Created:**

- `.github/workflows/ci.yml` — 88-line CI workflow (4 mandatory gates + fmt + test)
- `schemas/findings-v1.schema.json` — 15 494 bytes / 564 lines, the FROZEN v1 contract

**Modified:**

- `xtask/src/main.rs` — replaced placeholder with clap dispatcher + `gen_schema` (72 lines)
- `crates/miner-bench/src/main.rs` — `cargo fmt` applied (Rule 3 deviation)
- `crates/miner-core/src/config/mod.rs` — `cargo fmt` applied
- `crates/miner-core/src/error/codes.rs` — `cargo fmt` applied
- `crates/miner-core/src/error/stderr_emit.rs` — `cargo fmt` applied
- `crates/miner-core/src/findings/sink.rs` — `cargo fmt` applied
- `crates/miner-core/src/lib.rs` — `cargo fmt` applied
- `crates/miner-http/src/main.rs` — `cargo fmt` applied
- `crates/miner-mcp/src/main.rs` — `cargo fmt` applied

## Decisions Made

- **`gen_schema` parameter passed by reference (`&PathBuf`)** instead of by value to satisfy `clippy::needless-pass-by-value` (a workspace `pedantic`-warn lint). Zero contract impact — `gen_schema` is a private function inside the xtask binary.
- **Explicit `serde_json::Value` round-trip BEFORE `to_string_pretty`** rather than `to_string_pretty(&schema)` directly. The intermediate `Value` is the determinism guarantor — it lands in `BTreeMap`-backed `Map` (because the workspace `serde_json` pin omits `preserve_order`), so the pretty-print is alphabetic regardless of schemars' internal map type. This is the "belt-and-braces" interpretation of the must_haves: it's robust to silent regressions in either schemars OR a feature-unified `serde_json/preserve_order` slipping in from a downstream crate.
- **YAML-quoted `toolchain: "1.85"`** in the workflow rather than the bare numeric `toolchain: 1.85`. Two reasons: (a) yaml strings preserve trailing zeros (a future bump to `1.10` wouldn't silently parse as `1.1`); (b) dtolnay/rust-toolchain accepts string version specifiers natively. Consistent with `rust-toolchain.toml`.
- **Action SHA-pinning deferred to Phase 7** as the threat model already classifies it. Phase 1 uses `actions/checkout@v4` and `dtolnay/rust-toolchain@stable`; the workflow file's leading comment makes the deferral explicit.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 — Blocking] Pre-existing `cargo fmt` drift across eight source files**

- **Found during:** Task 2 (CI workflow verification — `cargo fmt --all -- --check` exited 1 with diff output against `miner-bench/src/main.rs`, `miner-core/src/{config/mod.rs, error/codes.rs, error/stderr_emit.rs, findings/sink.rs, lib.rs}`, `miner-http/src/main.rs`, `miner-mcp/src/main.rs`).
- **Issue:** Plan 04's StdoutSink/stderr_emit landings, Plan 05's figment config landings, and earlier wrapper-binary placeholders all introduced formatter drift that no preceding plan applied `cargo fmt --all` to. With the new CI workflow about to mandate `cargo fmt --all -- --check`, the very first PR including the workflow would fail its own fmt hygiene step before reaching Gate 1.
- **Fix:** Ran `cargo fmt --all` once. Eight files were rewrapped to satisfy rustfmt; no semantic changes (verified by re-running `cargo build --workspace --all-targets`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace --no-fail-fast` post-fmt — all exited 0; 29 unit tests still pass).
- **Files modified:** crates/miner-bench/src/main.rs, crates/miner-core/src/config/mod.rs, crates/miner-core/src/error/codes.rs, crates/miner-core/src/error/stderr_emit.rs, crates/miner-core/src/findings/sink.rs, crates/miner-core/src/lib.rs, crates/miner-http/src/main.rs, crates/miner-mcp/src/main.rs
- **Verification:** `cargo fmt --all -- --check` now exits 0; clippy still exits 0; build still exits 0; 29 tests still pass.
- **Committed in:** 58272a9 (part of Task 2 commit; explicitly documented in the commit message)

**Scope check:** Per the executor's scope boundary, only issues directly caused by the current task's changes are auto-fixed. The Task 2 plan body explicitly anticipates this scenario (`run \`cargo fmt --all\` first if not formatted`), so the fix is in-scope: Task 2 is the plan that activates the fmt gate, and pre-clean state for that gate is part of Task 2's contract.

---

**Total deviations:** 1 auto-fixed (1 blocking)
**Impact on plan:** Necessary precondition for the fmt hygiene step in the new CI workflow to pass on its own first PR. No scope creep — strictly fmt mechanics, zero semantic change, all clippy + tests still green.

## Issues Encountered

- **clippy::doc_markdown caught `PLAN`/`must_haves`/`BTreeMap` in xtask doc comments** — initial xtask main.rs draft had a doc paragraph referencing `PLAN 06 must_haves` and `BTreeMap (alphabetic) order` without backticks. `cargo clippy -p xtask --all-targets -- -D warnings` flagged these as `clippy::doc-markdown` errors (the workspace `pedantic` warn level escalates to deny under `-D warnings`). Fixed by backticking `PLAN 06`, `must_haves`, and `BTreeMap` in the affected doc lines. Total fix: 3 single-character edits.
- **clippy::needless_pass_by_value caught `gen_schema(out: PathBuf)`** — initial signature took `PathBuf` by value but `out` is only used for two read-only operations (`.parent()`, `.display()`, plus a single `&out` passthrough to `fs::write`). Fixed by changing to `&PathBuf` and updating the single call site to pass `&out`. Zero contract impact.

## User Setup Required

None — no external service configuration required. The CI workflow runs on GitHub's hosted runners; no credentials, secrets, or environment variables are referenced.

## Next Phase Readiness

**Plan 01-07 is unblocked:**

- The committed `schemas/findings-v1.schema.json` is available for Plan 07's integration tests (jsonschema validation of fixture output via `cargo run -p miner-cli -- emit-fixture | jsonschema ...`).
- The CI workflow exists and runs all four mandatory gates locally; Plan 07 can extend it with the integration-test step (likely a new step or modified `cargo test --workspace`).
- The `cargo run -p xtask -- gen-schema` invocation is idempotent and ready for any future plan that needs a schema regeneration touchpoint.

**No blockers.** Phase 1's discovery-time risks (T-01-02 schema drift, T-01-03 stdout pollution) are now mechanically enforced on every PR via Gates 2 and 4. The remaining Phase 1 work in Plan 07 is integration testing — the contract is now locked.

## Self-Check: PASSED

- `xtask/src/main.rs` — FOUND
- `schemas/findings-v1.schema.json` — FOUND (15 494 bytes; valid JSON; all 5 kind consts present; all 16 $defs entries present)
- `.github/workflows/ci.yml` — FOUND (4 mandatory gates + fmt + test, all locally exit 0)
- Commit `b460648` — FOUND (`feat(01-06): add xtask gen-schema with deterministic schema artifact`)
- Commit `58272a9` — FOUND (`feat(01-06): add CI workflow with four mandatory D-21 gates`)
- Twice-run byte-determinism on the regenerated schema — VERIFIED (`cmp` exit 0)
- All four CI gates exit 0 locally on the post-Task-2 tree — VERIFIED

---

*Phase: 01-foundations-contracts*
*Plan: 06*
*Completed: 2026-05-17*
