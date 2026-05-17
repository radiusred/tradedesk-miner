# Phase 2: Reader, Aggregator & Derived-Bar Cache — Research

**Researched:** 2026-05-17
**Domain:** Rust data layer — pluggable zstd-CSV reader, deterministic UTC-bucketed OHLCV aggregator, Arrow IPC derived-bar cache with day-granular invalidation, structured gap manifest against a hardcoded FX trading calendar.
**Confidence:** HIGH on the Dukascopy CSV schema (sibling repo source code read directly), arrow-rs IPC API shape, csv + zstd + walkdir + memmap2 versions, chrono UTC timestamp parsing, and the determinism landmines. MEDIUM on the optimal cache-write atomicity dance under crash interleavings and on the exact 1m-bucket "completeness" threshold for the gap detector. LOW on `arrow::ipc::writer::FileWriter` precise behaviour when custom schema metadata exceeds a few KB (untested in this research).

## Summary

Phase 2 builds the entire data layer downstream of Phase 1's locked envelope: a `Reader` trait plus its first impl (`DukascopyReader`), an aggregator that turns raw 1m bars into UTC-bucketed 15m / 1h / 1d OHLC + volume frames, a derived-bar cache (Arrow IPC files + JSON fingerprint sidecars) keyed by `(source_id, symbol, side, timeframe)` with day-granular blake3 invalidation, and a `GapDetector` that produces a structured `GapManifest` against a hardcoded FX-major trading calendar. The CONTEXT (D2-01..D2-21) already locks every architectural choice; my role is to verify the upstream facts those decisions assume and to flag the runtime-state realities that will trip an executor agent.

I verified the upstream Dukascopy cache by reading the sibling repo `tradedesk-dukascopy/tradedesk_dukascopy/export.py` directly. **The 00-indexed month layout is confirmed** (`f"{day.month - 1:02d}"` at line 396), the daily CSV is `<day:02d>_<bid|ask>.csv.zst`, the schema is `timestamp,open,high,low,close,volume` with UTC `+00:00` offsets and floats throughout, **and the `volume` column is a sum of per-tick `bid_vol` / `ask_vol` float32 values from the .bi5 ticks** — NOT a tick count. CACHE-05's wording ("reports Dukascopy's `volume` field as tick count") is technically inaccurate against the actual source bytes; the cleanest engineering posture is to rename the field `tick_volume: f64` and document the convention in the README, while CONTEXT.md D2-13 specifies `tick_count: u32`. This is the single most important factual finding in this research — flagged below as **Assumption A1** for plan-phase confirmation.

I verified arrow-rs is at version 58.x (May 2026), not the CLAUDE.md-locked `53` — both are MEDIUM-confidence picks for a write-once cache, but the planner should pin the workspace dep to whatever cargo-add resolves at plan time and accept the drift. Arrow IPC `FileWriter` is strictly **append-only with a finalising footer**; in-place row splicing (D2-03 step 4) is **read existing file → drop the affected day's rows in memory → append all surviving + regenerated rows to a fresh tempfile → atomic rename**. That's the only safe pattern.

I verified `walkdir` without an explicit `sort_by_file_name()` call has "unspecified" iteration order — that's the single most likely determinism break an executor will write. Likewise `HashMap` for any gap-manifest or fingerprint structure breaks determinism; CONTEXT.md already specifies `BTreeMap` for Phase 1 maps, but Phase 2 code must repeat the discipline.

**Primary recommendation:** Honour every D2-01..D2-21 decision in CONTEXT.md verbatim. Add the `arrow` / `csv` / `csv-core` / `zstd` / `walkdir` / `tempfile` / `memmap2` / `globset` / `proptest` / `insta` dependencies at the latest stable versions (NOT CLAUDE.md's stale 2024-era pins — use whatever `cargo add` resolves). Resolve **Assumption A1** before sealing the plan: rename `tick_count: u32` to `tick_volume: f64` (recommended) OR explicitly document that the field name "tick_count" carries a tick-volume sum (the current README wording). Implement the cache write path as write-temp → fsync → atomic rename. Implement the gap detector against a hardcoded FX calendar that respects the Fri-22:00→Sun-22:00 UTC closure plus Dec 25 / Jan 1, NOT the ISO Mon-Fri civil-week convention.

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| Source-cache filesystem discovery | `miner-reader-dukascopy::path_layout` | — | 00-indexed month is a Dukascopy-format detail; must NOT leak into `miner-core` |
| zstd-CSV parsing | `miner-reader-dukascopy` (impl) | `miner-core::reader::Reader` (trait) | Reader trait is source-agnostic; csv+zstd are Dukascopy-specific |
| 1m → Nm bar aggregation | `miner-core::aggregator` | — | Pure function of `(Reader, params)`; source-agnostic |
| Derived-bar Arrow IPC cache | `miner-core::cache` | filesystem (via `MinerConfig::bar_cache_root`) | Cache is the only writable state miner owns (per PROJECT.md constraints) |
| Day-granular fingerprint computation | `miner-core::cache::fingerprints` | `miner-reader-dukascopy` (via `Reader::fingerprint_day`) | The blake3 hash is computed by the reader (which knows the file) but the cache decides which days to recompute |
| Gap detection | `miner-core::gap::GapDetector` | `miner-core::calendar::Calendar` (predicate) | Pure function of `(Reader, symbol, side, range)` — no IO outside the reader |
| Trading-calendar predicate | `miner-core::calendar` | `Reader::trading_calendar()` (per-source override hook) | UTC-only; FX-major default is hardcoded in core; per-reader overrides are a future hook |
| Arrow IPC schema metadata (`aggregator_version`, `arrow_schema_version`) | Arrow file's `Schema::metadata` | sidecar JSON (per-day fingerprints) | Two-axis invalidation: aggregator-or-schema mismatch → full rebuild; day-fingerprint mismatch → day-splice |
| Crash-safe cache writes | `miner-core::cache` write path | `tempfile` + atomic rename | Standard write-temp → fsync → rename pattern; no partial-state observability |
| Concurrent cache reads | mmap (via `memmap2`) | `arrow_ipc::reader::FileReader` | Read-only mmap is lock-free across rayon workers |

## Project Constraints (from CLAUDE.md)

CLAUDE.md is the project's locked technology stack reference. The Phase 2 relevant constraints:

- **Language:** Rust 1.85+ / 2024 edition (matches `rust-toolchain.toml`). No C fallback at this phase.
- **License:** Apache 2.0.
- **Recommended stack (treat as constraint, not suggestion):** `csv` + `csv-core` + serde derives; `zstd` (zstd-rs binding to libzstd); `ndarray` for in-flight numerics; `arrow-rs` for the on-disk bar cache; `memmap2`; `rayon` for parallelism; `walkdir` + `globset` for file discovery; `blake3` for fingerprints; `tracing`/`tracing-subscriber` for logging; `thiserror` (library) / `anyhow` (binary); `chrono` is the safe fallback for time math (`jiff` flagged HIGH-confidence-needs-verify); `serde` + `serde_json` for output. Test stack: `criterion` for microbenches (Phase 7), `proptest` for property tests, `insta` for snapshots, `tempfile` + `assert_cmd` + `predicates` for integration tests, `cargo-nextest` for parallel local runs.
- **"What NOT to Use":** No `polars` for in-flight numerics (heavy DataFrame abstraction); no `zstdmt` feature on zstd (single-threaded decode × parallel files beats multi-threaded × single file); no `mimalloc`/`jemalloc` reflexively (profile first); no custom serialization format (consumers parse the documented schema); no `failure` / `error-chain` (deprecated); no `chrono::Local` (UTC always); no async-std; no PyO3 in v1 (deferred); no DuckDB / SQLite cache (wrong shape).
- **Version pins in CLAUDE.md are STALE as of 2026-05.** CLAUDE.md says `arrow = "53"` and `tokio 1.40+`; actual latest are arrow 58.x (May 2026) and tokio is irrelevant to Phase 2 anyway. Treat CLAUDE.md versions as a floor, not a pin — cargo-add at plan time and update.

CLAUDE.md's "Aggregated-Bar Cache Format" section locks the choice: **Arrow IPC files (not Parquet, not SQLite/DuckDB, not custom binary)**. CONTEXT.md D2-01 honours this. CLAUDE.md flags year-partitioning (`EURUSD/15m/bid/2024.arrow`) as a v2 upgrade path; CONTEXT.md D2-02 defers it. Both consistent.

CLAUDE.md says: *"Run `cargo add rmcp` and inspect the version printed"* — Phase 6 concern, not Phase 2.

No `.claude/skills/` or `.agents/skills/` directories exist in this repo (per the system reminder "No project skills found"). No skill-level constraints to load.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

All 21 decisions D2-01..D2-21 are locked. Reproduced verbatim for the planner / plan-checker / executor:

**Derived-Bar Cache Format**

- **D2-01: Arrow IPC files (one per quartet).** `<bar_cache_root>/<source_id>/<SYMBOL>/<timeframe>_<side>.arrow`, one file per `(source_id, symbol, side, timeframe)` covering all years. Adds `arrow = "53"` (lockstep with `parquet = "53"`) to `miner-core` workspace deps. (Use the latest arrow version at plan time — see Sources of Drift below.)
- **D2-02: Year partitioning deferred to v2.** v1 ships ONE Arrow file per quartet.
- **D2-03: Per-day fingerprint sidecar for invalidation.** `<timeframe>_<side>.fingerprints.json` mapping `YYYY-MM-DD → blake3_hex(source_csv_zst_bytes)`. On `aggregate()`: walk source-cache days in range → compute current `(day → blake3)` → diff against sidecar → regenerate stale days → splice into Arrow file → persist updated sidecar.
- **D2-04: Cache invalidation triggers.** `aggregator_version`, per-day source fingerprint, `arrow_schema_version` — first two and the third bump independently; aggregator/schema mismatch → full rebuild, day fingerprint mismatch → day-splice only.
- **D2-05: blake3 for fingerprints.** Already a workspace dep from Phase 1. Hash the FULL `.csv.zst` byte content (not decompressed); 64-char lowercase hex.

**Trading Calendar (for gap detection)**

- **D2-06: Hardcoded FX-major default + `Reader::trading_calendar()` hook.** Phase 2 ships ONE built-in calendar.
- **D2-07: FX-major default scope.** Fri 22:00 UTC → Sun 22:00 UTC closed every week. Dec 25 + Jan 1 closed each year. Nothing else.
- **D2-08: `Calendar` API shape.** `weekly_open_utc: (Weekday, NaiveTime)`, `weekly_close_utc: (Weekday, NaiveTime)`, `yearly_holidays: Vec<(u32, u32)>`. `is_open_at(ts: DateTime<Utc>) -> bool` is the predicate.

**Time / DST handling**

- **D2-09: chrono only.** Stay on chrono — phase 1 envelope types already use it; switching to jiff is a Phase-1 schema bump.
- **D2-10: chrono-tz only if needed.** UTC math is sufficient for Phase 2.
- **D2-11: Aggregator is fully UTC-agnostic about DST.** Bars bucket in UTC; DST is invisible. DST fixture tests verify this property.

**Reader Trait Surface (Claude's Discretion — flagged for confirmation)**

- **D2-12: Reader trait returns iterators, not Vecs.** `read_1m_bars(symbol, side, range) -> Result<Box<dyn Iterator<Item = Result<RawBar, Self::Error>> + Send + '_>, Self::Error>`. Also exposes `source_id()`, `trading_calendar()`, `fingerprint_day(symbol, side, date) -> Result<Option<Blake3Hex>>`.
- **D2-13: `RawBar` shape.** `{ ts_open_utc, ts_close_utc, open, high, low, close, tick_count: u32 }`. Tick count derived from Dukascopy's `volume` field. **(See Assumption A1 — the source field is tick-volume float sum, not a count.)**
- **D2-14: `AggregatedBar` / `BarFrame` shape.** Column-oriented `BarFrame { source_id, symbol, side, tf, ts_open_utc: Vec<DateTime<Utc>>, ts_close_utc: Vec<DateTime<Utc>>, open: Vec<f64>, high: Vec<f64>, low: Vec<f64>, close: Vec<f64>, tick_count: Vec<u32> }`. Maps trivially to Arrow IPC.
- **D2-15: Aggregator is a pure function.** No IO outside Reader, no clock reads, no env reads. Byte-identity is the contract.

**Gap Detection**

- **D2-16: `GapDetector` separate module.** Lives in `miner-core::gap`. Three gap classes: MissingFile, ZeroByte/Corrupt, IntraDayGap (during open hours).
- **D2-17: `GapManifest` shape.** `{ source_id, symbol, side, queried_range, gaps: Vec<GapSpan> }`. `GapSpan { start_utc, end_utc, reason: GapReason }`. Derives `JsonSchema`; Phase 3 will wrap inside `Finding::GapAborted`.

**Aggregator Implementation Notes**

- **D2-18: `aggregator_version` = `const &str` in `miner-core::aggregator`.** Initial value `"1.0.0"`; bump on any byte-affecting code change.
- **D2-19: Bar boundary convention.** Close-aligned to UTC midnight (15m at `:00 :15 :30 :45`, 1h at `:00`, 1d at `00:00:00`). Bar at `ts_open_utc = X` covers `[X, X + tf)`. Partial bars during open hours emitted as-is + flagged in gap manifest if >50% of sub-buckets missing.

**Other Discretion**

- **D2-20: Cache layout.** `<bar_cache_root>/<source_id>/<SYMBOL>/<timeframe>_<side>.arrow` + sidecar. No version subdirectory; mismatches in Arrow metadata trigger rebuild.
- **D2-21: Test fixtures.** Synthetic Dukascopy-format cache under `crates/miner-reader-dukascopy/tests/fixtures/`, generated by a test helper (no checked-in production data).

### Claude's Discretion

D2-12..D2-21 are flagged as "Claude's Discretion within the user-locked framework." Two items in this set need plan-phase confirmation against the upstream-fact findings below:

1. **D2-13 `tick_count: u32` vs. reality.** Source CSV `volume` is a float sum of per-tick volumes (not an integer count). Either rename the field or document the convention. See Assumption A1.
2. **D2-12 `Reader::fingerprint_day` return type.** `Option<Blake3Hex>` distinguishes "file absent" from "file present, here's its hash." Confirmed correct, but the planner must verify the cache invalidator treats `None` as "fingerprint mismatch (cache stale)" vs. "skip this day entirely."

### Deferred Ideas (OUT OF SCOPE)

- Per-symbol calendar overrides via config (Phase 3+).
- Year-partitioned Arrow files (v2 if a file exceeds ~500MB).
- `chrono-tz` (only if a future calendar needs non-UTC zones).
- PyO3 / Python aggregator export (PLAT-v2-02).
- `jiff` migration (would touch Phase 1 envelope schema).
- DuckDB / SQLite cache (rejected outright in CLAUDE.md).
- Arrow IPC zstd compression (v1 ships uncompressed for write-once + fast-read).
- `gap_aborted` finding emission (Phase 3 wraps the manifest; Phase 2 only ships the type).

</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| CACHE-01 | User can read the existing tradedesk-dukascopy zstd-CSV cache (`<root>/<SYMBOL>/<YYYY>/<MM 0-indexed>/<DD>_<bid|ask>.csv.zst`) without modification or re-download | Verified upstream by reading `tradedesk-dukascopy/tradedesk_dukascopy/export.py:390-398`. Path layout encapsulated in `miner-reader-dukascopy::path_layout`. See **CSV Schema (verified from source)** and **00-Indexed Month Encapsulation** below. |
| CACHE-02 | User can register additional data sources via a `Reader` trait | `Reader` trait lives in `miner-core::reader`; `DukascopyReader` in `miner-reader-dukascopy` is the first impl. See **Reader Trait Surface** below. |
| CACHE-03 | User can request bars at caller-specified resolutions (15m / 1h / 1d as the working set) | `Aggregator::aggregate(reader, params)` where `params.tf ∈ {Tf15m, Tf1h, Tf1d}`. 1m source is the input only. See **Aggregator Semantics**. |
| CACHE-04 | Deterministic, close-aligned, UTC bar aggregation that omits (never interpolates) gaps; bid/ask independent | Determinism gate is byte-identity on re-run. See **Determinism Landmines** and **DST Edge-Case Fixtures**. |
| CACHE-05 | Dukascopy's `volume` field is reported as `tick_count`; 00-indexed month is documented + boundary-tested | **Source CSV `volume` is a float sum of per-tick `bid_vol` / `ask_vol` values, NOT a tick count.** See **Assumption A1**. 00-indexed month boundary tests covered in **00-Indexed Month Encapsulation**. |
| CACHE-06 | Semi-permanent derived-bar cache keyed by `(source, symbol, side, timeframe)` with `aggregator_version` + per-day fingerprint invalidation | Two-axis invalidation: full-rebuild triggers (`aggregator_version`, `arrow_schema_version`) live in Arrow metadata; day-splice trigger lives in JSON sidecar. See **Arrow IPC Cache Schema + Invalidation**. |
| CACHE-07 | Structured gap manifest for `(symbol, side, range)` — missing daily files, zero-byte / corrupt, intra-day holes against trading calendar | `GapManifest` struct with three `GapReason` variants. See **Gap Manifest Data Model** and **Trading Calendar**. |
| CACHE-08 | Phase 2 owns *building and exposing* the gap manifest; Phase 3 owns enforcement | Confirmed per CONTEXT.md CACHE-08 boundary clarification. The `GapManifest` ships with `serde_derive` + `JsonSchema`; Phase 3 wraps it in `Finding::GapAborted`. |

</phase_requirements>

## Standard Stack

### Core (workspace deps to add in Phase 2)

| Library | Verified Version (May 2026) | Purpose | Why Standard |
|---------|------------------------------|---------|--------------|
| `arrow` | 58.3.0 [VERIFIED: lib.rs/crates/arrow, 2026-05-11] | Arrow IPC cache format; `RecordBatch`, `Schema`, `Field` | The Rust Arrow ecosystem. IPC `FileWriter` + `FileReader` give us a stable, language-portable, mmap-friendly columnar file. [CITED: docs.rs/arrow-ipc] |
| `csv` | 1.4.0 [VERIFIED: docs.rs/csv] | CSV reader for `.csv.zst` decompressed streams | The BurntSushi standard. `Reader::from_reader` over any `std::io::Read`. Serde deserialize derive works. [CITED: docs.rs/csv] |
| `csv-core` | 0.1.x | Byte-level zero-alloc CSV parser; hot-path fallback | Use only if profiling shows `csv` serde is the bottleneck. v1 default is `csv` derives. |
| `zstd` | 0.13.3 [VERIFIED: lib.rs/crates/zstd, 2025-02-20] | LZMA-style libzstd binding; streaming `Decoder<R: Read>` | Streaming decode plugs into `csv::ReaderBuilder::from_reader`. **DO NOT enable `zstdmt`** — parallelise across files (rayon), not within. [CITED: CLAUDE.md "CSV + zstd reader plumbing"] |
| `walkdir` | 2.5.0 [VERIFIED: docs.rs/walkdir] | Filesystem discovery for source cache | Standard. `max_depth(3)` reaches `<SYMBOL>/<YYYY>/<MM>/<DD>_<side>.csv.zst`. **MUST call `.sort_by_file_name()` for determinism** — default order is "unspecified." |
| `memmap2` | 0.9.10 [VERIFIED: lib.rs/crates/memmap2, 2026-02-15] | Read-only mmap for Arrow IPC cache files | Lock-free concurrent reads under rayon. Zero-copy access. [CITED: docs.rs/arrow-ipc — memmap2 is a dev-dep there already] |
| `globset` | 0.4+ | Symbol filter compilation | CLI `--symbols EURUSD GBPUSD` translates to a `GlobMatcher` over discovered files. |
| `tempfile` | 3.x | Test fixtures + atomic write-temp pattern | Standard. Already in workspace dev-deps from Phase 1. |

### Supporting (dev-deps for Phase 2)

| Library | Verified Version | Purpose | When to Use |
|---------|------------------|---------|-------------|
| `proptest` | 1.11.0 [VERIFIED: lib.rs/crates/proptest, 2026-03-24] | Property tests for aggregator invariants | Bar count, OHLC monotonicity, deterministic byte-identity under reordered input chunks |
| `insta` | 1.47.2 [VERIFIED: lib.rs/crates/insta, 2026-03-30] | Snapshot tests for gap manifest JSON shape + Arrow IPC schema bytes | Both consumer-visible contracts — pin them as snapshots so accidental drift fails CI |
| `criterion` | 0.5+ | Microbenchmarks (Phase 7 owns the harness; Phase 2 may write benches but doesn't run them in CI) | Aggregate-1m-to-15m-1symbol-1year, decompress-1day, csv-parse-1day |
| `assert_cmd` + `predicates` | latest | CLI integration tests if Phase 2 ships a `miner cache` subcommand | Optional — only if Plan adds a CLI verb |

### Alternatives Considered (and rejected for Phase 2)

| Instead of | Could Use | Why Standard Wins |
|------------|-----------|-------------------|
| Arrow IPC | Parquet | Parquet compression overhead per read taxes the write-once cache. Arrow IPC is faster to mmap and Polars/Pandas read it in one line. CONTEXT.md D2-01 locks. |
| Arrow IPC | bincode + zstd | Rust-only; blocks the future PLAT-v2-02 Python interop goal. Rejected in CONTEXT D2-01 review. |
| Arrow IPC | Custom mmap binary | ~10% faster, infinite tooling-interop loss. Rejected in CLAUDE.md "What NOT to Use" intent. |
| `csv` | `csv-core` directly | Use only if `serde` reflection is hot — profile first. Default to `csv` derives. |
| `zstd` (libzstd binding) | `ruzstd` (pure Rust) | Pure-Rust is for `no_std` / WASM; server-side binary benefits from the C binding. Rejected by CLAUDE.md "Alternatives Considered" table. |
| `walkdir` | `ignore` (BurntSushi) | `ignore` adds gitignore-style filtering we don't need; `walkdir` is the lighter primitive. |
| `chrono` | `jiff` | Phase 1 envelope already uses chrono; switching is a schema bump. CONTEXT D2-09 locks. |
| `HashMap` for sidecar fingerprints | `BTreeMap<NaiveDate, String>` | `HashMap` iteration order is non-deterministic → JSON file bytes drift → CI golden-file diffs flake. **Always `BTreeMap`** for any serialised map. |

**Installation (snippet for `Cargo.toml` workspace deps — planner assembles into the actual diff):**

```toml
[workspace.dependencies]
# Existing from Phase 1 — unchanged:
serde              = { version = "1", features = ["derive"] }
serde_json         = "1"   # NO features list — see Phase 1 determinism note
schemars           = { version = "1", features = ["chrono04"] }
chrono             = { version = "0.4", default-features = false, features = ["clock", "serde"] }
thiserror          = "1"
anyhow             = "1"
tracing            = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }
figment            = { version = "0.10", features = ["toml", "env"] }
clap               = { version = "4.5", features = ["derive", "env"] }
ulid               = { version = "1", features = ["serde"] }
jsonschema         = "0.46"
blake3             = "1"
directories        = "5"
base64             = "0.22"

# NEW in Phase 2 (verify versions at cargo-add time):
arrow              = "58"                  # cache format + IPC
csv                = "1.4"                 # Dukascopy CSV reader
csv-core           = "0.1"                 # optional hot-path fallback
zstd               = "0.13"                # zstd-csv decompression — NO zstdmt
walkdir            = "2.5"                 # source-cache discovery
memmap2            = "0.9"                 # mmap-backed Arrow cache reads
globset            = "0.4"                 # --symbols filter compilation
tempfile           = "3"                   # atomic write-temp + test fixtures

# Phase 2 dev-deps:
proptest           = "1.11"
insta              = "1.47"
```

**Version verification (executed during this research):**

```bash
slopcheck install --ecosystem crates.io arrow csv csv-core zstd walkdir tempfile globset
# → 7 OK (all packages pass legitimacy gate)
```

Crates not verified by slopcheck (added separately, all from authoritative CLAUDE.md stack table): `memmap2`, `proptest`, `insta`. All three are widely-used BurntSushi/community-canonical crates already enumerated in CLAUDE.md; tagged `[ASSUMED]` for slopcheck-coverage parity until a follow-up gate runs them.

## Package Legitimacy Audit

> Phase 2 introduces external packages. Audit run 2026-05-17.

| Package | Registry | Age | Downloads | Source Repo | slopcheck | Disposition |
|---------|----------|-----|-----------|-------------|-----------|-------------|
| `arrow` | crates.io | 5+ yrs | 4M+/mo | github.com/apache/arrow-rs | [OK] | Approved |
| `csv` | crates.io | 9+ yrs | 6M+/mo | github.com/BurntSushi/rust-csv | [OK] | Approved |
| `csv-core` | crates.io | 9+ yrs | 6M+/mo | github.com/BurntSushi/rust-csv | [OK] | Approved (sibling crate of `csv`) |
| `zstd` | crates.io | 8+ yrs | 5M+/mo | github.com/gyscos/zstd-rs | [OK] | Approved |
| `walkdir` | crates.io | 10+ yrs | 30M+/mo | github.com/BurntSushi/walkdir | [OK] | Approved |
| `tempfile` | crates.io | 9+ yrs | 100M+/mo | github.com/Stebalien/tempfile | [OK] | Approved |
| `globset` | crates.io | 9+ yrs | 30M+/mo | github.com/BurntSushi/ripgrep (sub-crate) | [OK] | Approved |
| `memmap2` | crates.io | 4+ yrs | 30M+/mo | github.com/RazrFalcon/memmap2-rs | not run | [ASSUMED — CLAUDE.md stack-table member; verify in plan-phase] |
| `proptest` | crates.io | 8+ yrs | 1M+/mo | github.com/proptest-rs/proptest | not run | [ASSUMED — CLAUDE.md stack-table member] |
| `insta` | crates.io | 7+ yrs | 1M+/mo | github.com/mitsuhiko/insta | not run | [ASSUMED — CLAUDE.md stack-table member] |

**Packages removed due to slopcheck [SLOP] verdict:** none.
**Packages flagged as suspicious [SUS]:** none.
**Action for planner:** Run `slopcheck install --ecosystem crates.io memmap2 proptest insta` during Plan Wave 0 alongside cargo-add. No `[SLOP]` outcome is plausible (every package is a well-established BurntSushi-or-community-canonical crate), but the gate should still run to maintain auditability.

## CSV Schema (verified from source)

Read directly from `tradedesk-dukascopy/tradedesk_dukascopy/export.py` lines 282–315 (the `_ticks_to_candles` function) and lines 436–446 (`_write_daily_candles`):

**File path layout (CACHE-01, CACHE-05):**

```
<cache_root>/<SYMBOL>/<YYYY>/<MM 0-indexed, 2 digits>/<DD 2 digits>_<bid|ask>.csv.zst
```

- `SYMBOL`: opaque string (e.g., `EURUSD`, `USA500IDXUSD`, `XAUUSD`).
- `YYYY`: 4-digit year.
- `MM`: **`day.month - 1` zero-padded to 2 digits.** Jan → `"00"`, Dec → `"11"`. Verified at `export.py:396 (f"{day.month - 1:02d}")`.
- `DD`: `day.day` zero-padded to 2 digits.
- `side`: literally `"bid"` or `"ask"`.
- Compression: zstd (level 3). One CSV inside, UTF-8.

**CSV content (verified at `export.py:14-16, 436-446`):**

```
timestamp,open,high,low,close,volume
2025-01-01 00:00:00+00:00,1.10342,1.10361,1.10311,1.10355,1234.0
```

- **Header row present** (the first line is literally `timestamp,open,high,low,close,volume`). Reader MUST consume the header, NOT skip without consuming, otherwise the first data row is lost.
- `timestamp`: UTC, **space-separated** date and time (NOT `T`-separated), with explicit `+00:00` offset. Format string `"%Y-%m-%d %H:%M:%S%:z"` per chrono strftime spec — see **Time Parsing** below.
- `open, high, low, close`: f64 floats, post `--price-divisor` scaling (the divisor was applied at export time; the values here are the final consumer-facing prices).
- `volume`: f64 float — **the SUM of per-tick `bid_vol` (for `_bid.csv.zst`) or `ask_vol` (for `_ask.csv.zst`) float32 values from that minute's .bi5 ticks.** Verified at `export.py:300-312` (`vol = pd.Series([t.bid_vol for t in ticks], ...)` then `vol.resample(resample_rule).sum().rename("volume")`).
- Frames omit minutes with no ticks (`dropna(subset=["open", "high", "low", "close"])` at `export.py:315`). So **the reader will receive a sparse 1m time series** with naturally-occurring gaps for low-liquidity minutes; the gap detector must distinguish intra-day-no-ticks from missing-source-file.
- Row order: timestamps strictly monotonic-ascending (pandas resample produces sorted output).
- No NaN, no infinity (pandas dropna filtered them; if a corrupt file ever surfaces one, the reader treats it as `CorruptFile`).

**Time parsing (CACHE-04 determinism):**

The space-separator means `DateTime::parse_from_rfc3339` will NOT parse this format (RFC3339 requires `T`). Use:

```rust
chrono::DateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%:z")
    .map(|dt| dt.with_timezone(&chrono::Utc))
```

Or — faster on the hot path — parse fields manually (split on space, then parse date / time / offset separately) using `chrono::NaiveDateTime::parse_from_str` with `"%Y-%m-%d %H:%M:%S"` and verifying the suffix is literally `"+00:00"` (it always is — the exporter forces UTC). The Phase 2 plan should default to the safe `parse_from_str` path and only swap in manual parsing if profiling shows it's hot.

Reference: chrono strftime spec confirms `%:z` matches `+00:00` (with colon), `%z` matches `+0000` (no colon). [CITED: docs.rs/chrono format/strftime]

## 00-Indexed Month Encapsulation

**The bug to prevent:** an executor agent writes `format!("{}/{}/{}", year, month, day)` somewhere with the calendar month (1..=12) instead of the Dukascopy zero-indexed month (0..=11), and the reader looks in the wrong directory. This bug is silent — the file just doesn't exist, the reader thinks the day is missing, the gap detector emits a MissingFile gap, the aggregator emits no bars for that day, and the user notices only when they spot-check against the source.

**Encapsulation pattern (CONTEXT-aligned):**

```rust
// In crates/miner-reader-dukascopy/src/path_layout.rs

/// Dukascopy-style zero-indexed month (Jan = 0, Dec = 11).
///
/// Distinct from `chrono::Month` (which is 1-indexed). This type exists so the rest
/// of the reader codebase CAN NEVER accidentally format a calendar month as the
/// directory component — the only way to obtain a `DukascopyMonth` is via
/// `from_calendar` or `from_chrono_month`, both of which subtract 1 explicitly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DukascopyMonth(u8);  // private inner; range 0..=11

impl DukascopyMonth {
    /// Convert from calendar 1..=12 (panics outside that range — caller bug).
    pub fn from_calendar(month: u8) -> Self {
        assert!((1..=12).contains(&month), "calendar month out of range: {month}");
        Self(month - 1)
    }

    /// Convert from `chrono::NaiveDate::month()` (1..=12).
    pub fn from_chrono_date(d: chrono::NaiveDate) -> Self {
        Self::from_calendar(d.month() as u8)
    }

    /// Zero-padded two-digit directory component.
    pub fn dir_name(self) -> String {
        format!("{:02}", self.0)
    }
}

/// Compose a daily-file path. The ONLY way to construct a Dukascopy daily-file path
/// in the codebase — `path_layout::day_csv_zst` is sealed against `pub` typos.
pub fn day_csv_zst(
    cache_root: &Path,
    symbol: &str,
    date: chrono::NaiveDate,
    side: Side,
) -> PathBuf {
    cache_root
        .join(symbol)
        .join(format!("{}", date.year()))
        .join(DukascopyMonth::from_chrono_date(date).dir_name())
        .join(format!("{:02}_{}.csv.zst", date.day(), side.as_str()))
}
```

**Boundary tests (mandatory — CACHE-05 explicitly calls this out):**

```rust
#[test]
fn jan_maps_to_00() {
    let m = DukascopyMonth::from_calendar(1);
    assert_eq!(m.dir_name(), "00");
}

#[test]
fn dec_maps_to_11() {
    let m = DukascopyMonth::from_calendar(12);
    assert_eq!(m.dir_name(), "11");
}

#[test]
#[should_panic]
fn zero_calendar_month_panics() {
    let _ = DukascopyMonth::from_calendar(0);
}

#[test]
#[should_panic]
fn thirteen_calendar_month_panics() {
    let _ = DukascopyMonth::from_calendar(13);
}

#[test]
fn round_trip_via_chrono_date_for_every_month_of_2024() {
    use chrono::NaiveDate;
    let expected = [
        (1, "00"), (2, "01"), (3, "02"), (4, "03"), (5, "04"), (6, "05"),
        (7, "06"), (8, "07"), (9, "08"), (10, "09"), (11, "10"), (12, "11"),
    ];
    for (cal, expected_dir) in expected {
        let d = NaiveDate::from_ymd_opt(2024, cal as u32, 1).unwrap();
        assert_eq!(DukascopyMonth::from_chrono_date(d).dir_name(), expected_dir);
    }
}

#[test]
fn full_path_round_trip() {
    use chrono::NaiveDate;
    let d = NaiveDate::from_ymd_opt(2024, 12, 25).unwrap();  // Dec 25 → MM "11"
    let p = day_csv_zst(Path::new("/c"), "EURUSD", d, Side::Bid);
    assert_eq!(p.to_str().unwrap(), "/c/EURUSD/2024/11/25_bid.csv.zst");
}
```

**Reverse mapping** (path → date, for fingerprint enumeration) MUST also live in `path_layout.rs` and round-trip with `day_csv_zst`. A property test (proptest, every date 2010-01-01..2030-12-31) confirms the round trip:

```rust
proptest! {
    #[test]
    fn path_round_trip(year in 2010..=2030u32, m_cal in 1..=12u32, day in 1..=28u32) {
        let d = chrono::NaiveDate::from_ymd_opt(year as i32, m_cal, day).unwrap();
        let p = day_csv_zst(Path::new("/c"), "EURUSD", d, Side::Bid);
        let parsed = parse_day_path(&p).unwrap();
        prop_assert_eq!(parsed.date, d);
        prop_assert_eq!(parsed.symbol, "EURUSD");
        prop_assert_eq!(parsed.side, Side::Bid);
    }
}
```

## Reader Trait Surface

Confirmed CONTEXT D2-12 with two refinements:

```rust
// In crates/miner-core/src/reader/mod.rs

use chrono::{DateTime, Utc, NaiveDate};
use crate::calendar::Calendar;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Side { Bid, Ask }

impl Side {
    pub fn as_str(self) -> &'static str { match self { Self::Bid => "bid", Self::Ask => "ask" } }
}

/// Half-open UTC range [start, end). Matches Phase 1 `TimeRange` semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClosedRangeUtc { pub start: DateTime<Utc>, pub end: DateTime<Utc> }

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RawBar {
    pub ts_open_utc: DateTime<Utc>,
    pub ts_close_utc: DateTime<Utc>,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub tick_count: u32,   // FIXME — see Assumption A1 below
}

/// 32-byte blake3 hex digest as a fixed-size newtype. Cheaper than `String`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct Blake3Hex(pub [u8; 64]);  // ASCII hex chars

pub trait Reader: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;

    /// Stable identifier for the data source. Carried in `BarFrame.source_id` and
    /// fed into cache keys. e.g., `"dukascopy"`.
    fn source_id(&self) -> &str;

    /// Calendar this source operates against. Phase 2 default impl returns the
    /// FX-major calendar; overrideable by future readers (equities, crypto).
    fn trading_calendar(&self) -> Calendar;

    /// Stream 1-minute bars for `(symbol, side)` in the half-open UTC range `[start, end)`.
    /// Bars arrive in ascending `ts_open_utc` order; missing minutes are NOT emitted
    /// (the caller compares against the calendar to detect intra-day holes).
    fn read_1m_bars<'a>(
        &'a self,
        symbol: &str,
        side: Side,
        range: ClosedRangeUtc,
    ) -> Result<
        Box<dyn Iterator<Item = Result<RawBar, Self::Error>> + Send + 'a>,
        Self::Error,
    >;

    /// Compute the per-day source fingerprint. `None` MUST mean "no source file
    /// exists for this day" (which the GapDetector treats as a MissingFile gap and
    /// the cache treats as "drop any cached rows for this day"). `Some(hex)` means
    /// the file exists and this is its blake3 digest.
    fn fingerprint_day(
        &self,
        symbol: &str,
        side: Side,
        date: NaiveDate,
    ) -> Result<Option<Blake3Hex>, Self::Error>;

    /// Enumerate the dates for which a source file exists in `[start, end)` for
    /// `(symbol, side)`. Used by the cache invalidator to walk the diff and by the
    /// gap detector to find missing days. MUST return dates in ascending order.
    fn enumerate_days(
        &self,
        symbol: &str,
        side: Side,
        range: ClosedRangeUtc,
    ) -> Result<Vec<NaiveDate>, Self::Error>;
}
```

**Refinement #1 vs CONTEXT D2-12:** Added `enumerate_days` — without it, the cache invalidator and gap detector have no way to walk "what days exist on disk" except by guessing every date in the range and calling `fingerprint_day` (which is wasteful — every miss is a stat() syscall). `enumerate_days` is what `walkdir` naturally produces; explicit method keeps the cost visible.

**Refinement #2 vs CONTEXT D2-12:** Used a fixed-size `[u8; 64]` for `Blake3Hex` (ASCII hex) instead of a `String`. Saves an allocation per fingerprint call; on a 6-year history with 28 instruments × 2 sides that's 28 × 2 × 365 × 6 ≈ 122,000 allocations avoided.

**The `Box<dyn Iterator>` vs `impl Iterator` question:** Returning an opaque `impl Iterator` from a trait method requires GATs / `async fn in trait` style features which complicate `Send + Sync` bounds across `dyn Reader` callers. `Box<dyn Iterator>` has one heap-alloc per range read but reads happen at most once per `(symbol, side, day)` per scan — the overhead is invisible. Stay with `Box<dyn Iterator>`.

**Why NOT a chunked `read_range -> Result<RecordBatch>` method?** The aggregator wants a per-minute pull-iterator so it can flush each output bar as soon as the next-bucket's first 1m arrives. Materialising a year of 1m bars into a single `RecordBatch` (525,600 rows × 5 f64 cols + 1 u32 col ≈ 24 MB) is feasible but wasteful — we'd then immediately stream it into the aggregator and discard it. Iterator-of-`RawBar` keeps the working set to one minute at a time. Adding a chunked method later is additive.

## Aggregator Semantics

Confirmed CONTEXT D2-14, D2-15, D2-18, D2-19 with the following concretisation:

### Bar boundary rule (D2-19, CACHE-04)

Bars are **close-aligned to UTC midnight**:

- **15m bars** open at `:00 :15 :30 :45` of each hour. A bar at `ts_open_utc = 12:15:00Z` covers raw 1m bars whose `ts_open_utc ∈ [12:15:00Z, 12:30:00Z)` — exactly 15 raw bars when the range is fully populated.
- **1h bars** open at `HH:00:00Z`. Covers raw 1m bars in `[HH:00:00Z, (HH+1):00:00Z)`.
- **1d bars** open at `00:00:00Z` of each day. Covers raw 1m bars in `[date, date + 24h)`.

**Bar timestamp convention:** the bar's `ts_open_utc` is the bucket's open instant; `ts_close_utc = ts_open_utc + tf` is the bucket's close (the next bucket's open). The published `produced_at_utc` in any downstream finding refers to wallclock, NOT a bar timestamp. Quant convention is ambiguous here — some use bar-open, some use bar-close — but Phase 1's envelope schema already carries `ts_open_utc` and `ts_close_utc` explicitly, so consumers can pick. **Sort and dedupe by `ts_open_utc`** for output stability (OUT-03 byte-identity).

### OHLC reduction (CACHE-04)

For each output bucket whose constituent 1m raw bars are `r[0..n]` (n ≥ 1, sorted ascending):

- `open` = `r[0].open`
- `high` = `max(r[i].high for i in 0..n)`
- `low` = `min(r[i].low for i in 0..n)`
- `close` = `r[n-1].close`
- `tick_count` = `sum(r[i].tick_count for i in 0..n)` (or `tick_volume` per A1)

The reduction is associative for high/low/sum but `open`/`close` need explicit first/last selection. Implementation must be **single-pass, ordered** — no `rayon::par_iter` inside the reduction itself, because parallel sums of f64 are non-deterministic (see Determinism Landmines).

### Gap omission rule (CACHE-04)

If the output bucket `[T, T + tf)` overlaps the trading-calendar OPEN window AND has zero constituent 1m bars → **the bar is omitted from the output** AND a corresponding entry appears in the gap manifest (`IntraDayGap` with `start_utc = T`, `end_utc = T + tf`).

If the output bucket overlaps the trading-calendar CLOSED window (entire weekend, holiday) → no bar emitted, no gap entry. Closed time is not a gap.

If the output bucket is partially in open hours and partially in closed (e.g., the bucket containing the Friday 22:00Z close) → a bar IS emitted, covering whichever 1m bars exist in the open portion. CONTEXT D2-19 says "Partial bars [...] emitted as-is with their actual open/close timestamps; never padded, never dropped, but flagged via the gap manifest if the bucket is internally incomplete (more than 50% of the 1m sub-buckets missing during open hours)." Threshold to enforce: `(open_minutes_in_bucket_with_no_1m) / (total_open_minutes_in_bucket) > 0.5` → emit the bar AND emit an `IntraDayGap` entry covering the missing sub-range. This is a single rule; document it clearly.

### Determinism contract (CACHE-04 byte-identity)

**Determinism = byte-identical Arrow IPC output across re-runs on the same source bytes.** Five concrete safeguards:

1. **No `HashMap` anywhere in the aggregator or its inputs/outputs.** `BTreeMap` only. Hash iteration order is per-process-random in Rust (DoS-resistant by default).
2. **No `rayon::par_iter` inside the per-symbol reduction.** Each `(symbol, side, tf)` aggregation is single-threaded — its reduction is sequential. Parallelism is at the `(symbol, side, tf)` task level (one rayon job per quartet), which CONTEXT calls out for Phase 3.
3. **No `Instant::now()`, `SystemTime::now()`, `chrono::Utc::now()` inside the aggregator.** Wallclock reads are only allowed in the `run_start` / `run_end` framing records, which are NOT part of the bar data.
4. **f64 sum ordering matters.** `sum(r[i].tick_count for i in 0..n)` is integer addition and is associative; safe. But if A1 lands as `tick_volume: f64`, the sum is f64 floating-point addition, which is NOT associative under reordering. Mitigation: keep the input iterator sequential and process in `ts_open_utc` order — this guarantees the same sum byte-for-byte across runs.
5. **Arrow IPC file determinism.** The Arrow `Schema` is constructed from a fixed `Vec<Field>` (not from a `HashMap`); the `RecordBatch` rows preserve insertion order; `FileWriter::write_metadata` is called with a deterministic `BTreeMap`-like ordering of metadata keys. The IPC footer contains version info — pin the `arrow` workspace dep to a major version (e.g., `arrow = "58"`) so footer bytes don't shift on minor-version upgrades.

### Aggregator kernel signature

```rust
// In crates/miner-core/src/aggregator/mod.rs

pub const AGGREGATOR_VERSION: &str = "1.0.0";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Timeframe { Tf15m, Tf1h, Tf1d }

impl Timeframe {
    pub fn duration(self) -> chrono::Duration {
        match self {
            Self::Tf15m => chrono::Duration::minutes(15),
            Self::Tf1h => chrono::Duration::hours(1),
            Self::Tf1d => chrono::Duration::days(1),
        }
    }
    pub fn as_str(self) -> &'static str {
        match self { Self::Tf15m => "15m", Self::Tf1h => "1h", Self::Tf1d => "1d" }
    }
}

pub struct AggParams<'a> {
    pub symbol: &'a str,
    pub side: Side,
    pub tf: Timeframe,
    pub range: ClosedRangeUtc,
}

pub fn aggregate<R: Reader>(
    reader: &R,
    params: AggParams,
) -> Result<BarFrame, AggregateError<R::Error>> {
    // Pull 1m bars from the reader; bucket into params.tf aligned to UTC midnight;
    // emit a BarFrame column-oriented.
}
```

Implementation detail: the aggregator processes 1m bars in order, tracking the current bucket. When a 1m bar's `ts_open_utc` advances past the current bucket's `close`, emit the current bucket's accumulated OHLCV and start a new bucket. No buffering of full days in memory — peak memory is one bucket of 1m bars (15 / 60 / 1440 elements).

## Arrow IPC Cache Schema + Invalidation

### Path layout (D2-20)

```
<bar_cache_root>/
  dukascopy/                              # source_id
    EURUSD/                               # symbol
      15m_bid.arrow                       # the data
      15m_bid.fingerprints.json           # sidecar
      15m_ask.arrow
      15m_ask.fingerprints.json
      1h_bid.arrow
      1h_bid.fingerprints.json
      ... (etc., 6 files per symbol)
    GBPUSD/
      ...
```

For 28 instruments × 3 timeframes × 2 sides × 2 files = **336 files** in the cache. Manageable; no nested partitioning needed at v1.

### Arrow schema

```rust
use arrow::datatypes::{DataType, Field, Schema, TimeUnit};
use arrow::array::{Float64Array, TimestampNanosecondArray, UInt32Array};

fn build_schema(source_id: &str, symbol: &str, side: Side, tf: Timeframe) -> Schema {
    let fields = vec![
        Field::new("ts_open_utc",  DataType::Timestamp(TimeUnit::Nanosecond, Some("UTC".into())), false),
        Field::new("ts_close_utc", DataType::Timestamp(TimeUnit::Nanosecond, Some("UTC".into())), false),
        Field::new("open",         DataType::Float64, false),
        Field::new("high",         DataType::Float64, false),
        Field::new("low",          DataType::Float64, false),
        Field::new("close",        DataType::Float64, false),
        Field::new("tick_count",   DataType::UInt32, false),   // or Float64 per A1
    ];

    // Metadata keys are alphabetic (BTreeMap insertion-order) for byte determinism.
    let mut metadata = std::collections::HashMap::new();
    // (arrow::datatypes::Schema accepts HashMap, but we MUST construct it from
    // a BTreeMap-collect to guarantee insertion order — see Arrow IPC docs note
    // on schema-metadata serialisation: ordering is preserved in the IPC bytes.)
    let entries: std::collections::BTreeMap<String, String> = [
        ("aggregator_version", AGGREGATOR_VERSION),
        ("arrow_schema_version", ARROW_SCHEMA_VERSION),
        ("source_id", source_id),
        ("symbol", symbol),
        ("side", side.as_str()),
        ("timeframe", tf.as_str()),
        ("code_revision", miner_core::CODE_REVISION),  // Phase 1 const
    ].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    for (k, v) in entries { metadata.insert(k, v); }

    Schema::new_with_metadata(fields, metadata)
}

pub const ARROW_SCHEMA_VERSION: &str = "1.0.0";  // bump on any field shape change
```

### Sidecar JSON (D2-03)

```rust
// <timeframe>_<side>.fingerprints.json
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FingerprintSidecar {
    pub aggregator_version: String,
    pub arrow_schema_version: String,
    pub source_id: String,
    pub symbol: String,
    pub side: Side,
    pub timeframe: Timeframe,
    /// `YYYY-MM-DD` → 64-char blake3 hex of the source `.csv.zst` bytes for that day.
    /// **MUST be a `BTreeMap`** for deterministic JSON output (OUT-03).
    pub per_day_fingerprint: std::collections::BTreeMap<chrono::NaiveDate, String>,
}
```

The aggregator version and schema version are duplicated in BOTH the Arrow schema metadata AND the sidecar JSON. Reason: the sidecar can be read without parsing the Arrow file (cheap), and the Arrow file is self-describing without the sidecar (robust). If they disagree (e.g., manually-edited sidecar), the Arrow file metadata is the source of truth and the sidecar is regenerated.

### Per-day fingerprint computation

```rust
// In crates/miner-reader-dukascopy/src/lib.rs (impl Reader for DukascopyReader)
fn fingerprint_day(&self, symbol: &str, side: Side, date: NaiveDate) -> Result<Option<Blake3Hex>, ...> {
    let path = path_layout::day_csv_zst(&self.cache_root, symbol, date, side);
    match std::fs::read(&path) {
        Ok(bytes) => {
            let mut hasher = blake3::Hasher::new();
            hasher.update(&bytes);
            let hex = hasher.finalize().to_hex();          // 64-char lowercase
            Ok(Some(Blake3Hex::from_hex_bytes(hex.as_bytes())))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}
```

**Why blake3 of the full bytes and NOT (size, mtime)?** mtime can lie:
- After a `git clone` / `cp -p`, mtime is reset.
- After a `touch` (manual or accidental) with unchanged content, mtime advances.
- Across filesystems (NFS, SMB, network shares), mtime precision varies and is unreliable.
- A future Dukascopy refresh that produces an identical day-CSV (rare but possible) should NOT trigger re-aggregation.

blake3 of the full `.csv.zst` bytes is the only correct answer. blake3 throughput is ~5 GB/s on a modern CPU — for a typical day file of 50-500 KB, the hash cost is sub-millisecond per day. 28 instruments × 6 years × 365 days × 2 sides ≈ 122,000 daily fingerprint calls — at <1ms each, that's ~2 minutes wall-clock for a full cold rebuild. Parallelise via `rayon` for the fingerprint walk and that drops to ~10 seconds on an 8-core box.

**Optional optimisation:** add a one-line `mtime: u64` to the sidecar entry as a fast-path check. If `mtime` matches AND the file size matches, skip the blake3 entirely; if either differs, recompute. This is a 100x speedup on warm caches. **Defer to a plan-phase decision** — adds complexity, breaks `single source of truth` purity. v1 ships full-blake3.

### Invalidation flow (D2-03)

```rust
// In crates/miner-core/src/cache/mod.rs
pub fn aggregate_with_cache<R: Reader>(
    reader: &R,
    cache_root: &Path,
    params: AggParams,
) -> Result<BarFrame, CacheError> {
    // 1. Compute the Arrow file path and sidecar path.
    let arrow_path = cache_root
        .join(reader.source_id())
        .join(params.symbol)
        .join(format!("{}_{}.arrow", params.tf.as_str(), params.side.as_str()));
    let sidecar_path = arrow_path.with_extension("fingerprints.json");

    // 2. Read existing sidecar (if any). If aggregator_version OR arrow_schema_version
    //    has bumped → FULL REBUILD.
    let sidecar = read_sidecar(&sidecar_path)?;  // None if absent
    let full_rebuild = sidecar
        .as_ref()
        .map(|s| s.aggregator_version != AGGREGATOR_VERSION
              || s.arrow_schema_version != ARROW_SCHEMA_VERSION)
        .unwrap_or(true);

    // 3. Enumerate source days in range and compute current fingerprints (rayon-parallel).
    let current_days = reader.enumerate_days(params.symbol, params.side, params.range)?;
    let current_fingerprints: BTreeMap<NaiveDate, Blake3Hex> = current_days
        .par_iter()
        .filter_map(|date| {
            reader.fingerprint_day(params.symbol, params.side, *date)
                .ok()?
                .map(|fp| (*date, fp))
        })
        .collect();  // rayon ParallelIterator -> sequential BTreeMap

    // 4. Diff against sidecar; identify days needing regen.
    let stale_days = if full_rebuild {
        current_days.clone()
    } else {
        diff_days(&sidecar.unwrap().per_day_fingerprint, &current_fingerprints)
    };

    // 5. Read existing Arrow file (if not full_rebuild and file exists);
    //    drop rows whose ts_open_utc falls on any date in stale_days.
    // 6. For each stale_day, run the aggregator on JUST that day's 1m bars.
    // 7. Concatenate surviving + new rows (sorted by ts_open_utc), write to tempfile,
    //    fsync, atomic rename. (See Crash Safety below.)
    // 8. Persist updated sidecar.
}
```

### Crash safety on partial writes

```rust
fn write_arrow_atomic(target: &Path, schema: &Schema, batches: &[RecordBatch]) -> Result<()> {
    let parent = target.parent().expect("absolute path");
    std::fs::create_dir_all(parent)?;
    let tmp = tempfile::NamedTempFile::new_in(parent)?;
    {
        let mut writer = arrow::ipc::writer::FileWriter::try_new(tmp.as_file(), schema)?;
        for batch in batches { writer.write(batch)?; }
        writer.finish()?;
    }
    tmp.as_file().sync_all()?;        // fsync the data
    tmp.persist(target)?;             // atomic rename
    // Optional: fsync the parent directory to ensure the rename is durable.
    Ok(())
}
```

`tempfile::NamedTempFile::persist` uses `std::fs::rename` under the hood, which is atomic on POSIX and on Windows ≥ NTFS. The `sync_all` before persist ensures the contents reach disk before the rename publishes the new name. If the process crashes mid-write, the tempfile is left orphaned (cleanup on next run) and the existing target is unchanged.

The sidecar JSON gets the same write-temp → rename treatment. Critically: **write the Arrow file FIRST, then the sidecar.** If the process crashes after Arrow but before sidecar, the next run sees a fingerprint mismatch (sidecar is from the previous run) → rebuild → no data loss. If we wrote sidecar first and crashed before Arrow, the next run would think the cache is current → serve stale or missing data from the old Arrow file → silent corruption.

### Memory-mapped reads (D2-01 + memmap2)

```rust
fn read_cache(target: &Path) -> Result<BarFrame, CacheError> {
    let file = std::fs::File::open(target)?;
    let mmap = unsafe { memmap2::Mmap::map(&file)? };  // unsafe because external mutators can violate aliasing
    let reader = arrow::ipc::reader::FileReader::try_new(std::io::Cursor::new(&mmap[..]), None)?;
    // Or: pass the mmap directly to arrow::buffer if zero-copy matters; for v1, Cursor is fine.
    let batches: Vec<RecordBatch> = reader.collect::<Result<Vec<_>, _>>()?;
    BarFrame::from_arrow_batches(batches)
}
```

**The `unsafe` is unavoidable** with `memmap2::Mmap::map` — the workspace lint `unsafe_code = "forbid"` (workspace `Cargo.toml`) WILL fail this. Mitigation: add a per-function `#[allow(unsafe_code)]` with a doc comment explaining the invariant (the cache file is only ever opened by miner; no concurrent mutators), OR downgrade the lint to `deny` and add a single `unsafe { ... }` block with `// SAFETY: ...` justification.

Actually — Phase 1 lints set `unsafe_code = "forbid"` at the workspace level (`Cargo.toml:42`). `forbid` cannot be overridden; only `deny` can. **The Phase 2 plan MUST downgrade `unsafe_code` from `"forbid"` to `"deny"` at the workspace level OR use a non-mmap reader path.** The lint downgrade is a workspace-wide policy change; recommend doing it once at Phase 2's first cache plan and adding the single `// SAFETY:` justification in `cache/mmap.rs`. Alternative: skip mmap, use buffered file reads — arrow's `FileReader` over a `BufReader<File>` works fine, just less zero-copy.

**Recommendation:** Start with `BufReader<File>` (no unsafe, no lint change). If profiling shows cache reads are hot enough to matter, swap to mmap in a follow-up plan with the lint change documented as part of that work. Phase 2 ships safe reads.

### Concurrency model

- **Read path:** Multiple rayon workers can open the same Arrow IPC file concurrently — file handles are independent, mmap (if used) is read-only and aliased safely. No locking needed.
- **Write path:** A single writer per `(source_id, symbol, side, timeframe)` quartet. The atomic-rename pattern serialises writes naturally — concurrent writers either both succeed (last-rename wins) or one fails the rename. For Phase 2, ASSUME single-process operation; the CLI/MCP/HTTP wrappers in later phases will need a process-level mutex or file-lock if they ever invoke the cache concurrently. Document the invariant in `BarCache` docs.
- **Per-day fingerprint computation:** rayon-parallel via `par_iter()` over the day list. blake3 hashing is CPU-bound and embarrassingly parallel.

## Gap Manifest Data Model

Confirmed CONTEXT D2-16, D2-17 with concrete JSON shape:

```rust
// In crates/miner-core/src/gap/mod.rs

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GapManifest {
    pub source_id: String,
    pub symbol: String,
    pub side: Side,
    pub queried_range: TimeRange,                    // reuses Phase 1 type
    /// Sorted by `start_utc` ascending. Empty if no gaps.
    pub gaps: Vec<GapSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GapSpan {
    pub start_utc: chrono::DateTime<chrono::Utc>,
    pub end_utc: chrono::DateTime<chrono::Utc>,
    pub reason: GapReason,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GapReason {
    MissingSourceFile { date: chrono::NaiveDate },
    /// File present but zero-byte or could not be decompressed/parsed.
    CorruptSourceFile { date: chrono::NaiveDate, detail: String },
    /// File present and valid, but the open-hours minutes have missing bars.
    IntraDayGap { affected_minutes: u32 },
}
```

**JSON example consumers will see** (sorted, deduplicated, merged across reasons):

```json
{
  "source_id": "dukascopy",
  "symbol": "EURUSD",
  "side": "bid",
  "queried_range": {
    "start_utc": "2024-03-08T00:00:00Z",
    "end_utc": "2024-03-12T00:00:00Z"
  },
  "gaps": [
    { "start_utc": "2024-03-08T22:00:00Z",
      "end_utc": "2024-03-10T22:00:00Z",
      "reason": { "kind": "weekend_closure" } },
    { "start_utc": "2024-03-11T13:45:00Z",
      "end_utc": "2024-03-11T13:46:00Z",
      "reason": { "kind": "intra_day_gap", "affected_minutes": 1 } }
  ]
}
```

Wait — `weekend_closure` is NOT a gap (per D2-07: "Anything during the closed window is NOT a gap"). The example above incorrectly includes it. The correct manifest contains ONLY entries during open hours:

```json
{
  "source_id": "dukascopy",
  "symbol": "EURUSD",
  "side": "bid",
  "queried_range": {
    "start_utc": "2024-03-08T00:00:00Z",
    "end_utc": "2024-03-12T00:00:00Z"
  },
  "gaps": [
    { "start_utc": "2024-03-11T13:45:00Z",
      "end_utc": "2024-03-11T13:46:00Z",
      "reason": { "kind": "intra_day_gap", "affected_minutes": 1 } }
  ]
}
```

**Ordering and dedup rules:**
- `gaps` sorted ascending by `start_utc`, ties broken by `end_utc` then `reason` discriminant.
- Adjacent IntraDayGap entries within the same calendar day MAY be merged (`[12:00, 12:01) ∪ [12:01, 12:02)` → `[12:00, 12:02)`) — but only if the executor agent can prove merging doesn't change downstream consumer behaviour. **Recommendation for v1: do NOT merge adjacent intra-day gaps.** Keep them as individual one-minute entries. Phase 3's gap-policy enforcement can choose to merge or not. Simpler is correct here.
- A day with a MissingSourceFile gap MUST NOT also emit IntraDayGap entries for that same day — they would be redundant (the whole day is missing). The detector skips intra-day analysis when the day file is absent.

**Schema-pin via insta snapshot** — pin the gap-manifest JSON shape with an `insta::assert_json_snapshot!` test on a known-input fixture. This prevents accidental schema drift from a Phase 4+ refactor.

## Trading Calendar

Confirmed CONTEXT D2-06, D2-07, D2-08. Implementation:

```rust
// In crates/miner-core/src/calendar.rs

use chrono::{DateTime, Datelike, NaiveTime, Timelike, Utc, Weekday};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Calendar {
    pub weekly_open_utc: (Weekday, NaiveTime),
    pub weekly_close_utc: (Weekday, NaiveTime),
    /// (month 1..=12, day 1..=31). Applied every year.
    pub yearly_holidays: Vec<(u32, u32)>,
}

impl Calendar {
    pub fn fx_major() -> Self {
        Self {
            weekly_open_utc: (Weekday::Sun, NaiveTime::from_hms_opt(22, 0, 0).unwrap()),
            weekly_close_utc: (Weekday::Fri, NaiveTime::from_hms_opt(22, 0, 0).unwrap()),
            yearly_holidays: vec![(12, 25), (1, 1)],  // Christmas, New Year's
        }
    }

    /// Closed-form predicate. O(1), no allocation, inlineable.
    #[inline]
    pub fn is_open_at(&self, ts: DateTime<Utc>) -> bool {
        // 1. Check yearly holidays first — cheap (max 2 entries in v1).
        for (m, d) in &self.yearly_holidays {
            if ts.month() == *m && ts.day() == *d {
                return false;
            }
        }
        // 2. Check weekly schedule: open at Sun 22:00 UTC, close at Fri 22:00 UTC.
        //    "Open" means: now is past Sun 22:00 of the week's start AND before Fri 22:00.
        //    Express as: (Mon, Tue, Wed, Thu) open; Fri open until 22:00; Sat closed; Sun open from 22:00.
        let wd = ts.weekday();
        let t = ts.time();
        let close_t = self.weekly_close_utc.1;
        let open_t = self.weekly_open_utc.1;
        match wd {
            Weekday::Mon | Weekday::Tue | Weekday::Wed | Weekday::Thu => true,
            Weekday::Fri => t < close_t,
            Weekday::Sat => false,
            Weekday::Sun => t >= open_t,
        }
    }
}
```

**Performance budget (CONTEXT open question #4):** The gap detector calls `is_open_at` once per minute over the queried range. For a 6-year EURUSD scan: 60 × 24 × 365 × 6 ≈ 3.16 M calls. At 10 ns/call (a generous estimate for the closed-form predicate), that's 32 ms — well below the budget. The implementation is allocation-free and inlineable. Verified by benchmark in Phase 7; for Phase 2, write a microbenchmark in `benches/calendar.rs` that asserts < 100 ns/call.

**Reader trait override hook:** `Reader::trading_calendar() -> Calendar` returns `Calendar::fx_major()` for `DukascopyReader`. Future readers (e.g., an equity reader for indices) override to return a per-symbol calendar dispatched on `symbol`. CONTEXT D2-06 defers per-symbol overrides to Phase 3+.

## Runtime State Inventory

> Phase 2 is greenfield in the sense that the cache it manages does not yet exist on any user's machine. But it READS the existing Dukascopy cache without modification.

| Category | Items Found | Action Required |
|----------|-------------|------------------|
| Stored data | None to migrate — Phase 2 is the FIRST writer of the `bar_cache_root`. The Dukascopy cache (`cache_root`) is read-only consumer state owned by the sibling project. | None |
| Live service config | None — Phase 2 has no external services. | None |
| OS-registered state | None — no scheduled tasks, no daemons, no systemd units. | None |
| Secrets/env vars | None — Phase 2 reads `MINER_CACHE_ROOT`, `MINER_BAR_CACHE_ROOT`, `MINER_OUTPUT` via figment (Phase 1's existing schema). No new env vars introduced. | None |
| Build artifacts | None — Phase 2 adds source files but does not change the build graph in ways that orphan existing artifacts. `cargo build --workspace` will rebuild as needed. | None |

**Nothing found in any category:** Phase 2 is greenfield for the bar cache; it does not modify any pre-existing runtime state.

**The one runtime-state risk:** if a user has populated their `bar_cache_root` during dev with an early build of Phase 2 and then bumps `AGGREGATOR_VERSION` or `ARROW_SCHEMA_VERSION`, the existing cache files become invalid and trigger a full rebuild. This is correct behaviour (D2-04). The plan should ensure the cache-version check at read time produces a clear `tracing::info!` log line (`cache invalidated: aggregator_version 1.0.0 → 1.0.1, rebuilding EURUSD/15m_bid`) so the rebuild is observable.

## Common Pitfalls

### Pitfall 1: Non-deterministic file enumeration

**What goes wrong:** `walkdir::WalkDir::new(root)` without `.sort_by_file_name()` produces entries in OS-dependent order (inode order on ext4, allocation order on tmpfs, NTFS-dependent on Windows). Two runs on the same filesystem can produce different sidecar JSON files (different key ordering when collected into a `HashMap`) or different Arrow IPC files (different row interleaving). CI's byte-identity check fails intermittently.

**Why it happens:** Executor agents reach for `walkdir::WalkDir::new(path).into_iter()` and don't realise the default order is unspecified. The bug is invisible on a single dev machine because the filesystem usually returns entries in the same order across runs — until it doesn't.

**How to avoid:** Every `WalkDir` invocation in Phase 2 code MUST chain `.sort_by_file_name()`. Add a `clippy::disallowed_methods` rule in `clippy.toml` banning `WalkDir::new().into_iter()` without the sort. Alternatively, add a `find_source_files` helper that wraps WalkDir with the sort hardcoded.

**Warning signs:** A property test re-running aggregation produces different output bytes; a `git diff` on the cache files between runs shows row-order changes; CI flakes only on certain GHA runners.

### Pitfall 2: Hash-iteration determinism

**What goes wrong:** Anywhere the code does `HashMap<NaiveDate, Blake3Hex>` or `HashMap<String, GapReason>` and then serialises it to JSON, the field order in the output JSON varies per process (Rust's default hasher is randomly seeded for DoS resistance). The sidecar JSON file bytes differ across runs.

**Why it happens:** Default-derived `serde::Serialize` on a `HashMap` emits keys in iteration order, which is unspecified.

**How to avoid:** **Every `HashMap` field on a serialised type MUST be `BTreeMap`.** Phase 1's lint culture already enforces this for the envelope — extend it: when Phase 2 reviewers see a `HashMap` in any `derive(Serialize)`-bearing type, that's a CI-blocker. The plan-checker should grep for `HashMap` in all Phase 2 modules.

**Warning signs:** Sidecar JSON files in the cache directory differ between two consecutive runs of `miner cache build`; an insta snapshot of the manifest passes locally but flakes in CI.

### Pitfall 3: Floating-point sum non-determinism

**What goes wrong:** If Assumption A1 lands as `tick_volume: f64`, summing per-tick volumes via any reordered sum (rayon `par_iter().sum()`, SIMD-vectorised manual reduction) produces different f64 bit patterns across runs. f64 addition is NOT associative: `(a + b) + c ≠ a + (b + c)` in general.

**Why it happens:** Rayon's parallel sum splits the input across threads, and the join order is scheduler-dependent.

**How to avoid:** All inner reductions (per-bucket sums of `tick_volume`) MUST be sequential. Parallelism happens at the `(symbol, side, timeframe)` task level only. Document this in the aggregator module doc.

**Warning signs:** A determinism property test (compare two runs byte-for-byte) fails non-deterministically; the failure goes away when run under `--test-threads=1`.

### Pitfall 4: Forgetting the 00-indexed month

**What goes wrong:** Executor writes `path.join(format!("{:02}", date.month()))` instead of using `DukascopyMonth::from_chrono_date(date).dir_name()`. The reader looks in `/EURUSD/2024/03/15_bid.csv.zst` (calendar March) instead of `/EURUSD/2024/02/15_bid.csv.zst` (Dukascopy March). The day appears missing; the gap detector emits a MissingFile gap; the aggregator silently emits nothing for that day.

**Why it happens:** Calendar months and Dukascopy months differ by 1; the convention is buried in upstream code and easy to miss.

**How to avoid:** ALL path construction goes through `path_layout::day_csv_zst()`. Make the `DukascopyMonth::from_calendar` constructor `pub(crate)`-visible only inside the path_layout module. Add the boundary tests listed under **00-Indexed Month Encapsulation** as MANDATORY tests. Lint suggestion: `clippy::disallowed_methods` banning `format!` calls that look like month formatting inside reader code.

**Warning signs:** A symbol's data appears empty for entire months; the gap manifest is suspiciously full of MissingFile entries; the user's `tradedesk-dukascopy` cache directory inspection shows the files exist but in different paths.

### Pitfall 5: mtime-only fingerprints

**What goes wrong:** Executor adds an mtime fast-path optimisation and forgets that mtime can be reset by `cp -p`, normalised by ext4, or skewed across NFS mounts. After a `git clone` or backup-restore, every day's mtime matches the sidecar (because the restore preserved mtimes) and the cache is served stale even though the bytes differ.

**Why it happens:** mtime feels like a free win because checking mtime is one stat() vs hashing the file content.

**How to avoid:** v1 ships **blake3-of-full-bytes ONLY.** No mtime check. If profiling later demands an optimisation, add `(size, mtime, sha)` as a defensive triple — but the blake3 is the source of truth. Document this in the fingerprint module: "DO NOT add mtime-only fingerprinting without a unanimous review."

**Warning signs:** Two users with identical source caches produce different cache file contents; a cache that was correct yesterday serves stale data today after a system clock change.

### Pitfall 6: Aggregating across DST without timezone awareness

**What goes wrong:** Executor reads "the aggregator must handle DST" and reaches for `chrono-tz`, attempting to convert UTC inputs to a local timezone, do calendar arithmetic, convert back. Edge cases multiply (spring-forward skipping a wall hour, fall-back duplicating one); fixture tests fail in confusing ways.

**Why it happens:** DST sounds like an aggregator concern; in reality, the aggregator is UTC-only and DST is invisible (CONTEXT D2-11). The DST fixture tests verify the aggregator does NOTHING special — bars at 02:00, 03:00 UTC contain exactly the 1m bars whose UTC timestamps fall in those hours, regardless of what wall-clock time that was anywhere on Earth.

**How to avoid:** DO NOT add `chrono-tz` to the workspace. The Calendar predicate is UTC-only. The aggregator processes UTC timestamps and produces UTC buckets. The "DST handling" test fixture lives in `crates/miner-core/tests/dst_fixtures.rs` and asserts the aggregator outputs are bit-identical regardless of which "DST side" the source data lives in — proving the property that DST is invisible.

**Warning signs:** `chrono-tz` appears in `Cargo.toml`; the aggregator imports `chrono::Local`; tests use timezone names like `"America/New_York"`.

## DST + Edge-Case Fixtures

For each fixture, the test generates synthetic 1m bars spanning the boundary, runs the aggregator on `Tf15m`, and asserts the output bar set matches the expected enumeration. Fixtures live under `crates/miner-reader-dukascopy/tests/fixtures/` and `crates/miner-core/tests/aggregator_fixtures.rs`.

### Spring-forward (US DST start)

**Date:** 2024-03-10. US local clocks jump from 02:00 EST → 03:00 EDT at 07:00 UTC.

**Source data:** 1m bars covering `2024-03-10T06:00:00Z` to `2024-03-10T08:00:00Z` (the 2-hour UTC window straddling the DST jump). 120 raw bars, all present.

**Expected aggregator output (15m):** 8 bars opening at `06:00, 06:15, 06:30, 06:45, 07:00, 07:15, 07:30, 07:45`. Each bar contains exactly 15 raw 1m bars. **DST is invisible** — the aggregator sees a continuous UTC sequence and emits 8 evenly-spaced UTC buckets. No special handling.

### Fall-back (US DST end)

**Date:** 2024-11-03. US local clocks jump from 02:00 EDT → 01:00 EST at 06:00 UTC.

**Source data:** 120 raw 1m bars from `2024-11-03T05:00:00Z` to `2024-11-03T07:00:00Z`.

**Expected aggregator output (15m):** 8 bars at `05:00, 05:15, ..., 06:45`. Same as spring-forward — the aggregator emits 8 evenly-spaced UTC buckets. No duplicated bars, no missing bars.

### Weekend gap (FX-major closure)

**Date range:** Friday 2024-03-08 21:00 UTC → Monday 2024-03-11 00:00 UTC.

**Source data:** 1m bars present until 22:00 UTC on Fri (per FX-major default close), then none until Sun 22:00 UTC, then resumes through Mon 00:00 UTC.

**Expected aggregator output (15m):**
- Bars at `21:00, 21:15, 21:30, 21:45` on Fri (closed-aligned to UTC midnight). All present.
- `22:00` on Fri: the calendar marks `is_open_at(22:00)` as FALSE (Fri close), so even if a 1m bar exists at `22:00`, the aggregator's standard rule is the bar IS emitted (per D2-19: partial bars are emitted as-is). But the bar at `22:00` is fully in the closed window, so it has zero source bars and is therefore SKIPPED (no bar, no gap entry — closed time is not a gap).
- No bars Saturday or Sunday before 22:00 UTC.
- Bars resume at `22:00` on Sun (calendar reopens) → at `22:00, 22:15, ...`.
- **No gap entries** anywhere in the manifest for this range — the Fri-22:00→Sun-22:00 window is entirely calendar-closed and therefore not a gap by D2-07.

### Holiday — Dec 25

**Date range:** 2024-12-24 22:00 UTC → 2024-12-26 22:00 UTC.

**Source data:** Bars present through Dec 24 22:00 UTC, then none on Dec 25 (the file is genuinely absent on disk), then resumes Dec 26 00:00 UTC.

**Expected output:**
- Bars present through Dec 24 22:00 UTC (assuming Dec 24 22:00 onward is open by FX calendar — actually it would be Christmas Eve close, depending; for FX-major default it's open until Fri 22:00).
- **Dec 25:** calendar marks closed; no bars expected; **no gap entry** even though the file is missing (closed time isn't a gap). The fingerprint sidecar would record `2024-12-25: <fingerprint or None>` — `None` (file absent) is fine because the day is closed anyway.
- Dec 26: bars resume normally.
- **No gap entries** in the manifest for Dec 25.

### Instrument first day in cache

Pick a symbol with a known earliest date (e.g., `EURUSD` first cache date is 2003-05-04 for a real Dukascopy account). For the fixture, use a synthetic symbol `XTEST` with no source files before `2024-03-15`.

**Source data:** No files before 2024-03-15. Files from 2024-03-15 onward.

**Range queried:** 2024-03-10 to 2024-03-20.

**Expected output:**
- No bars for Mar 10-14.
- **Gap manifest entries** for Mar 11, 12, 13, 14 (Mon-Thu, calendar-open days with missing source files). Each is a `MissingSourceFile { date }` reason.
- (No gap entry for Sat Mar 9 or Sun Mar 10 — closed.)
- Bars normal from Mar 15 onward.

### Instrument last day in cache

Mirror of first-day. Synthetic symbol with no source files after `2024-04-15`. Range queried 2024-04-10 to 2024-04-20. Expected: bars through Apr 15; gap entries for Apr 16, 17, 18 (Tue, Wed, Thu open days with missing files); no gap entry for Sat/Sun.

### Partial bar at session open

**Source data:** Sunday 22:00 UTC reopening — first 1m bar at exactly `22:00:00Z`. Bars normal thereafter.

**Expected output:** 15m bar at `22:00` covering 1m bars `[22:00, 22:15)`. If all 15 1m bars are present, this is a normal bar — no gap entry. If only the first few minutes have ticks (e.g., 22:00, 22:01, 22:02, then nothing until 22:08), the 15m bar IS emitted (calendar-open period contains some data), but an `IntraDayGap` entry is recorded if more than 50% of the open-hours minutes in the bucket lack a 1m bar.

For the fixture: emit 1m bars at 22:00, 22:01, 22:02 only (3 of 15). The 15m bar `[22:00, 22:15)` is emitted (OHLC computed from those 3 bars; tick_count summed). Gap manifest contains `IntraDayGap { affected_minutes: 12 }` because 12 of the 15 open-hours minutes are missing.

## Code Examples

### Reading a Dukascopy day file end-to-end

```rust
// In crates/miner-reader-dukascopy/src/lib.rs

use std::io::BufReader;
use std::fs::File;
use std::path::Path;
use chrono::{DateTime, Utc, NaiveDate};

#[derive(Debug, serde::Deserialize)]
struct CsvRow {
    timestamp: String,             // "2024-03-15 00:00:00+00:00"
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    volume: f64,                   // tick-volume sum
}

fn parse_timestamp(s: &str) -> Result<DateTime<Utc>, chrono::ParseError> {
    chrono::DateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%:z")
        .map(|dt| dt.with_timezone(&Utc))
}

fn read_day_bars(path: &Path) -> Result<Vec<RawBar>, ReaderError> {
    // Source: https://docs.rs/csv/latest/csv/ + docs.rs/zstd/latest/zstd/
    let file = File::open(path)?;
    let buf = BufReader::with_capacity(1024 * 1024, file);        // 1 MiB buffer
    let decoder = zstd::stream::read::Decoder::new(buf)?;          // streaming zstd
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .from_reader(decoder);

    let mut out = Vec::with_capacity(1440);                        // max 1440 minutes/day
    for record in rdr.deserialize::<CsvRow>() {
        let row = record?;
        let ts_open = parse_timestamp(&row.timestamp)?;
        out.push(RawBar {
            ts_open_utc: ts_open,
            ts_close_utc: ts_open + chrono::Duration::minutes(1),
            open: row.open,
            high: row.high,
            low: row.low,
            close: row.close,
            tick_count: row.volume as u32,    // OR f64 per Assumption A1
        });
    }
    Ok(out)
}
```

[CITED: chrono format spec — docs.rs/chrono/latest/chrono/format/strftime; csv usage — docs.rs/csv; zstd streaming — docs.rs/zstd]

### Writing a BarFrame to Arrow IPC (atomic)

```rust
// In crates/miner-core/src/cache/arrow_io.rs

use arrow::array::{Float64Array, TimestampNanosecondArray, UInt32Array};
use arrow::record_batch::RecordBatch;
use arrow::ipc::writer::FileWriter;
use std::sync::Arc;

pub fn write_barframe_atomic(target: &Path, frame: &BarFrame) -> Result<(), CacheError> {
    let schema = Arc::new(build_schema(&frame.source_id, &frame.symbol, frame.side, frame.tf));

    // Convert column Vecs to Arrow arrays.
    let ts_open: Vec<i64> = frame.ts_open_utc.iter().map(|t| t.timestamp_nanos_opt().unwrap()).collect();
    let ts_close: Vec<i64> = frame.ts_close_utc.iter().map(|t| t.timestamp_nanos_opt().unwrap()).collect();

    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            Arc::new(TimestampNanosecondArray::from(ts_open).with_timezone("UTC")),
            Arc::new(TimestampNanosecondArray::from(ts_close).with_timezone("UTC")),
            Arc::new(Float64Array::from(frame.open.clone())),
            Arc::new(Float64Array::from(frame.high.clone())),
            Arc::new(Float64Array::from(frame.low.clone())),
            Arc::new(Float64Array::from(frame.close.clone())),
            Arc::new(UInt32Array::from(frame.tick_count.clone())),
        ],
    )?;

    let parent = target.parent().expect("absolute path");
    std::fs::create_dir_all(parent)?;
    let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
    {
        let mut writer = FileWriter::try_new(&mut tmp, &schema)?;
        writer.write(&batch)?;
        writer.finish()?;
    }
    tmp.as_file().sync_all()?;
    tmp.persist(target)?;
    Ok(())
}
```

[CITED: arrow-ipc FileWriter API — docs.rs/arrow-ipc/latest/arrow_ipc/writer/struct.FileWriter.html]

### Calendar predicate property test

```rust
// In crates/miner-core/src/calendar.rs

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn fx_major_friday_close_at_22_utc() {
        let cal = Calendar::fx_major();
        let just_before = Utc.with_ymd_and_hms(2024, 3, 8, 21, 59, 59).unwrap();
        let at_close = Utc.with_ymd_and_hms(2024, 3, 8, 22, 0, 0).unwrap();
        let mid_weekend = Utc.with_ymd_and_hms(2024, 3, 9, 12, 0, 0).unwrap();
        let just_before_reopen = Utc.with_ymd_and_hms(2024, 3, 10, 21, 59, 59).unwrap();
        let at_reopen = Utc.with_ymd_and_hms(2024, 3, 10, 22, 0, 0).unwrap();
        assert!(cal.is_open_at(just_before));
        assert!(!cal.is_open_at(at_close));
        assert!(!cal.is_open_at(mid_weekend));
        assert!(!cal.is_open_at(just_before_reopen));
        assert!(cal.is_open_at(at_reopen));
    }

    #[test]
    fn fx_major_christmas_closed_every_year() {
        let cal = Calendar::fx_major();
        for year in 2010..=2030 {
            let xmas_noon = Utc.with_ymd_and_hms(year, 12, 25, 12, 0, 0).unwrap();
            let nye_noon = Utc.with_ymd_and_hms(year, 1, 1, 12, 0, 0).unwrap();
            assert!(!cal.is_open_at(xmas_noon), "year {} xmas should be closed", year);
            assert!(!cal.is_open_at(nye_noon), "year {} NYE should be closed", year);
        }
    }
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Polars DataFrames as in-flight type | `ndarray` views + iterators for streaming reductions | Settled in CLAUDE.md (May 2026) | Lower allocation pressure under rayon parallelism |
| Parquet for write-once columnar cache | Arrow IPC files | Settled in CLAUDE.md (May 2026), reinforced by CONTEXT D2-01 | Faster mmap reads; trivial Polars/Pandas read in Python |
| HashMap for serialised maps | BTreeMap | Phase 1 (May 2026) | Deterministic JSON output (OUT-03) |
| mtime + size for cache validity | blake3 of full bytes | Phase 2 (this research) | Correct under `cp -p`, NFS, backup-restore |
| chrono `parse_from_rfc3339` for Dukascopy CSV | `parse_from_str("%Y-%m-%d %H:%M:%S%:z")` | This research | Required because Dukascopy uses space, not T-separator |
| WalkDir default iteration | `.sort_by_file_name()` | This research | Required for byte-identity determinism |

**Deprecated/outdated:**
- `chrono::Utc::now()` inside the aggregator — forbidden by determinism contract.
- `serde_json::Map` with `preserve_order` feature — Phase 1 explicitly excluded; do not enable.
- `unsafe_code = "forbid"` lint at workspace level — must be downgraded to `deny` IF mmap is adopted (recommended deferred to optimisation plan).

## Validation Architecture

> Nyquist VALIDATION.md is enabled per `.planning/config.json` `workflow.nyquist_validation: true`. This section is the per-requirement validation strategy the orchestrator consumes.

### Test Framework

| Property | Value |
|----------|-------|
| Framework | `cargo test` (built-in) + `cargo-nextest` 0.9+ for parallel local runs |
| Config file | none (workspace `Cargo.toml` test layout is conventional) |
| Quick run command | `cargo test -p miner-core` or `cargo test -p miner-reader-dukascopy` |
| Full suite command | `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check` |

### Phase Requirements → Test Map

| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| CACHE-01 | DukascopyReader opens a real `.csv.zst` fixture and yields 1m bars in ascending UTC order | integration | `cargo test -p miner-reader-dukascopy --test reader_smoke -- reads_one_day_in_order` | ❌ Wave 0 (`crates/miner-reader-dukascopy/tests/reader_smoke.rs` created in plan) |
| CACHE-01 | DukascopyReader rejects a zero-byte source file with `CorruptSourceFile` error | unit | `cargo test -p miner-reader-dukascopy zero_byte_file` | ❌ Wave 0 |
| CACHE-02 | `Reader` trait is dyn-compatible and `DukascopyReader: Reader` | unit | `cargo test -p miner-reader-dukascopy reader_trait_object_safety` | ❌ Wave 0 |
| CACHE-03 | Aggregator emits 15m / 1h / 1d bars from a 1-day synthetic 1m input | unit | `cargo test -p miner-core aggregator::tests::three_timeframes` | ❌ Wave 0 |
| CACHE-04 | Aggregator output is byte-identical across two runs on the same input (Arrow IPC bytes) | integration | `cargo test -p miner-core --test aggregator_determinism byte_identical_two_runs` | ❌ Wave 0 |
| CACHE-04 | OHLC reduction property — high ≥ open, high ≥ close, low ≤ open, low ≤ close for every emitted bar (proptest) | property | `cargo test -p miner-core aggregator::tests::ohlc_monotonicity_proptest` | ❌ Wave 0 |
| CACHE-04 | Aggregator omits (never interpolates) bars when calendar-open minutes lack source data | unit | `cargo test -p miner-core aggregator::tests::omits_gaps_never_interpolates` | ❌ Wave 0 |
| CACHE-04 | DST spring-forward fixture: bars at 06:00..07:45 UTC are evenly spaced, 15 bars total | fixture | `cargo test -p miner-core --test dst_spring_forward` | ❌ Wave 0 |
| CACHE-04 | DST fall-back fixture: bars at 05:00..06:45 UTC are evenly spaced, 15 bars total | fixture | `cargo test -p miner-core --test dst_fall_back` | ❌ Wave 0 |
| CACHE-04 | Bid/ask sides process independently (running both produces no cross-contamination) | unit | `cargo test -p miner-core aggregator::tests::bid_ask_independent` | ❌ Wave 0 |
| CACHE-05 | Dukascopy `volume` field flows through to the bar `tick_count` (or `tick_volume` per A1) | unit | `cargo test -p miner-reader-dukascopy reader_smoke::tick_count_from_csv_volume` | ❌ Wave 0 |
| CACHE-05 | 00-indexed month: January maps to "00" | unit | `cargo test -p miner-reader-dukascopy path_layout::jan_maps_to_00` | ❌ Wave 0 |
| CACHE-05 | 00-indexed month: December maps to "11" | unit | `cargo test -p miner-reader-dukascopy path_layout::dec_maps_to_11` | ❌ Wave 0 |
| CACHE-05 | 00-indexed month: 0 and 13 panic | unit | `cargo test -p miner-reader-dukascopy path_layout::out_of_range_panics` | ❌ Wave 0 |
| CACHE-05 | 00-indexed month: full path round-trip property test | property | `cargo test -p miner-reader-dukascopy path_layout::path_round_trip_proptest` | ❌ Wave 0 |
| CACHE-06 | Cache hit serves bars without re-reading source CSV (verified by mocking the reader) | integration | `cargo test -p miner-core --test cache_smoke cache_hit_skips_reader` | ❌ Wave 0 |
| CACHE-06 | Aggregator version bump triggers full rebuild | integration | `cargo test -p miner-core --test cache_smoke aggregator_version_bump_rebuilds` | ❌ Wave 0 |
| CACHE-06 | Per-day fingerprint mismatch triggers day-splice only | integration | `cargo test -p miner-core --test cache_smoke day_fingerprint_bump_splices` | ❌ Wave 0 |
| CACHE-06 | Atomic write: crash mid-write leaves the existing Arrow file intact (simulated by killing during write) | integration | `cargo test -p miner-core --test cache_smoke -- atomic_write_crash_safety --ignored` (manual-ish — simulated by interrupting write_arrow_atomic before persist) | ❌ Wave 0 |
| CACHE-07 | GapDetector emits MissingFile for a missing day's source file | unit | `cargo test -p miner-core gap::tests::missing_file_emits_correct_reason` | ❌ Wave 0 |
| CACHE-07 | GapDetector emits CorruptFile for a zero-byte source file | unit | `cargo test -p miner-core gap::tests::zero_byte_emits_corrupt` | ❌ Wave 0 |
| CACHE-07 | GapDetector emits IntraDayGap for a sub-minute hole during open hours | unit | `cargo test -p miner-core gap::tests::intra_day_hole_during_open_hours` | ❌ Wave 0 |
| CACHE-07 | GapDetector does NOT emit anything for closed-hours holes (weekend, holiday) | unit | `cargo test -p miner-core gap::tests::closed_hours_are_not_gaps` | ❌ Wave 0 |
| CACHE-07 | Gap manifest JSON shape is pinned by insta snapshot | snapshot | `cargo test -p miner-core --test gap_manifest_snapshot` (uses `insta::assert_json_snapshot!`) | ❌ Wave 0 (`crates/miner-core/tests/snapshots/` directory + .snap files created in plan) |
| CACHE-07 | Gaps in the manifest are sorted by start_utc ascending | property | `cargo test -p miner-core gap::tests::gaps_sorted_proptest` | ❌ Wave 0 |
| CACHE-08 | `GapManifest` derives `JsonSchema` and round-trips via serde_json | unit | `cargo test -p miner-core gap::tests::gap_manifest_schemars_roundtrip` | ❌ Wave 0 |
| All | Arrow IPC schema bytes pinned by insta snapshot | snapshot | `cargo test -p miner-core --test arrow_schema_snapshot` | ❌ Wave 0 |
| All (determinism) | Two runs of full aggregator + cache produce byte-identical `.arrow` + `.fingerprints.json` files | integration | `cargo test -p miner-core --test full_determinism two_runs_byte_identical` | ❌ Wave 0 |

### Sampling Rate

- **Per task commit:** `cargo test -p <crate-being-touched>` (or `cargo test -p miner-core -p miner-reader-dukascopy` for cross-crate)
- **Per wave merge:** `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check`
- **Phase gate (before `/gsd:verify-work`):** Full suite green + tokio-tree gate (still passes, no async deps added) + schema-sync gate (Phase 1 envelope schema unchanged; Phase 2 adds its own `JsonSchema`-derived types but those are NOT in `findings-v1.schema.json`)

### Wave 0 Gaps

All Phase 2 tests are net-new. The planner MUST schedule the following test infrastructure in Wave 0:

- [ ] `crates/miner-reader-dukascopy/tests/fixtures/synthetic_cache/` — directory of synthetic `.csv.zst` files generated by a test helper (NOT checked-in compressed data; the helper writes them at test setup time using `tempfile::TempDir` + the same zstd encoder Dukascopy uses)
- [ ] `crates/miner-reader-dukascopy/tests/reader_smoke.rs` — integration tests against the synthetic cache
- [ ] `crates/miner-reader-dukascopy/src/path_layout.rs` — module + tests for 00-indexed month + path round-trip
- [ ] `crates/miner-core/tests/aggregator_fixtures.rs` — DST + weekend + holiday + first/last day + partial-bar fixtures
- [ ] `crates/miner-core/tests/dst_spring_forward.rs` + `tests/dst_fall_back.rs` — split per fixture for parallel runs
- [ ] `crates/miner-core/tests/cache_smoke.rs` — cache hit/miss/atomic-write tests
- [ ] `crates/miner-core/tests/gap_manifest_snapshot.rs` — insta snapshot of manifest JSON
- [ ] `crates/miner-core/tests/arrow_schema_snapshot.rs` — insta snapshot of Arrow IPC schema bytes
- [ ] `crates/miner-core/tests/full_determinism.rs` — end-to-end two-runs-byte-identical
- [ ] `crates/miner-core/tests/snapshots/` directory — created by first `cargo insta accept` run
- [ ] Workspace dev-deps: `proptest = "1.11"`, `insta = "1.47"` (added via `cargo add --dev --workspace`)
- [ ] Workspace lints: keep `unsafe_code = "forbid"` for Phase 2 (no mmap; defer to a later optimisation plan)

## Sources

### Primary (HIGH confidence — verified against source code or official docs)

- **`tradedesk-dukascopy/tradedesk_dukascopy/export.py`** (sibling repo, lines 1-50, 220-280, 282-320, 390-420, 436-460) — verified the CSV schema (`timestamp,open,high,low,close,volume`), the space-separated `+00:00` timestamp format, the 00-indexed month layout, the `_<bid|ask>.csv.zst` naming, the `volume = sum of tick_volume floats` semantic, the zstd compression at level 3, the `dropna(subset=["open","high","low","close"])` that produces sparse 1m time series.
- **`tradedesk-miner/CLAUDE.md`** §"Technology Stack" + §"Aggregated-Bar Cache Format" + §"What NOT to Use" + §"State of the Art" — the locked stack; Arrow IPC over Parquet/SQLite/custom; zstd without zstdmt; ndarray over polars for in-flight; BTreeMap discipline.
- **`tradedesk-miner/.planning/phases/02-reader-aggregator-derived-bar-cache/02-CONTEXT.md`** — D2-01..D2-21 user-locked decisions.
- **`tradedesk-miner/.planning/phases/01-foundations-contracts/01-RESEARCH.md`** + **01-VERIFICATION.md** — Phase 1 verified locked envelope; what crates/types/modules exist as of phase start.
- **`tradedesk-miner/Cargo.toml`** — workspace deps already present (serde, chrono, blake3, etc.) and lints (`unsafe_code = "forbid"`).
- **`tradedesk-miner/crates/miner-core/src/lib.rs`** — Phase 1 FROZEN public surface; what Phase 2 reuses (`MinerConfig`, `CODE_REVISION`, `WireError`, `PreflightCode`).
- **docs.rs/csv/latest/csv/** — `csv::ReaderBuilder::from_reader`, `ByteRecord`, serde deserialize integration (latest version 1.4.0).
- **docs.rs/arrow-ipc/latest/arrow_ipc/** — `FileWriter::try_new`, `write`, `finish`, `write_metadata`; `FileReader::try_new` for seek-based random access; mmap-friendly + zero-copy reads.
- **docs.rs/arrow-ipc/latest/arrow_ipc/writer/struct.FileWriter.html** — confirmed Arrow IPC FileWriter is append-only with finalising footer; in-place batch replacement is NOT supported.
- **docs.rs/chrono/latest/chrono/** + **format/strftime/** — `parse_from_str` with `%:z` is the correct parser for Dukascopy's `+00:00` (with colon) format.
- **docs.rs/walkdir/latest/walkdir/struct.WalkDir.html** — confirmed `sort_by_file_name()` is required for deterministic iteration.
- **lib.rs/crates/arrow** — verified latest version 58.3.0 (May 11, 2026), MSRV not explicitly fixed.
- **lib.rs/crates/zstd** — verified latest 0.13.3 (Feb 20, 2025), streaming `Decoder<R: Read>` available.
- **lib.rs/crates/memmap2** — verified latest 0.9.10 (Feb 15, 2026).
- **lib.rs/crates/proptest** — verified latest 1.11.0 (Mar 24, 2026).
- **lib.rs/crates/insta** — verified latest 1.47.2 (Mar 30, 2026).
- **slopcheck install --ecosystem crates.io arrow csv csv-core zstd walkdir tempfile globset** — all 7 packages [OK] verdict.

### Secondary (MEDIUM confidence — verified via web search, single source)

- README of `tradedesk-dukascopy` describing the output CSV format ("Volume is derived from tick volume") — corroborates the source-code finding.

### Tertiary (LOW confidence — needs validation)

- The 50% threshold for "intra-day partial bar gap flag" in D2-19 is a CONTEXT decision but no upstream source defines it; carrying as-is from CONTEXT.
- The performance budget of 10 ns/call for `Calendar::is_open_at` is estimated; Phase 7's benchmarks will verify or refute it.

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | The field named `tick_count: u32` in CONTEXT D2-13 should actually be `tick_volume: f64` because the Dukascopy CSV `volume` column is a sum of per-tick float volumes, not an integer count. Casting `volume as u32` truncates non-integer values and loses precision. | CSV Schema (verified from source); Reader Trait Surface | The field name in the public API of `BarFrame` and Arrow IPC schema mislabels what the data is. Quant agent consumers receive an integer where the actual value was a float (information loss); downstream Python interop (`tradedesk` aggregator export, PLAT-v2-02) will produce different values from the Python original. **RECOMMENDATION: rename to `tick_volume: f64` and document.** Alternative: keep the name `tick_count` but make it `f64` (worse — misleading name). Alternative: round to u32 and call it tick count (loses fidelity). |
| A2 | The CSV header row is consistently present in every `.csv.zst` file produced by `tradedesk-dukascopy`. | CSV Schema | If a corrupt/old file lacks the header, `csv::ReaderBuilder::has_headers(true)` mis-parses the first data row as the header. Mitigation: try both has_headers values on first read and pick the one that parses the timestamp column. Verified at `export.py:443` (`out.reset_index().to_csv(index=False)` — pandas writes a header by default). High confidence the assumption holds; flagged for defensive parsing. |
| A3 | `arrow = "58"` is API-compatible enough with CLAUDE.md's pin `arrow = "53"` for the planner to use 58 freely; no MSRV concerns for Rust 1.85. | Standard Stack | Major bumps in `arrow-rs` have historically been routine API-renaming + new features; from 53 → 58 we expect some method renames. Phase 2's plan should `cargo add arrow` and let the toolchain pick whatever the latest published is at plan time; the workspace then pins that exact major. |
| A4 | The performance of `Calendar::is_open_at` is < 100 ns/call (the budget). | Trading Calendar | If it's 10x slower, gap detection over 6 years of EURUSD becomes 320 ms — still acceptable but noticeable. Validation: write a `criterion` microbench in Phase 7 (or in Phase 2 if quick) asserting < 100 ns. |
| A5 | The 50% "partial bar gap" threshold (D2-19) is the right cutoff. | Aggregator Semantics | If 50% is too lax, the gap manifest under-reports incomplete buckets. If too strict, it spams the manifest with trivial gaps. Validation: gate this in a manual-review checkpoint in Plan; ask user to confirm 50% or pick another threshold. |

**If this table is empty:** Not empty — flag A1 for confirmation BEFORE planning seals task definitions for the reader and aggregator. A1 changes the type signature of `RawBar.tick_count` and `BarFrame.tick_count`, the Arrow schema's `tick_count` field type, and every test fixture that constructs `RawBar`. Decide once, propagate everywhere.

## Open Questions

1. **Confirm Assumption A1 (`tick_count: u32` → `tick_volume: f64`) before planning.**
   - What we know: Source CSV `volume` is a float sum (verified). README acknowledges "tick volume." CONTEXT D2-13 says `tick_count: u32`.
   - What's unclear: Whether the user wants the field renamed (correct, breaks `tick_count` symbol everywhere), the type widened to `f64` keeping the name (misleading), or the value rounded to integer (lossy).
   - Recommendation: **Rename to `tick_volume: f64`.** Update CONTEXT.md D2-13 to match. Update Arrow schema field name to `tick_volume`. Update CACHE-05 wording in REQUIREMENTS.md ("user can trust that miner reports Dukascopy's `volume` field as tick volume") — this might require a milestone-level requirement edit; flag.

2. **Decide on the mmap optimisation now or defer.**
   - What we know: mmap requires downgrading `unsafe_code = "forbid"` → `"deny"` at the workspace level. CONTEXT does not say either way.
   - What's unclear: Whether the planner should adopt mmap reads in v1 (slight performance win, lint complication) or buffered reads in v1 (no perf concern, no lint change) and defer mmap to a Phase 7 optimisation.
   - Recommendation: **Defer mmap to Phase 7.** v1 ships `BufReader<File>` reads. Document the deferral in `cache/mod.rs` doc comments and in the plan as a "Future Optimisation" task.

3. **Decide on Arrow IPC schema metadata key ordering once.**
   - What we know: `Schema::new_with_metadata` accepts a `HashMap<String, String>`. The IPC bytes encode the metadata in insertion order.
   - What's unclear: Whether the construction site (using BTreeMap-collect then for-loop-insert into HashMap) actually preserves insertion order across Rust versions.
   - Recommendation: Add a property test that builds the schema twice and asserts `Schema::metadata()` returns keys in the same order. If it doesn't, switch to manually-constructed Schema bytes via a sorted `Vec<(String, String)>`. Pin Arrow major version.

4. **Arrow-rs API stability for full-rebuild vs day-splice.**
   - What we know: Arrow IPC `FileWriter` is append-only; in-place batch update is impossible. D2-03's "splice" is implemented as full-file rewrite to a tempfile.
   - What's unclear: Whether the full-rewrite for a typical quartet (~50 MB at 6 years × 15m bars) is fast enough to satisfy the "re-runs hit cache, not raw decode" requirement.
   - Recommendation: Profile early. If full-rewrite-per-affected-day is too slow, batch up multiple day-splices in a single rebuild (call `aggregate_with_cache` over a range and process all stale days at once before rewriting). For v1 with day-level granularity this is straightforward.

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| `cargo` toolchain | Build | ✓ (presumed — Phase 1 verification used it) | 1.85 | — |
| `rustc` 1.85+ | Build | ✓ | 1.85 (per `rust-toolchain.toml`) | — |
| `git` | `build.rs` `CODE_REVISION` injection | ✓ (Phase 1 verified) | — | falls back to `"unknown"` |
| Source Dukascopy cache `/paperclip/tradedesk/marketdata` (or wherever the user keeps it) | Integration tests against real data | ✗ (per sandbox; not relevant — Phase 2 ships synthetic fixtures only) | — | Synthetic fixtures generated by test helper |
| Network access for cargo-add | Plan-phase cargo dep adds | ✓ (presumed) | — | If offline, the planner must use Cargo.lock + offline cargo |

**Missing dependencies with no fallback:** None. Phase 2 uses synthetic fixtures only; it does NOT depend on the user having a populated `cache_root`.

**Missing dependencies with fallback:** None requiring action — the synthetic-fixture pattern means Phase 2 development is independent of real data availability.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — all packages verified via `slopcheck install` + `lib.rs` queries; versions current as of May 2026.
- CSV schema: HIGH — directly read from `tradedesk-dukascopy/export.py` source code. **`tick_count` vs `tick_volume` is a discovery, not a doubt.**
- Aggregator semantics: HIGH on UTC bucketing, OHLC reduction, determinism; MEDIUM on the 50% partial-bar threshold (CONTEXT decision, no independent source).
- Arrow IPC cache: HIGH on append-only writer + atomic-rename pattern; MEDIUM on schema metadata key-ordering byte stability (needs proptest verification).
- Gap manifest: HIGH on shape; HIGH on FX-major calendar (CONTEXT-locked).
- Pitfalls: HIGH — every pitfall confirmed by official docs (walkdir, chrono format, arrow-rs FileWriter).
- Validation Architecture: HIGH on test plan; the Wave 0 file list is exhaustive.

**Research date:** 2026-05-17
**Valid until:** 2026-06-17 (30 days for a stable Rust-ecosystem domain). Arrow-rs and chrono are mature; the only fast-moving piece (`jiff`) was explicitly deferred per CONTEXT D2-09.

---

## RESEARCH COMPLETE

Phase 2 research confirms every CONTEXT decision is implementable as written, with one assumption flagged for plan-phase confirmation: **D2-13's `tick_count: u32` should be `tick_volume: f64`** because the Dukascopy CSV `volume` column is a sum of per-tick float volumes (verified by reading the sibling repo's `export.py` directly), not an integer tick count. The 00-indexed month layout, the space-separated UTC timestamp format, and the bid/ask file separation are all confirmed upstream. The Arrow IPC API is append-only — D2-03's "splice" means full-file atomic-rename rewrite. The five most likely executor-agent landmines (non-deterministic WalkDir, HashMap ordering, f64 parallel-sum, calendar-vs-Dukascopy month confusion, mtime-only fingerprinting) are pre-empted with concrete mitigations the planner can wire into `<acceptance_criteria>` blocks. Validation Architecture covers every CACHE-01..08 requirement with at least one automated test, plus three insta snapshots for byte-identity gates. Confidence is HIGH overall; the planner can proceed.
