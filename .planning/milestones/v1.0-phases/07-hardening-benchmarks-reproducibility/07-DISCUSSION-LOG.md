# Phase 7: Hardening, Benchmarks & Reproducibility — Discussion Log

**Gathered:** 2026-05-21
**For human reference / audit only — not consumed by downstream agents.**

## Discussion Flow

### Opening — gray-area selection (multi-select AskUserQuestion)

**Claude proposed four gray areas:**

1. Dev sample + quickstart proof-of-finding — fixture cache layout, size, scan/window for SC #4; Dukascopy caveats home (README vs `docs/`).
2. Bench harness composition — criterion vs `miner-bench` binary, the 28×3×6 sweep recipe, hyperfine wrapper, allocs<5%-of-hot-path measurement methodology.
3. Noise-replay sweep regression — null dataset, scans covered, FDR threshold, numeric pass criterion for "near-zero findings".
4. Security + license CI gates — `cargo audit` + `cargo deny`: fail vs warn, `deny.toml` policy, license allowlist, CI schedule.

**User response:** "please make the decisions based on typical Rust ecosystem expectations and best practices".

**Interpretation:** Effectively `--auto` mode for all four areas. Claude makes defensible defaults grounded in the existing project STACK.md, the Rust ecosystem's de-facto tooling, and the project's already-established posture (sync `miner-core`, stdout=findings/stderr=logs, Apache-2.0, six existing CI gates, three pinned family goldens). No follow-up questions — the user wants the planner to receive a fully-decided context.

### Claude's discretion — decisions captured

For each area, Claude wrote a locked decision in `07-CONTEXT.md` `<decisions>`. Summary:

#### D7-01: Fixture cache + quickstart

- Cache lives at `tests/fixtures/cache/` (workspace-shared, 2 instruments × bid × 1 month each, ≤ 5 MB compressed).
- Generator `scripts/generate-fixture-cache.sh` produces **synthetic stub** bytes from a deterministic seed (no Dukascopy redistribution).
- Quickstart scan: `seas.bucket.hour_of_day@1` on `EURUSD:bid` 15m for 2024-01-01:2024-01-31 — guaranteed to emit findings.
- README's existing `## Example` gets its hardcoded path replaced with the in-tree fixture path.

#### D7-02: Dukascopy caveats split

- README gets a 6-line summary paragraph after `## Example`.
- Deep doc `docs/data_sources.md` (~180-250 lines) covers cache layout, CSV schema, bid/ask independence, time zones / DST, gap policies, licensing posture, and the Apache-2.0 footer.

#### D7-03: Three-layer bench harness

- **Layer 1** — criterion microbenches at `crates/miner-core/benches/` (zstd decode, csv parse, aggregator, rolling corr, OLS, Ljung-Box). Not gated in CI.
- **Layer 2** — `miner-bench` binary becomes the 28-instrument × 3-timeframe × 6-year recipe runner; emits structured timing JSON.
- **Layer 3** — `scripts/run-bench.sh` wraps `hyperfine --warmup 3 --runs 5` and post-processes to `docs/bench-results.md`.
- **Allocation budget** — `dhat-rs` behind a `--features dhat` flag on `miner-bench`. Manual nightly recipe, not CI-gated.
- **Flamegraph** — `samply` documented in CONTRIBUTING.md; one reference image checked into `docs/bench-results/`.

#### D7-04: Noise-replay test

- Synthetic GBM (μ=0, σ=1e-4, N=100k bars) → phase-randomised via IAAFT → 100 surrogate instruments.
- Sweep across three scans (one per family) at α=0.05.
- Pass criterion: `assert!(false_positive_count <= 30)` (Wilson 99% upper bound for binomial(300, 0.05) is ~28; 30 leaves a slim safety margin).
- Test lives at `crates/miner-core/tests/noise_replay_regression.rs`, always-run by default.

#### D7-05: Security gates

- `cargo audit --deny warnings --deny unsound --deny unmaintained --deny yanked` on every push/PR. Fail-on-hit.
- `cargo deny check` on every push/PR with a `deny.toml` allowlist of permissive licenses (Apache-2.0, MIT, BSD-2/3-Clause, ISC, Unicode-DFS-2016, Unicode-3.0, Zlib, MPL-2.0).
- Bans: wildcard versions denied; multiple-versions warned (not denied — too aggressive for v1).
- Allowlist-by-exception with inline justification comments in `deny.toml`.
- No nightly schedule — every-push coverage is enough.

#### D7-06: Golden-fixture maintenance

- Add envelope-snapshot test (`tests/findings_envelope_snapshot.rs`) using `insta` or hand-rolled byte-equal.
- CONTRIBUTING.md gets a "Regenerating goldens" subsection walking through the three `generate_<scan>.py` scripts.
- No CI gate that re-runs the Python regen scripts (the goldens are pinned outputs, not living references).
- `REFERENCE-VERSIONS.md` stays at current pins for v1.0; v1.x can bump.

#### D7-07: Performance numbers home

- `docs/bench-results.md` is the canonical home — wall-clock numbers, flamegraph image, allocation summary.
- README does NOT embed any numbers (commit `f33878e` trimmed it; numbers go stale).
- The Documentation index does NOT get a new entry (bench numbers are an internal hardening artifact).
- The Roadmap section gets a one-line pointer.

### Deferred ideas

Surfaced during synthesis but explicitly out of Phase 7 scope:

- Property-test harness for new scans (future hardening pass).
- Lockfile-age automation (Dependabot / Renovate).
- Performance regression CI gates (need baseline variance data first).
- `#[ignore]` cleanup audit (could fold into Phase 7 plan if cheap).
- CHANGELOG.md scaffold (cheap; called out as a plan-phase decision).
- Dependabot / Renovate config (chore for v1.x).

### Open questions for plan-phase

Seven items left for the planner to finalise — listed in `07-CONTEXT.md` `<open_questions>`:

1. Fixture cache generator: real Dukascopy mirror or synthetic stub? (Recommendation: synthetic stub.)
2. Noise-replay test: `#[ignore]` or always-run?
3. `miner-bench` recipe TOML shape — bare `SweepManifest` or wrapper with bench knobs?
4. `docs/bench-results.md` initial state — include "How to reproduce" or defer to CONTRIBUTING.md?
5. `cargo deny` license allowlist baseline — audit current `Cargo.lock` first.
6. CHANGELOG.md scaffold — include in Phase 7 or push to milestone close?
7. README pointer to `docs/bench-results.md` — exact wording and placement.

### What was NOT discussed (out of scope)

- Whether to add new scans (no — phase 7 closes verification debt).
- Whether to extend the envelope (no — schema is locked).
- MCP / HTTP implementation (deferred to v2; tracked under PLAT-v2-07 + PLAT-v2-08).
- Releasing v1.0 (handled by `/gsd-complete-milestone` after Phase 7 verifies).
