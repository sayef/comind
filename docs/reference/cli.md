# Command line

| Command | Purpose |
|---|---|
| `comindly index <repo> [--enrich] [--flows] [--guide] [--no-embed] [--incremental]` | Index a single repo (embeds by default) |
| `comindly link <repos…> [--enrich] [--flows] [--guide] [--no-embed] [--incremental]` | Link several repos (cross-repo edges + blast radius) |
| `comindly explore <focus> [--index-dir <dir>]` | Zoom, blast radius, and context pack for a symbol |
| `comindly search <query…> [--repo <name>] [--index-dir <dir>] [--format md\|table]` | Graph-aware hybrid code search |
| `comindly find <query> [--repo <name>…] [--index-dir <dir>]` | Locate symbols by name/path substring |
| `comindly repos [--index-dir <dir>]` | List indexed repositories + symbol counts |
| `comindly stats [--repo <name>…] [--index-dir <dir>]` | Per-repo stats: symbols, edges, kinds, enrichment coverage |
| `comindly guide [--repo <name>…] [-o <file>] [--index-dir <dir>]` | Per-repo inferred coding style guides (built with `--guide`) |
| `comindly flow <focus> [--index-dir <dir>]` | Pre-generated flow walkthrough + live call trace |
| `comindly changed <repo> [--since <sha>]` | Files changed since a commit (git diff) |
| `comindly serve [--index-dir <dir>] [--format md\|json]` | MCP server over stdio |
| `comindly contracts [--type http\|sqs\|sns\|s3\|table] [--dangling]` | Service contracts with providers and consumers (FastAPI, Next.js, Express, Spring MVC, Feign, RestTemplate, WebClient, Go net/http/gin/echo/chi, axum, actix, reqwest, httpx/requests/fetch/axios, boto3, SQL; Terraform queues/topics/buckets and subscriptions) |
| `comindly packages [--skew]` | Internal packages: publisher, consumers, version skew |
| `comindly conventions [--repo <name>]` | Measured conventions, org baseline, recipes from history, and the team's own (each with an id) |
| `comindly conventions add "<rule>" [--repo <name>] [--dimension <d>] \| edit <id> "<rule>" \| rm <id> \| restore <id>` | Add a team convention, rewrite or hide one (measured ones can be restored); stored under `<index>/_conventions/`, untouched by re-indexing |
| `comindly preflight [repo] [--base <rev>] [--strict]` | Before committing or in MR CI: constraints, dead ends, failing checks, contracts, blast radius, missing companions ([MR job](../guides/ci.md#merge-request-preflight)) |
| `comindly remember <kind> --on <symbol> --title …` | Record a decision, failed attempt, constraint, gotcha, intent or question |
| `comindly why <symbol>` | Reasoning blame for a symbol (status, trust, evidence) |
| `comindly review <id> <confirm\|archive\|invalidate\|supersede\|dispute> [--reason "…"]` | Act on a ledger record; `comindly erase <id>` deletes one |
| `comindly ledger [--inbox] [--check] [--compact]` | All ledger records, only those needing review, evaluate constraint checks (non-zero exit on a violated `must`, for CI), or fold the inbox into the Lance snapshot |
| `comindly erase <id…> \| --actor <id\|me> [--yes]` | Erase ledger records (and relations about them) for good, including old snapshot versions; dry run without `--yes` |
| `comindly doctor` | Check this machine's setup: index and ledger reachable, local mirror, hooks, LLM key, GitLab token, signing, SCIP indexers |
| `comindly hook claude <event>` / `comindly init-hooks claude` | Claude Code hook adapter / config snippet |
| `comindly eval graph [--scip <dir>]` | Graph quality on the gold fixture (precision/recall per edge kind) |
| `comindly repos [add <folders…> [--branch b] \| remove <names…> \| scan <dir>]` | Indexed repositories, and the list `comindly link` indexes when given no folders |
| `comindly app [--index-dir <dir>]` | The UI in its own window (macOS) |
| `comindly team [set k=v… \| key]` | Team settings stored with the index (LLM, embedding model, LLM steps); `key` stores your own API key |
| `comindly join <index> [--profile p]` | Use a team's index on this machine |
| `comindly ui [--web [--port 7878] [--open]] [--index-dir <dir>]` | Terminal UI (incl. a ledger timeline), or a local web UI on 127.0.0.1 (overview, contracts, ledger review, explorer with a neighbourhood graph, conventions) |
| `comindly config <path\|init>` | Show or scaffold the config file |

Run `comindly <command> --help` for every flag. `--index-dir` is a local directory or an `s3://…` path and
defaults to the configured index location.
