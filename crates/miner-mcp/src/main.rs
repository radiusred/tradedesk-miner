//! Placeholder binary; MCP server implementation deferred to v2.
//!
//! See `docs/future_mcp_http.md` for the architectural sketch and the
//! rationale for the deferral. Logging goes to stderr so the stdout
//! JSONL stream stays clean (D-15, D-19).

fn main() {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();
    tracing::info!(
        "miner-mcp placeholder; implementation deferred to v2 -- see docs/future_mcp_http.md"
    );
}
