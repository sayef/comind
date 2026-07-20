//! CoMind parse — tree-sitter extraction of symbols + intra-file edges.
//!
//! Phase 1 scope: definitions (functions, methods, classes, interfaces), the `Contains`
//! hierarchy, and provisional `Calls` edges (callee bound by name, confidence < 1.0 —
//! real cross-file/cross-repo binding happens in `comind-resolve`, phase 3).
//!
//! Polyglot by construction: Python + TypeScript today, new languages are a match arm.

use std::path::{Path, MAIN_SEPARATOR};

use anyhow::{Context, Result};
use comind_core::{
    Edge, EdgeKind, GlobalSymbolId, Language, Position, Range, RepoId, Symbol, SymbolKind,
};
use ignore::WalkBuilder;
use rayon::prelude::*;
use tree_sitter::{Node, Parser};

const SCHEME: &str = "comind-treesitter";

/// Symbols and edges extracted from a repo (or a single file).
#[derive(Debug, Default)]
pub struct ParseOutput {
    pub symbols: Vec<Symbol>,
    pub edges: Vec<Edge>,
}

impl ParseOutput {
    fn merge(&mut self, other: ParseOutput) {
        self.symbols.extend(other.symbols);
        self.edges.extend(other.edges);
    }
}

/// Detect language from file extension. Returns `None` for files we don't parse.
fn detect(path: &Path) -> Option<Language> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("py" | "pyi") => Some(Language::Python),
        Some("ts") => Some(Language::TypeScript),
        Some("tsx") => Some(Language::TypeScript), // parsed with the TSX grammar
        Some("js" | "jsx" | "mjs" | "cjs") => Some(Language::JavaScript),
        _ => None,
    }
}

fn tree_sitter_language(lang: &Language, is_tsx: bool) -> Option<tree_sitter::Language> {
    match lang {
        Language::Python => Some(tree_sitter_python::LANGUAGE.into()),
        Language::TypeScript | Language::JavaScript if is_tsx => {
            Some(tree_sitter_typescript::LANGUAGE_TSX.into())
        }
        Language::TypeScript | Language::JavaScript => {
            Some(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
        }
        _ => None,
    }
}

/// Parse every supported source file under `root`, in parallel, respecting `.gitignore`.
pub fn parse_repo(root: &Path, repo_name: &str) -> Result<ParseOutput> {
    let files: Vec<_> = WalkBuilder::new(root)
        .hidden(false)
        .build()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
        .map(|e| e.into_path())
        .filter(|p| detect(p).is_some())
        .collect();

    let out = files
        .par_iter()
        .filter_map(|p| parse_file(p, repo_name, root).ok().flatten())
        .reduce(ParseOutput::default, |mut acc, o| {
            acc.merge(o);
            acc
        });
    Ok(out)
}

/// Parse a specific set of repo-relative files (for incremental indexing). Unsupported or
/// missing files are skipped. Paths are relative to `root`.
pub fn parse_files(root: &Path, repo_name: &str, rel_paths: &[String]) -> ParseOutput {
    rel_paths
        .par_iter()
        .filter_map(|rel| {
            let full = root.join(rel);
            if full.is_file() {
                parse_file(&full, repo_name, root).ok().flatten()
            } else {
                None
            }
        })
        .reduce(ParseOutput::default, |mut acc, o| {
            acc.merge(o);
            acc
        })
}

/// Parse a single file. `Ok(None)` means "unsupported language", not an error.
pub fn parse_file(path: &Path, repo_name: &str, root: &Path) -> Result<Option<ParseOutput>> {
    let Some(lang) = detect(path) else {
        return Ok(None);
    };
    let is_tsx = path.extension().and_then(|e| e.to_str()) == Some("tsx");
    let Some(ts_lang) = tree_sitter_language(&lang, is_tsx) else {
        return Ok(None);
    };

    let source = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let rel = path.strip_prefix(root).unwrap_or(path);
    let rel_str = rel.to_string_lossy().replace(MAIN_SEPARATOR, "/");

    let mut parser = Parser::new();
    parser
        .set_language(&ts_lang)
        .context("set tree-sitter language")?;
    let Some(tree) = parser.parse(&source, None) else {
        return Ok(None);
    };

    let repo = RepoId(repo_name.to_string());
    let module = module_path(&rel_str);

    // The file node is the root of the Contains hierarchy.
    let file_id = id(repo_name, &module);
    let file_symbol = Symbol {
        id: file_id.clone(),
        name: rel_str.clone(),
        kind: SymbolKind::File,
        language: lang.clone(),
        repo: repo.clone(),
        file_path: rel_str.clone(),
        range: node_range(&tree.root_node()),
        signature: None,
        docstring: None,
    };

    let mut out = ParseOutput {
        symbols: vec![file_symbol],
        edges: Vec::new(),
    };

    let ctx = Ctx {
        source: &source,
        lang: &lang,
        repo: &repo,
        rel_str: &rel_str,
        repo_name,
    };
    let scope = Scope {
        core: module,
        id: file_id.clone(),
        enclosing_callable: file_id,
    };
    walk(tree.root_node(), &scope, &ctx, &mut out);
    Ok(Some(out))
}

struct Ctx<'a> {
    source: &'a [u8],
    lang: &'a Language,
    repo: &'a RepoId,
    rel_str: &'a str,
    repo_name: &'a str,
}

#[derive(Clone)]
struct Scope {
    /// Descriptor path without the kind suffix, e.g. `cobrainer/foo/MyClass`.
    core: String,
    /// Id of the symbol that owns this scope (the `Contains` parent).
    id: GlobalSymbolId,
    /// Nearest enclosing function/method (or the file) — the source of `Calls` edges.
    enclosing_callable: GlobalSymbolId,
}

fn walk(node: Node, scope: &Scope, ctx: &Ctx, out: &mut ParseOutput) {
    if let Some(kind) = def_kind(ctx.lang, node.kind()) {
        if let Some(name) = child_text(node, "name", ctx.source) {
            let core = format!("{}/{}", scope.core, name);
            let sym_id = id(ctx.repo_name, &format!("{}{}", core, suffix(&kind)));

            out.symbols.push(Symbol {
                id: sym_id.clone(),
                name: name.to_string(),
                kind: kind.clone(),
                language: ctx.lang.clone(),
                repo: ctx.repo.clone(),
                file_path: ctx.rel_str.to_string(),
                range: node_range(&node),
                signature: Some(first_line(node, ctx.source)),
                docstring: None,
            });
            out.edges.push(edge(&scope.id, &sym_id, EdgeKind::Contains, 1.0));

            let enclosing = if is_callable(&kind) {
                sym_id.clone()
            } else {
                scope.enclosing_callable.clone()
            };
            let child_scope = Scope {
                core,
                id: sym_id,
                enclosing_callable: enclosing,
            };
            for c in children(node) {
                walk(c, &child_scope, ctx, out);
            }
            return;
        }
    }

    if let Some(cores) = import_targets(node, ctx.lang, ctx.source) {
        // Import statement: emit provisional Imports edges (external target, package "?").
        // comind-resolve binds these to real definitions — including cross-repo.
        for core in cores {
            let target = external_id(&core);
            out.edges
                .push(edge(&scope.id, &target, EdgeKind::Imports, 0.5));
        }
        return; // don't descend — imported names are not calls
    }

    if is_call(ctx.lang, node.kind()) {
        if let Some(callee) = callee_name(node, ctx.lang, ctx.source) {
            // Provisional target: name only, unresolved scope. Rebound in comind-resolve.
            let target = id(ctx.repo_name, &format!("?/{}()", callee));
            out.edges
                .push(edge(&scope.enclosing_callable, &target, EdgeKind::Calls, 0.4));
        }
    }

    for c in children(node) {
        walk(c, scope, ctx, out);
    }
}

// ---- language-specific tables -------------------------------------------------------

fn def_kind(lang: &Language, node_kind: &str) -> Option<SymbolKind> {
    match (lang, node_kind) {
        (Language::Python, "function_definition") => Some(SymbolKind::Function),
        (Language::Python, "class_definition") => Some(SymbolKind::Class),
        (Language::TypeScript | Language::JavaScript, "function_declaration") => {
            Some(SymbolKind::Function)
        }
        (Language::TypeScript | Language::JavaScript, "method_definition") => {
            Some(SymbolKind::Method)
        }
        (Language::TypeScript | Language::JavaScript, "class_declaration") => {
            Some(SymbolKind::Class)
        }
        (Language::TypeScript | Language::JavaScript, "interface_declaration") => {
            Some(SymbolKind::Interface)
        }
        _ => None,
    }
}

fn is_call(lang: &Language, node_kind: &str) -> bool {
    matches!(
        (lang, node_kind),
        (Language::Python, "call")
            | (Language::TypeScript | Language::JavaScript, "call_expression")
    )
}

/// Extract import targets as descriptor "cores" (e.g. `cobrainer/const/NamedOwner`).
/// `None` when `node` is not an import statement. Relative imports are skipped (they
/// resolve within the same package, not cross-repo).
fn import_targets(node: Node, lang: &Language, src: &[u8]) -> Option<Vec<String>> {
    if !matches!(lang, Language::Python) {
        return None; // Python carries the cross-repo signal we care about first
    }
    match node.kind() {
        // from <module> import a, b as c
        "import_from_statement" => {
            let module = node.child_by_field_name("module_name")?;
            let mtext = module.utf8_text(src).ok()?;
            if mtext.starts_with('.') {
                return Some(vec![]); // relative import
            }
            let mcore = mtext.replace('.', "/");
            let mut cur = node.walk();
            let cores = node
                .children_by_field_name("name", &mut cur)
                .filter_map(|n| import_name_text(n, src))
                .map(|name| format!("{}/{}", mcore, name.replace('.', "/")))
                .collect();
            Some(cores)
        }
        // import x.y, z as w
        "import_statement" => {
            let mut cur = node.walk();
            let cores = node
                .children_by_field_name("name", &mut cur)
                .filter_map(|n| import_name_text(n, src))
                .filter(|name| !name.starts_with('.'))
                .map(|name| name.replace('.', "/"))
                .collect();
            Some(cores)
        }
        _ => None,
    }
}

fn import_name_text(n: Node, src: &[u8]) -> Option<String> {
    match n.kind() {
        // `a as b` -> the imported name is `a`
        "aliased_import" => n
            .child_by_field_name("name")
            .and_then(|x| x.utf8_text(src).ok())
            .map(String::from),
        _ => n.utf8_text(src).ok().map(String::from),
    }
}

/// Provisional external reference: package `?`, descriptor = the import core. Resolved to a
/// real (possibly cross-repo) definition in comind-resolve.
fn external_id(core: &str) -> GlobalSymbolId {
    GlobalSymbolId {
        scheme: SCHEME.to_string(),
        package_manager: ".".to_string(),
        package: "?".to_string(),
        version: ".".to_string(),
        descriptor: core.to_string(),
    }
}

/// Extract the callee's simple name from a call node.
fn callee_name<'a>(node: Node, lang: &Language, src: &'a [u8]) -> Option<&'a str> {
    let _ = lang;
    let func = node.child_by_field_name("function")?;
    match func.kind() {
        // bare name: foo(...)
        "identifier" => func.utf8_text(src).ok(),
        // Python method call: obj.method(...)
        "attribute" => child_text(func, "attribute", src),
        // TS/JS member call: obj.method(...)
        "member_expression" => child_text(func, "property", src),
        _ => None,
    }
}

// ---- helpers ------------------------------------------------------------------------

/// Python: a `function_definition` is really a method when nested under a class — but we
/// key off `SymbolKind` from the def table, so treat both Function and Method as callable.
fn is_callable(kind: &SymbolKind) -> bool {
    matches!(kind, SymbolKind::Function | SymbolKind::Method)
}

fn suffix(kind: &SymbolKind) -> &'static str {
    match kind {
        SymbolKind::Class
        | SymbolKind::Interface
        | SymbolKind::Struct
        | SymbolKind::Enum
        | SymbolKind::Trait => "#",
        SymbolKind::Function | SymbolKind::Method => "().",
        _ => "",
    }
}

fn id(repo: &str, descriptor: &str) -> GlobalSymbolId {
    GlobalSymbolId {
        scheme: SCHEME.to_string(),
        package_manager: ".".to_string(),
        package: repo.to_string(),
        version: ".".to_string(),
        descriptor: descriptor.to_string(),
    }
}

fn edge(src: &GlobalSymbolId, dst: &GlobalSymbolId, kind: EdgeKind, confidence: f32) -> Edge {
    Edge {
        src: src.clone(),
        dst: dst.clone(),
        kind,
        confidence,
        cross_repo: false,
    }
}

fn module_path(rel: &str) -> String {
    let no_ext = rel.rsplit_once('.').map_or(rel, |(base, _)| base);
    no_ext
        .strip_suffix("/__init__")
        .unwrap_or(no_ext)
        .to_string()
}

fn child_text<'a>(node: Node, field: &str, src: &'a [u8]) -> Option<&'a str> {
    node.child_by_field_name(field)?.utf8_text(src).ok()
}

fn children(node: Node) -> Vec<Node> {
    let mut cursor = node.walk();
    node.children(&mut cursor).collect()
}

fn first_line(node: Node, src: &[u8]) -> String {
    node.utf8_text(src)
        .unwrap_or("")
        .lines()
        .next()
        .unwrap_or("")
        .trim_end()
        .to_string()
}

fn node_range(node: &Node) -> Range {
    let s = node.start_position();
    let e = node.end_position();
    Range {
        start: Position {
            line: s.row as u32 + 1,
            column: s.column as u32,
        },
        end: Position {
            line: e.row as u32 + 1,
            column: e.column as u32,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write(dir: &Path, name: &str, body: &str) {
        let p = dir.join(name);
        let mut f = std::fs::File::create(p).unwrap();
        f.write_all(body.as_bytes()).unwrap();
    }

    #[test]
    fn extracts_python_class_method_and_call() {
        let dir = std::env::temp_dir().join(format!("comind-parse-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        write(
            &dir,
            "svc.py",
            "class Service:\n    def run(self):\n        helper()\n\ndef helper():\n    pass\n",
        );

        let out = parse_repo(&dir, "demo").unwrap();
        let names: Vec<_> = out.symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"Service"), "got {names:?}");
        assert!(names.contains(&"run"), "got {names:?}");
        assert!(names.contains(&"helper"), "got {names:?}");

        // Service --contains--> run
        assert!(out.edges.iter().any(|e| e.kind == EdgeKind::Contains
            && e.src.descriptor.contains("Service#")
            && e.dst.descriptor.contains("run().")));
        // run --calls--> helper (provisional)
        assert!(out.edges.iter().any(|e| e.kind == EdgeKind::Calls
            && e.src.descriptor.contains("run().")
            && e.dst.descriptor.contains("helper")));

        std::fs::remove_dir_all(&dir).ok();
    }
}
