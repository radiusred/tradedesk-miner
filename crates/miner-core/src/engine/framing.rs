//! `RunStart` / `RunEnd` framing-record builders (D-09, D-11).
//!
//! Pattern analog: `miner-cli/src/main.rs::emit_fixture` (lines 140-165) — the
//! existing `RunStart` + `RunEnd` construction with shared `RunId: Copy`.
//! Phase 3 lifts the pattern out of `emit-fixture` into pure builder functions
//! the facade calls.
//!
//! ## Clock-read discipline (D3-23)
//!
//! The wall-clock is NEVER read inside these builders. Callers pass
//! `started` (and `ended`) `DateTime<Utc>` values they captured at the
//! facade boundary; the builders return `Finding::RunStart` /
//! `Finding::RunEnd` values composed from those inputs verbatim. This is
//! the determinism guarantee: same inputs → same JSONL output bytes modulo
//! `run_id` + timestamps + `wall_clock_ms` (the four volatile fields the
//! determinism test masks; pattern: `cli_streams.rs:323-344`).
//!
//! ## `dry_run` echo (D3-21 / Blocker 2)
//!
//! [`build_run_start`] emits `req.dry_run` verbatim into the `request` JSON
//! Value as a `Value::Bool`. The field is ALWAYS present — even when `false`
//! — mirroring the `dsr` / `fdr_q` null-but-present discipline locked in
//! Phase 1. Downstream consumers (Plan 04's `run_one`, Plan 06's
//! `dry_run.rs` and `scan_ljung_box.rs` integration tests, Phase 6 MCP/HTTP
//! wrappers) structurally rely on the field being present in
//! `RunStart.request`. The `request` builder MUST NOT use
//! `#[serde(skip_serializing_if)]` and MUST NOT omit the field.

use chrono::{DateTime, SecondsFormat, Utc};

use crate::findings::run_id::RunId;
use crate::findings::{Finding, RunEnd, RunStart, RunSummary};
use crate::scan::ScanRequest;

/// Build the opening `Finding::RunStart` envelope.
///
/// `run_id` is supplied by the caller (so the facade can share the same
/// `RunId` across `RunStart` and `RunEnd` — relies on `RunId: Copy`).
/// `started` is the caller-captured wall-clock `DateTime<Utc>` reading (so
/// the caller can compute `wall_clock_ms` against the same baseline when it
/// later calls `build_run_end`).
///
/// `code_revision` is `miner_core::CODE_REVISION` at the call site — the
/// builder is `code_revision`-agnostic so tests can inject a stable string.
///
/// ## `request` echo shape (PATTERNS line 550)
///
/// The `request: serde_json::Value` field carries run-level metadata only:
/// - `scan_id@version` — combined as `"{id}@{version}"`
/// - `instrument` — string
/// - `side` — wire form ("bid" / "ask")
/// - `timeframe` — wire form ("15m" / "1h" / "1d")
/// - `window` — object with RFC 3339 `start` / `end` strings (Z suffix)
/// - `gap_policy` — wire form (`"strict"` / `"continuous_only"`)
/// - `resolved_params` — cloned from the request
/// - `dry_run` — boolean echo of `req.dry_run` (D3-21 / Blocker 2)
///
/// `run_id`, timestamps, `param_hash`, and `sub_range` are deliberately NOT
/// inside this Value — they live on the typed `RunStart` / per-finding
/// structs (Pitfalls 4 + 6).
#[must_use]
pub fn build_run_start(
    req: &ScanRequest,
    run_id: RunId,
    started: DateTime<Utc>,
    code_revision: &str,
) -> Finding {
    // Build the request echo Value. Use `serde_json::Map` (BTreeMap-backed via
    // serde_json's default — no `preserve_order`) so iteration / key order is
    // deterministic across runs.
    let mut request = serde_json::Map::new();
    request.insert(
        "scan_id@version".to_string(),
        serde_json::Value::String(format!("{}@{}", req.scan_id, req.version)),
    );
    request.insert(
        "instrument".to_string(),
        serde_json::Value::String(req.instrument.clone()),
    );
    request.insert(
        "side".to_string(),
        serde_json::Value::String(req.side.as_str().to_string()),
    );
    request.insert(
        "timeframe".to_string(),
        serde_json::Value::String(req.timeframe.as_str().to_string()),
    );
    let mut window = serde_json::Map::new();
    window.insert(
        "start".to_string(),
        serde_json::Value::String(req.window.start.to_rfc3339_opts(SecondsFormat::Secs, true)),
    );
    window.insert(
        "end".to_string(),
        serde_json::Value::String(req.window.end.to_rfc3339_opts(SecondsFormat::Secs, true)),
    );
    request.insert("window".to_string(), serde_json::Value::Object(window));
    request.insert(
        "gap_policy".to_string(),
        serde_json::Value::String(req.gap_policy.as_str().to_string()),
    );
    request.insert(
        "resolved_params".to_string(),
        req.resolved_params.clone(),
    );
    // Blocker 2 / D3-21: `dry_run` is ALWAYS present — never omitted. This is
    // the audit-trail signal Plan 04's run_one + Plan 06's integration tests
    // rely on. Do NOT add `skip_serializing_if`; the field appears even when
    // `false`.
    request.insert(
        "dry_run".to_string(),
        serde_json::Value::Bool(req.dry_run),
    );

    Finding::RunStart(RunStart {
        run_id,
        started_at_utc: started,
        miner_version: env!("CARGO_PKG_VERSION").to_string(),
        code_revision: code_revision.to_string(),
        request: serde_json::Value::Object(request),
    })
}

/// Build the closing `Finding::RunEnd` envelope.
///
/// `wall_clock_ms` is computed from `ended.signed_duration_since(started)`
/// (mirror `miner-cli/src/main.rs:158`). The builder does NOT read the
/// wall-clock — both timestamps are caller-supplied (D3-23).
#[must_use]
pub fn build_run_end(
    run_id: RunId,
    started: DateTime<Utc>,
    ended: DateTime<Utc>,
    summary: RunSummary,
) -> Finding {
    Finding::RunEnd(RunEnd {
        run_id,
        ended_at_utc: ended,
        wall_clock_ms: ended.signed_duration_since(started).num_milliseconds(),
        summary,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aggregator::Timeframe;
    use crate::engine::gap_policy::GapPolicyKind;
    use crate::findings::TimeRange;
    use crate::reader::{Blake3Hex, ClosedRangeUtc, Side};
    use chrono::TimeZone;

    fn blake3_hex_zero() -> Blake3Hex {
        let bytes: [u8; 64] = [b'0'; 64];
        Blake3Hex::from_hex_bytes(&bytes)
    }

    fn sample_request(dry_run: bool) -> ScanRequest {
        let start = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2026, 2, 1, 0, 0, 0).unwrap();
        ScanRequest {
            scan_id: "stats.autocorr.ljung_box".into(),
            version: 1,
            instrument: "EURUSD".into(),
            side: Side::Bid,
            timeframe: Timeframe::Tf15m,
            window: ClosedRangeUtc { start, end },
            sub_range: TimeRange {
                start_utc: start,
                end_utc: end,
            },
            gap_policy: GapPolicyKind::ContinuousOnly,
            resolved_params: serde_json::json!({"lags": 20}),
            param_hash: blake3_hex_zero(),
            dry_run,
            #[cfg(any(test, feature = "test-internal"))]
            sleep_after_first_finding_ms: None,
        }
    }

    /// `build_run_start` carries its `run_id`, `started`, `code_revision`,
    /// and request-echo fields verbatim.
    #[test]
    fn build_run_start_carries_inputs_verbatim() {
        let req = sample_request(false);
        let run_id = RunId::new();
        let started = Utc.with_ymd_and_hms(2026, 5, 18, 12, 0, 0).unwrap();
        let finding = build_run_start(&req, run_id, started, "abc123");
        let Finding::RunStart(rs) = finding else {
            panic!("expected Finding::RunStart");
        };
        assert_eq!(rs.run_id, run_id, "run_id must be passed through");
        assert_eq!(rs.started_at_utc, started, "started must be passed through");
        assert_eq!(
            rs.code_revision, "abc123",
            "code_revision must be passed through"
        );
        // Request-echo fields.
        assert_eq!(
            rs.request.get("scan_id@version"),
            Some(&serde_json::Value::String(
                "stats.autocorr.ljung_box@1".into()
            )),
            "scan_id@version must be echoed"
        );
        assert_eq!(
            rs.request.get("instrument"),
            Some(&serde_json::Value::String("EURUSD".into())),
            "instrument must be echoed"
        );
        assert_eq!(
            rs.request.get("side"),
            Some(&serde_json::Value::String("bid".into())),
            "side must be wire form"
        );
        assert_eq!(
            rs.request.get("timeframe"),
            Some(&serde_json::Value::String("15m".into())),
            "timeframe must be wire form"
        );
        assert_eq!(
            rs.request.get("gap_policy"),
            Some(&serde_json::Value::String("continuous_only".into())),
            "gap_policy must be wire form"
        );
        assert_eq!(
            rs.request.get("resolved_params"),
            Some(&serde_json::json!({"lags": 20})),
            "resolved_params must be cloned through"
        );
        // Window object with RFC3339-Z strings.
        let window = rs
            .request
            .get("window")
            .expect("window must be present")
            .as_object()
            .expect("window must be an object");
        assert_eq!(
            window.get("start"),
            Some(&serde_json::Value::String("2026-01-01T00:00:00Z".into()))
        );
        assert_eq!(
            window.get("end"),
            Some(&serde_json::Value::String("2026-02-01T00:00:00Z".into()))
        );
        // Audit-trail forbidden fields.
        assert!(
            rs.request.get("run_id").is_none(),
            "run_id must NOT live inside request (Pitfall 6)"
        );
        assert!(
            rs.request.get("param_hash").is_none(),
            "param_hash must NOT live inside request (Pitfall 6)"
        );
        assert!(
            rs.request.get("sub_range").is_none(),
            "sub_range must NOT live inside request (Pitfall 4)"
        );
    }

    /// `build_run_end` carries its inputs verbatim and computes `wall_clock_ms`
    /// as `ended.signed_duration_since(started).num_milliseconds()`.
    #[test]
    fn build_run_end_carries_inputs_verbatim() {
        let run_id = RunId::new();
        let started = Utc.with_ymd_and_hms(2026, 5, 18, 12, 0, 0).unwrap();
        let ended = Utc.with_ymd_and_hms(2026, 5, 18, 12, 0, 5).unwrap();
        let summary = RunSummary::default();
        let finding = build_run_end(run_id, started, ended, summary.clone());
        let Finding::RunEnd(re) = finding else {
            panic!("expected Finding::RunEnd");
        };
        assert_eq!(re.run_id, run_id);
        assert_eq!(re.ended_at_utc, ended);
        assert_eq!(
            re.wall_clock_ms, 5_000,
            "wall_clock_ms must equal ended - started in ms"
        );
        assert_eq!(re.summary, summary);
    }

    /// Clock-isolation: passing two different `started` values produces two
    /// different `started_at_utc` values in the output, AND the builder does
    /// not implicitly read a clock — proven by the byte-equality of the
    /// `request` Value across two calls with identical inputs but different
    /// `started`.
    #[test]
    fn build_run_start_clock_isolation() {
        let req = sample_request(false);
        let run_id = RunId::new();
        let started1 = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let started2 = Utc.with_ymd_and_hms(2026, 6, 1, 0, 0, 0).unwrap();

        let Finding::RunStart(rs1) = build_run_start(&req, run_id, started1, "rev") else {
            panic!("expected RunStart");
        };
        let Finding::RunStart(rs2) = build_run_start(&req, run_id, started2, "rev") else {
            panic!("expected RunStart");
        };

        // `started_at_utc` reflects the passed value, NOT an implicit
        // wall-clock read.
        assert_eq!(rs1.started_at_utc, started1);
        assert_eq!(rs2.started_at_utc, started2);
        assert_ne!(rs1.started_at_utc, rs2.started_at_utc);

        // The `request` Value is byte-identical across the two calls — it
        // depends ONLY on the ScanRequest, not on the timestamp. If the
        // builder were implicitly reading a clock, the two requests would
        // differ (or even be the same but each evaluated against a fresh
        // clock); byte-equality confirms purity.
        let s1 = serde_json::to_string(&rs1.request).expect("ser1");
        let s2 = serde_json::to_string(&rs2.request).expect("ser2");
        assert_eq!(
            s1, s2,
            "request Value must be byte-identical across calls with identical req (no implicit clock read)"
        );
    }

    /// Blocker 2 / D3-21: `request.dry_run` is ALWAYS present and echoes
    /// `req.dry_run` verbatim. Tested for both `true` and `false`.
    #[test]
    fn build_run_start_request_carries_dry_run() {
        let run_id = RunId::new();
        let started = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();

        // dry_run = true → field present as Bool(true).
        let req_true = sample_request(true);
        let Finding::RunStart(rs_true) = build_run_start(&req_true, run_id, started, "rev") else {
            panic!("expected RunStart");
        };
        assert_eq!(
            rs_true.request.get("dry_run"),
            Some(&serde_json::Value::Bool(true)),
            "dry_run=true must echo as Value::Bool(true) in RunStart.request"
        );

        // dry_run = false → field present as Bool(false) (NOT None / omitted).
        let req_false = sample_request(false);
        let Finding::RunStart(rs_false) = build_run_start(&req_false, run_id, started, "rev")
        else {
            panic!("expected RunStart");
        };
        assert_eq!(
            rs_false.request.get("dry_run"),
            Some(&serde_json::Value::Bool(false)),
            "dry_run=false must echo as Value::Bool(false) in RunStart.request (NEVER omitted — \
             mirrors the dsr/fdr_q null-but-present discipline locked in Phase 1)"
        );

        // Also confirm the serialised form contains the field literal even
        // when false — a future `skip_serializing_if` regression would drop it.
        let s_false = serde_json::to_string(&rs_false.request).expect("serialise");
        assert!(
            s_false.contains("\"dry_run\":false"),
            "request JSON must contain literal \"dry_run\":false; got {s_false}"
        );
    }
}
