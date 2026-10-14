# Comind roadmap — from code index to engineering memory

> Status: **plan, not implemented**. Date: 2026-10-14. Companion to [ledger.md](ledger.md).

Two tracks:

- **Track A — make the existing engine excellent**: a precise cross-repo graph, a service-level
  contract graph, and measured conventions in place of LLM-prose style guides.
- **Track B — the reasoning ledger** ([ledger.md](ledger.md)): why the code is this way, what was
  tried, what must not be done.

Track A comes first: every ledger anchor, `ripple` and `preflight` result is only as good as the
graph underneath it. Ledger core (B1) can run in parallel once fingerprints exist.

---

## 1. Where the current engine falls short (from the code)

### 1.1 Cross-repo graph (`parse.rs`, `resolve.rs`)

| # | Gap | Effect |
|---|---|---|
| G1 | Calls bind only to a **unique same-repo** name (`resolve_call`); imported names are never used to bind calls | Cross-repo edges are only *file → symbol* imports. No cross-repo call graph, so `ripple`/`thread` stop at the repo boundary |
| G2 | Import edges originate at the enclosing scope (usually the **file**), not the function that uses the name | Blast radius is file-granular, noisy |
| G3 | Module path = repo-relative file path | `src/` layouts, package roots, `pyproject` package names, TS `package.json` names/`exports`, tsconfig `paths`, workspaces don't match import strings → silent misses (TS package imports essentially never bind) |
| G4 | No re-export following (`__init__.py`, TS barrels / `export * from`) | Imports via public package facades miss |
| G5 | Collisions take `defs.first()`; ambiguous calls are **dropped** | Arbitrary or lost edges, no confidence signal |
| G6 | Only Python + TS/JS; Python methods typed as `Function`; no `Inherits` / `Implements` / type `References` / module constants | Class hierarchies, models, typed dependencies invisible |
| G7 | No manifests/lockfiles → `version = "."` everywhere | No package nodes, no version skew ("service-a still on pkg-common 1.4") |
| G8 | No service-level links: HTTP routes ↔ clients, topics/queues, DB tables, env/config keys, infra | The real fabric between microservices is missing — the main reason "dots don't connect" |
| G9 | No SCIP ingestion despite the "SCIP-optional" design | No precision tier where teams already emit SCIP |
| G10 | No quality measurement (resolution rate, precision) | We can't see regressions or prove improvements |

### 1.2 Style guide (`styleguide.rs`)

| # | Gap | Effect |
|---|---|---|
| S1 | Excerpts start at **line 1** of files | Evidence is mostly import blocks, not the patterns |
| S2 | `files_matching` takes the **first N files alphabetically** | Unrepresentative samples; a `tests/` or `legacy/` file can define "the convention" |
| S3 | Library evidence = top of the first importing file, not the call-site | "How we use X" is guessed, not shown |
| S4 | Substring heuristics (`"except "`, `"/**"`) | Shallow, language-blind signals |
| S5 | Output is prose; claims aren't measured or checkable | Can't enforce, can't tell confidence, can hallucinate |
| S6 | Per-repo only, regenerated wholesale | No org baseline, no "this repo deviates", costly refresh |
| S7 | No history signal | Misses "how we *add* things here" — the most useful thing for an agent |

---

## 2. Track A — the engine

### A1. Precise resolution (fixes G1–G6, G9)

1. **Package-root discovery** per repo: `pyproject` (`[project].name`, setuptools/hatch/poetry
   package dirs, `src/` layout), `setup.cfg`; `package.json` `name`/`exports`/`main`, tsconfig
   `baseUrl`/`paths`; pnpm/npm/yarn/uv workspaces. Produces file → importable module path.
2. **Per-file binding table**: every import with aliases (`import a as b`, `from x import y as z`,
   TS default/named/namespace imports). Calls `z()`, `b.f()`, `ns.Cls.method()` bind through it —
   **this is what creates cross-repo call edges**.
3. **Light type flow** for method calls: `x = Cls(...)`, parameter/return annotations,
   `self.attr = Cls(...)` in constructors → `x.method()` binds to `Cls.method`.
4. **Re-export chains**: follow `__init__.py` and barrel re-exports to the defining symbol.
5. **Edges from the using symbol** (function/method), not the file. New edge kinds actually
   emitted: `Inherits`, `Implements`, `References` (type annotations), `Instantiates`, `Decorates`.
6. **Ambiguity kept, not dropped**: candidates with confidence; ranking prefers import-proximate
   definitions. Consumers can filter by confidence.
7. **Declarative language packs**: move per-language logic from match arms to tree-sitter query
   files (`queries/<lang>/{defs,refs,imports}.scm`). Target languages: **Python, TypeScript, Java,
   Kotlin, Rust, Go** (decided). Package identity per ecosystem: `pyproject`/`package.json`,
   Maven/Gradle coordinates + Java/Kotlin package FQNs, `Cargo.toml` crate names + `mod` tree,
   `go.mod` module path (Go import paths map exactly). All six have SCIP indexers (scip-python,
   scip-typescript, scip-java incl. Kotlin, rust-analyzer, scip-go) for the precision tier.
8. **SCIP ingestion**: `comind index --scip index.scip` replaces syntactic edges with compiler-grade
   ones (scip-python, scip-typescript, rust-analyzer, scip-java, scip-go). Same `GlobalSymbolId`.

### A2. Contract graph — connecting services, not just imports (fixes G8)

Extract boundary endpoints with framework query packs and normalize them into **contract nodes**:

| Contract | Providers | Consumers |
|---|---|---|
| `http:POST /v1/skills/{id}` | FastAPI/Flask/Django/Express/NestJS routes, OpenAPI specs | httpx/requests/fetch/axios calls + generated clients |
| `grpc:SkillService/Get` | `.proto` services, server impls | stubs |
| `topic:skill-updated` | Kafka/SQS/SNS/RabbitMQ/Celery publishers | subscribers/consumers |
| `table:employees` | ORM models (`__tablename__`), migrations | queries, raw SQL |
| `env:SKILLS_API_URL` / config key | Helm values, compose, Terraform, `.env.example` | `os.getenv`, settings classes |

- Edges: `Provides` / `Consumes` / `Publishes` / `Subscribes` / `Reads` / `Writes`, with confidence.
- Producers and consumers match **across repos** by normalized key (path templates normalized,
  base-URL env indirection resolved through config/infra files where possible).
- `ripple` traverses through contracts: change a route handler → see the frontend call in another
  repo that breaks. **Dangling consumers** (calls to routes nobody provides) and **dead providers**
  become a report of their own.
- Start with the frameworks the team actually uses; packs are additive.

#### A2.1 First packs: FastAPI, httpx, SQLAlchemy, boto3 (SQS, SNS, S3)

No existing library does this for Python/AWS (prior tools target Java, .NET or Spring+TS:
MicroGraal, Code2DFD, EasyFinder, Ariadne). We build on:

| Building block | Role |
|---|---|
| `ast-grep-core` + `ast-grep-config` (Rust, tree-sitter based) | Rule engine: framework packs are **YAML rules**, contributable without Rust. Check tree-sitter version compatibility with ours |
| `sqlparser` (sqlparser-rs) | Tables read/written in raw SQL (`text("…")`, Alembic `op.execute`) |
| `hcl-rs`, `serde_yaml` | Terraform, Helm, docker-compose, CloudFormation/serverless — the infra glue |
| `openapiv3` | Committed OpenAPI specs as an extra provider source |

| Pack | Providers | Consumers |
|---|---|---|
| FastAPI | `@app/@router.<method>(path)`, `APIRouter(prefix=…)` composed with `include_router(…, prefix=…)` across files; request/response pydantic models attached | — |
| httpx | — | `httpx.<method>(url)`, `client.<method>(url)`, `AsyncClient(base_url=…)`; f-strings normalized to path templates |
| SQLAlchemy | `__tablename__`, `Table("x", …)`, Alembic `op.create_table` | `select/insert/update/delete(Model)`, `session.query/add/merge`, raw SQL via sqlparser |
| SQS | Terraform `aws_sqs_queue` | `send_message[_batch]` (publish), `receive_message` (consume) |
| SNS | `aws_sns_topic`; `aws_sns_topic_subscription` links topic → queue/lambda | `publish(TopicArn=…)` |
| S3 | `aws_s3_bucket`, bucket notifications | `put_object`/`upload_file*` (write), `get_object`/`download_file*` (read), `Bucket(name)` |

Three mechanisms make this work on real code:

1. **Value resolution chain**: literal → module constant → settings field → env var → infra value
   (Helm/compose/Terraform). Unresolved values stay symbolic (`sqs:{env:EVENTS_QUEUE_URL}`) and still
   match across repos on the env var name.
2. **Wrapper propagation through the call graph**: if `pkg-common.events.publish(queue, msg)` calls
   `sqs.send_message(QueueUrl=queue)`, the wrapper is marked a producer summary and each caller with a
   resolvable `queue` argument becomes a producer. Internal wrappers are covered without hand-written
   rules; `comind.toml` can declare wrappers inference misses.
3. **Service identity**: repo ↔ service name from compose/Helm/Dockerfile (overridable in config),
   so `SKILLS_API_URL=http://skills-service` maps an httpx consumer to the FastAPI repo.

Bonus once request/response models are linked: **breaking-change detection** — removing a field
from a pydantic response model flags the consumers that read it.

#### A2.2 Contract packs for the other languages (after A2.1)

| Language | HTTP server | HTTP client | DB | AWS |
|---|---|---|---|---|
| TypeScript | Express, NestJS | fetch, axios | Prisma, TypeORM | AWS SDK v3 |
| Java / Kotlin | Spring MVC/WebFlux, Ktor | RestTemplate, WebClient, OkHttp, Feign | JPA/Hibernate, jOOQ, Exposed | AWS SDK v2 |
| Go | net/http, chi, gin, echo | net/http client | database/sql, sqlx, gorm | aws-sdk-go-v2 |
| Rust | axum, actix-web | reqwest | sqlx, diesel, sea-orm | aws-sdk-rust |

Same contract keys across languages, so a Go consumer matches a FastAPI provider.

Later: optional **observed edges** from OpenTelemetry / APM traces (e.g. Datadog service map) to
confirm static contract edges and discover ones static analysis missed.

### A3. Packages and versions (fixes G7)

- Parse manifests and lockfiles (`pyproject`/`uv.lock`/`poetry.lock`/`requirements*.txt`,
  `package.json`/`pnpm-lock.yaml`/`package-lock.json`) → `Package` nodes, `DependsOn{version}` edges.
- Internal packages link to the repo that publishes them. `ripple` labels each dependent with the
  version it is on: "breaks now" vs "breaks on upgrade". Version-skew report per internal package.
- Also feeds ledger dependency anchors ("retry when lance > 1.0.1").

### A4. History signals

- **Co-change mining** (logical coupling): symbols/files that change together — reveals coupling
  static analysis misses, including across repos when linked by MR/ticket ids in commits.
- **Recipes**: from commits that *added* a symbol of a given role (new route, new model, new
  consumer), mine the set of files touched together → "to add an endpoint here you touch
  `api/`, `schemas/`, `services/`, `tests/api/` — exemplar commit `abc123`".
- Churn and recency feed ranking. Ownership only via `CODEOWNERS` (team level), never per person.

### A5. Measure everything (fixes G10)

- **Fixture org** in `tests/fixtures/org/`: several small repos covering `src/` layout, aliases,
  re-exports, TS packages, HTTP/topic contracts, version skew — every gap above as a test.
- `comind eval graph`: precision/recall of tree-sitter edges against SCIP where available.
- `comind stats` reports resolution rate and unresolved-by-reason (third-party, ambiguous,
  unknown module) per repo.

### A6. Conventions engine — replaces the style guide (fixes S1–S7)

Mine conventions **deterministically**, let the LLM only phrase them.

```text
Convention {
  id, dimension, scope: org | repo | dir,
  rule:          "Loggers come from pkg-common `get_logger(__name__)`"
  support:       47 / 50 modules (94%)
  exemplars:     3 best instances (central, recent, tested), with file:line
  counterexamples: the 3 deviations, with file:line
  check?:        tree-sitter / ast-grep query or graph rule (→ enforceable)
}
```

Miners (AST + graph, not substrings):

| Dimension | How |
|---|---|
| Layout & roles | Classify symbols by role (route, service, repository, model, schema, test, CLI) from decorators, base classes, imports; measure where each role lives |
| Naming | Per kind *and* per role (existing casing logic, extended) |
| Signatures | Typed-param %, return annotations, kw-only, async share — per role |
| Data modeling | `Inherits` edges to BaseModel / dataclass / TypedDict / zod; where models live |
| Errors | Custom exception hierarchy via `Inherits`; broad-except rate; raise/catch sites |
| Logging & config | Call edges to logger factories and settings objects vs raw `os.getenv` |
| Internal-library preference | Wrapper vs raw (e.g. 40 call-sites to the internal HTTP client vs 3 to `requests`) → "prefer the wrapper", with the 3 deviations |
| Testing | Frameworks, fixtures, mock libraries from test-file imports; test location mapping |
| Recipes | From A4 co-change mining |

- **Exemplar selection** by centrality × recency × test coverage, with test/legacy/generated paths
  penalized (reuse `path_penalty`), and **real call-site excerpts** rather than file heads.
- **Org baseline**: aggregate across repos; each repo's guide shows the baseline once plus only its
  deviations and specifics.
- **LLM phrasing with citations**: every sentence must cite a convention id; a validator drops
  uncited claims.
- **Scoped delivery**: MCP `guide` returns the conventions relevant to the file/dir being edited
  (≤ ~800 tokens), not the whole document. The full guide stays available.
- **Incremental**: miners run per changed file; prose regenerates only for dimensions whose stats
  moved.
- **Enforceable**: conventions with a `check` become T4 constraints in the ledger and are evaluated
  by `preflight`.

---

## 3. Track B — the ledger

As specified in [ledger.md](ledger.md): typed, symbol-anchored, cross-repo records in the user's
object store; trust tiers; structural staleness; hooks; `why` / `remember` / `preflight`; the
built-in memory-on/off evaluation.

Track A feeds it directly: precise anchors (A1), contract-level constraints (A2), dependency
anchors (A3), harvested reverts and recipes (A4), and enforceable conventions (A6).

---

## 4. Milestones

| M | Scope | Exit criteria |
|---|---|---|
| **M0 Foundations** ✅ | rmcp 3.5 (MCP 2024-11-05 … 2026-07-28), lancedb 0.37.1 (0.38–0.40 don't build without `remote`), tree-sitter 0.27; fixture org + `comind eval graph` + ratchet test; resolution stats; symbol fingerprints. Declarative query packs moved to M1 (they are rewritten with the resolver anyway) | Baseline: precision 56%, recall 18%, **0 cross-repo edges** on the fixture org |
| **M1 Precise graph** ✅ | Python, TS/JS, Go, Java, Kotlin, Rust via `LangSpec` adapters (trait, not `.scm` packs: type flow needs scope logic); package roots, tsconfig paths, re-exports, type + return-type flow, inherits/implements/instantiates, module vars, JSX, closures; by-name fallback removed (precision); facts persisted → exact incremental; SCIP ingestion on by default | Fixture org 83/83 edges (100% P/R) across 6 languages; real Python org: imports 3.3k→11.6k, cross-repo edges 1.2k→5.6k; prism (TS) imports 0→3.4k; validated on gson, moshi, cobra, completr |
| **M2 Contracts + versions** 🟡 mostly done: Python + TS contracts (FastAPI, Next.js routes; httpx/requests/fetch/axios with wrapper propagation; base-URL suffix matching; boto3 SQS/SNS/S3; SQLAlchemy, pypika, raw SQL), values via constants/settings/env/`Path`/records, `comind contracts` + MCP `contracts`/zoom roles, ripple across contracts; packages + version skew (`comind packages`, MCP `repos`). Open: Go/JVM/Rust framework packs, Terraform/Helm values, AWS messaging through wrappers (param-flow summaries) | Fixture 120/120; real org: 29 service↔service + 30 frontend→backend HTTP links, `skill` table shared by 6 services, version skew on shared-lib/odin surfaced |
| **M3 Conventions v2** ✅ deterministic miners (naming, signatures, model bases, error roots, layout by role, logging provider, settings vs env, internal-wrapper preference, testing) + recipes from commit history; central exemplars + deviations; org baseline; `comind conventions`; MCP `guide` scoped by focus; LLM style guide grounded in measured conventions + exemplar excerpts. Enforceable checks move to M5 | Real org: `CobrainerError` root in 4 repos, `cobrainer.logging` 9/9, router recipe in skill-graph (controllers 100%, queries/tests 90%) |
| **M4 Ledger core + agent loop** 🟡 core done: typed records (intent/decision/attempt/constraint/gotcha/question + relations), symbol anchors with fingerprints, create-only inbox in the user's store (local or S3), derived status (active/moved/stale/orphaned/superseded/retracted) and trust (T0–T3), secret/bidi guards, CLI `remember`/`why`/`review`/`ledger --inbox`, MCP `why`/`remember`/`review`, Claude Code hooks (session-start digest, pre-edit just-in-time injection, per-session dedup; `comind init-hooks claude`). Open: Lance compaction, distiller, eval harness, zoom/context_pack ledger sections, daemon for <100 ms hooks | Hook ≈0.2 s (debug); agent records land at T1/T2, humans at T3 |
| **M5 Staleness, harvest, preflight** 🟡 structural staleness (fingerprints: stale / moved / orphaned), harvest from git (git + manual reverts → attempts, trailers → records), `preflight` CLI (working tree) + MCP (planned files/symbols): constraints & dead ends, contracts + consumers, blast radius, missing companions from recipes; zoom/context_pack carry known constraints. Open: compiled constraint checks, CI/MR bot, LLM distiller | Preflight on a real route shows constraint, failed attempt, cross-repo consumer and recipe gaps |
| **M6 TUI** 🟡 `comind ui` (ratatui): Overview (repos, contract linkage, ledger health, version skew), Contracts (providers/consumers), Ledger (status/trust, detail), Explorer (find → neighbourhood, blast radius, contract roles, recorded reasoning), Conventions (per repo + org baseline + recipes). Open: review actions from the inbox (confirm/retract), timeline view, terminal graph rendering | Render test over the fixture; snapshot over the real 9-repo index |
| **M7 Web UI** | `comind ui --web`: org map with contract edges, ripple lens, time slider | 50k+ nodes smooth |
| **M8 Launch hardening** | Signing, poisoning red-team, GDPR erasure path, docs | Ledger release gate passes |

---

## 5. Decisions needed

1. ~~Framework packs first~~ — decided: FastAPI, httpx, SQLAlchemy, boto3 SQS/SNS/S3 (§A2.1).
2. ~~Languages~~ — decided: Python, TypeScript, Java, Kotlin, Rust, Go (§A1.7, §A2.2).
3. ~~SCIP~~ — decided: **on by default** (`scip = "auto"`), first-class in M1.
   - `comind index` detects each repo's languages and runs the matching SCIP indexer; its exact
     resolution overrides tree-sitter's for that repo. Tree-sitter still provides structure
     (bodies, arguments, contracts, fingerprints) and is the fallback when an indexer is missing or
     the repo doesn't build — with a visible warning and per-repo "resolution: scip | tree-sitter"
     in `comind stats`.
   - `scip = "require"` makes CI fail instead of falling back; `scip = "off"` for quick local runs.
   - `comind scip install` fetches pinned indexer versions into the comind cache; a published CI
     image ships all five preinstalled (scip-python, scip-typescript, scip-java, rust-analyzer,
     scip-go). `docs/CI.md` and `comind-index.yml` install deps before indexing.
   - Live/local paths (agent hooks, uncommitted edits) stay tree-sitter for speed and overlay the
     last CI SCIP results for unchanged files.
   - SCIP output doubles as the gold set for `eval graph`.
4. ~~Track ordering~~ — decided: Track A first, ledger core in parallel after M0.
