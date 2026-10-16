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
  signing, compaction and erasure. Records come from people and from agents through MCP and hooks.
- `comind preflight` for uncommitted changes and MR pipelines; Claude Code and generic agent hooks with a
  local mirror for remote indexes; `comind doctor`.
- Terminal UI (`comind ui`) and local web UI (`comind ui --web`): a React app embedded in the binary,
  with a WebGL org map and symbol graph, a version slider, contracts, ledger review and a command palette.
