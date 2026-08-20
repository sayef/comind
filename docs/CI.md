# Comind CI — building the shared org index

**Model.** One central pipeline checks out the team's repos, runs
`comind link … --embed [--enrich]`, and writes a new **atomically-versioned** LanceDB dataset.
Lance's newest version manifest is the "latest" pointer every consumer reads — no coordination, no
staleness. comind needs **no remote git credentials** itself: the CI job clones the repos (with its
token); comind indexes the local checkouts and their `.git`.

The output location (`--index-dir`) is either a **local directory** (published as a downloadable CI
artifact) or an **`s3://…`** path. The shipped GitHub workflow uses the local-path + artifact
approach; the GitLab example below uses S3. Both are equivalent.

## GitHub Actions (shipped)

[`.github/workflows/comind-index.yml`](../.github/workflows/comind-index.yml) builds the index on a
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

## LLM provider (for `--enrich` / `--flows`)

Enrichment uses an LLM via Rig — provider-agnostic. Defaults to OpenAI (`OPENAI_API_KEY`, model
`gpt-4o-mini`). Set `COMIND_LLM_MODEL` and `COMIND_LLM_BASE_URL` to target any OpenAI-compatible
endpoint (a LiteLLM proxy, Ollama, vLLM, Azure).

## Per-repo incremental (optimization)

`comind index <repo> --index-dir <dir> --incremental` diffs the last-indexed commit (stored in the Lance
`repo_meta` table) against HEAD and reparses only changed files — useful for keeping a per-repo
dataset fresh cheaply. Today the central `link` re-parses sources; org-level re-merge from per-repo
datasets is a future optimization.
