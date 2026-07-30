# Comind — Rust Architecture

> Polyglot, cross-repo code-intelligence engine. CLI + MCP server, single static binary.
> Read-heavy / write-rare: the index is a **versioned artifact in S3**, not a live shared DB.

## Decisions (locked)

| Question | Decision |
|---|---|
| Language scope | **Polyglot from day 1** via tree-sitter grammars. Infra langs low priority. |
| Rewrite | **Full Rust rewrite.** LLM enrichment stays, via Rust HTTP (OpenAI/Anthropic). |
| Cross-repo graph | **Core launch requirement** — federated symbol identity from day 1. |
| Embeddings | **Local model in CI** (fastembed/ONNX). No source leaves org infra. |
| S3 | Assumed reachable (AWS SSO locally, IAM in CI). |

## The core reframe

The index only changes on push-to-master. So we do **not** run a concurrent
read/write database. Instead:

```
                    ┌──────────────────────────── CI on push to master ─────────────────────────┐
  repo A push ─────▶│ comind index → Lance tables {symbols, edges, embeddings, chunks}          │
                    │              → write to  s3://comind/<repo>/<commit>/                       │
                    │ link-resolver: merge stack-graphs across ALL repos → cross-repo edges       │
                    │              → s3://comind/_graph/<ts>/  → atomically bump "latest" pointer  │
                    └───────────────────────────────────────────────────────────────────────────┘
                                                     │
   each dev's MCP server / CLI ──▶ watch "latest" ──▶ pull delta ──▶ build in-mem petgraph ──▶ serve locally
```

Lance gives atomic versioning + cheap deltas for free. Consumers are stateless
read-replicas. This dissolves DuckDB's single-writer pain and the shared-DB problem.

## Retrieval stance (research-informed — see landscape below)

The frontier is **de-emphasizing vector search for agents** (Anthropic removed it from
Claude Code; Cody replaced embeddings with SCIP). So our backbone is **deterministic**,
not fuzzy. Vectors are an *optional fallback* for NL "where is the code that does X",
never the primary path.

Retrieval modes, in priority order:
1. **Exact** — symbol/FTS lookup (deterministic, like grep/SCIP).
2. **Structural** — graph traversal: `ripple` (blast radius), `thread` (exec trace), `zoom`.
3. **Ranked** — personalized PageRank over the symbol graph (Aider-style) to pack the
   **minimal token-budgeted context**, seeded by the agent's current focus.
4. **Semantic** — vector similarity, *optional*, only when 1–3 miss.

## Storage split (do NOT put the graph in a vector DB)

| Concern | Store |
|---|---|
| Nodes + edges + FTS payload (the backbone) | **Lance/Parquet tables in S3, versioned** |
| Multi-hop traversal (`ripple`, `thread`, `zoom`) + PageRank ranking | **In-memory `petgraph`**, built from edge tables |
| Embeddings + code chunks (*optional* semantic fallback) | **Lance dataset in S3, versioned** |

Lance earns its place as the **versioned, S3-backed columnar artifact store** — that is
its real value here, independent of whether we lean on embeddings. The whole org symbol
graph (40+ repos) is low-millions of edges — fits in RAM; BFS blast-radius is microseconds.
No graph database needed. (KuzuDB is the fallback only if the in-mem graph stops fitting.)

## Federated symbol identity (the differentiator)

Global symbol ID follows the **SCIP** scheme so identity is stable and unique across repos:

```
<scheme> <package-manager> <package-name> <version> <descriptor>
e.g.  scip-python  pip  acme  1.4.0  `acme/foo`/bar().
```

- Each repo indexes locally → emits symbols + intra-repo edges + unresolved references.
- The **link-resolver** binds cross-repo references by SCIP id: an unresolved reference in
  `service-a` (`from acme.foo import bar`) binds to the *definition* symbol in
  `pkg-common`, producing a **cross-repo edge** (`cross_repo = true`).
- `ripple(pkg-common::X)` then answers *"who across the org breaks if I change this?"*
  — the query no per-repo tool gives you.

**Resolver strategy — tree-sitter-first, SCIP-optional:**
- **Default:** tree-sitter `tags.scm` extraction → fast, universal, *syntactic* symbol/ref
  graph for every language. Single binary, no per-language toolchain. Good enough for
  ranking + most navigation.
- **Precision upgrade:** ingest **SCIP indexes** where a team already emits them
  (`scip-typescript`, `scip-python`, `rust-analyzer`, `scip-java`) for *semantic* accuracy.
- **Note:** GitHub's `stack-graphs` (which I initially proposed) was **archived Sept 2025**.
  It remains usable as an optional resolver but is unmaintained — SCIP is the safe backbone.

## Module map (single crate)

```
src/
  model.rs    domain model: GlobalSymbolId (SCIP), Symbol, Edge, Language, ranges. No heavy deps.
  parse.rs    tree-sitter: file → symbols + intra-file edges, per-language extractors.
  resolve.rs  cross-repo link-resolver (SCIP id binding).
  index.rs    Lance/object_store writers + readers; versioning; incremental change detection.
  graph.rs    petgraph load + traversal: ripple (blast radius), thread (exec trace), zoom.
  embed.rs    Model2Vec local embeddings + code-aware ranking (embed/rank.rs).
  llm.rs      wiki / style-guide / query-association generation via OpenAI.
  mcp.rs      rmcp server exposing find/zoom/ripple/thread/context_pack/guide.
  git.rs      git2 change detection for incremental indexing.
  main.rs     clap binary: `comind index|serve|resolve|query`. The single distributable.
```

One crate, one module per stage; `main.rs` uses the library as `comind::<module>`. (Earlier phases
below were built as a `cargo` workspace of `comind-*` crates and later consolidated into this single crate.)

MCP verb semantics are ported 1:1 from the Python app — that design is proven; keep it.

## Phases

- **P0 — Foundation:** workspace, `model` types (SCIP ids, Symbol/Edge), CI skeleton. ← *done*
- **P1 — Parse:** tree-sitter symbol/edge extraction, rayon-parallel. ← *done for Python + TypeScript*
  (`comind index <repo>` prints graph stats). Validated on `pkg-common` (2843 symbols,
  11361 edges, 0.33s debug) and `service-a`. **Known limitation:** Python methods are currently
  tagged `Function` (nested descriptor is correct, e.g. `.../ServiceCredentials/get_password().`);
  refine kind by class ancestry as a small follow-up. Provisional `Calls` edges (conf 0.4) await P3 resolution.
- **P2 — Index:** Lance writers, S3 layout, versioned "latest" pointer, incremental change detection.
  ← *round-trip done*. `comind index <repo> --to <uri>` writes `symbols.lance` + `edges.lance`
  and reads them back. Validated on **real S3** (`s3://YOUR-BUCKET/lancedb/pkg-common`,
  profile `your-profile`) — 2843 symbols / 11361 edges; Lance `_versions/*.manifest` confirmed in
  the bucket (the substrate for the "latest" pointer). Still TODO: incremental change detection,
  the optional embeddings table.
  **Latest pointer — done:** writes use Lance overwrite (new version, prior versions retained),
  so the newest version manifest *is* the atomic latest pointer and `checkout(v)` pins/rolls back.
  `comind link <repo>... --to <uri>` persists the resolved org graph to `<uri>/_graph`; validated
  on S3 (8321 symbols / 16724 edges; re-index advanced v1→v2, both manifests retained).
  Still TODO: incremental change detection (only re-parse changed files) and the embeddings table.
  **Toolchain notes:** requires Rust ≥1.94 (AWS SDK) via `rust-toolchain.toml` (pinned 1.97.1);
  `protoc` must be installed (`brew install protobuf`); lancedb **0.31.0** (lance 8.0.0) — do NOT
  use lancedb 0.23/lance 1.0.1, which trips the rustc ≥1.94 async-layout `recursion_limit`
  regression. Build Arrow types from `lancedb::arrow::*` re-exports, never a direct arrow dep.
- **P3 — Resolve:** intra-repo call binding + cross-repo link-resolver → federated edges.
  ← *done (import-based cross-repo)*. `parse` now extracts imports; `resolve`
  binds them to real definitions by SCIP descriptor-core. `comind link <repo>...` proved it on
  5 real repos: **757 cross-repo edges**; `ripple(acme/logging)` → impacts service-a,
  service-d, service-b, service-c (120 refs). **Caveat:** generic descriptor cores (e.g.
  `tests/utils`) can collide across repos and bind to the first match — refine by preferring
  package-qualified/import-path matches, and resolve calls cross-repo via imports (currently
  same-repo only). SCIP-index ingestion is the precision upgrade path.
- **P4 — Graph + Query:** petgraph traversal (ripple/thread/zoom) + ranked context packs.
  ← *traversal + packs done*. `graph::CodeGraph` builds an in-memory petgraph and serves
  `ripple` (reverse Calls/Imports reachability = blast radius), `thread` (forward call trace),
  `zoom` (container/members/callers/callees/importers), and `context_pack` (personalized
  PageRank over *dependency* edges only, files excluded, greedily packed to a token budget).
  `comind explore <focus> <repo>...` drives all three. Verified on 5 repos (8224 symbols):
  `ripple(Settings)` → 74 dependents across service-a/service-d/service-b; context pack =
  definition read-set to ~1489/1500 tokens with `file:line`. TODO: load from the persisted
  Lance graph instead of re-parsing (needed for `serve`), FTS/vector as optional retrieval modes.
- **P5 — Serve:** rmcp MCP server + CLI, single binary; parity with Python MCP verbs.
  ← *done*. `index::read_graph` loads the persisted org graph from Lance (local/S3) back
  into core types (fast consumer path — no re-parse; verified from S3). `mcp` (rmcp 2.2)
  serves 6 tools over stdio: `repos`, `find`, `zoom`, `ripple`, `thread`, `context_pack`. CLI:
  `comind serve --from <uri>` and `comind explore <focus> --from <uri>`. Smoke-tested with a real
  JSON-RPC handshake: tools/list advertises all 6; `ripple(Settings)` → 74 dependents grouped
  by repo. Note: MCP tool outputs must be object-rooted (list results wrapped in a DTO).
- **P6 — Enrich:** local embeddings + hybrid search + LLM enrichment — **done**.
  ← *embed + search done*. `embed` uses **Model2Vec** (`model2vec-rs`, pure-Rust, CPU,
  no ONNX) — the approach validated by **semble**. `comind search <query> --from <uri>` does
  semantic search **reranked by graph centrality + definition-boost + test-file penalty**
  (semble's fusion ideas + our unique dependency-graph signal). Verified: "postgres database
  connection executor" → the two most-depended-on executor classes rank top.
  Ranking (`embed::rank`) re-implements semble's ideas (studied from its GitHub source,
  not copied): adaptive fusion weight α (symbol query 0.3, NL 0.5), `is_symbol_query` detection,
  camelCase/snake_case identifier splitting → lexical-overlap signal, exact-name/definition boost,
  structured path penalties (test/examples/legacy ×0.3, `__init__`/barrels ×0.5, `.d.ts` ×0.7).
  **Our edge:** dependency-graph centrality (`dependents_count`) as an extra multiplier — impossible
  without the cross-repo graph. Demo: symbol query → exact class #1 (4.54 vs 1.18); NL query → the
  connection-relevant defs, tests penalized out.
  **Embeddings persisted (done):** `index::write/read_embeddings` stores a Lance
  `embeddings` table (`symbol_id`, `vector: FixedSizeList<Float32>[dim]` — the column a Lance
  vector index can be built on). `comind link --embed` writes vectors next to the graph;
  `comind search` loads them and only embeds the query → 0.62s (was ~10.8s) for 7471 vectors.
  TODO: real BM25 + RRF (k≈60) lexical retriever + file-saturation decay (×0.5/extra chunk);
  build a Lance ANN index on the vector column for >100k-symbol corpora.
  **LLM enrichment (done):** `llm` (OpenAI via async-openai, `gpt-4o-mini` default,
  bounded concurrency) generates per-symbol summaries + NL queries and infers a repo style guide.
  **Opt-in / data egress:** only runs with `comind link --enrich` (sends code signatures to
  OpenAI); never during plain indexing. Summaries+queries persisted to a Lance `enrichment` table
  (`symbol_id`, `summary`, `queries` JSON). Verified live end-to-end on the 5 repos.
  Full CI payload: `comind link <repos> --to s3://… --embed --enrich` → graph + cross-repo edges +
  embeddings + summaries + generated queries + style guide, one versioned artifact.
  **Enrichment surfaced (done):** `read_enrichment` loads summaries+queries; `search` folds the
  summary into lexical matching, boosts recall when the user query overlaps a symbol's LLM-generated
  queries, and prints the summary; the MCP `zoom`/`find`/`context_pack` results carry `summary`.
  Verified live (search + MCP `zoom` both return the `to_jsonable` summary).
  **CI (done):** [`.github/workflows/comind-index.yml`](.github/workflows/comind-index.yml) +
  [`docs/CI.md`](docs/CI.md) (GitLab variant, member-repo push triggers, OIDC/S3, opt-in enrich).
  TODO: persist the style guide to its own table.
- **P7 — Incremental + OSS.**
  **Incremental (done):** `git` (git2, vendored libgit2 → still a single static binary)
  detects changed files between the last-indexed commit and HEAD. `comind changed <repo> --since
  <sha>` reports add/modify/delete; `comind index <repo> --to <uri> --incremental [--since <sha>]`
  reparses only changed files, drops symbols/edges of modified+deleted files, merges, rewrites,
  and records HEAD in a Lance `repo_meta` table. Verified: full=2050 symbols; incremental from
  HEAD~3 reparsed 8 files → identical 2050 (consistent with full reindex); no-change run no-ops.
  **Remote auth is a CI concern, not comind's:** the CI job clones the repo (with its token), then
  comind indexes the local checkout + `.git` — comind never needs remote credentials.
  **Org-level incremental (done):** `comind link --incremental` stores per-repo HEADs (multi-row
  `repo_meta`), and on re-run computes stale symbols from each repo's git `changed_files` since its
  recorded commit. `--embed`/`--enrich` then **reuse prior vectors/summaries for non-stale symbols**
  and only recompute stale/new ones. Verified: full = 7471 embeds + 5 LLM calls; no-change
  incremental = 0 stale → 7471 reused/0 computed, 5 reused/0 API calls. So a push that touches one
  repo re-embeds/re-enriches only that repo's changed symbols. **CI done** (see above).
  **Style guide + `guide` tool (done):** `index::write/read_style_guide` persists the
  inferred guide (single-row table); the MCP server exposes it as a 7th tool `guide` → full
  verb-parity with the Python app (repos/find/zoom/ripple/thread/context_pack/guide) plus `search`.
  Verified live. README rewritten for the Rust project.
  **Native hybrid search (done):** switched from a hand-rolled lexical score to **LanceDB's own
  BM25 FTS + vector + RRF fusion**. `write_search_table` builds a denormalized `search` table
  (`symbol_id`, `text`, `vector`) with a native FTS (tantivy/BM25) index; `hybrid_search` runs
  `full_text_search + nearest_to + execute_hybrid` (RRF k=60 default). We apply our code-aware
  boosts + dependency-graph centrality on top of Lance's fused score. A vector ANN index
  (`Index::Auto`, IVF/PQ) is added automatically for corpora ≥10k symbols (flat/exact below).
  Incremental reuse reads prior vectors from the search table. Verified: NL query fuses BM25+vector
  (connect/postgres/config); symbol query → exact class #1; incremental 0-stale → all reused.
  **Cross-platform testing (done):** [`.github/workflows/test-matrix.yml`](.github/workflows/test-matrix.yml)
  builds a portable glibc binary once, then smoke-tests it (`--help` + `index` on a tiny repo) across
  distro images (ubuntu 20.04/22.04, debian 12, rockylinux 9, alpine musl `continue-on-error`) via
  `docker run` (avoids the Actions-in-Alpine-container limitation), plus native macOS (arm64 + x64).
  CLI now handles `--help`/`-V`. Windows deferred (aws-lc/libgit2 on MSVC needs nasm/protoc/cmake).
  **Release/distribution (done):** [`.github/workflows/release.yml`](.github/workflows/release.yml)
  cross-builds `comind-<target>.tar.gz` (+ sha256) for macOS arm64/x64 and Linux x64/arm64 on each
  `vX.Y.Z` tag → GitHub Releases; [`scripts/install.sh`](scripts/install.sh) is the `curl | sh`
  installer (OS/arch detect, private-repo `GITHUB_TOKEN`). Release build verified locally (exit 0).
  TODO (once repo is public): crates.io publish for `cargo install`; Homebrew tap.

The Python `app/` stays as the reference spec until P5 reaches verb parity, then is retired.

## Competitive landscape & why we're superior

Mid-2026 the field split three ways:

| Camp | Who | Strength | Weakness we exploit |
|---|---|---|---|
| Agentic grep / JIT | Claude Code, Cody (post-pivot) | Always fresh, simple | No cross-file/cross-repo structure, no blast radius |
| Semantic / vector | Cursor, Augment | Fuzzy recall | **Staleness**, noise; frontier de-emphasizing it |
| Deterministic graph | GitHub stack-graphs, Meta Glean, Sourcegraph SCIP | Precise, cross-repo | Heavy per-lang indexers, SaaS/enterprise, not agent-native |

Key signals: Anthropic removed vector search from Claude Code ("outperformed everything, by
a lot"); Cody replaced embeddings with SCIP; `stack-graphs` archived Sept 2025; Aider's
personalized-PageRank repomap sets the bar for token-efficient context; the winning metric
is **task-success-per-token** ("agents burn 10x the tokens they need").

**Our five differentiators:**
1. **Deterministic cross-repo graph** (SCIP identity) — precise like Glean, but self-hosted single binary.
2. **Zero-staleness team index** — push-to-master → versioned S3 → everyone shares one fresh index. Solves the problem that killed embeddings, at *team* scale.
3. **Cross-repo `ripple`** — org-wide blast radius; grep and per-repo tools can't answer it.
4. **Ranked, token-budgeted context packs over MCP** — Aider's insight, org-wide, delivered as *the minimal correct context to change safely*, not a search dump.
5. **Tree-sitter-first, SCIP-optional** — universal + fast by default, precise where it counts.

One-liner: *Deterministic, always-fresh, cross-repo code intelligence that hands your agent
the minimal correct context — self-hosted, single binary.*

Sources: [Sourcegraph — Agentic Coding in 2026](https://sourcegraph.com/blog/agentic-coding) ·
[AI agents don't need vector search anymore](https://buzzgrewal.medium.com/ai-agents-dont-need-vector-search-anymore-inside-the-agentic-search-stack-replacing-rag-in-2026-58efcabe4f6f) ·
[Code Intelligence Tools Compared](https://rywalker.com/research/code-intelligence-tools) ·
[Aider repomap](https://aider.chat/docs/repomap.html) ·
[SCIP announcement](https://sourcegraph.com/blog/announcing-scip) ·
[Stack graphs paper](https://arxiv.org/pdf/2211.01224) ·
[Cross-repository code navigation](https://sourcegraph.com/blog/cross-repository-code-navigation) ·
[AI assistants don't understand your code: LSP/SCIP](https://machinesdoitbetter.ai/ai-coding-assistants-dont-understand-your-code-lsp-scip-and-real-code-intelligence-2/)
