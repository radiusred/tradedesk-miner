//! `RunId(Ulid)` newtype: time-prefixed, 26-char Crockford-base32 unique identifier
//! for a single miner invocation (D-10, D-24).
//!
//! `Copy` is REQUIRED on the wrapper — Plan 05's `emit_fixture()` moves the same
//! `RunId` value into both `RunStart` and `RunEnd`. `ulid::Ulid` is itself `Copy`
//! (a 128-bit value), so deriving `Copy` here is safe.
//!
//! The manual `JsonSchema` impl emits a strict string schema with a Crockford-base32
//! pattern. The Crockford alphabet excludes `I`, `L`, `O`, `U` to avoid visually
//! ambiguous characters — hence the pattern `^[0-9A-HJKMNP-TV-Z]{26}$`.

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Serialize};
use ulid::Ulid;

/// Unique, time-prefixed, lexicographically-sortable identifier for a single miner run.
///
/// Wire form (`#[serde(transparent)]`): a 26-character Crockford-base32 string such as
/// `"01HZF9G09T8K3M4P5Q6R7S8T9V"`. Two identical-input replays produce different `RunId`s
/// — they are distinct executions (D-10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RunId(pub Ulid);

impl RunId {
    /// Generate a new ULID-backed run id. Uses the `ulid` crate's default
    /// generator (seeded RNG; deterministic-ULID work belongs to Phase 5 per D-24).
    #[must_use]
    pub fn new() -> Self {
        Self(Ulid::new())
    }
}

impl Default for RunId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for RunId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Delegate to ulid::Ulid's Display impl which produces the canonical
        // 26-char Crockford-base32 form (matches the JSON wire shape).
        self.0.fmt(f)
    }
}

impl JsonSchema for RunId {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "RunId".into()
    }
    fn schema_id() -> std::borrow::Cow<'static, str> {
        "miner_core::findings::RunId".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        // 26-char Crockford-base32; alphabet excludes I, L, O, U.
        serde_json::json!({
            "type": "string",
            "pattern": "^[0-9A-HJKMNP-TV-Z]{26}$",
            "description": "26-character Crockford-base32 ULID, time-prefixed and lexicographically sortable"
        })
        .try_into()
        .expect("valid schema fragment")
    }
}
