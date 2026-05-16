//! Phase 1 placeholder for the `miner` CLI binary.
//!
//! Plan 05 wires the clap parser, the figment config builder, and the `emit-fixture` /
//! `scan` / `sweep` subcommands. This stub establishes the stdout/stderr discipline
//! (D-15, D-19) from day one: logs go to stderr via `tracing-subscriber`; stdout is
//! reserved for the findings stream that future plans will emit through `FindingSink`.

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_writer(std::io::stderr).init();
    tracing::info!("miner-cli placeholder; Plan 05 wires clap + emit-fixture subcommand");
    Ok(())
}
