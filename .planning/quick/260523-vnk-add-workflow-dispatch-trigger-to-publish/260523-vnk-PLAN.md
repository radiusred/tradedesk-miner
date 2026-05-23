---
phase: quick-260523-vnk
plan: 01
type: execute
wave: 1
depends_on: []
files_modified:
  - .github/workflows/publish.yml
autonomous: true
requirements:
  - QUICK-260523-VNK-01
must_haves:
  truths:
    - "publish.yml can be triggered manually via the GitHub Actions UI / `gh workflow run`"
    - "Manual trigger accepts a required `tag` input naming the draft release to publish"
    - "When dispatched manually, the build job runs even though no workflow_run event exists"
    - "When dispatched manually, the 'Resolve draft release tag' step uses the provided tag verbatim and does NOT call `gh release list`"
    - "The existing workflow_run path (auto-trigger after Prepare Release success) still works unchanged"
    - "publish.yml still parses as valid YAML / GitHub Actions workflow syntax"
  artifacts:
    - path: ".github/workflows/publish.yml"
      provides: "Dual-triggered publish workflow (workflow_run + workflow_dispatch)"
      contains: "workflow_dispatch:"
  key_links:
    - from: "workflow_dispatch.inputs.tag"
      to: "Resolve draft release tag step"
      via: "${{ inputs.tag }} branch"
      pattern: "inputs\\.tag"
    - from: "build job if: gate"
      to: "workflow_dispatch event"
      via: "github.event_name == 'workflow_dispatch'"
      pattern: "github\\.event_name == 'workflow_dispatch'"
---

<objective>
Add a manual-retry escape hatch to `.github/workflows/publish.yml` so we (and future-us) can re-run the publish pipeline against an existing draft release without needing to re-trigger Prepare Release.

Concretely: the v1.0.2 draft release exists with no tarballs attached and no tag pushed. The original publish run was cancelled mid-flight and `gh run rerun` would replay the OLD (broken) publish.yml SHA, not the fixed one shipped in `40c3e90`. Adding a `workflow_dispatch` trigger lets us invoke the workflow at its current HEAD SHA against the existing draft, finishing the v1.0.2 publish today and giving us a permanent retry path going forward.

Purpose: Unblock v1.0.2 publish AND make publish.yml self-recoverable.
Output: One modified workflow file, three surgical edits, comments preserved.
</objective>

<execution_context>
@$HOME/.claude/get-shit-done/workflows/execute-plan.md
@$HOME/.claude/get-shit-done/templates/summary.md
</execution_context>

<context>
@.planning/STATE.md
@.github/workflows/publish.yml

<interfaces>
<!-- Current relevant slices of publish.yml. The executor uses these as exact anchors for Edit-tool operations. -->

Top-of-file trigger block (lines 18-21):
```yaml
on:
  workflow_run:
    workflows: ["Prepare Release"]
    types: [completed]
```

Build-job gate (lines 27-30):
```yaml
  build:
    name: Build ${{ matrix.target }}
    # Only run when Prepare Release succeeded; ignore failed / cancelled runs.
    if: ${{ github.event.workflow_run.conclusion == 'success' }}
```

Resolve-tag step (lines 65-85):
```yaml
      - name: Resolve draft release tag
        id: tag
        env:
          GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          REPO: ${{ github.repository }}
        run: |
          set -euo pipefail
          # Find the draft release that prepare-release.yml just created.
          # If there are multiple drafts (unlikely), use the most recently
          # created one — prepare-release runs serially per workflow_run.
          # NOTE: this step runs BEFORE actions/checkout, so we pass --repo
          # explicitly (gh otherwise tries to autodetect from CWD's .git).
          TAG=$(gh release list --repo "$REPO" --limit 10 \
                  --json tagName,isDraft,createdAt \
                  --jq 'map(select(.isDraft == true)) | sort_by(.createdAt) | last | .tagName')
          if [ -z "$TAG" ] || [ "$TAG" = "null" ]; then
            echo "::error::No draft release found. Did prepare-release.yml create one?"
            exit 1
          fi
          echo "::notice::Building artifacts for draft release ${TAG}"
          echo "tag=${TAG}" >> "$GITHUB_OUTPUT"
```
</interfaces>
</context>

<tasks>

<task type="auto">
  <name>Task 1: Add workflow_dispatch trigger + branch resolve-tag step</name>
  <files>.github/workflows/publish.yml</files>
  <action>
Make exactly THREE Edit operations on `.github/workflows/publish.yml`. Preserve every existing comment verbatim except where instructed. Do not reformat or re-indent unrelated lines. YAML indentation is two spaces throughout this file — match it exactly.

EDIT A — Extend the trigger block and prepend a brief escape-hatch note.

Find this exact block (lines 18-21):
```
on:
  workflow_run:
    workflows: ["Prepare Release"]
    types: [completed]
```

Replace with:
```
# Manual escape hatch: `workflow_dispatch` lets us re-run this workflow at
# the current HEAD SHA against an existing draft release (e.g. when an
# earlier run was cancelled and `gh run rerun` would replay a stale SHA).
on:
  workflow_run:
    workflows: ["Prepare Release"]
    types: [completed]
  workflow_dispatch:
    inputs:
      tag:
        description: "Draft release tag to publish (e.g. v1.0.2)"
        required: true
        type: string
```

EDIT B — Widen the build-job `if:` gate and update its inline comment.

Find this exact pair of lines (currently lines 29-30):
```
    # Only run when Prepare Release succeeded; ignore failed / cancelled runs.
    if: ${{ github.event.workflow_run.conclusion == 'success' }}
```

Replace with:
```
    # Run on manual dispatch OR when Prepare Release succeeded; ignore failed / cancelled workflow_run events.
    if: ${{ github.event_name == 'workflow_dispatch' || github.event.workflow_run.conclusion == 'success' }}
```

EDIT C — Branch the "Resolve draft release tag" step so manual dispatch uses the provided tag verbatim.

Find this exact `run:` body inside the `Resolve draft release tag` step (currently lines 70-85):
```
        run: |
          set -euo pipefail
          # Find the draft release that prepare-release.yml just created.
          # If there are multiple drafts (unlikely), use the most recently
          # created one — prepare-release runs serially per workflow_run.
          # NOTE: this step runs BEFORE actions/checkout, so we pass --repo
          # explicitly (gh otherwise tries to autodetect from CWD's .git).
          TAG=$(gh release list --repo "$REPO" --limit 10 \
                  --json tagName,isDraft,createdAt \
                  --jq 'map(select(.isDraft == true)) | sort_by(.createdAt) | last | .tagName')
          if [ -z "$TAG" ] || [ "$TAG" = "null" ]; then
            echo "::error::No draft release found. Did prepare-release.yml create one?"
            exit 1
          fi
          echo "::notice::Building artifacts for draft release ${TAG}"
          echo "tag=${TAG}" >> "$GITHUB_OUTPUT"
```

Replace with:
```
        env:
          GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          REPO: ${{ github.repository }}
          INPUT_TAG: ${{ inputs.tag }}
        run: |
          set -euo pipefail
          # On manual dispatch, the operator names the draft tag directly —
          # skip auto-detection so an out-of-order draft can't be picked.
          if [ -n "${INPUT_TAG:-}" ]; then
            TAG="${INPUT_TAG}"
          else
            # Find the draft release that prepare-release.yml just created.
            # If there are multiple drafts (unlikely), use the most recently
            # created one — prepare-release runs serially per workflow_run.
            # NOTE: this step runs BEFORE actions/checkout, so we pass --repo
            # explicitly (gh otherwise tries to autodetect from CWD's .git).
            TAG=$(gh release list --repo "$REPO" --limit 10 \
                    --json tagName,isDraft,createdAt \
                    --jq 'map(select(.isDraft == true)) | sort_by(.createdAt) | last | .tagName')
          fi
          if [ -z "$TAG" ] || [ "$TAG" = "null" ]; then
            echo "::error::No draft release found. Did prepare-release.yml create one?"
            exit 1
          fi
          echo "::notice::Building artifacts for draft release ${TAG}"
          echo "tag=${TAG}" >> "$GITHUB_OUTPUT"
```

CRITICAL NOTE on EDIT C: the original step already declares an `env:` block above this `run:` body (with `GH_TOKEN` and `REPO`). The replacement above DUPLICATES that `env:` block plus adds `INPUT_TAG`. Before applying EDIT C, inspect the existing step in the file — if the existing `env:` block is still present immediately above the `run:` you're replacing, REMOVE the duplicated `env:` lines (`GH_TOKEN`, `REPO`) from the replacement and ADD ONLY the `INPUT_TAG: ${{ inputs.tag }}` line into the existing `env:` block. Net result either way: the step must have `GH_TOKEN`, `REPO`, and `INPUT_TAG` in `env:` exactly once, and the `run:` body must contain the new branch.

Do NOT touch the `manifest` job, the `publish` job, the matrix, the staging step, the upload step, or `.github/workflows/prepare-release.yml`.
  </action>
  <verify>
    <automated>python3 -c "import yaml; yaml.safe_load(open('.github/workflows/publish.yml'))" &amp;&amp; grep -c "workflow_dispatch:" .github/workflows/publish.yml | grep -qx 1 &amp;&amp; grep -q "github.event_name == 'workflow_dispatch'" .github/workflows/publish.yml &amp;&amp; grep -q "INPUT_TAG:" .github/workflows/publish.yml &amp;&amp; grep -q 'description: "Draft release tag to publish (e.g. v1.0.2)"' .github/workflows/publish.yml &amp;&amp; grep -c "^      - name: Resolve draft release tag" .github/workflows/publish.yml | grep -qx 1</automated>
  </verify>
  <done>
    - `python3 -c "import yaml; yaml.safe_load(...)"` succeeds (file parses as valid YAML)
    - `workflow_dispatch:` appears exactly once in the `on:` block, with a required string input named `tag`
    - The build job's `if:` gate includes `github.event_name == 'workflow_dispatch'` as the first disjunct
    - The Resolve-tag step's `env:` block contains `INPUT_TAG: ${{ inputs.tag }}` exactly once alongside the pre-existing `GH_TOKEN` and `REPO`
    - The Resolve-tag step's `run:` body branches on `[ -n "${INPUT_TAG:-}" ]` and only calls `gh release list` in the else branch
    - The pre-existing `workflow_run` trigger and all its comments remain unchanged
    - Manifest job, publish job, matrix, build/stage/upload steps untouched
    - `prepare-release.yml` untouched
  </done>
</task>

</tasks>

<verification>
File-level sanity:
- `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/publish.yml'))"` exits 0
- `git diff --stat .github/workflows/publish.yml` shows only this one file changed
- `git diff .github/workflows/prepare-release.yml` is empty

Behavioural smoke (post-merge, run manually by user — NOT part of executor verify):
- `gh workflow run publish.yml -f tag=v1.0.2` from `main` after this PR merges
- Resulting run logs show "Building artifacts for draft release v1.0.2" without calling `gh release list`
- Existing trigger still intact: next time `prepare-release.yml` completes, publish.yml fires via workflow_run as before
</verification>

<success_criteria>
- `.github/workflows/publish.yml` parses as valid YAML
- The file contains all three edits (trigger, gate, resolve-tag branch) with comments preserved
- No other files in the repo are modified
- The diff is small (< ~25 net added lines) and surgical
</success_criteria>

<output>
Create `.planning/quick/260523-vnk-add-workflow-dispatch-trigger-to-publish/260523-vnk-01-SUMMARY.md` when done.
</output>
