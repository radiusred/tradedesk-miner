---
phase: 05-statistical-hygiene-sweep-runner
plan: 01
subsystem: api
tags:
  - phase-5
  - schema-additive
  - envelope-contract
  - workspace-deps
  - rand
  - rand_xoshiro
  - toml
  - effect-size
  - bootstrap
  - repro-envelope
  - fdr
  - null-method
  - hygiene

# Dependency graph
requires:
  - phase: 01-foundation
    provides: locked Finding envelope (schemars-derived JSON Schema, scan_id@version contract, seven locked envelope fields)
  - phase: 03-scan-engine-facade-cli
    provides: Scan trait + ScanArity Pattern F (snake_case enum + as_str sibling) + PreflightCode wire-form vocabulary + scan_trait_object_safe regression
  - phase: 04-scan-catalogue-anom-cross-seas
    provides: 22 production Scan impls (every one constructs Effect + ResultFinding struct literals) + WrongInstrumentArity precedent for additive PreflightCode variants
provides:
  - "EffectSize { kind, value } open-string effect-size statistic carried on Effect.effect_size"
  - "ReproEnvelope { master_seed, job_seed, bootstrap, null } carried on ResultFinding.repro (HYG-05 auditable reproducibility)"
  - "BootstrapSpec + NullSpec descriptors (open-string method + n) embedded in ReproEnvelope"
  - "Finding::SweepSummary(SweepSummaryFinding) seventh variant with run-level FDR-by-family + totals (HYG-02)"
  - "FdrFamilySummary + FindingFdrEntry + SweepTotals supporting structs"
  - "NullMethod { PhaseScramble, CircularShift } enum mirroring ScanArity Pattern F"
  - "Scan trait default-false dyn-safe supports_bootstrap() + supports_null_method(NullMethod) (D5-04)"
  - "PreflightCode::HygieneNotSupported variant + as_str arm + test row"
  - "Workspace deps: rand 0.8.6, rand_xoshiro 0.6.0, toml 0.8.23 (sync-only — FOUND-04 preserved)"
  - "Regenerated schemas/findings-v1.schema.json (additive diff only)"
affects:
  - 05-02
  - 05-03
  - 05-04
  - 05-05
  - 06-mcp-http-wrappers
  - 07-hardening

# Tech tracking
tech-stack:
  added:
    - "rand 0.8.6 (Rng + SeedableRng trait surface)"
    - "rand_xoshiro 0.6.0 (Xoshiro256PlusPlus portable PRNG for bootstrap + null)"
    - "toml 0.8.23 (sweep manifest deserialiser for Plan 05-04)"
  patterns:
    - "Pattern S2 (additive envelope): every new Option field carries #[serde(default)] without skip_serializing_if so the None case serialises as JSON null per OUT-03"
    - "Pattern F (enum + as_str): NullMethod mirrors ScanArity verbatim — same derives, snake_case, sibling as_str helper"
    - "Pattern G (PreflightCode growth): new variants inserted alphabetically-by-semantic-group between SweepTooLarge and InternalError"
    - "Snapshot additivity: insta snapshots updated mechanically with literal `null` insertion at the alphabetically-correct position (no field reordering)"

key-files:
  created: []
  modified:
    - "Cargo.toml (Phase-5 dep bucket)"
    - "crates/miner-core/Cargo.toml (workspace inheritance for rand, rand_xoshiro, toml)"
    - "crates/miner-core/src/findings/mod.rs (EffectSize + ReproEnvelope family + SweepSummaryFinding family + Effect.effect_size + ResultFinding.repro + Finding::SweepSummary)"
    - "crates/miner-core/src/scan/mod.rs (NullMethod enum + Scan::supports_bootstrap + Scan::supports_null_method)"
    - "crates/miner-core/src/error/codes.rs (PreflightCode::HygieneNotSupported + as_str + test row)"
    - "schemas/findings-v1.schema.json (regenerated — additive diff only)"
    - "22 scan files: 1 ljung_box + 11 anom + 5 cross + 5 seas — every Effect{} gained effect_size:None, every ResultFinding{} gained repro:None"
    - "crates/miner-core/src/engine/mod.rs (pair-arity stub Effect{}/ResultFinding{} + count_envelopes match arm)"
    - "tests/common/counting_sink.rs (sweep_summary_count field + arm)"
    - "tests/{dry_run,arity_preflight,schema_roundtrip}.rs (exhaustive-match arms + struct-literal fields)"
    - "18 insta snapshots updated with effect_size:null + repro:null fields"

key-decisions:
  - "rand_xoshiro picked over SmallRng/StdRng per 05-RESEARCH §1.5 anti-pattern — non-portable RNGs would break HYG-05 bit-for-bit reproducibility (Cargo.toml comment cites this verbatim)"
  - "bare toml 0.8 picked over figment for the sweep manifest per D5-01 (single config source, no env/profile layering required)"
  - "realfft INTENTIONALLY NOT added in Plan 05-01 — conditional on Plan 05-02 shipping IAAFT; documented as a single-line comment in the Phase-5 dep block so future re-readers do not add it speculatively"
  - "SweepSummaryFinding intentionally framing-like — NO locked envelope fields (schema_version / scan_id_at_version / param_hash / code_revision / data_slice / dsr / fdr_q) because sweep summary is run-level not scan-level (05-RESEARCH Open Question 3 recommendation)"
  - "Effect.effect_size + ResultFinding.repro carry only #[serde(default)] — explicitly NO #[serde(skip_serializing_if = ...)] — to preserve OUT-03 null-not-omitted serialisation discipline (Pitfall 8 / 05-PATTERNS Pattern S2)"
  - "Scan::supports_bootstrap + supports_null_method default-false — every per-scan opt-in happens in Plan 05-02; Plan 05-01 just lays the dyn-safe trait surface"
  - "Schema diff additivity preserved — schemars 1.x emits a new oneOf branch for SweepSummary + new optional properties for effect_size/repro WITHOUT growing any existing struct's `required` array (CI gate would catch a non-additive miss)"

patterns-established:
  - "Phase 5 dep bucket lives between Phase 4's ndarray block and [workspace.lints.rust] — preserves the per-phase visual grouping established in Phase 4"
  - "When a new Finding variant lands, three test files need their exhaustive match arms extended: tests/common/counting_sink.rs (counting helper), tests/dry_run.rs (envelope_kind), tests/arity_preflight.rs (log helper); engine::mod's count_envelopes also needs an arm"
  - "Per-scan Effect/ResultFinding field additions are mechanically inserted via a regex-anchored sweep over 22 scan files; the canonical position for new Option fields is JUST BEFORE the closing `extra,` (for Effect) and JUST AFTER `raw: …,` (for ResultFinding)"

requirements-completed:
  - OP-04
  - HYG-01
  - HYG-02
  - HYG-03
  - HYG-04
  - HYG-05

# Metrics
duration: ~45min
completed: 2026-05-20
---

# Phase 5 Plan 01: Schema + Contract Foundation Summary

**Schema-additive `Finding` envelope extensions (EffectSize, ReproEnvelope, SweepSummary variant), default-false `Scan::supports_bootstrap`/`supports_null_method`, `PreflightCode::HygieneNotSupported`, and `rand` + `rand_xoshiro` + `toml` workspace deps — Phase 5 type-system foundation that Plans 05-02 through 05-05 build against without revisiting.**

## Performance

- **Duration:** ~45 min
- **Started:** 2026-05-20T20:50Z (approx)
- **Completed:** 2026-05-20T21:55Z (approx)
- **Tasks:** 3 (all `type=auto` + `tdd=true` — produced 5 commits per RED/GREEN discipline)
- **Files modified:** 52 across the workspace (3 manifests + 26 source files + 18 insta snapshots + 2 schema/test helper files + 3 integration-test files)

## Accomplishments

- Locked the Phase-5 type-system contract: every kernel (Plan 05-02), the engine population rule (Plan 05-03), the sweep runner (Plan 05-04), and the CLI (Plan 05-05) now compile against the types in this commit without further envelope changes.
- Three sync-only workspace deps (`rand 0.8.6`, `rand_xoshiro 0.6.0`, `toml 0.8.23`) added with zero tokio/async-std drift in production edges (FOUND-04 invariant preserved — `cargo tree -p miner-core -e normal,build | grep -E 'tokio|async-std'` returns empty).
- Regenerated `schemas/findings-v1.schema.json` with a strictly additive diff (262 insertions, 2 deletions where the 2 deletions are doc-comment text mutations on `Effect` / `Finding` because the source doc-comments grew — no property removals, no `required`-array growth on any existing struct).
- All 655 `miner-core` lib tests pass + zero failures across the 58 workspace test binaries; `cargo clippy --workspace --all-targets -- -D warnings` is clean.
- Schema regeneration is byte-stable / idempotent — running `cargo xtask gen-schema` a second time produces no diff (verified via md5sum).

## Task Commits

1. **Task 1: Workspace deps (rand 0.8, rand_xoshiro 0.6, toml 0.8)** — `545ca5f` (chore)
2. **Task 2 RED:** failing tests for envelope additions — `1d554f9` (test)
3. **Task 2 GREEN:** envelope additions + 22-scan field sweep — `2ec17dc` (feat)
4. **Task 3 RED:** failing tests for Scan trait + error code — `674c6db` (test)
5. **Task 3 GREEN:** Scan trait + PreflightCode + schema regen + snapshot updates — `e4d790f` (feat)

_TDD discipline:_ Tasks 2 and 3 each produced a `test(…)` RED commit followed by a `feat(…)` GREEN commit per the workflow's RED→GREEN gate sequence. Task 1 (workspace deps) is type=chore because the "test" is `cargo build` / `cargo tree`, not a new test function.

**Resolved dependency versions** (from `Cargo.lock` after Task 1):
- `rand = "0.8.6"` (direct)
- `rand_xoshiro = "0.6.0"` (direct)
- `toml = "0.8.23"` (direct)
- `rand = "0.9.4"` (transitive — brought in by `ulid` / `rand_distr`, pre-existing, unchanged by this plan)

## Files Created/Modified

**Manifests (3):**
- `Cargo.toml` — Phase-5 dep bucket appended after the Phase-4 ndarray block.
- `crates/miner-core/Cargo.toml` — workspace inheritance for `rand`, `rand_xoshiro`, `toml`.
- `Cargo.lock` — regenerated by cargo.

**Core schema (3 source files):**
- `crates/miner-core/src/findings/mod.rs` — adds `EffectSize`, `BootstrapSpec`, `NullSpec`, `ReproEnvelope`, `SweepSummaryFinding`, `FdrFamilySummary`, `FindingFdrEntry`, `SweepTotals`; modifies `Effect` (`effect_size: Option<EffectSize>`), `ResultFinding` (`repro: Option<ReproEnvelope>`), `Finding` (adds `SweepSummary(SweepSummaryFinding)` variant).
- `crates/miner-core/src/scan/mod.rs` — adds `NullMethod` enum (mirrors `ScanArity`); adds `Scan::supports_bootstrap` and `Scan::supports_null_method` default-false trait methods.
- `crates/miner-core/src/error/codes.rs` — adds `PreflightCode::HygieneNotSupported` variant + `as_str` arm + test row.

**Per-scan field sweep (23 source files — 22 scans + 1 engine pair-stub):**
- `crates/miner-core/src/scan/ljung_box/mod.rs`
- 11 `crates/miner-core/src/scan/anom/*/mod.rs` files
- 5 `crates/miner-core/src/scan/cross/*/mod.rs` files
- 5 `crates/miner-core/src/scan/seas/*/mod.rs` files
- `crates/miner-core/src/engine/mod.rs` (pair-arity stub + `count_envelopes` exhaustive-match arm for `SweepSummary`)
- Each `Effect { … }` struct literal gained `effect_size: None,` (alphabetically between `ci95` and `extra`).
- Each `ResultFinding { … }` struct literal gained `repro: None,` (alphabetically after `raw: …,`).
- Two scan run-method bodies (seas/day_of_week, seas/hour_of_day) crossed the 100-line clippy::pedantic threshold; tagged with a local `#[allow(clippy::too_many_lines, reason = "…")]`.

**Test fixtures + exhaustive-match arms (5 files):**
- `crates/miner-core/tests/common/counting_sink.rs` — `sweep_summary_count` field + arm.
- `crates/miner-core/tests/dry_run.rs` — `envelope_kind` arm.
- `crates/miner-core/tests/arity_preflight.rs` — log helper arm + `Effect{}`/`ResultFinding{}` field additions.
- `crates/miner-core/tests/schema_roundtrip.rs` — `sample_effect_empty_extra` + the two `Finding::Result` constructions.

**Insta snapshots (18 files):**
- Every `Result`-envelope snapshot under `crates/miner-core/tests/snapshots/` gained `"effect_size": null,` (between `ci95` and `extra`) and `"repro": null,` (between `raw` and `run_id`). No other content changed.

**Schema (1 file):**
- `schemas/findings-v1.schema.json` — regenerated via `cargo xtask gen-schema`. Diff is 262 insertions, 2 deletions (both deletions are doc-comment text mutations); no property removals; no `required`-array additions on existing structs.

## Decisions Made

- **`rand_xoshiro` vs `SmallRng`/`StdRng`:** Picked `rand_xoshiro::Xoshiro256PlusPlus` per 05-RESEARCH §1.5 anti-pattern. `SmallRng` and `StdRng` are explicitly non-portable across rand versions and platforms, which would break HYG-05's bit-for-bit reproducibility contract. The `rand` crate is included only for its `Rng` + `SeedableRng` trait surface — no thread-local rng use anywhere in `miner-core`. (Cargo.toml comment cites this verbatim so future re-readers cannot regress.)
- **`toml` 0.8 vs `figment`:** Picked bare `toml` 0.8 per D5-01 for the sweep manifest deserialiser. The sweep manifest is a single config source — no env/profile layering — so `figment`'s strengths (provider chain) add only weight, not value, for this surface.
- **`realfft` excluded:** IAAFT phase-scramble is conditional on Plan 05-02 (the kernels plan). If Plan 05-02 ships IAAFT, Plan 05-02 adds `realfft`; if it ships `CircularShift`-only or simpler, no FFT dep needed. Documented in the Cargo.toml comment so this exclusion is not interpreted as an oversight.
- **`SweepSummaryFinding` shape:** Framing-like record (no locked envelope fields) per 05-RESEARCH Open Question 3. Run-level identity is carried by `run_id` + `produced_at_utc`; scan-level envelope fields (`schema_version`, `scan_id_at_version`, `param_hash`, `code_revision`, `data_slice`, `dsr`, `fdr_q`) don't make sense for a multi-scan FDR summary.
- **OUT-03 null-not-omitted preserved:** Both `Effect.effect_size` and `ResultFinding.repro` carry bare `#[serde(default)]` only — no `skip_serializing_if`. The None case MUST serialise as JSON `null`, not an omitted key, because Phase-1 consumers (and the regenerated JSON Schema's `oneOf` branches) treat field presence with literal `null` as a stable contract.
- **`Scan` default-false trait methods:** `supports_bootstrap` and `supports_null_method` default to `false` so every Phase-4 scan continues to compile unchanged. Plan 05-02 will land per-scan opt-ins via override impls (per-scan table at PATTERNS §"Scan trait extension"). The dyn-safe gate (`scan_trait_object_safe` regression test) is preserved — no generics, no `where Self: Sized`.

## Deviations from Plan

The plan executed largely as written. The following clarifications are documented for transparency, not because they were unplanned.

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Workspace field-sweep across 22 scans + 2 integration tests + 1 engine site**
- **Found during:** Task 2 GREEN (post-type-addition build).
- **Issue:** Adding non-default fields `Effect.effect_size` and `ResultFinding.repro` broke every existing struct literal in the production scan code (22 scans + 1 engine pair-stub + 2 integration test files) with E0063 "missing field" errors.
- **Fix:** Mechanical Python sweep inserted `effect_size: None,` (Effect literals) and `repro: None,` (ResultFinding literals) at the canonical position (alphabetically between adjacent fields), preserving formatting and comments. No semantic changes — every scan continues to emit the same wire form except for the two newly-null fields.
- **Files modified:** 22 scan files + `crates/miner-core/src/engine/mod.rs` + `crates/miner-core/tests/{arity_preflight,schema_roundtrip}.rs`.
- **Verification:** `cargo test -p miner-core --lib` 655/655 pass; the existing snapshot suite catches the wire-form change as the additive null-insertion (next deviation).
- **Committed in:** `2ec17dc` (Task 2 GREEN).

**2. [Rule 3 - Blocking] Insta snapshot updates for additive null fields**
- **Found during:** Task 3 GREEN (post-schema-regen integration-test run).
- **Issue:** 18 insta snapshot files under `crates/miner-core/tests/snapshots/` failed because the JSONL output now includes `"effect_size": null,` and `"repro": null,` per the OUT-03 contract.
- **Fix:** Mechanical Python sweep updated each snapshot in-place, inserting `"effect_size": null,` between `"ci95"` and `"extra"` (alphabetically) and `"repro": null,` between `"raw"` and `"run_id"` (alphabetically). No other snapshot content changed.
- **Files modified:** 18 `*.snap` files.
- **Verification:** `cargo test -p miner-core` runs all snapshot-driven integration tests green.
- **Committed in:** `e4d790f` (Task 3 GREEN).

**3. [Rule 3 - Blocking] Exhaustive match arms for Finding::SweepSummary**
- **Found during:** Task 3 GREEN (post-trait-addition full-workspace test run).
- **Issue:** `Finding::SweepSummary` triggered E0004 non-exhaustive-pattern errors in 4 places: `engine::mod`'s `count_envelopes` helper, `tests/common/counting_sink.rs::CountingSink::write_envelope`, `tests/dry_run.rs::envelope_kind`, and an arity_preflight log helper.
- **Fix:** Added an explicit `Finding::SweepSummary(_) => …` arm in each site. The engine test helper uses a no-op (single-run engine tests never produce SweepSummary); the `CountingSink` gains a new counter field; the two `envelope_kind`-style helpers return `"sweep_summary"`.
- **Files modified:** `crates/miner-core/src/engine/mod.rs`, `crates/miner-core/tests/common/counting_sink.rs`, `crates/miner-core/tests/dry_run.rs`, `crates/miner-core/tests/arity_preflight.rs`.
- **Verification:** `cargo test -p miner-core` 58 binaries green; `cargo clippy --workspace --all-targets -- -D warnings` clean.
- **Committed in:** `e4d790f` (Task 3 GREEN).

**4. [Rule 2 - Missing Critical] Local `clippy::too_many_lines` allow on two scan run methods**
- **Found during:** Task 3 GREEN clippy check.
- **Issue:** Two scan `run` methods (seas/day_of_week, seas/hour_of_day) reached 101 lines (the pedantic threshold is 100) because the `effect_size: None,` + `repro: None,` insertions nudged them over. Splitting the bodies would obscure the linear scan-build-emit flow without reducing complexity.
- **Fix:** Local `#[allow(clippy::too_many_lines, reason = "…")]` on the two methods only — no workspace-level lint relaxation. Reason string cites the Phase 5 plan and the additive growth.
- **Files modified:** `crates/miner-core/src/scan/seas/day_of_week/mod.rs`, `crates/miner-core/src/scan/seas/hour_of_day/mod.rs`.
- **Committed in:** `e4d790f` (Task 3 GREEN).

**5. [Rule 1 - Bug] Doc-comment missing backticks on `master_seed`/`param_hash`**
- **Found during:** Task 3 GREEN clippy `-D warnings` check.
- **Issue:** Two `clippy::doc_markdown` errors on freshly-added doc-comments in `ReproEnvelope::job_seed` and `findings/mod.rs::tests::sweep_summary_finding_uses_snake_case_kind` (`HashMap`/`BTreeMap` were unbacked) and one in `tests/common/counting_sink.rs` (`Finding::SweepSummary` unbacked).
- **Fix:** Wrapped identifiers in backticks per clippy guidance.
- **Files modified:** `crates/miner-core/src/findings/mod.rs`, `crates/miner-core/tests/common/counting_sink.rs`.
- **Committed in:** `e4d790f` (Task 3 GREEN). Note: the doc-comment fix on `job_seed` arrived AFTER the first schema regen, so the schema needed a second regen to pick up the updated doc text; this is reflected in the final committed schema (verified idempotent via md5sum after the amend).

---

**Total deviations:** 5 auto-fixed (all Rule 1 / Rule 2 / Rule 3 — no Rule 4 architectural changes; no scope creep).
**Impact on plan:** Every deviation is a mechanical consequence of the additive field/variant insertions. The wire contract evolved exactly as the plan specified — the deviations are the "how" of landing that contract through 22 scans + 18 snapshots + 4 exhaustive-match sites.

## Issues Encountered

**Plan-stated `cargo tree | grep tokio` check was misleading.** The plan's Task 1 verify line literally checks `cargo tree -p miner-core | grep -E 'tokio|async-std' | wc -l | grep -q '^0$'`, but the pre-existing `jsonschema` DEV dependency (Phase 1) pulls `reqwest` → `hyper` → `tokio` transitively. The CORRECT FOUND-04 invariant — and the one the project's previous CI gates respect — is `cargo tree -p miner-core -e normal,build`, which excludes dev-only edges. Verified pre-existing condition: BEFORE my changes, `cargo tree -p miner-core | grep tokio` already returned 11 matches. The actual FOUND-04 check (`-e normal,build`) returns 0 both before and after this plan, confirming the three new sync-only deps do not break the invariant. Documented here so the Plan 05-02 executor does not re-run the same misleading command.

**Single git --amend used on Task 3 GREEN.** While stabilising clippy + the resulting schema regen drift, I edited a doc comment on `ReproEnvelope::job_seed` AFTER the initial commit. The committed schema was therefore one regen behind. I amended the Task 3 GREEN commit (`b44d8d5` → `e4d790f`) with the corrected schema rather than creating a follow-up `chore(05-01): re-regen schema` commit. The GSD git protocol prefers new commits over amends; in retrospect a `chore(...)` follow-up would have been the cleaner choice. The amend was confined to a single commit (not a history rewrite); no work was lost; final state is byte-stable.

## User Setup Required

None — no external service configuration required. This plan adds compile-time type definitions only.

## Next Phase Readiness

- **Plan 05-02 (kernels) READY:** can import `EffectSize`, `ReproEnvelope`, `BootstrapSpec`, `NullSpec`, `NullMethod` directly. Per-scan opt-ins (`supports_bootstrap` / `supports_null_method` override impls) plug straight into the trait. The `realfft` dep decision lives in Plan 05-02 — if IAAFT ships, add it there.
- **Plan 05-03 (engine integration) READY:** can write the population rule "`repro = Some(_)` iff bootstrap or null was run" against `ResultFinding.repro` without further envelope changes. `PreflightCode::HygieneNotSupported` is available for the early-rejection path.
- **Plan 05-04 (sweep runner) READY:** can emit `Finding::SweepSummary(SweepSummaryFinding { … })` with `fdr_by_family: BTreeMap<…, FdrFamilySummary>` directly. The wire form is locked.
- **Plan 05-05 (CLI) READY:** can wire `--bootstrap-method`/`--null-method`/`--master-seed` flags against the `BootstrapSpec`/`NullSpec`/`NullMethod` types without back-pressure on Plan 05-01.
- **Zero blockers** for the rest of Phase 5. The CI gate (`cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `git diff --exit-code schemas/`) is green at this commit.

## Self-Check: PASSED

- `SUMMARY.md` exists at `.planning/phases/05-statistical-hygiene-sweep-runner/05-01-SUMMARY.md`.
- All 5 task commits exist: `545ca5f` (Task 1 chore), `1d554f9` (Task 2 RED), `2ec17dc` (Task 2 GREEN), `674c6db` (Task 3 RED), `e4d790f` (Task 3 GREEN).
- SUMMARY metadata commit: `623d6d9`.

---
*Phase: 05-statistical-hygiene-sweep-runner*
*Completed: 2026-05-20*
