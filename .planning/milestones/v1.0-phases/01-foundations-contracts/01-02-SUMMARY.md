---
phase: 01-foundations-contracts
plan: 02
subsystem: foundations
tags: [rust, schemars, figment, clap, spike, base64, json-schema, config-precedence]

# Dependency graph
requires: [plan-01-01]
provides:
  - "Verified: schemars 1.2.1 `serde_json::json!{...}.try_into()` -> `Schema` compiles and produces the expected `contentEncoding: \"base64\"` + `contentMediaType: \"application/octet-stream\"` fragment (closes RESEARCH Risk 2 / Assumption A1)"
  - "Verified: figment 0.10.19 + clap `Option<T>` + `#[serde(skip_serializing_if = \"Option::is_none\")]` + CLI-LAST merge order produces CLI > env > TOML > error precedence (closes RESEARCH Risk 5 / Assumption A4)"
  - "Verified: missing required config fields produce `figment::Error` (not silent default fallback)"
  - "crates/miner-core/src/spike_base64.rs — DELETION TARGET for Plan 03"
  - "crates/miner-core/src/spike_figment.rs — DELETION TARGET for Plan 05"
  - "Confirmation that Plan 03 may implement `Base64Bytes` exactly as RESEARCH §Pattern 2 specifies"
  - "Confirmation that Plan 05 may implement `miner-core::config` exactly as RESEARCH §Pattern 4 specifies"
affects: [plan-01-03, plan-01-05]

# Tech tracking
tech-stack:
  added:
    - "tempfile (transitive via figment `test` feature) for Jail-scoped tempdirs"
    - "figment `test` feature (enables `figment::Jail`) — dev-deps only"
  patterns:
    - "Manual `JsonSchema` impl via `serde_json::json!{...}.try_into().expect(\"valid schema fragment\")` for newtype-with-format types (Base64Bytes shape)"
    - "figment + clap CLI-wins precedence: `Option<T>` overlay + `#[serde(skip_serializing_if)]` + `Serialized::defaults(cli)` merged LAST"
    - "`figment::Jail` in tests instead of `unsafe { std::env::set_var(..) }` — satisfies the workspace `unsafe_code = \"forbid\"` lint while keeping env-var-driven precedence tests deterministic"

key-files:
  created:
    - "crates/miner-core/src/spike_base64.rs"
    - "crates/miner-core/tests/spike_schema.rs"
    - "crates/miner-core/src/spike_figment.rs"
    - "crates/miner-core/tests/spike_figment_precedence.rs"
  modified:
    - "crates/miner-core/src/lib.rs (added `pub mod spike_base64;` + `pub mod spike_figment;`)"
    - "crates/miner-core/Cargo.toml (added figment to [dependencies]; clap + figment-with-test-feature to [dev-dependencies])"
    - "Cargo.lock (figment `test` feature pulls tempfile + parking_lot dev-deps)"

key-decisions:
  - "schemars 1.2.1 + `serde_json::json!{}.try_into()` works as RESEARCH predicted — NO fallback needed. Plan 03 implements `Base64Bytes` verbatim per RESEARCH §Pattern 2."
  - "figment 0.10.19 + the steezeburger pattern produces correct CLI > env > TOML > error precedence — NO adjustment needed. Plan 05 implements `miner-core::config::build` verbatim per RESEARCH §Pattern 4."
  - "Tests use `figment::Jail` (canonical figment test fixture) rather than direct `std::env::set_var`. Reason: under Rust 1.85 + edition 2024 the env-mutation calls are `unsafe fn`; the workspace `unsafe_code = \"forbid\"` lint correctly rejects the `unsafe` block. `Jail` serialises env-var access process-wide AND scope-cleans on drop — both correctness wins. Plan 05's production test strategy should adopt the same approach."

requirements-completed: [FOUND-03, FOUND-05]
threats-mitigated: [T-01-02]

# Metrics
duration: 18min
completed: 2026-05-16
---

# Phase 01 Plan 02: Wave 2 Spikes — schemars + figment Verification Summary

**Both Risk 2 (schemars 1.x base64-with-shape) and Risk 5 (figment + clap CLI-wins precedence) verified — RESEARCH §Pattern 2 and §Pattern 4 may be implemented verbatim by Plans 03 and 05 respectively. Zero fallbacks required.**

## Performance

- **Duration:** 18 min
- **Started:** 2026-05-16T09:32:00Z (approximate, after orchestrator hand-off)
- **Completed:** 2026-05-16T09:50:31Z
- **Tasks:** 2 (both auto + TDD)
- **Files modified:** 4 created, 3 modified

## Spike Results

### Spike 1 — schemars 1.x base64-with-shape (Risk 2 / Assumption A1): **VERIFIED**

The single `[ASSUMED]` line in RESEARCH §"Architecture Patterns" Pattern 2 —

```rust
serde_json::json!({
    "type": "string",
    "contentEncoding": "base64",
    "contentMediaType": "application/octet-stream",
    "description": "Little-endian f64 bytes, base64-encoded"
})
.try_into()
.expect("valid schema fragment")
```

— compiles cleanly against `schemars = "1.2.1"` and produces the expected JSON Schema fragment. The integration test `tests/spike_schema.rs` calls `schemars::schema_for!(SpikeFinding)` and asserts:

- `"contentEncoding": "base64"` present in the generated schema
- `"contentMediaType": "application/octet-stream"` present
- `"data"` and `"shape"` properties present (composition chain through `SpikeRawArray { data: SpikeBase64Bytes, shape: Vec<u64>, dtype: SpikeDtype }` works)

**Outcome for Plan 03:** Implement `crates/miner-core/src/findings/base64_bytes.rs` exactly as RESEARCH §Pattern 2 specifies. The `try_into` line is the conversion. NO fallback (`#[schemars(schema_with = ...)]` field-level attribute) is required.

**Plan 03 action item:** Delete `crates/miner-core/src/spike_base64.rs` and `crates/miner-core/tests/spike_schema.rs`; remove `pub mod spike_base64;` from `crates/miner-core/src/lib.rs`.

### Spike 2 — figment + clap CLI-wins precedence (Risk 5 / Assumption A4): **VERIFIED**

The pattern from RESEARCH §"Architecture Patterns" Pattern 4 + §"Common Pitfalls" Pitfall 1 — `Option<T>` overlay struct with `#[serde(skip_serializing_if = "Option::is_none")]` on every field, `Serialized::defaults(cli)` merged LAST — produces the correct precedence CLI > env > TOML > error.

The integration test `tests/spike_figment_precedence.rs` covers five sub-cases inside four `figment::Jail` scopes (one Jail per logical context so env state cannot leak between sub-tests):

| # | Sources Set | Field Tested | Expected | Observed |
|---|-------------|--------------|----------|----------|
| 1 | TOML + env + CLI | `cache_root` | `/cli` (CLI wins) | `/cli` ✓ |
| 1 | TOML only | `bar_cache_root` | `/file/bar` (TOML survives) | `/file/bar` ✓ |
| 1 | TOML only | `output` | `Stdout` (TOML survives) | `Stdout` ✓ |
| 2 | TOML + env (no CLI) | `cache_root` | `/env` (env beats TOML) | `/env` ✓ |
| 3 | TOML only | `cache_root` | `/file` | `/file` ✓ |
| 4 | None | (any) | `figment::Error` | `Err(...)` ✓ |
| 5a | TOML + env + CLI | `bar_cache_root` + `output` (enum variant `File(PathBuf)`) | `/cli/bar` + `File("/cli/out.jsonl")` | matches ✓ |
| 5b | TOML + env | `bar_cache_root` | `/env/bar` | `/env/bar` ✓ |

**Outcome for Plan 05:** Implement `crates/miner-core/src/config/mod.rs` exactly as RESEARCH §Pattern 4 specifies. The merge order `Toml::file(path)` → `Env::prefixed("MINER_")` → `Serialized::defaults(cli)` is correct; the `Option<T>` + `skip_serializing_if` overlay is the mechanism that lets CLI-unset fields fall through to env/TOML. NO adjustment needed.

**Plan 05 action item:** Delete `crates/miner-core/src/spike_figment.rs` and `crates/miner-core/tests/spike_figment_precedence.rs`; remove `pub mod spike_figment;` from `crates/miner-core/src/lib.rs`. Decide whether the production `config` test strategy uses `figment::Jail` (recommended — already a dev-dep) or migrates to `figment::providers::Serialized` fixtures.

## Accomplishments

- `cargo test -p miner-core --test spike_schema` exits 0 (1 test, 1 passed).
- `cargo test -p miner-core --test spike_figment_precedence -- --test-threads=1` exits 0 (1 test with 8 sub-asserts across 4 Jail scopes, 1 passed).
- `cargo test -p miner-core` (full crate test) exits 0 (2 integration tests + 0 unit tests + 0 doctests, all passing).
- `cargo build --workspace` succeeds in 2.95s on rebuild (incremental).
- Both spike modules and tests carry deletion-target comments pointing to the responsible downstream plans (Plan 03 deletes the schemars spike; Plan 05 deletes the figment spike).
- The schemars-derived schema on `SpikeFinding` includes the embedding chain `SpikeFinding → SpikeRawArray → SpikeBase64Bytes`, proving the manual `JsonSchema` impl is reachable via derive composition (which is how the production `Finding` envelope in Plan 03 will reach it).
- The figment spike covers BOTH primitive-typed (`PathBuf`) AND enum-typed (`SpikeOutputDest::File(PathBuf)`) fields — verifying the precedence pattern generalises to the full envelope shape Plan 05 needs.

## Task Commits

Each task was committed atomically on `worktree-agent-a8b2201e40ee3bd10`:

1. **Task 1: Spike — schemars 1.x base64-with-shape** — `244b717` (`feat(01-02): spike schemars 1.x base64-with-shape pattern`)
2. **Task 2: Spike — figment + clap CLI-wins precedence** — `aaa89c2` (`feat(01-02): spike figment + clap CLI-wins precedence`)

(The final metadata commit covering this SUMMARY.md is committed by this executor after the SUMMARY is written.)

## Files Created/Modified

- **`crates/miner-core/src/spike_base64.rs`** (created) — `SpikeBase64Bytes(pub Vec<u8>)` newtype with manual `Serialize`/`Deserialize`/`JsonSchema` impls per RESEARCH §Pattern 2; composed into `SpikeRawArray { data, shape, dtype }` and wrapped in `SpikeFinding { array }` to exercise the embedding chain. Module-level docstring marks it for deletion by Plan 03.
- **`crates/miner-core/tests/spike_schema.rs`** (created) — Single integration test `spike_emits_content_encoding` that calls `schemars::schema_for!(SpikeFinding)` and asserts the four required substrings in the pretty-printed schema. Marked for deletion by Plan 03.
- **`crates/miner-core/src/spike_figment.rs`** (created) — `SpikeConfig` (non-optional fields), `SpikeOutputDest` enum (`Stdout` / `File(PathBuf)`), `SpikeCliOverrides` (all-`Option<T>` with `skip_serializing_if`), and `build_figment` factory honouring 01-CONTEXT D-16 precedence. `#[must_use]` on `build_figment` to satisfy clippy::pedantic. Marked for deletion by Plan 05.
- **`crates/miner-core/tests/spike_figment_precedence.rs`** (created) — Single integration test `spike_precedence_cli_wins_over_env_over_toml` spanning four `figment::Jail` scopes covering tests 1-5 (CLI>env>TOML, env>TOML, TOML alone, missing-required, field-level generalisation). Marked for deletion by Plan 05.
- **`crates/miner-core/src/lib.rs`** (modified) — Added `pub mod spike_base64;` (Task 1) and `pub mod spike_figment;` (Task 2) with deletion-target comments.
- **`crates/miner-core/Cargo.toml`** (modified) — Added `figment.workspace = true` to `[dependencies]` (Task 2; spike module lives in `src/`). Added `clap.workspace = true` to `[dev-dependencies]`. Replaced workspace-inherited `figment` with explicit `figment = { version = "0.10", features = ["toml", "env", "test"] }` in `[dev-dependencies]` so the test target gets `figment::Jail`.
- **`Cargo.lock`** (modified) — Resolved `tempfile 3.27.0`, `parking_lot 0.12.5`, `errno`, `fastrand`, `rustix`, `linux-raw-sys` as transitive deps of `figment` with the `test` feature.

## Decisions Made

- **No fallback for the schemars spike.** The `try_into` line works as RESEARCH predicted; Plan 03 uses Pattern 2 verbatim.
- **No fallback for the figment spike.** Pattern 4 + the steezeburger workaround produce the correct precedence; Plan 05 uses Pattern 4 verbatim.
- **`figment::Jail` (not `unsafe { std::env::set_var }`) for env-mutation tests.** The workspace `unsafe_code = "forbid"` lint is intentional (see workspace `Cargo.toml` lines 53-54). Plan 05's production tests should adopt the same `Jail` discipline. This is the *only* decision Plan 02 makes that wasn't pre-specified in RESEARCH/CONTEXT — and it's a strict improvement over the alternative.
- **figment with `features = ["toml", "env", "test"]` in `[dev-dependencies]`.** The base workspace inheritance provides only `["toml", "env"]`; the `test` feature is opt-in per-crate and only needed by miner-core's spike test today. Plan 05 may keep this pattern or move `test` to workspace.dependencies depending on whether other crates need `Jail`.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 — Blocking issue] `unsafe_code = "forbid"` blocks direct `std::env::set_var` calls**

- **Found during:** Task 2 (first attempt to run `cargo test -p miner-core --test spike_figment_precedence`)
- **Issue:** The plan's Task 2 instructions said "use `std::env::set_var`/`remove_var` carefully inside a single sequential test function." Under Rust 1.85 + edition 2024 those are `unsafe fn` — calling them requires an `unsafe { }` block. The workspace lint `unsafe_code = "forbid"` (Cargo.toml line 54) rejects any `unsafe` block, and `forbid` cannot be downgraded via `#[allow(unsafe_code)]`.
- **Fix:** Rewrote the test to use `figment::Jail` — the canonical figment test fixture. `Jail::set_env` is a safe-fn wrapper that ALSO serialises env-var access across the process (avoiding parallel-test races) AND scope-cleans on drop (no leaked env state). This is a strictly better pattern than the hand-rolled approach the plan suggested.
- **Files modified:** `crates/miner-core/Cargo.toml` (added `figment` with `test` feature in dev-deps), `crates/miner-core/tests/spike_figment_precedence.rs` (replaced `unsafe` env-mutation helpers with `Jail::expect_with`).
- **Commit:** `aaa89c2`
- **Justification for Rule 3 (not Rule 4 architectural):** This is a tooling-level adaptation, not a precedence or architecture change. The pattern under test (figment merge order, `Option<T>` + `skip_serializing_if`, CLI-last) is unchanged — only the test harness changed. RESEARCH's recommended pattern is still what Plan 05 will implement.

**2. [Rule 2 — Auto-add missing critical functionality] `#[must_use]` on `build_figment`**

- **Found during:** Task 2 (clippy pass)
- **Issue:** `clippy::must_use_candidate` (pedantic) warned that `build_figment` returns a `Figment` that should be `#[must_use]`. The workspace lints have `clippy::pedantic` at `warn`, which doesn't fail the build but does pollute output.
- **Fix:** Added `#[must_use]` to `build_figment` in `crates/miner-core/src/spike_figment.rs`.
- **Files modified:** `crates/miner-core/src/spike_figment.rs`.
- **Commit:** `aaa89c2` (included in the same commit; not split because it's part of the same logical work).

## Deferred Issues

**Pre-existing clippy warning in `crates/miner-core/build.rs` line 19** (`clippy::map_unwrap_or` — `map(<f>).unwrap_or_else(<g>)` pattern). This warning was introduced by Plan 01-01 (commit `f7f0bfe`) and is NOT caused by Plan 01-02 changes. Per the deviation scope boundary in `execute-plan.md` ("Only auto-fix issues DIRECTLY caused by the current task's changes"), this is out of scope for Plan 01-02. Logging it here for visibility:

- File: `crates/miner-core/build.rs:19`
- Issue: `let sha = Command::new("git").args(...).output().ok().map(|o| ...).unwrap_or_else(|| ...)` should use `map_or_else` per `clippy::map-unwrap-or` (level: error under `-D warnings`).
- Why it matters: blocks `cargo clippy -p miner-core --all-targets -- -D warnings` from passing in its current form.
- Recommendation: Plan 04 (CI gates) should either fix this in `build.rs` as part of its setup or exclude `build.rs` from the clippy gate. Either is fine — the build.rs SHA-injection logic is sound; only the lint pattern is sub-optimal.

## Issues Encountered

- **`std::env::set_var` is `unsafe fn` in Rust 1.85 + edition 2024.** Discovered while implementing the figment precedence test. The plan's instructions presumed a safe-fn version (which was true in earlier editions). The figment ecosystem provides `figment::Jail` exactly for this use case — see "Deviation 1" above.
- **`figment::Jail` is gated behind the `test` cargo feature.** The workspace inheritance for `figment` only enables `["toml", "env"]`. To get `Jail`, I had to add an explicit `figment = { version = "0.10", features = ["toml", "env", "test"] }` entry in `crates/miner-core/Cargo.toml` `[dev-dependencies]`. This shadows the workspace inheritance for the dev-dep target only — `[dependencies]` still uses the inherited form (no `test` feature in production builds).

## Threat Mitigation

- **T-01-02 (schema injection / drift):** The schemars spike establishes the schema-derivation pattern under test from day one. Any future change to `Base64Bytes::json_schema` that breaks the `contentEncoding` / `contentMediaType` advertisement will fail `tests/spike_schema.rs` (and, post-Plan-03, the equivalent test on the production `Base64Bytes` type). Combined with Plan 06's `cargo run -p xtask -- gen-schema && git diff --exit-code schemas/` CI gate, contributors cannot quietly drift the Rust types away from the published JSON Schema.

## User Setup Required

None — both spikes run entirely from `cargo test`; no external service configuration required.

## Next Phase Readiness

- **Plan 01-03 (Wave 3, Finding envelope types)** is UNBLOCKED. The recommended `Base64Bytes` Pattern 2 is verified to compile and produce the expected schema fragment.
- **Plan 01-05 (Wave 3, config layering)** is UNBLOCKED. The recommended figment + clap precedence is verified.
- **Plans 03 and 05 must delete the spike modules** as part of their work — the deletion-target comments and frontmatter `provides` list make this explicit.
- **Wave 3 may now start.** No remaining MEDIUM-confidence risks from RESEARCH's Open Risks section.

## Self-Check: PASSED

File existence (created files in this plan):

- `FOUND: crates/miner-core/src/spike_base64.rs`
- `FOUND: crates/miner-core/tests/spike_schema.rs`
- `FOUND: crates/miner-core/src/spike_figment.rs`
- `FOUND: crates/miner-core/tests/spike_figment_precedence.rs`

Commit hashes:

- `FOUND: 244b717` (Task 1)
- `FOUND: aaa89c2` (Task 2)

Plan-level verification:

- `cargo test -p miner-core --test spike_schema -- --nocapture` → 1 passed (pass)
- `cargo test -p miner-core --test spike_figment_precedence -- --test-threads=1` → 1 passed (pass)
- `cargo test -p miner-core` → 2 passed, 0 failed (pass)
- `cargo build --workspace` → `Finished dev profile … in 2.95s` (pass)

---
*Phase: 01-foundations-contracts*
*Completed: 2026-05-16*
