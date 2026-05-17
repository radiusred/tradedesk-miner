# Phase 2: Reader, Aggregator & Derived-Bar Cache — Context

**Gathered:** 2026-05-17
**Status:** Ready for planning

<domain>
## Phase Boundary

Phase 2 delivers the data layer every later phase consumes:

1. **Dukascopy zstd-CSV reader** — reads `<root>/<SYMBOL>/<YYYY>/<MM 00-indexed>/<DD>_<bid|ask>.csv.zst` 1-minute bars without re-download or layout change (CACHE-01); 00-indexed month quirk is boundary-tested and encapsulated.
2. **`Reader` trait** — pluggable data-source abstraction; Dukascopy is the first impl (CACHE-02).
3. **Aggregator** — pure function of (1m source bars, params); produces UTC-aligned, close-aligned, gap-omitting (never interpolated) OHLC + `tick_volume` bars at 15m / 1h / 1d (CACHE-03, CACHE-04, CACHE-05).
4. **Derived-bar cache (Arrow IPC)** — semi-permanent on-disk cache keyed by `(source_id, symbol, side, timeframe)` with `aggregator_version` + per-day source fingerprint invalidation (CACHE-06).
5. **Gap manifest** — structured pre-scan report of `(symbol, side, [(start, end, reason)])` covering missing daily files, zero-byte/corrupt files, and intra-day bar holes against the instrument's trading calendar (CACHE-07).
6. **Gap policy enforcement scaffolding** — the gap manifest is the input; actual `strict` / `continuous_only` policy enforcement at scan-engine boundary is Phase 3 (CACHE-08 is partially Phase 2 / partially Phase 3 — see boundary note below).

What Phase 2 does NOT deliver (belongs in later phases): the scan engine + facade (Phase 3), gap-policy enforcement on a running scan (Phase 3), any scans (Phase 4), the MCP/HTTP wrappers (Phase 6), the bench harness (Phase 7). Aggregator hooks for future PyO3 export (PLAT-v2-02) are an implementation byproduct of Arrow IPC, not a Phase 2 deliverable.

The user is not a Rust practitioner; downstream agents should default to the most pragmatic Rust-community-standard patterns for any choice not explicitly captured below.

**CACHE-08 boundary clarification:** Phase 2 owns *building and exposing* the gap manifest. Phase 3 owns *enforcing the policy* (deciding to abort vs partition vs proceed). The `kind: gap_aborted` finding emitted under `strict` policy is built from a Phase-2-supplied gap manifest but emitted by Phase 3 scan-engine code. Phase 2 ships the API surface that Phase 3 will call.
</domain>

<decisions>
## Implementation Decisions

### Derived-Bar Cache Format

- **D2-01: Arrow IPC files (one per quartet).** Cache files are `<bar_cache_root>/<source_id>/<SYMBOL>/<timeframe>_<side>.arrow` — one Arrow IPC file per `(source_id, symbol, side, timeframe)` covering all years. Adds `arrow = "53"` (lockstep with `parquet = "53"`) to `miner-core` workspace deps. Chosen specifically because PROJECT.md flags PLAT-v2-02 (Python aggregator export — replace tradedesk's Python 1m→Nm aggregation with miner's Rust impl) as a future goal: Arrow IPC is Polars/Pandas-readable in one line, making that goal trivial to land later. Rejected: Parquet (compression tax unnecessary for write-once cache), bincode+zstd (Rust-only, blocks Python interop), custom mmap binary (loses all tooling interop).

- **D2-02: Year partitioning deferred to v2.** v1 ships ONE Arrow file per quartet covering all years. CLAUDE.md notes year-partitioning (`EURUSD/15m/bid/2024.arrow`, `…/2025.arrow`) as a natural upgrade path for cheaper append + parallel reads, but for the v1 cache size (28 symbols × 6 years × 1.4M rows max) a single file per quartet is well-bounded. Re-evaluate if a quartet file exceeds ~500MB in profiling.

- **D2-03: Per-day fingerprint sidecar for invalidation.** Each `.arrow` file has a sibling `<timeframe>_<side>.fingerprints.json` mapping `YYYY-MM-DD → blake3_hex(source_csv_zst_bytes)`. On `aggregate(symbol, side, range)`:
  1. Walk source-cache days in range. Compute current `(day → blake3)` map.
  2. Read sidecar. Diff against current.
  3. Days with mismatched or missing fingerprints → regenerate bars for those days from source.
  4. Splice new bars into the existing Arrow file (read existing → drop affected day's rows → append regenerated rows → rewrite file).
  5. Persist updated sidecar.
  
  Cost: a Dukascopy refresh of one day forces re-aggregation of one day's bars (~5KB rewritten), not the whole multi-year file. Trade-off: implementation complexity vs. the 28-symbol × 6-year rebuild cost that whole-file invalidation would incur.

- **D2-04: Cache invalidation triggers.** The cache MUST regenerate when ANY of the following differ from the sidecar's record:
  - `aggregator_version` (a const string in `miner-core::aggregator`; bump on any aggregator code change that changes output bytes for the same input)
  - Per-day source fingerprint (blake3 over the source `.csv.zst` bytes)
  - Schema-version of the Arrow file (carried in Arrow file metadata; bump on any column shape change to the bar frame)
  
  `aggregator_version` and `arrow_schema_version` are both string constants stored in BOTH the Arrow file metadata AND the sidecar root — either-side change → full rebuild of the affected file. (Day-granular invalidation only kicks in for source fingerprint mismatches.)

- **D2-05: blake3 for fingerprints.** `blake3` is already a workspace dep (from Phase 1 `Cargo.toml`); reuse it. Hash is over the full `.csv.zst` byte content (not decompressed), so two byte-identical source files always produce the same fingerprint. Stored as the 64-char lowercase hex string.

### Trading Calendar (for gap detection)

- **D2-06: Hardcoded FX-major default + `Reader::trading_calendar()` hook.** Phase 2 ships ONE built-in calendar — the FX-major default — accessed via `Reader::trading_calendar() -> Calendar`. Per-symbol overrides at v1 are accomplished by individual readers implementing the method (e.g., a future `miner-reader-equity` would return a different calendar). No config-supplied per-symbol calendars in v1; that's a Phase 3+ concern when scan params bind calendar.

- **D2-07: FX-major default scope.** The hardcoded default is:
  - **Weekend closure:** Friday 22:00 UTC → Sunday 22:00 UTC closed every week.
  - **Holidays:** Dec 25 (Christmas Day) and Jan 1 (New Year's Day) closed each year — computed, not table-hardcoded.
  - **No other holidays in v1.** Good Friday, Thanksgiving, country-specific bank holidays are scan-engine concerns when they ever come up (per-instrument override hook is how they enter).
  - **Anything during the closed window is NOT a gap.** Anything during the open window with missing data IS a gap.

- **D2-08: `Calendar` API shape.** The exposed trait method:
  ```rust
  pub trait Reader {
      fn trading_calendar(&self) -> Calendar;
      // ... existing read methods …
  }
  
  pub struct Calendar {
      // Both members of a 24/5 schedule expressed in UTC.
      pub weekly_open_utc: (Weekday, NaiveTime),    // e.g., (Sunday, 22:00)
      pub weekly_close_utc: (Weekday, NaiveTime),   // e.g., (Friday, 22:00)
      pub yearly_holidays: Vec<(u32, u32)>,         // (month, day) tuples — applied every year
  }
  
  impl Calendar {
      pub fn is_open_at(&self, ts: DateTime<Utc>) -> bool { … }
      // Helper that scan code + gap-detector both call.
  }
  ```
  Closed-form predicate, no allocation per check, easy to test. Aggregator passes the calendar to the gap detector; both Phase 2 internal code and Phase 3 scan code call `is_open_at`.

### Time / DST handling

- **D2-09: chrono only.** Stay on `chrono` for Phase 2 — Phase 1 envelope types (`RunStart`, `RunEnd`, etc.) already use `chrono::DateTime<Utc>`, and the schema-sync CI gate would treat a `jiff` switch as a schema bump requiring `schema_version = 2`. CLAUDE.md's preference for `jiff` is acknowledged; defer migration to a dedicated future phase if the friction ever justifies it.

- **D2-10: chrono-tz only if needed.** Phase 2's primary time math is UTC-only (bars are bucketed in UTC, the FX calendar is expressed in UTC). If a per-symbol override calendar ever needs a non-UTC zone (Phase 3+), pull `chrono-tz` then. Phase 2 does NOT introduce `chrono-tz`.

- **D2-11: Aggregator is fully UTC-agnostic about DST.** Because bars are UTC-bucketed and FX defaults are UTC-expressed, DST is INVISIBLE to the aggregator. The DST spring-forward and fall-back fixture tests called for in Phase 2 success criterion #5 verify exactly this — the aggregator must produce correct UTC bars across DST transitions WITHOUT special-casing them. The test fixtures supply 1m source data spanning a DST boundary (in some non-UTC zone Dukascopy might report from); the assertion is "bar boundaries at 02:00, 03:00 UTC etc. contain exactly the source bars whose UTC timestamp falls in that hour, regardless of what wall-clock time that was in any other zone." Spring-forward and fall-back tests differ only in source data shape.

### Reader Trait Surface (Claude's Discretion — flagged for Plan-phase confirmation)

The user did not select "Reader trait surface" as a discussion area; default decisions follow. Mark these in PLAN.md if research surfaces a reason to deviate.

- **D2-12 (discretion): Reader trait returns iterators, not materialized Vecs.**
  ```rust
  pub trait Reader: Send + Sync {
      type Error: std::error::Error;
      
      fn source_id(&self) -> &str;
      fn trading_calendar(&self) -> Calendar;
      
      fn read_1m_bars(
          &self,
          symbol: &str,
          side: Side,
          range: ClosedRangeUtc,
      ) -> Result<Box<dyn Iterator<Item = Result<RawBar, Self::Error>> + Send + '_>, Self::Error>;
      
      fn fingerprint_day(&self, symbol: &str, side: Side, date: NaiveDate) -> Result<Option<Blake3Hex>, Self::Error>;
      // None = day file absent; Some(hex) = day fingerprint.
  }
  ```
  Streaming over years of 1-minute data is memory-bounded; the aggregator pulls a windowed view per output bar. `fingerprint_day` is a separate method so the cache invalidator can walk fingerprints without parsing CSV.

- **D2-13 (discretion): `RawBar` shape.** `RawBar { ts_open_utc: DateTime<Utc>, ts_close_utc: DateTime<Utc>, open: f64, high: f64, low: f64, close: f64, tick_volume: f64 }`. `ts_open_utc` is the start of the 60-second window; `ts_close_utc = ts_open_utc + 60s`. `tick_volume` comes from Dukascopy's per-resample `volume` field per CACHE-05, which `tradedesk-dukascopy/export.py:312` builds as `vol.resample(rule).sum()` over per-tick `bid_vol`/`ask_vol` floats — it is a **sum of per-tick float volumes** within the bar, not an integer tick count. The Dukascopy `volume` name is renamed to `tick_volume` at the reader boundary (never propagated as `volume`) so downstream consumers cannot confuse it with contract volume. Aggregation across bars: sum (matches Dukascopy's own `vol.resample().sum()` semantics).

- **D2-14 (discretion): `AggregatedBar` shape.** Same as `RawBar` but with `tf: Timeframe` instead of fixed 1m. The aggregator output. Column-oriented BarFrame is the materialized view at the cache boundary:
  ```rust
  pub struct BarFrame {
      pub source_id: String,
      pub symbol: String,
      pub side: Side,
      pub tf: Timeframe,
      pub ts_open_utc: Vec<DateTime<Utc>>,
      pub ts_close_utc: Vec<DateTime<Utc>>,
      pub open: Vec<f64>,
      pub high: Vec<f64>,
      pub low: Vec<f64>,
      pub close: Vec<f64>,
      pub tick_volume: Vec<f64>,
  }
  ```
  Column-oriented because that's the access pattern for scans (load one column at a time) AND the Arrow IPC mapping is trivial.

- **D2-15 (discretion): Aggregator is a pure function.** `Aggregator::aggregate(reader: &dyn Reader, params: AggParams) -> Result<BarFrame>` — no IO outside what the reader provides, no clock reads, no env reads. Determinism is the contract (CACHE-04 byte-identity).

### Gap Detection (Claude's Discretion — separable from cache)

- **D2-16 (discretion): `GapDetector` is a separate module.** Lives in `miner-core::gap` (NOT inside the aggregator or reader). Takes `(reader: &dyn Reader, symbol, side, range)` and produces `GapManifest`. Three gap classes (matching CACHE-07 verbatim):
  1. **Missing daily file** — `(start: day 00:00 UTC, end: day 24:00 UTC, reason: "missing_source_file")`
  2. **Zero-byte / corrupt source file** — same range, `reason: "corrupt_source_file"`
  3. **Intra-day hole during open hours** — `(start: hole_open, end: hole_close, reason: "intra_day_gap")` — computed by comparing actual 1m bar timestamps against `Calendar::is_open_at` per minute in the range.

- **D2-17 (discretion): `GapManifest` shape — feeds D-08's `gap_aborted` finding.**
  ```rust
  pub struct GapManifest {
      pub source_id: String,
      pub symbol: String,
      pub side: Side,
      pub queried_range: ClosedRangeUtc,
      pub gaps: Vec<GapSpan>,
  }
  pub struct GapSpan {
      pub start_utc: DateTime<Utc>,
      pub end_utc: DateTime<Utc>,
      pub reason: GapReason,  // enum: MissingSourceFile, CorruptSourceFile, IntraDayGap
  }
  ```
  Serde-derive `JsonSchema` — Phase 3 scan engine wraps this in a `Finding::GapAborted` envelope.

### Aggregator Implementation Notes (Claude's Discretion)

- **D2-18 (discretion): `aggregator_version` is a `const &str` in `miner-core::aggregator`.** Bump on any code change that alters output bytes for the same input. CI/PR review must check it bumped when aggregator logic changes; a simple `cargo test` snapshot test on a fixture would catch the inverse case (forgot to bump). Initial value: `"1.0.0"`.

- **D2-19 (discretion): Bar boundary convention.** Bars are close-aligned to UTC midnight: 15m bars at `:00`, `:15`, `:30`, `:45` of each hour; 1h bars at `:00`; 1d bars at `00:00:00 UTC` of each day. A bar at `ts_open_utc = X` covers `[X, X + tf)`. Partial bars (e.g., trading-session open with < 60s of source data in the first 1m bucket) are emitted as-is with their actual open/close timestamps; never padded, never dropped, but flagged via the gap manifest if the bucket is internally incomplete (more than 50% of the 1m sub-buckets missing during open hours).

### Other Claude's Discretion

- **D2-20 (discretion): Cache layout.** `<bar_cache_root>/<source_id>/<SYMBOL>/<timeframe>_<side>.arrow` plus `<…>.fingerprints.json` sidecar. `source_id` is the Reader's stable identifier (e.g., `"dukascopy"`). `bar_cache_root` already exists in `MinerConfig` (FOUND-05, D-16). No leading directory called `arrow_ipc/` or version subdirectory — `aggregator_version` mismatch in metadata triggers rebuild, not path migration.

- **D2-21 (discretion): Test cache fixtures.** Phase 2 ships a tiny synthetic Dukascopy-format cache under `crates/miner-reader-dukascopy/tests/fixtures/` covering a few days × 2 instruments × bid/ask. Fixture generation is a small test helper (no checked-in production data). Synthetic content lets us deterministically test boundary conditions (DST spring/fall, weekend gap, holiday on Jan 1, partial-bar session open).
</decisions>

<spec_lock>
No SPEC.md exists for this phase. Requirements come directly from REQUIREMENTS.md (CACHE-01..08) and the ROADMAP §"Phase 2" success criteria.
</spec_lock>

<canonical_refs>
## Canonical References (MANDATORY reading for downstream agents)

- `.planning/ROADMAP.md` — Phase 2 section: goal, success criteria, requirement IDs
- `.planning/REQUIREMENTS.md` — CACHE-01 through CACHE-08 verbatim
- `.planning/PROJECT.md` — Core value, evolution rules, future PLAT-v2-02 Python aggregator export goal that motivated D2-01 Arrow IPC choice
- `.planning/STATE.md` — Pending decision flagged as "Phase 2 open question" (now closed by D2-01)
- `.planning/phases/01-foundations-contracts/01-CONTEXT.md` — D-01..D-24 envelope and infra contracts. Particularly: D-08 (gap-policy outputs are findings), D-15 (clippy gate covers Phase 2 too), D-16 (figment config — `bar_cache_root` already in `MinerConfig`)
- `.planning/phases/01-foundations-contracts/01-RESEARCH.md` — Phase 1 research; sections on cache-format trade-offs and aggregator determinism are still applicable
- `./CLAUDE.md` — Project constraints (Rust 1.85 / edition 2024, Apache 2.0); §"Technology Stack" lists Arrow IPC, blake3, chrono/jiff trade-offs that grounded this discussion
- `./schemas/findings-v1.schema.json` — The locked envelope schema. Phase 2 must NOT mutate this; `kind: gap_aborted` and `data_slice` fields are pre-allocated for Phase 2/3 wiring
- `./crates/miner-core/src/findings/mod.rs` — Finding variants; Phase 2 fills `GapAbortedFinding` body via Phase 3 facade
- `./crates/miner-core/src/config/mod.rs` — `MinerConfig` already carries `cache_root` and `bar_cache_root` (FOUND-05). Phase 2 reads them; does not modify the schema

**External docs:** None referenced by user during this discussion. Anything Dukascopy-format-specific (the 00-indexed month quirk, the CSV column shape) is documented in the upstream sibling repo `tradedesk-dukascopy` but is NOT a runtime dependency — Phase 2 encapsulates that quirk in the reader.
</canonical_refs>

<code_context>
## Reusable Assets from Phase 1

- **`miner_core::config::MinerConfig`** — already has `cache_root: PathBuf`, `bar_cache_root: PathBuf`, `output: OutputDest`. Phase 2 reads these; does not extend the schema. (Per Phase 1 FROZEN public surface; new public types live in `miner_core::aggregator`, `miner_core::gap`, and `miner_reader_dukascopy::*`.)
- **`miner_core::CODE_REVISION`** — Phase 2 attaches this to BarFrame metadata in the Arrow file so cache rebuilds can be traced to a source revision.
- **`miner_core::error::WireError` + `PreflightCode`** — Phase 2 preflight errors (cache root unreadable, bar cache root unwritable, unknown symbol in source cache) emit `WireError` to stderr per D-06 / D-07. Add new `PreflightCode` variants as needed (e.g., `CacheRootNotFound`, `BarCacheNotWritable`, `SourceFileCorrupt`).
- **`clippy.toml`** — bans `println!`/`eprintln!`. Phase 2 code follows the same discipline; if reader/aggregator need progress output for the CLI's eventual `miner cache` subcommand, it goes through `tracing::info!` (stderr), not stdout.
- **`build.rs` pattern in `miner-core`** — git-SHA injection. Phase 2 inherits this; nothing to add unless `aggregator_version` should be tied to git SHA (decided no in D2-18 — explicit const string).

## New Phase 2 Modules

- `crates/miner-core/src/aggregator/mod.rs` — Aggregator struct + `aggregate(reader, params) -> BarFrame`
- `crates/miner-core/src/aggregator/bar_frame.rs` — `BarFrame` column-oriented type + Arrow IPC encode/decode
- `crates/miner-core/src/cache/mod.rs` — `BarCache` struct, `BarCache::get_or_build(reader, symbol, side, tf, range) -> BarFrame`
- `crates/miner-core/src/cache/fingerprints.rs` — sidecar JSON read/write + day-diff
- `crates/miner-core/src/gap/mod.rs` — `GapDetector`, `GapManifest`, `GapSpan`, `GapReason`
- `crates/miner-core/src/calendar.rs` — `Calendar` struct + FX-major default
- `crates/miner-reader-dukascopy/src/lib.rs` — `DukascopyReader` impl of `Reader` trait
- `crates/miner-reader-dukascopy/src/path_layout.rs` — 00-indexed month encapsulation
- `crates/miner-reader-dukascopy/tests/fixtures/` — synthetic cache test fixture generator

`Reader` trait itself lives in `miner-core::reader::Reader` so it can be implemented by multiple reader crates without circular deps.

## Dependency Direction (one-way, enforced)

```
miner-cli  ──┐
miner-mcp  ──┼──→  miner-reader-dukascopy  ──→  miner-core
miner-http ──┘
```

`miner-reader-dukascopy` depends on `miner-core` (for the `Reader` trait + `Calendar` + `RawBar` types). `miner-core` knows NOTHING about Dukascopy specifically. The CLI/MCP/HTTP wrappers wire a concrete Reader into a `BarCache` at construction time.

## New Workspace Dependencies (to be added in Phase 2)

| Crate | Where | Reason |
|-------|-------|--------|
| `arrow = "53"` | `miner-core` | D2-01 cache format |
| `csv = "1.3"` | `miner-reader-dukascopy` | Dukascopy CSV parsing |
| `csv-core = "0.1"` | `miner-reader-dukascopy` | Hot-path byte-level parser if `csv` profile is too slow (Plan can decide) |
| `zstd = "0.13"` | `miner-reader-dukascopy` | `.csv.zst` decompression |
| `walkdir = "2.5"` | `miner-reader-dukascopy` | Discover source files |
| `tempfile = "3"` | dev-deps both crates | Test fixtures |

All present in CLAUDE.md's recommended stack at HIGH confidence; no surprises.
</code_context>

<deferred_ideas>
## Noted for Later (not Phase 2 scope)

- **Per-symbol calendar overrides via config** — Out of scope (D2-06 limits to Reader-supplied calendars in v1). Phase 3 or beyond when scan params bind calendar.
- **Year-partitioned Arrow files** — Listed in CLAUDE.md as an upgrade path; v2 only if a quartet file exceeds ~500MB in profiling (D2-02).
- **`chrono-tz` for non-UTC calendars** — Pull in when a per-symbol override needs a non-UTC zone (D2-10). Not Phase 2.
- **PyO3 / Python aggregator export (PLAT-v2-02)** — Out-of-scope deferred capability; the Arrow IPC choice (D2-01) intentionally keeps the door open for free.
- **`jiff` migration** — Acknowledged in D2-09; deferred to a dedicated future phase if/when friction warrants. Would touch the Phase 1 envelope schema.
- **DuckDB / SQLite cache** — Wrong shape (row store) / too heavy. Documented rejection in CLAUDE.md; not revisitable in v1.
- **Compression on Arrow IPC** — `arrow-ipc` supports zstd compression natively. v1 ships uncompressed for write-once + fast-read; flip the bit if cache size becomes a constraint.
- **`gap_aborted` finding emission** — Phase 2 builds the gap manifest; Phase 3 wraps it in the `Finding::GapAborted` envelope. D2-16/D2-17 ship the type; do not import the finding type into Phase 2 code paths.
</deferred_ideas>

<open_questions>
## Open Questions for Research / Plan-Phase

None blocking. Items worth checking during `gsd-phase-researcher`:

1. **Arrow-rs API stability for the chosen pattern.** Specifically: can we write a `BarFrame` to `arrow_ipc::writer::FileWriter`, read it back via `arrow_ipc::reader::FileReader`, and update one day's worth of rows in-place without rewriting the whole file? If not, the "splice new bars into the existing file" step in D2-03 falls back to full-file rewrite per affected quartet — still cheaper than full multi-year rebuild but worth knowing the cost.
2. **`csv-core` vs `csv` performance on the hot path.** CLAUDE.md says profile first. Default to `csv` derives in Plan; the Plan can include a microbench task.
3. **Dukascopy CSV column layout.** Verify the column order and timestamp encoding in actual sample files. The 00-indexed month quirk is documented but other quirks (timezone of timestamps, decimal separator, header row presence) need spot-check via the sibling `tradedesk-dukascopy` repo or a fixture file the user supplies.
4. **`Calendar::is_open_at` performance budget.** The gap detector calls this once per minute over the queried range — 60 × 24 × 365 × 6 ≈ 3.2M calls per symbol per scan. Predicate must be O(1) (no allocation, no `chrono-tz` lookup); inline-friendly. Profile and inline if needed.
5. **Whether `Aggregator` and `BarCache` should share a fingerprint trait.** D2-03 splits fingerprint logic into a separate sidecar module. The Plan can validate whether a single `Fingerprint::compute(file: &Path) -> Blake3Hex` helper covers both source-file fingerprints and Arrow-IPC schema-fingerprint use cases.
</open_questions>

---

*Phase 2 context complete. 21 decisions captured (10 user-locked, 11 Claude's discretion within the user-locked framework). All 8 CACHE requirements mapped to specific modules. Ready for `/gsd:plan-phase 2`.*
