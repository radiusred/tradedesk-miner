---
phase: 02-reader-aggregator-derived-bar-cache
plan: 03
subsystem: testing
tags: [rust, integration-test, aggregator, dst, calendar, edge-cases, fixtures, btreemap, mockreader]

# Dependency graph
requires:
  - phase: 02-reader-aggregator-derived-bar-cache (Plan 02-01)
    provides: "Reader trait + RawBar + Side + ClosedRangeUtc + Blake3Hex (FROZEN re-export surface)."
  - phase: 02-reader-aggregator-derived-bar-cache (Plan 02-02)
    provides: "miner_core::aggregator::aggregate kernel + Timeframe + AggParams + BarFrame + AggregateError; shared MockReader + build_24h_1m_bars + build_partial_day_1m_bars + day_start_utc helpers at tests/aggregator_fixtures.rs; Calendar::fx_major (used only as background context — the aggregator itself is calendar-blind)."
provides:
  - "crates/miner-core/tests/dst_spring_forward.rs — 3 #[test] fns pinning the 2024-03-31T01:00:00Z London spring-forward UTC instant across Tf15m / Tf1h / Tf1d (T-02-09 mitigation)."
  - "crates/miner-core/tests/dst_fall_back.rs — 3 #[test] fns pinning the 2024-10-27T01:00:00Z London fall-back UTC instant across Tf15m / Tf1h / Tf1d (T-02-09 mitigation)."
  - "crates/miner-core/tests/aggregator_edge_cases.rs — 5 #[test] fns: weekend_gap_emits_no_bars, christmas_day_emits_no_bars, instrument_first_cache_day, instrument_last_cache_day, partial_bar_session_open (T-02-10 mitigation, D2-19 partial-bar contract)."
affects:
  - "02-04 gap detector (the partial_bar_session_open test pins the aggregator-side OHLCV behaviour; Plan 04 owns the gap-manifest entry that flags >50% missing sub-minutes)."
  - "02-05 derived-bar cache (the DST tests are the regression gate for any future bucketing-strategy refactor that the cache writer depends on for byte-identity)."
  - "02-06 phase finalisation (CACHE-04 success criterion 5 — DST spring/fall + 4 enumerated edge cases — is now CLOSED on the aggregator side)."

# Tech tracking
tech-stack:
  added: []  # No new workspace deps; chrono + MockReader fixtures + aggregator landed in Plans 02-01 / 02-02.
  patterns:
    - "Integration-test target enforces FROZEN public-surface usage: imports via `use miner_core::{aggregate, AggParams, ...}` only — never `miner_core::aggregator::*` direct paths."
    - "Shared MockReader substrate consumed via `mod aggregator_fixtures;` across three sibling integration files — Plan 02-02 owns the fixture module, Plan 02-03 consumes it; the unit-test fixture in src/aggregator.rs is intentionally duplicated (unit/integration boundary)."
    - "DST regression gates use UTC-monotonic + UTC-spacing assertions: `frame.ts_open_utc[i] - frame.ts_open_utc[i-1] == tf.duration()` catches both spring-forward (skipped-hour) and fall-back (repeated-hour) failure modes in one shape."
    - "Fall-back duplicate-hour gate: count occurrences of the transition `ts_open_utc` and assert == 1 — guards the failure mode where the repeated wall-clock hour leaks into UTC bucketing as two distinct bars at the same UTC instant."
    - "Edge-case fixture inputs use `build_partial_day_1m_bars(start, count, open)` (Plan 02-02 helper) to construct sparse / aligned bar sequences without polluting the test with bar-by-bar literals."
    - "`#![allow(clippy::float_cmp)]` at the test-file level is acceptable when the aggregator field is a verbatim pass-through of a source-bar f64 (open / high / low / close are selected, never arithmetically combined — except tick_volume sum, which uses tolerance assertions)."

key-files:
  created:
    - "crates/miner-core/tests/dst_spring_forward.rs (208 lines: 3 #[test] fns covering Tf15m / Tf1h / Tf1d for the 2024-03-31 transition; pulls in `mod aggregator_fixtures;` for MockReader + helpers)"
    - "crates/miner-core/tests/dst_fall_back.rs (225 lines: 3 #[test] fns covering Tf15m / Tf1h / Tf1d for the 2024-10-27 transition; additional duplicate-hour assertions absent from spring-forward)"
    - "crates/miner-core/tests/aggregator_edge_cases.rs (393 lines: 5 #[test] fns — weekend / christmas / first-day / last-day / partial-bar)"
  modified: []  # Tests-only plan; no source file changes.

key-decisions:
  - "DST tests rely on the aggregator being UTC-only — the fixture data is plain UTC 1m bars with NO holes, and the assertion is `ts_open_utc[i] - ts_open_utc[i-1] == tf.duration()`. The test name 'DST' is informational; the kernel sees the transition as ordinary contiguous UTC data, which is exactly the property the test exercises."
  - "Fall-back test adds an explicit `count_at_transition == 1` assertion that spring-forward does NOT need: spring-forward's failure mode is a `2h` delta between consecutive bars (caught by the spacing assertion alone); fall-back's failure mode is two bars at the same `ts_open_utc` (caught only by the count assertion). Keeping the asymmetry visible in the test code documents the asymmetry of the failure surface."
  - "weekend_gap_emits_no_bars constructs the source data such that the closed-hours window contains zero source 1m bars (matching the real Dukascopy export behaviour). The aggregator's gap-omission rule applies regardless of whether the absence is calendar-driven (weekend) or instrument-driven (cache boundary) — both surface as 'no source bars → no output bar'."
  - "Plan 02-03 deliberately does NOT test the gap-manifest side of weekend / holiday / partial-bar — that's Plan 02-04's snapshot test. The aggregator's responsibility is bar emission; the manifest's responsibility is closed-hours vs gap classification. Each plan tests its own contract surface."
  - "Used `#![allow(clippy::float_cmp)]` in aggregator_edge_cases.rs for the partial-bar test rather than per-assertion tolerance comparisons. The aggregator's `open` / `high` / `low` / `close` are pass-through selections from source f64 fields (no arithmetic), so byte-equality is the correct semantic — matches the precedent in `tests/aggregator_determinism.rs:21` where byte-equality IS the point of the test. The `tick_volume` sum still uses tolerance (`(a - b).abs() < 1e-9`) because it IS arithmetic."

patterns-established:
  - "Integration-only fixture tests imported through `mod aggregator_fixtures;`. The shared module under `tests/` is the canonical pattern for downstream phases that need MockReader-driven aggregator tests without crossing the unit/integration boundary in src/."
  - "DST regression-gate shape: build a 3-day UTC-contiguous dataset around the transition; aggregate at every supported timeframe; assert (a) total bar count, (b) uniform UTC spacing across all bars, (c) exact transition-instant existence + immediate-next-bar timestamp. Reusable for any future DST-class transition (Sydney, NY, Tokyo)."
  - "Fall-back specific guard: `iter().filter(|t| **t == transition).count() == 1` catches duplicate UTC entries that uniform-spacing assertions would miss when spacing is `Duration::zero()` between two equal timestamps (the spacing check would fail with delta=0, but the count check is more explicit about WHAT failed)."

requirements-completed:
  - CACHE-04

# Metrics
duration: ~25min
completed: 2026-05-18
---

# Phase 02 Plan 03: DST + edge-case integration tests Summary

**11 integration-test functions across 3 sibling test files that pin the aggregator's UTC-only contract (DST invisibility) and gap-omission semantics (weekend / holiday / instrument cache boundary / partial session open), closing CACHE-04 success criterion 5 on the aggregator side.**

## Performance

- **Duration:** ~25 min
- **Completed:** 2026-05-18
- **Tasks:** 3 (atomic commits)
- **Files created:** 3 (`tests/dst_spring_forward.rs`, `tests/dst_fall_back.rs`, `tests/aggregator_edge_cases.rs`)
- **Tests added:** 11 (3 spring-forward + 3 fall-back + 5 edge-case)
- **Lines of test code:** 826

## Accomplishments

- **DST regression gates closed.** Plan 02-02 shipped the UTC-only `aggregator::aggregate` kernel; Plan 02-03 makes any future localtime leak fail at the test layer. The 2024 London spring-forward (`2024-03-31T01:00:00Z`) and fall-back (`2024-10-27T01:00:00Z`) transitions are pinned across all three supported timeframes (15m / 1h / 1d), with uniform-UTC-spacing assertions guarding the boundary.
- **Edge-case enumeration complete.** CACHE-04 success criterion 5 enumerates DST × 2 + weekend gap + holiday + instrument first cache day + instrument last cache day + partial-bar session open. All 7 enumerated cases now have aggregator-side coverage (DST owns 6 of the 11 tests; the edge-case file owns the remaining 5). Plan 02-04 will close the gap-manifest side of weekend / holiday / partial-bar via a snapshot test.
- **Fixture reuse validated.** The shared `aggregator_fixtures::MockReader` + `build_24h_1m_bars` + `build_partial_day_1m_bars` substrate (owned by Plan 02-02 Task 2) drove all 11 tests without modification — confirming the substrate is sufficient for the downstream plans that depend on it (Plans 02-04 + 02-05 will consume it again).
- **FROZEN public surface honoured.** All three test files import exclusively via `miner_core::*` (no `miner_core::aggregator::*` direct paths), enforcing Plan 02-06's public-surface audit prerequisite at test-compile time.
- **Workspace clippy clean.** `cargo clippy --workspace --all-targets -- -D warnings` exits 0 after the test additions, including `clippy::float_cmp`, `clippy::doc_markdown`, and `clippy::doc_overindented_list_items` lints that all needed targeted handling.

## Task Commits

Each task was committed atomically on `worktree-agent-a8742e64af6b5b13c` (base `9c265e9`):

1. **Task 1: DST spring-forward fixture** — `0283daa` (test)
2. **Task 2: DST fall-back fixture** — `6e2b3ec` (test)
3. **Task 3: Aggregator edge cases (weekend / christmas / first day / last day / partial session open)** — `20a06ae` (test)

Final metadata commit: this plan's SUMMARY.md.

## Files Created

- `crates/miner-core/tests/dst_spring_forward.rs` (208 lines) — 3 tests pinning the 2024 London spring-forward UTC instant across Tf15m / Tf1h / Tf1d. Builds a 3-day MockReader (Sat 30 / Sun 31 / Mon 1 Mar 2024) with 4320 contiguous UTC 1m bars; asserts uniform `tf.duration()` spacing through the transition and pins the immediate-next-bar timestamp.
- `crates/miner-core/tests/dst_fall_back.rs` (225 lines) — 3 tests mirroring spring-forward for the 2024-10-27 fall-back transition. Adds explicit `count_at_transition == 1` assertions absent from spring-forward (the failure modes are asymmetric).
- `crates/miner-core/tests/aggregator_edge_cases.rs` (393 lines) — 5 tests covering weekend gap (Fri 22:00 UTC → Sun 22:00 UTC), Christmas Day holiday, instrument first cache day (range starts before earliest data), instrument last cache day (range ends after latest data), and partial-bar session open (D2-19: bucket with <50% sub-minutes present, bar IS emitted with whatever values are there).

## Test Results

```
$ cargo test --workspace --test dst_spring_forward --test dst_fall_back --test aggregator_edge_cases

running 3 tests
test bars_evenly_spaced_across_spring_forward ... ok
test bars_evenly_spaced_across_spring_forward_1h ... ok
test bars_evenly_spaced_across_spring_forward_1d ... ok
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

running 3 tests
test bars_evenly_spaced_across_fall_back ... ok
test bars_evenly_spaced_across_fall_back_1h ... ok
test bars_evenly_spaced_across_fall_back_1d ... ok
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

running 5 tests
test weekend_gap_emits_no_bars ... ok
test christmas_day_emits_no_bars ... ok
test instrument_first_cache_day ... ok
test instrument_last_cache_day ... ok
test partial_bar_session_open ... ok
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

All 11 tests pass. `cargo clippy --workspace --all-targets -- -D warnings` exits 0.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Workspace clippy lints on the partial-bar test file**

- **Found during:** Task 3 — initial `cargo clippy --workspace --all-targets -- -D warnings` failed with 9 errors after writing `aggregator_edge_cases.rs`.
- **Issues:**
  - `clippy::doc_markdown` × 2: `/// MockReader has ...` missing backticks on `MockReader`.
  - `clippy::doc_overindented_list_items` × 3: a paragraph beginning with `>50%` after a doc comment was parsed by clippy as a Markdown blockquote; subsequent lines without `>` markers tripped the lint.
  - `clippy::float_cmp` × 4: `assert_eq!` on f64 values for `open` / `high` / `low` / `close` / `tick_volume`.
- **Fixes:**
  - Wrapped `MockReader` in backticks where it appears at the start of a doc paragraph.
  - Wrapped `>50%` in backticks (`` `>50%` ``) — defangs the blockquote-marker interpretation.
  - Added a file-level `#![allow(clippy::float_cmp)]` with an explanatory comment (the aggregator's `open` / `high` / `low` / `close` are pass-through selections from source f64 fields, so byte-equality is the correct semantic; matches the precedent in `tests/aggregator_determinism.rs:21`). The `tick_volume` sum assertion uses tolerance comparison (`(a - b).abs() < 1e-9`) because it IS arithmetic.
- **Files modified:** `crates/miner-core/tests/aggregator_edge_cases.rs` (within the same Task 3 commit).
- **Commit:** `20a06ae` (Task 3 commit incorporates the fixes pre-commit).

No other deviations. Plan executed exactly as written for Tasks 1 and 2; Task 3 required the clippy fixes above to satisfy the plan's success criterion 5 (`cargo clippy --workspace --all-targets -- -D warnings exits 0`).

## Verification Against Plan Success Criteria

| # | Criterion | Result |
|---|-----------|--------|
| 1 | 3 DST spring-forward tests + 3 DST fall-back tests pass | OK (6/6 pass) |
| 2 | 5 edge-case tests (weekend, christmas, first day, last day, partial session open) pass | OK (5/5 pass) |
| 3 | All tests run via the public `miner_core::*` re-export surface | OK (no `miner_core::aggregator::*` paths in any of the 3 test files) |
| 4 | All tests use the shared `aggregator_fixtures::MockReader` from Plan 02 | OK (all 3 files `mod aggregator_fixtures;`) |
| 5 | `cargo clippy --workspace --all-targets -- -D warnings` exits 0 | OK |

## Threat Model Coverage

| Threat ID | Status | Evidence |
|-----------|--------|----------|
| T-02-09 (Tampering — localtime leak into the aggregator) | mitigated | 6 DST tests across Tf15m/Tf1h/Tf1d for both spring-forward and fall-back assert uniform UTC spacing through the transition. A localtime leak would fail at the first `frame.ts_open_utc[i] - frame.ts_open_utc[i-1] == tf.duration()` assertion across the transition. |
| T-02-10 (Tampering — aggregator silently emitting/dropping bars near range boundaries) | mitigated | `instrument_first_cache_day` + `instrument_last_cache_day` gate the range-vs-data-boundary behaviour: pre-history days emit no bars, post-history days emit no bars, in-range days emit their bars, no error path triggered. |

## Self-Check

Files created (verified via `[ -f path ] && echo FOUND`):

- FOUND: `crates/miner-core/tests/dst_spring_forward.rs`
- FOUND: `crates/miner-core/tests/dst_fall_back.rs`
- FOUND: `crates/miner-core/tests/aggregator_edge_cases.rs`

Commits exist (verified via `git log --oneline | grep`):

- FOUND: `0283daa` — Task 1 (DST spring-forward)
- FOUND: `6e2b3ec` — Task 2 (DST fall-back)
- FOUND: `20a06ae` — Task 3 (edge cases)

## Self-Check: PASSED
