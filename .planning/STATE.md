---
gsd_state_version: 1.0
milestone: v1.0
milestone_name: milestone
status: ready_to_plan
stopped_at: Phase 5 context gathered
last_updated: "2026-05-20T20:26:20.170Z"
last_activity: 2026-05-20 -- Phase 05 execution started
progress:
  total_phases: 7
  completed_phases: 5
  total_plans: 38
  completed_plans: 33
  percent: 71
---

# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-05-15)

**Core value:** Surface raw statistical candidates from years of OHLCV data fast enough that a quant agent can run wide sweeps and targeted queries interactively, without waiting on Python data pipelines.
**Current focus:** Phase 05 — statistical-hygiene-sweep-runner

## Current Position

Phase: 6
Plan: Not started
Status: Ready to plan
Last activity: 2026-05-21

Progress: [██████████] 100%

Next: Phase 5 (Statistical Hygiene & Sweep Runner) — effect sizes, bootstrap, phase-scramble nulls, BH-FDR, sweep manifest. Begin with `/gsd-discuss-phase 5` or `/gsd-plan-phase 5`.

## Performance Metrics

**Velocity:**

- Total plans completed: 18
- Average duration: -
- Total execution time: -

**By Phase:**

| Phase | Plans | Total | Avg/Plan |
|-------|-------|-------|----------|
| 01 | 7 | - | - |
| 02 | 6 | - | - |
| 03 | 7 | - | - |
| 05 | 5 | - | - |

**Recent Trend:**

- Last 5 plans: 03-03, 03-04, 03-05, 03-06, 03-07
- Trend: -

*Updated after each plan completion*
| Phase 04 P04 | 38 | 3 tasks | 14 files |
| Phase 04 P10 | ~45min | 3 tasks | 13 files (12 created, 1 modified) |
| Phase 04 P05 | ~45min | 3 tasks | 13 files (12 created, 1 modified) |
| Phase 04 P06 | 16min | 2 tasks | 8 files |
| Phase 04 P11 | ~45 min | 2 tasks | 14 files |
| Phase 04 P04-12 | ~40min | 3 tasks | 9 files |

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
- [Phase 04]: Plan 04-06: ANOM-08 ARCH-LM uses nalgebra DMatrix (heap, runtime-variable L+1 columns) NOT SMatrix — same pattern as Plan 04-05 ADF. Constant-u-squared early return guards against singular X'X for alternating-sign returns; R-squared clamped to [0,1] for F-stat denominator.
- [Phase 04]: Plan 04-06: ANOM-09 Jarque-Bera REUSES welford_pass from anom::summary::kernel — visibility bumped pub(super) -> pub(in crate::scan::anom) for sibling-submodule access. Moments byte-identical with ANOM-02 (pinned by to_bits()-equality test); JB formula = (n/6)*(S^2 + K^2/4) with statrs ChiSquared(2).
- [Phase 04]: Plan 04-06: Full statsmodels/scipy golden parity for ARCH-LM + JB deferred to Plan 04-11. This plan ships hand-derived closed-form kernel tests within 1e-10 (statistic) + 1e-12 (p-value via statrs). Sanity tests use synthetic regime-switching ARCH(0.99) (n=1000) + exp-squared-skewed inputs (n=500).
- [Phase 04]: Plan 04-06: ANOM family complete (11/11). All implementation Plans 04-03..04-10 shipped (11 ANOM + 4 CROSS + 6 SEAS). Plan 04-11 owns goldens, engle_granger adf_step reconciliation, and registry test tightening from >= 1 to exact final count.
- [Phase ?]: Plan 04-11: Stub-fixture fallback for Phase 4 goldens (Python 3.14 vs pinned 3.11 scipy/statsmodels); #[ignore]d cross-check tests behind provenance gate.
- [Phase ?]: Plan 04-11: ADF reconciliation kept local for Engle-Granger v1; canonical anom::adf re-route deferred to Phase 5 / HYG-01 alongside bootstrap CIs.
- [Phase ?]: Plan 04-11: cargo clippy -D warnings workspace cleanup deferred to Phase 7 hardening; only 3 in-scope LN_2 lints in drawdown/kernel.rs fixed. **AMENDED by Plan 04-13 (2026-05-20):** deferral reversed for the entire workspace (miner-core lib + tests + miner-cli) — all clippy::pedantic errors resolved, CI gate 2 now green. Phase 7 retains the deny-warnings audit responsibility for any NEW code added in Phases 5–6 + `cargo deny` / `cargo audit` sweeps.
- [Phase ?]: Plan 04-12: CR-01 (Pair-arity engine dispatch) closed — engine::run_one_with_registry now branches on scan.arity() and routes Pair scans through dispatch_pair_arity_body (wraps the previously-orphaned engine::gap_policy::dispatch_pair). Coverage tightened: arity_preflight + byte_identical_rerun + 4 CROSS integration tests now drive the engine path (9 separate tests trip a future regression).
- [Phase ?]: Plan 04-13 (2026-05-20): CI Gate 2 (cargo clippy --workspace --all-targets -- -D warnings) GREEN for the first time since Phase 4 implementation began. All clippy::pedantic errors resolved across miner-core lib + tests + miner-cli (88 lib-only inventory expanded to ~200 once lib compiled cleanly). Atomic-per-category commit discipline preserved (7 commits, 1 chore follow-up). `#[allow(..., reason = "...")]` for 5 intentional patterns (closed-form regression bodies, sample-size casts, CLI-bounded indices, canonical statistical notation, internal-facade pass-by-value convention). Crate-level `#![cfg_attr(test, allow(...))]` in lib.rs for test-fixture patterns (float_cmp on goldens, cast_* on synthetic OHLCV generators, etc.). Per-integration-test-file `#![allow(...)]` blocks. Plan 04-11's "deferred to Phase 7" decision reversed — Phase 7 retains only the deny-warnings audit for NEW code in Phases 5-6 + cargo deny / cargo audit sweeps.

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

Last session: 2026-05-20T19:04:24.506Z
Stopped at: Phase 5 context gathered
Resume file: .planning/phases/05-statistical-hygiene-sweep-runner/05-CONTEXT.md
Next action: Begin Phase 5 (Statistical Hygiene & Sweep Runner) via `/gsd-discuss-phase 5`. The Phase 5 plan in ROADMAP.md owns OP-04 (TOML sweep manifest fanout) + HYG-01..05 (effect sizes, BH-FDR, block bootstrap, phase-scrambled nulls, bit-for-bit reproducible RNG).
