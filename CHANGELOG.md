# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- Settings → Repositories → **From GitHub or GitLab**: list an organisation or group (github.com, GitHub
  Enterprise, gitlab.com, self-hosted GitLab with subgroups), pick repositories, and comindly clones and
  keeps them itself; `comindly repos add <clone URL>` does the same from the terminal.
- A team's repository list in its settings: colleagues see the repositories they are missing and add them
  in one click.
- **Create a team** on the first screen, next to **Join a team**; joining works before the first index.
- comindly's icon in the macOS Dock and app switcher for `comindly app`.
- `install.sh` verifies the download's SHA-256, and every release is installed on Linux and macOS and with
  `cargo install` before it counts.

### Changed

- Indexing always uses each repository's current default branch, asked from the remote; the per-repository
  branch setting and `repos add --branch` are gone.
- Connect is now **Get started**, and Repositories moved under **Settings**.
- Messages read as sentences, long commands wrap, and AWS credential errors are one line.

## [0.1.0](https://github.com/sayef/comindly/releases/tag/v0.1.0) - 2026-10-19

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
