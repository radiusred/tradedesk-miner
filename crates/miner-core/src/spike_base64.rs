//! Spike module — Risk 2 / Assumption A1 verification (Plan 01-02).
//!
//! Purpose: compile-test the schemars 1.x `Base64Bytes` newtype pattern that 01-RESEARCH
//! §"Architecture Patterns" Pattern 2 recommends, *before* Plan 03 commits the production
//! envelope types to it. The critical line under test is the
//! `serde_json::json!{...}.try_into().expect("...")` conversion inside `json_schema()` —
//! this is the only `[ASSUMED]` step in the schemars-1.x Pattern 2 recipe.
//!
//! **Deletion target:** Plan 03 deletes this module and the `pub mod spike_base64;`
//! re-export in `lib.rs`. The production `Base64Bytes` lives in
//! `crates/miner-core/src/findings/base64_bytes.rs`.
//!
//! Threat coverage: T-01-02 — establishes the schema-derivation pattern under test from
//! day one so a contributor cannot drift the Rust types away from the published schema.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Newtype wrapping raw bytes; serialises as base64 string, schema advertises
/// `contentEncoding: "base64"` + `contentMediaType: "application/octet-stream"`.
///
/// Spike-grade. The production type (Plan 03) will live at
/// `crates/miner-core/src/findings/base64_bytes.rs`.
#[derive(Debug, Clone, PartialEq)]
pub struct SpikeBase64Bytes(pub Vec<u8>);

impl Serialize for SpikeBase64Bytes {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&STANDARD.encode(&self.0))
    }
}

impl<'de> Deserialize<'de> for SpikeBase64Bytes {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        STANDARD
            .decode(&s)
            .map(SpikeBase64Bytes)
            .map_err(D::Error::custom)
    }
}

impl JsonSchema for SpikeBase64Bytes {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "SpikeBase64Bytes".into()
    }
    fn schema_id() -> std::borrow::Cow<'static, str> {
        "miner_core::spike_base64::SpikeBase64Bytes".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        // schemars 1.x: `Schema` wraps `serde_json::Value`. Build the fragment via
        // `serde_json::json!` and use `try_into()` to convert. This is the exact line
        // 01-RESEARCH §Architecture Patterns Pattern 2 marks as [ASSUMED] — the spike
        // exists to compile-test it.
        serde_json::json!({
            "type": "string",
            "contentEncoding": "base64",
            "contentMediaType": "application/octet-stream",
            "description": "Little-endian f64 bytes, base64-encoded"
        })
        .try_into()
        .expect("valid schema fragment")
    }
}

/// Dtype enum mirror — single F64 variant for v1 (D-01).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SpikeDtype {
    F64,
}

/// Composition target — exercises the embedding chain that production `RawArray` will use.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SpikeRawArray {
    pub data: SpikeBase64Bytes,
    pub shape: Vec<u64>,
    pub dtype: SpikeDtype,
}

/// Top-level type for the `schema_for!(SpikeFinding)` call — proves the manual
/// `JsonSchema` impl on `SpikeBase64Bytes` is reachable via the derive macro chain.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SpikeFinding {
    pub array: SpikeRawArray,
}
