//! Comindly configuration — persistent defaults for the data location and model choices.
//!
//! Precedence, highest first: CLI flag → environment variable → `config.toml` → built-in default.
//! Secrets (`OPENAI_API_KEY`) are never read from the file; keep them in the environment.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::PathBuf;

/// Non-secret preferences, all optional. Read from `config.toml` if present.
#[derive(Debug, Default, Deserialize)]
pub struct Config {
    /// Default index directory for `--index-dir` (local path or `s3://…`).
    pub index_dir: Option<String>,
    /// Default LLM model for `--enrich` / `--flows`.
    pub llm_model: Option<String>,
    /// Default embedding model (Hugging Face id or local path).
    pub embed_model: Option<String>,
    /// OpenAI-compatible base URL (Ollama, vLLM, LiteLLM proxy, Azure).
    pub llm_base_url: Option<String>,
    /// Cap on symbols enriched by `--enrich`. Absent = no cap (enrich the whole codebase).
    pub max_enrich: Option<usize>,
    /// Cap on flows narrated by `--flows`. Absent = no cap (narrate every entry point).
    pub max_flows: Option<usize>,
    /// Default output format when `--format` is not given. `search`: `md` | `table`;
    /// `serve`: `md` | `json`. Absent = `md`.
    pub format: Option<String>,
    /// Default for `index`/`link` steps; `--x` / `--no-x` on the CLI override these.
    /// Built-in defaults: all on; the LLM steps skip without a key, and `--no-llm` skips them all.
    pub embed: Option<bool>,
    pub enrich: Option<bool>,
    pub flows: Option<bool>,
    pub guide: Option<bool>,
    /// SCIP use in `index`/`link`: `auto` (default) | `require` | `off`.
    pub scip: Option<String>,
    /// SSH key that signs ledger records (default: git `user.signingkey` when `gpg.format=ssh`).
    pub signing_key: Option<String>,
    /// `allowed_signers` file for verifying records (default: git `gpg.ssh.allowedSignersFile`).
    pub allowed_signers: Option<String>,
    /// Seconds between freshness checks of the local mirror of a remote index (default 300).
    pub cache_ttl_secs: Option<u64>,
    /// AWS profile for S3 indexes when `AWS_PROFILE` is not set (credentials stay in `~/.aws`).
    pub aws_profile: Option<String>,
    /// Name shown on ledger records you write (default: git `user.name`).
    pub author_name: Option<String>,
    /// `false` keeps names off new ledger records (only the pseudonymous actor id is stored).
    pub record_names: Option<bool>,
    /// Re-index from the app when a listed repository's default branch moves on the remote.
    pub auto_index: Option<bool>,
    /// When the AWS SSO session has expired, start `aws sso login` (you still approve it).
    pub sso_auto_login: Option<bool>,
    /// Repositories `comindly link` indexes when given none: comindly checks out each one's default
    /// branch in its own worktree.
    #[serde(default)]
    pub repos: Vec<RepoEntry>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, serde::Serialize)]
pub struct RepoEntry {
    /// Your local clone.
    pub path: String,
    /// Branch to index (default: the remote's default branch).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
}

/// Replace the repository list in `config.toml`.
pub fn set_repos(list: &[RepoEntry]) -> Result<()> {
    let path = config_path();
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let mut doc: toml_edit::DocumentMut = text.parse().context("parse config.toml")?;
    let mut tables = toml_edit::ArrayOfTables::new();
    for r in list {
        let mut t = toml_edit::Table::new();
        t["path"] = toml_edit::value(r.path.as_str());
        if let Some(b) = &r.branch {
            t["branch"] = toml_edit::value(b.as_str());
        }
        tables.push(t);
    }
    if list.is_empty() {
        doc.remove("repos");
    } else {
        doc["repos"] = toml_edit::Item::ArrayOfTables(tables);
    }
    std::fs::create_dir_all(path.parent().context("config dir")?)?;
    std::fs::write(&path, doc.to_string()).with_context(|| format!("write {}", path.display()))
}

impl Config {
    /// Load `config.toml`, or an empty config if it is missing. A malformed file warns and
    /// falls back to defaults rather than aborting the command.
    pub fn load() -> Self {
        let path = config_path();
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        match toml::from_str(&text) {
            Ok(cfg) => cfg,
            Err(e) => {
                eprintln!("comindly: ignoring {}: {e}", path.display());
                Self::default()
            }
        }
    }

    /// Resolve the index location: flag → `COMINDLY_INDEX_DIR` → file → `~/.local/share/comindly`.
    pub fn index_dir(&self, flag: Option<&str>) -> String {
        if let Some(f) = flag {
            return expand_tilde(f);
        }
        if let Some(e) = non_empty_env("COMINDLY_INDEX_DIR") {
            return expand_tilde(&e);
        }
        if let Some(d) = &self.index_dir {
            return expand_tilde(d);
        }
        default_index_dir()
    }

    /// Resolve the graph dataset to read from: `<index_dir>/_graph`. The `_graph` suffix is an
    /// internal detail — callers pass the index dir (root), never the dataset path.
    pub fn graph_dir(&self, flag: Option<&str>) -> String {
        format!("{}/_graph", self.index_dir(flag).trim_end_matches('/'))
    }

    /// How often long-running readers (`serve`, `ui`, the hooks' local mirror) check for a new
    /// index version.
    pub fn fresh(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.cache_ttl_secs.unwrap_or(crate::cache::DEFAULT_TTL))
    }

    /// Resolve the LLM model: `COMINDLY_LLM_MODEL` → file → `crate::llm::DEFAULT_MODEL`.
    pub fn llm_model(&self) -> String {
        team(|t| t.llm_model.clone())
            .or_else(|| non_empty_env("COMINDLY_LLM_MODEL"))
            .or_else(|| self.llm_model.clone())
            .unwrap_or_else(|| crate::llm::DEFAULT_MODEL.to_string())
    }

    /// Resolve the embedding model: `COMINDLY_EMBED_MODEL` → file → `crate::embed::DEFAULT_MODEL`.
    pub fn embed_model(&self) -> String {
        team(|t| t.embed_model.clone())
            .or_else(|| non_empty_env("COMINDLY_EMBED_MODEL"))
            .or_else(|| self.embed_model.clone())
            .unwrap_or_else(|| crate::embed::DEFAULT_MODEL.to_string())
    }

    /// Resolve an OpenAI-compatible base URL: `COMINDLY_LLM_BASE_URL` → file → none.
    pub fn llm_base_url(&self) -> Option<String> {
        team(|t| t.llm_base_url.clone())
            .or_else(|| non_empty_env("COMINDLY_LLM_BASE_URL"))
            .or_else(|| self.llm_base_url.clone())
    }

    /// Cap on enriched symbols (`max_enrich` in the file); `usize::MAX` = no cap.
    pub fn max_enrich(&self) -> usize {
        self.max_enrich.unwrap_or(usize::MAX)
    }

    /// Cap on narrated flows (`max_flows` in the file); `usize::MAX` = no cap.
    pub fn max_flows(&self) -> usize {
        self.max_flows.unwrap_or(usize::MAX)
    }

    /// Default output format: `COMINDLY_FORMAT` → file → `md`.
    pub fn format(&self) -> String {
        non_empty_env("COMINDLY_FORMAT")
            .or_else(|| self.format.clone())
            .unwrap_or_else(|| "md".to_string())
    }

    /// Step defaults (file → built-in). All on by default; the LLM steps skip gracefully with a
    /// warning when `OPENAI_API_KEY` is missing, so a keyless `index` still succeeds.
    pub fn embed(&self) -> bool {
        self.embed.unwrap_or(true)
    }
    pub fn enrich(&self) -> bool {
        team(|t| t.enrich).or(self.enrich).unwrap_or(true)
    }
    pub fn flows(&self) -> bool {
        team(|t| t.flows).or(self.flows).unwrap_or(true)
    }
    pub fn guide(&self) -> bool {
        team(|t| t.guide).or(self.guide).unwrap_or(true)
    }

    /// SCIP mode: `COMINDLY_SCIP` → file → `auto`.
    pub fn scip(&self) -> crate::scip::Mode {
        non_empty_env("COMINDLY_SCIP")
            .or_else(|| self.scip.clone())
            .and_then(|s| crate::scip::Mode::parse(&s))
            .unwrap_or(crate::scip::Mode::Auto)
    }
}

/// `$XDG_CONFIG_HOME/comindly/config.toml`, else `~/.config/comindly/config.toml`.
pub fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

/// comindly's config folder.
pub fn config_dir() -> PathBuf {
    config_home().join("comindly")
}

/// A setting to write: a string, a flag or a number; `Remove` deletes the key.
pub enum Setting {
    Text(String),
    Flag(bool),
    Number(i64),
    Remove,
}

/// Write `changes` into `config.toml`, keeping its comments and layout.
pub fn update(changes: &[(&str, Setting)]) -> Result<()> {
    let path = config_path();
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let mut doc: toml_edit::DocumentMut = text.parse().context("parse config.toml")?;
    for (key, value) in changes {
        match value {
            Setting::Text(v) => doc[key] = toml_edit::value(v.as_str()),
            Setting::Flag(v) => doc[key] = toml_edit::value(*v),
            Setting::Number(v) => doc[key] = toml_edit::value(*v),
            Setting::Remove => {
                doc.remove(key);
            }
        }
    }
    std::fs::create_dir_all(path.parent().context("config dir")?)?;
    std::fs::write(&path, doc.to_string()).with_context(|| format!("write {}", path.display()))
}

/// A team setting, which wins over personal ones so everyone on an index runs the same.
fn team<T>(pick: impl Fn(&crate::team::TeamSettings) -> Option<T>) -> Option<T> {
    crate::team::current().and_then(|t| pick(&t))
}

/// The web UI's access token for browsers: created once, so bookmarks keep working.
pub fn ui_token() -> Result<String> {
    let path = config_dir().join("ui-token");
    if let Ok(t) = std::fs::read_to_string(&path) {
        if !t.trim().is_empty() {
            return Ok(t.trim().to_string());
        }
    }
    std::fs::create_dir_all(path.parent().context("config dir")?)?;
    let token = crate::web::new_token();
    std::fs::write(&path, &token).with_context(|| format!("write {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(token)
}

/// `$XDG_DATA_HOME/comindly`, else `~/.local/share/comindly`.
pub fn default_index_dir() -> String {
    data_home().join("comindly").to_string_lossy().into_owned()
}

/// Write a commented `config.toml` with the resolved defaults. Without `force`, errors if one
/// already exists (the caller handles confirmation).
pub fn init(force: bool) -> Result<PathBuf> {
    let path = config_path();
    if path.exists() && !force {
        anyhow::bail!("config already exists at {}", path.display());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let body = format!(
        "# Comindly configuration. CLI flags and environment variables override these.\n\
         # Secrets (OPENAI_API_KEY) belong in the environment, not here.\n\n\
         index_dir   = \"{}\"\n\
         llm_model   = \"{}\"\n\
         embed_model = \"{}\"\n\
         format      = \"md\"   # default output when --format is absent (search: md|table, serve: md|json)\n\
         # llm_base_url = \"http://localhost:11434/v1\"  # Ollama / vLLM / LiteLLM proxy\n\n\
         # index/link step defaults (CLI --x / --no-x override). enrich/flows/guide call an LLM\n\
         # (cost + sends code; need OPENAI_API_KEY). Set false to skip; they also skip with a\n\
         # warning if no key is set.\n\
         embed  = true\n\
         enrich = true\n\
         flows  = true\n\
         guide  = true\n\n\
         # SCIP (compiler-grade resolution): auto = use index.scip or installed indexers,\n\
         # require = fail without it, off = tree-sitter only.\n\
         scip   = \"auto\"\n\n\
         # Cost caps for LLM steps. Omit for no cap (cover the whole codebase).\n\
         # max_enrich = 200   # max symbols enriched by --enrich\n\
         # max_flows  = 50    # max flows narrated by --flows\n",
        default_index_dir(),
        crate::llm::DEFAULT_MODEL,
        crate::embed::DEFAULT_MODEL,
    );
    std::fs::write(&path, body).with_context(|| format!("write {}", path.display()))?;
    Ok(path)
}

fn non_empty_env(key: &str) -> Option<String> {
    env(key).filter(|v| !v.is_empty())
}

/// An environment variable, if set.
pub fn env(key: &str) -> Option<String> {
    std::env::var(key).ok()
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
}

fn config_home() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| home().join(".config"))
}

fn data_home() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| home().join(".local").join("share"))
}

/// Expand a leading `~` to `$HOME`; leave URIs (`s3://…`) and absolute paths untouched.
pub fn expand_tilde(s: &str) -> String {
    if let Some(rest) = s.strip_prefix("~/") {
        return home().join(rest).to_string_lossy().into_owned();
    }
    if s == "~" {
        return home().to_string_lossy().into_owned();
    }
    s.to_string()
}
