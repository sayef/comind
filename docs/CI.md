# CoMind CI — building the shared org index

**Model.** One central *indexing* pipeline checks out the team's repos, runs
`comind link … --to <s3> --embed --enrich`, and writes a new **atomically-versioned** Lance
dataset to S3. Lance's newest version manifest is the "latest" pointer every consumer reads, so
there is no coordination and no staleness. comind needs **no remote git credentials** itself — the
CI job clones the repos (with its token); comind indexes the local checkouts + their `.git`.

- GitHub Actions: [`.github/workflows/comind-index.yml`](../.github/workflows/comind-index.yml).
- GitLab CI: the example below.

## Member repos trigger a reindex on push to main

Each product repo fires the central pipeline when it merges to `main`.

**GitHub** (member repo → `.github/workflows/notify-comind.yml`):

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
            https://api.github.com/repos/your-org/comind/dispatches \
            -d '{"event_type":"comind-reindex"}'
```

**GitLab** (member repo `.gitlab-ci.yml`): trigger the comind project's pipeline via a
[pipeline trigger token](https://docs.gitlab.com/ci/triggers/):

```yaml
comind-reindex:
  stage: deploy
  rules: [{ if: '$CI_COMMIT_BRANCH == "main"' }]
  script:
    - curl -sf -X POST -F token=$COMIND_TRIGGER_TOKEN -F ref=main
        "$CI_SERVER_URL/api/v4/projects/$COMIND_PROJECT_ID/trigger/pipeline"
```

## GitLab indexing pipeline (central `comind` project `.gitlab-ci.yml`)

```yaml
index:
  image: rust:1.97
  variables:
    COMIND_S3_URI: s3://bucket-temporary-test/lancedb/org
    AWS_REGION: eu-central-1
    COMIND_LLM_MODEL: gpt-4o-mini
  rules:
    - if: '$CI_PIPELINE_SOURCE == "trigger"'
    - if: '$CI_PIPELINE_SOURCE == "schedule"'
  cache:
    key: { files: [Cargo.lock] }
    paths: [.cargo/registry, target]
  before_script:
    - apt-get update && apt-get install -y protobuf-compiler
    # Clone member repos with full history (a deploy token / CI job token with read scope).
    - git clone --depth=0 "https://oauth2:${COMIND_REPO_TOKEN}@gitlab.com/your-org/shared-lib.git" repos/shared-lib
    - git clone --depth=0 "https://oauth2:${COMIND_REPO_TOKEN}@gitlab.com/your-org/odin.git" repos/odin
    # … skill-graph, vineyard, skill-detector
  script:
    - cargo build --release -p comind-cli
    - ./target/release/comind link
        repos/shared-lib repos/odin repos/skill-graph repos/vineyard repos/skill-detector
        --to "$COMIND_S3_URI" --embed --enrich --enrich-top 200
```

Secrets required: `COMIND_REPO_TOKEN` (read repos), AWS creds (OIDC role or keys),
`OPENAI_API_KEY` (only if `--enrich`).

## Per-repo incremental (optimization)

For large repos, a per-repo job can keep a per-repo dataset fresh cheaply and the central job
re-merges. `comind index <repo> --to <s3>/repos --incremental` diffs the last-indexed commit
(stored in the Lance `repo_meta` table) against HEAD and reparses only changed files. Org-level
re-merge from per-repo Lance datasets is the next step (today `link` re-parses sources).
