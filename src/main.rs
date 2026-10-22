//! Comindly CLI — the single distributable binary.
//!
//! Subcommands: `index`, `link`, `explore`, `search`, `find`, `repos`, `stats`, `guide`,
//! `flow`, `changed`, `serve`, `config`, `eval`.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;
use std::process::ExitCode;

use comindly::model::{Edge, EdgeKind, Symbol, SymbolKind};

use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};

/// Cross-repo code intelligence for coding agents.
///
/// `--index-dir` defaults to the configured index dir (`comindly config path`) and accepts a folder
/// or an `s3://`, `gs://` or `az://` location. The same value is used to build and to read an index.
#[derive(Parser)]
#[command(name = "comindly", version, about, long_about = None)]
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
        /// Index directory — root; default: configured index dir (a folder, or an s3://, gs:// or az:// location)
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
        /// Skip every LLM step (enrich, flows, guide), whatever the config says
        #[arg(long = "no-llm")]
        no_llm: bool,
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
        /// Repository folders, indexed as they are; none: the configured list (`comindly repos`),
        /// each at its default branch in comindly's own worktree
        repos: Vec<String>,
        /// Index directory — root; default: configured index dir (a folder, or an s3://, gs:// or az:// location)
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
        /// Skip every LLM step (enrich, flows, guide), whatever the config says
        #[arg(long = "no-llm")]
        no_llm: bool,
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
        /// Index directory to read — root; default: configured index dir (a folder, or an s3://, gs:// or az:// location)
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
        /// Index directory to read — root; default: configured index dir (a folder, or an s3://, gs:// or az:// location)
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
        /// Index directory to read — root; default: configured index dir (a folder, or an s3://, gs:// or az:// location)
        #[arg(long = "index-dir")]
        index_dir: Option<String>,
    },
    /// Run the MCP server over stdio
    Serve {
        /// Index directory to read — root; default: configured index dir (a folder, or an s3://, gs:// or az:// location)
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
    /// Indexed repositories with their symbol counts, and the list `comindly link` indexes when
    /// given no folders (`add`, `remove`, `scan`)
    Repos {
        #[command(subcommand)]
        what: Option<ReposWhat>,
        /// Index directory to read — root; default: configured index dir
        #[arg(long = "index-dir", global = true)]
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
    /// Print per-repo inferred coding style guides (built with --guide)
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
    /// Confirm, archive, invalidate, supersede, dispute or retract a ledger record
    Review {
        id: String,
        /// confirm | archive (no longer applies) | invalidate (was wrong) | supersede | dispute | retract
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
        /// Fold new inbox records into the Lance snapshot and drop erased ones
        #[arg(long)]
        compact: bool,
    },
    /// Erase ledger records (and relations about them) by id or by author; dry run without --yes
    Erase {
        /// Record ids or id prefixes
        ids: Vec<String>,
        /// Every record by this pseudonymous actor (`me` for yours)
        #[arg(long)]
        actor: Option<String>,
        /// Delete for real
        #[arg(long)]
        yes: bool,
        #[arg(long = "index-dir")]
        index_dir: Option<String>,
    },
    /// Terminal UI (or `--web`: local web UI) for overview, contracts, ledger, explorer and conventions
    Ui {
        #[arg(long = "index-dir")]
        index_dir: Option<String>,
        /// Serve the web UI instead of the terminal UI
        #[arg(long)]
        web: bool,
        #[arg(long, default_value_t = 7878)]
        port: u16,
        /// With `--web`: the address to listen on (`0.0.0.0` in a container)
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        /// With `--web`: open it in the browser
        #[arg(long)]
        open: bool,
    },
    /// Team settings shared by everyone using this index (LLM, embedding model), and your own API key
    Team {
        #[command(subcommand)]
        what: Option<TeamWhat>,
        #[arg(long = "index-dir", global = true)]
        index_dir: Option<String>,
    },
    /// Use a team's index on this machine: remember its location
    Join {
        /// The team's index (a folder, s3://, gs:// or az:// location)
        index: String,
    },
    /// The web UI in its own window (macOS)
    App {
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
        /// Exit non-zero when a `must` constraint check fails in a changed file (CI)
        #[arg(long)]
        strict: bool,
        #[arg(long = "index-dir")]
        index_dir: Option<String>,
    },
    /// Check this machine's setup: index, ledger, hooks, keys, signing, SCIP indexers
    Doctor {
        #[arg(long = "index-dir")]
        index_dir: Option<String>,
    },
    /// Local mirror of a remote index (used by hooks)
    Cache {
        #[command(subcommand)]
        what: CacheWhat,
    },
    /// Agent hook adapter: reads the hook's JSON on stdin, prints context to inject (stdout JSON)
    Hook {
        /// Agent: `claude` (JSON for Claude Code), or `cursor` | `gemini` | `codex` | `generic`
        /// (plain markdown on stdout; the edited file is found in the payload or `COMINDLY_HOOK_FILE`)
        agent: String,
        /// Event: `session-start` | `pre-edit`
        event: String,
        #[arg(long = "index-dir")]
        index_dir: Option<String>,
    },
    /// Print the hook configuration to add to an agent's settings
    InitHooks {
        /// Agent: `claude` (JSON for Claude Code), or `cursor` | `gemini` | `codex` | `generic`
        /// (plain markdown on stdout; the edited file is found in the payload or `COMINDLY_HOOK_FILE`)
        agent: String,
    },
    /// Coding conventions per repo and org-wide: measured from the code, plus the team's own
    Conventions {
        #[command(subcommand)]
        what: Option<ConventionsWhat>,
        /// Index directory — root; default: configured index dir (a folder, or an s3://, gs:// or az:// location)
        #[arg(long = "index-dir", global = true)]
        index_dir: Option<String>,
        /// Only this repo (and the org baseline); with `add`, the repo it applies to (default: org-wide)
        #[arg(long, global = true)]
        repo: Option<String>,
    },
    /// Internal packages: who publishes each, who depends on it, and at which version
    Packages {
        /// Index directory to read — root; default: configured index dir (a folder, or an s3://, gs:// or az:// location)
        #[arg(long = "index-dir")]
        index_dir: Option<String>,
        /// Only consumers pinned to a version other than the publisher's
        #[arg(long)]
        skew: bool,
    },
    /// Service contracts (HTTP routes, queues, topics, buckets, tables) and who uses them
    Contracts {
        /// Index directory to read — root; default: configured index dir (a folder, or an s3://, gs:// or az:// location)
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
enum TeamWhat {
    /// Set team settings: name, llm_model, llm_base_url, embed_model, enrich, flows, guide (empty removes)
    Set {
        #[arg(required = true, value_name = "KEY=VALUE")]
        pairs: Vec<String>,
    },
    /// Store your LLM API key for this team in ~/.config/comindly/credentials (never shared)
    Key {
        /// Read the key from stdin instead of prompting
        #[arg(long)]
        stdin: bool,
        /// Store it as your default key, for every index
        #[arg(long)]
        default: bool,
    },
}

#[derive(Subcommand)]
enum ReposWhat {
    /// Add repository folders (your clones; comindly indexes their default branch)
    Add {
        #[arg(required = true)]
        paths: Vec<String>,
    },
    /// Remove repositories by name or folder
    Remove {
        #[arg(required = true)]
        repos: Vec<String>,
    },
    /// List git repositories under a folder
    Scan {
        dir: String,
        #[arg(long, default_value_t = 2)]
        depth: usize,
    },
}

#[derive(Subcommand)]
enum ConventionsWhat {
    /// Add a convention
    Add {
        rule: String,
        /// e.g. naming, errors, testing, libraries
        #[arg(long, default_value = "team")]
        dimension: String,
    },
    /// Rewrite a convention's rule (id from the listing)
    Edit { id: String, rule: String },
    /// Remove a convention; a measured one is hidden and can be restored
    Rm { id: String },
    /// Bring back a hidden or edited measured convention as measured
    Restore { id: String },
}

#[derive(Subcommand)]
enum CacheWhat {
    /// Bring the local mirror of a remote index up to date
    Refresh {
        #[arg(long = "index-dir")]
        index_dir: Option<String>,
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
    let matches = Cli::command().get_matches();
    // Commands that index, search, serve or show the UI run with the index's team settings.
    if let Some((name, sub)) = matches.subcommand() {
        let team = [
            "index", "link", "serve", "ui", "app", "search", "doctor", "guide", "flow",
        ];
        if team.contains(&name) {
            let flag = sub.try_get_one::<String>("index_dir").ok().flatten();
            let root = comindly::config::Config::load().index_dir(flag.map(String::as_str));
            let _ = comindly::team::apply(&root);
        }
    }
    let cli = match Cli::from_arg_matches(&matches) {
        Ok(c) => c,
        Err(e) => e.exit(),
    };
    match cli.cmd {
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
            no_llm,
            incremental,
            since,
            scip,
        } => {
            let cfg = comindly::config::Config::load();
            let Some(scip) = scip_mode(scip.as_deref(), &cfg) else {
                return ExitCode::FAILURE;
            };
            cmd_index(
                &repo,
                index_dir.as_deref(),
                tri(embed, no_embed).unwrap_or(cfg.embed()),
                !no_llm && tri(enrich, no_enrich).unwrap_or(cfg.enrich()),
                !no_llm && tri(flows, no_flows).unwrap_or(cfg.flows()),
                !no_llm && tri(guide, no_guide).unwrap_or(cfg.guide()),
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
            no_llm,
            incremental,
            scip,
        } => {
            let cfg = comindly::config::Config::load();
            let Some(scip) = scip_mode(scip.as_deref(), &cfg) else {
                return ExitCode::FAILURE;
            };
            let repos = if repos.is_empty() {
                match configured_checkouts(&cfg) {
                    Ok(r) => r,
                    Err(e) => {
                        comindly::ui::err(&comindly::storage::explain(&format!("{e:#}")));
                        return ExitCode::FAILURE;
                    }
                }
            } else {
                repos
            };
            let repos: Vec<&str> = repos.iter().map(String::as_str).collect();
            cmd_link(
                &repos,
                index_dir.as_deref(),
                tri(embed, no_embed).unwrap_or(cfg.embed()),
                !no_llm && tri(enrich, no_enrich).unwrap_or(cfg.enrich()),
                !no_llm && tri(flows, no_flows).unwrap_or(cfg.flows()),
                !no_llm && tri(guide, no_guide).unwrap_or(cfg.guide()),
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
            let fmt = fmt.unwrap_or_else(|| comindly::config::Config::load().format());
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
        Cmd::Repos { what, index_dir } => cmd_repo_list(what, index_dir.as_deref()),
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
            let fmt = fmt.unwrap_or_else(|| comindly::config::Config::load().format());
            let markdown = !matches!(fmt.as_str(), "json");
            cmd_serve(index_dir.as_deref(), markdown)
        }
        Cmd::Contracts {
            index_dir,
            kind,
            dangling,
        } => cmd_contracts(index_dir.as_deref(), kind.as_deref(), dangling),
        Cmd::Packages { index_dir, skew } => cmd_packages(index_dir.as_deref(), skew),
        Cmd::Conventions {
            what: None,
            index_dir,
            repo,
        } => cmd_conventions(index_dir.as_deref(), repo.as_deref()),
        Cmd::Conventions {
            what: Some(what),
            index_dir,
            repo,
        } => cmd_conventions_change(index_dir.as_deref(), repo.as_deref(), what),
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
            compact,
        } => cmd_ledger(index_dir.as_deref(), inbox, check, compact),
        Cmd::Hook {
            agent,
            event,
            index_dir,
        } => cmd_hook(&agent, &event, index_dir.as_deref()),
        Cmd::InitHooks { agent } => cmd_init_hooks(&agent),
        Cmd::Doctor { index_dir } => cmd_doctor(index_dir.as_deref()),
        Cmd::Cache {
            what: CacheWhat::Refresh { index_dir },
        } => {
            let root = comindly::config::Config::load().index_dir(index_dir.as_deref());
            let Some(dir) = comindly::cache::dir(&root) else {
                comindly::ui::err("no HOME for the cache");
                return ExitCode::FAILURE;
            };
            match comindly::cache::refresh(&root, &dir) {
                Ok(copied) => {
                    comindly::ui::ok(if copied {
                        "mirror updated (graph copied)"
                    } else {
                        "mirror up to date"
                    });
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    comindly::ui::err(&comindly::storage::explain(&format!("{e:#}")));
                    ExitCode::FAILURE
                }
            }
        }
        Cmd::Preflight {
            repo,
            base,
            strict,
            index_dir,
        } => cmd_preflight(&repo, &base, strict, index_dir.as_deref()),
        Cmd::Ui {
            index_dir,
            web,
            port,
            host,
            open,
        } => cmd_ui(index_dir.as_deref(), web.then_some((host, port)), open),
        Cmd::App { index_dir } => cmd_app(index_dir.as_deref()),
        Cmd::Team { what, index_dir } => cmd_team(what, index_dir.as_deref()),
        Cmd::Join { index } => cmd_join(&index),
        Cmd::Erase {
            ids,
            actor,
            yes,
            index_dir,
        } => cmd_erase(&ids, actor.as_deref(), yes, index_dir.as_deref()),

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
    if (enrich || flows || guide) && comindly::llm::LlmClient::from_env().is_err() {
        comindly::ui::warn(
            "enrich/flows/guide skipped — set OPENAI_API_KEY (or COMINDLY_LLM_BASE_URL) to enable",
        );
        (false, false, false)
    } else {
        (enrich, flows, guide)
    }
}

/// `--scip` flag → config → `auto`; `None` (after an error message) for an unknown value.
fn scip_mode(flag: Option<&str>, cfg: &comindly::config::Config) -> Option<comindly::scip::Mode> {
    match flag {
        None => Some(cfg.scip()),
        Some(f) => comindly::scip::Mode::parse(f).or_else(|| {
            comindly::ui::err(&format!("--scip must be auto, require or off (got {f:?})"));
            None
        }),
    }
}

/// Attach SCIP to one parsed repo, reporting what was found.
fn attach_scip(
    mode: comindly::scip::Mode,
    root: &Path,
    name: &str,
    out: &mut comindly::parse::ParseOutput,
) -> Result<(), String> {
    match comindly::scip::ingest(mode, root, name, &out.symbols, &mut out.facts) {
        Ok(a) if a.documents > 0 => {
            comindly::ui::note(&format!(
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
        let o = comindly::parse::parse_repo(Path::new(path), &name)
            .map_err(|e| format!("{name}: parse failed: {e:#}"))?;
        symbols.extend(o.symbols);
        edges.extend(o.edges);
        facts.extend(o.facts);
    }
    let resolved = comindly::resolve::resolve(&symbols, &edges, &facts);
    symbols.extend(resolved.symbols);
    Ok((symbols, resolved.edges))
}

/// `comindly explore <focus> <repo>...` — the agent-facing view: zoom, blast radius, and a
/// token-budgeted context pack for one symbol, across the whole corpus.
fn cmd_explore(focus: &str, index_dir: Option<&str>, repos: &[&str]) -> ExitCode {
    // With repo paths and no --index-dir, parse them on the fly; otherwise read the prebuilt
    // index (flag or configured default — comindly resolves the internal dataset path).
    let (symbols, edges) = if index_dir.is_none() && !repos.is_empty() {
        match parse_and_resolve(repos) {
            Ok(x) => x,
            Err(e) => {
                comindly::ui::err(&e);
                return ExitCode::FAILURE;
            }
        }
    } else {
        let uri = comindly::config::Config::load().graph_dir(index_dir);
        match comindly::index::read_graph_blocking(&uri) {
            Ok(x) => x,
            Err(e) => {
                comindly::ui::err(&friendly_load_err(&uri, &e));
                return ExitCode::FAILURE;
            }
        }
    };
    let g = comindly::graph::CodeGraph::build(&symbols, &edges);
    let dangling = if g.dangling_edges > 0 {
        format!(", {} dangling", g.dangling_edges)
    } else {
        String::new()
    };
    comindly::ui::note(&format!(
        "graph: {} symbols, {} edges{dangling}",
        g.node_count(),
        g.edge_count(),
    ));

    let Some(idx) = g.lookup(focus) else {
        comindly::ui::err(&format!("no symbol matching `{focus}`"));
        return ExitCode::FAILURE;
    };

    // zoom
    let z = g.zoom(idx);
    if let Some(f) = &z.focus {
        comindly::ui::header(&format!(
            "● {} [{}]  {}  {}",
            f.name, f.kind, f.repo, f.location
        ));
        if let Some(sig) = &f.signature {
            comindly::ui::note(sig);
        }
    }
    if let Some(c) = &z.container {
        // Drop the redundant location when the container is the file itself.
        if c.kind.eq_ignore_ascii_case("file") {
            comindly::ui::field("in", &c.name);
        } else {
            comindly::ui::field("in", &format!("{}  ({})", c.name, c.location));
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
    comindly::ui::header(&format!(
        "ripple (blast radius, ≤4 hops): {} dependents",
        hits.len()
    ));
    if !by_repo.is_empty() {
        let mut t = comindly::ui::table(&["repo", "dependents"]);
        for (repo, n) in &by_repo {
            t.add_row(vec![repo.clone(), n.to_string()]);
        }
        comindly::ui::right_align(&mut t, &[1]); // dependents
        println!("{t}");
    }

    // context pack — the minimal correct read-set within a token budget
    const BUDGET: usize = 1500;
    let pack = g.context_pack(idx, BUDGET);
    let total: usize = pack.iter().map(|(_, t)| t).sum();
    comindly::ui::header(&format!(
        "context pack for changing `{}` (~{total} tokens, budget {BUDGET}): {}",
        z.focus.as_ref().map_or("?", |f| f.name.as_str()),
        plural(pack.len(), "symbol")
    ));
    if !pack.is_empty() {
        let mut t = comindly::ui::table(&["symbol", "location"]);
        for (node, _) in pack.iter().take(15) {
            t.add_row(vec![node.name.clone(), node.location.clone()]);
        }
        println!("{t}");
    }
    ExitCode::SUCCESS
}

fn print_group(label: &str, nodes: &[comindly::graph::Node], limit: usize) {
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
    comindly::ui::field(label, &format!("{}{suffix}", names.join(", ")));
}

/// `comindly search <query...> --from <uri>` — semantic search reranked by graph centrality,
/// definition-boost, and test-file penalty (semble's fusion ideas + our unique graph signal).
fn cmd_search(query: &str, from: Option<&str>, repos: &[&str], markdown: bool) -> ExitCode {
    let uri = comindly::config::Config::load().graph_dir(from);
    let uri = uri.as_str();

    let (symbols, edges) = match comindly::index::read_graph_blocking(uri) {
        Ok(x) => x,
        Err(e) => {
            comindly::ui::err(&friendly_load_err(uri, &e));
            return ExitCode::FAILURE;
        }
    };
    let by_id: HashMap<String, Symbol> =
        symbols.iter().map(|s| (s.id.render(), s.clone())).collect();
    let g = comindly::graph::CodeGraph::build(&symbols, &edges);

    let embedder = match comindly::embed::Embedder::load_default() {
        Ok(e) => e,
        Err(e) => {
            comindly::ui::err(&format!("embedding model: {e:#}"));
            return ExitCode::FAILURE;
        }
    };

    // LLM enrichment (summaries + generated queries), if present — folded into ranking + shown.
    let enrich: HashMap<String, (String, Vec<String>)> =
        comindly::index::read_enrichment_blocking(uri)
            .ok()
            .flatten()
            .unwrap_or_default()
            .into_iter()
            .map(|(id, s, q)| (id.render(), (s, q)))
            .collect();

    // Native LanceDB hybrid retrieval (BM25 + vector, RRF-fused) + comindly's code-aware boosts
    // and dependency-graph centrality — shared with the `search` MCP tool. When scoping to a
    // repo, over-fetch then filter so we still return a full page.
    let limit = if repos.is_empty() { 12 } else { 60 };
    let mut hits = match comindly::search::hybrid_blocking(
        uri, &by_id, &g, &enrich, &embedder, query, limit,
    ) {
        Ok(h) => h,
        Err(e) => {
            comindly::ui::err(&format!(
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
        print!("{}", comindly::search::markdown(query, &hits));
    } else {
        comindly::ui::header(&format!("search: \"{query}\""));
        if hits.is_empty() {
            comindly::ui::note(
                "no matches — try a broader query (or the index was built with --no-embed)",
            );
            return ExitCode::SUCCESS;
        }
        let mut t =
            comindly::ui::table(&["score", "symbol", "kind", "deps", "location", "summary"]);
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
        comindly::ui::right_align(&mut t, &[0, 3]); // score, deps
        println!("{t}");
    }
    ExitCode::SUCCESS
}

/// Load the prebuilt graph for a read-only command, with a friendly missing-index error.
fn read_graph_or_err(from: Option<&str>) -> Result<(Vec<Symbol>, Vec<Edge>), ()> {
    let uri = comindly::config::Config::load().graph_dir(from);
    comindly::index::read_graph_blocking(&uri)
        .map_err(|e| comindly::ui::err(&friendly_load_err(&uri, &e)))
}

/// `comindly find <query>` — locate symbols by name/descriptor/path substring.
fn cmd_find(query: &str, from: Option<&str>, repos: &[&str]) -> ExitCode {
    let Ok((symbols, edges)) = read_graph_or_err(from) else {
        return ExitCode::FAILURE;
    };
    let g = comindly::graph::CodeGraph::build(&symbols, &edges);
    // Over-fetch when filtering by repo so a full page survives the filter.
    let mut hits = g.find(query, if repos.is_empty() { 30 } else { 200 });
    if !repos.is_empty() {
        hits.retain(|n| repos.iter().any(|r| n.repo.eq_ignore_ascii_case(r)));
        hits.truncate(30);
    }
    comindly::ui::header(&format!(
        "find: \"{query}\" ({})",
        plural(hits.len(), "match")
    ));
    if hits.is_empty() {
        comindly::ui::note("no symbols matched");
        return ExitCode::SUCCESS;
    }
    let mut t = comindly::ui::table(&["symbol", "kind", "repo", "location", "id"]);
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

/// `comindly repos` — list indexed repositories and their symbol counts.
fn cmd_repos(from: Option<&str>) -> ExitCode {
    let Ok((symbols, edges)) = read_graph_or_err(from) else {
        return ExitCode::FAILURE;
    };
    let g = comindly::graph::CodeGraph::build(&symbols, &edges);
    let repos = g.repos();
    comindly::ui::header(&format!("Indexed repositories ({})", repos.len()));
    if repos.is_empty() {
        comindly::ui::note("none");
        return ExitCode::SUCCESS;
    }
    let mut t = comindly::ui::table(&["repo", "symbols"]);
    for (r, n) in &repos {
        t.add_row(vec![r.clone(), n.to_string()]);
    }
    comindly::ui::right_align(&mut t, &[1]);
    println!("{t}");
    ExitCode::SUCCESS
}

/// `comindly stats [--repo x --repo y]` — per-repo symbols, edges, kinds, enrichment coverage.
/// With no `--repo`, reports every indexed repo separately.
fn cmd_stats(want: &[&str], from: Option<&str>) -> ExitCode {
    let Ok((symbols, edges)) = read_graph_or_err(from) else {
        return ExitCode::FAILURE;
    };
    // Ids of enriched symbols, to compute per-repo coverage.
    let uri = comindly::config::Config::load().graph_dir(from);
    let enriched: HashSet<String> = comindly::index::read_enrichment_blocking(&uri)
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
                comindly::ui::warn(&format!("no such repo in the index: {r}"));
            }
        }
        want.iter()
            .filter(|r| all.contains(**r))
            .map(|r| r.to_string())
            .collect()
    };
    if repos.is_empty() {
        comindly::ui::err("no matching repos");
        return ExitCode::FAILURE;
    }

    comindly::ui::header(&format!(
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
        comindly::ui::header(&format!("● {repo}"));
        comindly::ui::field("symbols", &rsyms.len().to_string());
        comindly::ui::field("edges", &redges.to_string());
        comindly::ui::field("enriched", &format!("{renr} / {}", rsyms.len()));

        let mut by_kind: BTreeMap<String, usize> = BTreeMap::new();
        for s in &rsyms {
            *by_kind.entry(format!("{:?}", s.kind)).or_default() += 1;
        }
        let mut kt = comindly::ui::table(&["kind", "count"]);
        for (k, n) in &by_kind {
            kt.add_row(vec![k.clone(), n.to_string()]);
        }
        comindly::ui::right_align(&mut kt, &[1]);
        println!("{kt}");
    }
    ExitCode::SUCCESS
}

/// `comindly guide [--repo x --repo y] [--output f]` — per-repo inferred coding style guides
/// (built with `--enrich`). With no `--repo`, prints every repo's guide; `--output` writes to a
/// file instead of the terminal.
fn cmd_guide(want: &[&str], from: Option<&str>, output: Option<&str>) -> ExitCode {
    let uri = comindly::config::Config::load().graph_dir(from);
    let guides = match comindly::index::read_style_guide_blocking(&uri) {
        Ok(g) => g,
        Err(e) => {
            comindly::ui::err(&friendly_load_err(&uri, &e));
            return ExitCode::FAILURE;
        }
    };
    if guides.is_empty() {
        comindly::ui::note("no style guide yet — build the index with --enrich");
        return ExitCode::SUCCESS;
    }
    for r in want {
        if !guides.iter().any(|(repo, _)| repo.eq_ignore_ascii_case(r)) {
            comindly::ui::warn(&format!("no style guide for repo: {r}"));
        }
    }
    let selected: Vec<&(String, String)> = guides
        .iter()
        .filter(|(repo, _)| want.is_empty() || want.iter().any(|r| repo.eq_ignore_ascii_case(r)))
        .collect();
    if selected.is_empty() {
        comindly::ui::err("no matching repos");
        return ExitCode::FAILURE;
    }
    if let Some(path) = output {
        let doc: String = selected
            .iter()
            .map(|(repo, guide)| format!("# Style guide — {repo}\n\n{guide}\n"))
            .collect::<Vec<_>>()
            .join("\n");
        match std::fs::write(path, doc) {
            Ok(()) => comindly::ui::ok(&format!(
                "style guide{} written to {path}",
                if selected.len() > 1 { "s" } else { "" }
            )),
            Err(e) => {
                comindly::ui::err(&format!("write {path} failed: {e}"));
                return ExitCode::FAILURE;
            }
        }
    } else {
        for (repo, guide) in selected {
            comindly::ui::header(&format!("Style guide — {repo}"));
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

/// `comindly changed <repo> [--since <sha>]` — show files changed since a commit (incremental
/// indexing targeting). With no `--since`, just prints the current HEAD commit.
fn cmd_changed(repo: &str, since: Option<&str>) -> ExitCode {
    let path = Path::new(repo);

    let head = match comindly::git::head_commit(path) {
        Ok(h) => h,
        Err(e) => {
            comindly::ui::err(&comindly::storage::explain(&format!("{e:#}")));
            return ExitCode::FAILURE;
        }
    };
    comindly::ui::field("HEAD", &head);

    let Some(base) = since else {
        comindly::ui::note("pass --since <commit> to list changed files");
        return ExitCode::SUCCESS;
    };
    match comindly::git::changed_files(path, base) {
        Ok(cs) => {
            comindly::ui::header(&format!(
                "changed since {base}: {} files ({} added, {} modified, {} deleted)",
                cs.total(),
                cs.added.len(),
                cs.modified.len(),
                cs.deleted.len()
            ));
            for p in cs.added.iter().take(10) {
                comindly::ui::note(&format!("+ {p}"));
            }
            for p in cs.modified.iter().take(10) {
                comindly::ui::note(&format!("~ {p}"));
            }
            for p in cs.deleted.iter().take(10) {
                comindly::ui::note(&format!("- {p}"));
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            comindly::ui::err(&comindly::storage::explain(&format!("{e:#}")));
            ExitCode::FAILURE
        }
    }
}

/// `comindly serve --from <lancedb-uri>` — run the MCP server over stdio. Only the MCP
/// protocol goes to stdout; diagnostics go to stderr so the stream stays clean.
fn cmd_serve(from: Option<&str>, markdown: bool) -> ExitCode {
    let uri = comindly::config::Config::load().graph_dir(from);
    let uri = uri.as_str();
    comindly::ui::step(&format!("loading graph from {uri}"));
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            comindly::ui::err(&format!("runtime: {e}"));
            return ExitCode::FAILURE;
        }
    };
    match rt.block_on(comindly::mcp::serve_stdio(uri, markdown)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            comindly::ui::err(&friendly_load_err(uri, &e));
            ExitCode::FAILURE
        }
    }
}

/// `comindly config <path|init>` — show or scaffold the config file.
fn human() -> comindly::ledger::Provenance {
    comindly::ledger::Provenance::human("cli")
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
    let Some(kind) = comindly::ledger::Kind::parse(kind) else {
        comindly::ui::err("kind must be decision, attempt, constraint, gotcha, intent or question");
        return ExitCode::FAILURE;
    };
    let cfg = comindly::config::Config::load();
    let (symbols, edges) = match comindly::index::read_graph_blocking(&cfg.graph_dir(from)) {
        Ok(x) => x,
        Err(e) => {
            comindly::ui::err(&format!(
                "no index to anchor to: {}",
                comindly::storage::explain(&format!("{e:#}"))
            ));
            return ExitCode::FAILURE;
        }
    };
    let g = comindly::graph::CodeGraph::build(&symbols, &edges);
    let by_id: HashMap<String, &Symbol> = symbols.iter().map(|s| (s.id.render(), s)).collect();
    let mut anchors = Vec::new();
    for focus in on {
        match g.lookup(focus).and_then(|i| by_id.get(&g.node(i).id)) {
            Some(s) => anchors.push(comindly::ledger::Anchor::of(s)),
            None => {
                comindly::ui::err(&format!(
                    "no symbol matches `{focus}` (try `comindly find`)"
                ));
                return ExitCode::FAILURE;
            }
        }
    }
    let mut rec = comindly::ledger::Record::new(kind, title, anchors, human());
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
    match comindly::ledger::put_blocking(&cfg.index_dir(from), &rec) {
        Ok(()) => {
            comindly::ui::ok(&format!(
                "recorded {} `{}` ({})",
                kind.as_str(),
                rec.title,
                rec.id
            ));
            ExitCode::SUCCESS
        }
        Err(e) => {
            comindly::ui::err(&comindly::storage::explain(&format!("{e:#}")));
            ExitCode::FAILURE
        }
    }
}

fn cmd_why(focus: &str, from: Option<&str>) -> ExitCode {
    let cfg = comindly::config::Config::load();
    let (symbols, edges) = match comindly::index::read_graph_blocking(&cfg.graph_dir(from)) {
        Ok(x) => x,
        Err(e) => {
            comindly::ui::err(&format!("no index: {e:#}"));
            return ExitCode::FAILURE;
        }
    };
    let g = comindly::graph::CodeGraph::build(&symbols, &edges);
    let Some((name, ids)) = g.scope_of(focus) else {
        comindly::ui::err(&format!("no symbol matches `{focus}`"));
        return ExitCode::FAILURE;
    };
    let records = comindly::ledger::all_blocking(&cfg.index_dir(from)).unwrap_or_default();
    let entries = comindly::ledger::derive(&records, &symbols);
    let about = comindly::ledger::about(&entries, &ids);
    println!(
        "{}",
        comindly::ledger::markdown(&format!("why `{name}`"), &about)
    );
    ExitCode::SUCCESS
}

fn cmd_review(id: &str, action: &str, reason: &str, from: Option<&str>) -> ExitCode {
    let root = comindly::config::Config::load().index_dir(from);
    let records = comindly::ledger::all_blocking(&root).unwrap_or_default();
    let Some(target) = records.iter().find(|r| r.id == id || r.id.starts_with(id)) else {
        comindly::ui::err(&format!("no ledger record `{id}`"));
        return ExitCode::FAILURE;
    };
    let put = comindly::ledger::Record::review(target, action, reason, human())
        .and_then(|rec| comindly::ledger::put_blocking(&root, &rec));
    match put {
        Ok(()) => {
            comindly::ui::ok(&format!("{action}: `{}`", target.title));
            ExitCode::SUCCESS
        }
        Err(e) => {
            comindly::ui::err(&comindly::storage::explain(&format!("{e:#}")));
            ExitCode::FAILURE
        }
    }
}

fn cmd_ledger(from: Option<&str>, inbox: bool, check: bool, compact: bool) -> ExitCode {
    let cfg = comindly::config::Config::load();
    if compact {
        return match comindly::ledger::compact_blocking(&cfg.index_dir(from)) {
            Ok((added, removed)) => {
                comindly::ui::ok(&format!(
                    "ledger compacted: {added} added, {removed} erased record(s) dropped"
                ));
                ExitCode::SUCCESS
            }
            Err(e) => {
                comindly::ui::err(&comindly::storage::explain(&format!("{e:#}")));
                ExitCode::FAILURE
            }
        };
    }
    let (symbols, edges) =
        comindly::index::read_graph_blocking(&cfg.graph_dir(from)).unwrap_or_default();
    let records = match comindly::ledger::all_blocking(&cfg.index_dir(from)) {
        Ok(r) => r,
        Err(e) => {
            comindly::ui::err(&comindly::storage::explain(&format!("{e:#}")));
            return ExitCode::FAILURE;
        }
    };
    let mut entries = comindly::ledger::derive(&records, &symbols);
    if check {
        let packages =
            comindly::index::read_packages_blocking(&cfg.graph_dir(from)).unwrap_or_default();
        let findings = comindly::checks::run(&entries, &symbols, &edges, &packages);
        if findings.is_empty() {
            comindly::ui::ok("no constraint carries a check (add one with --field check=...)");
            return ExitCode::SUCCESS;
        }
        println!(
            "## Constraint checks\n\n{}",
            comindly::checks::markdown(&findings, None)
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
    let packages =
        comindly::index::read_packages_blocking(&cfg.graph_dir(from)).unwrap_or_default();
    let findings = comindly::checks::run(&entries, &symbols, &edges, &packages);
    comindly::checks::promote(&mut entries, &findings);
    let shown = comindly::ledger::inbox(&entries, inbox);
    let title = if inbox { "Ledger inbox" } else { "Ledger" };
    println!("{}", comindly::ledger::markdown(title, &shown));
    let revived = comindly::checks::revivals(&entries, &packages);
    if !revived.is_empty() {
        println!("## Retry conditions met\n");
        for (e, why) in revived {
            println!("- {} — {why}", comindly::ledger::line(e));
        }
    }
    ExitCode::SUCCESS
}

fn cmd_erase(ids: &[String], actor: Option<&str>, yes: bool, from: Option<&str>) -> ExitCode {
    if ids.is_empty() && actor.is_none() {
        comindly::ui::err("name record ids or --actor (`me` for your own records)");
        return ExitCode::FAILURE;
    }
    if let Some(short) = ids.iter().find(|i| i.len() < 8) {
        comindly::ui::err(&format!("id prefix `{short}` is too short (8+ characters)"));
        return ExitCode::FAILURE;
    }
    let cfg = comindly::config::Config::load();
    let all = comindly::ledger::all_blocking(&cfg.index_dir(from)).unwrap_or_default();
    for i in ids {
        let n = all.iter().filter(|r| r.id.starts_with(i.as_str())).count();
        if n != 1 {
            comindly::ui::err(&format!(
                "id prefix `{i}` matches {n} records; give a longer one"
            ));
            return ExitCode::FAILURE;
        }
    }
    let actor = actor.map(|a| {
        if a == "me" {
            comindly::ledger::actor()
        } else {
            a.to_string()
        }
    });
    let pick = |r: &comindly::ledger::Record| {
        ids.iter().any(|i| r.id.starts_with(i.as_str()))
            || actor.as_deref() == Some(r.provenance.actor.as_str())
    };
    match comindly::ledger::erase_blocking(&cfg.index_dir(from), pick, yes) {
        Ok(gone) => {
            for r in &gone {
                println!("- {} `{}` ({})", r.kind.as_str(), r.title, r.id);
            }
            if yes {
                comindly::ui::ok(&format!("erased {} record(s)", gone.len()));
            } else {
                comindly::ui::ok(&format!(
                    "{} record(s) would be erased; re-run with --yes",
                    gone.len()
                ));
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            comindly::ui::err(&comindly::storage::explain(&format!("{e:#}")));
            ExitCode::FAILURE
        }
    }
}

fn cmd_ui(from: Option<&str>, web: Option<(String, u16)>, open: bool) -> ExitCode {
    use anyhow::Context as _;
    let root = comindly::config::Config::load().index_dir(from);
    let load = match web {
        Some(_) => Ok(comindly::tui::Data::load_or_empty(&root)),
        None => comindly::tui::Data::load(&root),
    };
    let run = load.and_then(|data| match web {
        Some((host, port)) => {
            let addr = format!("{host}:{port}");
            let listener =
                std::net::TcpListener::bind(&addr).with_context(|| format!("bind {addr}"))?;
            if let Some(url) = comindly::server::public_url() {
                anyhow::ensure!(
                    !comindly::server::admin_emails().is_empty(),
                    "server mode needs COMINDLY_ADMIN_EMAILS: who administers this server"
                );
                let sign_in = comindly::server::sign_in()?;
                if let Some(t) = comindly::server::env_secret("COMINDLY_ADMIN_TOKEN") {
                    anyhow::ensure!(
                        t.len() >= 32,
                        "COMINDLY_ADMIN_TOKEN is too short: use at least 32 random characters (e.g. `openssl rand -base64 32`)"
                    );
                }
                comindly::server::init(&root).context("open the server table")?;
                comindly::ui::ok(&format!("comindly server for {url}, listening on {addr}"));
                comindly::ui::note(match sign_in {
                    comindly::server::SignIn::Email => "members sign in by email",
                    comindly::server::SignIn::Code => "members sign in with codes admins hand out",
                });
                return comindly::web::serve(data, listener, comindly::web::Access::Server);
            }
            let token = comindly::config::ui_token()?;
            let url = format!("http://localhost:{port}/?token={token}");
            comindly::ui::ok(&format!("comindly web UI on {url} (Ctrl-C to stop)"));
            if open {
                open_in_browser(&url);
            }
            comindly::web::serve(data, listener, comindly::web::Access::Local(token))
        }
        None => comindly::tui::App::new(data).run(),
    });
    match run {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            comindly::ui::err(&comindly::storage::explain(&format!("{e:#}")));
            ExitCode::FAILURE
        }
    }
}

fn cmd_repo_list(what: Option<ReposWhat>, from: Option<&str>) -> ExitCode {
    use comindly::config::RepoEntry;
    let cfg = comindly::config::Config::load();
    let mut list = cfg.repos.clone();
    let absolute = |p: &str| {
        std::fs::canonicalize(comindly::config::expand_tilde(p))
            .map(|c| c.to_string_lossy().into_owned())
            .unwrap_or_else(|_| p.to_string())
    };
    let changed = match what {
        None => {
            let shown = cmd_repos(from);
            comindly::ui::header(
                "Indexed by `comindly link` (default branch, comindly's own worktree)",
            );
            if list.is_empty() {
                comindly::ui::note("none yet: `comindly repos add <folder>`");
            }
            for r in &list {
                let p = Path::new(&r.path);
                let branch = comindly::worktree::default_branch(p).unwrap_or_else(|_| "?".into());
                comindly::ui::field(
                    &comindly::worktree::name(p),
                    &format!("{}  (indexes {branch})", r.path),
                );
            }
            let _ = shown;
            return ExitCode::SUCCESS;
        }
        Some(ReposWhat::Scan { dir, depth }) => {
            let listed: Vec<String> = list.iter().map(|r| absolute(&r.path)).collect();
            for p in
                comindly::worktree::scan(Path::new(&comindly::config::expand_tilde(&dir)), depth)
            {
                let abs = p.to_string_lossy().into_owned();
                let mark = if listed.contains(&abs) { "✓" } else { " " };
                println!("{mark} {abs}");
            }
            return ExitCode::SUCCESS;
        }
        Some(ReposWhat::Add { paths }) => {
            for p in paths {
                let entry = if p.contains("://") || p.starts_with("git@") {
                    match comindly::hosting::managed_path(&p) {
                        Ok(dest) => RepoEntry {
                            path: dest.to_string_lossy().into_owned(),
                            remote: Some(p.clone()),
                        },
                        Err(e) => {
                            comindly::ui::err(&format!("{e:#}"));
                            return ExitCode::FAILURE;
                        }
                    }
                } else {
                    let path = absolute(&p);
                    if !Path::new(&path).join(".git").exists() {
                        comindly::ui::err(&format!("{p} is not a git repository"));
                        return ExitCode::FAILURE;
                    }
                    RepoEntry { path, remote: None }
                };
                if !list
                    .iter()
                    .any(|r| absolute(&r.path) == absolute(&entry.path))
                {
                    list.push(entry);
                }
            }
            true
        }
        Some(ReposWhat::Remove { repos }) => {
            let before = list.len();
            list.retain(|r| {
                let name = comindly::worktree::name(Path::new(&r.path));
                let gone = repos
                    .iter()
                    .any(|x| *x == name || absolute(x) == absolute(&r.path));
                if gone {
                    let _ = comindly::worktree::remove(Path::new(&r.path));
                }
                !gone
            });
            before != list.len()
        }
    };
    if changed {
        if let Err(e) = comindly::config::set_repos(&list) {
            comindly::ui::err(&comindly::storage::explain(&format!("{e:#}")));
            return ExitCode::FAILURE;
        }
        comindly::ui::ok(&format!(
            "{} repositories; index them with `comindly link`",
            list.len()
        ));
    }
    ExitCode::SUCCESS
}

/// Read a secret: the environment variable, stdin, or a hidden prompt.
fn read_secret(env: &str, stdin: bool, prompt: &str) -> anyhow::Result<String> {
    if let Ok(v) = std::env::var(env) {
        if !v.is_empty() {
            return Ok(v);
        }
    }
    let v = if stdin {
        let mut s = String::new();
        std::io::stdin().read_line(&mut s)?;
        s
    } else {
        rpassword::prompt_password(prompt)?
    };
    let v = v.trim().to_string();
    anyhow::ensure!(!v.is_empty(), "nothing entered");
    Ok(v)
}

fn cmd_team(what: Option<TeamWhat>, from: Option<&str>) -> ExitCode {
    let root = comindly::config::Config::load().index_dir(from);
    match team_run(what, &root) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            comindly::ui::err(&comindly::storage::explain(&format!("{e:#}")));
            ExitCode::FAILURE
        }
    }
}

fn team_run(what: Option<TeamWhat>, root: &str) -> anyhow::Result<()> {
    use anyhow::Context as _;
    use comindly::team;
    let rt = comindly::ledger::runtime()?;
    let stored = rt.block_on(team::load(root))?;
    let mut s = stored.settings.clone();
    match what {
        None => {
            comindly::ui::header(&format!("Team settings of {root}"));
            let show = |k: &str, v: Option<String>, default: &str| {
                let shown = v.unwrap_or_else(|| format!("{default}  (comindly default)"));
                comindly::ui::field(k, &shown)
            };
            show("name", s.name.clone(), "none");
            show(
                "llm_base_url",
                s.llm_base_url.clone(),
                comindly::llm::DEFAULT_BASE_URL,
            );
            show(
                "llm_model",
                s.llm_model.clone(),
                comindly::llm::DEFAULT_MODEL,
            );
            show(
                "embed_model",
                s.embed_model.clone(),
                comindly::embed::DEFAULT_MODEL,
            );
            for (k, v) in [("enrich", s.enrich), ("flows", s.flows), ("guide", s.guide)] {
                show(k, v.map(|b| b.to_string()), "true");
            }
            let source = match comindly::credentials::llm_key(s.name.as_deref()).1 {
                comindly::credentials::Source::Team => "your key for this team (credentials file)",
                comindly::credentials::Source::Default => "your default key (credentials file)",
                comindly::credentials::Source::Environment => "OPENAI_API_KEY",
                comindly::credentials::Source::Server => "stored on this server",
                comindly::credentials::Source::None => "none: `comindly team key`",
            };
            comindly::ui::field("api key on this machine", source);
            if let (Some(by), Some(at)) = (stored.changed_by, stored.changed_at) {
                comindly::ui::note(&format!(
                    "last changed by {by} at {}",
                    &at[..at.len().min(16)]
                ));
            }
            return Ok(());
        }
        Some(TeamWhat::Key { stdin, default }) => {
            let team = if default { None } else { s.name.clone() };
            let key = read_secret("COMINDLY_NEW_LLM_KEY", stdin, "LLM API key: ")?;
            comindly::credentials::set_llm_key(team.as_deref(), &key)?;
            let r#for = team.map_or("your default".to_string(), |t| format!("team {t}"));
            comindly::ui::ok(&format!(
                "key saved for {} in {}",
                r#for,
                comindly::credentials::path().display()
            ));
            return Ok(());
        }
        Some(TeamWhat::Set { pairs }) => {
            for p in pairs {
                let (k, v) = p
                    .split_once('=')
                    .with_context(|| format!("`{p}` is not KEY=VALUE"))?;
                let v = v.trim();
                let text = (!v.is_empty()).then(|| v.to_string());
                let flag = || -> anyhow::Result<Option<bool>> {
                    Ok(if v.is_empty() {
                        None
                    } else {
                        Some(v.parse().with_context(|| format!("{k} is true or false"))?)
                    })
                };
                match k.trim() {
                    "name" => s.name = text,
                    "llm_model" => s.llm_model = text,
                    "llm_base_url" => s.llm_base_url = text,
                    "embed_model" => {
                        comindly::ui::warn(
                            "a new embedding model needs a full re-index (`comindly link`)",
                        );
                        s.embed_model = text
                    }
                    "enrich" => s.enrich = flag()?,
                    "flows" => s.flows = flag()?,
                    "guide" => s.guide = flag()?,
                    other => anyhow::bail!("unknown team setting `{other}`"),
                }
            }
        }
    }
    rt.block_on(team::save(root, &s, stored.version))?;
    comindly::ui::ok("team settings saved; everyone using this index gets them");
    Ok(())
}

fn cmd_join(index: &str) -> ExitCode {
    let root = comindly::config::expand_tilde(index.trim());
    let run = || -> anyhow::Result<()> {
        let rt = comindly::ledger::runtime()?;
        let graph = format!("{}/_graph", root.trim_end_matches('/'));
        let versions = rt.block_on(comindly::index::graph_versions(&graph))?;
        anyhow::ensure!(!versions.is_empty(), "no index at {root}");
        let stored = rt.block_on(comindly::team::load(&root))?;
        comindly::config::update(&[("index_dir", comindly::config::Setting::Text(root.clone()))])?;
        let name = stored.settings.name.clone();
        comindly::ui::ok(&format!("joined {}", name.as_deref().unwrap_or(&root)));
        if comindly::credentials::llm_key(name.as_deref()).0.is_none() {
            comindly::ui::note(
                "for LLM steps, add your API key: `comindly team key` (or set OPENAI_API_KEY)",
            );
        }
        comindly::ui::note("connect your agent: `comindly app` → Connect, or `claude mcp add --scope user comindly -- comindly serve`");
        Ok(())
    };
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            comindly::ui::err(&comindly::storage::explain(&format!("{e:#}")));
            ExitCode::FAILURE
        }
    }
}

fn cmd_app(from: Option<&str>) -> ExitCode {
    #[cfg(target_os = "macos")]
    {
        let root = comindly::config::Config::load().index_dir(from);
        match comindly::app::run(root) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                comindly::ui::err(&comindly::storage::explain(&format!("{e:#}")));
                ExitCode::FAILURE
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = from;
        comindly::ui::err("the desktop app is macOS only for now; use `comindly ui --web --open`");
        ExitCode::FAILURE
    }
}

fn open_in_browser(url: &str) {
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(opener).arg(url).spawn();
}

fn cmd_preflight(repo: &str, base: &str, strict: bool, from: Option<&str>) -> ExitCode {
    let root = Path::new(repo);
    let name = std::fs::canonicalize(root)
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_else(|| repo_name(repo));
    let raw = match comindly::git::working_changes(root, base) {
        Ok(c) => c,
        Err(e) => {
            comindly::ui::err(&comindly::storage::explain(&format!("{e:#}")));
            return ExitCode::FAILURE;
        }
    };
    if raw.is_empty() {
        comindly::ui::ok("no changes against the base");
        return ExitCode::SUCCESS;
    }
    let cfg = comindly::config::Config::load();
    let (symbols, edges) = match comindly::index::read_graph_blocking(&cfg.graph_dir(from)) {
        Ok(x) => x,
        Err(e) => {
            comindly::ui::err(&format!("no index: {e:#}"));
            return ExitCode::FAILURE;
        }
    };
    let changes: Vec<comindly::preflight::Change> = raw
        .into_iter()
        .map(|(path, lines)| comindly::preflight::Change {
            repo: name.clone(),
            path,
            lines,
        })
        .collect();
    let g = comindly::graph::CodeGraph::build(&symbols, &edges);
    let records = comindly::ledger::all_blocking(&cfg.index_dir(from)).unwrap_or_default();
    let entries = comindly::ledger::derive(&records, &symbols);
    let recipes = comindly::index::read_recipes_blocking(&cfg.graph_dir(from)).unwrap_or_default();
    let packages =
        comindly::index::read_packages_blocking(&cfg.graph_dir(from)).unwrap_or_default();
    let findings = comindly::checks::run(&entries, &symbols, &edges, &packages);
    println!(
        "{}",
        comindly::preflight::report(&g, &symbols, &edges, &entries, &recipes, &findings, &changes)
    );
    let changed: Vec<String> = changes
        .iter()
        .map(|c| format!("{}/{}", c.repo, c.path))
        .collect();
    let blocking = findings.iter().any(|f| {
        f.severity == "must"
            && f.violations
                .iter()
                .any(|v| changed.contains(&format!("{}/{}", v.repo, v.path)))
    });
    if strict && blocking {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// Claude Code hook: emit `additionalContext` with ledger records relevant to the session/edit.
/// Never blocks or approves anything; any failure prints nothing and exits 0.
/// Graph symbols and ledger records for a hook: a remote (S3) index is read from its local
/// mirror, refreshed in the background when stale, so the agent never waits on the network.
fn hook_data(
    cfg: &comindly::config::Config,
    from: Option<&str>,
) -> Option<(Vec<Symbol>, Vec<comindly::ledger::Record>)> {
    let root = cfg.index_dir(from);
    if comindly::cache::is_remote(&root) {
        let dir = comindly::cache::dir(&root)?;
        let ttl = std::time::Duration::from_secs(
            cfg.cache_ttl_secs.unwrap_or(comindly::cache::DEFAULT_TTL),
        );
        if comindly::cache::stale(&dir, ttl) {
            comindly::cache::refresh_in_background(&root);
        }
        if let Some((symbols, _, records)) = comindly::cache::read(&dir) {
            return Some((symbols, records));
        }
    }
    let (symbols, _) = comindly::index::read_graph_blocking(&cfg.graph_dir(from)).ok()?;
    let records = comindly::ledger::all_blocking(&root).ok()?;
    Some((symbols, records))
}

/// The first string under `key` anywhere in a hook payload (agents nest tool input differently).
fn find_str<'v>(v: &'v serde_json::Value, key: &str) -> Option<&'v str> {
    match v {
        serde_json::Value::Object(m) => m
            .get(key)
            .and_then(|x| x.as_str())
            .or_else(|| m.values().find_map(|x| find_str(x, key))),
        serde_json::Value::Array(a) => a.iter().find_map(|x| find_str(x, key)),
        _ => None,
    }
}

fn cmd_doctor(from: Option<&str>) -> ExitCode {
    let cfg = comindly::config::Config::load();
    let root = cfg.index_dir(from);
    comindly::ui::header("comindly doctor");
    comindly::ui::field("index", &root);
    let checks = comindly::doctor::run(&cfg, &root);
    for c in &checks {
        if c.ok {
            comindly::ui::ok(&c.message);
        } else {
            comindly::ui::warn(&c.message);
        }
    }
    if checks.iter().all(|c| c.ok) {
        comindly::ui::ok("all good");
    }
    ExitCode::SUCCESS
}

fn cmd_hook(agent: &str, event: &str, from: Option<&str>) -> ExitCode {
    use std::io::Read;
    let claude = agent == "claude";
    if !claude && !matches!(agent, "generic" | "cursor" | "gemini" | "codex") {
        comindly::ui::err("agent must be claude, cursor, gemini, codex or generic");
        return ExitCode::FAILURE;
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
    let session = [
        "session_id",
        "sessionId",
        "conversation_id",
        "conversationId",
    ]
    .iter()
    .find_map(|k| find_str(&v, k))
    .unwrap_or("default")
    .to_string();
    let cfg = comindly::config::Config::load();
    let Some((symbols, records)) = hook_data(&cfg, from) else {
        return ExitCode::SUCCESS;
    };
    if records.is_empty() {
        return ExitCode::SUCCESS;
    }
    let entries = comindly::ledger::derive(&records, &symbols);
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
            let env = comindly::config::env("COMINDLY_HOOK_FILE");
            let Some(path) = env.as_deref().or_else(|| {
                ["file_path", "filePath", "target_file", "path", "file"]
                    .iter()
                    .find_map(|k| find_str(&v, k))
            }) else {
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
        .join("comindly-hook-sessions")
        .join(session.replace(['/', '\\'], "_"));
    let seen: HashSet<String> = std::fs::read_to_string(&seen_file)
        .map(|s| s.lines().map(str::to_string).collect())
        .unwrap_or_default();
    let fresh: Vec<&comindly::ledger::Entry> = comindly::ledger::about(&entries, &ids)
        .into_iter()
        .filter(|e| e.trust >= comindly::ledger::Trust::T2)
        .filter(|e| {
            matches!(
                e.status,
                comindly::ledger::Status::Active
                    | comindly::ledger::Status::Moved
                    | comindly::ledger::Status::Stale
            )
        })
        .filter(|e| !seen.contains(&e.record.id))
        .take(if hook_event == "SessionStart" { 8 } else { 4 })
        .collect();
    if fresh.is_empty() {
        return ExitCode::SUCCESS;
    }
    let mut text = comindly::ledger::markdown(
        "comindly ledger: reasoning recorded about this code",
        &fresh,
    );
    if text.len() > 9_000 {
        let cut = (0..=9_000)
            .rev()
            .find(|&i| text.is_char_boundary(i))
            .unwrap_or(0);
        text.truncate(cut);
    }
    let _ = std::fs::create_dir_all(seen_file.parent().unwrap_or(std::path::Path::new(".")));
    let mut lines: Vec<String> = seen.into_iter().collect();
    lines.extend(fresh.iter().map(|e| e.record.id.clone()));
    let _ = std::fs::write(&seen_file, lines.join("\n"));
    if !claude {
        println!("{text}");
        return ExitCode::SUCCESS;
    }
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
        comindly::ui::err("only `claude` is supported so far");
        return ExitCode::FAILURE;
    }
    let snippet = serde_json::json!({
        "hooks": {
            "SessionStart": [{
                "hooks": [{"type": "command", "command": "comindly hook claude session-start"}]
            }],
            "PreToolUse": [{
                "matcher": "Edit|MultiEdit|Write",
                "hooks": [{"type": "command", "command": "comindly hook claude pre-edit"}]
            }]
        }
    });
    comindly::ui::note("add to .claude/settings.json (project) or ~/.claude/settings.json (user):");
    println!(
        "{}",
        serde_json::to_string_pretty(&snippet).unwrap_or_default()
    );
    ExitCode::SUCCESS
}

/// Measured conventions with the team's changes applied, plus those changes.
fn load_conventions(
    from: Option<&str>,
) -> anyhow::Result<(
    Vec<comindly::conventions::Convention>,
    Vec<comindly::conventions::Manual>,
)> {
    let cfg = comindly::config::Config::load();
    let dir = cfg.graph_dir(from);
    let (symbols, edges) = comindly::index::read_graph_blocking(&dir)
        .map_err(|e| anyhow::anyhow!("no index at {dir}: {e:#}"))?;
    let Some(facts) = comindly::index::read_facts_blocking(&dir)? else {
        anyhow::bail!("index has no stored facts; re-run `comindly index`/`link`");
    };
    let mut all = comindly::conventions::mine(&symbols, &edges, &facts);
    all.extend(comindly::index::read_recipes_blocking(&dir).unwrap_or_default());
    let manual = comindly::conventions::manual_blocking(&cfg.index_dir(from))?;
    Ok((comindly::conventions::apply(all, &manual), manual))
}

fn cmd_conventions(from: Option<&str>, repo: Option<&str>) -> ExitCode {
    use comindly::conventions::{Origin, ORG};
    let (all, manual) = match load_conventions(from) {
        Ok(x) => x,
        Err(e) => {
            comindly::ui::err(&comindly::storage::explain(&format!("{e:#}")));
            return ExitCode::FAILURE;
        }
    };
    let mut by_repo: BTreeMap<&str, Vec<&comindly::conventions::Convention>> = BTreeMap::new();
    for c in &all {
        if repo.is_none_or(|r| c.repo == r || c.repo == ORG) {
            by_repo.entry(c.repo.as_str()).or_default().push(c);
        }
    }
    for (r, cs) in by_repo {
        comindly::ui::header(if r == ORG { "Org baseline" } else { r });
        for c in cs {
            let tag = match c.origin {
                Origin::Measured => "",
                Origin::Edited => ", edited",
                Origin::Manual => ", team",
            };
            comindly::ui::field(&c.dimension, &format!("{}  [{}{tag}]", c.rule, c.id()));
            if !c.exemplars.is_empty() {
                comindly::ui::note(&format!("e.g. {}", c.exemplars.join(", ")));
            }
            if !c.counterexamples.is_empty() {
                comindly::ui::note(&format!("except {}", c.counterexamples.join(", ")));
            }
        }
    }
    let hidden: Vec<_> = manual
        .iter()
        .filter(|m| m.hidden && repo.is_none_or(|r| m.repo == r || m.repo == ORG))
        .collect();
    if !hidden.is_empty() {
        comindly::ui::header("Hidden (comindly conventions restore <id>)");
        for m in hidden {
            comindly::ui::field(&m.dimension, &format!("{}  [{}, {}]", m.rule, m.id, m.repo));
        }
    }
    ExitCode::SUCCESS
}

fn cmd_conventions_change(
    from: Option<&str>,
    repo: Option<&str>,
    what: ConventionsWhat,
) -> ExitCode {
    use comindly::conventions::{change, Change, Manual, Write, ORG};
    let root = comindly::config::Config::load().index_dir(from);
    let restore = matches!(what, ConventionsWhat::Restore { .. });
    let write = match what {
        ConventionsWhat::Add { rule, dimension } => Ok(Write::Put(Manual::new(
            repo.unwrap_or(ORG),
            &dimension,
            &rule,
        ))),
        ConventionsWhat::Edit { id, rule } => load_conventions(from)
            .and_then(|(all, manual)| change(&all, &manual, &id, Change::Edit(rule))),
        ConventionsWhat::Rm { id } => load_conventions(from)
            .and_then(|(all, manual)| change(&all, &manual, &id, Change::Hide)),
        ConventionsWhat::Restore { id } => load_conventions(from)
            .and_then(|(all, manual)| change(&all, &manual, &id, Change::Restore)),
    };
    match write.and_then(|w| comindly::conventions::write_blocking(&root, &w).map(|_| w)) {
        Ok(Write::Put(m)) if m.hidden => comindly::ui::ok(&format!("hid {}", m.id)),
        Ok(Write::Put(m)) => comindly::ui::ok(&format!("saved {}: {}", m.id, m.rule)),
        Ok(Write::Delete(id)) if restore => comindly::ui::ok(&format!("restored {id}")),
        Ok(Write::Delete(id)) => comindly::ui::ok(&format!("removed {id}")),
        Err(e) => {
            comindly::ui::err(&comindly::storage::explain(&format!("{e:#}")));
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}

fn cmd_packages(from: Option<&str>, skew: bool) -> ExitCode {
    let dir = comindly::config::Config::load().graph_dir(from);
    let rows = match comindly::index::read_packages_blocking(&dir) {
        Ok(r) => r,
        Err(e) => {
            comindly::ui::err(&format!("no index at {dir}: {e:#}"));
            return ExitCode::FAILURE;
        }
    };
    let deps: Vec<_> = comindly::manifests::internal(&rows)
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
    comindly::ui::header(&format!("Internal dependencies — {}", deps.len()));
    println!("{table}");
    ExitCode::SUCCESS
}

/// Change recipes from each repo's recent first-parent history.
fn mine_recipes(repos: &[(String, std::path::PathBuf)]) -> Vec<comindly::conventions::Convention> {
    let out: Vec<_> = repos
        .iter()
        .filter_map(|(name, root)| {
            comindly::git::history(root, 500)
                .ok()
                .map(|h| comindly::recipes::mine(name, &h))
        })
        .flatten()
        .collect();
    if !out.is_empty() {
        comindly::ui::ok(&format!("recipes: {} from commit history", out.len()));
    }
    out
}

/// Manifests and lockfiles of each repo, and a one-line internal-dependency summary.
fn scan_packages(repos: &[(String, std::path::PathBuf)]) -> Vec<comindly::manifests::PackageRow> {
    let rows: Vec<_> = repos
        .iter()
        .flat_map(|(name, root)| comindly::manifests::scan(root, name))
        .collect();
    let deps = comindly::manifests::internal(&rows);
    if !deps.is_empty() {
        let skewed = deps.iter().filter(|d| d.skewed()).count();
        comindly::ui::ok(&format!(
            "internal packages: {} dependency link(s), {skewed} pinned to a different version than their publisher",
            deps.len()
        ));
    }
    rows
}

fn cmd_contracts(from: Option<&str>, kind: Option<&str>, dangling: bool) -> ExitCode {
    let dir = comindly::config::Config::load().graph_dir(from);
    let (symbols, edges) = match comindly::index::read_graph_blocking(&dir) {
        Ok(x) => x,
        Err(e) => {
            comindly::ui::err(&format!("no index at {dir}: {e:#}"));
            return ExitCode::FAILURE;
        }
    };
    let g = comindly::graph::CodeGraph::build(&symbols, &edges);
    let total = g.contracts(None, false).len();
    let views = g.contracts(kind, dangling);
    let list = |ns: &[comindly::graph::Node]| {
        let set: BTreeSet<String> = ns
            .iter()
            .map(|n| {
                if n.repo == comindly::resolve::CONTRACTS {
                    n.name.clone()
                } else {
                    format!("{}:{}", n.repo, n.name)
                }
            })
            .collect();
        if set.is_empty() {
            "—".to_string()
        } else {
            set.into_iter().collect::<Vec<_>>().join("\n")
        }
    };
    let mut table = comfy_table::Table::new();
    table
        .load_preset(comfy_table::presets::UTF8_FULL)
        .set_header(vec!["contract", "provided by", "used by"]);
    for v in &views {
        table.add_row(vec![
            v.contract.name.clone(),
            list(&v.providers),
            list(&v.consumers),
        ]);
    }
    comindly::ui::header(&format!("Contracts — {} shown of {total}", views.len()));
    println!("{table}");
    ExitCode::SUCCESS
}

fn cmd_eval_graph(fixture: &str, verbose: bool, scip: Option<&str>, repos: &[String]) -> ExitCode {
    let only: Vec<&str> = repos.iter().map(String::as_str).collect();
    let r = match comindly::eval::eval_with(
        Path::new(fixture),
        (!only.is_empty()).then_some(only.as_slice()),
        scip.map(Path::new),
    ) {
        Ok(r) => r,
        Err(e) => {
            comindly::ui::err(&format!("eval failed: {e:#}"));
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
    comindly::ui::header(&format!("Graph eval — {fixture}"));
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
        comindly::ui::note(&format!("references answered by SCIP: {}", r.scip_answers));
    }
    if verbose {
        comindly::ui::header("Missed");
        for g in &r.missed {
            comindly::ui::note(&format!("[{}] {} {} -> {}", g.case, g.kind, g.src, g.dst));
        }
        comindly::ui::header("Spurious");
        for (k, s, d) in &r.spurious {
            comindly::ui::note(&format!("{k} {s} -> {d}"));
        }
    }
    ExitCode::SUCCESS
}

fn cmd_config(action: Option<ConfigAction>) -> ExitCode {
    match action {
        None | Some(ConfigAction::Path) => {
            // Path stays plain on stdout so `$(comindly config path)` is scriptable.
            println!("{}", comindly::config::config_path().display());
            comindly::ui::field("index dir", &comindly::config::default_index_dir());
            ExitCode::SUCCESS
        }
        Some(ConfigAction::Init { overwrite }) => {
            use std::io::IsTerminal;
            let path = comindly::config::config_path();
            if path.exists() && !overwrite {
                // Can't prompt when piped/CI → refuse and point at --overwrite.
                if !std::io::stdin().is_terminal() {
                    comindly::ui::err("config exists — re-run with --overwrite to replace it");
                    return ExitCode::FAILURE;
                }
                // Deliberate interactive decline is a valid choice, not an error.
                if !prompt_yes(&path) {
                    comindly::ui::note("kept existing config");
                    return ExitCode::SUCCESS;
                }
            }
            match comindly::config::init(true) {
                Ok(path) => {
                    comindly::ui::ok(&format!("wrote {}", path.display()));
                    comindly::ui::note("edit it to set index_dir / llm_model / format, or run: comindly index . --embed");
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    comindly::ui::err(&comindly::storage::explain(&format!("{e:#}")));
                    ExitCode::FAILURE
                }
            }
        }
    }
}

/// Interactive y/N prompt (caller guarantees a TTY). `y`/`yes` = overwrite; anything else = keep.
fn prompt_yes(path: &Path) -> bool {
    use std::io::Write;
    comindly::ui::warn(&format!(
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

/// The configured repositories, each fetched and checked out at its default branch in comindly's
/// worktree (never in your folder).
fn configured_checkouts(cfg: &comindly::config::Config) -> anyhow::Result<Vec<String>> {
    if cfg.repos.is_empty() {
        anyhow::bail!(
            "no repositories: pass folders, or add them with `comindly repos add <folder>`"
        );
    }
    comindly::ui::header(&format!("Checking out {} repo(s)", cfg.repos.len()));
    let mut out = Vec::new();
    for r in &cfg.repos {
        let path = comindly::config::expand_tilde(&r.path);
        if let Some(url) = &r.remote {
            if !Path::new(&path).exists() {
                comindly::ui::note(&format!("cloning {url}"));
            }
            comindly::worktree::ensure_clone(url, Path::new(&path))
                .map_err(|e| anyhow::anyhow!("{url}: {e:#}"))?;
        }
        let c = comindly::worktree::prepare(Path::new(&path))
            .map_err(|e| anyhow::anyhow!("{path}: {e:#}"))?;
        comindly::ui::ok(&format!(
            "{} @ {} {}",
            comindly::worktree::name(Path::new(&path)),
            c.target,
            c.commit
        ));
        if let Some(why) = &c.stale {
            comindly::ui::warn(&format!(
                "could not fetch, indexing the last fetched state: {why}"
            ));
        }
        out.push(c.dir.to_string_lossy().into_owned());
    }
    Ok(out)
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
    scip: comindly::scip::Mode,
) -> ExitCode {
    let mut symbols: Vec<Symbol> = Vec::new();
    let mut edges: Vec<Edge> = Vec::new();
    let mut facts = Vec::new();
    if let Some(bad) = repos.iter().find(|p| !Path::new(p).is_dir()) {
        comindly::ui::err(&format!("no such repo directory: {bad}"));
        return ExitCode::FAILURE;
    }
    comindly::ui::header(&format!("Parsing {} repo(s)", repos.len()));
    for path in repos {
        let name = repo_name(path);
        match comindly::parse::parse_repo(Path::new(path), &name) {
            Ok(mut o) => {
                comindly::ui::field(
                    &name,
                    &format!("{} symbols, {} files", o.symbols.len(), o.facts.len()),
                );
                if let Err(e) = attach_scip(scip, Path::new(path), &name, &mut o) {
                    comindly::ui::err(&e);
                    return ExitCode::FAILURE;
                }
                symbols.extend(o.symbols);
                edges.extend(o.edges);
                facts.extend(o.facts);
            }
            Err(e) => {
                comindly::ui::err(&format!("{name}: parse failed: {e:#}"));
                return ExitCode::FAILURE;
            }
        }
    }

    let resolved = comindly::resolve::resolve(&symbols, &edges, &facts);
    symbols.extend(resolved.symbols.iter().cloned());
    if !resolved.symbols.is_empty() {
        let mut by_type: BTreeMap<&str, usize> = BTreeMap::new();
        for c in &resolved.symbols {
            *by_type
                .entry(c.name.split(':').next().unwrap_or_default())
                .or_default() += 1;
        }
        let parts: Vec<String> = by_type.iter().map(|(t, n)| format!("{t} {n}")).collect();
        comindly::ui::ok(&format!(
            "contracts: {} ({}), {} participant edges",
            resolved.symbols.len(),
            parts.join(", "),
            resolved.stats.contract_edges
        ));
    }
    let s = &resolved.stats;
    comindly::ui::ok(&format!(
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
    comindly::ui::note(&format!(
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
        comindly::ui::note(&format!("top unresolved: {}", top.join(", ")));
    }
    if comindly::config::env("COMINDLY_DEBUG").is_some() {
        for (m, ex) in &resolved.unresolved_examples {
            for e in ex {
                comindly::ui::note(&format!("  unresolved {m}: {e}"));
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
        comindly::ui::header("Top cross-repo dependencies (org-wide blast radius)");
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
        comindly::ui::header(&format!("ripple({descriptor})  [defined in {def_repo}]"));
        comindly::ui::note(&format!(
            "changing this would impact {} repos ({refs} references):",
            importers.len()
        ));
        for repo in importers {
            comindly::ui::note(&format!("- {repo}"));
        }
    }

    {
        // Persist the resolved org-wide graph. Overwrite creates a new Lance version whose
        // manifest is the atomic "latest" pointer every consumer reads (push-to-master model).
        let to_uri = comindly::config::Config::load().index_dir(to);
        let uri = to_uri.as_str();
        let dst = format!("{}/_graph", uri.trim_end_matches('/'));
        comindly::ui::header("Persisting");
        comindly::ui::step(&format!("writing org graph to {dst}"));
        let roots: Vec<(String, std::path::PathBuf)> = repos
            .iter()
            .map(|p| (repo_name(p), std::path::PathBuf::from(p)))
            .collect();
        let packages = scan_packages(&roots);
        let recipes = mine_recipes(&roots);
        match comindly::index::write_graph_blocking(&dst, &symbols, &resolved.edges)
            .and_then(|v| comindly::index::write_facts_blocking(&dst, &facts).map(|_| v))
            .and_then(|v| comindly::index::write_packages_blocking(&dst, &packages).map(|_| v))
            .and_then(|v| comindly::index::write_recipes_blocking(&dst, &recipes).map(|_| v))
        {
            Ok((sv, ev)) => comindly::ui::ok(&format!(
                "org graph: symbols v{sv}, edges v{ev} (latest pointer)"
            )),
            Err(e) => {
                comindly::ui::err(&format!(
                    "write failed: {}",
                    comindly::storage::explain(&format!("{e:#}"))
                ));
                return ExitCode::FAILURE;
            }
        }
        match comindly::index::count_rows_blocking(&dst) {
            Ok((s, e)) => comindly::ui::note(&format!("read back: {s} symbols, {e} edges")),
            Err(e) => {
                comindly::ui::err(&format!("read-back failed: {e:#}"));
                return ExitCode::FAILURE;
            }
        }

        // Incremental: figure out which symbols are stale (their repo's changed files, per git),
        // so we recompute embeddings/enrichment only for those and reuse the rest.
        let current_heads: Vec<comindly::index::IndexedRepo> = repos
            .iter()
            .filter_map(|p| {
                comindly::git::head_commit(Path::new(p)).ok().map(|commit| {
                    comindly::index::IndexedRepo {
                        name: repo_name(p),
                        commit,
                        remote: comindly::worktree::remote_url(Path::new(p)),
                    }
                })
            })
            .collect();
        let stale_ids: HashSet<String> = if incremental {
            compute_stale_ids(repos, &dst, &symbols)
        } else {
            symbols.iter().map(|s| s.id.render()).collect() // full recompute
        };
        if incremental {
            comindly::ui::note(&format!(
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

        let cfg = comindly::config::Config::load();
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
        let _ = comindly::index::write_repo_commits_blocking(&dst, &current_heads);
    }

    ExitCode::SUCCESS
}

/// Pre-generate flow walkthroughs (opt-in `--flows`): pick the top entry points by how
/// much they orchestrate (forward call-trace size), trace each with `thread`, narrate via the
/// LLM, and persist to the `flows` table. Sends call traces to the OpenAI API.
fn run_flows(dst: &str, symbols: &[Symbol], edges: &[Edge], top: usize) -> ExitCode {
    let client = match comindly::llm::LlmClient::from_env() {
        Ok(c) => c,
        Err(e) => {
            // No/invalid LLM key: skip this step rather than failing the whole index.
            comindly::ui::warn(&format!("skipping (no LLM client): {e:#}"));
            return ExitCode::SUCCESS;
        }
    };
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            comindly::ui::err(&format!("runtime: {e}"));
            return ExitCode::FAILURE;
        }
    };
    let g = comindly::graph::CodeGraph::build(symbols, edges);

    comindly::ui::header("Narrating flows (LLM)");
    // Entry points whose flow reaches furthest, across repositories first.
    let candidates: Vec<(&Symbol, usize)> = g
        .entry_points(top)
        .into_iter()
        .filter_map(|(idx, n)| {
            let id = g.node(idx).id;
            symbols.iter().find(|s| s.id.render() == id).map(|s| (s, n))
        })
        .collect();

    if candidates.is_empty() {
        comindly::ui::note("flows: no multi-step flows found to narrate");
        return ExitCode::SUCCESS;
    }
    comindly::ui::warn("--flows sends call traces to the OpenAI API — opt-in egress.");

    // Precompute call traces (graph ops) up front, then narrate concurrently.
    let prep: Vec<(comindly::model::GlobalSymbolId, String, String, String)> = candidates
        .iter()
        .filter_map(|(s, _)| {
            let idx = g.lookup(&s.id.render())?;
            let trace = g
                .flow(idx, 6)
                .iter()
                .map(|h| {
                    format!(
                        "d{} {} {}:{} {}",
                        h.depth, h.via, h.node.repo, h.node.name, h.node.location
                    )
                })
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

    let pb = comindly::ui::progress(prep.len() as u64, "narrating flows");
    let rows: Vec<(comindly::model::GlobalSymbolId, String, Vec<String>)> = rt.block_on(async {
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
    comindly::ui::note(&format!(
        "tokens: {} in / {} out ({} total)",
        kfmt(i),
        kfmt(o),
        kfmt(i + o)
    ));

    match comindly::index::write_flows_blocking(dst, &rows) {
        Ok(v) => comindly::ui::ok(&format!("flows v{v}: {} narrated", rows.len())),
        Err(e) => {
            comindly::ui::err(&format!("write flows failed: {e:#}"));
            return ExitCode::FAILURE;
        }
    }
    if let Some((_, narr, _)) = rows.first() {
        comindly::ui::note(&format!("e.g. {}", truncate(narr, 120)));
    }
    ExitCode::SUCCESS
}

/// `comindly flow <focus> --from <uri>` — the pre-generated flow walkthrough for an entry point (if
/// the index was built with `--flows`), followed by its live forward call trace.
fn cmd_flow(focus: &str, from: Option<&str>) -> ExitCode {
    let uri = comindly::config::Config::load().graph_dir(from);
    let uri = uri.as_str();
    let (symbols, edges) = match comindly::index::read_graph_blocking(uri) {
        Ok(x) => x,
        Err(e) => {
            comindly::ui::err(&friendly_load_err(uri, &e));
            return ExitCode::FAILURE;
        }
    };
    let g = comindly::graph::CodeGraph::build(&symbols, &edges);
    let Some(idx) = g.lookup(focus) else {
        comindly::ui::err(&format!("no symbol matching `{focus}`"));
        return ExitCode::FAILURE;
    };
    let fid = g.zoom(idx).focus.map(|n| n.id);

    // Pre-generated narration, if present.
    if let Some(fid) = &fid {
        let stored = comindly::index::read_flows_blocking(uri)
            .ok()
            .flatten()
            .unwrap_or_default()
            .into_iter()
            .find(|(id, _, _)| &id.render() == fid);
        match stored {
            Some((_, narr, queries)) => {
                comindly::ui::header(&format!("flow: {focus}"));
                println!("{narr}\n");
                if !queries.is_empty() {
                    comindly::ui::field("Ask", &queries.join(" · "));
                }
            }
            None => comindly::ui::note(&format!(
                "no pre-generated narration for `{focus}` — showing the raw trace; run `index/link --flows` to narrate"
            )),
        }
    }

    // Live forward call trace.
    let hops = g.thread(idx, 4);
    comindly::ui::header(&format!("flow trace from `{focus}` ({} steps)", hops.len()));
    if hops.is_empty() {
        comindly::ui::note("no outgoing calls from here");
    } else {
        let mut t = comindly::ui::table(&["depth", "via", "symbol", "location"]);
        for h in &hops {
            t.add_row(vec![
                format!("d{}", h.depth),
                h.via.to_string(),
                h.node.name.clone(),
                h.node.location.clone(),
            ]);
        }
        comindly::ui::right_align(&mut t, &[0]);
        println!("{t}");
    }
    ExitCode::SUCCESS
}

/// Rendered ids of symbols whose content may have changed since the last index: any symbol in a
/// repo that is new (no recorded commit / not a git repo) or whose file changed since that commit.
fn compute_stale_ids(repos: &[&str], dst: &str, symbols: &[Symbol]) -> HashSet<String> {
    let prior = comindly::index::read_repo_commits_blocking(dst).unwrap_or_default();
    // repo -> Some(changed files) or None (treat whole repo as stale)
    let mut changed: HashMap<String, Option<HashSet<String>>> = HashMap::new();
    for p in repos {
        let name = repo_name(p);
        let set = match (prior.get(&name), comindly::git::head_commit(Path::new(p))) {
            (Some(base), Ok(_)) => {
                comindly::git::changed_files(Path::new(p), base)
                    .ok()
                    .map(|cs| {
                        cs.added
                            .into_iter()
                            .chain(cs.modified)
                            .collect::<HashSet<_>>()
                    })
            }
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
    let embedder = match comindly::embed::Embedder::load_default() {
        Ok(e) => e,
        Err(e) => {
            comindly::ui::err(&format!("embedding model: {e:#}"));
            return ExitCode::FAILURE;
        }
    };
    let prior: HashMap<String, Vec<f32>> = if incremental {
        comindly::index::read_search_vectors_blocking(dst)
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
    let mut rows: Vec<(comindly::model::GlobalSymbolId, String, Vec<f32>)> =
        Vec::with_capacity(non_file.len());
    let mut reused = 0usize;
    let mut to_embed: Vec<&Symbol> = Vec::new();
    for s in &non_file {
        let rid = s.id.render();
        if !stale_ids.contains(&rid) {
            if let Some(v) = prior.get(&rid) {
                rows.push((s.id.clone(), comindly::embed::symbol_text(s), v.clone()));
                reused += 1;
                continue;
            }
        }
        to_embed.push(s);
    }
    comindly::ui::header("Building search index");
    let pb = comindly::ui::spinner(&format!(
        "embedding {} symbols + building BM25 index",
        to_embed.len()
    ));
    let texts: Vec<String> = to_embed
        .iter()
        .map(|s| comindly::embed::symbol_text(s))
        .collect();
    for (s, v) in to_embed.iter().zip(embedder.embed(&texts)) {
        rows.push((s.id.clone(), comindly::embed::symbol_text(s), v));
    }
    let write = comindly::index::write_search_table_blocking(dst, &rows);
    pb.finish_and_clear();
    match write {
        Ok(v) => comindly::ui::ok(&format!(
            "search index v{v}: {} symbols ({reused} reused, {} embedded) + BM25 FTS index",
            rows.len(),
            to_embed.len()
        )),
        Err(e) => {
            comindly::ui::err(&format!("write search table failed: {e:#}"));
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
    let client = match comindly::llm::LlmClient::from_env() {
        Ok(c) => c,
        Err(e) => {
            // No/invalid LLM key: skip this step rather than failing the whole index.
            comindly::ui::warn(&format!("skipping (no LLM client): {e:#}"));
            return ExitCode::SUCCESS;
        }
    };
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            comindly::ui::err(&format!("runtime: {e}"));
            return ExitCode::FAILURE;
        }
    };
    let g = comindly::graph::CodeGraph::build(symbols, edges);

    comindly::ui::header("Enriching symbols (LLM)");
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
        comindly::index::read_enrichment_blocking(dst)
            .ok()
            .flatten()
            .unwrap_or_default()
            .into_iter()
            .map(|(id, s, q)| (id.render(), (s, q)))
            .collect()
    } else {
        HashMap::new()
    };

    let mut rows: Vec<(comindly::model::GlobalSymbolId, String, Vec<String>)> = Vec::new();
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
    let results: Vec<Option<comindly::llm::Enrichment>> = if items.is_empty() {
        Vec::new()
    } else {
        comindly::ui::warn(
            "--enrich sends code (names/signatures) to the OpenAI API — opt-in egress.",
        );
        let pb = comindly::ui::progress(items.len() as u64, "enriching symbols");
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
        comindly::ui::note(&format!(
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

    match comindly::index::write_enrichment_blocking(dst, &rows) {
        Ok(v) => comindly::ui::ok(&format!(
            "enrichment v{v}: {} symbols ({reused} reused, {generated} generated{})",
            rows.len(),
            if failed > 0 {
                format!(", {failed} failed")
            } else {
                String::new()
            }
        )),
        Err(e) => {
            comindly::ui::err(&format!("write enrichment failed: {e:#}"));
            return ExitCode::FAILURE;
        }
    }

    // Preview a few. Regenerate the style guide only when something changed.
    for (_, summary, queries) in rows.iter().take(3) {
        comindly::ui::note(&format!("• {summary}"));
        if let Some(q) = queries.first() {
            comindly::ui::note(&format!("  e.g. \"{q}\""));
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
    let client = match comindly::llm::LlmClient::from_env() {
        Ok(c) => c,
        Err(e) => {
            // No/invalid LLM key: skip this step rather than failing the whole index.
            comindly::ui::warn(&format!("skipping (no LLM client): {e:#}"));
            return ExitCode::SUCCESS;
        }
    };
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            comindly::ui::err(&format!("runtime: {e}"));
            return ExitCode::FAILURE;
        }
    };
    comindly::ui::header("Style guides (per repo)");
    comindly::ui::warn("--guide sends code samples + config to the LLM — opt-in egress.");
    let conventions = match comindly::index::read_facts_blocking(dst) {
        Ok(Some(facts)) => comindly::conventions::mine(symbols, edges, &facts),
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
        let sections = comindly::styleguide::build_sections(root, &rsyms, &redges, &rconv);
        if sections.is_empty() {
            continue;
        }
        let pb = comindly::ui::progress(sections.len() as u64, &format!("{repo}: guide sections"));
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
            comindly::ui::warn(&format!("{repo}: no sections generated"));
            continue;
        }
        doc.push_str("\n_AI-generated from the codebase; review before enforcing._\n");
        comindly::ui::ok(&format!(
            "{repo}: style guide ({} sections)",
            doc.matches("\n## ").count() + 1
        ));
        guide_rows.push((repo.clone(), doc));
    }
    if !guide_rows.is_empty() {
        let _ = comindly::index::write_style_guide_blocking(dst, &guide_rows);
    }
    let (i, o) = client.token_usage();
    comindly::ui::note(&format!("tokens: {} in / {} out", kfmt(i), kfmt(o)));
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
        format!("no index at {root} — build one first: comindly index <repo>")
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
    scip: comindly::scip::Mode,
) -> ExitCode {
    // `--since` implies incremental.
    let incremental = incremental || since.is_some();
    let root = Path::new(repo);
    if !root.is_dir() {
        comindly::ui::err(&format!("no such repo directory: {repo}"));
        return ExitCode::FAILURE;
    }
    let repo_name = repo_name(repo);
    // Same default location `link` writes to, so `search`/`serve` find it with zero config.
    let dst = format!(
        "{}/_graph",
        comindly::config::Config::load()
            .index_dir(to)
            .trim_end_matches('/')
    );

    // Incremental path: diff against a base commit and reparse only what changed.
    if incremental {
        let base = since.map(str::to_string).or_else(|| {
            comindly::index::read_repo_meta_blocking(&dst)
                .ok()
                .flatten()
        });
        match base {
            Some(base) => return incremental_index(root, &repo_name, &dst, &base),
            None => comindly::ui::note("no recorded base commit — doing a full index"),
        }
    }

    comindly::ui::header(&format!("Indexing {repo_name}"));
    let mut out = match comindly::parse::parse_repo(root, &repo_name) {
        Ok(o) => o,
        Err(e) => {
            comindly::ui::err(&comindly::storage::explain(&format!("{e:#}")));
            return ExitCode::FAILURE;
        }
    };
    if let Err(e) = attach_scip(scip, root, &repo_name, &mut out) {
        comindly::ui::err(&e);
        return ExitCode::FAILURE;
    }
    // Bind provisional call/import edges to real definitions so ripple/thread/context_pack work.
    let resolved = comindly::resolve::resolve(&out.symbols, &out.edges, &out.facts);
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
    comindly::ui::field("symbols", &out.symbols.len().to_string());
    for (k, n) in &by_kind {
        comindly::ui::note(&format!("{k:<10} {n}"));
    }
    comindly::ui::field(
        "edges",
        &format!("{} (contains={contains}, calls={calls})", out.edges.len()),
    );

    comindly::ui::header("Persisting");
    comindly::ui::step(&format!("writing graph to {dst}"));
    match comindly::index::write_graph_blocking(&dst, &out.symbols, &out.edges)
        .and_then(|v| comindly::index::write_facts_blocking(&dst, &out.facts).map(|_| v))
        .and_then(|v| {
            // Keep other repos' package rows already in a shared index dir.
            let mut rows: Vec<_> = comindly::index::read_packages_blocking(&dst)
                .unwrap_or_default()
                .into_iter()
                .filter(|r| r.repo != repo_name)
                .collect();
            rows.extend(scan_packages(&[(repo_name.clone(), root.to_path_buf())]));
            comindly::index::write_packages_blocking(&dst, &rows)?;
            let mut recipes: Vec<_> = comindly::index::read_recipes_blocking(&dst)
                .unwrap_or_default()
                .into_iter()
                .filter(|r| r.repo != repo_name)
                .collect();
            recipes.extend(mine_recipes(&[(repo_name.clone(), root.to_path_buf())]));
            comindly::index::write_recipes_blocking(&dst, &recipes).map(|_| v)
        }) {
        Ok((sv, ev)) => comindly::ui::ok(&format!("graph: symbols v{sv}, edges v{ev}")),
        Err(e) => {
            comindly::ui::err(&format!(
                "write failed: {}",
                comindly::storage::explain(&format!("{e:#}"))
            ));
            return ExitCode::FAILURE;
        }
    }
    // Record the HEAD commit so later runs can index incrementally.
    if let Ok(head) = comindly::git::head_commit(root) {
        let _ = comindly::index::write_repo_meta_blocking(&dst, &repo_name, &head);
        comindly::ui::note(&format!("recorded commit {}", short(&head)));
    }

    // Optional enrichment steps — recompute everything (full index).
    let all: HashSet<String> = out.symbols.iter().map(|s| s.id.render()).collect();
    let cfg = comindly::config::Config::load();
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
        comindly::ui::note("next: comindly search \"<question>\"  ·  comindly serve");
    } else {
        comindly::ui::note(
            "next: comindly explore <symbol> (search needs embeddings — drop --no-embed)",
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
    let head = match comindly::git::head_commit(root) {
        Ok(h) => h,
        Err(e) => {
            comindly::ui::err(&comindly::storage::explain(&format!("{e:#}")));
            return ExitCode::FAILURE;
        }
    };
    let cs = match comindly::git::changed_files(root, base) {
        Ok(c) => c,
        Err(e) => {
            comindly::ui::err(&comindly::storage::explain(&format!("{e:#}")));
            return ExitCode::FAILURE;
        }
    };
    comindly::ui::header(&format!(
        "Incremental {} → {}: {} changed ({}+ {}~ {}-)",
        short(base),
        short(&head),
        cs.total(),
        cs.added.len(),
        cs.modified.len(),
        cs.deleted.len()
    ));
    if cs.total() == 0 {
        let _ = comindly::index::write_repo_meta_blocking(dst, repo_name, &head);
        comindly::ui::ok("index already up to date");
        return ExitCode::SUCCESS;
    }

    let (prior_syms, prior_edges) = match comindly::index::read_graph_blocking(dst) {
        Ok(x) => x,
        Err(e) => {
            comindly::ui::err(&format!(
                "no prior index at {dst} ({e:#}); run a full index first"
            ));
            return ExitCode::FAILURE;
        }
    };

    let dropped: HashSet<String> = cs.to_drop().cloned().collect();
    let to_parse: Vec<String> = cs.to_parse().cloned().collect();
    let newout = comindly::parse::parse_files(root, repo_name, &to_parse);

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
    let facts = match comindly::index::read_facts_blocking(dst) {
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
            comindly::ui::warn("index has no stored facts; run a full index for exact edges");
            newout.facts
        }
    };
    // Contract nodes are derived; drop the old ones and re-derive from all facts.
    symbols.retain(|s| s.kind != SymbolKind::Contract);
    let resolved = comindly::resolve::resolve(&symbols, &edges, &facts);
    symbols.extend(resolved.symbols);
    let edges = resolved.edges;

    if let Err(e) = comindly::index::write_graph_blocking(dst, &symbols, &edges)
        .and_then(|_| comindly::index::write_facts_blocking(dst, &facts))
    {
        comindly::ui::err(&format!(
            "write failed: {}",
            comindly::storage::explain(&format!("{e:#}"))
        ));
        return ExitCode::FAILURE;
    }
    let _ = comindly::index::write_repo_meta_blocking(dst, repo_name, &head);
    comindly::ui::ok(&format!(
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
