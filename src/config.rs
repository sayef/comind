//! Comind configuration — persistent defaults for the data location and model choices.
//!
//! Precedence, highest first: CLI flag → environment variable → `config.toml` → built-in default.
//! Secrets (`OPENAI_API_KEY`) are never read from the file; keep them in the environment.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::PathBuf;

/// Non-secret preferences, all optional. Read from `config.toml` if present.
#[derive(Debug, Default, Deserialize)]
pub struct Config {
    /// Default LanceDB location for `--to` / `--from` (local path or `s3://…`).
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
                eprintln!("comind: ignoring {}: {e}", path.display());
                Self::default()
            }
        }
    }

    /// Resolve the index location: flag → `COMIND_INDEX_DIR` → file → `~/.local/share/comind`.
    pub fn index_dir(&self, flag: Option<&str>) -> String {
        if let Some(f) = flag {
            return expand_tilde(f);
        }
        if let Some(e) = non_empty_env("COMIND_INDEX_DIR") {
            return expand_tilde(&e);
        }
        if let Some(d) = &self.index_dir {
            return expand_tilde(d);
        }
        default_index_dir()
    }

    /// Resolve the graph dataset to read from. An explicit flag is used verbatim (callers pass
    /// the full `<root>/_graph` path); otherwise default to `<index_dir>/_graph`, matching where
    /// `link` writes.
    pub fn graph_dir(&self, flag: Option<&str>) -> String {
        match flag {
            Some(f) => expand_tilde(f),
            None => format!("{}/_graph", self.index_dir(None).trim_end_matches('/')),
        }
    }

    /// Resolve the LLM model: `COMIND_LLM_MODEL` → file → `crate::llm::DEFAULT_MODEL`.
    pub fn llm_model(&self) -> String {
        non_empty_env("COMIND_LLM_MODEL")
            .or_else(|| self.llm_model.clone())
            .unwrap_or_else(|| crate::llm::DEFAULT_MODEL.to_string())
    }

    /// Resolve the embedding model: `COMIND_EMBED_MODEL` → file → `crate::embed::DEFAULT_MODEL`.
    pub fn embed_model(&self) -> String {
        non_empty_env("COMIND_EMBED_MODEL")
            .or_else(|| self.embed_model.clone())
            .unwrap_or_else(|| crate::embed::DEFAULT_MODEL.to_string())
    }

    /// Resolve an OpenAI-compatible base URL: `COMIND_LLM_BASE_URL` → file → none.
    pub fn llm_base_url(&self) -> Option<String> {
        non_empty_env("COMIND_LLM_BASE_URL").or_else(|| self.llm_base_url.clone())
    }

    /// Cap on enriched symbols (`max_enrich` in the file); `usize::MAX` = no cap.
    pub fn max_enrich(&self) -> usize {
        self.max_enrich.unwrap_or(usize::MAX)
    }

    /// Cap on narrated flows (`max_flows` in the file); `usize::MAX` = no cap.
    pub fn max_flows(&self) -> usize {
        self.max_flows.unwrap_or(usize::MAX)
    }
}

/// `$XDG_CONFIG_HOME/comind/config.toml`, else `~/.config/comind/config.toml`.
pub fn config_path() -> PathBuf {
    config_home().join("comind").join("config.toml")
}

/// `$XDG_DATA_HOME/comind`, else `~/.local/share/comind`.
pub fn default_index_dir() -> String {
    data_home().join("comind").to_string_lossy().into_owned()
}

/// Write a commented `config.toml` with the resolved defaults. Errors if one already exists.
pub fn init() -> Result<PathBuf> {
    let path = config_path();
    if path.exists() {
        anyhow::bail!("config already exists at {}", path.display());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let body = format!(
        "# Comind configuration. CLI flags and environment variables override these.\n\
         # Secrets (OPENAI_API_KEY) belong in the environment, not here.\n\n\
         index_dir   = \"{}\"\n\
         llm_model   = \"{}\"\n\
         embed_model = \"{}\"\n\
         # llm_base_url = \"http://localhost:11434/v1\"  # Ollama / vLLM / LiteLLM proxy\n\n\
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
    std::env::var(key).ok().filter(|v| !v.is_empty())
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
fn expand_tilde(s: &str) -> String {
    if let Some(rest) = s.strip_prefix("~/") {
        return home().join(rest).to_string_lossy().into_owned();
    }
    if s == "~" {
        return home().to_string_lossy().into_owned();
    }
    s.to_string()
}
