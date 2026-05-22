#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Radius Red Ltd.
#
# Generate the synthetic Dukascopy-shape fixture cache at
# `tests/fixtures/cache/` from a deterministic seed (Plan 07-02).
#
# Output is byte-identical across machines: the underlying Rust generator
# (`crates/miner-bench/src/bin/gen-fixtures.rs`) uses the Numerical Recipes
# LCG (PATTERNS Pattern C) for per-day closes plus single-threaded zstd
# level 3 compression (RESEARCH Pitfall 4). The companion `SHA256SUMS`
# file is regenerated alongside the bytes; `sha256sum -c` against it
# proves byte-identity for the current run. Bytes and SHA256SUMS are
# gitignored (see `.gitignore`) — fresh clones generate them on demand
# via this script before running the README example.
#
# Usage:
#   bash scripts/generate-fixture-cache.sh

set -euo pipefail

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

# Clean state — wipe the per-symbol trees but preserve `.gitkeep` and the
# SHA256SUMS file (the Rust binary rewrites SHA256SUMS deterministically).
rm -rf tests/fixtures/cache/EURUSD tests/fixtures/cache/GBPUSD

# Generate the bytes + write SHA256SUMS via the Rust binary. The binary
# emits a one-line JSON summary to stdout; tracing logs go to stderr.
cargo run --release -p miner-bench --bin gen-fixtures

# Round-trip determinism check: the generator wrote both the bytes and the
# SHA256SUMS file; `sha256sum -c` confirms they match.
( cd tests/fixtures/cache && sha256sum -c SHA256SUMS )

echo "[generate-fixture-cache] OK — synthetic cache regenerated and SHA256SUMS verified."
