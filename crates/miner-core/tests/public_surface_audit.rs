//! Plan 02-06 / Task 2 — FROZEN public surface audit (CACHE / T-02-20).
//!
//! Pure compile-time gate. Every Phase 2 type listed in 02-CONTEXT.md's
//! "frozen public-surface block" MUST be reachable through `use miner_core::*`
//! re-exports. If a future refactor accidentally removes a re-export (or names
//! a new internal module that shadows one), this test fails to compile.
//!
//! ## Why a separate audit test
//!
//! The integration tests in `tests/aggregator_*.rs`, `tests/cache_smoke.rs`,
//! `tests/gap_manifest_snapshot.rs` etc. each use SOME subset of the FROZEN
//! surface — but not all of it. A consumer who needs (say) `BarFrame` +
//! `Calendar` together must be able to name them both via `miner_core::*`
//! without reaching into `miner_core::aggregator::*` or `miner_core::cache::*`.
//! This audit test names every Phase 2 type explicitly so a missing re-export
//! is caught at the source of the contract instead of in some downstream
//! consumer crate.
//!
//! Phase 1's own re-export surface (`Finding`, `MinerConfig`, etc.) is
//! intentionally NOT re-audited here — Phase 1's `tests/schema_roundtrip.rs`
//! already exercises that surface. This file is Phase 2 specific.

// Force the FROZEN re-exports to be named through `miner_core::*` only —
// NOT through `miner_core::aggregator::*` / `miner_core::cache::*` /
// `miner_core::gap::*` / `miner_core::reader::*` / `miner_core::calendar::*`.
// Naming through the inner modules would bypass the audit (the modules are
// `pub` for convenience but the FROZEN surface contract is what downstream
// consumers compile against).
use chrono::TimeZone;
use miner_core::{
    AGGREGATOR_VERSION, ARROW_SCHEMA_VERSION, AggParams, AggregateError, BarCache, BarFrame,
    Blake3Hex, CacheError, Calendar, ClosedRangeUtc, FingerprintSidecar, GapDetector, GapManifest,
    GapReason, GapSpan, RawBar, Reader, Side, Timeframe, aggregate, build_arrow_schema,
};

/// Every name in the FROZEN block must be reachable + usable. The test body
/// constructs basic values and reads basic consts to force every name to be a
/// real (non-fictitious) re-export.
#[test]
#[allow(unused_variables)] // Audit-test: many bindings exist solely to force a name resolution.
fn phase_2_public_surface_present() {
    // ---------------------------------------------------------------------
    // 02-01 surface (Reader trait + supporting types + Calendar)
    // ---------------------------------------------------------------------

    let bid: Side = Side::Bid;
    let ask: Side = Side::Ask;

    // ClosedRangeUtc — public struct with two `DateTime<Utc>` fields.
    let range: ClosedRangeUtc = ClosedRangeUtc {
        start: chrono::Utc
            .with_ymd_and_hms(2024, 6, 12, 0, 0, 0)
            .single()
            .expect("valid"),
        end: chrono::Utc
            .with_ymd_and_hms(2024, 6, 13, 0, 0, 0)
            .single()
            .expect("valid"),
    };

    // RawBar — public struct, all fields public per Plan 02-01.
    let raw: RawBar = RawBar {
        ts_open_utc: range.start,
        ts_close_utc: range.start + chrono::Duration::minutes(1),
        open: 1.0,
        high: 1.0,
        low: 1.0,
        close: 1.0,
        tick_volume: 1.0,
    };

    // Blake3Hex — manual `from_hex_bytes` constructor (private inner field).
    let hex: Blake3Hex = Blake3Hex::from_hex_bytes(&[b'0'; 64]);

    // Reader is a trait — coerce `EmptyReader` to `&dyn Reader<…>` to prove
    // the trait is reachable AND dyn-compatible at the audit site (in
    // addition to `tests/reader_trait_object_safety.rs`).
    let empty = EmptyReader;
    let dyn_reader: &dyn Reader<Error = std::io::Error> = &empty;
    assert_eq!(dyn_reader.source_id(), "empty");

    // Calendar — public struct + FX-major default via `Default` or `fx_major()`.
    let cal: Calendar = Calendar::fx_major();
    let cal_default: Calendar = Calendar::default();
    assert_eq!(
        cal, cal_default,
        "Calendar::default() must equal fx_major()"
    );

    // ---------------------------------------------------------------------
    // 02-02 surface (aggregator)
    // ---------------------------------------------------------------------

    // AGGREGATOR_VERSION const reachable + parses as 1.x.y.
    let agg_ver: &'static str = AGGREGATOR_VERSION;
    assert!(
        agg_ver.starts_with("1."),
        "AGGREGATOR_VERSION must be `1.x.y` form; got {agg_ver:?}"
    );

    // Timeframe enum — three variants.
    let tf_15m: Timeframe = Timeframe::Tf15m;
    let tf_hourly: Timeframe = Timeframe::Tf1h;
    let tf_daily: Timeframe = Timeframe::Tf1d;

    // AggParams — public-fields struct (the aggregate fn's input).
    let params: AggParams<'_> = AggParams {
        symbol: "EURUSD",
        side: bid,
        tf: tf_15m,
        range,
    };

    // BarFrame — name the type. Constructing a BarFrame here would require
    // accessing its public fields; we just type-check that the name is
    // reachable via the predicate closure.
    let bar_frame_predicate: &dyn Fn(&BarFrame) = &|_f: &BarFrame| ();

    // AggregateError — name the type via `type_name` so the import is used.
    let agg_err_name = std::any::type_name::<AggregateError<std::io::Error>>();
    assert!(agg_err_name.contains("AggregateError"));

    // Capture `aggregate` as a value pointing at a concrete instantiation.
    // This forces the FUNCTION ITEM to be reachable via
    // `miner_core::aggregate` (not via `miner_core::aggregator::aggregate`).
    // The type ascription on the binding is the compile-time gate; the
    // runtime non-null check on the fn-pointer just keeps the binding alive
    // through optimisation.
    let aggregate_fn: fn(
        &EmptyReader,
        AggParams<'_>,
    ) -> Result<BarFrame, AggregateError<std::io::Error>> = aggregate::<EmptyReader>;
    assert!(
        (aggregate_fn as usize) != 0,
        "aggregate fn pointer must be non-null"
    );

    // ---------------------------------------------------------------------
    // 02-04 surface (gap detector + manifest)
    // ---------------------------------------------------------------------

    let gap_detector: GapDetector = GapDetector;
    let gap_manifest_predicate: &dyn Fn(&GapManifest) = &|_m: &GapManifest| ();
    let gap_span_predicate: &dyn Fn(&GapSpan) = &|_s: &GapSpan| ();
    let gap_reason_predicate: &dyn Fn(&GapReason) = &|_r: &GapReason| ();

    // ---------------------------------------------------------------------
    // 02-05 surface (cache + fingerprint sidecar + Arrow schema builder)
    // ---------------------------------------------------------------------

    let schema_ver: &'static str = ARROW_SCHEMA_VERSION;
    assert!(
        schema_ver.starts_with("1."),
        "ARROW_SCHEMA_VERSION must be `1.x.y` form; got {schema_ver:?}"
    );

    let cache: BarCache = BarCache::new("/tmp/audit-cache-root");
    let cache_err_predicate: &dyn Fn(&CacheError) = &|_e: &CacheError| ();
    let sidecar_predicate: &dyn Fn(&FingerprintSidecar) = &|_f: &FingerprintSidecar| ();

    let schema = build_arrow_schema("dukascopy", "EURUSD", bid, tf_15m);
    assert_eq!(
        schema.fields().len(),
        7,
        "Arrow schema has 7 fields per Plan 02-05 (ts_open_utc, ts_close_utc, open, high, low, close, tick_volume)"
    );
}

// ===========================================================================
// Phase 3 (03-05) public surface audit — same discipline as Phase 2 above,
// scoped to the new names landed by the scan engine facade. Every Phase 3
// type listed in 03-CONTEXT.md's frozen-block extension MUST be reachable
// through `use miner_core::*` re-exports.
// ===========================================================================

use miner_core::{
    DryRunFinding, GapDispatch, GapPolicyKind, LjungBoxScan, Registry, RunOutcome, Scan, ScanCtx,
    ScanError, ScanFindingShape, ScanRequest, bootstrap, run_one,
};

#[test]
#[allow(unused_variables)]
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
    assert!((run_one_fn as usize) != 0, "run_one fn pointer must be non-null");

    // ---------------------------------------------------------------------
    // 03-02 surface — DryRunFinding (the new Finding variant body type)
    // ---------------------------------------------------------------------
    let dry_run_predicate: &dyn Fn(&DryRunFinding) = &|_d: &DryRunFinding| ();
}

// ===========================================================================
// Minimal EmptyReader stub used to instantiate the `aggregate::<R>` signature
// + coerce to `&dyn Reader<…>` from within the audit test. Carries the same
// dyn-compatible shape as DukascopyReader without pulling in the sibling
// reader crate.
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
    ) -> Result<miner_core::reader::RawBarIter<'a, Self::Error>, Self::Error> {
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
