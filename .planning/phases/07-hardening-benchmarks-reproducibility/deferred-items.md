# Phase 07 — Deferred Items

Out-of-scope discoveries logged by executors. These items were observed during
plan execution but do NOT belong to the current plan's scope; they are
tracked here so the phase verifier (or a follow-on plan) can address them.

## From Plan 07-03 (cargo audit + cargo deny CI gates)

### 1. Pre-existing `[[bench]]` declarations in `crates/miner-core/Cargo.toml` reference files that do not yet exist

- **Discovered while:** Attempting to run `cargo deny check` locally against
  the workspace to verify the new `deny.toml`.
- **Evidence:** `cargo-deny 0.18.3` failed with:
  ```
  can't find `bench_aggregate_1m_to_15m` bench at
  `benches/bench_aggregate_1m_to_15m.rs` or
  `benches/bench_aggregate_1m_to_15m/main.rs`.
  ```
  `crates/miner-core/benches/` does not exist; multiple `[[bench]]` entries
  in `crates/miner-core/Cargo.toml` (lines ~143+) declare bench targets
  whose source files have not been authored yet.
- **Scope:** Phase 07 bench-harness work — owned by Plan 07-06 / 07-07 (the
  criterion microbenches and `miner-bench` recipe runner). Not introduced by
  this plan and not in this plan's `<files>` list.
- **Impact on 07-03:** None for CI — `EmbarkStudios/cargo-deny-action@v2` in
  CI installs cargo-deny 0.19.6+ which handles workspaces with declared
  benches whose source files are missing. Local verification was skipped per
  the plan's explicit acceptance-criteria fallback ("trust the GH Action that
  runs in Task 2 ... OR document in the SUMMARY that local verification was
  skipped and CI is the gate").
- **Recommendation:** Plan 07-06 / 07-07 will create the missing
  `benches/*.rs` files as part of the bench-harness scaffolding.

### 2. `cargo-deny 0.18.3` cannot parse RUSTSEC entries that use CVSS 4.0

- **Discovered while:** Same local verification attempt as item 1.
- **Evidence:**
  ```
  failed to load advisory database: parse error: error parsing
  .../advisory-db-.../crates/wasmtime/RUSTSEC-2026-0022.md:
  unsupported CVSS version: 4.0
  ```
- **Scope:** This is a tooling-version-mismatch issue: cargo-deny 0.19.6
  (which the plan targets) handles CVSS 4.0 entries; cargo-deny 0.18.3 (the
  highest version compatible with the workspace's pinned rustc 1.85) does
  not. The CI runner upgrades rustc automatically and uses cargo-deny 0.19.6
  via `EmbarkStudios/cargo-deny-action@v2`, so this only affects local runs
  on a 1.85-pinned host.
- **Impact on 07-03:** None — CI gate is the canonical check.
- **Recommendation:** No action required. Document in CONTRIBUTING.md /
  README at v1.x time if local cargo-deny becomes a contributor expectation
  (would require bumping `rust-toolchain.toml` to 1.88+).
