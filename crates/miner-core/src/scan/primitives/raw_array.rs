//! `f64_slice_to_raw_array` — shared `&[f64] -> RawArray` packer.
//!
//! Plan-research §3 recommends lifting the helper currently duplicated inline
//! in [`crate::scan::ljung_box`]'s `f64_slice_to_raw_array` (Phase 3 lines
//! ~298-312) to this primitives module so the 22 Phase 4 scans share one
//! copy. The body is verbatim from [PATTERNS.md] Pattern A.

use crate::findings::{Base64Bytes, Dtype, RawArray};

/// Pack a `&[f64]` into a [`RawArray`] with `Dtype::F64`, shape `vec![n]`, and
/// little-endian f64 bytes per D-01.
///
/// Centralises the byte-layout rule so individual call sites stay focused on
/// kernel math. Byte-identical to the Phase 3 `LjungBox` helper (the Phase 3
/// statsmodels golden remains byte-identical after `LjungBoxScan` is refactored
/// to call this primitive).
#[inline]
#[must_use]
#[allow(
    clippy::cast_precision_loss,
    reason = "slice length feeds RawArray.shape: Vec<u64>; bar counts fit in u64 trivially"
)]
pub fn f64_slice_to_raw_array(s: &[f64]) -> RawArray {
    let mut bytes = Vec::with_capacity(s.len() * 8);
    for v in s {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    RawArray {
        data: Base64Bytes(bytes),
        shape: vec![s.len() as u64],
        dtype: Dtype::F64,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f64_slice_to_raw_array_packs_le_f64() {
        let raw = f64_slice_to_raw_array(&[1.0_f64, 2.0, 3.0]);
        assert_eq!(raw.shape, vec![3]);
        assert!(matches!(raw.dtype, Dtype::F64));
        assert_eq!(raw.data.0.len(), 24, "3 * 8 bytes");
        // Decode the first 8 bytes; must equal 1.0.
        let mut buf = [0u8; 8];
        buf.copy_from_slice(&raw.data.0[0..8]);
        assert_eq!(f64::from_le_bytes(buf), 1.0_f64);
    }

    #[test]
    fn f64_slice_to_raw_array_empty() {
        let raw = f64_slice_to_raw_array(&[]);
        assert_eq!(raw.shape, vec![0]);
        assert!(raw.data.0.is_empty());
    }
}
