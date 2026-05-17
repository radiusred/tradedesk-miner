---
phase: 02
slug: reader-aggregator-derived-bar-cache
status: planned
nyquist_compliant: true
wave_0_complete: false
created: 2026-05-17
plan_phase_completed: 2026-05-17
revision_pass: 1
---

# Phase 02 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.
> Derived from `02-RESEARCH.md` §"Validation Architecture" — keep in sync.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | `cargo test` (built-in) + `cargo-nextest` (optional local parallel runner) |
| **Config file** | none — workspace `Cargo.toml` test layout (each crate's `tests/` directory) |
| **Quick run command** | `cargo test -p miner-core` (or `cargo test -p miner-reader-dukascopy` for reader-only changes) |
| **Full suite command** | `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check` |
| **Estimated runtime** | ~30 seconds quick / ~90 seconds full (Phase 2 baseline, before Phase 7 bench layer) |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p <crate-being-touched>` (cross-crate touches → `cargo test -p miner-core -p miner-reader-dukascopy`)
- **After every plan wave:** Run `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check`
- **Before `/gsd:verify-work`:** Full suite green + tokio-tree gate (no async dep introduced) + schema-sync gate (Phase 1 envelope schema bytes unchanged; `JsonSchema` derives for new `GapManifest` types do NOT appear in `findings-v1.schema.json`)
- **Max feedback latency:** ~30 seconds (per-crate quick run)

---

## Per-Task Verification Map

> Populated by `gsd-planner` during plan-phase. Each task in every `*-PLAN.md` MUST appear as a row here with an `<automated>` verify or an explicit Wave 0 dependency. Rows below are the contract from RESEARCH.md §"Phase Requirements → Test Map". Revision pass 1 confirms all `aggregator::tests::*` paths match the renamed module from Plan 02-02 (file `crates/miner-core/src/aggregator.rs`, `pub mod aggregator;`).

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 02-01-T4 | 02-01 | 0 | CACHE-01 | T-02-02 | DukascopyReader opens a real `.csv.zst` fixture and yields 1m bars in ascending UTC order | integration | `cargo test -p miner-reader-dukascopy --test reader_smoke reads_one_day_in_order` | ⬜ Wave 0 | ⬜ pending |
| 02-01-T4 | 02-01 | 0 | CACHE-01 | T-02-02 | DukascopyReader rejects a zero-byte source file with `CorruptSourceFile` | unit | `cargo test -p miner-reader-dukascopy zero_byte_file` | ⬜ Wave 0 | ⬜ pending |
| 02-01-T2 | 02-01 | 0 | CACHE-02 | — | `Reader` trait is dyn-compatible (inline test in miner-core) | unit | `cargo test -p miner-core reader::tests::reader_trait_object_safe` | ⬜ Wave 0 | ⬜ pending |
| 02-06-T2 | 02-06 | 3 | CACHE-02 | — | `DukascopyReader: Reader` (standalone integration test) | integration | `cargo test -p miner-reader-dukascopy --test reader_trait_object_safety dukascopy_reader_is_dyn_compatible` | ⬜ Wave 0 | ⬜ pending |
| 02-02-T2 | 02-02 | 1 | CACHE-03 | — | Aggregator emits 15m / 1h / 1d bars from 1-day synthetic 1m input | unit | `cargo test -p miner-core aggregator::tests::three_timeframes` | ⬜ Wave 0 | ⬜ pending |
| 02-02-T3 | 02-02 | 1 | CACHE-04 | T-02-05 | Aggregator output is byte-identical across two runs on the same input | integration | `cargo test -p miner-core --test aggregator_determinism byte_identical_two_runs` | ⬜ Wave 0 | ⬜ pending |
| 02-02-T2 | 02-02 | 1 | CACHE-04 | — | OHLC monotonicity: high ≥ open/close, low ≤ open/close (proptest) | property | `cargo test -p miner-core aggregator::tests::ohlc_monotonicity_proptest` | ⬜ Wave 0 | ⬜ pending |
| 02-02-T2 | 02-02 | 1 | CACHE-04 | T-02-06 | Aggregator omits (never interpolates) when source minutes are missing | unit | `cargo test -p miner-core aggregator::tests::omits_gaps_never_interpolates` | ⬜ Wave 0 | ⬜ pending |
| 02-03-T1 | 02-03 | 1 | CACHE-04 | T-02-09 | DST spring-forward fixture: bars are evenly spaced through the transition | fixture | `cargo test -p miner-core --test dst_spring_forward` | ⬜ Wave 0 | ⬜ pending |
| 02-03-T2 | 02-03 | 1 | CACHE-04 | T-02-09 | DST fall-back fixture: bars are evenly spaced through the transition | fixture | `cargo test -p miner-core --test dst_fall_back` | ⬜ Wave 0 | ⬜ pending |
| 02-02-T2 | 02-02 | 1 | CACHE-04 | T-02-06 | Bid/ask sides process independently (no cross-contamination) | unit | `cargo test -p miner-core aggregator::tests::bid_ask_independent` | ⬜ Wave 0 | ⬜ pending |
| 02-03-T3 | 02-03 | 1 | CACHE-04 | T-02-10 | Aggregator edge cases (weekend / holiday / instrument first/last day / partial session open) | fixture | `cargo test -p miner-core --test aggregator_edge_cases` | ⬜ Wave 0 | ⬜ pending |
| 02-01-T4 | 02-01 | 0 | CACHE-05 | — | Dukascopy `volume` field surfaces as `tick_volume: f64` (per-bar SUM, not count) | unit | `cargo test -p miner-reader-dukascopy --test reader_smoke tick_volume_from_csv_volume` | ⬜ Wave 0 | ⬜ pending |
| 02-01-T3 | 02-01 | 0 | CACHE-05 | T-02-04 | 00-indexed month: January → "00" | unit | `cargo test -p miner-reader-dukascopy path_layout::jan_maps_to_00` | ⬜ Wave 0 | ⬜ pending |
| 02-01-T3 | 02-01 | 0 | CACHE-05 | T-02-04 | 00-indexed month: December → "11" | unit | `cargo test -p miner-reader-dukascopy path_layout::dec_maps_to_11` | ⬜ Wave 0 | ⬜ pending |
| 02-01-T3 | 02-01 | 0 | CACHE-05 | T-02-04 | 00-indexed month: invalid inputs (0, 13) panic / Err | unit | `cargo test -p miner-reader-dukascopy path_layout::out_of_range_panics` | ⬜ Wave 0 | ⬜ pending |
| 02-01-T3 | 02-01 | 0 | CACHE-05 | T-02-04 | 00-indexed month: full path round-trip property | property | `cargo test -p miner-reader-dukascopy path_layout::path_round_trip_proptest` | ⬜ Wave 0 | ⬜ pending |
| 02-05-T2 | 02-05 | 2 | CACHE-06 | T-02-14 | Cache hit serves bars without re-reading source CSV | integration | `cargo test -p miner-core --test cache_smoke cache_hit_skips_reader` | ⬜ Wave 0 | ⬜ pending |
| 02-05-T2 | 02-05 | 2 | CACHE-06 | T-02-14 | `aggregator_version` bump triggers full rebuild | integration | `cargo test -p miner-core --test cache_smoke aggregator_version_bump_rebuilds` | ⬜ Wave 0 | ⬜ pending |
| 02-05-T2 | 02-05 | 2 | CACHE-06 | T-02-14 | Per-day fingerprint mismatch triggers day-splice only | integration | `cargo test -p miner-core --test cache_smoke day_fingerprint_bump_splices` | ⬜ Wave 0 | ⬜ pending |
| 02-05-T2 | 02-05 | 2 | CACHE-06 | T-02-15 | Atomic write: tempfile dropped without persist leaves existing Arrow file byte-unchanged AND no stale `*.tmp*` files in parent dir | integration | `cargo test -p miner-core --test cache_smoke atomic_write_crash_safety` | ⬜ Wave 0 | ⬜ pending |
| 02-05-T2 | 02-05 | 2 | CACHE-06 | T-02-16 | Arrow IPC bytes are deterministic under shuffled-metadata construction (BTreeMap-source proptest) | property | `cargo test -p miner-core --test cache_smoke arrow_bytes_deterministic_under_shuffled_construction` | ⬜ Wave 0 | ⬜ pending |
| 02-04-T2 | 02-04 | 1 | CACHE-07 | T-02-12 | GapDetector emits `MissingSourceFile` for a missing day file | unit | `cargo test -p miner-core gap::tests::missing_file_emits_correct_reason` | ⬜ Wave 0 | ⬜ pending |
| 02-04-T2 | 02-04 | 1 | CACHE-07 | T-02-12 | GapDetector emits `CorruptSourceFile` for a zero-byte source file | unit | `cargo test -p miner-core gap::tests::zero_byte_emits_corrupt` | ⬜ Wave 0 | ⬜ pending |
| 02-04-T2 | 02-04 | 1 | CACHE-07 | T-02-12 | GapDetector emits `IntraDayGap` for sub-minute holes during open hours | unit | `cargo test -p miner-core gap::tests::intra_day_hole_during_open_hours` | ⬜ Wave 0 | ⬜ pending |
| 02-04-T2 | 02-04 | 1 | CACHE-07 | T-02-13 | GapDetector does NOT emit anything for closed-hours holes | unit | `cargo test -p miner-core gap::tests::closed_hours_are_not_gaps` | ⬜ Wave 0 | ⬜ pending |
| 02-04-T3 | 02-04 | 1 | CACHE-07 | T-02-12 | Gap manifest JSON shape pinned by insta snapshot | snapshot | `cargo test -p miner-core --test gap_manifest_snapshot` | ⬜ Wave 0 | ⬜ pending |
| 02-04-T3 | 02-04 | 1 | CACHE-07 | T-02-12 | Gaps in the manifest are sorted by `start_utc` ascending (proptest) | property | `cargo test -p miner-core gap::tests::gaps_sorted_proptest` | ⬜ Wave 0 | ⬜ pending |
| 02-04-T1 | 02-04 | 1 | CACHE-08 | — | `GapManifest` derives `JsonSchema` and round-trips via serde_json | unit | `cargo test -p miner-core gap::tests::gap_manifest_schemars_roundtrip` | ⬜ Wave 0 | ⬜ pending |
| 02-05-T3 | 02-05 | 2 | ALL (schema) | T-02-16, T-02-18 | Arrow IPC schema bytes pinned by insta snapshot | snapshot | `cargo test -p miner-core --test arrow_schema_snapshot` | ⬜ Wave 0 | ⬜ pending |
| 02-05-T3 | 02-05 | 2 | ALL (schema) | T-02-16 | Arrow schema metadata keys iterate in sorted order (BTreeMap-source gate) | unit | `cargo test -p miner-core --test arrow_schema_snapshot schema_metadata_keys_sorted` | ⬜ Wave 0 | ⬜ pending |
| 02-06-T1 | 02-06 | 3 | ALL (determinism) | T-02-19 | Two full runs of aggregator + cache produce byte-identical `.arrow` + `.fingerprints.json` | integration | `cargo test -p miner-core --test full_determinism two_runs_byte_identical` | ⬜ Wave 0 | ⬜ pending |
| 02-06-T2 | 02-06 | 3 | ALL (surface) | T-02-20 | Phase 2 FROZEN public surface complete: every Phase 2 type reachable via `use miner_core::*` | integration | `cargo test -p miner-core --test public_surface_audit phase_2_public_surface_present` | ⬜ Wave 0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

> All `TBD | TBD` rows have been replaced with real `{plan-id}-T{task-num}` task IDs. The map is locked for execution-phase. Revision pass 1 added two rows from the B4 fix (`arrow_bytes_deterministic_under_shuffled_construction` proptest and `schema_metadata_keys_sorted` ancillary gate) and uses `aggregator::tests::*` paths throughout (file `crates/miner-core/src/aggregator.rs`).

---

## Wave 0 Requirements

Test infrastructure that MUST land in Wave 0 (before any Wave 1 task can be verified):

- [ ] `crates/miner-reader-dukascopy/tests/fixtures/` — synthetic `.csv.zst` generator helper (writes via `tempfile::TempDir` using the same zstd encoder as `tradedesk-dukascopy`; no binary fixtures checked in) — **OWNED BY: Plan 02-01 Task 4**
- [ ] `crates/miner-reader-dukascopy/tests/reader_smoke.rs` — integration tests against the synthetic cache — **OWNED BY: Plan 02-01 Task 4**
- [ ] `crates/miner-reader-dukascopy/src/path_layout.rs` — module + tests for `DukascopyMonth` (00-indexed) and full path round-trip — **OWNED BY: Plan 02-01 Task 3**
- [ ] `crates/miner-core/tests/aggregator_fixtures.rs` — shared fixture builders (MockReader + build_24h_1m_bars + build_partial_day_1m_bars) — **OWNED BY: Plan 02-02 Task 2**
- [ ] `crates/miner-core/tests/dst_spring_forward.rs` + `tests/dst_fall_back.rs` — split per fixture for parallel runs — **OWNED BY: Plan 02-03 Tasks 1+2**
- [ ] `crates/miner-core/tests/aggregator_edge_cases.rs` — weekend / holiday / first-day / last-day / partial-session-open — **OWNED BY: Plan 02-03 Task 3**
- [ ] `crates/miner-core/tests/cache_smoke.rs` — cache hit/miss/atomic-write tests — **OWNED BY: Plan 02-05 Task 2**
- [ ] `crates/miner-core/tests/gap_manifest_snapshot.rs` — insta snapshot of manifest JSON — **OWNED BY: Plan 02-04 Task 3**
- [ ] `crates/miner-core/tests/arrow_schema_snapshot.rs` — insta snapshot of Arrow IPC schema bytes — **OWNED BY: Plan 02-05 Task 3**
- [ ] `crates/miner-core/tests/full_determinism.rs` — end-to-end two-runs-byte-identical — **OWNED BY: Plan 02-06 Task 1**
- [ ] `crates/miner-core/tests/snapshots/.gitkeep` directory — created on first `cargo insta accept`; the `.gitkeep` is staged + git-tracked from Wave 0 so the snapshots dir is committed before any `.snap` files arrive — **OWNED BY: Plan 02-04 Task 3** (creates and git-tracks .gitkeep)
- [ ] Workspace dev-deps added: `proptest`, `insta`, `tempfile`, `serial_test` — **OWNED BY: Plan 02-01 Task 1**
- [ ] Workspace runtime deps lockstep: `arrow` and `parquet` at the same major (per amended D2-01: latest stable, floor 53+) — **OWNED BY: Plan 02-01 Task 1**
- [ ] Workspace lints: keep `unsafe_code = "forbid"` for Phase 2 (no mmap; defer to a later optimisation plan) — **VERIFIED: workspace Cargo.toml unchanged from Phase 1**

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| 50% partial-bar gap threshold (D2-19) is the right cutoff for production tooling | CACHE-04 / CACHE-07 | Threshold has no upstream source; product judgement | After plan execution, run `miner gaps EURUSD bid 2024-06-01..2024-06-30 --include-partial` and review whether the partial-bar entries match operator intuition. Adjust threshold and re-snapshot if needed. |

---

## Validation Sign-Off

- [x] All tasks have `<automated>` verify or Wave 0 dependencies
- [x] Sampling continuity: no 3 consecutive tasks without automated verify
- [x] Wave 0 covers all MISSING references (synthetic-fixture helpers, snapshot scaffolding, dev-deps)
- [x] No watch-mode flags (`cargo test` only — no `cargo watch` in commands)
- [x] Feedback latency < 30s (per-crate quick run) and < 90s (full suite)
- [x] `nyquist_compliant: true` set in frontmatter — all rows bound to plan task IDs
- [x] Revision pass 1 applied: aggregator module renamed; arrow/parquet major lockstep documented; atomic write surface split into write_arrow_to_tempfile + persist_arrow_tempfile; .gitkeep git-tracked; gap-tests enumerated explicitly; arrow metadata determinism enforced via BTreeMap-sourced sorted Vec to the IPC encoder.

**Approval:** approved by Plan 02-06 (planner-phase completion, revision pass 1).

---

## CACHE-NN Coverage Map

| Requirement | Plans | Status |
|-------------|-------|--------|
| CACHE-01 | 02-01 (Tasks 3+4) | CLOSED |
| CACHE-02 | 02-01 (Task 2 inline), 02-06 (Task 2 standalone integration) | CLOSED |
| CACHE-03 | 02-02 (Task 2) | CLOSED |
| CACHE-04 | 02-02 (kernel + byte-identity Tasks 2+3), 02-03 (DST + edge cases all 3 tasks), 02-06 (end-to-end Task 1) | CLOSED |
| CACHE-05 | 02-01 (Tasks 3 path_layout + 4 reader.tick_volume) | CLOSED |
| CACHE-06 | 02-05 (cache Tasks 1+2+3 including atomic_write_crash_safety + arrow_bytes_deterministic_under_shuffled_construction), 02-06 (end-to-end determinism Task 1) | CLOSED |
| CACHE-07 | 02-04 (Tasks 1+2+3) | CLOSED |
| CACHE-08 | 02-04 (Task 1 — data shape ships; Phase 3 owns enforcement) | CLOSED for Phase 2 scope |

**Coverage:** 8/8 CACHE-NN requirements mapped to at least one plan task. No orphans.

---

## Plan → Wave Map

| Plan | Wave | Depends On | Files Modified (high-level) |
|------|------|------------|----------------------------|
| 02-01 | 0 | (none) | Workspace deps (arrow + parquet at same major per amended D2-01); `miner-core::reader`; `miner-reader-dukascopy` crate full impl |
| 02-02 | 1 | 02-01 | `miner-core::calendar`; `miner-core::aggregator` (file `aggregator.rs`, NOT `aggregate.rs` — revision pass 1 rename); aggregator_fixtures + determinism integration test |
| 02-03 | 1 | 02-01, 02-02 (intra-wave dependency — Plan 03 compiles against `crates/miner-core/tests/aggregator_fixtures.rs` from Plan 02-02 Task 2; execute-phase serialises 02-02 → 02-03 inside Wave 1) | `tests/dst_spring_forward.rs`, `tests/dst_fall_back.rs`, `tests/aggregator_edge_cases.rs` |
| 02-04 | 1 | 02-01 (parallel with 02-02/02-03) | `miner-core::gap`; gap_manifest_snapshot integration test; `tests/snapshots/.gitkeep` (git-tracked) |
| 02-05 | 2 | 02-02 (needs BarFrame + AGGREGATOR_VERSION + `aggregator` module name) | `miner-core::cache` (two-step write_arrow_to_tempfile / persist_arrow_tempfile API; BTreeMap-sourced metadata to IPC encoder for byte-determinism); cache_smoke + arrow_schema_snapshot tests |
| 02-06 | 3 | 02-05 (needs BarCache), 02-04 (parallel-tolerant), 02-03 | `tests/full_determinism.rs`; `tests/public_surface_audit.rs`; `tests/reader_trait_object_safety.rs`; VALIDATION.md finalisation |

**Wave 1 execution detail (revision pass 1):** Plans 02-02 and 02-03 both touch `crates/miner-core/tests/`. Plan 02-03 declares `02-02` in its `depends_on` so `gsd-executor` serialises 02-02 → 02-03 inside Wave 1 (intra-wave dependency edges are honoured regardless of wave number). Plan 02-04 touches `crates/miner-core/src/gap.rs` exclusively and can run in true parallel with 02-02/02-03 — but writes to `crates/miner-core/src/lib.rs` which is shared; sequence the lib.rs touches.
