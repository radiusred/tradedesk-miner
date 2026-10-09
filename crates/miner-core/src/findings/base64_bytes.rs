//! `Base64Bytes(Vec<u8>)` newtype: raw bytes that serialise as a base64 string and
//! advertise `contentEncoding: "base64"` + `contentMediaType: "application/octet-stream"`
//! in the JSON Schema.
//!
//! Ported verbatim from the Plan 01-02 spike (`spike_base64.rs`) which verified the
//! schemars 1.x `serde_json::json!{}.try_into()` recipe compiles and produces the
//! expected schema fragment. The spike module has been deleted by this plan.
//!
//! Why a manual `JsonSchema` impl: JSON Schema 2020-12's `contentEncoding`/`contentMediaType`
//! keywords are not expressible via `#[derive(JsonSchema)]` — the derive macro only knows
//! about the standard schema vocabulary. The manual impl builds the schema fragment as a
//! `serde_json::Value` and uses `try_into()` to convert into schemars' `Schema` wrapper.
//!
//! Threat coverage: T-01-02 (schema injection / drift). Plan 06's CI gate will diff the
//! schemars-derived schema against the checked-in `schemas/findings-v1.schema.json`;
//! any change to this `JsonSchema` impl must also regenerate the artifact.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Newtype wrapping raw bytes. Serialises as a base64 string; schema advertises
/// `contentEncoding: "base64"` + `contentMediaType: "application/octet-stream"`.
///
/// **Does NOT derive `Copy`.** The inner `Vec<u8>` is heap-owned; cloning is explicit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Base64Bytes(pub Vec<u8>);

impl Serialize for Base64Bytes {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&STANDARD.encode(&self.0))
    }
}

impl<'de> Deserialize<'de> for Base64Bytes {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        STANDARD
            .decode(&s)
            .map(Base64Bytes)
            .map_err(D::Error::custom)
    }
}

impl JsonSchema for Base64Bytes {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Base64Bytes".into()
    }
    fn schema_id() -> std::borrow::Cow<'static, str> {
        "miner_core::findings::Base64Bytes".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        // schemars 1.x: `Schema` wraps `serde_json::Value`. Build the fragment via
        // `serde_json::json!` and convert with `try_into()`. Verified by the Plan 01-02
        // spike — see `.planning/phases/01-foundations-contracts/01-02-SUMMARY.md`.
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

/// Dtype enum carried alongside a `RawArray`. Only `F64` is used in v1 (D-01: all raw
/// payloads are little-endian f64 bytes). Adding additional dtypes is additive — a new
/// variant does not break consumers that only know about `f64`, because they will simply
/// skip arrays they cannot decode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Dtype {
    F64,
}
