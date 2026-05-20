---
phase: 04-scan-catalogue-anom-cross-seas
plan: 07
subsystem: scan/cross

tags:
  - rust
  - scan
  - cross
  - pair-arity
  - vector-output
  - rolling

requires:
  - phase: 04-scan-catalogue-anom-cross-seas
    plan: 02
    provides: "scan::primitives::time_alignment::inner_join, scan::primitives::returns::log_returns, ScanCtx.bars_pair + bars_pair() accessor, ScanArity::Pair, register_cross_scans per-family helper, engine::preflight::validate_arity (D4-02), D4-03 sources Vec field"

provides:
  - "scan::cross::corr_rolling::{PearsonRollingScan, SpearmanRollingScan} — CROSS-02 rolling Pearson + Spearman correlation (Pair arity)"
  - "scan::cross::ols_rolling::OlsRollingScan — CROSS-03 rolling OLS regression with nalgebra DMatrix (Pair arity)"
  - "scan::cross::corr_rolling::kernel::{rolling_pearson, rolling_spearman, rank_with_ties} — per-window kernels with scipy 'average' tie convention"
  - "scan::cross::ols_rolling::kernel::{rolling_ols, OlsWindowResults} — per-window OLS via normal equations"
  - "Three new entries in `miner scans` catalogue (cross.corr.pearson_rolling@1, cross.corr.spearman_rolling@1, cross.ols.rolling@1), all with arity=pair"
  - "Integration tests: tests/scan_corr_rolling.rs (2 tests) + tests/scan_ols_rolling.rs (1 test)"

affects:
  - "04-08 (Wave 4 — CROSS-04 lead-lag + CROSS-05 cointegration): consumes the corr_rolling/ols_rolling module template + the same `register_cross_scans` helper"
  - "04-08 also authors shuffled-future proptest extensions for `cross.corr.pearson_rolling`, `cross.corr.spearman_rolling`, and `cross.ols.rolling` in a single Wave-4 modification of tests/shuffled_future_regression.rs (deferred from this plan to avoid Wave-3 file-write conflict with Plan 04-03's ANOM-03 proptest)"
  - "04-11 (Wave 7 — phase integration): registry::bootstrap() count assertion tightens to the full 23-scan count; tests/scan_corr_rolling.rs + tests/scan_ols_rolling.rs will gain insta envelope-shape snapshots"

tech-stack:
  added:
    - "(none) — Plan 04-01 added nalgebra; this plan only consumes it"
  patterns:
    - "Pattern A — Imports block + helper-lift (consumed `primitives::returns::log_returns` + `primitives::raw_array::f64_slice_to_raw_array`)"
    - "Pattern C — Two-leg CROSS scan body (ctx.bars_pair + inner_join + log_returns per leg + envelope construction with leg-labelled raw.series + D4-03 sources Vec len 2)"
    - "Pattern D — Vector-output finding (effect.value = last-window scalar; effect.extra carries `values` / per-window vectors)"
    - "Pattern E — Per-family registrar (3 `r.register(...)` lines appended to `register_cross_scans`; registry.rs::bootstrap untouched)"
    - "Pattern J — Per-scan integration test (direct Scan::run dispatch against deterministic seeded fixture)"

key-files:
  created:
    - "crates/miner-core/src/scan/cross/corr_rolling/mod.rs (~620 lines, 12 lib tests)"
    - "crates/miner-core/src/scan/cross/corr_rolling/kernel.rs (~315 lines, 12 kernel tests)"
    - "crates/miner-core/src/scan/cross/ols_rolling/mod.rs (~430 lines, 11 lib tests)"
    - "crates/miner-core/src/scan/cross/ols_rolling/kernel.rs (~245 lines, 5 kernel tests)"
    - "crates/miner-core/tests/scan_corr_rolling.rs (2 integration tests)"
    - "crates/miner-core/tests/scan_ols_rolling.rs (1 integration test)"
  modified:
    - "crates/miner-core/src/scan/cross/mod.rs — register three CROSS scans + module declarations + re-exports + replaced registry-noop test with positive register-includes-all-three test"
    - "crates/miner-core/src/scan/registry.rs — test count assertions widened to `>=` (NOT bootstrap() body — that's untouched per the per-family registrar contract)"
    - "crates/miner-cli/src/main.rs — handle_scans_subcommand catalogue test now searches by scan_id rather than assuming length 1"
    - "crates/miner-cli/tests/scans_catalogue.rs — integration test now iterates all catalogue lines + locates LjungBox by id rather than asserting length 1"

key-decisions:
  - "raw.series uses the canonical `timestamps_ms` key (D-03 invariant on Raw::new), not the plan's `timestamps_ms_aligned` literal — the raw block only carries aligned data so the suffix is unambiguous (Rule 3 deviation; documented in FINDING_SHAPE doc-comments)"
  - "Both Pearson and Spearman rolling scans share a single `corr_rolling` module with a `CorrKind` enum dispatching the kernel choice — keeps inner-join + windowing + envelope-construction code DRY without breaking the per-scan trait impl boundary"
  - "OLS regresses `returns_a ~ returns_b` (a as response, b as regressor) — so for `b = 2*a` in close-price space, log returns are equal (log returns are scale-invariant) so β = 1; for `y = a, x = 2*a` in arbitrary slices, β = 0.5 by definition"
  - "nalgebra::DMatrix used instead of SMatrix because window size is a runtime parameter; SMatrix requires const-generic rows. DMatrix is O(window) heap per call which is acceptable for typical windows (3..512). Documented in mod.rs doc-comment per CLAUDE.md 'small fixed sizes' note"
  - "rank_with_ties uses bitwise f64 equality (`to_bits()`) for tie detection — matches scipy.stats.spearmanr's behavior on equal floats"
  - "threshold_crossings is encoded as an f64 Vec (1.0 / 0.0) rather than a bool Vec — fits the single F64 Dtype RawArray wire form (findings/base64_bytes.rs:75 only declares F64 in v1)"
  - "Registry tests use `>=` count assertions (not `==`) — Rule 3 deviation so Plan 04-03..04-10 can extend without breaking this test; Plan 04-11 will tighten to the exact 23-count"

requirements-completed:
  - CROSS-02
  - CROSS-03

# Metrics
duration: 22min
completed: 2026-05-20
---

# Phase 04 Plan 07: CROSS Wave 3 Rolling Scans Summary

**Three new Pair-arity CROSS scans (Pearson rolling correlation, Spearman rolling correlation, OLS rolling regression) landed via the `register_cross_scans` per-family helper. Each scan inner-joins the two legs once via the CROSS-01 primitive, computes per-leg log returns, runs a per-window kernel, and emits exactly one `Finding::Result` envelope with vector arrays in `effect.extra` and leg-labelled keys in `raw.series` (D4-03). The engine's Pair branch in `run_one_with_registry` is NOT touched — the scans are validated end-to-end via direct `Scan::run` dispatch in integration tests; the full engine-side Pair dispatch is deferred to Plan 04-11.**

## Performance

- **Duration:** ~22 min
- **Started:** 2026-05-19T23:39:44Z
- **Completed:** 2026-05-20T00:02:07Z
- **Tasks:** 2 of 2 (all autonomous)
- **Files created:** 6
- **Files modified:** 4
- **Lines added (commit diff sum):** ~2,500
- **New tests:** 40 lib unit tests + 3 integration tests

## Accomplishments

### CROSS-02: rolling Pearson + Spearman correlation (Task 1, commit `ec5dc41`)

- **`scan::cross::corr_rolling`** module hosts BOTH `PearsonRollingScan` and `SpearmanRollingScan` (one module, two registered scans sharing the inner-join + windowing scaffold; only the per-window kernel call differs via a `CorrKind` enum).
- **Kernel `rolling_pearson`** — naive `O(n*window)` two-pass mean+covariance per window. Zero-variance window → `f64::NAN` (caller converts to `ScanError::Kernel`).
- **Kernel `rolling_spearman`** — per-window rank both legs via `rank_with_ties` (scipy `method = "average"` average-rank tie correction), then run Pearson on the ranks.
- **`rank_with_ties`** — bitwise-equal tie detection via `f64::to_bits()`, 1-indexed ranks averaged across tied groups.
- **`effect.value` = last-window correlation** (RESEARCH §1.3 trader-intuition canonical pick). `effect.extra` carries `{values, window_starts_ms, window_length, threshold_crossings, threshold}` with `threshold_crossings` as a 1.0/0.0 indicator vector.
- **Hand-derived kernel tests within 1e-12:** perfect linear → r=1.0, perfect negative → r=-1.0, hand-derived window=4 case → r=-1.0.
- **2 integration tests** drive both variants against a 64-bar two-leg LCG fixture via direct `Scan::run` dispatch (n=59 windows for window=5, n=58 for window=6).

### CROSS-03: rolling OLS regression (Task 2, commit `c89b3f6`)

- **`scan::cross::ols_rolling`** module computes per-window β, α, R², and residual_std via the normal equations using `nalgebra::DMatrix`.
- **`DMatrix` (not `SMatrix`)** — window size is a runtime parameter; SMatrix would require const-generic rows. DMatrix is O(window) heap per call which is acceptable for typical windows. Documented in mod.rs per CLAUDE.md "small fixed sizes" note.
- **`fit_window`** uses `try_inverse` (NOT `inverse`) so singular normal-equations matrices propagate as NaN (caller converts to `ScanError::Kernel`).
- **Convention pinned:** the scan regresses `returns_a ~ returns_b` (response = a, regressor = b). For `y = a, x = 2*a` in arbitrary slices, β = 0.5; for `b = 2*a` in close-price space, log returns are equal (scale invariance), so β = 1.
- **`effect.value` = last-window β** (RESEARCH §1.3 hedge-ratio canonical pick); `effect.extra` carries `{betas, alphas, r2s, residual_stds, window_starts_ms, window_length}`.
- **Hand-derived kernel tests within 1e-12:** identical inputs → β=1, α=0, R²=1, residual_std=0; y=a, x=2*a → β=0.5; y=3+2x → β=2, α=3, R²=1.
- **1 integration test** drives the OLS scan against a 64-bar two-leg LCG fixture (n=58 windows for window=6).

### Registration via per-family helper (registry.rs untouched)

All three scans register via `register_cross_scans` (Pattern E per-family registrar — Plan 04-02 contract). The `bootstrap()` body in `registry.rs` is UNTOUCHED — the only changes to `registry.rs` are test-count widenings in `mod tests` (Rule 3 deviation; Plan 04-11 will tighten to exact-23 count).

```rust
pub fn register_cross_scans(r: &mut Registry) {
    r.register(Box::new(PearsonRollingScan));
    r.register(Box::new(SpearmanRollingScan));
    r.register(Box::new(OlsRollingScan));
}
```

Catalogue verification: `MINER_CACHE_ROOT=/tmp/c MINER_BAR_CACHE_ROOT=/tmp/bc MINER_OUTPUT=stdout cargo run -p miner-cli --quiet -- scans` emits 4 lines total (LjungBox + 3 CROSS) — every CROSS line carries `"arity":"pair"`.

### Shuffled-future proptests deferred to Plan 04-08

Per the plan's explicit guidance: `tests/shuffled_future_regression.rs` is NOT modified in this plan. Plan 04-08 (Wave 4) authors all Pair-arity rolling shuffled-future proptest extensions (pearson_rolling, spearman_rolling, ols_rolling, plus lead_lag) in a single Wave-4 modification. Rationale: Plan 04-03 (also Wave 3) already writes to that file for the ANOM-03 vol_rolling proptest; same-wave file-write conflict is forbidden by the planner's wave invariant. The structural look-ahead-safety guarantee (the engine's `bars_up_to` API; Plan 04-02) is the v1 contract; the proptests are regression pins one wave later.

## Task Commits

1. **Task 1: CROSS-02 cross.corr.pearson_rolling + cross.corr.spearman_rolling** — `ec5dc41` (feat)
2. **Task 2: CROSS-03 cross.ols.rolling** — `c89b3f6` (feat)

## Files Created / Modified

**Created (6 files, ~1,610 lines):**
- 2 corr_rolling files (mod.rs + kernel.rs)
- 2 ols_rolling files (mod.rs + kernel.rs)
- 2 integration test files (scan_corr_rolling.rs + scan_ols_rolling.rs)

**Modified (4 files):**
- `crates/miner-core/src/scan/cross/mod.rs` — module declarations + register lines + replaced registry-noop test with positive register-includes-three test
- `crates/miner-core/src/scan/registry.rs` — test count assertions widened to `>=` (bootstrap() body untouched)
- `crates/miner-cli/src/main.rs` — `handle_scans_subcommand` test now searches by scan_id rather than asserting length 1
- `crates/miner-cli/tests/scans_catalogue.rs` — integration test iterates all catalogue lines and locates LjungBox by id

## Decisions Made

- **D-03-canonical-key (Rule 3 deviation):** `Raw::new` requires the literal key `"timestamps_ms"`. The plan's `<behavior>` spec listed `"timestamps_ms_aligned"` as the joint-timestamps raw-series key but applying it would violate D-03. Used the canonical key under the same semantic; the raw block only carries aligned data so the suffix is unambiguous. Documented in `FINDING_SHAPE` doc-comments for both `corr_rolling` and `ols_rolling`.
- **Single corr_rolling module for both Pearson + Spearman** — they share the inner-join + windowing + envelope construction; only the per-window kernel call differs (dispatched via `CorrKind` enum). Keeps the diffs scoped to one module while preserving the per-scan trait impl boundary.
- **OLS convention: `returns_a ~ returns_b`** (response = leg A; regressor = leg B). β represents the hedge ratio "how many units of b to short per long unit of a." Test `ols_rolling_known_beta_2x_close_scaling` pins this — `b = 2*a` in close space yields equal log returns (scale invariance) so β = 1.
- **nalgebra `DMatrix` over `SMatrix`** — window size is a runtime parameter (3..N); SMatrix would require const-generic rows / template specialization per window value. DMatrix preserves the dense-linear-algebra path with O(window) heap per call. Documented in mod.rs per CLAUDE.md "small fixed sizes" guidance.
- **Bitwise tie detection in `rank_with_ties`** — uses `f64::to_bits()` equality (NOT `==` floating-point equality). Matches scipy's behavior on f64 inputs; the choice is documented in the helper's doc-comment.
- **`threshold_crossings` is f64-encoded** (1.0 / 0.0) rather than a bool vector — fits the single F64 `Dtype` RawArray wire form (`findings/base64_bytes.rs:75` only declares F64 in v1). Future additive `Dtype::Bool` variants could refine the encoding without breaking consumers.
- **Lower-bound registry assertions** — `bootstrap_registers_ljung_box_scan` and `bootstrap_invokes_all_three_family_registrars` use `r.scans.len() >= N` so Plan 04-03..04-10 don't break them as they extend; Plan 04-11 will tighten to the exact 23-count.

## Deviations from Plan

### Rule 3 (Blocking-issue auto-fix) — 3 instances

**1. [Rule 3 - Wire-form key vs D-03 invariant] raw.series uses `timestamps_ms` not `timestamps_ms_aligned`**

- **Found during:** Task 1 (corr_rolling envelope construction).
- **Issue:** Plan's `<behavior>` spec said `raw.series.{returns_a, returns_b, timestamps_ms_aligned}` and the acceptance assertion `raw_series_keys == &["returns_a", "returns_b", "timestamps_ms_aligned"]`. However the D-03 invariant in `findings/mod.rs::Raw::new` rejects any `Raw` whose `series` lacks the literal key `"timestamps_ms"`. The plan's wire-form spec would violate the structural invariant.
- **Fix:** Used the canonical key `"timestamps_ms"` for the joint aligned timestamps. The raw block only carries aligned data (it's the OUTPUT of `inner_join`) so the `_aligned` suffix is unambiguous and redundant. Documented in `FINDING_SHAPE` doc-comments in both `corr_rolling/mod.rs` and `ols_rolling/mod.rs`. Updated the integration tests to assert the canonical key.
- **Files modified:** `crates/miner-core/src/scan/cross/corr_rolling/mod.rs` (FINDING_SHAPE doc + key insertion), `crates/miner-core/src/scan/cross/ols_rolling/mod.rs` (same), test bodies.
- **Verification:** All lib + integration tests pass; the D-03 invariant test (`scan_corr_rolling_pearson_happy_path`'s `raw.series.contains_key("timestamps_ms")` assertion) is green.
- **Committed in:** Task 1 (`ec5dc41`).

**2. [Rule 3 - Registry test count cascade] `bootstrap_registers_ljung_box_scan` + `bootstrap_invokes_all_three_family_registrars` use `>=` assertions**

- **Found during:** Task 1 (running unit tests after the first `r.register(...)` line added).
- **Issue:** The existing tests asserted `r.scans.len() == 1` ("Plan 04-02 ships LjungBox + 3 empty family helpers = 1 total"). Registering CROSS scans breaks both tests immediately. The plan's stated invariant — "`git diff --stat crates/miner-core/src/scan/registry.rs` shows ZERO changes from this task's commits" — would require leaving the tests broken.
- **Fix:** Widened both assertions from `==` to `>=` lower bounds. The `bootstrap()` body in `registry.rs` is UNTOUCHED — only the test bodies inside `mod tests` were modified. This satisfies the per-family registrar contract (registry::bootstrap is "locked"; only the tests need updating as the registrar pattern is extended). Plan 04-11 will tighten back to exact-23 once all families are populated.
- **Files modified:** `crates/miner-core/src/scan/registry.rs` (test bodies only; bootstrap() unchanged).
- **Verification:** `cargo test -p miner-core --lib scan::registry` passes (6/6).
- **Committed in:** Task 1 (`ec5dc41`) updated test bodies; Task 2 (`c89b3f6`) tightened the count from `>= 3` to `>= 4`.

**3. [Rule 3 - CLI catalogue test cascade] `scans_emits_one_line_per_registered_scan` + `handle_scans_subcommand_emits_one_line_per_registered_scan_via_vec_sink`**

- **Found during:** Task 1 (running full workspace tests after registration).
- **Issue:** Both CLI tests assumed the catalogue had length 1 (Phase 3 baseline) and asserted on `lines[0]` directly. After registering CROSS scans, the catalogue has multiple lines and the Phase 3 LjungBox line is no longer at index 0 (alphabetical ordering puts cross.* before stats.*).
- **Fix:** Both tests now iterate ALL lines, look up the LjungBox line by scan_id, and assert its shape. The catalogue-schema validation loop now iterates every line. The negative findings-v1 schema assertion does the same.
- **Files modified:** `crates/miner-cli/src/main.rs` (test), `crates/miner-cli/tests/scans_catalogue.rs`.
- **Verification:** All workspace tests pass.
- **Committed in:** Task 1 (`ec5dc41`).

### Plan deviation summary

**Total deviations:** 3 (all Rule 3 — blocking-issue resolutions). No scope creep; each deviation was a required adaptation to land the plan's CROSS scan registration without breaking existing tests. The plan's `<verification>` "all prior plan tests continue to pass" criterion is satisfied via the deviations.

## Issues Encountered

- **D-03 invariant collision** (resolved as Deviation 1): the plan's wire-form key spec clashed with `Raw::new`'s structural validation. Pivot to the canonical key is the only viable option.
- **Clippy doc-markdown lints on new files** — `cargo clippy --workspace --all-targets -- -D warnings` flagged several missing-backtick lints on identifier names in module-level doc-comments. Fixed by wrapping `DMatrix`, `SMatrix`, `RESEARCH.md`, etc., in backticks. After the fix, clippy reports ONLY the 2 pre-existing main-branch errors (`reader.rs:100`, `ljung_box/mod.rs:79`).
- **Clippy `cast_precision_loss` on `usize as f64`** — added `#[allow(clippy::cast_precision_loss, reason = "...")]` annotations at the three call sites in `corr_rolling/kernel.rs` and `ols_rolling/kernel.rs`. Window sizes are bounded above by aligned_n (typically <= 512), far below f64 mantissa precision (2^52).
- **Clippy `similar_names`** in corr_rolling/mod.rs on `leg_a_source_id` + `leg_b_source_id` — refactored to a single `per_leg_source_ids: (String, String)` tuple to dodge the lint without sacrificing clarity.
- **Clippy `same_item_push`** in `fit_window` — refactored `for _ in 0..window { design_data.push(1.0) }` to `design_data.resize(window, 1.0)`.
- **Clippy `manual_let_else`** in `fit_window` — refactored `match xtx.try_inverse() { Some(inv) => inv, None => return NaN }` to `let Some(xtx_inv) = ... else { return NaN }`.

## User Setup Required

None — no new external dependencies, env vars, secrets, or service configuration. The integration tests run via direct `Scan::run` dispatch and require only the existing miner-core test harness.

## Next Phase Readiness

**Plan 04-08 (Wave 4 — CROSS-04 lead-lag + CROSS-05 cointegration) unblocked.** The per-family registrar is now populated with 3 CROSS scans; Plan 04-08 appends 2 more lines (`LeadLagScan`, `CointegrationScan`) following the same pattern. Plan 04-08 also owns the deferred shuffled-future proptest extensions for all 5 CROSS rolling scans (including this plan's 3 Pair-arity rolling scans) in a single Wave-4 modification of `tests/shuffled_future_regression.rs`.

**Plan 04-11 (Wave 7) entry points:**
- Tighten `bootstrap_registers_ljung_box_scan` + `bootstrap_invokes_all_three_family_registrars` from `>=` to exact-23 count assertions.
- Add insta envelope snapshots for `scan_corr_rolling.rs` + `scan_ols_rolling.rs`.
- Wire the engine's Pair branch in `run_one_with_registry` for end-to-end CLI/HTTP/MCP execution (currently the integration tests drive scans directly; the CLI cannot yet dispatch Pair-arity scans against a live bar-cache).

**No blockers** — all three plan-required Pair-arity rolling scans (Pearson + Spearman + OLS) are registered with full kernel implementations, primitive consumption (inner_join + log_returns), envelope shape compliance (D4-03 sources Vec len 2, leg-labelled raw.series), and parameter validation.

## Threat Flags

(No section emitted — every change was anticipated by the plan's threat model; the three Rule 3 deviations are structural-invariant compliance, not new threat surface.)

## Self-Check: PASSED

Verified:
- [x] `crates/miner-core/src/scan/cross/corr_rolling/{mod,kernel}.rs` exist (`ls` confirms).
- [x] `crates/miner-core/src/scan/cross/ols_rolling/{mod,kernel}.rs` exist.
- [x] `crates/miner-core/tests/scan_corr_rolling.rs` + `tests/scan_ols_rolling.rs` exist.
- [x] `grep -n 'cross.corr.pearson_rolling' crates/miner-core/src/scan/cross/corr_rolling/mod.rs` → 1 match (PEARSON_SCAN_ID constant).
- [x] `grep -n 'cross.corr.spearman_rolling' crates/miner-core/src/scan/cross/corr_rolling/mod.rs` → 1 match.
- [x] `grep -n 'cross.ols.rolling' crates/miner-core/src/scan/cross/ols_rolling/mod.rs` → 1 match.
- [x] `grep -n 'ScanArity::Pair' crates/miner-core/src/scan/cross/corr_rolling/mod.rs` → 2 matches (Pearson + Spearman impls).
- [x] `grep -n 'ScanArity::Pair' crates/miner-core/src/scan/cross/ols_rolling/mod.rs` → 1 match.
- [x] `grep -n 'fn rolling_pearson\|fn rolling_spearman\|fn rank_with_ties' crates/miner-core/src/scan/cross/corr_rolling/kernel.rs` → 3 matches.
- [x] `grep -n 'fn rolling_ols' crates/miner-core/src/scan/cross/ols_rolling/kernel.rs` → 1 match.
- [x] `grep -nE 'PearsonRollingScan|SpearmanRollingScan|OlsRollingScan' crates/miner-core/src/scan/cross/mod.rs` → 6 matches (3 re-exports + 3 register lines).
- [x] `grep -nE 'nalgebra|DMatrix|SMatrix' crates/miner-core/src/scan/cross/ols_rolling/kernel.rs` → multiple matches.
- [x] `git diff 85e9ec9..HEAD crates/miner-core/src/scan/registry.rs` shows only test-body changes (bootstrap() untouched).
- [x] `git diff 85e9ec9..HEAD crates/miner-core/tests/shuffled_future_regression.rs` shows ZERO changes.
- [x] `cargo test -p miner-core --lib scan::cross::corr_rolling::tests` → 12/12 passes.
- [x] `cargo test -p miner-core --lib scan::cross::ols_rolling::tests` → 11/11 passes.
- [x] `cargo test -p miner-core --test scan_corr_rolling` → 2/2 passes.
- [x] `cargo test -p miner-core --test scan_ols_rolling` → 1/1 passes.
- [x] `cargo test -p miner-core --test arity_preflight --test two_leg_facade --test scan_ljung_box --test shuffled_future_regression` → all green.
- [x] `cargo run -p miner-cli --quiet -- scans` (with MINER_CACHE_ROOT / MINER_BAR_CACHE_ROOT / MINER_OUTPUT env) → 4 lines: pearson_rolling, spearman_rolling, ols.rolling, ljung_box. All CROSS lines show `"arity":"pair"`.
- [x] `cargo test --workspace` → ALL green.
- [x] `cargo clippy -p miner-core --all-targets -- -D warnings` exits with ONLY the 2 pre-existing main-branch errors (`reader.rs:100`, `ljung_box/mod.rs:79`). Zero NEW lints introduced.
- [x] Task 1 commit `ec5dc41` present in git log.
- [x] Task 2 commit `c89b3f6` present in git log.

---
*Phase: 04-scan-catalogue-anom-cross-seas*
*Completed: 2026-05-20*
