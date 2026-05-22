# Milestones

## v1.0 v1.0 MVP (Shipped: 2026-05-22)

**Phases completed:** 7 phases, 50 plans, 124 tasks

**Key accomplishments:**

- Virtual Cargo workspace (resolver=3, edition 2024, MSRV 1.85) with seven crates, locked dependency table, rust-toolchain pin, xtask alias, and build.rs-driven `MINER_CODE_REVISION` injection — `cargo build --workspace` compiles cleanly and `miner-core` is provably zero-async.
- Both Risk 2 (schemars 1.x base64-with-shape) and Risk 5 (figment + clap CLI-wins precedence) verified — RESEARCH §Pattern 2 and §Pattern 4 may be implemented verbatim by Plans 03 and 05 respectively. Zero fallbacks required.
- The Finding envelope contract is now the source of truth for Phase 1 and beyond. All five variants compile with `Serialize + Deserialize + JsonSchema`; all map-typed fields are `BTreeMap`; `RunId` is `Copy`; `RunSummary` is `Default`; `dsr`/`fdr_q` serialise as JSON `null`; the seven locked common fields are inlined into each applicable variant; spike modules from Plan 01-02 are deleted. The `lib.rs` FROZEN public surface exposes 24 names — every one Plans 05/06/07 import. 14 unit tests pass; `cargo build -p miner-core` and `cargo build --workspace` both succeed.
- FOUND-02 (stdout = findings, stderr = logs, CI-enforced via clippy) and OUT-01 (NDJSON-on-stdout with per-envelope flush) both land in this plan. Three layers of T-01-03 defence are now active: (1) `StdoutSink` is the only type that opens `io::stdout()`, (2) workspace `clippy.toml` mechanically rejects `println!` / `eprintln!` / `print!` / `eprint!` / `dbg!` everywhere except two audited exemptions (`build.rs` for the cargo build-script protocol, `xtask` for dev-only command output), (3) `stderr_emit` is the sanctioned stderr writer for structured pre-flight errors so contributors never reach for `eprintln!`. `cargo clippy --workspace --all-targets -- -D warnings` runs clean; 22 unit tests pass; the manual sanity-check (inject `println!`, confirm clippy rejects, revert) was performed and the file was restored to a clean state before commit.
- FOUND-05 (CLI > env > TOML > error precedence with zero hardcoded paths in the library) and FOUND-02 (tracing → stderr in every binary's main) are now satisfied end-to-end. `miner emit-fixture` produces 2 JSONL lines on stdout + a tracing log on stderr and exits 0. Preflight failures emit a single WireError JSON line to stderr with the correctly-classified `code` field (`missing_required_config` vs `invalid_config`) and exit 1. The figment-error-kind classification is locked by Test 7 in miner-core and consumed by `classify_figment_error` in miner-cli; the BLOCKER from PLAN 05's must_haves ("mapping every figment error to MissingRequiredConfig is FORBIDDEN") is fixed.
- FOUND-03 (locked envelope JSON schema with single source of truth via schemars) and FOUND-04 (CI-enforced async-runtime-free miner-core) both land in this plan. `cargo run -p xtask -- gen-schema` now produces a byte-deterministic `schemas/findings-v1.schema.json` (proven by twice-run `cmp` against a separate path); `.github/workflows/ci.yml` runs the four mandatory D-21 gates — build, clippy with `-D warnings`, tokio-tree grep against miner-core, and schema-sync diff — plus fmt + test hygiene; T-01-02 (schema drift) and T-01-03 (stdout pollution) are now mechanically enforced on every PR.
- FOUND-02, FOUND-03, FOUND-05, OUT-01, OUT-02 and OUT-03 (FULL closure, not partial) all

land in this plan as automated integration tests. `cargo test --workspace` now exercises
the locked envelope schema against every `Finding` variant at runtime (D-22), proves the
CLI > env > TOML > error precedence works through the public re-export surface, spawns
the actual `miner` binary via `assert_cmd` and asserts stdout/stderr split + per-line
schema validation + masked twice-run byte-identity. README.md ships the Phase 1
Quickstart. All seven sign-off gates exit 0 locally. Phase 1 is complete.

- Wave 0 reader foundation: `miner_core::Reader` trait + `DukascopyReader` zstd-CSV impl + sealed `DukascopyMonth` 00-indexed path encapsulation + `SyntheticCache` test fixture — unblocks every subsequent Phase 2 wave.
- Pure-function `aggregate(reader, AggParams)` kernel that emits 15m / 1h / 1d BarFrames from 1m source bars via deterministic UTC bucketing and gap omission, plus the FX-major closed-form `Calendar::is_open_at` predicate shared with Plan 04's gap detector.
- 11 integration-test functions across 3 sibling test files that pin the aggregator's UTC-only contract (DST invisibility) and gap-omission semantics (weekend / holiday / instrument cache boundary / partial session open), closing CACHE-04 success criterion 5 on the aggregator side.
- Wave 1 gap-detection data model: `miner_core::gap` exports `GapDetector` (pure-function `detect`), the `GapManifest`/`GapSpan`/`GapReason` types, and the insta-pinned JSON wire form. CACHE-07 closed; CACHE-08 ships the type Phase 3 will emit.
- Read-mostly derived-bar cache: one Arrow IPC file per `(source_id, symbol, side, timeframe)` quartet plus a sibling `<…>.fingerprints.json` sidecar carrying per-day blake3 fingerprints. Two-axis invalidation (full-rebuild on version drift; day-splice on per-day fingerprint drift) with crash-safe atomic writes. CACHE-06 CLOSED.
- End-to-end byte-identity gate proves the Reader → aggregate → BarCache pipeline is fully deterministic; FROZEN public-surface audit gates all 20 Phase 2 re-exports; standalone DukascopyReader dyn-compat test seals CACHE-02; VALIDATION.md marks Phase 2 closed. Phase 2 COMPLETE.
- 1. [Rule 3 - Blocking issue] Tokio dev-dep transitive (jsonschema → reqwest → tokio) appears in `cargo tree -p miner-core`
- 1. [Rule 3 - Blocking issue] `ClosedRangeUtc` lacks Serialize/Deserialize/JsonSchema
- `engine/param_hash.rs` (155 lines, 4 unit tests):
- 1. [Rule 1 - Bug] cancel_before_subrange original hole position produced Tf15m-misaligned sub-range
- 1. [Rule 3 — Blocking issue] cfg-gating strategy required dev-dep feature propagation
- 1. [Rule 3 - Blocking issue] miner-core integration tests cannot reach the cfg-gated `ScanRequest.sleep_after_first_finding_ms` field by default
- Typed MinerError::Preflight variant + reader/cache/scan-IO/scan-miner-error wrapping + cancel-honoring exit-code routing — closes all three Phase 3 code-review gaps and restores the D-09 framing invariant + D3-24 cancel-overrides-everything contract.
- Phase 4 facade-shape extension landed: Scan::arity() trait method, ScanArity enum, InstrumentSpec struct, ScanRequest.instruments Vec, DataSlice.sources Vec, PreflightCode::WrongInstrumentArity, and ndarray/ndarray-stats/nalgebra workspace deps — D4-01/02/03 in place ahead of the 22-scan rollout.
- Wave 2 wired the Phase 4 D4-02 / D4-04 / D4-06 facade extensions: returns + time-alignment + raw-array primitives, arity preflight, two-leg gap dispatch, ScanCtx.bars_pair + bars_up_to(ts) look-ahead-safety API, per-family registrar stubs, repeatable `--instrument SYMBOL:side` CLI flag, and the `arity` field on the `miner scans` catalogue. The 22-scan rollout in Waves 3-7 now consumes a stable, self-contained primitives + registrar surface.
- Wave-3 shipped the first three callable ANOM scans (`stats.returns.profile@1`, `stats.summary.welford@1`, `stats.vol.rolling@1`) following the Phase 3 LjungBox gold-standard pattern. Every scan is registered via `scan::anom::register_anom_scans` (Pattern E) so `crates/miner-core/src/scan/registry.rs::bootstrap()` stays untouched as the canonical entry point. The look-ahead-safety proptest for VolRollingScan is committed alongside the scan and passes for 1000 seeds.
- Wave-4 shipped three single-shot ANOM scans (`stats.autocorr.ljung_box_sq@1`, `stats.outliers.z_and_mad@1`, `stats.drawdown.profile@1`) completing the "easy" half of ANOM. ANOM-04 (squared variant), ANOM-10 (outliers), and ANOM-11 (drawdown) are now shipped; Plans 04-05/04-06 own the five hand-derived heavyweight tests (ADF, KPSS, VR, ARCH-LM, Jarque-Bera). Phase 3 LjungBox golden continues to pass byte-identically — D4-06 / Pitfall 9 invariant preserved.
- Wave-5 shipped three hand-derived heavyweight ANOM scans (`stats.stationarity.adf@1`, `stats.stationarity.kpss@1`, `stats.variance_ratio.lo_mackinlay@1`) covering the three stationarity / autocorrelation tests flagged as "not in any comprehensive Rust crate" by STATE.md Phase 4 implementation risk. Each kernel is hand-derived against scipy/statsmodels/arch references with hand-derivable closed-form unit tests; full statsmodels parity goldens land in Plan 04-11. Phase 3 LjungBox golden continues to pass byte-identically — Pitfall 9 invariant preserved.
- 1. [Rule 1 — Bug] ARCH-LM `debug_assert!(lag >= 1)` removed in favour of `Err` return
- Three new Pair-arity CROSS scans (Pearson rolling correlation, Spearman rolling correlation, OLS rolling regression) landed via the `register_cross_scans` per-family helper. Each scan inner-joins the two legs once via the CROSS-01 primitive, computes per-leg log returns, runs a per-window kernel, and emits exactly one `Finding::Result` envelope with vector arrays in `effect.extra` and leg-labelled keys in `raw.series` (D4-03). The engine's Pair branch in `run_one_with_registry` is NOT touched — the scans are validated end-to-end via direct `Scan::run` dispatch in integration tests; the full engine-side Pair dispatch is deferred to Plan 04-11.
- Closes the Phase 4 v1 catalogue contract: 22 registered scans + 1 ANOM-04 squared variant pinned by integration tests; ROADMAP SC#4 (consistent envelope shape) locked by `byte_identical_rerun.rs`; SC#5 (golden fixtures) pinned by three `#[ignore]`d cross-check tests gated on provenance — green after a pinned-Python 3.11 venv regenerates the JSONL goldens.
- Status:
- Schema-additive `Finding` envelope extensions (EffectSize, ReproEnvelope, SweepSummary variant), default-false `Scan::supports_bootstrap`/`supports_null_method`, `PreflightCode::HygieneNotSupported`, and `rand` + `rand_xoshiro` + `toml` workspace deps — Phase 5 type-system foundation that Plans 05-02 through 05-05 build against without revisiting.
- Five pure-math hygiene kernels (`effect_size`, `bootstrap`, `null`, `fdr`, `seed`) shipped under the new `crates/miner-core/src/scan/hygiene/` module. Zero new workspace dependencies. 32 unit tests pass in 60 ms total. IAAFT phase-scramble deferred to Phase 7; null.rs ships only `circular_shift_null_p`.
- `BootstrapMethod` enum + six additive `ScanRequest` Option fields +

per-scan `supports_bootstrap`/`supports_null_method` overrides on 19 of 22
Phase 4 scans + `Effect.effect_size` populated on every scan with the
canonical D5-03 kind + `engine::preflight::validate_hygiene_support`
rejecting unsupported method requests. Post-`Scan::run` hygiene-kernel
invocation (CI / p-value population + ReproEnvelope) intentionally deferred
to a follow-up plan.

- Sweep runner end-to-end: TOML manifest deserialisation +

cartesian expansion + rayon-parallel job fanout with
deterministic-order buffered drain + end-of-sweep BH-FDR aggregation

+ `Finding::SweepSummary` envelope emission. `DryRunFinding` gained

the `planned_job_count` additive field. New workspace dep
`rayon 1.10`. Five integration tests pin the contract: sweep_smoke,
sweep_dry_run, sweep_summary_emission, sweep_byte_identical_rerun
(no-hygiene + hygiene-on), and fdr_family_scoping (all four
`[fdr].family` enum values). 750 lib tests + 9 new integration
tests pass; FOUND-04 invariant preserved (no tokio/async-std);
schema diff is 10 insertions only.

- `miner sweep <manifest.toml>` subcommand wired end-to-end with the

four-tier exit-code router; universal `miner scan` hygiene flags
(`--bootstrap`, `--bootstrap-n`, `--null`, `--null-n`, `--seed`)
mirroring the manifest `[hygiene]` grammar; `cargo xtask gen-schema`
extended to publish `schemas/sweep-manifest-v1.schema.json`; README
Quickstart for the Quant-agent workflow; R 4.4.x / `tseries` /
`stats` reference pinning; two new integration tests
(sweep_subcommand_smoke + sigint_mid_sweep) prove end-to-end behaviour
of `miner sweep` against the synthetic Dukascopy cache. Phase 5
sign-off ready.

- Phase 6 reshaped from CODE (rmcp MCP server + axum HTTP server) to DOCS (design contract + docs/ folder); OP-02 + OP-03 reclassified to v2 (PLAT-v2-07, PLAT-v2-08); root ARCHITECTURE.md published as the public-audience system map; canonical Apache-2.0 footer template seeded for re-use by Plans 06-02 / 06-03.
- The three reference docs that describe miner's locked Finding envelope, its 23-scan v1 inventory, and the TOML sweep grammar are published; each carries the canonical Apache-2.0 footer byte-identical to docs/.license-footer.md, and every documented field / variant / scan_id / TOML block name has a verified source match under crates/miner-core/src/.
- Phase 6 closes with the consumer-facing CLI subprocess guide (docs/agent_integration.md), the architectural sketch for the deferred MCP + HTTP wrappers (docs/future_mcp_http.md), two runnable examples under docs/examples/, the README ## Documentation cross-link section, and D6-08 placeholder-main retargeting — all without leaking a single async dependency into miner-core or touching the two wrapper-crate Cargo.toml files.
- uv-driven pinned-Python-3.11 regen recipe lands `scripts/regen-goldens.sh`; three family goldens (ANOM-02 / CROSS-05 / SEAS-01) regenerated against scipy 1.14.1 / statsmodels 0.14.6 / pandas 2.2.3; the three previously `#[ignore]`d golden-parity tests are now active under `cargo test --workspace`.
- Synthetic Dukascopy-shape fixture cache scaffold + deterministic Rust generator (`gen-fixtures`) writing LCG-seeded CSV.zst bytes at single-threaded zstd level 3, plus README ## Example block swapped to `./tests/fixtures/cache` + `seas.bucket.hour_of_day@1` per D7-01.
- Supply-chain CI gates landed via deny.toml (v2 schema, 9-license allowlist) plus two new CI steps wired through rustsec/audit-check@v2.0.0 and EmbarkStudios/cargo-deny-action@v2.
- 1. [Rule 3 - Verification gate] Lowercase `noise-replay` substring
- IAAFT phase-scramble null kernel (Theiler 1992) sibling to circular_shift_null_p — closes the largest non-doc verification-debt item from Plan 05-02 — plus a 250-job synthetic-null regression test proving BH-FDR controls multiple testing at α=0.05 (≤30 false positives) AND byte-identical SweepSummary across reruns (HYG-05).
- Layer 1 of the D7-03 bench harness: six criterion microbench files exercising the hot kernels (zstd, csv, aggregator, rolling-corr, Ljung-Box, OLS-4D) with HTML reports under `target/criterion/`.
- New `docs/data_sources.md` deep reference (six required sections covering cache layout, CSV schema, bid/ask independence, time zones + DST, gap policies, and licensing posture) plus a 6-line README `## Data source caveats` summary block linking to it.
- Replace miner-bench placeholder with the production recipe-runner binary; wire dhat-rs behind a miner-bench-only `--features dhat` Cargo gate; ship hyperfine + dhat wrapper scripts + canonical `docs/bench-results.md`.
- Hand-rolled byte-equal envelope-snapshot test + pinned `envelope_snapshot.jsonl` golden — the byte-determinism gate that closes ROADMAP Phase 7 success criterion #1.

---
