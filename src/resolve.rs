//! Comind resolve — bind provisional references to real definitions, across repos.
//!
//! `comind-parse` emits two kinds of unresolved edge:
//!   * `Imports` to an *external* target (package `?`, descriptor = the import path core),
//!   * `Calls` to a provisional target (descriptor `?/<name>()`).
//!
//! This crate rebinds them against the union of definitions from the whole corpus:
//!   * an import `from acme.const import Settings` in `service-a` binds to the definition
//!     `acme/const/Settings#` in `pkg-common` → a **cross-repo edge**. This is the
//!     signal `ripple` traverses for org-wide blast radius.
//!   * a call `foo()` binds to a same-repo definition of `foo` when unambiguous.
//!
//! Identity is by SCIP descriptor "core" (the descriptor minus its kind suffix), so a class
//! `Foo#`, a function `bar().`, or a module `pkg/mod` all match their import-path form.

use std::collections::HashMap;

use crate::model::{Edge, EdgeKind, GlobalSymbolId, Symbol};

/// Strip the SCIP kind suffix so a definition's descriptor matches its import-path form.
/// `acme/const/Settings#` -> `acme/const/Settings`;
/// `acme/utils/run().`      -> `acme/utils/run`.
fn core_of(descriptor: &str) -> &str {
    if let Some(s) = descriptor.strip_suffix("().") {
        s
    } else if let Some(s) = descriptor.strip_suffix('#') {
        s
    } else {
        descriptor
    }
}

fn is_external_import(target: &GlobalSymbolId) -> bool {
    target.package == "?"
}

fn provisional_call_name(target: &GlobalSymbolId) -> Option<&str> {
    target
        .descriptor
        .strip_prefix("?/")
        .and_then(|s| s.strip_suffix("()"))
}

/// Result of resolving a corpus: the rebound edge set plus a summary.
#[derive(Debug, Default)]
pub struct Resolved {
    pub edges: Vec<Edge>,
    pub stats: ResolveStats,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ResolveStats {
    pub resolved_imports: usize,
    pub resolved_calls: usize,
    pub cross_repo_edges: usize,
    /// Imports whose target was not found in the corpus (e.g. third-party libraries).
    pub unresolved_imports: usize,
}

/// Resolve the merged `symbols`/`edges` of one or more repos.
///
/// `Contains` and already-resolved edges pass through untouched. Provisional `Imports`/`Calls`
/// edges are rebound where a definition is found; unresolved provisional edges are dropped
/// from the output (they carry no navigable target).
pub fn resolve(symbols: &[Symbol], edges: &[Edge]) -> Resolved {
    // Definition indexes over the whole corpus.
    let mut by_core: HashMap<&str, Vec<&Symbol>> = HashMap::new();
    let mut by_name: HashMap<&str, Vec<&Symbol>> = HashMap::new();
    for s in symbols {
        by_core
            .entry(core_of(&s.id.descriptor))
            .or_default()
            .push(s);
        by_name.entry(s.name.as_str()).or_default().push(s);
    }

    let mut out = Resolved::default();
    for e in edges {
        match e.kind {
            EdgeKind::Imports if is_external_import(&e.dst) => {
                match resolve_import(&e.dst, &by_core) {
                    Some(def) => out.push(bind(&e.src, &def.id, EdgeKind::Imports, 0.95)),
                    None => out.stats.unresolved_imports += 1,
                }
            }
            EdgeKind::Calls if provisional_call_name(&e.dst).is_some() => {
                let name = provisional_call_name(&e.dst).unwrap();
                if let Some(def) = resolve_call(&e.src, name, &by_name) {
                    out.push(bind(&e.src, &def.id, EdgeKind::Calls, 0.8));
                }
                // ambiguous / unknown calls are dropped (no reliable target)
            }
            // Contains, and any pre-resolved edge, pass through.
            _ => out.edges.push(e.clone()),
        }
    }
    out
}

/// An import binds to the unique definition matching its descriptor core. If several repos
/// define the same core, take the first (explicit import paths make collisions rare).
fn resolve_import<'a>(
    target: &GlobalSymbolId,
    by_core: &HashMap<&str, Vec<&'a Symbol>>,
) -> Option<&'a Symbol> {
    by_core
        .get(target.descriptor.as_str())
        .and_then(|defs| defs.first().copied())
}

/// A call binds to a same-repo definition of `name` when there is exactly one; otherwise it
/// is left unresolved (name-only cross-repo call binding is too noisy to trust).
fn resolve_call<'a>(
    src: &GlobalSymbolId,
    name: &str,
    by_name: &HashMap<&str, Vec<&'a Symbol>>,
) -> Option<&'a Symbol> {
    let defs = by_name.get(name)?;
    let same_repo: Vec<&Symbol> = defs
        .iter()
        .copied()
        .filter(|d| d.id.package == src.package)
        .collect();
    match same_repo.as_slice() {
        [only] => Some(*only),
        _ => None,
    }
}

impl Resolved {
    fn push(&mut self, e: Edge) {
        if e.kind == EdgeKind::Imports {
            self.stats.resolved_imports += 1;
        } else if e.kind == EdgeKind::Calls {
            self.stats.resolved_calls += 1;
        }
        if e.cross_repo {
            self.stats.cross_repo_edges += 1;
        }
        self.edges.push(e);
    }
}

fn bind(src: &GlobalSymbolId, dst: &GlobalSymbolId, kind: EdgeKind, confidence: f32) -> Edge {
    Edge {
        src: src.clone(),
        dst: dst.clone(),
        kind,
        confidence,
        cross_repo: src.package != dst.package,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Language, Position, Range, RepoId, SymbolKind};

    fn sym(pkg: &str, descriptor: &str, name: &str, kind: SymbolKind) -> Symbol {
        Symbol {
            id: GlobalSymbolId {
                scheme: "comind-treesitter".into(),
                package_manager: ".".into(),
                package: pkg.into(),
                version: ".".into(),
                descriptor: descriptor.into(),
            },
            name: name.into(),
            kind,
            language: Language::Python,
            repo: RepoId(pkg.into()),
            file_path: "x.py".into(),
            range: Range {
                start: Position { line: 1, column: 0 },
                end: Position { line: 1, column: 0 },
            },
            signature: None,
            docstring: None,
        }
    }

    fn ext_import(src_pkg: &str, core: &str) -> Edge {
        Edge {
            src: GlobalSymbolId {
                scheme: "comind-treesitter".into(),
                package_manager: ".".into(),
                package: src_pkg.into(),
                version: ".".into(),
                descriptor: "service-a/thing".into(),
            },
            dst: GlobalSymbolId {
                scheme: "comind-treesitter".into(),
                package_manager: ".".into(),
                package: "?".into(),
                version: ".".into(),
                descriptor: core.into(),
            },
            kind: EdgeKind::Imports,
            confidence: 0.5,
            cross_repo: false,
        }
    }

    #[test]
    fn import_binds_cross_repo() {
        let symbols = vec![sym(
            "pkg-common",
            "acme/const/Settings#",
            "Settings",
            SymbolKind::Class,
        )];
        let edges = vec![ext_import("service-a", "acme/const/Settings")];

        let r = resolve(&symbols, &edges);
        assert_eq!(r.stats.resolved_imports, 1);
        assert_eq!(r.stats.cross_repo_edges, 1);
        let e = &r.edges[0];
        assert!(e.cross_repo);
        assert_eq!(e.dst.package, "pkg-common");
        assert_eq!(e.dst.descriptor, "acme/const/Settings#");
    }

    #[test]
    fn unknown_import_is_counted_not_bound() {
        let symbols = vec![sym("pkg-common", "acme/const/X#", "X", SymbolKind::Class)];
        let edges = vec![ext_import("service-a", "numpy/array")];
        let r = resolve(&symbols, &edges);
        assert_eq!(r.stats.resolved_imports, 0);
        assert_eq!(r.stats.unresolved_imports, 1);
        assert!(r.edges.is_empty());
    }
}
