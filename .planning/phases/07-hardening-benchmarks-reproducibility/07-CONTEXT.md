# Phase 7: Hardening, Benchmarks & Reproducibility — Context

**Gathered:** 2026-05-21
**Status:** Ready for planning
**Scope posture:** Verification-debt closure. No new capabilities, no new scans, no new envelope fields. Phase 7 lands the CI gates, golden-file regressions, bench harness, and README data-source caveats that prove the existing v1 surface delivers what it claims over time, on a fresh checkout, against pinned reference outputs.

<domain>
## Phase Boundary

Phase 7 closes verification debt for seven already-implemented requirements (FOUND-02, FOUND-03, FOUND-04, CACHE-04, OUT-03, HYG-02, HYG-05) and produces five proof artifacts that pin the v1 release:

1. **Golden-file regression suite** — one representative scan per family (already exists for SEAS via `seas.bucket.hour_of_day.jsonl`, CROSS via `cross.cointegration.engle_granger.jsonl`, ANOM via `stats.summary.welford.jsonl`) plus a locked findings-envelope snapshot test. Re-runs are byte-identical. Wired into `cargo test --workspace` so CI catches drift.
2. **Noise-replay sweep regression** — a sweep against a deterministic synthetic null produces near-zero findings at the configured FDR threshold, proving the BH-FDR machinery (HYG-02) controls multiple testing as advertised.
3. **`miner-bench` harness + hyperfine recipes** — criterion microbenches for the hot kernels, a `miner-bench` binary that runs the 28-instrument × 3-timeframe × 6-year sweep, `scripts/run-bench.sh` wrapping hyperfine, and a `dhat-rs` allocation-budget recipe behind a Cargo feature flag.
4. **Clone-and-run quickstart** — a small fixture cache checked into `tests/fixtures/cache/` plus an updated README example that, on a fresh `git clone`, produces ≥1 finding via `cargo run -p miner-cli -- scan ...` without any external download or hardcoded path.
5. **README + `docs/data_sources.md`** — landing-page README adds a short Dukascopy caveats paragraph linking to a new deep doc `docs/data_sources.md` covering the 00-indexed months, tick-count `volume` semantics, bid/ask side independence, weekend/holiday gaps, and licensing posture. `cargo audit` + `cargo deny` are wired into CI and pass clean.

What Phase 7 does NOT deliver:

- New scans, new envelope variants, new envelope fields, new requirement IDs.
- Implementation of OP-02 / OP-03 (MCP + HTTP servers — those are v2 / PLAT-v2-07 + PLAT-v2-08).
- Property-test harness for new scans (could be a future hardening pass, out of scope here).
- Performance regression CI gates (allocs/wall-clock numbers go into the README as reference points, not as CI thresholds — too noisy on shared runners; documented as a manual nightly recipe).
- A separate "release" milestone bump — v1.0 sign-off happens via `/gsd-complete-milestone` after Phase 7 verifies.

Inherited carry-over from Phase 6:

- OP-02 + OP-03 reclassified to v2 (PLAT-v2-07 + PLAT-v2-08); no MCP/HTTP work in Phase 7.
- README trimmed to landing-page shape (commit `f33878e`); the new "Show me what it does" example is the natural insertion point for the clone-and-run scan.
- `docs/` folder shipped with the topical reference guides; `docs/data_sources.md` slots in as the natural sibling to `docs/agent_integration.md`.
- `CONTRIBUTING.md` scaffolded (commit `b5ead1c`); the new `cargo audit` + `cargo deny` gates extend the existing "Quality gates" table.

The user delegated all four gray-area decisions to "Rust-ecosystem typical expectations and best practices". The decisions below are Claude-made on that mandate and are now locked for the planner.

</domain>

<decisions>
## Implementation Decisions

### D7-01: Fixture cache + clone-and-run quickstart (Claude's discretion, user delegated)

**Location:** A new directory `tests/fixtures/cache/` at the repo root (workspace-shared, not crate-scoped) — so the same path works whether the user runs `cargo run` from the root or from any crate.

**Layout (matches Dukascopy production):**

```
tests/fixtures/cache/
  EURUSD/
    2024/
      00/                                    ← January, 00-indexed
        01_bid.csv.zst
        ...
        31_bid.csv.zst
      01/                                    ← February
        01_bid.csv.zst
        ...
  GBPUSD/                                    ← second instrument for CROSS smoke
    2024/
      00/
        01_bid.csv.zst
        ...
```

**Size budget:** ≤ 5 MB total compressed. Two instruments × bid side × one month each is sufficient for both the single-instrument example and the CROSS-family smoke. Bar-resolution synthesis runs at scan time; no checked-in derived-bar cache.

**Generator:** `scripts/generate-fixture-cache.sh` — invokes `tradedesk-dukascopy` (already in the user's stack) against a pinned date range to produce the bytes, then validates with `sha256sum` against a checked-in `tests/fixtures/cache/SHA256SUMS`. Plan-phase decides whether the generator runs against a real Dukascopy mirror (requires network) or against a synthetic stub the team controls. Recommendation: **synthetic stub** — eliminates the network dependency, makes CI hermetic, and dodges the licensing question of redistributing Dukascopy bytes. The stub generator emits CSVs with the same column shape, plausible OHLCV ranges, and deterministic-from-seed values so reruns produce byte-identical fixture bytes.

**Quickstart scan (the proof-of-finding for SC #4):**

```sh
MINER_CACHE_ROOT=./tests/fixtures/cache \
MINER_BAR_CACHE_ROOT=/tmp/bar \
MINER_OUTPUT=stdout \
cargo run -p miner-cli -- scan seas.bucket.hour_of_day@1 \
    --instrument EURUSD:bid --timeframe 15m \
    --window 2024-01-01:2024-01-31
```

Chosen because:

- Single-instrument (no CROSS-family pairing concerns for a quickstart).
- One month of 15m bars (~2,976 bars) is more than enough for `seas.bucket.hour_of_day@1` to emit a non-empty result with stable `t-stat` arrays.
- The fixture covers the same scan against `GBPUSD:bid` so the README example also serves as an obvious "now try this" follow-up.

**README integration:** The existing `## Example` block (`f33878e`) gets its hardcoded `MINER_CACHE_ROOT=/path/to/cache` replaced with `./tests/fixtures/cache`. One additional sentence: "If you cloned the repo, this works as-is — no external download needed."

### D7-02: Dukascopy data-source caveats — README summary + docs/data_sources.md deep dive (Claude's discretion)

**README addition** (~6 lines): A short paragraph after `## Example` flagging the four most-surprising Dukascopy semantics, linking to the deep doc:

```markdown
## Data source caveats

`tradedesk-miner` reads the cache layout `tradedesk-dukascopy` produces. A few
non-obvious conventions matter for interpreting findings:

- Months are **00-indexed** on disk (`2024/00/` = January, `2024/11/` = December).
- The `volume` column is a **tick count**, not lot volume.
- Bid and ask sides are processed independently; spread reconstruction is out of scope.
- Weekend and exchange-holiday gaps are intentional, not missing data; the
  `gap_policy` flag controls how scans treat them.

See [docs/data_sources.md](docs/data_sources.md) for the full reference, including
the data licensing posture.
```

**docs/data_sources.md** (target ~180-250 lines incl. footer):

- `## Cache layout` — the on-disk path convention with worked examples.
- `## CSV schema` — column-by-column: `timestamp_utc` (UTC ms since epoch), `open` / `high` / `low` / `close` (mid-quote for the chosen side), `volume` (tick count for the bar). Cite `crates/miner-reader-dukascopy/src/` as ground truth.
- `## Bid vs ask independence` — why miner treats sides as separate logical instruments (`SYMBOL:side` in the CLI). What an agent should do if it wants spread-aware findings (run the same scan on both sides, compare).
- `## Time zones and DST` — UTC throughout; the aggregator's DST-handling tests at `crates/miner-core/tests/dst_*.rs` are the canonical contract.
- `## Gap policies` — restate the `strict` vs `continuous_only` semantics for a data-source-doc audience (the agent_integration.md version is consumer-shaped; this version is data-shape-shaped). Weekend gaps and exchange holidays are not anomalies — they are the data.
- `## Licensing posture` — Dukascopy's bid/ask cache is licensed to the end user; `tradedesk-dukascopy` downloads the bytes on the end user's machine. miner reads bytes that already exist on disk and does not redistribute them. The checked-in `tests/fixtures/cache/` directory contains **synthetic** data produced by `scripts/generate-fixture-cache.sh` — no Dukascopy-licensed bytes are committed to the repo. Anyone building against real data needs their own Dukascopy access; link to `https://www.dukascopy.com/swiss/english/marketwatch/historical/` for the upstream terms.
- `## See Also` + Apache-2.0 footer.

### D7-03: Bench harness composition — criterion + miner-bench + hyperfine + dhat-rs (Claude's discretion)

**Three-layer harness:**

**Layer 1 — `criterion` microbenches** under `crates/miner-core/benches/`:

- `bench_zstd_decompress_1day.rs` — single-file zstd decode throughput.
- `bench_csv_parse_1day.rs` — `csv` crate vs `csv-core` parsing speed (informs CONTRIBUTING.md guidance on when to hand-roll).
- `bench_aggregate_1m_to_15m.rs` — `Aggregator::aggregate` for one symbol-year of 1m bars.
- `bench_rolling_corr.rs` — rolling-Pearson and rolling-Spearman kernels.
- `bench_ljung_box.rs` — Ljung-Box Q-stat across lag grids (5, 10, 20, 50).
- `bench_ols_fit_4d.rs` — small fixed-size OLS via `nalgebra` (CROSS hot path).

Each bench: `cargo bench -p miner-core` runs it; `--save-baseline` lets contributors regression-test their own changes locally. CI does NOT run these (criterion's variance on shared runners is too high for a useful gate).

**Layer 2 — `miner-bench` recipe binary** at `crates/miner-bench/src/main.rs`:

- Reads a TOML recipe file (`benches/recipes/full-sweep.toml`) describing the 28-instrument × 3-timeframe × 6-year sweep declared in success criterion #3.
- Spawns `miner sweep` against a configurable cache root (`MINER_CACHE_ROOT` env var; defaults to a documented production path the contributor sets up locally).
- Emits structured timing data (per-instrument, per-timeframe, total wall clock, peak RSS, total findings, total scan-errors) to stdout as a single JSON object — hyperfine-friendly.
- Existing crate stays binary-only; do not add criterion to `miner-bench`'s deps.

**Layer 3 — `scripts/run-bench.sh` hyperfine wrapper:**

```sh
hyperfine --warmup 3 --runs 5 --export-json /tmp/bench.json \
  "cargo run --release -p miner-bench -- --recipe benches/recipes/full-sweep.toml"
```

The script post-processes `/tmp/bench.json` into a markdown table appended to `docs/bench-results.md` (Phase 7 creates this file with reference numbers measured on the developer's workstation; subsequent runs replace the table). The README references this doc but does not embed numbers (they go stale).

**Allocation budget (success criterion #3, "allocations below 5% of hot path"):**

- Add a `dhat` Cargo feature to `miner-bench` (default off). With `--features dhat`, the binary uses `dhat::Alloc` as a global allocator and dumps `dhat-heap.json` at exit.
- `scripts/run-alloc-profile.sh` runs `cargo run --release --features dhat -p miner-bench -- --recipe benches/recipes/single-job.toml`, then runs `dh_view` (or just inspects the JSON) to check the proportion of allocations attributed to the scan hot path.
- The 5% target is documented in CONTRIBUTING.md as a regression-aware goal, NOT a CI gate. If allocs exceed 5% on a Phase 7 commit, the team treats it as a P1 bug, but CI does not auto-fail (dhat profiling is single-threaded and slow).

**Flamegraph (success criterion #3):**

- `samply` documented in CONTRIBUTING.md as the recommended profiler (modern replacement for `cargo-flamegraph`, output viewable in Firefox profiler).
- Recipe: `samply record cargo run --release -p miner-bench -- --recipe benches/recipes/single-job.toml`.
- One reference flamegraph image checked into `docs/bench-results.md` (with a date and the commit SHA at which it was captured).

### D7-04: Noise-replay sweep regression — phase-scrambled IAAFT on synthetic GBM (Claude's discretion)

**Test location:** `crates/miner-core/tests/noise_replay_regression.rs`. Wired into `cargo test --workspace` so CI runs it on every push. The test is slow (~30-60s); plan-phase decides whether to mark it `#[ignore]` and run only in CI, or always run.

**Null dataset construction (no real data, fully reproducible):**

1. Seed a deterministic 64-bit RNG from a literal constant (`0xC0FFEE_C0FFEE`).
2. Generate synthetic 1-minute log-returns as GBM with `μ=0, σ=1e-4` for N=100,000 bars (≈ 70 trading days). One series per virtual instrument.
3. Phase-randomise each series via miner-core's IAAFT machinery (the same machinery `null = "iaaft"` exercises in the sweep) to produce 100 statistically-null surrogate series.
4. Materialise the surrogate series as in-memory 15m bars via the existing aggregator (test fixtures cover this pattern; no on-disk cache needed).

**Sweep manifest (in-memory or fixture TOML):**

```toml
[sweep]
seed = 0xCAFEBABE

[fdr]
family = "scan_id"
alpha  = 0.05

[[jobs]]
scan        = "stats.autocorr.ljung_box@1"
instruments = ["NULL_00:bid", "NULL_01:bid", ..., "NULL_99:bid"]
timeframes  = ["15m"]
windows     = ["2024-01-01:2024-03-31"]
params      = { lags = [10] }

[[jobs]]
scan        = "cross.cointegration.engle_granger@1"
instruments = [["NULL_00:bid", "NULL_01:bid"], ["NULL_02:bid", "NULL_03:bid"], ...]
timeframes  = ["15m"]
windows     = ["2024-01-01:2024-03-31"]
params      = {}

[[jobs]]
scan        = "seas.bucket.hour_of_day@1"
instruments = ["NULL_00:bid", "NULL_01:bid", ..., "NULL_99:bid"]
timeframes  = ["15m"]
windows     = ["2024-01-01:2024-03-31"]
params      = {}
```

100 jobs per scan × 3 scans = 300 total tests at α=0.05.

**Pass criterion (numerical, not "near-zero"):**

- Under H0, each individual test has P(reject) = α = 0.05.
- After BH-FDR adjustment over 300 tests, the expected number of `q ≤ 0.05` is bounded by `α × 300 = 15` — but BH controls FDR, so the realised count should be near zero for a well-behaved null (the null is global — every q-value should be large).
- **Concrete bound:** `assert!(false_positive_count <= 30)` — the Wilson 99% upper bound for a binomial(300, 0.05) is ~28, so 30 gives a slim safety margin for the FDR-adjustment artefacts. If the count exceeds 30, BH-FDR is provably broken.
- The test also asserts `false_positive_count > 0 || N_runs == 1` — a literal zero across multiple seeds would be suspicious (overly conservative FDR is also wrong).

**What the test asserts:**

1. The sweep completes with `jobs_planned == jobs_completed == 300`, `scan_errors == 0`, `gap_aborted == 0`.
2. `SweepSummary` emits one `FdrFamilySummary` per `scan_id` (locked v1 scoping per D5-02).
3. Across all family entries, `count(q_value <= 0.05) <= 30`.
4. Re-running the test with the same seed produces a byte-identical `SweepSummary` (this also exercises HYG-05 reproducibility).

### D7-05: Security + license CI gates — cargo audit on every PR, cargo deny on every PR, allowlist-by-exception (Claude's discretion)

**`cargo audit`:**

- Add a new CI job step `name: cargo audit` after the existing `name: schema sync` step in `.github/workflows/ci.yml`.
- Command: `cargo audit --deny warnings --deny unsound --deny unmaintained --deny yanked`.
- Failure mode: **fail the build** on any advisory hit. No silent acceptance.
- Tolerance: zero days. New advisories block PRs immediately.
- If a CVE genuinely needs a temporary ignore (upstream hasn't released a fix yet), document it in `audit.toml` with an `# explicit-ignore: RUSTSEC-YYYY-NNNN — <one-line reason> — review by YYYY-MM-DD` comment. The review-by date is a hard checklist item picked up by the next nightly run (out of v1 scope to automate that reminder).

**`cargo deny`:**

- Add `deny.toml` at repo root with:
  - `[licenses] allow = ["Apache-2.0", "MIT", "BSD-2-Clause", "BSD-3-Clause", "ISC", "Unicode-DFS-2016", "Unicode-3.0", "Zlib", "MPL-2.0"]` — the standard permissive set plus MPL-2.0 (used by `webpki-roots` and a couple of tokio-adjacent crates we don't pull but might transitively).
  - `[licenses] confidence-threshold = 0.93` (default).
  - `[bans] multiple-versions = "warn"` — duplicate-dep warnings, not failures (a release-blocking ban is too aggressive for a v1; revisit at v1.x).
  - `[bans] wildcards = "deny"` — `version = "*"` requirements are banned.
  - `[advisories] vulnerability = "deny"`, `unmaintained = "deny"`, `yanked = "deny"` — same posture as `cargo audit`.
  - `[sources] unknown-registry = "deny"`, `unknown-git = "deny"` — no surprise registries / forks.
- Add a CI job step `name: cargo deny check` directly after `cargo audit`.
- Failure mode: **fail the build** on any license outside the allowlist or any banned-dep / unknown-source hit.
- Plan-phase verifies the current dependency tree against the allowlist; if anything outside the allowlist surfaces (e.g., a GPL'd dev-dependency), either the dep is replaced or the allowlist is expanded with an inline justification.

**Allowlist-by-exception policy** (documented in CONTRIBUTING.md):

- New deps must satisfy the allowlist out of the box. PRs that change `Cargo.lock` re-run both `cargo audit` and `cargo deny check`.
- If a contributor needs a license outside the allowlist, the PR explains why in the description, and the allowlist addition lands as a separate commit with the justification in the commit message and an inline `# allowed-for: <reason>` comment in `deny.toml`.
- No automated "lockfile age" gate in v1 (`cargo upgrade`-style automation is its own future-work).

**Schedule:** Both gates run on every push and every PR (matches the existing fmt/clippy/test/schema-sync gates). No separate nightly schedule for v1 — the existing gates run on every push, and `cargo audit`'s database fetch is fast enough that adding it doesn't materially slow the pipeline.

### D7-06: Golden-fixture maintenance discipline (Claude's discretion)

The three existing family goldens (`stats.summary.welford.jsonl`, `cross.cointegration.engle_granger.jsonl`, `seas.bucket.hour_of_day.jsonl`) are bit-for-bit pinned against the Python reference versions in `crates/miner-core/tests/goldens/REFERENCE-VERSIONS.md`.

**Phase 7 adds:**

- A locked findings-envelope snapshot test (`tests/findings_envelope_snapshot.rs`) using `insta` or a hand-rolled byte-equal assertion against a checked-in `tests/goldens/envelope_snapshot.jsonl`. Pinned to the current schema version; bumps require a `## Schema Evolution Log` entry in `docs/findings_envelope.md` (out of scope for Phase 7's automation, but the manual contract is documented in CONTRIBUTING.md).
- A `## Regenerating goldens` section in CONTRIBUTING.md walking through `python3 generate_<scan>.py > <scan>.jsonl` for each family, committing the diff, and noting that the resulting commit must be a single `chore: regen goldens after <reason>` PR (no mixing with behavioural changes).
- No CI gate that re-runs the Python regen scripts on every PR — the goldens are pinned outputs, not living references. Plan-phase confirms the existing tests under `crates/miner-core/tests/` that compare miner output to the JSONL goldens are wired into `cargo test --workspace`.

`REFERENCE-VERSIONS.md` stays at its current pinned versions for v1.0; v1.x can bump if a Python release fixes a known reference bug.

### D7-07: Performance numbers in docs/bench-results.md, NOT in README (Claude's discretion)

The README intentionally has no embedded benchmark numbers (commit `f33878e` trimmed it to landing-page shape). Phase 7 creates `docs/bench-results.md` as the single canonical home for:

- Reference wall-clock numbers from the 28×3×6 sweep, captured via `scripts/run-bench.sh` on a documented workstation (CPU, RAM, OS, commit SHA).
- One reference flamegraph image captured via `samply` (PNG checked into `docs/bench-results/flamegraph-<sha>.png`).
- One reference `dhat-heap.json` snapshot summary (allocation-budget proof) — JSON checked in, with a short paragraph summarising the top-5 allocation sites.

The README's "Roadmap" section gets a one-line pointer at this doc; the "Documentation" index does not get a new entry (bench numbers are an internal hardening artifact, not a load-bearing consumer doc).

</decisions>

<deferred>
## Noted for Later

- **Property-test harness for new scans** — `proptest` invariants per scan family (e.g., aggregator monotonicity, OHLC ordering, ratio bounds). Belongs in a later hardening pass when new scans are added. Out of Phase 7 scope.
- **Lockfile-age automation** — Dependabot / Renovate config to auto-bump deps weekly. Out of v1 scope; matches the "no `cargo upgrade` automation" posture in D7-05.
- **Performance regression CI gate** — turn `docs/bench-results.md` numbers into a CI threshold once we have enough baseline data to know what variance bounds make sense on the chosen runner. v1.x.
- **`#[ignore]` cleanup audit** — any test currently `#[ignore]`'d that's now ready to enable. Pre-`/gsd-complete-milestone` housekeeping; could fold into Phase 7 plan if cheap.
- **CHANGELOG.md scaffold** — common convention; not blocking for v1.0 but worth standing up before release. Could land alongside Phase 7's hardening if planner thinks it's cheap.
- **Dependabot / Renovate config** — see lockfile-age automation. Belongs as a v1.x or v2 chore.

</deferred>

<canonical_refs>
## Canonical References

Downstream agents (researcher, planner) MUST consult these files; they ground every decision above:

- `.planning/ROADMAP.md` — Phase 7 row (success criteria #1-5, "no new v1 REQ-IDs").
- `.planning/REQUIREMENTS.md` — the seven requirement IDs Phase 7 closes (FOUND-02, FOUND-03, FOUND-04, CACHE-04, OUT-03, HYG-02, HYG-05) and the traceability table footer that documents the closure.
- `.planning/STATE.md` — current Deferred Items table (OP-02 + OP-03 carry-over, Phase 6) and verification debt counters.
- `.planning/research/STACK.md` — bench-harness section (criterion, divan alternative, iai-callgrind future-work), security gates section (cargo-audit, cargo-deny rationale).
- `.planning/research/ARCHITECTURE.md` — sync-core + async-edges section (FOUND-04 invariant the noise-replay test exercises end-to-end).
- `.planning/phases/01-foundations-contracts/01-CONTEXT.md` — D-15 / D-19 / D-21 / D-22 (the four CI gates already in place; Phase 7 adds two more).
- `.planning/phases/02-reader-aggregator-derived-bar-cache/02-CONTEXT.md` — derived-bar cache invariants (the bench harness's aggregator path uses these).
- `.planning/phases/03-scan-engine-facade-cli/03-CONTEXT.md` — D3-22 SIGINT, D3-24 four-tier exit codes, D3-23 byte-identical re-run (the byte-determinism test the goldens rely on).
- `.planning/phases/05-statistical-hygiene-sweep-runner/05-CONTEXT.md` — D5-02 BH-FDR scoping (the noise-replay test exercises this).
- `.planning/phases/06-mcp-http-wrappers/06-CONTEXT.md` — D6-08 placeholder-binary invariant (Phase 7 must not add tokio/axum/rmcp to the placeholder Cargo.tomls).
- `.planning/phases/06-mcp-http-wrappers/06-REVIEW.md` — the doc-vs-source verification posture (every documented identifier matches a Rust source identifier); Phase 7 inherits this discipline for `docs/data_sources.md`.
- `crates/miner-core/tests/goldens/REFERENCE-VERSIONS.md` — pinned statsmodels / scipy / pandas versions for the existing three family goldens.
- `crates/miner-core/tests/goldens/generate_*.py` — the regen scripts for each family golden.
- `crates/miner-bench/Cargo.toml` — current state (only `tracing` deps; Phase 7 adds criterion to the workspace `[dev-dependencies]` and the dhat feature to this crate).
- `crates/miner-bench/src/main.rs` — current placeholder; Phase 7 replaces with the recipe-runner.
- `.github/workflows/ci.yml` — existing six gates (build, clippy, fmt, test, tokio-free miner-core, schema sync); Phase 7 adds cargo audit + cargo deny.
- `.githooks/pre-commit` — local mirror of fmt + clippy CI gates; Phase 7 does NOT add cargo audit / cargo deny to the pre-commit hook (too slow for the per-commit path).
- `CONTRIBUTING.md` — the existing "Quality gates" table extends with two new rows (cargo audit + cargo deny) and a new "Regenerating goldens" subsection.
- `README.md` — the existing `## Example` block (commit `f33878e`) is the integration point for the clone-and-run quickstart and the new "Data source caveats" section.
- `/home/darren/projects/radiusred/tradedesk-dukascopy/` — the upstream cache producer; cite for the 00-indexed-months convention and the licensing posture in `docs/data_sources.md`. Pin a specific commit SHA when `docs/data_sources.md` lands.

</canonical_refs>

<code_context>
## Reusable Assets and Patterns

**Goldens scaffolding** is already in place — Phase 7 hooks tests into them, not the other way round:

- `crates/miner-core/tests/goldens/stats.summary.welford.jsonl` (ANOM representative)
- `crates/miner-core/tests/goldens/cross.cointegration.engle_granger.jsonl` (CROSS representative)
- `crates/miner-core/tests/goldens/seas.bucket.hour_of_day.jsonl` (SEAS representative)
- `crates/miner-core/tests/goldens/generate_*.py` regen scripts (3 files)
- `crates/miner-core/tests/goldens/REFERENCE-VERSIONS.md` pinned reference library versions
- `crates/miner-core/tests/goldens/python-requirements.lock` lockfile for the regen environment

**Byte-determinism plumbing** already exists:

- `cli_streams::emit_fixture_byte_identical_when_volatile_fields_masked` already proves byte-identical re-runs on the masked `emit-fixture` invocation. Phase 7 generalises this pattern to the three family goldens and the new envelope snapshot.

**Aggregator + DST tests** are exhaustive at `crates/miner-core/tests/dst_*.rs` and `aggregator_edge_cases.rs`. Phase 7's `docs/data_sources.md` cites these as the canonical contract for "how miner handles time zones".

**Existing CI workflow** at `.github/workflows/ci.yml` runs six gates. The new `cargo audit` + `cargo deny` steps slot in after `schema sync` with the same shell-step pattern.

**Pre-commit hook** at `.githooks/pre-commit` covers fmt + clippy. Phase 7 does NOT extend it; the security gates live in CI only.

**Placeholder `miner-bench` crate** at `crates/miner-bench/`:

- `Cargo.toml`: only `tracing` + `tracing-subscriber` (workspace deps).
- `src/main.rs`: 14-line placeholder emitting one tracing-info line.
- Phase 7 replaces both. The recipe binary becomes the main artefact; criterion lives separately under `crates/miner-core/benches/`.

**Tradedesk sibling repo** at `/home/darren/projects/radiusred/tradedesk/`:

- `README.md` lines 380-410 carry the Apache-2.0 footer pattern miner already mirrors.
- `docs/data_sources_guide.md` is the closest sibling to the planned `docs/data_sources.md` — section discipline, table of contents posture, footer pattern. Plan-phase reads this file and mirrors the layout.

</code_context>

<open_questions>
## Open Questions for plan-phase

These are decisions plan-phase is empowered to make but worth flagging explicitly:

1. **Fixture cache generator: real Dukascopy mirror or synthetic stub?** Recommendation is **synthetic stub** (D7-01). Plan-phase confirms by reading `/home/darren/projects/radiusred/tradedesk-dukascopy/` to verify the column shape miner expects, then writes a Rust or shell generator that produces matching bytes from a deterministic seed. If the stub turns out to violate `csv` + `zstd` round-trip assumptions discovered during research, fall back to a fixture-from-real-data approach with a documented one-time licensing waiver.

2. **Noise-replay test: `#[ignore]` or always-run?** Default to always-run (30-60s is tolerable in CI). If post-implementation timing measurements show the test pushes the CI job over 5 minutes total, plan-phase or executor can switch it to `#[ignore]` + run explicitly under a `cargo test -- --ignored noise_replay` step.

3. **`miner-bench` recipe TOML shape.** D7-03 names `benches/recipes/full-sweep.toml` and `benches/recipes/single-job.toml`. Plan-phase finalises the recipe shape (is it just a `SweepManifest`? A wrapper around `SweepManifest` with bench-only knobs like warmup-count?). If it's the latter, define the wrapper type in `miner-bench` and document the grammar in a short header comment.

4. **`docs/bench-results.md` initial state.** Phase 7 creates the file with placeholder numbers measured on the developer's local workstation. Plan-phase decides whether to also include a "How to reproduce" section walking through `scripts/run-bench.sh` invocation, or to defer that to CONTRIBUTING.md.

5. **`cargo deny` license allowlist baseline.** Plan-phase runs `cargo deny check licenses` against the current `Cargo.lock` and surfaces any licenses outside the proposed allowlist before Phase 7 code lands. If the audit surfaces a license that needs adding, the addition lands in the same PR with an inline justification.

6. **CHANGELOG.md scaffold.** Listed as deferred but cheap. Plan-phase calls it: add a v1.0 changelog skeleton to Phase 7 or push to Phase 7.x / v1.0 milestone close. Recommendation: include — single new file, zero risk, useful at release time.

7. **README pointer to docs/bench-results.md.** D7-07 says the README "Roadmap" section gets a one-line pointer. Plan-phase confirms the line placement and exact wording.

</open_questions>

<success_criteria>
Phase 7 is complete when:

1. `cargo test --workspace` runs the three family goldens + the envelope snapshot test + the noise-replay regression test, all pass, and re-runs produce byte-identical output for each.
2. `cargo bench -p miner-core` runs all criterion microbenches without errors. (Numbers documented in `docs/bench-results.md`; not gated.)
3. `scripts/run-bench.sh` against the 28×3×6 recipe produces reproducible hyperfine timings; the recipe's output landed in `docs/bench-results.md` with the capture commit SHA.
4. `scripts/run-alloc-profile.sh` produces a `dhat-heap.json` whose top-5 allocation sites are documented in `docs/bench-results.md`; the proportion attributed to the scan hot path is recorded.
5. A `git clone` of the repo, followed by the README quickstart, produces at least one `Finding::Result` envelope from `cargo run -p miner-cli -- scan seas.bucket.hour_of_day@1 --instrument EURUSD:bid --timeframe 15m --window 2024-01-01:2024-01-31` against the checked-in fixture cache — with no external network calls.
6. `docs/data_sources.md` exists, covers the five Dukascopy caveats (00-indexed months, tick-count volume, bid/ask independence, gaps, licensing), ends with the canonical Apache-2.0 footer byte-identical to `docs/.license-footer.md`, and the README has a 6-line summary pointing at it.
7. `.github/workflows/ci.yml` runs `cargo audit` and `cargo deny check` on every push and PR. Both pass clean on `main` at the moment Phase 7 ships.
8. `deny.toml` lives at the repo root with the allowlist from D7-05; the current dependency tree satisfies it.
9. `CONTRIBUTING.md` extends its "Quality gates" table with two new rows (cargo audit + cargo deny) and adds a "Regenerating goldens" subsection.
10. `cargo tree -p miner-core --edges normal,build` continues to show zero `tokio` / `async-std` / `smol` / `axum` / `rmcp` deps (FOUND-04 preserved through Phase 7's bench harness work).
11. The three placeholder mains (`miner-mcp`, `miner-http`, AND `miner-bench` — wait, `miner-bench` gets a real implementation in this phase; only `miner-mcp` and `miner-http` stay as placeholders) keep their D6-08 invariant: zero Cargo.toml deltas for `miner-mcp` and `miner-http`.

</success_criteria>

---

*Generated by `/gsd-discuss-phase 7` on 2026-05-21. All four gray-area decisions were delegated to "Rust-ecosystem typical expectations and best practices" — the resulting choices above are Claude-made on that mandate and are now locked for the planner. If any decision looks wrong, edit this file before running `/gsd-plan-phase 7`.*
