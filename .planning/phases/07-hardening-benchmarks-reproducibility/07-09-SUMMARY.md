---
phase: 07-hardening-benchmarks-reproducibility
plan: 09
subsystem: testing
tags: [byte-determinism, golden-file, integration-test, finding-envelope, snapshot, jsonl]

# Dependency graph
requires:
  - phase: 07-hardening-benchmarks-reproducibility
    provides: "Plan 07-01 real family goldens (stats.summary.welford / cross.cointegration.engle_granger / seas.bucket.hour_of_day) — envelope_snapshot.jsonl sits alongside them as the fourth golden"
provides:
  - "Hand-rolled byte-equal envelope snapshot test (NOT insta) — locks the Finding envelope shape against silent schema drift"
  - "envelope_snapshot.jsonl golden — pinned masked envelope bytes for RunStart + RunEnd framing records"
  - "Closes ROADMAP Phase 7 success criterion #1 (byte-determinism gate) together with Plan 07-01 and Plan 07-05"
affects: ["consumer parsers (quant agent, future tradedesk)", "v1.0 release readiness", "Phase 7 milestone close"]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Hand-rolled byte-equal golden (Pattern from cli_streams::emit_fixture_byte_identical_when_volatile_fields_masked) — preferred over insta for envelope-shape goldens because it requires no review ceremony on first push"
    - "5-field volatile-field mask (run_id, started_at_utc, ended_at_utc, produced_at_utc, wall_clock_ms) via common::mask_volatile_fields — shared across all envelope-determinism integration tests"
    - "#[ignore]d regen helper — operator-triggered via `cargo test ... -- --ignored regenerate_*`; schema-evolution requires both the regen AND a documented rationale in the same PR"

key-files:
  created:
    - "crates/miner-core/tests/findings_envelope_snapshot.rs"
    - "crates/miner-core/tests/goldens/envelope_snapshot.jsonl"
  modified:
    - ".planning/phases/07-hardening-benchmarks-reproducibility/deferred-items.md"

key-decisions:
  - "Plan 07-09: in-process emit-fixture replication (option b) — `assert_cmd` is not in miner-core dev-deps, and Cargo.toml is off-limits this session (Plan 07-06 owns the manifest in a parallel worktree). The in-process path exercises the same FindingSink envelope-write code path so the byte-equality assertion still covers the shared serialisation discipline."
  - "Plan 07-09: 4 #[test] attributes (3 active + 1 #[ignore]d regen helper) — exceeds the acceptance criterion of 3 active tests; the variant-coverage assertion adds a third active gate beyond the matches-golden + byte-identical-rerun pair."
  - "Plan 07-09: golden body is 2 envelopes (RunStart + RunEnd) — the emit-fixture invocation emits exactly these two framing records per D-09 / D-11. ScanError / DryRun / Result envelopes are NOT part of this golden because the invocation does not emit them; family goldens (Plan 07-01) cover Result-variant byte-determinism."

patterns-established:
  - "Pattern: byte-determinism gate via hand-rolled mask-then-byte-compare — codified by the three active tests in findings_envelope_snapshot.rs"
  - "Pattern: pinned envelope test uses fixed miner_version + code_revision literals so the golden does not churn on workspace version bumps; volatile fields are masked, version strings are pinned"

requirements-completed: [FOUND-02, FOUND-03, OUT-03]

# Metrics
duration: 10min
completed: 2026-05-22
---

# Phase 7 Plan 09: Locked Findings-Envelope Snapshot Summary

**Hand-rolled byte-equal envelope-snapshot test + pinned `envelope_snapshot.jsonl` golden — the byte-determinism gate that closes ROADMAP Phase 7 success criterion #1.**

## Performance

- **Duration:** ~10 min
- **Started:** 2026-05-22T10:16:15Z
- **Completed:** 2026-05-22T10:27:12Z
- **Tasks:** 1 (TDD: RED + GREEN — single behavioural task)
- **Files modified:** 3 (2 created + 1 modified)

## Accomplishments

- Locked the `Finding` envelope JSON shape against silent schema drift via a hand-rolled byte-equal golden gate (NOT `insta` — per 07-RESEARCH Pitfall 8). Any rename / reorder / map-collapse in the seven locked envelope fields, the variant payload structs, or the `BTreeMap` discipline now fails this test.
- Pinned three active behavioural assertions:
  1. `envelope_snapshot_matches_golden` — byte-equality vs `tests/goldens/envelope_snapshot.jsonl` (FOUND-03 + the locked-envelope contract).
  2. `envelope_snapshot_byte_identical_across_runs` — two in-process invocations produce byte-identical masked envelopes (OUT-03 closure end-to-end across the serialised form).
  3. `envelope_snapshot_covers_all_emitted_variants` — the invocation's `kind` discriminator set equals `{run_start, run_end}` exactly; adding a new variant is a deliberate PR-level update.
- Authored a `#[ignore]`d regen helper (`regenerate_envelope_snapshot_golden`) so schema-evolution is operator-triggered and PR-reviewed — drift cannot be silently masked.
- Together with Plan 07-01 (real family goldens) and Plan 07-05 (noise-replay regression), ROADMAP Phase 7 success criterion #1 is now closed: "User can run the full golden-file regression suite (one representative scan per family, plus the locked findings-envelope snapshot test) and observe byte-identical JSONL across runs."

## Task Commits

Each task was committed atomically:

1. **Task 1: Author findings_envelope_snapshot.rs + generate envelope_snapshot.jsonl golden + verify byte-identical re-run** — `fb16ee2` (test)

_Note: TDD RED + GREEN were combined into a single commit because the golden cannot exist before the test (you cannot author a byte-equal assertion against a non-existent file). The plan acknowledges this — the regen helper is the documented two-step path._

## Files Created/Modified

- `crates/miner-core/tests/findings_envelope_snapshot.rs` — hand-rolled byte-equal envelope snapshot test (3 active `#[test]` + 1 `#[ignore]`d regen helper); uses `mod common;` + `include_str!("goldens/envelope_snapshot.jsonl")`; replicates `miner emit-fixture` in-process via `BufferSink` since `assert_cmd` is not in `miner-core` dev-deps.
- `crates/miner-core/tests/goldens/envelope_snapshot.jsonl` — pinned masked envelope bytes (2 lines: `kind=run_start` + `kind=run_end`; 5-field volatile mask applied: `run_id`, `started_at_utc`, `ended_at_utc`, `produced_at_utc`, `wall_clock_ms`).
- `.planning/phases/07-hardening-benchmarks-reproducibility/deferred-items.md` — appended item #3 documenting pre-existing `cargo clippy -- -D warnings` failures in `engine/hygiene_dispatch.rs` and `scan/hygiene/null.rs` (out of scope for this plan; lib code from Phase 5 hygiene).

## Decisions Made

- **In-process emit-fixture replication (option b in the plan's `<action>` step).** The plan offered two ways to capture envelope bytes:
  - (a) Spawn `miner emit-fixture` via `assert_cmd::Command::cargo_bin`.
  - (b) Construct envelopes directly via `BufferSink` and `Finding::RunStart` / `Finding::RunEnd`.

  Option (a) requires `assert_cmd` in `crates/miner-core/Cargo.toml`'s `[dev-dependencies]`. Cargo.toml modifications were explicitly forbidden this session because Plan 07-06 (criterion benches) is being executed concurrently in a worktree that will be merged later — touching Cargo.toml would create a 3-way merge conflict. Option (b) achieves the same envelope-write code path (`FindingSink::write_envelope` → `serde_json::to_vec` → `BTreeMap` ordering) without the dev-dep churn.

- **Fixed miner_version + code_revision literals.** The real `emit_fixture` in `miner-cli/src/main.rs` uses `env!("CARGO_PKG_VERSION")` and `miner_core::CODE_REVISION`. Both would make the golden churn on every release bump or git commit. The golden's contract is over envelope SHAPE, not version strings. Fixed literals (`"0.1.0"` and `"test-revision-fixed"`) keep the byte-equal gate stable across CI runs.

- **`miner_version` and `code_revision` are NOT in the volatile-field mask.** They are pinned literal strings in the test; masking them would defeat the schema-shape pin (a real drift like renaming `miner_version` → `app_version` would be silently masked).

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Removed unused `chrono::TimeZone` import**
- **Found during:** Task 1 (initial compile)
- **Issue:** `use chrono::{TimeZone, Utc};` imported `TimeZone` but the in-process invocation only uses `Utc::now()` (no `Utc.with_ymd_and_hms` calls), so `TimeZone` was unused — `cargo test` emitted a `warning: unused import: TimeZone`. The acceptance criterion `cargo clippy ... -- -D warnings` would have failed on this warning alone.
- **Fix:** Replaced with `use chrono::Utc;`.
- **Files modified:** `crates/miner-core/tests/findings_envelope_snapshot.rs`
- **Verification:** Re-ran `cargo test -p miner-core --test findings_envelope_snapshot` — zero warnings, all 3 active tests pass.
- **Committed in:** `fb16ee2` (Task 1 commit)

---

**Total deviations:** 1 auto-fixed (1 bug — unused import)
**Impact on plan:** Negligible. The deviation was a single-line fix during the RED → GREEN transition; no scope creep.

## Issues Encountered

- **`cargo clippy -p miner-core --test findings_envelope_snapshot -- -D warnings` fails on pre-existing lib errors.** 6 clippy errors live in `crates/miner-core/src/engine/hygiene_dispatch.rs` and `crates/miner-core/src/scan/hygiene/null.rs` (Phase 5 hygiene code; pre-existing on `main` HEAD). Confirmed by running `cargo clippy -p miner-core --lib -- -D warnings` standalone — same 6 errors with no test-crate dependency. The new test file itself emits zero clippy warnings. Logged as item #3 in `deferred-items.md`; out of scope for Plan 07-09 per the executor SCOPE BOUNDARY rule.

- **`crates/miner-cli/tests/sigint_mid_sweep.rs::sigint_mid_sweep_preserves_streamed_findings` is timing-flaky under `cargo test --workspace`.** It passed when run in isolation (`cargo test -p miner-cli --test sigint_mid_sweep`) but failed once under workspace-parallel execution with a `SweepSummary` ordering race ("kinds: [..., 'sweep_summary', 'run_end'] — SweepSummary MUST be suppressed when SIGINT lands mid-sweep"). Unrelated to Plan 07-09 — the failing test exercises Phase 5 SIGINT + sweep machinery; the new snapshot test does not touch SIGINT or sweep code paths.

## Threat Flags

None — the new test file consumes the existing public surface (`Finding`, `RunStart`, `RunEnd`, `RunSummary`, `RunId`, `BufferSink`, `common::mask_volatile_fields`) and writes a single read-only golden file. No new network endpoints, auth paths, file-access patterns, or schema changes at trust boundaries.

## TDD Gate Compliance

Plan-level TDD (frontmatter `type: execute` with task-level `tdd="true"`) requires a RED commit (test) and a GREEN commit (feat/impl). Per the executor's TDD plan-level guidance, RED + GREEN may collapse into a single commit when the test is the deliverable AND the golden file is part of the test's own state machine (the regen helper writes the golden ONCE; the byte-equal test then runs against it).

The single commit `fb16ee2` is typed `test(07-09): ...` reflecting the test-first nature of the task. A separate GREEN commit would have been required only if the deliverable were a production code change validated by the test — which is not the case here (the deliverable IS the test + the golden it locks against).

The plan-level TDD compliance is satisfied by:
1. The test was authored against an EMPTY golden first (RED — confirmed: `envelope_snapshot_matches_golden` failed with `left == "..." right == ""`).
2. The regen helper populated the golden (GREEN — confirmed: all 3 active tests passed afterward).
3. Both steps are documented in this Summary's `Task Commits` note.

## User Setup Required

None — the snapshot test runs entirely in-process and reads the committed golden via `include_str!`. No external services, environment variables, or dashboard configuration.

## Phase 7 Status

**Phase 7 is COMPLETE.** This is the final plan in Phase 7 (9/9). Together with Plan 07-01 (family goldens), Plan 07-05 (noise-replay regression), and Plans 07-02 / 07-03 / 07-04 / 07-06 / 07-07 / 07-08 (CI hardening / supply-chain / doc-lint / benches / data-source caveats), the Phase 7 deliverables now address ROADMAP Phase 7's five success criteria:

1. **Byte-determinism gate (golden-file regression suite + locked envelope snapshot)** — CLOSED by Plans 07-01 + 07-05 + 07-09.
2. **CI hardening (deny warnings, supply-chain gates)** — addressed by Plans 07-02 + 07-03 + 07-04.
3. **Documentation completeness (data-source caveats, agent integration)** — addressed by Plans 06-* (Phase 6 docs) + Plan 07-07.
4. **Benchmark harness (criterion + miner-bench)** — Plan 07-06 (concurrent worktree; merging soon).
5. **Reproducibility envelope round-trip** — addressed by Plans 05-* + the envelope snapshot now pinned here.

Run `/gsd-complete-milestone` once Plan 07-06 merges to ceremonially close v1.0.

## Next Phase Readiness

- ✅ All Phase 7 plans complete or in-flight (07-06 in concurrent worktree).
- ✅ Phase 7 verifier can run a final pass over PHASE-7 success criteria.
- ✅ `/gsd-complete-milestone` → v1.0 release ceremony unblocked once 07-06 merges.

---

## Self-Check: PASSED

Verified the SUMMARY's claims against the filesystem and git history:

```
[FOUND] crates/miner-core/tests/findings_envelope_snapshot.rs
[FOUND] crates/miner-core/tests/goldens/envelope_snapshot.jsonl
[FOUND] commit fb16ee2 (test(07-09): land locked findings-envelope snapshot test)
[FOUND] 3 active + 1 ignored test (grep -c '#\[test\]' returns 4; one #[ignore] attribute on the regen helper at line 199 — `grep -c '#\[ignore'` returns 2 because of an `#[ignore]` substring inside a doc comment on line 29; only the line-199 attribute is a real Rust attribute)
[FOUND] golden contains <masked_run_id>, <masked_started_at_utc>, <masked_ended_at_utc> sentinels
[FOUND] cargo test -p miner-core --test findings_envelope_snapshot passes (3 passed, 1 ignored)
[FOUND] cargo test -p miner-core --test scan_summary_welford --test scan_engle_granger --test scan_seas_hour_of_day passes
```

---
*Phase: 07-hardening-benchmarks-reproducibility*
*Completed: 2026-05-22*
