# Contributing to tradedesk-miner

Thanks for your interest. This document covers the development setup, the
quality gates your changes need to pass, and what to expect when opening a
pull request.

## Development setup

Prerequisites: Rust 1.85+ stable (`rustup default 1.85`) and git.

```sh
git clone https://github.com/radiusred/tradedesk-miner
cd tradedesk-miner
./scripts/install-git-hooks.sh   # one-time: wires the pre-commit gate
cargo build --workspace
cargo test --workspace
```

`install-git-hooks.sh` points `core.hooksPath` at the tracked `.githooks/`
directory so your local pre-commit hook mirrors the CI fmt and clippy gates.

### Hook override environment variables

| Variable | Effect |
|---|---|
| `MINER_AUTOFIX=1` | Hook re-stages fmt fixes and continues instead of aborting |
| `MINER_SKIP_CLIPPY=1` | Hook skips clippy (CI still enforces the gate) |
| `git commit --no-verify` | Bypasses the hook entirely |

## Quality gates

CI runs the gates below on every push and PR. The pre-commit hook covers
gates 1 and 2; run the others locally before pushing.

1. `cargo fmt --all -- --check` — formatting drift fails the build.
2. `cargo clippy --workspace --all-targets -- -D warnings` — lints, including
   the workspace `clippy.toml` `disallowed-macros` rule that bans `println!` /
   `eprintln!` / `dbg!` outside the sanctioned `StdoutSink` and the logging
   adapter. Stdout = findings, stderr = logs.
3. `cargo test --workspace --no-fail-fast` — unit, integration, doctest, and
   golden-fixture suites.
4. `cargo build --workspace --all-targets` — compile health across every
   crate and target kind.
5. **Tokio-free `miner-core`.** `cargo tree -p miner-core --edges
   normal,build` must show zero async-runtime crates (`tokio`, `async-std`,
   `smol`, `async-trait`, `async-io`, `async-channel`, `async-executor`,
   `async-task`). Async lives only at the wrapper edges via
   `tokio::task::spawn_blocking`. Dev-dependencies are exempt.
6. **Schema sync.** `cargo run -p xtask -- gen-schema` regenerates
   `schemas/findings-v1.schema.json` from the `schemars` derives. The
   committed schema is the contract — if you change a Rust type that affects
   the envelope, re-run the gen and commit the diff in the same PR.
7. **cargo audit.** CI runs `rustsec/audit-check@v2.0.0` against the
   [RustSec advisory database](https://rustsec.org/) on every push and PR.
   Fails the build on any advisory hit. Zero days tolerance. If a CVE
   genuinely needs a temporary ignore — for example, upstream has not
   released a fix yet — document it in `deny.toml`'s `[advisories] ignore`
   array with an inline `RUSTSEC-YYYY-NNNN — <one-line reason> — review by
   YYYY-MM-DD` comment so the ignore is auditable and time-boxed.
8. **cargo deny check.** CI runs `EmbarkStudios/cargo-deny-action@v2`
   against the [`deny.toml`](deny.toml) at the repo root. Four sub-checks
   run as one gate: licenses (the locked allowlist of permissive licenses
   in `deny.toml`'s `[licenses] allow`), bans (`wildcards = "deny"`,
   `multiple-versions = "warn"`), advisories (mirrors the cargo audit gate
   so a single config controls the policy), and sources
   (`unknown-registry = "deny"`, `unknown-git = "deny"`). New dependencies
   must satisfy the license allowlist out of the box; the policy is
   **allowlist-by-exception**, meaning if a contributor needs a license
   outside the current allowlist, the PR explains why and the allowlist
   extension lands as a separate commit in `deny.toml` with an inline
   `# allowed-for: <crate>@<version> — <license> — <reason>` comment.

## Regenerating goldens

The three family goldens
(`crates/miner-core/tests/goldens/stats.summary.welford.jsonl`,
`crates/miner-core/tests/goldens/cross.cointegration.engle_granger.jsonl`,
`crates/miner-core/tests/goldens/seas.bucket.hour_of_day.jsonl`) are
bit-for-bit pinned against the Python reference versions documented in
[`crates/miner-core/tests/goldens/REFERENCE-VERSIONS.md`](crates/miner-core/tests/goldens/REFERENCE-VERSIONS.md).
Regen is required only when `REFERENCE-VERSIONS.md` is bumped or when one of
the `generate_*.py` scripts themselves changes; otherwise the committed
goldens are the source of truth and the integration tests run against them
unchanged.

The canonical recipe is a single command:

```sh
./scripts/regen-goldens.sh
```

The script uses [`uv`](https://docs.astral.sh/uv/) to materialise an
isolated Python 3.11 venv at `.venv-goldens/` (gitignored), installs the
exact wheel set from
`crates/miner-core/tests/goldens/python-requirements.lock` with `--no-deps`
(so the lockfile is the single source of truth for every transitive
version), and runs the three `generate_*.py` scripts. Re-running the
script must produce a no-op diff against the committed goldens
(idempotency check) — any unexpected drift indicates a `REFERENCE-VERSIONS.md`
mismatch or a generator-script change.

**Commit discipline.** The resulting diff must land as a single
`chore: regen goldens after <reason>` commit (for example,
`chore(07): regenerate family goldens after scipy bump`) — never mix a
golden regen with behavioural changes in the same commit, because the
golden diff is large, machine-generated, and obscures the intent of
adjacent code changes. Review the diff carefully and confirm the
`provenance.*_version` values match the new
[`REFERENCE-VERSIONS.md`](crates/miner-core/tests/goldens/REFERENCE-VERSIONS.md)
pins before committing.

## Profiling

For performance investigation, `samply` is the recommended profiler — a
modern replacement for `cargo-flamegraph` whose output renders directly
in the Firefox profiler UI:

```sh
cargo install samply@0.13.1
cargo build --release --bin miner-bench
MINER_CACHE_ROOT=./tests/fixtures/cache \
MINER_BAR_CACHE_ROOT=/tmp/bar \
MINER_OUTPUT=stdout \
  samply record ./target/release/miner-bench \
    --recipe benches/recipes/single-job.toml
```

For heap-allocation profiling, use the dhat wrapper at
[`scripts/run-alloc-profile.sh`](scripts/run-alloc-profile.sh) (requires
the `dhat` Cargo feature on `miner-bench`). For wall-clock benchmarks,
use [`scripts/run-bench.sh`](scripts/run-bench.sh) (hyperfine wrapper).
The full reproduction recipes — including how to refresh the
benchmark tables — live in [`BENCHMARKING.md`](BENCHMARKING.md)
`## How to reproduce`.

## Pull request expectations

- **One concern per PR.** Small, atomic commits beat one large rewrite.
- **Conventional commit messages.** `feat:` / `fix:` / `docs:` / `chore:` /
  `test:` / `refactor:` are the common prefixes; an optional scope helps
  (e.g. `feat(scan): ...`, `docs(envelope): ...`).
- **Tests for behavioural changes.** Add or update a test that would have
  caught the bug or proves the new behaviour. The
  `crates/miner-core/tests/goldens/` fixtures pin reference outputs against
  pinned `statsmodels` / `scipy` / `pandas` versions — regen via the bundled
  `generate_<scan>.py` recipes.
- **Documentation.** If your change affects the `Finding` envelope, the scan
  catalogue, the sweep manifest grammar, or the CLI surface, update the
  matching doc under [`docs/`](docs/) in the same PR.
- **License headers.** New Rust source files follow the existing pattern (no
  per-file header; the workspace LICENSE applies). New scripts and runnable
  examples carry `# SPDX-License-Identifier: Apache-2.0` and
  `# Copyright 2026 Radius Red Ltd.` on the first two lines.
- **Run the gates locally.** Don't ship a PR that you haven't seen pass
  `cargo fmt --check && cargo clippy --workspace --all-targets -- -D
  warnings && cargo test --workspace` on your own machine.

## Reporting bugs

Open an issue with a minimal reproduction: the exact CLI invocation, the
expected JSONL fragment, and the actual JSONL fragment (truncated is fine).
If the bug is data-shaped, attach the smallest possible cache slice that
triggers it.

## License

By contributing, you agree that your contributions will be licensed under
the Apache License, Version 2.0. See [LICENSE](LICENSE).
