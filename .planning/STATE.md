---
gsd_state_version: 1.0
milestone: v1.0
milestone_name: milestone
status: executing
stopped_at: Plan 04-05 complete — ANOM-05/06/07 stationarity scans shipped (ANOM family 9/11)
last_updated: "2026-05-20T12:23:17.000Z"
last_activity: 2026-05-20
progress:
  total_phases: 7
  completed_phases: 3
  total_plans: 31
  completed_plans: 28
  percent: 90
---

# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-05-15)

**Core value:** Surface raw statistical candidates from years of OHLCV data fast enough that a quant agent can run wide sweeps and targeted queries interactively, without waiting on Python data pipelines.
**Current focus:** Phase 04 — scan-catalogue-anom-cross-seas

## Current Position

Phase: 04 (scan-catalogue-anom-cross-seas) — EXECUTING
Plan: 11 of 11 — only Plan 04-06 (ARCH-LM + JB) + 04-11 (Phase sign-off) remain
Status: Plan 04-05 shipped (ANOM-05/06/07 hand-derived heavyweight stationarity scans)
Last activity: 2026-05-20

Progress: [█████████░] 90%

Next: Plan 04-06 (ARCH-LM + Jarque-Bera) to complete ANOM family at 11/11, then Plan 04-11 (Phase 4 sign-off — goldens + engle_granger adf_step reconciliation + registry test tightening).

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
| Phase 04 P05 | ~45min | 3 tasks | 13 files (12 created, 1 modified) |

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
- Plan 04-05: ADF AIC lag selection uses sequential summation (Pitfall 4 — explicit `for k in 0..=max_lag` loop, NO rayon par_iter; determinism over throughput). Pinned by `adf_aic_lag_selection_deterministic_seq_summation` test running the scan 5x and verifying identical lag selection.
- Plan 04-05: ADF uses nalgebra DMatrix (heap-allocated, runtime-variable dimensions) NOT SMatrix as the plan literally specified — SMatrix requires compile-time-fixed COLS, incompatible with runtime-variable lag count. The heap allocation is bounded (≤ max_lag+4 columns, dozens) and runs once per regression. KPSS uses Matrix2 (compile-time fixed 2x2) for the 2-parameter regression='ct' detrend.
- Plan 04-05: ADF MacKinnon p-value uses a DOCUMENTED SIMPLIFICATION (accepted T-04-05-04 disposition): linear interpolation between tabulated 1%/5%/10% crits + asymptotic-normal tail damping via statrs::Normal. Accuracy ≈ 1e-3; sufficient for accept/reject at standard α. Plan 04-11 reconciles against the full MacKinnon (1996) response surface if golden parity within 1e-8 requires.
- Plan 04-05: KPSS auto-lag truncation formula `int(4 * (n/100)^(1/4))` per statsmodels default; p-value linear-interpolation BOUNDED at [0.01, 0.10] per statsmodels convention.
- Plan 04-05: VR effect.value = VR at max(k_values); effect.p_value is None; four parallel arrays {k_values, vr_values, z_stats, p_values} in effect.extra. Sequential k loop (Pitfall 4).
- Plan 04-05: Engle-Granger local adf_step (Plan 04-08) UNTOUCHED — Plan 04-11 owns reconciliation against the canonical scan::anom::adf::kernel::adfuller.

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

Last session: 2026-05-20T12:23:17.000Z
Stopped at: Plan 04-05 complete — ANOM-05/06/07 hand-derived stationarity scans shipped (ANOM family 9/11)
Resume file: None
Next action: Execute Plan 04-06 (ARCH-LM/Jarque-Bera) to complete ANOM family at 11/11, then Plan 04-11 (Phase sign-off — goldens + engle_granger adf_step reconciliation + registry test tightening).
