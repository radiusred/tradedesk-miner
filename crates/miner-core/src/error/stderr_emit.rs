//! Pre-flight error emitter (writes structured-error JSON to stderr — D-06).
//!
//! Plan 04 fills in the actual `Write`-backed emitter that the wrappers will call
//! when pre-flight rejection happens. This file exists in Plan 03 so the
//! `pub mod stderr_emit;` declaration in `error/mod.rs` resolves and Plan 04's
//! placement is not a file-create conflict.
