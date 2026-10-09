#!/usr/bin/env bash
#
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Radius Red Ltd.
#
# Regenerate the Phase 4 family goldens (ANOM-02, CROSS-05, SEAS-01) against
# the pinned Python 3.11 + statsmodels/scipy/pandas reference versions in
# `crates/miner-core/tests/goldens/REFERENCE-VERSIONS.md`.
#
#   ./scripts/regen-goldens.sh
#
# This uses `uv` to materialise an isolated Python 3.11 venv at
# `.venv-goldens/` (gitignored) and installs the lockfile-pinned wheel set
# from `crates/miner-core/tests/goldens/python-requirements.lock` with
# `--no-deps` so the lockfile is the single source of truth for every
# transitive version.
#
# Regen is required only when REFERENCE-VERSIONS.md is bumped or when the
# generate_*.py scripts themselves change; otherwise the committed goldens
# are bit-for-bit pinned and re-running this script must produce a no-op
# diff (idempotency check).

set -euo pipefail

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

command -v uv >/dev/null 2>&1 || { echo "uv not installed; install via https://docs.astral.sh/uv/" >&2; exit 2; }

uv venv --python 3.11 --clear .venv-goldens

uv pip install --no-deps -r crates/miner-core/tests/goldens/python-requirements.lock --python .venv-goldens/bin/python

.venv-goldens/bin/python crates/miner-core/tests/goldens/generate_summary_welford.py \
  > crates/miner-core/tests/goldens/stats.summary.welford.jsonl

.venv-goldens/bin/python crates/miner-core/tests/goldens/generate_engle_granger.py \
  > crates/miner-core/tests/goldens/cross.cointegration.engle_granger.jsonl

.venv-goldens/bin/python crates/miner-core/tests/goldens/generate_hour_of_day.py \
  > crates/miner-core/tests/goldens/seas.bucket.hour_of_day.jsonl

echo "[regen-goldens] OK — three goldens regenerated; review diff and commit as a single \"chore(07): regenerate family goldens\" commit per CONTRIBUTING.md ## Regenerating goldens."
