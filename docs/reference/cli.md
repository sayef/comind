# Command line

| Command | Purpose |
|---|---|
| `comind index <repo> [--enrich] [--flows] [--guide] [--no-embed] [--incremental]` | Index a single repo (embeds by default) |
| `comind link <repos…> [--enrich] [--flows] [--guide] [--no-embed] [--incremental]` | Link several repos (cross-repo edges + blast radius) |
| `comind explore <focus> [--index-dir <dir>]` | Zoom, blast radius, and context pack for a symbol |
| `comind search <query…> [--repo <name>] [--index-dir <dir>] [--format md\|table]` | Graph-aware hybrid code search |
| `comind find <query> [--repo <name>…] [--index-dir <dir>]` | Locate symbols by name/path substring |
| `comind repos [--index-dir <dir>]` | List indexed repositories + symbol counts |
| `comind stats [--repo <name>…] [--index-dir <dir>]` | Per-repo stats: symbols, edges, kinds, enrichment coverage |
| `comind guide [--repo <name>…] [-o <file>] [--index-dir <dir>]` | Per-repo inferred coding style guides (built with `--guide`) |
| `comind flow <focus> [--index-dir <dir>]` | Pre-generated flow walkthrough + live call trace |
| `comind changed <repo> [--since <sha>]` | Files changed since a commit (git diff) |
| `comind serve [--index-dir <dir>] [--format md\|json]` | MCP server over stdio |
| `comind contracts [--type http\|sqs\|sns\|s3\|table] [--dangling]` | Service contracts with providers and consumers (FastAPI, Next.js, Express, Spring MVC, Feign, RestTemplate, WebClient, Go net/http/gin/echo/chi, axum, actix, reqwest, httpx/requests/fetch/axios, boto3, SQL; Terraform queues/topics/buckets and subscriptions) |
| `comind packages [--skew]` | Internal packages: publisher, consumers, version skew |
| `comind conventions [--repo <name>]` | Measured conventions, org baseline, recipes from history |
| `comind preflight [repo] [--base <rev>] [--strict]` | Before committing or in MR CI: constraints, dead ends, failing checks, contracts, blast radius, missing companions ([MR job](../guides/ci.md#merge-request-preflight)) |
| `comind remember <kind> --on <symbol> --title …` | Record a decision, failed attempt, constraint, gotcha, intent or question |
| `comind why <symbol>` | Reasoning blame for a symbol (status, trust, evidence) |
| `comind review <id> <confirm\|retract\|supersede\|dispute>` | Act on a ledger record |
| `comind ledger [--inbox] [--check] [--compact]` | All ledger records, only those needing review, evaluate constraint checks (non-zero exit on a violated `must`, for CI), or fold the inbox into the Lance snapshot |
| `comind erase <id…> \| --actor <id\|me> [--yes]` | Erase ledger records (and relations about them) for good, including old snapshot versions; dry run without `--yes` |
| `comind distill git <repo> \| mr <repo> [--iid N] [--list] [--dry-run]` | Propose ledger records (failed attempts, decisions, constraints, gotchas) from commit messages, or from the merged GitLab MR of the current commit (run per merge in CI, no backfill; `@mentions` removed; `--list` shows what would be sent). Needs `GITLAB_TOKEN` for `mr`. Proposals land at T1 in the review inbox. Agent sessions are never read |
| `comind doctor` | Check this machine's setup: index and ledger reachable, local mirror, hooks, LLM key, GitLab token, signing, SCIP indexers |
| `comind harvest <repos…>` | Seed the ledger from git history (reverts, commit trailers) |
| `comind hook claude <event>` / `comind init-hooks claude` | Claude Code hook adapter / config snippet |
| `comind eval graph [--scip <dir>]` | Graph quality on the gold fixture (precision/recall per edge kind) |
| `comind ui [--web [--port 7878]] [--index-dir <dir>]` | Terminal UI (incl. a ledger timeline), or a local web UI on 127.0.0.1 (overview, contracts, ledger review, explorer with a neighbourhood graph, conventions) |
| `comind config <path\|init>` | Show or scaffold the config file |

Run `comind <command> --help` for every flag. `--index-dir` is a local directory or an `s3://…` path and
defaults to the configured index location.
