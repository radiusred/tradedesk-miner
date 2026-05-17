//! Phase 6: implementation forthcoming.
//!
//! Phase 1 placeholder. The MCP server (built on `rmcp`) lands in Phase 6 once the
//! envelope contract, engine, and facade are stable. Logging goes to stderr so the
//! stdout JSONL stream stays clean (D-15, D-19).

fn main() {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();
    tracing::info!("miner-mcp placeholder; real implementation lands in Phase 6 (rmcp)");
}
