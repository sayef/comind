# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

First release of comindly: cross-repo code intelligence for coding agents, in one binary and your own
storage.

### Added

- Indexing of many repositories into one versioned graph in a local folder or S3 (`comindly index`,
  `comindly link`), with tree-sitter for Python, TypeScript/JavaScript, Go, Java, Kotlin and Rust,
  SCIP indexes when present, and incremental rebuilds.
- An MCP server (`comindly serve`) and CLI for agents and people: hybrid code search, zoom, blast
  radius, context packs, flows and evaluation (`comindly eval graph`).
- Service-contract graph: HTTP routes and clients (FastAPI, Next.js, Express, Spring MVC, Feign,
  RestTemplate, WebClient, Go net/http/gin/echo/chi, axum, actix, reqwest, httpx/requests/fetch/axios),
  SQS/SNS/S3 (also through wrappers and constructors), SQL tables, Terraform queues/topics/buckets and
  subscriptions, values from YAML config; `comindly contracts` and the MCP `contracts` tool.
- Package versions and version skew across repos (`comindly packages`).
- Conventions and co-change recipes (`comindly conventions`, MCP `guide`); the team can add, edit and hide
  conventions from the CLI or the web UI.
- Reasoning ledger: typed records anchored to symbols, stored in your own store, with derived staleness,
  trust levels, review (`remember`, `why`, `review`, `ledger`), self-enforcing constraint checks, SSH
  signing, compaction and erasure. Records come from people and from agents through MCP and hooks, show
  who wrote and reviewed them, and can be confirmed, archived, invalidated or deleted.
- `comindly preflight` for uncommitted changes and MR pipelines; Claude Code and generic agent hooks with a
  local mirror for remote indexes; `comindly doctor`.
- A desktop app on macOS (`comindly app`: the web UI in a native WebKit window), the local web UI
  (`comindly ui --web [--open]`, guarded by an access token) and the terminal UI (`comindly ui`): org and
  symbol dependency graphs, a version slider, contracts, packages, ledger review and a command palette.
- Repositories and Settings in the app: what is in the index and where it lives on this machine, scan,
  clone over HTTPS or SSH, index with progress in comindly's own worktrees; `comindly repos`.
- Team settings stored with the index (LLM, embedding model, LLM steps), which win over personal ones;
  API keys stay per person in `~/.config/comindly/credentials` or `OPENAI_API_KEY`; `comindly team`,
  `comindly join` and a Join flow in the app.
- S3 through the standard AWS chain (profiles, SSO, `credential_process`, custom endpoints such as MinIO),
  with S3 errors explained and credentials kept out of messages.
- Docs at comindly.pages.dev, and `examples/acme`, a small example shop of five repositories used for the
  screenshots (`scripts/screenshots.sh`).
