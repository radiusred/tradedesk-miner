//! M33-R3 — the coverage-only dry run.
//!
//! [`job_coverage`] answers "which sub-ranges would this job scan?" without
//! running the scan. It detects the gaps of each leg from the cache's
//! timestamps only (`GapDetector::detect`, which reads through
//! `Reader::read_1m_timestamps`: no OHLC or volume field is parsed, so a day
//! whose timestamps are valid gives the same coverage whatever its price
//! fields hold), then takes the same gap-policy steps
//! the run takes ([`gap_policy::open_gaps_at_timeframe`], [`gap_policy::dispatch`],
//! [`gap_policy::drop_closed_subranges`], [`gap_policy::snap_subranges_to_timeframe`]).
//! No bars are aggregated and no statistic is computed.
//!
//! A reader error is reported in the entry (`status: error`), not as a
//! `scan_error` envelope: a dry run touches no `RunSummary` counter.

use tradedesk_data::gap::{GapDetector, GapManifest};
use tradedesk_data::reader::Reader;

use crate::engine::gap_policy::{self, GapDispatch};
use crate::findings::{CoverageStatus, JobCoverage, Source, TimeRange};
use crate::scan::ScanRequest;

/// Compute the coverage entry for one job.
///
/// `req.instruments` has one leg (single scan) or two legs (a CROSS scan,
/// whose joint gaps are the union of both legs' gaps, as in
/// [`gap_policy::dispatch_pair_at_timeframe`]).
///
/// # Panics
/// When `req.instruments` is empty. Preflight (`validate_arity`) rejects such
/// a request before a dry run starts.
#[must_use]
pub fn job_coverage<R: Reader>(job_index: u64, req: &ScanRequest, reader: &R) -> JobCoverage {
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
    let mut entry = JobCoverage {
        job_index,
        scan_id_at_version: format!("{}@{}", req.scan_id, req.version),
        sources,
        window: TimeRange {
            start_utc: req.window.start,
            end_utc: req.window.end,
        },
        gap_policy: req.gap_policy.as_str().to_string(),
        status: CoverageStatus::Covered,
        sub_ranges: Vec::new(),
        gaps: Vec::new(),
        message: None,
    };

    let mut manifests: Vec<GapManifest> = Vec::with_capacity(req.instruments.len());
    for leg in &req.instruments {
        match GapDetector::detect(reader, &leg.symbol, leg.side, req.window) {
            Ok(m) => manifests.push(m),
            Err(e) => {
                entry.status = CoverageStatus::Error;
                entry.message = Some(format!("reader: {e}"));
                return entry;
            }
        }
    }
    let joint = manifests
        .into_iter()
        .reduce(|a, b| crate::scan::primitives::time_alignment::intersect_gaps(&a, &b))
        .expect("ScanRequest.instruments must be non-empty post-preflight (D4-02)");

    let effective = gap_policy::open_gaps_at_timeframe(&joint, req.timeframe, &req.calendar);
    entry.gaps = effective
        .gaps
        .iter()
        .filter(|g| g.end_utc > req.window.start && g.start_utc < req.window.end)
        .map(|g| TimeRange {
            start_utc: g.start_utc.max(req.window.start),
            end_utc: g.end_utc.min(req.window.end),
        })
        .collect();

    let dispatched = gap_policy::drop_closed_subranges(
        gap_policy::dispatch(&effective, req.window, req.gap_policy),
        req.window,
        &req.calendar,
    );
    match dispatched {
        GapDispatch::Aborted(_) => entry.status = CoverageStatus::GapAborted,
        GapDispatch::SubRanges(subs) => {
            entry.sub_ranges = gap_policy::snap_subranges_to_timeframe(subs, req.timeframe);
        }
    }
    entry
}
