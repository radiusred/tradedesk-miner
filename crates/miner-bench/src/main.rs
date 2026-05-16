//! Phase 7: implementation forthcoming.
//!
//! Phase 1 placeholder. The real benchmark harness (criterion microbenches + scan-recipe
//! wall-clock runs measured with hyperfine) lands in Phase 7. Per D-23 + D-15: NO
//! `println!` — logs go to stderr through tracing-subscriber so the lint introduced in
//! Plan 04 catches accidental stdout writes from this crate.

fn main() {
    tracing_subscriber::fmt().with_writer(std::io::stderr).init();
    tracing::info!("miner-bench placeholder; real harness lands in Phase 7 (criterion)");
}
