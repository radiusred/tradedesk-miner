#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Radius Red Ltd.
#
# Wall-clock benchmark recipe runner. Wraps `cargo run --release -p miner-bench
# --bin miner-bench -- --recipe benches/recipes/full-sweep.toml` with
# `hyperfine` warmup + multi-run statistics so the wall-clock numbers are
# stable enough to land in `docs/bench-results.md`.
#
# Output:
#   /tmp/miner-bench.json — hyperfine's JSON export (mean / median / stddev /
#                           individual run timings). Post-process into the
#                           markdown table at `docs/bench-results.md
#                           ## Wall-clock results`.
#
# Pre-reqs:
#   * hyperfine 1.20+ installed: `cargo install hyperfine@1.20.0`
#     (CONTRIBUTING.md ## Profiling references the same pin.)
#   * MINER_CACHE_ROOT must point at a production-shape Dukascopy cache for
#     the 28 × 3 × 6 sweep. The checked-in fixture cache covers only 2
#     symbols × 1 month (sized for `benches/recipes/single-job.toml`); running
#     the full sweep against it produces many `ScanError` envelopes from
#     missing instrument/year files — that's expected if you're sanity-
#     checking the pipeline but it is NOT a useful benchmark workload.
#   * MINER_BAR_CACHE_ROOT writable scratch path (defaults to /tmp/bar-bench
#     below).
#   * MINER_OUTPUT=stdout (the runner emits one JSON timing line to stdout;
#     log lines go to stderr).
#
# Usage:
#   bash scripts/run-bench.sh
#
# Override the cache root for a fixture-only smoke check (NOT a real bench):
#   MINER_CACHE_ROOT=./tests/fixtures/cache bash scripts/run-bench.sh

set -euo pipefail

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

if ! command -v hyperfine >/dev/null 2>&1; then
    echo "run-bench: hyperfine not installed; run 'cargo install hyperfine@1.20.0'" >&2
    exit 2
fi

# Provide writable defaults for the bar-cache + output env vars so the
# miner-bench binary's `MinerConfig::resolve` call has everything it needs.
# The bench user is expected to set MINER_CACHE_ROOT explicitly — it has no
# safe default.
: "${MINER_BAR_CACHE_ROOT:=/tmp/bar-bench}"
: "${MINER_OUTPUT:=stdout}"
export MINER_BAR_CACHE_ROOT MINER_OUTPUT

if [ -z "${MINER_CACHE_ROOT:-}" ]; then
    echo "run-bench: MINER_CACHE_ROOT not set; the full-sweep recipe expects a production-shape Dukascopy cache." >&2
    echo "         For a smoke check against the checked-in fixture, run:" >&2
    echo "           MINER_CACHE_ROOT=./tests/fixtures/cache bash scripts/run-bench.sh" >&2
    exit 2
fi
export MINER_CACHE_ROOT

# Hyperfine warmup + multi-run statistics. Per RESEARCH Pattern 7: 3 warmup
# iterations + 5 measured runs is enough to drive stddev below ~3% on a warm
# system; bump --runs if your reference workstation shows noisy numbers.
hyperfine \
    --warmup 3 \
    --runs 5 \
    --export-json /tmp/miner-bench.json \
    "cargo run --release -p miner-bench --bin miner-bench -- --recipe benches/recipes/full-sweep.toml"

echo "[run-bench] hyperfine export: /tmp/miner-bench.json"
echo "[run-bench] paste the timing summary into docs/bench-results.md ## Wall-clock results"
