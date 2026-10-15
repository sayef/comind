# CI and merge requests

**Model.** One central pipeline checks out the team's repos, runs
`comind link … --embed [--enrich]`, and writes a new **atomically-versioned** LanceDB dataset.
Lance's newest version manifest is the "latest" pointer every consumer reads — no coordination, no
staleness. comind needs **no remote git credentials** itself: the CI job clones the repos (with its
token); comind indexes the local checkouts and their `.git`.

The output location (`--index-dir`) is either a **local directory** (published as a downloadable CI
artifact) or an **`s3://…`** path. The shipped GitHub workflow uses the local-path + artifact
approach; the GitLab example below uses S3. Both are equivalent.

## GitHub Actions (shipped)

[`.github/workflows/comind-index.yml`](https://github.com/sayef/comind/blob/main/.github/workflows/comind-index.yml) builds the index on a
schedule (and on demand). To enable it:

- **Variable** `COMIND_INDEX_ENABLED = true` — lets the schedule run.
- **Variable** `COMIND_REPOS` — space/newline list of `owner/repo` to index (kept in settings, not
  committed, so repo names stay out of the public workflow file).
- **Secret** `COMIND_REPO_TOKEN` — a fine-grained PAT with `Contents: read` (only for **private**
  repos; omit for public).
- **Secret** `OPENAI_API_KEY` *(optional)* — turns on `--enrich`.

It clones the listed repos, runs `comind link … --embed --incremental`, and uploads the resulting
index directory (which contains the internal `_graph` dataset) as the `comind-index` artifact. Consume it with:

```bash
gh run download -R <owner>/comind -n comind-index   # → ./ (contains _graph)
comind serve --index-dir .
```

## Member repos trigger a reindex on push to main

Each product repo can fire the central pipeline when it merges to `main`.

**GitHub** (member repo → `notify-comind.yml`), matching the `repository_dispatch` hook in
`comind-index.yml`:

```yaml
on: { push: { branches: [main] } }
jobs:
  notify:
    runs-on: ubuntu-latest
    steps:
      - run: |
          curl -sf -X POST \
            -H "Authorization: token ${{ secrets.COMIND_DISPATCH_TOKEN }}" \
            -H "Accept: application/vnd.github+json" \
            https://api.github.com/repos/<owner>/comind/dispatches \
            -d '{"event_type":"comind-reindex"}'
```

**GitLab** (member repo `.gitlab-ci.yml`) — trigger the comind project's pipeline via a
[pipeline trigger token](https://docs.gitlab.com/ci/triggers/):

```yaml
comind-reindex:
  stage: deploy
  rules: [{ if: '$CI_COMMIT_BRANCH == "main"' }]
  script:
    - curl -sf -X POST -F token=$COMIND_TRIGGER_TOKEN -F ref=main
        "$CI_SERVER_URL/api/v4/projects/$COMIND_PROJECT_ID/trigger/pipeline"
```

## GitLab indexing pipeline (S3 variant)

```yaml
index:
  image: rust:1.97
  variables:
    COMIND_S3_URI: s3://YOUR-BUCKET/lancedb/org
    AWS_REGION: us-east-1
  rules:
    - if: '$CI_PIPELINE_SOURCE == "trigger"'
    - if: '$CI_PIPELINE_SOURCE == "schedule"'
  cache:
    key: { files: [Cargo.lock] }
    paths: [.cargo/registry, target]
  before_script:
    - apt-get update && apt-get install -y protobuf-compiler cmake
    # Clone member repos with full history (deploy/job token, read scope).
    - git clone "https://oauth2:${COMIND_REPO_TOKEN}@gitlab.com/<group>/pkg-common.git" repos/pkg-common
    - git clone "https://oauth2:${COMIND_REPO_TOKEN}@gitlab.com/<group>/service-a.git" repos/service-a
    # … more repos
  script:
    - cargo build --release
    - ./target/release/comind link repos/* --index-dir "$COMIND_S3_URI" --embed --incremental
```

Secrets: `COMIND_REPO_TOKEN` (read repos), AWS creds (OIDC role or keys), and an LLM key if using
`--enrich`.

## SCIP (compiler-grade resolution)

`index`/`link` default to `--scip auto`: a repo's `index.scip` (any depth ≤ 4, gitignored is fine)
is ingested; otherwise any SCIP indexer on `PATH` that matches the repo's manifests runs
(`scip-typescript`, `scip-python`, `scip-go`, `scip-java`, `rust-analyzer scip`). Indexers need the
repo's dependencies installed, so install them before indexing. `--scip require` fails the job when
a repo yields no SCIP; `--scip off` skips it. Tree-sitter resolution covers everything SCIP doesn't.

## LLM provider (for `--enrich` / `--flows`)

Enrichment uses an LLM via Rig — provider-agnostic. Defaults to OpenAI (`OPENAI_API_KEY`, model
`gpt-4o-mini`). Set `COMIND_LLM_MODEL` and `COMIND_LLM_BASE_URL` to target any OpenAI-compatible
endpoint (a LiteLLM proxy, Ollama, vLLM, Azure).

## Per-repo incremental (optimization)

`comind index <repo> --index-dir <dir> --incremental` diffs the last-indexed commit (stored in the Lance
`repo_meta` table) against HEAD and reparses only changed files — useful for keeping a per-repo
dataset fresh cheaply. Today the central `link` re-parses sources; org-level re-merge from per-repo
datasets is a future optimization.

## Merge-request preflight

Run `preflight` in each member repo's MR pipeline against the shared index and post the report as
an MR note. It lists recorded constraints and dead ends on the touched code, contracts other
services consume, blast radius, missing co-changes and failing constraint checks. `--strict` fails
the job when a `must` constraint check is violated in a changed file. Checks run on the shared
index, so they see the default branch plus the MR's changed paths, not edges new in the MR.

```yaml
comind-preflight:
  stage: test
  rules: [{ if: '$CI_PIPELINE_SOURCE == "merge_request_event"' }]
  script:
    - comind preflight . --base "$CI_MERGE_REQUEST_DIFF_BASE_SHA" --index-dir "$COMIND_INDEX" --strict > preflight.md || STATUS=$?
    - |
      jq -Rs '{body: .}' preflight.md | curl -sf -X POST \
        -H "PRIVATE-TOKEN: $COMIND_NOTE_TOKEN" -H "Content-Type: application/json" \
        --data @- "$CI_API_V4_URL/projects/$CI_PROJECT_ID/merge_requests/$CI_MERGE_REQUEST_IID/notes"
    - exit ${STATUS:-0}
```

After linking, the scheduled index job should also run, against the same `--index-dir`:

```bash
comind harvest <repos…> --index-dir "$COMIND_INDEX"     # reverts and commit trailers → records (no LLM)
comind ledger --compact --index-dir "$COMIND_INDEX"     # fold new records into the snapshot
comind ledger --check --index-dir "$COMIND_INDEX"       # optional: fail on violated `must` rules
```

Harvested ids are deterministic, so re-running it never duplicates records.


## Distil the merged MR (forward only)

On each pipeline of the default branch, `comind distill mr` finds the merge request that merged
the commit and, if it has a human discussion or a real description, asks the configured LLM for
at most three ledger records (failed attempts, decisions, constraints, gotchas). `@mentions` are
removed before sending; proposals land at T1 and need a human confirmation before agents see
them. Past MRs are not mined.

```yaml
comind-distill:
  stage: .post
  rules: [{ if: '$CI_COMMIT_BRANCH == $CI_DEFAULT_BRANCH' }]
  allow_failure: true
  script:
    - comind distill mr . --index-dir "$COMIND_INDEX"
```

Variables: `GITLAB_TOKEN` (project access token, `read_api`; the job token cannot read MR
discussions) and `OPENAI_API_KEY` (or `COMIND_LLM_BASE_URL` for a self-hosted model). Use
`--list` to see what would be sent without calling the LLM.
