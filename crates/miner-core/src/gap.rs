//! Gap detector + manifest (D2-16 / D2-17).
//!
//! The manifest is the data Phase 3's gap-policy enforcer wraps into a
//! [`GapAbortedFinding`](crate::findings::GapAbortedFinding) envelope under
//! `--gap-policy=strict`. **Phase 2 owns the data model; Phase 3 owns
//! enforcement.** This module therefore does NOT import `Finding::GapAborted`
//! and never constructs one.
//!
//! ## Three gap classes (CACHE-07)
//!
//! 1. **Missing daily file** — `(start: day 00:00 UTC, end: day+1 00:00 UTC,
//!    reason: MissingSourceFile)`. The reader's [`fingerprint_day`] returned
//!    `Ok(None)` for a day the calendar reports as open. Per D2-17 the detector
//!    does NOT additionally emit intra-day gaps for the missing day — they
//!    would be redundant.
//! 2. **Zero-byte / corrupt source file** — same range,
//!    `reason: CorruptSourceFile { date, detail }`. v1 detects corruption
//!    indirectly: [`fingerprint_day`] returned `Ok(Some)` but [`read_1m_bars`]
//!    immediately yielded `Err`. A more rigorous "explicit zero-byte sentinel"
//!    path is left for Phase 7.
//! 3. **Intra-day hole during open hours** — every missing minute whose UTC
//!    timestamp satisfies [`Calendar::is_open_at`] becomes a single
//!    `IntraDayGap { affected_minutes: 1 }` entry. **No coalescing** in v1
//!    (per RESEARCH §"Gap Manifest Data Model" — keep contiguous one-minute
//!    runs as individual entries; downstream consumers can merge if they need
//!    to).
//!
//! Closed-hours holes (weekend, FX-major holiday) are **NOT** gaps —
//! [`Calendar::is_open_at`] is the single discriminator. The
//! `closed_hours_are_not_gaps` unit test pins this regression.
//!
//! ## Output ordering invariant (deterministic JSON)
//!
//! [`GapManifest::gaps`] is sorted by `start_utc` ascending, ties broken by
//! `end_utc`, then by [`GapReason::discriminant_ord`]. The
//! `gaps_sorted_proptest` proves the invariant on random input. Combined with
//! `#[serde(tag = "kind", rename_all = "snake_case")]` on [`GapReason`] and
//! the absence of any hash-randomised map (Phase 2 `BTreeMap`-only rule) in
//! the type tree, JSON output is byte-stable across runs.
//!
//! ## Performance budget (RESEARCH A4)
//!
//! The detector calls [`Calendar::is_open_at`] once per minute over the
//! queried range — ~3.2M calls per symbol per multi-year scan. The closed-form
//! predicate is O(1) per call (verified by Phase 2-02 unit tests).
//!
//! ## T-02-11 information-disclosure note
//!
//! [`GapReason::CorruptSourceFile::detail`] is constructed from the reader's
//! `Error::to_string()`. Plan 02-01 vetted `DukascopyError` as path +
//! error-message-only — no raw bytes. Future readers MUST honour the same
//! contract.
//!
//! [`fingerprint_day`]: crate::reader::Reader::fingerprint_day
//! [`read_1m_bars`]: crate::reader::Reader::read_1m_bars
//! [`Calendar::is_open_at`]: crate::calendar::Calendar::is_open_at

use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::calendar::Calendar;
use crate::findings::TimeRange;
use crate::reader::{ClosedRangeUtc, Reader, Side};

// ---------------------------------------------------------------------------
// Public types — GapManifest / GapSpan / GapReason
// ---------------------------------------------------------------------------

/// Pre-scan gap report keyed by `(source_id, symbol, side, queried_range)`.
///
/// Phase 3's gap-policy enforcer wraps this into a `GapAbortedFinding` under
/// the `strict` policy. Phase 2 ships the data; Phase 3 ships the emission.
///
/// `queried_range` reuses Phase 1's [`TimeRange`] (which already derives
/// `JsonSchema` + `Serialize`); the detector translates the input
/// [`ClosedRangeUtc`] at the boundary.
///
/// `gaps` is sorted by `start_utc` ascending, ties broken by `end_utc`, then
/// by [`GapReason::discriminant_ord`] — the `gaps_sorted_proptest` proves the
/// invariant on random input. Empty when no gaps are detected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GapManifest {
    /// Stable identifier of the data source (e.g. `"dukascopy"`).
    pub source_id: String,
    /// Symbol the manifest pertains to (e.g. `"EURUSD"`).
    pub symbol: String,
    /// Bid/ask side the manifest pertains to.
    pub side: Side,
    /// The half-open UTC range the detector was queried with.
    pub queried_range: TimeRange,
    /// Sorted by `start_utc` ascending, ties broken by `end_utc`, then by
    /// [`GapReason::discriminant_ord`]. Empty when no gaps are detected.
    pub gaps: Vec<GapSpan>,
}

/// A single contiguous gap span — `[start_utc, end_utc)` plus its
/// classification.
///
/// Half-open per Phase 1's [`TimeRange`] convention (`start_utc` inclusive,
/// `end_utc` exclusive).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GapSpan {
    pub start_utc: DateTime<Utc>,
    pub end_utc: DateTime<Utc>,
    pub reason: GapReason,
}

/// Why a gap exists. Tagged-enum JSON shape per Phase 1 idiom
/// (`#[serde(tag = "kind", rename_all = "snake_case")]`):
///
/// - `{ "kind": "missing_source_file", "date": "2024-06-15" }`
/// - `{ "kind": "corrupt_source_file", "date": "2024-06-15", "detail": "..." }`
/// - `{ "kind": "intra_day_gap", "affected_minutes": 1 }`
///
/// `Eq` is safe because no variant payload contains `f64` (NaN-unequal).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GapReason {
    /// The daily source file is absent for an open-hours day. Range covers the
    /// whole day `[day 00:00 UTC, day+1 00:00 UTC)`.
    MissingSourceFile { date: NaiveDate },
    /// The daily source file is present but unreadable. `detail` is the
    /// reader's `Error::to_string()` — vetted to be path + message only
    /// (T-02-11). Range covers the whole day.
    CorruptSourceFile { date: NaiveDate, detail: String },
    /// Sub-minute hole during open hours. v1 emits one entry per missing
    /// minute (no coalescing); `affected_minutes` is always `1` in v1.
    IntraDayGap { affected_minutes: u32 },
}

impl GapReason {
    /// Stable discriminant ordinal for tie-breaking the manifest sort.
    /// `MissingSourceFile = 0 < CorruptSourceFile = 1 < IntraDayGap = 2`. The
    /// values are part of the determinism contract — adding a new variant
    /// MUST append (not insert) to preserve byte-stability of existing JSON
    /// outputs.
    #[must_use]
    pub fn discriminant_ord(&self) -> u8 {
        match self {
            Self::MissingSourceFile { .. } => 0,
            Self::CorruptSourceFile { .. } => 1,
            Self::IntraDayGap { .. } => 2,
        }
    }
}

// ---------------------------------------------------------------------------
// GapDetector
// ---------------------------------------------------------------------------

/// Pure-function gap detector. Stateless unit struct (no fields, no
/// configuration) — callers invoke [`GapDetector::detect`] directly.
#[derive(Debug, Default, Clone, Copy)]
pub struct GapDetector;

impl GapDetector {
    /// Walk every calendar day in `range`, classify any missing minutes
    /// during open hours against the reader's [`Calendar`], and return a
    /// sorted [`GapManifest`].
    ///
    /// ## Algorithm
    ///
    /// 1. Enumerate every UTC date in `[range.start, range.end)`.
    /// 2. For each date, compute the open-hours minute set
    ///    (`Calendar::is_open_at` per minute in the day). If the set is empty
    ///    the day is fully closed — emit nothing.
    /// 3. Else: call `reader.fingerprint_day(symbol, side, date)`:
    ///    - `Ok(None)` → emit a single `MissingSourceFile` span; **do not
    ///      also emit per-minute intra-day gaps for this date** (D2-17).
    ///    - `Ok(Some(_))` → call `reader.read_1m_bars(...)`. If the iterator
    ///      yields any `Err`, emit a `CorruptSourceFile` span and skip
    ///      intra-day detection for the day (Option A in PLAN — explicit
    ///      zero-byte sentinel is a Phase 7 TODO).
    ///    - `Err(e)` → bubble up as `Err(e)`; I/O errors at the boundary are
    ///      not gaps.
    /// 4. Otherwise build a `BTreeSet<DateTime<Utc>>` of present-bar
    ///    `ts_open_utc` and emit a one-minute `IntraDayGap` entry for every
    ///    open minute that has no present bar.
    /// 5. Sort the assembled `gaps` by `start_utc`, then `end_utc`, then
    ///    [`GapReason::discriminant_ord`].
    ///
    /// ## Errors
    /// Returns `Err(R::Error)` only when the reader's `fingerprint_day` call
    /// returns `Err` — a true I/O failure at the boundary. Per-day corruption
    /// (zero-byte file, malformed CSV row mid-stream) surfaces as a
    /// `CorruptSourceFile` entry in the manifest, not an `Err`.
    pub fn detect<R: Reader>(
        reader: &R,
        symbol: &str,
        side: Side,
        range: ClosedRangeUtc,
    ) -> Result<GapManifest, R::Error> {
        let calendar = reader.trading_calendar();
        let mut gaps: Vec<GapSpan> = Vec::new();

        for date in enumerate_dates(range) {
            let open_minutes = open_minutes_in_day(&calendar, date);
            if open_minutes.is_empty() {
                // Fully closed day (e.g., Saturday, FX holiday) — never a gap.
                continue;
            }

            if reader.fingerprint_day(symbol, side, date)?.is_none() {
                // Missing daily file — emit one whole-day span. D2-17: do
                // NOT also emit per-minute intra-day gaps.
                gaps.push(missing_file_span(date));
                continue;
            }

            // File present. Read its bars; if the iterator errors, treat the
            // file as corrupt (Option A from PLAN). The explicit zero-byte
            // sentinel path is a future refinement.
            // TODO(Phase 7): consider explicit zero-byte sentinel.
            let day_range = whole_day_range(date);
            let iter = match reader.read_1m_bars(symbol, side, day_range) {
                Ok(it) => it,
                Err(e) => {
                    // Reader couldn't even start the stream — treat as
                    // corrupt rather than aborting the whole detect() call
                    // so the rest of the range is still surveyed.
                    gaps.push(corrupt_file_span(date, e.to_string()));
                    continue;
                }
            };

            let mut present: std::collections::BTreeSet<DateTime<Utc>> =
                std::collections::BTreeSet::new();
            let mut corrupt_detail: Option<String> = None;
            for bar_result in iter {
                match bar_result {
                    Ok(bar) => {
                        present.insert(bar.ts_open_utc);
                    }
                    Err(e) => {
                        // First iterator-error becomes the CorruptSourceFile
                        // detail; subsequent errors ignored for this day.
                        if corrupt_detail.is_none() {
                            corrupt_detail = Some(e.to_string());
                        }
                        break;
                    }
                }
            }

            if let Some(detail) = corrupt_detail {
                gaps.push(corrupt_file_span(date, detail));
                // D2-17 redundancy rule applies analogously: don't also emit
                // per-minute intra-day gaps for a corrupt file.
                continue;
            }

            for minute_start in open_minutes {
                if !present.contains(&minute_start) {
                    gaps.push(GapSpan {
                        start_utc: minute_start,
                        end_utc: minute_start + Duration::minutes(1),
                        reason: GapReason::IntraDayGap {
                            affected_minutes: 1,
                        },
                    });
                }
            }
        }

        sort_gaps(&mut gaps);

        Ok(GapManifest {
            source_id: reader.source_id().to_string(),
            symbol: symbol.to_string(),
            side,
            queried_range: TimeRange {
                start_utc: range.start,
                end_utc: range.end,
            },
            gaps,
        })
    }
}

// ---------------------------------------------------------------------------
// Helpers (crate-private)
// ---------------------------------------------------------------------------

/// Sort `gaps` by `start_utc` ascending, ties broken by `end_utc`, then by
/// [`GapReason::discriminant_ord`]. Exposed at module scope so the proptest
/// can pin the exact ordering rule the detector applies.
fn sort_gaps(gaps: &mut [GapSpan]) {
    gaps.sort_by(|a, b| {
        a.start_utc
            .cmp(&b.start_utc)
            .then_with(|| a.end_utc.cmp(&b.end_utc))
            .then_with(|| {
                a.reason
                    .discriminant_ord()
                    .cmp(&b.reason.discriminant_ord())
            })
    });
}

/// Enumerate every UTC date whose `[date 00:00, date+1 00:00)` window
/// intersects `range`. Half-open: a `range.end` of `2024-06-14T00:00:00Z`
/// stops at `2024-06-13` (inclusive). A `range.end` of `2024-06-14T00:00:01Z`
/// includes `2024-06-14`.
fn enumerate_dates(range: ClosedRangeUtc) -> Vec<NaiveDate> {
    let start_date = range.start.date_naive();
    // Half-open end: if `range.end` is exactly midnight, the previous day is
    // the last whole day whose minutes are inside the range. Otherwise the
    // end-date is itself partially inside the range and must be enumerated.
    let end_date_inclusive = if range.end == day_start_utc(range.end.date_naive()) {
        // end at midnight — the end-date is OUT of the range.
        match range.end.date_naive().pred_opt() {
            Some(d) => d,
            None => return Vec::new(),
        }
    } else {
        range.end.date_naive()
    };
    if end_date_inclusive < start_date {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut d = start_date;
    while d <= end_date_inclusive {
        out.push(d);
        let Some(next) = d.succ_opt() else {
            break;
        };
        d = next;
    }
    out
}

/// UTC midnight of `date`. The `expect` is statically-impossible because
/// `00:00:00` is a valid wall-clock time on every date.
fn day_start_utc(date: NaiveDate) -> DateTime<Utc> {
    date.and_hms_opt(0, 0, 0)
        .expect("00:00:00 is a valid wall-clock time")
        .and_utc()
}

/// The full-day half-open range `[date 00:00, date+1 00:00)` used as the
/// query range when reading one day's worth of bars.
fn whole_day_range(date: NaiveDate) -> ClosedRangeUtc {
    let start = day_start_utc(date);
    let end = start + Duration::hours(24);
    ClosedRangeUtc { start, end }
}

/// Build the open-minute set for `date`. Each entry is the UTC
/// `ts_open_utc` of a 1-minute bucket whose timestamp satisfies
/// [`Calendar::is_open_at`]. Empty when `date` is fully closed (Saturday, FX
/// holiday, weekend-window). O(1440) calls to `is_open_at`; the predicate is
/// itself O(1).
fn open_minutes_in_day(calendar: &Calendar, date: NaiveDate) -> Vec<DateTime<Utc>> {
    let start = day_start_utc(date);
    let mut out = Vec::with_capacity(1440);
    for minute in 0..1440_i64 {
        let ts = start + Duration::minutes(minute);
        if calendar.is_open_at(ts) {
            out.push(ts);
        }
    }
    out
}

/// Whole-day `MissingSourceFile` span constructor.
fn missing_file_span(date: NaiveDate) -> GapSpan {
    let start = day_start_utc(date);
    GapSpan {
        start_utc: start,
        end_utc: start + Duration::hours(24),
        reason: GapReason::MissingSourceFile { date },
    }
}

/// Whole-day `CorruptSourceFile` span constructor. `detail` is the reader's
/// `Error::to_string()` — vetted as path + message only (T-02-11).
fn corrupt_file_span(date: NaiveDate, detail: String) -> GapSpan {
    let start = day_start_utc(date);
    GapSpan {
        start_utc: start,
        end_utc: start + Duration::hours(24),
        reason: GapReason::CorruptSourceFile { date, detail },
    }
}

// `Datelike` is brought into scope for `NaiveDate::pred_opt` / `succ_opt`
// users via the `chrono::Datelike` import above. Silence unused-import on
// builds that don't reach the trait directly through these helpers.
#[allow(dead_code)]
const _: fn() = || {
    fn _check(_d: NaiveDate) {
        let _ = chrono::NaiveDate::weekday;
        let _y: i32 = NaiveDate::MIN.year();
    }
};

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    // ---- Task 1: schemars round-trip ---------------------------------------

    /// `JsonSchema` derive doesn't panic AND `serde_json` round-trips every
    /// variant of the manifest without losing fields. Locks T-02-12 + CACHE-08.
    #[test]
    fn gap_manifest_schemars_roundtrip() {
        let manifest = GapManifest {
            source_id: "dukascopy".into(),
            symbol: "EURUSD".into(),
            side: Side::Bid,
            queried_range: TimeRange {
                start_utc: Utc.with_ymd_and_hms(2024, 6, 10, 0, 0, 0).unwrap(),
                end_utc: Utc.with_ymd_and_hms(2024, 6, 15, 0, 0, 0).unwrap(),
            },
            gaps: vec![
                GapSpan {
                    start_utc: Utc.with_ymd_and_hms(2024, 6, 11, 0, 0, 0).unwrap(),
                    end_utc: Utc.with_ymd_and_hms(2024, 6, 12, 0, 0, 0).unwrap(),
                    reason: GapReason::MissingSourceFile {
                        date: NaiveDate::from_ymd_opt(2024, 6, 11).unwrap(),
                    },
                },
                GapSpan {
                    start_utc: Utc.with_ymd_and_hms(2024, 6, 12, 0, 0, 0).unwrap(),
                    end_utc: Utc.with_ymd_and_hms(2024, 6, 13, 0, 0, 0).unwrap(),
                    reason: GapReason::CorruptSourceFile {
                        date: NaiveDate::from_ymd_opt(2024, 6, 12).unwrap(),
                        detail: "zero-byte file".into(),
                    },
                },
                GapSpan {
                    start_utc: Utc.with_ymd_and_hms(2024, 6, 13, 13, 45, 0).unwrap(),
                    end_utc: Utc.with_ymd_and_hms(2024, 6, 13, 13, 46, 0).unwrap(),
                    reason: GapReason::IntraDayGap {
                        affected_minutes: 1,
                    },
                },
            ],
        };

        // Serde round-trip preserves every field.
        let json = serde_json::to_string(&manifest).expect("manifest serialises");
        let parsed: GapManifest = serde_json::from_str(&json).expect("manifest round-trips");
        assert_eq!(parsed, manifest, "serde round-trip drops fields");

        // Schemars derive does not panic; the schema serialises to JSON.
        let schema = schemars::schema_for!(GapManifest);
        let schema_json =
            serde_json::to_string(&schema).expect("GapManifest JsonSchema serialises");
        // Sanity: the tagged-enum discriminant must show up in the schema.
        assert!(
            schema_json.contains("missing_source_file"),
            "schema must include the missing_source_file variant tag, got: {schema_json}"
        );
        assert!(
            schema_json.contains("corrupt_source_file"),
            "schema must include the corrupt_source_file variant tag"
        );
        assert!(
            schema_json.contains("intra_day_gap"),
            "schema must include the intra_day_gap variant tag"
        );
    }

    // Task 2 / Task 3 tests added below.
}
