//! Evidence-based style-guide inputs.
//!
//! Produces a compact, deterministic **evidence pack** for one repo — measured naming/size stats
//! from the parsed symbols plus a light source line-scan for idioms/docstrings/typing — and
//! extracts **enforced-convention facts** from the repo's tooling config (editorconfig, ruff/black,
//! pre-commit, prettier, tsconfig, Cargo/rustfmt, version pins). The rendered markdown grounds the
//! LLM synthesis in counts + `file:line` examples rather than generic guesses.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use crate::model::{Symbol, SymbolKind};

/// Build the full evidence block (measured stats + enforced-config facts) for `repo`, whose files
/// live under `root`. `symbols` are that repo's symbols. Returns markdown to feed the LLM.
pub fn evidence_block(root: &Path, symbols: &[&Symbol]) -> String {
    let mut o = String::new();
    naming_section(&mut o, symbols);
    size_section(&mut o, symbols);
    scan_section(&mut o, root, symbols);
    config_section(&mut o, root);
    o
}

// ---- naming ---------------------------------------------------------------------------------

fn kind_label(k: &SymbolKind) -> Option<&'static str> {
    match k {
        SymbolKind::Function => Some("functions"),
        SymbolKind::Method => Some("methods"),
        SymbolKind::Class => Some("classes"),
        SymbolKind::Interface => Some("interfaces"),
        _ => None,
    }
}

fn casing(name: &str) -> &'static str {
    let core = name.trim_matches('_');
    if core.is_empty() {
        return "other";
    }
    if core.contains('-') {
        return "kebab-case";
    }
    let has_upper = core.chars().any(|c| c.is_ascii_uppercase());
    let has_lower = core.chars().any(|c| c.is_ascii_lowercase());
    let has_us = core.contains('_');
    if !has_lower && has_upper {
        return "SCREAMING_SNAKE";
    }
    if has_us && has_lower {
        return "snake_case";
    }
    let first = core.chars().next().unwrap();
    if first.is_ascii_uppercase() && has_lower {
        return "PascalCase";
    }
    if first.is_ascii_lowercase() && has_upper {
        return "camelCase";
    }
    if !has_upper {
        return "snake_case"; // single lowercase word
    }
    "other"
}

fn naming_section(o: &mut String, symbols: &[&Symbol]) {
    let _ = writeln!(o, "## Naming (measured)");
    let mut groups: BTreeMap<&str, Vec<&Symbol>> = BTreeMap::new();
    for s in symbols {
        if let Some(label) = kind_label(&s.kind) {
            groups.entry(label).or_default().push(s);
        }
    }
    if groups.is_empty() {
        let _ = writeln!(o, "- (no named symbols)\n");
        return;
    }
    for (label, syms) in &groups {
        let mut hist: BTreeMap<&str, usize> = BTreeMap::new();
        for s in syms {
            *hist.entry(casing(&s.name)).or_default() += 1;
        }
        let total = syms.len();
        let (dom, dom_n) = hist
            .iter()
            .max_by_key(|(_, n)| **n)
            .map(|(k, n)| (*k, *n))
            .unwrap_or(("other", 0));
        let pct = (dom_n as f64 / total as f64 * 100.0).round() as u64;
        // Private prefix ratio (leading underscore).
        let priv_n = syms.iter().filter(|s| s.name.starts_with('_')).count();
        let counter: Vec<String> = syms
            .iter()
            .filter(|s| casing(&s.name) != dom)
            .take(4)
            .map(|s| format!("{}@{}:{}", s.name, s.file_path, s.range.start.line))
            .collect();
        let _ = write!(o, "- {label}: {dom} {pct}% ({dom_n}/{total})");
        if priv_n > 0 {
            let _ = write!(o, "; leading-underscore {priv_n}/{total}");
        }
        if !counter.is_empty() {
            let _ = write!(o, "; counter: {}", counter.join(", "));
        }
        o.push('\n');
    }
    o.push('\n');
}

// ---- size -----------------------------------------------------------------------------------

fn pct(mut v: Vec<u32>, p: f64) -> u32 {
    if v.is_empty() {
        return 0;
    }
    v.sort_unstable();
    let idx = ((v.len() as f64 - 1.0) * p).round() as usize;
    v[idx]
}

fn size_section(o: &mut String, symbols: &[&Symbol]) {
    let fn_lens: Vec<u32> = symbols
        .iter()
        .filter(|s| matches!(s.kind, SymbolKind::Function | SymbolKind::Method))
        .map(|s| s.range.end.line.saturating_sub(s.range.start.line) + 1)
        .collect();
    let file_lens: Vec<u32> = symbols
        .iter()
        .filter(|s| matches!(s.kind, SymbolKind::File))
        .map(|s| s.range.end.line)
        .collect();
    let _ = writeln!(o, "## Size (measured)");
    if !fn_lens.is_empty() {
        let _ = writeln!(
            o,
            "- function length (lines): median {}, p95 {}",
            pct(fn_lens.clone(), 0.5),
            pct(fn_lens, 0.95)
        );
    }
    if !file_lens.is_empty() {
        let _ = writeln!(
            o,
            "- file length (lines): median {}, p95 {}",
            pct(file_lens.clone(), 0.5),
            pct(file_lens, 0.95)
        );
    }
    o.push('\n');
}

// ---- source line-scan (docstrings, typing, idioms) ------------------------------------------

#[derive(Default)]
struct Scan {
    // docstrings
    doc_public: usize,
    doc_documented: usize,
    doc_google: usize,
    doc_numpy: usize,
    doc_rest: usize,
    doc_jsdoc: usize,
    // python typing (single-line signatures)
    py_sig: usize,
    py_ret_annot: usize,
    // idiom counts
    bare_except: usize,
    except_exception: usize,
    print_calls: usize,
    logger: usize,
    fstring: usize,
    format_call: usize,
    async_def: usize,
    dataclass: usize,
    pydantic: usize,
    // ts idioms
    ts_interface: usize,
    ts_type_alias: usize,
    ts_enum: usize,
    console_log: usize,
    any_type: usize,
}

fn is_py(path: &str) -> bool {
    path.ends_with(".py")
}
fn is_ts(path: &str) -> bool {
    path.ends_with(".ts")
        || path.ends_with(".tsx")
        || path.ends_with(".js")
        || path.ends_with(".jsx")
}

fn scan_section(o: &mut String, root: &Path, symbols: &[&Symbol]) {
    // Unique files with their symbols.
    let mut by_file: BTreeMap<&str, Vec<&Symbol>> = BTreeMap::new();
    for s in symbols {
        by_file.entry(s.file_path.as_str()).or_default().push(s);
    }
    let mut sc = Scan::default();
    for (path, syms) in &by_file {
        let Ok(src) = std::fs::read_to_string(root.join(path)) else {
            continue;
        };
        let lines: Vec<&str> = src.lines().collect();
        // idiom counts over the file
        for l in &lines {
            let t = l.trim_start();
            if is_py(path) {
                if t.starts_with("except:") || t == "except:" {
                    sc.bare_except += 1;
                } else if t.starts_with("except Exception") {
                    sc.except_exception += 1;
                }
                if t.starts_with("print(") {
                    sc.print_calls += 1;
                }
                if t.contains("logging.") || t.contains("logger.") || t.contains("getLogger") {
                    sc.logger += 1;
                }
                if t.contains("f\"") || t.contains("f'") {
                    sc.fstring += 1;
                }
                if t.contains(".format(") {
                    sc.format_call += 1;
                }
                if t.starts_with("async def") {
                    sc.async_def += 1;
                }
                if t.starts_with("@dataclass") {
                    sc.dataclass += 1;
                }
                if t.contains("BaseModel") {
                    sc.pydantic += 1;
                }
            } else if is_ts(path) {
                if t.starts_with("interface ") {
                    sc.ts_interface += 1;
                }
                if t.starts_with("type ") && t.contains('=') {
                    sc.ts_type_alias += 1;
                }
                if t.starts_with("enum ") || t.starts_with("export enum ") {
                    sc.ts_enum += 1;
                }
                if t.contains("console.log") {
                    sc.console_log += 1;
                }
                if t.contains(": any") {
                    sc.any_type += 1;
                }
            }
        }
        // per-symbol docstrings + python typing
        for s in syms {
            match s.kind {
                SymbolKind::Function | SymbolKind::Method | SymbolKind::Class => {}
                _ => continue,
            }
            let public = !s.name.starts_with('_');
            if public {
                sc.doc_public += 1;
            }
            let start = s.range.start.line as usize; // 1-based
            if is_py(path) {
                // docstring = one of the ~3 lines after the def/class header starts with a quote
                let mut documented = false;
                for l in lines.iter().skip(start).take(3) {
                    let t = l.trim_start();
                    if t.starts_with("\"\"\"") || t.starts_with("'''") || t.starts_with("r\"\"\"") {
                        documented = true;
                        break;
                    }
                    if !t.is_empty() && !t.starts_with('#') {
                        break;
                    }
                }
                if documented && public {
                    sc.doc_documented += 1;
                }
                // typing from single-line signature
                if let Some(sig) = &s.signature {
                    if sig.contains('(') && sig.trim_end().ends_with(':') {
                        sc.py_sig += 1;
                        if sig.contains("->") {
                            sc.py_ret_annot += 1;
                        }
                    }
                }
            } else if is_ts(path) && start >= 1 {
                // JSDoc block ends on the line above the symbol
                if let Some(prev) = lines.get(start - 2) {
                    if prev.trim_start().starts_with("*/") || prev.trim_start().starts_with("/**") {
                        if public {
                            sc.doc_documented += 1;
                        }
                        sc.doc_jsdoc += 1;
                    }
                }
            }
        }
        // docstring style detection over the whole file
        if src.contains("Args:") || src.contains("Returns:") || src.contains("Raises:") {
            sc.doc_google += 1;
        }
        if src.contains("Parameters\n") && src.contains("----------") {
            sc.doc_numpy += 1;
        }
        if src.contains(":param ") || src.contains(":returns:") {
            sc.doc_rest += 1;
        }
    }

    let _ = writeln!(o, "## Docstrings & typing (measured)");
    if sc.doc_public > 0 {
        let dpct = (sc.doc_documented as f64 / sc.doc_public as f64 * 100.0).round() as u64;
        let style = [
            ("Google", sc.doc_google),
            ("NumPy", sc.doc_numpy),
            ("reST", sc.doc_rest),
            ("JSDoc", sc.doc_jsdoc),
        ]
        .into_iter()
        .max_by_key(|(_, n)| *n)
        .filter(|(_, n)| *n > 0)
        .map(|(k, _)| k)
        .unwrap_or("none");
        let _ = writeln!(
            o,
            "- public symbols documented: {dpct}% ({}/{}); dominant style: {style}",
            sc.doc_documented, sc.doc_public
        );
    }
    if sc.py_sig > 0 {
        let rpct = (sc.py_ret_annot as f64 / sc.py_sig as f64 * 100.0).round() as u64;
        let _ = writeln!(
            o,
            "- python return-type annotations: {rpct}% ({}/{} single-line sigs)",
            sc.py_ret_annot, sc.py_sig
        );
    }
    o.push('\n');

    let _ = writeln!(o, "## Idioms (measured counts)");
    let py = [
        ("bare `except:`", sc.bare_except),
        ("`except Exception`", sc.except_exception),
        ("`print(`", sc.print_calls),
        ("logger", sc.logger),
        ("f-strings", sc.fstring),
        ("`.format(`", sc.format_call),
        ("`async def`", sc.async_def),
        ("`@dataclass`", sc.dataclass),
        ("pydantic `BaseModel`", sc.pydantic),
    ];
    let ts = [
        ("`interface`", sc.ts_interface),
        ("`type` alias", sc.ts_type_alias),
        ("`enum`", sc.ts_enum),
        ("`console.log`", sc.console_log),
        ("`: any`", sc.any_type),
    ];
    let mut any = false;
    for (label, n) in py.into_iter().chain(ts) {
        if n > 0 {
            let _ = writeln!(o, "- {label}: {n}");
            any = true;
        }
    }
    if !any {
        let _ = writeln!(o, "- (none detected)");
    }
    o.push('\n');
}

// ---- enforced-config facts ------------------------------------------------------------------

fn config_section(o: &mut String, root: &Path) {
    let mut facts: Vec<String> = Vec::new();
    let read = |name: &str| std::fs::read_to_string(root.join(name)).ok();

    // .editorconfig (INI)
    if let Some(txt) = read(".editorconfig") {
        for line in txt.lines() {
            let l = line.trim();
            for key in [
                "indent_style",
                "indent_size",
                "max_line_length",
                "end_of_line",
                "insert_final_newline",
            ] {
                if let Some(v) = l
                    .strip_prefix(key)
                    .and_then(|r| r.trim_start().strip_prefix('='))
                {
                    facts.push(format!("{key} = {} (.editorconfig)", v.trim()));
                }
            }
        }
    }

    // pyproject.toml
    if let Some(txt) = read("pyproject.toml") {
        if let Ok(v) = txt.parse::<toml::Value>() {
            let g = |path: &[&str]| dig_toml(&v, path);
            if let Some(x) = g(&["tool", "ruff", "line-length"]) {
                facts.push(format!("ruff line-length = {x} (pyproject.toml)"));
            }
            if let Some(x) = g(&["tool", "ruff", "lint", "select"]) {
                facts.push(format!("ruff lint select = {x} (pyproject.toml)"));
            }
            if let Some(x) = g(&["tool", "ruff", "format", "quote-style"]) {
                facts.push(format!("ruff quote-style = {x} (pyproject.toml)"));
            }
            if let Some(x) = g(&["tool", "black", "line-length"]) {
                facts.push(format!("black line-length = {x} (pyproject.toml)"));
            }
            if let Some(x) = g(&["tool", "mypy", "strict"]) {
                facts.push(format!("mypy strict = {x} (pyproject.toml)"));
            }
            if g(&["tool", "pytest", "ini_options"]).is_some() {
                facts.push("test framework: pytest (pyproject.toml)".into());
            }
            if let Some(x) = g(&["project", "requires-python"]) {
                facts.push(format!("requires-python = {x} (pyproject.toml)"));
            }
        }
    }

    // .pre-commit-config.yaml — light grep for hook ids (enforced toolchain).
    if let Some(txt) = read(".pre-commit-config.yaml") {
        let ids: Vec<&str> = txt
            .lines()
            .filter_map(|l| {
                l.trim()
                    .strip_prefix("- id:")
                    .or_else(|| l.trim().strip_prefix("id:"))
            })
            .map(|s| s.trim())
            .collect();
        if !ids.is_empty() {
            facts.push(format!("pre-commit hooks (ENFORCED): {}", ids.join(", ")));
        }
    }

    // package.json / tsconfig.json / prettier (JSON)
    if let Some(txt) = read("package.json") {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&txt) {
            if let Some(t) = v.get("type").and_then(|x| x.as_str()) {
                facts.push(format!("package type = {t} (package.json)"));
            }
            if let Some(n) = v.pointer("/engines/node").and_then(|x| x.as_str()) {
                facts.push(format!("node engine = {n} (package.json)"));
            }
        }
    }
    if let Some(txt) = read("tsconfig.json") {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&strip_jsonc(&txt)) {
            if let Some(s) = v.pointer("/compilerOptions/strict") {
                facts.push(format!("tsconfig strict = {s} (tsconfig.json)"));
            }
            if let Some(t) = v
                .pointer("/compilerOptions/target")
                .and_then(|x| x.as_str())
            {
                facts.push(format!("tsconfig target = {t} (tsconfig.json)"));
            }
        }
    }
    for pf in [".prettierrc", ".prettierrc.json"] {
        if let Some(txt) = read(pf) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&txt) {
                for k in ["printWidth", "semi", "singleQuote", "trailingComma"] {
                    if let Some(x) = v.get(k) {
                        facts.push(format!("prettier {k} = {x} ({pf})"));
                    }
                }
            }
        }
    }

    // Cargo.toml / rustfmt.toml
    if let Some(txt) = read("Cargo.toml") {
        if let Ok(v) = txt.parse::<toml::Value>() {
            if let Some(x) = dig_toml(&v, &["package", "edition"]) {
                facts.push(format!("rust edition = {x} (Cargo.toml)"));
            }
            if let Some(x) = dig_toml(&v, &["package", "rust-version"]) {
                facts.push(format!("rust MSRV = {x} (Cargo.toml)"));
            }
            if dig_toml(&v, &["lints", "clippy"]).is_some() {
                facts.push("clippy lints configured in [lints.clippy] (Cargo.toml)".into());
            }
        }
    }
    if let Some(txt) = read("rustfmt.toml").or_else(|| read(".rustfmt.toml")) {
        if let Ok(v) = txt.parse::<toml::Value>() {
            for k in [
                "max_width",
                "edition",
                "imports_granularity",
                "group_imports",
            ] {
                if let Some(x) = dig_toml(&v, &[k]) {
                    facts.push(format!("rustfmt {k} = {x} (rustfmt.toml)"));
                }
            }
        }
    }

    // version pins
    for (name, label) in [
        (".python-version", "python"),
        (".nvmrc", "node"),
        (".tool-versions", "tool-versions"),
    ] {
        if let Some(txt) = read(name) {
            let v = txt.trim();
            if !v.is_empty() {
                facts.push(format!("{label} pin: {} ({name})", v.replace('\n', "; ")));
            }
        }
    }

    let _ = writeln!(o, "## Enforced config (ground truth)");
    if facts.is_empty() {
        let _ = writeln!(o, "- (no tooling config detected)\n");
        return;
    }
    for f in &facts {
        let _ = writeln!(o, "- {f}");
    }
    o.push('\n');
}

/// Navigate nested TOML tables; return a scalar's display string if present.
fn dig_toml(v: &toml::Value, path: &[&str]) -> Option<String> {
    let mut cur = v;
    for (i, k) in path.iter().enumerate() {
        cur = cur.get(k)?;
        if i == path.len() - 1 {
            return Some(match cur {
                toml::Value::String(s) => s.clone(),
                other => other.to_string(),
            });
        }
    }
    None
}

/// Strip `//` and `/* */` comments from JSONC (tsconfig) so serde_json can parse it.
fn strip_jsonc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    let (mut in_str, mut esc) = (false, false);
    while let Some(c) = chars.next() {
        if in_str {
            out.push(c);
            if esc {
                esc = false;
            } else if c == '\\' {
                esc = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => {
                in_str = true;
                out.push(c);
            }
            '/' if chars.peek() == Some(&'/') => {
                for n in chars.by_ref() {
                    if n == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut prev = ' ';
                for n in chars.by_ref() {
                    if prev == '*' && n == '/' {
                        break;
                    }
                    prev = n;
                }
            }
            _ => out.push(c),
        }
    }
    out
}
