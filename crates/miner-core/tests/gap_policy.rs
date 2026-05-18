//! Phase 3 integration test — gap-policy dispatch behaviour.
//!
//! Pattern analog: `tests/cache_smoke.rs` (whole file — module-doc lists every
//! test with VALIDATION row name verbatim; one named test per scenario against
//! a synthetic substrate).
//!
//! ## Five tests, matching the VALIDATION.md OUT-04 / SC-3a..SC-3e row IDs verbatim
//!
//! - `strict_with_gaps_emits_single_gap_aborted` — strict + non-empty manifest → one GapAborted.
//! - `continuous_only_partitions_and_inlines_manifest` — continuous_only + gaps → SubRanges + inlined manifest.
//! - `strict_zero_gaps_emits_result_with_none_manifest` — strict fast path.
//! - `continuous_only_zero_gaps_emits_empty_manifest` — continuous_only fast path.
//! - `never_silently_emits_on_hole_proptest` — proptest: under any random gap manifest,
//!   `dispatch` NEVER returns SubRanges covering a hole.
//!
//! Wave 0 scaffold: every test `#[ignore]`. Plan 03-03 fills the bodies.

#![allow(dead_code, unused_imports, unexpected_cfgs)]

#[test]
#[ignore = "Wave 5 implements (Plan 03-03) — Wave 0 scaffold per VALIDATION harness"]
fn strict_with_gaps_emits_single_gap_aborted() {
    unimplemented!(
        "Plan 03-03 implements per OUT-04/SC-3a — build manifest with 1 gap, \
         call gap_policy::dispatch(.., Strict), assert Aborted(manifest)"
    )
}

#[test]
#[ignore = "Wave 5 implements (Plan 03-03) — Wave 0 scaffold per VALIDATION harness"]
fn continuous_only_partitions_and_inlines_manifest() {
    unimplemented!(
        "Plan 03-03 implements per OUT-04/SC-3b — build manifest with N gaps over a known range, \
         call gap_policy::dispatch(.., ContinuousOnly), assert SubRanges has N+1 elements \
         and the union equals (requested - gaps)"
    )
}

#[test]
#[ignore = "Wave 5 implements (Plan 03-03) — Wave 0 scaffold per VALIDATION harness"]
fn strict_zero_gaps_emits_result_with_none_manifest() {
    unimplemented!(
        "Plan 03-03 implements per OUT-04/SC-3c — build empty manifest, \
         call dispatch(.., Strict), assert SubRanges(vec![requested as TimeRange])"
    )
}

#[test]
#[ignore = "Wave 5 implements (Plan 03-03) — Wave 0 scaffold per VALIDATION harness"]
fn continuous_only_zero_gaps_emits_empty_manifest() {
    unimplemented!(
        "Plan 03-03 implements per OUT-04/SC-3d — build empty manifest, \
         call dispatch(.., ContinuousOnly), assert SubRanges(vec![requested as TimeRange])"
    )
}

// Proptest — wrapped in proptest! and gated by cfg(disabled_in_scaffold) so
// the cargo build paths do not generate inner test fns from the macro until
// Plan 03-03 wires the real body. Plan 03-03 removes the cfg-gate.
#[cfg(disabled_in_scaffold)]
mod proptest_block {
    use proptest::prelude::*;

    proptest! {
        /// SC-3e — under ANY random gap manifest, `dispatch` NEVER returns a
        /// `SubRanges` element whose `[start, end)` overlaps a gap. Look-ahead
        /// safety for the continuous_only partitioner.
        #[test]
        fn never_silently_emits_on_hole_proptest(_seed in 0u64..1_000) {
            unimplemented!(
                "Plan 03-03 implements per OUT-04/SC-3e — proptest: generate random gap manifests + \
                 ranges, call dispatch(.., ContinuousOnly), assert every SubRanges element is gap-free"
            )
        }
    }
}
