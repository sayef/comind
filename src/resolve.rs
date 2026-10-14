//! Comind resolve — bind per-file facts to definitions, within and across repos.
//!
//! A reference path (`self.api.get`, `events.publish`, `kit.formatDate`) is resolved segment by
//! segment through imports, re-exports, modules, classes (incl. bases) and typed variables.
//! Emits `Imports`, `Calls`, `Instantiates`, `Inherits` and `Implements` edges.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use crate::facts::{FileFacts, Import, ImportItem, RefKind, TypeSource};
use crate::model::{Edge, EdgeKind, Language, Symbol, SymbolKind};
use crate::packages::strip_ts_ext;

/// Guard against re-export / alias cycles.
const MAX_DEPTH: u32 = 12;

#[derive(Debug, Default)]
pub struct Resolved {
    pub edges: Vec<Edge>,
    pub stats: ResolveStats,
    /// Top-level module → count of import items that didn't resolve.
    pub unresolved_modules: HashMap<String, usize>,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ResolveStats {
    pub resolved_imports: usize,
    /// Imports whose target is outside the corpus (mostly third-party).
    pub unresolved_imports: usize,
    /// Calls and instantiations bound through scopes, imports and types.
    pub resolved_calls: usize,
    pub unresolved_calls: usize,
    /// `Inherits` + `Implements`.
    pub type_edges: usize,
    pub cross_repo_edges: usize,
}

#[derive(Debug, Clone, PartialEq)]
enum Value {
    File(String),
    /// A Python module by dotted name (a namespace package may have no file).
    Module(String),
    Sym(String),
    Instance(String),
}

/// Resolve the merged corpus. Input edges pass through; new edges come from `facts`.
pub fn resolve(symbols: &[Symbol], edges: &[Edge], facts: &[FileFacts]) -> Resolved {
    let b = Binder::new(symbols, edges, facts);
    let mut out = Resolved {
        edges: edges.to_vec(),
        ..Default::default()
    };
    let mut new_edges: Vec<(String, String, EdgeKind, f32)> = Vec::new();

    // Bases first: method lookup walks them.
    for f in facts {
        for r in &f.refs {
            if !matches!(r.kind, RefKind::Inherits | RefKind::Implements) {
                continue;
            }
            let scope = b.parent.get(&r.from).cloned().unwrap_or(f.file_id.clone());
            let Some(Value::Sym(t)) = b.resolve_path(&r.path, &scope, f, 0) else {
                continue;
            };
            if !b.is_type(&t) {
                continue;
            }
            b.bases
                .borrow_mut()
                .entry(r.from.clone())
                .or_default()
                .push(t.clone());
            let kind = if r.kind == RefKind::Inherits {
                EdgeKind::Inherits
            } else {
                EdgeKind::Implements
            };
            new_edges.push((r.from.clone(), t, kind, 0.9));
            out.stats.type_edges += 1;
        }
    }

    for f in facts {
        for imp in &f.imports {
            let base = b.import_target(f, imp);
            for item in &imp.items {
                let target = match item {
                    ImportItem::Name { name, .. } => {
                        base.as_ref().and_then(|v| b.member(v, name, 0))
                    }
                    ImportItem::Module { .. } | ImportItem::All => base.clone(),
                };
                match target.and_then(|v| b.node_of(&v, &f.repo)) {
                    Some(t) if t != f.file_id => {
                        new_edges.push((f.file_id.clone(), t, EdgeKind::Imports, 0.95));
                        out.stats.resolved_imports += 1;
                    }
                    Some(_) => {}
                    None => {
                        out.stats.unresolved_imports += 1;
                        *out.unresolved_modules
                            .entry(top_module(f, imp))
                            .or_default() += 1;
                    }
                }
            }
        }

        for r in &f.refs {
            if !matches!(r.kind, RefKind::Call | RefKind::New) {
                continue;
            }
            let Some(Value::Sym(t)) = b.resolve_path(&r.path, &r.from, f, 0) else {
                out.stats.unresolved_calls += 1;
                continue;
            };
            let kind = if b.is_type(&t) {
                EdgeKind::Instantiates
            } else if b.is_callable(&t) {
                EdgeKind::Calls
            } else {
                out.stats.unresolved_calls += 1;
                continue;
            };
            out.stats.resolved_calls += 1;
            new_edges.push((r.from.clone(), t, kind, 0.9));
        }
    }

    let mut seen: HashSet<(String, String, EdgeKind)> = out
        .edges
        .iter()
        .map(|e| (e.src.render(), e.dst.render(), e.kind.clone()))
        .collect();
    for (src, dst, kind, confidence) in new_edges {
        let (Some(s), Some(d)) = (b.sym(&src), b.sym(&dst)) else {
            continue;
        };
        if !seen.insert((src, dst, kind.clone())) {
            continue;
        }
        let cross_repo = s.repo != d.repo;
        if cross_repo {
            out.stats.cross_repo_edges += 1;
        }
        out.edges.push(Edge {
            src: s.id.clone(),
            dst: d.id.clone(),
            kind,
            confidence,
            cross_repo,
        });
    }
    out
}

struct Binder<'a> {
    syms: HashMap<String, &'a Symbol>,
    parent: HashMap<String, String>,
    /// Parent id → child name → child id.
    members: HashMap<String, HashMap<&'a str, String>>,
    facts: HashMap<&'a str, &'a FileFacts>,
    py_modules: HashMap<&'a str, Vec<&'a FileFacts>>,
    /// Every dotted prefix of a Python module name (namespace packages).
    py_packages: HashSet<String>,
    js_modules: HashMap<&'a str, Vec<&'a FileFacts>>,
    by_path: HashMap<(&'a str, &'a str), &'a FileFacts>,
    types: HashMap<(&'a str, &'a str), &'a TypeSource>,
    bases: RefCell<HashMap<String, Vec<String>>>,
}

impl<'a> Binder<'a> {
    fn new(symbols: &'a [Symbol], edges: &[Edge], facts: &'a [FileFacts]) -> Self {
        let syms: HashMap<String, &Symbol> = symbols.iter().map(|s| (s.id.render(), s)).collect();
        let mut parent = HashMap::new();
        let mut members: HashMap<String, HashMap<&str, String>> = HashMap::new();
        for e in edges.iter().filter(|e| e.kind == EdgeKind::Contains) {
            let (p, c) = (e.src.render(), e.dst.render());
            if let Some(child) = syms.get(&c) {
                members
                    .entry(p.clone())
                    .or_default()
                    .insert(child.name.as_str(), c.clone());
            }
            parent.insert(c, p);
        }
        let mut b = Binder {
            syms,
            parent,
            members,
            facts: HashMap::new(),
            py_modules: HashMap::new(),
            py_packages: HashSet::new(),
            js_modules: HashMap::new(),
            by_path: HashMap::new(),
            types: HashMap::new(),
            bases: RefCell::new(HashMap::new()),
        };
        for f in facts {
            b.facts.insert(f.file_id.as_str(), f);
            b.by_path.insert((f.repo.as_str(), f.path.as_str()), f);
            for m in &f.modules {
                if matches!(f.language, Language::Python) {
                    b.py_modules.entry(m.as_str()).or_default().push(f);
                    let mut prefix = String::new();
                    for part in m.split('.') {
                        if !prefix.is_empty() {
                            prefix.push('.');
                        }
                        prefix.push_str(part);
                        b.py_packages.insert(prefix.clone());
                    }
                } else {
                    b.js_modules.entry(m.as_str()).or_default().push(f);
                }
            }
            for t in &f.types {
                b.types
                    .entry((t.scope.as_str(), t.name.as_str()))
                    .or_insert(&t.source);
            }
        }
        b
    }

    fn sym(&self, id: &str) -> Option<&'a Symbol> {
        self.syms.get(id).copied()
    }

    fn is_type(&self, id: &str) -> bool {
        self.sym(id).is_some_and(|s| {
            matches!(
                s.kind,
                SymbolKind::Class | SymbolKind::Interface | SymbolKind::Struct | SymbolKind::Trait
            )
        })
    }

    fn is_callable(&self, id: &str) -> bool {
        self.sym(id)
            .is_some_and(|s| matches!(s.kind, SymbolKind::Function | SymbolKind::Method))
    }

    /// Prefer a definition in the importing repo; otherwise a unique one elsewhere.
    fn pick(&self, cands: Option<&Vec<&'a FileFacts>>, repo: &str) -> Option<&'a FileFacts> {
        let cands = cands?;
        cands
            .iter()
            .find(|f| f.repo == repo)
            .or_else(|| (cands.len() == 1).then(|| &cands[0]))
            .copied()
    }

    fn py_file(&self, module: &str, repo: &str) -> Option<&'a FileFacts> {
        self.pick(self.py_modules.get(module), repo)
    }

    /// What an import statement's module refers to.
    fn import_target(&self, f: &FileFacts, imp: &Import) -> Option<Value> {
        match f.language {
            Language::Python => {
                if imp.level == 0 {
                    return Some(Value::Module(imp.module.clone()));
                }
                let own = f.modules.first()?;
                let mut parts: Vec<&str> = own.split('.').collect();
                let is_pkg = f.path.ends_with("__init__.py") || f.path.ends_with("__init__.pyi");
                let drop = imp.level as usize - usize::from(is_pkg);
                parts.truncate(parts.len().checked_sub(drop)?);
                if !imp.module.is_empty() {
                    parts.push(&imp.module);
                }
                Some(Value::Module(parts.join(".")))
            }
            _ if imp.file.is_some() => self
                .by_path
                .get(&(f.repo.as_str(), imp.file.as_deref()?))
                .map(|t| Value::File(t.file_id.clone())),
            _ if imp.module.starts_with('.') => {
                let dir = f.path.rsplit_once('/').map_or("", |(d, _)| d);
                let base = normalize(dir, &imp.module)?;
                let stem = strip_ts_ext(&base);
                [
                    ".ts",
                    ".tsx",
                    ".d.ts",
                    ".js",
                    ".jsx",
                    ".mjs",
                    "/index.ts",
                    "/index.tsx",
                    "/index.js",
                ]
                .iter()
                .find_map(|e| {
                    let path = format!("{stem}{e}");
                    self.by_path
                        .get(&(f.repo.as_str(), path.as_str()))
                        .map(|t| Value::File(t.file_id.clone()))
                })
            }
            _ => self
                .pick(self.js_modules.get(imp.module.as_str()), &f.repo)
                .map(|t| Value::File(t.file_id.clone())),
        }
    }

    /// The graph node a value denotes (file or symbol id).
    fn node_of(&self, v: &Value, repo: &str) -> Option<String> {
        match v {
            Value::File(id) | Value::Sym(id) => Some(id.clone()),
            Value::Module(m) => self.py_file(m, repo).map(|f| f.file_id.clone()),
            Value::Instance(_) => None,
        }
    }

    fn member(&self, v: &Value, name: &str, depth: u32) -> Option<Value> {
        if depth > MAX_DEPTH {
            return None;
        }
        match v {
            Value::File(f) => self.module_member(f, name, depth + 1),
            Value::Module(m) => {
                let file = self.py_modules.get(m.as_str()).and_then(|c| c.first());
                if let Some(x) = file.and_then(|f| self.module_member(&f.file_id, name, depth + 1))
                {
                    return Some(x);
                }
                let sub = format!("{m}.{name}");
                self.py_packages
                    .contains(&sub)
                    .then_some(Value::Module(sub))
            }
            Value::Sym(id) if self.is_type(id) => self.class_member(id, name, depth + 1),
            // A module variable: its members come from its inferred type.
            Value::Sym(id) => {
                let var = self
                    .sym(id)
                    .filter(|s| matches!(s.kind, SymbolKind::Variable | SymbolKind::Constant))?;
                let scope = self.parent.get(id)?;
                let src = self.types.get(&(scope.as_str(), var.name.as_str()))?;
                let inst = self.type_of(src, scope, depth + 1)?;
                self.member(&inst, name, depth + 1)
            }
            Value::Instance(c) => {
                for recv in ["self", "this"] {
                    let attr = format!("{recv}.{name}");
                    if let Some(src) = self.types.get(&(c.as_str(), attr.as_str())) {
                        return self.type_of(src, c, depth + 1);
                    }
                }
                self.class_member(c, name, depth + 1)
            }
        }
    }

    /// A name in a module's namespace: its definitions, then what its imports bind.
    fn module_member(&self, file: &str, name: &str, depth: u32) -> Option<Value> {
        if depth > MAX_DEPTH {
            return None;
        }
        if let Some(id) = self.members.get(file).and_then(|m| m.get(name)) {
            return Some(Value::Sym(id.clone()));
        }
        let f = self.facts.get(file)?;
        for imp in f.imports.iter().rev() {
            for item in &imp.items {
                let hit = match item {
                    ImportItem::Name { name: n, alias } => {
                        if alias.as_deref().unwrap_or(n) != name {
                            continue;
                        }
                        self.import_target(f, imp)
                            .and_then(|t| self.member(&t, n, depth + 1))
                    }
                    ImportItem::Module { alias: Some(a) } if a == name => {
                        self.import_target(f, imp)
                    }
                    // `import a.b` binds `a`.
                    ImportItem::Module { alias: None }
                        if matches!(f.language, Language::Python)
                            && imp.module.split('.').next() == Some(name) =>
                    {
                        Some(Value::Module(name.to_string()))
                    }
                    ImportItem::All => self
                        .import_target(f, imp)
                        .and_then(|t| self.member(&t, name, depth + 1)),
                    _ => continue,
                };
                if hit.is_some() {
                    return hit;
                }
            }
        }
        None
    }

    fn class_member(&self, class: &str, name: &str, depth: u32) -> Option<Value> {
        if depth > MAX_DEPTH {
            return None;
        }
        if let Some(id) = self.members.get(class).and_then(|m| m.get(name)) {
            return Some(Value::Sym(id.clone()));
        }
        let bases = self.bases.borrow().get(class).cloned().unwrap_or_default();
        bases
            .iter()
            .find_map(|b| self.class_member(b, name, depth + 1))
    }

    /// The instance type a hint denotes, read in `scope`.
    fn type_of(&self, src: &TypeSource, scope: &str, depth: u32) -> Option<Value> {
        if depth > MAX_DEPTH {
            return None;
        }
        match src {
            TypeSource::Annotation(p) | TypeSource::Construct(p) => {
                let f = self.file_facts_of(scope)?;
                match self.resolve_path(p, scope, f, depth + 1)? {
                    Value::Sym(t) if self.is_type(&t) => Some(Value::Instance(t)),
                    _ => None,
                }
            }
            TypeSource::Alias { name, scope: s } => {
                let f = self.file_facts_of(s)?;
                match self.resolve_name(name, s, f, depth + 1)? {
                    v @ Value::Instance(_) => Some(v),
                    _ => None,
                }
            }
        }
    }

    fn file_facts_of(&self, id: &str) -> Option<&'a FileFacts> {
        let mut cur = id;
        loop {
            if let Some(f) = self.facts.get(cur) {
                return Some(f);
            }
            cur = self.parent.get(cur)?;
        }
    }

    /// Scopes from `scope` outward to (and including) the file.
    fn scope_chain(&self, scope: &str) -> Vec<String> {
        let mut out = vec![scope.to_string()];
        let mut cur = scope;
        while let Some(p) = self.parent.get(cur) {
            out.push(p.clone());
            cur = p;
        }
        out
    }

    fn enclosing_class(&self, scope: &str) -> Option<String> {
        self.scope_chain(scope)
            .into_iter()
            .find(|s| self.is_type(s))
    }

    fn resolve_name(&self, head: &str, scope: &str, f: &FileFacts, depth: u32) -> Option<Value> {
        if head == "self" || head == "this" {
            return self.enclosing_class(scope).map(Value::Instance);
        }
        for s in self.scope_chain(scope) {
            if let Some(src) = self.types.get(&(s.as_str(), head)) {
                return self.type_of(src, &s, depth + 1);
            }
            // Functions nested in an enclosing callable (closures).
            if self.is_callable(&s) {
                if let Some(id) = self.members.get(&s).and_then(|m| m.get(head)) {
                    return Some(Value::Sym(id.clone()));
                }
            }
        }
        self.module_member(&f.file_id, head, depth + 1)
    }

    fn resolve_path(
        &self,
        path: &[String],
        scope: &str,
        f: &FileFacts,
        depth: u32,
    ) -> Option<Value> {
        let (head, rest) = path.split_first()?;
        let mut v = self.resolve_name(head, scope, f, depth)?;
        for seg in rest {
            v = self.member(&v, seg, depth)?;
        }
        Some(v)
    }
}

/// The package an import names: `fastapi`, `@scope/pkg`, or `./rel` for relative imports.
fn top_module(f: &FileFacts, imp: &Import) -> String {
    if imp.level > 0 || imp.module.starts_with('.') {
        return format!("(relative in {})", f.repo);
    }
    let mut parts = imp.module.split(['.', '/']);
    let first = parts.next().unwrap_or_default();
    match (first.starts_with('@'), parts.next()) {
        (true, Some(second)) => format!("{first}/{second}"),
        _ => first.to_string(),
    }
}

/// Join a relative module specifier onto a directory, resolving `.` and `..`.
fn normalize(dir: &str, spec: &str) -> Option<String> {
    let mut parts: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
    for seg in spec.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    Some(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_relative_specifiers() {
        assert_eq!(normalize("src/a", "./b").as_deref(), Some("src/a/b"));
        assert_eq!(normalize("src/a", "../c/d").as_deref(), Some("src/c/d"));
        assert_eq!(normalize("", "./x").as_deref(), Some("x"));
        assert_eq!(normalize("", "../x"), None);
    }
}
