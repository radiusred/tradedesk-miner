//! Compile-time `MINER_CODE_REVISION` injection.
//!
//! Mitigates threat T-01-04 (code revision tampering / repudiation): every finding envelope
//! carries `code_revision`; populating it at compile time means a deployed binary cannot lie
//! about which source revision built it. The `dirty-<sha>` suffix on uncommitted builds
//! prevents a locally-modified tree from masquerading as a clean release.
//!
//! Uses only `std::process::Command` (no tokio, no async — confirmed sync per the FOUND-04
//! invariant). If `git` is not available on the build host, falls back to "unknown" so
//! offline / source-tarball builds still compile.
//!
//! See 01-RESEARCH §"Code Examples" Example 4 for the canonical shape this implements.

use std::process::Command;

fn main() {
    // Resolve the current HEAD commit SHA. `unwrap_or_else` handles the case where git is
    // absent OR the source tree isn't a git checkout (e.g., a published crate tarball).
    let sha = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8(o.stdout).ok()
            } else {
                None
            }
        })
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    // `git diff --quiet` exits non-zero when there are unstaged worktree changes against HEAD.
    // We treat "git not available" as not-dirty (we already fell back to "unknown" above).
    let dirty = Command::new("git")
        .args(["diff", "--quiet"])
        .status()
        .map(|s| !s.success())
        .unwrap_or(false);

    let rev = if dirty {
        format!("dirty-{sha}")
    } else {
        sha
    };

    // Emit the env var that `env!("MINER_CODE_REVISION")` reads inside lib.rs.
    println!("cargo:rustc-env=MINER_CODE_REVISION={rev}");
    // Re-run only when HEAD moves (avoids invalidating the build cache on every cargo invocation).
    println!("cargo:rerun-if-changed=.git/HEAD");
}
