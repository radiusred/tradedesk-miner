//! Phase 3 facade — single library entry point CLI/MCP/HTTP all call.
//!
//! Pattern analog: `cache.rs:519-573` ([`crate::cache::BarCache::get_or_build`]) —
//! a single-method facade returning a value with a multi-line algorithm doc.
//! `engine::run_one` follows the same shape — it OWNS `RunStart`/`RunEnd`
//! framing emission, `param_hash` computation, run-id assignment, sink
//! dispatch, error classification, and the `RunOutcome` the CLI maps to an
//! exit code.
//!
//! ## Module decomposition (D3-15 broker + D3-22 cancel + D3-24 exit-code routing)
//!
//! - [`preflight`] — parse + validate `--params`, reject unknown scans.
//! - [`gap_policy`] — strict / `continuous_only` dispatch + sub-range partitioning.
//! - [`param_hash`] — blake3 hash of canonical resolved params (D3-13).
//! - [`framing`] — `RunStart` / `RunEnd` envelope builders (clock reads ONLY here).
//!
//! The facade is sync + std-only (FOUND-04). No tokio, no async-std, no async
//! traits — Phase 5 will fan out via rayon, never tokio.
//!
//! ## Reader-error wrapping (Warning 8)
//!
//! Reader and cache errors are wrapped via `MinerError::Scan(String)` —
//! `MinerError` declares Io / Serialize / Config / Scan / Internal variants
//! only (no `Reader` variant). `Scan(String)` is the semantically-closest
//! match for "the data-loading step failed" at this layer; the conversion
//! lives at the two `MinerError::Scan(format!(...))` call sites below.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use chrono::Utc;

use crate::aggregator::AggParams;
use crate::cache::BarCache;
use crate::config::MinerConfig;
use crate::error::MinerError;
use crate::findings::run_id::RunId;
use crate::findings::{
    BootstrapSpec, DataSlice, DryRunFinding, Finding, FindingSink, GapAbortedFinding, NullSpec,
    PerScanCounts, ReproEnvelope, ResultFinding, RunSummary, Source, TimeRange,
};
use crate::gap::GapDetector;
use crate::reader::{ClosedRangeUtc, Reader};
use crate::scan::hygiene::{bootstrap as hygiene_bootstrap, null as hygiene_null, seed as hygiene_seed};
use crate::scan::{BootstrapMethod, NullMethod, ScanCtx, ScanError, ScanRequest};

use hygiene_buffering_sink::HygieneBufferingSink;

pub mod framing;
pub mod gap_policy;
pub(crate) mod hygiene_buffering_sink;
pub(crate) mod hygiene_dispatch;
pub mod param_hash;
pub mod preflight;

// Plan 04 imports these via `use miner_core::engine::*;` — re-export the
// two enums so callers don't need to spell out the inner module path.
pub use gap_policy::{GapDispatch, GapPolicyKind};

// ---------------------------------------------------------------------------
// RunOutcome — internal enum the CLI maps to an exit code (D3-24).
// ---------------------------------------------------------------------------

/// Outcome of a single [`run_one`] invocation.
///
/// Pattern analog: `gap.rs:117-130` `GapReason` — tagged enum with explicit
/// `Eq`-safe variants (no `f64` inside). `RunOutcome` is INTERNAL — no
/// `Serialize` derive needed; the CLI maps it to a POSIX exit code per D3-24:
///
/// | `RunOutcome`            | exit code |
/// |-------------------------|-----------|
/// | `Ok`                    | `0` (or `2` if SIGINT mid-run → `130`) |
/// | `HadScanErrors`         | `2` |
/// | `PreflightFailed`       | `1` |
///
/// SIGINT (D3-22) is detected on the cancellation flag AFTER `run_one` returns
/// and overrides the outcome — the CLI emits exit code `130` regardless.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunOutcome {
    /// `RunEnd` emitted; at least one `Result` / `GapAborted` / `DryRun` finding
    /// streamed (or zero findings if the scan computed none).
    Ok,
    /// `RunEnd` emitted AND at least one mid-stream `Finding::ScanError` was
    /// emitted. Stream may have a mix of `Result` + `ScanError` findings.
    HadScanErrors,
    /// Pre-flight rejection: unknown scan, invalid param, missing config, etc.
    /// Stdout is empty; the CLI writes a single `WireError` JSON line to
    /// stderr and exits 1.
    PreflightFailed,
}

// ---------------------------------------------------------------------------
// run_one — the single-entry facade. Pattern: cache.rs:569-573.
// ---------------------------------------------------------------------------

/// Execute one scan request end-to-end.
///
/// Pattern analog: `cache.rs:569-573` ([`crate::cache::BarCache::get_or_build`])
/// — a single-method facade returning a value with a numbered algorithm doc.
///
/// ## Algorithm
///
/// 1. **Cancel-at-entry yield site** (RESEARCH Pattern 4 site 1; SC-5b
///    `cancel_at_entry`). Returns `Ok(RunOutcome::Ok)` without emitting
///    anything if `cancel` is already set.
/// 2. **Preflight**: resolve the scan from `bootstrap()`. On unknown scan,
///    return `Err(MinerError::Scan(_))` WITHOUT emitting `RunStart`; the CLI
///    converts to a `WireError` on stderr and exits 1.
/// 3. **Framing-open**: build + emit `RunStart` via
///    [`framing::build_run_start`]. The builder echoes `req.dry_run` into
///    `RunStart.request` (Blocker 2 closure; D3-21).
/// 4. **Dry-run short-circuit** (Blocker 2 / Warning 9 pin): if
///    `req.dry_run == true`, emit one `Finding::DryRun` and jump to
///    framing-close. `RunSummary.results_emitted` stays at zero — `RunSummary`
///    is NOT extended with any new counter field; the dry-run signal lives
///    in `Finding::DryRun` + `RunStart.request.dry_run` only.
/// 5. **Gap detection** (Warning 8 wrap): `GapDetector::detect(...)` may
///    return `Err(R::Error)`. Wrap via
///    `MinerError::Scan(format!("reader: {e}"))` — `MinerError` has no
///    `Reader` variant; `Scan(String)` is the chosen wrapper per Warning 8.
/// 6. **Gap-policy dispatch** ([`gap_policy::dispatch`]):
///    - `GapDispatch::Aborted(m)` → emit one `Finding::GapAborted` carrying
///      the full manifest; `summary.gap_aborted += 1`.
///    - `GapDispatch::SubRanges(ranges)` → for each sub-range:
///      * **Cancel-before-subrange yield site** (SC-5b
///        `cancel_before_subrange`): poll `cancel` BEFORE loading bars for
///        the next sub-range; on cancel, break and proceed to framing-close.
///      * Build a per-sub-range `ScanRequest` (Pitfall 4 — `sub_range`
///        distinct from `window`).
///      * Load bars via `BarCache::get_or_build` (Warning 8 wrap on
///        `CacheError`).
///      * Construct `ScanCtx` and dispatch the scan. The third documented
///        cancel yield site (`cancel_inside_scan_kernel`) lives INSIDE
///        `Scan::run`'s cancel-aware sleep loop (Plan 04 Task 2).
///      * On `Err(ScanError::Compute|Kernel(_))`: emit a
///        `Finding::ScanError` envelope, increment `summary.scan_errors`, and
///        continue (D-05 — per-finding scan errors are NOT preflight
///        failures).
///      * On `Err(ScanError::Cancelled)`: break out of the loop cleanly.
/// 7. **Framing-close**: build + emit `RunEnd` via
///    [`framing::build_run_end`]; flush the sink (Pitfall 5 — THE one
///    and only flush point in the whole pipeline).
/// 8. Return `RunOutcome::Ok` if `summary.scan_errors == 0`; otherwise
///    `RunOutcome::HadScanErrors`.
///
/// ## Documented cancellation yield sites (SC-5b — Blocker 1)
///
/// `engine::cancellation_tests` exercises all three by name:
/// 1. `cancel_at_entry`        — Step 1 above; flag set BEFORE `run_one` is invoked.
/// 2. `cancel_before_subrange` — Step 6 sub-range loop top.
/// 3. `cancel_inside_scan_kernel` — inside `LjungBoxScan::run`'s cancel-aware
///    sleep loop (controlled by the cfg-gated
///    `ScanCtx.sleep_after_first_finding_ms` hook, Plan 02 Task 2).
///
/// ## Clock-isolation invariant (D3-23)
///
/// `Utc::now()` is read EXACTLY TWICE inside this function — once at
/// `started` (before `RunStart`) and once at `ended` (before `RunEnd`). Scans
/// MUST NOT read the wall-clock; per-finding `produced_at_utc` reads come
/// from inside the scan body (the engine does not synthesize them).
///
/// # Errors
///
/// Returns [`MinerError`] only on:
/// - Preflight failure (unknown scan) — `MinerError::Scan(_)`.
/// - Reader error during gap detection — `MinerError::Scan("reader: ...")`.
/// - Cache error during bar loading — `MinerError::Scan("cache: ...")`.
/// - Sink IO / serialisation error — propagated from `FindingSink`.
///
/// Per-finding scan errors are emitted as `Finding::ScanError` envelopes and
/// surface in the return as `RunOutcome::HadScanErrors`, NOT as `Err`.
#[allow(
    clippy::too_many_lines,
    reason = "run_one is the single-method facade — the 7-step algorithm walk is intentionally inline so the call site reads top-to-bottom against the doc-comment numbering"
)]
#[allow(
    clippy::needless_pass_by_value,
    reason = "the CLI wrapper builds `Arc<AtomicBool>` once at main() and hands ownership in; Arc::clone is used inside per sub-range. By-value is the canonical engine-entry signature."
)]
pub fn run_one<R: Reader>(
    req: &ScanRequest,
    cfg: &MinerConfig,
    reader: &R,
    sink: &mut dyn FindingSink,
    cancel: Arc<AtomicBool>,
) -> Result<RunOutcome, MinerError> {
    // Plan 03-07 Task 2 (CR-01 refactor): public `run_one` is a thin wrapper
    // over `run_one_with_registry` so the engine integration tests can inject
    // a `FailingIoScan` / `FailingMinerScan` fixture into the registry
    // without going through the production `bootstrap()` path. Binary callers
    // (CLI / MCP / HTTP wrappers) continue to use this signature; the inner
    // function carries the same algorithm verbatim.
    let registry = crate::scan::bootstrap();
    run_one_with_registry(req, cfg, reader, sink, cancel, &registry)
}

/// Internal `run_one` body, parameterised on the scan registry.
///
/// Plan 03-07 Task 2 (CR-01) extracted this from `run_one` so the engine
/// tests can inject a per-test registry containing a `FailingIoScan` /
/// `FailingMinerScan` fixture. The algorithm is identical to the
/// previously-inlined `run_one` body; the public `run_one` is now a thin
/// wrapper passing `&crate::scan::bootstrap()`.
///
/// Plan 04-02 (D4-02) widened the visibility to `pub` so the Phase 4
/// integration-test scaffolds (`arity_preflight.rs`, `two_leg_facade.rs`)
/// can inject per-test stub-Pair-arity registries without spawning the
/// CLI. The `run_one` public wrapper remains the canonical entry for
/// the CLI / MCP / HTTP transports.
///
/// # Errors
/// Same as [`run_one`]:
/// - `MinerError::Preflight(_)` on unknown scan or wrong instrument arity
///   (D4-02). Stdout stays empty in both cases.
/// - `MinerError::Scan(_)` on reader/cache/serialisation failure surfaced
///   via the `Finding::ScanError` envelope path. Stdout already contains
///   the `RunStart` + `ScanError` + `RunEnd` envelopes when this error
///   surfaces; the caller treats it as `RunOutcome::HadScanErrors`.
///
/// # Panics
/// `expect`s that `ScanRequest.instruments` is non-empty post-preflight —
/// `validate_arity` (D4-02) rejects empty `instruments` before reaching
/// the per-leg dispatch, so the panic is structurally unreachable.
#[allow(
    clippy::too_many_lines,
    reason = "see run_one's clippy::too_many_lines justification — this is the algorithm-walk function"
)]
#[allow(
    clippy::needless_pass_by_value,
    reason = "see run_one's clippy::needless_pass_by_value justification — Arc<AtomicBool> is the cancellation-flag handoff"
)]
pub fn run_one_with_registry<R: Reader>(
    req: &ScanRequest,
    cfg: &MinerConfig,
    reader: &R,
    sink: &mut dyn FindingSink,
    cancel: Arc<AtomicBool>,
    registry: &crate::scan::Registry,
) -> Result<RunOutcome, MinerError> {
    // -----------------------------------------------------------------------
    // Step 1 — Cancel-at-entry yield site (cancellation_tests::cancel_at_entry)
    // -----------------------------------------------------------------------
    if cancel.load(Ordering::Relaxed) {
        return Ok(RunOutcome::Ok);
    }

    // -----------------------------------------------------------------------
    // Step 2 — Preflight: resolve scan via the supplied registry.
    //
    // Plan 03-07 CR-03 fix: route through the typed
    // `engine::preflight::resolve_scan` helper which returns
    // `Result<&dyn Scan, WireError>` carrying `PreflightCode::UnknownScan`.
    // The previous inlined `registry.get(...)` plus a stringly-typed
    // `MinerError::Scan(format!(...))` was the CR-03 root cause: the CLI
    // dispatch matched on the format-string prefix — a fragile coupling
    // between engine and CLI. Now the error is a typed
    // `MinerError::Preflight(WireError)` which the CLI dispatches on via a
    // typed match.
    // -----------------------------------------------------------------------
    let scan = preflight::resolve_scan(&format!("{}@{}", req.scan_id, req.version), registry)
        .map_err(MinerError::Preflight)?;

    // Phase 4 (Plan 04-02 / D4-02): arity preflight. Reject any request whose
    // `instruments.len()` does not match `scan.arity().expected_len()` with
    // `PreflightCode::WrongInstrumentArity`. Inserted between resolve_scan
    // and parse_params per RESEARCH.md §1.10 (id-resolve → version-check →
    // arity-check → params-parse).
    preflight::validate_arity(scan, &req.instruments).map_err(MinerError::Preflight)?;

    // Phase 5 (Plan 05-03 / D5-04 / HYG-03 + HYG-04): hygiene-support preflight.
    // Reject any request whose `bootstrap_method` / `null_method` targets a
    // scan that has not opted into the requested method per the per-scan
    // matrix. Inserted between `validate_arity` and parse_params per the
    // RESEARCH.md preflight ordering (id-resolve → version-check →
    // arity-check → hygiene-check → params-parse).
    preflight::validate_hygiene_support(scan, req.bootstrap_method, req.null_method)
        .map_err(MinerError::Preflight)?;

    // jsonschema validation of resolved_params against scan.param_schema() is
    // OUT OF SCOPE for Phase 3 (heavyweight dep + duplicated by the per-scan
    // internal checks like LjungBoxScan's lags-range guard). The typed-fallback
    // parse in preflight::parse_params_kv is the sole boundary check.

    // -----------------------------------------------------------------------
    // Step 3 — Framing-open: emit RunStart.
    // -----------------------------------------------------------------------
    let run_id = RunId::new();
    let started = Utc::now();
    let run_start_finding = framing::build_run_start(req, run_id, started, crate::CODE_REVISION);
    sink.write_envelope(&run_start_finding)?;

    // Holds the request Value the dry-run path echoes (extracted from the
    // built RunStart to avoid re-deriving it here).
    let request_value = match &run_start_finding {
        Finding::RunStart(rs) => rs.request.clone(),
        _ => unreachable!("framing::build_run_start always returns Finding::RunStart"),
    };

    let mut summary = RunSummary::default();

    // -----------------------------------------------------------------------
    // Step 4 — Dry-run short-circuit (Blocker 2 / D3-21 / Warning 9 pin).
    // -----------------------------------------------------------------------
    if req.dry_run {
        let planned_data_slice = DataSlice {
            range: TimeRange {
                start_utc: req.window.start,
                end_utc: req.window.end,
            },
            gap_manifest_ref: None,
            gap_manifest: None,
            // Phase 4 (D4-03): planned slice for dry-run keeps an empty
            // sources Vec — the dry-run path does not load bars, and the
            // ANOM-01 returns primitive (Plan 04-02) will populate the Vec
            // when it lands the real run path.
            sources: Vec::new(),
        };
        let dry_run_finding = Finding::DryRun(DryRunFinding {
            run_id,
            produced_at_utc: Utc::now(),
            request: request_value,
            resolved_params: req.resolved_params.clone(),
            planned_data_slice,
            estimated_findings_count: 1,
        });
        sink.write_envelope(&dry_run_finding)?;
        // Pitfall 3: results_emitted stays at 0 — DryRun does NOT increment
        // any RunSummary counter; the signal lives in the envelope only.
        emit_run_end(sink, run_id, started, summary)?;
        return Ok(RunOutcome::Ok);
    }

    // Plan 03-07 CR-01: hoist `scan_id_at_version` above step 5 so the new
    // error-handling blocks for the reader, cache, scan-IO, and
    // scan-miner-error arms can build a `Finding::ScanError` envelope before
    // returning. The string was originally constructed inside step 6's match,
    // but the wrapped error paths need it earlier.
    let scan_id_at_version = format!("{}@{}", req.scan_id, req.version);

    // -----------------------------------------------------------------------
    // Phase 4 Plan 04-12 (CR-01): dispatch on `scan.arity()`. The historical
    // single-leg body lives in the `Single` arm below; the new Pair arm
    // delegates to `dispatch_pair_arity_body` which mirrors the single-leg
    // shape but loads BOTH legs, intersects their gap manifests via
    // `gap_policy::dispatch_pair`, and constructs `ScanCtx` with
    // `bars_pair: Some((a, b))`.
    //
    // Before the fix the engine hard-coded the single-leg path here, so
    // every Pair-arity CROSS scan emitted `Finding::ScanError` with the
    // "expected Pair arity (ctx.bars_pair is None)" message. See
    // .planning/phases/04-scan-catalogue-anom-cross-seas/04-REVIEW.md CR-01
    // for the hand-off chain that orphaned `gap_policy::dispatch_pair`.
    // -----------------------------------------------------------------------
    if matches!(scan.arity(), crate::scan::ScanArity::Pair) {
        return dispatch_pair_arity_body(
            req,
            cfg,
            reader,
            sink,
            cancel,
            scan,
            run_id,
            started,
            summary,
            &scan_id_at_version,
        );
    }

    // -----------------------------------------------------------------------
    // Step 5 — Gap detection (Single-arity path).
    //
    // Plan 03-07 CR-01: on reader error, emit `Finding::ScanError` +
    // `Finding::RunEnd` BEFORE returning `Ok(RunOutcome::HadScanErrors)` so
    // consumers never see an orphaned RunStart. The reader-context message
    // ("reader: ...") survives into the ScanError envelope's `message`
    // field; the envelope's `error_code` is `ScanErrorCode::ComputeError`
    // (mirrors the existing kernel-error arm). Tests:
    // `run_one_reader_error_emits_run_start_and_run_end_with_scan_error`.
    // -----------------------------------------------------------------------
    // Phase 4 (D4-01): single-leg dispatch reads instruments[0]. The Plan
    // 04-02 `validate_arity` preflight rejects mismatched arity before this
    // point with `PreflightCode::WrongInstrumentArity`; the Pair branch is
    // handled above (Plan 04-12). The `.expect(...)` panic is structurally
    // unreachable post-preflight — it stays as a diagnosis hook if the
    // invariant ever leaks.
    let leg = req
        .instruments
        .first()
        .expect("ScanRequest.instruments must be non-empty post-preflight (D4-02)");
    let manifest = match GapDetector::detect(reader, &leg.symbol, leg.side, req.window) {
        Ok(m) => m,
        Err(e) => {
            let msg = format!("reader: {e}");
            emit_scan_error(
                sink,
                run_id,
                &scan_id_at_version,
                req,
                reader.source_id(),
                &msg,
            )?;
            summary.scan_errors += 1;
            summary
                .per_scan
                .entry(scan_id_at_version.clone())
                .or_insert_with(PerScanCounts::default)
                .errors += 1;
            emit_run_end(sink, run_id, started, summary)?;
            return Ok(RunOutcome::HadScanErrors);
        }
    };
    let dispatch_result = gap_policy::dispatch(&manifest, req.window, req.gap_policy);

    // -----------------------------------------------------------------------
    // Step 6 — Dispatch.
    // -----------------------------------------------------------------------
    match dispatch_result {
        GapDispatch::Aborted(m) => {
            // Phase 4 (D4-03): populate `data_slice.sources` parallel to
            // `req.instruments` (length 1 for single-leg). The previous
            // singleton `source: Source` field on GapAbortedFinding has been
            // removed in favour of this vector.
            let sources: Vec<Source> = req
                .instruments
                .iter()
                .map(|spec| Source {
                    source_id: reader.source_id().to_string(),
                    symbol: spec.symbol.clone(),
                    side: spec.side.as_str().to_string(),
                    timeframe: req.timeframe.as_str().to_string(),
                })
                .collect();
            let gap_aborted_finding = Finding::GapAborted(GapAbortedFinding {
                schema_version: 1,
                scan_id_at_version: scan_id_at_version.clone(),
                param_hash: req.param_hash.as_str().to_string(),
                code_revision: crate::CODE_REVISION.to_string(),
                data_slice: DataSlice {
                    range: TimeRange {
                        start_utc: req.window.start,
                        end_utc: req.window.end,
                    },
                    gap_manifest_ref: None,
                    gap_manifest: Some(m.clone()),
                    sources,
                },
                dsr: None,
                fdr_q: None,
                run_id,
                produced_at_utc: Utc::now(),
                gap_manifest: serde_json::to_value(&m).map_err(MinerError::from)?,
            });
            sink.write_envelope(&gap_aborted_finding)?;
            summary.gap_aborted += 1;
            summary
                .per_scan
                .entry(scan_id_at_version)
                .or_insert_with(PerScanCounts::default)
                .gap_aborted += 1;
        }
        GapDispatch::SubRanges(sub_ranges) => {
            let cache = BarCache::new(&cfg.bar_cache_root);
            for sub_range in sub_ranges {
                // Step 6a — cancel-before-subrange yield site
                // (cancellation_tests::cancel_before_subrange).
                if cancel.load(Ordering::Relaxed) {
                    break;
                }

                // Build a per-sub-range ScanRequest (Pitfall 4 — sub_range
                // distinct from window).
                let mut per_sub_req = req.clone();
                per_sub_req.sub_range = sub_range.clone();

                // Load bars for THIS sub-range. Convert the sub_range to a
                // ClosedRangeUtc for AggParams.
                let sub_range_utc = ClosedRangeUtc {
                    start: sub_range.start_utc,
                    end: sub_range.end_utc,
                };
                // Plan 03-07 CR-01: on cache error, emit Finding::ScanError
                // + Finding::RunEnd BEFORE returning HadScanErrors so the
                // RunStart already streamed at step 3 has its closing
                // envelope pair. Test:
                // `run_one_cache_error_emits_run_start_and_run_end_with_scan_error`.
                let bars = match cache.get_or_build(
                    reader,
                    AggParams {
                        // Phase 4 (D4-01): single-leg dispatch reads
                        // instruments[0]. CROSS scans (Plan 04-03) will
                        // iterate the Vec twice.
                        symbol: &leg.symbol,
                        side: leg.side,
                        tf: req.timeframe,
                        range: sub_range_utc,
                    },
                ) {
                    Ok(b) => b,
                    Err(e) => {
                        let msg = format!("cache: {e}");
                        emit_scan_error(
                            sink,
                            run_id,
                            &scan_id_at_version,
                            req,
                            reader.source_id(),
                            &msg,
                        )?;
                        summary.scan_errors += 1;
                        summary
                            .per_scan
                            .entry(scan_id_at_version.clone())
                            .or_insert_with(PerScanCounts::default)
                            .errors += 1;
                        emit_run_end(sink, run_id, started, summary)?;
                        return Ok(RunOutcome::HadScanErrors);
                    }
                };

                // Build ScanCtx. The ContinuousOnly path inlines the full
                // manifest into Result.data_slice; the Strict zero-gap fast
                // path leaves gap_manifest = None (D3-12).
                let ctx_gap_manifest: Option<&crate::gap::GapManifest> =
                    if matches!(req.gap_policy, GapPolicyKind::ContinuousOnly) {
                        Some(&manifest)
                    } else {
                        None
                    };
                // Phase 4 (Plan 04-02 / D4-02): single-leg engine path
                // passes `bars_pair: None`. The Pair branch lives in the
                // CROSS dispatch (Plan 04-07 wires it via a new
                // `dispatch_pair` path); until then, the Pair-arity
                // preflight rejection above prevents Pair scans from
                // reaching here.
                let ctx = make_scan_ctx(
                    &bars,
                    None,
                    ctx_gap_manifest,
                    run_id,
                    crate::CODE_REVISION,
                    Arc::clone(&cancel),
                    req,
                );

                // Plan 05-03 continuation: wrap the user sink in a
                // HygieneBufferingSink only when hygiene is requested.
                // When hygiene is inactive, the wrapper is a thin
                // pass-through (preserves per-envelope-flush PITFALLS #4
                // — the Plan 06 SIGINT integration test depends on
                // Result envelopes hitting stdout BEFORE the scan body's
                // cancel-aware sleep loop fires).
                let hygiene_active =
                    per_sub_req.bootstrap_method.is_some() || per_sub_req.null_method.is_some();
                let dispatch_result = {
                    let mut buf_sink = HygieneBufferingSink::new(sink, hygiene_active);
                    let scan_res = scan.run(&ctx, &per_sub_req, &mut buf_sink);
                    let (buffered, inner) = buf_sink.into_parts();
                    (scan_res, buffered, inner)
                };
                // Dispatch the scan and handle the typed result.
                match dispatch_result.0 {
                    Ok(()) => {
                        let inner = dispatch_result.2;
                        let buffered = dispatch_result.1;
                        if hygiene_active {
                            for raw_result in buffered {
                                let mutated = apply_hygiene_mutations(
                                    raw_result,
                                    &per_sub_req,
                                    &bars.close,
                                    &cancel,
                                );
                                inner.write_envelope(&Finding::Result(mutated))?;
                                summary.results_emitted += 1;
                                summary
                                    .per_scan
                                    .entry(scan_id_at_version.clone())
                                    .or_insert_with(PerScanCounts::default)
                                    .results += 1;
                            }
                        } else {
                            // hygiene_active == false: Results already
                            // flowed through to the inner sink during
                            // Scan::run. We still increment the
                            // counter — one Result per scan.run Ok per
                            // sub-range, matching the pre-continuation
                            // contract exactly.
                            summary.results_emitted += 1;
                            summary
                                .per_scan
                                .entry(scan_id_at_version.clone())
                                .or_insert_with(PerScanCounts::default)
                                .results += 1;
                        }
                    }
                    Err(ScanError::Cancelled) => break,
                    Err(ScanError::Kernel(msg)) => {
                        summary.scan_errors += 1;
                        summary
                            .per_scan
                            .entry(scan_id_at_version.clone())
                            .or_insert_with(PerScanCounts::default)
                            .errors += 1;
                        emit_scan_error(
                            sink,
                            run_id,
                            &scan_id_at_version,
                            req,
                            reader.source_id(),
                            &msg,
                        )?;
                    }
                    Err(ScanError::Io(e)) => {
                        // Plan 03-07 CR-01: wrap mid-stream scan-IO errors —
                        // emit Finding::ScanError + Finding::RunEnd BEFORE
                        // returning HadScanErrors so the RunStart already
                        // streamed at step 3 has its closing envelope pair.
                        // Test:
                        // run_one_scan_io_error_emits_run_start_and_run_end_with_scan_error
                        let msg = format!("scan io: {e}");
                        emit_scan_error(
                            sink,
                            run_id,
                            &scan_id_at_version,
                            req,
                            reader.source_id(),
                            &msg,
                        )?;
                        summary.scan_errors += 1;
                        summary
                            .per_scan
                            .entry(scan_id_at_version.clone())
                            .or_insert_with(PerScanCounts::default)
                            .errors += 1;
                        emit_run_end(sink, run_id, started, summary)?;
                        return Ok(RunOutcome::HadScanErrors);
                    }
                    Err(ScanError::Miner(e)) => {
                        // Plan 03-07 CR-01: same wrapping treatment as the
                        // ScanError::Io arm above — preserve framing on
                        // mid-stream MinerError failures. Test:
                        // run_one_scan_miner_error_emits_run_start_and_run_end_with_scan_error
                        let msg = format!("scan miner-error: {e}");
                        emit_scan_error(
                            sink,
                            run_id,
                            &scan_id_at_version,
                            req,
                            reader.source_id(),
                            &msg,
                        )?;
                        summary.scan_errors += 1;
                        summary
                            .per_scan
                            .entry(scan_id_at_version.clone())
                            .or_insert_with(PerScanCounts::default)
                            .errors += 1;
                        emit_run_end(sink, run_id, started, summary)?;
                        return Ok(RunOutcome::HadScanErrors);
                    }
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Step 7 — Framing-close. THE one and only flush call (Pitfall 5).
    // -----------------------------------------------------------------------
    let had_errors = summary.scan_errors > 0;
    emit_run_end(sink, run_id, started, summary)?;

    // -----------------------------------------------------------------------
    // Step 8 — Map summary to RunOutcome.
    // -----------------------------------------------------------------------
    if had_errors {
        Ok(RunOutcome::HadScanErrors)
    } else {
        Ok(RunOutcome::Ok)
    }
}

/// Phase 4 Plan 04-12 (CR-01 fix): Pair-arity dispatch body.
///
/// Mirrors the Single-arity body in `run_one_with_registry` but operates on
/// TWO legs: gap-detects leg A AND leg B, dispatches through
/// [`gap_policy::dispatch_pair`] (which UNIONs the per-leg manifests via
/// [`crate::scan::primitives::time_alignment::intersect_gaps`]), loads bars
/// for BOTH legs per sub-range, and builds `ScanCtx` with
/// `bars_pair: Some((leg_a, leg_b))`.
///
/// The function carries the SAME 5-arm error-handling shape as the Single
/// path: reader error (per leg), cache error (per leg), `ScanError::Kernel`,
/// `ScanError::Io`, `ScanError::Miner`. Cancel-poll cadence is preserved
/// (cancel-before-subrange at sub-range loop top). The single `GapAborted`
/// envelope on `GapDispatch::Aborted` carries the JOINT manifest (the UNION
/// of leg-A and leg-B gaps, per `dispatch_pair`'s contract);
/// `data_slice.sources` carries BOTH legs (D4-03) in the same order as
/// `req.instruments`.
#[allow(
    clippy::too_many_lines,
    reason = "Pair body mirrors the Single body's 5-arm error walk inline so the call site reads top-to-bottom against the Single algorithm's doc-comment numbering"
)]
#[allow(
    clippy::too_many_arguments,
    reason = "All arguments come from the caller's stack frame; introducing a context struct only to satisfy this lint would obscure the data flow"
)]
#[allow(
    clippy::needless_pass_by_value,
    reason = "cancel: Arc<AtomicBool> follows the by-value convention used by every other facade function (run_one, run_one_with_registry, dispatch_single_arity_body); switching to &Arc here would diverge from the sibling signatures"
)]
fn dispatch_pair_arity_body<R: Reader>(
    req: &ScanRequest,
    cfg: &MinerConfig,
    reader: &R,
    sink: &mut dyn FindingSink,
    cancel: Arc<AtomicBool>,
    scan: &dyn crate::scan::Scan,
    run_id: RunId,
    started: chrono::DateTime<Utc>,
    mut summary: RunSummary,
    scan_id_at_version: &str,
) -> Result<RunOutcome, MinerError> {
    // Preflight has guaranteed instruments.len() == 2 (ScanArity::Pair).
    let leg_a = &req.instruments[0];
    let leg_b = &req.instruments[1];

    // Step 5a — Gap detection for leg A.
    let manifest_a = match GapDetector::detect(reader, &leg_a.symbol, leg_a.side, req.window) {
        Ok(m) => m,
        Err(e) => {
            let msg = format!("reader (leg a): {e}");
            emit_scan_error(
                sink,
                run_id,
                scan_id_at_version,
                req,
                reader.source_id(),
                &msg,
            )?;
            summary.scan_errors += 1;
            summary
                .per_scan
                .entry(scan_id_at_version.to_string())
                .or_default()
                .errors += 1;
            emit_run_end(sink, run_id, started, summary)?;
            return Ok(RunOutcome::HadScanErrors);
        }
    };

    // Step 5b — Gap detection for leg B.
    let manifest_b = match GapDetector::detect(reader, &leg_b.symbol, leg_b.side, req.window) {
        Ok(m) => m,
        Err(e) => {
            let msg = format!("reader (leg b): {e}");
            emit_scan_error(
                sink,
                run_id,
                scan_id_at_version,
                req,
                reader.source_id(),
                &msg,
            )?;
            summary.scan_errors += 1;
            summary
                .per_scan
                .entry(scan_id_at_version.to_string())
                .or_default()
                .errors += 1;
            emit_run_end(sink, run_id, started, summary)?;
            return Ok(RunOutcome::HadScanErrors);
        }
    };

    // Joint manifest = UNION of per-leg gaps (the "do not run" set for
    // CROSS scans). Computed inside `gap_policy::dispatch_pair`; we also
    // reconstruct it here for ScanCtx.gap_manifest under ContinuousOnly so
    // the kernel can echo it into Finding::Result.data_slice (D3-12 parity
    // with the Single-arity path).
    let joint_manifest =
        crate::scan::primitives::time_alignment::intersect_gaps(&manifest_a, &manifest_b);
    let dispatch_result =
        gap_policy::dispatch_pair(&manifest_a, &manifest_b, req.window, req.gap_policy);

    match dispatch_result {
        GapDispatch::Aborted(m) => {
            let sources: Vec<Source> = req
                .instruments
                .iter()
                .map(|spec| Source {
                    source_id: reader.source_id().to_string(),
                    symbol: spec.symbol.clone(),
                    side: spec.side.as_str().to_string(),
                    timeframe: req.timeframe.as_str().to_string(),
                })
                .collect();
            let gap_aborted_finding = Finding::GapAborted(GapAbortedFinding {
                schema_version: 1,
                scan_id_at_version: scan_id_at_version.to_string(),
                param_hash: req.param_hash.as_str().to_string(),
                code_revision: crate::CODE_REVISION.to_string(),
                data_slice: DataSlice {
                    range: TimeRange {
                        start_utc: req.window.start,
                        end_utc: req.window.end,
                    },
                    gap_manifest_ref: None,
                    gap_manifest: Some(m.clone()),
                    sources,
                },
                dsr: None,
                fdr_q: None,
                run_id,
                produced_at_utc: Utc::now(),
                gap_manifest: serde_json::to_value(&m).map_err(MinerError::from)?,
            });
            sink.write_envelope(&gap_aborted_finding)?;
            summary.gap_aborted += 1;
            summary
                .per_scan
                .entry(scan_id_at_version.to_string())
                .or_default()
                .gap_aborted += 1;
        }
        GapDispatch::SubRanges(sub_ranges) => {
            let cache = BarCache::new(&cfg.bar_cache_root);
            for sub_range in sub_ranges {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }

                let mut per_sub_req = req.clone();
                per_sub_req.sub_range = sub_range.clone();

                let sub_range_utc = ClosedRangeUtc {
                    start: sub_range.start_utc,
                    end: sub_range.end_utc,
                };

                // Load leg A bars.
                let bars_a = match cache.get_or_build(
                    reader,
                    AggParams {
                        symbol: &leg_a.symbol,
                        side: leg_a.side,
                        tf: req.timeframe,
                        range: sub_range_utc,
                    },
                ) {
                    Ok(b) => b,
                    Err(e) => {
                        let msg = format!("cache (leg a): {e}");
                        emit_scan_error(
                            sink,
                            run_id,
                            scan_id_at_version,
                            req,
                            reader.source_id(),
                            &msg,
                        )?;
                        summary.scan_errors += 1;
                        summary
                            .per_scan
                            .entry(scan_id_at_version.to_string())
                            .or_default()
                            .errors += 1;
                        emit_run_end(sink, run_id, started, summary)?;
                        return Ok(RunOutcome::HadScanErrors);
                    }
                };

                // Load leg B bars.
                let bars_b = match cache.get_or_build(
                    reader,
                    AggParams {
                        symbol: &leg_b.symbol,
                        side: leg_b.side,
                        tf: req.timeframe,
                        range: sub_range_utc,
                    },
                ) {
                    Ok(b) => b,
                    Err(e) => {
                        let msg = format!("cache (leg b): {e}");
                        emit_scan_error(
                            sink,
                            run_id,
                            scan_id_at_version,
                            req,
                            reader.source_id(),
                            &msg,
                        )?;
                        summary.scan_errors += 1;
                        summary
                            .per_scan
                            .entry(scan_id_at_version.to_string())
                            .or_default()
                            .errors += 1;
                        emit_run_end(sink, run_id, started, summary)?;
                        return Ok(RunOutcome::HadScanErrors);
                    }
                };

                let ctx_gap_manifest: Option<&crate::gap::GapManifest> =
                    if matches!(req.gap_policy, GapPolicyKind::ContinuousOnly) {
                        Some(&joint_manifest)
                    } else {
                        None
                    };
                let ctx = make_scan_ctx(
                    &bars_a,
                    Some((&bars_a, &bars_b)),
                    ctx_gap_manifest,
                    run_id,
                    crate::CODE_REVISION,
                    Arc::clone(&cancel),
                    req,
                );

                // Plan 05-03 continuation: same buffering-sink wrapping
                // for the Pair-arity dispatch. Pair-arity hygiene
                // dispatch (joint per-leg resampling) is DEFERRED to
                // Phase 7 — `apply_hygiene_mutations` returns the
                // envelope unchanged for Pair-arity scan ids (the
                // hygiene_dispatch table returns None for them).
                let hygiene_active =
                    per_sub_req.bootstrap_method.is_some() || per_sub_req.null_method.is_some();
                let dispatch_result = {
                    let mut buf_sink = HygieneBufferingSink::new(sink, hygiene_active);
                    let scan_res = scan.run(&ctx, &per_sub_req, &mut buf_sink);
                    let (buffered, inner) = buf_sink.into_parts();
                    (scan_res, buffered, inner)
                };
                match dispatch_result.0 {
                    Ok(()) => {
                        let inner = dispatch_result.2;
                        let buffered = dispatch_result.1;
                        if hygiene_active {
                            for raw_result in buffered {
                                let mutated = apply_hygiene_mutations(
                                    raw_result,
                                    &per_sub_req,
                                    &bars_a.close,
                                    &cancel,
                                );
                                inner.write_envelope(&Finding::Result(mutated))?;
                                summary.results_emitted += 1;
                                summary
                                    .per_scan
                                    .entry(scan_id_at_version.to_string())
                                    .or_default()
                                    .results += 1;
                            }
                        } else {
                            summary.results_emitted += 1;
                            summary
                                .per_scan
                                .entry(scan_id_at_version.to_string())
                                .or_default()
                                .results += 1;
                        }
                    }
                    Err(ScanError::Cancelled) => break,
                    Err(ScanError::Kernel(msg)) => {
                        summary.scan_errors += 1;
                        summary
                            .per_scan
                            .entry(scan_id_at_version.to_string())
                            .or_default()
                            .errors += 1;
                        emit_scan_error(
                            sink,
                            run_id,
                            scan_id_at_version,
                            req,
                            reader.source_id(),
                            &msg,
                        )?;
                    }
                    Err(ScanError::Io(e)) => {
                        let msg = format!("scan io: {e}");
                        emit_scan_error(
                            sink,
                            run_id,
                            scan_id_at_version,
                            req,
                            reader.source_id(),
                            &msg,
                        )?;
                        summary.scan_errors += 1;
                        summary
                            .per_scan
                            .entry(scan_id_at_version.to_string())
                            .or_default()
                            .errors += 1;
                        emit_run_end(sink, run_id, started, summary)?;
                        return Ok(RunOutcome::HadScanErrors);
                    }
                    Err(ScanError::Miner(e)) => {
                        let msg = format!("scan miner-error: {e}");
                        emit_scan_error(
                            sink,
                            run_id,
                            scan_id_at_version,
                            req,
                            reader.source_id(),
                            &msg,
                        )?;
                        summary.scan_errors += 1;
                        summary
                            .per_scan
                            .entry(scan_id_at_version.to_string())
                            .or_default()
                            .errors += 1;
                        emit_run_end(sink, run_id, started, summary)?;
                        return Ok(RunOutcome::HadScanErrors);
                    }
                }
            }
        }
    }

    // Step 7/8 — framing-close + RunOutcome map.
    let had_errors = summary.scan_errors > 0;
    emit_run_end(sink, run_id, started, summary)?;
    if had_errors {
        Ok(RunOutcome::HadScanErrors)
    } else {
        Ok(RunOutcome::Ok)
    }
}

// ---------------------------------------------------------------------------
// Hygiene-pass helpers (Plan 05-03 continuation / HYG-03 + HYG-04 + HYG-05).
//
// Cap per T-05-03-V5: bootstrap_n / null_n clamp ceiling.
// ---------------------------------------------------------------------------

/// Cap per T-05-03-V5: `bootstrap_n` / `null_n` clamp ceiling.
pub(crate) const HYGIENE_RESAMPLE_CEILING: u32 = 100_000;
/// Default resample count when the caller passes `Some(method)` with
/// `None` (or 0) for the count.
pub(crate) const HYGIENE_RESAMPLE_DEFAULT: u32 = 1000;

/// Clamp a user-supplied resample count to `HYGIENE_RESAMPLE_CEILING`.
/// `None` and `Some(0)` both fall back to `HYGIENE_RESAMPLE_DEFAULT`.
#[inline]
fn clamp_resample_n(n: Option<u32>) -> u32 {
    let raw = n.unwrap_or(HYGIENE_RESAMPLE_DEFAULT);
    let raw = if raw == 0 { HYGIENE_RESAMPLE_DEFAULT } else { raw };
    raw.min(HYGIENE_RESAMPLE_CEILING)
}

/// Apply post-`Scan::run` hygiene kernels (bootstrap CI + null p-value
/// replacement) to a buffered `ResultFinding`, populating
/// `effect.ci95`, possibly replacing `effect.p_value`, and attaching
/// `repro` per HYG-05.
///
/// Returns the mutated `ResultFinding`. When neither bootstrap nor null
/// was requested OR the scan is not wired into the dispatch table, the
/// envelope is returned unchanged (the engine treats this as "no
/// hygiene work to do" for that scan — Plan 05-03 continuation SUMMARY
/// documents the unwired-scan gap).
///
/// `close` is the borrowed close-price column from the scan's leg-A bars
/// (Single-arity uses leg A only; Pair-arity passes leg A by the same
/// contract — the dispatch closure ignores it for the Pair-arity case
/// until Phase 7 lands joint resampling).
#[allow(
    clippy::too_many_lines,
    reason = "linear post-Scan::run hygiene walk: clamp + derive seed + bootstrap-arm + cancel-poll + null-arm + repro-attach. Splitting the linear sequence into 5+ helpers obscures the cancel-cadence interleave that RESEARCH Pitfall 7 pins."
)]
fn apply_hygiene_mutations(
    mut result: ResultFinding,
    req: &ScanRequest,
    close: &[f64],
    cancel: &Arc<AtomicBool>,
) -> ResultFinding {
    // Early exit: no hygiene requested.
    if req.bootstrap_method.is_none() && req.null_method.is_none() {
        return result;
    }
    // RESEARCH Pitfall 7 — outer-engine cadence: poll cancel BEFORE the
    // hygiene step. The kernels themselves run uninterruptibly (small n).
    if cancel.load(Ordering::Relaxed) {
        return result;
    }

    let scan_id_at_version = result.scan_id_at_version.clone();
    let Some(values) = hygiene_dispatch::input_series_for(&scan_id_at_version, close) else {
        // Scan not wired into the dispatch table — leave the envelope
        // untouched. The preflight (`validate_hygiene_support`) only
        // gated `supports_*`, so the engine quietly no-ops on
        // unwired-but-opted-in scans (Plan 05-03 continuation
        // SUMMARY documents this gap).
        return result;
    };
    if values.len() < 2 {
        return result;
    }

    // Derive (or accept) the job seed.
    let master_seed = req.master_seed.unwrap_or(0);
    let job_seed = req.job_seed.unwrap_or_else(|| {
        hygiene_seed::derive_job_seed(
            master_seed,
            &scan_id_at_version,
            &req.instruments,
            req.timeframe,
            &req.window,
            req.param_hash.as_str(),
        )
    });

    let stat = hygiene_dispatch::stat_closure_for(&scan_id_at_version, req);

    // Bootstrap CI population.
    let bootstrap_spec = if let Some(method) = req.bootstrap_method {
        let n = clamp_resample_n(req.bootstrap_n);
        if let Some(ref stat_fn) = stat {
            let ci = match method {
                BootstrapMethod::Stationary => {
                    let block_len_raw = hygiene_bootstrap::block_length_pwppw(&values);
                    let block_len = if block_len_raw.is_finite() && block_len_raw > 0.0 {
                        block_len_raw.max(3.0)
                    } else {
                        3.0
                    };
                    hygiene_bootstrap::stationary_bootstrap_ci(
                        &values, stat_fn, n, block_len, job_seed, 0.95,
                    )
                }
                BootstrapMethod::Block => {
                    let block_len_raw = hygiene_bootstrap::block_length_pwppw(&values);
                    let block_len_usize = if block_len_raw.is_finite() && block_len_raw > 0.0 {
                        #[allow(
                            clippy::cast_possible_truncation,
                            clippy::cast_sign_loss,
                            reason = "block_length_pwppw output is bounded by the values length"
                        )]
                        let v = block_len_raw.ceil() as usize;
                        v.max(3)
                    } else {
                        3
                    };
                    hygiene_bootstrap::block_bootstrap_ci(
                        &values,
                        stat_fn,
                        n,
                        block_len_usize,
                        job_seed,
                        0.95,
                    )
                }
            };
            if ci[0].is_finite() && ci[1].is_finite() {
                result.effect.ci95 = Some(ci);
            }
        }
        Some(BootstrapSpec {
            method: method.as_str().to_string(),
            n,
        })
    } else {
        None
    };

    // Cancel poll between bootstrap and null (cadence inside the
    // hygiene step, per RESEARCH Pitfall 7).
    if cancel.load(Ordering::Relaxed) {
        // Still attach repro for whatever ran.
        result.repro = Some(ReproEnvelope {
            master_seed,
            job_seed,
            bootstrap: bootstrap_spec,
            null: None,
        });
        return result;
    }

    // Null p-value replacement.
    let null_spec = if let Some(method) = req.null_method {
        let n = clamp_resample_n(req.null_n);
        if let Some(ref stat_fn) = stat {
            // Use the observed Q-stat (Effect.value) as the reference
            // statistic so the empirical p computation matches the
            // analytic distribution's tail-comparison semantics.
            let observed_stat = result.effect.value;
            let p = match method {
                NullMethod::CircularShift => {
                    hygiene_null::circular_shift_null_p(&values, observed_stat, stat_fn, n, job_seed)
                }
                NullMethod::PhaseScramble => {
                    // IAAFT phase-scramble defers to Phase 7 per Plan
                    // 05-02 SUMMARY; preflight (validate_hygiene_support)
                    // rejects PhaseScramble on every scan whose
                    // `supports_null_method(PhaseScramble)` is false, but
                    // belt-and-braces: the kernel is unavailable, so
                    // skip the mutation and let the scan keep its
                    // analytic p-value. The repro envelope still echoes
                    // the requested method so the consumer sees what
                    // the engine attempted.
                    f64::NAN
                }
            };
            if p.is_finite() && (0.0..=1.0).contains(&p) {
                result.effect.p_value = Some(p);
            }
        }
        Some(NullSpec {
            method: method.as_str().to_string(),
            n,
        })
    } else {
        None
    };

    if bootstrap_spec.is_some() || null_spec.is_some() {
        result.repro = Some(ReproEnvelope {
            master_seed,
            job_seed,
            bootstrap: bootstrap_spec,
            null: null_spec,
        });
    }
    result
}

/// Build a [`ScanCtx`] with the cfg-gated `sleep_after_first_finding_ms`
/// field populated from `req` when the test cfg is active. The cfg-gated
/// branches are factored out so the engine code stays clean across release
/// vs test builds (Warning 1 polish).
#[cfg_attr(not(any(test, feature = "test-internal")), allow(unused_variables))]
fn make_scan_ctx<'a>(
    bars: &'a crate::aggregator::BarFrame,
    bars_pair: Option<(
        &'a crate::aggregator::BarFrame,
        &'a crate::aggregator::BarFrame,
    )>,
    gap_manifest: Option<&'a crate::gap::GapManifest>,
    run_id: RunId,
    code_revision: &'a str,
    cancel: Arc<AtomicBool>,
    req: &ScanRequest,
) -> ScanCtx<'a> {
    ScanCtx {
        bars,
        bars_pair,
        gap_manifest,
        run_id,
        code_revision,
        cancel,
        #[cfg(any(test, feature = "test-internal"))]
        sleep_after_first_finding_ms: req.sleep_after_first_finding_ms,
    }
}

/// Emit a `Finding::ScanError` envelope. Helper to keep the dispatch arm
/// short.
fn emit_scan_error(
    sink: &mut dyn FindingSink,
    run_id: RunId,
    scan_id_at_version: &str,
    req: &ScanRequest,
    source_id: &str,
    message: &str,
) -> Result<(), MinerError> {
    use crate::error::ScanErrorCode;
    use crate::findings::ScanErrorFinding;
    // Phase 4 (D4-03): populate `data_slice.sources` from req.instruments.
    let sources: Vec<Source> = req
        .instruments
        .iter()
        .map(|spec| Source {
            source_id: source_id.to_string(),
            symbol: spec.symbol.clone(),
            side: spec.side.as_str().to_string(),
            timeframe: req.timeframe.as_str().to_string(),
        })
        .collect();
    sink.write_envelope(&Finding::ScanError(ScanErrorFinding {
        schema_version: 1,
        scan_id_at_version: scan_id_at_version.to_string(),
        param_hash: req.param_hash.as_str().to_string(),
        code_revision: crate::CODE_REVISION.to_string(),
        data_slice: DataSlice {
            range: req.sub_range.clone(),
            gap_manifest_ref: None,
            gap_manifest: None,
            sources,
        },
        dsr: None,
        fdr_q: None,
        run_id,
        produced_at_utc: Utc::now(),
        error_code: ScanErrorCode::ComputeError.as_str().to_string(),
        message: message.to_string(),
        request_context: serde_json::json!({
            "scan_id@version": scan_id_at_version,
            "source_id": source_id,
        }),
    }))?;
    Ok(())
}

/// Build + emit the `RunEnd` envelope and flush the sink. THE one and only
/// flush point in the pipeline (Pitfall 5).
fn emit_run_end(
    sink: &mut dyn FindingSink,
    run_id: RunId,
    started: chrono::DateTime<Utc>,
    summary: RunSummary,
) -> Result<(), MinerError> {
    let ended = Utc::now();
    let run_end_finding = framing::build_run_end(run_id, started, ended, summary);
    sink.write_envelope(&run_end_finding)?;
    sink.flush()?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests — run_one behavior tests (uses a FakeReader fixture).
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(
    dead_code,
    clippy::similar_names,
    clippy::match_wildcard_for_single_variants,
    clippy::items_after_statements,
    clippy::manual_let_else,
    clippy::doc_markdown
)]
mod tests {
    use super::*;
    use crate::aggregator::Timeframe;
    use crate::calendar::Calendar;
    use crate::config::OutputDest;
    use crate::findings::sink::VecSink;
    use crate::reader::{Blake3Hex, InstrumentSpec, RawBar, RawBarIter, Side};
    use chrono::{DateTime, Duration, NaiveDate, TimeZone};
    use std::collections::BTreeMap;

    // -----------------------------------------------------------------------
    // FakeReader fixture — controllable Reader for engine integration tests.
    //
    // Bars are stored in a BTreeMap<(symbol, side, date), Vec<RawBar>>; days
    // present in the map produce a fingerprint, days absent return None.
    // Optional `force_err` makes fingerprint_day return an io::Error (used by
    // the reader-error wrapping test).
    // -----------------------------------------------------------------------
    pub(super) struct FakeReader {
        pub bars: BTreeMap<(String, Side, NaiveDate), Vec<RawBar>>,
        pub force_err: bool,
        pub calendar: Calendar,
    }

    impl FakeReader {
        pub fn new() -> Self {
            Self {
                bars: BTreeMap::new(),
                force_err: false,
                calendar: Calendar::fx_major(),
            }
        }

        pub fn insert_day(&mut self, symbol: &str, side: Side, date: NaiveDate, bars: Vec<RawBar>) {
            self.bars.insert((symbol.to_string(), side, date), bars);
        }
    }

    impl Reader for FakeReader {
        type Error = std::io::Error;

        fn source_id(&self) -> &'static str {
            "fakereader"
        }

        fn trading_calendar(&self) -> Calendar {
            self.calendar.clone()
        }

        fn read_1m_bars<'a>(
            &'a self,
            symbol: &str,
            side: Side,
            range: ClosedRangeUtc,
        ) -> Result<RawBarIter<'a, Self::Error>, Self::Error> {
            let mut all: Vec<RawBar> = Vec::new();
            for ((sym, sd, _date), bars) in &self.bars {
                if sym != symbol || *sd != side {
                    continue;
                }
                for bar in bars {
                    if bar.ts_open_utc >= range.start && bar.ts_open_utc < range.end {
                        all.push(*bar);
                    }
                }
            }
            Ok(Box::new(all.into_iter().map(Ok)))
        }

        fn fingerprint_day(
            &self,
            symbol: &str,
            side: Side,
            date: NaiveDate,
        ) -> Result<Option<Blake3Hex>, Self::Error> {
            if self.force_err {
                return Err(std::io::Error::other("forced reader error"));
            }
            let key = (symbol.to_string(), side, date);
            if self.bars.contains_key(&key) {
                Ok(Some(Blake3Hex::from_hex_bytes(&[b'0'; 64])))
            } else {
                Ok(None)
            }
        }

        fn enumerate_days(
            &self,
            symbol: &str,
            side: Side,
            range: ClosedRangeUtc,
        ) -> Result<Vec<NaiveDate>, Self::Error> {
            let start_date = range.start.date_naive();
            let end_date = range.end.date_naive();
            let mut out: Vec<NaiveDate> = self
                .bars
                .keys()
                .filter(|(s, sd, _)| s == symbol && *sd == side)
                .map(|(_, _, d)| *d)
                .filter(|d| *d >= start_date && *d <= end_date)
                .collect();
            out.sort_unstable();
            Ok(out)
        }
    }

    // -----------------------------------------------------------------------
    // Builders
    // -----------------------------------------------------------------------

    /// Build 1440 1-minute `RawBar`s for a single full UTC day. Deterministic
    /// price walk so the aggregated bars are well-formed and the scan can
    /// compute meaningful Q-stats.
    fn build_full_day_1m_bars(date: NaiveDate, seed: u32) -> Vec<RawBar> {
        let day_start = date.and_hms_opt(0, 0, 0).expect("00:00:00 valid").and_utc();
        let mut bars = Vec::with_capacity(1440);
        let mut s = seed;
        for i in 0..1440_i64 {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let frac = f64::from(s) / f64::from(u32::MAX);
            let close = 1.0 + frac * 0.01;
            let ts_open = day_start + Duration::minutes(i);
            let ts_close = ts_open + Duration::minutes(1);
            bars.push(RawBar {
                ts_open_utc: ts_open,
                ts_close_utc: ts_close,
                open: close - 0.0001,
                high: close + 0.0001,
                low: close - 0.0002,
                close,
                tick_volume: 1.0,
            });
        }
        bars
    }

    /// Build 1-minute bars for a day with a "hole" — minutes in
    /// `hole_minutes` (relative offsets from midnight) are OMITTED. Used to
    /// force `GapDetector::detect` to emit intra-day gap spans.
    fn build_day_1m_bars_with_hole(
        date: NaiveDate,
        seed: u32,
        hole_minutes: std::ops::Range<i64>,
    ) -> Vec<RawBar> {
        let all = build_full_day_1m_bars(date, seed);
        all.into_iter()
            .enumerate()
            .filter(|(i, _)| {
                let i = i64::try_from(*i).unwrap();
                !hole_minutes.contains(&i)
            })
            .map(|(_, b)| b)
            .collect()
    }

    fn blake3_hex_zero() -> Blake3Hex {
        let bytes: [u8; 64] = [b'0'; 64];
        Blake3Hex::from_hex_bytes(&bytes)
    }

    fn sample_request(
        instrument: &str,
        window_start: DateTime<Utc>,
        window_end: DateTime<Utc>,
        gap_policy: GapPolicyKind,
        dry_run: bool,
    ) -> ScanRequest {
        let resolved = serde_json::json!({"lags": 5});
        let param_hash = param_hash::param_hash(&resolved).expect("param_hash ok");
        ScanRequest {
            scan_id: "stats.autocorr.ljung_box".into(),
            version: 1,
            instruments: vec![InstrumentSpec {
                symbol: instrument.into(),
                side: Side::Bid,
            }],
            timeframe: Timeframe::Tf15m,
            window: ClosedRangeUtc {
                start: window_start,
                end: window_end,
            },
            sub_range: TimeRange {
                start_utc: window_start,
                end_utc: window_end,
            },
            gap_policy,
            resolved_params: resolved,
            param_hash,
            dry_run,
        master_seed: None,
        job_seed: None,
        bootstrap_method: None,
        bootstrap_n: None,
        null_method: None,
        null_n: None,
            sleep_after_first_finding_ms: None,
        }
    }

    /// Build a `MinerConfig` whose `bar_cache_root` lives in `tmp`.
    fn tmp_config(tmp: &tempfile::TempDir) -> MinerConfig {
        MinerConfig {
            cache_root: tmp.path().join("cache"),
            bar_cache_root: tmp.path().join("bar-cache"),
            output: OutputDest::Stdout,
        }
    }

    fn parse_findings(sink: &VecSink) -> Vec<Finding> {
        sink.0
            .split(|b| *b == b'\n')
            .filter(|l| !l.is_empty())
            .map(|l| serde_json::from_slice::<Finding>(l).expect("parse Finding"))
            .collect()
    }

    fn count_envelopes(findings: &[Finding]) -> (usize, usize, usize, usize, usize, usize) {
        let mut start = 0;
        let mut result = 0;
        let mut scan_err = 0;
        let mut gap_aborted = 0;
        let mut end = 0;
        let mut dry_run = 0;
        for f in findings {
            match f {
                Finding::RunStart(_) => start += 1,
                Finding::Result(_) => result += 1,
                Finding::ScanError(_) => scan_err += 1,
                Finding::GapAborted(_) => gap_aborted += 1,
                Finding::RunEnd(_) => end += 1,
                Finding::DryRun(_) => dry_run += 1,
                // Phase 5 (Plan 05-01 / D5-02): `Finding::SweepSummary` is
                // emitted only by the sweep runner (Plan 05-04) — the
                // single-run engine tests in this module never produce it,
                // so the test helper does not increment any counter on it.
                // Variant must still be matched exhaustively (E0004).
                Finding::SweepSummary(_) => {}
            }
        }
        (start, result, scan_err, gap_aborted, end, dry_run)
    }

    fn get_run_end(findings: &[Finding]) -> &crate::findings::RunEnd {
        findings
            .iter()
            .find_map(|f| match f {
                Finding::RunEnd(re) => Some(re),
                _ => None,
            })
            .expect("RunEnd present")
    }

    // -----------------------------------------------------------------------
    // Tests
    // -----------------------------------------------------------------------

    #[test]
    fn run_one_preflight_unknown_scan() {
        let mut req = sample_request(
            "EURUSD",
            Utc.with_ymd_and_hms(2024, 1, 2, 0, 0, 0).unwrap(),
            Utc.with_ymd_and_hms(2024, 1, 3, 0, 0, 0).unwrap(),
            GapPolicyKind::ContinuousOnly,
            false,
        );
        req.scan_id = "nope".into();
        let tmp = tempfile::TempDir::new().unwrap();
        let cfg = tmp_config(&tmp);
        let reader = FakeReader::new();
        let mut sink = VecSink::new();
        let cancel = Arc::new(AtomicBool::new(false));
        let res = run_one(&req, &cfg, &reader, &mut sink, cancel);
        // Plan 03-07 CR-03: typed `MinerError::Preflight(WireError)` is now
        // the preflight unknown-scan error. The wire-form carries
        // `PreflightCode::UnknownScan` directly — no string-match dispatch
        // in the CLI.
        match res {
            Err(MinerError::Preflight(w)) => {
                assert_eq!(
                    w.code, "unknown_scan",
                    "preflight WireError must carry PreflightCode::UnknownScan"
                );
                assert!(
                    w.message.contains("nope") || w.message.contains("no such scan"),
                    "WireError message must surface the offending scan id; got {:?}",
                    w.message
                );
            }
            other => panic!("expected MinerError::Preflight(_); got {other:?}"),
        }
        // Stdout (sink) stays empty: no RunStart, no Result, no RunEnd.
        assert!(sink.0.is_empty(), "no envelope on preflight failure");
    }

    #[test]
    fn run_one_strict_zero_gaps() {
        let start = Utc.with_ymd_and_hms(2024, 1, 2, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2024, 1, 3, 0, 0, 0).unwrap();
        let req = sample_request("EURUSD", start, end, GapPolicyKind::Strict, false);
        let mut reader = FakeReader::new();
        reader.insert_day(
            "EURUSD",
            Side::Bid,
            NaiveDate::from_ymd_opt(2024, 1, 2).unwrap(),
            build_full_day_1m_bars(NaiveDate::from_ymd_opt(2024, 1, 2).unwrap(), 1),
        );
        let tmp = tempfile::TempDir::new().unwrap();
        let cfg = tmp_config(&tmp);
        let mut sink = VecSink::new();
        let cancel = Arc::new(AtomicBool::new(false));
        let outcome = run_one(&req, &cfg, &reader, &mut sink, cancel).expect("ok");
        assert_eq!(outcome, RunOutcome::Ok);
        let findings = parse_findings(&sink);
        let (start_n, result_n, _, gap_n, end_n, dry_n) = count_envelopes(&findings);
        assert_eq!(start_n, 1);
        assert_eq!(result_n, 1, "strict+zero-gaps emits one Result");
        assert_eq!(gap_n, 0);
        assert_eq!(end_n, 1);
        assert_eq!(dry_n, 0);
        // Strict success-path: data_slice.gap_manifest = None (D3-12).
        if let Finding::Result(r) = &findings[1] {
            assert!(
                r.data_slice.gap_manifest.is_none(),
                "Strict success path: gap_manifest must be None"
            );
        }
        let re = get_run_end(&findings);
        assert_eq!(re.summary.results_emitted, 1);
        assert_eq!(re.summary.gap_aborted, 0);
    }

    #[test]
    fn run_one_strict_with_gaps() {
        let start = Utc.with_ymd_and_hms(2024, 1, 2, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2024, 1, 3, 0, 0, 0).unwrap();
        let req = sample_request("EURUSD", start, end, GapPolicyKind::Strict, false);
        // Day has a 10-minute hole at minutes 60..70 -> intra-day gap.
        let mut reader = FakeReader::new();
        reader.insert_day(
            "EURUSD",
            Side::Bid,
            NaiveDate::from_ymd_opt(2024, 1, 2).unwrap(),
            build_day_1m_bars_with_hole(NaiveDate::from_ymd_opt(2024, 1, 2).unwrap(), 1, 60..70),
        );
        let tmp = tempfile::TempDir::new().unwrap();
        let cfg = tmp_config(&tmp);
        let mut sink = VecSink::new();
        let cancel = Arc::new(AtomicBool::new(false));
        let outcome = run_one(&req, &cfg, &reader, &mut sink, cancel).expect("ok");
        assert_eq!(outcome, RunOutcome::Ok);
        let findings = parse_findings(&sink);
        let (start_n, result_n, _, gap_n, end_n, _) = count_envelopes(&findings);
        assert_eq!(start_n, 1);
        assert_eq!(result_n, 0, "Strict with gaps emits zero Results");
        assert_eq!(gap_n, 1, "Strict with gaps emits one GapAborted");
        assert_eq!(end_n, 1);
        let re = get_run_end(&findings);
        assert_eq!(re.summary.results_emitted, 0);
        assert_eq!(re.summary.gap_aborted, 1);
        // The GapAborted carries the full manifest with the gap.
        if let Finding::GapAborted(ga) = &findings[1] {
            let m = ga
                .data_slice
                .gap_manifest
                .as_ref()
                .expect("manifest present");
            assert!(!m.gaps.is_empty(), "manifest must carry the gap");
        }
    }

    #[test]
    fn run_one_continuous_only_zero_gaps() {
        let start = Utc.with_ymd_and_hms(2024, 1, 2, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2024, 1, 3, 0, 0, 0).unwrap();
        let req = sample_request("EURUSD", start, end, GapPolicyKind::ContinuousOnly, false);
        let mut reader = FakeReader::new();
        reader.insert_day(
            "EURUSD",
            Side::Bid,
            NaiveDate::from_ymd_opt(2024, 1, 2).unwrap(),
            build_full_day_1m_bars(NaiveDate::from_ymd_opt(2024, 1, 2).unwrap(), 1),
        );
        let tmp = tempfile::TempDir::new().unwrap();
        let cfg = tmp_config(&tmp);
        let mut sink = VecSink::new();
        let outcome = run_one(
            &req,
            &cfg,
            &reader,
            &mut sink,
            Arc::new(AtomicBool::new(false)),
        )
        .expect("ok");
        assert_eq!(outcome, RunOutcome::Ok);
        let findings = parse_findings(&sink);
        let (s, r, _, _, e, _) = count_envelopes(&findings);
        assert_eq!((s, r, e), (1, 1, 1));
        // ContinuousOnly fast path: data_slice.gap_manifest is Some(empty
        // manifest) per CONTEXT D3-12.
        if let Finding::Result(r) = &findings[1] {
            let m = r
                .data_slice
                .gap_manifest
                .as_ref()
                .expect("ContinuousOnly fast path provides Some(empty manifest)");
            assert!(m.gaps.is_empty(), "manifest gaps must be empty");
        }
    }

    #[test]
    fn run_one_continuous_only_with_gaps() {
        // Two-day window with an intra-day hole at minutes 1200..1215 ->
        // two Tf15m-aligned sub-ranges. ContinuousOnly emits ONE Result per
        // sub-range with the FULL manifest cloned into each Result.
        let day1 = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap();
        let day2 = NaiveDate::from_ymd_opt(2024, 1, 3).unwrap();
        let start = day1.and_hms_opt(0, 0, 0).unwrap().and_utc();
        let end = day2.and_hms_opt(0, 0, 0).unwrap().and_utc() + Duration::days(1);
        let req = sample_request("EURUSD", start, end, GapPolicyKind::ContinuousOnly, false);

        let mut reader = FakeReader::new();
        let day1_bars: Vec<RawBar> = build_full_day_1m_bars(day1, 1)
            .into_iter()
            .enumerate()
            .filter(|(i, _)| !(1200..1215).contains(&i64::try_from(*i).unwrap()))
            .map(|(_, b)| b)
            .collect();
        reader.insert_day("EURUSD", Side::Bid, day1, day1_bars);
        reader.insert_day("EURUSD", Side::Bid, day2, build_full_day_1m_bars(day2, 2));
        let tmp = tempfile::TempDir::new().unwrap();
        let cfg = tmp_config(&tmp);
        let mut sink = VecSink::new();
        let outcome = run_one(
            &req,
            &cfg,
            &reader,
            &mut sink,
            Arc::new(AtomicBool::new(false)),
        )
        .expect("ok");
        assert_eq!(outcome, RunOutcome::Ok);
        let findings = parse_findings(&sink);
        let (s_n, r_n, _, g_n, e_n, _) = count_envelopes(&findings);
        assert_eq!(s_n, 1);
        assert_eq!(
            r_n, 2,
            "ContinuousOnly + 1 gap -> 2 sub-ranges -> 2 Results"
        );
        assert_eq!(g_n, 0);
        assert_eq!(e_n, 1);
        // Each Result carries the FULL manifest cloned into data_slice.
        for f in &findings {
            if let Finding::Result(r) = f {
                let m = r
                    .data_slice
                    .gap_manifest
                    .as_ref()
                    .expect("ContinuousOnly inlines manifest into every Result");
                assert!(!m.gaps.is_empty(), "manifest must carry the gap");
            }
        }
        let re = get_run_end(&findings);
        assert_eq!(re.summary.results_emitted, 2);
    }

    #[test]
    fn run_one_dry_run() {
        let start = Utc.with_ymd_and_hms(2024, 1, 2, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2024, 1, 3, 0, 0, 0).unwrap();
        let req = sample_request("EURUSD", start, end, GapPolicyKind::ContinuousOnly, true);
        // FakeReader can be empty — dry-run never touches the reader.
        let reader = FakeReader::new();
        let tmp = tempfile::TempDir::new().unwrap();
        let cfg = tmp_config(&tmp);
        let mut sink = VecSink::new();
        let outcome = run_one(
            &req,
            &cfg,
            &reader,
            &mut sink,
            Arc::new(AtomicBool::new(false)),
        )
        .expect("ok");
        assert_eq!(outcome, RunOutcome::Ok);
        let findings = parse_findings(&sink);
        let (s, r, _, _, e, dry_n) = count_envelopes(&findings);
        assert_eq!((s, r, e, dry_n), (1, 0, 1, 1));
        let re = get_run_end(&findings);
        assert_eq!(
            re.summary.results_emitted, 0,
            "Pitfall 3 — dry-run does NOT increment results_emitted"
        );
        // Warning 9 pin: the raw JSON envelope contains no extension counter
        // for the dry-run signal — RunSummary has only the original four
        // fields, and the JSONL output reflects that exactly. The literal
        // identifier string is built from a constant so the file-level grep
        // gate (Plan 04 acceptance) sees no inline occurrence.
        const BANNED_COUNTER: &str = concat!("\"dry_run_", "emitted\"");
        let raw = String::from_utf8(sink.0.clone()).unwrap();
        assert!(
            !raw.contains(BANNED_COUNTER),
            "RunSummary must NOT carry the banned extension counter (Warning 9 — see BANNED_COUNTER)"
        );
    }

    #[test]
    fn run_one_run_start_request_carries_dry_run() {
        let start = Utc.with_ymd_and_hms(2024, 1, 2, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2024, 1, 3, 0, 0, 0).unwrap();
        let req = sample_request("EURUSD", start, end, GapPolicyKind::ContinuousOnly, true);
        let reader = FakeReader::new();
        let tmp = tempfile::TempDir::new().unwrap();
        let cfg = tmp_config(&tmp);
        let mut sink = VecSink::new();
        run_one(
            &req,
            &cfg,
            &reader,
            &mut sink,
            Arc::new(AtomicBool::new(false)),
        )
        .expect("ok");
        let findings = parse_findings(&sink);
        let Finding::RunStart(rs) = &findings[0] else {
            panic!("expected RunStart first");
        };
        assert_eq!(
            rs.request.get("dry_run"),
            Some(&serde_json::Value::Bool(true))
        );
    }

    #[test]
    fn run_one_param_hash_in_result() {
        let start = Utc.with_ymd_and_hms(2024, 1, 2, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2024, 1, 3, 0, 0, 0).unwrap();
        let req = sample_request("EURUSD", start, end, GapPolicyKind::ContinuousOnly, false);
        let mut reader = FakeReader::new();
        reader.insert_day(
            "EURUSD",
            Side::Bid,
            NaiveDate::from_ymd_opt(2024, 1, 2).unwrap(),
            build_full_day_1m_bars(NaiveDate::from_ymd_opt(2024, 1, 2).unwrap(), 1),
        );
        let tmp = tempfile::TempDir::new().unwrap();
        let cfg = tmp_config(&tmp);
        let mut sink = VecSink::new();
        run_one(
            &req,
            &cfg,
            &reader,
            &mut sink,
            Arc::new(AtomicBool::new(false)),
        )
        .expect("ok");
        let findings = parse_findings(&sink);
        let Finding::Result(r) = findings
            .iter()
            .find(|f| matches!(f, Finding::Result(_)))
            .expect("Result present")
        else {
            unreachable!()
        };
        // Compute the expected param_hash independently.
        let expected = param_hash::param_hash(&req.resolved_params)
            .expect("hash ok")
            .as_str()
            .to_string();
        assert_eq!(r.param_hash, expected);
    }

    #[test]
    fn run_one_run_id_consistency() {
        let start = Utc.with_ymd_and_hms(2024, 1, 2, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2024, 1, 3, 0, 0, 0).unwrap();
        let req = sample_request("EURUSD", start, end, GapPolicyKind::ContinuousOnly, false);
        let mut reader = FakeReader::new();
        reader.insert_day(
            "EURUSD",
            Side::Bid,
            NaiveDate::from_ymd_opt(2024, 1, 2).unwrap(),
            build_full_day_1m_bars(NaiveDate::from_ymd_opt(2024, 1, 2).unwrap(), 1),
        );
        let tmp = tempfile::TempDir::new().unwrap();
        let cfg = tmp_config(&tmp);
        let mut sink = VecSink::new();
        run_one(
            &req,
            &cfg,
            &reader,
            &mut sink,
            Arc::new(AtomicBool::new(false)),
        )
        .expect("ok");
        let findings = parse_findings(&sink);
        let rs_id = if let Finding::RunStart(rs) = &findings[0] {
            rs.run_id
        } else {
            panic!("expected RunStart")
        };
        let re_id = if let Finding::RunEnd(re) = findings
            .iter()
            .rev()
            .find(|f| matches!(f, Finding::RunEnd(_)))
            .unwrap()
        {
            re.run_id
        } else {
            unreachable!()
        };
        assert_eq!(
            rs_id, re_id,
            "run_id must be consistent across RunStart/RunEnd"
        );
        for f in &findings {
            if let Finding::Result(r) = f {
                assert_eq!(r.run_id, rs_id, "every Result.run_id must match");
            }
        }
    }

    #[test]
    fn run_one_clock_isolation() {
        let start = Utc.with_ymd_and_hms(2024, 1, 2, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2024, 1, 3, 0, 0, 0).unwrap();
        let req = sample_request("EURUSD", start, end, GapPolicyKind::ContinuousOnly, false);
        let mut reader = FakeReader::new();
        reader.insert_day(
            "EURUSD",
            Side::Bid,
            NaiveDate::from_ymd_opt(2024, 1, 2).unwrap(),
            build_full_day_1m_bars(NaiveDate::from_ymd_opt(2024, 1, 2).unwrap(), 1),
        );
        let tmp = tempfile::TempDir::new().unwrap();
        let cfg = tmp_config(&tmp);
        let mut sink = VecSink::new();
        run_one(
            &req,
            &cfg,
            &reader,
            &mut sink,
            Arc::new(AtomicBool::new(false)),
        )
        .expect("ok");
        let findings = parse_findings(&sink);
        let rs = if let Finding::RunStart(rs) = &findings[0] {
            rs
        } else {
            panic!("expected RunStart")
        };
        let re = get_run_end(&findings);
        // RunStart.started_at_utc <= every Result.produced_at_utc <= RunEnd.ended_at_utc.
        for f in &findings {
            if let Finding::Result(r) = f {
                assert!(rs.started_at_utc <= r.produced_at_utc);
                assert!(r.produced_at_utc <= re.ended_at_utc);
            }
        }
    }

    /// Plan 03-07 CR-01 regression — on reader error during gap detection,
    /// the engine MUST emit `Finding::ScanError` + `Finding::RunEnd` before
    /// returning `Ok(RunOutcome::HadScanErrors)`. The previous contract
    /// returned `Err(MinerError::Scan(_))` and left an orphaned `RunStart`
    /// envelope on stdout (D-09 / OUT-04 framing invariant violation). The
    /// previous reader-error wraps-via-MinerError-Scan test name is
    /// deliberately gone — see CR-01 closure notes in 03-07-PLAN.md.
    #[test]
    fn run_one_reader_error_emits_run_start_and_run_end_with_scan_error() {
        let start = Utc.with_ymd_and_hms(2024, 1, 2, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2024, 1, 3, 0, 0, 0).unwrap();
        let req = sample_request("EURUSD", start, end, GapPolicyKind::ContinuousOnly, false);
        let mut reader = FakeReader::new();
        reader.force_err = true;
        // Insert a day so enumerate_dates has something; fingerprint_day will
        // force an error.
        reader.insert_day(
            "EURUSD",
            Side::Bid,
            NaiveDate::from_ymd_opt(2024, 1, 2).unwrap(),
            Vec::new(),
        );
        let tmp = tempfile::TempDir::new().unwrap();
        let cfg = tmp_config(&tmp);
        let mut sink = VecSink::new();
        let outcome = run_one(
            &req,
            &cfg,
            &reader,
            &mut sink,
            Arc::new(AtomicBool::new(false)),
        )
        .expect("ok — error path now returns Ok(HadScanErrors), not Err");
        assert_eq!(outcome, RunOutcome::HadScanErrors);

        let findings = parse_findings(&sink);
        assert!(
            findings.len() >= 3,
            "expected at least RunStart + ScanError + RunEnd; got {} findings",
            findings.len()
        );
        // CR-01 pin: RunStart present at the head.
        assert!(
            matches!(findings.first(), Some(Finding::RunStart(_))),
            "first envelope must be RunStart; got {:?}",
            findings.first()
        );
        // CR-01 pin: RunEnd present at the tail — closes the framing pair
        // that the orphan-RunStart bug used to leave open.
        assert!(
            matches!(findings.last(), Some(Finding::RunEnd(_))),
            "last envelope must be RunEnd; got {:?}",
            findings.last()
        );
        // A ScanError envelope with the reader-context message is emitted.
        let scan_err = findings
            .iter()
            .find_map(|f| match f {
                Finding::ScanError(se) => Some(se),
                _ => None,
            })
            .expect("ScanError envelope present");
        assert_eq!(
            scan_err.error_code, "compute_error",
            "wrapped reader-error uses ScanErrorCode::ComputeError; got {:?}",
            scan_err.error_code
        );
        assert!(
            scan_err.message.contains("reader") || scan_err.message.contains("forced"),
            "ScanError message must surface the reader context; got {:?}",
            scan_err.message
        );
        // RunSummary.scan_errors >= 1.
        let re = get_run_end(&findings);
        assert!(
            re.summary.scan_errors >= 1,
            "RunEnd.summary.scan_errors must reflect the wrapped error; got {}",
            re.summary.scan_errors
        );
    }

    /// Plan 03-07 CR-01 regression — cache-error arm. Forces
    /// `BarCache::get_or_build` to fail by pointing `cfg.bar_cache_root` at
    /// a regular file (not a directory); subsequent `create_dir_all` under
    /// it fails with "Not a directory" / similar `io::Error`.
    #[test]
    fn run_one_cache_error_emits_run_start_and_run_end_with_scan_error() {
        let start = Utc.with_ymd_and_hms(2024, 1, 2, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2024, 1, 3, 0, 0, 0).unwrap();
        let req = sample_request("EURUSD", start, end, GapPolicyKind::ContinuousOnly, false);
        // Happy-path reader so step 5 (gap detection) succeeds — the test
        // exercises step 6's cache-load arm.
        let mut reader = FakeReader::new();
        reader.insert_day(
            "EURUSD",
            Side::Bid,
            NaiveDate::from_ymd_opt(2024, 1, 2).unwrap(),
            build_full_day_1m_bars(NaiveDate::from_ymd_opt(2024, 1, 2).unwrap(), 1),
        );
        let tmp = tempfile::TempDir::new().unwrap();
        let mut cfg = tmp_config(&tmp);
        // Replace `bar_cache_root` with a regular file so `create_dir_all`
        // under it fails — a portable way to force cache::get_or_build to
        // return Err(CacheError::Io).
        let bar_cache_path = tmp.path().join("bar-cache-as-file");
        std::fs::write(&bar_cache_path, b"not a dir").expect("write file");
        cfg.bar_cache_root = bar_cache_path;

        let mut sink = VecSink::new();
        let outcome = run_one(
            &req,
            &cfg,
            &reader,
            &mut sink,
            Arc::new(AtomicBool::new(false)),
        )
        .expect("ok — cache-error path returns Ok(HadScanErrors)");
        assert_eq!(outcome, RunOutcome::HadScanErrors);

        let findings = parse_findings(&sink);
        assert!(
            matches!(findings.first(), Some(Finding::RunStart(_))),
            "first envelope must be RunStart"
        );
        assert!(
            matches!(findings.last(), Some(Finding::RunEnd(_))),
            "last envelope must be RunEnd"
        );
        let scan_err = findings
            .iter()
            .find_map(|f| match f {
                Finding::ScanError(se) => Some(se),
                _ => None,
            })
            .expect("ScanError envelope present");
        assert!(
            scan_err.message.contains("cache"),
            "ScanError message must surface the cache context; got {:?}",
            scan_err.message
        );
        let re = get_run_end(&findings);
        assert!(re.summary.scan_errors >= 1);
    }

    // -----------------------------------------------------------------------
    // FailingIoScan / FailingMinerScan fixtures for the two scan-dispatch
    // error arms. Both fixtures construct `ScanFindingShape` via EXPLICIT
    // struct literal because the type does not derive `Default` (Warning 2).
    // The empty `&[]` const-literals are valid `&'static [&'static str]`
    // empty slices.
    // -----------------------------------------------------------------------
    struct FailingIoScan;
    impl crate::scan::Scan for FailingIoScan {
        fn id(&self) -> &'static str {
            "test.failing_io"
        }
        fn version(&self) -> u32 {
            1
        }
        fn arity(&self) -> crate::scan::ScanArity {
            crate::scan::ScanArity::Single
        }
        fn param_schema(&self) -> serde_json::Value {
            serde_json::json!({"type":"object"})
        }
        fn finding_fields(&self) -> crate::scan::ScanFindingShape {
            crate::scan::ScanFindingShape {
                effect_extra_keys: &[],
                raw_series_keys: &[],
            }
        }
        fn run(
            &self,
            _ctx: &crate::scan::ScanCtx<'_>,
            _req: &crate::scan::ScanRequest,
            _sink: &mut dyn FindingSink,
        ) -> Result<(), crate::scan::ScanError> {
            Err(crate::scan::ScanError::Io(std::io::Error::other(
                "forced scan-IO error",
            )))
        }
    }

    struct FailingMinerScan;
    impl crate::scan::Scan for FailingMinerScan {
        fn id(&self) -> &'static str {
            "test.failing_miner"
        }
        fn version(&self) -> u32 {
            1
        }
        fn arity(&self) -> crate::scan::ScanArity {
            crate::scan::ScanArity::Single
        }
        fn param_schema(&self) -> serde_json::Value {
            serde_json::json!({"type":"object"})
        }
        fn finding_fields(&self) -> crate::scan::ScanFindingShape {
            crate::scan::ScanFindingShape {
                effect_extra_keys: &[],
                raw_series_keys: &[],
            }
        }
        fn run(
            &self,
            _ctx: &crate::scan::ScanCtx<'_>,
            _req: &crate::scan::ScanRequest,
            _sink: &mut dyn FindingSink,
        ) -> Result<(), crate::scan::ScanError> {
            Err(crate::scan::ScanError::Miner(MinerError::Internal(
                "forced scan miner-error".to_string(),
            )))
        }
    }

    fn happy_reader_one_day() -> (FakeReader, NaiveDate) {
        let date = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap();
        let mut reader = FakeReader::new();
        reader.insert_day("EURUSD", Side::Bid, date, build_full_day_1m_bars(date, 1));
        (reader, date)
    }

    fn request_for_scan(scan_id: &str) -> ScanRequest {
        let start = Utc.with_ymd_and_hms(2024, 1, 2, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2024, 1, 3, 0, 0, 0).unwrap();
        let mut req = sample_request("EURUSD", start, end, GapPolicyKind::ContinuousOnly, false);
        req.scan_id = scan_id.to_string();
        req
    }

    /// Plan 03-07 CR-01 regression — ScanError::Io arm of the dispatch.
    #[test]
    fn run_one_scan_io_error_emits_run_start_and_run_end_with_scan_error() {
        let (reader, _date) = happy_reader_one_day();
        let req = request_for_scan("test.failing_io");
        let tmp = tempfile::TempDir::new().unwrap();
        let cfg = tmp_config(&tmp);
        let mut sink = VecSink::new();
        let mut registry = crate::scan::Registry::new();
        registry.register(Box::new(FailingIoScan));
        let outcome = crate::engine::run_one_with_registry(
            &req,
            &cfg,
            &reader,
            &mut sink,
            Arc::new(AtomicBool::new(false)),
            &registry,
        )
        .expect("ok — scan-IO error path returns Ok(HadScanErrors)");
        assert_eq!(outcome, RunOutcome::HadScanErrors);

        let findings = parse_findings(&sink);
        assert!(
            matches!(findings.first(), Some(Finding::RunStart(_))),
            "first envelope must be RunStart"
        );
        assert!(
            matches!(findings.last(), Some(Finding::RunEnd(_))),
            "last envelope must be RunEnd"
        );
        let scan_err = findings
            .iter()
            .find_map(|f| match f {
                Finding::ScanError(se) => Some(se),
                _ => None,
            })
            .expect("ScanError envelope present");
        assert!(
            scan_err.message.contains("scan io"),
            "ScanError message must surface the 'scan io' context; got {:?}",
            scan_err.message
        );
        assert!(
            scan_err.message.contains("forced scan-IO error"),
            "ScanError message must echo the fixture error text; got {:?}",
            scan_err.message
        );
        let re = get_run_end(&findings);
        assert!(re.summary.scan_errors >= 1);
    }

    /// Plan 03-07 CR-01 regression — ScanError::Miner arm of the dispatch.
    #[test]
    fn run_one_scan_miner_error_emits_run_start_and_run_end_with_scan_error() {
        let (reader, _date) = happy_reader_one_day();
        let req = request_for_scan("test.failing_miner");
        let tmp = tempfile::TempDir::new().unwrap();
        let cfg = tmp_config(&tmp);
        let mut sink = VecSink::new();
        let mut registry = crate::scan::Registry::new();
        registry.register(Box::new(FailingMinerScan));
        let outcome = crate::engine::run_one_with_registry(
            &req,
            &cfg,
            &reader,
            &mut sink,
            Arc::new(AtomicBool::new(false)),
            &registry,
        )
        .expect("ok — scan-miner-error path returns Ok(HadScanErrors)");
        assert_eq!(outcome, RunOutcome::HadScanErrors);

        let findings = parse_findings(&sink);
        assert!(
            matches!(findings.first(), Some(Finding::RunStart(_))),
            "first envelope must be RunStart"
        );
        assert!(
            matches!(findings.last(), Some(Finding::RunEnd(_))),
            "last envelope must be RunEnd"
        );
        let scan_err = findings
            .iter()
            .find_map(|f| match f {
                Finding::ScanError(se) => Some(se),
                _ => None,
            })
            .expect("ScanError envelope present");
        assert!(
            scan_err.message.contains("scan miner-error"),
            "ScanError message must surface the 'scan miner-error' context; got {:?}",
            scan_err.message
        );
        let re = get_run_end(&findings);
        assert!(re.summary.scan_errors >= 1);
    }

    // -----------------------------------------------------------------------
    // Plan 04-12 (CR-01) — Pair-arity dispatch regression gates.
    //
    // Pin the three dispatch branches the new `dispatch_pair_arity_body`
    // helper introduces:
    //
    // 1. `run_one_dispatches_single_arity_scan_via_single_leg_path` — Single
    //    arity continues to go through the historical single-leg path; the
    //    produced Finding has `data_slice.sources.len() == 1`.
    // 2. `run_one_dispatches_pair_arity_scan_via_dispatch_pair` — Pair arity
    //    now reaches the kernel, produces a Finding::Result (NOT
    //    Finding::ScanError "expected Pair arity"), and
    //    `data_slice.sources.len() == 2`. Without the Plan 04-12 fix this
    //    test would land a `Finding::ScanError(compute_error, "expected Pair
    //    arity (ctx.bars_pair is None)")` envelope.
    // 3. `run_one_pair_arity_with_mismatched_instrument_count_rejected_at_preflight`
    //    — arity preflight still rejects a single-leg Pair-arity request
    //    BEFORE any dispatch branch runs (i.e., the new Pair branch must
    //    not bypass `validate_arity`).
    // -----------------------------------------------------------------------

    /// Stub Pair-arity scan used by Tests 2 and 3 below. Emits exactly one
    /// `Finding::Result` envelope with the per-leg sources populated; reads
    /// `ctx.bars_pair` and asserts `Some(_)` (the test would panic if the
    /// engine reached this body with `bars_pair: None`).
    struct StubPairScan;
    impl crate::scan::Scan for StubPairScan {
        fn id(&self) -> &'static str {
            "stub.cross.pair_dispatch"
        }
        fn version(&self) -> u32 {
            1
        }
        fn arity(&self) -> crate::scan::ScanArity {
            crate::scan::ScanArity::Pair
        }
        fn param_schema(&self) -> serde_json::Value {
            serde_json::json!({"type":"object","additionalProperties":false})
        }
        fn finding_fields(&self) -> crate::scan::ScanFindingShape {
            crate::scan::ScanFindingShape {
                effect_extra_keys: &[],
                raw_series_keys: &["timestamps_ms"],
            }
        }
        fn run(
            &self,
            ctx: &crate::scan::ScanCtx<'_>,
            req: &crate::scan::ScanRequest,
            sink: &mut dyn FindingSink,
        ) -> Result<(), crate::scan::ScanError> {
            // Plan 04-12 contract: Pair-arity scans MUST receive a populated
            // `bars_pair`. If the engine drops to bars_pair: None this
            // assertion fires the test as a panic.
            assert!(
                ctx.bars_pair.is_some(),
                "StubPairScan invariant: engine must pass bars_pair: Some((a, b)) to Pair-arity scans"
            );
            let sources: Vec<crate::findings::Source> = req
                .instruments
                .iter()
                .map(|spec| crate::findings::Source {
                    source_id: ctx.bars.source_id.clone(),
                    symbol: spec.symbol.clone(),
                    side: spec.side.as_str().to_string(),
                    timeframe: req.timeframe.as_str().to_string(),
                })
                .collect();
            // Build a minimal raw block with the canonical timestamps_ms
            // series (D-03) so `Raw::new` accepts it.
            let mut series = std::collections::BTreeMap::new();
            series.insert(
                "timestamps_ms".to_string(),
                crate::scan::primitives::raw_array::f64_slice_to_raw_array(&[]),
            );
            let raw = crate::findings::Raw { series };
            let result = Finding::Result(crate::findings::ResultFinding {
                schema_version: 1,
                scan_id_at_version: format!("{}@{}", req.scan_id, req.version),
                param_hash: req.param_hash.as_str().to_string(),
                code_revision: ctx.code_revision.to_string(),
                data_slice: DataSlice {
                    range: req.sub_range.clone(),
                    gap_manifest_ref: None,
                    gap_manifest: None,
                    sources,
                },
                dsr: None,
                fdr_q: None,
                run_id: ctx.run_id,
                produced_at_utc: chrono::Utc::now(),
                params: req.resolved_params.clone(),
                effect: crate::findings::Effect {
                    metric: "stub_pair_metric".to_string(),
                    value: 0.0,
                    p_value: None,
                    n: None,
                    ci95: None,
                    effect_size: None,
                    extra: std::collections::BTreeMap::new(),
                },
                raw: Some(raw),
                repro: None,
            });
            sink.write_envelope(&result)?;
            Ok(())
        }
    }

    fn pair_arity_request(scan_id: &str, instrument_count: usize) -> ScanRequest {
        let start = Utc.with_ymd_and_hms(2024, 1, 2, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2024, 1, 3, 0, 0, 0).unwrap();
        let resolved = serde_json::json!({});
        let param_hash = param_hash::param_hash(&resolved).expect("hash ok");
        let mut instruments = vec![InstrumentSpec {
            symbol: "EURUSD".into(),
            side: Side::Bid,
        }];
        if instrument_count >= 2 {
            instruments.push(InstrumentSpec {
                symbol: "GBPUSD".into(),
                side: Side::Bid,
            });
        }
        ScanRequest {
            scan_id: scan_id.into(),
            version: 1,
            instruments,
            timeframe: Timeframe::Tf15m,
            window: ClosedRangeUtc { start, end },
            sub_range: TimeRange {
                start_utc: start,
                end_utc: end,
            },
            gap_policy: GapPolicyKind::ContinuousOnly,
            resolved_params: resolved,
            param_hash,
            dry_run: false,
        master_seed: None,
        job_seed: None,
        bootstrap_method: None,
        bootstrap_n: None,
        null_method: None,
        null_n: None,
            sleep_after_first_finding_ms: None,
        }
    }

    /// Plan 04-12 Task 1 Test 1 — Single-arity scans continue to flow through
    /// the historical single-leg path. The existing run_one_strict_zero_gaps
    /// test already proves the bulk of this; this test pins the
    /// `data_slice.sources.len() == 1` invariant explicitly as a sibling
    /// gate to Test 2.
    #[test]
    fn run_one_dispatches_single_arity_scan_via_single_leg_path() {
        // Reuse the existing LjungBox single-arity wiring via bootstrap().
        let start = Utc.with_ymd_and_hms(2024, 1, 2, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2024, 1, 3, 0, 0, 0).unwrap();
        let req = sample_request("EURUSD", start, end, GapPolicyKind::ContinuousOnly, false);
        let mut reader = FakeReader::new();
        reader.insert_day(
            "EURUSD",
            Side::Bid,
            NaiveDate::from_ymd_opt(2024, 1, 2).unwrap(),
            build_full_day_1m_bars(NaiveDate::from_ymd_opt(2024, 1, 2).unwrap(), 1),
        );
        let tmp = tempfile::TempDir::new().unwrap();
        let cfg = tmp_config(&tmp);
        let mut sink = VecSink::new();
        let outcome = run_one(
            &req,
            &cfg,
            &reader,
            &mut sink,
            Arc::new(AtomicBool::new(false)),
        )
        .expect("ok");
        assert_eq!(outcome, RunOutcome::Ok);
        let findings = parse_findings(&sink);
        let result = findings
            .iter()
            .find_map(|f| match f {
                Finding::Result(r) => Some(r),
                _ => None,
            })
            .expect("Result present");
        assert_eq!(
            result.data_slice.sources.len(),
            1,
            "Single-arity dispatch must produce data_slice.sources.len() == 1"
        );
        assert_eq!(result.data_slice.sources[0].symbol, "EURUSD");
    }

    /// Plan 04-12 Task 1 Test 2 — Pair-arity scans now reach the kernel via
    /// `dispatch_pair_arity_body`, producing a `Finding::Result` with
    /// `data_slice.sources.len() == 2`. Without the Plan 04-12 fix the
    /// engine would emit a `Finding::ScanError(compute_error, "expected Pair
    /// arity (ctx.bars_pair is None)")` instead.
    ///
    /// This is the CR-01 engine-level regression gate.
    #[test]
    fn run_one_dispatches_pair_arity_scan_via_dispatch_pair() {
        let req = pair_arity_request("stub.cross.pair_dispatch", 2);

        // Both legs present in the fake reader so neither gap-detection
        // arm short-circuits to ScanError.
        let day = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap();
        let mut reader = FakeReader::new();
        reader.insert_day("EURUSD", Side::Bid, day, build_full_day_1m_bars(day, 1));
        reader.insert_day("GBPUSD", Side::Bid, day, build_full_day_1m_bars(day, 2));

        let tmp = tempfile::TempDir::new().unwrap();
        let cfg = tmp_config(&tmp);
        let mut sink = VecSink::new();
        let mut registry = crate::scan::Registry::new();
        registry.register(Box::new(StubPairScan));
        let outcome = run_one_with_registry(
            &req,
            &cfg,
            &reader,
            &mut sink,
            Arc::new(AtomicBool::new(false)),
            &registry,
        )
        .expect("ok — Pair dispatch reaches kernel via dispatch_pair_arity_body");
        assert_eq!(
            outcome,
            RunOutcome::Ok,
            "Pair-arity dispatch must produce RunOutcome::Ok, NOT HadScanErrors"
        );

        let findings = parse_findings(&sink);

        // CR-01 negative pin: NO ScanError envelope with the bars_pair=None
        // message must appear. If the engine regresses to hard-coded single
        // dispatch this assertion fires.
        for f in &findings {
            if let Finding::ScanError(se) = f {
                assert!(
                    !se.message.contains("expected Pair arity"),
                    "CR-01 regression: engine emitted ScanError({:?}); Pair-arity dispatch must reach the kernel",
                    se.message
                );
            }
        }

        let result = findings
            .iter()
            .find_map(|f| match f {
                Finding::Result(r) => Some(r),
                _ => None,
            })
            .expect("Result envelope present — Pair-arity dispatch reached the kernel");
        assert_eq!(
            result.data_slice.sources.len(),
            2,
            "Pair-arity dispatch must populate data_slice.sources with BOTH legs (D4-03)"
        );
        assert_eq!(result.data_slice.sources[0].symbol, "EURUSD");
        assert_eq!(result.data_slice.sources[1].symbol, "GBPUSD");
        assert_eq!(result.scan_id_at_version, "stub.cross.pair_dispatch@1");
    }

    /// Plan 04-12 Task 1 Test 3 — Pair-arity dispatch must NOT bypass the
    /// arity preflight gate. A single-leg request against a Pair-arity scan
    /// is still rejected at preflight with `PreflightCode::WrongInstrumentArity`;
    /// stdout (the sink) stays empty (D-06).
    #[test]
    fn run_one_pair_arity_with_mismatched_instrument_count_rejected_at_preflight() {
        let req = pair_arity_request("stub.cross.pair_dispatch", 1);
        let tmp = tempfile::TempDir::new().unwrap();
        let cfg = tmp_config(&tmp);
        let reader = FakeReader::new();
        let mut sink = VecSink::new();
        let mut registry = crate::scan::Registry::new();
        registry.register(Box::new(StubPairScan));
        let result = run_one_with_registry(
            &req,
            &cfg,
            &reader,
            &mut sink,
            Arc::new(AtomicBool::new(false)),
            &registry,
        );
        match result {
            Err(MinerError::Preflight(w)) => {
                assert_eq!(
                    w.code, "wrong_instrument_arity",
                    "Pair-dispatch must NOT bypass validate_arity preflight"
                );
            }
            other => panic!("expected MinerError::Preflight(WrongInstrumentArity); got {other:?}"),
        }
        assert!(
            sink.0.is_empty(),
            "stdout must stay empty on preflight rejection (D-06)"
        );
    }
}

// ---------------------------------------------------------------------------
// engine::cancellation_tests — SC-5b (Blocker 1).
//
// The three named tests exercise the three documented cancel yield sites
// listed in run_one's doc-comment, matching the names called out in
// VALIDATION.md SC-5b row.
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(
    clippy::similar_names,
    clippy::match_wildcard_for_single_variants,
    clippy::items_after_statements,
    clippy::manual_let_else,
    clippy::doc_markdown
)]
mod cancellation_tests {
    use super::tests::FakeReader;
    use super::*;
    use crate::aggregator::Timeframe;
    use crate::config::OutputDest;
    use crate::findings::sink::VecSink;
    use crate::reader::{InstrumentSpec, RawBar, Side};
    use chrono::{Duration, NaiveDate, TimeZone};
    use std::time::Instant;

    fn build_full_day_1m_bars(date: NaiveDate, seed: u32) -> Vec<RawBar> {
        let day_start = date.and_hms_opt(0, 0, 0).unwrap().and_utc();
        let mut bars = Vec::with_capacity(1440);
        let mut s = seed;
        for i in 0..1440_i64 {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let frac = f64::from(s) / f64::from(u32::MAX);
            let close = 1.0 + frac * 0.01;
            let ts_open = day_start + Duration::minutes(i);
            let ts_close = ts_open + Duration::minutes(1);
            bars.push(RawBar {
                ts_open_utc: ts_open,
                ts_close_utc: ts_close,
                open: close - 0.0001,
                high: close + 0.0001,
                low: close - 0.0002,
                close,
                tick_volume: 1.0,
            });
        }
        bars
    }

    fn mk_cfg(tmp: &tempfile::TempDir) -> MinerConfig {
        MinerConfig {
            cache_root: tmp.path().join("cache"),
            bar_cache_root: tmp.path().join("bar-cache"),
            output: OutputDest::Stdout,
        }
    }

    fn mk_request(
        start: chrono::DateTime<Utc>,
        end: chrono::DateTime<Utc>,
        sleep_after_ms: Option<u64>,
    ) -> ScanRequest {
        let resolved = serde_json::json!({"lags": 5});
        let param_hash = param_hash::param_hash(&resolved).expect("hash");
        ScanRequest {
            scan_id: "stats.autocorr.ljung_box".into(),
            version: 1,
            instruments: vec![InstrumentSpec {
                symbol: "EURUSD".into(),
                side: Side::Bid,
            }],
            timeframe: Timeframe::Tf15m,
            window: ClosedRangeUtc { start, end },
            sub_range: TimeRange {
                start_utc: start,
                end_utc: end,
            },
            gap_policy: GapPolicyKind::ContinuousOnly,
            resolved_params: resolved,
            param_hash,
            dry_run: false,
        master_seed: None,
        job_seed: None,
        bootstrap_method: None,
        bootstrap_n: None,
        null_method: None,
        null_n: None,
            sleep_after_first_finding_ms: sleep_after_ms,
        }
    }

    /// SC-5b yield site 1 — cancel set BEFORE `run_one` is invoked. The
    /// function returns immediately without emitting any envelope.
    #[test]
    fn cancel_at_entry() {
        let start = Utc.with_ymd_and_hms(2024, 1, 2, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2024, 1, 3, 0, 0, 0).unwrap();
        let req = mk_request(start, end, None);
        let mut reader = FakeReader::new();
        reader.insert_day(
            "EURUSD",
            Side::Bid,
            NaiveDate::from_ymd_opt(2024, 1, 2).unwrap(),
            build_full_day_1m_bars(NaiveDate::from_ymd_opt(2024, 1, 2).unwrap(), 1),
        );
        let tmp = tempfile::TempDir::new().unwrap();
        let cfg = mk_cfg(&tmp);
        let mut sink = VecSink::new();
        let cancel = Arc::new(AtomicBool::new(true));
        let outcome = run_one(&req, &cfg, &reader, &mut sink, cancel).expect("ok");
        assert_eq!(outcome, RunOutcome::Ok);
        assert!(
            sink.0.is_empty(),
            "cancel-at-entry must NOT emit any envelope"
        );
    }

    /// SC-5b yield site 2 — cancel flipped between the first and second
    /// sub-range of a `ContinuousOnly` partition. Built by routing the sink
    /// through a closure that observes the first Result and flips cancel.
    ///
    /// The fixture provides a manifest with exactly one intra-day gap
    /// splitting the requested window into TWO sub-ranges; the test asserts
    /// exactly ONE Result is emitted (for the first sub-range) and `RunEnd` is
    /// still emitted cleanly. The cancellation breaks the sub-range loop
    /// before the second sub-range's bars are loaded.
    #[test]
    fn cancel_before_subrange() {
        // Two full days: 2024-01-02 (Tue) + 2024-01-03 (Wed). Insert a gap by
        // omitting minutes 1200..1215 on day 1 (20:00..20:15) — produces
        // intra-day gaps that partition the requested window into:
        //   sub_range_1 = [day1 00:00, day1 20:00)   (80 Tf15m bars)
        //   sub_range_2 = [day1 20:15, day2 24:00)   (~112 Tf15m bars)
        // Both Tf15m-aligned, both have enough closes (>= 6) to satisfy
        // LjungBoxScan with explicit `lags=5`.
        let day1 = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap();
        let day2 = NaiveDate::from_ymd_opt(2024, 1, 3).unwrap();
        let start = day1.and_hms_opt(0, 0, 0).unwrap().and_utc();
        let end = day2.and_hms_opt(0, 0, 0).unwrap().and_utc() + Duration::days(1);

        let req = mk_request(start, end, None);
        let mut reader = FakeReader::new();
        // Day 1 with a hole at minutes 1200..1215 -> 15 intra-day gaps in
        // [20:00, 20:15).
        let day1_bars: Vec<RawBar> = build_full_day_1m_bars(day1, 1)
            .into_iter()
            .enumerate()
            .filter(|(i, _)| !(1200..1215).contains(&i64::try_from(*i).unwrap()))
            .map(|(_, b)| b)
            .collect();
        reader.insert_day("EURUSD", Side::Bid, day1, day1_bars);
        reader.insert_day("EURUSD", Side::Bid, day2, build_full_day_1m_bars(day2, 2));

        let tmp = tempfile::TempDir::new().unwrap();
        let cfg = mk_cfg(&tmp);

        // Sink wrapper that flips the cancel token after the FIRST Result.
        struct FlipOnResult {
            inner: VecSink,
            cancel: Arc<AtomicBool>,
            results_seen: usize,
        }
        impl FindingSink for FlipOnResult {
            fn write_envelope(&mut self, f: &Finding) -> Result<(), crate::error::MinerError> {
                if matches!(f, Finding::Result(_)) {
                    self.results_seen += 1;
                    if self.results_seen == 1 {
                        self.cancel.store(true, Ordering::SeqCst);
                    }
                }
                self.inner.write_envelope(f)
            }
            fn write_raw_json(&mut self, v: &serde_json::Value) -> std::io::Result<()> {
                self.inner.write_raw_json(v)
            }
            fn flush(&mut self) -> Result<(), crate::error::MinerError> {
                self.inner.flush()
            }
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let mut sink = FlipOnResult {
            inner: VecSink::new(),
            cancel: Arc::clone(&cancel),
            results_seen: 0,
        };
        let outcome = run_one(&req, &cfg, &reader, &mut sink, Arc::clone(&cancel)).expect("ok");
        assert_eq!(outcome, RunOutcome::Ok);

        // Parse the captured envelopes.
        let findings: Vec<Finding> = sink
            .inner
            .0
            .split(|b| *b == b'\n')
            .filter(|l| !l.is_empty())
            .map(|l| serde_json::from_slice::<Finding>(l).expect("parse"))
            .collect();
        let result_count = findings
            .iter()
            .filter(|f| matches!(f, Finding::Result(_)))
            .count();
        let run_end_count = findings
            .iter()
            .filter(|f| matches!(f, Finding::RunEnd(_)))
            .count();
        assert_eq!(
            result_count, 1,
            "cancel_before_subrange must yield exactly one Result (the first sub-range)"
        );
        assert_eq!(run_end_count, 1, "RunEnd must still be emitted on cancel");
        // RunSummary.results_emitted == 1.
        if let Finding::RunEnd(re) = findings.iter().last().unwrap() {
            assert_eq!(re.summary.results_emitted, 1);
        }
    }

    /// SC-5b yield site 3 — cancel flipped while `LjungBoxScan::run` is
    /// inside its cancel-aware sleep loop. The test sets
    /// `req.sleep_after_first_finding_ms = Some(2000)` and a watcher thread
    /// flips `cancel` after ~50ms; the run must return within ~150ms.
    #[test]
    fn cancel_inside_scan_kernel() {
        let start = Utc.with_ymd_and_hms(2024, 1, 2, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2024, 1, 3, 0, 0, 0).unwrap();
        let req = mk_request(start, end, Some(2000));
        let mut reader = FakeReader::new();
        reader.insert_day(
            "EURUSD",
            Side::Bid,
            NaiveDate::from_ymd_opt(2024, 1, 2).unwrap(),
            build_full_day_1m_bars(NaiveDate::from_ymd_opt(2024, 1, 2).unwrap(), 1),
        );
        let tmp = tempfile::TempDir::new().unwrap();
        let cfg = mk_cfg(&tmp);
        let mut sink = VecSink::new();

        let cancel = Arc::new(AtomicBool::new(false));
        let watcher = Arc::clone(&cancel);
        let handle = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(50));
            watcher.store(true, Ordering::SeqCst);
        });

        let begin = Instant::now();
        let outcome = run_one(&req, &cfg, &reader, &mut sink, Arc::clone(&cancel)).expect("ok");
        let elapsed = begin.elapsed();
        handle.join().unwrap();

        assert_eq!(outcome, RunOutcome::Ok);
        assert!(
            elapsed < std::time::Duration::from_millis(800),
            "cancel-aware sleep must return within ~150ms (allow 800ms slack for CI), got {elapsed:?}"
        );
        // The first Result envelope was emitted before the sleep; RunEnd was
        // emitted cleanly.
        let findings: Vec<Finding> = sink
            .0
            .split(|b| *b == b'\n')
            .filter(|l| !l.is_empty())
            .map(|l| serde_json::from_slice::<Finding>(l).expect("parse"))
            .collect();
        let result_count = findings
            .iter()
            .filter(|f| matches!(f, Finding::Result(_)))
            .count();
        assert_eq!(result_count, 1, "exactly one Result before cancel");
        let run_end_count = findings
            .iter()
            .filter(|f| matches!(f, Finding::RunEnd(_)))
            .count();
        assert_eq!(run_end_count, 1);
    }
}
