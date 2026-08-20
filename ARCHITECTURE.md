# Comind — Rust Architecture

> Polyglot, cross-repo code-intelligence engine. CLI + MCP server, single static binary.
> Read-heavy / write-rare: the index is a **versioned artifact in S3**, not a live shared DB.

## Decisions (locked)

| Question | Decision |
|---|---|
| Language scope | **Polyglot from day 1** via tree-sitter grammars. Infra langs low priority. |
| Rewrite | **Full Rust rewrite.** LLM enrichment stays, via Rig (provider-agnostic). |
| Cross-repo graph | **Core launch requirement** — federated symbol identity from day 1. |
| Embeddings | **Local model in CI** (Model2Vec). No source leaves org infra. |
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
  llm.rs      wiki / style-guide / query-association generation via Rig (provider-agnostic).
  mcp.rs      rmcp server: search/suggest/repos/find/zoom/ripple/thread/flow/context_pack/guide.
  ui.rs       styled stderr output (indicatif progress + console + comfy-table tables).
  config.rs   config.toml + defaults (index dir, models, format, caps); precedence resolver.
  git.rs      git2 change detection for incremental indexing.
  main.rs     clap binary: `comind index|link|explore|search|flow|changed|serve|config`. The single distributable.
```

One crate, one module per stage; `main.rs` uses the library as `comind::<module>`. (Earlier phases
below were built as a `cargo` workspace of `comind-*` crates and later consolidated into this single crate.)

MCP verb semantics follow a design proven in the original Python implementation.

## Competitive landscape

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
