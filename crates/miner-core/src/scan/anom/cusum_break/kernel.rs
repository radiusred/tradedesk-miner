//! `stats.regime.cusum_break@1` kernel — CUSUM structural-break / regime
//! detection (RAD-3841, Tier-2 build for RAD-3545).
//!
//! Detects breaks in the **mean** (drift) and/or **volatility** of the
//! single-leg log-return series via a "Bai-Perron-lite" binary segmentation
//! driven by two classical CUSUM statistics:
//!
//! - **Mean** — the standardized CUSUM (Brownian-bridge) statistic. On a
//!   segment of `n` demeaned returns with sample std `σ`, the partial-sum
//!   bridge `B_k = (Σ_{i<=k}(x_i-μ)) / (σ·√n)` peaks at the most likely mean
//!   changepoint; `max_k |B_k|` is the test statistic (sup of a Brownian
//!   bridge ⇒ Kolmogorov critical value ≈ 1.358 at 5 %).
//! - **Volatility** — the Inclán-Tiao CUSUM-of-squares statistic on the
//!   demeaned series: `D_k = (Σ_{i<=k} e_i)/(Σ e) − (k+1)/n` with `e_i =
//!   (x_i-μ)²`, test statistic `√(n/2)·max_k |D_k|` (same Kolmogorov critical
//!   value), the canonical single variance-change detector.
//!
//! ## Binary segmentation (multiple breaks)
//!
//! Each target is segmented independently: find the single most-likely break
//! on `[lo,hi)`; if its statistic exceeds `threshold` AND both resulting
//! children are at least `min_segment` long, record it and recurse on each
//! child. A segment shorter than `2·min_segment`, or with degenerate variance,
//! is never split. This recovers an unknown number of breaks without fitting a
//! global penalty (the "lite" of Bai-Perron).
//!
//! ## Final segment statistics
//!
//! Breaks are collected as bare indices first, then the FINAL partition's
//! adjacent segments define each break's pre/post window — so the reported
//! pre/post mean, std and (robust) median always describe the regimes either
//! side of the break in the fully-segmented series, not the transient
//! recursion sub-range. The per-segment median reuses the shared
//! [`crate::scan::primitives::robust::median`] primitive (the same robust
//! center `cross::cointegration_rolling` measures beta-drift against — RAD-3841
//! "do not duplicate" reuse contract).
//!
//! Determinism: pure arithmetic + `total_cmp` sorts, no RNG / clock /
//! allocation-order-dependent output (OUT-03).

use crate::scan::primitives::returns::log_returns;
use crate::scan::primitives::robust::median;

/// Default break threshold — the Kolmogorov sup-of-Brownian-bridge 5 % critical
/// value, shared by the mean-CUSUM and Inclán-Tiao vol statistics. LOWER ⇒ more
/// sensitive (more breaks).
pub const DEFAULT_THRESHOLD: f64 = 1.358;

/// Default minimum segment length (bars of the return series) for a valid
/// split. Mirrors the 30-bar floor used across the single-leg ANOM family.
pub const DEFAULT_MIN_SEGMENT: usize = 30;

/// Hard floor on `min_segment` — a sample std (ddof = 1) needs at least 2
/// points to be defined.
pub const MIN_SEGMENT_FLOOR: usize = 2;

/// A segment whose total dispersion is at or below this is treated as
/// degenerate (≈ constant); its CUSUM bridge is undefined so no break is
/// sought there.
const DISP_EPS: f64 = 1e-12;

/// Which structural break(s) the scan detects (the `target` param).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakTarget {
    /// Breaks in the mean (drift) of the return series only.
    Mean,
    /// Breaks in the volatility (variance) of the return series only.
    Vol,
    /// Both mean and volatility breaks (each target segmented independently).
    Both,
}

impl BreakTarget {
    /// Stable wire label for the `target` param.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            BreakTarget::Mean => "mean",
            BreakTarget::Vol => "vol",
            BreakTarget::Both => "both",
        }
    }

    /// Parse the `target` param label.
    #[must_use]
    pub fn from_label(s: &str) -> Option<Self> {
        match s {
            "mean" => Some(BreakTarget::Mean),
            "vol" => Some(BreakTarget::Vol),
            "both" => Some(BreakTarget::Both),
            _ => None,
        }
    }

    fn detects_mean(self) -> bool {
        matches!(self, BreakTarget::Mean | BreakTarget::Both)
    }

    fn detects_vol(self) -> bool {
        matches!(self, BreakTarget::Vol | BreakTarget::Both)
    }
}

/// The kind of a single detected break (the per-finding `target` label).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakKind {
    /// A break in the mean / drift.
    Mean,
    /// A break in the volatility / variance.
    Vol,
}

impl BreakKind {
    /// Stable wire label echoed into the finding's `effect.extra.target`.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            BreakKind::Mean => "mean",
            BreakKind::Vol => "vol",
        }
    }

    /// Stable sort key (`Mean` before `Vol`).
    fn order(self) -> u8 {
        match self {
            BreakKind::Mean => 0,
            BreakKind::Vol => 1,
        }
    }
}

/// One detected changepoint, indexed in **return-series** space (`index` is the
/// first index of the post-break segment). The scan body maps this to
/// close-series space for the emitted `break_index` / `break_ts_ms`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BreakPoint {
    /// Which statistic flagged this break.
    pub kind: BreakKind,
    /// First index of the post-break segment in the return series.
    pub index: usize,
    /// The normalized CUSUM statistic at detection (compared to `threshold`).
    pub statistic: f64,
    /// Pre-break segment mean (final-partition segment ending at `index`).
    pub pre_mean: f64,
    /// Pre-break segment sample std (ddof = 1).
    pub pre_std: f64,
    /// Pre-break segment robust median.
    pub pre_median: f64,
    /// Pre-break segment length (return bars).
    pub pre_n: usize,
    /// Post-break segment mean (final-partition segment starting at `index`).
    pub post_mean: f64,
    /// Post-break segment sample std (ddof = 1).
    pub post_std: f64,
    /// Post-break segment robust median.
    pub post_median: f64,
    /// Post-break segment length (return bars).
    pub post_n: usize,
    /// 1-based ordinal of the regime this break OPENS within its target's
    /// segmentation (regime 0 is the baseline before the first break).
    pub regime_index: usize,
}

/// Detect mean and/or volatility structural breaks on the log returns of a
/// single-leg close series.
///
/// `returns` is the (already differenced) log-return series. Returns the
/// detected breaks sorted by `(index, kind)` for deterministic emission; an
/// empty vec when the series is stationary, too short, or degenerate.
#[must_use]
pub fn detect_breaks(
    returns: &[f64],
    target: BreakTarget,
    threshold: f64,
    min_segment: usize,
) -> Vec<BreakPoint> {
    let mut breaks: Vec<BreakPoint> = Vec::new();

    if target.detects_mean() {
        let mut idx = Vec::new();
        segment(
            returns,
            0,
            returns.len(),
            BreakKind::Mean,
            threshold,
            min_segment,
            &mut idx,
        );
        build_regimes(returns, idx, BreakKind::Mean, &mut breaks);
    }
    if target.detects_vol() {
        let mut idx = Vec::new();
        segment(
            returns,
            0,
            returns.len(),
            BreakKind::Vol,
            threshold,
            min_segment,
            &mut idx,
        );
        build_regimes(returns, idx, BreakKind::Vol, &mut breaks);
    }

    breaks.sort_by(|a, b| {
        a.index
            .cmp(&b.index)
            .then_with(|| a.kind.order().cmp(&b.kind.order()))
    });
    breaks
}

/// Convenience wrapper: compute log returns of `closes` and detect breaks. The
/// returned `BreakPoint::index` is in RETURN-series space (offset by 1 from the
/// close series).
#[must_use]
pub fn cusum_break(
    closes: &[f64],
    target: BreakTarget,
    threshold: f64,
    min_segment: usize,
) -> Vec<BreakPoint> {
    let returns = log_returns(closes);
    detect_breaks(&returns, target, threshold, min_segment)
}

/// Binary segmentation of `x[lo..hi]` for `kind`, appending `(split_index,
/// statistic)` pairs (split index in `x`-space) for every accepted break.
fn segment(
    x: &[f64],
    lo: usize,
    hi: usize,
    kind: BreakKind,
    threshold: f64,
    min_segment: usize,
    out: &mut Vec<(usize, f64)>,
) {
    let n = hi - lo;
    if n < 2 * min_segment {
        return;
    }
    let Some((rel_tau, stat)) = cusum_candidate(&x[lo..hi], kind) else {
        return;
    };
    let split = lo + rel_tau;
    if stat > threshold && (split - lo) >= min_segment && (hi - split) >= min_segment {
        out.push((split, stat));
        segment(x, lo, split, kind, threshold, min_segment, out);
        segment(x, split, hi, kind, threshold, min_segment, out);
    }
}

/// The single most-likely changepoint of `seg` for `kind`. Returns
/// `(rel_tau, statistic)` where `rel_tau` (in `1..seg.len()`) is the first
/// index of the post segment, or `None` when the segment is too short or
/// degenerate (zero dispersion).
fn cusum_candidate(seg: &[f64], kind: BreakKind) -> Option<(usize, f64)> {
    match kind {
        BreakKind::Mean => cusum_mean(seg),
        BreakKind::Vol => cusum_vol(seg),
    }
}

/// Standardized (Brownian-bridge) CUSUM for a mean shift.
#[allow(
    clippy::cast_precision_loss,
    reason = "segment length << 2^52 for any realistic bar count"
)]
fn cusum_mean(seg: &[f64]) -> Option<(usize, f64)> {
    let n = seg.len();
    if n < 2 {
        return None;
    }
    let nf = n as f64;
    let mean = seg.iter().sum::<f64>() / nf;
    let var = seg.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (nf - 1.0);
    let std = var.sqrt();
    if !std.is_finite() || std <= DISP_EPS {
        return None;
    }
    let denom = std * nf.sqrt();
    let mut cum = 0.0_f64;
    let mut best = 0.0_f64;
    let mut arg = 0usize;
    for (k, v) in seg.iter().enumerate() {
        cum += v - mean;
        let bridge = (cum / denom).abs();
        if bridge > best {
            best = bridge;
            arg = k + 1; // post segment starts AFTER index k
        }
    }
    if arg == 0 || arg >= n {
        return None;
    }
    Some((arg, best))
}

/// Inclán-Tiao CUSUM-of-squares for a variance shift.
#[allow(
    clippy::cast_precision_loss,
    reason = "segment length << 2^52 for any realistic bar count"
)]
fn cusum_vol(seg: &[f64]) -> Option<(usize, f64)> {
    let n = seg.len();
    if n < 2 {
        return None;
    }
    let nf = n as f64;
    let mean = seg.iter().sum::<f64>() / nf;
    let sq: Vec<f64> = seg.iter().map(|v| (v - mean).powi(2)).collect();
    let total: f64 = sq.iter().sum();
    if !total.is_finite() || total <= DISP_EPS {
        return None;
    }
    let mut cum = 0.0_f64;
    let mut best = 0.0_f64;
    let mut arg = 0usize;
    for (k, e) in sq.iter().enumerate() {
        cum += *e;
        let d_k = (cum / total) - ((k + 1) as f64) / nf;
        let stat = d_k.abs();
        if stat > best {
            best = stat;
            arg = k + 1;
        }
    }
    if arg == 0 || arg >= n {
        return None;
    }
    // Inclán-Tiao scaling: √(n/2)·max|D_k| ~ sup |Brownian bridge|.
    Some((arg, (nf / 2.0).sqrt() * best))
}

/// Turn a set of break indices (in `x`-space) into [`BreakPoint`]s whose
/// pre/post stats describe the FINAL partition's adjacent segments.
fn build_regimes(
    x: &[f64],
    mut idx: Vec<(usize, f64)>,
    kind: BreakKind,
    out: &mut Vec<BreakPoint>,
) {
    if idx.is_empty() {
        return;
    }
    idx.sort_by(|a, b| a.0.cmp(&b.0));
    let n = x.len();
    // bounds = [0, i1, i2, ..., ik, n] — consecutive pairs are the segments.
    let mut bounds: Vec<usize> = Vec::with_capacity(idx.len() + 2);
    bounds.push(0);
    bounds.extend(idx.iter().map(|(i, _)| *i));
    bounds.push(n);

    for (j, (split, stat)) in idx.iter().enumerate() {
        let (pre_mean, pre_std, pre_median, pre_n) = segment_stats(&x[bounds[j]..*split]);
        let (post_mean, post_std, post_median, post_n) = segment_stats(&x[*split..bounds[j + 2]]);
        out.push(BreakPoint {
            kind,
            index: *split,
            statistic: *stat,
            pre_mean,
            pre_std,
            pre_median,
            pre_n,
            post_mean,
            post_std,
            post_median,
            post_n,
            regime_index: j + 1,
        });
    }
}

/// `(mean, sample_std_ddof1, median, len)` of a segment. Empty ⇒ all-NaN/0;
/// single-element ⇒ std 0. The median reuses the shared robust primitive.
#[allow(
    clippy::cast_precision_loss,
    reason = "segment length << 2^52 for any realistic bar count"
)]
fn segment_stats(seg: &[f64]) -> (f64, f64, f64, usize) {
    let n = seg.len();
    if n == 0 {
        return (f64::NAN, f64::NAN, f64::NAN, 0);
    }
    let nf = n as f64;
    let mean = seg.iter().sum::<f64>() / nf;
    let std = if n >= 2 {
        (seg.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (nf - 1.0)).sqrt()
    } else {
        0.0
    };
    (mean, std, median(seg), n)
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    /// Deterministic LCG noise in `[-0.5, 0.5)·scale`, mean ~0.
    fn noise(n: usize, seed: u32, scale: f64) -> Vec<f64> {
        let mut s = seed;
        (0..n)
            .map(|_| {
                s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (f64::from(s) / f64::from(u32::MAX) - 0.5) * scale
            })
            .collect()
    }

    #[test]
    fn target_label_round_trip() {
        for t in [BreakTarget::Mean, BreakTarget::Vol, BreakTarget::Both] {
            assert_eq!(BreakTarget::from_label(t.label()), Some(t));
        }
        assert_eq!(BreakTarget::from_label("garbage"), None);
    }

    /// A stationary (single-regime) noise series flags no breaks. Uses a
    /// conservative threshold (≈0.001 % level) so a single fixed-seed noise
    /// draw cannot trip a top-level false positive — segmentation only recurses
    /// AFTER a break, so a stationary series is tested exactly once per target.
    #[test]
    fn stationary_series_flags_none() {
        let x = noise(400, 0x1234_5678, 0.01);
        let breaks = detect_breaks(&x, BreakTarget::Both, 2.5, 30);
        assert!(
            breaks.is_empty(),
            "stationary series must flag no breaks; got {breaks:?}"
        );
    }

    /// An injected MEAN shift at the midpoint is flagged near the true
    /// changepoint with a Mean-kind break.
    #[test]
    fn mean_shift_flagged_near_changepoint() {
        let cp = 200usize;
        let mut x = noise(400, 0xABCD_1234, 0.01);
        for v in x.iter_mut().skip(cp) {
            *v += 0.05; // large drift step relative to 0.01 noise
        }
        let breaks = detect_breaks(&x, BreakTarget::Mean, DEFAULT_THRESHOLD, 30);
        assert!(!breaks.is_empty(), "must detect the injected mean shift");
        let nearest = breaks
            .iter()
            .map(|b| b.index.abs_diff(cp))
            .min()
            .expect("non-empty");
        assert!(nearest <= 15, "nearest break {nearest} bars from cp={cp}");
        assert!(breaks.iter().all(|b| b.kind == BreakKind::Mean));
    }

    /// An injected VOLATILITY shift at the midpoint is flagged near the true
    /// changepoint with a Vol-kind break.
    #[test]
    fn vol_shift_flagged_near_changepoint() {
        let cp = 200usize;
        let mut x = noise(400, 0x0F0F_0F0F, 0.01);
        for v in x.iter_mut().skip(cp) {
            *v *= 8.0; // 8x volatility regime
        }
        let breaks = detect_breaks(&x, BreakTarget::Vol, DEFAULT_THRESHOLD, 30);
        assert!(!breaks.is_empty(), "must detect the injected vol shift");
        let nearest = breaks
            .iter()
            .map(|b| b.index.abs_diff(cp))
            .min()
            .expect("non-empty");
        assert!(nearest <= 20, "nearest break {nearest} bars from cp={cp}");
        assert!(breaks.iter().all(|b| b.kind == BreakKind::Vol));
    }

    /// `min_segment` is respected: every reported segment is at least
    /// `min_segment` long.
    #[test]
    fn min_segment_respected() {
        let cp = 200usize;
        let mut x = noise(400, 0x5151_5151, 0.01);
        for v in x.iter_mut().skip(cp) {
            *v += 0.05;
        }
        let min_seg = 30usize;
        let breaks = detect_breaks(&x, BreakTarget::Mean, DEFAULT_THRESHOLD, min_seg);
        for b in &breaks {
            assert!(b.pre_n >= min_seg, "pre_n {} < {min_seg}", b.pre_n);
            assert!(b.post_n >= min_seg, "post_n {} < {min_seg}", b.post_n);
        }
    }

    /// Determinism: identical inputs ⇒ identical break vectors.
    #[test]
    fn deterministic_across_runs() {
        let mut x = noise(400, 0x9999_1111, 0.01);
        for v in x.iter_mut().skip(200) {
            *v += 0.04;
        }
        let a = detect_breaks(&x, BreakTarget::Both, DEFAULT_THRESHOLD, 30);
        let b = detect_breaks(&x, BreakTarget::Both, DEFAULT_THRESHOLD, 30);
        assert_eq!(a, b);
    }

    /// A series shorter than `2·min_segment` yields no breaks (no panic).
    #[test]
    fn too_short_yields_empty() {
        let x = noise(40, 0x2222_3333, 0.01);
        assert!(detect_breaks(&x, BreakTarget::Both, DEFAULT_THRESHOLD, 30).is_empty());
    }

    /// A perfectly constant series is degenerate ⇒ no breaks (no div-by-zero).
    #[test]
    fn constant_series_yields_empty() {
        let x = vec![0.0_f64; 400];
        assert!(detect_breaks(&x, BreakTarget::Both, DEFAULT_THRESHOLD, 30).is_empty());
    }
}
