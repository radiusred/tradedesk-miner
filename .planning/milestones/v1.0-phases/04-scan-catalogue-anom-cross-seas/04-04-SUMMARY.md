---
phase: 04-scan-catalogue-anom-cross-seas
plan: 04
subsystem: scan-catalogue-anom

tags:
  - rust
  - scan
  - anom

requires:
  - phase: 04-scan-catalogue-anom-cross-seas
    plan: 03
    provides: "primitives::returns::log_returns + primitives::raw_array::f64_slice_to_raw_array (consumed by all three new scans); scan::anom::register_anom_scans helper (append-only contract per Pattern E)"

provides:
  - "LjungBoxSqScan (ANOM-04 squared variant) with id=stats.autocorr.ljung_box_sq, version=1, arity=Single — reuses Phase 3 biased_acf + ljung_box_q_and_p kernels on SQUARED log-returns (Pitfall 9 byte-equivalence preserved on level variant)"
  - "OutliersZAndMadScan (ANOM-10) with id=stats.outliers.z_and_mad, version=1, arity=Single — hand-derived Iglewicz-Hoaglin modified-z (0.6745*(x-median)/MAD) + scipy.stats.zscore-compatible ddof=0 z-scores; union outlier index reporting"
  - "DrawdownProfileScan (ANOM-11) with id=stats.drawdown.profile, version=1, arity=Single — hand-derived single-pass peak-trough sweep over cumulative log-equity curve emitting max_drawdown + per-episode peaks/troughs/durations/recovery times + p50/p95/p99 percentiles of episode magnitudes"
  - "scan::anom::ljung_box_sq::kernel::square_returns helper (element-wise square; 6 unit tests including basic, empty, singleton, zeros, length-invariant, sign-invariant)"
  - "scan::anom::outliers::kernel::{z_scores, modified_z_scores, median, mad} pure-arithmetic helpers (11 hand-derived unit tests within 1e-12 including Iglewicz-Hoaglin 0.6745*4/1==2.698 reference)"
  - "scan::anom::drawdown::kernel::{cumulative_log_equity, compute_drawdown_profile, DrawdownProfile struct} hand-derived V-shape + compound-V kernel tests within 1e-12"
  - "scan::ljung_box::kernel::{biased_acf, ljung_box_q_and_p} visibility widened from pub(super) to pub(crate) (non-behavioural — Phase 3 statsmodels golden continues to pass byte-identically per D4-06 / Pitfall 9 invariant)"
  - "scan::anom::register_anom_scans now registers six ANOM scans alphabetical by id: ljung_box_sq, drawdown, outliers, returns, summary, vol"
  - "Three integration tests with insta envelope snapshots: scan_ljung_box_squared, scan_outliers, scan_drawdown"
  - "Per-scan envelope wire-form: series_kind discriminator on ANOM-04-sq encoded as raw UTF-8 byte payload inside Dtype::F64 RawArray (preserves v1 wire-form; no schema_version bump)"

affects:
  - "04-05 (ANOM-05 ADF, ANOM-06 KPSS, ANOM-07 variance ratio): consumes the same primitives::returns + register_anom_scans append contract; appends after stats.vol.rolling alphabetical-by-id"
  - "04-06 (ANOM-08 ARCH-LM, ANOM-09 Jarque-Bera): same primitives + helper; ANOM family complete after Plan 04-06"
  - "04-11 (Phase-end integration): final tightening of registry tests from `>= 1` to exact count once full 22-scan catalogue is registered; the three goldens commitments (one each of ANOM/CROSS/SEAS) land here"
  - "Phase 5/6 wrappers: 22-scan catalogue grows by 3 in this plan to 13 scans total; MCP/HTTP wrappers introspect via `miner scans` JSONL (every line carries `arity:single` per D4-02)"

tech-stack:
  added:
    - "(none) — Plan 04-01 added every Phase 4 dep; this plan consumes pure-f64 primitives + statrs (transitively via the Phase 3 ljung_box::kernel which the squared variant calls)"
  patterns:
    - "Pattern A — Single-leg ANOM scan body (Scan impl + helpers + 12-named-tests block); applied 3x (ljung_box_sq, outliers, drawdown)"
    - "Pattern B — Kernel split (mod.rs + kernel.rs); #[inline] pub(super) pure fns + sibling tests block; applied 3x"
    - "Pattern E — Per-family registrar (`register_anom_scans` appends r.register lines INSIDE the helper; registry.rs::bootstrap() body untouched)"
    - "Pattern J — Integration test 8-step walk (insta envelope snapshot via common::mask_volatile_fields); applied 3x"
    - "Kernel reuse via visibility widening — Phase 3 `biased_acf` + `ljung_box_q_and_p` re-called on a different (squared) input vector; Pitfall 9 invariant preserved (Phase 3 LjungBox golden byte-identical post-change)"

key-files:
  created:
    - "crates/miner-core/src/scan/anom/ljung_box_sq/mod.rs (~580 lines, 12 unit tests, 1 Scan impl, encodes series_kind as UTF-8 RawArray under Dtype::F64)"
    - "crates/miner-core/src/scan/anom/ljung_box_sq/kernel.rs (~95 lines, 6 unit tests, square_returns helper)"
    - "crates/miner-core/src/scan/anom/outliers/mod.rs (~575 lines, 16 unit tests, 1 Scan impl)"
    - "crates/miner-core/src/scan/anom/outliers/kernel.rs (~270 lines, 11 unit tests, z_scores + modified_z_scores + median + mad)"
    - "crates/miner-core/src/scan/anom/drawdown/mod.rs (~525 lines, 14 unit tests, 1 Scan impl)"
    - "crates/miner-core/src/scan/anom/drawdown/kernel.rs (~370 lines, 6 unit tests, cumulative_log_equity + compute_drawdown_profile + DrawdownProfile struct)"
    - "crates/miner-core/tests/scan_ljung_box_squared.rs (~140 lines, 1 integration test + insta snapshot)"
    - "crates/miner-core/tests/scan_outliers.rs (~140 lines, 1 integration test + insta snapshot)"
    - "crates/miner-core/tests/scan_drawdown.rs (~145 lines, 1 integration test + insta snapshot)"
    - "crates/miner-core/tests/snapshots/scan_ljung_box_squared__ljung_box_squared_happy_path.snap"
    - "crates/miner-core/tests/snapshots/scan_outliers__outliers_happy_path.snap"
    - "crates/miner-core/tests/snapshots/scan_drawdown__drawdown_happy_path.snap"
  modified:
    - "crates/miner-core/src/scan/anom/mod.rs — pub mod {drawdown,ljung_box_sq,outliers} declarations + re-exports; register_anom_scans body extended by 3 r.register lines; test extended with 3 new assertions"
    - "crates/miner-core/src/scan/ljung_box/kernel.rs — visibility widened on biased_acf + ljung_box_q_and_p from pub(super) to pub(crate); non-behavioural change (Phase 3 statsmodels golden continues to pass byte-identically per Pitfall 9)"

key-decisions:
  - "ANOM-04 series_kind discriminator: encoded as UTF-8 string bytes packed into a Dtype::F64 RawArray with shape [label.len()]. This preserves the v1 catalogue wire-form (no schema_version bump) without requiring a new Dtype::Utf8 variant. The integration test decodes the bytes back via std::str::from_utf8 to verify the round-trip. A future schema-version bump may introduce a proper Dtype::Utf8 variant."
  - "ANOM-04 visibility widening uses pub(crate) not pub — the squared variant lives in the same crate as the Phase 3 ljung_box module; no external consumers needed. Phase 3 LjungBox golden test (scan_ljung_box.rs:ljung_box_matches_statsmodels_golden) continues to pass byte-identically (Pitfall 9 invariant)."
  - "ANOM-10 outlier reporting is the UNION of z-outliers and modified-z-outliers — a bar is flagged if |z| > z_threshold OR |modified_z| > modified_z_threshold. The wire form reports the index union AND parallel value vectors (z + modified-z values at the union indices). Plan rationale: agents can disambiguate by comparing the two value columns against the two threshold scalars."
  - "ANOM-10 constant-input rejection (T-04-04-01): MAD == 0 -> ScanError::Kernel converted by the scan body (kernel returns (zeros, 0.0); body inspects mad == 0 and emits the error). Prevents emitting a degenerate finding with all-zero modified-z scores."
  - "ANOM-10 modified-z formula uses 0.6745 (the asymptotically-consistent constant per Iglewicz-Hoaglin 1993) — verified by hand-derivation test against 0.6745*4 == 2.698 and 0.6745*10 == 6.745 within 1e-12."
  - "ANOM-11 drawdown algorithm: single O(n) pass tracking running_peak + episode state (in_drawdown, trough_value, trough_idx). When equity[t] >= running_peak AND in_drawdown is true, close the episode and record peak index, trough index, duration (ts[trough]-ts[peak]), recovery time (ts[t]-ts[trough]). max_dd is signed (always <= 0). Hand-derived V-shape: closes [10, 20, 10, 20] gives max_dd == ln(0.5) within 1e-12."
  - "ANOM-11 series_close branch: closes treated as equity directly (no cumsum). Plan rationale: callers can opt-in to absolute-level drawdown without going through log_returns + cumsum. Default remains series=log_returns."
  - "ANOM-11 percentiles vector has FIXED length 3 (p50, p95, p99) — zeros if no closed episode (monotone-up series OR series ending underwater without recovery)."
  - "ANOM-11 ends-underwater branch: an episode is recorded ONLY if running_peak is re-attained. A series that ends below its all-time peak produces max_dd (signed) but empty peaks/troughs/durations vectors — pinned by drawdown_profile_ends_underwater_no_recovery test."
  - "Per-family registrar contract preserved (Pattern E): `crate::scan::registry.rs::bootstrap()` body is NOT touched in this plan. Only `scan::anom::mod::register_anom_scans` grew by three `r.register(...)` lines."
  - "Alphabetical insertion order in register_anom_scans: ljung_box_sq -> drawdown -> outliers -> returns -> summary -> vol (lexicographic by scan-id)."
  - "INSTA_UPDATE=always required for first-time snapshot creation (INSTA_UPDATE=auto only updates existing snapshots). Used INSTA_UPDATE=always once per integration test, then committed the .snap file."

requirements-completed:
  - ANOM-04
  - ANOM-10
  - ANOM-11

# Metrics
duration: 38min
completed: 2026-05-20
---

# Phase 04 Plan 04: ANOM Wave-4 Scans Summary

**Wave-4 shipped three single-shot ANOM scans (`stats.autocorr.ljung_box_sq@1`, `stats.outliers.z_and_mad@1`, `stats.drawdown.profile@1`) completing the "easy" half of ANOM. ANOM-04 (squared variant), ANOM-10 (outliers), and ANOM-11 (drawdown) are now shipped; Plans 04-05/04-06 own the five hand-derived heavyweight tests (ADF, KPSS, VR, ARCH-LM, Jarque-Bera). Phase 3 LjungBox golden continues to pass byte-identically — D4-06 / Pitfall 9 invariant preserved.**

## Performance

- **Duration:** ~38 min
- **Started:** 2026-05-20T11:25:00Z (approx — agent spawn)
- **Completed:** 2026-05-20T12:03:00Z
- **Tasks:** 3 of 3 (all autonomous)
- **Files created:** 12 (6 source modules + 3 integration tests + 3 insta snapshots)
- **Files modified:** 2 (anom/mod.rs, ljung_box/kernel.rs visibility widening)
- **Lines added (commit diff sum):** ~3,560
- **New tests:** 65 lib unit tests + 3 integration tests
- **Commits:** 4 (3 feat + 1 style/clippy cleanup)

## Accomplishments

- **ANOM-04 `stats.autocorr.ljung_box_sq@1`** — reuses the Phase 3 `biased_acf` + `ljung_box_q_and_p` kernels on SQUARED log-returns (volatility-clustering test per Tsay 2010 §5). `effect.metric = "ljung_box_q_squared"`, `effect.extra` carries the `series_kind` UTF-8 discriminator. 12 unit tests including the kernel-input-is-squared invariant (decodes the f64 returns_squared bytes and asserts against ref `log_returns(closes)^2` within 1e-12), q-stat-on-iid-returns smoke check, and the 9-scenario named-tests block. The Phase 3 LjungBox statsmodels golden (`scan_ljung_box.rs::ljung_box_matches_statsmodels_golden`) continues to pass byte-identically (Pitfall 9 invariant; verified by re-running after the visibility widening).
- **ANOM-10 `stats.outliers.z_and_mad@1`** — hand-derived Iglewicz-Hoaglin modified-z (0.6745*(x-median)/MAD) + scipy.stats.zscore-compatible ddof=0 z-scores. Union outlier reporting (z OR modified-z). 16 mod.rs unit tests + 11 kernel.rs unit tests (all hand-derived within 1e-12). Includes the constant-input MAD=0 ScanError::Kernel rejection (T-04-04-01 mitigation) and the 0.6745*4 == 2.698 / 0.6745*10 == 6.745 Iglewicz-Hoaglin reference test. Default thresholds: z=3.0, modified_z=3.5.
- **ANOM-11 `stats.drawdown.profile@1`** — hand-derived single-pass peak-trough sweep over the cumulative log-equity curve. Emits max_drawdown + per-episode `{peaks, troughs, durations_ms, time_to_recover_ms}` + `[p50, p95, p99]` percentiles of episode magnitudes. 14 mod.rs unit tests + 6 kernel.rs unit tests. Hand-derived V-shape: closes [10, 20, 10, 20] → max_dd == ln(0.5) ≈ -0.6931 within 1e-12; compound-V [10, 8, 5, 7, 10, 9, 11] verifies two closed episodes with the expected percentiles [3.0, 4.8, 4.96]; ends-underwater [5, 3, 1] verifies max_dd is tracked but no episode is recorded.
- **Per-family registrar contract preserved** — `register_anom_scans` extended by three `r.register(...)` lines (alphabetical: ljung_box_sq → drawdown → outliers, interleaved with the existing Wave-3 lines). `registry.rs::bootstrap()` production code is NOT modified.
- **CLI catalogue** — `miner scans` JSONL stream now lists 13 scans total: 1 Phase 3 LjungBox + 4 Wave-3 ANOM + 3 Wave-4 ANOM (this plan) + 3 Wave-3 CROSS + 3 Wave-3 SEAS = 13 (verified via `cargo run -p miner-cli -- scans 2>/dev/null | grep -c '^{'` → 13). All three new scans carry `arity:"single"` per D4-02.

## Task Commits

1. **Task 1 — ANOM-04 stats.autocorr.ljung_box_sq@1** — `9c98070` (feat)
2. **Task 2 — ANOM-10 stats.outliers.z_and_mad@1** — `ae3c9db` (feat)
3. **Task 3 — ANOM-11 stats.drawdown.profile@1** — `e4804a5` (feat)
4. **Clippy doc_markdown cleanup** — `cb4d882` (style)

## Files Created / Modified

See the `key-files` frontmatter for the exhaustive list. Headline counts:

**Created (12 files, ~3,560 lines):**
- 6 source files: `scan/anom/{ljung_box_sq,outliers,drawdown}/{mod,kernel}.rs`
- 3 integration tests: `tests/scan_{ljung_box_squared,outliers,drawdown}.rs`
- 3 insta snapshots under `tests/snapshots/`

**Modified (2 files):**
- `scan/anom/mod.rs` — register helper body + re-exports + test
- `scan/ljung_box/kernel.rs` — pub(super) → pub(crate) on `biased_acf` + `ljung_box_q_and_p` (non-behavioural)

## Decisions Made

- **ANOM-04 series_kind encoding via Dtype::F64 RawArray** — the catalogue's v1 wire-form supports only `Dtype::F64`; we pack the UTF-8 bytes of `"squared_returns"` into the RawArray's `data` field with shape `[15]` (the label byte length) and dtype `f64`. Consumers decode via `std::str::from_utf8(&extra["series_kind"].data.0)`. A future schema-version bump can introduce `Dtype::Utf8` to make the encoding explicit; today's contract is fully implementable and self-describing via the byte length.
- **ANOM-04 visibility widening uses `pub(crate)` not `pub`** — the squared variant lives in the same crate; no external consumers need the kernel functions. The Phase 3 LjungBox golden test (`scan_ljung_box.rs::ljung_box_matches_statsmodels_golden`) continues to pass byte-identically (Pitfall 9 invariant verified by `cargo test -p miner-core --test scan_ljung_box --test scan_ljung_box_squared` post-change).
- **ANOM-10 outlier-flagging is the UNION** of z- and modified-z criteria (a bar is an outlier if EITHER `|z| > z_threshold` OR `|modified_z| > modified_z_threshold`). Per-criterion value vectors are emitted in parallel to the index list so the Quant agent can disambiguate downstream.
- **ANOM-10 constant-input rejection (T-04-04-01)** — `MAD == 0` → `ScanError::Kernel`. The kernel returns `mad=0, mz=zeros` for constant input; the scan body inspects and converts. Prevents publishing degenerate findings.
- **ANOM-10 default thresholds: z=3.0, modified_z=3.5** — per Iglewicz-Hoaglin 1993 and standard z-score outlier convention. Both are configurable via `params`.
- **ANOM-11 series_close branch uses closes directly as equity** — no cumsum needed; the kernel is generic over the input curve. Default remains `series=log_returns` which goes through the cumulative-log-equity path.
- **ANOM-11 percentile vector has FIXED length 3** (`p50, p95, p99`) — zeros if no closed episode. Avoids variable-length vectors in the catalogue.
- **ANOM-11 ends-underwater means no closed episode** — `peaks`/`troughs`/`durations`/`recover` vectors are empty if the series never re-attains its running peak. `max_dd` is still tracked. Pinned by `drawdown_profile_ends_underwater_no_recovery` test.
- **Per-family registrar contract preserved** — `register_anom_scans` is the SOLE registration path for ANOM scans (Pattern E). `registry.rs::bootstrap()` is NOT modified.
- **Alphabetical-by-id insertion order** in `register_anom_scans`: `ljung_box_sq → drawdown → outliers → returns → summary → vol` (lexicographic).
- **INSTA_UPDATE=always for first-time snapshot creation** — `INSTA_UPDATE=auto` only updates existing snapshots; new ones require `always` for first-run creation. Applied once per integration test, snapshots committed alongside the test code (consistent with Plan 04-03 SUMMARY's note on the same gotcha).

## Deviations from Plan

### Rule 3 (Blocking-issue auto-fix) — 1 instance

**1. [Rule 3 — Blocking compile error] `usize::is_multiple_of(2)` unstable on Rust 1.85**

- **Found during:** Task 2 build.
- **Issue:** Initial implementation of `kernel::median` used `n.is_multiple_of(2)` for the even-length branch. This method is gated behind unstable feature `unsigned_is_multiple_of` (issue #128101) and the workspace pins Rust 1.85 / Edition 2024 stable. Build failed with `E0658: use of unstable library feature`.
- **Fix:** Replaced `n.is_multiple_of(2)` with `n % 2 == 0` (the canonical stable Rust idiom). Behavioural equivalent; no change to test expectations.
- **Files modified:** `crates/miner-core/src/scan/anom/outliers/kernel.rs` line 107.
- **Verification:** Full `cargo build -p miner-core --all-targets` green; `cargo test -p miner-core --lib scan::anom::outliers` 27 tests green.
- **Committed in:** `ae3c9db` (Task 2 commit; the in-process fix landed before the commit).

### Plan deviation summary

**Total deviations:** 1 (Rule 3 — Rust stable feature constraint). No scope creep — the deviation was a one-line stable-Rust idiom substitution. No clippy lints introduced beyond the 2 pre-existing main-branch errors (`reader.rs:100`, `ljung_box/mod.rs:79`) carried forward from Plan 04-03.

## Issues Encountered

- **`INSTA_UPDATE=auto` does NOT auto-accept new snapshots** — only existing snapshots can be auto-updated under `auto`. New snapshots require `INSTA_UPDATE=always` for first-run creation. Same issue noted in Plan 04-03 SUMMARY; the fix is identical: run with `INSTA_UPDATE=always` once per new integration test then commit the .snap file.
- **`n.is_multiple_of(2)` is unstable on Rust 1.85** — replaced with `n % 2 == 0` per Deviation 1 above.
- **`clippy::doc_markdown` lints on 11 doc-comments introduced by new code** — backtick-wrapped `BTreeMap`, `RawArray`, `log_returns`, `returns.len()`, `log_returns.len()`, `LjungBox` identifiers across the three new modules. Cleaned up in commit `cb4d882`. Post-cleanup, `cargo clippy -p miner-core --all-targets -- -D warnings` reports only the 2 pre-existing main-branch errors documented in Plan 04-03 SUMMARY.

## User Setup Required

None — no new external dependencies, env vars, secrets, or service configuration. Integration tests run against deterministic synthetic LCG fixtures; no Python golden regeneration is needed for this plan (Plan 04-11 owns goldens regen).

## Next Plan Readiness

**Plan 04-05 (ANOM-05 ADF + ANOM-06 KPSS + ANOM-07 variance ratio) unblocked.** The per-family registrar pattern is established, primitives are stable, and the test infrastructure (synthetic LCG fixtures + insta snapshots + common::mask_volatile_fields) is reused-as-is for the next three ANOM scans.

**Plan 04-05 entry points:**
- Append `r.register(Box::new(<NewScan>))` lines INSIDE `scan::anom::register_anom_scans`, alphabetical by scan-id. Plan 04-05 adds: `stats.stationarity.adf` (after `stats.outliers.z_and_mad`), `stats.stationarity.kpss` (right after ADF), `stats.variance_ratio.lo_mackinlay` (after vol.rolling).
- Continue Pattern A 12+ named-tests block + Pattern J 8-step integration walk.
- Continue using `primitives::returns::log_returns` + `primitives::raw_array::f64_slice_to_raw_array` via `use crate::scan::primitives::{returns::log_returns, raw_array::f64_slice_to_raw_array};`.
- Continue committing scan code + integration test + insta snapshot in a single commit per task.
- ADF / KPSS / Lo-MacKinlay variance-ratio kernels are hand-derived against statsmodels references (no Rust crate exists per RESEARCH §"Don't Hand-Roll"). Plan 04-11 owns the formal statsmodels golden.

**No blockers.** The 3 Wave-4 ANOM scans + 3 new integration tests land cleanly; Phase 3 LjungBox golden + every Phase 4 Wave-2/Wave-3 facade test continues to pass byte-identically; `cargo test --workspace` is fully green.

## Threat Flags

No new security-relevant surface introduced beyond what the plan's `<threat_model>` block already classifies:
- **T-04-04-01** (constant input → MAD=0 → NaN risk) — addressed in `outliers::kernel::modified_z_scores` (returns (zeros, 0.0)) + `outliers::mod::run` (converts to ScanError::Kernel). Pinned by `outliers_zero_variance_emits_scan_error` test.
- **T-04-04-02** (drawdown DoS via quadratic state) — addressed by single O(n) sweep with at most k ≤ n episodes recorded. No nested loops.
- **T-04-04-03** (LjungBox visibility widening) — verified non-behavioural by the Phase 3 statsmodels golden continuing to pass byte-identically (`scan_ljung_box.rs::ljung_box_matches_statsmodels_golden` test re-run post-change).
- **T-04-04-SC** (no new packages installed) — confirmed: `Cargo.toml` workspace dependencies untouched.

## Self-Check: PASSED

Verified:
- [x] `crates/miner-core/src/scan/anom/{ljung_box_sq,outliers,drawdown}/{mod,kernel}.rs` all exist (`ls` confirmed; 6 files).
- [x] `pub struct LjungBoxSqScan` present in `ljung_box_sq/mod.rs` (1 match).
- [x] `pub struct OutliersZAndMadScan` present in `outliers/mod.rs` (1 match).
- [x] `pub struct DrawdownProfileScan` present in `drawdown/mod.rs` (1 match).
- [x] `SCAN_ID = "stats.autocorr.ljung_box_sq"` / `stats.outliers.z_and_mad` / `stats.drawdown.profile` literals present (1 match each).
- [x] `pub(crate) fn biased_acf` + `pub(crate) fn ljung_box_q_and_p` present in `ljung_box/kernel.rs` (visibility widened from pub(super)).
- [x] `fn square_returns` present in `ljung_box_sq/kernel.rs` (1 match).
- [x] `0.6745` constant present in `outliers/kernel.rs` (Iglewicz-Hoaglin).
- [x] `fn cumulative_log_equity` + `fn compute_drawdown_profile` present in `drawdown/kernel.rs`.
- [x] `r.register(Box::new(LjungBoxSqScan));` + `r.register(Box::new(DrawdownProfileScan));` + `r.register(Box::new(OutliersZAndMadScan));` present inside `register_anom_scans` body.
- [x] `git diff 2dec2ca..HEAD -- crates/miner-core/src/scan/registry.rs` shows ZERO changes (Pattern E preserved).
- [x] 4 commits present in `git log --oneline 2dec2ca..HEAD`: `cb4d882`, `e4804a5`, `ae3c9db`, `9c98070`.
- [x] `crates/miner-core/tests/scan_{ljung_box_squared,outliers,drawdown}.rs` all exist + their `tests/snapshots/scan_*.snap` files exist.
- [x] `cargo test -p miner-core --lib scan::anom` — 126 ANOM lib tests pass (79 prior + 18 ljung_box_sq + 27 outliers + 20 drawdown = 144 total inside scan::anom; the filter matches submodule tests too).
- [x] `cargo test -p miner-core --test scan_ljung_box_squared --test scan_outliers --test scan_drawdown --test scan_ljung_box` — all 4 integration tests pass (LjungBox golden byte-identical post visibility widening).
- [x] `cargo test --workspace` — full workspace test run produces zero failures.
- [x] `cargo clippy -p miner-core --all-targets -- -D warnings` reports ONLY the 2 pre-existing main-branch errors (`reader.rs:100`, `ljung_box/mod.rs:79`) — zero NEW lints introduced.
- [x] `cargo run -p miner-cli -- scans 2>/dev/null | grep -E '"scan_id":"stats\.(autocorr\.ljung_box_sq|outliers\.z_and_mad|drawdown\.profile)"' | wc -l` returns 3 (all three new scans in the JSONL catalogue, each carrying `arity:"single"`).
- [x] `cargo run -p miner-cli -- scans 2>/dev/null | grep -c '^{'` returns 13 (1 Phase 3 + 6 ANOM + 3 CROSS + 3 SEAS = 13 total registered scans post-Plan-04-04).
- [x] `registry.rs untouched — registration via register_anom_scans` (Pattern E; the only Plan 04-04 production diff outside the new scan modules is the pub(super)→pub(crate) visibility widening in `ljung_box/kernel.rs`).

---
*Phase: 04-scan-catalogue-anom-cross-seas*
*Completed: 2026-05-20*
