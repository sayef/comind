//! Comind CLI — the single distributable binary.
//!
//! Subcommands: `index`, `link`, `explore`, `search`, `find`, `repos`, `stats`, `guide`,
//! `flow`, `changed`, `serve`, `config`, `eval`.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;
use std::process::ExitCode;

use comind::model::{Edge, EdgeKind, Symbol, SymbolKind};

use clap::{Parser, Subcommand};

/// Cross-repo code intelligence for coding agents.
///
/// `--index-dir` defaults to the configured index dir (`comind config path`) and accepts a local
/// path or an `s3://bucket/prefix` URI. The same value is used to build and to read an index.
#[derive(Parser)]
#[command(name = "comind", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Index a single repo into the default index dir (searchable by default)
    Index {
        /// Repository path
        repo: String,
        /// Index directory — root; default: configured index dir (local path or s3://…)
        #[arg(long = "index-dir")]
        index_dir: Option<String>,
        /// Vector embeddings for hybrid search (default on; config `embed`)
        #[arg(long, overrides_with = "no_embed")]
        embed: bool,
        #[arg(long = "no-embed", overrides_with = "embed")]
        no_embed: bool,
        /// LLM per-symbol summaries + queries (sends code; needs OPENAI_API_KEY; config `enrich`)
        #[arg(long, overrides_with = "no_enrich")]
        enrich: bool,
        #[arg(long = "no-enrich", overrides_with = "enrich")]
        no_enrich: bool,
        /// LLM flow walkthroughs (sends call traces; needs OPENAI_API_KEY; config `flows`)
        #[arg(long, overrides_with = "no_flows")]
        flows: bool,
        #[arg(long = "no-flows", overrides_with = "flows")]
        no_flows: bool,
        /// Evidence-based style guide (sends code/config; needs OPENAI_API_KEY; config `guide`)
        #[arg(long, overrides_with = "no_guide")]
        guide: bool,
        #[arg(long = "no-guide", overrides_with = "guide")]
        no_guide: bool,
        /// Reparse only files changed since the last indexed commit
        #[arg(long)]
        incremental: bool,
        /// Diff against this commit instead of the recorded base
        #[arg(long)]
        since: Option<String>,
        /// SCIP: auto (index.scip or installed indexers) | require | off; config `scip`
        #[arg(long)]
        scip: Option<String>,
    },
    /// Link several repos into one cross-repo index (adds cross-repo edges + blast radius)
    Link {
        /// Repository paths
        #[arg(required = true)]
        repos: Vec<String>,
        /// Index directory — root; default: configured index dir (local path or s3://…)
        #[arg(long = "index-dir")]
        index_dir: Option<String>,
        /// Vector embeddings for hybrid search (default on; config `embed`)
        #[arg(long, overrides_with = "no_embed")]
        embed: bool,
        #[arg(long = "no-embed", overrides_with = "embed")]
        no_embed: bool,
        /// LLM per-symbol summaries + queries (sends code; needs OPENAI_API_KEY; config `enrich`)
        #[arg(long, overrides_with = "no_enrich")]
        enrich: bool,
        #[arg(long = "no-enrich", overrides_with = "enrich")]
        no_enrich: bool,
        /// LLM flow walkthroughs (sends call traces; needs OPENAI_API_KEY; config `flows`)
        #[arg(long, overrides_with = "no_flows")]
        flows: bool,
        #[arg(long = "no-flows", overrides_with = "flows")]
        no_flows: bool,
        /// Evidence-based style guide (sends code/config; needs OPENAI_API_KEY; config `guide`)
        #[arg(long, overrides_with = "no_guide")]
        guide: bool,
        #[arg(long = "no-guide", overrides_with = "guide")]
        no_guide: bool,
        /// Recompute only symbols in files changed since the last index
        #[arg(long)]
        incremental: bool,
        /// SCIP: auto (index.scip or installed indexers) | require | off; config `scip`
        #[arg(long)]
        scip: Option<String>,
    },
    /// Zoom, blast radius, and context pack for a symbol
    Explore {
        /// Symbol name or fragment to focus on
        focus: String,
        /// Index directory to read — root; default: configured index dir (local path or s3://…)
        #[arg(long = "index-dir")]
        index_dir: Option<String>,
        /// Repo paths to parse on the fly instead of reading an index
        repos: Vec<String>,
    },
    /// Graph-aware hybrid code search
    Search {
        /// Natural-language query
        #[arg(required = true, num_args = 1..)]
        query: Vec<String>,
        /// Index directory to read — root; default: configured index dir (local path or s3://…)
        #[arg(long = "index-dir")]
        index_dir: Option<String>,
        /// Restrict results to one or more repos (repeatable: --repo x --repo y)
        #[arg(long)]
        repo: Vec<String>,
        /// Output format: `md` or `table` (default from config, else `md`)
        #[arg(long)]
        format: Option<String>,
        /// Shortcut for `--format md`
        #[arg(long)]
        md: bool,
    },
    /// Files changed since a commit (git diff)
    Changed {
        /// Repository path
        repo: String,
        /// Base commit (default: HEAD's parent)
        #[arg(long)]
        since: Option<String>,
    },
    /// Pre-generated flow walkthrough + live call trace
    Flow {
        /// Entry-point symbol
        focus: String,
        /// Index directory to read — root; default: configured index dir (local path or s3://…)
        #[arg(long = "index-dir")]
        index_dir: Option<String>,
    },
    /// Run the MCP server over stdio
    Serve {
        /// Index directory to read — root; default: configured index dir (local path or s3://…)
        #[arg(long = "index-dir")]
        index_dir: Option<String>,
        /// Output format handed to the agent: `md` (default) or `json`
        #[arg(long)]
        format: Option<String>,
        /// Shortcut for `--format json`
        #[arg(long)]
        json: bool,
    },
    /// Find symbols by name or path substring
    Find {
        /// Name or descriptor/path substring
        query: String,
        /// Index directory to read — root; default: configured index dir
        #[arg(long = "index-dir")]
        index_dir: Option<String>,
        /// Limit to one or more repos (repeatable: --repo x --repo y)
        #[arg(long)]
        repo: Vec<String>,
    },
    /// List indexed repositories and their symbol counts
    Repos {
        /// Index directory to read — root; default: configured index dir
        #[arg(long = "index-dir")]
        index_dir: Option<String>,
    },
    /// Index statistics per repo: symbols, edges, kinds, enrichment coverage
    Stats {
        /// Index directory to read — root; default: configured index dir
        #[arg(long = "index-dir")]
        index_dir: Option<String>,
        /// Limit to one or more repos (repeatable: --repo x --repo y). Omit for all repos.
        #[arg(long)]
        repo: Vec<String>,
    },
    /// Print per-repo inferred coding style guides (built with --enrich)
    Guide {
        /// Index directory to read — root; default: configured index dir
        #[arg(long = "index-dir")]
        index_dir: Option<String>,
        /// Limit to one or more repos (repeatable: --repo x --repo y)
        #[arg(long)]
        repo: Vec<String>,
        /// Write the guide(s) to a file instead of the terminal
        #[arg(long, short = 'o')]
        output: Option<String>,
    },
    /// Record why code is the way it is: a decision, failed attempt, constraint, gotcha, intent or question
    Remember {
        /// decision | attempt | constraint | gotcha | intent | question
        kind: String,
        /// The symbol (or file) this is about (repeatable)
        #[arg(long)]
        on: Vec<String>,
        /// One-line statement of the fact
        #[arg(long)]
        title: String,
        #[arg(long, default_value = "")]
        body: String,
        /// When a future reader needs this
        #[arg(long)]
        trigger: Option<String>,
        /// Evidence: commit:<sha>, mr:!123, test:<path>::<name>, url:…, or text (repeatable)
        #[arg(long)]
        evidence: Vec<String>,
        /// Kind-specific field `key=value` (repeatable), e.g. outcome=reverted
        #[arg(long)]
        field: Vec<String>,
        #[arg(long = "index-dir")]
        index_dir: Option<String>,
    },
    /// Reasoning blame: recorded decisions, failed attempts and constraints about a symbol
    Why {
        focus: String,
        #[arg(long = "index-dir")]
        index_dir: Option<String>,
    },
    /// Confirm, retract, supersede or dispute a ledger record
    Review {
        id: String,
        /// confirm | retract | supersede | dispute
        action: String,
        #[arg(long, default_value = "")]
        reason: String,
        #[arg(long = "index-dir")]
        index_dir: Option<String>,
    },
    /// The ledger with derived status; `--inbox` shows only what needs review
    Ledger {
        #[arg(long = "index-dir")]
        index_dir: Option<String>,
        /// Only stale, orphaned, or unconfirmed agent records
        #[arg(long)]
        inbox: bool,
        /// Evaluate constraint checks; exits non-zero when a `must` constraint is violated (CI)
        #[arg(long)]
        check: bool,
    },
    /// Terminal UI: overview, contracts, ledger, explorer and conventions
    Ui {
        #[arg(long = "index-dir")]
        index_dir: Option<String>,
    },
    /// Before committing: recorded constraints/dead ends, contracts, blast radius and missing
    /// companions for your uncommitted changes
    Preflight {
        /// Repository path (default: current directory)
        #[arg(default_value = ".")]
        repo: String,
        /// Compare the working tree against this revision
        #[arg(long, default_value = "HEAD")]
        base: String,
        #[arg(long = "index-dir")]
        index_dir: Option<String>,
    },
    /// Seed the ledger from git history: reverts → failed attempts, commit trailers → records
    Harvest {
        /// Repository paths (already indexed)
        #[arg(required = true)]
        repos: Vec<String>,
        /// How many first-parent commits to scan per repo
        #[arg(long, default_value_t = 2000)]
        limit: usize,
        #[arg(long = "index-dir")]
        index_dir: Option<String>,
    },
    /// Agent hook adapter: reads the hook's JSON on stdin, prints context to inject (stdout JSON)
    Hook {
        /// Agent: `claude`
        agent: String,
        /// Event: `session-start` | `pre-edit`
        event: String,
        #[arg(long = "index-dir")]
        index_dir: Option<String>,
    },
    /// Print the hook configuration to add to an agent's settings
    InitHooks {
        /// Agent: `claude`
        agent: String,
    },
    /// Measured coding conventions per repo and org-wide, with exemplars and deviations
    Conventions {
        /// Index directory to read — root; default: configured index dir (local path or s3://…)
        #[arg(long = "index-dir")]
        index_dir: Option<String>,
        /// Only this repo (and the org baseline)
        #[arg(long)]
        repo: Option<String>,
    },
    /// Internal packages: who publishes each, who depends on it, and at which version
    Packages {
        /// Index directory to read — root; default: configured index dir (local path or s3://…)
        #[arg(long = "index-dir")]
        index_dir: Option<String>,
        /// Only consumers pinned to a version other than the publisher's
        #[arg(long)]
        skew: bool,
    },
    /// Service contracts (HTTP routes, queues, topics, buckets, tables) and who uses them
    Contracts {
        /// Index directory to read — root; default: configured index dir (local path or s3://…)
        #[arg(long = "index-dir")]
        index_dir: Option<String>,
        /// Only contracts of this type: http, sqs, sns, s3, table
        #[arg(long = "type")]
        kind: Option<String>,
        /// Only contracts missing a provider or a consumer
        #[arg(long)]
        dangling: bool,
    },
    /// Measure graph quality against a gold set
    Eval {
        #[command(subcommand)]
        what: EvalWhat,
    },
    /// Show or scaffold the config file
    Config {
        #[command(subcommand)]
        action: Option<ConfigAction>,
    },
}

#[derive(Subcommand)]
enum EvalWhat {
    /// Precision/recall of resolved edges on a fixture org (subdir per repo + expected.toml)
    Graph {
        /// Fixture directory
        #[arg(default_value = "tests/fixtures/org")]
        fixture: String,
        /// List every missed and spurious edge
        #[arg(long, short)]
        verbose: bool,
        /// Directory of `<repo>.scip` indexes to attach
        #[arg(long)]
        scip: Option<String>,
        /// Limit to these fixture repos (repeatable)
        #[arg(long)]
        repo: Vec<String>,
    },
}

#[derive(Subcommand)]
enum ConfigAction {
    /// Print the config file location and resolved index dir
    Path,
    /// Write a commented config.toml with the current defaults
    Init {
        /// Overwrite an existing config file (discards its current contents)
        #[arg(long)]
        overwrite: bool,
    },
}

fn main() -> ExitCode {
    match Cli::parse().cmd {
        Cmd::Index {
            repo,
            index_dir,
            embed,
            no_embed,
            enrich,
            no_enrich,
            flows,
            no_flows,
            guide,
            no_guide,
            incremental,
            since,
            scip,
        } => {
            let cfg = comind::config::Config::load();
            let Some(scip) = scip_mode(scip.as_deref(), &cfg) else {
                return ExitCode::FAILURE;
            };
            cmd_index(
                &repo,
                index_dir.as_deref(),
                tri(embed, no_embed).unwrap_or(cfg.embed()),
                tri(enrich, no_enrich).unwrap_or(cfg.enrich()),
                tri(flows, no_flows).unwrap_or(cfg.flows()),
                tri(guide, no_guide).unwrap_or(cfg.guide()),
                incremental,
                since.as_deref(),
                scip,
            )
        }
        Cmd::Link {
            repos,
            index_dir,
            embed,
            no_embed,
            enrich,
            no_enrich,
            flows,
            no_flows,
            guide,
            no_guide,
            incremental,
            scip,
        } => {
            let cfg = comind::config::Config::load();
            let Some(scip) = scip_mode(scip.as_deref(), &cfg) else {
                return ExitCode::FAILURE;
            };
            let repos: Vec<&str> = repos.iter().map(String::as_str).collect();
            cmd_link(
                &repos,
                index_dir.as_deref(),
                tri(embed, no_embed).unwrap_or(cfg.embed()),
                tri(enrich, no_enrich).unwrap_or(cfg.enrich()),
                tri(flows, no_flows).unwrap_or(cfg.flows()),
                tri(guide, no_guide).unwrap_or(cfg.guide()),
                incremental,
                scip,
            )
        }
        Cmd::Explore {
            focus,
            index_dir,
            repos,
        } => {
            let repos: Vec<&str> = repos.iter().map(String::as_str).collect();
            cmd_explore(&focus, index_dir.as_deref(), &repos)
        }
        Cmd::Search {
            query,
            index_dir,
            repo,
            format,
            md,
        } => {
            // CLI flag → config → default. `md`/`markdown` = markdown; anything else = table.
            let fmt = if md { Some("md".to_string()) } else { format };
            let fmt = fmt.unwrap_or_else(|| comind::config::Config::load().format());
            let markdown = matches!(fmt.as_str(), "md" | "markdown");
            let repos: Vec<&str> = repo.iter().map(String::as_str).collect();
            cmd_search(&query.join(" "), index_dir.as_deref(), &repos, markdown)
        }
        Cmd::Find {
            query,
            index_dir,
            repo,
        } => {
            let repos: Vec<&str> = repo.iter().map(String::as_str).collect();
            cmd_find(&query, index_dir.as_deref(), &repos)
        }
        Cmd::Repos { index_dir } => cmd_repos(index_dir.as_deref()),
        Cmd::Stats { index_dir, repo } => {
            let repos: Vec<&str> = repo.iter().map(String::as_str).collect();
            cmd_stats(&repos, index_dir.as_deref())
        }
        Cmd::Guide {
            index_dir,
            repo,
            output,
        } => {
            let repos: Vec<&str> = repo.iter().map(String::as_str).collect();
            cmd_guide(&repos, index_dir.as_deref(), output.as_deref())
        }
        Cmd::Changed { repo, since } => cmd_changed(&repo, since.as_deref()),
        Cmd::Flow { focus, index_dir } => cmd_flow(&focus, index_dir.as_deref()),
        Cmd::Serve {
            index_dir,
            format,
            json,
        } => {
            // CLI flag → config → default. `json` = raw JSON; anything else = markdown.
            let fmt = if json {
                Some("json".to_string())
            } else {
                format
            };
            let fmt = fmt.unwrap_or_else(|| comind::config::Config::load().format());
            let markdown = !matches!(fmt.as_str(), "json");
            cmd_serve(index_dir.as_deref(), markdown)
        }
        Cmd::Contracts {
            index_dir,
            kind,
            dangling,
        } => cmd_contracts(index_dir.as_deref(), kind.as_deref(), dangling),
        Cmd::Packages { index_dir, skew } => cmd_packages(index_dir.as_deref(), skew),
        Cmd::Conventions { index_dir, repo } => {
            cmd_conventions(index_dir.as_deref(), repo.as_deref())
        }
        Cmd::Remember {
            kind,
            on,
            title,
            body,
            trigger,
            evidence,
            field,
            index_dir,
        } => cmd_remember(
            &kind,
            &on,
            &title,
            &body,
            trigger,
            evidence,
            &field,
            index_dir.as_deref(),
        ),
        Cmd::Why { focus, index_dir } => cmd_why(&focus, index_dir.as_deref()),
        Cmd::Review {
            id,
            action,
            reason,
            index_dir,
        } => cmd_review(&id, &action, &reason, index_dir.as_deref()),
        Cmd::Ledger {
            index_dir,
            inbox,
            check,
        } => cmd_ledger(index_dir.as_deref(), inbox, check),
        Cmd::Hook {
            agent,
            event,
            index_dir,
        } => cmd_hook(&agent, &event, index_dir.as_deref()),
        Cmd::InitHooks { agent } => cmd_init_hooks(&agent),
        Cmd::Harvest {
            repos,
            limit,
            index_dir,
        } => cmd_harvest(&repos, limit, index_dir.as_deref()),
        Cmd::Preflight {
            repo,
            base,
            index_dir,
        } => cmd_preflight(&repo, &base, index_dir.as_deref()),
        Cmd::Ui { index_dir } => cmd_ui(index_dir.as_deref()),

        Cmd::Eval {
            what:
                EvalWhat::Graph {
                    fixture,
                    verbose,
                    scip,
                    repo,
                },
        } => cmd_eval_graph(&fixture, verbose, scip.as_deref(), &repo),
        Cmd::Config { action } => cmd_config(action),
    }
}

/// If any LLM step is requested but no LLM client is configured, warn once and disable them all —
/// so a keyless `index`/`link` still builds the graph + embeddings and exits 0.
fn gate_llm_steps(enrich: bool, flows: bool, guide: bool) -> (bool, bool, bool) {
    if (enrich || flows || guide) && comind::llm::LlmClient::from_env().is_err() {
        comind::ui::warn(
            "enrich/flows/guide skipped — set OPENAI_API_KEY (or COMIND_LLM_BASE_URL) to enable",
        );
        (false, false, false)
    } else {
        (enrich, flows, guide)
    }
}

/// `--scip` flag → config → `auto`; `None` (after an error message) for an unknown value.
fn scip_mode(flag: Option<&str>, cfg: &comind::config::Config) -> Option<comind::scip::Mode> {
    match flag {
        None => Some(cfg.scip()),
        Some(f) => comind::scip::Mode::parse(f).or_else(|| {
            comind::ui::err(&format!("--scip must be auto, require or off (got {f:?})"));
            None
        }),
    }
}

/// Attach SCIP to one parsed repo, reporting what was found.
fn attach_scip(
    mode: comind::scip::Mode,
    root: &Path,
    name: &str,
    out: &mut comind::parse::ParseOutput,
) -> Result<(), String> {
    match comind::scip::ingest(mode, root, name, &out.symbols, &mut out.facts) {
        Ok(a) if a.documents > 0 => {
            comind::ui::note(&format!(
                "{name}: SCIP {} index(es), {} files, {} defs, {} refs",
                a.indexes, a.documents, a.definitions, a.references
            ));
            Ok(())
        }
        Ok(_) => Ok(()),
        Err(e) => Err(format!("{e:#}")),
    }
}

/// Resolve a `--x` / `--no-x` flag pair to an explicit choice, or `None` to fall back to config.
/// clap's `overrides_with` guarantees at most one is set.
fn tri(on: bool, off: bool) -> Option<bool> {
    if on {
        Some(true)
    } else if off {
        Some(false)
    } else {
        None
    }
}

/// Parse and cross-repo-resolve a set of repos into a merged corpus.
fn parse_and_resolve(repos: &[&str]) -> Result<(Vec<Symbol>, Vec<Edge>), String> {
    let mut symbols: Vec<Symbol> = Vec::new();
    let mut edges: Vec<Edge> = Vec::new();
    let mut facts = Vec::new();
    for path in repos {
        let name = repo_name(path);
        let o = comind::parse::parse_repo(Path::new(path), &name)
            .map_err(|e| format!("{name}: parse failed: {e:#}"))?;
        symbols.extend(o.symbols);
        edges.extend(o.edges);
        facts.extend(o.facts);
    }
    let resolved = comind::resolve::resolve(&symbols, &edges, &facts);
    symbols.extend(resolved.symbols);
    Ok((symbols, resolved.edges))
}

/// `comind explore <focus> <repo>...` — the agent-facing view: zoom, blast radius, and a
/// token-budgeted context pack for one symbol, across the whole corpus.
fn cmd_explore(focus: &str, index_dir: Option<&str>, repos: &[&str]) -> ExitCode {
    // With repo paths and no --index-dir, parse them on the fly; otherwise read the prebuilt
    // index (flag or configured default — comind resolves the internal dataset path).
    let (symbols, edges) = if index_dir.is_none() && !repos.is_empty() {
        match parse_and_resolve(repos) {
            Ok(x) => x,
            Err(e) => {
                comind::ui::err(&e);
                return ExitCode::FAILURE;
            }
        }
    } else {
        let uri = comind::config::Config::load().graph_dir(index_dir);
        match comind::index::read_graph_blocking(&uri) {
            Ok(x) => x,
            Err(e) => {
                comind::ui::err(&friendly_load_err(&uri, &e));
                return ExitCode::FAILURE;
            }
        }
    };
    let g = comind::graph::CodeGraph::build(&symbols, &edges);
    let dangling = if g.dangling_edges > 0 {
        format!(", {} dangling", g.dangling_edges)
    } else {
        String::new()
    };
    comind::ui::note(&format!(
        "graph: {} symbols, {} edges{dangling}",
        g.node_count(),
        g.edge_count(),
    ));

    let Some(idx) = g.lookup(focus) else {
        comind::ui::err(&format!("no symbol matching `{focus}`"));
        return ExitCode::FAILURE;
    };

    // zoom
    let z = g.zoom(idx);
    if let Some(f) = &z.focus {
        comind::ui::header(&format!(
            "● {} [{}]  {}  {}",
            f.name, f.kind, f.repo, f.location
        ));
        if let Some(sig) = &f.signature {
            comind::ui::note(sig);
        }
    }
    if let Some(c) = &z.container {
        // Drop the redundant location when the container is the file itself.
        if c.kind.eq_ignore_ascii_case("file") {
            comind::ui::field("in", &c.name);
        } else {
            comind::ui::field("in", &format!("{}  ({})", c.name, c.location));
        }
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
    comind::ui::header(&format!(
        "ripple (blast radius, ≤4 hops): {} dependents",
        hits.len()
    ));
    if !by_repo.is_empty() {
        let mut t = comind::ui::table(&["repo", "dependents"]);
        for (repo, n) in &by_repo {
            t.add_row(vec![repo.clone(), n.to_string()]);
        }
        comind::ui::right_align(&mut t, &[1]); // dependents
        println!("{t}");
    }

    // context pack — the minimal correct read-set within a token budget
    const BUDGET: usize = 1500;
    let pack = g.context_pack(idx, BUDGET);
    let total: usize = pack.iter().map(|(_, t)| t).sum();
    comind::ui::header(&format!(
        "context pack for changing `{}` (~{total} tokens, budget {BUDGET}): {}",
        z.focus.as_ref().map_or("?", |f| f.name.as_str()),
        plural(pack.len(), "symbol")
    ));
    if !pack.is_empty() {
        let mut t = comind::ui::table(&["symbol", "location"]);
        for (node, _) in pack.iter().take(15) {
            t.add_row(vec![node.name.clone(), node.location.clone()]);
        }
        println!("{t}");
    }
    ExitCode::SUCCESS
}

fn print_group(label: &str, nodes: &[comind::graph::Node], limit: usize) {
    if nodes.is_empty() {
        return;
    }
    let names: Vec<String> = nodes.iter().take(limit).map(|n| n.name.clone()).collect();
    let more = nodes.len().saturating_sub(limit);
    let suffix = if more > 0 {
        format!(" (+{more})")
    } else {
        String::new()
    };
    comind::ui::field(label, &format!("{}{suffix}", names.join(", ")));
}

/// `comind search <query...> --from <uri>` — semantic search reranked by graph centrality,
/// definition-boost, and test-file penalty (semble's fusion ideas + our unique graph signal).
fn cmd_search(query: &str, from: Option<&str>, repos: &[&str], markdown: bool) -> ExitCode {
    let uri = comind::config::Config::load().graph_dir(from);
    let uri = uri.as_str();

    let (symbols, edges) = match comind::index::read_graph_blocking(uri) {
        Ok(x) => x,
        Err(e) => {
            comind::ui::err(&friendly_load_err(uri, &e));
            return ExitCode::FAILURE;
        }
    };
    let by_id: HashMap<String, Symbol> =
        symbols.iter().map(|s| (s.id.render(), s.clone())).collect();
    let g = comind::graph::CodeGraph::build(&symbols, &edges);

    let embedder = match comind::embed::Embedder::load_default() {
        Ok(e) => e,
        Err(e) => {
            comind::ui::err(&format!("embedding model: {e:#}"));
            return ExitCode::FAILURE;
        }
    };

    // LLM enrichment (summaries + generated queries), if present — folded into ranking + shown.
    let enrich: HashMap<String, (String, Vec<String>)> =
        comind::index::read_enrichment_blocking(uri)
            .ok()
            .flatten()
            .unwrap_or_default()
            .into_iter()
            .map(|(id, s, q)| (id.render(), (s, q)))
            .collect();

    // Native LanceDB hybrid retrieval (BM25 + vector, RRF-fused) + comind's code-aware boosts
    // and dependency-graph centrality — shared with the `search` MCP tool. When scoping to a
    // repo, over-fetch then filter so we still return a full page.
    let limit = if repos.is_empty() { 12 } else { 60 };
    let mut hits =
        match comind::search::hybrid_blocking(uri, &by_id, &g, &enrich, &embedder, query, limit) {
            Ok(h) => h,
            Err(e) => {
                comind::ui::err(&format!(
                    "hybrid search failed (was the index built with --no-embed?): {e:#}"
                ));
                return ExitCode::FAILURE;
            }
        };
    if !repos.is_empty() {
        hits.retain(|h| repos.iter().any(|r| h.repo.eq_ignore_ascii_case(r)));
        hits.truncate(12);
    }

    if markdown {
        print!("{}", comind::search::markdown(query, &hits));
    } else {
        comind::ui::header(&format!("search: \"{query}\""));
        if hits.is_empty() {
            comind::ui::note(
                "no matches — try a broader query (or the index was built with --no-embed)",
            );
            return ExitCode::SUCCESS;
        }
        let mut t = comind::ui::table(&["score", "symbol", "kind", "deps", "location", "summary"]);
        for h in &hits {
            t.add_row(vec![
                format!("{:.3}", h.score),
                h.name.clone(),
                h.kind.to_string(),
                h.deps.to_string(),
                h.location.clone(),
                h.summary.clone().unwrap_or_default(),
            ]);
        }
        comind::ui::right_align(&mut t, &[0, 3]); // score, deps
        println!("{t}");
    }
    ExitCode::SUCCESS
}

/// Load the prebuilt graph for a read-only command, with a friendly missing-index error.
fn read_graph_or_err(from: Option<&str>) -> Result<(Vec<Symbol>, Vec<Edge>), ()> {
    let uri = comind::config::Config::load().graph_dir(from);
    comind::index::read_graph_blocking(&uri)
        .map_err(|e| comind::ui::err(&friendly_load_err(&uri, &e)))
}

/// `comind find <query>` — locate symbols by name/descriptor/path substring.
fn cmd_find(query: &str, from: Option<&str>, repos: &[&str]) -> ExitCode {
    let Ok((symbols, edges)) = read_graph_or_err(from) else {
        return ExitCode::FAILURE;
    };
    let g = comind::graph::CodeGraph::build(&symbols, &edges);
    // Over-fetch when filtering by repo so a full page survives the filter.
    let mut hits = g.find(query, if repos.is_empty() { 30 } else { 200 });
    if !repos.is_empty() {
        hits.retain(|n| repos.iter().any(|r| n.repo.eq_ignore_ascii_case(r)));
        hits.truncate(30);
    }
    comind::ui::header(&format!(
        "find: \"{query}\" ({})",
        plural(hits.len(), "match")
    ));
    if hits.is_empty() {
        comind::ui::note("no symbols matched");
        return ExitCode::SUCCESS;
    }
    let mut t = comind::ui::table(&["symbol", "kind", "repo", "location", "id"]);
    for n in &hits {
        t.add_row(vec![
            n.name.clone(),
            n.kind.clone(),
            n.repo.clone(),
            n.location.clone(),
            n.id.clone(),
        ]);
    }
    println!("{t}");
    ExitCode::SUCCESS
}

/// `comind repos` — list indexed repositories and their symbol counts.
fn cmd_repos(from: Option<&str>) -> ExitCode {
    let Ok((symbols, edges)) = read_graph_or_err(from) else {
        return ExitCode::FAILURE;
    };
    let g = comind::graph::CodeGraph::build(&symbols, &edges);
    let repos = g.repos();
    comind::ui::header(&format!("Indexed repositories ({})", repos.len()));
    if repos.is_empty() {
        comind::ui::note("none");
        return ExitCode::SUCCESS;
    }
    let mut t = comind::ui::table(&["repo", "symbols"]);
    for (r, n) in &repos {
        t.add_row(vec![r.clone(), n.to_string()]);
    }
    comind::ui::right_align(&mut t, &[1]);
    println!("{t}");
    ExitCode::SUCCESS
}

/// `comind stats [--repo x --repo y]` — per-repo symbols, edges, kinds, enrichment coverage.
/// With no `--repo`, reports every indexed repo separately.
fn cmd_stats(want: &[&str], from: Option<&str>) -> ExitCode {
    let Ok((symbols, edges)) = read_graph_or_err(from) else {
        return ExitCode::FAILURE;
    };
    // Ids of enriched symbols, to compute per-repo coverage.
    let uri = comind::config::Config::load().graph_dir(from);
    let enriched: HashSet<String> = comind::index::read_enrichment_blocking(&uri)
        .ok()
        .flatten()
        .unwrap_or_default()
        .into_iter()
        .map(|(id, _, _)| id.render())
        .collect();

    // All repos present, sorted.
    let all: BTreeSet<String> = symbols.iter().map(|s| s.id.package.clone()).collect();
    // Which repos to report: the requested ones (warn on unknown) or all.
    let repos: Vec<String> = if want.is_empty() {
        all.iter().cloned().collect()
    } else {
        for r in want {
            if !all.contains(*r) {
                comind::ui::warn(&format!("no such repo in the index: {r}"));
            }
        }
        want.iter()
            .filter(|r| all.contains(**r))
            .map(|r| r.to_string())
            .collect()
    };
    if repos.is_empty() {
        comind::ui::err("no matching repos");
        return ExitCode::FAILURE;
    }

    comind::ui::header(&format!(
        "Index stats — {} across {}",
        plural(symbols.len(), "symbol"),
        plural(all.len(), "repo")
    ));

    for repo in &repos {
        let rsyms: Vec<&Symbol> = symbols.iter().filter(|s| &s.id.package == repo).collect();
        let redges = edges.iter().filter(|e| &e.src.package == repo).count();
        let renr = rsyms
            .iter()
            .filter(|s| enriched.contains(&s.id.render()))
            .count();
        comind::ui::header(&format!("● {repo}"));
        comind::ui::field("symbols", &rsyms.len().to_string());
        comind::ui::field("edges", &redges.to_string());
        comind::ui::field("enriched", &format!("{renr} / {}", rsyms.len()));

        let mut by_kind: BTreeMap<String, usize> = BTreeMap::new();
        for s in &rsyms {
            *by_kind.entry(format!("{:?}", s.kind)).or_default() += 1;
        }
        let mut kt = comind::ui::table(&["kind", "count"]);
        for (k, n) in &by_kind {
            kt.add_row(vec![k.clone(), n.to_string()]);
        }
        comind::ui::right_align(&mut kt, &[1]);
        println!("{kt}");
    }
    ExitCode::SUCCESS
}

/// `comind guide [--repo x --repo y] [--output f]` — per-repo inferred coding style guides
/// (built with `--enrich`). With no `--repo`, prints every repo's guide; `--output` writes to a
/// file instead of the terminal.
fn cmd_guide(want: &[&str], from: Option<&str>, output: Option<&str>) -> ExitCode {
    let uri = comind::config::Config::load().graph_dir(from);
    let guides = match comind::index::read_style_guide_blocking(&uri) {
        Ok(g) => g,
        Err(e) => {
            comind::ui::err(&friendly_load_err(&uri, &e));
            return ExitCode::FAILURE;
        }
    };
    if guides.is_empty() {
        comind::ui::note("no style guide yet — build the index with --enrich");
        return ExitCode::SUCCESS;
    }
    for r in want {
        if !guides.iter().any(|(repo, _)| repo.eq_ignore_ascii_case(r)) {
            comind::ui::warn(&format!("no style guide for repo: {r}"));
        }
    }
    let selected: Vec<&(String, String)> = guides
        .iter()
        .filter(|(repo, _)| want.is_empty() || want.iter().any(|r| repo.eq_ignore_ascii_case(r)))
        .collect();
    if selected.is_empty() {
        comind::ui::err("no matching repos");
        return ExitCode::FAILURE;
    }
    if let Some(path) = output {
        let doc: String = selected
            .iter()
            .map(|(repo, guide)| format!("# Style guide — {repo}\n\n{guide}\n"))
            .collect::<Vec<_>>()
            .join("\n");
        match std::fs::write(path, doc) {
            Ok(()) => comind::ui::ok(&format!(
                "style guide{} written to {path}",
                if selected.len() > 1 { "s" } else { "" }
            )),
            Err(e) => {
                comind::ui::err(&format!("write {path} failed: {e}"));
                return ExitCode::FAILURE;
            }
        }
    } else {
        for (repo, guide) in selected {
            comind::ui::header(&format!("Style guide — {repo}"));
            println!("{guide}");
        }
    }
    ExitCode::SUCCESS
}

fn truncate(s: &str, n: usize) -> String {
    // Count by characters, not bytes, so multibyte UTF-8 never splits mid-codepoint.
    if s.chars().count() > n {
        let cut = n.saturating_sub(1);
        format!("{}…", s.chars().take(cut).collect::<String>())
    } else {
        s.to_string()
    }
}

/// `comind changed <repo> [--since <sha>]` — show files changed since a commit (incremental
/// indexing targeting). With no `--since`, just prints the current HEAD commit.
fn cmd_changed(repo: &str, since: Option<&str>) -> ExitCode {
    let path = Path::new(repo);

    let head = match comind::git::head_commit(path) {
        Ok(h) => h,
        Err(e) => {
            comind::ui::err(&format!("{e:#}"));
            return ExitCode::FAILURE;
        }
    };
    comind::ui::field("HEAD", &head);

    let Some(base) = since else {
        comind::ui::note("pass --since <commit> to list changed files");
        return ExitCode::SUCCESS;
    };
    match comind::git::changed_files(path, base) {
        Ok(cs) => {
            comind::ui::header(&format!(
                "changed since {base}: {} files ({} added, {} modified, {} deleted)",
                cs.total(),
                cs.added.len(),
                cs.modified.len(),
                cs.deleted.len()
            ));
            for p in cs.added.iter().take(10) {
                comind::ui::note(&format!("+ {p}"));
            }
            for p in cs.modified.iter().take(10) {
                comind::ui::note(&format!("~ {p}"));
            }
            for p in cs.deleted.iter().take(10) {
                comind::ui::note(&format!("- {p}"));
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            comind::ui::err(&format!("{e:#}"));
            ExitCode::FAILURE
        }
    }
}

/// `comind serve --from <lancedb-uri>` — run the MCP server over stdio. Only the MCP
/// protocol goes to stdout; diagnostics go to stderr so the stream stays clean.
fn cmd_serve(from: Option<&str>, markdown: bool) -> ExitCode {
    let uri = comind::config::Config::load().graph_dir(from);
    let uri = uri.as_str();
    comind::ui::step(&format!("loading graph from {uri}"));
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            comind::ui::err(&format!("runtime: {e}"));
            return ExitCode::FAILURE;
        }
    };
    match rt.block_on(comind::mcp::serve_stdio(uri, markdown)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            comind::ui::err(&friendly_load_err(uri, &e));
            ExitCode::FAILURE
        }
    }
}

/// `comind config <path|init>` — show or scaffold the config file.
fn human() -> comind::ledger::Provenance {
    comind::ledger::Provenance {
        author: comind::ledger::AuthorKind::Human,
        actor: comind::ledger::actor(),
        source: "cli".into(),
        agent: None,
        session: None,
    }
}

#[allow(clippy::too_many_arguments)] // CLI dispatch: flags map 1:1 to clap args
fn cmd_remember(
    kind: &str,
    on: &[String],
    title: &str,
    body: &str,
    trigger: Option<String>,
    evidence: Vec<String>,
    fields: &[String],
    from: Option<&str>,
) -> ExitCode {
    let Some(kind) = comind::ledger::Kind::parse(kind) else {
        comind::ui::err("kind must be decision, attempt, constraint, gotcha, intent or question");
        return ExitCode::FAILURE;
    };
    let cfg = comind::config::Config::load();
    let (symbols, edges) = match comind::index::read_graph_blocking(&cfg.graph_dir(from)) {
        Ok(x) => x,
        Err(e) => {
            comind::ui::err(&format!("no index to anchor to: {e:#}"));
            return ExitCode::FAILURE;
        }
    };
    let g = comind::graph::CodeGraph::build(&symbols, &edges);
    let by_id: HashMap<String, &Symbol> = symbols.iter().map(|s| (s.id.render(), s)).collect();
    let mut anchors = Vec::new();
    for focus in on {
        match g.lookup(focus).and_then(|i| by_id.get(&g.node(i).id)) {
            Some(s) => anchors.push(comind::ledger::Anchor::of(s)),
            None => {
                comind::ui::err(&format!("no symbol matches `{focus}` (try `comind find`)"));
                return ExitCode::FAILURE;
            }
        }
    }
    let mut rec = comind::ledger::Record::new(kind, title, anchors, human());
    rec.body = body.to_string();
    rec.trigger = trigger;
    rec.evidence = evidence;
    for f in fields {
        if let Some((k, v)) = f.split_once('=') {
            rec.fields.insert(
                k.trim().to_string(),
                serde_json::Value::String(v.trim().into()),
            );
        }
    }
    match comind::ledger::put_blocking(&cfg.index_dir(from), &rec) {
        Ok(()) => {
            comind::ui::ok(&format!(
                "recorded {} `{}` ({})",
                kind.as_str(),
                rec.title,
                rec.id
            ));
            ExitCode::SUCCESS
        }
        Err(e) => {
            comind::ui::err(&format!("{e:#}"));
            ExitCode::FAILURE
        }
    }
}

fn cmd_why(focus: &str, from: Option<&str>) -> ExitCode {
    let cfg = comind::config::Config::load();
    let (symbols, edges) = match comind::index::read_graph_blocking(&cfg.graph_dir(from)) {
        Ok(x) => x,
        Err(e) => {
            comind::ui::err(&format!("no index: {e:#}"));
            return ExitCode::FAILURE;
        }
    };
    let g = comind::graph::CodeGraph::build(&symbols, &edges);
    let Some((name, ids)) = g.scope_of(focus) else {
        comind::ui::err(&format!("no symbol matches `{focus}`"));
        return ExitCode::FAILURE;
    };
    let records = comind::ledger::all_blocking(&cfg.index_dir(from)).unwrap_or_default();
    let entries = comind::ledger::derive(&records, &symbols);
    let about = comind::ledger::about(&entries, &ids);
    println!(
        "{}",
        comind::ledger::markdown(&format!("why `{name}`"), &about)
    );
    ExitCode::SUCCESS
}

fn cmd_review(id: &str, action: &str, reason: &str, from: Option<&str>) -> ExitCode {
    let Some(kind) = comind::ledger::RelationKind::parse(action) else {
        comind::ui::err("action must be confirm, retract, supersede or dispute");
        return ExitCode::FAILURE;
    };
    let root = comind::config::Config::load().index_dir(from);
    let records = comind::ledger::all_blocking(&root).unwrap_or_default();
    let Some(target) = records.iter().find(|r| r.id == id || r.id.starts_with(id)) else {
        comind::ui::err(&format!("no ledger record `{id}`"));
        return ExitCode::FAILURE;
    };
    let rec = comind::ledger::Record::relation(kind, target, reason, human());
    match comind::ledger::put_blocking(&root, &rec) {
        Ok(()) => {
            comind::ui::ok(&format!("{action}: `{}`", target.title));
            ExitCode::SUCCESS
        }
        Err(e) => {
            comind::ui::err(&format!("{e:#}"));
            ExitCode::FAILURE
        }
    }
}

fn cmd_ledger(from: Option<&str>, inbox: bool, check: bool) -> ExitCode {
    let cfg = comind::config::Config::load();
    let (symbols, edges) =
        comind::index::read_graph_blocking(&cfg.graph_dir(from)).unwrap_or_default();
    let records = match comind::ledger::all_blocking(&cfg.index_dir(from)) {
        Ok(r) => r,
        Err(e) => {
            comind::ui::err(&format!("{e:#}"));
            return ExitCode::FAILURE;
        }
    };
    let mut entries = comind::ledger::derive(&records, &symbols);
    if check {
        let packages =
            comind::index::read_packages_blocking(&cfg.graph_dir(from)).unwrap_or_default();
        let findings = comind::checks::run(&entries, &symbols, &edges, &packages);
        if findings.is_empty() {
            comind::ui::ok("no constraint carries a check (add one with --field check=...)");
            return ExitCode::SUCCESS;
        }
        println!(
            "## Constraint checks\n\n{}",
            comind::checks::markdown(&findings, None)
        );
        let failed = findings
            .iter()
            .any(|f| f.severity == "must" && !f.violations.is_empty());
        return if failed {
            ExitCode::FAILURE
        } else {
            ExitCode::SUCCESS
        };
    }
    let packages = comind::index::read_packages_blocking(&cfg.graph_dir(from)).unwrap_or_default();
    let findings = comind::checks::run(&entries, &symbols, &edges, &packages);
    comind::checks::promote(&mut entries, &findings);
    let shown = comind::ledger::inbox(&entries, inbox);
    let title = if inbox { "Ledger inbox" } else { "Ledger" };
    println!("{}", comind::ledger::markdown(title, &shown));
    ExitCode::SUCCESS
}

fn cmd_ui(from: Option<&str>) -> ExitCode {
    let cfg = comind::config::Config::load();
    let dir = cfg.graph_dir(from);
    let (symbols, edges) = match comind::index::read_graph_blocking(&dir) {
        Ok(x) => x,
        Err(e) => {
            comind::ui::err(&format!("no index at {dir}: {e:#}"));
            return ExitCode::FAILURE;
        }
    };
    let mut conventions = match comind::index::read_facts_blocking(&dir) {
        Ok(Some(facts)) => comind::conventions::mine(&symbols, &edges, &facts),
        _ => Vec::new(),
    };
    conventions.extend(comind::index::read_recipes_blocking(&dir).unwrap_or_default());
    let records = comind::ledger::all_blocking(&cfg.index_dir(from)).unwrap_or_default();
    let entries = comind::ledger::derive(&records, &symbols);
    let deps = comind::manifests::internal(
        &comind::index::read_packages_blocking(&dir).unwrap_or_default(),
    );
    let graph = comind::graph::CodeGraph::build(&symbols, &edges);
    let data = comind::tui::Data {
        contracts: graph.contracts(None, false),
        graph,
        symbols,
        edges,
        entries,
        conventions,
        deps,
        root: Some(cfg.index_dir(from)),
    };
    match comind::tui::App::new(data).run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            comind::ui::err(&format!("{e:#}"));
            ExitCode::FAILURE
        }
    }
}

fn cmd_preflight(repo: &str, base: &str, from: Option<&str>) -> ExitCode {
    let root = Path::new(repo);
    let name = std::fs::canonicalize(root)
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_else(|| repo_name(repo));
    let raw = match comind::git::working_changes(root, base) {
        Ok(c) => c,
        Err(e) => {
            comind::ui::err(&format!("{e:#}"));
            return ExitCode::FAILURE;
        }
    };
    if raw.is_empty() {
        comind::ui::ok("no changes against the base");
        return ExitCode::SUCCESS;
    }
    let cfg = comind::config::Config::load();
    let (symbols, edges) = match comind::index::read_graph_blocking(&cfg.graph_dir(from)) {
        Ok(x) => x,
        Err(e) => {
            comind::ui::err(&format!("no index: {e:#}"));
            return ExitCode::FAILURE;
        }
    };
    let changes: Vec<comind::preflight::Change> = raw
        .into_iter()
        .map(|(path, lines)| comind::preflight::Change {
            repo: name.clone(),
            path,
            lines,
        })
        .collect();
    let g = comind::graph::CodeGraph::build(&symbols, &edges);
    let records = comind::ledger::all_blocking(&cfg.index_dir(from)).unwrap_or_default();
    let entries = comind::ledger::derive(&records, &symbols);
    let recipes = comind::index::read_recipes_blocking(&cfg.graph_dir(from)).unwrap_or_default();
    let packages = comind::index::read_packages_blocking(&cfg.graph_dir(from)).unwrap_or_default();
    let findings = comind::checks::run(&entries, &symbols, &edges, &packages);
    println!(
        "{}",
        comind::preflight::report(&g, &symbols, &edges, &entries, &recipes, &findings, &changes)
    );
    ExitCode::SUCCESS
}

fn cmd_harvest(repos: &[String], limit: usize, from: Option<&str>) -> ExitCode {
    let cfg = comind::config::Config::load();
    let symbols = match comind::index::read_graph_blocking(&cfg.graph_dir(from)) {
        Ok((s, _)) => s,
        Err(e) => {
            comind::ui::err(&format!("index the repos first: {e:#}"));
            return ExitCode::FAILURE;
        }
    };
    let root = cfg.index_dir(from);
    let existing: HashSet<String> = comind::ledger::all_blocking(&root)
        .unwrap_or_default()
        .into_iter()
        .map(|r| r.id)
        .collect();
    let mut by_kind: BTreeMap<&str, usize> = BTreeMap::new();
    for path in repos {
        let name = repo_name(path);
        let history = match comind::git::all_history(Path::new(path), limit) {
            Ok(h) => h,
            Err(e) => {
                comind::ui::warn(&format!("{name}: {e:#}"));
                continue;
            }
        };
        for rec in comind::ledger::harvest(&name, &history, &symbols) {
            if existing.contains(&rec.id) {
                continue;
            }
            match comind::ledger::put_blocking(&root, &rec) {
                Ok(()) => *by_kind.entry(rec.kind.as_str()).or_default() += 1,
                Err(e) => comind::ui::warn(&format!("{name}: {e:#}")),
            }
        }
    }
    let parts: Vec<String> = by_kind.iter().map(|(k, n)| format!("{n} {k}")).collect();
    comind::ui::ok(&format!(
        "harvested {}",
        if parts.is_empty() {
            "nothing new".into()
        } else {
            parts.join(", ")
        }
    ));
    ExitCode::SUCCESS
}

/// Claude Code hook: emit `additionalContext` with ledger records relevant to the session/edit.
/// Never blocks or approves anything; any failure prints nothing and exits 0.
fn cmd_hook(agent: &str, event: &str, from: Option<&str>) -> ExitCode {
    use std::io::Read;
    if agent != "claude" {
        return ExitCode::SUCCESS;
    }
    let mut input = String::new();
    let _ = std::io::stdin().read_to_string(&mut input);
    let v: serde_json::Value = serde_json::from_str(&input).unwrap_or_default();
    let cwd = v
        .get("cwd")
        .and_then(|c| c.as_str())
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_default();
    let session = v
        .get("session_id")
        .and_then(|s| s.as_str())
        .unwrap_or("default")
        .to_string();
    let cfg = comind::config::Config::load();
    let Ok((symbols, _)) = comind::index::read_graph_blocking(&cfg.graph_dir(from)) else {
        return ExitCode::SUCCESS;
    };
    let Ok(records) = comind::ledger::all_blocking(&cfg.index_dir(from)) else {
        return ExitCode::SUCCESS;
    };
    if records.is_empty() {
        return ExitCode::SUCCESS;
    }
    let entries = comind::ledger::derive(&records, &symbols);
    let (hook_event, ids): (&str, Vec<String>) = match event {
        "session-start" => {
            // Every symbol of the repo this session works in.
            let repo = repo_at(&symbols, &cwd);
            let ids = symbols
                .iter()
                .filter(|s| Some(&s.repo.0) == repo.as_ref())
                .map(|s| s.id.render())
                .collect();
            ("SessionStart", ids)
        }
        "pre-edit" => {
            let Some(path) = v
                .get("tool_input")
                .and_then(|t| t.get("file_path"))
                .and_then(|p| p.as_str())
            else {
                return ExitCode::SUCCESS;
            };
            let ids = symbols
                .iter()
                .filter(|s| !s.file_path.is_empty() && path.ends_with(&format!("/{}", s.file_path)))
                .map(|s| s.id.render())
                .collect();
            ("PreToolUse", ids)
        }
        _ => return ExitCode::SUCCESS,
    };
    let seen_file = std::env::temp_dir()
        .join("comind-hook-sessions")
        .join(session.replace(['/', '\\'], "_"));
    let seen: HashSet<String> = std::fs::read_to_string(&seen_file)
        .map(|s| s.lines().map(str::to_string).collect())
        .unwrap_or_default();
    let fresh: Vec<&comind::ledger::Entry> = comind::ledger::about(&entries, &ids)
        .into_iter()
        .filter(|e| e.trust >= comind::ledger::Trust::T2)
        .filter(|e| {
            matches!(
                e.status,
                comind::ledger::Status::Active
                    | comind::ledger::Status::Moved
                    | comind::ledger::Status::Stale
            )
        })
        .filter(|e| !seen.contains(&e.record.id))
        .take(if hook_event == "SessionStart" { 8 } else { 4 })
        .collect();
    if fresh.is_empty() {
        return ExitCode::SUCCESS;
    }
    let mut text =
        comind::ledger::markdown("comind ledger: reasoning recorded about this code", &fresh);
    text.truncate(9_000);
    let _ = std::fs::create_dir_all(seen_file.parent().unwrap_or(std::path::Path::new(".")));
    let mut lines: Vec<String> = seen.into_iter().collect();
    lines.extend(fresh.iter().map(|e| e.record.id.clone()));
    let _ = std::fs::write(&seen_file, lines.join("\n"));
    let out = serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": hook_event,
            "additionalContext": text,
        }
    });
    println!("{out}");
    ExitCode::SUCCESS
}

/// The indexed repo a working directory belongs to (by directory name).
fn repo_at(symbols: &[Symbol], cwd: &Path) -> Option<String> {
    let repos: BTreeSet<&str> = symbols.iter().map(|s| s.repo.0.as_str()).collect();
    cwd.ancestors()
        .filter_map(|a| a.file_name().and_then(|n| n.to_str()))
        .find(|n| repos.contains(n))
        .map(str::to_string)
}

fn cmd_init_hooks(agent: &str) -> ExitCode {
    if agent != "claude" {
        comind::ui::err("only `claude` is supported so far");
        return ExitCode::FAILURE;
    }
    let snippet = serde_json::json!({
        "hooks": {
            "SessionStart": [{
                "hooks": [{"type": "command", "command": "comind hook claude session-start"}]
            }],
            "PreToolUse": [{
                "matcher": "Edit|MultiEdit|Write",
                "hooks": [{"type": "command", "command": "comind hook claude pre-edit"}]
            }]
        }
    });
    comind::ui::note("add to .claude/settings.json (project) or ~/.claude/settings.json (user):");
    println!(
        "{}",
        serde_json::to_string_pretty(&snippet).unwrap_or_default()
    );
    ExitCode::SUCCESS
}

fn cmd_conventions(from: Option<&str>, repo: Option<&str>) -> ExitCode {
    let dir = comind::config::Config::load().graph_dir(from);
    let (symbols, edges) = match comind::index::read_graph_blocking(&dir) {
        Ok(x) => x,
        Err(e) => {
            comind::ui::err(&format!("no index at {dir}: {e:#}"));
            return ExitCode::FAILURE;
        }
    };
    let Ok(Some(facts)) = comind::index::read_facts_blocking(&dir) else {
        comind::ui::err("index has no stored facts; re-run `comind index`/`link`");
        return ExitCode::FAILURE;
    };
    let mut all = comind::conventions::mine(&symbols, &edges, &facts);
    all.extend(comind::index::read_recipes_blocking(&dir).unwrap_or_default());
    let mut by_repo: BTreeMap<&str, Vec<&comind::conventions::Convention>> = BTreeMap::new();
    for c in &all {
        if repo.is_none_or(|r| c.repo == r || c.repo == comind::conventions::ORG) {
            by_repo.entry(c.repo.as_str()).or_default().push(c);
        }
    }
    for (r, cs) in by_repo {
        let title = if r == comind::conventions::ORG {
            "Org baseline".to_string()
        } else {
            r.to_string()
        };
        comind::ui::header(&title);
        for c in cs {
            comind::ui::field(&c.dimension, &c.rule);
            if !c.exemplars.is_empty() {
                comind::ui::note(&format!("e.g. {}", c.exemplars.join(", ")));
            }
            if !c.counterexamples.is_empty() {
                comind::ui::note(&format!("except {}", c.counterexamples.join(", ")));
            }
        }
    }
    ExitCode::SUCCESS
}

fn cmd_packages(from: Option<&str>, skew: bool) -> ExitCode {
    let dir = comind::config::Config::load().graph_dir(from);
    let rows = match comind::index::read_packages_blocking(&dir) {
        Ok(r) => r,
        Err(e) => {
            comind::ui::err(&format!("no index at {dir}: {e:#}"));
            return ExitCode::FAILURE;
        }
    };
    let deps: Vec<_> = comind::manifests::internal(&rows)
        .into_iter()
        .filter(|d| !skew || d.skewed())
        .collect();
    let mut table = comfy_table::Table::new();
    table
        .load_preset(comfy_table::presets::UTF8_FULL)
        .set_header(vec![
            "package",
            "published by",
            "consumer",
            "requires",
            "locked",
        ]);
    let opt = |v: &Option<String>| v.clone().unwrap_or_else(|| "—".into());
    for d in &deps {
        let locked = match (&d.locked, d.skewed()) {
            (Some(l), true) => format!("{l}  ≠"),
            _ => opt(&d.locked),
        };
        table.add_row(vec![
            format!("{} ({})", d.name, d.ecosystem),
            format!("{} @ {}", d.publisher, opt(&d.published)),
            d.consumer.clone(),
            opt(&d.requirement),
            locked,
        ]);
    }
    comind::ui::header(&format!("Internal dependencies — {}", deps.len()));
    println!("{table}");
    ExitCode::SUCCESS
}

/// Change recipes from each repo's recent first-parent history.
fn mine_recipes(repos: &[(String, std::path::PathBuf)]) -> Vec<comind::conventions::Convention> {
    let out: Vec<_> = repos
        .iter()
        .filter_map(|(name, root)| {
            comind::git::history(root, 500)
                .ok()
                .map(|h| comind::recipes::mine(name, &h))
        })
        .flatten()
        .collect();
    if !out.is_empty() {
        comind::ui::ok(&format!("recipes: {} from commit history", out.len()));
    }
    out
}

/// Manifests and lockfiles of each repo, and a one-line internal-dependency summary.
fn scan_packages(repos: &[(String, std::path::PathBuf)]) -> Vec<comind::manifests::PackageRow> {
    let rows: Vec<_> = repos
        .iter()
        .flat_map(|(name, root)| comind::manifests::scan(root, name))
        .collect();
    let deps = comind::manifests::internal(&rows);
    if !deps.is_empty() {
        let skewed = deps.iter().filter(|d| d.skewed()).count();
        comind::ui::ok(&format!(
            "internal packages: {} dependency link(s), {skewed} pinned to a different version than their publisher",
            deps.len()
        ));
    }
    rows
}

fn cmd_contracts(from: Option<&str>, kind: Option<&str>, dangling: bool) -> ExitCode {
    let dir = comind::config::Config::load().graph_dir(from);
    let (symbols, edges) = match comind::index::read_graph_blocking(&dir) {
        Ok(x) => x,
        Err(e) => {
            comind::ui::err(&format!("no index at {dir}: {e:#}"));
            return ExitCode::FAILURE;
        }
    };
    let by_id: HashMap<String, &Symbol> = symbols.iter().map(|s| (s.id.render(), s)).collect();
    // contract id → (providers, consumers) as `repo:name`
    let mut roles: BTreeMap<String, (BTreeSet<String>, BTreeSet<String>)> = BTreeMap::new();
    for s in symbols.iter().filter(|s| s.kind == SymbolKind::Contract) {
        roles.entry(s.name.clone()).or_default();
    }
    for e in &edges {
        let (Some(src), Some(dst)) = (by_id.get(&e.src.render()), by_id.get(&e.dst.render()))
        else {
            continue;
        };
        if dst.kind != SymbolKind::Contract {
            continue;
        }
        let who = format!("{}:{}", src.repo.0, src.name);
        let slot = roles.entry(dst.name.clone()).or_default();
        if comind::graph::PROVIDES.contains(&e.kind) {
            slot.0.insert(who);
        } else {
            slot.1.insert(who);
        }
    }
    let mut table = comfy_table::Table::new();
    table
        .load_preset(comfy_table::presets::UTF8_FULL)
        .set_header(vec!["contract", "provided by", "used by"]);
    let mut shown = 0;
    for (name, (prov, cons)) in &roles {
        if kind.is_some_and(|k| !name.starts_with(&format!("{k}:"))) {
            continue;
        }
        if dangling && !prov.is_empty() && !cons.is_empty() {
            continue;
        }
        let list = |s: &BTreeSet<String>| {
            if s.is_empty() {
                "—".to_string()
            } else {
                s.iter().cloned().collect::<Vec<_>>().join("\n")
            }
        };
        table.add_row(vec![name.clone(), list(prov), list(cons)]);
        shown += 1;
    }
    comind::ui::header(&format!("Contracts — {shown} shown of {}", roles.len()));
    println!("{table}");
    ExitCode::SUCCESS
}

fn cmd_eval_graph(fixture: &str, verbose: bool, scip: Option<&str>, repos: &[String]) -> ExitCode {
    let only: Vec<&str> = repos.iter().map(String::as_str).collect();
    let r = match comind::eval::eval_with(
        Path::new(fixture),
        (!only.is_empty()).then_some(only.as_slice()),
        scip.map(Path::new),
    ) {
        Ok(r) => r,
        Err(e) => {
            comind::ui::err(&format!("eval failed: {e:#}"));
            return ExitCode::FAILURE;
        }
    };
    let pct = |x: f64| format!("{:.0}%", x * 100.0);
    let mut kinds = comfy_table::Table::new();
    kinds
        .load_preset(comfy_table::presets::UTF8_FULL)
        .set_header(vec![
            "kind",
            "found",
            "missed",
            "spurious",
            "precision",
            "recall",
        ]);
    for (k, c) in r.by_kind.iter().chain([(&"total".to_string(), &r.total)]) {
        kinds.add_row(vec![
            k.clone(),
            c.tp.to_string(),
            c.fn_.to_string(),
            c.fp.to_string(),
            pct(c.precision()),
            pct(c.recall()),
        ]);
    }
    comind::ui::header(&format!("Graph eval — {fixture}"));
    println!("{kinds}");
    let mut cases = comfy_table::Table::new();
    cases
        .load_preset(comfy_table::presets::UTF8_FULL)
        .set_header(vec!["case", "found / expected"]);
    for (case, (found, expected)) in &r.by_case {
        cases.add_row(vec![case.clone(), format!("{found} / {expected}")]);
    }
    println!("{cases}");
    if scip.is_some() {
        comind::ui::note(&format!("references answered by SCIP: {}", r.scip_answers));
    }
    if verbose {
        comind::ui::header("Missed");
        for g in &r.missed {
            comind::ui::note(&format!("[{}] {} {} -> {}", g.case, g.kind, g.src, g.dst));
        }
        comind::ui::header("Spurious");
        for (k, s, d) in &r.spurious {
            comind::ui::note(&format!("{k} {s} -> {d}"));
        }
    }
    ExitCode::SUCCESS
}

fn cmd_config(action: Option<ConfigAction>) -> ExitCode {
    match action {
        None | Some(ConfigAction::Path) => {
            // Path stays plain on stdout so `$(comind config path)` is scriptable.
            println!("{}", comind::config::config_path().display());
            comind::ui::field("index dir", &comind::config::default_index_dir());
            ExitCode::SUCCESS
        }
        Some(ConfigAction::Init { overwrite }) => {
            use std::io::IsTerminal;
            let path = comind::config::config_path();
            if path.exists() && !overwrite {
                // Can't prompt when piped/CI → refuse and point at --overwrite.
                if !std::io::stdin().is_terminal() {
                    comind::ui::err("config exists — re-run with --overwrite to replace it");
                    return ExitCode::FAILURE;
                }
                // Deliberate interactive decline is a valid choice, not an error.
                if !prompt_yes(&path) {
                    comind::ui::note("kept existing config");
                    return ExitCode::SUCCESS;
                }
            }
            match comind::config::init(true) {
                Ok(path) => {
                    comind::ui::ok(&format!("wrote {}", path.display()));
                    comind::ui::note("edit it to set index_dir / llm_model / format, or run: comind index . --embed");
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    comind::ui::err(&format!("{e:#}"));
                    ExitCode::FAILURE
                }
            }
        }
    }
}

/// Interactive y/N prompt (caller guarantees a TTY). `y`/`yes` = overwrite; anything else = keep.
fn prompt_yes(path: &Path) -> bool {
    use std::io::Write;
    comind::ui::warn(&format!(
        "{} already exists; overwriting resets every value to its default.",
        path.display()
    ));
    eprint!("Overwrite it? [y/N] ");
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    if std::io::stdin().read_line(&mut line).is_err() {
        return false;
    }
    matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

fn repo_name(path: &str) -> String {
    let p = Path::new(path);
    // Use the directory's own name; for `.`/`..`/relative paths, resolve to the real dir first.
    match p.file_name().and_then(|n| n.to_str()) {
        Some(n) if n != "." && n != ".." => n.to_string(),
        _ => std::fs::canonicalize(p)
            .ok()
            .and_then(|c| c.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "repo".to_string()),
    }
}

/// Parse several repos, resolve references across them, and report the cross-repo graph
/// (the org-wide blast-radius signal that per-repo tools cannot produce).
#[allow(clippy::too_many_arguments)] // CLI dispatch: flags map 1:1 to clap args
fn cmd_link(
    repos: &[&str],
    to: Option<&str>,
    embed: bool,
    enrich: bool,
    flows: bool,
    guide: bool,
    incremental: bool,
    scip: comind::scip::Mode,
) -> ExitCode {
    let mut symbols: Vec<Symbol> = Vec::new();
    let mut edges: Vec<Edge> = Vec::new();
    let mut facts = Vec::new();
    if let Some(bad) = repos.iter().find(|p| !Path::new(p).is_dir()) {
        comind::ui::err(&format!("no such repo directory: {bad}"));
        return ExitCode::FAILURE;
    }
    comind::ui::header(&format!("Parsing {} repo(s)", repos.len()));
    for path in repos {
        let name = repo_name(path);
        match comind::parse::parse_repo(Path::new(path), &name) {
            Ok(mut o) => {
                comind::ui::field(
                    &name,
                    &format!("{} symbols, {} files", o.symbols.len(), o.facts.len()),
                );
                if let Err(e) = attach_scip(scip, Path::new(path), &name, &mut o) {
                    comind::ui::err(&e);
                    return ExitCode::FAILURE;
                }
                symbols.extend(o.symbols);
                edges.extend(o.edges);
                facts.extend(o.facts);
            }
            Err(e) => {
                comind::ui::err(&format!("{name}: parse failed: {e:#}"));
                return ExitCode::FAILURE;
            }
        }
    }

    let resolved = comind::resolve::resolve(&symbols, &edges, &facts);
    symbols.extend(resolved.symbols.iter().cloned());
    if !resolved.symbols.is_empty() {
        let mut by_type: BTreeMap<&str, usize> = BTreeMap::new();
        for c in &resolved.symbols {
            *by_type
                .entry(c.name.split(':').next().unwrap_or_default())
                .or_default() += 1;
        }
        let parts: Vec<String> = by_type.iter().map(|(t, n)| format!("{t} {n}")).collect();
        comind::ui::ok(&format!(
            "contracts: {} ({}), {} participant edges",
            resolved.symbols.len(),
            parts.join(", "),
            resolved.stats.contract_edges
        ));
    }
    let s = &resolved.stats;
    comind::ui::ok(&format!(
        "resolved: {} imports, {} calls, {} inherits/implements; {} cross-repo edges{}",
        s.resolved_imports,
        s.resolved_calls,
        s.type_edges,
        s.cross_repo_edges,
        if s.scip_answers > 0 {
            format!(" ({} answered by SCIP)", s.scip_answers)
        } else {
            String::new()
        }
    ));
    comind::ui::note(&format!(
        "unresolved: {} imports, {} calls (mostly third-party or dynamic)",
        s.unresolved_imports, s.unresolved_calls
    ));
    let mut top: Vec<_> = resolved.unresolved_modules.iter().collect();
    top.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    let top: Vec<String> = top
        .iter()
        .take(12)
        .map(|(m, n)| format!("{m} {n}"))
        .collect();
    if !top.is_empty() {
        comind::ui::note(&format!("top unresolved: {}", top.join(", ")));
    }
    if std::env::var("COMIND_DEBUG").is_ok() {
        for (m, ex) in &resolved.unresolved_examples {
            for e in ex {
                comind::ui::note(&format!("  unresolved {m}: {e}"));
            }
        }
    }

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

    if !ranked.is_empty() {
        comind::ui::header("Top cross-repo dependencies (org-wide blast radius)");
        let mut table = comfy_table::Table::new();
        table
            .load_preset(comfy_table::presets::UTF8_FULL)
            .set_header(vec!["symbol", "defined-in", "repos", "refs"]);
        for (descriptor, (def_repo, importers, refs)) in ranked.iter().take(15) {
            let short = if descriptor.chars().count() > 46 {
                let tail: String = descriptor
                    .chars()
                    .rev()
                    .take(45)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect();
                format!("…{tail}")
            } else {
                descriptor.clone()
            };
            table.add_row(vec![
                short,
                def_repo.clone(),
                importers.len().to_string(),
                refs.to_string(),
            ]);
        }
        println!("{table}");
    }

    // ripple demo on the single most-depended-on symbol.
    if let Some((descriptor, (def_repo, importers, refs))) = ranked.first() {
        comind::ui::header(&format!("ripple({descriptor})  [defined in {def_repo}]"));
        comind::ui::note(&format!(
            "changing this would impact {} repos ({refs} references):",
            importers.len()
        ));
        for repo in importers {
            comind::ui::note(&format!("- {repo}"));
        }
    }

    {
        // Persist the resolved org-wide graph. Overwrite creates a new Lance version whose
        // manifest is the atomic "latest" pointer every consumer reads (push-to-master model).
        let to_uri = comind::config::Config::load().index_dir(to);
        let uri = to_uri.as_str();
        let dst = format!("{}/_graph", uri.trim_end_matches('/'));
        comind::ui::header("Persisting");
        comind::ui::step(&format!("writing org graph to {dst}"));
        let roots: Vec<(String, std::path::PathBuf)> = repos
            .iter()
            .map(|p| (repo_name(p), std::path::PathBuf::from(p)))
            .collect();
        let packages = scan_packages(&roots);
        let recipes = mine_recipes(&roots);
        match comind::index::write_graph_blocking(&dst, &symbols, &resolved.edges)
            .and_then(|v| comind::index::write_facts_blocking(&dst, &facts).map(|_| v))
            .and_then(|v| comind::index::write_packages_blocking(&dst, &packages).map(|_| v))
            .and_then(|v| comind::index::write_recipes_blocking(&dst, &recipes).map(|_| v))
        {
            Ok((sv, ev)) => comind::ui::ok(&format!(
                "org graph: symbols v{sv}, edges v{ev} (latest pointer)"
            )),
            Err(e) => {
                comind::ui::err(&format!("write failed: {e:#}"));
                return ExitCode::FAILURE;
            }
        }
        match comind::index::count_rows_blocking(&dst) {
            Ok((s, e)) => comind::ui::note(&format!("read back: {s} symbols, {e} edges")),
            Err(e) => {
                comind::ui::err(&format!("read-back failed: {e:#}"));
                return ExitCode::FAILURE;
            }
        }

        // Incremental: figure out which symbols are stale (their repo's changed files, per git),
        // so we recompute embeddings/enrichment only for those and reuse the rest.
        let current_heads: Vec<(String, String)> = repos
            .iter()
            .filter_map(|p| {
                comind::git::head_commit(Path::new(p))
                    .ok()
                    .map(|h| (repo_name(p), h))
            })
            .collect();
        let stale_ids: HashSet<String> = if incremental {
            compute_stale_ids(repos, &dst, &symbols)
        } else {
            symbols.iter().map(|s| s.id.render()).collect() // full recompute
        };
        if incremental {
            comind::ui::note(&format!(
                "incremental: {} of {} symbols stale",
                stale_ids.len(),
                symbols.len()
            ));
        }

        if embed {
            if let ExitCode::FAILURE = run_embed(&dst, &symbols, &stale_ids, incremental) {
                return ExitCode::FAILURE;
            }
        }

        let cfg = comind::config::Config::load();
        let repo_roots: Vec<(String, std::path::PathBuf)> = repos
            .iter()
            .map(|p| (repo_name(p), std::path::PathBuf::from(p)))
            .collect();
        let (enrich, flows, guide) = gate_llm_steps(enrich, flows, guide);
        if enrich {
            if let ExitCode::FAILURE = run_enrich(
                &dst,
                &symbols,
                &resolved.edges,
                cfg.max_enrich(),
                &stale_ids,
                incremental,
            ) {
                return ExitCode::FAILURE;
            }
        }
        if guide {
            if let ExitCode::FAILURE =
                run_style_guides(&dst, &symbols, &resolved.edges, &repo_roots)
            {
                return ExitCode::FAILURE;
            }
        }

        if flows {
            if let ExitCode::FAILURE = run_flows(&dst, &symbols, &resolved.edges, cfg.max_flows()) {
                return ExitCode::FAILURE;
            }
        }

        // Record current per-repo commits so the next --incremental run can diff.
        let _ = comind::index::write_repo_commits_blocking(&dst, &current_heads);
    }

    ExitCode::SUCCESS
}

/// Pre-generate flow walkthroughs (opt-in `--flows`): pick the top entry points by how
/// much they orchestrate (forward call-trace size), trace each with `thread`, narrate via the
/// LLM, and persist to the `flows` table. Sends call traces to the OpenAI API.
fn run_flows(dst: &str, symbols: &[Symbol], edges: &[Edge], top: usize) -> ExitCode {
    let client = match comind::llm::LlmClient::from_env() {
        Ok(c) => c,
        Err(e) => {
            // No/invalid LLM key: skip this step rather than failing the whole index.
            comind::ui::warn(&format!("skipping (no LLM client): {e:#}"));
            return ExitCode::SUCCESS;
        }
    };
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            comind::ui::err(&format!("runtime: {e}"));
            return ExitCode::FAILURE;
        }
    };
    let g = comind::graph::CodeGraph::build(symbols, edges);

    comind::ui::header("Narrating flows (LLM)");
    // Entry points = functions/methods ranked by forward call-trace size (biggest flows first).
    let mut candidates: Vec<(&Symbol, usize)> = symbols
        .iter()
        .filter(|s| matches!(s.kind, SymbolKind::Function | SymbolKind::Method))
        // Skip test/example/legacy files — narrate production flows, where it's most useful.
        .filter(|s| comind::embed::rank::path_penalty(&s.file_path) > 0.3)
        .filter_map(|s| {
            let idx = g.lookup(&s.id.render())?;
            let reach = g.thread(idx, 4).len();
            (reach > 0).then_some((s, reach))
        })
        .collect();
    candidates.sort_by_key(|(_, r)| Reverse(*r));
    candidates.truncate(top);

    if candidates.is_empty() {
        comind::ui::note("flows: no multi-step flows found to narrate");
        return ExitCode::SUCCESS;
    }
    comind::ui::warn("--flows sends call traces to the OpenAI API — opt-in egress.");

    // Precompute call traces (graph ops) up front, then narrate concurrently.
    let prep: Vec<(comind::model::GlobalSymbolId, String, String, String)> = candidates
        .iter()
        .filter_map(|(s, _)| {
            let idx = g.lookup(&s.id.render())?;
            let trace = g
                .thread(idx, 4)
                .iter()
                .map(|h| format!("d{} {} {} {}", h.depth, h.via, h.node.name, h.node.location))
                .collect::<Vec<_>>()
                .join("\n");
            Some((
                s.id.clone(),
                s.name.clone(),
                s.signature.clone().unwrap_or_default(),
                trace,
            ))
        })
        .collect();

    let pb = comind::ui::progress(prep.len() as u64, "narrating flows");
    let rows: Vec<(comind::model::GlobalSymbolId, String, Vec<String>)> = rt.block_on(async {
        use futures::StreamExt;
        futures::stream::iter(prep.iter())
            .map(|(id, name, sig, trace)| {
                let (pb, client) = (&pb, &client);
                async move {
                    let r = client
                        .narrate_flow(name, sig, trace)
                        .await
                        .ok()
                        .map(|(narr, q)| (id.clone(), narr, q));
                    let (i, o) = client.token_usage();
                    pb.set_message(format!("{} in / {} out tok", kfmt(i), kfmt(o)));
                    pb.inc(1);
                    r
                }
            })
            .buffered(8)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .flatten()
            .collect()
    });
    pb.finish_and_clear();
    let (i, o) = client.token_usage();
    comind::ui::note(&format!(
        "tokens: {} in / {} out ({} total)",
        kfmt(i),
        kfmt(o),
        kfmt(i + o)
    ));

    match comind::index::write_flows_blocking(dst, &rows) {
        Ok(v) => comind::ui::ok(&format!("flows v{v}: {} narrated", rows.len())),
        Err(e) => {
            comind::ui::err(&format!("write flows failed: {e:#}"));
            return ExitCode::FAILURE;
        }
    }
    if let Some((_, narr, _)) = rows.first() {
        comind::ui::note(&format!("e.g. {}", truncate(narr, 120)));
    }
    ExitCode::SUCCESS
}

/// `comind flow <focus> --from <uri>` — the pre-generated flow walkthrough for an entry point (if
/// the index was built with `--flows`), followed by its live forward call trace.
fn cmd_flow(focus: &str, from: Option<&str>) -> ExitCode {
    let uri = comind::config::Config::load().graph_dir(from);
    let uri = uri.as_str();
    let (symbols, edges) = match comind::index::read_graph_blocking(uri) {
        Ok(x) => x,
        Err(e) => {
            comind::ui::err(&friendly_load_err(uri, &e));
            return ExitCode::FAILURE;
        }
    };
    let g = comind::graph::CodeGraph::build(&symbols, &edges);
    let Some(idx) = g.lookup(focus) else {
        comind::ui::err(&format!("no symbol matching `{focus}`"));
        return ExitCode::FAILURE;
    };
    let fid = g.zoom(idx).focus.map(|n| n.id);

    // Pre-generated narration, if present.
    if let Some(fid) = &fid {
        let stored = comind::index::read_flows_blocking(uri)
            .ok()
            .flatten()
            .unwrap_or_default()
            .into_iter()
            .find(|(id, _, _)| &id.render() == fid);
        match stored {
            Some((_, narr, queries)) => {
                comind::ui::header(&format!("flow: {focus}"));
                println!("{narr}\n");
                if !queries.is_empty() {
                    comind::ui::field("Ask", &queries.join(" · "));
                }
            }
            None => comind::ui::note(&format!(
                "no pre-generated narration for `{focus}` — showing the raw trace; run `index/link --flows` to narrate"
            )),
        }
    }

    // Live forward call trace.
    let hops = g.thread(idx, 4);
    comind::ui::header(&format!("flow trace from `{focus}` ({} steps)", hops.len()));
    if hops.is_empty() {
        comind::ui::note("no outgoing calls from here");
    } else {
        let mut t = comind::ui::table(&["depth", "via", "symbol", "location"]);
        for h in &hops {
            t.add_row(vec![
                format!("d{}", h.depth),
                h.via.to_string(),
                h.node.name.clone(),
                h.node.location.clone(),
            ]);
        }
        comind::ui::right_align(&mut t, &[0]);
        println!("{t}");
    }
    ExitCode::SUCCESS
}

/// Rendered ids of symbols whose content may have changed since the last index: any symbol in a
/// repo that is new (no recorded commit / not a git repo) or whose file changed since that commit.
fn compute_stale_ids(repos: &[&str], dst: &str, symbols: &[Symbol]) -> HashSet<String> {
    let prior = comind::index::read_repo_commits_blocking(dst).unwrap_or_default();
    // repo -> Some(changed files) or None (treat whole repo as stale)
    let mut changed: HashMap<String, Option<HashSet<String>>> = HashMap::new();
    for p in repos {
        let name = repo_name(p);
        let set = match (prior.get(&name), comind::git::head_commit(Path::new(p))) {
            (Some(base), Ok(_)) => comind::git::changed_files(Path::new(p), base)
                .ok()
                .map(|cs| {
                    cs.added
                        .into_iter()
                        .chain(cs.modified)
                        .collect::<HashSet<_>>()
                }),
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
fn run_embed(
    dst: &str,
    symbols: &[Symbol],
    stale_ids: &HashSet<String>,
    incremental: bool,
) -> ExitCode {
    let embedder = match comind::embed::Embedder::load_default() {
        Ok(e) => e,
        Err(e) => {
            comind::ui::err(&format!("embedding model: {e:#}"));
            return ExitCode::FAILURE;
        }
    };
    let prior: HashMap<String, Vec<f32>> = if incremental {
        comind::index::read_search_vectors_blocking(dst)
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
    let mut rows: Vec<(comind::model::GlobalSymbolId, String, Vec<f32>)> =
        Vec::with_capacity(non_file.len());
    let mut reused = 0usize;
    let mut to_embed: Vec<&Symbol> = Vec::new();
    for s in &non_file {
        let rid = s.id.render();
        if !stale_ids.contains(&rid) {
            if let Some(v) = prior.get(&rid) {
                rows.push((s.id.clone(), comind::embed::symbol_text(s), v.clone()));
                reused += 1;
                continue;
            }
        }
        to_embed.push(s);
    }
    comind::ui::header("Building search index");
    let pb = comind::ui::spinner(&format!(
        "embedding {} symbols + building BM25 index",
        to_embed.len()
    ));
    let texts: Vec<String> = to_embed
        .iter()
        .map(|s| comind::embed::symbol_text(s))
        .collect();
    for (s, v) in to_embed.iter().zip(embedder.embed(&texts)) {
        rows.push((s.id.clone(), comind::embed::symbol_text(s), v));
    }
    let write = comind::index::write_search_table_blocking(dst, &rows);
    pb.finish_and_clear();
    match write {
        Ok(v) => comind::ui::ok(&format!(
            "search index v{v}: {} symbols ({reused} reused, {} embedded) + BM25 FTS index",
            rows.len(),
            to_embed.len()
        )),
        Err(e) => {
            comind::ui::err(&format!("write search table failed: {e:#}"));
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
    let client = match comind::llm::LlmClient::from_env() {
        Ok(c) => c,
        Err(e) => {
            // No/invalid LLM key: skip this step rather than failing the whole index.
            comind::ui::warn(&format!("skipping (no LLM client): {e:#}"));
            return ExitCode::SUCCESS;
        }
    };
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            comind::ui::err(&format!("runtime: {e}"));
            return ExitCode::FAILURE;
        }
    };
    let g = comind::graph::CodeGraph::build(symbols, edges);

    comind::ui::header("Enriching symbols (LLM)");
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
        comind::index::read_enrichment_blocking(dst)
            .ok()
            .flatten()
            .unwrap_or_default()
            .into_iter()
            .map(|(id, s, q)| (id.render(), (s, q)))
            .collect()
    } else {
        HashMap::new()
    };

    let mut rows: Vec<(comind::model::GlobalSymbolId, String, Vec<String>)> = Vec::new();
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
        .map(|s| {
            (
                s.name.clone(),
                s.signature.clone().unwrap_or_default(),
                s.file_path.clone(),
            )
        })
        .collect();
    let results: Vec<Option<comind::llm::Enrichment>> = if items.is_empty() {
        Vec::new()
    } else {
        comind::ui::warn(
            "--enrich sends code (names/signatures) to the OpenAI API — opt-in egress.",
        );
        let pb = comind::ui::progress(items.len() as u64, "enriching symbols");
        let out = rt.block_on(async {
            use futures::StreamExt;
            futures::stream::iter(items.iter())
                .map(|(n, s, c)| {
                    let (pb, client) = (&pb, &client);
                    async move {
                        let r = client.enrich_symbol(n, s, c).await.ok();
                        let (i, o) = client.token_usage();
                        pb.set_message(format!("{} in / {} out tok", kfmt(i), kfmt(o)));
                        pb.inc(1);
                        r
                    }
                })
                .buffered(8)
                .collect::<Vec<_>>()
                .await
        });
        pb.finish_and_clear();
        let (i, o) = client.token_usage();
        comind::ui::note(&format!(
            "tokens: {} in / {} out ({} total)",
            kfmt(i),
            kfmt(o),
            kfmt(i + o)
        ));
        out
    };
    let mut generated = 0usize;
    for (s, r) in to_call.iter().zip(results) {
        if let Some(e) = r {
            rows.push((s.id.clone(), e.summary, e.queries));
            generated += 1;
        }
    }
    // rows = reused (prior) + generated (this run); failed API calls are dropped, so derive
    // `reused` from the total to avoid underflow when some calls fail.
    let reused = rows.len().saturating_sub(generated);
    let failed = items.len().saturating_sub(generated);

    match comind::index::write_enrichment_blocking(dst, &rows) {
        Ok(v) => comind::ui::ok(&format!(
            "enrichment v{v}: {} symbols ({reused} reused, {generated} generated{})",
            rows.len(),
            if failed > 0 {
                format!(", {failed} failed")
            } else {
                String::new()
            }
        )),
        Err(e) => {
            comind::ui::err(&format!("write enrichment failed: {e:#}"));
            return ExitCode::FAILURE;
        }
    }

    // Preview a few. Regenerate the style guide only when something changed.
    for (_, summary, queries) in rows.iter().take(3) {
        comind::ui::note(&format!("• {summary}"));
        if let Some(q) = queries.first() {
            comind::ui::note(&format!("  e.g. \"{q}\""));
        }
    }
    ExitCode::SUCCESS
}

/// Evidence-based style guide per repo (opt-in `--guide`): measured stats + enforced-config facts
/// → citation-required LLM synthesis. Independent of `--enrich`. Sends code/config to the LLM.
fn run_style_guides(
    dst: &str,
    symbols: &[Symbol],
    edges: &[Edge],
    repo_roots: &[(String, std::path::PathBuf)],
) -> ExitCode {
    let client = match comind::llm::LlmClient::from_env() {
        Ok(c) => c,
        Err(e) => {
            // No/invalid LLM key: skip this step rather than failing the whole index.
            comind::ui::warn(&format!("skipping (no LLM client): {e:#}"));
            return ExitCode::SUCCESS;
        }
    };
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            comind::ui::err(&format!("runtime: {e}"));
            return ExitCode::FAILURE;
        }
    };
    comind::ui::header("Style guides (per repo)");
    comind::ui::warn("--guide sends code samples + config to the LLM — opt-in egress.");
    let conventions = match comind::index::read_facts_blocking(dst) {
        Ok(Some(facts)) => comind::conventions::mine(symbols, edges, &facts),
        _ => Vec::new(),
    };
    let mut guide_rows: Vec<(String, String)> = Vec::new();
    for (repo, root) in repo_roots {
        let rsyms: Vec<&Symbol> = symbols.iter().filter(|s| &s.id.package == repo).collect();
        if rsyms.is_empty() {
            continue;
        }
        let redges: Vec<Edge> = edges
            .iter()
            .filter(|e| &e.src.package == repo)
            .cloned()
            .collect();
        let rconv: Vec<_> = conventions
            .iter()
            .filter(|c| &c.repo == repo)
            .cloned()
            .collect();
        let sections = comind::styleguide::build_sections(root, &rsyms, &redges, &rconv);
        if sections.is_empty() {
            continue;
        }
        let pb = comind::ui::progress(sections.len() as u64, &format!("{repo}: guide sections"));
        // One focused LLM call per section, concurrently; preserves order for stitching.
        let bodies: Vec<Option<String>> = rt.block_on(async {
            use futures::StreamExt;
            futures::stream::iter(sections.iter())
                .map(|sec| {
                    let (pb, client) = (&pb, &client);
                    async move {
                        let r = client
                            .guide_section(repo, &sec.title, &sec.guidance, &sec.evidence)
                            .await
                            .ok();
                        pb.inc(1);
                        r
                    }
                })
                .buffered(4)
                .collect()
                .await
        });
        pb.finish_and_clear();
        // Stitch sections, dropping empties / "no convention" replies.
        let mut doc = String::new();
        for (sec, body) in sections.iter().zip(bodies) {
            let body = body.unwrap_or_default();
            let t = body.trim();
            if t.is_empty() || t.eq_ignore_ascii_case("No consistent convention observed.") {
                continue;
            }
            doc.push_str(&format!("## {}\n\n{}\n\n", sec.title, t));
        }
        if doc.is_empty() {
            comind::ui::warn(&format!("{repo}: no sections generated"));
            continue;
        }
        doc.push_str("\n_AI-generated from the codebase; review before enforcing._\n");
        comind::ui::ok(&format!(
            "{repo}: style guide ({} sections)",
            doc.matches("\n## ").count() + 1
        ));
        guide_rows.push((repo.clone(), doc));
    }
    if !guide_rows.is_empty() {
        let _ = comind::index::write_style_guide_blocking(dst, &guide_rows);
    }
    let (i, o) = client.token_usage();
    comind::ui::note(&format!("tokens: {} in / {} out", kfmt(i), kfmt(o)));
    ExitCode::SUCCESS
}

/// Compact `1.2k`-style formatting for token counts.
fn kfmt(n: u64) -> String {
    if n >= 1000 {
        format!("{:.1}k", n as f64 / 1000.0)
    } else {
        n.to_string()
    }
}

/// `n word` / `n words` — naive English pluralization for count phrases.
fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        format!("{n} {word}")
    } else {
        format!("{n} {word}s")
    }
}

/// Turn a LanceDB "dataset/table not found" error into an actionable one-liner, hiding the raw
/// internal backtrace. `uri` is the internal `<root>/_graph` path; we show the root the user gave.
fn friendly_load_err(uri: &str, e: &anyhow::Error) -> String {
    let root = uri.trim_end_matches("/_graph");
    if e.to_string().contains("not found") {
        format!("no index at {root} — build one first: comind index <repo>")
    } else {
        format!("load from {root} failed: {e:#}")
    }
}

#[allow(clippy::too_many_arguments)] // CLI dispatch: flags map 1:1 to clap args
fn cmd_index(
    repo: &str,
    to: Option<&str>,
    embed: bool,
    enrich: bool,
    flows: bool,
    guide: bool,
    incremental: bool,
    since: Option<&str>,
    scip: comind::scip::Mode,
) -> ExitCode {
    // `--since` implies incremental.
    let incremental = incremental || since.is_some();
    let root = Path::new(repo);
    if !root.is_dir() {
        comind::ui::err(&format!("no such repo directory: {repo}"));
        return ExitCode::FAILURE;
    }
    let repo_name = repo_name(repo);
    // Same default location `link` writes to, so `search`/`serve` find it with zero config.
    let dst = format!(
        "{}/_graph",
        comind::config::Config::load()
            .index_dir(to)
            .trim_end_matches('/')
    );

    // Incremental path: diff against a base commit and reparse only what changed.
    if incremental {
        let base = since
            .map(str::to_string)
            .or_else(|| comind::index::read_repo_meta_blocking(&dst).ok().flatten());
        match base {
            Some(base) => return incremental_index(root, &repo_name, &dst, &base),
            None => comind::ui::note("no recorded base commit — doing a full index"),
        }
    }

    comind::ui::header(&format!("Indexing {repo_name}"));
    let mut out = match comind::parse::parse_repo(root, &repo_name) {
        Ok(o) => o,
        Err(e) => {
            comind::ui::err(&format!("{e:#}"));
            return ExitCode::FAILURE;
        }
    };
    if let Err(e) = attach_scip(scip, root, &repo_name, &mut out) {
        comind::ui::err(&e);
        return ExitCode::FAILURE;
    }
    // Bind provisional call/import edges to real definitions so ripple/thread/context_pack work.
    let resolved = comind::resolve::resolve(&out.symbols, &out.edges, &out.facts);
    out.edges = resolved.edges;
    out.symbols.extend(resolved.symbols);

    let mut by_kind: BTreeMap<String, usize> = BTreeMap::new();
    for s in &out.symbols {
        *by_kind.entry(format!("{:?}", s.kind)).or_default() += 1;
    }
    let calls = out
        .edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Calls)
        .count();
    let contains = out
        .edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Contains)
        .count();
    comind::ui::field("symbols", &out.symbols.len().to_string());
    for (k, n) in &by_kind {
        comind::ui::note(&format!("{k:<10} {n}"));
    }
    comind::ui::field(
        "edges",
        &format!("{} (contains={contains}, calls={calls})", out.edges.len()),
    );

    comind::ui::header("Persisting");
    comind::ui::step(&format!("writing graph to {dst}"));
    match comind::index::write_graph_blocking(&dst, &out.symbols, &out.edges)
        .and_then(|v| comind::index::write_facts_blocking(&dst, &out.facts).map(|_| v))
        .and_then(|v| {
            // Keep other repos' package rows already in a shared index dir.
            let mut rows: Vec<_> = comind::index::read_packages_blocking(&dst)
                .unwrap_or_default()
                .into_iter()
                .filter(|r| r.repo != repo_name)
                .collect();
            rows.extend(scan_packages(&[(repo_name.clone(), root.to_path_buf())]));
            comind::index::write_packages_blocking(&dst, &rows)?;
            let mut recipes: Vec<_> = comind::index::read_recipes_blocking(&dst)
                .unwrap_or_default()
                .into_iter()
                .filter(|r| r.repo != repo_name)
                .collect();
            recipes.extend(mine_recipes(&[(repo_name.clone(), root.to_path_buf())]));
            comind::index::write_recipes_blocking(&dst, &recipes).map(|_| v)
        }) {
        Ok((sv, ev)) => comind::ui::ok(&format!("graph: symbols v{sv}, edges v{ev}")),
        Err(e) => {
            comind::ui::err(&format!("write failed: {e:#}"));
            return ExitCode::FAILURE;
        }
    }
    // Record the HEAD commit so later runs can index incrementally.
    if let Ok(head) = comind::git::head_commit(root) {
        let _ = comind::index::write_repo_meta_blocking(&dst, &repo_name, &head);
        comind::ui::note(&format!("recorded commit {}", short(&head)));
    }

    // Optional enrichment steps — recompute everything (full index).
    let all: HashSet<String> = out.symbols.iter().map(|s| s.id.render()).collect();
    let cfg = comind::config::Config::load();
    if embed {
        if let ExitCode::FAILURE = run_embed(&dst, &out.symbols, &all, false) {
            return ExitCode::FAILURE;
        }
    }
    let repo_roots = vec![(repo_name.clone(), root.to_path_buf())];
    let (enrich, flows, guide) = gate_llm_steps(enrich, flows, guide);
    if enrich {
        if let ExitCode::FAILURE = run_enrich(
            &dst,
            &out.symbols,
            &out.edges,
            cfg.max_enrich(),
            &all,
            false,
        ) {
            return ExitCode::FAILURE;
        }
    }
    if guide {
        if let ExitCode::FAILURE = run_style_guides(&dst, &out.symbols, &out.edges, &repo_roots) {
            return ExitCode::FAILURE;
        }
    }
    if flows {
        if let ExitCode::FAILURE = run_flows(&dst, &out.symbols, &out.edges, cfg.max_flows()) {
            return ExitCode::FAILURE;
        }
    }
    if embed {
        comind::ui::note("next: comind search \"<question>\"  ·  comind serve");
    } else {
        comind::ui::note(
            "next: comind explore <symbol> (search needs embeddings — drop --no-embed)",
        );
    }
    ExitCode::SUCCESS
}

fn short(sha: &str) -> String {
    // Char-safe: a `--since` ref can be non-ASCII, so never byte-slice it.
    sha.chars().take(8).collect()
}

/// Incremental index: reparse only files changed since `base`, drop symbols/edges of
/// modified+deleted files, merge, and rewrite. Keeps the index fresh at near-diff cost.
fn incremental_index(root: &Path, repo_name: &str, dst: &str, base: &str) -> ExitCode {
    let head = match comind::git::head_commit(root) {
        Ok(h) => h,
        Err(e) => {
            comind::ui::err(&format!("{e:#}"));
            return ExitCode::FAILURE;
        }
    };
    let cs = match comind::git::changed_files(root, base) {
        Ok(c) => c,
        Err(e) => {
            comind::ui::err(&format!("{e:#}"));
            return ExitCode::FAILURE;
        }
    };
    comind::ui::header(&format!(
        "Incremental {} → {}: {} changed ({}+ {}~ {}-)",
        short(base),
        short(&head),
        cs.total(),
        cs.added.len(),
        cs.modified.len(),
        cs.deleted.len()
    ));
    if cs.total() == 0 {
        let _ = comind::index::write_repo_meta_blocking(dst, repo_name, &head);
        comind::ui::ok("index already up to date");
        return ExitCode::SUCCESS;
    }

    let (prior_syms, prior_edges) = match comind::index::read_graph_blocking(dst) {
        Ok(x) => x,
        Err(e) => {
            comind::ui::err(&format!(
                "no prior index at {dst} ({e:#}); run a full index first"
            ));
            return ExitCode::FAILURE;
        }
    };

    let dropped: HashSet<String> = cs.to_drop().cloned().collect();
    let to_parse: Vec<String> = cs.to_parse().cloned().collect();
    let newout = comind::parse::parse_files(root, repo_name, &to_parse);

    // Map this repo's symbol ids to their files so we can drop edges owned by changed files.
    let file_of: HashMap<String, String> = prior_syms
        .iter()
        .filter(|s| s.repo.0 == repo_name)
        .map(|s| (s.id.render(), s.file_path.clone()))
        .collect();
    let before = prior_syms.len();

    let mut symbols: Vec<Symbol> = prior_syms
        .into_iter()
        .filter(|s| !(s.repo.0 == repo_name && dropped.contains(&s.file_path)))
        .collect();
    let mut edges: Vec<Edge> = prior_edges
        .into_iter()
        .filter(|e| {
            file_of
                .get(&e.src.render())
                .is_none_or(|f| !dropped.contains(f))
        })
        .collect();
    symbols.extend(newout.symbols);
    edges.extend(newout.edges);
    // With stored facts, keep only structure and re-resolve everything (no stale edges).
    let facts = match comind::index::read_facts_blocking(dst) {
        Ok(Some(prior)) => {
            edges.retain(|e| e.kind == EdgeKind::Contains);
            let mut all: Vec<_> = prior
                .into_iter()
                .filter(|f| {
                    f.repo != repo_name
                        || !(dropped.contains(&f.path) || to_parse.contains(&f.path))
                })
                .collect();
            all.extend(newout.facts);
            all
        }
        _ => {
            comind::ui::warn("index has no stored facts; run a full index for exact edges");
            newout.facts
        }
    };
    // Contract nodes are derived; drop the old ones and re-derive from all facts.
    symbols.retain(|s| s.kind != SymbolKind::Contract);
    let resolved = comind::resolve::resolve(&symbols, &edges, &facts);
    symbols.extend(resolved.symbols);
    let edges = resolved.edges;

    if let Err(e) = comind::index::write_graph_blocking(dst, &symbols, &edges)
        .and_then(|_| comind::index::write_facts_blocking(dst, &facts))
    {
        comind::ui::err(&format!("write failed: {e:#}"));
        return ExitCode::FAILURE;
    }
    let _ = comind::index::write_repo_meta_blocking(dst, repo_name, &head);
    comind::ui::ok(&format!(
        "reparsed {} files → symbols {before}→{}, edges {}",
        to_parse.len(),
        symbols.len(),
        edges.len()
    ));
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::{short, truncate};

    #[test]
    fn truncate_is_char_safe() {
        // Multibyte chars near the boundary must not panic (byte-slicing would).
        assert_eq!(truncate("abc", 5), "abc");
        assert_eq!(truncate("日本語テキスト", 3), "日本…");
        assert_eq!(truncate("—↳“smart”", 3), "—↳…");
        assert_eq!(truncate("x", 0), "…"); // n == 0 must not underflow
    }

    #[test]
    fn short_is_char_safe() {
        assert_eq!(short("abcdef1234"), "abcdef12");
        assert_eq!(short("日本語branch"), "日本語branc"); // 8 chars, no byte-boundary panic
        assert_eq!(short("abc"), "abc");
    }
}
