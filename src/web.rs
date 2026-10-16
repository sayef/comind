//! Local web UI (`comind ui --web`): one embedded page over a small JSON API, bound to
//! 127.0.0.1. Ledger reviews write to the index root like the terminal UI.

use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::extract::{Query, State};
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::RwLock;

use crate::graph::{Node, Zoom};
use crate::ledger::{AuthorKind, Entry, Provenance, Record, RelationKind, Status};
use crate::tui::Data;

mod assets {
    include!(concat!(env!("OUT_DIR"), "/ui_assets.rs"));
}

struct AppState {
    data: Data,
    entries: RwLock<Vec<Entry>>,
}

type Shared = Arc<AppState>;

pub fn serve(mut data: Data, port: u16) -> Result<()> {
    let entries = std::mem::take(&mut data.entries);
    let state = Arc::new(AppState {
        data,
        entries: RwLock::new(entries),
    });
    let app = Router::new()
        .route("/api/overview", get(overview))
        .route("/api/orgmap", get(orgmap))
        .route("/api/versions", get(versions))
        .route("/api/contracts", get(contracts))
        .route("/api/ledger", get(ledger))
        .route("/api/conventions", get(conventions))
        .route("/api/find", get(find))
        .route("/api/node", get(node))
        .route("/api/review", post(review))
        .fallback(get(asset))
        .layer(axum::middleware::from_fn(local_only))
        .with_state(state);
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async move {
        let addr = format!("127.0.0.1:{port}");
        let listener = tokio::net::TcpListener::bind(&addr)
            .await
            .with_context(|| format!("bind {addr}"))?;
        crate::ui::ok(&format!("comind web UI on http://{addr} (Ctrl-C to stop)"));
        axum::serve(listener, app).await.context("serve")
    })
}

/// Reject requests whose `Host` is not loopback, so a DNS-rebinding page cannot read the API.
async fn local_only(
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    let name = host.rsplit_once(':').map_or(host, |(h, _)| h);
    if matches!(name, "127.0.0.1" | "localhost" | "[::1]") {
        next.run(req).await
    } else {
        (
            StatusCode::FORBIDDEN,
            "comind web UI answers on localhost only",
        )
            .into_response()
    }
}

/// The embedded UI: a file by path, or `index.html` for client-side routes.
async fn asset(uri: axum::http::Uri) -> impl IntoResponse {
    let path = uri.path().trim_start_matches('/');
    let find = |p: &str| {
        assets::ASSETS
            .iter()
            .find(|(n, _)| *n == p)
            .map(|(_, b)| *b)
    };
    let (name, body) = match find(path) {
        Some(b) if !path.is_empty() => (path, b),
        _ if path.starts_with("api/") || path.starts_with("assets/") => {
            return (
                StatusCode::NOT_FOUND,
                [(header::CONTENT_TYPE, "text/plain")],
                &b"not found"[..],
            )
                .into_response();
        }
        _ => ("index.html", find("index.html").unwrap_or_default()),
    };
    let mime = match name.rsplit('.').next() {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript",
        Some("css") => "text/css",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("json") => "application/json",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    };
    // Bundled assets carry a content hash in their name; the page itself must stay fresh.
    let cache = if name.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, mime), (header::CACHE_CONTROL, cache)],
        body,
    )
        .into_response()
}

async fn overview(State(s): State<Shared>) -> Json<Value> {
    let d = &s.data;
    let entries = s.entries.read().await;
    let mut status: BTreeMap<String, usize> = BTreeMap::new();
    for e in entries.iter() {
        *status
            .entry(format!("{:?}", e.status).to_lowercase())
            .or_default() += 1;
    }
    let linked = d
        .contracts
        .iter()
        .filter(|c| !c.providers.is_empty() && !c.consumers.is_empty())
        .count();
    let cross = d.edges.iter().filter(|e| e.cross_repo).count();
    let skew: Vec<Value> = d
        .deps
        .iter()
        .filter(|x| x.skewed())
        .map(|x| {
            json!({"package": x.name, "publisher": x.publisher, "published": x.published,
                   "consumer": x.consumer, "locked": x.locked})
        })
        .collect();
    Json(json!({
        "repos": d.graph.repos().into_iter().map(|(r, n)| json!({"repo": r, "symbols": n})).collect::<Vec<_>>(),
        "symbols": d.symbols.len(),
        "edges": d.edges.len(),
        "cross_repo_edges": cross,
        "contracts": d.contracts.len(),
        "linked_contracts": linked,
        "ledger": status,
        "skew": skew,
        "writable": d.root.is_some(),
    }))
}

#[derive(Deserialize)]
struct AtQ {
    /// Index into `/api/versions` (oldest first); absent = the loaded (latest) graph.
    at: Option<usize>,
}

fn graph_uri(s: &AppState) -> Option<String> {
    s.data
        .root
        .as_ref()
        .map(|r| format!("{}/_graph", r.trim_end_matches('/')))
}

async fn versions(State(s): State<Shared>) -> Json<Value> {
    let list = match graph_uri(&s) {
        Some(u) => crate::index::graph_versions(&u).await.unwrap_or_default(),
        None => Vec::new(),
    };
    Json(json!(list
        .into_iter()
        .map(|(_, _, t)| t)
        .collect::<Vec<_>>()))
}

async fn orgmap(State(s): State<Shared>, Query(q): Query<AtQ>) -> impl IntoResponse {
    let d = &s.data;
    let Some(i) = q.at else {
        return (
            StatusCode::OK,
            Json(org(&d.graph, &d.symbols, &d.edges, &d.contracts)),
        );
    };
    let past = async {
        let uri = graph_uri(&s).context("read-only UI has no index root")?;
        let list = crate::index::graph_versions(&uri).await?;
        let (sv, ev, _) = *list.get(i).context("no such version")?;
        let (symbols, edges) = crate::index::read_graph_at(&uri, Some((sv, ev))).await?;
        let graph = crate::graph::CodeGraph::build(&symbols, &edges);
        let contracts = graph.contracts(None, false);
        anyhow::Ok(org(&graph, &symbols, &edges, &contracts))
    };
    match past.await {
        Ok(v) => (StatusCode::OK, Json(v)),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": format!("{e:#}")})),
        ),
    }
}

/// Repos as nodes; edges aggregate cross-repo code dependencies and contract links.
fn org(
    graph: &crate::graph::CodeGraph,
    symbols: &[crate::model::Symbol],
    edges: &[crate::model::Edge],
    contracts: &[crate::graph::ContractView],
) -> Value {
    let repo_of: std::collections::HashMap<String, &str> = symbols
        .iter()
        .map(|x| (x.id.render(), x.repo.0.as_str()))
        .collect();
    let mut links: BTreeMap<(String, String, &str), usize> = BTreeMap::new();
    for e in edges.iter().filter(|e| e.cross_repo) {
        if let (Some(a), Some(b)) = (repo_of.get(&e.src.render()), repo_of.get(&e.dst.render())) {
            if *a != crate::resolve::CONTRACTS && *b != crate::resolve::CONTRACTS && a != b {
                *links
                    .entry((a.to_string(), b.to_string(), "code"))
                    .or_default() += 1;
            }
        }
    }
    let real = |n: &&Node| n.repo != crate::resolve::CONTRACTS;
    // A queue subscribed to a topic is a contract consuming a contract: its own consumers are
    // the topic's consumers too.
    let by_id: std::collections::HashMap<&str, &crate::graph::ContractView> = contracts
        .iter()
        .map(|c| (c.contract.id.as_str(), c))
        .collect();
    for c in contracts {
        let mut consumers: Vec<&Node> = c.consumers.iter().filter(real).collect();
        for via in c.consumers.iter().filter(|n| !real(n)) {
            if let Some(q) = by_id.get(via.id.as_str()) {
                consumers.extend(q.consumers.iter().filter(real));
            }
        }
        for u in consumers {
            for p in c.providers.iter().filter(real).filter(|p| p.repo != u.repo) {
                *links
                    .entry((u.repo.clone(), p.repo.clone(), "contract"))
                    .or_default() += 1;
            }
        }
    }
    let nodes: Vec<Value> = graph
        .repos()
        .into_iter()
        .map(|(r, n)| json!({"id": r, "name": format!("{r} ({n})"), "repo": r, "location": ""}))
        .collect();
    let edges: Vec<Value> = links
        .into_iter()
        .map(|((a, b, k), n)| json!({"src": a, "dst": b, "kind": k, "weight": n}))
        .collect();
    json!({"nodes": nodes, "edges": edges})
}

async fn contracts(State(s): State<Shared>) -> Json<Value> {
    Json(json!(s.data.contracts))
}

async fn ledger(State(s): State<Shared>) -> Json<Value> {
    let entries = s.entries.read().await;
    let mut v: Vec<&Entry> = entries.iter().collect();
    v.sort_by(|a, b| b.record.created.cmp(&a.record.created));
    Json(json!(v))
}

#[derive(Deserialize)]
struct RepoQ {
    repo: Option<String>,
}

async fn conventions(State(s): State<Shared>, Query(q): Query<RepoQ>) -> Json<Value> {
    let v: Vec<_> = s
        .data
        .conventions
        .iter()
        .filter(|c| q.repo.as_deref().is_none_or(|r| c.repo == r))
        .collect();
    Json(json!(v))
}

#[derive(Deserialize)]
struct FindQ {
    q: String,
}

async fn find(State(s): State<Shared>, Query(q): Query<FindQ>) -> Json<Value> {
    Json(json!(s.data.graph.find(&q.q, 40)))
}

#[derive(Deserialize)]
struct NodeQ {
    focus: String,
}

async fn node(State(s): State<Shared>, Query(q): Query<NodeQ>) -> impl IntoResponse {
    let g = &s.data.graph;
    let Some(idx) = g.lookup(&q.focus) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "no such symbol"})),
        );
    };
    let z = g.zoom(idx);
    let ripple = g.ripple(idx, 4);
    let mut repos: BTreeMap<String, usize> = BTreeMap::new();
    for h in &ripple {
        *repos.entry(h.node.repo.clone()).or_default() += 1;
    }
    let mut ids = vec![q.focus.clone()];
    if let Some(f) = &z.focus {
        ids.push(f.id.clone());
        if let Some((_, scope)) = g.scope_of(&f.id) {
            ids.extend(scope);
        }
    }
    let entries = s.entries.read().await;
    let about: Vec<&Entry> = crate::ledger::about(&entries, &ids);
    (
        StatusCode::OK,
        Json(json!({
            "zoom": z,
            "graph": neighbourhood(&z),
            "ripple": {"total": ripple.len(), "repos": repos},
            "ledger": about,
        })),
    )
}

/// Nodes and labelled edges around the focus, for the canvas graph.
fn neighbourhood(z: &Zoom) -> Value {
    let Some(f) = &z.focus else {
        return json!({"nodes": [], "edges": []});
    };
    let mut nodes: BTreeMap<&str, &Node> = BTreeMap::from([(f.id.as_str(), f)]);
    let mut edges: Vec<Value> = Vec::new();
    let mut add = |a: &Node, b: &Node, kind: &str| {
        edges.push(json!({"src": a.id, "dst": b.id, "kind": kind}));
    };
    const CAP: usize = 40;
    let groups: [(&[Node], bool, &str); 5] = [
        (&z.callers, false, "calls"),
        (&z.callees, true, "calls"),
        (&z.importers, false, "imports"),
        (&z.members, true, "contains"),
        (&z.consumers, false, "consumes"),
    ];
    for (list, outgoing, kind) in groups {
        for n in list.iter().take(CAP) {
            nodes.insert(&n.id, n);
            if outgoing {
                add(f, n, kind);
            } else {
                add(n, f, kind);
            }
        }
    }
    for p in z.providers.iter().take(CAP) {
        nodes.insert(&p.id, p);
        add(p, f, "provides");
    }
    if let Some(c) = &z.container {
        nodes.insert(&c.id, c);
        add(c, f, "contains");
    }
    for r in &z.contracts {
        nodes.insert(&r.contract.id, &r.contract);
        add(f, &r.contract, r.role.as_str());
        for c in r.counterparts.iter().take(CAP) {
            nodes.insert(&c.id, c);
            add(c, &r.contract, "counterpart");
        }
    }
    json!({"nodes": nodes.values().collect::<Vec<_>>(), "edges": edges, "focus": f.id})
}

#[derive(Deserialize)]
struct ReviewBody {
    id: String,
    action: String,
    #[serde(default)]
    reason: String,
}

async fn review(State(s): State<Shared>, Json(b): Json<ReviewBody>) -> impl IntoResponse {
    match apply_review(&s, &b).await {
        Ok(msg) => (StatusCode::OK, Json(json!({"ok": true, "message": msg}))),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"ok": false, "message": format!("{e:#}")})),
        ),
    }
}

async fn apply_review(s: &AppState, b: &ReviewBody) -> Result<String> {
    let root = s.data.root.as_deref().context("read-only UI")?;
    let kind = RelationKind::parse(&b.action).context("unknown action")?;
    let ledger = crate::ledger::Ledger::open(root)?;
    let target = {
        let entries = s.entries.read().await;
        entries
            .iter()
            .find(|e| e.record.id == b.id)
            .map(|e| e.record.clone())
            .context("no such record")?
    };
    if matches!(kind, RelationKind::Retracts) && target.kind == crate::ledger::Kind::Relation {
        anyhow::bail!("relations cannot be retracted");
    }
    let rec = Record::relation(
        kind,
        &target,
        &b.reason,
        Provenance {
            author: AuthorKind::Human,
            actor: crate::ledger::actor(),
            source: "comind-web".into(),
            agent: None,
            session: None,
        },
    );
    ledger.put(&rec).await?;
    let records = ledger.all().await?;
    let fresh = crate::ledger::derive(&records, &s.data.symbols);
    let status = fresh
        .iter()
        .find(|e| e.record.id == b.id)
        .map_or(Status::Active, |e| e.status);
    *s.entries.write().await = fresh;
    Ok(format!("{} → {:?}", target.title, status))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(id: &str, repo: &str) -> Node {
        Node {
            id: id.into(),
            name: id.into(),
            kind: "function".into(),
            repo: repo.into(),
            location: String::new(),
            signature: None,
        }
    }

    #[test]
    fn neighbourhood_links_focus_both_ways() {
        let z = Zoom {
            focus: Some(n("f", "a")),
            callers: vec![n("caller", "b")],
            callees: vec![n("callee", "a")],
            ..Default::default()
        };
        let g = neighbourhood(&z);
        assert_eq!(g["nodes"].as_array().unwrap().len(), 3);
        let edges: Vec<(String, String)> = g["edges"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| {
                (
                    e["src"].as_str().unwrap().into(),
                    e["dst"].as_str().unwrap().into(),
                )
            })
            .collect();
        assert!(edges.contains(&("caller".into(), "f".into())));
        assert!(edges.contains(&("f".into(), "callee".into())));
    }
}
