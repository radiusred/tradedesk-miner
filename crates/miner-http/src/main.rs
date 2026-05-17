//! Phase 6: implementation forthcoming.
//!
//! Phase 1 placeholder. The HTTP server (built on axum + tokio + tower) lands in Phase 6.
//! Logging goes to stderr so the stdout JSONL stream stays clean (D-15, D-19).

fn main() {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();
    tracing::info!("miner-http placeholder; real implementation lands in Phase 6 (axum)");
}
