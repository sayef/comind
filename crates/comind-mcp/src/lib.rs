//! Comind MCP server — the code graph as tools a coding agent calls live.
//!
//! Loads the resolved org graph from LanceDB once at startup (local mmap or S3), holds it in
//! memory, and serves deterministic navigation over stdio:
//!
//!   * `repos`        — indexed repositories + symbol counts
//!   * `find`         — locate symbols by name/path substring
//!   * `zoom`         — 360° view of a symbol (callers, callees, importers, members)
//!   * `ripple`       — org-wide blast radius (who breaks if I change this)
//!   * `thread`       — forward call trace from an entry point
//!   * `context_pack` — the minimal token-budgeted read-set to change a symbol safely
//!
//! Every result carries an exact `file:line`, so the agent jumps straight to code.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use comind_graph::{CodeGraph, Node as GNode};
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::{schemars, tool, tool_router, ServiceExt};
use serde::{Deserialize, Serialize};

/// Per-symbol LLM enrichment keyed by rendered symbol id: `(summary, generated_queries)`.
type Enrichment = HashMap<String, (String, Vec<String>)>;

#[derive(Clone)]
pub struct ComindServer {
    graph: Arc<CodeGraph>,
    enrichment: Arc<Enrichment>,
    style_guide: Arc<Option<String>>,
}

impl ComindServer {
    /// Build a result node, attaching its LLM summary when available.
    fn node_dto(&self, n: GNode) -> NodeDto {
        let summary = self.enrichment.get(&n.id).map(|(s, _)| s.clone());
        NodeDto {
            id: n.id,
            name: n.name,
            kind: n.kind,
            repo: n.repo,
            location: n.location,
            signature: n.signature,
            summary,
        }
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

// MCP requires tool output schemas to have an object root, so list results are wrapped.
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

// ---- tool parameters --------------------------------------------------------------------

#[derive(Deserialize, schemars::JsonSchema, Default)]
struct NoArgs {}

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
    focus: String,
    /// Max hops to traverse (default 4).
    depth: Option<u32>,
}

#[derive(Deserialize, schemars::JsonSchema, Default)]
struct PackParam {
    focus: String,
    /// Token budget for the read-set (default 1500).
    token_budget: Option<u32>,
}

// ---- tools ------------------------------------------------------------------------------

#[tool_router(server_handler)]
impl ComindServer {
    #[tool(name = "repos", description = "List indexed repositories and their symbol counts")]
    fn repos(&self, _p: Parameters<NoArgs>) -> Json<ReposDto> {
        Json(ReposDto {
            repos: self
                .graph
                .repos()
                .into_iter()
                .map(|(repo, symbols)| RepoCount { repo, symbols })
                .collect(),
        })
    }

    #[tool(name = "find", description = "Find code symbols by name or path substring")]
    fn find(&self, Parameters(p): Parameters<FindParam>) -> Json<FindDto> {
        let limit = p.limit.unwrap_or(20) as usize;
        Json(FindDto {
            results: self
                .graph
                .find(&p.query, limit)
                .into_iter()
                .map(|n| self.node_dto(n))
                .collect(),
        })
    }

    #[tool(
        name = "zoom",
        description = "360° view of a symbol: definition, container, callers, callees, importers, members"
    )]
    fn zoom(&self, Parameters(p): Parameters<FocusParam>) -> Json<ZoomDto> {
        let Some(idx) = self.graph.lookup(&p.focus) else {
            return Json(ZoomDto {
                found: false,
                focus: None,
                container: None,
                callers: vec![],
                callees: vec![],
                importers: vec![],
                members: vec![],
            });
        };
        let z = self.graph.zoom(idx);
        Json(ZoomDto {
            found: true,
            focus: z.focus.map(|n| self.node_dto(n)),
            container: z.container.map(|n| self.node_dto(n)),
            callers: z.callers.into_iter().map(|n| self.node_dto(n)).collect(),
            callees: z.callees.into_iter().map(|n| self.node_dto(n)).collect(),
            importers: z.importers.into_iter().map(|n| self.node_dto(n)).collect(),
            members: z.members.into_iter().map(|n| self.node_dto(n)).collect(),
        })
    }

    #[tool(
        name = "ripple",
        description = "Blast radius: who transitively depends on this symbol (cross-repo), grouped by repo"
    )]
    fn ripple(&self, Parameters(p): Parameters<DepthParam>) -> Json<RippleDto> {
        let depth = p.depth.unwrap_or(4) as usize;
        let Some(idx) = self.graph.lookup(&p.focus) else {
            return Json(RippleDto {
                focus: p.focus,
                found: false,
                total_dependents: 0,
                by_repo: vec![],
                dependents: vec![],
            });
        };
        let hops = self.graph.ripple(idx, depth);
        let mut by_repo: std::collections::BTreeMap<String, usize> =
            std::collections::BTreeMap::new();
        for h in &hops {
            *by_repo.entry(h.node.repo.clone()).or_default() += 1;
        }
        Json(RippleDto {
            focus: p.focus,
            found: true,
            total_dependents: hops.len(),
            by_repo: by_repo
                .into_iter()
                .map(|(repo, symbols)| RepoCount { repo, symbols })
                .collect(),
            dependents: hops.into_iter().map(hop_dto).collect(),
        })
    }

    #[tool(name = "guide", description = "The repo's inferred coding style guide (naming, typing, structure)")]
    fn guide(&self, _p: Parameters<NoArgs>) -> Json<GuideDto> {
        Json(GuideDto {
            found: self.style_guide.is_some(),
            guide: (*self.style_guide).clone(),
        })
    }

    #[tool(name = "thread", description = "Forward call trace from an entry-point symbol")]
    fn thread(&self, Parameters(p): Parameters<DepthParam>) -> Json<ThreadDto> {
        let depth = p.depth.unwrap_or(4) as usize;
        let trace = match self.graph.lookup(&p.focus) {
            Some(idx) => self.graph.thread(idx, depth).into_iter().map(hop_dto).collect(),
            None => vec![],
        };
        Json(ThreadDto { trace })
    }

    #[tool(
        name = "context_pack",
        description = "The minimal token-budgeted set of symbols to read in order to change the focus symbol safely"
    )]
    fn context_pack(&self, Parameters(p): Parameters<PackParam>) -> Json<PackDto> {
        let budget = p.token_budget.unwrap_or(1500) as usize;
        let Some(idx) = self.graph.lookup(&p.focus) else {
            return Json(PackDto {
                focus: p.focus,
                found: false,
                est_total_tokens: 0,
                token_budget: budget,
                items: vec![],
            });
        };
        let pack = self.graph.context_pack(idx, budget);
        let est_total_tokens = pack.iter().map(|(_, t)| t).sum();
        Json(PackDto {
            focus: p.focus,
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
        })
    }
}

fn hop_dto(h: comind_graph::Hop) -> HopDto {
    HopDto {
        name: h.node.name,
        kind: h.node.kind,
        repo: h.node.repo,
        location: h.node.location,
        depth: h.depth,
        via: format!("{:?}", h.via),
    }
}

/// Load the graph (and any LLM enrichment) from `uri` and serve MCP over stdio until the
/// client disconnects.
pub async fn serve_stdio(uri: &str) -> Result<()> {
    let (symbols, edges) = comind_index::read_graph(uri).await?;
    let graph = Arc::new(CodeGraph::build(&symbols, &edges));

    let enrichment: Enrichment = comind_index::read_enrichment(uri)
        .await
        .ok()
        .flatten()
        .unwrap_or_default()
        .into_iter()
        .map(|(id, s, q)| (id.render(), (s, q)))
        .collect();

    let style_guide = comind_index::read_style_guide(uri).await.ok().flatten();

    let server = ComindServer {
        graph,
        enrichment: Arc::new(enrichment),
        style_guide: Arc::new(style_guide),
    };
    let service = server.serve(rmcp::transport::io::stdio()).await?;
    service.waiting().await?;
    Ok(())
}
