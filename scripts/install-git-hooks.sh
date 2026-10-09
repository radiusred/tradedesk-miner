#!/usr/bin/env bash
#
# Wire the repo-local git hooks at `.githooks/` into this clone by pointing
# `core.hooksPath` at the tracked directory. Run once after cloning:
#
#   ./scripts/install-git-hooks.sh
#
# This keeps the hooks version-controlled — `.git/hooks/` stays the default
# `*.sample` set and the actual hooks live under `.githooks/` where every
# clone gets the same enforcement.

set -euo pipefail

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

if [ ! -d .githooks ]; then
    echo "install-git-hooks: .githooks/ directory missing — wrong repo or stale checkout" >&2
    exit 1
fi

# Make sure every hook in the tracked directory is executable.
chmod +x .githooks/*

# Point core.hooksPath at the tracked directory.
git config core.hooksPath .githooks

echo "install-git-hooks: configured core.hooksPath = .githooks (active hooks: $(ls .githooks | tr '\n' ' '))"
