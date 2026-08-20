//! Comind MCP server — the code graph as tools a coding agent calls live.
//!
//! Loads the resolved org graph from LanceDB once at startup (local mmap or S3), holds it in
//! memory, and serves deterministic navigation over stdio:
//!
//!   * `search`       — natural-language / hybrid code search (semantic + lexical + centrality)
//!   * `repos`        — indexed repositories + symbol counts
//!   * `suggest`      — pre-generated questions you can ask (query catalog)
//!   * `find`         — locate symbols by name/path substring
//!   * `zoom`         — 360° view of a symbol (callers, callees, importers, members)
//!   * `ripple`       — org-wide blast radius (who breaks if I change this)
//!   * `thread`       — forward call trace from an entry point
//!   * `flow`         — pre-generated walkthrough of a flow + its live call trace
//!   * `context_pack` — the minimal token-budgeted read-set to change a symbol safely
//!   * `guide`        — the repo's inferred style guide
//!
//! Results are handed back as **markdown by default** (readable for the agent); the same data is
//! also attached as structured JSON. Pass `serve --format json` to make the text block raw JSON.
//! Every result carries an exact `file:line`.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::Arc;

use crate::embed::Embedder;
use crate::graph::{CodeGraph, Node as GNode};
use crate::model::Symbol;
use anyhow::Result;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock};
use rmcp::{schemars, tool, tool_router, ServiceExt};
use serde::{Deserialize, Serialize};

/// Per-symbol LLM enrichment keyed by rendered symbol id: `(summary, generated_queries)`.
type Enrichment = HashMap<String, (String, Vec<String>)>;

#[derive(Clone)]
pub struct ComindServer {
    graph: Arc<CodeGraph>,
    enrichment: Arc<Enrichment>,
    style_guide: Arc<Option<String>>,
    /// Pre-generated flow walkthroughs by entry-point id: `(narration, flow_questions)`.
    flows: Arc<HashMap<String, (String, Vec<String>)>>,
    /// Symbols by rendered id — used by `search` to look up candidate metadata.
    symbols: Arc<HashMap<String, Symbol>>,
    /// Query embedder for `search`; `None` if the model could not be loaded (offline).
    embedder: Arc<Option<Embedder>>,
    uri: Arc<String>,
    /// Hand results back as markdown text (default). `false` → raw JSON text.
    markdown: bool,
}

impl ComindServer {
    /// Build a result node, attaching its LLM summary when available.
    fn node_dto(&self, n: GNode) -> NodeDto {
        let enr = self.enrichment.get(&n.id);
        NodeDto {
            id: n.id,
            name: n.name,
            kind: n.kind,
            repo: n.repo,
            location: n.location,
            signature: n.signature,
            summary: enr.map(|(s, _)| s.clone()),
            queries: enr.map(|(_, q)| q.clone()).unwrap_or_default(),
        }
    }

    /// Wrap a tool DTO into a result: markdown text (default) or JSON text, plus the structured
    /// JSON in every case so structured-content clients keep working.
    fn reply<T: Serialize>(&self, dto: &T, md: String) -> CallToolResult {
        let value = serde_json::to_value(dto).ok();
        let text = if self.markdown {
            md
        } else {
            value
                .as_ref()
                .and_then(|v| serde_json::to_string_pretty(v).ok())
                .unwrap_or_default()
        };
        let mut r = CallToolResult::success(vec![ContentBlock::text(text)]);
        r.structured_content = value;
        r
    }
}

// ---- DTOs (serialized to the agent) -----------------------------------------------------

#[derive(Serialize, schemars::JsonSchema)]
struct NodeDto {
    id: String,
    name: String,
    kind: String,
    repo: String,
    /// `path:line` — jump straight here.
    location: String,
    signature: Option<String>,
    /// One-line LLM summary, when the index was enriched.
    summary: Option<String>,
    /// Natural-language questions this symbol answers (pre-generated during `--enrich`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    queries: Vec<String>,
}

#[derive(Serialize, schemars::JsonSchema)]
struct HopDto {
    name: String,
    kind: String,
    repo: String,
    location: String,
    depth: usize,
    via: String,
}

#[derive(Serialize, schemars::JsonSchema)]
struct RepoCount {
    repo: String,
    symbols: usize,
}

#[derive(Serialize, schemars::JsonSchema)]
struct ReposDto {
    repos: Vec<RepoCount>,
}

#[derive(Serialize, schemars::JsonSchema)]
struct FindDto {
    results: Vec<NodeDto>,
}

#[derive(Serialize, schemars::JsonSchema)]
struct ThreadDto {
    focus: String,
    trace: Vec<HopDto>,
}

#[derive(Serialize, schemars::JsonSchema)]
struct FlowDto {
    focus: String,
    found: bool,
    /// Pre-generated LLM walkthrough (present only if the index was built with `--flows`).
    narration: Option<String>,
    queries: Vec<String>,
    trace: Vec<HopDto>,
}

#[derive(Serialize, schemars::JsonSchema)]
struct ZoomDto {
    found: bool,
    focus: Option<NodeDto>,
    container: Option<NodeDto>,
    callers: Vec<NodeDto>,
    callees: Vec<NodeDto>,
    importers: Vec<NodeDto>,
    members: Vec<NodeDto>,
}

#[derive(Serialize, schemars::JsonSchema)]
struct RippleDto {
    focus: String,
    found: bool,
    total_dependents: usize,
    by_repo: Vec<RepoCount>,
    dependents: Vec<HopDto>,
}

#[derive(Serialize, schemars::JsonSchema)]
struct PackItem {
    name: String,
    location: String,
    est_tokens: usize,
    summary: Option<String>,
}

#[derive(Serialize, schemars::JsonSchema)]
struct GuideDto {
    found: bool,
    guide: Option<String>,
}

#[derive(Serialize, schemars::JsonSchema)]
struct PackDto {
    focus: String,
    found: bool,
    est_total_tokens: usize,
    token_budget: usize,
    items: Vec<PackItem>,
}

#[derive(Serialize, schemars::JsonSchema)]
struct SearchHitDto {
    /// Stable handle to pass to zoom/ripple/thread/flow/context_pack.
    id: String,
    name: String,
    kind: String,
    repo: String,
    location: String,
    signature: Option<String>,
    score: f32,
    deps: usize,
    summary: Option<String>,
}

#[derive(Serialize, schemars::JsonSchema)]
struct SearchDto {
    query: String,
    results: Vec<SearchHitDto>,
}

#[derive(Serialize, schemars::JsonSchema)]
struct SuggestItem {
    query: String,
    symbol: String,
    location: String,
}

#[derive(Serialize, schemars::JsonSchema)]
struct SuggestDto {
    total: usize,
    suggestions: Vec<SuggestItem>,
}

// ---- tool parameters --------------------------------------------------------------------

#[derive(Deserialize, schemars::JsonSchema, Default)]
struct NoArgs {}

#[derive(Deserialize, schemars::JsonSchema, Default)]
struct SearchParam {
    /// A natural-language question ("how do we create a migration?") or a symbol name.
    query: String,
    /// Max results (default 12).
    limit: Option<u32>,
}

#[derive(Deserialize, schemars::JsonSchema, Default)]
struct SuggestParam {
    /// Optional keyword to filter suggestions (matches the question or the symbol name).
    about: Option<String>,
    /// Max suggestions (default 40).
    limit: Option<u32>,
}

#[derive(Deserialize, schemars::JsonSchema, Default)]
struct FindParam {
    /// Symbol name or descriptor/path substring.
    query: String,
    /// Max results (default 20).
    limit: Option<u32>,
}

#[derive(Deserialize, schemars::JsonSchema, Default)]
struct FocusParam {
    /// A symbol name or SCIP descriptor (e.g. `acme/const/Settings`).
    focus: String,
}

#[derive(Deserialize, schemars::JsonSchema, Default)]
struct DepthParam {
    /// A symbol name or SCIP descriptor (from `find`/`search`, e.g. `acme/const/Settings`).
    focus: String,
    /// Max hops to traverse (default 4).
    depth: Option<u32>,
}

#[derive(Deserialize, schemars::JsonSchema, Default)]
struct PackParam {
    /// A symbol name or SCIP descriptor (from `find`/`search`, e.g. `acme/const/Settings`).
    focus: String,
    /// Token budget for the read-set (default 1500).
    token_budget: Option<u32>,
}

// ---- tools ------------------------------------------------------------------------------

/// First-contact orientation shown to the agent as MCP server `instructions`.
const INSTRUCTIONS: &str = "Deterministic code intelligence over an indexed codebase.\n\
New here? Call `repos` to see what's indexed and `suggest` for ready-made questions.\n\
Workflow: `find`/`search` return a symbol `id` → pass that id to `zoom` (360° view), \
`ripple` (who breaks if I change this), `thread` (forward call trace), `context_pack` \
(minimal safe edit read-set), or `flow` (narrated walkthrough). `search` needs embeddings \
(on by default unless the index was built with --no-embed); the graph tools always work.";

#[tool_router]
impl ComindServer {
    #[tool(
        name = "search",
        description = "Natural-language + lexical hybrid code search, reranked by dependency-graph centrality. Ask a question ('how do we create a migration?') or pass a symbol name."
    )]
    async fn search(&self, Parameters(p): Parameters<SearchParam>) -> CallToolResult {
        let limit = p.limit.unwrap_or(12) as usize;
        let Some(embedder) = self.embedder.as_ref() else {
            let dto = SearchDto {
                query: p.query.clone(),
                results: vec![],
            };
            return self.reply(
                &dto,
                "_search unavailable: the embedding model could not be loaded (offline?). Graph tools (find/zoom/ripple/thread/context_pack) still work._".into(),
            );
        };
        let hits = crate::search::hybrid(
            &self.uri,
            &self.symbols,
            &self.graph,
            &self.enrichment,
            embedder,
            &p.query,
            limit,
        )
        .await
        .unwrap_or_default();
        let md = crate::search::markdown(&p.query, &hits);
        let dto = SearchDto {
            query: p.query.clone(),
            results: hits.iter().map(hit_dto).collect(),
        };
        self.reply(&dto, md)
    }

    #[tool(
        name = "repos",
        description = "List indexed repositories and their symbol counts"
    )]
    fn repos(&self, _p: Parameters<NoArgs>) -> CallToolResult {
        let dto = ReposDto {
            repos: self
                .graph
                .repos()
                .into_iter()
                .map(|(repo, symbols)| RepoCount { repo, symbols })
                .collect(),
        };
        let md = md_repos(&dto);
        self.reply(&dto, md)
    }

    #[tool(
        name = "suggest",
        description = "Suggested questions you can ask about this codebase (pre-generated during --enrich). Optionally filter by a keyword to discover what flows/topics exist."
    )]
    fn suggest(&self, Parameters(p): Parameters<SuggestParam>) -> CallToolResult {
        let limit = p.limit.unwrap_or(40) as usize;
        let about = p.about.as_deref().map(str::to_lowercase);
        let mut items: Vec<SuggestItem> = Vec::new();
        for (id, (_, queries)) in self.enrichment.iter() {
            let (symbol, location) = self
                .symbols
                .get(id)
                .map(|s| {
                    (
                        s.name.clone(),
                        format!("{}:{}", s.file_path, s.range.start.line),
                    )
                })
                .unwrap_or_else(|| (id.clone(), String::new()));
            for q in queries {
                if let Some(a) = &about {
                    if !q.to_lowercase().contains(a) && !symbol.to_lowercase().contains(a) {
                        continue;
                    }
                }
                items.push(SuggestItem {
                    query: q.clone(),
                    symbol: symbol.clone(),
                    location: location.clone(),
                });
            }
        }
        items.sort_by(|a, b| a.symbol.cmp(&b.symbol).then(a.query.cmp(&b.query)));
        let total = items.len();
        items.truncate(limit);
        let dto = SuggestDto {
            total,
            suggestions: items,
        };
        let md = md_suggest(&dto);
        self.reply(&dto, md)
    }

    #[tool(
        name = "find",
        description = "Find symbols by name or path substring. Returns each match with its `id` and `file:line` — the entry point to the graph tools: pass an `id` to zoom/ripple/thread/context_pack/flow."
    )]
    fn find(&self, Parameters(p): Parameters<FindParam>) -> CallToolResult {
        let limit = p.limit.unwrap_or(20) as usize;
        let dto = FindDto {
            results: self
                .graph
                .find(&p.query, limit)
                .into_iter()
                .map(|n| self.node_dto(n))
                .collect(),
        };
        let md = md_find(&p.query, &dto);
        self.reply(&dto, md)
    }

    #[tool(
        name = "zoom",
        description = "360° view of one symbol — definition, container, callers, callees, importers, members. Use to understand a symbol before changing it. Takes a symbol id/name from find/search."
    )]
    fn zoom(&self, Parameters(p): Parameters<FocusParam>) -> CallToolResult {
        let dto = match self.graph.lookup(&p.focus) {
            None => ZoomDto {
                found: false,
                focus: None,
                container: None,
                callers: vec![],
                callees: vec![],
                importers: vec![],
                members: vec![],
            },
            Some(idx) => {
                let z = self.graph.zoom(idx);
                ZoomDto {
                    found: true,
                    focus: z.focus.map(|n| self.node_dto(n)),
                    container: z.container.map(|n| self.node_dto(n)),
                    callers: z.callers.into_iter().map(|n| self.node_dto(n)).collect(),
                    callees: z.callees.into_iter().map(|n| self.node_dto(n)).collect(),
                    importers: z.importers.into_iter().map(|n| self.node_dto(n)).collect(),
                    members: z.members.into_iter().map(|n| self.node_dto(n)).collect(),
                }
            }
        };
        let md = md_zoom(&p.focus, &dto);
        self.reply(&dto, md)
    }

    #[tool(
        name = "ripple",
        description = "Blast radius (reverse deps): who transitively depends on this symbol, cross-repo, grouped by repo. Use to gauge the impact of changing it. This is the reverse direction of `thread`. Returns dependents with depth + repo counts."
    )]
    fn ripple(&self, Parameters(p): Parameters<DepthParam>) -> CallToolResult {
        let depth = p.depth.unwrap_or(4) as usize;
        let dto = match self.graph.lookup(&p.focus) {
            None => RippleDto {
                focus: p.focus.clone(),
                found: false,
                total_dependents: 0,
                by_repo: vec![],
                dependents: vec![],
            },
            Some(idx) => {
                let hops = self.graph.ripple(idx, depth);
                let mut by_repo: std::collections::BTreeMap<String, usize> =
                    std::collections::BTreeMap::new();
                for h in &hops {
                    *by_repo.entry(h.node.repo.clone()).or_default() += 1;
                }
                RippleDto {
                    focus: p.focus.clone(),
                    found: true,
                    total_dependents: hops.len(),
                    by_repo: by_repo
                        .into_iter()
                        .map(|(repo, symbols)| RepoCount { repo, symbols })
                        .collect(),
                    dependents: hops.into_iter().map(hop_dto).collect(),
                }
            }
        };
        let md = md_ripple(&dto);
        self.reply(&dto, md)
    }

    #[tool(
        name = "guide",
        description = "The repo's inferred coding style guide (naming, typing, structure)"
    )]
    fn guide(&self, _p: Parameters<NoArgs>) -> CallToolResult {
        let dto = GuideDto {
            found: self.style_guide.is_some(),
            guide: (*self.style_guide).clone(),
        };
        let md = md_guide(&dto);
        self.reply(&dto, md)
    }

    #[tool(
        name = "thread",
        description = "Forward call trace from an entry point: what this symbol calls, transitively, with depth + edge kind. Use to follow execution downstream. `flow` = this trace plus a narrated explanation; `ripple` = the reverse (who calls in)."
    )]
    fn thread(&self, Parameters(p): Parameters<DepthParam>) -> CallToolResult {
        let depth = p.depth.unwrap_or(4) as usize;
        let trace = match self.graph.lookup(&p.focus) {
            Some(idx) => self
                .graph
                .thread(idx, depth)
                .into_iter()
                .map(hop_dto)
                .collect(),
            None => vec![],
        };
        let dto = ThreadDto {
            focus: p.focus.clone(),
            trace,
        };
        let md = md_thread(&dto);
        self.reply(&dto, md)
    }

    #[tool(
        name = "flow",
        description = "Explain how a flow works: a pre-generated LLM walkthrough of an entry point (when the index was built with --flows) plus its live forward call trace."
    )]
    fn flow(&self, Parameters(p): Parameters<DepthParam>) -> CallToolResult {
        let depth = p.depth.unwrap_or(4) as usize;
        let dto = match self.graph.lookup(&p.focus) {
            None => FlowDto {
                focus: p.focus.clone(),
                found: false,
                narration: None,
                queries: vec![],
                trace: vec![],
            },
            Some(idx) => {
                let fid = self.graph.zoom(idx).focus.map(|n| n.id);
                let (narration, queries) = fid
                    .as_ref()
                    .and_then(|id| self.flows.get(id))
                    .map(|(n, q)| (Some(n.clone()), q.clone()))
                    .unwrap_or((None, vec![]));
                let trace = self
                    .graph
                    .thread(idx, depth)
                    .into_iter()
                    .map(hop_dto)
                    .collect();
                FlowDto {
                    focus: p.focus.clone(),
                    found: true,
                    narration,
                    queries,
                    trace,
                }
            }
        };
        let md = md_flow(&dto);
        self.reply(&dto, md)
    }

    #[tool(
        name = "context_pack",
        description = "The minimal token-budgeted set of symbols to read before editing the focus symbol safely. Use to assemble just-enough context for a change. Returns ranked symbols with locations + token estimates within the budget."
    )]
    fn context_pack(&self, Parameters(p): Parameters<PackParam>) -> CallToolResult {
        let budget = p.token_budget.unwrap_or(1500) as usize;
        let dto = match self.graph.lookup(&p.focus) {
            None => PackDto {
                focus: p.focus.clone(),
                found: false,
                est_total_tokens: 0,
                token_budget: budget,
                items: vec![],
            },
            Some(idx) => {
                let pack = self.graph.context_pack(idx, budget);
                let est_total_tokens = pack.iter().map(|(_, t)| t).sum();
                PackDto {
                    focus: p.focus.clone(),
                    found: true,
                    est_total_tokens,
                    token_budget: budget,
                    items: pack
                        .into_iter()
                        .map(|(n, t)| PackItem {
                            summary: self.enrichment.get(&n.id).map(|(s, _)| s.clone()),
                            name: n.name,
                            location: n.location,
                            est_tokens: t,
                        })
                        .collect(),
                }
            }
        };
        let md = md_pack(&dto);
        self.reply(&dto, md)
    }
}

#[rmcp::tool_handler(router = Self::tool_router())]
impl rmcp::ServerHandler for ComindServer {
    fn get_info(&self) -> rmcp::model::ServerInfo {
        rmcp::model::ServerInfo::new(
            rmcp::model::ServerCapabilities::builder()
                .enable_tools()
                .build(),
        )
        .with_server_info(rmcp::model::Implementation::from_build_env())
        .with_instructions(INSTRUCTIONS)
    }
}

fn hop_dto(h: crate::graph::Hop) -> HopDto {
    HopDto {
        name: h.node.name,
        kind: h.node.kind,
        repo: h.node.repo,
        location: h.node.location,
        depth: h.depth,
        via: h.via.as_str().to_string(),
    }
}

fn hit_dto(h: &crate::search::SearchHit) -> SearchHitDto {
    SearchHitDto {
        id: h.id.clone(),
        name: h.name.clone(),
        kind: h.kind.clone(),
        repo: h.repo.clone(),
        location: h.location.clone(),
        signature: h.signature.clone(),
        score: h.score,
        deps: h.deps,
        summary: h.summary.clone(),
    }
}

// ---- markdown rendering (the agent-facing handover) -------------------------------------

fn node_bullet(o: &mut String, n: &NodeDto) {
    let _ = write!(o, "- **{}** _{}_ — `{}`", n.name, n.kind, n.location);
    if !n.repo.is_empty() {
        let _ = write!(o, " · {}", n.repo);
    }
    if let Some(s) = &n.summary {
        let _ = write!(o, " — {s}");
    }
    // Stable handle to pass back to zoom/ripple/thread/flow/context_pack.
    let _ = write!(o, "  ·  id `{}`", n.id);
    o.push('\n');
}

fn node_section(o: &mut String, title: &str, nodes: &[NodeDto]) {
    if nodes.is_empty() {
        return;
    }
    let _ = write!(o, "\n**{title}** ({})\n", nodes.len());
    for n in nodes {
        node_bullet(o, n);
    }
}

fn md_repos(d: &ReposDto) -> String {
    let mut o = String::from("## Indexed repositories\n\n");
    if d.repos.is_empty() {
        return o + "_none_\n";
    }
    for r in &d.repos {
        let _ = writeln!(o, "- **{}** — {} symbols", r.repo, r.symbols);
    }
    o
}

fn md_suggest(d: &SuggestDto) -> String {
    let shown = d.suggestions.len();
    let mut o = if d.total > shown {
        format!(
            "## Suggested questions ({shown} of {} — pass `about` to filter)\n\n",
            d.total
        )
    } else {
        format!("## Suggested questions ({shown})\n\n")
    };
    if d.suggestions.is_empty() {
        return o + "_none — index was built without `--enrich`, or no match_\n";
    }
    let mut cur = "";
    for it in &d.suggestions {
        if it.symbol != cur {
            let _ = write!(o, "\n**{}** `{}`\n", it.symbol, it.location);
            cur = &it.symbol;
        }
        let _ = writeln!(o, "- {}", it.query);
    }
    o
}

fn md_find(query: &str, d: &FindDto) -> String {
    let mut o = format!("## `find` \"{query}\" — {} result(s)\n\n", d.results.len());
    if d.results.is_empty() {
        return o + "_no symbols matched_\n";
    }
    for n in &d.results {
        node_bullet(&mut o, n);
    }
    o
}

fn md_zoom(focus: &str, d: &ZoomDto) -> String {
    if !d.found {
        return format!("## zoom `{focus}`\n\n_no symbol matched — call `find` with the name to get its id, then retry._\n");
    }
    let mut o = String::new();
    if let Some(f) = &d.focus {
        let _ = writeln!(o, "## {} _{}_\n`{}`", f.name, f.kind, f.location);
        if let Some(sig) = &f.signature {
            let _ = writeln!(o, "\n```\n{sig}\n```");
        }
        if let Some(s) = &f.summary {
            let _ = writeln!(o, "\n{s}");
        }
        if !f.queries.is_empty() {
            let _ = writeln!(o, "\n**Ask:** {}", f.queries.join(" · "));
        }
    } else {
        let _ = writeln!(o, "## zoom `{focus}`");
    }
    if let Some(c) = &d.container {
        let _ = write!(o, "\n**Container**\n");
        node_bullet(&mut o, c);
    }
    node_section(&mut o, "Members", &d.members);
    node_section(&mut o, "Callers", &d.callers);
    node_section(&mut o, "Callees", &d.callees);
    node_section(&mut o, "Importers", &d.importers);
    o
}

fn md_hop_lines(o: &mut String, hops: &[HopDto]) {
    for h in hops {
        let _ = writeln!(
            o,
            "- _d{}_ via **{}** — **{}** `{}`",
            h.depth, h.via, h.name, h.location
        );
    }
}

fn md_ripple(d: &RippleDto) -> String {
    if !d.found {
        return format!("## Blast radius: `{}`\n\n_no symbol matched — call `find` with the name to get its id, then retry._\n", d.focus);
    }
    let mut o = format!(
        "## Blast radius: `{}`\n\n**{} dependent(s)**",
        d.focus, d.total_dependents
    );
    if !d.by_repo.is_empty() {
        let by: Vec<String> = d
            .by_repo
            .iter()
            .map(|r| format!("{} ({})", r.repo, r.symbols))
            .collect();
        let _ = write!(o, " across {}", by.join(", "));
    }
    o.push_str("\n\n");
    md_hop_lines(&mut o, &d.dependents);
    o
}

fn md_thread(d: &ThreadDto) -> String {
    let mut o = format!(
        "## Call trace from `{}` — {} step(s)\n\n",
        d.focus,
        d.trace.len()
    );
    if d.trace.is_empty() {
        return o + "_no outgoing calls found_\n";
    }
    md_hop_lines(&mut o, &d.trace);
    o
}

fn md_flow(d: &FlowDto) -> String {
    if !d.found {
        return format!("## Flow: `{}`\n\n_no symbol matched — call `find` with the name to get its id, then retry._\n", d.focus);
    }
    let mut o = format!("## Flow: `{}`\n\n", d.focus);
    match &d.narration {
        Some(n) => {
            let _ = writeln!(o, "{n}\n");
        }
        None => {
            let _ = writeln!(
                o,
                "_no pre-generated narration (build the index with `--flows` to add one) — raw call trace below._\n"
            );
        }
    }
    if !d.queries.is_empty() {
        let _ = writeln!(o, "**Ask:** {}\n", d.queries.join(" · "));
    }
    let _ = writeln!(o, "**Call trace** ({} steps)", d.trace.len());
    md_hop_lines(&mut o, &d.trace);
    o
}

fn md_pack(d: &PackDto) -> String {
    if !d.found {
        return format!("## Context pack: `{}`\n\n_no symbol matched — call `find` with the name to get its id, then retry._\n", d.focus);
    }
    let mut o = format!(
        "## Context to change `{}`\n\n_~{} of {} token budget · {} symbols_\n\n",
        d.focus,
        d.est_total_tokens,
        d.token_budget,
        d.items.len()
    );
    for (i, it) in d.items.iter().enumerate() {
        let _ = write!(
            o,
            "{}. **{}** — `{}` _(~{} tok)_",
            i + 1,
            it.name,
            it.location,
            it.est_tokens
        );
        if let Some(s) = &it.summary {
            let _ = write!(o, " — {s}");
        }
        o.push('\n');
    }
    o
}

fn md_guide(d: &GuideDto) -> String {
    match &d.guide {
        Some(g) => format!("## Style guide\n\n{g}\n"),
        None => "## Style guide\n\n_none — index was built without `--enrich`_\n".into(),
    }
}

/// Load the graph (and any LLM enrichment) from `uri` and serve MCP over stdio until the
/// client disconnects. `markdown` controls whether tool results are handed back as markdown
/// text (default) or raw JSON.
pub async fn serve_stdio(uri: &str, markdown: bool) -> Result<()> {
    let (symbols, edges) = crate::index::read_graph(uri).await?;
    let graph = Arc::new(CodeGraph::build(&symbols, &edges));
    let by_id: HashMap<String, Symbol> =
        symbols.iter().map(|s| (s.id.render(), s.clone())).collect();

    let enrichment: Enrichment = crate::index::read_enrichment(uri)
        .await
        .ok()
        .flatten()
        .unwrap_or_default()
        .into_iter()
        .map(|(id, s, q)| (id.render(), (s, q)))
        .collect();

    let style_guide = crate::index::read_style_guide(uri).await.ok().flatten();

    let flows: HashMap<String, (String, Vec<String>)> = crate::index::read_flows(uri)
        .await
        .ok()
        .flatten()
        .unwrap_or_default()
        .into_iter()
        .map(|(id, narr, q)| (id.render(), (narr, q)))
        .collect();

    // The embedder powers `search`; graph tools work without it, so a load failure is non-fatal.
    let embedder = match crate::embed::Embedder::load_default() {
        Ok(e) => Some(e),
        Err(e) => {
            eprintln!("comind serve: embedding model unavailable, `search` disabled: {e:#}");
            None
        }
    };

    let server = ComindServer {
        graph,
        enrichment: Arc::new(enrichment),
        style_guide: Arc::new(style_guide),
        flows: Arc::new(flows),
        symbols: Arc::new(by_id),
        embedder: Arc::new(embedder),
        uri: Arc::new(uri.to_string()),
        markdown,
    };
    let service = server.serve(rmcp::transport::io::stdio()).await?;
    service.waiting().await?;
    Ok(())
}
