---
phase: 4
slug: scan-catalogue-anom-cross-seas
status: draft
nyquist_compliant: true
wave_0_complete: false
created: 2026-05-19
updated: 2026-05-19
---

# Phase 4 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.
> Source: RESEARCH.md §Validation Architecture.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | `cargo test` (Rust 2024) + `cargo nextest` for fast feedback |
| **Config file** | `Cargo.toml` workspace + per-crate `[dev-dependencies]` |
| **Quick run command** | `cargo nextest run -p miner-core --no-fail-fast` |
| **Full suite command** | `cargo test --workspace --all-features` |
| **Estimated runtime** | ~30s quick, ~90s full (per Phase 3 baseline) |

---

## Sampling Rate

- **After every task commit:** Run `cargo nextest run -p miner-core <test_filter>` (scoped to changed scan module)
- **After every plan wave:** Run `cargo test --workspace`
- **Before `/gsd:verify-work`:** Full suite green + `cargo run -p xtask -- gen-schema` produces no schema-version bump (or documented additive diff)
- **Max feedback latency:** ~30s

---

## Per-Task Verification Map

> Populated by planner. Each task lists: task ID, plan, wave, requirement(s), test type, automated verify command, fixture/golden references. Each row corresponds to one `<automated>` block from a task in Plans 04-01..04-11.
> Test type taxonomy: **unit** (cargo test --lib), **integration** (cargo test --test), **proptest** (proptest cases inside an integration file), **golden** (statsmodels/scipy/pandas reference cross-check), **scaffolding** (creates files/dirs/configs verified by `test -f` or `wc -l`), **checkpoint** (human-verify task).

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | Status |
|---------|------|------|-------------|-----------|-------------------|--------|
| 04-01-T1 (schemars regen spike + workspace deps + SCHEMA-DIFF memo) | 04-01 | 1 | ANOM-01, CROSS-01 (facade prerequisite) | unit + scaffolding | `cargo build -p miner-core && cargo tree -p miner-core 2>&1 \| grep -cE 'tokio\|async-std\|smol' \| grep -q '^0$' && test -f .planning/phases/04-scan-catalogue-anom-cross-seas/04-01-SCHEMA-DIFF.md && grep -q 'D4-03' .planning/phases/04-scan-catalogue-anom-cross-seas/04-01-SCHEMA-DIFF.md && grep -q 'D4-01' .planning/phases/04-scan-catalogue-anom-cross-seas/04-01-SCHEMA-DIFF.md` | ⬜ pending |
| 04-01-T2 (Scan trait arity + ScanArity enum + InstrumentSpec + Vec generalisation) | 04-01 | 1 | ANOM-01, CROSS-01 (facade prerequisite) | unit | `cargo build -p miner-core --all-targets && cargo test -p miner-core --lib scan::tests::scan_arity_serialises_snake_case scan::tests::instrument_spec_parse_round_trip scan::tests::scan_request_instruments_len_one_serialises findings::tests::data_slice_sources_vec_round_trip scan::tests::ljung_box_scan_reports_single_arity 2>&1 \| grep -v '^#' \| grep -c 'test result: ok' \| grep -q '^[1-9]'` | ⬜ pending |
| 04-01-T3 (PreflightCode::WrongInstrumentArity + schema regen idempotent gate) | 04-01 | 1 | ANOM-01, CROSS-01 (facade prerequisite) | unit + integration (schema-sync) | `cargo test -p miner-core --lib error::codes::tests && cargo run -p xtask -- gen-schema >/dev/null 2>&1 && cargo run -p xtask -- gen-schema >/dev/null 2>&1 && git diff --exit-code schemas/ && grep -q '"wrong_instrument_arity"' schemas/findings-v1.schema.json` | ⬜ pending |
| 04-02-T1a (primitives returns + time_alignment + raw_array + LjungBox refactor) | 04-02 | 2 | ANOM-01, CROSS-01 | unit + integration (Phase 3 golden) | `cargo build -p miner-core --all-targets && cargo test -p miner-core --lib scan::primitives && cargo test -p miner-core --test scan_ljung_box && cargo clippy -p miner-core --all-targets -- -D warnings` | ⬜ pending |
| 04-02-T1b (anom/cross/seas namespace stubs + registry.rs wiring + goldens scaffolding) | 04-02 | 2 | ANOM-01, CROSS-01 | unit + scaffolding | `cargo build -p miner-core --all-targets && cargo test -p miner-core --lib scan::anom scan::cross scan::seas && cargo test -p miner-core --test scan_ljung_box && test -f crates/miner-core/tests/goldens/REFERENCE-VERSIONS.md && grep -q 'statsmodels==0.14.6' crates/miner-core/tests/goldens/REFERENCE-VERSIONS.md && test -f crates/miner-core/tests/goldens/python-requirements.lock` | ⬜ pending |
| 04-02-T2 (engine validate_arity + dispatch_pair + ScanCtx bars_pair/bars_up_to + test scaffolds) | 04-02 | 2 | ANOM-01, CROSS-01 | unit + integration | `cargo build -p miner-core --all-targets && cargo test -p miner-core --lib engine::preflight::tests scan::tests::scan_ctx_bars_up_to && cargo test -p miner-core --test arity_preflight --test two_leg_facade --test gap_intersect_cross --test scan_ljung_box --test gap_policy --test dry_run --test scan_facade_determinism` | ⬜ pending |
| 04-02-T3 (CLI repeatable --instrument parser + scans catalogue arity field) | 04-02 | 2 | ANOM-01, CROSS-01 | unit (CLI) + integration | `cargo build -p miner-cli && cargo test -p miner-cli --lib scan_args::tests && cargo run -p miner-cli --quiet -- scans 2>/dev/null \| head -1 \| grep -q '"arity"'` | ⬜ pending |
| 04-03-T1 (ANOM-01 stats.returns.profile@1) | 04-03 | 3 | ANOM-01 | unit + integration | `cargo build -p miner-core --all-targets && cargo test -p miner-core --lib scan::anom::returns::tests && cargo test -p miner-core --test scan_returns_profile --test scan_ljung_box && cargo run -p miner-cli --quiet -- scans 2>/dev/null \| grep -q '"id":"stats.returns.profile"'` | ⬜ pending |
| 04-03-T2 (ANOM-02 stats.summary.welford@1) | 04-03 | 3 | ANOM-02 | unit + integration | `cargo build -p miner-core --all-targets && cargo test -p miner-core --lib scan::anom::summary::tests && cargo test -p miner-core --test scan_summary_welford --test scan_returns_profile --test scan_ljung_box && cargo run -p miner-cli --quiet -- scans 2>/dev/null \| grep -q '"id":"stats.summary.welford"'` | ⬜ pending |
| 04-03-T3 (ANOM-03 stats.vol.rolling@1 + vol_rolling_shuffled_future_invariant proptest) | 04-03 | 3 | ANOM-03 | unit + integration + proptest | `cargo build -p miner-core --all-targets && cargo test -p miner-core --lib scan::anom::vol::tests && cargo test -p miner-core --test scan_vol_rolling --test shuffled_future_regression --test scan_returns_profile --test scan_summary_welford --test scan_ljung_box && cargo run -p miner-cli --quiet -- scans 2>/dev/null \| grep -q '"id":"stats.vol.rolling"'` | ⬜ pending |
| 04-04-T1 (ANOM-04 stats.autocorr.ljung_box_sq@1) | 04-04 | 4 | ANOM-04 | unit + integration | `cargo build -p miner-core --all-targets && cargo test -p miner-core --lib scan::anom::ljung_box_sq::tests && cargo test -p miner-core --test scan_ljung_box_squared --test scan_ljung_box && cargo run -p miner-cli --quiet -- scans 2>/dev/null \| grep -q '"id":"stats.autocorr.ljung_box_sq"'` | ⬜ pending |
| 04-04-T2 (ANOM-10 stats.outliers.z_and_mad@1) | 04-04 | 4 | ANOM-10 | unit + integration | `cargo build -p miner-core --all-targets && cargo test -p miner-core --lib scan::anom::outliers::tests && cargo test -p miner-core --test scan_outliers && cargo run -p miner-cli --quiet -- scans 2>/dev/null \| grep -q '"id":"stats.outliers.z_and_mad"'` | ⬜ pending |
| 04-04-T3 (ANOM-11 stats.drawdown.profile@1) | 04-04 | 4 | ANOM-11 | unit + integration | `cargo build -p miner-core --all-targets && cargo test -p miner-core --lib scan::anom::drawdown::tests && cargo test -p miner-core --test scan_drawdown --test scan_outliers --test scan_ljung_box_squared --test scan_ljung_box && cargo run -p miner-cli --quiet -- scans 2>/dev/null \| grep -q '"id":"stats.drawdown.profile"'` | ⬜ pending |
| 04-05-T1 (ANOM-05 stats.stationarity.adf@1) | 04-05 | 5 | ANOM-05 | unit + integration | `cargo build -p miner-core --all-targets && cargo test -p miner-core --lib scan::anom::adf::tests && cargo test -p miner-core --test scan_adf --test scan_ljung_box && cargo run -p miner-cli --quiet -- scans 2>/dev/null \| grep -q '"id":"stats.stationarity.adf"'` | ⬜ pending |
| 04-05-T2 (ANOM-06 stats.stationarity.kpss@1) | 04-05 | 5 | ANOM-06 | unit + integration | `cargo build -p miner-core --all-targets && cargo test -p miner-core --lib scan::anom::kpss::tests && cargo test -p miner-core --test scan_kpss --test scan_adf && cargo run -p miner-cli --quiet -- scans 2>/dev/null \| grep -q '"id":"stats.stationarity.kpss"'` | ⬜ pending |
| 04-05-T3 (ANOM-07 stats.variance_ratio.lo_mackinlay@1) | 04-05 | 5 | ANOM-07 | unit + integration | `cargo build -p miner-core --all-targets && cargo test -p miner-core --lib scan::anom::variance_ratio::tests && cargo test -p miner-core --test scan_variance_ratio --test scan_kpss --test scan_adf --test scan_ljung_box && cargo run -p miner-cli --quiet -- scans 2>/dev/null \| grep -q '"id":"stats.variance_ratio.lo_mackinlay"'` | ⬜ pending |
| 04-06-T1 (ANOM-08 stats.heteroskedasticity.arch_lm@1) | 04-06 | 6 | ANOM-08 | unit + integration | `cargo build -p miner-core --all-targets && cargo test -p miner-core --lib scan::anom::arch_lm::tests && cargo test -p miner-core --test scan_arch_lm --test scan_ljung_box && cargo run -p miner-cli --quiet -- scans 2>/dev/null \| grep -q '"id":"stats.heteroskedasticity.arch_lm"'` | ⬜ pending |
| 04-06-T2 (ANOM-09 stats.normality.jarque_bera@1 + ANOM family count assertion) | 04-06 | 6 | ANOM-09 | unit + integration | `cargo build -p miner-core --all-targets && cargo test -p miner-core --lib scan::anom::jarque_bera::tests && cargo test -p miner-core --test scan_jarque_bera --test scan_arch_lm --test scan_ljung_box && cargo run -p miner-cli --quiet -- scans 2>/dev/null \| grep -c '"family":"stats"' \| awk '{ if ($1 >= 11) exit 0; else exit 1 }'` | ⬜ pending |
| 04-07-T1 (CROSS-02 cross.corr.pearson_rolling@1 + cross.corr.spearman_rolling@1) | 04-07 | 3 | CROSS-02 | unit + integration | `cargo build -p miner-core --all-targets && cargo test -p miner-core --lib scan::cross::corr_rolling::tests && cargo test -p miner-core --test scan_corr_rolling --test arity_preflight --test two_leg_facade --test scan_ljung_box --test shuffled_future_regression && cargo run -p miner-cli --quiet -- scans 2>/dev/null \| grep -c '"id":"cross.corr.' \| grep -q '^2$'` | ⬜ pending |
| 04-07-T2 (CROSS-03 cross.ols.rolling@1) | 04-07 | 3 | CROSS-03 | unit + integration | `cargo build -p miner-core --all-targets && cargo test -p miner-core --lib scan::cross::ols_rolling::tests && cargo test -p miner-core --test scan_ols_rolling --test scan_corr_rolling --test arity_preflight --test two_leg_facade --test scan_ljung_box --test shuffled_future_regression && cargo run -p miner-cli --quiet -- scans 2>/dev/null \| grep -q '"id":"cross.ols.rolling"'` | ⬜ pending |
| 04-08-T1 (CROSS-04 cross.lead_lag.ccf@1 + FOUR shuffled-future proptests: lead_lag, pearson_rolling, spearman_rolling, ols_rolling) | 04-08 | 4 | CROSS-04 | unit + integration + proptest (4 fns) | `cargo build -p miner-core --all-targets && cargo test -p miner-core --lib scan::cross::lead_lag::tests && cargo test -p miner-core --test scan_lead_lag --test shuffled_future_regression --test scan_corr_rolling --test scan_ols_rolling --test scan_ljung_box && cargo run -p miner-cli --quiet -- scans 2>/dev/null \| grep -q '"id":"cross.lead_lag.ccf"'` | ⬜ pending |
| 04-08-T2 (CROSS-05 cross.cointegration.engle_granger@1 + D4-09 statsmodels.coint citation + max_lag rationale in SUMMARY) | 04-08 | 4 | CROSS-05 | unit + integration | `cargo build -p miner-core --all-targets && cargo test -p miner-core --lib scan::cross::engle_granger::tests && cargo test -p miner-core --test scan_engle_granger --test scan_lead_lag --test scan_corr_rolling --test scan_ols_rolling --test scan_adf --test scan_ljung_box && cargo run -p miner-cli --quiet -- scans 2>/dev/null \| grep -c '"id":"cross\.' \| grep -q '^4$'` | ⬜ pending |
| 04-09-T1 (SEAS-01 seas.bucket.hour_of_day@1 + bucketing helper) | 04-09 | 3 | SEAS-01 | unit + integration | `cargo build -p miner-core --all-targets && cargo test -p miner-core --lib scan::seas::hour_of_day::tests scan::seas::bucketing::tests && cargo test -p miner-core --test scan_seas_hour_of_day --test scan_ljung_box && cargo run -p miner-cli --quiet -- scans 2>/dev/null \| grep -q '"id":"seas.bucket.hour_of_day"'` | ⬜ pending |
| 04-09-T2 (SEAS-02 seas.bucket.day_of_week@1) | 04-09 | 3 | SEAS-02 | unit + integration | `cargo build -p miner-core --all-targets && cargo test -p miner-core --lib scan::seas::day_of_week::tests && cargo test -p miner-core --test scan_seas_day_of_week --test scan_seas_hour_of_day && cargo run -p miner-cli --quiet -- scans 2>/dev/null \| grep -q '"id":"seas.bucket.day_of_week"'` | ⬜ pending |
| 04-09-T3 (SEAS-03 seas.bucket.session@1) | 04-09 | 3 | SEAS-03 | unit + integration | `cargo build -p miner-core --all-targets && cargo test -p miner-core --lib scan::seas::session::tests && cargo test -p miner-core --test scan_seas_session --test scan_seas_hour_of_day --test scan_seas_day_of_week && cargo run -p miner-cli --quiet -- scans 2>/dev/null \| grep -q '"id":"seas.bucket.session"'` | ⬜ pending |
| 04-10-T1 (SEAS-04 seas.bucket.eom_som@1) | 04-10 | 4 | SEAS-04 | unit + integration | `cargo build -p miner-core --all-targets && cargo test -p miner-core --lib scan::seas::eom_som::tests && cargo test -p miner-core --test scan_seas_eom_som --test scan_seas_session && cargo run -p miner-cli --quiet -- scans 2>/dev/null \| grep -q '"id":"seas.bucket.eom_som"'` | ⬜ pending |
| 04-10-T2 (SEAS-05 seas.test.anova_kruskal@1) | 04-10 | 4 | SEAS-05 | unit + integration | `cargo build -p miner-core --all-targets && cargo test -p miner-core --lib scan::seas::anova_kw::tests && cargo test -p miner-core --test scan_seas_anova_kruskal --test scan_seas_eom_som && cargo run -p miner-cli --quiet -- scans 2>/dev/null \| grep -q '"id":"seas.test.anova_kruskal"'` | ⬜ pending |
| 04-10-T3 (SEAS-06 seas.event.pre_post_window@1 + 23-scan catalogue assertion) | 04-10 | 4 | SEAS-06 | unit + integration | `cargo build -p miner-core --all-targets && cargo test -p miner-core --lib scan::seas::event_window::tests && cargo test -p miner-core --test scan_seas_event_window --test scan_seas_anova_kruskal --test scan_seas_eom_som --test scan_seas_session --test scan_seas_day_of_week --test scan_seas_hour_of_day && cargo run -p miner-cli --quiet -- scans 2>/dev/null \| grep -c '^{' \| awk '$1 >= 23 { exit 0 } { exit 1 }'` | ⬜ pending |
| 04-11-T1 (three goldens + Python generators + golden cross-check integration tests) | 04-11 | 7 | SC#5 (golden fixtures) | golden + integration | `test -f crates/miner-core/tests/goldens/stats.summary.welford.jsonl && test -f crates/miner-core/tests/goldens/cross.cointegration.engle_granger.jsonl && test -f crates/miner-core/tests/goldens/seas.bucket.hour_of_day.jsonl && grep -q '0.14.6\|1.14.1' crates/miner-core/tests/goldens/stats.summary.welford.jsonl && cargo test -p miner-core --test scan_ljung_box --test scan_summary_welford --test scan_engle_granger --test scan_seas_hour_of_day` | ⬜ pending |
| 04-11-T2 (schema regen idempotent + byte_identical_rerun + README Quickstart + Phase 4 sign-off memo) | 04-11 | 7 | SC#4 (envelope shape) | integration + scaffolding | `cargo run -p xtask -- gen-schema && cargo run -p xtask -- gen-schema && git diff --exit-code schemas/ && cargo test -p miner-core --test byte_identical_rerun && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && test -f .planning/phases/04-scan-catalogue-anom-cross-seas/04-11-PHASE-VERIFICATION.md` | ⬜ pending |
| 04-11-T3 (human verification — README Quickstart + golden tolerances + 22-requirement sign-off) | 04-11 | 7 | SC#4, SC#5 | checkpoint | human-check (gate=blocking; resume-signal "approved — Phase 4 ready for /gsd-verify-work") | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

**Coverage summary:** 30 task rows · 22 requirement IDs covered (ANOM-01..11, CROSS-01..05, SEAS-01..06) + 2 ROADMAP success criteria (SC#4, SC#5) attached to 04-11 · 1 checkpoint task · zero auto/tdd tasks missing an `<automated>` block (Nyquist compliant).

**Sampling continuity:** No 3 consecutive tasks lack an automated verify — every `auto` and `tdd` task in 04-01..04-10 has an `<automated>` block; 04-11 mixes auto + checkpoint.

---

## Wave 0 Requirements

- [ ] `crates/miner-core/tests/goldens/` directory created (Plan 04-02 Task 1b)
- [ ] `crates/miner-core/tests/goldens/REFERENCE-VERSIONS.md` pins statsmodels + scipy versions (Plan 04-02 Task 1b)
- [ ] `crates/miner-core/tests/goldens/python-requirements.lock` committed (placeholder OR populated) (Plan 04-02 Task 1b; Plan 04-11 Task 1 finalises)
- [ ] `scripts/gen-goldens.py` (or equivalent generators per scan) committed for reproducible golden regeneration (Plan 04-11 Task 1 — generate_summary_welford.py, generate_engle_granger.py, generate_hour_of_day.py)
- [ ] `crates/miner-core/Cargo.toml` adds `ndarray`, `ndarray-stats`, `nalgebra` per RESEARCH.md §Dependency-add audit (Plan 04-01 Task 1)
- [ ] Per-family namespace stubs `scan/anom/mod.rs`, `scan/cross/mod.rs`, `scan/seas/mod.rs` with `register_<family>_scans` helpers (Plan 04-02 Task 1b) — locks the contract that Plans 04-03..04-10 only append to these helpers (registry.rs untouched)

---

## Sampling Dimensions (from RESEARCH.md §Validation Architecture)

### Coverage
Each of the 22 scans gets:
- One happy-path integration test against deterministic synthetic data
- One checked-in statsmodels/scipy golden where a Python reference exists (3 of the 22 are formally pinned in Plan 04-11; the other 19 inherit determinism via the same envelope-construction code path and the byte-identical-rerun test in 04-11)
- Goldens stored at `crates/miner-core/tests/goldens/<scan_id>.jsonl`
- Reference versions pinned in `crates/miner-core/tests/REFERENCE-VERSIONS.md`

### Edge
- `N=0` input → `Finding::ScanError` with `InsufficientData` code
- `N=1` input → most scans error; summary-stats scan emits trivial finding
- All-zero returns → variance-zero handling for normalised stats (correlations, t-stats)
- All-NaN input → must `Finding::ScanError`, NOT propagate NaN into envelope
- Single timestamp gap mid-window (rolling scans)
- CROSS legs with zero overlap → `Finding::GapAborted` (strict) or `InsufficientData` (continuous_only)

### Adversarial
- **Shuffled-future regression** (D3-09 pattern from Phase 3, extended to every rolling/causal scan):
  - Plan 04-03 Task 3: `vol_rolling_shuffled_future_invariant` (ANOM-03) — Wave 3
  - Plan 04-08 Task 1: `lead_lag_shuffled_future_invariant` (CROSS-04), `pearson_rolling_shuffled_future_invariant` (CROSS-02), `spearman_rolling_shuffled_future_invariant` (CROSS-02), `ols_rolling_shuffled_future_invariant` (CROSS-03) — Wave 4 (deferred from Plan 04-07 Wave 3 to avoid same-wave file-write conflict with Plan 04-03)
- **Zero-variance leg in CROSS** → correlation/OLS undefined; must `Finding::ScanError`, not emit NaN.
- **Cointegrating residual with near-zero half-life** (CROSS-05 numerical degenerate case for OU fit).
- **Trading-session boundary edges** (SEAS-03): bars exactly at session-boundary timestamp must bucket deterministically.

### Cross-validation
- **Byte-identical re-run** (D3-23, every scan) — modulo `run_id` + clock-read fields. Pinned for representative ANOM/CROSS/SEAS in Plan 04-11 Task 2 via `tests/byte_identical_rerun.rs` (ROADMAP Phase 4 SC#4 evidence).
- **CLI/MCP/HTTP parity** (Phase 6 will enforce). Phase 4 emits findings whose only run-id/clock-read fields differ across surfaces; the rest must be byte-identical.
- **Schema regen + diff check** (per D4-01/D4-03). `cargo run -p xtask -- gen-schema` produces a diff that is classified as additive (or fallback to D4-03-ALT documented). Idempotent gate: Plan 04-01 Task 3 + Plan 04-11 Task 2.

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Quickstart README example produces expected JSONL | Success Criterion #4 | Touches doc consistency; integration covers stat correctness | Follow README quickstart for one ANOM/CROSS/SEAS scan; eyeball envelope shape consistency. Pinned by Plan 04-11 Task 3 (human-verify checkpoint). |

---

## Validation Sign-Off

- [x] All 22 scan tasks have `<automated>` verify or Wave 0 dependencies (28 auto/tdd tasks across 04-01..04-11 + 1 checkpoint; Nyquist compliant)
- [x] Sampling continuity: no 3 consecutive tasks without automated verify (every auto/tdd task carries an `<automated>` block; the checkpoint in 04-11 is the only exception, and it follows two consecutive auto tasks)
- [x] Wave 0 covers goldens directory + REFERENCE-VERSIONS.md + dep additions (Plan 04-01 Task 1 + Plan 04-02 Task 1b)
- [x] No watch-mode flags (all commands are one-shot `cargo test` / `cargo build` / `cargo run`)
- [x] Feedback latency < 30s for quick, < 90s for full (per Phase 3 baseline; no longer-running tests added)
- [x] `nyquist_compliant: true` set in frontmatter — per-task verification map populated above

**Approval:** ready for executor consumption (planner-side checklist complete).
