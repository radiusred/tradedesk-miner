//! Phase 5 sweep manifest — typed TOML deserialiser + preflight validator.
//!
//! Pattern analog: `crate::engine::preflight::resolve_scan_id_at_version` +
//! `crate::engine::preflight::validate_arity` — typed `Result<_, WireError>`
//! return; `WireError::preflight(PreflightCode::*, ...)` + `with_context(...)`
//! for context keys. The TOML deserialiser itself follows the
//! `crate::config` figment derive pattern (`serde::Deserialize` on every
//! sub-struct; `#[serde(default = "...")]` for the typed defaults).
//!
//! ## Decisions implemented (D5-01)
//!
//! - `[sweep]` (default `max_jobs = 100_000`) — `T-05-04-V5-SIZE`
//!   mitigation: cartesian expansion is rejected when
//!   `estimated_job_count > max_jobs` with
//!   `PreflightCode::SweepTooLarge`.
//! - `[hygiene]` global hygiene defaults (per-job `[[jobs].hygiene]` may
//!   override).
//! - `[fdr]` — `family = "scan_id"` default (scope: per-`scan_id@version`);
//!   `alpha = 0.05`.
//! - `[[jobs]]` — each block carries `scan`, `instruments`,
//!   `timeframes`, `windows`, optional `gap_policy`, optional `params`
//!   (`BTreeMap`), optional per-block `hygiene`.
//!
//! ## Threat model (T-05-04-V5-DEEP / T-05-04-V5-SIZE)
//!
//! `toml = "0.8"` enforces a default 256-level nesting limit (RESEARCH
//! §"Security Domain"). The cartesian-cardinality gate
//! (`estimated_job_count > [sweep].max_jobs`) prevents a manifest
//! declaring 10⁹ jobs from materialising the `Vec<ResolvedJob>`.
//!
//! ## `BTreeMap` discipline (Pitfall 6)
//!
//! `params: BTreeMap<String, serde_json::Value>` — NEVER `HashMap`. The
//! `serde_json::Map` produced by toml deserialisation is `BTreeMap`-backed
//! by default (workspace `serde_json` has no features list, per the
//! `Cargo.toml` note). The `BTreeMap` discipline preserves byte-stable
//! `param_hash` across TOML key-reorderings.

use std::collections::BTreeMap;
use std::path::Path;

use schemars::JsonSchema;
use serde::Deserialize;

use crate::error::{MinerError, PreflightCode, WireError};
use crate::scan::Registry;

// ---------------------------------------------------------------------------
// SweepManifest — root deserialiser
// ---------------------------------------------------------------------------

/// Root of the sweep manifest deserialiser tree.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct SweepManifest {
    #[serde(default)]
    pub sweep: SweepConfig,
    #[serde(default)]
    pub hygiene: HygieneBlock,
    #[serde(default)]
    pub fdr: FdrConfig,
    #[serde(default, rename = "jobs")]
    pub jobs: Vec<JobBlock>,
}

/// `[sweep]` block — sweep-level seed + cardinality ceiling.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SweepConfig {
    pub seed: Option<u64>,
    #[serde(default = "default_max_jobs")]
    pub max_jobs: u64,
}

impl Default for SweepConfig {
    fn default() -> Self {
        // `Default::default()` is reached when the `[sweep]` table is OMITTED
        // from the manifest entirely. The serde `#[serde(default = "...")]`
        // attributes only fire when the FIELD is missing inside a present
        // table; the table itself relies on this Default to seed the
        // typed-default values.
        Self {
            seed: None,
            max_jobs: default_max_jobs(),
        }
    }
}

#[must_use]
fn default_max_jobs() -> u64 {
    100_000
}

/// `[hygiene]` (and per-job `[[jobs].hygiene]`) block — global hygiene defaults.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct HygieneBlock {
    pub bootstrap: Option<String>,
    #[serde(default)]
    pub bootstrap_n: u32,
    pub null: Option<String>,
    #[serde(default)]
    pub null_n: u32,
}

/// `[fdr]` block — FDR scope + alpha.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct FdrConfig {
    #[serde(default = "default_fdr_family")]
    pub family: String,
    #[serde(default = "default_alpha")]
    pub alpha: f64,
}

impl Default for FdrConfig {
    fn default() -> Self {
        Self {
            family: default_fdr_family(),
            alpha: default_alpha(),
        }
    }
}

#[must_use]
fn default_fdr_family() -> String {
    "scan_id".to_string()
}

#[must_use]
fn default_alpha() -> f64 {
    0.05
}

/// One `[[jobs]]` block — single scan id, multi-instrument / multi-tf /
/// multi-window / multi-param-grid axis declaration.
///
/// `instruments` is `serde_json::Value` (NOT a typed `Vec<String>` or
/// `Vec<Vec<String>>`) because Single-arity scans take a flat
/// `Vec<String>` whereas Pair-arity scans take a nested
/// `Vec<Vec<String>>` (D5-01 arity split). The shape is dispatched per-
/// block in `job_graph::parse_instruments_grid` against the registry-
/// resolved `Scan::arity()`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct JobBlock {
    pub scan: String,
    pub instruments: serde_json::Value,
    pub timeframes: Vec<String>,
    pub windows: Vec<String>,
    #[serde(default)]
    pub gap_policy: Option<String>,
    #[serde(default)]
    pub params: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    pub hygiene: Option<HygieneBlock>,
}

// ---------------------------------------------------------------------------
// read_manifest — file-path entry point
// ---------------------------------------------------------------------------

/// Read a TOML manifest from disk into a typed [`SweepManifest`].
///
/// # Errors
/// - `MinerError::Io` — file does not exist / cannot be read.
/// - `MinerError::Preflight(WireError::preflight(InvalidParameter, ...))` —
///   the bytes do not parse as a valid `SweepManifest` (TOML syntax error
///   OR schema mismatch).
pub fn read_manifest(path: &Path) -> Result<SweepManifest, MinerError> {
    let s = std::fs::read_to_string(path).map_err(MinerError::Io)?;
    parse_manifest_str(&s)
}

/// Parse a TOML manifest from a string. Helper for tests + the
/// CLI-piped-stdin path (deferred to Plan 05-05).
///
/// # Errors
/// - `MinerError::Preflight(WireError::preflight(InvalidParameter, ...))` —
///   TOML parse error.
pub fn parse_manifest_str(s: &str) -> Result<SweepManifest, MinerError> {
    toml::from_str::<SweepManifest>(s).map_err(|e| {
        MinerError::Preflight(WireError::preflight(
            PreflightCode::InvalidParameter,
            format!("TOML parse error: {e}"),
        ))
    })
}

// ---------------------------------------------------------------------------
// validate — preflight + cardinality + arity + hygiene-support gates
// ---------------------------------------------------------------------------

/// Preflight-validate a manifest. Returns the estimated job count when
/// the manifest is acceptable; returns a typed `MinerError::Preflight`
/// on rejection.
///
/// Validation walks per-`[[jobs]]` block:
/// 1. Scan id resolves in the registry (else
///    [`PreflightCode::UnknownScan`]).
/// 2. `instruments` shape matches the scan's arity (else
///    [`PreflightCode::InvalidParameter`] — D5-01 arity mismatch).
/// 3. Per-block hygiene support (else
///    [`PreflightCode::HygieneNotSupported`]).
/// 4. Total estimated job count ≤ `[sweep].max_jobs` (else
///    [`PreflightCode::SweepTooLarge`] — `T-05-04-V5-SIZE`).
///
/// # Errors
/// See [`PreflightCode`] variants above.
#[allow(
    clippy::too_many_lines,
    reason = "linear preflight walk: cardinality gate -> per-block (resolve scan -> arity check -> hygiene-support check -> hygiene-method parse) -> tally. Splitting per-step would obscure the documented validation ordering."
)]
pub fn validate(manifest: &SweepManifest, registry: &Registry) -> Result<u64, MinerError> {
    // WR-03: HYGIENE_RESAMPLE_CEILING enforced at preflight (defence-in-
    // depth — the engine also clamps at the call site, but rejecting
    // here surfaces the constraint explicitly).
    const CEILING: u64 = crate::engine::HYGIENE_RESAMPLE_CEILING as u64;

    // WR-03: validate [fdr].alpha before any other check. NaN must be
    // rejected explicitly because the `(0.0..=1.0).contains(&NaN)` check
    // returns false on its own, but a `debug_assert!` inside `bh_fdr` only
    // fires under `cfg(debug_assertions)`; in release builds a NaN alpha
    // would silently ride into FdrFamilySummary.alpha on the wire.
    if !(0.0..=1.0).contains(&manifest.fdr.alpha) || manifest.fdr.alpha.is_nan() {
        return Err(MinerError::Preflight(
            WireError::preflight(
                PreflightCode::InvalidParameter,
                format!("[fdr].alpha must be in [0, 1]; got {}", manifest.fdr.alpha),
            )
            .with_context(
                "fdr_alpha",
                serde_json::Number::from_f64(manifest.fdr.alpha)
                    .map_or(serde_json::Value::Null, serde_json::Value::Number),
            ),
        ));
    }

    // WR-03: validate [hygiene].bootstrap_n / null_n against the documented
    // HYGIENE_RESAMPLE_CEILING. The engine clamps over-cap counts at the
    // call site, but rejecting at preflight surfaces the constraint
    // explicitly rather than letting the user think they got 1M
    // resamples when they actually got 100k.
    if u64::from(manifest.hygiene.bootstrap_n) > CEILING {
        return Err(MinerError::Preflight(
            WireError::preflight(
                PreflightCode::InvalidParameter,
                format!(
                    "[hygiene].bootstrap_n exceeds HYGIENE_RESAMPLE_CEILING ({CEILING}); got {}",
                    manifest.hygiene.bootstrap_n
                ),
            )
            .with_context(
                "bootstrap_n",
                serde_json::Value::Number(serde_json::Number::from(manifest.hygiene.bootstrap_n)),
            )
            .with_context(
                "ceiling",
                serde_json::Value::Number(serde_json::Number::from(CEILING)),
            ),
        ));
    }
    if u64::from(manifest.hygiene.null_n) > CEILING {
        return Err(MinerError::Preflight(
            WireError::preflight(
                PreflightCode::InvalidParameter,
                format!(
                    "[hygiene].null_n exceeds HYGIENE_RESAMPLE_CEILING ({CEILING}); got {}",
                    manifest.hygiene.null_n
                ),
            )
            .with_context(
                "null_n",
                serde_json::Value::Number(serde_json::Number::from(manifest.hygiene.null_n)),
            )
            .with_context(
                "ceiling",
                serde_json::Value::Number(serde_json::Number::from(CEILING)),
            ),
        ));
    }

    // Tally the estimated cardinality first (cheap; no Vec materialisation)
    // so the SweepTooLarge gate fires BEFORE we walk per-block validation.
    let estimated = crate::sweep::job_graph::estimated_job_count(manifest);
    if estimated > manifest.sweep.max_jobs {
        return Err(MinerError::Preflight(
            WireError::preflight(
                PreflightCode::SweepTooLarge,
                format!(
                    "sweep would expand to {estimated} jobs; exceeds [sweep].max_jobs = {max}",
                    max = manifest.sweep.max_jobs,
                ),
            )
            .with_context(
                "estimated_job_count",
                serde_json::Value::Number(serde_json::Number::from(estimated)),
            )
            .with_context(
                "max_jobs",
                serde_json::Value::Number(serde_json::Number::from(manifest.sweep.max_jobs)),
            ),
        ));
    }

    // Per-block validation.
    for (block_idx, block) in manifest.jobs.iter().enumerate() {
        let scan = crate::engine::preflight::resolve_scan(&block.scan, registry)
            .map_err(MinerError::Preflight)?;

        // Arity check: instruments shape vs scan.arity().
        let arity = scan.arity();
        crate::sweep::job_graph::validate_instruments_shape(&block.instruments, arity).map_err(
            |msg| {
                MinerError::Preflight(
                    WireError::preflight(
                        PreflightCode::InvalidParameter,
                        format!("[[jobs[{block_idx}]]] arity mismatch: {msg}"),
                    )
                    .with_context(
                        "block_index",
                        serde_json::Value::Number(serde_json::Number::from(block_idx)),
                    )
                    .with_context("scan_id", serde_json::Value::String(block.scan.clone())),
                )
            },
        )?;

        // Hygiene support — merge per-block hygiene over the global hygiene
        // block (per-block wins when both set).
        let merged_hygiene = merge_hygiene(&manifest.hygiene, block.hygiene.as_ref());

        // WR-03 (per-block): the merged per-block bootstrap_n / null_n
        // must also respect HYGIENE_RESAMPLE_CEILING; per-block overrides
        // can sneak past the global check above.
        if u64::from(merged_hygiene.bootstrap_n) > CEILING {
            return Err(MinerError::Preflight(
                WireError::preflight(
                    PreflightCode::InvalidParameter,
                    format!(
                        "[[jobs[{block_idx}].hygiene]].bootstrap_n exceeds HYGIENE_RESAMPLE_CEILING ({CEILING}); got {}",
                        merged_hygiene.bootstrap_n
                    ),
                )
                .with_context(
                    "block_index",
                    serde_json::Value::Number(serde_json::Number::from(block_idx)),
                ),
            ));
        }
        if u64::from(merged_hygiene.null_n) > CEILING {
            return Err(MinerError::Preflight(
                WireError::preflight(
                    PreflightCode::InvalidParameter,
                    format!(
                        "[[jobs[{block_idx}].hygiene]].null_n exceeds HYGIENE_RESAMPLE_CEILING ({CEILING}); got {}",
                        merged_hygiene.null_n
                    ),
                )
                .with_context(
                    "block_index",
                    serde_json::Value::Number(serde_json::Number::from(block_idx)),
                ),
            ));
        }
        if let Some(bootstrap_str) = merged_hygiene.bootstrap.as_deref() {
            let bm = parse_bootstrap_method(bootstrap_str).map_err(|msg| {
                MinerError::Preflight(WireError::preflight(
                    PreflightCode::InvalidParameter,
                    format!("[[jobs[{block_idx}]]] invalid bootstrap method: {msg}"),
                ))
            })?;
            if !scan.supports_bootstrap() {
                return Err(MinerError::Preflight(
                    WireError::preflight(
                        PreflightCode::HygieneNotSupported,
                        format!(
                            "{} does not support bootstrap method {:?}",
                            scan.id(),
                            bm.as_str()
                        ),
                    )
                    .with_context("scan_id", serde_json::Value::String(scan.id().to_string()))
                    .with_context(
                        "requested_method",
                        serde_json::Value::String("bootstrap".to_string()),
                    )
                    .with_context("method", serde_json::Value::String(bm.as_str().to_string())),
                ));
            }
        }
        if let Some(null_str) = merged_hygiene.null.as_deref() {
            let nm = parse_null_method(null_str).map_err(|msg| {
                MinerError::Preflight(WireError::preflight(
                    PreflightCode::InvalidParameter,
                    format!("[[jobs[{block_idx}]]] invalid null method: {msg}"),
                ))
            })?;
            if !scan.supports_null_method(nm) {
                return Err(MinerError::Preflight(
                    WireError::preflight(
                        PreflightCode::HygieneNotSupported,
                        format!(
                            "{} does not support null method {:?}",
                            scan.id(),
                            nm.as_str()
                        ),
                    )
                    .with_context("scan_id", serde_json::Value::String(scan.id().to_string()))
                    .with_context(
                        "requested_method",
                        serde_json::Value::String("null".to_string()),
                    )
                    .with_context("method", serde_json::Value::String(nm.as_str().to_string())),
                ));
            }
        }
    }

    Ok(estimated)
}

// ---------------------------------------------------------------------------
// Hygiene parsing helpers — shared with executor / job_graph
// ---------------------------------------------------------------------------

/// Merge the global `[hygiene]` defaults with an optional per-block
/// `[[jobs].hygiene]` override. Per-block wins when both set; otherwise
/// the global is the source of truth. Numeric `_n` fields prefer the
/// per-block value when non-zero.
#[must_use]
pub fn merge_hygiene(global: &HygieneBlock, per_block: Option<&HygieneBlock>) -> HygieneBlock {
    let Some(pb) = per_block else {
        return global.clone();
    };
    HygieneBlock {
        bootstrap: pb.bootstrap.clone().or_else(|| global.bootstrap.clone()),
        bootstrap_n: if pb.bootstrap_n > 0 {
            pb.bootstrap_n
        } else {
            global.bootstrap_n
        },
        null: pb.null.clone().or_else(|| global.null.clone()),
        null_n: if pb.null_n > 0 {
            pb.null_n
        } else {
            global.null_n
        },
    }
}

/// Parse a wire-form bootstrap method string into the typed enum.
///
/// # Errors
/// Returns a static-ish String error message when the input is not one
/// of `"stationary"` / `"block"`.
pub fn parse_bootstrap_method(s: &str) -> Result<crate::scan::BootstrapMethod, String> {
    match s {
        "stationary" => Ok(crate::scan::BootstrapMethod::Stationary),
        "block" => Ok(crate::scan::BootstrapMethod::Block),
        other => Err(format!(
            "unknown bootstrap method {other:?}; expected \"stationary\" or \"block\""
        )),
    }
}

/// Parse a wire-form null method string into the typed enum.
///
/// # Errors
/// Returns a static-ish String error message when the input is not one
/// of `"circular_shift"` / `"phase_scramble"`.
pub fn parse_null_method(s: &str) -> Result<crate::scan::NullMethod, String> {
    match s {
        "circular_shift" => Ok(crate::scan::NullMethod::CircularShift),
        "phase_scramble" => Ok(crate::scan::NullMethod::PhaseScramble),
        other => Err(format!(
            "unknown null method {other:?}; expected \"circular_shift\" or \"phase_scramble\""
        )),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(
    clippy::doc_markdown,
    reason = "test names + plan tags in doc-comments are descriptive prose, not code identifiers; backticking each one would be noise"
)]
mod tests {
    use super::*;
    use crate::scan::bootstrap as registry_bootstrap;

    /// Test 1 (Plan 05-04 Task 1): minimal manifest parses with default
    /// `[sweep].max_jobs = 100_000`, `[fdr].family = "scan_id"`,
    /// `[fdr].alpha = 0.05`.
    #[test]
    fn manifest_parse_minimal_uses_typed_defaults() {
        let toml = r#"
            [[jobs]]
            scan = "stats.autocorr.ljung_box@1"
            instruments = ["EURUSD:bid"]
            timeframes = ["15m"]
            windows = ["2024-01-01:2024-01-02"]
        "#;
        let m: SweepManifest = toml::from_str(toml).expect("parse");
        assert_eq!(m.sweep.max_jobs, 100_000, "default max_jobs");
        assert_eq!(m.fdr.family, "scan_id", "default fdr family");
        assert!((m.fdr.alpha - 0.05).abs() < 1e-12, "default alpha");
        assert_eq!(m.jobs.len(), 1);
    }

    /// Test 2 (Plan 05-04 Task 1): full fixture with `[sweep]`,
    /// `[hygiene]`, `[fdr]`, and two `[[jobs]]` blocks — one Single-arity
    /// (flat `instruments`) and one Pair-arity (nested `instruments`).
    #[test]
    fn manifest_parse_full_fixture_two_jobs_mixed_arity() {
        let toml = r#"
            [sweep]
            seed = 57005
            max_jobs = 1000

            [hygiene]
            bootstrap = "stationary"
            bootstrap_n = 50
            null = "circular_shift"
            null_n = 50

            [fdr]
            family = "scan_family"
            alpha = 0.10

            [[jobs]]
            scan = "stats.autocorr.ljung_box@1"
            instruments = ["EURUSD:bid", "GBPUSD:bid"]
            timeframes = ["15m", "1h"]
            windows = ["2024-01-01:2024-06-30"]
            params = { lags = [5, 10] }

            [[jobs]]
            scan = "cross.corr.pearson_rolling@1"
            instruments = [["EURUSD:bid", "GBPUSD:bid"]]
            timeframes = ["1h"]
            windows = ["2024-01-01:2024-06-30"]
            params = { window = [60] }
        "#;
        let m: SweepManifest = toml::from_str(toml).expect("parse");
        assert_eq!(m.sweep.seed, Some(57005));
        assert_eq!(m.sweep.max_jobs, 1000);
        assert_eq!(m.hygiene.bootstrap.as_deref(), Some("stationary"));
        assert_eq!(m.hygiene.bootstrap_n, 50);
        assert_eq!(m.fdr.family, "scan_family");
        assert!((m.fdr.alpha - 0.10).abs() < 1e-12);
        assert_eq!(m.jobs.len(), 2);

        // Single-arity block: instruments parses as a flat array of strings.
        let single = &m.jobs[0];
        let arr = single.instruments.as_array().expect("flat array");
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0], serde_json::Value::String("EURUSD:bid".into()));

        // Pair-arity block: instruments parses as a nested 2-array.
        let pair = &m.jobs[1];
        let outer = pair.instruments.as_array().expect("nested outer array");
        assert_eq!(outer.len(), 1, "one pair declared");
        let inner = outer[0].as_array().expect("inner pair array");
        assert_eq!(inner.len(), 2, "pair has length 2");
    }

    /// Test 3 (Plan 05-04 Task 1): `[sweep] max_jobs = 4` with a
    /// manifest expanding to 8 jobs is rejected with
    /// `PreflightCode::SweepTooLarge`.
    #[test]
    fn validate_rejects_too_large_sweep() {
        let toml = r#"
            [sweep]
            max_jobs = 4

            [[jobs]]
            scan = "stats.autocorr.ljung_box@1"
            instruments = ["EURUSD:bid", "GBPUSD:bid"]
            timeframes = ["15m", "1h"]
            windows = ["2024-01-01:2024-01-02"]
            params = { lags = [5, 10] }
        "#;
        let m: SweepManifest = toml::from_str(toml).expect("parse");
        let reg = registry_bootstrap();
        let err = validate(&m, &reg).expect_err("must reject");
        let MinerError::Preflight(w) = err else {
            panic!("expected Preflight error; got {err:?}");
        };
        assert_eq!(w.code, "sweep_too_large");
        // Context: estimated == 8, max_jobs == 4.
        assert_eq!(
            w.context.get("estimated_job_count"),
            Some(&serde_json::Value::Number(serde_json::Number::from(8u64)))
        );
        assert_eq!(
            w.context.get("max_jobs"),
            Some(&serde_json::Value::Number(serde_json::Number::from(4u64)))
        );
    }

    /// Test 4 (Plan 05-04 Task 1): Single-arity scan with nested
    /// instruments is rejected with `PreflightCode::InvalidParameter`.
    #[test]
    fn validate_rejects_nested_instruments_for_single_arity() {
        let toml = r#"
            [[jobs]]
            scan = "stats.autocorr.ljung_box@1"
            instruments = [["EURUSD:bid", "GBPUSD:bid"]]
            timeframes = ["15m"]
            windows = ["2024-01-01:2024-01-02"]
        "#;
        let m: SweepManifest = toml::from_str(toml).expect("parse");
        let reg = registry_bootstrap();
        let err = validate(&m, &reg).expect_err("must reject");
        let MinerError::Preflight(w) = err else {
            panic!("expected Preflight error; got {err:?}");
        };
        assert_eq!(w.code, "invalid_parameter");
        assert!(
            w.message.contains("arity mismatch") || w.message.contains("Single"),
            "message must mention arity mismatch; got {:?}",
            w.message
        );
    }

    /// Test 4b (Plan 05-04 Task 1): Pair-arity scan with flat
    /// instruments is also rejected with
    /// `PreflightCode::InvalidParameter`.
    #[test]
    fn validate_rejects_flat_instruments_for_pair_arity() {
        let toml = r#"
            [[jobs]]
            scan = "cross.corr.pearson_rolling@1"
            instruments = ["EURUSD:bid", "GBPUSD:bid"]
            timeframes = ["15m"]
            windows = ["2024-01-01:2024-01-02"]
        "#;
        let m: SweepManifest = toml::from_str(toml).expect("parse");
        let reg = registry_bootstrap();
        let err = validate(&m, &reg).expect_err("must reject");
        let MinerError::Preflight(w) = err else {
            panic!("expected Preflight error; got {err:?}");
        };
        assert_eq!(w.code, "invalid_parameter");
        assert!(
            w.message.contains("arity mismatch") || w.message.contains("Pair"),
            "message must mention arity mismatch; got {:?}",
            w.message
        );
    }

    /// Test (Plan 05-04 Task 1): merge_hygiene per-block override semantics.
    #[test]
    fn merge_hygiene_per_block_overrides_global() {
        let global = HygieneBlock {
            bootstrap: Some("stationary".to_string()),
            bootstrap_n: 100,
            null: Some("circular_shift".to_string()),
            null_n: 100,
        };
        let per_block = HygieneBlock {
            bootstrap: Some("block".to_string()),
            bootstrap_n: 0, // 0 → inherits global 100
            null: None,     // inherits global
            null_n: 200,    // overrides global
        };
        let merged = merge_hygiene(&global, Some(&per_block));
        assert_eq!(merged.bootstrap.as_deref(), Some("block"));
        assert_eq!(merged.bootstrap_n, 100);
        assert_eq!(merged.null.as_deref(), Some("circular_shift"));
        assert_eq!(merged.null_n, 200);
    }

    /// Test (Plan 05-04 Task 1): merge_hygiene with no per-block returns
    /// the global clone.
    #[test]
    fn merge_hygiene_with_none_returns_global_clone() {
        let global = HygieneBlock {
            bootstrap: Some("stationary".to_string()),
            bootstrap_n: 100,
            null: None,
            null_n: 0,
        };
        let merged = merge_hygiene(&global, None);
        assert_eq!(merged.bootstrap.as_deref(), Some("stationary"));
        assert_eq!(merged.bootstrap_n, 100);
    }

    /// Test (Plan 05-04 Task 1): parse_bootstrap_method round-trip + error.
    #[test]
    fn parse_bootstrap_method_round_trip() {
        assert!(matches!(
            parse_bootstrap_method("stationary"),
            Ok(crate::scan::BootstrapMethod::Stationary)
        ));
        assert!(matches!(
            parse_bootstrap_method("block"),
            Ok(crate::scan::BootstrapMethod::Block)
        ));
        assert!(parse_bootstrap_method("nope").is_err());
    }

    /// Test (Plan 05-04 Task 1): parse_null_method round-trip + error.
    #[test]
    fn parse_null_method_round_trip() {
        assert!(matches!(
            parse_null_method("circular_shift"),
            Ok(crate::scan::NullMethod::CircularShift)
        ));
        assert!(matches!(
            parse_null_method("phase_scramble"),
            Ok(crate::scan::NullMethod::PhaseScramble)
        ));
        assert!(parse_null_method("none").is_err());
    }

    /// WR-03: out-of-range [fdr].alpha is rejected at preflight with
    /// `PreflightCode::InvalidParameter`.
    #[test]
    fn validate_rejects_out_of_range_fdr_alpha() {
        for bad in [-0.5_f64, 1.5, 100.0, -1.0, f64::NAN] {
            let toml = format!(
                r#"
                    [fdr]
                    family = "scan_id"
                    alpha = {bad}

                    [[jobs]]
                    scan = "stats.autocorr.ljung_box@1"
                    instruments = ["EURUSD:bid"]
                    timeframes = ["15m"]
                    windows = ["2024-01-01:2024-01-02"]
                "#,
                bad = if bad.is_nan() {
                    "nan".to_string()
                } else {
                    format!("{bad}")
                },
            );
            // NaN is the awkward case: TOML doesn't have a NaN literal.
            // Skip the NaN literal path; the runtime NaN check is
            // exercised by the constructed manifest below.
            if !bad.is_nan() {
                let m: SweepManifest = toml::from_str(&toml).expect("parse");
                let reg = registry_bootstrap();
                let err = validate(&m, &reg).expect_err("must reject");
                let MinerError::Preflight(w) = err else {
                    panic!("expected Preflight; got {err:?}");
                };
                assert_eq!(w.code, "invalid_parameter");
                assert!(
                    w.message.contains("[fdr].alpha"),
                    "message must mention [fdr].alpha; got {:?}",
                    w.message
                );
            }
        }

        // NaN alpha via direct struct construction (TOML doesn't parse NaN).
        let mut m = SweepManifest::default();
        m.fdr.alpha = f64::NAN;
        m.jobs.push(JobBlock {
            scan: "stats.autocorr.ljung_box@1".to_string(),
            instruments: serde_json::json!(["EURUSD:bid"]),
            timeframes: vec!["15m".to_string()],
            windows: vec!["2024-01-01:2024-01-02".to_string()],
            gap_policy: None,
            params: BTreeMap::new(),
            hygiene: None,
        });
        let reg = registry_bootstrap();
        let err = validate(&m, &reg).expect_err("must reject NaN alpha");
        let MinerError::Preflight(w) = err else {
            panic!("expected Preflight; got {err:?}");
        };
        assert_eq!(w.code, "invalid_parameter");
        assert!(w.message.contains("[fdr].alpha"));
    }

    /// WR-03: `[hygiene].bootstrap_n` over `HYGIENE_RESAMPLE_CEILING` is
    /// rejected at preflight rather than silently clamped at the engine.
    #[test]
    fn validate_rejects_over_ceiling_bootstrap_n() {
        let toml = r#"
            [hygiene]
            bootstrap = "stationary"
            bootstrap_n = 1000000

            [[jobs]]
            scan = "stats.autocorr.ljung_box@1"
            instruments = ["EURUSD:bid"]
            timeframes = ["15m"]
            windows = ["2024-01-01:2024-01-02"]
        "#;
        let m: SweepManifest = toml::from_str(toml).expect("parse");
        let reg = registry_bootstrap();
        let err = validate(&m, &reg).expect_err("must reject");
        let MinerError::Preflight(w) = err else {
            panic!("expected Preflight; got {err:?}");
        };
        assert_eq!(w.code, "invalid_parameter");
        assert!(
            w.message.contains("bootstrap_n") && w.message.contains("HYGIENE_RESAMPLE_CEILING"),
            "message must mention bootstrap_n + ceiling; got {:?}",
            w.message
        );
    }

    /// WR-03: same for per-block `[[jobs].hygiene].null_n` — over-cap
    /// values are caught by the per-block merged check.
    #[test]
    fn validate_rejects_over_ceiling_per_block_null_n() {
        let toml = r#"
            [[jobs]]
            scan = "stats.autocorr.ljung_box@1"
            instruments = ["EURUSD:bid"]
            timeframes = ["15m"]
            windows = ["2024-01-01:2024-01-02"]

            [[jobs.hygiene]]
            "#;
        // Per-block hygiene must be `[jobs.hygiene]` (table), not an array.
        // Use direct struct construction for clarity.
        let _ = toml; // silence the placeholder above; see direct
        // construction below.
        let mut m = SweepManifest::default();
        m.jobs.push(JobBlock {
            scan: "stats.autocorr.ljung_box@1".to_string(),
            instruments: serde_json::json!(["EURUSD:bid"]),
            timeframes: vec!["15m".to_string()],
            windows: vec!["2024-01-01:2024-01-02".to_string()],
            gap_policy: None,
            params: BTreeMap::new(),
            hygiene: Some(HygieneBlock {
                bootstrap: None,
                bootstrap_n: 0,
                null: Some("circular_shift".to_string()),
                null_n: 1_000_000,
            }),
        });
        let reg = registry_bootstrap();
        let err = validate(&m, &reg).expect_err("must reject");
        let MinerError::Preflight(w) = err else {
            panic!("expected Preflight; got {err:?}");
        };
        assert_eq!(w.code, "invalid_parameter");
        assert!(
            w.message.contains("null_n") && w.message.contains("HYGIENE_RESAMPLE_CEILING"),
            "message must mention null_n + ceiling; got {:?}",
            w.message
        );
    }
}
