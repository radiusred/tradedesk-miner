# Bench results

This file is the single canonical home for `tradedesk-miner` performance
numbers (D7-07). The README intentionally avoids embedded wall-clock or
allocation numbers — they go stale fast and the README is the wrong
surface for per-revision data. Update this doc as a separate
`chore(07): refresh bench numbers as of <commit-sha>` PR whenever the
reference workstation produces a new snapshot.

The reproduction recipes for every table on this page are documented
under `## How to reproduce` below — the artifacts they depend on
(`benches/recipes/*.toml`, `scripts/run-bench.sh`,
`scripts/run-alloc-profile.sh`, `crates/miner-bench`) all live in the
repo. The benches themselves were authored in Plan 07-06 (criterion
microbenches) and Plan 07-08 (recipe-runner + hyperfine/dhat wrappers).

## Reference workstation

| Field | Value |
|---|---|
| CPU | TBD — populate at bench capture time |
| RAM | TBD |
| OS | TBD |
| Rust toolchain | TBD (the workspace pins 1.85 stable; the capture run must use the same channel + minor version) |
| Commit SHA | TBD (`git rev-parse HEAD` at capture) |
| Capture date | TBD (UTC) |

Replace every `TBD` cell in a single commit when a new capture lands.
Do NOT mix capture-config updates with code changes — the bench numbers
are evidence, the code is the subject under test.

## Wall-clock results

Captured via `scripts/run-bench.sh` (hyperfine 1.20.0, `--warmup 3
--runs 5`). The script's JSON export at `/tmp/miner-bench.json` is the
source of truth; the table below is a hand-rendered summary.

| Recipe | Median wall clock | Runs | Notes |
|---|---|---|---|
| `benches/recipes/full-sweep.toml` | TBD ms | 5 | 28 instruments × 3 timeframes × 6 years × 3 scan families |
| `benches/recipes/single-job.toml` | TBD ms | 5 | dhat profiling target (single instrument × 15m × January 2024) |

The full-sweep recipe assumes a production-shape Dukascopy cache at
`MINER_CACHE_ROOT`; the checked-in fixture cache is sized for the
single-job recipe and produces many `ScanError` envelopes when fed to
the full sweep. The single-job recipe runs against the fixture cache
out of the box.

## Allocation budget

Captured via `scripts/run-alloc-profile.sh` (dhat 0.3.3 global
allocator behind the `dhat` Cargo feature on `miner-bench`). The
script writes `dhat-heap.json` to CWD; inspect either via
[`dh_view.html`](https://valgrind.org/dhat) or `jq '.callstacks[]'`.

| Site | Bytes allocated | % of total | Notes |
|---|---|---|---|
| TBD — populate from dhat-heap.json top-5 | TBD | TBD | TBD |

D7-03 success criterion #3 sets a 5 % allocation-overhead target for
the scan hot path (everything outside `miner_core::sweep::*` +
`miner_core::scan::*` + `miner_core::cache::*`). The target is a
regression-aware **goal**, not a CI gate — there's no automated
threshold check; reviewers compare new snapshots against the
historically captured numbers.

## Reference flamegraph

The reference flamegraph captures the hottest scan family per RESEARCH
Open Question 5 — `cross.cointegration.engle_granger@1` (full ADF +
OLS + half-life inner loop). Capture recipe via `samply` 0.13.1:

```sh
cargo install samply@0.13.1
cargo build --release --bin miner-bench
MINER_CACHE_ROOT=./tests/fixtures/cache \
MINER_BAR_CACHE_ROOT=/tmp/bar \
MINER_OUTPUT=stdout \
  samply record ./target/release/miner-bench \
    --recipe benches/recipes/single-job.toml
```

`samply record` opens the Firefox profiler UI on completion; export
the flat flamegraph PNG and commit it as
`docs/bench-results/flamegraph-<sha>.png` (where `<sha>` is the short
commit SHA at capture time). The `docs/bench-results/` directory is
tracked via a `.gitkeep` marker so the path exists even before the
first PNG lands.

No reference PNG captured yet — re-run the recipe above to produce
one.

## How to reproduce

A complete refresh of every table on this page is four commands plus
two manual edits:

1. Install the harness tooling. Both pins are documented in
   `CONTRIBUTING.md` ## Profiling:

   ```sh
   cargo install hyperfine@1.20.0 samply@0.13.1
   ```

2. Generate the fixture cache if it's absent (`run-alloc-profile.sh`
   does this automatically, but for direct invocation):

   ```sh
   bash scripts/generate-fixture-cache.sh
   ```

3. Capture wall-clock numbers. Requires a production-shape Dukascopy
   cache at `MINER_CACHE_ROOT` (see `docs/data_sources.md` for the
   cache layout):

   ```sh
   MINER_CACHE_ROOT=/path/to/dukascopy-cache bash scripts/run-bench.sh
   ```

   The hyperfine JSON export lives at `/tmp/miner-bench.json`; copy
   the median + stddev into `## Wall-clock results` above.

4. Capture allocation budget:

   ```sh
   bash scripts/run-alloc-profile.sh
   ```

   `dhat-heap.json` lands in the repo root (gitignored). Load it in
   `dh_view.html` and copy the top-5 callstacks by allocated bytes
   into `## Allocation budget` above.

5. Capture the reference flamegraph via the samply recipe in
   `## Reference flamegraph`; commit the PNG at
   `docs/bench-results/flamegraph-<sha>.png`.

6. Commit the refreshed tables + new flamegraph (if any) as a single
   `chore(07): refresh bench numbers as of <sha>` PR. The capture
   workstation's CPU / RAM / OS / Rust toolchain go into the
   `## Reference workstation` table in the same commit.

## See Also

- [README.md](../README.md) — `## Performance` section is the one-line
  pointer back to this file.
- [CONTRIBUTING.md](../CONTRIBUTING.md) — `## Profiling` subsection
  documents the samply recipe for ad-hoc performance investigation.
- [ARCHITECTURE.md](../ARCHITECTURE.md) — the layered design the
  scan-hot-path percentage is computed against.
- [docs/data_sources.md](data_sources.md) — Dukascopy cache layout
  the full-sweep recipe expects at `MINER_CACHE_ROOT`.

---

## License

Licensed under the Apache License, Version 2.0.
See: https://www.apache.org/licenses/LICENSE-2.0

Copyright 2026 [Radius Red Ltd.](https://github.com/radiusred) | [Contact](mailto:opensource@radiusred.uk)
