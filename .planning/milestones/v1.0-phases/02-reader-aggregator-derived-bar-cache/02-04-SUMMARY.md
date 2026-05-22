---
phase: 02-reader-aggregator-derived-bar-cache
plan: 04
subsystem: infra
tags: [rust, gap-detector, gap-manifest, schemars, insta, proptest, btreemap, determinism, fx-major]

# Dependency graph
requires:
  - phase: 02-reader-aggregator-derived-bar-cache (Plan 02-01)
    provides: "Reader trait + RawBar + Side + ClosedRangeUtc + Blake3Hex; insta + proptest dev-deps."
  - phase: 02-reader-aggregator-derived-bar-cache (Plan 02-02)
    provides: "Calendar::fx_major() + Calendar::is_open_at predicate — the open/closed-hours discriminator the gap detector reuses verbatim."
provides:
  - "miner_core::gap::GapManifest — Serialize + Deserialize + JsonSchema; queried_range reuses Phase 1 TimeRange; gaps Vec sorted by (start_utc, end_utc, GapReason::discriminant_ord)."
  - "miner_core::gap::GapSpan — half-open [start_utc, end_utc) plus a GapReason."
  - "miner_core::gap::GapReason — #[serde(tag = \"kind\", rename_all = \"snake_case\")] tagged enum with MissingSourceFile / CorruptSourceFile / IntraDayGap variants. Wire form: missing_source_file / corrupt_source_file / intra_day_gap."
  - "miner_core::gap::GapDetector — pure-function `detect(&R: Reader, symbol, side, range) -> Result<GapManifest, R::Error>`. Skips fully-closed days via Calendar::is_open_at; honours D2-17 redundancy rule (missing/corrupt day does NOT also emit per-minute IntraDayGap)."
  - "GapDetector / GapManifest / GapReason / GapSpan re-exported through the FROZEN miner_core::* public surface."
  - "crates/miner-core/tests/gap_manifest_snapshot.rs + snapshots/gap_manifest_snapshot__gap_manifest_json_shape_pinned.snap — insta pin of the JSON wire form."
  - "crates/miner-core/tests/snapshots/.gitkeep — directory committed before any future snapshot tests land."
affects:
  - "02-05 derived-bar cache: optional consumer of GapManifest for invalidation-related diagnostics; main coupling is the BTreeMap discipline already shared."
  - "02-06 phase finalisation: public-surface audit verifies gap::* re-exports; full-determinism harness will exercise the detector against a real SyntheticCache day set."
  - "Phase 3 scan engine: wraps GapManifest into Finding::GapAborted under --gap-policy=strict (CACHE-08 enforcement)."

# Tech tracking
tech-stack:
  added: []  # All workspace deps already declared by Plan 02-01.
  patterns:
    - "Tagged-enum JSON shape (#[serde(tag = \"kind\", rename_all = \"snake_case\")]) reused from Phase 1's Finding envelope (findings/mod.rs:293-301). Adding a new GapReason variant MUST append to discriminant_ord values to preserve byte-stability of existing JSON outputs."
    - "Stable discriminant_ord() helper on a tagged enum — single-source-of-truth for the (start, end, discriminant) sort key. Pinned by gaps_sorted_proptest."
    - "TimeRange reuse — GapManifest.queried_range = Phase 1's TimeRange (not a redefinition). The ClosedRangeUtc -> TimeRange translation happens once at the detector boundary so downstream consumers see a single time-window type on the wire."
    - "Inline LocalMockReader in #[cfg(test)] mod tests, distinct from the public tests/aggregator_fixtures.rs::MockReader. Mirrors Phase 1's sink.rs:399-409 unit/integration test fixture split pattern — unit tests cannot import code from `tests/` integration targets."
    - "Insta snapshot pinning the on-wire JSON. .snap.new -> .snap acceptance workflow; committed `.snap` file makes future drift a CI gate. Wave 0 introduction (no Phase 1 analog)."
    - "Proptest-driven sort invariant verification — gates a future refactor that breaks the sort without coupling to the full detect() pipeline."

key-files:
  created:
    - "crates/miner-core/src/gap.rs (~470 lines: GapManifest + GapSpan + GapReason + GapDetector::detect + 5 unit tests + 1 proptest, plus inline LocalMockReader)"
    - "crates/miner-core/tests/gap_manifest_snapshot.rs (insta::assert_json_snapshot! pin)"
    - "crates/miner-core/tests/snapshots/gap_manifest_snapshot__gap_manifest_json_shape_pinned.snap (committed insta snapshot)"
    - "crates/miner-core/tests/snapshots/.gitkeep (directory anchor)"
  modified:
    - "crates/miner-core/src/lib.rs (pub mod gap; + Plan 02-04 FROZEN re-exports: GapDetector + GapManifest + GapReason + GapSpan)"
    - "crates/miner-core/Cargo.toml (enable insta's `json` feature for assert_json_snapshot!)"
    - "Cargo.lock (resolver output for the insta `json` feature transitively pulling serde_json's existing graph — no new top-level deps)"

key-decisions:
  - "Reuse Phase 1's TimeRange for GapManifest.queried_range — do NOT redefine. TimeRange already derives Serialize + JsonSchema; redefining would split the schema surface and complicate Phase 3's GapAbortedFinding wrapping."
  - "Inline LocalMockReader in src/gap.rs's #[cfg(test)] mod tests, separate from tests/aggregator_fixtures.rs::MockReader. VALIDATION.md mandates unit-test paths under gap::tests::*; unit tests cannot import integration `tests/` modules. The duplication is the documented Phase 1 precedent (sink.rs:399-409)."
  - "Option A for corrupt-file detection (read_1m_bars Err on first poll = corrupt). Option B (explicit ZERO_BYTE_SENTINEL on Blake3Hex) would add API surface to the Reader contract for a v1 detection that works without it. Documented as a Phase 7 TODO in gap.rs."
  - "No coalescing of adjacent IntraDayGap entries in v1 — per RESEARCH §'Gap Manifest Data Model' L865. Each missing open-hours minute = one entry with affected_minutes = 1. Simpler is correct; consumers can merge if they need to."
  - "GapDetector is a unit struct (no fields, no configuration). detect() is a pure associated function, mirroring miner_core::aggregator::aggregate's pure-function pattern. Phase 2's gap detector has zero state; the calendar comes from the Reader, the timestamps come from the bars."
  - "Boundary-handling for half-open ranges: range.end at exactly UTC midnight excludes that date; any time past midnight includes it. Documented in enumerate_dates and mirrors the half-open [start, end) convention from ClosedRangeUtc / TimeRange (Phase 1)."

patterns-established:
  - "Tagged-enum JSON shape on Phase 2 data types — #[serde(tag = \"kind\", rename_all = \"snake_case\")] (mirrors Finding::* in findings/mod.rs:293-301). Adding a new variant requires appending (not inserting) the discriminant_ord value to keep existing JSON outputs byte-stable."
  - "Stable-sort tie-breaker via discriminant_ord() — when two entries share a primary key, the discriminant ordinal breaks the tie deterministically. Encoded once in gap.rs, verified by gaps_sorted_proptest."
  - "Read-only Reader trait consumer pattern — GapDetector::detect takes a generic R: Reader, mirroring miner_core::aggregator::aggregate. Same one-way arrow (gap -> reader -> calendar). Phase 3+ readers plug in without code changes here."
  - "Insta snapshot acceptance via committed .snap file. cargo insta accept (or manual .snap.new -> .snap rename) is the workflow; CI treats subsequent drift as a hard failure."

requirements-completed:
  - CACHE-07
  - CACHE-08

# Metrics
duration: ~30min
completed: 2026-05-18
---

# Phase 02 Plan 04: GapDetector + GapManifest Summary

**Wave 1 gap-detection data model: `miner_core::gap` exports `GapDetector` (pure-function `detect`), the `GapManifest`/`GapSpan`/`GapReason` types, and the insta-pinned JSON wire form. CACHE-07 closed; CACHE-08 ships the type Phase 3 will emit.**

## Performance

- **Duration:** ~30 min
- **Completed:** 2026-05-18
- **Tasks:** 3
- **Files created:** 4 (gap.rs, gap_manifest_snapshot.rs, .gitkeep, .snap)
- **Files modified:** 3 (lib.rs, Cargo.toml, Cargo.lock)
- **Tests added:** 7 (5 inline `gap::tests::*` unit tests + 1 proptest + 1 insta snapshot integration test)

## Accomplishments

- **`GapDetector::detect`** is the pure-function entry point — `(reader, symbol, side, range)` → `Result<GapManifest, R::Error>`. Walks every UTC date in the half-open query range, consults the reader's `Calendar` to skip fully-closed days, and classifies missing/corrupt/intra-day gaps in one pass.
- **`GapManifest`** carries `(source_id, symbol, side, queried_range, gaps)`. `queried_range` reuses Phase 1's `TimeRange` (which already derives `JsonSchema`); `gaps` is sorted by `(start_utc, end_utc, GapReason::discriminant_ord)` with the invariant verified by `gaps_sorted_proptest`.
- **`GapReason`** is a `#[serde(tag = "kind", rename_all = "snake_case")]` tagged enum — three variants (`MissingSourceFile`, `CorruptSourceFile`, `IntraDayGap`) mirroring the workspace's canonical tagged-enum idiom from `Finding` in `findings/mod.rs:293-301`. Wire form pinned by the insta snapshot.
- **D2-17 redundancy rule** honoured: when a day is missing (no source file) or corrupt (read errors), no per-minute `IntraDayGap` entries are emitted for it — the whole-day `MissingSourceFile`/`CorruptSourceFile` span subsumes them.
- **Closed-hours-are-not-gaps invariant** locked by a dedicated unit test against Saturday 2024-06-15 with no data: `manifest.gaps` is empty because `Calendar::is_open_at` returns `false` for every minute.
- **6 unit tests + 1 integration snapshot test** all enumerated by their full paths (W1 fix in the plan: no umbrella `gap::tests` filter that would silently pass if a test were dropped).
- **`.gitkeep` discipline** — the `tests/snapshots/` directory has a tracked `.gitkeep` from day one, so future snapshot tests (Plan 02-05's arrow_schema_snapshot) land into a committed home without first-run friction.
- **Public surface extended** — `pub use gap::{GapDetector, GapManifest, GapReason, GapSpan}` joins the FROZEN re-export block in `crates/miner-core/src/lib.rs`. Phase 3's scan engine imports through this surface.

## Task Commits

Each task was committed atomically (worktree branch `worktree-agent-aac5a7b2987e0061a`, base `82753e58`):

1. **Task 1: GapManifest/GapSpan/GapReason types + schemars roundtrip** — `9328739` (feat)
2. **Task 2: GapDetector::detect + 4 gap-class unit tests** — `70bd3fc` (feat)
3. **Task 3: gaps_sorted_proptest + insta snapshot + lib.rs re-exports** — `f9c86a1` (feat)

_Plan metadata commit (this SUMMARY) follows._

## Files Created/Modified

### Created

- `crates/miner-core/src/gap.rs` — `GapManifest` + `GapSpan` + `GapReason` (tagged enum with `discriminant_ord` helper) + `GapDetector::detect` + inline `LocalMockReader` (for unit tests) + 5 unit tests + 1 proptest
- `crates/miner-core/tests/gap_manifest_snapshot.rs` — `insta::assert_json_snapshot!` pin
- `crates/miner-core/tests/snapshots/gap_manifest_snapshot__gap_manifest_json_shape_pinned.snap` — the committed snapshot
- `crates/miner-core/tests/snapshots/.gitkeep` — directory anchor

### Modified

- `crates/miner-core/src/lib.rs` — append `pub mod gap;` + `pub use gap::{GapDetector, GapManifest, GapReason, GapSpan};` (Plan 02-04 re-exports)
- `crates/miner-core/Cargo.toml` — enable `insta = { version = "1.47", features = ["json"] }` so `assert_json_snapshot!` is available
- `Cargo.lock` — resolver output for the insta `json` feature (no new top-level workspace deps)

## Decisions Made

See `key-decisions` in frontmatter. Highlights:

- **Reuse Phase 1's `TimeRange`** for `GapManifest.queried_range` — do NOT redefine. The plan called this out explicitly; the snapshot test verifies the `start_utc`/`end_utc` field names match Phase 1 verbatim.
- **Inline `LocalMockReader`** for the unit tests, separate from the public `tests/aggregator_fixtures.rs::MockReader` Plan 02-02 ships. VALIDATION mandates the unit-test path `gap::tests::*`; unit tests cannot import integration `tests/` modules, so the duplication is the documented Phase 1 precedent (sink.rs:399-409).
- **Option A for corrupt-file detection** — `read_1m_bars` `Err` on first poll = corrupt. Adding an explicit `ZERO_BYTE_SENTINEL` on `Blake3Hex` would expand the Reader API surface for a v1 detection that works without it; deferred to Phase 7 with a TODO in the code.
- **No coalescing of intra-day gaps** — per RESEARCH §"Gap Manifest Data Model" (line 865). One entry per missing open-hours minute, `affected_minutes = 1`. Simpler is correct; consumers can merge if they need to.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] `cargo clippy --workspace --all-targets -- -D warnings` rejected `match reader.fingerprint_day(...)?` with two arms**

- **Found during:** Task 2 (`GapDetector::detect`)
- **Issue:** `cargo clippy` (with pedantic warns) fires `clippy::single_match_else` on a `match Option { None => ..., Some(_) => ... }` form where the `Some` arm is the larger block. The pattern is an equality check on `None` — clippy suggests an `if`/`if let` instead.
- **Fix:** Rewrote as `if reader.fingerprint_day(...)?.is_none() { ... continue; } /* rest is the Some(_) path */`. Same semantics, single-arm flow, clippy clean.
- **Files modified:** `crates/miner-core/src/gap.rs`
- **Verification:** `cargo clippy -p miner-core --lib -- -D warnings` exits 0.
- **Committed in:** `9328739` (Task 1 — fix landed before the unit tests so the file shape was clean from first commit).

**2. [Rule 3 - Blocking] `insta::assert_json_snapshot!` not available without the `json` feature**

- **Found during:** Task 3 (`gap_manifest_snapshot.rs`)
- **Issue:** Plan 02-01 declared `insta = "1.47"` as a dev-dep but did not enable the `json` feature. `insta::assert_json_snapshot!` is gated behind that feature.
- **Fix:** Change `crates/miner-core/Cargo.toml` to `insta = { version = "1.47", features = ["json"] }`. No new top-level workspace dep; `Cargo.lock` was rebuilt for the feature flag.
- **Files modified:** `crates/miner-core/Cargo.toml`, `Cargo.lock`
- **Verification:** `cargo test -p miner-core --test gap_manifest_snapshot` exits 0 after accepting the snapshot.
- **Committed in:** `f9c86a1` (Task 3).

**3. [Rule 1 - Bug] `chrono::Datelike` imported but unused after refactor**

- **Found during:** Task 1 (gap.rs initial draft)
- **Issue:** I initially imported `Datelike` for what I thought would be `.year()` / `.month()` extraction during day enumeration, then refactored to use `date_naive()` directly (which doesn't need the trait). The trailing `Datelike` import then triggered `unused_imports`.
- **Fix:** Drop the `Datelike` import. Removed the placeholder `_check` const that was put in to silence the unused-import warning — clean code is better than allow-listed code.
- **Files modified:** `crates/miner-core/src/gap.rs`
- **Verification:** `cargo build -p miner-core` clean; `cargo clippy` clean.
- **Committed in:** `70bd3fc` (Task 2 — colocated with the test work that exercised the cleaned-up imports).

---

**Total deviations:** 3 auto-fixed (2 × Rule 3 blocking, 1 × Rule 1 cleanup)
**Impact on plan:** All deviations are small structural adjustments needed to honour the workspace's pedantic-clippy gate or to expose existing dev-deps to the test code. No scope creep, no functionality cut. The `insta` `json`-feature flip is the only externally-visible change (a single Cargo.toml line + a Cargo.lock rebuild).

## Threat Surface Audit

Plan `<threat_model>` STRIDE entries are all mitigated as planned:

- **T-02-11 Information Disclosure (gap manifest detail strings):** `GapReason::CorruptSourceFile::detail` is constructed from the reader's `Error::to_string()`. Documented in the module docstring + on the variant; honoured by Plan 02-01's `DukascopyError` (vetted in 02-01 SUMMARY as path + message only, no raw bytes).
- **T-02-12 Tampering (unsorted/duplicate gaps):** `gaps_sorted_proptest` proves the sort invariant on random input; the insta snapshot pins the JSON shape and order; the D2-17 redundancy rule (missing/corrupt day does NOT also emit IntraDayGap) is locked in `missing_file_emits_correct_reason` (Wed has full bars and emits nothing) + a code path that `continue`s after pushing the whole-day span.
- **T-02-13 Repudiation (closed-hours holes silently emitted):** `closed_hours_are_not_gaps` gates against this with a Saturday-only test that asserts `manifest.gaps.is_empty()`. `Calendar::is_open_at` is the single discriminator — no parallel implementation in gap.rs.

No new threat surface introduced outside the plan's threat model.

## Known Stubs

None. The plan's Phase 7 TODO comment about an explicit zero-byte sentinel on `Blake3Hex` is forward-looking, not a stub — the v1 implementation works as documented without it.

## Issues Encountered

None beyond the documented deviations above. No upstream blocker, no environmental issue.

## Self-Check: PASSED

- [x] `crates/miner-core/src/gap.rs` contains `GapManifest`, `GapSpan`, `GapReason` with all derives (`Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema`).
- [x] `GapReason` is `#[serde(tag = "kind", rename_all = "snake_case")]` (`grep -c '#\[serde(tag = "kind"' crates/miner-core/src/gap.rs` = 3).
- [x] `grep -c HashMap crates/miner-core/src/gap.rs` = 0 (BTreeMap discipline intact).
- [x] `cargo test -p miner-core gap::tests::gap_manifest_schemars_roundtrip` exits 0.
- [x] `cargo test -p miner-core gap::tests::missing_file_emits_correct_reason` exits 0.
- [x] `cargo test -p miner-core gap::tests::zero_byte_emits_corrupt` exits 0.
- [x] `cargo test -p miner-core gap::tests::intra_day_hole_during_open_hours` exits 0.
- [x] `cargo test -p miner-core gap::tests::closed_hours_are_not_gaps` exits 0.
- [x] `cargo test -p miner-core gap::tests::gaps_sorted_proptest` exits 0.
- [x] `cargo test -p miner-core --test gap_manifest_snapshot` exits 0 (after `cargo insta accept` equivalent rename).
- [x] `crates/miner-core/tests/snapshots/gap_manifest_snapshot__gap_manifest_json_shape_pinned.snap` exists on disk AND is git-tracked.
- [x] `crates/miner-core/tests/snapshots/.gitkeep` exists on disk AND is git-tracked (`git ls-files --error-unmatch ...` returns the path).
- [x] `crates/miner-core/src/lib.rs` re-exports `GapDetector`, `GapManifest`, `GapReason`, `GapSpan` (1 `pub use gap::` line containing all 4 names).
- [x] Snapshot contains `missing_source_file` / `corrupt_source_file` / `intra_day_gap` (snake-case-tagged with `"kind"` discriminator); does NOT contain `tick_count` / `weekend_closure` / `hashMap`.
- [x] All commits exist in git log: `9328739`, `70bd3fc`, `f9c86a1`.
- [x] `cargo test --workspace` green (no FAILED in any test suite).
- [x] `cargo clippy --workspace --all-targets -- -D warnings` exits 0.
- [x] `cargo tree -p miner-core --edges normal,build` has no `tokio` / `async-std` / `async-trait` (FOUND-04 gate intact — no new deps added that would change this).

## Next Plan Readiness

**Wave 1 Plan 02-04 complete.** Wave 2 (Plans 02-05 / 02-06) can consume Plan 02-04's outputs:

- **Plan 02-05** (derived-bar cache): may consume `GapManifest` for invalidation-related diagnostics, though the main consumer is Phase 3's scan engine. The `tests/snapshots/.gitkeep` directory anchor this plan landed is the home for Plan 02-05's `arrow_schema_snapshot__*.snap` files — no further scaffolding needed.
- **Plan 02-06** (phase finalisation): the public-surface audit will verify `GapDetector` / `GapManifest` / `GapReason` / `GapSpan` are reachable via `use miner_core::*`. The full-determinism test wraps `aggregate` + Arrow IPC; the gap detector is not on that path but its presence in the FROZEN surface is part of the audit.

No blockers. No follow-ups requested. Phase 3 takes over the gap-policy enforcement (`Finding::GapAborted` emission) — `miner_core::findings::GapAbortedFinding` was pre-allocated in Phase 1 specifically for this hand-off.

---
*Phase: 02-reader-aggregator-derived-bar-cache*
*Plan: 04*
*Completed: 2026-05-18*
