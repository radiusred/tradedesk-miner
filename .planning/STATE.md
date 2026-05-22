---
gsd_state_version: 1.0
milestone: v1.0
milestone_name: milestone
status: completed
stopped_at: Plan 07-09 (locked findings-envelope snapshot test) complete; envelope_snapshot.jsonl golden + 3 active byte-determinism tests shipped
last_updated: "2026-05-22T11:47:26.611Z"
last_activity: 2026-05-22
progress:
  total_phases: 7
  completed_phases: 7
  total_plans: 50
  completed_plans: 50
  percent: 100
---

# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-05-15)

**Core value:** Surface raw statistical candidates from years of OHLCV data fast enough that a quant agent can run wide sweeps and targeted queries interactively, without waiting on Python data pipelines.
**Current focus:** Phase 07 — hardening-benchmarks-reproducibility

## Current Position

Phase: 07
Plan: Not started
Status: Milestone complete
Last activity: 2026-05-22

Progress: [██████████] 100%

Next: Phase 7 closes the v1.0 milestone. The bench harness, fixture-cache regenerator, CHANGELOG, IAAFT closure, cargo-audit/deny gates, criterion microbenches, data_sources doc, recipe runner + dhat/hyperfine wrappers, and envelope snapshot golden are all shipped. First post-merge follow-up: a `chore(07): refresh bench numbers as of <sha>` PR populates the TBD cells in docs/bench-results.md from a reference workstation.

## Performance Metrics

**Velocity:**

- Total plans completed: 30
- Average duration: -
- Total execution time: -

**By Phase:**

| Phase | Plans | Total | Avg/Plan |
|-------|-------|-------|----------|
| 01 | 7 | - | - |
| 02 | 6 | - | - |
| 03 | 7 | - | - |
| 05 | 5 | - | - |
| 06 | 3 | - | - |
| 07 | 9 | - | - |

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
| Phase 06 P01 | ~8min | 2 tasks | 6 files |
| Phase 6 P2 | 12min | 3 tasks | 3 files |
| Phase 06 P03 | 25min | 3 tasks | 7 files |
| Phase 07 P03 | ~7min | 3 tasks tasks | 4 files files |
| Phase 07 P07 | ~12min | 2 tasks | 2 files |
| Phase 07 P09 | ~10min | - tasks | - files |
| Phase 07 P08 | 16min | 3 tasks tasks | 13 files files |

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
- [Phase ?]: Plan 06-01: D6-05 Pattern A applied — OP-02 + OP-03 moved fully into v2 PLAT-v2-07 + PLAT-v2-08; 3-column traceability table preserved (v2 doc pointer rides in Status cell, no schema change). 50 v1 requirements (was 52).
- [Phase ?]: Plan 06-01: ROADMAP Phase 6 reshaped from CODE to DOCS per D6-01 + Open Question #7 — Goal + 5 Success Criteria rewritten to describe the docs deliverable; rmcp Research-flag blockquote removed; Phase 7 plan-list pollution (3 orphan 06-0?-PLAN.md bullets) cleaned to TBD placeholder.
- [Phase ?]: Plan 06-01: License-footer URL form locked to bare URL (no markdown autolink) per D6-04 + Open Question #6 default (4-of-5 tradedesk sibling-repo majority). docs/.license-footer.md is the single source of truth (8 lines, byte-identical to ARCHITECTURE.md tail); Plans 06-02 + 06-03 paste it verbatim.
- [Phase ?]: Plan 06-01: ARCHITECTURE.md uses plain-text section labels (Overview / Data Flow (high level) / Sync core + async edges / Key design decisions) — NOT H2 headings — per the tradedesk sibling-repo pattern. Only the trailing License heading is H2. Replaces tradedesk's 'Live vs Backtest paths' section with miner's 'Sync core + async edges' (FOUND-04 / D-15 / D-19).
- [Phase ?]: D6-02-FOOTER: docs/.license-footer.md paste-verbatim is the single load-bearing constraint for v1 docs; diff-verified byte-identity is the acceptance gate
- [Phase ?]: D6-02-CATALOGUE: per-scan H3 block layout (5-10 lines each) over wide-table for scan_catalogue.md; matches indicator_guide.md depth without ballooning
- [Phase ?]: [Phase 6 Plan 03] Phase 6 docs-only deliverable complete: docs/agent_integration.md + docs/future_mcp_http.md + docs/examples/* + README ## Documentation section + D6-08 placeholder-main retargeting. Zero new deps; cargo tree -p miner-core still zero async-deps; rustfmt expanded both mains to 14 lines vs the plan's 12-line target.
- [Phase ?]: [Phase 6 Plan 03] 12 Open Questions dispositioned: #1 per-doc line counts hit; #2 per-scan compact-block applied (06-02); #3 + #4 example CI smoke-tests DEFERRED to Phase 7; #5 Pattern A applied (06-01); #6 bare URL footer (06-01); #7 success-criteria rewrite applied (06-01); #8 + #10 CONTRIBUTING.md DEFERRED; #9 doc-lint CI gate DEFERRED to Phase 7; #11 placeholder mains updated (D6-08); #12 SPDX one-liner applied.
- [Phase ?]: Plan 07-03: deny.toml uses cargo-deny 0.19.6+ v2 schema; the older [advisories] keys (vulnerability, unsound, notice, severity-threshold) from CONTEXT.md D7-05 are REMOVED in 0.14+ and MUST NOT be reintroduced (RESEARCH §Pitfall 6). All advisories now emit errors by default.
- [Phase ?]: Plan 07-03: cargo audit + cargo deny check land as CI-only gates via rustsec/audit-check@v2.0.0 and EmbarkStudios/cargo-deny-action@v2; major-version action refs match Phase 1's CI convention. SHA pinning is a separate hardening pass.
- [Phase ?]: Plan 07-03: D7-05 allowlist-by-exception is dual — license extensions land as a separate commit in deny.toml with inline '# allowed-for: <crate>@<version> — <license> — <reason>'; temporary advisory ignores land in [advisories] ignore with inline 'RUSTSEC-YYYY-NNNN — <reason> — review by YYYY-MM-DD'.
- [Phase ?]: Plan 07-03: local cargo-deny verification skipped per plan's explicit fallback. cargo-deny 0.19.6 requires rustc 1.88+ but workspace pins 1.85; cargo-deny 0.18.3 trips on pre-existing Plan 07-06 [[bench]] entries and on a CVSS 4.0 RUSTSEC entry. CI gate (cargo-deny-action@v2) is canonical.
- [Phase ?]: Plan 07-07 (D7-02): docs/data_sources.md uses '# Dukascopy data source caveats' title (source-specific) since the Reader trait is pluggable; README link target is file-level. Licensing-posture pin: tradedesk-dukascopy commit f218d41 (2026-05-13).
- [Phase ?]: Plan 07-09: Hand-rolled byte-equal envelope snapshot test landed at crates/miner-core/tests/findings_envelope_snapshot.rs + crates/miner-core/tests/goldens/envelope_snapshot.jsonl. NOT insta (per 07-RESEARCH Pitfall 8). Replicates miner emit-fixture in-process via BufferSink because assert_cmd is not in miner-core dev-deps and Cargo.toml is off-limits (Plan 07-06 concurrent worktree). Closes ROADMAP Phase 7 success criterion #1 together with Plans 07-01 + 07-05.
- [Phase ?]: Plan 07-08: D7-03 Layers 2 + 3 fully closed — miner-bench is now the recipe runner (replaces Phase 1 14-line placeholder); dhat 0.3.3 wired behind a miner-bench-only --features dhat Cargo gate (FOUND-04 preserved — miner-core stays dhat-free + tokio-free); benches/recipes/*.toml are plain SweepManifest TOML per RESEARCH Open Question 3 (no bench-wrapper type); scripts/run-bench.sh wraps hyperfine 1.20+, scripts/run-alloc-profile.sh wraps the dhat feature build.
- [Phase ?]: Plan 07-08: docs/bench-results.md is the single canonical home for perf numbers per D7-07 — README intentionally avoids embedded benchmark numbers. README ## Performance one-line pointer + CONTRIBUTING ## Profiling subsection (samply 0.13.1 recipe targets cross.cointegration.engle_granger@1 per RESEARCH Open Question 5). How-to-reproduce content lives in docs/bench-results.md ## How to reproduce per RESEARCH Open Question 4.

### Pending Todos

None yet.

### Blockers/Concerns

- **Phase 6 deferred (now docs-only):** design documented in docs/future_mcp_http.md; v2 owns the rmcp re-research + implementation (tracked as PLAT-v2-07 + PLAT-v2-08).
- **Phase 4 implementation risk:** ADF, KPSS, Engle-Granger, block bootstrap, BH-FDR, and DSR are not covered by any comprehensive Rust stats crate. Plan time for hand-rolled implementations validated against scipy/statsmodels golden outputs.

Resolved this phase:

- ~~Phase 2 open question: Arrow IPC vs bincode+zstd for derived-bar cache~~ → **Arrow IPC chosen**; locked under `crates/miner-core/src/cache.rs` with two-axis invalidation and tempfile-rename crash-safety.

## Deferred Items

Items acknowledged and carried forward from previous milestone close:

| Category | Item | Status | Deferred At |
|----------|------|--------|-------------|
| Operator surface | OP-02 (MCP) + OP-03 (HTTP) | Design documented v1 (docs/future_mcp_http.md); implementation deferred to v2 (PLAT-v2-07, PLAT-v2-08) | Phase 6 |
| Engle-Granger parity | engle_granger_matches_statsmodels_coint_golden | Test `#[ignore]`d; pre-existing kernel gap; HYG-01 owns reconciliation | Phase 7 |
| Clippy workspace gate | gen-fixtures.rs + hygiene_dispatch.rs lints | Pre-existing under `--all-targets -D warnings`; tracked in 07/deferred-items.md items 3-5 | Phase 7 |
| Context-question debt | 21 open research notes across 01-07 CONTEXT.md files | Implicitly resolved by shipped phase work; acknowledged at v1.0 close 2026-05-22 (no per-question follow-up required) | Milestone close |

## Session Continuity

Last session: 2026-05-22T11:01:16.944Z
Stopped at: Plan 07-09 (locked findings-envelope snapshot test) complete; envelope_snapshot.jsonl golden + 3 active byte-determinism tests shipped
Resume file: None
Next action: Begin Phase 5 (Statistical Hygiene & Sweep Runner) via `/gsd-discuss-phase 5`. The Phase 5 plan in ROADMAP.md owns OP-04 (TOML sweep manifest fanout) + HYG-01..05 (effect sizes, BH-FDR, block bootstrap, phase-scrambled nulls, bit-for-bit reproducible RNG).
