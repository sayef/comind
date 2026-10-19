# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- Evaluation harness (`comind eval graph`) with a fixture org and gold edges, run as a test.
- Fact-based cross-repo resolver for Python, TypeScript/JavaScript, Go, Java, Kotlin and Rust, with
  SCIP indexes used when present.
- Service-contract graph: HTTP routes and clients (FastAPI, Next.js, Express, Spring MVC, Feign,
  RestTemplate, WebClient, Go net/http/gin/echo/chi, axum, actix, reqwest, httpx/requests/fetch/axios),
  SQS/SNS/S3 (also through wrappers and constructors), SQL tables, Terraform queues/topics/buckets and
  subscriptions, values from YAML config; `comind contracts` and the MCP `contracts` tool.
- Package versions and version skew across repos (`comind packages`).
- Conventions and co-change recipes (`comind conventions`, MCP `guide`); the team can add, edit and hide
  conventions from the CLI or the web UI.
- Reasoning ledger: typed records anchored to symbols, stored in your own store, with derived staleness,
  trust levels, review (`remember`, `why`, `review`, `ledger`), self-enforcing constraint checks, SSH
  signing, compaction and erasure. Records come from people and from agents through MCP and hooks, show
  who wrote and reviewed them, and can be confirmed, archived, invalidated or deleted.
- `comind preflight` for uncommitted changes and MR pipelines; Claude Code and generic agent hooks with a
  local mirror for remote indexes; `comind doctor`.
- Repositories and Settings in the app: scan a folder and pick repositories, clone, index with progress
  (each repository's default branch in comind's own worktree), and settings for the index location, AWS
  profile and name; `comind repos` and `comind link` with no folders.
- Team settings stored with the index (LLM, embedding model, LLM steps and an API key encrypted with a
  generated team password), which win over personal ones; `comind team`, `comind join` and a Join flow in
  the app.
- S3 through the standard AWS chain (profiles, SSO, `credential_process`, custom endpoints such as MinIO).
- A desktop app on macOS (`comind app`: the web UI in a native WebKit window), the local web UI
  (`comind ui --web [--open]`, guarded by an access token) and the terminal UI (`comind ui`): a React app embedded in the binary,
  with dependency graphs for the org and each symbol (React Flow), a version slider, contracts, ledger review and a command palette.
