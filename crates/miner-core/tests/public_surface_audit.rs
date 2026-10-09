//! FROZEN public surface audit (Plan 03-05).
//!
//! Pure compile-time gate. Every Phase 3 scan-engine name in the frozen block MUST be
//! reachable through `use miner_core::*` re-exports. If a future refactor accidentally
//! removes a re-export (or names a new internal module that shadows one), this test fails
//! to compile.
//!
//! The Phase 2 half of this audit (the reader trait, calendar, aggregator, gap detector
//! and bar cache) left with the data layer: those names are `tradedesk-data`'s public
//! surface now, and the miner names them through `tradedesk_data`.

use tradedesk_data::{Blake3Hex, Calendar, ClosedRangeUtc, Reader, Side};

// ===========================================================================
// Phase 3 (03-05) public surface audit, scoped to the names landed by the scan
// engine facade. Every Phase 3 type listed in 03-CONTEXT.md's frozen-block
// extension MUST be reachable through `use miner_core::*` re-exports.
// ===========================================================================

use miner_core::{
    DryRunFinding, GapDispatch, GapPolicyKind, LjungBoxScan, Registry, RunOutcome, Scan, ScanCtx,
    ScanError, ScanFindingShape, ScanRequest, bootstrap, run_one,
};

#[test]
#[allow(unused_variables, clippy::type_complexity)]
fn phase_3_public_surface_present() {
    // ---------------------------------------------------------------------
    // 03-02 surface — Scan trait + support types + Registry + bootstrap
    // ---------------------------------------------------------------------

    // Scan is a trait — coerce LjungBoxScan to `&dyn Scan` to prove dyn-safety.
    let scan = LjungBoxScan;
    let dyn_scan: &dyn Scan = &scan;
    assert_eq!(dyn_scan.id(), "stats.autocorr.ljung_box");
    assert_eq!(dyn_scan.version(), 1);

    // ScanFindingShape — declarative emit-shape (compile-time consts).
    let shape: ScanFindingShape = dyn_scan.finding_fields();
    assert!(shape.effect_extra_keys.contains(&"lags"));

    // Registry — BTreeMap-backed catalogue.
    let registry: Registry = bootstrap();
    assert!(registry.iter().count() >= 1);

    // ScanRequest / ScanCtx — name the types via predicate closures.
    let scan_request_predicate: &dyn Fn(&ScanRequest) = &|_r: &ScanRequest| ();
    let scan_ctx_predicate: &dyn Fn(&ScanCtx<'_>) = &|_c: &ScanCtx<'_>| ();

    // ScanError — name the type.
    let scan_err_name = std::any::type_name::<ScanError>();
    assert!(scan_err_name.contains("ScanError"));

    // ---------------------------------------------------------------------
    // 03-03 / 03-04 surface — engine facade + gap-policy types
    // ---------------------------------------------------------------------

    // GapPolicyKind / GapDispatch / RunOutcome — name the types.
    let gp: GapPolicyKind = GapPolicyKind::ContinuousOnly;
    assert_eq!(gp.as_str(), "continuous_only");
    let gd_predicate: &dyn Fn(&GapDispatch) = &|_g: &GapDispatch| ();
    let outcome: RunOutcome = RunOutcome::Ok;
    assert_eq!(outcome, RunOutcome::Ok);

    // run_one — fn-pointer type ascription forces the FUNCTION ITEM to be
    // reachable via `miner_core::run_one` (not via `miner_core::engine::run_one`).
    let run_one_fn: fn(
        &ScanRequest,
        &miner_core::MinerConfig,
        &EmptyReader,
        &mut dyn miner_core::FindingSink,
        std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<RunOutcome, miner_core::MinerError> = run_one::<EmptyReader>;
    assert!(
        (run_one_fn as usize) != 0,
        "run_one fn pointer must be non-null"
    );

    // ---------------------------------------------------------------------
    // 03-02 surface — DryRunFinding (the new Finding variant body type)
    // ---------------------------------------------------------------------
    let dry_run_predicate: &dyn Fn(&DryRunFinding) = &|_d: &DryRunFinding| ();
}

// ===========================================================================
// Minimal EmptyReader stub used to instantiate the `run_one::<R>` signature.
// Carries the same shape as DukascopyReader without reading any data.
// ===========================================================================

struct EmptyReader;

impl Reader for EmptyReader {
    type Error = std::io::Error;

    fn source_id(&self) -> &'static str {
        "empty"
    }

    fn trading_calendar(&self) -> Calendar {
        Calendar::fx_major()
    }

    fn read_1m_bars<'a>(
        &'a self,
        _symbol: &str,
        _side: Side,
        _range: ClosedRangeUtc,
    ) -> Result<tradedesk_data::reader::RawBarIter<'a, Self::Error>, Self::Error> {
        Ok(Box::new(std::iter::empty()))
    }

    fn fingerprint_day(
        &self,
        _symbol: &str,
        _side: Side,
        _date: chrono::NaiveDate,
    ) -> Result<Option<Blake3Hex>, Self::Error> {
        Ok(None)
    }

    fn enumerate_days(
        &self,
        _symbol: &str,
        _side: Side,
        _range: ClosedRangeUtc,
    ) -> Result<Vec<chrono::NaiveDate>, Self::Error> {
        Ok(Vec::new())
    }
}
