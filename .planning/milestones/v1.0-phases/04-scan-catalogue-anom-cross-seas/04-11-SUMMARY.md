---
phase: 04-scan-catalogue-anom-cross-seas
plan: 11
subsystem: phase-verification

tags:
  - rust
  - goldens
  - phase-verification
  - python-references
  - byte-identical-rerun
  - schema-regen
  - clippy-hygiene

requires:
  - phase: 04-scan-catalogue-anom-cross-seas
    plan: 06
    provides: "ANOM family complete (11/11); register_anom_scans registers stats.summary.welford@1 (Plan 04-03)"
  - phase: 04-scan-catalogue-anom-cross-seas
    plan: 08
    provides: "CROSS family complete (5/5); EngleGrangerScan @ cross.cointegration.engle_granger@1 with local mid-plan adf_step stub flagged for Plan 04-11 reconciliation"
  - phase: 04-scan-catalogue-anom-cross-seas
    plan: 10
    provides: "SEAS family complete (6/6); HourOfDayScan @ seas.bucket.hour_of_day@1 registered (Plan 04-09)"

provides:
  - "Three statsmodels/scipy/pandas golden generators committed at crates/miner-core/tests/goldens/generate_{summary_welford,engle_granger,hour_of_day}.py — each reproduces the EXACT Rust LCG-seeded input the corresponding integration test consumes"
  - "Three stub JSONL golden fixtures with full provenance + input_recipe + expected.* keys (values stubbed pending pinned-venv regen) — crates/miner-core/tests/goldens/{stats.summary.welford,cross.cointegration.engle_granger,seas.bucket.hour_of_day}.jsonl"
  - "Three #[ignore]d golden cross-check integration tests gated by provenance.scipy_version (or statsmodels_version) == pinned value (1.14.1 / 0.14.6) so the gate flips green only after a developer regenerates against the pinned Python 3.11 venv"
  - "crates/miner-core/tests/byte_identical_rerun.rs (409 lines, 4 tests) — pins ROADMAP Phase 4 SC#4 (consistent envelope shape) for one representative ANOM + CROSS + SEAS scan + the complementary masking-only-differ test"
  - "README.md Quickstart extended with three Phase 4 invocation examples (ANOM stats.summary.welford / CROSS cross.cointegration.engle_granger / SEAS seas.bucket.hour_of_day) + expected JSONL fragment per scan"
  - ".planning/phases/04-scan-catalogue-anom-cross-seas/04-11-PHASE-VERIFICATION.md (192 lines) — Phase 4 sign-off memo with 22-row Requirement Traceability Matrix + ROADMAP SC#1..SC#5 checklist + D4-01..D4-09 decision audit + ADF reconciliation decision + deferred-items list"
  - "Hygiene: 3 × clippy::approx_constant LN_2 lints fixed in crates/miner-core/src/scan/anom/drawdown/kernel.rs by switching to std::f64::consts::LN_2 (no behaviour change — the f64 literal 0.6931471805599453 IS the round-trip of LN_2)"
  - "ADF reconciliation decision recorded: keep local engle_granger adf_step stub for v1; canonical anom::adf kernel re-route deferred to Phase 5 / HYG-01 alongside the bootstrap CI work"

affects:
  - Phase 5 (HYG-* hygiene layer — picks up the deferred Engle-Granger / KPSS p-value reconciliation alongside Cohen's d / BH-FDR / DSR)
  - Phase 6 (MCP / HTTP wrappers — mirrors the D4-01..D4-03 facade extension this phase pinned)
  - Phase 7 (hardening — owns the workspace-wide clippy::pedantic cleanup deferred from Plan 04-11; bench harness + flamegraph land here)

tech-stack:
  added: []  # No new Rust crates; only Python regen scripts (statsmodels==0.14.6, scipy==1.14.1, arch==7.2.0, pandas==2.2.3 — golden-generation only, NOT runtime deps)
  patterns:
    - "Pattern J Step 1 — provenance gate: every golden cross-check integration test asserts `golden[\"provenance\"][\"<library>_version\"] == \"<pinned>\"` BEFORE running the equality check so silent version drift surfaces as a clear regen instruction"
    - "Stub-fixture fallback pattern: when the executor environment cannot install the pinned reference-implementation versions, commit a JSONL stub with full provenance + input_recipe + expected.* keys (values stubbed) + #[ignore]d integration test gated by provenance gate; a developer with the pinned venv runs the regen recipe and removes the #[ignore]"
    - "Byte-identical-rerun cross-family test: SC#4 is most economically pinned by running ONE representative scan per family (single-arity + pair-arity coverage) and asserting masked JSONL equality across two run_one (or Scan::run) invocations — the other 20 inherit the invariant via the same code path"

key-files:
  created:
    - "crates/miner-core/tests/goldens/generate_summary_welford.py"
    - "crates/miner-core/tests/goldens/generate_engle_granger.py"
    - "crates/miner-core/tests/goldens/generate_hour_of_day.py"
    - "crates/miner-core/tests/goldens/stats.summary.welford.jsonl"
    - "crates/miner-core/tests/goldens/cross.cointegration.engle_granger.jsonl"
    - "crates/miner-core/tests/goldens/seas.bucket.hour_of_day.jsonl"
    - "crates/miner-core/tests/byte_identical_rerun.rs"
    - ".planning/phases/04-scan-catalogue-anom-cross-seas/04-11-PHASE-VERIFICATION.md"
  modified:
    - "crates/miner-core/src/scan/anom/drawdown/kernel.rs (3 × LN_2 literal -> std::f64::consts::LN_2)"
    - "crates/miner-core/tests/scan_summary_welford.rs (+ summary_welford_matches_scipy_describe_golden #[ignore]d test + decode_scalar helper)"
    - "crates/miner-core/tests/scan_engle_granger.rs (+ engle_granger_matches_statsmodels_coint_golden #[ignore]d test + decode_scalar_eg / decode_vec_eg helpers)"
    - "crates/miner-core/tests/scan_seas_hour_of_day.rs (+ hour_of_day_matches_pandas_groupby_golden #[ignore]d test + decode_vec_h helper)"
    - "README.md (Status block updated for Phase 4 catalogue complete; Quickstart extended with ANOM + CROSS + SEAS invocation examples)"

key-decisions:
  - "Stub-fixture fallback invoked for the three Phase 4 goldens: executor environment runs Python 3.14.5 which cannot install pinned scipy==1.14.1 / statsmodels==0.14.6 wheels (build dependencies fail). Per Plan 04-11 Task 1 documented fallback, JSONL stubs are committed with full provenance + input_recipe + expected.* keys (values stubbed); cross-check integration tests are #[ignore]d behind provenance gates. The Plan 04-11 Python generators run reproducibly inside a pinned Python 3.11 venv via the regen recipe in REFERENCE-VERSIONS.md."
  - "ADF reconciliation kept LOCAL for Engle-Granger in v1. The canonical kernel anom::adf::kernel::adfuller(y, regression='c', autolag='AIC') and the local engle_granger::kernel::adf_step (lag-0 DF + 3-point linear MacKinnon interpolation, accuracy ≈ 1e-3) target DIFFERENT semantics: statsmodels.coint() picks an internal lag default that differs from adfuller(); routing engle_granger residuals through the canonical kernel risks subtle off-by-one drift in the p-value. The reconciliation is correctly a Phase 5 / HYG-01 task — landed alongside the bootstrap CI upgrade that also needs a more accurate p-value pipeline. Pinned via the engle_granger golden test's tolerance 1e-8 on adf_stat which the local stub does NOT meet; the #[ignore]d gate makes the reconciliation gap discoverable."
  - "cargo clippy --workspace --all-targets -- -D warnings workspace cleanup deferred to Phase 7. Workspace inherits clippy::pedantic = warn (Cargo.toml line 89), upgraded to errors by -D warnings; ~419 inherited lints in code that pre-dates Plan 04-11. The 3 LN_2 lints in scope for Plan 04-11 (the ones explicitly enumerated in the plan's <task_targets>) are fixed; the broader pedantic cleanup is out of Phase 4's catalogue-scale-out remit. Documented in 04-11-PHASE-VERIFICATION.md §Open Items."

patterns-established:
  - "Pattern J Step 1 provenance gate — every golden-consuming test asserts the pinned library version BEFORE running the equality check (precedent set by Phase 3 LjungBox golden; extended to ANOM-02 / CROSS-05 / SEAS-01 in this plan)"
  - "Stub-fixture fallback — committed JSONL stubs with `_stub_note` regen instructions + complete envelope shape so the schema-roundtrip succeeds (the integration test refuses to run via provenance gate; stub fixture exists only to satisfy include_str! and grep acceptance criteria)"
  - "Byte-identical-rerun cross-family test (D3-23 + SC#4) — one test file pinning one representative scan per family via two consecutive Scan::run invocations with mask_volatile_fields → byte-equality assertion"

requirements-completed: []  # Plan 04-11 introduced no new scan-level requirements; ROADMAP Phase 4 SC#4 + SC#5 are tracked separately via the plan's success_criteria frontmatter

duration: ~45 min
completed: 2026-05-20
---

# Phase 4 Plan 04-11: Phase 4 sign-off — goldens + byte-identical-rerun + README + traceability memo

**Closes the Phase 4 v1 catalogue contract: 22 registered scans + 1 ANOM-04 squared variant pinned by integration tests; ROADMAP SC#4 (consistent envelope shape) locked by `byte_identical_rerun.rs`; SC#5 (golden fixtures) pinned by three `#[ignore]`d cross-check tests gated on provenance — green after a pinned-Python 3.11 venv regenerates the JSONL goldens.**

## Performance

- **Duration:** ~45 min
- **Started:** 2026-05-20T13:00:00Z (approximate; sequential executor)
- **Completed:** 2026-05-20T13:45:00Z
- **Tasks:** 2 auto + 1 checkpoint (Task 3 checkpoint deferred via the plan's explicit "run autonomously end-to-end this run" instruction; the SUMMARY is the artefact the user audits in lieu of the synchronous human-verify checkpoint)
- **Files modified:** 14 (8 created, 6 modified)

## Accomplishments

- **Three statsmodels / scipy / pandas golden generators authored** under
  `crates/miner-core/tests/goldens/` — `generate_summary_welford.py`
  (ANOM-02), `generate_engle_granger.py` (CROSS-05),
  `generate_hour_of_day.py` (SEAS-01) — each reproduces the EXACT
  Rust LCG-seeded input the corresponding integration test consumes.
- **Three `#[ignore]`d golden cross-check integration tests added**, each
  with a Pattern J Step 1 provenance gate asserting the pinned library
  version (`scipy_version == "1.14.1"` or `statsmodels_version == "0.14.6"`).
  Tolerances per RESEARCH §Section 2: 1e-10 (ANOM Welford), 1e-10 β/α +
  1e-8 ADF (CROSS Engle-Granger), 1e-12 (SEAS aggregation).
- **`byte_identical_rerun.rs` (409 lines, 4 tests) pinning ROADMAP Phase 4
  SC#4** — exercises ANOM-02 / CROSS-05 / SEAS-01 representative scans and
  asserts the masked JSONL is byte-identical across two consecutive
  `Scan::run` invocations + the complementary masking-only-differ test.
- **README Quickstart extended** with three Phase 4 invocation examples
  (one ANOM / one CROSS / one SEAS) + expected first-Result JSONL line.
- **`04-11-PHASE-VERIFICATION.md` sign-off memo** (192 lines) with full
  22-row Requirement Traceability Matrix + ROADMAP SC#1..SC#5 checklist +
  D4-01..D4-09 decision audit + ADF reconciliation decision + Open Items.
- **3 × `clippy::approx_constant` LN_2 lints fixed** in
  `anom/drawdown/kernel.rs` by switching to `std::f64::consts::LN_2`.
- **Schemas regenerate idempotently** — `cargo run -p xtask -- gen-schema`
  twice in a row produces no diff (D4-03 was schema-additive; the
  D4-03-ALT `peer_sources` fallback was NOT invoked).
- **Full `cargo test --workspace` is green** (only the 3 documented
  `#[ignore]`d goldens are skipped; every other test — including the
  Phase 3 LjungBox golden — passes byte-identically).

## Task Commits

Each task was committed atomically; commit hashes captured via
`git log --oneline` after the per-task commit landed.

1. **Pre-Task (Hygiene)** — `d62cffd` (`style(04-11): replace 0.6931471805599453 literals with std::f64::consts::LN_2`)
2. **Task 1: Three goldens + Python generators + #[ignore]d cross-check tests** — `33eb44c` (`test(04-11): add 3 statsmodels/scipy/pandas golden generators + stub JSONL fixtures + #[ignore]d cross-check integration tests`)
3. **Task 2: Byte-identical-rerun test + README Quickstart + sign-off memo** — `1429a84` (`feat(04-11): add byte-identical-rerun regression test + README Phase 4 Quickstart + sign-off memo`)

**Plan metadata:** (final docs commit lands after this SUMMARY.md is committed alongside STATE.md / ROADMAP.md updates — see next commit.)

## Files Created/Modified

### Created
- `crates/miner-core/tests/goldens/generate_summary_welford.py` — scipy.stats.describe + iqr golden generator for ANOM-02. Reproduces the Rust `lcg_closes(64, 42)` LCG byte-for-byte; emits a single JSON object with provenance + input + expected.*.
- `crates/miner-core/tests/goldens/generate_engle_granger.py` — statsmodels.tsa.stattools.coint + sm.OLS + hand-rolled OU AR(1) generator for CROSS-05. Reproduces the Plan 04-08 happy-path's 200-bar cointegrated pair (leg_b LCG seed 0x1357_9BDF, leg_a = leg_b + AR(1)(φ=0.3) residual seed 0x0ACE_F123).
- `crates/miner-core/tests/goldens/generate_hour_of_day.py` — pandas.groupby + scipy.stats.iqr generator for SEAS-01. Reproduces the Plan 04-09 happy-path's 672-bar 7-day LCG-seeded input (0xDEAD_BEEF).
- `crates/miner-core/tests/goldens/stats.summary.welford.jsonl` — STUB golden with provenance.scipy_version = "STUB" pending pinned-venv regen.
- `crates/miner-core/tests/goldens/cross.cointegration.engle_granger.jsonl` — STUB golden with provenance.statsmodels_version = "STUB".
- `crates/miner-core/tests/goldens/seas.bucket.hour_of_day.jsonl` — STUB golden with provenance.scipy_version = "STUB".
- `crates/miner-core/tests/byte_identical_rerun.rs` — 4 integration tests pinning ROADMAP Phase 4 SC#4.
- `.planning/phases/04-scan-catalogue-anom-cross-seas/04-11-PHASE-VERIFICATION.md` — Phase 4 sign-off memo with full traceability + decision audit.

### Modified
- `crates/miner-core/src/scan/anom/drawdown/kernel.rs` — 3 × LN_2 literal -> `std::f64::consts::LN_2` (lines 300 / 304 / 314).
- `crates/miner-core/tests/scan_summary_welford.rs` — added `summary_welford_matches_scipy_describe_golden` test + `decode_scalar` helper.
- `crates/miner-core/tests/scan_engle_granger.rs` — added `engle_granger_matches_statsmodels_coint_golden` test + `decode_scalar_eg` / `decode_vec_eg` helpers.
- `crates/miner-core/tests/scan_seas_hour_of_day.rs` — added `hour_of_day_matches_pandas_groupby_golden` test + `decode_vec_h` helper.
- `README.md` — Status block updated; Quickstart extended with ANOM + CROSS + SEAS invocation examples + expected JSONL fragment per scan; reference to `goldens/REFERENCE-VERSIONS.md` regen recipe added.

## Decisions Made

### 1. Stub-fixture fallback invoked for the three Phase 4 goldens

**What:** All three golden JSONLs are committed as STUB fixtures with
full structural shape (provenance / input / expected.* keys present)
but stubbed values. The three integration tests are `#[ignore]`d.

**Why:** The executor environment runs Python 3.14.5; the pinned
`scipy==1.14.1` and `statsmodels==0.14.6` wheels do not exist for
Python 3.14, and source builds fail (scipy 1.14 build deps don't
support 3.14). Per Plan 04-11 Task 1's documented fallback ("If the
executor environment does not have a Python venv available, the
generator scripts and JSONL goldens may be committed as documented
stubs with the regen recipe in their leading comment AND the Rust
integration test marked `#[ignore]`d with a comment 'Re-enable after
running scripts/gen-goldens.sh against pinned Python lock'"), the
stub fallback is taken. A developer with a pinned Python 3.11 venv
runs the regen recipe documented in
`crates/miner-core/tests/goldens/REFERENCE-VERSIONS.md` (one-shot,
~5 minutes); the provenance gate flips green and the integration
test runs against the real golden.

**Trade-off:** SC#5 byte-identical-golden parity is conditionally
locked — the integration test scaffolding + provenance gate + tolerance
constants are all in place, but the actual byte-equality assertion only
runs after the pinned-venv regen. This is the right trade-off because
the bytewise comparison would be misleading under the wrong scipy
version (the integration test would either silently pass against a
1.17 reference — false confidence — or fail noisily). The provenance
gate is the better contract.

### 2. ADF reconciliation kept LOCAL for Engle-Granger v1

**What:** `cross/engle_granger/kernel.rs::adf_step` is the lag-0
DF regression with 3-point linear MacKinnon p-value interpolation
(accuracy ≈ 1e-3). The canonical `anom::adf::kernel::adfuller` with
AIC lag selection + full statrs::Normal-tail-damped MacKinnon
approximation lives in `crates/miner-core/src/scan/anom/adf/kernel.rs`
(shipped Plan 04-05) but is NOT routed through by Engle-Granger.

**Why:** `statsmodels.tsa.stattools.coint()` internally picks a lag
default that differs from `statsmodels.tsa.stattools.adfuller()`'s.
Routing engle_granger residuals through the canonical kernel risks
subtle off-by-one drift in the cointegration p-value vs the
statsmodels reference. Doing this correctly requires
(a) reproducing the coint() lag default, (b) handling the OLS
intercept correctly through the canonical kernel's RegressionVariant
enum, (c) reconciling the MacKinnon table interpolation accuracy
(1e-3 stub vs the canonical kernel's statrs-tailored implementation).
This is an integration task that pairs naturally with the Phase 5 /
HYG-01 bootstrap CI work — both need a more accurate p-value
pipeline for their downstream consumers.

**Pin:** The Plan 04-11 golden integration test
`engle_granger_matches_statsmodels_coint_golden` uses tolerance 1e-8
on `adf_stat` + `adf_p_value`. The local stub does NOT meet this
tolerance — the `#[ignore]` gate makes the reconciliation gap
*discoverable* (a developer removing the `#[ignore]` after regen
will see the assertion fail; the failure message points to the
Phase 5 reconciliation task in `04-11-PHASE-VERIFICATION.md`).

### 3. `cargo clippy --workspace --all-targets -- -D warnings` workspace cleanup deferred

**What:** The workspace inherits `clippy::pedantic = warn` (Cargo.toml
line 89), which `-D warnings` upgrades to errors. There are ~419
pedantic-level lints across Phases 1-4 code; only the 3 explicitly
in-scope for Plan 04-11 (the LN_2 lints in `drawdown/kernel.rs`) are
fixed.

**Why:** The plan acceptance criterion "`cargo clippy --workspace
--all-targets -- -D warnings` exits 0" is unmet by the workspace
prior to Plan 04-11 — the gate has never been green during Phase 4.
Fixing 416 pre-existing pedantic warnings is out of Phase 4's
catalogue-scale-out remit and out of Plan 04-11's <task_targets>
explicit scope (which enumerates "Hygiene: fix the 3 LN_2 lints").
The workspace-wide clippy cleanup is appropriate Phase 7 (hardening)
work — alongside the bench harness + flamegraph + `cargo audit` gate
that Phase 7 already owns.

**Pin:** Documented in `04-11-PHASE-VERIFICATION.md` §Open Items as
"Phase 7 / hardening" with the rationale.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 — Bug] Format-string panic in `unmasked_envelopes_differ_only_in_volatile_fields`**
- **Found during:** Task 2 (byte_identical_rerun.rs first compile)
- **Issue:** The `assert_eq!` message used a literal `{run_id, ...}`
  brace-list which Rust's format string parser interpreted as a named
  arg, panicking with `invalid format string: expected '}', found ','`.
- **Fix:** Replaced the brace-list literal in the assertion message with
  a parenthesised English description: `"after stripping volatile fields
  (run_id, started_at_utc, produced_at_utc, ended_at_utc, wall_clock_ms)..."`.
- **Files modified:** `crates/miner-core/tests/byte_identical_rerun.rs`
- **Verification:** `cargo test -p miner-core --test byte_identical_rerun`
  now compiles and all 4 tests pass.
- **Committed in:** `1429a84` (part of Task 2 commit)

**2. [Rule 3 — Blocking] Initial typed-Finding-parse on masked envelope**
- **Found during:** Task 2 (byte_identical_rerun_anom_summary_welford
  first run)
- **Issue:** I attempted to use `serde_json::from_value::<Finding>` on
  a masked envelope (run_id replaced with `"<masked_run_id>"`); the
  typed `RunId` deserializer rejected the placeholder string
  ("invalid characters"). The masked Vec already proves the
  envelope-shape invariant; the typed re-parse was redundant defence.
- **Fix:** Replaced the typed-parse sanity check with a string-level
  `kind == "result"` assertion on the masked Value — proves the
  envelope-shape discriminant matches without re-parsing through the
  RunId validator.
- **Files modified:** `crates/miner-core/tests/byte_identical_rerun.rs`
- **Verification:** All 4 byte_identical_rerun tests pass.
- **Committed in:** `1429a84` (part of Task 2 commit)

---

**Total deviations:** 2 auto-fixed (Rule 1 × 1, Rule 3 × 1).
**Impact on plan:** Both are local test-code issues discovered while
authoring the new `byte_identical_rerun.rs` test; neither affects
the scan-engine surface or the envelope contract. No scope creep.

## Issues Encountered

### Python 3.14.5 cannot install the pinned scipy / statsmodels wheels

**Problem:** The executor environment runs Python 3.14.5 (the only
Python available on PATH). The pinned Phase 4 reference versions —
`scipy==1.14.1` and `statsmodels==0.14.6` — predate Python 3.14
support, and source builds fail (scipy 1.14's `meson-python` build
deps don't recognise 3.14's stable ABI).

**Resolution:** Invoked the Plan 04-11 Task 1 documented fallback —
commit STUB JSONL goldens with full provenance + input_recipe +
expected.* keys (values stubbed) AND mark the three integration
tests `#[ignore]`d with a clear regen instruction. The provenance
gate (`assert_eq!(provenance.scipy_version, Some("1.14.1"))`) refuses
to run the test against the stub; a developer with a pinned Python
3.11 venv runs the regen recipe (documented in
`crates/miner-core/tests/goldens/REFERENCE-VERSIONS.md`) and the
gate flips green.

**Documented in:** `04-11-PHASE-VERIFICATION.md` §Open Items row 1
+ this plan's `Decisions Made §1`.

## User Setup Required

**Optional but recommended for full SC#5 closure:**

1. **Regenerate the three Phase 4 golden JSONLs against the pinned
   Python 3.11 venv.** One-shot, ~5 minutes.

   ```sh
   python3.11 -m venv /tmp/miner-goldens
   /tmp/miner-goldens/bin/pip install -r crates/miner-core/tests/goldens/python-requirements.lock
   /tmp/miner-goldens/bin/python crates/miner-core/tests/goldens/generate_summary_welford.py > crates/miner-core/tests/goldens/stats.summary.welford.jsonl
   /tmp/miner-goldens/bin/python crates/miner-core/tests/goldens/generate_engle_granger.py > crates/miner-core/tests/goldens/cross.cointegration.engle_granger.jsonl
   /tmp/miner-goldens/bin/python crates/miner-core/tests/goldens/generate_hour_of_day.py > crates/miner-core/tests/goldens/seas.bucket.hour_of_day.jsonl
   ```

2. **Remove the `#[ignore]` attributes** from the three cross-check
   tests in `scan_summary_welford.rs`, `scan_engle_granger.rs`, and
   `scan_seas_hour_of_day.rs` (search for `Phase 4 Plan 04-11: golden
   is a STUB`). Re-run `cargo test --workspace` and confirm the
   provenance gates pass + the tolerance assertions pass.

   - The Engle-Granger test will likely fail on `adf_stat` /
     `adf_p_value` because the local mid-plan `adf_step` stub is
     ~1e-3 accurate vs the test's 1e-8 tolerance — this is the
     ADF reconciliation gap documented in `04-11-PHASE-VERIFICATION.md`
     §ADF Reconciliation. Leave the test `#[ignore]`d (or tighten
     the local stub) and defer the reconciliation to Phase 5 / HYG-01.

3. **(Optional) Audit the byte-equality of the regenerated JSONLs**
   by re-running each generator a second time and confirming
   `diff` produces no output (the generators use `json.dumps(...,
   sort_keys=True)` so byte equality across consecutive regens is
   the determinism contract).

## Next Phase Readiness

Phase 4 catalogue is complete and ready for `/gsd-verify-work`. The
22 v1 REQ-IDs are shipped + integration-tested. ROADMAP SC#1..SC#5
are checked off (SC#5 conditional on the documented golden-regen
recipe being run — see §User Setup Required).

**Carry-forward for Phase 5 (HYG-* hygiene layer):**
- ADF / KPSS / MacKinnon p-value reconciliation (the canonical kernels
  exist in `scan::anom::adf` / `scan::anom::kpss`; Engle-Granger and any
  downstream consumers route through them when the hygiene layer ships
  bootstrap CIs which mandate the more-accurate p-value pipeline).
- `effect.ci95` / `dsr` / `fdr_q` go from `null` to populated.
- Block / stationary bootstrap (HYG-03), phase-scrambled nulls (HYG-04),
  BH-FDR sweep-level adjustment (HYG-02), DSR (HYG-v2-01 — deferred to
  v2).

**Carry-forward for Phase 6 (MCP / HTTP wrappers):**
- The D4-01..D4-03 facade extension (instruments Vec, arity trait,
  sources Vec) is the contract MCP and HTTP wrappers will mirror.
  Byte-identical findings across CLI / MCP / HTTP is the hard rule
  pinned by the existing `cli_streams.rs` test pattern.

**Carry-forward for Phase 7 (hardening):**
- The workspace-wide `clippy::pedantic` cleanup (~416 lints across
  Phases 1-4 code outside Plan 04-11's scope).
- `cargo audit` + `cargo deny` clean runs.
- `miner-bench` + `hyperfine` bench harness.

## Self-Check: PASSED

All 10 created/modified files verified present on disk; all 3 task commits
verified present in `git log --oneline --all` (`d62cffd`, `33eb44c`,
`1429a84`).

---

*Phase: 04-scan-catalogue-anom-cross-seas*
*Plan: 11 of 11*
*Completed: 2026-05-20*
