---
phase: 07-hardening-benchmarks-reproducibility
plan: 07
subsystem: docs
tags: [docs, dukascopy, licensing, data-source, markdown]

# Dependency graph
requires:
  - phase: 07-hardening-benchmarks-reproducibility
    provides: Plan 07-02 — synthetic fixture cache + README ## Example quickstart edits (the insertion-point anchor)
provides:
  - docs/data_sources.md — deep Dukascopy caveats reference (Cache layout / CSV schema / Bid vs ask / Time zones / Gap policies / Licensing posture)
  - README ## Data source caveats section — 6-line summary linking to the deep doc
affects: [07-08-performance-readme, future-readers, agent-onboarding]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "docs/.license-footer.md byte-identical paste pattern at file tail (PATTERNS Pattern A)"
    - "Sectional H2 layout mirroring docs/agent_integration.md"
    - "Verified-against-commit-SHA pin for cross-repo doc accuracy (RESEARCH Open Question 6)"

key-files:
  created:
    - docs/data_sources.md
  modified:
    - README.md

key-decisions:
  - "Title chosen as `# Dukascopy data source caveats` (more specific than the generic `# Data source caveats`) — picks the explicit-disambiguation option since miner may grow non-Dukascopy readers later (the Reader trait is pluggable per ARCHITECTURE.md)."
  - "Licensing-posture pin lands as `Verified against tradedesk-dukascopy commit f218d41 (2026-05-13)` — sibling repo /home/darren/projects/radiusred/tradedesk-dukascopy/ was a live git repo on the executor's machine; no omission note needed."
  - "README ## Data source caveats inserted between the ## Example block (ending with the `For the full catalogue...` paragraph) and ## Design principles — preserves the visual flow Example → caveats → principles."

patterns-established:
  - "Cross-repo verification pin: docs that cite a sibling repo's conventions include a `Verified against <repo> commit <sha> (<date>)` line so future drift is detectable. Re-run `git -C <sibling-repo> log -1 --format='%h %ad' --date=short` and update at each phase-7-style hardening pass."
  - "Apache-2.0 footer is sourced from docs/.license-footer.md via byte-identical paste (verified via `diff -q <(tail -8 <doc>) <(cat docs/.license-footer.md)` exiting 0)."

requirements-completed: []

# Metrics
duration: ~12min
completed: 2026-05-22
---

# Phase 07 Plan 07: Dukascopy data-source caveats (D7-02) Summary

**New `docs/data_sources.md` deep reference (six required sections covering cache layout, CSV schema, bid/ask independence, time zones + DST, gap policies, and licensing posture) plus a 6-line README `## Data source caveats` summary block linking to it.**

## Performance

- **Duration:** ~12 min
- **Started:** 2026-05-22 (sequential executor)
- **Completed:** 2026-05-22
- **Tasks:** 2
- **Files modified:** 2 (1 created, 1 modified)

## Accomplishments

- Authored `docs/data_sources.md` (283 lines incl. footer) documenting all six required sections from D7-02 with ground-truth citations to `crates/miner-reader-dukascopy/src/{path_layout,reader}.rs`, `crates/miner-core/tests/dst_{spring_forward,fall_back}.rs`, and `tradedesk-dukascopy/tradedesk_dukascopy/export.py` (tick-volume semantic at line ~312).
- Pinned the licensing-posture verification line to `tradedesk-dukascopy` commit `f218d41 (2026-05-13)` per RESEARCH Open Question 6 (sibling repo was a live git repo on the executor's machine; no omission fallback needed).
- Added README `## Data source caveats` 6-line summary between `## Example` and `## Design principles`, byte-identical to the canonical body in D7-02 / 07-CONTEXT.md.
- Apache-2.0 footer in `docs/data_sources.md` is byte-identical to `docs/.license-footer.md` (verified via `diff -q <(tail -8 ...) <(cat docs/.license-footer.md)` exiting 0).

## Task Commits

Each task was committed atomically:

1. **Task 1: Author docs/data_sources.md** — `b583953` (docs)
2. **Task 2: Insert README ## Data source caveats section** — `3619e30` (docs)

## Files Created/Modified

- `docs/data_sources.md` (CREATED, 283 lines) — Deep Dukascopy caveats reference with H1 title, intro, six required H2 sections (Cache layout, CSV schema, Bid vs ask independence, Time zones and DST, Gap policies, Licensing posture), a `## See Also` related-docs trailer, and a byte-identical Apache-2.0 footer pasted from `docs/.license-footer.md`. Cites all four ground-truth sources (reader, parser, DST tests, upstream Dukascopy terms).
- `README.md` (MODIFIED, +14 lines) — New `## Data source caveats` section inserted between `## Example` and `## Design principles`. Body matches the canonical D7-02 text verbatim. Plan 07-02's quickstart edits (`./tests/fixtures/cache`, `seas.bucket.hour_of_day@1`) are preserved.

## Decisions Made

- **Title disambiguation:** Used `# Dukascopy data source caveats` for the deep doc's H1 (rather than the README's generic `## Data source caveats`). Rationale: the `Reader` trait is pluggable per ARCHITECTURE.md, so a future non-Dukascopy reader would need its own caveats doc — keeping the title source-specific avoids a future rename. The README link target stays `docs/data_sources.md` (file-name level, not heading level) so no anchor adjustment is needed.
- **Licensing-posture pin:** Captured `f218d41 (2026-05-13)` from the live tradedesk-dukascopy git repo; documented inline as `Verified against tradedesk-dukascopy commit f218d41 (2026-05-13).` No omission fallback triggered.
- **CSV schema table:** Used a markdown table (Column / CSV type / In-memory type / Meaning / Source) rather than per-column `###` subsections — denser, easier to scan, matches the explicit "Tables welcome for the CSV schema column list" guidance in Task 1's action body.
- **README insertion point:** Placed the new section AFTER the Example block's closing `For the full catalogue ...` paragraph and BEFORE `## Design principles` — preserves the Example → caveats → principles narrative flow.

## Deviations from Plan

None — plan executed exactly as written. All acceptance-criteria checks (six required sections present, footer byte-identical to canonical, 00-indexed / tick-count / synthetic / Dukascopy URL / reader path / DST test citations present, line count 283 in [130, 320] range, no emojis, README links to docs/data_sources.md, README appears after `## Example`, cargo build clean) passed first time.

The plan's Task 1 read-list mentioned `crates/miner-reader-dukascopy/src/parser.rs` but that file does not exist in the current tree — the CSV parsing logic lives entirely inside `reader.rs` (the `RawRow` `serde::Deserialize` struct + the `csv_reader.into_deserialize::<RawRow>()` pipeline inside `day_bar_iter`). The doc cites `reader.rs` accordingly, which is the actual ground-truth source. No deviation rule triggered (Rule 3 doesn't apply — the citation lives in the right place; the plan's list just had a stale filename).

## Issues Encountered

None.

## User Setup Required

None — pure documentation change, no environment configuration required.

## Next Phase Readiness

- Plan 07-07 (D7-02) shipped clean; no follow-ups needed.
- The README `## Data source caveats` section is in place AHEAD of Plan 07-08's planned `## Performance` pointer (per the plan's Wave-3 sequencing note: "Do NOT add the `## Performance` pointer in this plan — Plan 07-08 owns that addition"). Insertion point for 07-08 is between `## Data source caveats` and `## Design principles`.
- Verification SHA may need refreshing at the Phase 7 close pass if `tradedesk-dukascopy` cuts a new release before Phase 7 wraps; the verification recipe is `git -C /home/darren/projects/radiusred/tradedesk-dukascopy log -1 --format='%h %ad' --date=short` and the line lives in `docs/data_sources.md` under `## Licensing posture` → `### Verification pin`.

## Self-Check: PASSED

- `docs/data_sources.md` exists (283 lines).
- README `## Data source caveats` section present.
- Commits `b583953` and `3619e30` exist in `git log --all --oneline`.
- Apache-2.0 footer byte-identical to `docs/.license-footer.md` (`diff -q` exits 0).
- `cargo build --workspace` finishes clean (sanity).

---

*Phase: 07-hardening-benchmarks-reproducibility*
*Plan: 07*
*Completed: 2026-05-22*
