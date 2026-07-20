//! CoMind CLI — the single distributable binary: `comind index|serve|resolve|query`.
//!
//! Status: scaffold. `index` (phase 1) parses a repo and prints graph stats; the
//! remaining subcommands land per phase (see ../../ARCHITECTURE.md).

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;
use std::process::ExitCode;

use comind_core::{Edge, EdgeKind, Symbol, SymbolKind};

fn usage() {
    println!(
        "comind {} — cross-repo code intelligence for agents\n\nUSAGE:\n  comind index <repo-path> [--to <uri>] [--incremental] [--since <sha>]\n  comind link <repo-path>... [--to <uri>] [--embed] [--enrich] [--incremental]\n  comind changed <repo-path> [--since <sha>]\n  comind explore <focus> (--from <uri> | <repo>...)\n  comind search <query...> --from <uri>\n  comind serve --from <uri>\n\n  -h, --help       show this help\n  -V, --version    show version",
        env!("CARGO_PKG_VERSION")
    );
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("index") => cmd_index(&args[2..]),
        Some("link") => cmd_link(&args[2..]),
        Some("explore") => cmd_explore(&args[2..]),
        Some("search") => cmd_search(&args[2..]),
        Some("changed") => cmd_changed(&args[2..]),
        Some("serve") => cmd_serve(&args[2..]),
        Some("-h" | "--help" | "help") | None => {
            usage();
            ExitCode::SUCCESS
        }
        Some("-V" | "--version") => {
            println!("comind {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("comind: unknown subcommand `{other}`\n");
            usage();
            ExitCode::FAILURE
        }
    }
}

/// Parse and cross-repo-resolve a set of repos into a merged corpus.
fn parse_and_resolve(repos: &[&str]) -> Result<(Vec<Symbol>, Vec<Edge>), String> {
    let mut symbols: Vec<Symbol> = Vec::new();
    let mut edges: Vec<Edge> = Vec::new();
    for path in repos {
        let name = repo_name(path);
        let o = comind_parse::parse_repo(Path::new(path), &name)
            .map_err(|e| format!("{name}: parse failed: {e:#}"))?;
        symbols.extend(o.symbols);
        edges.extend(o.edges);
    }
    let resolved = comind_resolve::resolve(&symbols, &edges);
    Ok((symbols, resolved.edges))
}

/// `comind explore <focus> <repo>...` — the agent-facing view: zoom, blast radius, and a
/// token-budgeted context pack for one symbol, across the whole corpus.
fn cmd_explore(args: &[String]) -> ExitCode {
    // `comind explore <focus> [--from <lancedb-uri> | <repo-path>...]`
    let Some((focus, rest)) = args.split_first() else {
        eprintln!("comind explore: usage: comind explore <focus> (--from <uri> | <repo-path>...)");
        return ExitCode::FAILURE;
    };
    let mut from: Option<&str> = None;
    let mut repos: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "--from" => {
                from = rest.get(i + 1).map(String::as_str);
                i += 2;
            }
            p => {
                repos.push(p);
                i += 1;
            }
        }
    }

    // Fast path: load the prebuilt graph from LanceDB (no re-parse). Otherwise parse+resolve.
    let (symbols, edges) = match from {
        Some(uri) => match comind_index::read_graph_blocking(uri) {
            Ok(x) => x,
            Err(e) => {
                eprintln!("comind explore: load from {uri} failed: {e:#}");
                return ExitCode::FAILURE;
            }
        },
        None => {
            if repos.is_empty() {
                eprintln!("comind explore: give --from <uri> or one or more <repo-path>");
                return ExitCode::FAILURE;
            }
            match parse_and_resolve(&repos) {
                Ok(x) => x,
                Err(e) => {
                    eprintln!("{e}");
                    return ExitCode::FAILURE;
                }
            }
        }
    };
    let g = comind_graph::CodeGraph::build(&symbols, &edges);
    eprintln!(
        "(graph: {} symbols, {} edges, {} dangling)",
        g.node_count(),
        g.edge_count(),
        g.dangling_edges
    );

    let Some(idx) = g.lookup(focus) else {
        eprintln!("comind explore: no symbol matching `{focus}`");
        return ExitCode::FAILURE;
    };

    // zoom
    let z = g.zoom(idx);
    if let Some(f) = &z.focus {
        println!("● {} [{}]  {}  {}", f.name, f.kind, f.repo, f.location);
        if let Some(sig) = &f.signature {
            println!("  {sig}");
        }
    }
    if let Some(c) = &z.container {
        println!("  in: {}  ({})", c.name, c.location);
    }
    print_group("calls", &z.callees, 8);
    print_group("called by", &z.callers, 8);
    print_group("imported by", &z.importers, 8);
    print_group("members", &z.members, 8);

    // ripple — blast radius grouped by repo
    let hits = g.ripple(idx, 4);
    let mut by_repo: BTreeMap<String, usize> = BTreeMap::new();
    for h in &hits {
        *by_repo.entry(h.node.repo.clone()).or_default() += 1;
    }
    println!("\nripple (blast radius, ≤4 hops): {} dependents", hits.len());
    for (repo, n) in &by_repo {
        println!("  {repo:<16} {n}");
    }

    // context pack — the minimal correct read-set within a token budget
    const BUDGET: usize = 1500;
    let pack = g.context_pack(idx, BUDGET);
    let total: usize = pack.iter().map(|(_, t)| t).sum();
    println!(
        "\ncontext pack for changing `{}` (~{total} tokens, budget {BUDGET}): {} symbols",
        z.focus.as_ref().map_or("?", |f| f.name.as_str()),
        pack.len()
    );
    for (node, _) in pack.iter().take(15) {
        println!("  {:<28} {}", node.name, node.location);
    }
    ExitCode::SUCCESS
}

fn print_group(label: &str, nodes: &[comind_graph::Node], limit: usize) {
    if nodes.is_empty() {
        return;
    }
    let names: Vec<String> = nodes
        .iter()
        .take(limit)
        .map(|n| n.name.clone())
        .collect();
    let more = nodes.len().saturating_sub(limit);
    let suffix = if more > 0 { format!(" (+{more})") } else { String::new() };
    println!("  {label}: {}{suffix}", names.join(", "));
}

/// `comind search <query...> --from <uri>` — semantic search reranked by graph centrality,
/// definition-boost, and test-file penalty (semble's fusion ideas + our unique graph signal).
fn cmd_search(args: &[String]) -> ExitCode {
    let mut from: Option<&str> = None;
    let mut query_parts: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--from" => {
                from = args.get(i + 1).map(String::as_str);
                i += 2;
            }
            w => {
                query_parts.push(w);
                i += 1;
            }
        }
    }
    let query = query_parts.join(" ");
    let (Some(uri), false) = (from, query.is_empty()) else {
        eprintln!("comind search: usage: comind search <query...> --from <lancedb-uri>");
        return ExitCode::FAILURE;
    };

    let (symbols, edges) = match comind_index::read_graph_blocking(uri) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("comind search: load from {uri} failed: {e:#}");
            return ExitCode::FAILURE;
        }
    };
    let by_id: HashMap<String, &Symbol> = symbols.iter().map(|s| (s.id.render(), s)).collect();
    let g = comind_graph::CodeGraph::build(&symbols, &edges);

    let embedder = match comind_embed::Embedder::load_default() {
        Ok(e) => e,
        Err(e) => {
            eprintln!("comind search: embedding model: {e:#}");
            return ExitCode::FAILURE;
        }
    };

    // LLM enrichment (summaries + generated queries), if present — folded into ranking + shown.
    let enrich: HashMap<String, (String, Vec<String>)> = comind_index::read_enrichment_blocking(uri)
        .ok()
        .flatten()
        .unwrap_or_default()
        .into_iter()
        .map(|(id, s, q)| (id.render(), (s, q)))
        .collect();

    // Native LanceDB hybrid retrieval: BM25 full-text + vector, fused with RRF. We then apply
    // code-aware boosts and our dependency-graph centrality signal on top.
    use comind_embed::rank;
    let keywords = rank::query_keywords(&query);
    let symbolish = rank::is_symbol_query(&query);
    let qvec = embedder.embed_query(&query);

    let candidates = match comind_index::hybrid_search_blocking(uri, &query, qvec, 80) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("comind search: hybrid search failed (did you run `link --embed`?): {e:#}");
            return ExitCode::FAILURE;
        }
    };

    let mut ranked: Vec<(f32, &Symbol, usize, Option<String>)> = candidates
        .into_iter()
        .filter_map(|(rid, base)| {
            let sym = *by_id.get(&rid)?;
            let deps = g.dependents_count(&rid);
            let enr = enrich.get(&rid);

            let is_def = matches!(
                sym.kind,
                SymbolKind::Function
                    | SymbolKind::Method
                    | SymbolKind::Class
                    | SymbolKind::Interface
                    | SymbolKind::Struct
                    | SymbolKind::Enum
            );
            let def_boost = if is_def { 1.2 } else { 1.0 };
            // strong exact-name boost for symbol queries (semble's definition boost)
            let exact_boost = if symbolish && sym.name.eq_ignore_ascii_case(query.trim()) {
                2.5
            } else {
                1.0
            };
            let penalty = rank::path_penalty(&sym.file_path);
            let graph_boost = 1.0 + 0.15 * (1.0 + deps as f32).ln();
            // recall boost: LLM-generated queries that overlap the user's query
            let query_boost = enr.map_or(1.0, |(_, qs)| 1.0 + 0.4 * query_match(&keywords, qs));

            let score = base * def_boost * exact_boost * penalty * graph_boost * query_boost;
            let summary = enr.map(|(s, _)| s.clone());
            Some((score, sym, deps, summary))
        })
        .collect();
    ranked.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    println!("search: {query:?}\n");
    for (score, sym, deps, summary) in ranked.iter().take(12) {
        println!(
            "  {score:>6.3}  {:<26} {:<9} deps={deps:<3} {}:{}",
            truncate(&sym.name, 26),
            format!("{:?}", sym.kind),
            sym.file_path,
            sym.range.start.line
        );
        if let Some(s) = summary {
            println!("          ↳ {s}");
        }
    }
    ExitCode::SUCCESS
}

/// Max overlap between the user's keywords and any one LLM-generated query (0..1).
fn query_match(keywords: &[String], generated: &[String]) -> f32 {
    if keywords.is_empty() {
        return 0.0;
    }
    generated
        .iter()
        .map(|q| {
            let qk = comind_embed::rank::query_keywords(q);
            keywords.iter().filter(|k| qk.contains(*k)).count() as f32 / keywords.len() as f32
        })
        .fold(0.0, f32::max)
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() > n {
        format!("{}…", &s[..n - 1])
    } else {
        s.to_string()
    }
}

/// `comind changed <repo> [--since <sha>]` — show files changed since a commit (incremental
/// indexing targeting). With no `--since`, just prints the current HEAD commit.
fn cmd_changed(args: &[String]) -> ExitCode {
    let mut repo: Option<&str> = None;
    let mut since: Option<&str> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--since" => {
                since = args.get(i + 1).map(String::as_str);
                i += 2;
            }
            p => {
                repo = Some(p);
                i += 1;
            }
        }
    }
    let Some(repo) = repo else {
        eprintln!("comind changed: usage: comind changed <repo> [--since <commit>]");
        return ExitCode::FAILURE;
    };
    let path = Path::new(repo);

    let head = match comind_git::head_commit(path) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("comind changed: {e:#}");
            return ExitCode::FAILURE;
        }
    };
    println!("HEAD: {head}");

    let Some(base) = since else {
        return ExitCode::SUCCESS;
    };
    match comind_git::changed_files(path, base) {
        Ok(cs) => {
            println!(
                "changed since {base}: {} files ({} added, {} modified, {} deleted)",
                cs.total(),
                cs.added.len(),
                cs.modified.len(),
                cs.deleted.len()
            );
            for p in cs.added.iter().take(10) {
                println!("  + {p}");
            }
            for p in cs.modified.iter().take(10) {
                println!("  ~ {p}");
            }
            for p in cs.deleted.iter().take(10) {
                println!("  - {p}");
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("comind changed: {e:#}");
            ExitCode::FAILURE
        }
    }
}

/// `comind serve --from <lancedb-uri>` — run the MCP server over stdio. Only the MCP
/// protocol goes to stdout; diagnostics go to stderr so the stream stays clean.
fn cmd_serve(args: &[String]) -> ExitCode {
    let mut from: Option<&str> = None;
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--from" {
            from = args.get(i + 1).map(String::as_str);
            i += 2;
        } else {
            i += 1;
        }
    }
    let Some(uri) = from else {
        eprintln!("comind serve: --from <lancedb-uri> is required");
        return ExitCode::FAILURE;
    };
    eprintln!("comind serve: loading graph from {uri} ...");
    let rt = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("comind serve: runtime: {e}");
            return ExitCode::FAILURE;
        }
    };
    match rt.block_on(comind_mcp::serve_stdio(uri)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("comind serve: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn repo_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("repo")
        .to_string()
}

/// Parse several repos, resolve references across them, and report the cross-repo graph
/// (the org-wide blast-radius signal that per-repo tools cannot produce).
fn cmd_link(args: &[String]) -> ExitCode {
    // Parse `<repo-path>... [--to <lancedb-uri>] [--embed]`.
    let mut repos: Vec<&str> = Vec::new();
    let mut to: Option<&str> = None;
    let mut embed = false;
    let mut enrich = false;
    let mut enrich_top: usize = 40;
    let mut incremental = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--to" => {
                to = args.get(i + 1).map(String::as_str);
                i += 2;
            }
            "--embed" => {
                embed = true;
                i += 1;
            }
            "--enrich" => {
                enrich = true;
                i += 1;
            }
            "--enrich-top" => {
                enrich_top = args.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(enrich_top);
                enrich = true;
                i += 2;
            }
            "--incremental" => {
                incremental = true;
                i += 1;
            }
            p => {
                repos.push(p);
                i += 1;
            }
        }
    }
    if repos.is_empty() {
        eprintln!("comind link: give one or more <repo-path> arguments");
        return ExitCode::FAILURE;
    }

    let mut symbols: Vec<Symbol> = Vec::new();
    let mut edges: Vec<Edge> = Vec::new();
    println!("parsing {} repos:", repos.len());
    for path in &repos {
        let name = repo_name(path);
        match comind_parse::parse_repo(Path::new(path), &name) {
            Ok(o) => {
                println!("  {name:<16} {:>6} symbols, {:>6} edges", o.symbols.len(), o.edges.len());
                symbols.extend(o.symbols);
                edges.extend(o.edges);
            }
            Err(e) => {
                eprintln!("  {name}: parse failed: {e:#}");
                return ExitCode::FAILURE;
            }
        }
    }

    let resolved = comind_resolve::resolve(&symbols, &edges);
    let s = &resolved.stats;
    println!(
        "\nresolved: {} imports ({} cross-repo), {} calls; {} imports unresolved (third-party)",
        s.resolved_imports, s.cross_repo_edges, s.resolved_calls, s.unresolved_imports
    );

    // Rank cross-repo import targets by how many distinct repos depend on them.
    // (dst descriptor) -> (defining repo, set of importer repos, total refs)
    let mut targets: BTreeMap<String, (String, BTreeSet<String>, usize)> = BTreeMap::new();
    for e in &resolved.edges {
        if e.kind == EdgeKind::Imports && e.cross_repo {
            let entry = targets
                .entry(e.dst.descriptor.clone())
                .or_insert_with(|| (e.dst.package.clone(), BTreeSet::new(), 0));
            entry.1.insert(e.src.package.clone());
            entry.2 += 1;
        }
    }

    let mut ranked: Vec<_> = targets.into_iter().collect();
    ranked.sort_by(|a, b| b.1 .1.len().cmp(&a.1 .1.len()).then(b.1 .2.cmp(&a.1 .2)));

    println!("\ntop cross-repo dependencies (org-wide blast radius):");
    println!("  {:<48} {:<12} repos  refs", "symbol", "defined-in");
    for (descriptor, (def_repo, importers, refs)) in ranked.iter().take(15) {
        let short = if descriptor.len() > 46 {
            format!("…{}", &descriptor[descriptor.len() - 45..])
        } else {
            descriptor.clone()
        };
        println!("  {short:<48} {def_repo:<12} {:>4}  {refs:>5}", importers.len());
    }

    // ripple demo on the single most-depended-on symbol.
    if let Some((descriptor, (def_repo, importers, refs))) = ranked.first() {
        println!("\nripple({descriptor})  [defined in {def_repo}]");
        println!("  changing this would impact {} repos ({refs} references):", importers.len());
        for repo in importers {
            println!("    - {repo}");
        }
    }

    if let Some(uri) = to {
        // Persist the resolved org-wide graph. Overwrite creates a new Lance version whose
        // manifest is the atomic "latest" pointer every consumer reads (push-to-master model).
        let dst = format!("{}/_graph", uri.trim_end_matches('/'));
        println!("\npersisting resolved org graph to {dst} ...");
        match comind_index::write_graph_blocking(&dst, &symbols, &resolved.edges) {
            Ok((sv, ev)) => {
                println!("wrote org graph: symbols v{sv}, edges v{ev}  (latest pointer)")
            }
            Err(e) => {
                eprintln!("comind link: write failed: {e:#}");
                return ExitCode::FAILURE;
            }
        }
        match comind_index::count_rows_blocking(&dst) {
            Ok((s, e)) => println!("read back: {s} symbols, {e} edges"),
            Err(e) => {
                eprintln!("comind link: read-back failed: {e:#}");
                return ExitCode::FAILURE;
            }
        }

        // Incremental: figure out which symbols are stale (their repo's changed files, per git),
        // so we recompute embeddings/enrichment only for those and reuse the rest.
        let current_heads: Vec<(String, String)> = repos
            .iter()
            .filter_map(|p| {
                comind_git::head_commit(Path::new(p)).ok().map(|h| (repo_name(p), h))
            })
            .collect();
        let stale_ids: HashSet<String> = if incremental {
            compute_stale_ids(&repos, &dst, &symbols)
        } else {
            symbols.iter().map(|s| s.id.render()).collect() // full recompute
        };
        if incremental {
            eprintln!("(incremental: {} of {} symbols stale)", stale_ids.len(), symbols.len());
        }

        if embed {
            if let ExitCode::FAILURE = run_embed(&dst, &symbols, &stale_ids, incremental) {
                return ExitCode::FAILURE;
            }
        }

        if enrich {
            if let ExitCode::FAILURE =
                run_enrich(&dst, &symbols, &resolved.edges, enrich_top, &stale_ids, incremental)
            {
                return ExitCode::FAILURE;
            }
        }

        // Record current per-repo commits so the next --incremental run can diff.
        let _ = comind_index::write_repo_commits_blocking(&dst, &current_heads);
    }

    ExitCode::SUCCESS
}

/// Rendered ids of symbols whose content may have changed since the last index: any symbol in a
/// repo that is new (no recorded commit / not a git repo) or whose file changed since that commit.
fn compute_stale_ids(repos: &[&str], dst: &str, symbols: &[Symbol]) -> HashSet<String> {
    let prior = comind_index::read_repo_commits_blocking(dst).unwrap_or_default();
    // repo -> Some(changed files) or None (treat whole repo as stale)
    let mut changed: HashMap<String, Option<HashSet<String>>> = HashMap::new();
    for p in repos {
        let name = repo_name(p);
        let set = match (prior.get(&name), comind_git::head_commit(Path::new(p))) {
            (Some(base), Ok(_)) => comind_git::changed_files(Path::new(p), base)
                .ok()
                .map(|cs| cs.added.into_iter().chain(cs.modified).collect::<HashSet<_>>()),
            _ => None, // no prior commit, or not a git repo → everything stale
        };
        changed.insert(name, set);
    }
    symbols
        .iter()
        .filter(|s| match changed.get(&s.id.package) {
            Some(Some(files)) => files.contains(&s.file_path),
            _ => true, // unknown repo or whole-repo-stale
        })
        .map(|s| s.id.render())
        .collect()
}

/// Embed symbols, reusing prior vectors for non-stale symbols (incremental) and computing only
/// stale/new ones. Persists the merged embeddings.
fn run_embed(dst: &str, symbols: &[Symbol], stale_ids: &HashSet<String>, incremental: bool) -> ExitCode {
    let embedder = match comind_embed::Embedder::load_default() {
        Ok(e) => e,
        Err(e) => {
            eprintln!("comind link: embedding model: {e:#}");
            return ExitCode::FAILURE;
        }
    };
    let prior: HashMap<String, Vec<f32>> = if incremental {
        comind_index::read_search_vectors_blocking(dst)
            .ok()
            .flatten()
            .unwrap_or_default()
            .into_iter()
            .map(|(id, v)| (id.render(), v))
            .collect()
    } else {
        HashMap::new()
    };

    let non_file: Vec<&Symbol> = symbols
        .iter()
        .filter(|s| !matches!(s.kind, SymbolKind::File))
        .collect();
    // (symbol, search text, vector) — text feeds the BM25 index, vector the semantic side.
    let mut rows: Vec<(comind_core::GlobalSymbolId, String, Vec<f32>)> =
        Vec::with_capacity(non_file.len());
    let mut reused = 0usize;
    let mut to_embed: Vec<&Symbol> = Vec::new();
    for s in &non_file {
        let rid = s.id.render();
        if !stale_ids.contains(&rid) {
            if let Some(v) = prior.get(&rid) {
                rows.push((s.id.clone(), comind_embed::symbol_text(s), v.clone()));
                reused += 1;
                continue;
            }
        }
        to_embed.push(s);
    }
    let texts: Vec<String> = to_embed.iter().map(|s| comind_embed::symbol_text(s)).collect();
    for (s, v) in to_embed.iter().zip(embedder.embed(&texts)) {
        rows.push((s.id.clone(), comind_embed::symbol_text(s), v));
    }
    match comind_index::write_search_table_blocking(dst, &rows) {
        Ok(v) => println!(
            "search index v{v}: {} symbols ({reused} reused, {} embedded) + BM25 FTS index",
            rows.len(),
            to_embed.len()
        ),
        Err(e) => {
            eprintln!("comind link: write search table failed: {e:#}");
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}

/// LLM enrichment (opt-in): summarize + generate NL queries for the most-central definitions,
/// infer a style guide, and persist to Lance. Sends code signatures to the OpenAI API.
/// Incremental: reuse prior summaries for non-stale symbols; only call the API for stale/new ones.
fn run_enrich(
    dst: &str,
    symbols: &[Symbol],
    edges: &[Edge],
    top: usize,
    stale_ids: &HashSet<String>,
    incremental: bool,
) -> ExitCode {
    let client = match comind_llm::LlmClient::from_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("comind link: {e:#}");
            return ExitCode::FAILURE;
        }
    };
    let rt = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("comind link: runtime: {e}");
            return ExitCode::FAILURE;
        }
    };
    let g = comind_graph::CodeGraph::build(symbols, edges);

    // Most-central definitions first — enrich where it matters most (bounded cost).
    let mut defs: Vec<&Symbol> = symbols
        .iter()
        .filter(|s| {
            matches!(
                s.kind,
                SymbolKind::Function
                    | SymbolKind::Method
                    | SymbolKind::Class
                    | SymbolKind::Interface
                    | SymbolKind::Struct
                    | SymbolKind::Enum
            )
        })
        .collect();
    defs.sort_by_key(|s| Reverse(g.dependents_count(&s.id.render())));
    defs.truncate(top);

    // Reuse prior enrichment for non-stale symbols; only hit the API for the rest.
    let prior: HashMap<String, (String, Vec<String>)> = if incremental {
        comind_index::read_enrichment_blocking(dst)
            .ok()
            .flatten()
            .unwrap_or_default()
            .into_iter()
            .map(|(id, s, q)| (id.render(), (s, q)))
            .collect()
    } else {
        HashMap::new()
    };

    let mut rows: Vec<(comind_core::GlobalSymbolId, String, Vec<String>)> = Vec::new();
    let mut to_call: Vec<&Symbol> = Vec::new();
    for s in &defs {
        let rid = s.id.render();
        if !stale_ids.contains(&rid) {
            if let Some((sum, q)) = prior.get(&rid) {
                rows.push((s.id.clone(), sum.clone(), q.clone()));
                continue;
            }
        }
        to_call.push(s);
    }

    let items: Vec<(String, String, String)> = to_call
        .iter()
        .map(|s| (s.name.clone(), s.signature.clone().unwrap_or_default(), s.file_path.clone()))
        .collect();
    if !items.is_empty() {
        eprintln!(
            "NOTE: --enrich sends code (names/signatures) to the OpenAI API — opt-in egress."
        );
        eprintln!("enriching {} symbols via {} ...", items.len(), comind_llm::DEFAULT_MODEL);
    }
    let results = rt.block_on(client.enrich_batch(&items));
    for (s, r) in to_call.iter().zip(results) {
        if let Some(e) = r {
            rows.push((s.id.clone(), e.summary, e.queries));
        }
    }

    match comind_index::write_enrichment_blocking(dst, &rows) {
        Ok(v) => println!(
            "enrichment v{v}: {} total ({} reused, {} generated)",
            rows.len(),
            rows.len() - items.len(),
            items.len()
        ),
        Err(e) => {
            eprintln!("comind link: write enrichment failed: {e:#}");
            return ExitCode::FAILURE;
        }
    }

    // Preview a few. Regenerate the style guide only when something changed.
    for (_, summary, queries) in rows.iter().take(3) {
        println!("  • {summary}");
        if let Some(q) = queries.first() {
            println!("    e.g. \"{q}\"");
        }
    }
    if !items.is_empty() {
        let samples: Vec<String> = items.iter().take(20).map(|(_, sig, _)| sig.clone()).collect();
        match rt.block_on(client.style_guide(&samples)) {
            Ok(guide) => {
                let _ = comind_index::write_style_guide_blocking(dst, &guide);
                println!("\nstyle guide (persisted; preview):\n{}", truncate_lines(&guide, 8));
            }
            Err(e) => eprintln!("comind link: style guide failed (non-fatal): {e:#}"),
        }
    }
    ExitCode::SUCCESS
}

fn truncate_lines(s: &str, n: usize) -> String {
    s.lines().take(n).collect::<Vec<_>>().join("\n")
}

fn cmd_index(args: &[String]) -> ExitCode {
    // Parse `<repo-path> [--to <uri>] [--incremental] [--since <commit>]`.
    let mut path: Option<&str> = None;
    let mut to: Option<&str> = None;
    let mut incremental = false;
    let mut since: Option<&str> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--to" => {
                to = args.get(i + 1).map(String::as_str);
                i += 2;
            }
            "--incremental" => {
                incremental = true;
                i += 1;
            }
            "--since" => {
                since = args.get(i + 1).map(String::as_str);
                incremental = true;
                i += 2;
            }
            p => {
                path = Some(p);
                i += 1;
            }
        }
    }
    let Some(path) = path else {
        eprintln!("comind index: missing <repo-path>");
        return ExitCode::FAILURE;
    };
    let root = Path::new(path);
    let repo_name = root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("repo")
        .to_string();
    let dst = to.map(|u| format!("{}/{repo_name}", u.trim_end_matches('/')));

    // Incremental path: diff against a base commit and reparse only what changed.
    if incremental {
        let Some(dst) = &dst else {
            eprintln!("comind index: --incremental requires --to <uri>");
            return ExitCode::FAILURE;
        };
        let base = since
            .map(str::to_string)
            .or_else(|| comind_index::read_repo_meta_blocking(dst).ok().flatten());
        match base {
            Some(base) => return incremental_index(root, &repo_name, dst, &base),
            None => eprintln!("(no recorded base commit — doing a full index)"),
        }
    }

    let out = match comind_parse::parse_repo(root, &repo_name) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("comind index: {e:#}");
            return ExitCode::FAILURE;
        }
    };

    let mut by_kind: BTreeMap<String, usize> = BTreeMap::new();
    for s in &out.symbols {
        *by_kind.entry(format!("{:?}", s.kind)).or_default() += 1;
    }
    let calls = out.edges.iter().filter(|e| e.kind == EdgeKind::Calls).count();
    let contains = out
        .edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Contains)
        .count();

    println!("repo: {repo_name}");
    println!("symbols: {}", out.symbols.len());
    for (k, n) in &by_kind {
        println!("  {k:<10} {n}");
    }
    println!("edges: {} (contains={contains}, calls={calls})", out.edges.len());

    println!("\nsample callables:");
    for s in out
        .symbols
        .iter()
        .filter(|s| matches!(s.kind, SymbolKind::Function | SymbolKind::Method))
        .take(8)
    {
        println!("  {}", s.id.descriptor);
    }

    if let Some(dst) = &dst {
        // Persist to LanceDB under a per-repo prefix, then read back to prove the round-trip.
        println!("\npersisting to {dst} ...");
        match comind_index::write_graph_blocking(dst, &out.symbols, &out.edges) {
            Ok((sv, ev)) => println!("wrote LanceDB versions: symbols v{sv}, edges v{ev}"),
            Err(e) => {
                eprintln!("comind index: write failed: {e:#}");
                return ExitCode::FAILURE;
            }
        }
        // Record the HEAD commit so later runs can index incrementally.
        if let Ok(head) = comind_git::head_commit(root) {
            let _ = comind_index::write_repo_meta_blocking(dst, &repo_name, &head);
            println!("recorded commit {}", short(&head));
        }
    }
    ExitCode::SUCCESS
}

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(8)]
}

/// Incremental index: reparse only files changed since `base`, drop symbols/edges of
/// modified+deleted files, merge, and rewrite. Keeps the index fresh at near-diff cost.
fn incremental_index(root: &Path, repo_name: &str, dst: &str, base: &str) -> ExitCode {
    let head = match comind_git::head_commit(root) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("comind index: {e:#}");
            return ExitCode::FAILURE;
        }
    };
    let cs = match comind_git::changed_files(root, base) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("comind index: {e:#}");
            return ExitCode::FAILURE;
        }
    };
    println!(
        "incremental {} → {}: {} changed ({}+ {}~ {}-)",
        short(base),
        short(&head),
        cs.total(),
        cs.added.len(),
        cs.modified.len(),
        cs.deleted.len()
    );
    if cs.total() == 0 {
        let _ = comind_index::write_repo_meta_blocking(dst, repo_name, &head);
        println!("index already up to date");
        return ExitCode::SUCCESS;
    }

    let (prior_syms, prior_edges) = match comind_index::read_graph_blocking(dst) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("comind index: no prior index at {dst} ({e:#}); run a full index first");
            return ExitCode::FAILURE;
        }
    };

    let dropped: HashSet<String> = cs.to_drop().cloned().collect();
    let to_parse: Vec<String> = cs.to_parse().cloned().collect();
    let newout = comind_parse::parse_files(root, repo_name, &to_parse);

    // Map each symbol id to its file so we can drop edges owned by changed files.
    let file_of: HashMap<String, String> =
        prior_syms.iter().map(|s| (s.id.render(), s.file_path.clone())).collect();
    let before = prior_syms.len();

    let mut symbols: Vec<Symbol> = prior_syms
        .into_iter()
        .filter(|s| !dropped.contains(&s.file_path))
        .collect();
    let mut edges: Vec<Edge> = prior_edges
        .into_iter()
        .filter(|e| file_of.get(&e.src.render()).is_none_or(|f| !dropped.contains(f)))
        .collect();
    symbols.extend(newout.symbols);
    edges.extend(newout.edges);

    if let Err(e) = comind_index::write_graph_blocking(dst, &symbols, &edges) {
        eprintln!("comind index: write failed: {e:#}");
        return ExitCode::FAILURE;
    }
    let _ = comind_index::write_repo_meta_blocking(dst, repo_name, &head);
    println!(
        "reparsed {} files → symbols {before}→{}, edges {}",
        to_parse.len(),
        symbols.len(),
        edges.len()
    );
    ExitCode::SUCCESS
}
