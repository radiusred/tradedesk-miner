//! tradedesk-miner core library.
//!
//! Phase 1 (this plan) lays only the scaffolding: a `CODE_REVISION` constant populated at
//! compile time by `build.rs`. Plan 03 lands the `findings`, `config`, and `error` modules
//! and the locked v1 schema types.

/// Git SHA of the source revision that produced this build; `dirty-<sha>` when the tree had
/// uncommitted changes; `"unknown"` when git was unavailable (e.g., tarball builds).
///
/// Wired into every `Finding` envelope's `code_revision` field starting in Plan 03; mitigates
/// threat T-01-04 (a deployed binary cannot lie about which source revision built it).
pub const CODE_REVISION: &str = env!("MINER_CODE_REVISION");
