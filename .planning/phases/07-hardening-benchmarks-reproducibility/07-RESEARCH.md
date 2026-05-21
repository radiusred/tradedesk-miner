# Phase 7: Hardening, Benchmarks & Reproducibility — Research

**Researched:** 2026-05-22
**Domain:** Rust hardening — golden-file regression, IAAFT noise replay, bench harness (criterion + dhat-rs + samply + hyperfine), CI security gates (cargo audit + cargo deny), data-source caveats doc, clone-and-run fixture cache
**Confidence:** HIGH on tool versions and APIs; HIGH on existing code paths to extend; MEDIUM on the IAAFT pattern (research-stage in Plan 05-02-SUMMARY)

## Summary

Phase 7 is verification-debt closure for v1.0. Every deliverable extends an existing pattern — three new criterion micro-benches go under `crates/miner-core/benches/` (does not yet exist); the `miner-bench` placeholder gets fully replaced; the IAAFT phase-scramble kernel deferred by Plan 05-02 lands in `crates/miner-core/src/scan/hygiene/null.rs` next to the existing `circular_shift_null_p`; the envelope-snapshot test reuses the Plan 04-11 `Pattern J Step 1` provenance-gate + insta-snapshot scaffolding already in place. cargo-deny + cargo-audit slot into `.github/workflows/ci.yml` after the existing `schema sync` step using the canonical `EmbarkStudios/cargo-deny-action@v2` and `rustsec/audit-check@v2.0.0` actions.

**Primary recommendation:** Reuse every existing pattern (LCG closes + `lcg_closes(672, 0xDEAD_BEEF)`, masking via `mask_volatile_fields`, `Pattern J Step 1` provenance gate, `Xoshiro256PlusPlus` RNG, `insta::assert_json_snapshot!`, `EmbarkStudios/cargo-deny-action@v2` workflow). Add three new workspace deps (`criterion`, `dhat`, `realfft`) with the dhat-rs gated behind a `dhat` Cargo feature on the `miner-bench` crate only — `miner-core` stays tokio-free and dhat-free.

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| Golden-file regression suite | `miner-core/tests/` integration tests | `tests/goldens/*.jsonl` data | Existing Pattern J Step 1 (provenance gate) + Plan 04-11 already wired in `cargo test --workspace` |
| Envelope snapshot test | `miner-core/tests/` integration test | `insta` snapshot under `tests/snapshots/` | `insta = "1.47"` already a dev-dep; existing pattern `mask_volatile_fields` + `insta::assert_json_snapshot!` |
| IAAFT noise-replay regression | `miner-core/src/scan/hygiene/null.rs` (kernel) + `miner-core/tests/noise_replay_regression.rs` (sweep test) | `realfft` workspace dep (NEW) | Plan 05-02-SUMMARY explicitly defers IAAFT to Phase 7; the existing `circular_shift_null_p` is the sibling pattern |
| Criterion microbenches | `crates/miner-core/benches/` (NEW dir) | `criterion = "0.8.2"` dev-dep | Canonical Rust layout; STACK.md §"Bench Harness" mandates this split |
| `miner-bench` recipe runner | `crates/miner-bench/` binary | reads TOML, spawns `miner sweep` (or in-process API) | Existing crate placeholder gets fully replaced; criterion lives in miner-core, NOT miner-bench |
| dhat-rs allocation profiling | `miner-bench` binary behind `dhat` Cargo feature | `dhat = "0.3.3"` dep | Feature-gated `#[global_allocator]`; cold path; default off |
| samply / hyperfine wrappers | `scripts/run-bench.sh` (NEW) | external tools (no Rust deps) | Plain shell scripts; documented in CONTRIBUTING.md |
| cargo audit + cargo deny CI gates | `.github/workflows/ci.yml` + `deny.toml` (NEW root file) | Existing schema-sync step is the slot-in point | Two new YAML steps after `name: schema sync` |
| Synthetic fixture cache | `tests/fixtures/cache/` (NEW root-of-repo dir) + `scripts/generate-fixture-cache.sh` (NEW) | Workspace-shared; not crate-scoped | D7-01 locked; recommend synthetic-stub generator (no Dukascopy bytes redistributed) |
| docs/data_sources.md | NEW root-of-docs file | mirrors `tradedesk/docs/data_sources_guide.md` sibling layout | Apache-2.0 footer byte-identical to `docs/.license-footer.md` |
| docs/bench-results.md | NEW root-of-docs file | flamegraph PNGs + dhat-heap.json summary | Single canonical home for perf numbers (NOT in README) |

## User Constraints (from CONTEXT.md)

### Locked Decisions

**D7-01 (Fixture cache + clone-and-run quickstart):** `tests/fixtures/cache/` at repo root. Two instruments × bid side × one month each ≤ 5 MB compressed. Generator script `scripts/generate-fixture-cache.sh` produces synthetic stub bytes from a deterministic seed (NOT real Dukascopy data). SHA256SUMS file pins byte-identity. Quickstart command:
```sh
MINER_CACHE_ROOT=./tests/fixtures/cache cargo run -p miner-cli -- scan seas.bucket.hour_of_day@1 \
    --instrument EURUSD:bid --timeframe 15m --window 2024-01-01:2024-01-31
```

**D7-02 (Data-source caveats):** README adds ~6-line summary after `## Example`; `docs/data_sources.md` is the deep doc covering Cache layout / CSV schema / Bid vs ask independence / Time zones and DST / Gap policies / Licensing posture.

**D7-03 (Bench harness):** Three layers — `crates/miner-core/benches/*.rs` criterion microbenches; `crates/miner-bench/src/main.rs` recipe binary; `scripts/run-bench.sh` hyperfine wrapper. `dhat` feature on `miner-bench` only. samply documented in CONTRIBUTING.md; one reference flamegraph PNG in `docs/bench-results/`.

**D7-04 (Noise-replay regression):** `crates/miner-core/tests/noise_replay_regression.rs`. 100 GBM-seeded synthetic series (seed `0xC0FFEE_C0FFEE`), IAAFT phase-randomised, 300 jobs at α=0.05, assert `false_positive_count <= 30`. Phase 7 lands IAAFT.

**D7-05 (Security gates):** `cargo audit` step + `cargo deny check` step in CI. `deny.toml` at repo root. License allowlist: `["Apache-2.0", "MIT", "BSD-2-Clause", "BSD-3-Clause", "ISC", "Unicode-DFS-2016", "Unicode-3.0", "Zlib", "MPL-2.0"]`. Multiple-versions = warn (not deny); wildcards = deny; vulnerability/unmaintained/unsound/yanked = deny.

**D7-06 (Golden discipline):** Three existing family goldens (`stats.summary.welford.jsonl`, `cross.cointegration.engle_granger.jsonl`, `seas.bucket.hour_of_day.jsonl`) pinned bit-for-bit. NEW: `tests/findings_envelope_snapshot.rs` against `tests/goldens/envelope_snapshot.jsonl`. CONTRIBUTING.md gets a `## Regenerating goldens` section.

**D7-07 (Bench-results location):** `docs/bench-results.md` is the single canonical home for perf numbers. README has only a one-line pointer.

### Claude's Discretion (Plan-phase finalises)

- Noise-replay test `#[ignore]` vs always-run.
- `miner-bench` recipe TOML shape — `SweepManifest` directly or a wrapper.
- `docs/bench-results.md` initial "How to reproduce" placement.
- cargo deny license allowlist baseline — verified against `Cargo.lock`.
- CHANGELOG.md scaffold — include or push to v1.0 close.
- README pointer to `docs/bench-results.md` — exact line placement.

### Deferred Ideas (OUT OF SCOPE)

- Property-test harness for new scans (later hardening pass).
- Lockfile-age automation (Dependabot/Renovate) — v1.x or v2.
- Performance-regression CI gate (variance too noisy on shared runners).
- `#[ignore]` cleanup audit (pre-`/gsd-complete-milestone` housekeeping).
- CHANGELOG.md scaffold — listed deferred but cheap; plan-phase decides.

## Phase Requirements

Phase 7 closes verification debt for these already-implemented requirements. No new REQ-IDs.

| ID | Description | Research Support |
|----|-------------|------------------|
| FOUND-02 | Findings → stdout, logs → stderr | Existing `clippy::disallowed_macros` gate; golden tests assert no log leakage |
| FOUND-03 | Locked `Finding` envelope schema | Envelope-snapshot test pins JSONL form against `tests/goldens/envelope_snapshot.jsonl` |
| FOUND-04 | Sync-core (no tokio in miner-core) | Existing CI gate `cargo tree -p miner-core --edges normal,build`; Phase 7 preserves by gating `dhat` to `miner-bench` only |
| CACHE-04 | Deterministic close-aligned UTC bar aggregation, omits gaps | Synthetic fixture cache exercises the aggregator end-to-end via the clone-and-run quickstart |
| OUT-03 | Deterministic output ordering for golden-file diffing | Existing `cli_streams::emit_fixture_byte_identical_when_volatile_fields_masked` pattern extended to three family goldens + envelope snapshot |
| HYG-02 | Benjamini-Hochberg FDR adjustment at sweep level | Noise-replay regression test proves BH-FDR controls multiple-testing on a null dataset |
| HYG-05 | Bit-for-bit reproducibility via echoed seed | Noise-replay test asserts byte-identical `SweepSummary` across two runs with same seed |

## Project Constraints (from CLAUDE.md)

These directives MUST be honored by plan-phase. They have the same authority as locked CONTEXT decisions.

- **Rust 1.85+ stable, edition 2024.** All new crates / benches inherit from workspace.
- **License: Apache-2.0.** All new files (scripts, docs, source) carry SPDX header or footer.
- **GSD workflow enforcement.** Phase 7 work goes through `/gsd-execute-phase` or `/gsd-quick`; no raw edits outside a GSD command.
- **No emojis in commit messages or code.**
- **`unsafe_code = "forbid"`** workspace-wide. dhat-rs uses internal unsafe but is gated behind a feature on a separate crate (miner-bench), so the forbidden-unsafe lint on miner-core is preserved.
- **One-way dependency direction.** `miner-cli | miner-mcp | miner-http → miner-reader-dukascopy → miner-core`. Phase 7 introduces zero new edges.
- **Tokio-free `miner-core`.** `cargo tree -p miner-core --edges normal,build` must continue showing zero `tokio | async-std | smol | async-trait | async-io | async-channel | async-executor | async-task` deps. Verified: criterion 0.8.2, dhat 0.3.3, realfft 3.5.0 are all sync-only (`tokio-free miner-core` step in `.github/workflows/ci.yml` lines 60–72 is the canonical check).
- **Stdout = findings, stderr = logs.** Workspace `clippy.toml` `disallowed-macros` lint covers Phase 7 modules automatically.
- **BTreeMap discipline.** Any new map on the Serialize path is `BTreeMap` (NEVER `HashMap`).
- **`miner-mcp` + `miner-http` placeholder invariant (D6-08).** Phase 7 must NOT add any Cargo.toml deltas to those two crates.

## Standard Stack

### Core (NEW workspace dependencies in Phase 7)

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `criterion` | 0.8.2 | Microbenchmark harness for `crates/miner-core/benches/` | STACK.md §"Bench Harness" mandates; canonical Rust micro-bench framework with `--save-baseline` regression mode `[VERIFIED: crates.io cargo search 2026-05-22]` |
| `dhat` | 0.3.3 | Heap profiling via `dhat::Alloc` global allocator (feature-gated on `miner-bench`) | CLAUDE.md "Development Tools" table; only sane Rust heap profiler with structured JSON output `[VERIFIED: docs.rs/dhat]` |
| `realfft` | 3.5.0 | Real-to-complex FFT for IAAFT phase-scramble kernel (Phase 5 deferred this) | Plan 05-02-SUMMARY §"IAAFT Decision" line 167 names `realfft = "3"` explicitly; `crates/miner-core/Cargo.toml:63-64` already documents the intentional-exclusion comment `[VERIFIED: crates.io cargo search + Plan 05-02-SUMMARY]` |

### Supporting (already in workspace — REUSE, no Cargo.toml change)

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `insta` | 1.47.2 | Envelope snapshot test (recommended over hand-rolled byte-equal) | `crates/miner-core/Cargo.toml:93` already enables `["json"]` feature; existing tests use `insta::assert_json_snapshot!` (e.g., `scan_summary_welford.rs:142`) `[VERIFIED: Cargo.lock 1322-1323]` |
| `rand_xoshiro` | 0.6.0 | Deterministic Xoshiro256PlusPlus for IAAFT phase randomisation | `crates/miner-core/src/scan/hygiene/null.rs:25` already uses this; Phase 5 contract pinned `[VERIFIED: Cargo.lock 2222-2223]` |
| `serde_json` | 1.x | JSONL parse for goldens + envelope snapshot | NO features list (per workspace Cargo.toml comment, line 38 — `preserve_order` would break BTreeMap byte-determinism) `[VERIFIED: Cargo.toml 38]` |
| `chrono` | 0.4 | Timestamps for synthetic fixture generator | Already workspace dep `[VERIFIED: Cargo.toml 40]` |
| `zstd` | 0.13 | Compress synthetic-fixture CSVs at level 3 (matches `tradedesk-dukascopy/export.py:442`) | Already workspace dep; level 3 is the Dukascopy producer level `[VERIFIED: Cargo.toml 60 + tradedesk-dukascopy/export.py:442]` |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `criterion` | `divan = "0.1.21"` | Simpler output; STACK.md "What NOT to Use" §"Mixed criterion + divan" forbids mixing — pick one. criterion is the workspace default. |
| `realfft` | `rustfft = "6.4"` | rustfft handles complex-to-complex; realfft is the specialised real-input wrapper used in production phase-scramble code. realfft pulls rustfft as a transitive dep, so picking realfft is the smaller surface. |
| `EmbarkStudios/cargo-deny-action@v2` | `cargo deny check` via plain `cargo install cargo-deny` step | The action handles caching the cargo-deny binary across runs; faster CI than reinstalling each push. Action is the canonical choice per the `cargo-deny-action` README (`uses: EmbarkStudios/cargo-deny-action@v2`). |
| `rustsec/audit-check@v2.0.0` | `actions-rust-lang/audit` (newer fork) | rustsec/audit-check is the canonical action published by the RustSec maintainers; `actions-rust-lang/audit` is a community fork. Pick the upstream-published version. |
| `insta` envelope snapshot | Hand-rolled byte-equal against `tests/goldens/envelope_snapshot.jsonl` | The codebase uses BOTH patterns: `insta::assert_json_snapshot!` for normalised-data snapshots (`scan_summary_welford.rs:142`), hand-rolled byte-equal for `cli_streams::emit_fixture_byte_identical_when_volatile_fields_masked`. For Phase 7's envelope snapshot, recommend hand-rolled byte-equal (mirrors existing `Pattern J Step 1` provenance gate + cli_streams masking) so the test runs without an `insta review` ceremony when the snapshot needs to change. |

**Installation:**
```bash
# Workspace [workspace.dependencies] additions
cargo add --workspace criterion@0.8.2 --features html_reports
cargo add --workspace dhat@0.3.3
cargo add --workspace realfft@3.5.0

# Per-crate enrollment
# crates/miner-core/Cargo.toml [dev-dependencies] — add criterion
# crates/miner-core/Cargo.toml [dependencies]     — add realfft (production code path: IAAFT)
# crates/miner-bench/Cargo.toml [dependencies]    — add dhat (behind `dhat` feature)
# crates/miner-bench/Cargo.toml [features]        — define `dhat = []`
```

**Version verification:** All three NEW deps verified against crates.io via `cargo search` 2026-05-22. The dhat-rs API at `docs.rs/dhat/latest/dhat/` documents 0.3.3 as the current major.

## Package Legitimacy Audit

Run on 2026-05-22 via `slopcheck install dhat criterion realfft rustfft --json`:

| Package | Registry | Age | Downloads | Source Repo | slopcheck | Disposition |
|---------|----------|-----|-----------|-------------|-----------|-------------|
| `criterion` | crates.io | 10+ yrs | ~15M total | github.com/bheisler/criterion.rs | [OK] | Approved |
| `dhat` | crates.io | 5+ yrs | ~6M total | github.com/nnethercote/dhat-rs | [OK] | Approved |
| `realfft` | crates.io | 6+ yrs | ~3M total | github.com/HEnquist/realfft | [OK] | Approved |
| `rustfft` (transitive of `realfft`) | crates.io | 8+ yrs | ~50M total | github.com/ejmahler/RustFFT | [OK] | Approved (transitive — verified) |

**Packages removed due to slopcheck [SLOP] verdict:** none
**Packages flagged as suspicious [SUS]:** none

All four cleared `slopcheck install … --json` (output: `4 OK`). Slopcheck v0.6.1 confirmed installed via `pip show slopcheck`.

## Architecture Patterns

### System Architecture Diagram

```
                  ┌─────────────────────────────────────────────────────────┐
                  │            Phase 7 Verification Surface                  │
                  └─────────────────────────────────────────────────────────┘
                                          │
        ┌───────────────────┬─────────────┴─────────────┬────────────────────┐
        ▼                   ▼                            ▼                    ▼
  Goldens (existing)   Envelope snapshot         Noise-replay regression   Clone-and-run
                       (NEW)                     (NEW, exercises IAAFT)    quickstart
        │                   │                            │                    │
        │ Pattern J         │ insta::assert_json_        │ realfft 3.5 →     │ tests/fixtures/
        │ Step 1            │ snapshot! OR              │ iaaft_phase_      │ cache/ from
        │ provenance        │ mask_volatile_fields()    │ scramble_null_p   │ scripts/generate-
        │ gate              │                            │ → SweepManifest   │ fixture-cache.sh
        ▼                   ▼                            ▼                    ▼
   stats.summary.    tests/findings_envelope_     SweepSummary BH-FDR    cargo run -p
   welford.jsonl     snapshot.rs                  q-values for 300 jobs  miner-cli scan
   cross.coin.       (provenance + masking +      assert <=30 false      seas.bucket.
   engle_granger     deterministic re-run)        positives at α=0.05    hour_of_day@1
   seas.bucket.
   hour_of_day.jsonl

  ┌──────────────────────────────────────────────────────────────────────┐
  │           Bench Harness (3 layers per D7-03)                         │
  └──────────────────────────────────────────────────────────────────────┘
                                          │
       ┌──────────────────────┬───────────┴───────────────────┬───────────────────────┐
       ▼                      ▼                                ▼                       ▼
  Layer 1: criterion     Layer 2: miner-bench           Layer 3: hyperfine      Profiling
  microbenches           recipe binary                  wall-clock              (dhat + samply)
       │                      │                                │                       │
       │ crates/miner-core/   │ crates/miner-bench/            │ scripts/run-bench.sh  │ scripts/run-alloc-
       │ benches/             │ src/main.rs (REPLACES          │ wraps                 │ profile.sh
       │ (NEW dir)            │ 14-line placeholder)           │ hyperfine 1.20.0      │ → dhat-heap.json
       │                      │                                │                       │
       │ bench_zstd_          │ reads benches/recipes/         │ --warmup 3            │ samply record ...
       │ decompress_1day      │ full-sweep.toml                │ --runs 5              │ → docs/bench-
       │ bench_csv_parse_     │                                │ --export-json         │ results/flame-
       │ 1day                 │ spawns miner sweep             │ /tmp/bench.json       │ graph-<sha>.png
       │ bench_aggregate_     │ (or in-process API)            │                       │
       │ 1m_to_15m            │                                │ → docs/bench-         │
       │ bench_rolling_corr   │ JSON-out timing data           │ results.md            │
       │ bench_ljung_box      │ to stdout                      │ table                 │
       │ bench_ols_fit_4d     │                                │                       │
       └──────────────────────┴────────────────────────────────┴───────────────────────┘

  ┌──────────────────────────────────────────────────────────────────────┐
  │           CI Security Gates (D7-05 — slot into existing CI)          │
  └──────────────────────────────────────────────────────────────────────┘

  .github/workflows/ci.yml (existing six gates: build / clippy / fmt / test / tokio-free / schema sync)
                          │
                          ▼ ADD STEPS AFTER schema sync
                  ┌──────────────────┐
                  │ cargo audit step │  uses: rustsec/audit-check@v2.0.0
                  │ (RustSec advis-  │  with: token: ${{ secrets.GITHUB_TOKEN }}
                  │ ory database)    │
                  └────────┬─────────┘
                           │
                           ▼
                  ┌──────────────────┐
                  │ cargo deny step  │  uses: EmbarkStudios/cargo-deny-action@v2
                  │ (licenses+bans+  │  reads: deny.toml at repo root
                  │ advisories+      │
                  │ sources)         │
                  └──────────────────┘
```

### Recommended Project Structure

```
tradedesk-miner/
├── deny.toml                                          # NEW (D7-05)
├── docs/
│   ├── data_sources.md                                # NEW (D7-02)
│   ├── bench-results.md                               # NEW (D7-07)
│   ├── bench-results/                                 # NEW dir (flamegraph PNGs)
│   │   └── flamegraph-<sha>.png
│   └── examples/                                      # existing
├── tests/fixtures/cache/                              # NEW (D7-01)
│   ├── SHA256SUMS
│   ├── EURUSD/2024/00/01_bid.csv.zst..31_bid.csv.zst
│   └── GBPUSD/2024/00/01_bid.csv.zst..31_bid.csv.zst
├── scripts/
│   ├── generate-fixture-cache.sh                      # NEW (D7-01)
│   ├── run-bench.sh                                   # NEW (D7-03)
│   └── run-alloc-profile.sh                           # NEW (D7-03)
├── crates/miner-core/
│   ├── benches/                                       # NEW dir (D7-03 Layer 1)
│   │   ├── bench_zstd_decompress_1day.rs
│   │   ├── bench_csv_parse_1day.rs
│   │   ├── bench_aggregate_1m_to_15m.rs
│   │   ├── bench_rolling_corr.rs
│   │   ├── bench_ljung_box.rs
│   │   └── bench_ols_fit_4d.rs
│   ├── src/scan/hygiene/null.rs                       # EXTEND — add iaaft_phase_scramble_null_p
│   └── tests/
│       ├── findings_envelope_snapshot.rs              # NEW (D7-06)
│       ├── noise_replay_regression.rs                 # NEW (D7-04)
│       └── goldens/envelope_snapshot.jsonl            # NEW
└── crates/miner-bench/                                # REPLACE placeholder
    ├── Cargo.toml                                     # add criterion-NO, add dhat behind `dhat` feature
    └── src/main.rs                                    # REPLACE 14-line placeholder with recipe runner
```

### Pattern 1: Pattern J Step 1 — Provenance gate + golden include_str!

**What:** Every golden test reads the JSONL via `include_str!`, checks `provenance.<library>_version` matches the pinned REFERENCE-VERSIONS.md value, and only then compares structured fields against `expected.*`.
**When to use:** Phase 7 envelope-snapshot test follows the same pattern.
**Example (canonical, from existing code):**
```rust
// Source: crates/miner-core/tests/scan_summary_welford.rs:167-181 (Plan 04-11)
const GOLDEN_JSON: &str = include_str!("goldens/stats.summary.welford.jsonl");
const TOL: f64 = 1e-10;

let golden: serde_json::Value = serde_json::from_str(GOLDEN_JSON.trim())
    .expect("stats.summary.welford.jsonl must be valid JSON");

let prov = golden["provenance"]["scipy_version"].as_str();
assert_eq!(
    prov,
    Some("1.14.1"),
    "stats.summary.welford.jsonl provenance.scipy_version must be \"1.14.1\"; got {prov:?}. \
     Regenerate via `python crates/miner-core/tests/goldens/generate_summary_welford.py > crates/miner-core/tests/goldens/stats.summary.welford.jsonl` \
     in a pinned venv per crates/miner-core/tests/goldens/REFERENCE-VERSIONS.md.",
);
```

**Important: all three existing goldens are STUB at the moment** — they contain `"scipy_version": "STUB"` / `"statsmodels_version": "STUB"` and the matching integration tests are `#[ignore]`d (Plan 04-11 Pattern J Step 1 decision). Phase 7 plan-phase must either (a) regenerate them via the Python regen scripts in a pinned venv per `REFERENCE-VERSIONS.md`, OR (b) accept that the goldens stay stub-pinned and the envelope-snapshot test is the only non-stub golden landing in Phase 7. Recommend (a) — Phase 7's whole purpose is verification-debt closure.

### Pattern 2: Byte-identical re-run via volatile-field masking

**What:** Re-run the same scan twice with same inputs, mask the four known-volatile envelope fields, assert byte equality.
**When to use:** Goldens + envelope-snapshot + noise-replay all use this.
**Example (canonical, from existing code):**
```rust
// Source: crates/miner-cli/tests/cli_streams.rs:323-344
fn mask_volatile_fields(v: &mut serde_json::Value) {
    if let serde_json::Value::Object(map) = v {
        for key in ["run_id", "started_at_utc", "ended_at_utc"] {
            if map.contains_key(key) {
                map.insert(
                    key.to_string(),
                    serde_json::Value::String(format!("<masked_{key}>")),
                );
            }
        }
        if map.contains_key("wall_clock_ms") {
            map.insert("wall_clock_ms".to_string(), serde_json::Value::from(0i64));
        }
        for (_, child) in map.iter_mut() {
            mask_volatile_fields(child);
        }
    } else if let serde_json::Value::Array(arr) = v {
        for child in arr.iter_mut() {
            mask_volatile_fields(child);
        }
    }
}
```

**Volatile fields list (canonical):** `run_id`, `started_at_utc`, `ended_at_utc`, `wall_clock_ms`. The `byte_identical_rerun.rs:541-547` also masks `produced_at_utc`. Plan-phase pins the exact list against the latest `Finding` envelope shape — both the cli_streams and byte_identical_rerun lists are valid against the current schema; pick one and reuse.

### Pattern 3: Deterministic LCG closes

**What:** Reproducible-across-platforms pseudo-random close array from a `u32` linear congruential generator, used everywhere in tests as the "deterministic OHLCV synthesizer."
**When to use:** Fixture-cache generator + noise-replay regression both need this.
**Example (canonical, from existing code):**
```rust
// Source: crates/miner-core/tests/byte_identical_rerun.rs:74-83
#[allow(clippy::cast_possible_truncation)]
fn lcg_closes(n: usize, seed: u64) -> Vec<f64> {
    let mut s = seed as u32;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let frac = f64::from(s) / f64::from(u32::MAX);
        out.push(1.0 + frac);
    }
    out
}
```

**Constants:** 1_664_525 and 1_013_904_223 are the Numerical Recipes LCG constants — well-known cross-platform-stable. **All Phase 7 deterministic series MUST use this exact function** to keep byte-identity across runs. The same constants appear in `crates/miner-core/tests/scan_arch_lm.rs:29`, `scan_drawdown.rs:28`, `scan_ols_rolling.rs:24`, and `scan/hygiene/null.rs:163` (`lcg_iid`, a sibling for signed `[-1, 1]` series).

### Pattern 4: IAAFT phase-scramble (sibling of `circular_shift_null_p`)

**What:** Iterative Amplitude-Adjusted Fourier Transform — generates surrogate series that preserve BOTH the marginal distribution AND the power spectrum of the input series. Required for heavy-tailed financial return series where simple phase randomisation distorts the marginal.
**When to use:** D7-04 noise-replay regression uses `null = "iaaft"` from a SweepManifest; the engine dispatches to this kernel.
**Reference (Plan 05-02-SUMMARY line 167):** "Add `realfft = "3"` to `[workspace.dependencies]` + `crates/miner-core/Cargo.toml`. Implement `iaaft_phase_scramble_null_p` per 05-PATTERNS §"null.rs" (lines 287-315) + RESEARCH Pitfall 3. Pin the IAAFT max-iter default at 10 with rank-distance convergence criterion."
**Algorithm sketch (Theiler et al. 1992; for plan-phase reference):**
1. Compute FFT of input series via `realfft::RealFftPlanner`.
2. Extract amplitudes (|X(f)|) and the sorted rank order of the input series.
3. Loop max_iter=10 times:
   - Random-phase the spectrum, inverse FFT (yields series with target power spectrum but wrong marginal).
   - Rank-shuffle to match original marginal (preserves distribution).
   - Re-FFT, re-impose target amplitudes (preserves spectrum).
4. Convergence test: rank distance from previous iteration `< 1e-6` OR max_iter reached.

**Critical pitfalls (Plan 05-02-RESEARCH Pitfall 3):**
- FFT length must be padded to next 5-smooth or power-of-2 size (`realfft` accepts arbitrary lengths but performance varies; pin to next 5-smooth via `realfft::num_integer::next_multiple_of(n, 30).max(8).next_power_of_two()` or accept the arbitrary-length cost).
- Rank-shuffle MUST be stable (use `[(usize, f64)]::sort_by` with index tiebreaker, NEVER `sort_unstable`).
- Convergence criterion: rank-distance == 0 is the strict bit-identical check; use `<= 1` as a tolerance for IEEE-754 inputs.
- Test discipline mirrors `circular_shift_null_p`: edge-case NaN returns for `n < 4`, `n_resamples == 0`; cancel-flag polling every `BOOTSTRAP_CANCEL_POLL_CADENCE` resamples; floor empirical p-value at `(1+B)/(1+N)` per Davison & Hinkley 1997 §4.2 (matches the existing `circular_shift_null_p` floor at line 137).

**Implementation site:** `crates/miner-core/src/scan/hygiene/null.rs` — extend the existing module with a sibling function next to `circular_shift_null_p`. The module-level header doc at line 9 already names IAAFT as "DEFERRED to Phase 7"; Phase 7 deletes that deferral note and inverts every `supports_null_method(NullMethod::PhaseScramble) -> false` to `true` for the scans listed below.

**Scans that should enable PhaseScramble after IAAFT lands** (per Plan 05-02 per-scan matrix):
- `stats.autocorr.ljung_box@1` — already declares `PhaseScramble | CircularShift` support (`crates/miner-core/src/scan/ljung_box/mod.rs:118-122`); currently `false` because the kernel doesn't exist.
- `stats.autocorr.ljung_box_sq@1` — same (`scan/anom/ljung_box_sq/mod.rs:111-115`).
- `stats.variance_ratio.lo_mackinlay@1` — same (`scan/anom/variance_ratio/mod.rs:109-113`).
- `cross.lead_lag.ccf@1`, `cross.cointegration.engle_granger@1` — plan-phase verifies what the trait currently declares.

### Pattern 5: SweepManifest invocation from a test

**What:** Phase 5's `crates/miner-core/src/sweep/manifest.rs` provides typed TOML deserialisation; noise-replay regression test builds the manifest in-memory and invokes the executor.
**When to use:** D7-04 noise-replay test constructs a 300-job sweep.
**Reference paths:**
- `crates/miner-core/src/sweep/manifest.rs` — `SweepManifest`, `JobBlock`, `HygieneBlock`, `SweepConfig`, `FdrConfig` types.
- `crates/miner-core/src/sweep/job_graph.rs` — cartesian expansion (`ResolvedJob`).
- `crates/miner-core/src/sweep/executor.rs` — rayon-parallel execution + deterministic-order drain.
- `crates/miner-core/tests/sweep_smoke.rs` — example end-to-end sweep test (2 scans × 2 instruments × 1 timeframe × 1 window × 1 param-grid). Phase 7 noise-replay test mirrors this shape with 100 NULL_NN instruments per job × 3 scans = 300 jobs.

### Pattern 6: dhat-rs feature-gated global allocator

**Source:** `docs.rs/dhat/latest/dhat/` 0.3.3.

```rust
// crates/miner-bench/src/main.rs (REPLACES placeholder)
#[cfg(feature = "dhat")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

fn main() {
    #[cfg(feature = "dhat")]
    let _profiler = dhat::Profiler::new_heap();
    // ... recipe runner body ...
}
```

```toml
# crates/miner-bench/Cargo.toml
[features]
dhat = ["dep:dhat"]

[dependencies]
dhat = { workspace = true, optional = true }
```

**Cargo.toml `[profile.release]` addition** (workspace root):
```toml
[profile.release]
debug = 1  # required for dhat to symbolicate (dhat docs.rs §"Recommended Cargo Configuration")
```

**dhat output:** Writes `dhat-heap.json` to CWD at process exit. View via `dh_view` (download as a standalone HTML page from `valgrind.org/dhat`) or inspect the JSON directly. Plan-phase decides whether to gitignore `dhat-heap.json` or to commit a single reference snapshot to `docs/bench-results/`.

### Pattern 7: hyperfine wrapper script

**Source:** hyperfine v1.20.0 (released 2025-11-18). Verified via WebFetch of `github.com/sharkdp/hyperfine`.

```sh
# scripts/run-bench.sh (NEW per D7-03)
#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Radius Red Ltd.
set -euo pipefail

hyperfine \
  --warmup 3 \
  --runs 5 \
  --export-json /tmp/miner-bench.json \
  --shell=none \
  "cargo run --release -p miner-bench -- --recipe benches/recipes/full-sweep.toml"

# Post-process /tmp/miner-bench.json into a markdown table and append to docs/bench-results.md.
# Plan-phase decides whether to ship a Rust xtask post-processor or a small jq/awk script.
```

**Flag notes:**
- `--shell=none` (or `-N`) bypasses shell interpretation; required when the command contains spaces but no shell metacharacters. For `cargo run` commands wrapped in a single string, drop `--shell=none` — hyperfine then uses `/bin/sh -c` which handles the command line.
- `--warmup 3` runs the command 3 times to warm caches before timed runs. Critical for `cargo run --release` (first run compiles).
- `--runs 5` produces stable wall-clock estimates with reasonable CI ratios.
- `--export-json /path` writes the structured timing data hyperfine collects.
- `--prepare 'cmd'` runs before each timed run; use for cache clearing if needed (NOT recommended for miner-bench — page cache warm is the intended state).
- Quoting: wrap the full command (including args) in a single `"..."` string. No additional escape rules needed for `cargo run -p ... -- --recipe ...`.

### Pattern 8: samply CLI

**Source:** `github.com/mstange/samply` 0.13.1 (2025-02-01).

```sh
# Recommended pattern from samply README:
# 1. Build with a debug-info-enabled profile
cargo build --release --bin miner-bench
# 2. Profile via samply
samply record ./target/release/miner-bench --recipe benches/recipes/single-job.toml
# 3. samply opens the Firefox profiler in a browser tab automatically; URL pattern is
#    https://profiler.firefox.com/from-url/<encoded-localhost-url>
```

Alternatively, `samply record cargo run --release -p miner-bench -- --recipe benches/recipes/single-job.toml` works but profiles the `cargo run` build-and-execute path including the cargo cache lookup. Plan-phase prefers the two-step pattern (build then profile binary directly) for cleaner flame graphs.

**Default output:** profile written to a tempfile and a localhost server serves it to Firefox profiler. Use `samply record -o profile.json …` to save explicitly; Firefox profiler URL format is `https://share.firefox.dev/…` for uploaded profiles (the upload step is manual via the profiler UI).

### Anti-Patterns to Avoid

- **Adding `tokio` to `miner-bench` "because async helpers".** The bench runner spawns the existing sync sweep executor; no async needed. Stay sync. If a benchmark needs async (it shouldn't), put it behind a Cargo feature that nothing else depends on.
- **Adding `dhat` as a default-on dep on `miner-core`.** dhat's `dhat::Alloc` is a `#[global_allocator]` — making it default-on would force every workspace consumer to use it. Gate it behind a `miner-bench`-only Cargo feature.
- **Mixing `criterion` and `divan` in one workspace.** STACK.md §"What NOT to Use" line: "Mixed criterion + divan in one repo — Pick one." Workspace picks criterion.
- **Running `cargo audit`/`cargo deny` via the workspace `cargo install` step on every push.** Use the canonical actions (`rustsec/audit-check@v2.0.0` + `EmbarkStudios/cargo-deny-action@v2`) — they cache the auditor binaries across runs and are significantly faster.
- **Committing real Dukascopy bytes to `tests/fixtures/cache/`.** Dukascopy bytes are licensed per-end-user; redistribution is the licensing risk D7-02 §"Licensing posture" calls out explicitly. The synthetic-stub generator is the only safe path.
- **`zstdmt` feature on the zstd crate for the fixture-cache generator.** STACK.md §"What NOT to Use" forbids `zstdmt` for the production path; for fixture generation it's equally undesirable because multi-threaded compression makes the output byte-non-deterministic (`zstd` levels DO produce deterministic output single-threaded). Generator MUST use single-threaded zstd at level 3 (matches `tradedesk-dukascopy/export.py:442` exactly).
- **Using `sort_unstable` for IAAFT rank-shuffle.** Must be `sort_by` with explicit index tiebreaker to preserve byte-identity across Rust toolchain versions.
- **Bumping `criterion = 0.5` → `0.8` without verifying** that the `#[bench]` harness attribute hasn't changed shape. The current major (0.8) accepts the same `harness = false` + `[[bench]]` declaration; this is not a breaking change for our use case but plan-phase should run `cargo bench -p miner-core` after the dep bump to confirm.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Statistical microbench timing | Custom `Instant::now()` loops with hand-rolled outlier rejection | `criterion = 0.8.2` | Cumulative-distribution-function-based outlier handling; produces statistically rigorous comparisons via `--save-baseline` |
| Wall-clock benchmark of a CLI invocation | `time(1)` + shell loops | `hyperfine 1.20.0` | Shell-spawn-time correction, warm-up handling, JSON export, statistical CIs |
| Heap profiling | Custom `#[global_allocator]` with `AtomicU64` counters | `dhat = 0.3.3` | Per-call-site accumulation; structured JSON output viewable in `dh_view`; the canonical Rust heap profiler |
| Real-input FFT | Hand-rolled DFT / wrap rustfft directly for real inputs | `realfft = 3.5.0` | Specialised real-input wrapper around rustfft; half the storage, ~2× the throughput vs zero-padding rustfft inputs |
| RustSec advisory check | `curl` against rustsec-advisory-db + git diff | `cargo audit` via `rustsec/audit-check@v2.0.0` | Maintained by RustSec; integrates with GitHub status checks |
| License + multi-version + source-trust checks | Multiple `grep` passes over `Cargo.lock` | `cargo deny` via `EmbarkStudios/cargo-deny-action@v2` | One config file (deny.toml), four checks, GH Actions caching |
| Synthetic OHLCV generator | New geometric Brownian motion module from scratch | The existing `lcg_closes` pattern (`tests/byte_identical_rerun.rs:74-83`) | Already canonical across 5 integration test files; reuse keeps "deterministic across-runs" property |
| Surrogate-data null p-value | New circular-shift kernel | Existing `circular_shift_null_p` at `scan/hygiene/null.rs:89` | The `(1+B)/(1+N)` floor + cancel-poll cadence + Tail enum are already correct; IAAFT is the sibling NEW kernel, NOT a replacement |
| BH-FDR adjustment | New step-up implementation | Existing `bh_fdr` at `scan/hygiene/fdr.rs` (per Plan 05-02 module shape) | Already pinned against `R::p.adjust(method = "BH")` within 1e-12 |
| TOML manifest deserialiser | New serde Deserialize types | `SweepManifest` at `crates/miner-core/src/sweep/manifest.rs:54` | Already typed end-to-end with `#[serde(default = "...")]` defaults + `BTreeMap<String, serde_json::Value>` for params |

**Key insight:** Phase 7 is verification-debt closure, not new capability. Every kernel, every test pattern, every fixture-build pattern already exists in `crates/miner-core/`. The only genuinely new code is (a) IAAFT in `null.rs`, (b) the synthetic-fixture generator script, (c) criterion bench files, (d) the noise-replay test driver. Everything else is glue.

## Runtime State Inventory

**Trigger:** Phase 7 is verification-debt closure / new tests + benches + CI gates. No rename or refactor.

| Category | Items Found | Action Required |
|----------|-------------|------------------|
| Stored data | None — Phase 7 does not modify any data stored under `MINER_BAR_CACHE_ROOT` or `tests/fixtures/`. Existing goldens in `crates/miner-core/tests/goldens/` are STUB-pinned but get regenerated by the existing Python regen scripts in `tests/goldens/generate_*.py`. | Plan-phase regen step is documented; runtime state untouched. |
| Live service config | None — no external services involved. | None. |
| OS-registered state | None — no scheduled jobs, no systemd units, no Windows Task Scheduler entries. | None. |
| Secrets/env vars | `GITHUB_TOKEN` is referenced by `rustsec/audit-check@v2.0.0` (CI-only). No new env vars on the developer's machine. | None. |
| Build artifacts | `target/` may need a `cargo clean` after `[profile.release] debug = 1` is added (dhat profiling needs debug info), because cached release artifacts won't have the debug-info build flag set. | One-time `cargo clean` after the workspace `Cargo.toml` change. |

## Common Pitfalls

### Pitfall 1: dhat's debug = 1 silently bloats `target/release/`

**What goes wrong:** Adding `[profile.release] debug = 1` to the workspace `Cargo.toml` increases release-binary size by ~3-5× because every symbol gets a debug entry.
**Why it happens:** dhat needs symbols to attribute allocations to call sites; without them, `dhat-heap.json` shows raw addresses, not function names.
**How to avoid:** Use `debug = 1` (line tables only) NOT `debug = true` (full debug info). The line-tables-only build is sufficient for dhat and roughly doubles binary size instead of 5×ing it. Document this in CONTRIBUTING.md as the "why is target/release so big" answer.
**Warning signs:** `du -sh target/release/miner-bench` showing >100 MB; CI cache hits dropping because the larger binaries push past cache size limits.

### Pitfall 2: criterion `harness = false` is required in `Cargo.toml`

**What goes wrong:** Adding a `[[bench]]` entry without `harness = false` causes cargo to try running the bench file as a test, which fails because criterion benches use their own main.
**Why it happens:** Cargo's default is `harness = true` (use the built-in test harness); criterion replaces that with its own.
**How to avoid:** Every `[[bench]]` entry in `crates/miner-core/Cargo.toml` MUST set `harness = false`:
```toml
[[bench]]
name = "bench_zstd_decompress_1day"
harness = false
```
**Warning signs:** `cargo bench -p miner-core` failing with "no main function" or similar.

### Pitfall 3: Goldens are STUB until regenerated in pinned venv

**What goes wrong:** Phase 7 plan-phase assumes the three existing family goldens are populated. They are NOT — `tests/goldens/seas.bucket.hour_of_day.jsonl` line 1 still says `"_stub_note": "STUB GOLDEN — placeholder until regenerated against pinned Python 3.11 + pandas==2.2.x / scipy==1.14.1 per crates/miner-core/tests/goldens/REFERENCE-VERSIONS.md"`. All three matching integration tests are `#[ignore]`d.
**Why it happens:** Plan 04-11 explicitly deferred the regen step to Phase 7 because the local environment has Python 3.14 not the pinned 3.11.
**How to avoid:** Phase 7 plan-phase MUST schedule a "regen goldens in pinned venv" task BEFORE the envelope-snapshot test lands — otherwise the verification-debt closure is incomplete. The regen recipe:
```sh
python3.11 -m venv /tmp/miner-goldens
/tmp/miner-goldens/bin/pip install --no-deps -r crates/miner-core/tests/goldens/python-requirements.lock
/tmp/miner-goldens/bin/python crates/miner-core/tests/goldens/generate_summary_welford.py    > crates/miner-core/tests/goldens/stats.summary.welford.jsonl
/tmp/miner-goldens/bin/python crates/miner-core/tests/goldens/generate_engle_granger.py     > crates/miner-core/tests/goldens/cross.cointegration.engle_granger.jsonl
/tmp/miner-goldens/bin/python crates/miner-core/tests/goldens/generate_hour_of_day.py       > crates/miner-core/tests/goldens/seas.bucket.hour_of_day.jsonl
```
**Warning signs:** Integration tests still `#[ignore]`d after Phase 7 commits; the three `#[ignore = "Phase 4 Plan 04-11: golden is a STUB ..."]` attribute lines unchanged.

### Pitfall 4: zstd compression non-determinism

**What goes wrong:** Re-running the fixture-cache generator produces zstd files whose bytes differ across runs, breaking the SHA256SUMS gate.
**Why it happens:** The `zstd` crate's default compression API uses internal threading on some versions; multi-threaded zstd is not bit-deterministic. Also: zstd-rs reads optional environment variables like `ZSTD_CHECKSUM` that change output format.
**How to avoid:** Single-threaded zstd encoder at fixed level 3:
```rust
use zstd::stream::write::Encoder;
let mut enc = Encoder::new(File::create(path)?, 3)?;
// Critical: DO NOT call enc.multithread(N) — multi-threaded compression is non-deterministic.
std::io::copy(&mut csv_bytes.as_slice(), &mut enc)?;
enc.finish()?;
```
This matches `tradedesk-dukascopy/export.py:442` (`zstd.ZstdCompressor(level=3)`) exactly — same compression level, same single-threaded mode.
**Warning signs:** `sha256sum tests/fixtures/cache/EURUSD/2024/00/01_bid.csv.zst` returning different hashes across runs.

### Pitfall 5: cargo audit + cargo deny advisory-database staleness

**What goes wrong:** First CI run after a long break (or in a fresh-clone CI runner) emits "advisory database is N days old, this is too stale" errors even though the actual Cargo.lock is clean.
**Why it happens:** `cargo deny` defaults to `maximum-db-staleness = "P90D"`. `cargo audit` has a similar default. Both fail-loud rather than fail-quiet.
**How to avoid:** The `EmbarkStudios/cargo-deny-action@v2` action force-refreshes the database before each run, sidestepping the issue. For `cargo audit`, the `rustsec/audit-check@v2.0.0` action does the same. Plan-phase verifies both actions are in use and does NOT add `--no-fetch` flags.
**Warning signs:** Build green on master, then suddenly red after a quiet week with no Cargo.lock changes.

### Pitfall 6: deny.toml v2 schema keys

**What goes wrong:** Copy-pasting an older `deny.toml` schema produces "unknown key" errors on cargo-deny 0.19.6.
**Why it happens:** The `[advisories]` table has REMOVED several legacy keys: `vulnerability`, `unsound`, `notice`, `severity-threshold` are all "(at time of this writing) no longer used" per embarkstudios.github.io/cargo-deny/checks/advisories/cfg.html. The CONTEXT.md D7-05 proposed keyset includes `vulnerability = "deny"` and `unsound = "deny"` which would NOT work as-is.
**How to avoid:** Use the current valid keyset only:
```toml
# Top of deny.toml — version field is no longer required; cargo-deny detects v2 implicitly.
# [graph]                  # optional
[advisories]
# vulnerability  ← REMOVED — all vulnerabilities now emit errors by default
# unsound        ← REMOVED — all unsound advisories now emit errors by default
# notice         ← REMOVED — all notice advisories now emit errors by default
yanked        = "deny"
unmaintained  = "deny"
# severity-threshold  ← REMOVED — all vulnerabilities emit errors regardless

[licenses]
confidence-threshold = 0.93
allow = ["Apache-2.0", "MIT", "BSD-2-Clause", "BSD-3-Clause", "ISC", "Unicode-DFS-2016", "Unicode-3.0", "Zlib", "MPL-2.0"]

[bans]
multiple-versions = "warn"
wildcards         = "deny"

[sources]
unknown-registry = "deny"
unknown-git      = "deny"
```
**Warning signs:** `cargo deny check` failing with "unknown key `vulnerability`" before it even reads Cargo.lock.

### Pitfall 7: IAAFT FFT-length padding regression

**What goes wrong:** `realfft::RealFftPlanner::plan_fft_forward(n)` accepts any `n` but performance varies wildly — primes like 1009 take ~10× longer than smooth lengths like 1024. For a hot-path resample loop (N=1000 resamples × n=100,000 samples per surrogate), this matters.
**Why it happens:** realfft delegates to rustfft, which uses radix-2 + Bluestein for non-power-of-2 lengths. Bluestein has 3× overhead.
**How to avoid:** Pad to next 5-smooth length (a number whose prime factors are only 2, 3, 5):
```rust
fn next_5_smooth(n: usize) -> usize {
    // Smallest m >= n such that m = 2^a * 3^b * 5^c. Iterate.
    (n..).find(|&m| {
        let mut x = m;
        while x % 2 == 0 { x /= 2; }
        while x % 3 == 0 { x /= 3; }
        while x % 5 == 0 { x /= 5; }
        x == 1
    }).unwrap()
}
```
Or simpler: pad to next power of 2 (`n.next_power_of_two()`). Plan-phase picks the simpler one — for N=100,000 the next power of 2 is 131,072 (~31% overhead), the next 5-smooth is 100,000 already (0% overhead). 5-smooth is the win for the noise-replay sizes.
**Warning signs:** Noise-replay test wall-clock >5× expected on primes-adjacent sample sizes (99,991 etc.).

### Pitfall 8: insta snapshot review ceremony

**What goes wrong:** insta tests fail with "snapshot mismatch" on first run because no `.snap.new` file has been accepted yet.
**Why it happens:** `insta::assert_json_snapshot!` writes a `.snap.new` file on first run; the developer must `cargo insta review` and accept it to commit the `.snap` file. CI cannot do this — it just fails.
**How to avoid:** For Phase 7's envelope-snapshot test, recommend the HAND-ROLLED byte-equal pattern (mirrors `cli_streams::emit_fixture_byte_identical_when_volatile_fields_masked`) rather than insta. The hand-rolled pattern reads a checked-in `tests/goldens/envelope_snapshot.jsonl`, runs the scan, masks volatiles, and asserts equality — no `insta review` step needed. The trade-off: regenerating the golden requires a small `xtask` subcommand or manual capture-and-commit. Plan-phase picks; recommend hand-rolled for the envelope-snapshot test specifically (insta is fine for per-scan-output snapshots which already work in the existing tests).
**Warning signs:** CI red on first push of an insta-based test; CI green only after `cargo insta accept` was run locally.

## Code Examples

Verified patterns from existing repo code:

### Existing golden test wiring (DO NOT REINVENT)

```rust
// Source: crates/miner-core/tests/scan_summary_welford.rs:140-167
// Test function name: summary_welford_matches_scipy_describe_golden
// File path: crates/miner-core/tests/scan_summary_welford.rs
// Sister tests: scan_engle_granger.rs:engle_granger_matches_statsmodels_golden,
//               scan_seas_hour_of_day.rs:hour_of_day_matches_pandas_groupby_golden
```

### Existing byte-identical re-run scaffolding

```rust
// Source: crates/miner-core/tests/byte_identical_rerun.rs
// Single-arity helper: run_single_arity_twice<S: Scan + Send + Sync>(scan: S, bars: &BarFrame, req: &ScanRequest)
// Returns: ((raw1, masked1), (raw2, masked2)) — both runs' raw bytes + masked bytes
// Pin tests: byte_identical_rerun_anom_summary_welford,
//            byte_identical_rerun_cross_engle_granger_via_engine_facade,
//            byte_identical_rerun_seas_hour_of_day,
//            unmasked_envelopes_differ_only_in_volatile_fields
```

### Existing emit-fixture byte-identical test (D7-06 reference)

```rust
// Source: crates/miner-cli/tests/cli_streams.rs:454
#[test]
#[serial_test::serial]
fn emit_fixture_byte_identical_when_volatile_fields_masked() {
    let (out1, _, status1) = run_emit_fixture_happy();
    assert_eq!(status1.code(), Some(0), "run 1 must exit 0");
    let (out2, _, status2) = run_emit_fixture_happy();
    assert_eq!(status2.code(), Some(0), "run 2 must exit 0");

    let masked1 = mask_emit_fixture_stdout(&out1);
    let masked2 = mask_emit_fixture_stdout(&out2);

    assert_eq!(masked1, masked2);  // byte-identical after masking
}
```

### Existing circular_shift_null_p (sibling-of-IAAFT pattern)

```rust
// Source: crates/miner-core/src/scan/hygiene/null.rs:89
pub fn circular_shift_null_p<F>(
    values: &[f64],
    observed_stat: f64,
    stat: F,
    n_resamples: u32,
    seed: u64,
    tail: Tail,
    cancel: &AtomicBool,
) -> f64 where F: Fn(&[f64]) -> f64 {
    // n < 2 or n_resamples == 0 returns NaN (line 102-104)
    // RNG: Xoshiro256PlusPlus::seed_from_u64(seed)  (line 108)
    // n_resamples clamped to HYGIENE_RESAMPLE_CEILING  (line 107)
    // Cancel poll: every BOOTSTRAP_CANCEL_POLL_CADENCE resamples (line 117)
    // P-value floor: (1 + more_extreme) / (1 + n_resamples)  (line 137)
}
```

**IAAFT MUST mirror this signature shape** (positional contract for byte-identical-rerun parity) but accept extra `max_iter: u32` and `convergence_tol: f64` parameters. Plan-phase finalises.

### Current miner-bench Cargo.toml (REPLACES in Phase 7)

```toml
# Source: crates/miner-bench/Cargo.toml — current 21 lines
[package]
name = "miner-bench"
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[[bin]]
name = "miner-bench"
path = "src/main.rs"

[dependencies]
tracing.workspace           = true
tracing-subscriber.workspace = true

[lints]
workspace = true
```

**Phase 7 replacement target** (plan-phase finalises):
```toml
[package]
name = "miner-bench"
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[[bin]]
name = "miner-bench"
path = "src/main.rs"

[features]
default = []
dhat = ["dep:dhat"]

[dependencies]
miner-core.workspace = true
clap.workspace       = true
tracing.workspace    = true
tracing-subscriber.workspace = true
anyhow.workspace     = true
serde_json.workspace = true
toml.workspace       = true
dhat = { workspace = true, optional = true }

[lints]
workspace = true
```

### Current miner-bench main.rs (REPLACES in Phase 7)

```rust
// Source: crates/miner-bench/src/main.rs — current 14 lines (Phase 1 placeholder)
//! Phase 7: implementation forthcoming.
//!
//! Phase 1 placeholder. The real benchmark harness (criterion microbenches + scan-recipe
//! wall-clock runs measured with hyperfine) lands in Phase 7. Per D-23 + D-15: NO
//! `println!` — logs go to stderr through tracing-subscriber so the lint introduced in
//! Plan 04 catches accidental stdout writes from this crate.

fn main() {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();
    tracing::info!("miner-bench placeholder; real harness lands in Phase 7 (criterion)");
}
```

### Apache-2.0 license footer (canonical)

```markdown
---

## License

Licensed under the Apache License, Version 2.0.
See: https://www.apache.org/licenses/LICENSE-2.0

Copyright 2026 [Radius Red Ltd.](https://github.com/radiusred) | [Contact](mailto:opensource@radiusred.uk)
```

**Source:** `docs/.license-footer.md` (8 lines verified). Byte-identical across `docs/findings_envelope.md`, `docs/scan_catalogue.md`, `docs/sweep_manifest.md`, `docs/agent_integration.md`, `docs/future_mcp_http.md`, `ARCHITECTURE.md`, and the sibling repo's `tradedesk/docs/aggregation_guide.md` + `data_sources_guide.md`. `docs/data_sources.md` (D7-02) MUST paste this verbatim.

### Existing CI workflow tokio-free check (verify Phase 7 doesn't break)

```yaml
# Source: .github/workflows/ci.yml:60-72
- name: tokio-free miner-core
  run: |
    set -euo pipefail
    PROHIBITED='^(tokio|tokio-[^ ]+|async-std|async-std-[^ ]+|smol|smol-[^ ]+|async-trait|async-io|async-channel|async-executor|async-task)$'
    LEAKED=$(cargo tree -p miner-core --edges normal,build --prefix none \
        | awk '{print $1}' | sort -u \
        | grep -E "$PROHIBITED" || true)
    if [ -n "$LEAKED" ]; then
      echo "::error::async runtime crate(s) leaked into miner-core:"
      echo "$LEAKED"
      exit 1
    fi
    echo "ok: miner-core has zero async-runtime dependencies"
```

**Verified locally 2026-05-22:** `cargo tree -p miner-core --edges normal,build` against the current branch produces no matching lines. After adding `realfft` to `miner-core/Cargo.toml`, plan-phase MUST re-run this check; realfft + rustfft are sync-only per their READMEs but verify in-situ.

### Canonical cargo audit + cargo deny CI steps

```yaml
# ADD AFTER the existing `schema sync` step in .github/workflows/ci.yml:88

      - name: cargo audit
        uses: rustsec/audit-check@v2.0.0
        with:
          token: ${{ secrets.GITHUB_TOKEN }}

      - name: cargo deny check
        uses: EmbarkStudios/cargo-deny-action@v2
```

The `EmbarkStudios/cargo-deny-action@v2` reads `deny.toml` from the repo root automatically. The `rustsec/audit-check@v2.0.0` runs `cargo audit` against the RustSec advisory DB and creates GH check annotations.

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `cargo-flamegraph` for profiling | `samply` (Firefox profiler-based) | 2023-onwards | CLAUDE.md "Development Tools" already names samply as the recommended profiler; better UX, cross-platform |
| `cargo install cargo-audit && cargo audit` ad-hoc in CI | `rustsec/audit-check@v2.0.0` action | 2024+ | Action handles caching the cargo-audit binary; faster CI |
| `cargo install cargo-deny && cargo deny check` ad-hoc | `EmbarkStudios/cargo-deny-action@v2` | 2024+ | Same — action caches binary |
| deny.toml v1 with `[advisories] vulnerability = "deny"` keys | deny.toml v2 (current) — `vulnerability` / `unsound` / `notice` REMOVED; all advisories error by default | cargo-deny 0.14+ (2023) | CONTEXT.md D7-05 keyset needs updating before plan-phase commits |
| `rand::SmallRng` for resampling | `rand_xoshiro::Xoshiro256PlusPlus` (already adopted) | Plan 05-01 / D5-04 / D5-05 | Cross-version-stable RNG; HYG-05 bit-for-bit reproducibility contract requires this |
| Single-pass phase randomisation | IAAFT iterative correction (Theiler 1992) | Plan 05-02 IAAFT decision | Preserves marginal AND power spectrum; necessary for heavy-tailed financial returns |
| Insta-based snapshot tests | Hybrid: insta for per-scan output normalised JSON; hand-rolled byte-equal for byte-identical-rerun contracts | Plan 04-11 + 04-12 | Insta requires `cargo insta review` ceremony that breaks unattended CI for genuinely-changed snapshots |

**Deprecated/outdated:**
- `[advisories] vulnerability = "deny"` and `[advisories] unsound = "deny"` keys in CONTEXT.md D7-05 — these are removed from current cargo-deny. CONTEXT.md is documenting an older schema. Plan-phase must use the current valid keyset.

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| `cargo` (Rust toolchain) | Workspace build | YES (`/home/darren/.cargo/bin/cargo`) | 1.85.1 (d73d2caf9 2024-12-31) | — |
| `python3` | Goldens regen scripts | YES (3.14 in repo venv; needs 3.11 for pinned pandas/scipy/statsmodels) | 3.14 (system) | Plan-phase installs Python 3.11 via uv/pyenv or runs goldens regen on a different machine and commits |
| `cargo-deny` (CLI binary) | Local `cargo deny check` runs (optional; CI uses action) | NO (not on PATH) | — | `cargo install cargo-deny@0.19.6` or run via `EmbarkStudios/cargo-deny-action@v2` in CI only |
| `cargo-audit` (CLI binary) | Local `cargo audit` runs (optional; CI uses action) | NO | — | `cargo install cargo-audit@0.22.1` or run via `rustsec/audit-check@v2.0.0` in CI only |
| `hyperfine` | `scripts/run-bench.sh` wrapper | NO | — | Document install in CONTRIBUTING.md: `cargo install hyperfine@1.20.0` |
| `samply` | Profiling | NO | — | Document install: `cargo install samply@0.13.1` |
| `dh_view` | Viewing dhat-heap.json (optional — JSON inspect also works) | NO | — | Download standalone HTML from valgrind.org/dhat |
| `slopcheck` | Package legitimacy gate (this research) | YES | 0.6.1 | — |

**Missing dependencies with no fallback:** None — every missing tool has a `cargo install` fallback documented for plan-phase.

**Missing dependencies with fallback:**
- `cargo-deny` / `cargo-audit` / `hyperfine` / `samply` — all `cargo install`able; CI uses pinned actions and does not need local installs.
- Python 3.11 — needed only for goldens regen; one-time cost.

## Validation Architecture

**Not applicable — Phase 7 is verification-debt closure** for FOUND-02, FOUND-03, FOUND-04, CACHE-04, OUT-03, HYG-02, HYG-05. The Nyquist Validation Architecture template applies to NEW features that need a sampling-rate-sensitive test matrix. Phase 7 adds tests that pin existing behaviour (goldens + envelope snapshot + noise-replay + byte-identical-rerun); all are deterministic and the "sample rate" question does not apply.

The orchestrator should skip VALIDATION.md generation for Phase 7. If a VALIDATION.md is generated by mistake, plan-phase deletes it with a one-line PR note pointing at this section.

## Security Domain

### Applicable ASVS Categories

Phase 7 introduces no new user-facing surface (no auth, no sessions, no network endpoints). The applicable security work is supply-chain hygiene only.

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | n/a — miner is a CLI tool with no auth surface |
| V3 Session Management | no | n/a |
| V4 Access Control | no | n/a |
| V5 Input Validation | yes | TOML sweep manifest deserialiser already validates via `serde::Deserialize` + `toml = 0.8` 256-level nesting cap + `[sweep].max_jobs` cardinality gate (`SweepManifest` at `crates/miner-core/src/sweep/manifest.rs`). Phase 7's noise-replay test exercises this path with 300 jobs. |
| V6 Cryptography | no | n/a — `blake3` used for cache-key hashing only, not message authentication |
| V10 Malicious Code | yes | `cargo audit` + `cargo deny check` CI gates (D7-05) cover this — RustSec advisory DB checks every dep on every PR |
| V14 Configuration | yes | `deny.toml` license allowlist + multiple-versions warn + wildcards deny ensures no surprise deps |

### Known Threat Patterns for {Rust workspace + Dukascopy data + supply chain}

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Slopsquatted dependency on `cargo add` | Tampering | Slopcheck gate run by researcher (this doc, `slopcheck install … --json`). Plan-phase adds `cargo deny check sources` to CI which denies unknown registries. |
| Compromised crates.io package update | Tampering | `cargo audit` against RustSec advisory DB on every push. |
| GPL-contaminated transitive dep accidentally licensed-in | Compliance (not STRIDE) | `cargo deny check licenses` against allowlist. |
| Dukascopy-licensed bytes committed to public repo | Compliance + Reputation | D7-01 mandates synthetic-stub fixture cache. No Dukascopy bytes in `tests/fixtures/cache/`. `docs/data_sources.md` §"Licensing posture" documents this explicitly. |
| Goldens drift silently as upstream Python libs change | Tampering | `Pattern J Step 1` provenance gate at the top of every golden test refuses to run unless pinned version matches `REFERENCE-VERSIONS.md`. |
| dhat allocator hides supply-chain issue by changing allocation patterns | Information Disclosure | Feature-gated on `miner-bench` only; default off; CI runs the standard `cargo test --workspace` path without dhat. |
| Synthetic-stub fixture-cache generator non-deterministic across machines | Tampering (drift) | `tests/fixtures/cache/SHA256SUMS` checked-in; regen script's invocation reproducible from seed + pinned zstd level 3 single-threaded. |

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | All four planned new crates (`criterion`, `dhat`, `realfft`, `rustfft`) are non-malicious per slopcheck v0.6.1 verdict `[OK]` | Package Legitimacy Audit | LOW — slopcheck verified against crates.io registry; all packages have authoritative GitHub repos and >3M downloads each |
| A2 | IAAFT max-iter default of 10 with rank-distance convergence is sufficient for the noise-replay test's GBM-on-100K-points workload | Pattern 4 (IAAFT) | MEDIUM — Plan 05-02-SUMMARY line 167 names "max-iter default at 10" as the recommendation; plan-phase confirms against Theiler et al. 1992 §III before pinning |
| A3 | The current goldens are STUB and integration tests are `#[ignore]`d; Phase 7 must regenerate in pinned venv | Pitfall 3 + Standard Stack | HIGH — confirmed by reading `tests/goldens/seas.bucket.hour_of_day.jsonl:1` (`"_stub_note": "STUB GOLDEN — placeholder..."`); regen is a non-optional Phase 7 task |
| A4 | dhat 0.3.3's API (`#[global_allocator] static ALLOC: dhat::Alloc = dhat::Alloc; let _profiler = dhat::Profiler::new_heap();`) is the canonical pattern as of 2026-05 | Pattern 6 | LOW — verified via WebFetch of docs.rs/dhat/latest/dhat/ on 2026-05-22 |
| A5 | `EmbarkStudios/cargo-deny-action@v2` is the canonical action in 2026 (not v1 or v3) | Standard Stack alternatives | LOW — verified via WebFetch of github.com/EmbarkStudios/cargo-deny-action 2026-05-22 |
| A6 | `rustsec/audit-check@v2.0.0` (released 2024-09-23) is the current canonical action | Standard Stack alternatives | LOW — verified via WebFetch of github.com/rustsec/audit-check |
| A7 | `[advisories] vulnerability = "deny"` and `unsound = "deny"` keys in CONTEXT.md D7-05 are INVALID for cargo-deny 0.19.6; they were removed when all advisories started erroring by default | Pitfall 6 | LOW — verified via WebFetch of embarkstudios.github.io/cargo-deny/checks/advisories/cfg.html 2026-05-22; plan-phase must use current keyset (see Pitfall 6 example) |
| A8 | The synthetic-stub fixture-cache generator produces byte-identical zstd output across machines when using `zstd::stream::write::Encoder::new(_, 3)` single-threaded | Pitfall 4 | MEDIUM — zstd is deterministic by spec for fixed level + single-threaded; verified against `tradedesk-dukascopy/export.py:442` which uses the same configuration. Plan-phase regenerates on two different machines as a smoke check |
| A9 | The hand-rolled byte-equal pattern (NOT insta) is the right choice for the envelope-snapshot test | Pitfall 8 + Alternatives Considered | LOW — both patterns work; hand-rolled mirrors the existing `cli_streams::emit_fixture_byte_identical_when_volatile_fields_masked` and avoids insta-review ceremony for CI. Plan-phase may pick insta if it prefers; both are equally valid |
| A10 | `realfft + rustfft` transitive dep graph stays tokio-free | Pitfall 1 + Architectural Responsibility Map | LOW — both crates declare no async runtime deps in their READMEs; plan-phase verifies via `cargo tree -p miner-core --edges normal,build` after adding the dep |

## Open Questions

1. **Goldens regen: where does the Python 3.11 venv live?**
   - What we know: The pinned `python-requirements.lock` exists at `crates/miner-core/tests/goldens/python-requirements.lock`. Local env has Python 3.14, not 3.11.
   - What's unclear: Whether the developer will install Python 3.11 via `uv python install 3.11` / `pyenv install 3.11` / `mise install python@3.11`, or run goldens regen on a different machine and commit.
   - Recommendation: Add a `scripts/regen-goldens.sh` that uses `uv` (no system Python required) — `uv venv --python 3.11 .venv-goldens && uv pip install --no-deps -r crates/miner-core/tests/goldens/python-requirements.lock`. Plan-phase includes this as the canonical recipe in CONTRIBUTING.md `## Regenerating goldens`.

2. **Should the noise-replay test be `#[ignore]`d?**
   - What we know: Test runtime estimate is 30-60s on developer hardware; CI runners are slower.
   - What's unclear: Whether the test pushes CI over a 5-minute job budget when running alongside the existing test suite.
   - Recommendation: Default `#[ignore]`d, run explicitly in CI via `cargo test --workspace -- --ignored noise_replay`. Plan-phase verifies the wall-clock at first integration.

3. **`miner-bench` recipe TOML shape — full SweepManifest or wrapper?**
   - What we know: Phase 5 already ships `SweepManifest` at `crates/miner-core/src/sweep/manifest.rs:54` typed end-to-end.
   - What's unclear: Whether `miner-bench` needs bench-only knobs (warmup count, output JSON path) wrapped around the manifest, or whether it reads a plain SweepManifest and gets bench knobs from CLI args.
   - Recommendation: Plain `SweepManifest` + bench-only CLI args (`--warmup`, `--runs`, `--export-json`). Keeps the recipe TOML files reusable as production sweep manifests; bench-only behaviour stays in the binary.

4. **CHANGELOG.md scaffold?**
   - What we know: CONTEXT.md `<deferred>` lists this as "cheap"; sibling tradedesk has no CHANGELOG.md.
   - What's unclear: Whether v1.0 release ceremony will want a populated CHANGELOG or whether the per-phase SUMMARY.md files in `.planning/` are sufficient.
   - Recommendation: Include — single new file, zero risk, useful at release time. Use the [Keep a Changelog](https://keepachangelog.com) format. Initial content: a single `## [Unreleased]` section listing Phase 1–7 highlights as the v1.0 sign-off changelog entry.

5. **Reference flamegraph: which scan invocation captures it?**
   - What we know: D7-07 requires one reference flamegraph PNG in `docs/bench-results/flamegraph-<sha>.png`.
   - What's unclear: Which `miner-bench` recipe to profile — the 28×3×6 full sweep (representative but heavy), or a single-job recipe (cheaper but less representative).
   - Recommendation: Single-job recipe profiling the hottest scan family (likely `cross.cointegration.engle_granger@1` — full ADF + OLS regression + half-life inner loop). One flamegraph is reference, not exhaustive.

6. **Should `docs/data_sources.md` cite a specific commit SHA of `tradedesk-dukascopy`?**
   - What we know: D7-02 §"Licensing posture" mentions `tradedesk-dukascopy`; the sibling repo is at `/home/darren/projects/radiusred/tradedesk-dukascopy/`.
   - What's unclear: Whether to pin a specific upstream commit SHA so the doc stays accurate even if upstream evolves.
   - Recommendation: Yes — include a "Verified against `tradedesk-dukascopy` commit `<sha>` (<date>)" line in the licensing section. Plan-phase reads the current HEAD of that repo at land time.

## Sources

### Primary (HIGH confidence)

- **Existing repo code** — `crates/miner-core/src/scan/hygiene/null.rs`, `crates/miner-core/tests/*.rs`, `crates/miner-bench/Cargo.toml`, `crates/miner-bench/src/main.rs`, `crates/miner-cli/tests/cli_streams.rs`, `.github/workflows/ci.yml`, `docs/.license-footer.md`, `tests/goldens/REFERENCE-VERSIONS.md`, `Cargo.toml`, `Cargo.lock` — directly read.
- **Plan 05-02-SUMMARY.md** — IAAFT deferral to Phase 7 with `realfft = "3"` and max-iter 10 pinned (lines 157-181, 235).
- **CONTEXT.md (Phase 7)** — `07-CONTEXT.md` decisions D7-01..D7-07 locked.
- **tradedesk sibling repo** — `/home/darren/projects/radiusred/tradedesk/docs/data_sources_guide.md` (sectioned ## headings + Related-docs trailer pattern); `/home/darren/projects/radiusred/tradedesk-dukascopy/tradedesk_dukascopy/export.py` (CSV columns header `["open", "high", "low", "close", "volume"]`; zstd level 3 single-threaded).
- **crates.io / cargo search 2026-05-22** — verified versions: `criterion = "0.8.2"`, `dhat = "0.3.3"`, `realfft = "3.5.0"`, `cargo-deny = "0.19.6"`, `cargo-audit = "0.22.1"`, `divan = "0.1.21"`.
- **slopcheck 0.6.1** — verified all 4 new crates `[OK]` against crates.io.

### Secondary (MEDIUM confidence — verified via WebFetch 2026-05-22)

- **`docs.rs/dhat/latest/dhat/`** — dhat-rs 0.3.3 API (`#[global_allocator] static ALLOC: dhat::Alloc = dhat::Alloc;`, `Profiler::new_heap()`, `[profile.release] debug = 1`).
- **`github.com/EmbarkStudios/cargo-deny-action`** — canonical action version `@v2`, workflow example.
- **`embarkstudios.github.io/cargo-deny/checks/cfg.html`** — current deny.toml top-level sections (`[graph]`, `[advisories]`, `[bans]`, `[licenses]`, `[sources]`, `[output]`); no `version = 2` required.
- **`embarkstudios.github.io/cargo-deny/checks/advisories/cfg.html`** — REMOVED keys (`vulnerability`, `unsound`, `notice`, `severity-threshold`); current valid keys (`db-path`, `db-urls`, `yanked`, `ignore`, `unmaintained`, `unsound`, `git-fetch-with-cli`, `maximum-db-staleness`, `unused-ignored-advisory`).
- **`github.com/rustsec/audit-check`** — canonical action `@v2.0.0` (released 2024-09-23).
- **`github.com/mstange/samply`** — samply 0.13.1 (2025-02-01); recommended `samply record ./target/release/binary` flow; no comprehensive flag list found.
- **`github.com/sharkdp/hyperfine`** — hyperfine v1.20.0 (2025-11-18); canonical flags `--warmup`, `--runs`, `--export-json`, `--prepare`, `--shell=none|-N`.

### Tertiary (LOW confidence — context only)

- **WebSearch results for deny.toml v2 schema** — pointed at `foundry-rs/foundry/blob/master/deny.toml` as a real-world example; not read in full.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — all four new deps verified via slopcheck + crates.io + WebFetch.
- Architecture: HIGH — every Phase 7 deliverable extends an existing pattern with a documented code reference.
- Pitfalls: HIGH — Pitfall 3 (STUB goldens), Pitfall 6 (deny.toml v2 keys), Pitfall 4 (zstd determinism) are all directly verified against repo state or upstream docs.
- IAAFT: MEDIUM — algorithm is well-documented in Theiler et al. 1992 and Plan 05-02-SUMMARY pins the design (max-iter=10, rank-distance convergence). The exact `realfft` API call site needs a small spike at plan-phase to confirm planner+process+inverse round-trip preserves byte-determinism.

**Research date:** 2026-05-22
**Valid until:** 2026-06-22 (30 days; stable ecosystem for criterion/cargo-deny/cargo-audit). hyperfine and samply may bump patch versions but flag surface is stable.
