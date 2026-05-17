---
phase: 02-reader-aggregator-derived-bar-cache
plan: 01
subsystem: infra
tags: [rust, reader, dukascopy, zstd, csv, blake3, arrow, walkdir, chrono, proptest, insta]

# Dependency graph
requires:
  - phase: 01-foundations-contracts
    provides: "FROZEN public surface of miner-core (FindingSink trait shape analog, WireError, MinerConfig); workspace clippy/unsafe gates; build.rs git-SHA injection"
provides:
  - "miner_core::reader::Reader trait + RawBar, Side, ClosedRangeUtc, Blake3Hex types"
  - "miner_core::calendar::Calendar (placeholder stub; Plan 02-02 fills with FX-major default + is_open_at)"
  - "miner_reader_dukascopy::DukascopyReader concrete impl with BufReader<File> -> zstd::Decoder -> csv::Reader pipeline (no mmap, no unsafe)"
  - "miner_reader_dukascopy::path_layout — sealed DukascopyMonth newtype, day_csv_zst (only public path constructor), parse_day_path inverse + thiserror PathParseError"
  - "miner_reader_dukascopy::DukascopyError thiserror enum + From<DukascopyError> for miner_core::WireError"
  - "SyntheticCache + one_minute_csv_body + whole_day_range test fixture helpers (used by Plans 02-02..02-06)"
  - "Workspace [workspace.dependencies] declarations for arrow=58, parquet=58, csv=1.4, zstd=0.13, walkdir=2.5, tempfile=3, serial_test=3"
  - "miner-core dev-deps proptest=1.11, insta=1.47 (Wave 0 test infrastructure)"
affects:
  - "02-02 aggregator (consumes Reader trait + RawBar; replaces Calendar stub)"
  - "02-03 DST/edge-case tests (consume SyntheticCache fixture)"
  - "02-04 gap detector (consumes Reader + RawBar)"
  - "02-05 derived-bar cache (consumes Reader, fingerprint_day, Blake3Hex; uses arrow workspace dep)"
  - "02-06 phase finalisation (consumes everything; runs full determinism + public-surface-audit tests)"

# Tech tracking
tech-stack:
  added:
    - "arrow 58 (workspace dep — Plan 02-05 cache format)"
    - "parquet 58 (lockstep major with arrow per CLAUDE.md Version Compatibility)"
    - "csv 1.4 (Dukascopy CSV parsing)"
    - "zstd 0.13 (.csv.zst streaming decompression — single-threaded per-file)"
    - "walkdir 2.5 (deterministic source-file enumeration with .sort_by_file_name())"
    - "tempfile 3 (TempDir test fixtures + Plan 02-05 atomic rename)"
    - "serial_test 3 (env-mutating-test serialisation — unused in 02-01 but pre-declared workspace-wide)"
    - "proptest 1.11 (miner-core + miner-reader-dukascopy dev-dep — path_round_trip_proptest)"
    - "insta 1.47 (miner-core dev-dep — Plan 02-04/02-05 snapshot tests)"
  patterns:
    - "Sealed-newtype with private inner field + smart constructor invariant (DukascopyMonth, mirrors Phase 1 RunId)"
    - "Manual Serialize/Deserialize/JsonSchema impl for fixed-size byte arrays (Blake3Hex; serde derives cap arrays at 32)"
    - "Object-safe trait + Send + Sync bound + associated Error type (Reader; mirrors Phase 1 FindingSink)"
    - "Box<dyn Iterator + Send + 'a> return for streaming methods (no impl-Trait-in-trait gymnastics)"
    - "Type alias for complex return types (RawBarIter<'a, E>) to silence clippy::type_complexity uniformly"
    - "Arc<str> sharing into per-iteration closures to escape FnMut borrow-check"
    - "Read-up-front-then-decode pattern: full .csv.zst bytes into Vec<u8> first, so zero-byte detection + blake3 hashing share one buffer"
    - "Boundary rename at deserialisation: CSV `volume` -> RawBar.tick_volume: f64 (D2-13 A1 invariant — never propagated as `volume`/`tick_count`)"

key-files:
  created:
    - "crates/miner-core/src/reader.rs (Reader trait + Side + ClosedRangeUtc + RawBar + Blake3Hex + RawBarIter)"
    - "crates/miner-core/src/calendar.rs (Calendar placeholder — Plan 02-02 fills)"
    - "crates/miner-reader-dukascopy/src/path_layout.rs (DukascopyMonth, day_csv_zst, parse_day_path, PathParseError)"
    - "crates/miner-reader-dukascopy/src/error.rs (DukascopyError + WireError conversion)"
    - "crates/miner-reader-dukascopy/src/reader.rs (DukascopyReader + impl miner_core::Reader)"
    - "crates/miner-reader-dukascopy/tests/fixtures/mod.rs (SyntheticCache + one_minute_csv_body + whole_day_range)"
    - "crates/miner-reader-dukascopy/tests/reader_smoke.rs (5 integration tests)"
  modified:
    - "Cargo.toml (workspace deps: arrow, parquet, csv, zstd, walkdir, tempfile, serial_test)"
    - "Cargo.lock (resolver output)"
    - "crates/miner-core/Cargo.toml (deps: arrow + tempfile; dev-deps: proptest + insta)"
    - "crates/miner-core/src/lib.rs (FROZEN surface extended: Calendar + Blake3Hex/ClosedRangeUtc/RawBar/Reader/Side; new pub mod reader + pub mod calendar)"
    - "crates/miner-reader-dukascopy/Cargo.toml (replace stub deps with csv/zstd/walkdir/chrono/blake3/thiserror/tracing/serde + dev-deps tempfile/proptest)"
    - "crates/miner-reader-dukascopy/src/lib.rs (replace placeholder with module declarations + FROZEN re-exports)"

key-decisions:
  - "Calendar shipped as placeholder-only stub. Plan 02-02 owns the real shape (fx_major + is_open_at); shipping the placeholder now keeps the dependency graph one-PR and lets downstream plans import miner_core::Calendar from day one without churn when the real implementation lands."
  - "Reader::source_id() return type tightened from `&str` to `&'static str` in the trait. The pedantic clippy lint `unnecessary_lifetimes_for_returns` fires on the impl when the trait declares `&str`; every reader's source-id is a compile-time constant, so the stricter trait signature reflects reality."
  - "Blake3Hex Serialize/Deserialize/JsonSchema impls are MANUAL (not derived). serde's default-derived impls cap fixed-size array length at 32; we need 64 (blake3 hex). Mirrors the RunId manual-impl pattern in findings/run_id.rs."
  - "Read-up-front for zero-byte detection. std::fs::read pulls the full .csv.zst into Vec<u8> BEFORE the zstd decoder runs, so an empty file becomes a CorruptSourceFile iterator item rather than a misleading 'no bars' empty stream. The same Vec<u8> feeds the blake3 hasher when fingerprint_day is called (no double-read)."
  - "Iterator-of-Result via Arc<str>-shared symbol. read_1m_bars's flat_map closure receives a fresh Arc::clone (refcount bump) per day so the borrow checker accepts the day-iterator escaping the FnMut closure. Cheap (no String alloc per day) and dyn-compatible."
  - "Workspace pins `arrow = 58` and `parquet = 58` at the same major. Phase 2 plan only USES arrow at runtime (Plan 02-05), but declaring parquet at the same major locks the lockstep contract per CLAUDE.md §'Version Compatibility' so a future addition cannot drift."

patterns-established:
  - "Path-layout encapsulation: DukascopyMonth(u8) with private inner + day_csv_zst sole public constructor — the 00-indexed-month quirk cannot leak out of path_layout.rs."
  - "Streaming-reader pipeline shape: BufReader::with_capacity(1MB) -> zstd::stream::read::Decoder -> csv::ReaderBuilder::has_headers(true).from_reader. Single-threaded zstd (no zstdmt feature), 1MB buffer caps amortise syscall cost. No mmap (workspace forbid(unsafe_code))."
  - "Boundary-rename for terminology drift: CSV `volume` (string) is parsed into a private RawRow struct via serde, then renamed to RawBar.tick_volume: f64 at construction time. The boundary catches any future drift via grep."
  - "walkdir must sort: every WalkDir::new(...).sort_by_file_name() chain — mandatory for byte-identity across runs (PATTERNS pitfall 1). Single call site in reader.rs:170."
  - "Object-safety regression test: compile-time `fn _accept(_: &dyn Reader<Error = std::io::Error>)` mirrors Phase 1's FindingSink::trait_object_safe — workspace stops building if the trait becomes non-dyn-compatible."
  - "Test-fixture helpers in tests/fixtures/mod.rs imported as `mod fixtures;` from integration tests — mirrors Phase 1's pattern, no shared crate needed."

requirements-completed:
  - CACHE-01
  - CACHE-02
  - CACHE-05

# Metrics
duration: ~35min
completed: 2026-05-18
---

# Phase 02 Plan 01: Reader infrastructure (Wave 0) Summary

**Wave 0 reader foundation: `miner_core::Reader` trait + `DukascopyReader` zstd-CSV impl + sealed `DukascopyMonth` 00-indexed path encapsulation + `SyntheticCache` test fixture — unblocks every subsequent Phase 2 wave.**

## Performance

- **Duration:** ~35 min
- **Completed:** 2026-05-18
- **Tasks:** 4
- **Files modified:** 11 (5 created, 6 modified, plus Cargo.lock)
- **Tests added:** 17 (3 reader-unit, 9 path_layout, 5 reader_smoke integration)

## Accomplishments

- **Reader trait** in `miner-core` exposes the pluggable data-source surface every later Phase-2 wave consumes (aggregator, gap detector, cache).
- **`DukascopyReader`** is the first concrete impl — streams 1-minute bars from `<root>/<SYMBOL>/<YYYY>/<MM 00-indexed>/<DD>_<bid|ask>.csv.zst` in ascending UTC order via a `BufReader(1MB) -> zstd::Decoder -> csv::Reader` pipeline.
- **`DukascopyMonth(u8)`** sealed-newtype encapsulates the 00-indexed-month quirk so calendar/Dukascopy month confusion is structurally impossible. Full round-trip via `day_csv_zst` / `parse_day_path` is enforced by a 256-case proptest.
- **`RawBar.tick_volume: f64`** (A1 invariant) — the CSV `volume` column is renamed at the reader boundary; the type system guarantees the rename cannot drift back to `volume` / `tick_count`.
- **`SyntheticCache` test fixture** writes zstd-compressed CSV days under a `TempDir`, matching the production layout — the substrate every later Phase-2 wave builds its tests on.
- **Workspace dep graph extended** with arrow / parquet (same major lockstep per D2-01), csv, zstd, walkdir, tempfile, serial_test for runtime; proptest + insta for dev. FOUND-04 gate (zero tokio/async in `cargo tree -p miner-core --edges normal,build`) intact.

## Task Commits

Each task was committed atomically (worktree branch `worktree-agent-ab92e82b9c3929cf0`, base `3efaf38`):

1. **Task 1: add Phase 2 workspace deps + extend miner-core/reader-dukascopy deps** — `be94ec6` (feat)
2. **Task 2: Reader trait + RawBar/Side/ClosedRangeUtc/Blake3Hex types + dyn-compat test** — `64e0901` (feat)
3. **Task 3: DukascopyMonth newtype + day_csv_zst + parse_day_path + path_layout tests** — `622eafb` (feat)
4. **Task 4: DukascopyError + DukascopyReader impl + SyntheticCache fixture + reader_smoke integration tests** — `4777821` (feat)

_Plan metadata commit (this SUMMARY) follows._

## Files Created/Modified

### Created

- `crates/miner-core/src/reader.rs` — Reader trait + Side + ClosedRangeUtc + RawBar + Blake3Hex (manual serde/JsonSchema for `[u8; 64]`) + RawBarIter type alias + object-safety test
- `crates/miner-core/src/calendar.rs` — Placeholder stub for Plan 02-02 to fill
- `crates/miner-reader-dukascopy/src/path_layout.rs` — Sealed DukascopyMonth + day_csv_zst (only public constructor) + parse_day_path inverse + PathParseError + 9 tests (including 256-case proptest)
- `crates/miner-reader-dukascopy/src/error.rs` — DukascopyError thiserror enum (Io, Csv, WalkDir, TimestampParse, MissingField, CorruptSourceFile, PathLayout, HexDecode) + From<PathParseError> + From<DukascopyError> for miner_core::WireError
- `crates/miner-reader-dukascopy/src/reader.rs` — DukascopyReader struct + impl miner_core::Reader (source_id, trading_calendar, read_1m_bars, fingerprint_day, enumerate_days) + day_bar_iter helper
- `crates/miner-reader-dukascopy/tests/fixtures/mod.rs` — SyntheticCache + one_minute_csv_body + whole_day_range test fixtures
- `crates/miner-reader-dukascopy/tests/reader_smoke.rs` — 5 integration tests (reads_one_day_in_order, zero_byte_file, tick_volume_from_csv_volume, fingerprint_round_trip, enumerate_days_sorted_ascending)

### Modified

- `Cargo.toml` — Append arrow / parquet / csv / zstd / walkdir / tempfile / serial_test to `[workspace.dependencies]`
- `crates/miner-core/Cargo.toml` — Add `arrow.workspace = true` + `tempfile.workspace = true` to `[dependencies]`; add `proptest = "1.11"` + `insta = "1.47"` to `[dev-dependencies]`
- `crates/miner-core/src/lib.rs` — Append-only: `pub mod calendar; pub mod reader;` + `pub use calendar::Calendar; pub use reader::{Blake3Hex, ClosedRangeUtc, RawBar, Reader, Side};`
- `crates/miner-reader-dukascopy/Cargo.toml` — Replace stub deps with the full Phase 2 set (csv/zstd/walkdir/chrono/blake3/thiserror/tracing/serde + dev-deps tempfile/proptest)
- `crates/miner-reader-dukascopy/src/lib.rs` — Replace placeholder with module declarations + FROZEN re-exports

## Decisions Made

See `key-decisions` in frontmatter. Two highlights:

- **`Reader::source_id() -> &'static str`** (NOT `&str`) — every reader's source-id is a compile-time constant; the stricter trait signature reflects reality and silences pedantic clippy lints uniformly on every impl. (Minor trait-signature tightening from the original CONTEXT D2-12 sketch; consistent with the spirit of the contract since none of the documented use-cases require dynamic dispatch on this value.)
- **`Blake3Hex` has manual `Serialize` / `Deserialize` / `JsonSchema` impls.** serde's default-derived impls cap fixed-size arrays at 32; blake3 hex is 64. Mirrors `RunId`'s manual-impl pattern from Phase 1 (`findings/run_id.rs:48-65`).

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] `Blake3Hex` could not `#[derive(Serialize, Deserialize, JsonSchema)]` on `[u8; 64]`**

- **Found during:** Task 2 (Reader trait + supporting types)
- **Issue:** Plan called for `#[derive(Serialize, Deserialize, JsonSchema)]` on `Blake3Hex(pub [u8; 64])`. serde 1.0 only auto-derives those traits for fixed-size arrays up to length 32; the compile failed with `the trait bound [u8; 64]: serde::Serialize is not satisfied` (and equivalents for Deserialize + JsonSchema).
- **Fix:** Implemented `Serialize`, `Deserialize`, and `JsonSchema` MANUALLY on `Blake3Hex`. Wire form is the 64-char lowercase-hex string with regex `^[0-9a-f]{64}$`. Validation in `Deserialize` rejects non-hex / uppercase / wrong-length input. Mirrors the RunId pattern at `findings/run_id.rs:48-65`. Also kept the inner field private (was originally `pub [u8; 64]` per plan) and added `as_str` + `as_bytes` accessors plus `from_hex_bytes` constructor; private-by-default is the safer invariant.
- **Files modified:** `crates/miner-core/src/reader.rs`
- **Verification:** `cargo test -p miner-core --lib reader::tests::blake3_hex_as_str_matches_bytes` passes; `cargo build --workspace` clean.
- **Committed in:** `64e0901` (Task 2)

**2. [Rule 3 - Blocking] Trait method returned `&str` triggered `clippy::elidable_lifetime_names` on impls**

- **Found during:** Task 4 (DukascopyReader impl) — clippy pedantic fail on `source_id(&self) -> &str { "dukascopy" }`
- **Issue:** Returning a `&'static str` literal through a trait method declared as `-> &str` is "an unnecessary lifetime tie to `&self`" — clippy's `unnecessary_lifetimes_for_returns` warns. Allowing it on every impl is noisy; allowing it on the trait declaration is wrong because the trait's contract is "any &str". Per CONTEXT D2-12 the documented examples all return constants.
- **Fix:** Tighten the trait method signature in `miner-core/src/reader.rs` to `fn source_id(&self) -> &'static str`. Documented why in the trait-method doc comment. Updated the impl in `DukascopyReader::source_id` to match.
- **Files modified:** `crates/miner-core/src/reader.rs`, `crates/miner-reader-dukascopy/src/reader.rs`
- **Verification:** `cargo clippy --workspace --all-targets -- -D warnings` exits 0 (was failing before).
- **Committed in:** `4777821` (Task 4 — trait edit + impl update colocated since they have to be in lockstep)

**3. [Rule 3 - Blocking] `Reader::read_1m_bars` trait return type triggered `clippy::type_complexity`**

- **Found during:** Task 2 (Reader trait)
- **Issue:** `-> Result<Box<dyn Iterator<Item = Result<RawBar, Self::Error>> + Send + 'a>, Self::Error>` is two levels of nesting + a trait object — clippy flags it as "very complex type used".
- **Fix:** Introduced `pub type RawBarIter<'a, E> = Box<dyn Iterator<Item = Result<RawBar, E>> + Send + 'a>` adjacent to the trait. The trait method now reads `-> Result<RawBarIter<'a, Self::Error>, Self::Error>`. Same wire type; readable name. Not added to `lib.rs`'s FROZEN block (it's a return-type alias, not a consumer-facing type).
- **Files modified:** `crates/miner-core/src/reader.rs`
- **Verification:** clippy gate green; trait + impl compile unchanged.
- **Committed in:** `64e0901` (Task 2)

**4. [Rule 1 - Bug] FnMut closure couldn't pass `&str` symbol into flat_map iterator**

- **Found during:** Task 4 (`DukascopyReader::read_1m_bars`)
- **Issue:** Initial impl had `let symbol_owned = symbol.to_string(); days.into_iter().flat_map(move |date| day_bar_iter(self, &symbol_owned, side, date, range))`. The borrow checker rejects this: an `FnMut` closure can't let a reference to its captured `String` escape into the returned `flat_map` iterator (each per-day iterator must outlive the closure body's call).
- **Fix:** Switched to `let symbol_arc: std::sync::Arc<str> = std::sync::Arc::from(symbol)` and pass `Arc<str>` by value (cheap refcount bump per day) into `day_bar_iter`. Added `#[allow(clippy::needless_pass_by_value)]` on `day_bar_iter` with rationale-doc — taking `&Arc<str>` would re-trigger the same borrow problem.
- **Files modified:** `crates/miner-reader-dukascopy/src/reader.rs`
- **Verification:** `cargo test -p miner-reader-dukascopy --test reader_smoke` — all 5 tests green.
- **Committed in:** `4777821` (Task 4)

**5. [Rule 1 - Bug] `tracing::debug!(symbol, ...)` cannot accept `Arc<str>` as the field value**

- **Found during:** Task 4 — compile error after switching to `Arc<str>` (deviation #4 above)
- **Issue:** `tracing`'s `Value` trait isn't implemented for `Arc<str>` directly; only `&str` / `&T` / `Box<T>` / etc.
- **Fix:** `tracing::debug!(symbol = %symbol, ?side, ?date, ...)` — the `%` sigil routes through `Display`, which `Arc<str>` provides via deref-to-`str`.
- **Files modified:** `crates/miner-reader-dukascopy/src/reader.rs`
- **Verification:** build clean; tracing emit still structured.
- **Committed in:** `4777821` (Task 4)

**6. [Rule 1 - Bug] Test fixtures triggered `clippy::cast_possible_wrap` + `clippy::cast_precision_loss`**

- **Found during:** Task 4 (`fixtures/mod.rs::one_minute_csv_body`)
- **Issue:** `i as i64` (usize -> i64 cast for chrono::Duration::minutes) and `(i as f64) * 0.0001` / `(i + 1) as f64` (usize -> f64 for synthetic OHLC values) both fire clippy pedantic lints. Lints are intended for production code where wraparound is possible; in test fixtures `i` is bounded by `count` parameter (max 1440 per call in practice).
- **Fix:** Added `#[allow(clippy::cast_possible_wrap, clippy::cast_precision_loss)]` on `one_minute_csv_body` with rationale-doc comment ("bounded by the synthetic-test domain").
- **Files modified:** `crates/miner-reader-dukascopy/tests/fixtures/mod.rs`
- **Verification:** clippy gate green.
- **Committed in:** `4777821` (Task 4)

**7. [Rule 1 - Bug] Documentation backticks + let-else + match-to-if-let lints**

- **Found during:** Tasks 2 + 3 + 4 (assorted)
- **Issue:** `clippy::doc_markdown` (missing-backticks on `tick_volume` / `f64` / `BufReader` / `TempDir` / `flat_map`); `clippy::let_underscore` / `clippy::single_match_else` on `match path_layout::parse_day_path(p) { Ok(parsed) => parsed, Err(_) => { ...continue } }`; `clippy::missing_panics_doc` on `Blake3Hex::as_str`.
- **Fix:** Reword doc comments to use backticks; rewrite match → let-else; add `# Panics` doc section.
- **Files modified:** `crates/miner-core/src/reader.rs`, `crates/miner-reader-dukascopy/src/reader.rs`, `crates/miner-reader-dukascopy/tests/fixtures/mod.rs`
- **Verification:** `cargo clippy --workspace --all-targets -- -D warnings` green.
- **Committed in:** `64e0901` (T2), `622eafb` (T3), `4777821` (T4)

---

**Total deviations:** 7 auto-fixed (5 × Rule 3 blocking, 2 × Rule 1 bug)
**Impact on plan:** All auto-fixes were small structural adjustments needed to make the planned types compile and pass the workspace's pedantic-clippy gate. No scope creep; no functionality cut. The two highest-leverage changes — manual `Blake3Hex` serde impl and `source_id -> &'static str` — are documented at the type/trait level so downstream plans inherit the rationale without re-deriving it.

## Threat Surface Audit

Task 4 introduces the reader-side trust boundary (filesystem → Reader). All STRIDE entries from the plan `<threat_model>` are mitigated as planned:

- **T-02-01 Tampering, source .csv.zst bytes**: blake3 fingerprint over FULL file bytes — implemented in `DukascopyReader::fingerprint_day` (hashing `std::fs::read(&path)` output BEFORE zstd decompress). Cache invalidation surface lands in Plan 02-05.
- **T-02-02 DoS, malformed CSV / unbounded zstd**: `BufReader::with_capacity(1MB)` caps the read buffer; default-streaming zstd decoder has no allocation amplifier; csv::Reader is line-oriented (one row in memory at a time).
- **T-02-03 Info Disclosure, error messages with paths**: accepted — `DukascopyError` variants embed paths for diagnostics; wire-form `WireError` strips Debug formatting.
- **T-02-04 Tampering, DukascopyMonth wrong index**: private inner u8 + `from_calendar` smart constructor + `out_of_range_panics` test + 256-case `path_round_trip_proptest` enforces the round-trip invariant.
- **T-02-SC Supply chain**: All Phase 2 deps (arrow, csv, zstd, walkdir, tempfile, proptest, insta) are documented CLAUDE.md stack-table members; no new packages introduced beyond the audited set.

No new threat surface introduced outside the plan's threat model.

## Known Stubs

- `crates/miner-core/src/calendar.rs` — `Calendar` is a placeholder unit struct. Plan 02-02 fills the real fields (`weekly_open_utc`, `weekly_close_utc`, `yearly_holidays`) and the `is_open_at` predicate. **Why intentional:** Decision documented in CONTEXT D2-06/D2-07/D2-08 — the real Calendar shape belongs to Plan 02-02; shipping the placeholder here keeps the dependency graph one-PR so downstream Plan 02-04 can already `use miner_core::Calendar` without churn when Plan 02-02 lands. `Reader::trading_calendar()` is wired but returns the empty placeholder; no consumer in Plan 02-01 inspects the calendar's contents.
- `crates/miner-reader-dukascopy/src/reader.rs` — `DukascopyReader::trading_calendar` returns the placeholder via `self.calendar.clone()`. Consequence: until Plan 02-02 lands, `is_open_at` is unavailable on the calendar — but no Plan 02-01 caller invokes it (only Plan 02-02's gap-detector and aggregator do).

No production stubs that block this plan's goal — the goal is the reader trait + impl + fixture, all complete.

## Issues Encountered

None beyond the documented auto-fixes above. No upstream blocker, no environmental issue.

## Self-Check: PASSED

- [x] `Cargo.toml` workspace deps include `arrow = "58"` AND `parquet = "58"` at same major.
- [x] `crates/miner-core/src/reader.rs` exports the 5 public types + `Reader` trait.
- [x] `crates/miner-core/src/calendar.rs` exists (placeholder stub).
- [x] `crates/miner-core/src/lib.rs` re-exports `Calendar` + `Blake3Hex/ClosedRangeUtc/RawBar/Reader/Side`.
- [x] `crates/miner-reader-dukascopy/src/path_layout.rs` contains `pub fn day_csv_zst`.
- [x] `DukascopyMonth(u8)` inner field is private (`grep -E 'pub\s+u8'` returns empty).
- [x] `crates/miner-reader-dukascopy/src/reader.rs` contains `impl Reader for DukascopyReader`.
- [x] `crates/miner-reader-dukascopy/src/error.rs` exports `DukascopyError` + `From<DukascopyError> for WireError`.
- [x] `crates/miner-reader-dukascopy/tests/fixtures/mod.rs` exports `SyntheticCache`, `one_minute_csv_body`, `write_day`, `write_zero_byte_day`.
- [x] All commits exist in git log: `be94ec6`, `64e0901`, `622eafb`, `4777821`.
- [x] `cargo test --workspace` green (path_layout 9, smoke 5, miner-core lib 32 — all suites pass).
- [x] `cargo clippy --workspace --all-targets -- -D warnings` exits 0.
- [x] `cargo fmt --all --check` exits 0.
- [x] `cargo tree -p miner-core --edges normal,build | grep -E '(tokio|async-std|async-trait)'` empty (FOUND-04 gate intact).
- [x] `grep 'tick_count' reader.rs` returns 0 in both `miner-core/src/reader.rs` and `miner-reader-dukascopy/src/reader.rs` (A1 hard-rename held).
- [x] `grep 'println!|eprintln!|dbg!'` returns 0 in `miner-reader-dukascopy/src/reader.rs` (tracing-only).
- [x] `walkdir::WalkDir::new(...)` call in reader.rs:170 chains `.sort_by_file_name()`.

## Next Phase Readiness

**Wave 0 (Plan 02-01) complete.** The next wave of Phase 2 (Plans 02-02 / 02-03 / 02-04 in Wave 1) can start in parallel against this baseline:

- **Plan 02-02** (aggregator + Calendar): replaces the placeholder `Calendar` with the FX-major default + `is_open_at` predicate; consumes the `Reader` trait + `RawBar`; lands the aggregator kernel.
- **Plan 02-03** (DST / edge-case tests): depends on 02-02's aggregator fixtures; uses `SyntheticCache` to build DST-transition synthetic days.
- **Plan 02-04** (gap detector + manifest): consumes `Reader` + `Calendar` + `enumerate_days` for the gap-detection passes. Insta-snapshot tests use the `insta = 1.47` dev-dep landed in this plan.

No blockers. No follow-ups requested. The placeholder `Calendar` is the only intentional stub — Plan 02-02's first task replaces it transparently.

---
*Phase: 02-reader-aggregator-derived-bar-cache*
*Plan: 01*
*Completed: 2026-05-18*
