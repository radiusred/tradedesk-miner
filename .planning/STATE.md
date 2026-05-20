---
gsd_state_version: 1.0
milestone: v1.0
milestone_name: milestone
status: executing
stopped_at: Plan 04-10 complete — SEAS family shipped (6/6)
last_updated: "2026-05-20T10:30:00.000Z"
last_activity: 2026-05-20
progress:
  total_phases: 7
  completed_phases: 3
  total_plans: 31
  completed_plans: 27
  percent: 87
---

# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-05-15)

**Core value:** Surface raw statistical candidates from years of OHLCV data fast enough that a quant agent can run wide sweeps and targeted queries interactively, without waiting on Python data pipelines.
**Current focus:** Phase 04 — scan-catalogue-anom-cross-seas

## Current Position

Phase: 04 (scan-catalogue-anom-cross-seas) — EXECUTING
Plan: 10 of 11 (SEAS family complete; ANOM 04-05/04-06 + final 04-11 remain)
Status: Plan 04-10 shipped
Last activity: 2026-05-20

Progress: [████████▊░] 87%

Next: Plan 04-05 (ANOM ADF + KPSS + variance ratio) OR Plan 04-06 (ARCH-LM + Jarque-Bera) OR Plan 04-11 (Phase 4 sign-off — blocked on 04-05/06).

## Performance Metrics

**Velocity:**

- Total plans completed: 13
- Average duration: -
- Total execution time: -

**By Phase:**

| Phase | Plans | Total | Avg/Plan |
|-------|-------|-------|----------|
| 01 | 7 | - | - |
| 02 | 6 | - | - |
| 03 | 7 | - | - |

**Recent Trend:**

- Last 5 plans: 03-03, 03-04, 03-05, 03-06, 03-07
- Trend: -

*Updated after each plan completion*
| Phase 04 P04 | 38 | 3 tasks | 14 files |
| Phase 04 P10 | ~45min | 3 tasks | 13 files (12 created, 1 modified) |

## Accumulated Context

### Decisions

Decisions are logged in PROJECT.md Key Decisions table.
Recent decisions affecting current work:

- Roadmap structure: Horizontal-layers build order (workspace → reader/aggregator → engine/facade/CLI → catalogue → hygiene/sweep → wrappers → hardening) per ARCHITECTURE.md and SUMMARY.md.
- Phase 1 locks the `Finding` envelope JSON schema with `schema_version`, `scan@version`, `param_hash`, `code_revision`, `data_slice`, and reserved-but-null DSR + FDR-q fields — schema-version retrofitting is painful and is treated as non-negotiable from day one.
- `miner-core` is sync + rayon only; tokio enters only via `spawn_blocking` inside `miner-mcp` and `miner-http`. Enforced by CI checking `cargo tree -p miner-core` for tokio/async.
- Stdout = findings, stderr = logs. Enforced in CI via `clippy::disallowed_macros` banning `println!` / `eprintln!` outside the findings sink and logging adapter.
- Phase 2: derived-bar cache format is **Arrow IPC** (one file per `(source_id, symbol, side, timeframe)` quartet) with a sidecar JSON of per-day blake3 fingerprints. Two-axis invalidation (`aggregator_version` / `arrow_schema_version` mismatch → full rebuild; per-day fingerprint mismatch → day-splice). Crash-safe via tempfile-rename. `unsafe_code = "forbid"` workspace-wide; no mmap.
- Plan 04-10: ANOVA + Kruskal-Wallis bundled into a single SEAS-05 meta-scan (`seas.test.anova_kruskal@1`) with the parametric F-stat as `effect.value` and the non-parametric stats in `effect.extra`. Consumers can read either branch from the same envelope.
- Plan 04-10: EOM/SOM bucket indexing scheme — `0..cutoff_n = EOM-N..EOM-1` (most-recent-first), `cutoff_n..2*cutoff_n = SOM-1..SOM-N`. Labels emitted as UTF-8 JSON byte array (matches SEAS-03 session_boundaries_utc encoding).
- Plan 04-10: Event-window bar resolution uses `partition_point(|&t| t < event_ts)`; the event bar is the first bar of the post window, pre window stops one bar before. Events outside bar range OR with insufficient pre/post bars silently skipped (consistent with SEAS-04 middle-of-month exclusion). MAX_EVENT_TIMESTAMPS = 10^5 (T-04-10-01 DOS mitigation).

### Pending Todos

None yet.

### Blockers/Concerns

- **Phase 6 (MCP & HTTP wrappers):** `rmcp` is the highest-risk dependency in the stack. Plan-phase must re-run `gsd-research` on rmcp (crate name, version, stdio + streamable-HTTP transport, streaming tool-result chunks, tokio compatibility) before kickoff. Fallback is a hand-rolled JSON-RPC-over-stdio (~500 LOC against `serde_json`) which does not affect the HTTP wrapper.
- **Phase 4 implementation risk:** ADF, KPSS, Engle-Granger, block bootstrap, BH-FDR, and DSR are not covered by any comprehensive Rust stats crate. Plan time for hand-rolled implementations validated against scipy/statsmodels golden outputs.

Resolved this phase:

- ~~Phase 2 open question: Arrow IPC vs bincode+zstd for derived-bar cache~~ → **Arrow IPC chosen**; locked under `crates/miner-core/src/cache.rs` with two-axis invalidation and tempfile-rename crash-safety.

## Deferred Items

Items acknowledged and carried forward from previous milestone close:

| Category | Item | Status | Deferred At |
|----------|------|--------|-------------|
| *(none)* | | | |

## Session Continuity

Last session: 2026-05-20T10:30:00.000Z
Stopped at: Plan 04-10 complete — SEAS family shipped (6/6)
Resume file: None
Next action: Execute Plan 04-05 (ANOM ADF/KPSS/variance ratio) or Plan 04-06 (ARCH-LM/Jarque-Bera). Plan 04-11 (Phase sign-off) blocked until ANOM family at 11/11.
