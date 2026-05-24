# Changelog

All notable changes to tradedesk-miner are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- **`continuous_only` (and `strict`) gap policies are now timeframe-aware.** Previously, every sub-minute hole during open hours split the requested window into a separate sub-range — so a multi-week scan at `--timeframe 1d` was shredded into hundreds of single-day sub-ranges, most of which `snap_subranges_to_timeframe` then dropped for being shorter than one bucket. The engine now projects the gap manifest onto the requested aggregation timeframe via the new `engine::gap_policy::effective_manifest_for_timeframe` helper before dispatching: a hole counts as a gap only when it fully covers at least one bucket at the requested `tf`. The raw 1-minute manifest is still preserved in `Finding::Result.data_slice.gap_manifest` and `Finding::GapAborted.gap_manifest` so data-quality information is not lost. (RAD-2642.)

### Added

- `engine::gap_policy::effective_manifest_for_timeframe(&GapManifest, Timeframe) -> GapManifest` projects a 1-minute-resolution gap manifest onto a requested aggregation timeframe; documented in module docs with the RAD-2642 rationale.
- `engine::gap_policy::dispatch_at_timeframe` and `dispatch_pair_at_timeframe` — thin wrappers that compose `effective_manifest_for_timeframe` with the existing `dispatch` / `dispatch_pair` primitives. The engine's single-leg and pair-arity facade now route through these wrappers.
- New regression tests in `engine::gap_policy::tests` (six unit tests + one proptest) and one new integration test (`run_one_absorbs_sub_bucket_hole_at_15m`).
- Bench harness — six criterion microbenches under `crates/miner-core/benches/`, the `miner-bench` recipe runner replacing the Phase 1 placeholder, `scripts/run-bench.sh` hyperfine wrapper, and `scripts/run-alloc-profile.sh` dhat wrapper.
- IAAFT phase-scramble null kernel (`iaaft_phase_scramble_null_p` in `crates/miner-core/src/scan/hygiene/null.rs`); `Scan::supports_null_method(NullMethod::PhaseScramble)` now returns `true` for the five scans documented in 07-RESEARCH.md Pattern 4.
- Clone-and-run fixture cache at `tests/fixtures/cache/` (synthetic-stub bytes; no Dukascopy-licensed bytes); deterministic generator at `scripts/generate-fixture-cache.sh` + `crates/miner-bench/src/bin/gen-fixtures.rs`; byte-identity gated by `tests/fixtures/cache/SHA256SUMS`.
- `docs/data_sources.md` — Dukascopy caveats reference (cache layout, CSV schema, bid/ask independence, time zones + DST, gap policies, licensing posture).
- `docs/bench-results.md` — single canonical home for wall-clock numbers, allocation budget, and the reference flamegraph.
- `cargo audit` + `cargo deny check` CI gates; `deny.toml` allowlist at repo root.
- Findings-envelope snapshot test (`crates/miner-core/tests/findings_envelope_snapshot.rs`) + locked `tests/goldens/envelope_snapshot.jsonl`.
- noise-replay sweep regression test (`crates/miner-core/tests/noise_replay_regression.rs`) — 300-job synthetic-null sweep proving BH-FDR controls multiple-testing.
- `scripts/regen-goldens.sh` — uv-driven pinned-Python-3.11 regen recipe; CONTRIBUTING.md `## Regenerating goldens` subsection.

### Changed

- `crates/miner-bench/src/main.rs` — Phase 1 placeholder replaced with the recipe-runner binary.
- `README.md` — `## Example` block now uses the new fixture cache (`MINER_CACHE_ROOT=./tests/fixtures/cache`); added `## Data source caveats` summary; added `## Performance` pointer to `docs/bench-results.md`.
- `Cargo.toml` workspace — added `criterion`, `dhat`, and `realfft` to `[workspace.dependencies]`; added `[profile.release] debug = 1` for dhat symbol attribution.
- `CONTRIBUTING.md` — extended `## Quality gates` table with `cargo audit` + `cargo deny check` (rows 7-8); added the samply profiler subsection.

### Fixed

- Family golden tests un-`#[ignore]`d after pinned-Python-3.11 regen (Plan 04-11 deferred this; Plan 07-01 closes it).

## [1.0.0] — TBD (v1.0 sign-off after Phase 7 ships)

### Added

- **Phase 1 (Foundations & Contracts):** Rust workspace with `miner-core` library + `miner-cli` / `miner-mcp` / `miner-http` / `miner-bench` binaries; locked `Finding` envelope JSON schema; stdout=findings / stderr=logs CI-enforced discipline; figment config precedence (flag > env > file).
- **Phase 2 (Reader, Aggregator & Derived-Bar Cache):** Dukascopy reader against the existing zstd-CSV cache; deterministic UTC-aligned bar aggregator at 15m / 1h / 1d; Arrow IPC derived-bar cache with `(aggregator_version, per-day fingerprint)` two-axis invalidation; structured gap manifest.
- **Phase 3 (Scan Engine, Facade & CLI):** `Scan` trait + registry; engine facade with look-ahead-safe windowing; `strict` and `continuous_only` gap policies; CLI wrapper with four-tier exit codes + SIGINT cleanup; first end-to-end scan (Ljung-Box).
- **Phase 4 (Scan Catalogue):** 22 v1 scans across ANOM (11), CROSS (5), SEAS (6) families — every scan emits the locked `Finding` envelope; three family goldens pinned bit-for-bit against scipy / statsmodels / pandas.
- **Phase 5 (Statistical Hygiene & Sweep Runner):** effect sizes (Cohen's d / Hedges' g / Cliff's delta / VR-minus-one); block + stationary bootstrap CIs (Politis-Romano); circular-shift null distributions (IAAFT lands in Phase 7); Benjamini-Hochberg FDR at sweep level; TOML sweep manifest with parallel rayon executor; bit-for-bit reproducible via `repro` envelope.
- **Phase 6 (MCP & HTTP Wrappers — Docs-Only):** root ARCHITECTURE.md; docs/ folder with `findings_envelope.md`, `scan_catalogue.md`, `sweep_manifest.md`, `agent_integration.md`, `future_mcp_http.md`; MCP + HTTP wrapper implementation deferred to v2 (PLAT-v2-07 + PLAT-v2-08).

---

## License

Licensed under the Apache License, Version 2.0.
See: https://www.apache.org/licenses/LICENSE-2.0

Copyright 2026 [Radius Red Ltd.](https://github.com/radiusred) | [Contact](mailto:opensource@radiusred.uk)
