//! Evidence for the LLM style-guide synthesis.
//!
//! Surface stats (naming casing, docstring style, idiom counts) can be *computed*, but the valuable
//! conventions — how the repo uses its libraries, AWS/DB/HTTP I/O, its own shared modules — cannot
//! be derived from statistics; the model has to read real code. So this assembles a bounded
//! **evidence payload**: the dependency stack, an import-frequency histogram, and real code
//! excerpts (a representative call-site per top dependency, plus entrypoint/config/test files),
//! alongside the surface stats. `COMIND_DEBUG_EVIDENCE=1` prints the payload to stderr.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use crate::model::{Edge, EdgeKind, Symbol, SymbolKind};

const MAX_EXCERPTS: usize = 12;
const EXCERPT_LINES: usize = 20;
const CHAR_BUDGET: usize = 16000; // rough cap on the whole payload

pub fn evidence_block(root: &Path, symbols: &[&Symbol], edges: &[Edge]) -> String {
    let mut o = String::new();
    file_tree(&mut o, symbols);
    stack(&mut o, root);
    let hist = import_histogram(&mut o, symbols, edges);
    surface(&mut o, symbols, root);
    excerpts(&mut o, root, symbols, &hist);
    if o.len() > CHAR_BUDGET {
        o.truncate(CHAR_BUDGET);
        o.push_str("\n…(truncated)\n");
    }
    if std::env::var("COMIND_DEBUG_EVIDENCE").is_ok() {
        eprintln!("\n----- evidence payload -----\n{o}\n----- end evidence -----\n");
    }
    o
}

// ---- file tree -----------------------------------------------------------------------------
fn file_tree(o: &mut String, symbols: &[&Symbol]) {
    let mut dirs: BTreeMap<String, usize> = BTreeMap::new();
    for s in symbols
        .iter()
        .filter(|s| matches!(s.kind, SymbolKind::File))
    {
        let top = s.file_path.split('/').next().unwrap_or(".").to_string();
        let key = if s.file_path.contains('/') {
            format!("{top}/")
        } else {
            "(root)".into()
        };
        *dirs.entry(key).or_default() += 1;
    }
    let _ = writeln!(o, "## Layout (top-level dirs → file count)");
    for (d, n) in dirs.iter() {
        let _ = writeln!(o, "- {d}  {n}");
    }
    o.push('\n');
}

// ---- dependency stack ----------------------------------------------------------------------
fn stack(o: &mut String, root: &Path) {
    let read = |n: &str| std::fs::read_to_string(root.join(n)).ok();
    let mut deps: Vec<String> = Vec::new();

    if let Some(txt) = read("pyproject.toml") {
        if let Ok(v) = txt.parse::<toml::Value>() {
            if let Some(arr) = v
                .get("project")
                .and_then(|p| p.get("dependencies"))
                .and_then(|d| d.as_array())
            {
                for d in arr {
                    if let Some(s) = d.as_str() {
                        deps.push(s.to_string());
                    }
                }
            }
            if let Some(tbl) = v
                .get("tool")
                .and_then(|t| t.get("poetry"))
                .and_then(|p| p.get("dependencies"))
                .and_then(|d| d.as_table())
            {
                for (k, val) in tbl {
                    if k != "python" {
                        deps.push(format!("{k} {}", val.as_str().unwrap_or("")));
                    }
                }
            }
        }
    }
    if deps.is_empty() {
        if let Some(txt) = read("requirements.txt") {
            for l in txt.lines() {
                let l = l.trim();
                if !l.is_empty() && !l.starts_with('#') {
                    deps.push(l.to_string());
                }
            }
        }
    }
    if let Some(txt) = read("package.json") {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&txt) {
            if let Some(obj) = v.get("dependencies").and_then(|d| d.as_object()) {
                for (k, ver) in obj {
                    deps.push(format!("{k} {}", ver.as_str().unwrap_or("")));
                }
            }
        }
    }
    if !deps.is_empty() {
        let _ = writeln!(o, "## Declared dependencies");
        for d in deps.iter().take(40) {
            let _ = writeln!(o, "- {}", d.trim());
        }
        o.push('\n');
    }
}

// ---- import histogram + call-site targets --------------------------------------------------
/// Returns ranked `(module, importing_file_paths)` for third-party imports, most-used first.
fn import_histogram(
    o: &mut String,
    symbols: &[&Symbol],
    edges: &[Edge],
) -> Vec<(String, Vec<String>)> {
    let id2file: BTreeMap<String, String> = symbols
        .iter()
        .map(|s| (s.id.render(), s.file_path.clone()))
        .collect();
    // module -> set of importing files
    let mut mods: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for e in edges.iter().filter(|e| e.kind == EdgeKind::Imports) {
        let module = e.dst.descriptor.split('/').next().unwrap_or("").to_string();
        if module.is_empty() {
            continue;
        }
        let file = id2file.get(&e.src.render()).cloned().unwrap_or_default();
        mods.entry(module).or_default().insert(file);
    }
    let mut ranked: Vec<(String, Vec<String>)> = mods
        .into_iter()
        .map(|(m, fs)| {
            (
                m,
                fs.into_iter().filter(|f| !f.is_empty()).collect::<Vec<_>>(),
            )
        })
        .collect();
    ranked.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(&b.0)));
    if !ranked.is_empty() {
        let _ = writeln!(o, "## Most-used imports (module → #files)");
        for (m, fs) in ranked.iter().take(18) {
            let _ = writeln!(o, "- {m}  ({} files)", fs.len());
        }
        o.push('\n');
    }
    ranked
}

// ---- surface stats (condensed) -------------------------------------------------------------
fn casing(name: &str) -> &'static str {
    let core = name.trim_matches('_');
    if core.is_empty() {
        return "other";
    }
    let up = core.chars().any(|c| c.is_ascii_uppercase());
    let lo = core.chars().any(|c| c.is_ascii_lowercase());
    let us = core.contains('_');
    if !lo && up {
        return "SCREAMING_SNAKE";
    }
    if us && lo {
        return "snake_case";
    }
    let f = core.chars().next().unwrap();
    if f.is_ascii_uppercase() && lo {
        return "PascalCase";
    }
    if f.is_ascii_lowercase() && up {
        return "camelCase";
    }
    "snake_case"
}

fn surface(o: &mut String, symbols: &[&Symbol], root: &Path) {
    let dom = |kinds: &[SymbolKind]| -> Option<(String, usize, usize)> {
        let mut h: BTreeMap<&str, usize> = BTreeMap::new();
        let mut n = 0;
        for s in symbols.iter().filter(|s| kinds.contains(&s.kind)) {
            *h.entry(casing(&s.name)).or_default() += 1;
            n += 1;
        }
        h.into_iter()
            .max_by_key(|(_, c)| *c)
            .map(|(k, c)| (k.to_string(), c, n))
    };
    let _ = writeln!(o, "## Surface stats (measured)");
    if let Some((k, c, n)) = dom(&[SymbolKind::Function, SymbolKind::Method]) {
        let _ = writeln!(o, "- functions/methods: {k} {}/{n}", c);
    }
    if let Some((k, c, n)) = dom(&[SymbolKind::Class, SymbolKind::Interface]) {
        let _ = writeln!(o, "- classes: {k} {}/{n}", c);
    }
    // one cheap idiom + docstring scan over a sample of files
    let mut files: Vec<&str> = symbols
        .iter()
        .filter(|s| matches!(s.kind, SymbolKind::File))
        .map(|s| s.file_path.as_str())
        .collect();
    files.sort();
    files.dedup();
    let (mut logger, mut prints, mut fstr, mut fmt, mut bare_exc, mut broad_exc) =
        (0, 0, 0, 0, 0, 0);
    let (mut g, mut numpy, mut rest) = (0, 0, 0);
    for f in files.iter().take(400) {
        if let Ok(src) = std::fs::read_to_string(root.join(f)) {
            logger += src.matches("logger.").count() + src.matches("getLogger").count();
            prints += src.matches("print(").count();
            fstr += src.matches("f\"").count() + src.matches("f'").count();
            fmt += src.matches(".format(").count();
            bare_exc += src.matches("except:").count();
            broad_exc += src.matches("except Exception").count();
            if src.contains("Args:") || src.contains("Returns:") {
                g += 1;
            }
            if src.contains("Parameters\n") {
                numpy += 1;
            }
            if src.contains(":param ") {
                rest += 1;
            }
        }
    }
    let style = [("Google", g), ("NumPy", numpy), ("reST", rest)]
        .into_iter()
        .max_by_key(|(_, n)| *n)
        .filter(|(_, n)| *n > 0)
        .map(|(k, _)| k)
        .unwrap_or("none/mixed");
    let _ = writeln!(o, "- docstring style (dominant): {style}");
    let _ = writeln!(o, "- logging: logger×{logger} vs print×{prints}; strings: f-string×{fstr} vs .format×{fmt}; except: bare×{bare_exc}, broad×{broad_exc}");
    o.push('\n');
}

// ---- real code excerpts --------------------------------------------------------------------
fn read_excerpt(root: &Path, file: &str, lines: usize) -> Option<String> {
    let src = std::fs::read_to_string(root.join(file)).ok()?;
    let head: Vec<&str> = src.lines().take(lines).collect();
    if head.iter().all(|l| l.trim().is_empty()) {
        return None;
    }
    Some(head.join("\n"))
}

fn excerpts(o: &mut String, root: &Path, symbols: &[&Symbol], hist: &[(String, Vec<String>)]) {
    let mut picked: Vec<String> = Vec::new();
    let push_file = |picked: &mut Vec<String>, f: &str| {
        if !f.is_empty() && !picked.iter().any(|p| p == f) && picked.len() < MAX_EXCERPTS {
            picked.push(f.to_string());
        }
    };

    // 1) one representative call-site file per top third-party module
    for (_m, files) in hist.iter().take(8) {
        if let Some(f) = files.first() {
            push_file(&mut picked, f);
        }
    }
    // 2) notable files by name (entrypoints / infra / config / layers)
    let all_files: BTreeSet<&str> = symbols
        .iter()
        .filter(|s| matches!(s.kind, SymbolKind::File))
        .map(|s| s.file_path.as_str())
        .collect();
    let hints = [
        "main.py",
        "app.py",
        "cli.py",
        "settings.py",
        "config.py",
        "client",
        "handler",
        "service",
        "repository",
        "conftest.py",
    ];
    for f in &all_files {
        let low = f.to_lowercase();
        if hints
            .iter()
            .any(|h| low.ends_with(h) || low.contains(&format!("/{h}")) || low.contains(h))
        {
            push_file(&mut picked, f);
        }
    }
    // 3) one test file
    if let Some(t) = all_files.iter().find(|f| {
        let l = f.to_lowercase();
        l.contains("test_") || l.ends_with("_test.py") || l.contains("/tests/")
    }) {
        push_file(&mut picked, t);
    }

    if picked.is_empty() {
        return;
    }
    let _ = writeln!(
        o,
        "## Representative code excerpts (infer library/infra/architecture conventions from these)"
    );
    for f in &picked {
        if let Some(ex) = read_excerpt(root, f, EXCERPT_LINES) {
            let _ = writeln!(o, "\n### {f}\n```\n{ex}\n```");
        }
    }
    o.push('\n');
}
