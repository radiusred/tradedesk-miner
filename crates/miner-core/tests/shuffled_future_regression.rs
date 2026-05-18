//! Phase 3 integration test — look-ahead-safety proptest (D3-09).
//!
//! Pattern analog: `tests/cache_smoke.rs::arrow_bytes_deterministic_under_shuffled_construction`
//! (proptest harness layout — `use proptest::prelude::*` + `proptest! { #[test]
//! fn name(args in strategy) { ... } }`).
//!
//! ## D3-09 invariant
//!
//! Statistics up to time `T` MUST be byte-identical when bars at index `>T`
//! are shuffled. The shuffle is a deterministic permutation seeded by the
//! proptest `seed` so failures are reproducible.
//!
//! ## Wave 0 scaffold
//!
//! `proptest!` macro does not honour `#[ignore]` at the inner test-fn level.
//! The plan instructs gating the WHOLE MODULE behind `#[cfg(disabled_in_scaffold)]`
//! so cargo's normal build paths skip the file entirely. Plan 03-06 flips the
//! cfg-gate to `cfg(test)` (or removes it) when it wires the real body.

#![allow(dead_code, unused_imports, unexpected_cfgs)]

// Gate the entire test module so cargo test --no-run + cargo test do not
// attempt to compile proptest's #[test]-generated entry points. Plan 03-06
// removes this cfg-gate when it implements the proptest body. The
// `unexpected_cfgs` allow above silences Rust 2024's unknown-cfg warning
// while the gate is in place.
#[cfg(disabled_in_scaffold)]
mod inner {
    use proptest::prelude::*;

    proptest! {
        /// D3-09 — Ljung-Box up to time T MUST be byte-identical when bars
        /// at index >T are shuffled. The shuffle is deterministic per `seed`.
        ///
        /// Plan 03-06 fills:
        /// 1. Build a deterministic BarFrame from `seed` (N bars, e.g. N = 256).
        /// 2. Compute Ljung-Box up to cutpoint T = N/2.
        /// 3. Shuffle bars at indices [T..N) using a seeded permutation.
        /// 4. Recompute Ljung-Box up to T.
        /// 5. Assert the pre-T Q-stat is byte-identical.
        #[test]
        fn look_ahead_safe_under_post_t_shuffle(seed in 0u64..1_000) {
            unimplemented!(
                "Plan 03-06 implements look_ahead_safe_under_post_t_shuffle per D3-09"
            )
        }
    }
}
