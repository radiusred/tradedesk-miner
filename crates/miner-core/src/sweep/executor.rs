//! Phase 5 sweep executor — `run_sweep` (Plan 05-04 Task 2 / D5-04 / OP-04).
//!
//! Pattern analog: `crate::engine::run_one_with_registry` — multi-step
//! algorithm walk with documented cancel-poll sites and a single
//! framing-close. `run_sweep` wraps that per-job, layered:
//!
//! 1. Preflight (`manifest::validate`) + framing-open (`RunStart`).
//! 2. Dry-run short-circuit (`Finding::DryRun` with `planned_job_count`
//!    populated; per RESEARCH Pattern 5 — NO new `SweepDryRun` variant).
//! 3. Rayon-parallel job fanout into per-job `Vec<Finding>` buffers
//!    (RESEARCH Pattern 4 — deterministic-order buffered drain).
//! 4. Sequential, manifest-order drain to the real sink. While draining,
//!    capture `(family_key, finding_index_within_family, raw_p)` triples
//!    per `[fdr].family` scope.
//! 5. BH-FDR aggregation + `Finding::SweepSummary` emission (skipped on
//!    SIGINT — D5-04 / HYG-05; CLI maps `RunOutcome::Ok` + cancel to
//!    exit 130).
//! 6. Framing-close (`RunEnd` + sink flush).
//!
//! ## Per-job framing suppression
//!
//! Each per-job rayon worker calls `engine::run_one_with_registry` into
//! a `JobSink` wrapper. The wrapper SWALLOWS `Finding::RunStart` and
//! `Finding::RunEnd` envelopes (those are emitted ONCE by the sweep at
//! steps 1 and 6) and APPENDS every other variant to the per-job
//! `Vec<Finding>` buffer. Mid-stream `Finding::ScanError` /
//! `Finding::GapAborted` envelopes flow through unchanged.
//!
//! ## Cancel cadence (RESEARCH Pitfall 7)
//!
//! - `cancel_at_entry` — top of `run_sweep_with_registry` (before
//!   `par_iter`).
//! - `cancel_per_job` — top of each per-job rayon closure.
//! - `cancel_before_summary` — between the drain loop and the
//!   BH-FDR/`SweepSummary` emission. When set, the `SweepSummary` is
//!   NOT emitted; partially-streamed `Result`s persist.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use chrono::Utc;
use rayon::prelude::*;

use crate::cache::BarCache;
use crate::config::MinerConfig;
use crate::engine::{RunOutcome, run_one_with_registry};
use crate::error::{MinerError, ScanErrorCode};
use crate::findings::{
    DataSlice, DryRunFinding, FdrFamilySummary, Finding, FindingFdrEntry, FindingSink, RunSummary,
    ScanErrorFinding, Source, SweepSummaryFinding, SweepTotals, TimeRange, run_id::RunId,
};
use crate::reader::Reader;
use crate::scan::ScanRequest;
use crate::sweep::manifest::{SweepManifest, validate};

// ---------------------------------------------------------------------------
// SweepOptions
// ---------------------------------------------------------------------------

/// Sweep execution options carried alongside the manifest.
///
/// `dry_run` short-circuits the rayon fanout — the executor emits one
/// `Finding::DryRun` with `planned_job_count == Some(estimated_count)`
/// and returns `RunOutcome::Ok` without invoking any scan bodies (per
/// RESEARCH Pattern 5).
///
/// `sleep_after_first_finding_ms` is the SIGINT-race hook threaded through
/// `job_to_scan_request` onto every per-job
/// `ScanRequest.sleep_after_first_finding_ms`. Plan 05-05's `sigint_mid_sweep`
/// integration test uses it to make the cancel race deterministic. The field
/// itself is plain `Option<u64>` (always present) because callers may need
/// to forward `None` even from a release build; the cfg gate lives on the
/// **kernel-side** sleep loop (`ScanCtx.sleep_after_first_finding_ms` /
/// `LjungBoxScan::run`), which is the actual "test-only behaviour" boundary.
/// Setting this field from a release-built binary is a no-op because the
/// downstream `ScanRequest.sleep_after_first_finding_ms` field is itself
/// `#[cfg(any(test, feature = "test-internal"))]`-gated.
#[derive(Debug, Clone, Default)]
pub struct SweepOptions {
    pub dry_run: bool,
    /// Plan 05-05 — SIGINT-race hook (see struct docs). Setter call site in
    /// the CLI is `#[cfg(any(test, feature = "test-internal"))]`-gated; this
    /// field stays ungated so the `SweepOptions { .. }` struct literal in
    /// the CLI compiles uniformly across feature sets.
    pub sleep_after_first_finding_ms: Option<u64>,
}

// ---------------------------------------------------------------------------
// JobSink — per-job in-memory FindingSink that swallows framing envelopes
// ---------------------------------------------------------------------------

/// Per-job `FindingSink` impl backed by a `Vec<Finding>`. Swallows
/// `RunStart` and `RunEnd` envelopes (the sweep emits those once) but
/// passes every other variant through to the buffer.
struct JobSink {
    pub buf: Vec<Finding>,
}

impl JobSink {
    fn new() -> Self {
        Self { buf: Vec::new() }
    }
}

impl FindingSink for JobSink {
    fn write_envelope(&mut self, finding: &Finding) -> Result<(), MinerError> {
        // Swallow the per-job framing envelopes — the sweep emits a single
        // pair of RunStart/RunEnd around all jobs.
        if matches!(finding, Finding::RunStart(_) | Finding::RunEnd(_)) {
            return Ok(());
        }
        self.buf.push(finding.clone());
        Ok(())
    }

    fn write_raw_json(&mut self, _v: &serde_json::Value) -> std::io::Result<()> {
        // No raw-json passthrough in the sweep — the engine only ever
        // writes typed envelopes through the FindingSink trait surface.
        Ok(())
    }

    fn flush(&mut self) -> Result<(), MinerError> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// run_sweep — public entry point
// ---------------------------------------------------------------------------

/// Public sweep entry point. Resolves the production scan registry via
/// [`crate::scan::bootstrap`] then delegates to
/// [`run_sweep_with_registry`].
///
/// Note on `cache: &BarCache`: the per-job [`crate::engine::run_one_with_registry`]
/// call constructs its own `BarCache` from `cfg.bar_cache_root`. The
/// caller-supplied cache is presently a reserved hook (signature
/// parity with Plan 05-05's CLI) — `_cache` underscore prefix signals
/// the intentional non-use while keeping the parameter name in the
/// public surface.
///
/// # Errors
/// - `MinerError::Preflight(_)` — manifest fails preflight validation
///   (unknown scan, arity mismatch, `SweepTooLarge`, hygiene not
///   supported, etc).
/// - `MinerError::Scan(_)` — reader / cache failure surfaced inside a
///   per-job run (forwarded by `run_one_with_registry`).
/// - `MinerError::Serialize` — sink JSON serialisation failure.
#[allow(
    clippy::needless_pass_by_value,
    reason = "matches engine::run_one's by-value Arc<AtomicBool> convention"
)]
pub fn run_sweep<R: Reader + Sync>(
    manifest: SweepManifest,
    opts: SweepOptions,
    cfg: &MinerConfig,
    reader: &R,
    cache: &BarCache,
    sink: &mut dyn FindingSink,
    cancel: Arc<AtomicBool>,
) -> Result<RunOutcome, MinerError> {
    let registry = crate::scan::bootstrap();
    run_sweep_with_registry(manifest, opts, cfg, reader, cache, sink, cancel, &registry)
}

/// Internal entry point parameterised on a scan registry — tests inject
/// per-test registries.
///
/// # Errors
/// Same as [`run_sweep`].
#[allow(
    clippy::too_many_arguments,
    reason = "all arguments come from the caller's stack frame; introducing a context struct hides the data flow"
)]
#[allow(
    clippy::too_many_lines,
    reason = "linear algorithm walk: preflight -> RunStart -> dry-run short-circuit -> par_iter -> deterministic drain -> SweepSummary -> RunEnd. Splitting hides the cancel-cadence interleave."
)]
#[allow(
    clippy::needless_pass_by_value,
    reason = "matches engine::run_one_with_registry's by-value Arc<AtomicBool> convention"
)]
pub fn run_sweep_with_registry<R: Reader + Sync>(
    manifest: SweepManifest,
    opts: SweepOptions,
    cfg: &MinerConfig,
    reader: &R,
    _cache: &BarCache,
    sink: &mut dyn FindingSink,
    cancel: Arc<AtomicBool>,
    registry: &crate::scan::Registry,
) -> Result<RunOutcome, MinerError> {
    // -----------------------------------------------------------------------
    // Step 1 — cancel-at-entry yield site (RESEARCH Pattern 4 site 1).
    // -----------------------------------------------------------------------
    if cancel.load(Ordering::Relaxed) {
        return Ok(RunOutcome::Ok);
    }

    // -----------------------------------------------------------------------
    // Step 2 — preflight (typed Preflight error on rejection).
    // -----------------------------------------------------------------------
    let estimated_count = validate(&manifest, registry)?;

    // -----------------------------------------------------------------------
    // Step 3 — framing-open (RunStart).
    // -----------------------------------------------------------------------
    let run_id = RunId::new();
    let started = Utc::now();
    let run_start = build_sweep_run_start(&manifest, run_id, started);
    sink.write_envelope(&run_start)?;

    // -----------------------------------------------------------------------
    // Step 4 — dry-run short-circuit (per RESEARCH Pattern 5).
    //
    // Emit ONE Finding::DryRun with planned_job_count = expanded count.
    // No scan bodies, no SweepSummary; jump straight to framing-close.
    // -----------------------------------------------------------------------
    if opts.dry_run {
        let dry_run_finding = build_sweep_dry_run(&manifest, run_id, estimated_count);
        sink.write_envelope(&dry_run_finding)?;
        emit_sweep_run_end(sink, run_id, started, SweepTotals::default())?;
        return Ok(RunOutcome::Ok);
    }

    // -----------------------------------------------------------------------
    // Step 5 — expand to ResolvedJob list (deterministic D5-01 order).
    // -----------------------------------------------------------------------
    let jobs = crate::sweep::job_graph::expand(&manifest, registry)?;

    // -----------------------------------------------------------------------
    // Step 6 — rayon-parallel fanout (RESEARCH Pattern 4).
    // Each worker writes into a per-job JobSink (in-memory Vec<Finding>).
    // -----------------------------------------------------------------------
    // Plan 05-05: thread the SweepOptions sleep-hook into every per-job
    // ScanRequest. The downstream `ScanRequest.sleep_after_first_finding_ms`
    // field is cfg-gated to `test` / `test-internal`, so the propagation
    // here only compiles into the request struct under matching cfg.
    // Release builds (no `test-internal`) drop the field assignment.
    let sleep_after_first_finding_ms = opts.sleep_after_first_finding_ms;

    let buffered: Vec<(usize, Vec<Finding>)> = jobs
        .par_iter()
        .enumerate()
        .map(|(idx, job)| {
            // Cancel-poll at top of each worker (Pattern 4 site 2).
            if cancel.load(Ordering::Relaxed) {
                return (idx, Vec::new());
            }
            let scan_req = {
                #[allow(unused_mut)]
                let mut r = job_to_scan_request(job);
                #[cfg(any(test, feature = "test-internal"))]
                {
                    r.sleep_after_first_finding_ms = sleep_after_first_finding_ms;
                }
                #[cfg(not(any(test, feature = "test-internal")))]
                {
                    let _ = sleep_after_first_finding_ms;
                }
                r
            };
            let mut job_sink = JobSink::new();
            // Best-effort: per-job errors become ScanError envelopes
            // INSIDE the per-job sink (engine convention from Plan 03-07).
            // The Vec<Finding> contains zero or more Result/ScanError/etc
            // envelopes; the deterministic drain replays them in order.
            //
            // WR-02: capture per-job MinerError::Preflight and synthesise a
            // ScanError envelope into the job sink. `run_one_with_registry`
            // returns Preflight for unknown-scan / arity-mismatch /
            // hygiene-not-supported failures and emits NO envelope before
            // returning — without this synthesis the sweep would silently
            // drop the job (defence-in-depth against drift between
            // `manifest::validate` and the per-job engine checks).
            let outcome = run_one_with_registry(
                &scan_req,
                cfg,
                reader,
                &mut job_sink,
                Arc::clone(&cancel),
                registry,
            );
            if let Err(err) = outcome {
                let synth = build_synthetic_per_job_error(&scan_req, &err, run_id);
                let _ = job_sink.write_envelope(&synth);
            }
            (idx, job_sink.buf)
        })
        .collect();

    // -----------------------------------------------------------------------
    // Step 7 — deterministic-order drain (Pattern 4).
    //
    // Sort buffered tuples by manifest-order idx; for each Finding:
    //   - if Result + Some(p_value), capture (family_key, finding_index, raw_p)
    //     for BH-FDR scoping.
    //   - write through to the real sink (per-envelope flush).
    // -----------------------------------------------------------------------
    let mut buffered = buffered;
    buffered.sort_by_key(|(idx, _)| *idx);

    let mut totals = SweepTotals {
        jobs_run: 0,
        results_emitted: 0,
        scan_errors: 0,
        gap_aborted: 0,
    };
    // family_key -> Vec<(finding_index_within_family, raw_p)>
    let mut by_family: BTreeMap<String, Vec<(usize, f64)>> = BTreeMap::new();

    let fdr_family_scope = manifest.fdr.family.clone();

    for (_idx, findings) in buffered {
        totals.jobs_run = totals.jobs_run.saturating_add(1);
        for finding in findings {
            // Tally + family-scope BEFORE writing through, so the family
            // index ordering matches the streaming output position.
            match &finding {
                Finding::Result(r) => {
                    totals.results_emitted = totals.results_emitted.saturating_add(1);
                    if let Some(p) = r.effect.p_value {
                        // CR-03: skip NaN p-values from family aggregation.
                        // NaN p-values arise legitimately from analytic
                        // kernels on degenerate inputs (e.g., constant-
                        // variance bucket → t-stat NaN); including them
                        // in the BH-FDR walk would corrupt q-values for
                        // every other entry in the family. The kernel
                        // itself defends against this (see
                        // `scan::hygiene::fdr::bh_fdr`), but filtering at
                        // the boundary keeps the family vector clean and
                        // the rejection point visible.
                        if !p.is_nan() {
                            if let Some(family_key) =
                                scope_family(&r.scan_id_at_version, &fdr_family_scope)
                            {
                                let entries = by_family.entry(family_key).or_default();
                                let finding_index_within_family = entries.len();
                                entries.push((finding_index_within_family, p));
                            }
                        }
                    }
                }
                Finding::ScanError(_) => {
                    totals.scan_errors = totals.scan_errors.saturating_add(1);
                }
                Finding::GapAborted(_) => {
                    totals.gap_aborted = totals.gap_aborted.saturating_add(1);
                }
                _ => {}
            }
            sink.write_envelope(&finding)?;
        }
    }

    // -----------------------------------------------------------------------
    // Step 8 — SIGINT short-circuit (HYG-05 / D5-04). Skip SweepSummary
    // emission if cancel was set during the par_iter or drain. Already-
    // streamed Results persist by virtue of having been written above.
    // The CLI maps RunOutcome::Ok + cancel-set → exit 130.
    // -----------------------------------------------------------------------
    if cancel.load(Ordering::Relaxed) {
        emit_sweep_run_end(sink, run_id, started, totals)?;
        return Ok(RunOutcome::Ok);
    }

    // -----------------------------------------------------------------------
    // Step 9 — BH-FDR aggregation + Finding::SweepSummary emission
    // (Plan 05-02 hygiene::fdr::bh_fdr).
    //
    // `[fdr].family = "none"` short-circuits: emit SweepSummary with
    // EMPTY fdr_by_family + the tallied totals. The SweepSummary
    // envelope is ALWAYS emitted (never suppressed) for consistency —
    // the consumer can branch on fdr_by_family.is_empty(). See
    // SUMMARY.md decision 3.
    // -----------------------------------------------------------------------
    let fdr_by_family = if fdr_family_scope == "none" {
        BTreeMap::new()
    } else {
        let mut out: BTreeMap<String, FdrFamilySummary> = BTreeMap::new();
        for (family_key, entries) in by_family {
            let p_values: Vec<f64> = entries.iter().map(|(_, p)| *p).collect();
            let q_values = crate::scan::hygiene::fdr::bh_fdr(&p_values, manifest.fdr.alpha);
            let per_finding: Vec<FindingFdrEntry> = entries
                .iter()
                .zip(q_values.iter())
                .map(|((finding_index_within_family, raw_p), q)| FindingFdrEntry {
                    finding_index: *finding_index_within_family as u64,
                    raw_p: *raw_p,
                    q_value: *q,
                })
                .collect();
            out.insert(
                family_key,
                FdrFamilySummary {
                    method: "benjamini_hochberg".to_string(),
                    alpha: manifest.fdr.alpha,
                    per_finding,
                },
            );
        }
        out
    };

    let summary_finding = Finding::SweepSummary(SweepSummaryFinding {
        run_id,
        produced_at_utc: Utc::now(),
        fdr_by_family,
        totals,
    });
    sink.write_envelope(&summary_finding)?;

    // -----------------------------------------------------------------------
    // Step 10 — framing-close + RunOutcome map.
    // -----------------------------------------------------------------------
    let had_errors = totals.scan_errors > 0;
    emit_sweep_run_end(sink, run_id, started, totals)?;
    if had_errors {
        Ok(RunOutcome::HadScanErrors)
    } else {
        Ok(RunOutcome::Ok)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a `Finding::RunStart` for a sweep run. Mirrors
/// `engine::framing::build_run_start` but the `request` value carries a
/// `kind = "sweep"` discriminator plus the manifest-derived counts.
fn build_sweep_run_start(
    manifest: &SweepManifest,
    run_id: RunId,
    started: chrono::DateTime<Utc>,
) -> Finding {
    use crate::findings::RunStart;
    let mut request = serde_json::Map::new();
    request.insert(
        "kind".to_string(),
        serde_json::Value::String("sweep".to_string()),
    );
    request.insert(
        "max_jobs".to_string(),
        serde_json::Value::Number(serde_json::Number::from(manifest.sweep.max_jobs)),
    );
    request.insert(
        "fdr_family".to_string(),
        serde_json::Value::String(manifest.fdr.family.clone()),
    );
    request.insert(
        "fdr_alpha".to_string(),
        serde_json::Value::Number(
            serde_json::Number::from_f64(manifest.fdr.alpha).unwrap_or_else(|| 0.into()),
        ),
    );
    request.insert(
        "blocks".to_string(),
        serde_json::Value::Number(serde_json::Number::from(manifest.jobs.len())),
    );
    request.insert(
        "dry_run".to_string(),
        serde_json::Value::Bool(false),
    );

    Finding::RunStart(RunStart {
        run_id,
        started_at_utc: started,
        miner_version: env!("CARGO_PKG_VERSION").to_string(),
        code_revision: crate::CODE_REVISION.to_string(),
        request: serde_json::Value::Object(request),
    })
}

/// Build a `Finding::DryRun` for a sweep dry-run — `planned_job_count`
/// is populated; `planned_data_slice` is a placeholder (sweep doesn't
/// have a single contiguous data slice).
fn build_sweep_dry_run(
    manifest: &SweepManifest,
    run_id: RunId,
    estimated_count: u64,
) -> Finding {
    use crate::findings::{DataSlice, TimeRange};
    // Use the first job's first window as the placeholder data slice,
    // when present; otherwise an epoch-zero placeholder.
    let epoch = chrono::DateTime::<Utc>::from_timestamp(0, 0).expect("epoch valid");
    let epoch_range = TimeRange {
        start_utc: epoch,
        end_utc: epoch,
    };
    let placeholder_range = manifest
        .jobs
        .first()
        .and_then(|first_job| first_job.windows.first())
        .and_then(|first_window_str| {
            crate::engine::preflight::parse_iso_utc_window(first_window_str).ok()
        })
        .map_or(epoch_range, |w| TimeRange {
            start_utc: w.start,
            end_utc: w.end,
        });

    let mut request = serde_json::Map::new();
    request.insert(
        "kind".to_string(),
        serde_json::Value::String("sweep".to_string()),
    );
    request.insert("dry_run".to_string(), serde_json::Value::Bool(true));
    request.insert(
        "estimated_job_count".to_string(),
        serde_json::Value::Number(serde_json::Number::from(estimated_count)),
    );

    Finding::DryRun(DryRunFinding {
        run_id,
        produced_at_utc: Utc::now(),
        request: serde_json::Value::Object(request),
        // Resolved-params is per-job and not meaningful at sweep dry-run
        // level — surface the manifest's sweep-level config instead.
        resolved_params: serde_json::json!({
            "max_jobs": manifest.sweep.max_jobs,
            "fdr_family": manifest.fdr.family,
            "fdr_alpha": manifest.fdr.alpha,
        }),
        planned_data_slice: DataSlice {
            range: placeholder_range,
            gap_manifest_ref: None,
            gap_manifest: None,
            sources: Vec::new(),
        },
        // Best-effort: estimated 1 finding per job.
        estimated_findings_count: estimated_count,
        // Plan 05-04 / RESEARCH Pattern 5: sweep --dry-run populates this.
        planned_job_count: Some(estimated_count),
    })
}

/// Build + emit the sweep `Finding::RunEnd` envelope and flush the sink.
fn emit_sweep_run_end(
    sink: &mut dyn FindingSink,
    run_id: RunId,
    started: chrono::DateTime<Utc>,
    totals: SweepTotals,
) -> Result<(), MinerError> {
    use crate::findings::RunEnd;
    let ended = Utc::now();
    // Build a RunSummary mirroring the per-job totals so the existing
    // RunEnd contract is preserved (Phase 3 wire shape unchanged).
    let summary = RunSummary {
        results_emitted: totals.results_emitted,
        scan_errors: totals.scan_errors,
        gap_aborted: totals.gap_aborted,
        per_scan: BTreeMap::new(),
    };
    let finding = Finding::RunEnd(RunEnd {
        run_id,
        ended_at_utc: ended,
        wall_clock_ms: ended.signed_duration_since(started).num_milliseconds(),
        summary,
    });
    sink.write_envelope(&finding)?;
    sink.flush()?;
    Ok(())
}

/// WR-02 helper: build a `Finding::ScanError` envelope from a per-job
/// `MinerError`. Used to surface preflight rejections (or any other
/// uncaught engine error) that `run_one_with_registry` returned WITHOUT
/// emitting an envelope itself.
///
/// Always returns a Finding — every `MinerError` reaching the per-job
/// boundary is something the user should see. `Preflight` carries the
/// typed `code` + `message`; other variants synthesise a `compute_error`
/// envelope (defence-in-depth — those variants normally surface their
/// own envelopes inside the engine).
fn build_synthetic_per_job_error(
    req: &ScanRequest,
    err: &MinerError,
    run_id: RunId,
) -> Finding {
    // Pull a structured `WireError` out of the typed error where possible.
    // For non-Preflight variants we fabricate a minimal `error_code` +
    // message; the per-job ScanError envelope is the engine's existing
    // wire convention so consumers do not need a new error path.
    let (error_code, message): (String, String) = match err {
        MinerError::Preflight(w) => (w.code.clone(), w.message.clone()),
        other => {
            // Other MinerError variants (Io / Scan / Serialize) already
            // surface their own envelopes inside the engine when they
            // arise mid-run; if one returns at the per-job boundary the
            // engine's framing-close discipline already swallowed the
            // emitter. Synthesise a compute-error envelope so the user
            // sees something rather than silent loss.
            (
                ScanErrorCode::ComputeError.as_str().to_string(),
                format!("per-job engine error: {other}"),
            )
        }
    };

    let sources: Vec<Source> = req
        .instruments
        .iter()
        .map(|spec| Source {
            source_id: String::new(),
            symbol: spec.symbol.clone(),
            side: spec.side.as_str().to_string(),
            timeframe: req.timeframe.as_str().to_string(),
        })
        .collect();
    let data_slice = DataSlice {
        range: TimeRange {
            start_utc: req.window.start,
            end_utc: req.window.end,
        },
        gap_manifest_ref: None,
        gap_manifest: None,
        sources,
    };

    Finding::ScanError(ScanErrorFinding {
        schema_version: 1,
        scan_id_at_version: format!("{}@{}", req.scan_id, req.version),
        param_hash: req.param_hash.as_str().to_string(),
        code_revision: crate::CODE_REVISION.to_string(),
        data_slice,
        dsr: None,
        fdr_q: None,
        run_id,
        produced_at_utc: Utc::now(),
        error_code,
        message,
        request_context: serde_json::json!({
            "scan_id": req.scan_id,
            "kind": "per_job_preflight",
        }),
    })
}

/// Translate a `ResolvedJob` into a single-shot `ScanRequest` for
/// `engine::run_one_with_registry`.
fn job_to_scan_request(job: &crate::sweep::job_graph::ResolvedJob) -> ScanRequest {
    use crate::findings::TimeRange;
    let sub_range = TimeRange {
        start_utc: job.window.start,
        end_utc: job.window.end,
    };
    ScanRequest {
        scan_id: job.scan_id.clone(),
        version: job.version,
        instruments: job.instruments.clone(),
        timeframe: job.timeframe,
        window: job.window,
        sub_range,
        gap_policy: job.gap_policy,
        resolved_params: job.resolved_params.clone(),
        // Blake3Hex is Copy — direct value pass-through.
        param_hash: job.param_hash,
        dry_run: false,
        master_seed: Some(job.master_seed),
        job_seed: Some(job.job_seed),
        bootstrap_method: job.bootstrap_method,
        bootstrap_n: job.bootstrap_n,
        null_method: job.null_method,
        null_n: job.null_n,
        #[cfg(any(test, feature = "test-internal"))]
        sleep_after_first_finding_ms: None,
    }
}

/// Compute the BH-FDR family key for a `scan_id@version` per the
/// `[fdr].family` scope:
/// - `"scan_id"` (default) — per-`scan_id@version` family.
/// - `"scan_family"` — by `scan_id` prefix before the first `.`
///   (e.g. `"stats"`, `"cross"`, `"seas"`).
/// - `"all"` — single family `"all"` for the whole sweep.
/// - `"none"` — `None` (caller skips; `SweepSummary` still emits with
///   empty `fdr_by_family`).
/// - unknown — falls back to `"scan_id"` (defensive default).
#[allow(
    clippy::match_same_arms,
    reason = "the `_` wildcard fallback intentionally mirrors the `\"scan_id\"` arm body — the alternative (omit the explicit arm) would silently treat invalid family values as `\"scan_id\"` without surfacing the documented `_` defensive-fallback contract in the source"
)]
fn scope_family(scan_id_at_version: &str, fdr_family: &str) -> Option<String> {
    match fdr_family {
        "scan_id" => Some(scan_id_at_version.to_string()),
        "scan_family" => Some(family_prefix(scan_id_at_version)),
        "all" => Some("all".to_string()),
        "none" => None,
        _ => Some(scan_id_at_version.to_string()),
    }
}

/// Extract the scan-family prefix (e.g. `"stats"`, `"cross"`, `"seas"`)
/// from a `scan_id@version` string. Splits on the first `'.'`; falls
/// back to the full string if no dot is present.
fn family_prefix(scan_id_at_version: &str) -> String {
    let id_only = scan_id_at_version
        .split_once('@')
        .map_or(scan_id_at_version, |(id, _v)| id);
    id_only
        .split_once('.')
        .map_or_else(|| id_only.to_string(), |(prefix, _rest)| prefix.to_string())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(
    clippy::doc_markdown,
    reason = "test names + plan tags in doc-comments are descriptive prose, not code identifiers"
)]
mod tests {
    use super::*;

    /// Plan 05-04 Task 2 — `family_prefix` splits on the first `.`
    /// before `@version`.
    #[test]
    fn family_prefix_extracts_first_dot_segment() {
        assert_eq!(family_prefix("stats.autocorr.ljung_box@1"), "stats");
        assert_eq!(family_prefix("cross.corr.pearson_rolling@1"), "cross");
        assert_eq!(family_prefix("seas.bucket.hour_of_day@1"), "seas");
        // No `.` — full prefix as fallback.
        assert_eq!(family_prefix("scan_id_only@1"), "scan_id_only");
    }

    /// Plan 05-04 Task 2 — `scope_family` dispatches the four `[fdr].family`
    /// values.
    #[test]
    fn scope_family_dispatches_all_four_values() {
        let sid = "stats.autocorr.ljung_box@1";
        assert_eq!(
            scope_family(sid, "scan_id"),
            Some("stats.autocorr.ljung_box@1".to_string())
        );
        assert_eq!(
            scope_family(sid, "scan_family"),
            Some("stats".to_string())
        );
        assert_eq!(scope_family(sid, "all"), Some("all".to_string()));
        assert_eq!(scope_family(sid, "none"), None);
        // Unknown -> fallback to scan_id.
        assert_eq!(
            scope_family(sid, "garbage"),
            Some("stats.autocorr.ljung_box@1".to_string())
        );
    }

    /// Plan 05-04 Task 2 / Test 6 (FOUND-04 invariant) — sweep executor
    /// does NOT pull in tokio / async-std via rayon.
    ///
    /// This test compiles only if no async dep is required by the
    /// `run_sweep` signature. The actual transitive-dep check lives in
    /// the verify command (`cargo tree -p miner-core -e normal,build |
    /// grep -E 'tokio|async-std'` empty).
    #[test]
    fn run_sweep_signature_compiles_without_async_runtime() {
        // Compile-only assertion: SweepOptions implements Send + Sync
        // (rayon's par_iter implicitly requires Sync over the shared
        // state — our cancel: Arc<AtomicBool> + the registry both
        // satisfy this trivially).
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<SweepOptions>();
        assert_send_sync::<crate::sweep::job_graph::ResolvedJob>();
    }

    /// WR-02: a `MinerError::Preflight` returned from
    /// `run_one_with_registry` is synthesised into a `Finding::ScanError`
    /// envelope carrying the typed preflight `code` + `message` so the
    /// sweep output surfaces the rejection rather than silently dropping
    /// the job.
    #[test]
    fn build_synthetic_per_job_error_preserves_preflight_code_and_message() {
        use crate::aggregator::Timeframe;
        use crate::error::{PreflightCode, WireError};
        use crate::findings::TimeRange;
        use crate::reader::{Blake3Hex, ClosedRangeUtc, InstrumentSpec, Side};
        use chrono::TimeZone;

        let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2024, 2, 1, 0, 0, 0).unwrap();
        let window = ClosedRangeUtc { start, end };
        let req = ScanRequest {
            scan_id: "stats.autocorr.ljung_box".to_string(),
            version: 1,
            instruments: vec![InstrumentSpec {
                symbol: "EURUSD".into(),
                side: Side::Bid,
            }],
            timeframe: Timeframe::Tf15m,
            window,
            sub_range: TimeRange {
                start_utc: start,
                end_utc: end,
            },
            gap_policy: crate::GapPolicyKind::ContinuousOnly,
            resolved_params: serde_json::json!({}),
            param_hash: Blake3Hex::from_hex_bytes(&[b'0'; 64]),
            dry_run: false,
            master_seed: None,
            job_seed: None,
            bootstrap_method: None,
            bootstrap_n: None,
            null_method: None,
            null_n: None,
            #[cfg(any(test, feature = "test-internal"))]
            sleep_after_first_finding_ms: None,
        };
        let err = MinerError::Preflight(WireError::preflight(
            PreflightCode::HygieneNotSupported,
            "synth: scan does not support requested null method",
        ));
        let run_id = RunId::new();
        let synth = build_synthetic_per_job_error(&req, &err, run_id);
        let Finding::ScanError(se) = synth else {
            panic!("expected Finding::ScanError; got something else");
        };
        assert_eq!(se.error_code, "hygiene_not_supported");
        assert!(
            se.message
                .contains("does not support requested null method"),
            "message lost: {}",
            se.message
        );
        assert_eq!(se.run_id, run_id, "run_id threaded through unchanged");
        assert_eq!(
            se.scan_id_at_version, "stats.autocorr.ljung_box@1",
            "scan_id@version composed from req fields"
        );
        assert_eq!(
            se.data_slice.sources.len(),
            1,
            "single-instrument source vector"
        );
    }
}
