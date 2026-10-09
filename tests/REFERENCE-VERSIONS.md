# Reference Versions — Workspace-level

This file pins the external reference toolchains used to generate golden
fixtures consumed by the workspace integration-test suite. It lives at the
workspace root because Phase 5 added cross-language reference provenance
(`R 4.x` for BH-FDR + Politis-White block-length goldens) that does not fit
the per-crate `tests/goldens/REFERENCE-VERSIONS.md` layout — the
language-specific pinning lives next to the goldens themselves.

For Phase 4 statsmodels / scipy / pandas pinning, see
`crates/miner-core/tests/goldens/REFERENCE-VERSIONS.md`.

## Phase 5 reference (R 4.x + tseries + stats core)

The Phase 5 statistical hygiene layer ships these reference goldens (where
applicable):

- **Benjamini-Hochberg FDR** — R `stats::p.adjust(method = "BH")` is the
  reference oracle for the `bh_fdr` kernel (Plan 05-02 / HYG-02).
- **Politis-White (2009) block-length** — R `tseries::b.star()` is the
  reference oracle for the stationary-bootstrap block-length heuristic
  used by Plan 05-02's bootstrap kernel.

### Pinned versions

| Tool / package | Pinned version | Purpose |
| -------------- | -------------- | ------- |
| `R`            | 4.4.x          | Reference interpreter (apt/dnf/Homebrew default). |
| `tseries`      | 0.10.x         | Politis-White (2009) `b.star()` heuristic. |
| `stats` (built-in) | shipped with R 4.4.x | `p.adjust(method = "BH")` BH-FDR oracle. |

Install (Debian/Ubuntu):

```sh
sudo apt-get install r-base r-cran-tseries
```

Install (`renv`-managed) — minimal `renv.lock` excerpt:

```r
install.packages("tseries", version = "0.10-58", repos = "https://cloud.r-project.org")
```

### Regeneration recipe (manual-only path)

Plan 05-02 ships golden p-values + q-values + block-length references as
checked-in fixtures under `crates/miner-core/tests/goldens/phase5/`. The
canonical regen recipe is:

```sh
# Output the BH-FDR goldens to JSONL; consumed by
# crates/miner-core/tests/bh_fdr_goldens.rs
Rscript scripts/gen_fdr_goldens.R > crates/miner-core/tests/goldens/phase5/bh_fdr.jsonl

# Output the Politis-White block-length goldens; consumed by
# crates/miner-core/tests/bootstrap_block_length_golden.rs
Rscript scripts/gen_block_length_goldens.R > crates/miner-core/tests/goldens/phase5/block_length.jsonl
```

If `scripts/gen_fdr_goldens.R` / `scripts/gen_block_length_goldens.R` are
absent in the current source tree, the goldens are **manually-only
verifiable** per `.planning/phases/05-statistical-hygiene-sweep-runner/05-VALIDATION.md`'s
"Manual-Only Verifications" table. The `bootstrap_block_length_golden.rs`
integration test is `#[ignore]`d in CI until the R provenance scripts are
checked in.

### Rationale

R `stats::p.adjust(method = "BH")` is the canonical implementation of
Benjamini-Hochberg (1995) FDR control — present in base R since version
2.x, defined exactly by the original paper's recipe, and used as the
reference in every statistical-methods textbook published since.
`tseries::b.star()` is the canonical implementation of Politis-White
(2009)'s block-length heuristic for the stationary bootstrap (Politis &
Romano 1994) — there is no equivalent reference in Python's scientific
ecosystem (`statsmodels` ships `arch_model` for ARCH-family bootstraps,
but not the Politis-White heuristic).

Pinning R + `tseries` + `stats` is therefore the minimal reference-
language surface for Phase 5; no additional pins are required.
