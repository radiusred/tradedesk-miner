---
phase: 02
slug: reader-aggregator-derived-bar-cache
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-05-17
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

> Populated by `gsd-planner` during plan-phase. Each task in every `*-PLAN.md` MUST appear as a row here with an `<automated>` verify or an explicit Wave 0 dependency. Rows below are the contract from RESEARCH.md §"Phase Requirements → Test Map".

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| TBD | TBD | 0 | CACHE-01 | — | DukascopyReader opens a real `.csv.zst` fixture and yields 1m bars in ascending UTC order | integration | `cargo test -p miner-reader-dukascopy --test reader_smoke reads_one_day_in_order` | ❌ Wave 0 | ⬜ pending |
| TBD | TBD | 0 | CACHE-01 | — | DukascopyReader rejects a zero-byte source file with `CorruptSourceFile` | unit | `cargo test -p miner-reader-dukascopy zero_byte_file` | ❌ Wave 0 | ⬜ pending |
| TBD | TBD | 0 | CACHE-02 | — | `Reader` trait is dyn-compatible and `DukascopyReader: Reader` | unit | `cargo test -p miner-reader-dukascopy reader_trait_object_safety` | ❌ Wave 0 | ⬜ pending |
| TBD | TBD | 1 | CACHE-03 | — | Aggregator emits 15m / 1h / 1d bars from 1-day synthetic 1m input | unit | `cargo test -p miner-core aggregator::tests::three_timeframes` | ❌ Wave 0 | ⬜ pending |
| TBD | TBD | 1 | CACHE-04 | — | Aggregator output is byte-identical across two runs on the same input | integration | `cargo test -p miner-core --test aggregator_determinism byte_identical_two_runs` | ❌ Wave 0 | ⬜ pending |
| TBD | TBD | 1 | CACHE-04 | — | OHLC monotonicity: high ≥ open/close, low ≤ open/close (proptest) | property | `cargo test -p miner-core aggregator::tests::ohlc_monotonicity_proptest` | ❌ Wave 0 | ⬜ pending |
| TBD | TBD | 1 | CACHE-04 | — | Aggregator omits (never interpolates) when source minutes are missing | unit | `cargo test -p miner-core aggregator::tests::omits_gaps_never_interpolates` | ❌ Wave 0 | ⬜ pending |
| TBD | TBD | 1 | CACHE-04 | — | DST spring-forward fixture: bars are evenly spaced through the transition | fixture | `cargo test -p miner-core --test dst_spring_forward` | ❌ Wave 0 | ⬜ pending |
| TBD | TBD | 1 | CACHE-04 | — | DST fall-back fixture: bars are evenly spaced through the transition | fixture | `cargo test -p miner-core --test dst_fall_back` | ❌ Wave 0 | ⬜ pending |
| TBD | TBD | 1 | CACHE-04 | — | Bid/ask sides process independently (no cross-contamination) | unit | `cargo test -p miner-core aggregator::tests::bid_ask_independent` | ❌ Wave 0 | ⬜ pending |
| TBD | TBD | 0 | CACHE-05 | — | Dukascopy `volume` field surfaces as `tick_volume: f64` (per-bar SUM, not count) | unit | `cargo test -p miner-reader-dukascopy reader_smoke::tick_volume_from_csv_volume` | ❌ Wave 0 | ⬜ pending |
| TBD | TBD | 0 | CACHE-05 | — | 00-indexed month: January → "00" | unit | `cargo test -p miner-reader-dukascopy path_layout::jan_maps_to_00` | ❌ Wave 0 | ⬜ pending |
| TBD | TBD | 0 | CACHE-05 | — | 00-indexed month: December → "11" | unit | `cargo test -p miner-reader-dukascopy path_layout::dec_maps_to_11` | ❌ Wave 0 | ⬜ pending |
| TBD | TBD | 0 | CACHE-05 | — | 00-indexed month: invalid inputs (0, 13) panic / Err | unit | `cargo test -p miner-reader-dukascopy path_layout::out_of_range_panics` | ❌ Wave 0 | ⬜ pending |
| TBD | TBD | 0 | CACHE-05 | — | 00-indexed month: full path round-trip property | property | `cargo test -p miner-reader-dukascopy path_layout::path_round_trip_proptest` | ❌ Wave 0 | ⬜ pending |
| TBD | TBD | 2 | CACHE-06 | — | Cache hit serves bars without re-reading source CSV | integration | `cargo test -p miner-core --test cache_smoke cache_hit_skips_reader` | ❌ Wave 0 | ⬜ pending |
| TBD | TBD | 2 | CACHE-06 | — | `aggregator_version` bump triggers full rebuild | integration | `cargo test -p miner-core --test cache_smoke aggregator_version_bump_rebuilds` | ❌ Wave 0 | ⬜ pending |
| TBD | TBD | 2 | CACHE-06 | — | Per-day fingerprint mismatch triggers day-splice only | integration | `cargo test -p miner-core --test cache_smoke day_fingerprint_bump_splices` | ❌ Wave 0 | ⬜ pending |
| TBD | TBD | 2 | CACHE-06 | — | Atomic write: crash mid-write leaves existing Arrow file intact | integration | `cargo test -p miner-core --test cache_smoke atomic_write_crash_safety` | ❌ Wave 0 | ⬜ pending |
| TBD | TBD | 1 | CACHE-07 | — | GapDetector emits `MissingFile` for a missing day file | unit | `cargo test -p miner-core gap::tests::missing_file_emits_correct_reason` | ❌ Wave 0 | ⬜ pending |
| TBD | TBD | 1 | CACHE-07 | — | GapDetector emits `CorruptFile` for a zero-byte source file | unit | `cargo test -p miner-core gap::tests::zero_byte_emits_corrupt` | ❌ Wave 0 | ⬜ pending |
| TBD | TBD | 1 | CACHE-07 | — | GapDetector emits `IntraDayGap` for sub-minute holes during open hours | unit | `cargo test -p miner-core gap::tests::intra_day_hole_during_open_hours` | ❌ Wave 0 | ⬜ pending |
| TBD | TBD | 1 | CACHE-07 | — | GapDetector does NOT emit anything for closed-hours holes | unit | `cargo test -p miner-core gap::tests::closed_hours_are_not_gaps` | ❌ Wave 0 | ⬜ pending |
| TBD | TBD | 1 | CACHE-07 | — | Gap manifest JSON shape pinned by insta snapshot | snapshot | `cargo test -p miner-core --test gap_manifest_snapshot` | ❌ Wave 0 | ⬜ pending |
| TBD | TBD | 1 | CACHE-07 | — | Gaps in the manifest are sorted by `start_utc` ascending (proptest) | property | `cargo test -p miner-core gap::tests::gaps_sorted_proptest` | ❌ Wave 0 | ⬜ pending |
| TBD | TBD | 1 | CACHE-08 | — | `GapManifest` derives `JsonSchema` and round-trips via serde_json | unit | `cargo test -p miner-core gap::tests::gap_manifest_schemars_roundtrip` | ❌ Wave 0 | ⬜ pending |
| TBD | TBD | 2 | ALL (schema) | — | Arrow IPC schema bytes pinned by insta snapshot | snapshot | `cargo test -p miner-core --test arrow_schema_snapshot` | ❌ Wave 0 | ⬜ pending |
| TBD | TBD | 2 | ALL (determinism) | — | Two full runs of aggregator + cache produce byte-identical `.arrow` + `.fingerprints.json` | integration | `cargo test -p miner-core --test full_determinism two_runs_byte_identical` | ❌ Wave 0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

> Planner: replace each `TBD | TBD` Task ID / Plan column with the real `{NN-NN-NN}` identifier when the plan is sealed.

---

## Wave 0 Requirements

Test infrastructure that MUST land in Wave 0 (before any Wave 1 task can be verified):

- [ ] `crates/miner-reader-dukascopy/tests/fixtures/` — synthetic `.csv.zst` generator helper (writes via `tempfile::TempDir` using the same zstd encoder as `tradedesk-dukascopy`; no binary fixtures checked in)
- [ ] `crates/miner-reader-dukascopy/tests/reader_smoke.rs` — integration tests against the synthetic cache
- [ ] `crates/miner-reader-dukascopy/src/path_layout.rs` — module + tests for `DukascopyMonth` (00-indexed) and full path round-trip
- [ ] `crates/miner-core/tests/aggregator_fixtures.rs` — shared fixture builders (DST, weekend, holiday, first/last cache day, partial-bar session open)
- [ ] `crates/miner-core/tests/dst_spring_forward.rs` + `tests/dst_fall_back.rs` — split per fixture for parallel runs
- [ ] `crates/miner-core/tests/cache_smoke.rs` — cache hit/miss/atomic-write tests
- [ ] `crates/miner-core/tests/gap_manifest_snapshot.rs` — insta snapshot of manifest JSON
- [ ] `crates/miner-core/tests/arrow_schema_snapshot.rs` — insta snapshot of Arrow IPC schema bytes
- [ ] `crates/miner-core/tests/full_determinism.rs` — end-to-end two-runs-byte-identical
- [ ] `crates/miner-core/tests/snapshots/` directory — created on first `cargo insta accept`
- [ ] Workspace dev-deps added: `proptest`, `insta`, `tempfile` (via `cargo add --dev --workspace`)
- [ ] Workspace lints: keep `unsafe_code = "forbid"` for Phase 2 (no mmap; defer to a later optimisation plan)

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| 50% partial-bar gap threshold (D2-19) is the right cutoff for production tooling | CACHE-04 / CACHE-07 | Threshold has no upstream source; product judgement | After plan execution, run `miner gaps EURUSD bid 2024-06-01..2024-06-30 --include-partial` and review whether the partial-bar entries match operator intuition. Adjust threshold and re-snapshot if needed. |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references (synthetic-fixture helpers, snapshot scaffolding, dev-deps)
- [ ] No watch-mode flags (`cargo test` only — no `cargo watch` in commands)
- [ ] Feedback latency < 30s (per-crate quick run) and < 90s (full suite)
- [ ] `nyquist_compliant: true` set in frontmatter once all rows are bound to plan task IDs

**Approval:** pending
