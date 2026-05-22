# Release Process

`tradedesk-miner` ships prebuilt binaries via GitHub Releases so consumers
(notably the RadiusRed Quant agent) don't need a Rust toolchain on the
target host. This document describes how a release moves from `main` to a
published Release page.

## At a glance

```
maintainer triggers prepare-release.yml (workflow_dispatch)
        │
        ▼
analyze ───── inspects conventional commits since last tag,
              computes next semver (major/minor/patch)
        │
        ▼
gate    ───── pauses until a reviewer approves the
              `release-approval` GitHub Environment
        │
        ▼
execute ───── bumps [workspace.package].version in Cargo.toml,
              commits "chore: release vX.Y.Z [skip ci]",
              generates release notes via git-cliff + cliff.toml,
              creates the GitHub Release
        │
        ▼ (release.published event)
publish.yml — cross-compiles `miner` for each target triple,
              uploads tarballs + checksums + SHA256SUMS manifest
              to the Release page
```

This is the Cargo analog of the sibling-repo pattern
(`tradedesk` and `tradedesk-dukascopy` use the radiusred reusable
release workflow + PyPI publishing). The job structure, GitHub App
authentication, and `release-approval` environment-gating are deliberately
identical; only the version-bump and packaging steps differ.

## Versioning

The workspace uses **a single version** declared at
`[workspace.package].version` in the root `Cargo.toml`. Every crate
inherits via `version.workspace = true`. The `prepare-release.yml`
`execute` step rewrites that one line; `cargo metadata` refreshes
`Cargo.lock`, and the commit lands on `main` as
`chore: release vX.Y.Z [skip ci]`.

Bumps are inferred from [Conventional Commits](https://www.conventionalcommits.org/):

| Commit prefix on any commit since the last tag | Resulting bump |
|------------------------------------------------|----------------|
| `<type>(scope)?!:` header (e.g. `feat!:`)      | **major**      |
| `BREAKING CHANGE:` / `BREAKING-CHANGE:` footer | **major**      |
| `feat(scope)?:`                                | **minor**      |
| anything else (`fix`, `chore`, `docs`, …)      | **patch**      |

## What you need (one-time setup)

In Settings → Secrets and variables → Actions:

- Variable: `RELEASE_APP_CLIENT_ID` — radiusred-release GitHub App client id
- Secret:   `RELEASE_APP_PRIVATE_KEY` — the App's private key (PEM)

In Settings → Environments:

- Environment: `release-approval` with required reviewers set to the
  designated release maintainers

These three settings are identical to `tradedesk` and `tradedesk-dukascopy`;
re-use the same App / reviewer list.

## Cutting a release

1. Land all feature work on `main`. Confirm CI is green.
2. Go to Actions → Prepare Release → Run workflow (branch: `main`).
3. The `analyze` job prints the planned bump and tag in the run summary.
4. The `gate` job pauses; a reviewer clicks **Review deployments → Approve and deploy**
   in the `release-approval` environment.
5. The `execute` job bumps the workspace version, commits, generates
   release notes from commit history (via `cliff.toml`), and creates the
   GitHub Release.
6. `publish.yml` fires automatically on `release.published`, builds the
   binary for each target triple, and attaches the tarballs + a
   `SHA256SUMS` manifest to the Release page.
7. Users install via the README snippet — no toolchain required.

## Target triples

Currently published per release:

- `x86_64-unknown-linux-gnu`  — primary (Quant agent host class)
- `aarch64-unknown-linux-gnu` — Graviton / Ampere
- `aarch64-apple-darwin`      — Apple Silicon developer workstations
- `x86_64-apple-darwin`       — Intel Mac developer workstations

To add a target, append to the matrix in `.github/workflows/publish.yml`.
Common candidates: `x86_64-unknown-linux-musl` (fully static for old
distros), `x86_64-pc-windows-msvc` (Windows).

## Verifying a release

Each Release page includes a `SHA256SUMS` manifest covering all artifacts.
Users verify before unpacking:

```sh
shasum -a 256 -c SHA256SUMS --ignore-missing
```

The release-tarball internal `SHA256SUMS` file (committed under
`tests/fixtures/cache/`, written by `scripts/generate-fixture-cache.sh`)
is unrelated — that one pins the synthetic-cache regression-test data
per Plan 07-02.

## Rolling back a release

`prepare-release.yml` produces an annotated tag and a "chore: release vX.Y.Z"
commit on `main`. If a release needs to be withdrawn:

1. `gh release delete vX.Y.Z` — removes the GitHub Release and its assets
2. `git push --delete origin vX.Y.Z` — removes the remote tag
3. Revert the version-bump commit on `main` via a follow-up PR

(`publish.yml` artifacts are tied to the Release, so deleting the Release
also removes them.)
