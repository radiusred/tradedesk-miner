# Coding Conventions

**Analysis Date:** 2026-05-25

## Naming Patterns

**Files:**
- Source modules use `snake_case.rs` (e.g., `aggregator.rs`, `path_layout.rs`, `gap_policy.rs`)
- Test files named for the thing tested: `scan_ljung_box.rs`, `aggregator_determinism.rs`
- Benchmark files prefixed `bench_`: `bench_rolling_corr.rs`, `bench_ols_fit_4d.rs`
- Kernel sub-modules always split into `kernel.rs` + `mod.rs` under a directory of the scan name

**Directories:**
- Scan families under `src/scan/anom/`, `src/scan/cross/`, `src/scan/seas/`
- Each scan is a subdirectory: `src/scan/anom/ljung_box_sq/kernel.rs` + `src/scan/anom/ljung_box_sq/mod.rs`
- Integration test helpers under `tests/common/mod.rs`, `tests/fixtures/mod.rs`
- Snapshot artefacts under `tests/snapshots/`
- Golden JSON files under `tests/goldens/` (hand-rolled byte-equal) or `tests/fixtures/` (statsmodels)

**Functions:**
- `snake_case` throughout
- Pure statistical kernels are `#[inline] pub(crate) fn` (never `pub`)
- Builder/constructor methods named `new`, `from_calendar`, `from_str`
- `as_str()` for enum → canonical wire-form string (used on `Side`, `Timeframe`, `GapPolicyKind`)
- `from_str(s: &str) -> Result<Self, &str>` for wire-form parse (NOT `impl std::str::FromStr`)
- `#[must_use]` on every non-mutating getter and constructor: 99 occurrences in `miner-core/src`

**Variables:**
- `snake_case`; loop indices `i`, `j`; accumulator pairs `sum_a`, `sum_b`; scratch `buf`
- LCG seed variables: `s` (u32 state), `seed` (u64 input)
- Return series: `out` for output `Vec` built in-place

**Types:**
- Structs, enums: `UpperCamelCase`
- Enum variant prefixes used when Rust identifiers cannot start with digits: `Tf15m`, `Tf1h`, `Tf1d`
- Error newtypes: `DukascopyError`, `AggregateError`, `CacheError`, `ScanError`, `MinerError`
- Trait objects: `&dyn Reader<Error = E>`, `&mut dyn FindingSink`
- Constants: `SCREAMING_SNAKE_CASE` — `AGGREGATOR_VERSION`, `ARROW_SCHEMA_VERSION`, `CODE_REVISION`, `BOOTSTRAP_CANCEL_POLL_CADENCE`

## Code Style

**Formatting:**
- `rustfmt` — enforced in CI via `cargo fmt --all -- --check`
- Rust 2024 edition; workspace MSRV 1.85

**Linting:**
- `clippy::pedantic` enabled workspace-wide at `warn` level (`[workspace.lints.clippy] pedantic = { level = "warn", priority = -1 }`)
- `unsafe_code = "forbid"` at workspace level — no `unsafe` blocks anywhere
- Production code must not use `println!`, `print!`, `eprintln!`, `eprint!`, `dbg!` — banned via `clippy.toml` `disallowed-macros`
- `cargo clippy --workspace --all-targets -- -D warnings` gates CI (Gate 2)
- Selective `#[allow(clippy::...)]` on specific functions always carries a `reason = "..."` argument (Rust 2024 `allow` syntax)
- `#[cfg_attr(test, allow(clippy::float_cmp, ...))]` at crate level in `lib.rs` for test-only relaxations — never in production paths

## Import Organization

**Order:**
1. `std::` imports
2. Third-party crate imports (alphabetical within group)
3. `crate::` or `super::` internal imports

**Example pattern:**
```rust
use std::collections::BTreeMap;
use std::io::{self, Write};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::findings::{Finding, FindingSink};
use crate::error::MinerError;
```

**Path Aliases:**
- None — no workspace-level `[alias]` for imports; only `[alias] xtask = "run --package xtask --"`

## Error Handling

**Two-layer error model:**
- Library errors: `thiserror`-derived enums (`MinerError`, `AggregateError`, `CacheError`, `ScanError`, `DukascopyError`)
- Wire/serialisable errors: `WireError` struct with open-string `code` field — NOT an enum so adding codes is additive
- Binary edges: `anyhow::Error` via `?` propagation in CLI `main()`
- `MinerError` does NOT derive `Serialize` — `Io(#[from] std::io::Error)` is incompatible with serde-derive

**Construction pattern:**
```rust
// Library errors — typed enums with #[from] bridges
#[derive(Debug, thiserror::Error)]
pub enum MinerError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("preflight error: {}", _0.message)]  // positional literal (not {0}) when inner lacks Display
    Preflight(WireError),
}

// Wire errors — open-string code, builder methods
WireError::preflight(PreflightCode::UnknownScan, "no such scan: foo@1")
WireError::scan(ScanErrorCode::ComputeError, "NaN in residuals")
    .with_context("param", json!("lags"))
```

**Error propagation:**
- `?` operator throughout; explicit `map_err` at adapter boundaries
- `expect("reason")` only for statically-impossible panics (e.g., `NaiveTime::from_hms_opt(22,0,0).expect("22:00:00 is a valid NaiveTime")`)
- Never bare `.unwrap()` in production paths; test code uses `expect("context string")`

## Logging

**Framework:** `tracing` crate (`tracing::info!`, `tracing::debug!`, `tracing::warn!`, `tracing::error!`)

**Initialisation:** `tracing-subscriber` with `.with_writer(std::io::stderr)` in each binary's `main()` — logs MUST land on stderr, never stdout

**Patterns:**
- Structured fields: `tracing::info!(symbol = %symbol, tf = %tf, "cache hit")` — span context wraps scan + instrument + timeframe nesting
- `miner-core` emits tracing spans; the CLI wrapper initialises the subscriber
- No `eprintln!` in any module — only `tracing::*!` or `stderr_emit::write_preflight_error`
- `stderr_emit::emit_to_stderr` is the ONLY path for structured preflight-rejection JSON to stderr

## Serde Conventions

**Critical rules (determinism contract OUT-03):**
- All map-typed fields use `BTreeMap` — NEVER `HashMap` — for alphabetic key order
- `serde_json` crate has NO `features = [...]` list in workspace deps (keeps BTreeMap-backed Map, not IndexMap)
- Optional fields that MUST serialise as JSON `null` (not omitted): NO `#[serde(skip_serializing_if = "Option::is_none")]` on those fields
- Additive optional fields use bare `#[serde(default)]` (enables legacy round-trip) without `skip_serializing_if`
- Open-string discriminators: `"kind"` (Finding), `"code"` (WireError), `"metric"` (Effect) are `String`, not enums, so adding values is additive
- `#[serde(tag = "kind", rename_all = "snake_case")]` on the `Finding` enum — all new variants automatically get snake_case discriminators
- `#[serde(rename = "scan_id@version")]` for field names containing `@` that can't be Rust identifiers

## Module Design

**Exports:**
- `miner-core/src/lib.rs` contains a FROZEN public surface block with explicit `pub use` re-exports
- Adding a name is backwards-compatible; removing one is a contract break
- Inner modules are `pub mod` for convenience but consumers MUST use `miner_core::TypeName` paths, not `miner_core::module::TypeName`

**Feature flags:**
- `test-internal` feature gates test-only fields/hooks (e.g., `ScanRequest.sleep_after_first_finding_ms`)
- `miner-core` dev-deps include `miner-core = { path = ".", features = ["test-internal"] }` — the self-reference pattern
- Production `cargo build` activates neither `cfg(test)` nor the feature — gated fields absent from release surface

**Crate dependency direction (strict one-way):**
```
miner-core (no internal deps)
    ↑
miner-reader-dukascopy
    ↑
miner-cli, miner-http, miner-mcp, miner-bench
```
- `miner-core` has ZERO workspace-internal dependencies
- Dev-dep cycles (`miner-core` dev-depends on `miner-reader-dukascopy`) are accepted and documented

## Comments

**When to Comment:**
- Every `pub` item carries a `///` doc comment explaining purpose, invariants, and error conditions
- Every module has a `//!` crate/module doc explaining the design decision and relevant design doc references (e.g., `D2-13`, `CACHE-04`, `OUT-03`)
- Phase/plan references in module docs: `Phase 3 (Plan 03-02 fills bodies)`, `Plan 04 owns the gap manifest side`
- Non-obvious `#[allow(...)]` always carries `reason = "..."` (Rust 2024)
- `// TODO(Phase N):` for deferred work tied to a future phase

**Inline comments:**
- Multi-line comments before complex expressions explaining the invariant being upheld
- `// Pin N — <what is pinned>` notation in tests that pin specific contracts

## Function Design

**Size:** Kernel functions are small and pure; `mod.rs` scan implementations (`Scan::run`) are longer but split across the `kernel.rs` helper
**Parameters:** 
- Pure kernels: `fn name(input: &[f64], param: usize) -> Vec<f64>`
- Scan entry: `fn run(&self, ctx: &ScanCtx, req: &ScanRequest, sink: &mut dyn FindingSink) -> Result<(), ScanError>`
**Return Values:**
- `Result<T, E>` for fallible operations
- `Option<T>` for nullable lookups
- `#[must_use]` on getters and constructors

---

*Convention analysis: 2026-05-25*
