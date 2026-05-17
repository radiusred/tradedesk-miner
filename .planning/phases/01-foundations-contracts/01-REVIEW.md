---
phase: 01-foundations-contracts
reviewed: 2026-05-17T00:00:00Z
depth: standard
files_reviewed: 31
files_reviewed_list:
  - .cargo/config.toml
  - Cargo.toml
  - clippy.toml
  - crates/miner-bench/Cargo.toml
  - crates/miner-bench/src/main.rs
  - crates/miner-cli/Cargo.toml
  - crates/miner-cli/src/cli.rs
  - crates/miner-cli/src/main.rs
  - crates/miner-cli/tests/cli_streams.rs
  - crates/miner-core/build.rs
  - crates/miner-core/Cargo.toml
  - crates/miner-core/src/config/mod.rs
  - crates/miner-core/src/error/codes.rs
  - crates/miner-core/src/error/mod.rs
  - crates/miner-core/src/error/stderr_emit.rs
  - crates/miner-core/src/findings/base64_bytes.rs
  - crates/miner-core/src/findings/mod.rs
  - crates/miner-core/src/findings/run_id.rs
  - crates/miner-core/src/findings/sink.rs
  - crates/miner-core/src/lib.rs
  - crates/miner-core/tests/config_precedence.rs
  - crates/miner-core/tests/schema_roundtrip.rs
  - crates/miner-http/Cargo.toml
  - crates/miner-http/src/main.rs
  - crates/miner-mcp/Cargo.toml
  - crates/miner-mcp/src/main.rs
  - crates/miner-reader-dukascopy/Cargo.toml
  - crates/miner-reader-dukascopy/src/lib.rs
  - .github/workflows/ci.yml
  - .gitignore
  - README.md
  - rust-toolchain.toml
  - xtask/Cargo.toml
  - xtask/src/main.rs
findings:
  critical: 2
  warning: 7
  info: 6
  total: 15
status: issues_found
---

# Phase 1: Code Review Report

**Reviewed:** 2026-05-17
**Depth:** standard
**Files Reviewed:** 34 (one file path appears in two sibling crates as test+lib + Cargo.toml)
**Status:** issues_found

## Summary

The Phase 1 contract surface is broadly coherent and the test scaffolding is unusually
thorough for a Phase 1 (round-trip, schema-validation, twice-run byte-identity, env
isolation via `figment::Jail`). The locked envelope shape, error vocabulary, single
sanctioned sink writer, and `figment` builder all match the documented decisions.

However, adversarial review surfaces fifteen findings — two Critical, seven Warning,
six Info — that contradict claims in the README and the phase contracts:

1. **The CLI silently ignores the resolved `OutputDest`.** `emit_fixture()` hard-codes
   `StdoutSink::new()` regardless of `cfg.output`. If a user (or agent) sets
   `MINER_OUTPUT=/tmp/out.jsonl`, validation succeeds, the file is never written, and the
   findings appear on stdout instead. This breaks the documented "any other string is
   treated as a file path" contract (cli.rs:43-48) and means the env/TOML/CLI precedence
   for `output` is tested in isolation but unwired in production.
2. **Threat T-01-04 (code revision tampering) has a hole.** `build.rs` uses
   `git diff --quiet` (worktree-vs-index) rather than `git diff --quiet HEAD`
   (worktree-vs-HEAD). A binary built from a tree with **staged-but-uncommitted**
   changes will carry a clean SHA in `code_revision` — exactly the masquerade the
   `dirty-<sha>` suffix is supposed to prevent.

Beyond the two BLOCKERs, the most consequential WARNINGs are:

- The `Env`-layer path for `OutputDest::File(...)` is non-functional (only `"stdout"`
  parses; any path string fails serde deserialization with `unknown variant`). The
  documentation and tests do not surface this.
- No `version` field is declared on any crate; Cargo defaults silently to `"0.0.0"`, and
  this string is then stamped into every envelope as `RunStart.miner_version`. Agents
  reading the wire stream cannot distinguish miner releases.
- `blake3` is listed as a workspace dep on `miner-core` and never used in source. This
  is dead dep weight and a hint that some Plan-03 ambition slipped.
- `dsr` and `fdr_q` are not in the schema's `required` list, so the OUT-02
  "MUST serialise as null in v1" contract is enforced by the producer alone — a
  conforming consumer cannot reject a finding that omits them.

CI is wired with all four D-21 gates (build, clippy, tokio-tree grep, schema-sync diff)
plus fmt and test. Stream discipline holds via three layers (clippy bans, `StdoutSink`
gating, `tracing → stderr`); no violations found in product code.

## Critical Issues

### CR-01: CLI silently ignores resolved `OutputDest`; `MINER_OUTPUT=<path>` is a no-op in v1

**File:** `crates/miner-cli/src/main.rs:50-66`, `crates/miner-cli/src/main.rs:104-130`,
`crates/miner-cli/src/cli.rs:43-48`

**Issue:**
The CLI carefully resolves `cfg.output` via `MinerConfig::resolve(...)` and immediately
discards the result by binding to `_cfg`:

```rust
let _cfg: MinerConfig = match MinerConfig::resolve(toml_path.as_deref(), parsed.overrides()) {
    Ok(c) => c,
    Err(e) => { ... }
};
```

`emit_fixture()` then unconditionally constructs `StdoutSink::new()`:

```rust
fn emit_fixture() -> anyhow::Result<()> {
    tracing::info!("emitting fixture");
    let mut sink = StdoutSink::new();
    ...
```

Combined with the public CLI documentation:

```rust
/// Override the output destination. `stdout` for streaming JSONL on stdout
/// (the v1 default for agent-operability); any other string is treated as
/// a file path.
#[arg(long, global = true, env = "MINER_OUTPUT")]
pub output: Option<String>,
```

…and the locked `OutputDest::File(PathBuf)` variant in `MinerConfig`, this means
a user invocation like:

```sh
MINER_CACHE_ROOT=/c MINER_BAR_CACHE_ROOT=/b MINER_OUTPUT=/tmp/out.jsonl \
  cargo run -p miner-cli -- emit-fixture
```

…is silently misleading: config validation passes (the `File(PathBuf)` variant is
constructed via `Cli::overrides()`), but findings land on **stdout**, not the file.
`/tmp/out.jsonl` is never created. Worse, agent operators piping to `jq` will see
findings that they thought were redirected.

This is the kind of silent-success/wrong-side-effect bug Phase 1 specifically called out
in the contract surface ("agent-operability: every miner capability must be reachable
from CLI, MCP, and HTTP without divergent behavior"). The documented behaviour
(`OutputDest::File` is "any other string is treated as a file path") is not implemented.

**Fix:** Either implement the dispatch, or remove the public claim. Two acceptable shapes:

```rust
// Option A — wire it. (preferred)
fn make_sink(dest: &OutputDest) -> anyhow::Result<Box<dyn FindingSink>> {
    match dest {
        OutputDest::Stdout => Ok(Box::new(StdoutSink::new())),
        OutputDest::File(p) => {
            let f = std::fs::OpenOptions::new()
                .create(true).append(true).open(p)?;
            // (Add a `FileSink` impl in miner-core that mirrors StdoutSink semantics
            // — BufWriter + per-envelope flush — to keep the single-sink discipline.)
            Ok(Box::new(FileSink::new(f)))
        }
    }
}

fn main() -> anyhow::Result<()> {
    ...
    let cfg = MinerConfig::resolve(...)?;     // no `_cfg`
    let mut sink = make_sink(&cfg.output)?;
    match parsed.command {
        Command::EmitFixture => emit_fixture(&mut *sink)?,
    }
    ...
}
```

```rust
// Option B — fail-loud until the wiring lands.
let cfg = MinerConfig::resolve(...)?;
if !matches!(cfg.output, OutputDest::Stdout) {
    let err = WireError::preflight(
        PreflightCode::InvalidConfig,
        "output: only `stdout` is supported in Phase 1",
    );
    let _ = emit_to_stderr(&err);
    std::process::exit(1);
}
```

Whichever path is chosen, add an end-to-end test that asserts `MINER_OUTPUT=<file>`
either writes to the file (Option A) OR rejects with a structured WireError (Option B) —
the current `cli_streams.rs` does not exercise this path.

---

### CR-02: `build.rs` dirty-tree detection misses staged changes — T-01-04 mitigation has a hole

**File:** `crates/miner-core/build.rs:49-55`

**Issue:**
The `dirty` flag is computed via:

```rust
let dirty = Command::new("git")
    .args(["diff", "--quiet"])
    .status()
    .map(|s| !s.success())
    .unwrap_or(false);
```

`git diff --quiet` (no revision argument) compares **worktree vs. index**. Files that
have been `git add`ed but not yet committed look "clean" to this command, even though
they differ from `HEAD`. The companion `git rev-parse HEAD` returns the previous commit's
SHA. Net effect: an attacker (or careless developer) can:

```sh
git add malicious_patch.rs
cargo build --release   # binary records clean HEAD SHA, no `dirty-` prefix
git checkout malicious_patch.rs   # tree returns to clean
```

…and ship a binary whose `code_revision` lies about what it was built from. This is
the exact threat (T-01-04 / "code revision tampering / repudiation") the build script's
module-level docstring claims to mitigate.

`git diff --quiet HEAD` (with the `HEAD` argument) compares **worktree vs. HEAD**,
catching both staged and unstaged divergence. Equivalent alternatives are
`git status --porcelain` (then check empty) or running both `git diff --quiet` and
`git diff --quiet --cached`.

**Fix:**

```rust
// Compares worktree (staged + unstaged) to HEAD — closes the staged-changes gap.
let dirty = Command::new("git")
    .args(["diff", "--quiet", "HEAD"])
    .status()
    .map(|s| !s.success())
    .unwrap_or(false);
```

Also worth considering: include untracked files in the check (`git status --porcelain`
with non-empty output) — a tree with an untracked `secret.rs` that `lib.rs` `include!`s
would still register clean today.

Add a smoke test in `xtask` (e.g., `cargo xtask verify-clean-build`) that prints
`MINER_CODE_REVISION` and refuses to publish a release artifact unless the value lacks
the `dirty-` prefix; the test value alone won't fix the detection logic but will at
least surface unexpected `dirty-` markers in CI.

---

## Warnings

### WR-01: Env- and TOML-layer `output` paths cannot encode `OutputDest::File(...)`

**File:** `crates/miner-core/src/config/mod.rs:57-62`, `crates/miner-cli/src/cli.rs:43-83`

**Issue:**
`OutputDest` is an externally-tagged enum (no `#[serde(untagged)]` or `#[serde(rename_all=…)]`
trickery beyond `snake_case` for the tag itself). Its wire-form for the `File` variant is:

```json
{"file": "/tmp/out.jsonl"}
```

…NOT a bare string. The CLI's `Cli::overrides()` converts the user-supplied string
`/tmp/out.jsonl` into `OutputDest::File(PathBuf::from(s))` Rust-side, bypassing serde
on the way into `CliOverrides`. But the figment env layer and the TOML layer go through
serde:

- TOML `output = "/tmp/out.jsonl"` deserialises against `OutputDest`, fails with
  `Kind::UnknownVariant("/tmp/out.jsonl")` → `PreflightCode::InvalidConfig`.
- `MINER_OUTPUT=/tmp/out.jsonl` (with no `--output` flag and no clap `env` capture) would
  travel through figment's env layer to the same deserialiser, with the same failure.

In practice clap captures `MINER_OUTPUT` via `#[arg(env = "MINER_OUTPUT")]` first and
funnels it through `Cli::overrides()`, so users can still set `MINER_OUTPUT=/path`
*when invoking the CLI*. But the `figment::Jail` precedence tests in
`config_precedence.rs` (which bypass clap) cannot test `OutputDest::File` via the env
layer — every such test would have to write `MINER_OUTPUT=stdout`. Future MCP / HTTP
wrappers that don't go through clap would inherit this restriction.

This is also a docstring-vs-behaviour mismatch: `cli.rs:43-48` says "any other string is
treated as a file path", which is true via CLI but not via env when env is consumed by
figment directly.

**Fix:** Implement a custom `Deserialize` impl for `OutputDest` that accepts a bare
string AND the tagged form:

```rust
impl<'de> Deserialize<'de> for OutputDest {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Tagged(OutputDestTagged),    // {"stdout": null} / {"file": "..."}
            Bare(String),                // "stdout" or "/path"
        }
        match Raw::deserialize(d)? {
            Raw::Bare(s) if s.eq_ignore_ascii_case("stdout") => Ok(Self::Stdout),
            Raw::Bare(s) => Ok(Self::File(PathBuf::from(s))),
            Raw::Tagged(t) => Ok(t.into()),
        }
    }
}
```

…or document the limitation in `OutputDest`'s rustdoc and the README quickstart, and
add a test that asserts `MINER_OUTPUT=/path` fails with `InvalidConfig` when consumed
via figment env.

---

### WR-02: No crate declares a `version`; Cargo silently defaults `miner_version` to "0.0.0"

**File:** `Cargo.toml:30-34` and every `crates/*/Cargo.toml`

**Issue:**
The workspace `[workspace.package]` block declares `edition`, `rust-version`, and
`license`, but **not** `version`. No individual crate sets `version = "..."` or
`version.workspace = true`. Cargo accepts this (newer toolchains assume `0.0.0` for
unpublished workspace members) and `Cargo.lock` records:

```
name = "miner-cli"
version = "0.0.0"
```

The CLI then stamps this into every envelope's `RunStart.miner_version`
(`crates/miner-cli/src/main.rs:113`):

```rust
miner_version: env!("CARGO_PKG_VERSION").to_string(),
```

Result: every finding is reported as `"miner_version": "0.0.0"`, which:
- Defeats agent ability to skew-detect between miner releases (the Quant agent in the
  README architecture diagram is expected to pin against miner versions).
- Breaks the eventual `cargo publish` path if any wrapper crate is ever published.
- Makes the `code_revision` (git SHA) the **only** identity carried in the envelope —
  which is fine until someone rebuilds from a tagged release where `code_revision`
  may legitimately equal `"unknown"` (tarball install).

**Fix:** Add `version = "0.1.0"` (or similar) to `[workspace.package]` in the root
`Cargo.toml`, then in each member crate add `version.workspace = true` alongside
`edition.workspace = true`. Bump it deliberately on every Phase boundary so envelope
provenance is meaningful.

---

### WR-03: `blake3` declared on `miner-core` but never used in source

**File:** `crates/miner-core/Cargo.toml:34`, `Cargo.toml:49`

**Issue:**
`miner-core/Cargo.toml` lists `blake3.workspace = true`. A `grep -rn "blake3"` over
`crates/miner-core/src/` returns three doc-comment hits and one test fixture string
(`"blake3-deadbeef"`) — no production `use blake3` or `blake3::Hasher`. The Plan 03
docstring claims `param_hash` is "blake3 hash of resolved params (post-defaults)", but
no hashing helper is exposed. Wrappers / future readers cannot compute a param_hash
without re-introducing the dep.

This is benign for compile health (Cargo doesn't refuse unused deps), but:

- Adds compile-time cost on a deps-light Phase 1 build.
- Suggests an intended-but-missing helper. If the contract really is "param_hash is the
  blake3 hash of the resolved params JSON", then `miner-core` should ship the canonical
  helper that does the hash, otherwise wrappers will each invent their own (non-canonical)
  serialisation and the contract becomes ambiguous.

**Fix:** Either land the helper in this phase (e.g. a `pub fn param_hash(params: &serde_json::Value) -> String`
in `miner-core::findings` that BLAKE3-hashes the deterministic `serde_json::to_string`
form) — closes the Plan-03 ambition properly — or drop the `blake3` workspace dep from
`miner-core/Cargo.toml` until the helper actually lands in Phase 2/3.

---

### WR-04: Schema does not require `dsr` / `fdr_q`; OUT-02 contract is producer-only

**File:** `schemas/findings-v1.schema.json:279-298`, `crates/miner-core/src/findings/mod.rs:208-234`

**Issue:**
`ResultFinding` (and `ScanErrorFinding`, `GapAbortedFinding`) declare `dsr: Option<f64>`
and `fdr_q: Option<f64>` WITHOUT `#[serde(skip_serializing_if = "Option::is_none")]`,
so the Rust producer always emits `"dsr": null, "fdr_q": null`. But the generated
schema lists only 10 fields as `required` for `ResultFinding` (schema_version, scan_id@version,
param_hash, code_revision, data_slice, run_id, produced_at_utc, source, params, effect) —
`dsr` and `fdr_q` are absent from the required list.

Schema consumers that strictly validate "must contain key" will accept envelopes that
omit `dsr`/`fdr_q` entirely. The `dsr_and_fdr_q_present_as_null_in_v1` test in
`schema_roundtrip.rs:255-280` proves the **Rust producer** behaves correctly, but the
schema does not bind future producers (e.g., a Python re-implementation, or a wrapper
that maps to its own type) to the same contract.

OUT-02 is documented as "dsr/fdr_q MUST serialise as JSON `null` (NOT absent) in v1",
which is a wire contract; the schema is the canonical machine-readable form of that
contract.

**Fix:**
Either (a) add `dsr` and `fdr_q` to the `required` list in each of
`ResultFinding`, `ScanErrorFinding`, `GapAbortedFinding` in the schema (you'll need a
custom schemars `JsonSchema` impl or a `schemars` attribute to force-include
`Option`-typed fields into `required`), or (b) update OUT-02 + the rustdoc on each
struct to say "the v1 reference Rust producer always emits these as null; the schema
does not enforce presence." Document the rationale either way; current state has the
docstring contradict the artifact.

---

### WR-05: README quickstart and tests assume current dir without protection from leaked `./miner.toml`

**File:** `crates/miner-cli/src/cli.rs:111-126`, `crates/miner-cli/tests/cli_streams.rs:202-246`

**Issue:**
`resolve_toml_path` falls back to `./miner.toml` (CWD) as the third lookup. The CWD
fallback is documented and serial-tested under `tempfile::TempDir` for the
"missing config" path. But `cargo test --workspace` runs every test from the workspace
root, where a developer may have a local `./miner.toml` they keep for ad-hoc invocations.
Tests that use `run_emit_fixture_happy` (Tests 1–4, 7 in `cli_streams.rs`) do NOT
override `current_dir`, so the spawned `miner` binary inherits the workspace CWD.

If the developer keeps `./miner.toml` in the repo root, those tests pick it up,
silently overriding the test's `MINER_CACHE_ROOT=/tmp/cache` env vars and producing
non-reproducible test runs.

Mitigations in the current code: `env_clear()` removes everything, then re-sets the three
required `MINER_*` env vars. Per Plan 05, `MINER_CACHE_ROOT` (env layer) wins over the
TOML file. So precedence does protect the values — but anything in the TOML file that
ISN'T overridden by env (e.g., a future `MINER_LOG_LEVEL` field) would survive.

**Fix:** Add `current_dir(tmp.path())` to the happy-path test setup — the tests already
own a `tempfile::TempDir` plumbing convention for the failing-path tests; mirror it. Or
make `resolve_toml_path` skip the CWD fallback when invoked via a test marker env var.
Belt-and-braces fix: add an explicit `.gitignore` entry for `/miner.toml` so a developer
can't accidentally commit one.

---

### WR-06: `classify_figment_error` clones the entire error chain on every preflight failure

**File:** `crates/miner-cli/src/main.rs:77-100`

**Issue:**
```rust
let first_kind = err
    .clone()
    .into_iter()
    .next()
    .map_or(Kind::Message(String::new()), |e| e.kind);
```

`figment::Error` is a non-trivial structure (path vec, optional `prev` boxed chain,
tag, profile, metadata). `err.clone()` walks the whole chain. We then take only the
**first** element of the iterator and throw the clone away. This is functionally fine
(figment's `IntoIterator` for `Error` exists — though `for<'a> &'a Error: IntoIterator`
might also work and avoids the clone), but on a preflight error path where every byte
matters for diagnostic latency, the unnecessary clone is wasteful.

More subtly: the `.map_or(Kind::Message(String::new()), ...)` fallback for the empty
iterator case is dead code — figment's iterator always yields at least the head error.
Silently mapping a (theoretically impossible) empty iterator to `InvalidConfig` is fine,
but is invisible if it ever fires.

**Fix:**

```rust
fn classify_figment_error(err: &figment::Error) -> PreflightCode {
    use figment::error::Kind;
    // Use the by-ref iterator if figment supports it; otherwise document why
    // the clone is necessary.
    let first = err.into_iter().next().expect("figment::Error always yields one item");
    match first.kind {
        Kind::MissingField(_) => PreflightCode::MissingRequiredConfig,
        _ => PreflightCode::InvalidConfig,
    }
}
```

…or document the clone with an inline comment explaining figment's owning-iterator
shape. The exhaustive match for the non-`MissingField` arm is defensive against new
variants — fine to keep, but the wildcard form is shorter and only loses compile-time
detection of newly-added variants (and `figment::Error::Kind` is not `#[non_exhaustive]`).

---

### WR-07: `Base64Bytes::deserialize` accepts any base64 garbage without length / shape cross-check

**File:** `crates/miner-core/src/findings/base64_bytes.rs:37-45`

**Issue:**
`Base64Bytes::deserialize` accepts any valid base64 string. Within a `RawArray`, the
schema then carries a `shape: Vec<u64>` field alongside `data: Base64Bytes` and
`dtype: Dtype::F64`. There is no validation that `data.len() == shape.iter().product() * size_of::<f64>()`
on either the producer or consumer side. A scan could emit `shape: [1000, 1]` (8000
bytes expected) but `data` with only 16 bytes, and the envelope round-trips silently.

This will become a real footgun in Phase 2+ when actual scan kernels start emitting raw
arrays. Consumers that decode and `bytemuck::cast_slice::<u8, f64>` will hit
length-mismatch panics far from the producer's bug.

Phase 1 is when the envelope contract is locked, so this is the right moment to add a
constructor-level check.

**Fix:** Make `RawArray` use a `RawArray::new(...)` constructor that validates
`data.0.len() == 8 * shape.iter().product::<u64>() as usize` and refuses construction
otherwise. Document the invariant on the schema (`description` field on `RawArray.data`).
The current `Raw::new` already validates a different invariant (D-03 `timestamps_ms`);
extend the pattern.

---

## Info

### IN-01: Two `BufWriter`-flushing patterns coexist in the same module — drift risk

**File:** `crates/miner-core/src/findings/sink.rs:190-212`

**Issue:** The `StdoutSink` production impl (lines 89-102) and the test-only `WriterSink<W>`
impl (lines 194-212) duplicate the BufWriter + per-envelope-flush logic. The test
helper's docstring acknowledges this ("mirrors `StdoutSink` exactly") and exists because
we cannot test against the real stdout handle. If anyone changes the production
flush semantics (e.g., switches to non-per-envelope flushing), the test helper would
silently keep the old semantics and the flush regression gate would pass anyway.

**Fix:** Extract a single `write_finding_to<W: Write>(writer: &mut W, finding: &Finding) -> Result<(), MinerError>`
free function in `sink.rs` and have BOTH `StdoutSink::write_envelope` and
`WriterSink::write_envelope` delegate to it. Same outcome, one source of truth.

---

### IN-02: `unsafe_code = "forbid"` is the wrong granularity for build dependencies

**File:** `Cargo.toml:53-54`

**Issue:** The workspace lints set `unsafe_code = "forbid"`. This applies to every member
crate's source and prevents wholesale `#![allow(unsafe_code)]`. Good intent for product
code; mildly inconvenient for `xtask` (dev-only, never shipped) where future helpers
might legitimately want `mmap`-based scratch dirs or similar. The current code does not
need unsafe anywhere, but the comment in `tests/config_precedence.rs:15-20` already notes
the friction this caused for `figment::Jail` selection.

**Fix:** Consider scoping to `lib.rs` / `main.rs` of product crates only via per-crate
`[lints]` overrides if/when xtask needs an unsafe block. Not urgent — current code is
clean.

---

### IN-03: `MinerConfig::resolve` returns `figment::Error` directly, leaking impl into the public surface

**File:** `crates/miner-core/src/config/mod.rs:92`, `crates/miner-core/src/lib.rs:34`

**Issue:**
The error type for the canonical config-resolution entry point is `figment::Error`. This
makes `figment` part of the public API surface of `miner-core`: every downstream crate
calling `MinerConfig::resolve` needs `figment::error::Kind` to classify the failure
(see `miner-cli::classify_figment_error`). Replacing figment with `config` or a hand-rolled
provider in a future phase becomes a breaking change.

**Fix:** Introduce a `miner_core::config::ConfigError` enum with at least
`MissingField(String)` and `Invalid(String)` variants and a `From<figment::Error>`
impl that does the classification once, in the library. `miner-cli::main` then matches
on a typed error and the figment leak is contained.

---

### IN-04: `RunId::Default` produces fresh ULIDs — surprising for `Default`

**File:** `crates/miner-core/src/findings/run_id.rs:34-38`

**Issue:** `impl Default for RunId { fn default() -> Self { Self::new() } }` is convenient
but unusual — `Default` is normally pure (no side effects, deterministic). Two
`RunId::default()` calls produce different values. Documented expectation for `Default`
in the Rust API guidelines is "reasonable default value", typically zero. This isn't a
bug, but it can surprise anyone using `RunId::default()` in test scaffolding expecting
deterministic IDs.

**Fix:** Either remove the `Default` impl and force callers to use `RunId::new()`
explicitly, or document the non-determinism on the `Default` impl with a rustdoc note.

---

### IN-05: README quickstart Step 3 will create literal directories at the user's filesystem root

**File:** `README.md:42-46`

**Issue:**
```sh
MINER_CACHE_ROOT=/tmp/cache \
MINER_BAR_CACHE_ROOT=/tmp/bar \
MINER_OUTPUT=stdout \
cargo run -p miner-cli -- emit-fixture
```

These directories don't exist. `emit-fixture` doesn't actually open them in Phase 1
(consistent with the placeholder readers) so the command works, but anyone running the
quickstart on macOS or Linux who later adds path-existence validation will see surprising
failures. More importantly, agents reading the README and reproducing this pattern will
encode `/tmp/cache` as a real path.

**Fix:** Change the README example to use `$(pwd)/cache` and `$(pwd)/bar` with a `mkdir -p`
preamble, or document that v1 `emit-fixture` does not touch these paths (and mark them as
"valid placeholders until the reader lands in Phase 2").

---

### IN-06: `mask_volatile_fields` in `cli_streams.rs` recurses without depth limit and silently shadows nested fields

**File:** `crates/miner-cli/tests/cli_streams.rs:323-344`

**Issue:**
```rust
fn mask_volatile_fields(v: &mut serde_json::Value) {
    if let serde_json::Value::Object(map) = v {
        for key in ["run_id", "started_at_utc", "ended_at_utc"] {
            if map.contains_key(key) {
                map.insert(key.to_string(), serde_json::Value::String(format!("<masked_{key}>")));
            }
        }
        ...
        for (_, child) in map.iter_mut() {
            mask_volatile_fields(child);
        }
    } else if let serde_json::Value::Array(arr) = v {
        for child in arr.iter_mut() { mask_volatile_fields(child); }
    }
}
```

This recurses into every nested object/array and masks every occurrence of those keys.
For Phase 1's flat envelopes, fine. But if a future envelope nests a `run_id` inside a
`request_context` field (e.g., echoing a previous run's id for correlation), the mask
flattens both into the same sentinel and the byte-identity test passes even when the
inner `run_id` legitimately differs across runs. That's the *opposite* of what the
twice-run determinism test is checking.

Less critical: unbounded recursion on adversarial input (e.g., a thousand-deep nested
JSON object) could stack-overflow the test harness. Test-only code, so the threat
surface is small, but the recursion-depth pattern is the kind of thing that gets reused.

**Fix:** Either limit masking to the top-level object (which is what Phase 1 envelopes
actually need), or restrict it to a documented list of paths (`$.run_id`,
`$.started_at_utc`, etc.) using something like JSON-pointer-style traversal.

---

## Structural Findings (fallow)

No structural findings block was supplied with the review prompt; nothing to record here.

---

_Reviewed: 2026-05-17_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
