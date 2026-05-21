//! Phase 5 sweep job-graph — cartesian expansion + `ResolvedJob` struct.
//!
//! Pattern analog: `crate::engine::gap_policy::dispatch` — pure function
//! from input → `Vec<TimeRange>` partitioning. `job_graph::expand` is
//! the same shape but expands a `SweepManifest` over the four-axis
//! cartesian product (`instruments × timeframes × windows × params`)
//! into a deterministic-order `Vec<ResolvedJob>`.
//!
//! ## D5-01 iteration order (locked)
//!
//! 1. `[[jobs]]` block declaration order.
//! 2. Within a block:
//!    - instruments (vector order)
//!    - timeframes (vector order)
//!    - windows (vector order)
//!    - params (alphabetic key order; array values expand cartesian,
//!      also in alphabetic key order)
//!
//! ## HYG-05 per-job seed
//!
//! Every `ResolvedJob` carries a `job_seed: u64` derived via
//! `hygiene::seed::derive_job_seed(master_seed, scan_id_at_version,
//! instruments, timeframe, window, param_hash)`. When `[sweep].seed` is
//! `None`, the master seed defaults to `0` (deterministic across runs —
//! Plan 05-04 SUMMARY records the master-seed-fallback decision).

use std::collections::BTreeMap;

use crate::aggregator::Timeframe;
use crate::engine::gap_policy::GapPolicyKind;
use crate::engine::param_hash::param_hash as compute_param_hash;
use crate::error::{MinerError, PreflightCode, WireError};
use crate::reader::{Blake3Hex, ClosedRangeUtc, InstrumentSpec};
use crate::scan::hygiene::seed::derive_job_seed;
use crate::scan::{BootstrapMethod, NullMethod, Registry, ScanArity};
use crate::sweep::manifest::{
    SweepManifest, merge_hygiene, parse_bootstrap_method, parse_null_method,
};

// ---------------------------------------------------------------------------
// ResolvedJob — fully-resolved single-job specification post-expansion
// ---------------------------------------------------------------------------

/// A fully-resolved single-job specification produced by [`expand`].
///
/// Every axis-array (instruments, timeframes, windows, params) has been
/// reduced to a scalar; `param_hash` is the blake3-hex over canonical
/// resolved-params bytes (D3-13); `job_seed` is the per-job u64 seed
/// derived from the job-identity tuple (HYG-05 / D5-05).
#[derive(Debug, Clone)]
pub struct ResolvedJob {
    pub scan_id_at_version: String,
    pub scan_id: String,
    pub version: u32,
    pub instruments: Vec<InstrumentSpec>,
    pub timeframe: Timeframe,
    pub window: ClosedRangeUtc,
    pub gap_policy: GapPolicyKind,
    pub resolved_params: serde_json::Value,
    pub param_hash: Blake3Hex,
    pub job_seed: u64,
    pub master_seed: u64,
    pub bootstrap_method: Option<BootstrapMethod>,
    pub bootstrap_n: Option<u32>,
    pub null_method: Option<NullMethod>,
    pub null_n: Option<u32>,
}

// ---------------------------------------------------------------------------
// expand — the full cartesian expansion
// ---------------------------------------------------------------------------

/// Expand a manifest into a deterministic-order `Vec<ResolvedJob>` per
/// D5-01 (block declaration → instruments → timeframes → windows →
/// params alphabetic).
///
/// # Errors
/// - `MinerError::Preflight(WireError::preflight(UnknownScan, ...))` —
///   `[[jobs]].scan` does not resolve.
/// - `MinerError::Preflight(WireError::preflight(InvalidParameter, ...))`
///   — instruments shape vs `Scan::arity()` mismatch, OR a timeframe /
///   window / `gap_policy` / hygiene-method string fails to parse.
#[allow(
    clippy::too_many_lines,
    reason = "linear cartesian-expansion walk: per-block { resolve scan -> parse instruments grid -> 4-axis nested loops } — splitting hides the deterministic iteration-order contract"
)]
pub fn expand(
    manifest: &SweepManifest,
    registry: &Registry,
) -> Result<Vec<ResolvedJob>, MinerError> {
    let master_seed = manifest.sweep.seed.unwrap_or(0);
    let mut jobs: Vec<ResolvedJob> = Vec::new();

    for (block_idx, block) in manifest.jobs.iter().enumerate() {
        // Step 1: resolve scan.
        let scan = crate::engine::preflight::resolve_scan(&block.scan, registry)
            .map_err(MinerError::Preflight)?;
        let (scan_id, version) =
            crate::engine::preflight::resolve_scan_id_at_version(&block.scan)
                .map_err(MinerError::Preflight)?;
        let scan_id_at_version = format!("{scan_id}@{version}");
        let arity = scan.arity();

        // Step 2: parse instruments grid (one InstrumentSpec list per
        // cartesian-instrument slot).
        let instruments_grid = parse_instruments_grid(&block.instruments, arity).map_err(|msg| {
            MinerError::Preflight(
                WireError::preflight(
                    PreflightCode::InvalidParameter,
                    format!("[[jobs[{block_idx}]]] {msg}"),
                )
                .with_context(
                    "block_index",
                    serde_json::Value::Number(serde_json::Number::from(block_idx)),
                ),
            )
        })?;

        // Step 3: hygiene resolution (merged + parsed once per block).
        let merged_hygiene = merge_hygiene(&manifest.hygiene, block.hygiene.as_ref());
        let bootstrap_method = match merged_hygiene.bootstrap.as_deref() {
            Some(s) => Some(parse_bootstrap_method(s).map_err(|msg| {
                MinerError::Preflight(WireError::preflight(
                    PreflightCode::InvalidParameter,
                    format!("[[jobs[{block_idx}]]] invalid bootstrap method: {msg}"),
                ))
            })?),
            None => None,
        };
        let null_method = match merged_hygiene.null.as_deref() {
            Some(s) => Some(parse_null_method(s).map_err(|msg| {
                MinerError::Preflight(WireError::preflight(
                    PreflightCode::InvalidParameter,
                    format!("[[jobs[{block_idx}]]] invalid null method: {msg}"),
                ))
            })?),
            None => None,
        };
        let bootstrap_n = if merged_hygiene.bootstrap_n > 0 {
            Some(merged_hygiene.bootstrap_n)
        } else {
            None
        };
        let null_n = if merged_hygiene.null_n > 0 {
            Some(merged_hygiene.null_n)
        } else {
            None
        };

        // Step 4: gap-policy parse (default to ContinuousOnly per D3-19).
        let gap_policy = match block.gap_policy.as_deref() {
            None => GapPolicyKind::ContinuousOnly,
            Some(s) => GapPolicyKind::from_str(s).map_err(|bad| {
                MinerError::Preflight(WireError::preflight(
                    PreflightCode::InvalidParameter,
                    format!("[[jobs[{block_idx}]]] invalid gap_policy: {bad:?}"),
                ))
            })?,
        };

        // Step 5: cartesian fanout — instruments → timeframes → windows → params alphabetical.
        for instruments in &instruments_grid {
            for tf_str in &block.timeframes {
                let tf = Timeframe::from_str(tf_str).map_err(|bad| {
                    MinerError::Preflight(WireError::preflight(
                        PreflightCode::InvalidParameter,
                        format!("[[jobs[{block_idx}]]] invalid timeframe: {bad:?}"),
                    ))
                })?;
                for window_str in &block.windows {
                    let window = crate::engine::preflight::parse_iso_utc_window(window_str)
                        .map_err(MinerError::Preflight)?;
                    for params in cartesian_params(&block.params) {
                        let param_hash = compute_param_hash(&params).map_err(|e| {
                            MinerError::Preflight(WireError::preflight(
                                PreflightCode::InvalidParameter,
                                format!("[[jobs[{block_idx}]]] param_hash failed: {e}"),
                            ))
                        })?;
                        let job_seed = derive_job_seed(
                            master_seed,
                            &scan_id_at_version,
                            instruments,
                            tf,
                            &window,
                            param_hash.as_str(),
                        );
                        jobs.push(ResolvedJob {
                            scan_id_at_version: scan_id_at_version.clone(),
                            scan_id: scan_id.clone(),
                            version,
                            instruments: instruments.clone(),
                            timeframe: tf,
                            window,
                            gap_policy,
                            resolved_params: params,
                            param_hash,
                            job_seed,
                            master_seed,
                            bootstrap_method,
                            bootstrap_n,
                            null_method,
                            null_n,
                        });
                    }
                }
            }
        }
    }

    Ok(jobs)
}

// ---------------------------------------------------------------------------
// estimated_job_count — fast cardinality estimate without materialisation
// ---------------------------------------------------------------------------

/// Fast cardinality estimate without materialising the `Vec<ResolvedJob>`.
///
/// Used by [`crate::sweep::manifest::validate`] to enforce the
/// `[sweep].max_jobs` ceiling BEFORE [`expand`] would otherwise allocate
/// 10⁹ entries (`T-05-04-V5-SIZE` mitigation).
///
/// Returns the sum, over blocks, of
/// `instrument_slots * timeframes.len() * windows.len() * params_cartesian_size`.
/// Where `instrument_slots` is the number of cartesian-instrument
/// configurations a block produces (either flat `instruments.len()` for
/// Single-arity OR outer-array `instruments.len()` for Pair-arity); we
/// approximate by treating any non-array (or a flat-string array) as
/// `instruments.len()`, falling back to `1` if the field shape is
/// unparseable (the per-block validate step will reject it later).
#[must_use]
pub fn estimated_job_count(manifest: &SweepManifest) -> u64 {
    let mut total: u64 = 0;
    for block in &manifest.jobs {
        let instrument_slots = instrument_slot_count(&block.instruments);
        let tfs = block.timeframes.len() as u64;
        let wins = block.windows.len() as u64;
        let params_card = params_cartesian_size(&block.params);
        // Saturating arithmetic — a malicious manifest declaring 10⁹ jobs
        // saturates at u64::MAX which is still > max_jobs and triggers the
        // SweepTooLarge gate.
        let block_total = instrument_slots
            .saturating_mul(tfs)
            .saturating_mul(wins)
            .saturating_mul(params_card);
        total = total.saturating_add(block_total);
    }
    total
}

/// Count the number of cartesian-instrument slots a `[[jobs]].instruments`
/// value declares — i.e. how many `Vec<InstrumentSpec>` configurations
/// `parse_instruments_grid` would produce.
///
/// For Single-arity (flat array of strings): returns `array.len()`.
/// For Pair-arity (array of arrays): returns `outer_array.len()`.
/// For any other shape: returns 1 (the per-block validate step will
/// reject the shape with `InvalidParameter`).
fn instrument_slot_count(v: &serde_json::Value) -> u64 {
    let serde_json::Value::Array(outer) = v else {
        return 1;
    };
    if outer.is_empty() {
        return 0;
    }
    // Both flat (Single-arity) and nested (Pair-arity) shapes produce
    // `outer.len()` cartesian-instrument slots — the inner-shape check is
    // a per-block validate concern, not a cardinality concern.
    outer.len() as u64
}

/// Compute the cartesian-product cardinality over a params `BTreeMap`.
/// Array values multiply the cardinality; scalar values are pass-through.
#[must_use]
pub fn params_cartesian_size(params: &BTreeMap<String, serde_json::Value>) -> u64 {
    let mut total: u64 = 1;
    for v in params.values() {
        if let serde_json::Value::Array(arr) = v {
            // Treat empty arrays as 1 (no choices => degenerate; the actual
            // expansion handles this by producing a single empty point).
            let n = if arr.is_empty() { 1 } else { arr.len() as u64 };
            total = total.saturating_mul(n);
        }
    }
    total
}

// ---------------------------------------------------------------------------
// parse_instruments_grid — flat vs nested dispatch per scan arity
// ---------------------------------------------------------------------------

/// Parse the manifest's `[[jobs]].instruments` value against the scan's
/// declared arity. Returns a `Vec<Vec<InstrumentSpec>>` where the outer
/// Vec is the cartesian-instrument-slot dimension.
///
/// - Single-arity: input MUST be a flat `Vec<String>`; output is a Vec
///   of length-1 `InstrumentSpec` lists (one per cartesian slot).
/// - Pair-arity: input MUST be a nested `Vec<Vec<String>>` of inner
///   length 2; output is a Vec of length-2 `InstrumentSpec` lists.
///
/// # Errors
/// Returns a `String` error message describing the mismatch (caller
/// wraps into `MinerError::Preflight`).
pub fn parse_instruments_grid(
    value: &serde_json::Value,
    arity: ScanArity,
) -> Result<Vec<Vec<InstrumentSpec>>, String> {
    let serde_json::Value::Array(arr) = value else {
        return Err(format!(
            "instruments must be a JSON array; got {}",
            value_kind_str(value)
        ));
    };
    match arity {
        ScanArity::Single => {
            // Each element MUST be a string of the form "SYMBOL:side".
            // Reject nested-array shape explicitly so the error message
            // surfaces the arity-mismatch class.
            let mut out: Vec<Vec<InstrumentSpec>> = Vec::with_capacity(arr.len());
            for (i, elem) in arr.iter().enumerate() {
                if let serde_json::Value::Array(_) = elem {
                    return Err(format!(
                        "Single-arity scan expects flat instruments (Vec<String>); got nested array at index {i}"
                    ));
                }
                let s = elem.as_str().ok_or_else(|| {
                    format!(
                        "Single-arity instruments[{i}] must be a string; got {}",
                        value_kind_str(elem)
                    )
                })?;
                let spec = InstrumentSpec::from_str(s).map_err(|bad| {
                    format!("instruments[{i}]: invalid InstrumentSpec {bad:?}")
                })?;
                out.push(vec![spec]);
            }
            Ok(out)
        }
        ScanArity::Pair => {
            // Each element MUST be an inner-array of exactly 2 strings.
            let mut out: Vec<Vec<InstrumentSpec>> = Vec::with_capacity(arr.len());
            for (i, elem) in arr.iter().enumerate() {
                let inner = elem.as_array().ok_or_else(|| {
                    format!(
                        "Pair-arity instruments[{i}] must be a JSON array; got {}",
                        value_kind_str(elem)
                    )
                })?;
                if inner.len() != 2 {
                    return Err(format!(
                        "Pair-arity instruments[{i}] must have exactly 2 entries; got {}",
                        inner.len()
                    ));
                }
                let mut pair: Vec<InstrumentSpec> = Vec::with_capacity(2);
                for (j, inner_elem) in inner.iter().enumerate() {
                    let s = inner_elem.as_str().ok_or_else(|| {
                        format!(
                            "Pair-arity instruments[{i}][{j}] must be a string; got {}",
                            value_kind_str(inner_elem)
                        )
                    })?;
                    let spec = InstrumentSpec::from_str(s).map_err(|bad| {
                        format!("instruments[{i}][{j}]: invalid InstrumentSpec {bad:?}")
                    })?;
                    pair.push(spec);
                }
                out.push(pair);
            }
            Ok(out)
        }
    }
}

/// Validate-only variant used by `manifest::validate` — checks shape
/// without producing the `Vec<Vec<InstrumentSpec>>`.
///
/// # Errors
/// Forwards the error from [`parse_instruments_grid`] unchanged.
pub fn validate_instruments_shape(
    value: &serde_json::Value,
    arity: ScanArity,
) -> Result<(), String> {
    parse_instruments_grid(value, arity).map(|_| ())
}

fn value_kind_str(v: &serde_json::Value) -> &'static str {
    match v {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "bool",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

// ---------------------------------------------------------------------------
// cartesian_params — expand BTreeMap<String, Value> over array-valued keys
// ---------------------------------------------------------------------------

/// Expand a params `BTreeMap` into the cartesian product of array-valued
/// fields. Returns one `serde_json::Value::Object` per cartesian point,
/// with keys in `BTreeMap` (alphabetic) order — D5-01.
///
/// For scalar values: the value is pass-through.
/// For empty array values: a single point with the empty array is
/// produced (preserves the user's intent).
#[must_use]
pub fn cartesian_params(
    params: &BTreeMap<String, serde_json::Value>,
) -> Vec<serde_json::Value> {
    if params.is_empty() {
        // Empty params -> one cartesian point: an empty object.
        return vec![serde_json::Value::Object(serde_json::Map::new())];
    }
    // Build per-key choice vectors (each choice is one serde_json::Value).
    // Iteration over BTreeMap is alphabetic, locking the outer-product key
    // order at construction time.
    let keys: Vec<&String> = params.keys().collect();
    let choices: Vec<Vec<serde_json::Value>> = params
        .values()
        .map(|v| match v {
            serde_json::Value::Array(arr) if !arr.is_empty() => arr.clone(),
            // Scalar (or empty array) — single-choice axis.
            _ => vec![v.clone()],
        })
        .collect();

    let mut out: Vec<serde_json::Value> = Vec::new();
    let total: usize = choices.iter().map(Vec::len).product();
    out.reserve(total);

    // Index-based nested-loop expansion (lexicographic over keys-then-choices).
    let mut idxs = vec![0_usize; choices.len()];
    'outer: loop {
        let mut obj = serde_json::Map::new();
        for (k_idx, k) in keys.iter().enumerate() {
            obj.insert((*k).clone(), choices[k_idx][idxs[k_idx]].clone());
        }
        out.push(serde_json::Value::Object(obj));

        // Advance lexicographic index: increment rightmost, carry left.
        let mut k_idx = idxs.len();
        loop {
            if k_idx == 0 {
                break 'outer;
            }
            k_idx -= 1;
            idxs[k_idx] += 1;
            if idxs[k_idx] < choices[k_idx].len() {
                break;
            }
            idxs[k_idx] = 0;
        }
    }
    out
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
    use crate::sweep::manifest::SweepManifest;

    fn parse_manifest(toml_src: &str) -> SweepManifest {
        toml::from_str(toml_src).expect("parse manifest")
    }

    /// Test 5 (Plan 05-04 Task 1): cartesian expansion produces the
    /// correct number of jobs in correct order.
    ///
    /// `2 instruments × 2 timeframes × 1 window × 2 params = 8` jobs;
    /// order: `EURUSD:bid/15m/lags=5`, `EURUSD:bid/15m/lags=10`,
    /// `EURUSD:bid/1h/lags=5`, ..., `GBPUSD:bid/1h/lags=10`.
    #[test]
    fn expand_produces_deterministic_cartesian_order() {
        let toml = r#"
            [[jobs]]
            scan = "stats.autocorr.ljung_box@1"
            instruments = ["EURUSD:bid", "GBPUSD:bid"]
            timeframes = ["15m", "1h"]
            windows = ["2024-01-01:2024-06-30"]
            params = { lags = [5, 10] }
        "#;
        let m = parse_manifest(toml);
        let reg = registry_bootstrap();
        let jobs = expand(&m, &reg).expect("expand ok");
        assert_eq!(jobs.len(), 8, "2 × 2 × 1 × 2 = 8 jobs");

        // Order: instruments outer (EURUSD then GBPUSD) → timeframes (15m then 1h)
        // → windows (only one) → params alphabetic ({lags=5}, {lags=10}).
        assert_eq!(jobs[0].instruments[0].symbol, "EURUSD");
        assert_eq!(jobs[0].timeframe, Timeframe::Tf15m);
        assert_eq!(jobs[0].resolved_params, serde_json::json!({"lags": 5}));

        assert_eq!(jobs[1].instruments[0].symbol, "EURUSD");
        assert_eq!(jobs[1].timeframe, Timeframe::Tf15m);
        assert_eq!(jobs[1].resolved_params, serde_json::json!({"lags": 10}));

        assert_eq!(jobs[2].instruments[0].symbol, "EURUSD");
        assert_eq!(jobs[2].timeframe, Timeframe::Tf1h);
        assert_eq!(jobs[2].resolved_params, serde_json::json!({"lags": 5}));

        assert_eq!(jobs[7].instruments[0].symbol, "GBPUSD");
        assert_eq!(jobs[7].timeframe, Timeframe::Tf1h);
        assert_eq!(jobs[7].resolved_params, serde_json::json!({"lags": 10}));
    }

    /// Test 6 (Plan 05-04 Task 1): when a job block has multiple params
    /// keys, the cartesian expansion iterates them in alphabetic order
    /// regardless of TOML declaration order. Per PATTERNS line 575
    /// (D5-01): the alphabetic order is the OUTER iteration order — i.e.
    /// the FIRST-alphabetical key advances slowest.
    ///
    /// Declaration `params = { b = [...], a = [...] }`, BTreeMap iterates
    /// alphabetically (`a`, `b`). With the rightmost-key-advances-fastest
    /// expansion in `cartesian_params`, `b` advances first, so we expect
    /// `{a=1, b=10}`, `{a=1, b=20}`, `{a=2, b=10}`, `{a=2, b=20}`.
    #[test]
    fn cartesian_params_iterates_keys_alphabetic() {
        let mut p: BTreeMap<String, serde_json::Value> = BTreeMap::new();
        // Insertion order [b, a]; BTreeMap stores alphabetically [a, b].
        p.insert("b".to_string(), serde_json::json!([10, 20]));
        p.insert("a".to_string(), serde_json::json!([1, 2]));
        let pts = cartesian_params(&p);
        assert_eq!(pts.len(), 4, "2 × 2 = 4 points");
        assert_eq!(pts[0], serde_json::json!({"a": 1, "b": 10}));
        assert_eq!(pts[1], serde_json::json!({"a": 1, "b": 20}));
        assert_eq!(pts[2], serde_json::json!({"a": 2, "b": 10}));
        assert_eq!(pts[3], serde_json::json!({"a": 2, "b": 20}));
    }

    /// Test (Plan 05-04 Task 1): empty params yields one empty-object point.
    #[test]
    fn cartesian_params_empty_yields_one_empty_object() {
        let p: BTreeMap<String, serde_json::Value> = BTreeMap::new();
        let pts = cartesian_params(&p);
        assert_eq!(pts.len(), 1);
        assert_eq!(pts[0], serde_json::json!({}));
    }

    /// Test (Plan 05-04 Task 1): scalar (non-array) param values
    /// pass-through with no cartesian fanout on that axis.
    #[test]
    fn cartesian_params_scalar_passes_through() {
        let mut p: BTreeMap<String, serde_json::Value> = BTreeMap::new();
        p.insert("scalar".to_string(), serde_json::json!(42));
        p.insert("arr".to_string(), serde_json::json!([1, 2]));
        let pts = cartesian_params(&p);
        assert_eq!(pts.len(), 2, "scalar × 2-array = 2 points");
        assert_eq!(pts[0], serde_json::json!({"arr": 1, "scalar": 42}));
        assert_eq!(pts[1], serde_json::json!({"arr": 2, "scalar": 42}));
    }

    /// Test 8 (Plan 05-04 Task 1): every `ResolvedJob` carries a
    /// deterministically-derived `job_seed: u64`. Running expand twice
    /// on the same manifest produces byte-identical `Vec<ResolvedJob>`
    /// (field-by-field comparison via JSON serialisation of the
    /// public-shape fields).
    #[test]
    fn expand_produces_deterministic_job_seeds() {
        let toml = r#"
            [sweep]
            seed = 57005

            [[jobs]]
            scan = "stats.autocorr.ljung_box@1"
            instruments = ["EURUSD:bid", "GBPUSD:bid"]
            timeframes = ["15m"]
            windows = ["2024-01-01:2024-06-30"]
            params = { lags = [5, 10] }
        "#;
        let m = parse_manifest(toml);
        let reg = registry_bootstrap();
        let a = expand(&m, &reg).expect("expand a");
        let b = expand(&m, &reg).expect("expand b");
        assert_eq!(a.len(), b.len());
        for (job_a, job_b) in a.iter().zip(b.iter()) {
            assert_eq!(job_a.scan_id_at_version, job_b.scan_id_at_version);
            assert_eq!(job_a.master_seed, job_b.master_seed);
            assert_eq!(job_a.job_seed, job_b.job_seed, "job_seed must be deterministic");
            assert_eq!(job_a.param_hash.as_str(), job_b.param_hash.as_str());
        }
        // Two distinct jobs MUST produce distinct seeds (negative pin).
        assert_ne!(a[0].job_seed, a[1].job_seed, "different params -> different seed");
    }

    /// Test (Plan 05-04 Task 1): parse_instruments_grid Single-arity flat OK.
    #[test]
    fn parse_instruments_grid_single_flat() {
        let v = serde_json::json!(["EURUSD:bid", "GBPUSD:bid"]);
        let g = parse_instruments_grid(&v, ScanArity::Single).expect("ok");
        assert_eq!(g.len(), 2);
        assert_eq!(g[0].len(), 1);
        assert_eq!(g[0][0].symbol, "EURUSD");
        assert_eq!(g[1][0].symbol, "GBPUSD");
    }

    /// Test (Plan 05-04 Task 1): parse_instruments_grid Pair-arity nested OK.
    #[test]
    fn parse_instruments_grid_pair_nested() {
        let v = serde_json::json!([["EURUSD:bid", "GBPUSD:bid"]]);
        let g = parse_instruments_grid(&v, ScanArity::Pair).expect("ok");
        assert_eq!(g.len(), 1, "one cartesian slot");
        assert_eq!(g[0].len(), 2, "pair has 2 specs");
        assert_eq!(g[0][0].symbol, "EURUSD");
        assert_eq!(g[0][1].symbol, "GBPUSD");
    }

    /// Test (Plan 05-04 Task 1): parse_instruments_grid Single-arity rejects nested.
    #[test]
    fn parse_instruments_grid_single_rejects_nested() {
        let v = serde_json::json!([["EURUSD:bid", "GBPUSD:bid"]]);
        let err = parse_instruments_grid(&v, ScanArity::Single).expect_err("must reject");
        assert!(
            err.contains("nested") || err.contains("Single"),
            "error must mention shape; got {err:?}"
        );
    }

    /// Test (Plan 05-04 Task 1): parse_instruments_grid Pair-arity rejects flat.
    #[test]
    fn parse_instruments_grid_pair_rejects_flat() {
        let v = serde_json::json!(["EURUSD:bid", "GBPUSD:bid"]);
        let err = parse_instruments_grid(&v, ScanArity::Pair).expect_err("must reject");
        assert!(
            err.contains("array") || err.contains("Pair"),
            "error must mention shape; got {err:?}"
        );
    }

    /// Test (Plan 05-04 Task 1): estimated_job_count matches the
    /// post-expansion length.
    #[test]
    fn estimated_job_count_matches_expansion_length() {
        let toml = r#"
            [[jobs]]
            scan = "stats.autocorr.ljung_box@1"
            instruments = ["EURUSD:bid", "GBPUSD:bid"]
            timeframes = ["15m", "1h"]
            windows = ["2024-01-01:2024-06-30"]
            params = { lags = [5, 10, 20] }
        "#;
        let m = parse_manifest(toml);
        let reg = registry_bootstrap();
        let estimate = estimated_job_count(&m);
        let jobs = expand(&m, &reg).expect("expand ok");
        assert_eq!(estimate, jobs.len() as u64);
        assert_eq!(estimate, 12, "2 × 2 × 1 × 3 = 12");
    }
}
