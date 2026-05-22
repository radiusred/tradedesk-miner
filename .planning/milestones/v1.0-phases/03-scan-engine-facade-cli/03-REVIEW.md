---
phase: 03-scan-engine-facade-cli
reviewed: 2026-05-18T00:00:00Z
depth: standard
files_reviewed: 42
files_reviewed_list:
  - crates/miner-cli/Cargo.toml
  - crates/miner-cli/src/cli.rs
  - crates/miner-cli/src/main.rs
  - crates/miner-cli/src/scan_args.rs
  - crates/miner-cli/tests/fixtures/ar1_seed.rs
  - crates/miner-cli/tests/fixtures/mod.rs
  - crates/miner-cli/tests/fixtures/statsmodels_golden.rs
  - crates/miner-cli/tests/scans_catalogue.rs
  - crates/miner-cli/tests/scan_subcommand_smoke.rs
  - crates/miner-cli/tests/sigint_preserves_stream.rs
  - crates/miner-core/Cargo.toml
  - crates/miner-core/src/aggregator.rs
  - crates/miner-core/src/engine/framing.rs
  - crates/miner-core/src/engine/gap_policy.rs
  - crates/miner-core/src/engine/mod.rs
  - crates/miner-core/src/engine/param_hash.rs
  - crates/miner-core/src/engine/preflight.rs
  - crates/miner-core/src/findings/mod.rs
  - crates/miner-core/src/findings/sink.rs
  - crates/miner-core/src/lib.rs
  - crates/miner-core/src/reader.rs
  - crates/miner-core/src/scan/ljung_box/kernel.rs
  - crates/miner-core/src/scan/ljung_box/mod.rs
  - crates/miner-core/src/scan/mod.rs
  - crates/miner-core/src/scan/registry.rs
  - crates/miner-core/src/scan/shape.rs
  - crates/miner-core/tests/common/counting_sink.rs
  - crates/miner-core/tests/common/mod.rs
  - crates/miner-core/tests/common/synthetic_cache.rs
  - crates/miner-core/tests/dry_run.rs
  - crates/miner-core/tests/fixtures/generate_golden.py
  - crates/miner-core/tests/gap_policy.rs
  - crates/miner-core/tests/public_surface_audit.rs
  - crates/miner-core/tests/scan_facade_determinism.rs
  - crates/miner-core/tests/scan_ljung_box.rs
  - crates/miner-core/tests/schema_roundtrip.rs
  - crates/miner-core/tests/shuffled_future_regression.rs
  - schemas/findings-v1.schema.json
  - schemas/scans-catalogue-v1.schema.json
  - xtask/Cargo.toml
  - xtask/src/main.rs
findings:
  critical: 3
  warning: 7
  info: 5
  total: 15
status: issues_found
---

# Phase 3: Code Review Report

**Reviewed:** 2026-05-18
**Depth:** standard
**Files Reviewed:** 42
**Status:** issues_found

## Summary

The Phase 3 scan-engine-facade-cli implementation is broad, well-documented, and has heavy test coverage including byte-determinism, look-ahead-safety, golden-fixture, and SIGINT-race integration tests. The locked envelope discipline (D-12..D-14 / OUT-02), `BTreeMap`-only map invariant (OUT-03), `dry_run` short-circuit (D3-21), and clock-isolation (D3-23) are all enforced at the type level with sibling pinning tests.

However, the adversarial review surfaces three correctness/contract bugs in the CLI ↔ engine boundary that compromise documented contracts:

1. **Torn `RunStart`/`RunEnd` framing on reader/cache errors** — `engine::run_one` emits `RunStart` then propagates `MinerError::Scan` from gap detection / cache loading WITHOUT emitting `RunEnd`. The CLI then exits via anyhow without going through `compute_exit_code`. Consumers see an orphaned `RunStart` line on stdout — a wire-protocol violation.
2. **Exit-code routing bypassed on non-preflight errors** — any `run_one` error other than the literal-string-matched `"unknown scan:"` skips `compute_exit_code`. SIGINT racing with a sink IO or reader error yields exit 1 instead of the contracted 130 (D3-24).
3. **String-match dispatch on `MinerError::Scan` is fragile** — `handle_scan_subcommand` uses `msg.starts_with("unknown scan:")` to classify preflight failures. A future rename of the format string in `run_one` silently regresses preflight classification with no compile-time gate.

Several quality issues also surfaced: an unused `code_revision` parameter on `ScanArgs::to_scan_request`, hardcoded `target/debug/miner` path in the SIGINT integration test, duplicated `mask_volatile_fields` helpers in two test trees, and `FileSink::create` not auto-creating parent directories.

No security vulnerabilities (injection, deserialisation, secrets) were identified.

## Critical Issues

### CR-01: Torn `RunStart`/`RunEnd` framing on reader/cache errors

**File:** `crates/miner-core/src/engine/mod.rs:208-251` and `:313-323`
**Issue:**
`run_one` emits `RunStart` at line 208 BEFORE the gap-detection call at line 250 (`GapDetector::detect(...).map_err(|e| MinerError::Scan(format!("reader: {e}")))?`). When the reader fails, the `?` returns `Err(MinerError::Scan(_))` and `emit_run_end` is NEVER reached. Same applies at line 323 for `cache.get_or_build` failures.

This violates the documented wire-protocol invariant: "every run emits a closing `RunEnd`" (D-09, framing.rs:1-7). Consumers (Quant agent, MCP/HTTP wrappers, the test scaffolding in `parse_findings`) structurally rely on the pair. Downstream JSONL parsers will either hang waiting for `RunEnd` or report incomplete envelopes.

The unit test `run_one_reader_error_wraps_via_miner_error_scan` (engine/mod.rs:1113-1149) actually proves this — it asserts the error variant but does NOT assert sink contents (`assert!(sink.0.is_empty())` was deliberately NOT added). The test therefore silently accepts the torn-framing behaviour as correct.

**Fix:**
Wrap step 5 (gap detection) and step 6's cache loads such that on `Err`, `emit_run_end` runs before returning. Example:

```rust
// Step 5
let manifest = match GapDetector::detect(reader, &req.instrument, req.side, req.window) {
    Ok(m) => m,
    Err(e) => {
        // Emit a ScanError finding + RunEnd before returning HadScanErrors.
        emit_scan_error(sink, run_id, &scan_id_at_version, req, reader.source_id(),
                        &format!("reader: {e}"))?;
        summary.scan_errors += 1;
        emit_run_end(sink, run_id, started, summary)?;
        return Ok(RunOutcome::HadScanErrors);
    }
};
```

Same pattern for the `cache.get_or_build` arm. Add a regression test that asserts `sink.0` contains both `RunStart` AND `RunEnd` when the reader is forced to error.

---

### CR-02: SIGINT exit-code routing bypassed on non-preflight errors

**File:** `crates/miner-cli/src/main.rs:107-111` and `:291-301`
**Issue:**
`handle_scan_subcommand` returns `anyhow::Result<RunOutcome>`. On any `MinerError` that is NOT `MinerError::Scan(msg starting with "unknown scan:")`, it wraps in `anyhow::anyhow!("engine::run_one: {e}")` (line 300). The `?` in `main` (line 108) propagates the Err and `compute_exit_code` is never called — meaning the SIGINT-overrides-everything rule from D3-24 is silently broken whenever a sink IO error or reader error coincides with an in-flight SIGINT.

```rust
// main.rs line 108
let outcome = handle_scan_subcommand(scan_args, &cfg, &mut *sink, Arc::clone(&cancel))?;
let code = compute_exit_code(cancel.load(Ordering::SeqCst), &outcome);
std::process::exit(code);
```

Note that `compute_exit_code` is only called on the `Ok` arm. On `Err`, anyhow's `Termination` impl prints the error and exits 1, regardless of whether `cancel` is set. So:
- SIGINT + broken pipe (downstream consumer closed) → exit 1, not 130.
- SIGINT + reader IO error → exit 1, not 130.

CONTEXT D3-24 explicitly mandates `cancelled? → 130` regardless of the other tier. The current routing only honours that for the `Ok(outcome)` path.

**Fix:**
Restructure so `compute_exit_code` runs on every path, including the Err one:

```rust
Command::Scan(scan_args) => {
    let result = handle_scan_subcommand(scan_args, &cfg, &mut *sink, Arc::clone(&cancel));
    let outcome = match result {
        Ok(o) => o,
        Err(e) => {
            tracing::error!(error = %e, "scan failed");
            RunOutcome::HadScanErrors // or a new variant
        }
    };
    let code = compute_exit_code(cancel.load(Ordering::SeqCst), &outcome);
    std::process::exit(code);
}
```

Add an integration test that flips `cancel` immediately after spawning a child against a forced-broken sink, asserts exit 130.

---

### CR-03: String-match dispatch on `MinerError::Scan` for preflight classification

**File:** `crates/miner-cli/src/main.rs:293-299` and `crates/miner-core/src/engine/mod.rs:193-195`
**Issue:**
The CLI demotes "unknown scan" engine errors to `PreflightFailed` via a substring match on the error message:

```rust
// main.rs:293
Err(miner_core::error::MinerError::Scan(msg)) if msg.starts_with("unknown scan:") => {
    let err = WireError::preflight(PreflightCode::UnknownScan, msg);
    let _ = emit_to_stderr(&err);
    Ok(RunOutcome::PreflightFailed)
}
```

The matching string is produced in `engine/mod.rs:194`:
```rust
MinerError::Scan(format!("unknown scan: {}@{}", req.scan_id, req.version))
```

If a future refactor changes the format string (drops the trailing colon, adds a prefix like `"engine: unknown scan:"`, switches to typed errors), the CLI silently re-classifies unknown-scan preflight failures as runtime errors. There is no compile-time gate locking the relationship.

Both grep paths (`grep -n "unknown scan:" crates/miner-core/src/engine/mod.rs` and `crates/miner-cli/src/main.rs`) would need to be kept in lockstep manually. The `engine::preflight::resolve_scan` helper already returns the typed `PreflightCode::UnknownScan` via `WireError`; the engine bypasses this path by re-implementing the lookup at run_one's step 2.

**Fix:**
Introduce a typed engine-level error variant for preflight-only failures (e.g., extend `MinerError` with `Preflight(PreflightCode, String)` or surface `WireError` directly), then dispatch on the typed code:

```rust
Err(MinerError::Preflight(PreflightCode::UnknownScan, msg)) => {
    let err = WireError::preflight(PreflightCode::UnknownScan, msg);
    let _ = emit_to_stderr(&err);
    Ok(RunOutcome::PreflightFailed)
}
```

Alternatively, factor the preflight resolution into `engine::preflight::resolve_scan` (which already returns typed `WireError`) and call it from `run_one` so the error vocabulary is typed throughout.

## Warnings

### WR-01: `ScanArgs::to_scan_request` accepts but never uses `code_revision`

**File:** `crates/miner-cli/src/scan_args.rs:124`
**Issue:**
The function signature `pub fn to_scan_request(&self, _code_revision: &str) -> Result<ScanRequest, WireError>` accepts a `_code_revision` parameter (underscore prefix = ignored) yet the doc comment on lines 114-116 claims:

> `code_revision` is `miner_core::CODE_REVISION` at the call site (the CLI's `main()` injects it so tests can substitute a stable string).

The parameter is never used in the body. Both production call sites pass `miner_core::CODE_REVISION` (main.rs:274, main.rs:470), creating the illusion of dependency injection that does not exist. A test that "substitutes a stable string" by passing a different value would observe no behavioural difference.

**Fix:** Either remove the parameter entirely or actually use it (e.g., for a future-extensibility field on `ScanRequest`). Update the doc comment accordingly:

```rust
pub fn to_scan_request(&self) -> Result<ScanRequest, WireError> { ... }
```

Call sites become `args.to_scan_request()`.

---

### WR-02: Hardcoded `target/debug/miner` path ignores `CARGO_TARGET_DIR`

**File:** `crates/miner-cli/tests/sigint_preserves_stream.rs:45-59`
**Issue:**
```rust
fn target_miner_path() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent().expect("crate parent")
        .parent().expect("workspace root")
        .join("target")
        .join("debug")
        .join("miner")
}
```

This always resolves to `<workspace_root>/target/debug/miner`. If the environment sets `CARGO_TARGET_DIR=/tmp/cargo-target` (common in CI to avoid network filesystem slowness) or `cargo build` is run with `--target-dir`, the test spawns the stale or non-existent binary at the hardcoded path. Cargo also nests target dirs under `target/<profile>` differently when `--release` is used.

**Fix:** Use the `CARGO_TARGET_TMPDIR` or `OUT_DIR` env, or query `cargo metadata`. Simpler: use `assert_cmd::Command::cargo_bin("miner")` (which already handles `CARGO_TARGET_DIR`) — though that won't pick up the explicit `--features test-internal` rebuild. A safe approach:

```rust
fn target_miner_path() -> PathBuf {
    // Honour CARGO_TARGET_DIR if set, else fall back to workspace target/.
    let target_dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent().unwrap().parent().unwrap().join("target")
        });
    target_dir.join("debug").join("miner")
}
```

---

### WR-03: `FileSink::create` does not create parent directory

**File:** `crates/miner-core/src/findings/sink.rs:173-180`
**Issue:**
```rust
pub fn create<P: AsRef<Path>>(path: P) -> Result<Self, MinerError> {
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(MinerError::Io)?;
    Ok(Self::from_file(file))
}
```

`OpenOptions::create(true)` creates the FILE but not its parent directory. If a user runs `miner scan ... --output ~/.local/share/miner/findings.jsonl` and the parent does not exist, the call fails with an opaque `std::io::ErrorKind::NotFound`. The CLI then surfaces this via anyhow without a friendly hint.

**Fix:** Create the parent directory at `create_dir_all` granularity before opening:

```rust
pub fn create<P: AsRef<Path>>(path: P) -> Result<Self, MinerError> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(MinerError::Io)?;
        }
    }
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(MinerError::Io)?;
    Ok(Self::from_file(file))
}
```

---

### WR-04: `resolve_toml_path` returns a relative `./miner.toml` PathBuf

**File:** `crates/miner-cli/src/cli.rs:127-142`
**Issue:**
```rust
let cwd = Path::new("./miner.toml");
if cwd.exists() {
    return Some(cwd.to_path_buf());
}
```

The function returns the relative path verbatim. `figment::providers::Toml::file` then resolves it against the process's CWD at the moment figment reads it. If the cwd changes between `resolve_toml_path` and the figment-file read (unlikely in `main()` but possible in library use or future async paths), figment looks in the wrong location.

`cwd.exists()` itself implicitly uses the current cwd, so the answer is correct at the time of the check, but using a non-canonical relative path means a `chdir` between `resolve` and `read` is silent rather than failing loudly.

**Fix:** Canonicalise the path to absolute form on success:

```rust
let cwd = Path::new("./miner.toml");
if cwd.exists() {
    return cwd.canonicalize().ok().or_else(|| Some(cwd.to_path_buf()));
}
```

This is also a robustness pin: any future regression that moves `MinerConfig::resolve` into a different thread/context will continue to work without depending on cwd staying constant.

---

### WR-05: `LjungBoxScan::run` reads `Utc::now()` violating D3-23 clock-isolation discipline

**File:** `crates/miner-core/src/scan/ljung_box/mod.rs:208`
**Issue:**
The engine documentation at `engine/mod.rs:150-155` says:

> `Utc::now()` is read EXACTLY TWICE inside this function — once at `started` (before `RunStart`) and once at `ended` (before `RunEnd`). Scans MUST NOT read the wall-clock; per-finding `produced_at_utc` reads come from inside the scan body (the engine does not synthesize them).

But the same comment ALSO says "Scans MUST NOT read the wall-clock" — yet `LjungBoxScan::run` at line 208 does:
```rust
produced_at_utc: Utc::now(),
```

This is a documented contradiction. Reading the comment one way, every scan IS allowed to read `Utc::now()` for its per-finding `produced_at_utc`. Reading it the other way, scans should NOT read the clock — which would mean the engine must pass a `produced_at_utc` baseline into `ScanCtx`. The current behaviour matches the first reading but the comment includes the explicit "MUST NOT" prohibition.

Either the comment in `engine/mod.rs:153-154` is wrong, or `LjungBoxScan::run` violates it. The look-ahead-safety proptest (`shuffled_future_regression.rs`) and byte-determinism test (`scan_facade_determinism.rs`) work around this by masking `produced_at_utc` before comparison — which suggests the implementers know about the clock read and accept it.

**Fix:**
Pick one and pin it. Recommended: clarify the doc comment to say "Scans MAY read `Utc::now()` for their per-finding `produced_at_utc` timestamp; the engine does NOT pre-synthesize it. Cross-run byte-determinism tests mask `produced_at_utc` as a volatile field." Update the comment in `engine/mod.rs:150-155` and the framing module's clock-isolation section so the discipline is consistent.

Alternatively, plumb a `produced_at_utc` parameter through `ScanCtx` and make the engine populate it from a single clock read.

---

### WR-06: `parse_iso_utc_window` produces misleading error for non-date inputs

**File:** `crates/miner-core/src/engine/preflight.rs:199-227` and `:233-250`
**Issue:**
Consider `parse_iso_utc_window("abcdefghij:2024-01-01")`:
- `s.find('T')` → None
- Falls through to date-only path (line 215)
- `s.len() > 11` is true (21 > 11)
- `lhs = "abcdefghij"` (10 chars, contains no T)
- sep at position 10 is `:` → matched
- rhs = "2024-01-01"
- `parse_iso_utc("abcdefghij")` fails — but the error message is `"ISO 8601 datetime must end with 'Z' (strict UTC, A3): \"abcdefghij\""`

The user sees a complaint about a Z suffix on input that is not even close to a datetime. The "date-only" check `NaiveDate::parse_from_str(s, "%Y-%m-%d")` fails silently and falls through to the strict-Z branch.

**Fix:** Detect "looks like a date" (e.g., regex `^\d{4}-\d{2}-\d{2}`) and emit a date-specific error first; or always produce a generic "could not parse as ISO 8601 date or datetime" message when neither path succeeds.

---

### WR-07: Duplicated `mask_volatile_fields` helpers risk drift

**File:** `crates/miner-core/tests/common/mod.rs:72-98` and `crates/miner-cli/tests/fixtures/mod.rs:124-150`
**Issue:**
Two near-identical implementations exist; each acknowledges the duplication in a doc-comment:

> Mirrors `crates/miner-cli/tests/cli_streams.rs::mask_volatile_fields` (whose `mod` block is not reachable from sibling integration tests — Cargo compiles each test file as a separate crate). Keep the masked-key list in sync...

Both also note a third copy (`cli_streams.rs::mask_volatile_fields`) exists. Three copies are kept in lockstep manually. Adding a new volatile field (e.g., a future `ttl_ms` framing addition) requires touching three files; the test suite does not gate against missing one.

**Fix:** Move `mask_volatile_fields` into `miner-core` under a `pub(crate)` or `#[cfg(any(test, feature = "test-internal"))]` gate so all three test trees consume the same source. Alternatively, hoist into a workspace-level `dev-deps` test-utilities crate (e.g., `miner-test-utils`).

## Info

### IN-01: Dead code in `scan_subcommand_smoke.rs` kept solely to silence warnings

**File:** `crates/miner-cli/tests/scan_subcommand_smoke.rs:27-30` and `:326-331`
**Issue:**
`schema_path()`, `use tempfile::TempDir`, and `_ensure_schema_path` exist only to suppress unused-import warnings:

```rust
#[allow(dead_code)]
fn _ensure_schema_path(_p: &Path) {
    let _ = schema_path();
    let _ = TempDir::new();
}
```

The comment says "Plan 06 keeps the schema path resolver around for future use" but no test in this file actually calls `schema_path()`. Dead code grows.

**Fix:** Remove the unused imports and `_ensure_schema_path`. If a future plan needs them, re-add at that time.

---

### IN-02: Test name "invalid_params" exercises `--side` not `--params`

**File:** `crates/miner-cli/tests/scan_subcommand_smoke.rs:163-202`
**Issue:**
`fn invalid_params_emits_wireerror_exit_1` passes `--side middle` rather than malformed `--params KEY=VAL`. The body acknowledges this:

> Supply --side with an invalid value so preflight rejects with invalid_parameter (the malformed-KEY=VAL params path is preempted by clap because clap accepts the string verbatim before preflight runs; an invalid --side value tests the same boundary code path)

Test name vs body drift. A reader scanning test names for coverage would conclude `--params` rejection is tested; it is not.

**Fix:** Rename to `invalid_side_emits_wireerror_exit_1`, or add a separate test that exercises malformed `--params` directly via `parse_params_kv` unit-level OR via a clap-pre-validated value like `--params 'lags=not-a-number'`. (Note: A9 typed-fallback means `lags=foo` becomes `{"lags": "foo"}` and is only rejected later by LjungBoxScan; a true invalid-params boundary test would need `--params malformed-no-equals`.)

---

### IN-03: `Pid::from_raw(child_pid as i32)` truncates without bounds check

**File:** `crates/miner-cli/tests/sigint_preserves_stream.rs:134-158`
**Issue:**
```rust
let child_pid = child.id();  // u32
...
kill(Pid::from_raw(child_pid as i32), Signal::SIGINT).expect("kill SIGINT");
```

`as i32` truncates u32 values above `i32::MAX`. Linux PIDs are typically capped at `pid_max` (default 32768, max ~4M) so this is safe in practice — but the cast is not documented and would produce a wrong PID on a system with `pid_max > 2^31`.

**Fix:** Use `i32::try_from(child_pid).expect("PID fits in i32")` to fail loudly if the assumption ever breaks:

```rust
let pid_i32 = i32::try_from(child_pid).expect("PID fits in i32");
kill(Pid::from_raw(pid_i32), Signal::SIGINT).expect("kill SIGINT");
```

---

### IN-04: `wall_clock_ms` can be negative if clock skews

**File:** `crates/miner-core/src/engine/framing.rs:124-136` and `crates/miner-cli/src/main.rs:199`
**Issue:**
`wall_clock_ms = ended.signed_duration_since(started).num_milliseconds()` (returns `i64`). If `Utc::now()` between the two reads is non-monotonic (NTP backstep, VM clock jump), the value can be negative. The schema (line 478-480) declares `wall_clock_ms: int64` without `minimum: 0`, so the schema accepts the negative value — but consumers assuming durations are non-negative will misbehave.

**Fix:** Either (a) use `std::time::Instant` for the duration calculation (monotonic) while keeping `Utc::now()` reads for the timestamps, or (b) clamp `wall_clock_ms` to `>= 0` and emit a `tracing::warn!` if clock skew is detected. Option (a) is the principled fix:

```rust
let start_instant = std::time::Instant::now();
// ... after the scan ...
let elapsed_ms = i64::try_from(start_instant.elapsed().as_millis()).unwrap_or(i64::MAX);
```

---

### IN-05: `serde_json` `preserve_order` feature would break determinism without a compile-time gate

**File:** `crates/miner-core/src/engine/param_hash.rs:53-66` and `crates/miner-core/src/engine/preflight.rs:119-145`
**Issue:**
`param_hash` and `parse_params_kv` both depend on `serde_json::Map` being `BTreeMap`-backed (alphabetic key ordering, deterministic serialisation). This is true ONLY when the workspace's `serde_json` dependency does NOT activate the `preserve_order` feature. The current `Cargo.toml` is correct, but there is no compile-time assertion preventing a future plan from adding the feature in a different crate (which would propagate through Cargo feature unification and silently break `param_hash` byte-stability).

The unit test `param_hash_btreemap_order_invariant` (param_hash.rs:115-138) only proves the runtime invariant on the test's own `serde_json::Map`. It does NOT prevent a `serde_json/preserve_order` feature from being enabled in another crate.

**Fix:** Add a compile-time gate that fails to build if `serde_json/preserve_order` is active. One approach is a build-script check; another is a static-assertion-style check:

```rust
// In a build.rs or in a doc-comment-gated test:
#[cfg(feature = "serde_json/preserve_order")]
compile_error!("miner-core requires serde_json without `preserve_order` for determinism (OUT-03)");
```

(Cargo features don't usually transitively expose like this in user code, but a `cargo tree -e features` CI gate or a `cargo deny` rule can enforce it.)

---

_Reviewed: 2026-05-18_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
