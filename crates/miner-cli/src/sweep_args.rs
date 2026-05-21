// Plan 05-05: SweepArgs is the Phase 5 CLI binding for `miner sweep`. Mirrors
// `scan_args.rs`'s clap-`Args` derive + `to_*` conversion shape so the
// `--bootstrap / --bootstrap-n / --null / --null-n / --seed` flags share a
// common parser surface with `miner scan`.
#![allow(clippy::doc_lazy_continuation)]

//! `SweepArgs` — clap-derive struct + `to_manifest()` conversion.
//!
//! Pattern analog: `scan_args.rs:56-110` (`ScanArgs` clap-Args derive +
//! `to_scan_request`). `SweepArgs` follows the same shape but wraps a
//! `Command::Sweep(SweepArgs)` variant.
//!
//! ## D5-04 / D5-05 CLI surface
//!
//! ```text
//! miner sweep <manifest.toml> \
//!     [--dry-run] [--seed N] \
//!     [--bootstrap stationary|block] [--bootstrap-n N] \
//!     [--null phase_scramble|circular_shift] [--null-n N]
//! ```
//!
//! Each flag mirrors a `[hygiene]` or `[sweep]` block field in the parsed
//! `SweepManifest` and overrides it when supplied. `--dry-run` short-circuits
//! to the sweep executor's `DryRunFinding` emission per Plan 05-04.
//!
//! ## Cfg-gated test-only hook (mirrors `scan_args.rs`)
//!
//! Under `#[cfg(any(test, feature = "test-internal"))]` `SweepArgs` gains a
//! `--sleep-after-first-finding-ms <ms>` flag (`hide = true` on `--help`) used
//! by the `sigint_mid_sweep.rs` integration test to make the SIGINT race
//! deterministic. Release builds do NOT compile the flag in.

use std::path::PathBuf;

use clap::Args;
use miner_core::error::{MinerError, PreflightCode, WireError};
use miner_core::sweep::manifest::{SweepManifest, read_manifest};

/// `miner sweep` subcommand arguments.
///
/// Pattern: `scan_args.rs:56-110` (clap `Args` derive + `#[arg]` flags). The
/// positional `manifest` is the path to a TOML manifest file; the six optional
/// flags override the manifest's `[sweep]` / `[hygiene]` block fields when set.
#[derive(Debug, Args)]
pub struct SweepArgs {
    /// Positional `<manifest.toml>` — path to a TOML sweep manifest. Parsed
    /// into a [`SweepManifest`] by [`SweepArgs::to_manifest`].
    pub manifest: PathBuf,

    /// Short-circuit to `DryRunFinding` emission (per Plan 05-04). The sweep
    /// runner emits one `Finding::DryRun` with `planned_job_count` and skips
    /// all scan bodies. Exit 0.
    #[arg(long)]
    pub dry_run: bool,

    /// Override `[sweep].seed` in the manifest. The master seed propagates
    /// through `ScanRequest.master_seed` into the hygiene pipeline's
    /// `Xoshiro256PlusPlus` PRNG so the run is bit-for-bit reproducible
    /// (HYG-05).
    #[arg(long)]
    pub seed: Option<u64>,

    /// Override `[hygiene].bootstrap` in the manifest. Accepted values:
    /// `"stationary"` (Politis-Romano 1994) / `"block"` (fixed-block). Unknown
    /// strings are rejected with `PreflightCode::InvalidParameter` at
    /// preflight (engine `manifest::validate`).
    #[arg(long)]
    pub bootstrap: Option<String>,

    /// Override `[hygiene].bootstrap_n` in the manifest. `0` (default — flag
    /// not supplied) leaves the manifest value alone; positive values
    /// override. Engine caps at `100_000` per `T-05-03-V5` mitigation.
    #[arg(long, default_value_t = 0)]
    pub bootstrap_n: u32,

    /// Override `[hygiene].null` in the manifest. Accepted values:
    /// `"phase_scramble"` (FFT phase randomisation; Theiler et al. 1992) /
    /// `"circular_shift"` (canonical fallback). Unknown strings are rejected
    /// at preflight with `PreflightCode::InvalidParameter`.
    #[arg(long)]
    pub null: Option<String>,

    /// Override `[hygiene].null_n` in the manifest. `0` leaves the manifest
    /// value alone; positive overrides. Engine caps at `100_000`.
    #[arg(long, default_value_t = 0)]
    pub null_n: u32,

    /// **Test-only Pitfall 8 hook** — mirror of `ScanArgs.sleep_after_first_finding_ms`.
    /// Forwarded into the sweep executor's per-job `ScanCtx` so the SIGINT race
    /// in `sigint_mid_sweep.rs` is deterministic. Hidden from `--help`; gated to
    /// `cfg(test)` / `feature = "test-internal"`. NEVER reachable in release
    /// production builds.
    #[cfg(any(test, feature = "test-internal"))]
    #[arg(long = "sleep-after-first-finding-ms", hide = true)]
    pub sleep_after_first_finding_ms: Option<u64>,
}

impl SweepArgs {
    /// Load the manifest TOML file and apply CLI overrides.
    ///
    /// Read order: file → `toml::from_str` → CLI-flag overrides for `[sweep].seed`,
    /// `[hygiene].bootstrap`, `[hygiene].bootstrap_n`, `[hygiene].null`,
    /// `[hygiene].null_n`. CLI flags only override when supplied (None / 0
    /// means "leave the manifest value alone").
    ///
    /// # Errors
    /// - [`WireError`] with code [`PreflightCode::InvalidParameter`] on TOML
    ///   parse failure.
    /// - [`WireError`] with code [`PreflightCode::InvalidParameter`] on file
    ///   read failure (path does not exist / permission denied — surfaced via
    ///   the wire form so the consumer sees a structured error).
    pub fn to_manifest(&self) -> Result<SweepManifest, WireError> {
        let mut manifest = read_manifest(&self.manifest).map_err(|err| match err {
            MinerError::Preflight(w) => w,
            MinerError::Io(io_err) => WireError::preflight(
                PreflightCode::InvalidParameter,
                format!(
                    "could not read manifest {}: {io_err}",
                    self.manifest.display()
                ),
            )
            .with_context(
                "manifest_path",
                serde_json::Value::String(self.manifest.display().to_string()),
            ),
            other => WireError::preflight(
                PreflightCode::InvalidParameter,
                format!("manifest load failed: {other}"),
            ),
        })?;

        // Apply CLI overrides.
        if let Some(seed) = self.seed {
            manifest.sweep.seed = Some(seed);
        }
        if let Some(ref bootstrap) = self.bootstrap {
            manifest.hygiene.bootstrap = Some(bootstrap.clone());
        }
        if self.bootstrap_n > 0 {
            manifest.hygiene.bootstrap_n = self.bootstrap_n;
        }
        if let Some(ref null) = self.null {
            manifest.hygiene.null = Some(null.clone());
        }
        if self.null_n > 0 {
            manifest.hygiene.null_n = self.null_n;
        }

        Ok(manifest)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{Cli, Command};
    use clap::Parser;
    use tempfile::TempDir;

    fn unwrap_sweep_args(cli: &Cli) -> &SweepArgs {
        match &cli.command {
            Command::Sweep(args) => args,
            other => panic!("expected Command::Sweep; got {other:?}"),
        }
    }

    /// Test 1: clap parses `miner sweep <manifest>` with positional arg only.
    #[test]
    fn sweep_args_parses_positional_only() {
        let cli = Cli::try_parse_from(["miner", "sweep", "manifest.toml"]).expect("parse ok");
        let args = unwrap_sweep_args(&cli);
        assert_eq!(args.manifest, PathBuf::from("manifest.toml"));
        assert!(!args.dry_run);
        assert!(args.seed.is_none());
        assert!(args.bootstrap.is_none());
        assert_eq!(args.bootstrap_n, 0);
        assert!(args.null.is_none());
        assert_eq!(args.null_n, 0);
    }

    /// Test 2: clap parses with all flags.
    #[test]
    fn sweep_args_parses_all_flags() {
        let cli = Cli::try_parse_from([
            "miner",
            "sweep",
            "m.toml",
            "--dry-run",
            "--seed",
            "57005",
            "--bootstrap",
            "stationary",
            "--bootstrap-n",
            "500",
            "--null",
            "circular_shift",
            "--null-n",
            "200",
        ])
        .expect("parse ok");
        let args = unwrap_sweep_args(&cli);
        assert_eq!(args.manifest, PathBuf::from("m.toml"));
        assert!(args.dry_run);
        assert_eq!(args.seed, Some(57005));
        assert_eq!(args.bootstrap.as_deref(), Some("stationary"));
        assert_eq!(args.bootstrap_n, 500);
        assert_eq!(args.null.as_deref(), Some("circular_shift"));
        assert_eq!(args.null_n, 200);
    }

    /// Test 3: `SweepArgs::to_manifest` applies CLI overrides over a parsed
    /// TOML manifest. CLI seed and hygiene values override even when the
    /// manifest specifies different values.
    #[test]
    fn sweep_args_to_manifest_applies_cli_overrides() {
        let dir = TempDir::new().expect("tempdir");
        let manifest_path = dir.path().join("m.toml");
        std::fs::write(
            &manifest_path,
            r#"
            [sweep]
            seed = 0xBEEF
            [hygiene]
            bootstrap = "block"
            bootstrap_n = 100
            null = "phase_scramble"
            null_n = 100
            [[jobs]]
            scan = "stats.autocorr.ljung_box@1"
            instruments = ["EURUSD:bid"]
            timeframes = ["15m"]
            windows = ["2024-01-01:2024-01-02"]
            "#,
        )
        .expect("write manifest");

        let args = SweepArgs {
            manifest: manifest_path,
            dry_run: false,
            seed: Some(0xDEAD),
            bootstrap: Some("stationary".to_string()),
            bootstrap_n: 500,
            null: Some("circular_shift".to_string()),
            null_n: 200,
            #[cfg(any(test, feature = "test-internal"))]
            sleep_after_first_finding_ms: None,
        };
        let manifest = args.to_manifest().expect("to_manifest ok");
        assert_eq!(manifest.sweep.seed, Some(0xDEAD), "CLI seed overrides");
        assert_eq!(
            manifest.hygiene.bootstrap.as_deref(),
            Some("stationary"),
            "CLI bootstrap overrides"
        );
        assert_eq!(manifest.hygiene.bootstrap_n, 500, "CLI bootstrap_n overrides");
        assert_eq!(
            manifest.hygiene.null.as_deref(),
            Some("circular_shift"),
            "CLI null overrides"
        );
        assert_eq!(manifest.hygiene.null_n, 200, "CLI null_n overrides");
    }

    /// Test 3b: when no CLI overrides are supplied, the manifest values pass
    /// through unchanged.
    #[test]
    fn sweep_args_to_manifest_leaves_unset_flags_alone() {
        let dir = TempDir::new().expect("tempdir");
        let manifest_path = dir.path().join("m.toml");
        std::fs::write(
            &manifest_path,
            r#"
            [sweep]
            seed = 0xBEEF
            [hygiene]
            bootstrap = "block"
            bootstrap_n = 100
            [[jobs]]
            scan = "stats.autocorr.ljung_box@1"
            instruments = ["EURUSD:bid"]
            timeframes = ["15m"]
            windows = ["2024-01-01:2024-01-02"]
            "#,
        )
        .expect("write manifest");

        let args = SweepArgs {
            manifest: manifest_path,
            dry_run: false,
            seed: None,
            bootstrap: None,
            bootstrap_n: 0,
            null: None,
            null_n: 0,
            #[cfg(any(test, feature = "test-internal"))]
            sleep_after_first_finding_ms: None,
        };
        let manifest = args.to_manifest().expect("to_manifest ok");
        assert_eq!(manifest.sweep.seed, Some(0xBEEF), "manifest seed preserved");
        assert_eq!(manifest.hygiene.bootstrap.as_deref(), Some("block"));
        assert_eq!(manifest.hygiene.bootstrap_n, 100);
    }

    /// Test 3c: missing manifest file → `InvalidParameter` `WireError`.
    #[test]
    fn sweep_args_to_manifest_missing_file_returns_wire_error() {
        let args = SweepArgs {
            manifest: PathBuf::from("/this/path/does/not/exist/m.toml"),
            dry_run: false,
            seed: None,
            bootstrap: None,
            bootstrap_n: 0,
            null: None,
            null_n: 0,
            #[cfg(any(test, feature = "test-internal"))]
            sleep_after_first_finding_ms: None,
        };
        let err = args.to_manifest().expect_err("must reject");
        assert_eq!(err.code, "invalid_parameter");
    }

    /// Test 6: under cfg(test) the `--sleep-after-first-finding-ms` flag is
    /// reachable and parseable by clap.
    #[test]
    #[cfg(any(test, feature = "test-internal"))]
    fn sweep_args_sleep_after_first_finding_ms_under_test_cfg() {
        let cli = Cli::try_parse_from([
            "miner",
            "sweep",
            "m.toml",
            "--sleep-after-first-finding-ms",
            "2000",
        ])
        .expect("parse ok");
        let args = unwrap_sweep_args(&cli);
        assert_eq!(args.sleep_after_first_finding_ms, Some(2000));
    }
}
