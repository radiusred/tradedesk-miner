//! xtask: dev-only command runner.
//!
//! Plan 06 adds the `gen-schema` subcommand (regenerates `schemas/findings-v1.schema.json`
//! from the schemars derives in `miner-core`). This Phase 1 stub keeps the workspace
//! buildable and the `cargo xtask` alias resolvable until then.
//!
//! `disallowed_macros` is allowed here at the binary scope: xtask is dev-only (never
//! shipped, never run from the CLI/MCP/HTTP wrappers) so its stderr/stdout discipline does
//! not need to match the production binaries' rules. The clippy lint that bans `println!`
//! / `eprintln!` everywhere else (Plan 04) is intentionally relaxed inside xtask so
//! dev-loop diagnostic output stays ergonomic.
#![allow(clippy::disallowed_macros)]

fn main() {
    eprintln!("xtask: no subcommands wired yet (Plan 06 adds gen-schema)");
}
