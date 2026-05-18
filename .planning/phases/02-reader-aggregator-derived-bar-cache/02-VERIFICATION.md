---
phase: 02-reader-aggregator-derived-bar-cache
verified: 2026-05-18T00:00:00Z
status: passed
score: 5/5 success criteria verified
overrides_applied: 0
---

# Phase 2: Reader, Aggregator & Derived-Bar Cache — Verification Report

**Phase Goal:** User can read the existing tradedesk-dukascopy zstd-CSV cache through a pluggable `Reader` trait, materialise deterministic UTC-aligned higher-timeframe bars from the aggregator, serve them from a versioned derived-bar cache, and obtain a structured gap manifest before any scan runs.
**Verified:** 2026-05-18
**Status:** PHASE COMPLETE
**Re-verification:** No — initial verification

---

## Headline Verdict: PHASE COMPLETE

All 5 ROADMAP success criteria are DELIVERED. All 8 CACHE-NN requirements are CLOSED (with the CACHE-08 boundary correctly scoped as described in 02-CONTEXT.md). All cross-cutting invariants pass. The workspace is green under `cargo test`, `cargo clippy -- -D warnings`, and `cargo fmt --check`.

---

## Goal Achievement

### Observable Truths (ROADMAP Success Criteria)

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | User can point DukascopyReader at an existing `<root>/<SYMBOL>/<YYYY>/<MM 00-indexed>/<DD>_<bid\|ask>.csv.zst` cache and read 1-minute bars without modification, with the 00-indexed month quirk encapsulated and boundary-tested | VERIFIED | `path_layout::DukascopyMonth` sealed newtype enforces `0..=11`; `day_csv_zst()` is the only path constructor; 9 unit tests in `path_layout::tests::*` including `jan_maps_to_00`, `dec_maps_to_11`, `out_of_range_panics`, `path_round_trip_proptest`. Integration test `reads_one_day_in_order` drives the real zstd-CSV stack against synthetic fixtures |
| 2 | User can request 15m/1h/1d bars and receive deterministic, close-aligned, UTC OHLC bars that omit (never interpolate) gaps, process bid/ask independently, and expose Dukascopy `volume` as `tick_volume: f64` | VERIFIED | `aggregator.rs:aggregate()` pure function; `three_timeframes` test verifies 1440/15=96, 1440/60=24, 1440/1440=1 bar counts; `omits_gaps_never_interpolates` verifies 95 bars when one 15m bucket is absent; `bid_ask_independent` verifies no cross-contamination; `tick_volume_from_csv_volume` integration test proves the A1 rename; DST tests (`dst_spring_forward`, `dst_fall_back`) prove UTC-agnostic bucketing; `ohlc_monotonicity_proptest` proves OHLC invariant across 64 random cases |
| 3 | User can re-run any aggregation and have miner serve bars from the on-disk Arrow IPC cache keyed by `(source_id, symbol, side, timeframe)` with `aggregator_version` + per-day source fingerprint invalidation | VERIFIED | `BarCache::get_or_build()` fully implemented in `cache.rs`; `cache_hit_skips_reader` verifies second call skips reader; `aggregator_version_bump_rebuilds` verifies version drift triggers rebuild; `day_fingerprint_bump_splices` verifies stale-day diff; `two_runs_byte_identical` (and `two_runs_byte_identical_three_timeframes`) prove byte-identical Arrow IPC + sidecar across two fresh pipeline runs |
| 4 | User can request a gap report for any `(symbol, side, range)` and receive a structured gap manifest enumerating `(start, end, reason)` tuples for missing daily files, zero-byte/corrupt files, and intra-day bar holes against the instrument's trading calendar | VERIFIED | `GapDetector::detect()` in `gap.rs`; `missing_file_emits_correct_reason` tests `MissingSourceFile`; `zero_byte_emits_corrupt` tests `CorruptSourceFile`; `intra_day_hole_during_open_hours` tests `IntraDayGap`; `closed_hours_are_not_gaps` proves weekend silence; `gap_manifest_json_shape_pinned` insta snapshot pins JSON wire format including `kind` tags; `gaps_sorted_proptest` proves sort invariant on random inputs |
| 5 | User can run aggregator edge-case fixture tests (DST spring-forward, DST fall-back, weekend gap, holiday, instrument first/last cache day, partial-bar session open) and observe byte-identical re-run determinism | VERIFIED | `dst_spring_forward.rs` (3 tests: 15m, 1h, 1d across 2024-03-31 London clock change); `dst_fall_back.rs` (3 tests: 15m, 1h, 1d across 2024-10-27 London clock change); `aggregator_edge_cases.rs` (weekend, FX holiday Christmas, partial-session-open, first-cache-day, last-cache-day); `full_determinism::two_runs_byte_identical` (byte-identical across runs); `arrow_bytes_deterministic_under_shuffled_construction` proptest (16 cases) |

**Score:** 5/5 truths verified

---

### Deferred Items

CACHE-08 policy enforcement (`strict` / `continuous_only`) is deferred to Phase 3 by explicit design (02-CONTEXT.md §"CACHE-08 boundary clarification"). Phase 2 delivers the data model (`GapManifest`, `GapSpan`, `GapReason` with `JsonSchema` derive) and the detector; Phase 3 delivers the policy-enforcement wrapper at the scan-engine boundary.

| # | Item | Addressed In | Evidence |
|---|------|-------------|----------|
| 1 | `strict` / `continuous_only` gap policy enforcement at scan-engine boundary | Phase 3 | ROADMAP Phase 3 SC #3: "User can choose `--gap-policy strict` and have miner abort the scan… or `continuous_only`…"; 02-CONTEXT.md explicitly assigns enforcement to Phase 3 |
| 2 | Day-splice (row-level Arrow file update) optimisation | Phase 7 or later | 02-CONTEXT.md D2-02; `cache.rs` "Day-splice implementation note" documents full-rebuild v1 behaviour is behaviourally equivalent and intentionally deferred |

---

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `crates/miner-core/src/reader.rs` | `Reader` trait, `RawBar`, `Side`, `Blake3Hex`, `ClosedRangeUtc` | VERIFIED | All types present, `Reader` is dyn-compatible (compile-time test), all fields public |
| `crates/miner-core/src/aggregator.rs` | `aggregate()`, `BarFrame`, `Timeframe`, `AggParams`, `AGGREGATOR_VERSION` | VERIFIED | 958-line implementation with full inline MockReader + 8 unit tests |
| `crates/miner-core/src/calendar.rs` | `Calendar`, `fx_major()`, `is_open_at()` | VERIFIED | O(1) predicate, 8 boundary tests, `Default` = `fx_major()` |
| `crates/miner-core/src/gap.rs` | `GapDetector`, `GapManifest`, `GapSpan`, `GapReason` | VERIFIED | 811-line implementation with 5 unit tests + proptest |
| `crates/miner-core/src/cache.rs` | `BarCache`, `build_arrow_schema`, `FingerprintSidecar`, `ARROW_SCHEMA_VERSION`, atomic write | VERIFIED | 754-line implementation with two-step atomic API, BTreeMap metadata discipline |
| `crates/miner-core/src/cache/fingerprints.rs` | `FingerprintSidecar`, `read_sidecar`, `write_sidecar_atomic`, `diff_days` | VERIFIED | All functions implemented with 5 unit tests |
| `crates/miner-reader-dukascopy/src/reader.rs` | `DukascopyReader` implementing `Reader`, full zstd-CSV pipeline | VERIFIED | 322-line implementation with zstd decoder + CSV deserialization + blake3 fingerprinting |
| `crates/miner-reader-dukascopy/src/path_layout.rs` | `DukascopyMonth`, `day_csv_zst`, `parse_day_path` | VERIFIED | Sealed newtype enforcing `0..=11`, 9 unit tests + proptest |
| `crates/miner-reader-dukascopy/tests/fixtures/mod.rs` | Synthetic cache fixture builder | VERIFIED | `SyntheticCache`, `one_minute_csv_body`, `write_day`, `write_zero_byte_day` all implemented |
| Integration tests: `tests/{aggregator_determinism,aggregator_edge_cases,aggregator_fixtures,arrow_schema_snapshot,cache_smoke,dst_fall_back,dst_spring_forward,full_determinism,gap_manifest_snapshot,public_surface_audit}.rs` | All per-VALIDATION.md rows | VERIFIED | All 12 test files present and exercising real assertions |
| Insta snapshots: `gap_manifest_snapshot__gap_manifest_json_shape_pinned.snap`, 3x `arrow_schema_snapshot__*.snap` | Non-empty pinned values | VERIFIED | Snapshots contain real JSON / schema field content |

---

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| `DukascopyReader` | `miner_core::Reader` | `impl Reader for DukascopyReader` | WIRED | `reader.rs:94` `impl Reader for DukascopyReader`; all 4 trait methods implemented |
| `DukascopyReader::read_1m_bars` | `path_layout::day_csv_zst` | `enumerate_days` + `day_bar_iter` | WIRED | `reader.rs:111` calls `enumerate_days`; `day_bar_iter` resolves via `path_layout` |
| `aggregate()` | `BarFrame` | `emit_bucket()` accumulator | WIRED | `aggregator.rs:281-393`; `frame.ts_open_utc.push(...)` etc. in `emit_bucket` |
| `BarCache::get_or_build` | `aggregate()` + `write_arrow_atomic` | `cache.rs:660-666` | WIRED | `aggregate(reader, params)` called then `write_arrow_atomic(&arrow_p, ...)` |
| `GapDetector::detect` | `Calendar::is_open_at` | `open_minutes_in_day()` inner loop | WIRED | `gap.rs:356-365`; 1440-call loop per day |
| `GapManifest` | `JsonSchema` | `#[derive(JsonSchema)]` | WIRED | `gap.rs:82`; `gap_manifest_schemars_roundtrip` test verifies the schema serialises with all `kind` tags |
| `FingerprintSidecar` | `BTreeMap<NaiveDate, String>` | `per_day_fingerprint` field | WIRED | `fingerprints.rs:63`; BTreeMap key ordering enforces byte-deterministic JSON |
| `public_surface_audit` | All Phase 2 re-exports via `use miner_core::*` | `lib.rs:43-56` | WIRED | Test names every Phase 2 type by the top-level re-export path; all compile and pass |

---

### Data-Flow Trace (Level 4)

| Artifact | Data Variable | Source | Produces Real Data | Status |
|----------|--------------|--------|--------------------|--------|
| `aggregate()` | `frame.ts_open_utc / open / high / low / close / tick_volume` | `Reader::read_1m_bars` iterator (CSV rows parsed from `.csv.zst` bytes) | Yes — per-bar OHLC from CSV with `tick_volume = row.volume` (renamed at boundary) | FLOWING |
| `BarCache::get_or_build` | Arrow IPC bytes on disk | `aggregate()` → `bar_frame_to_record_batch` → `write_arrow_atomic` | Yes — `two_runs_byte_identical` verifies non-empty Arrow file starts with `ARROW1` magic | FLOWING |
| `GapDetector::detect` | `GapManifest::gaps` | `Reader::fingerprint_day` + `Reader::read_1m_bars` per day | Yes — `missing_file_emits_correct_reason` drives 3 days through a real `LocalMockReader` with varying day payloads | FLOWING |
| `FingerprintSidecar` | `per_day_fingerprint` BTreeMap | `Reader::fingerprint_day` per enumerated day in `BarCache::get_or_build` | Yes — `day_fingerprint_bump_splices` verifies specific changed/unchanged entries | FLOWING |

---

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| `cargo test --workspace` — all 125 tests pass | `~/.cargo/bin/cargo test --workspace` | 0 failed across 24 test suites | PASS |
| `cargo clippy --workspace --all-targets -- -D warnings` exits 0 | `~/.cargo/bin/cargo clippy ...` | No warnings emitted; exit code 0 | PASS |
| `cargo fmt --all --check` exits 0 | `~/.cargo/bin/cargo fmt --all --check` | No output; exit code 0 | PASS |
| `cargo tree -p miner-core` has zero async/tokio deps | `~/.cargo/bin/cargo tree -p miner-core --edges normal,build \| grep -E 'tokio\|async'` | Empty output | PASS |
| Byte-identical Arrow IPC across two pipeline runs | `full_determinism::two_runs_byte_identical` | `run1_arrow == run2_arrow`, both start with `ARROW1` | PASS |
| 00-indexed month: Jan maps to "00", Dec maps to "11" | `cargo test path_layout::tests::jan_maps_to_00 path_layout::tests::dec_maps_to_11` | Both pass | PASS |

---

### Requirements Coverage

| Requirement | Plans | Description | Status | Evidence |
|-------------|-------|-------------|--------|---------|
| CACHE-01 | 02-01 | Read existing Dukascopy zstd-CSV cache without modification | SATISFIED | `DukascopyReader` reads `<root>/<SYMBOL>/<YYYY>/<MM 00-indexed>/<DD>_<side>.csv.zst`; `reads_one_day_in_order` integration test |
| CACHE-02 | 02-01, 02-06 | `Reader` trait for pluggable data sources | SATISFIED | `miner_core::reader::Reader` trait with `Send + Sync + dyn`-compatible contract; `reader_trait_object_safe` unit test; `dukascopy_reader_is_dyn_compatible` integration test |
| CACHE-03 | 02-02 | Bars at caller-specified resolutions (15m/1h/1d) | SATISFIED | `Timeframe` enum + `aggregate()` pure function; `three_timeframes` test verifies all three resolutions |
| CACHE-04 | 02-02, 02-03, 02-06 | Deterministic, close-aligned, UTC, gap-omitting, bid/ask-independent aggregation | SATISFIED | `omits_gaps_never_interpolates`, `bid_ask_independent`, all DST tests, `aggregator_edge_cases`, `two_runs_byte_identical` |
| CACHE-05 | 02-01 | `volume` exposed as `tick_volume: f64`, 00-indexed month documented + boundary-tested | SATISFIED | `tick_volume_from_csv_volume` integration test; `jan_maps_to_00`, `dec_maps_to_11`, `out_of_range_panics`, `path_round_trip_proptest` |
| CACHE-06 | 02-05, 02-06 | On-disk Arrow IPC derived-bar cache with `aggregator_version` + per-day fingerprint invalidation | SATISFIED | `BarCache::get_or_build()` fully implemented; all 5 `cache_smoke` tests pass; `atomic_write_crash_safety` proves write discipline |
| CACHE-07 | 02-04 | Structured gap manifest: missing file, corrupt file, intra-day holes | SATISFIED | `GapDetector::detect()` with 4 unit tests + proptest + `gap_manifest_json_shape_pinned` insta snapshot |
| CACHE-08 | 02-04 (Phase 2 scope only) | Gap manifest data type for Phase 3 policy enforcement | SATISFIED (Phase 2 scope) | `GapManifest` / `GapSpan` / `GapReason` with `JsonSchema` derive; enforcement (`strict` / `continuous_only`) explicitly deferred to Phase 3 per 02-CONTEXT.md |

---

### Invariant Check Results

#### `unsafe` gate

```
grep -rn '\bunsafe\b' crates/ --include="*.rs" | grep -v 'SAFETY:\|// SAFETY\|unsafe_code\|forbid.*unsafe\|# Safety'
```

Result: 2 hits, both in doc-comment prose (`"no unsafe is required"` and `"unsafe block (within figment's…)"`). Zero executable `unsafe` blocks. Workspace lint `unsafe_code = "forbid"` confirmed intact.

#### `println!` / `eprintln!` / `dbg!` gate

```
grep -rn 'println!\|eprintln!\|dbg!' crates/ --include="*.rs"
```

Result: 2 hits in `crates/miner-core/build.rs` only — both are `println!("cargo:rustc-env=...")` and `println!("cargo:rerun-if-changed=...")`. These are cargo build-script protocol directives, explicitly exempted by the `build.rs` doc comment and by the fact that `build.rs` runs in a separate compilation context. Zero hits in library or binary source. All non-test source respects the `clippy.toml` `disallowed_macros` gate.

#### `tick_count` gate (A1 invariant)

```
grep -rn 'tick_count' crates/ --include="*.rs"
```

Result: empty. The A1 invariant holds — only `tick_volume: f64` exists in the codebase.

#### `HashMap` in `Serialize`-derived types

```
grep -rn 'HashMap' crates/ --include="*.rs"
```

Result: 2 hits, both in `crates/miner-core/src/cache.rs`:
- Line 64: `use std::collections::{BTreeMap, HashMap};` (import)
- Line 196: `let meta_hash: HashMap<String, String> = meta_btree.into_iter().collect();`

Context: `HashMap` is used only as an ephemeral intermediate for `Schema::with_metadata(HashMap<String, String>)` (the Arrow API requires `HashMap`). The **source** of the keys is the preceding `BTreeMap<String, String>` (`meta_btree`). No `Serialize`-derived struct or enum uses `HashMap` as a field type — every serialised type (`FingerprintSidecar`, `GapManifest`, `GapSpan`, `GapReason`) uses `BTreeMap` or no map at all. The `arrow_bytes_deterministic_under_shuffled_construction` proptest and `schema_metadata_keys_sorted` test confirm byte-identity. **COMPLIANT.**

#### Async/tokio dependency gate (FOUND-04)

```
cargo tree -p miner-core --edges normal,build | grep -E 'tokio|async-std|async-trait'
```

Result: empty. `miner-core` has zero async transitive dependencies.

#### `cargo test --workspace` gate

Result: **125 tests pass, 0 fail, 0 errors** across 24 test suites.

#### `cargo clippy --workspace --all-targets -- -D warnings` gate

Result: exit code 0. No warnings emitted.

#### `cargo fmt --all --check` gate

Result: exit code 0. No formatting drift.

---

### Public Surface Audit

The `public_surface_audit::phase_2_public_surface_present` test names every Phase 2 type via `use miner_core::{...}` (the top-level re-export path, not the inner module path). The following types are confirmed reachable:

**02-01 (Reader + Calendar):** `Side`, `ClosedRangeUtc`, `RawBar`, `Blake3Hex`, `Reader`, `Calendar`

**02-02 (Aggregator):** `AGGREGATOR_VERSION`, `Timeframe`, `AggParams`, `BarFrame`, `AggregateError`, `aggregate`

**02-04 (Gap):** `GapDetector`, `GapManifest`, `GapSpan`, `GapReason`

**02-05 (Cache):** `ARROW_SCHEMA_VERSION`, `BarCache`, `CacheError`, `FingerprintSidecar`, `build_arrow_schema`

All 20 names compile and are exercised in the test body. The test passes.

---

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| `gap.rs:214` | 214 | `// TODO(Phase 7): consider explicit zero-byte sentinel.` | INFO | References a formal future phase; not a blocker. The existing behaviour (treat read-error as `CorruptSourceFile`) is documented and tested |
| `miner-mcp/src/main.rs` | 3, 11 | "placeholder" in Phase 1 stub binary | INFO | Expected — Phase 1 stub; Phase 6 lands the real implementation. Not a Phase 2 artifact |
| `miner-http/src/main.rs` | 3, 10 | "placeholder" in Phase 1 stub binary | INFO | Expected — Phase 1 stub; Phase 6 lands the real implementation. Not a Phase 2 artifact |

No `TBD`, `FIXME`, or `XXX` markers exist in any Phase 2 source file. No blocker anti-patterns.

**Day-splice simplification note:** `cache.rs:551-558` documents that v1 does a full-range re-aggregation whenever any day is stale, rather than a true row-level splice. The `day_fingerprint_bump_splices` test asserts behavioural equivalence. This is an intentional v1 simplification, not a correctness gap.

---

### Human Verification Required

None. All success criteria are verifiable programmatically. The one manual-only item in VALIDATION.md (the 50% partial-bar gap threshold product judgement) is a Phase 2 tuning concern with no user-facing entry point in this phase — it is a Phase 3+ operator concern.

---

## Gaps Summary

No gaps. All ROADMAP success criteria are DELIVERED. All CACHE-NN requirements are CLOSED within their stated Phase 2 scope. All workspace gates pass.

---

## Follow-ups for Phase 3+

The following are not gaps but are observations relevant to Phase 3 planning:

1. **CACHE-08 policy enforcement is the first task Phase 3 must address.** The `GapManifest` API surface (D2-17) is ready; Phase 3 must wrap it into `Finding::GapAborted` under `strict` policy and partition windows under `continuous_only` policy.

2. **Day-splice optimisation (`D2-02`)**: The v1 cache always re-aggregates the full quartet range on any staleness. For a 28-symbol × 6-year deployment, a single refreshed day triggers re-aggregation of years of data for that quartet. Phase 7 (benchmarking) is the right place to measure whether this is a real-world bottleneck and implement the true row-splice path.

3. **`mmap` for Arrow IPC reads**: The cache currently uses `BufReader<File>`. The `forbid(unsafe_code)` lint was the blocking factor (per cache.rs §Safety note). If Phase 7 profiling shows Arrow reads are a bottleneck, a targeted relaxation of the lint to `deny` for the `miner-reader-dukascopy` crate (not `miner-core`) would enable `memmap2` adoption. This is a Phase 7 concern.

4. **Partial-bar gap threshold (D2-19)**: The 50% threshold is currently baked into the doc comment but not enforced in code — the aggregator emits partial-bucket bars regardless of completeness, and the gap manifest separately classifies open-hours holes. The VALIDATION.md manual-only item covers this; Phase 3's scan-engine code may want an operator tuning knob.

---

*Verified: 2026-05-18*
*Verifier: Claude (gsd-verifier)*
