#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Radius Red Ltd.
#
# Heap-allocation profiler for the miner-bench single-job recipe. Builds
# `miner-bench` with `--features dhat` so `dhat::Alloc` is installed as the
# global allocator; the dhat profiler writes `dhat-heap.json` to CWD when
# the binary exits.
#
# Pre-reqs:
#   * `[profile.release] debug = 1` in workspace Cargo.toml (Plan 07-06
#     landed this — line-tables-only debug info is required for dhat symbol
#     attribution per 07-RESEARCH.md §"Pitfall 1"; full `debug = true` is
#     forbidden — it 5x'es release-binary size).
#   * dhat-heap.json viewer:
#       - Download `dh_view.html` from https://valgrind.org/dhat (standalone
#         offline viewer), OR
#       - Inspect the JSON directly: `jq '.callstacks[]' dhat-heap.json`.
#   * MINER_CACHE_ROOT pointing at the fixture cache (default below).
#
# Usage:
#   bash scripts/run-alloc-profile.sh
#
# The default cache root is `./tests/fixtures/cache` — the script will
# (re-)generate the fixture if its sentinel file is absent.

set -euo pipefail

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

: "${MINER_CACHE_ROOT:=./tests/fixtures/cache}"
: "${MINER_BAR_CACHE_ROOT:=/tmp/bar-alloc-profile}"
: "${MINER_OUTPUT:=stdout}"
export MINER_CACHE_ROOT MINER_BAR_CACHE_ROOT MINER_OUTPUT

# Generate the fixture cache if it isn't present. The sentinel file is the
# first January-2024 weekday for EURUSD bid side — if THAT file exists, the
# full fixture set is assumed to be in place (gen-fixtures.rs is atomic).
SENTINEL="tests/fixtures/cache/EURUSD/2024/00/01_bid.csv.zst"
if [ ! -f "$SENTINEL" ]; then
    echo "[run-alloc-profile] fixture sentinel $SENTINEL missing; regenerating via scripts/generate-fixture-cache.sh" >&2
    bash scripts/generate-fixture-cache.sh
fi

# Build + run with the dhat feature. Release profile is required so symbol
# attribution lines up with the production build that hyperfine measures.
cargo run --release --features dhat -p miner-bench --bin miner-bench -- \
    --recipe benches/recipes/single-job.toml

echo "[run-alloc-profile] dhat-heap.json written to CWD: $(pwd)/dhat-heap.json"
echo "[run-alloc-profile] inspect via dh_view (https://valgrind.org/dhat) or 'jq .callstacks[] dhat-heap.json'"
