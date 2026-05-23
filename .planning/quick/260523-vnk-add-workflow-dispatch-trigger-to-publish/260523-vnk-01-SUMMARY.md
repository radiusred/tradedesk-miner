---
phase: quick-260523-vnk
plan: 01
subsystem: ci
tags: [ci, github-actions, release, escape-hatch]
dependency_graph:
  requires:
    - .github/workflows/publish.yml (pre-existing workflow_run trigger + Resolve-tag step)
  provides:
    - Manual workflow_dispatch trigger on publish.yml
    - Operator-supplied draft-release tag input
    - Permanent self-recovery path for cancelled / stale-SHA publish runs
  affects:
    - Unblocks v1.0.2 publish (draft release exists, no tarballs attached)
    - Future publish-failure recovery without re-running prepare-release.yml
tech_stack:
  added: []
  patterns:
    - "GitHub Actions dual-trigger workflow (workflow_run + workflow_dispatch)"
    - "Conditional job execution via github.event_name in if: gate"
    - "Step-level env passthrough of workflow_dispatch.inputs.*"
key_files:
  created: []
  modified:
    - .github/workflows/publish.yml
decisions:
  - "Use workflow_dispatch (not repository_dispatch) — operator-facing UI button in the Actions tab + first-class gh CLI support via `gh workflow run`"
  - "`tag` input is required (no auto-detect fallback on manual dispatch) — eliminates the risk of an out-of-order draft being picked when the operator's intent is to re-publish a specific tag"
  - "Merge INPUT_TAG into the existing env: block rather than duplicating GH_TOKEN/REPO — keeps the Resolve step's env declaration as a single source of truth"
  - "Branch the Resolve step's run: body on `[ -n \"${INPUT_TAG:-}\" ]` (not on github.event_name) — keeps the bash logic decoupled from the GitHub Actions context and lets both code paths share the downstream tag-validation block"
metrics:
  duration_sec: 71
  completed_date: "2026-05-23"
  tasks: 1
  files_modified: 1
  commits: 1
---

# Quick Task 260523-vnk: workflow_dispatch trigger on publish.yml

Adds a manual-retry escape hatch to `.github/workflows/publish.yml` so the publish pipeline can be re-run at the current HEAD SHA against an existing draft release without needing to re-trigger `prepare-release.yml`.

## What Changed

One file: `.github/workflows/publish.yml` (+26 / -10 lines, 1 commit). Three surgical edits delivered as planned:

1. **EDIT A — Trigger block extended.** Added `workflow_dispatch.inputs.tag` (required string, with `description: "Draft release tag to publish (e.g. v1.0.2)"`) alongside the existing `workflow_run` trigger. Prepended an inline comment block explaining the escape-hatch purpose (3 lines, sits directly above the existing `on:` key).
2. **EDIT B — Build-job `if:` gate widened.** Changed from
   `if: ${{ github.event.workflow_run.conclusion == 'success' }}` to
   `if: ${{ github.event_name == 'workflow_dispatch' || github.event.workflow_run.conclusion == 'success' }}`
   and updated the inline comment so the gate's intent reads correctly under both triggers. Manual-dispatch disjunct is first so the gate short-circuits on the new path.
3. **EDIT C — Resolve-tag step branched on operator input.** Merged `INPUT_TAG: ${{ inputs.tag }}` into the step's *existing* `env:` block (no duplicated `GH_TOKEN` / `REPO`, per the plan's CRITICAL NOTE). Wrapped the `run:` body's pre-existing `gh release list` lookup in an `else` branch and added a fast path: when `INPUT_TAG` is non-empty, `TAG="${INPUT_TAG}"` is used verbatim and `gh release list` is skipped entirely. The shared `[ -z "$TAG" ] || [ "$TAG" = "null" ]` validation block downstream is reused for both paths.

Everything else — `manifest` job, `publish` job, the matrix, the staging step, the upload step, and `prepare-release.yml` — is untouched.

## Why This Way

- The original v1.0.2 publish run was cancelled mid-flight. The draft release exists with no tarballs attached and no tag pushed. `gh run rerun` would replay the *old* (broken) `publish.yml` SHA — not the fix shipped in `40c3e90`. `workflow_dispatch` re-runs always execute the workflow at the branch's current HEAD SHA, which is exactly the recovery semantics needed.
- Beyond unblocking v1.0.2 specifically, this change converts publish.yml into a self-recoverable workflow for the indefinite future: any cancelled / failed publish run can be re-driven by hand against the existing draft without needing prepare-release.yml to fire again.

## Verification

Plan's automated verify command (single bash one-liner from the `<verify>` block) passes:

```
python3 -c "import yaml; yaml.safe_load(open('.github/workflows/publish.yml'))"  # OK
grep -c "workflow_dispatch:" .github/workflows/publish.yml                       # = 1
grep -q "github.event_name == 'workflow_dispatch'"                               # present
grep -q "INPUT_TAG:"                                                             # present
grep -q 'description: "Draft release tag to publish (e.g. v1.0.2)"'              # present
grep -c "^      - name: Resolve draft release tag"                               # = 1 (no duplicate step)
```

File-level sanity from the plan's `<verification>` section:
- `git diff --stat .github/workflows/publish.yml` → `1 file changed, 26 insertions(+), 10 deletions(-)` (single file, surgical scope)
- `git diff .github/workflows/prepare-release.yml` → empty (sibling workflow untouched)
- `GH_TOKEN` appears exactly once in the Resolve-tag step (no duplicated `env:` block)

## Deviations from Plan

None — plan executed exactly as written. The plan's CRITICAL NOTE on EDIT C correctly anticipated the existing `env:` block was still present, and the merge-rather-than-duplicate path was taken.

## Behavioural Smoke (post-merge, manual)

Out of executor scope per the plan, but for record:
- After PR merges to `main`, run `gh workflow run publish.yml -f tag=v1.0.2` from `main`.
- Resulting run logs should show `::notice::Building artifacts for draft release v1.0.2` without any preceding `gh release list` call.
- Existing trigger remains intact: next `prepare-release.yml` success fires `publish.yml` via `workflow_run` exactly as before.

## Commits

- `153943c` — fix(ci): add workflow_dispatch trigger to publish.yml for manual retries

## Self-Check: PASSED

- `.github/workflows/publish.yml` — FOUND (26 lines added, 10 removed)
- `.planning/quick/260523-vnk-add-workflow-dispatch-trigger-to-publish/260523-vnk-01-SUMMARY.md` — created by this step
- Commit `153943c` — FOUND in `git log --oneline -1`
- `prepare-release.yml` — unchanged (empty diff)
- `workflow_dispatch:` count = 1, `INPUT_TAG:` count = 1, `GH_TOKEN:` in Resolve step = 1 (no duplication)
