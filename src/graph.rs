//! Comind graph — the query layer a coding agent actually consumes.
//!
//! Loads the resolved symbol/edge graph into an in-memory `petgraph` (built once, queried in
//! microseconds) and answers the questions grep can't:
//!
//!   * [`CodeGraph::ripple`] — *who breaks if I change this?* Reverse-reachable callers and
//!     importers, multi-hop, across repos. The org-wide blast radius.
//!   * [`CodeGraph::thread`] — *what does this do downstream?* Forward call trace from an entry.
//!   * [`CodeGraph::zoom`]   — *show me this symbol in context.* Container, members, callers,
//!     callees, importers in one structured view.
//!   * [`CodeGraph::context_pack`] — *give me exactly what I need to read to change X safely,*
//!     within a token budget. Personalized-PageRank proximity ranking, greedily packed.
//!
//! Every result carries an exact `file:line` location and the signature, so the agent jumps
//! straight to code instead of searching for it.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use crate::model::{Edge, EdgeKind, Symbol};
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use petgraph::Direction::{Incoming, Outgoing};

/// A node as returned to callers — flat, printable, agent-friendly.
#[derive(Debug, Clone)]
pub struct Node {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub repo: String,
    /// `path:line` — jump straight here.
    pub location: String,
    pub signature: Option<String>,
}

/// A node plus how far it sits from the focus (BFS depth) and via which relation.
#[derive(Debug, Clone)]
pub struct Hop {
    pub node: Node,
    pub depth: usize,
    pub via: EdgeKind,
}

/// The 360° view of one symbol.
#[derive(Debug, Clone, Default)]
pub struct Zoom {
    pub focus: Option<Node>,
    pub container: Option<Node>,
    pub members: Vec<Node>,
    pub callers: Vec<Node>,
    pub callees: Vec<Node>,
    pub importers: Vec<Node>,
}

struct NodeData {
    sym: Symbol,
}

pub struct CodeGraph {
    g: DiGraph<NodeData, EdgeKind>,
    by_id: HashMap<String, NodeIndex>,
    by_name: HashMap<String, Vec<NodeIndex>>,
    /// edges we couldn't attach because an endpoint symbol was missing.
    pub dangling_edges: usize,
}

impl CodeGraph {
    /// Build from the resolved corpus. Edge endpoints are matched by rendered SCIP id.
    pub fn build(symbols: &[Symbol], edges: &[Edge]) -> Self {
        let mut g = DiGraph::new();
        let mut by_id = HashMap::new();
        let mut by_name: HashMap<String, Vec<NodeIndex>> = HashMap::new();

        for s in symbols {
            let key = s.id.render();
            if by_id.contains_key(&key) {
                continue; // dedupe (same symbol can appear once per file scan)
            }
            let idx = g.add_node(NodeData { sym: s.clone() });
            by_id.insert(key, idx);
            by_name.entry(s.name.clone()).or_default().push(idx);
        }

        let mut dangling_edges = 0;
        for e in edges {
            match (by_id.get(&e.src.render()), by_id.get(&e.dst.render())) {
                (Some(&a), Some(&b)) => {
                    g.add_edge(a, b, e.kind.clone());
                }
                _ => dangling_edges += 1,
            }
        }

        Self {
            g,
            by_id,
            by_name,
            dangling_edges,
        }
    }

    pub fn node_count(&self) -> usize {
        self.g.node_count()
    }
    pub fn edge_count(&self) -> usize {
        self.g.edge_count()
    }

    fn node_of(&self, idx: NodeIndex) -> Node {
        let s = &self.g[idx].sym;
        Node {
            id: s.id.render(),
            name: s.name.clone(),
            kind: format!("{:?}", s.kind),
            repo: s.id.package.clone(),
            location: format!("{}:{}", s.file_path, s.range.start.line),
            signature: s.signature.clone(),
        }
    }

    /// Resolve a user-supplied focus string to a node: exact rendered id, then exact
    /// descriptor, then descriptor substring, then symbol name. Prefers definitions
    /// (functions/classes) over files, and shorter descriptors (less nested) on ties.
    pub fn lookup(&self, focus: &str) -> Option<NodeIndex> {
        if let Some(&i) = self.by_id.get(focus) {
            return Some(i);
        }
        let mut candidates: Vec<NodeIndex> = self
            .g
            .node_indices()
            .filter(|&i| {
                let d = &self.g[i].sym.id.descriptor;
                d == focus || d.contains(focus)
            })
            .collect();
        if candidates.is_empty() {
            if let Some(v) = self.by_name.get(focus) {
                candidates = v.clone();
            }
        }
        candidates.sort_by_key(|&i| {
            let s = &self.g[i].sym;
            let is_file = matches!(s.kind, crate::model::SymbolKind::File) as usize;
            (is_file, s.id.descriptor.len())
        });
        candidates.first().copied()
    }

    /// Indexed repos with their symbol counts (for a `repos` overview tool).
    pub fn repos(&self) -> Vec<(String, usize)> {
        let mut m: BTreeMap<String, usize> = BTreeMap::new();
        for i in self.g.node_indices() {
            *m.entry(self.g[i].sym.id.package.clone()).or_default() += 1;
        }
        m.into_iter().collect()
    }

    /// Cheap centrality proxy: number of direct dependents (incoming `Calls`/`Imports`).
    /// Used to boost widely-depended-on code in search ranking — a signal pure semantic/
    /// lexical search (e.g. semble) lacks.
    pub fn dependents_count(&self, rendered_id: &str) -> usize {
        let Some(&idx) = self.by_id.get(rendered_id) else {
            return 0;
        };
        self.g
            .edges_directed(idx, Incoming)
            .filter(|e| matches!(e.weight(), EdgeKind::Calls | EdgeKind::Imports))
            .count()
    }

    /// Find symbols by name or descriptor substring. Prefers definitions over files and
    /// shorter (less nested) descriptors. Returns up to `limit`.
    pub fn find(&self, query: &str, limit: usize) -> Vec<Node> {
        let mut v: Vec<NodeIndex> = self
            .g
            .node_indices()
            .filter(|&i| {
                let s = &self.g[i].sym;
                s.name == query || s.name.contains(query) || s.id.descriptor.contains(query)
            })
            .collect();
        v.sort_by_key(|&i| {
            let s = &self.g[i].sym;
            let is_file = matches!(s.kind, crate::model::SymbolKind::File) as usize;
            (is_file, s.id.descriptor.len())
        });
        v.into_iter().take(limit).map(|i| self.node_of(i)).collect()
    }

    /// *Who breaks if I change `focus`?* Reverse reachability over `Calls`/`Imports`
    /// (callers of callers, importers), multi-hop, cross-repo. Structural `Contains` is
    /// ignored — a containing module is not a dependency.
    pub fn ripple(&self, focus: NodeIndex, max_depth: usize) -> Vec<Hop> {
        self.bfs(
            focus,
            max_depth,
            Incoming,
            &[EdgeKind::Calls, EdgeKind::Imports],
        )
    }

    /// *What does `focus` do downstream?* Forward call trace.
    pub fn thread(&self, focus: NodeIndex, max_depth: usize) -> Vec<Hop> {
        self.bfs(focus, max_depth, Outgoing, &[EdgeKind::Calls])
    }

    fn bfs(
        &self,
        start: NodeIndex,
        max_depth: usize,
        dir: petgraph::Direction,
        kinds: &[EdgeKind],
    ) -> Vec<Hop> {
        let mut seen: HashSet<NodeIndex> = HashSet::from([start]);
        let mut q: VecDeque<(NodeIndex, usize)> = VecDeque::from([(start, 0)]);
        let mut out = Vec::new();
        while let Some((n, depth)) = q.pop_front() {
            if depth >= max_depth {
                continue;
            }
            for e in self.g.edges_directed(n, dir) {
                if !kinds.contains(e.weight()) {
                    continue;
                }
                let next = if dir == Incoming {
                    e.source()
                } else {
                    e.target()
                };
                if seen.insert(next) {
                    out.push(Hop {
                        node: self.node_of(next),
                        depth: depth + 1,
                        via: e.weight().clone(),
                    });
                    q.push_back((next, depth + 1));
                }
            }
        }
        out
    }

    /// The structured neighborhood of `focus`.
    pub fn zoom(&self, focus: NodeIndex) -> Zoom {
        let mut z = Zoom {
            focus: Some(self.node_of(focus)),
            ..Default::default()
        };
        for e in self.g.edges_directed(focus, Incoming) {
            match e.weight() {
                EdgeKind::Contains => z.container = Some(self.node_of(e.source())),
                EdgeKind::Calls => z.callers.push(self.node_of(e.source())),
                EdgeKind::Imports => z.importers.push(self.node_of(e.source())),
                _ => {}
            }
        }
        for e in self.g.edges_directed(focus, Outgoing) {
            match e.weight() {
                EdgeKind::Contains => z.members.push(self.node_of(e.target())),
                EdgeKind::Calls => z.callees.push(self.node_of(e.target())),
                _ => {}
            }
        }
        z
    }

    /// *The minimal correct context to change `focus` safely, within `token_budget`.*
    ///
    /// Ranks all symbols by personalized-PageRank proximity to `focus` (restart at focus,
    /// edges treated as undirected for proximity), then greedily packs the highest-ranked
    /// until the estimated token budget is exhausted. Returns `(node, est_tokens)` pairs.
    pub fn context_pack(&self, focus: NodeIndex, token_budget: usize) -> Vec<(Node, usize)> {
        let ranks = self.personalized_pagerank(focus, 0.85, 40);
        let mut ranked: Vec<(NodeIndex, f32)> = ranks
            .into_iter()
            .enumerate()
            .filter(|&(i, _)| NodeIndex::new(i) != focus)
            .map(|(i, r)| (NodeIndex::new(i), r))
            .collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Focus itself is always first in the pack. Skip files — a pack is a read-set of
        // definitions (functions/classes/methods), not whole files.
        let mut pack = Vec::new();
        let mut spent = est_tokens(&self.g[focus].sym);
        pack.push((self.node_of(focus), spent));
        for (idx, rank) in ranked {
            if rank <= 0.0 {
                break; // unreachable from focus
            }
            if matches!(self.g[idx].sym.kind, crate::model::SymbolKind::File) {
                continue;
            }
            let t = est_tokens(&self.g[idx].sym);
            if spent + t > token_budget {
                continue;
            }
            spent += t;
            pack.push((self.node_of(idx), t));
        }
        pack
    }

    /// Personalized PageRank with restart at `seed`, over the **dependency** view of the graph
    /// (`Calls`/`Imports` only, treated as undirected). Structural `Contains` edges are
    /// excluded so proximity reflects real usage, not mere co-location in a file/module.
    fn personalized_pagerank(&self, seed: NodeIndex, damping: f32, iters: usize) -> Vec<f32> {
        let n = self.g.node_count();
        if n == 0 {
            return vec![];
        }
        // Undirected neighbor lists over dependency edges only.
        let mut neighbors: Vec<Vec<usize>> = vec![Vec::new(); n];
        for e in self.g.edge_references() {
            if !matches!(e.weight(), EdgeKind::Calls | EdgeKind::Imports) {
                continue;
            }
            let (a, b) = (e.source().index(), e.target().index());
            neighbors[a].push(b);
            neighbors[b].push(a);
        }
        let s = seed.index();
        let mut rank = vec![0.0f32; n];
        rank[s] = 1.0;
        for _ in 0..iters {
            let mut next = vec![0.0f32; n];
            // teleport mass back to the seed
            next[s] += 1.0 - damping;
            for u in 0..n {
                if rank[u] == 0.0 || neighbors[u].is_empty() {
                    continue;
                }
                let share = damping * rank[u] / neighbors[u].len() as f32;
                for &v in &neighbors[u] {
                    next[v] += share;
                }
            }
            rank = next;
        }
        rank
    }
}

/// Rough token cost of including a symbol in a context pack (signature + location + slack).
fn est_tokens(s: &Symbol) -> usize {
    let sig = s.signature.as_deref().map_or(0, str::len);
    (sig + s.file_path.len() + 24) / 4
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{GlobalSymbolId, Language, Position, Range, RepoId, SymbolKind};

    fn id(pkg: &str, desc: &str) -> GlobalSymbolId {
        GlobalSymbolId {
            scheme: "t".into(),
            package_manager: ".".into(),
            package: pkg.into(),
            version: ".".into(),
            descriptor: desc.into(),
        }
    }
    fn sym(pkg: &str, desc: &str, name: &str, kind: SymbolKind) -> Symbol {
        Symbol {
            id: id(pkg, desc),
            name: name.into(),
            kind,
            language: Language::Python,
            repo: RepoId(pkg.into()),
            file_path: format!("{pkg}/f.py"),
            range: Range {
                start: Position { line: 3, column: 0 },
                end: Position { line: 9, column: 0 },
            },
            signature: Some(format!("def {name}(): ...")),
            docstring: None,
        }
    }
    fn edge(s: &Symbol, d: &Symbol, k: EdgeKind) -> Edge {
        Edge {
            src: s.id.clone(),
            dst: d.id.clone(),
            kind: k,
            confidence: 1.0,
            cross_repo: s.id.package != d.id.package,
        }
    }

    #[test]
    fn ripple_finds_transitive_cross_repo_callers() {
        // lib.util  <-calls-  app.mid  <-calls-  app.top   (app depends on lib)
        let util = sym("lib", "lib/util().", "util", SymbolKind::Function);
        let mid = sym("app", "app/mid().", "mid", SymbolKind::Function);
        let top = sym("app", "app/top().", "top", SymbolKind::Function);
        let syms = vec![util.clone(), mid.clone(), top.clone()];
        let edges = vec![
            edge(&mid, &util, EdgeKind::Calls),
            edge(&top, &mid, EdgeKind::Calls),
        ];
        let g = CodeGraph::build(&syms, &edges);
        let focus = g.lookup("lib/util().").unwrap();
        let hits = g.ripple(focus, 5);
        let names: Vec<_> = hits.iter().map(|h| h.node.name.as_str()).collect();
        assert!(names.contains(&"mid"));
        assert!(names.contains(&"top")); // transitive
        assert!(hits.iter().any(|h| h.node.repo == "app")); // cross-repo blast radius
    }

    #[test]
    fn context_pack_respects_budget_and_leads_with_focus() {
        let a = sym("lib", "lib/a().", "a", SymbolKind::Function);
        let b = sym("lib", "lib/b().", "b", SymbolKind::Function);
        let c = sym("lib", "lib/c().", "c", SymbolKind::Function);
        let syms = vec![a.clone(), b.clone(), c.clone()];
        let edges = vec![edge(&a, &b, EdgeKind::Calls), edge(&b, &c, EdgeKind::Calls)];
        let g = CodeGraph::build(&syms, &edges);
        let focus = g.lookup("lib/a().").unwrap();
        let pack = g.context_pack(focus, 10_000);
        assert_eq!(pack[0].0.name, "a"); // focus leads
        assert!(pack.len() >= 2); // pulled in reachable neighbors

        let tiny = g.context_pack(focus, 1); // only focus fits
        assert_eq!(tiny.len(), 1);
    }
}
