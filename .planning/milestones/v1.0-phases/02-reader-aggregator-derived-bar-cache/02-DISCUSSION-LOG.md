# Phase 2: Reader, Aggregator & Derived-Bar Cache — Discussion Log

**Discussed:** 2026-05-17
**Mode:** default (no overlay flags)
**Outcome:** 21 decisions captured in `02-CONTEXT.md`

This log is for human reference (audits, retrospectives). Downstream agents read `02-CONTEXT.md`, not this file.

---

## Carried Forward From Phase 1 (Not Re-Discussed)

- Sync + rayon only — `tokio` MUST NOT enter `miner-core` (FOUND-04, CI-enforced)
- `BTreeMap` + `serde_json` without `preserve_order` — envelope-byte determinism (OUT-03)
- `figment` config layering with `MINER_*` env, `.split("__")`; `cache_root` + `bar_cache_root` already in `MinerConfig`
- Output via `FindingSink`; logs through `tracing` → stderr
- `volume` field is `tick_count`, not contract volume (CACHE-05)
- 00-indexed Dukascopy month layout is the inherited quirk to encapsulate (CACHE-01)
- Workspace shape pinned: reader/aggregator code lives in `miner-reader-dukascopy` + `miner-core`
- STATE.md flagged "Arrow IPC vs bincode+zstd" as the headline open question

## Gray Areas Identified

Phase 2 gray areas presented to user (multi-select):

1. Derived-bar cache file format
2. Reader trait surface + Bar shape
3. Trading calendar source for gap detection
4. Time crate (chrono vs jiff) + DST handling

**User selected:** 1, 3, 4.
**Not selected:** 2 (Reader trait surface) — captured as Claude's discretion (D2-12 through D2-15).

---

## Area 1: Derived-Bar Cache File Format

### Q1: Which cache format?

Options presented:
- Arrow IPC (recommended; columnar, mmap-friendly, language-portable, free Python interop)
- bincode + zstd (Rust-native, opaque to Python)
- Parquet (columnar + compression, slower writes)
- Custom mmap binary (fastest, loses all tooling interop)

**User selected:** Arrow IPC.

Rationale recorded in D2-01: PROJECT.md flags PLAT-v2-02 (Python aggregator export) as a future goal; Arrow IPC keeps that door open for free.

### Q2: Cache invalidation granularity?

Options presented:
- Per-day fingerprint sidecar (recommended; honors CACHE-06 literally)
- Whole-file fingerprint (simpler, but punitive on 28×6 cache)
- mtime + size tuple (cheaper, fragile)
- Always rebuild (defeats the cache)

**User selected:** Per-day fingerprint sidecar.

Captured in D2-03/D2-04/D2-05. Sidecar shape: `<…>.fingerprints.json` mapping `YYYY-MM-DD → blake3_hex`, sibling of the `.arrow` file.

### Q3: Cache file layout (single vs partitioned)?

Not asked — Claude's discretion. Captured in D2-02: one file per `(source, symbol, side, timeframe)` covering all years; year-partitioning deferred to v2 per CLAUDE.md's natural upgrade path.

---

## Area 2: Trading Calendar Source for Gap Detection

### Q1: Where does the trading calendar come from?

Options presented:
- Hardcoded FX-major + `Reader::trading_calendar()` hook (recommended)
- Config-supplied TOML calendar (heavy v1 contract burden)
- External crate (opinionated dep, wrong scope)
- Defer to Phase 3 (CACHE-07 ships incomplete)

**User selected:** Hardcoded FX-major + Reader override hook.

Captured in D2-06/D2-08. Reader trait grows `trading_calendar() -> Calendar`; Dukascopy reader returns FX-major default.

### Q2: How extensive is the v1 FX-major default?

Options presented:
- Weekends + Christmas + New Year's Day (recommended)
- Weekends only
- Weekends + configurable holiday list (TOML)

**User selected:** Weekends + Dec 25 + Jan 1.

Captured in D2-07. Yearly holidays are computed `(month, day)` tuples; no per-year curation. Anything more exotic enters via the Reader override hook.

---

## Area 3: Time Crate (chrono vs jiff) + DST Handling

### Q1: Which time crate?

Options presented:
- Stay on chrono (recommended; Phase 1 envelope contract continuity)
- Adopt jiff for Phase 2, keep chrono for envelope (two-crate compromise)
- Swap envelope to jiff too (Phase 1 contract break)

**User selected:** Stay on chrono.

Captured in D2-09. Schema-sync CI gate would treat a jiff swap as a `schema_version` bump; not justified by Phase 2 needs. `chrono-tz` only pulled if non-UTC override calendar surfaces (Phase 3+).

### Q2: DST handling — not asked.

Claude's discretion: D2-11 — aggregator is fully UTC-agnostic about DST because bars are UTC-bucketed and FX defaults are UTC-expressed. DST tests (Phase 2 success criterion #5) verify exactly this property: spring-forward and fall-back source data must produce correct UTC bars without DST special-casing in aggregator code.

---

## Claude's Discretion Decisions

Areas not selected by the user; defaulted per the locked decisions and CLAUDE.md research:

- **Reader trait shape** (D2-12, D2-13) — iterator-returning, RawBar with `ts_open_utc` + `ts_close_utc` explicit boundaries
- **Aggregator API** (D2-14, D2-15, D2-18) — pure function of `(reader, params)`; column-oriented `BarFrame` output; `aggregator_version` const string starting at "1.0.0"
- **GapDetector module** (D2-16, D2-17) — separate module in `miner-core::gap`; emits `GapManifest` that Phase 3 wraps in `Finding::GapAborted`
- **Cache layout** (D2-20) — `<bar_cache_root>/<source_id>/<SYMBOL>/<timeframe>_<side>.arrow`
- **Test fixtures** (D2-21) — synthetic Dukascopy-format cache generated by a test helper; no production data checked in

All flagged for Plan-phase confirmation in `02-CONTEXT.md`.

---

## Scope Creep Redirects

None. Discussion stayed within Phase 2 boundary.

---

## Deferred Ideas (not Phase 2)

Captured in `02-CONTEXT.md` `<deferred_ideas>`:
- Per-symbol calendar overrides via config (Phase 3+)
- Year-partitioned Arrow files (v2 upgrade if file size becomes a constraint)
- `chrono-tz` for non-UTC calendars (Phase 3+ when needed)
- PyO3 / Python aggregator export (PLAT-v2-02 — out of scope but D2-01 keeps the door open)
- `jiff` migration (deferred to a future dedicated phase)
- DuckDB / SQLite cache (CLAUDE.md documented rejection; not revisitable in v1)
- Compression on Arrow IPC (flip the bit if cache size becomes a constraint)

---

*Session ended after 5 questions across 3 selected areas. CONTEXT.md captures the decisions; this log captures the trade-off conversation.*
