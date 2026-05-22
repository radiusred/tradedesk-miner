---
phase: 07-hardening-benchmarks-reproducibility
plan: 06
subsystem: testing
tags: [criterion, microbench, rust, ndarray, nalgebra, statrs, csv, zstd, fft-free]

requires:
  - phase: 07-hardening-benchmarks-reproducibility
    provides: tests/fixtures/cache/EURUSD/2024/00/01_bid.csv.zst (Plan 07-02 synthetic Dukascopy fixture)
  - phase: 07-hardening-benchmarks-reproducibility
    provides: realfft workspace dep + miner-core null kernel landed (Plan 07-05; serialized via shared workspace Cargo.toml edits)
provides:
  - "Six criterion microbench files under `crates/miner-core/benches/` exercising the hot kernels (zstd decode, csv parse, aggregator, rolling Pearson/Spearman, Ljung-Box, OLS-4D)"
  - "criterion 0.7 + dhat 0.3 in `[workspace.dependencies]`; criterion wired as a `[dev-dependencies]` entry on miner-core; dhat held as workspace-only (consumer = miner-bench, Plan 07-08)"
  - "`[profile.release] debug = 1` (line-tables-only) workspace setting — precondition for Plan 07-08's dhat symbol attribution"
  - "Six `[[bench]]` entries with `harness = false` registered on miner-core"
affects: [07-07, 07-08, future regression-test plans]

tech-stack:
  added: [criterion = 0.7 (html_reports), dhat = 0.3, csv (now also a miner-core dev-dep)]
  patterns: [criterion bench file shape (inputs-outside-loop / kernel-inside-black_box), kernel mirror-in-bench convention (inline a faithful copy when the production kernel is pub(crate) to avoid public-surface promotion)]

key-files:
  created:
    - "crates/miner-core/benches/bench_zstd_decompress_1day.rs"
    - "crates/miner-core/benches/bench_csv_parse_1day.rs"
    - "crates/miner-core/benches/bench_aggregate_1m_to_15m.rs"
    - "crates/miner-core/benches/bench_rolling_corr.rs"
    - "crates/miner-core/benches/bench_ljung_box.rs"
    - "crates/miner-core/benches/bench_ols_fit_4d.rs"
    - ".planning/phases/07-hardening-benchmarks-reproducibility/deferred-items.md"
  modified:
    - "Cargo.toml"
    - "crates/miner-core/Cargo.toml"
    - "crates/miner-core/src/engine/hygiene_dispatch.rs (Rule 3 — fixed 3 pre-existing clippy::pedantic warnings blocking the -D warnings acceptance gate)"
    - "crates/miner-core/src/scan/hygiene/null.rs (Rule 3 — fixed 3 pre-existing clippy::pedantic warnings blocking the -D warnings acceptance gate)"
    - "Cargo.lock (criterion 0.7 transitive deps)"

key-decisions:
  - "criterion pinned to 0.7 not 0.8 — criterion 0.8.2 requires rustc 1.86, workspace pins to 1.85 (Rule 1 deviation; acceptance criteria are version-agnostic)."
  - "Per-bench kernels for ljung_box, rolling_corr, and ols_fit_4d are inlined as byte-identical mirrors of the production pub(crate) kernels rather than promoting visibility (plan's recommended approach). Each bench file's module-doc comment points at the production source path."
  - "csv added as a miner-core dev-dep (Rule 3 — the bench mirrors the production CSV parse callsite in miner-reader-dukascopy)."

patterns-established:
  - "criterion bench file canonical shape: SPDX header on lines 1-2, module-doc paragraph describing kernel + input + bench-name string + report path, use std::hint::black_box (NOT criterion::black_box — deprecated), input construction OUTSIDE b.iter, kernel call INSIDE wrapped in black_box, criterion_group! + criterion_main! at the bottom."
  - "Kernel-mirror-in-bench: when the production kernel is pub(crate), copy the function body verbatim into the bench file with a module-doc pointer to the source path. Avoids both visibility promotion and Scan::run envelope-build overhead."

requirements-completed:
  - FOUND-04

duration: ~50min
completed: 2026-05-22
---

# Phase 7 Plan 06: Criterion Microbench Layer 1 Summary

**Layer 1 of the D7-03 bench harness: six criterion microbench files exercising the hot kernels (zstd, csv, aggregator, rolling-corr, Ljung-Box, OLS-4D) with HTML reports under `target/criterion/`.**

## Performance

- **Duration:** ~50 minutes
- **Started:** 2026-05-22T10:46:56Z (first edit attempt against Cargo.toml)
- **Completed:** 2026-05-22T11:33:00Z (Task 2 commit landed)
- **Tasks:** 2 of 2 (both completed)
- **Files created:** 7 (six bench files + deferred-items.md)
- **Files modified:** 4 (Cargo.toml + miner-core/Cargo.toml + hygiene_dispatch.rs + null.rs)

## Accomplishments

- All six bench files compile under `cargo bench -p miner-core --no-run` with **zero warnings**.
- All six benches run end-to-end under `cargo bench -p miner-core --bench <name> --quick` and populate `target/criterion/<bench>/{index.html, report/, base/, new/, sample.json}` per criterion's HTML-report convention.
- Workspace-level `[profile.release] debug = 1` landed — precondition for Plan 07-08's dhat heap profiler (line-tables-only DWARF for symbol attribution per 07-RESEARCH Pitfall 1; rejects `debug = true` which would 5x release binary size).
- FOUND-04 invariant preserved: `cargo tree -p miner-core --edges normal,build` produces no tokio/async leak. criterion sits as a `[dev-dependencies]` entry on miner-core only; dhat is workspace-declared but not pulled into miner-core's dep graph (Plan 07-08 owns it via miner-bench).
- `cargo clippy -p miner-core --benches -- -D warnings` passes (the primary acceptance gate the plan's `<verify>` block requires).

### Empirical first timings (informational, from `--quick` runs)

| Bench | Timing (median) |
|---|---|
| `zstd_decompress_1day_eurusd` | 129.6 µs |
| `csv_parse_1day_eurusd` | 300.8 µs |
| `aggregate_1m_to_15m_360000_bars` | 31.7 ms |
| `rolling_corr_pearson_w100_n10000` | 1.24 ms |
| `rolling_corr_spearman_w100_n10000` | 34.0 ms |
| `ljung_box_q_p_n10000_lag5`  | ~250 µs |
| `ljung_box_q_p_n10000_lag10` | 354 µs |
| `ljung_box_q_p_n10000_lag20` | 473 µs |
| `ljung_box_q_p_n10000_lag50` | 1.36 ms |
| `ols_fit_4d_n10000` | 118 µs |

(Quick-mode samples on a single-machine workstation; not regression-grade. Future plans will own baselines via `--save-baseline`.)

## Task Commits

1. **Task 1: Add criterion + dhat workspace deps, [[bench]] entries, debug=1** — `6b918fc` (chore)
2. **Task 2: Author six criterion microbench files** (with bundled deviation fixes) — `83a8bb6` (feat)

## Files Created/Modified

### Created

- `crates/miner-core/benches/bench_zstd_decompress_1day.rs` — zstd decode of one fixture day; bench name `zstd_decompress_1day_eurusd`.
- `crates/miner-core/benches/bench_csv_parse_1day.rs` — CSV parse of decompressed bytes via `csv::ReaderBuilder` + serde deserialise; bench name `csv_parse_1day_eurusd`.
- `crates/miner-core/benches/bench_aggregate_1m_to_15m.rs` — synthesises 250 trading days × 1440 1m bars (= 360 000 bars) via the canonical LCG (PATTERNS Pattern C), then runs `miner_core::aggregator::aggregate` over an in-memory `Reader` impl. Bench name `aggregate_1m_to_15m_360000_bars`.
- `crates/miner-core/benches/bench_rolling_corr.rs` — two functions in one criterion_group: `bench_pearson_rolling` + `bench_spearman_rolling`, each window=100 over n=10000 synthetic close pairs. Kernels mirror production at `crates/miner-core/src/scan/cross/corr_rolling/kernel.rs`.
- `crates/miner-core/benches/bench_ljung_box.rs` — four bench functions sweeping lag ∈ {5,10,20,50} on n=10000 synthetic log-returns. Mirrors production at `crates/miner-core/src/scan/ljung_box/kernel.rs` (biased_acf + ljung_box_q_and_p).
- `crates/miner-core/benches/bench_ols_fit_4d.rs` — 4-column design matrix (intercept + 3 LCG regressors) × n=10000 rows; runs nalgebra normal-equations OLS. Bench name `ols_fit_4d_n10000`.
- `.planning/phases/07-hardening-benchmarks-reproducibility/deferred-items.md` — log of pre-existing `gen-fixtures.rs` clippy errors that fail the `--all-targets -D warnings` workspace gate (out of scope for Plan 07-06).

### Modified

- `Cargo.toml` — extended Phase 7 additions block with `criterion = { version = "0.7", features = ["html_reports"] }` and `dhat = "0.3"`. Added `[profile.release]` block with `debug = 1`. Comment updated to acknowledge both Plan 07-05's realfft add and Plan 07-06's adds.
- `crates/miner-core/Cargo.toml` — added `criterion.workspace = true` and `csv.workspace = true` under `[dev-dependencies]`. Registered six `[[bench]]` entries with `harness = false` (REQUIRED per 07-RESEARCH Pitfall 2).
- `crates/miner-core/src/engine/hygiene_dispatch.rs` — three Plan 07-05 doc-markdown backticks added (`PhaseScramble`, `lead_lag`, `engle_granger`); `use std::cell::RefCell;` moved to top of `pair_iaaft_phase_scramble_null_p`; `match { Some/None }` block rewritten as `let-else`.
- `crates/miner-core/src/scan/hygiene/null.rs` — `time_scratch.iter_mut()` → `&mut time_scratch`; two doc-markdown backticks added (`n_resamples`, `100_000`).

## Decisions Made

- **criterion 0.7 (not 0.8)** — Rule 1 deviation. criterion 0.8.2 requires rustc 1.86, the workspace pins to 1.85. criterion 0.7 is the latest minor compatible with the locked toolchain. The plan's acceptance criteria do not pin a specific version (`grep -cE '^\s*criterion\s*='` returns 1; version is not in the regex). When the workspace MSRV bumps to 1.86 a follow-up can bump criterion back to 0.8 with no API churn (the canonical API surface — `Criterion`, `criterion_group!`, `criterion_main!`, `black_box` — is unchanged).
- **Mirror kernels in bench files** — for the three kernels whose production form is `pub(crate)` (rolling_pearson/spearman, biased_acf/ljung_box_q_and_p, fit_ols_intercept_slope), each bench file inlines a byte-identical copy of the kernel math with a module-doc pointer back to the production source. The alternative paths the plan offered (a) promoting visibility to `pub` (pollutes the public surface) or (b) wrapping behind a `bench-internals` Cargo feature (extra build flag) or (c) calling `Scan::run` (adds envelope-build overhead that drowns the kernel timing) — each carried more cost than the inline mirror.
- **`std::hint::black_box` over `criterion::black_box`** — every bench uses `std::hint::black_box` because `criterion::black_box` is deprecated in 0.7 and warns on use (workspace warns turn into errors under the acceptance `-D warnings` gate).

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] criterion = "0.8" incompatible with locked rustc 1.85**

- **Found during:** Task 2 (`cargo bench -p miner-core --no-run`)
- **Issue:** criterion 0.8.2 requires rustc 1.86 (MSRV bump in the criterion 0.8 series). The workspace `[workspace.package] rust-version = "1.85"` pins to 1.85.1 (the installed toolchain).
- **Fix:** Pinned to `criterion = { version = "0.7", features = ["html_reports"] }` in `Cargo.toml`. The acceptance criteria are version-agnostic (`grep -cE '^\s*criterion\s*='` returns 1 in both cases). Plan's PATTERNS doc references "criterion 0.5+" as broadly compatible.
- **Files modified:** `Cargo.toml`
- **Verification:** `cargo bench -p miner-core --no-run` compiles cleanly with criterion 0.7.
- **Committed in:** `83a8bb6` (folded into Task 2 commit since the fix is paired with the bench file additions).

**2. [Rule 3 - Blocking] miner-core dev-deps missing `csv` for bench_csv_parse_1day**

- **Found during:** Task 2 (`cargo bench -p miner-core --no-run` after Task 1 commit)
- **Issue:** `bench_csv_parse_1day.rs` invokes `csv::ReaderBuilder` to mirror the production parse callsite in `crates/miner-reader-dukascopy/src/reader.rs`. miner-core has no `csv` dep at any scope (the production path lives in miner-reader-dukascopy, not miner-core).
- **Fix:** Added `csv.workspace = true` to miner-core's `[dev-dependencies]`. Dev-only — does NOT leak into the production graph; FOUND-04 invariant preserved.
- **Files modified:** `crates/miner-core/Cargo.toml`
- **Verification:** `cargo bench -p miner-core --no-run` compiles the csv bench cleanly. FOUND-04 gate (`cargo tree -p miner-core --edges normal,build`) shows no async leak.
- **Committed in:** `83a8bb6`

**3. [Rule 3 - Blocking] Pre-existing clippy::pedantic warnings in lib block the `--benches -- -D warnings` gate**

- **Found during:** Task 2 verification (`cargo clippy -p miner-core --benches -- -D warnings`)
- **Issue:** Six pre-existing pedantic warnings in `src/engine/hygiene_dispatch.rs` (Plan 07-05) and `src/scan/hygiene/null.rs` (Plan 07-05) were landed under a non-strict clippy run; the Plan 07-06 acceptance gate uses `-D warnings` which converts all six to errors. The lib must lint clean for the bench gate to even reach the bench files.
- **Fix:** Six zero-behaviour-change style fixes:
  1. `engine/hygiene_dispatch.rs:576` — three `doc_markdown` backticks (`PhaseScramble`, `lead_lag`, `engle_granger`).
  2. `engine/hygiene_dispatch.rs:626` — `items_after_statements`: moved `use std::cell::RefCell;` to top of `pair_iaaft_phase_scramble_null_p`.
  3. `engine/hygiene_dispatch.rs:654` — `manual_let_else`: `match { Some(s) => s, None => return f64::NAN }` rewritten as `let-else`.
  4. `scan/hygiene/null.rs:372` — `explicit_iter_loop`: `time_scratch.iter_mut()` → `&mut time_scratch`.
  5. `scan/hygiene/null.rs:672` — `doc_markdown` backtick on `n_resamples`.
  6. `scan/hygiene/null.rs:851` — `doc_markdown` backtick on `100_000`.
- **Files modified:** `crates/miner-core/src/engine/hygiene_dispatch.rs`, `crates/miner-core/src/scan/hygiene/null.rs`
- **Verification:** `cargo clippy -p miner-core --benches -- -D warnings` now exits 0. Unit-test behaviour unchanged — the fixes are pure style.
- **Committed in:** `83a8bb6`

### Deferred Issues

**1. `cargo clippy --workspace --all-targets -- -D warnings` fails on pre-existing `gen-fixtures.rs` lints**

- The plan's `<verification>` block lists the workspace-wide clippy gate but the failures live in `crates/miner-bench/src/bin/gen-fixtures.rs` (Plan 07-02 code, not Plan 07-06). Logged to `.planning/phases/07-hardening-benchmarks-reproducibility/deferred-items.md` for a follow-up cleanup plan. Plan 07-06's stricter `cargo clippy -p miner-core --benches -- -D warnings` gate — the gate the plan acceptance criteria explicitly require — passes.

## Verification Results

| Gate | Result |
|---|---|
| `cargo metadata --no-deps --format-version 1` | PASS (exit 0) |
| `cargo bench -p miner-core --no-run` | PASS (six bench binaries compile, zero warnings) |
| `cargo bench -p miner-core --bench <each> -- --quick` | PASS (all six produce timings + HTML reports) |
| `cargo tree -p miner-core --edges normal,build | grep -E '(tokio|async-)'` | PASS (empty — FOUND-04 preserved) |
| `cargo clippy -p miner-core --benches -- -D warnings` | PASS (exit 0) |
| `target/criterion/` populated | PASS (10 report subdirs — one per bench function across six bench files) |
| `cargo clippy --workspace --all-targets -- -D warnings` | DEFERRED (pre-existing failures in `gen-fixtures.rs`, see deferred-items.md) |
| Per-file SPDX + `criterion_main!` | PASS (all six bench files) |
| No `println!` in benches | PASS (`grep -c println! crates/miner-core/benches/*.rs` returns 0 across all files) |

## Known Stubs

None — every bench exercises a real kernel against real (synthetic-but-deterministic) inputs and produces real timing distributions.

## Notes for Plan 07-07 + 07-08

- **`cargo clean` may be required before Plan 07-08's first dhat profiling run** — the `[profile.release] debug = 1` setting only takes effect on freshly-built artifacts; cached release builds from before this commit lack the line-table debug info dhat needs for symbol attribution.
- **dhat is workspace-declared but NOT yet pulled into any crate.** Plan 07-08 adds `dhat.workspace = true` to `crates/miner-bench/Cargo.toml` behind a `dhat` feature and wires the `#[global_allocator]` static (per 07-RESEARCH Pattern 6). miner-core stays dhat-free (FOUND-04).
- **`miner-bench` may want to add a wholesale-runner that drives criterion across all six benches** — currently `cargo bench -p miner-core` runs all six but also runs lib unittests; future plans can decide whether to split the bench harness into a separate binary that excludes lib tests.

## Self-Check: PASSED

All files claimed exist:
- `crates/miner-core/benches/bench_zstd_decompress_1day.rs`: PRESENT
- `crates/miner-core/benches/bench_csv_parse_1day.rs`: PRESENT
- `crates/miner-core/benches/bench_aggregate_1m_to_15m.rs`: PRESENT
- `crates/miner-core/benches/bench_rolling_corr.rs`: PRESENT
- `crates/miner-core/benches/bench_ljung_box.rs`: PRESENT
- `crates/miner-core/benches/bench_ols_fit_4d.rs`: PRESENT
- `.planning/phases/07-hardening-benchmarks-reproducibility/deferred-items.md`: PRESENT

All commit hashes valid:
- `6b918fc` (Task 1): in git log
- `83a8bb6` (Task 2): in git log
