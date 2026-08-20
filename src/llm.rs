//! Comind LLM enrichment — per-symbol summaries, symbol-aware query generation, and a
//! repo style guide, via **Rig** (provider-agnostic: OpenAI by default; any OpenAI-compatible
//! endpoint via `COMIND_LLM_BASE_URL`; Bedrock/Vertex/others can be added behind cargo features).
//!
//! **Opt-in / data egress:** these functions send code (signatures, snippets) to the configured
//! LLM provider. They run only when the caller explicitly enables enrichment
//! (`comind link --enrich`), never as part of plain indexing. Requires `OPENAI_API_KEY` (or a
//! provider key + `COMIND_LLM_BASE_URL`).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::{Context, Result};
use futures::stream::{self, StreamExt};
use rig::client::{CompletionClient, ProviderClient};
use rig::completion::{AssistantContent, Completion};
use rig::providers::openai;

/// Default model — cheap and fast, suitable for bulk symbol summaries.
pub const DEFAULT_MODEL: &str = "gpt-4o-mini";

/// Max concurrent in-flight enrichment requests.
const CONCURRENCY: usize = 8;

pub struct LlmClient {
    // Chat Completions API (works with OpenAI *and* any OpenAI-compatible endpoint), not the
    // Responses API — proxies/Ollama/vLLM implement `/chat/completions`, not `/responses`.
    client: openai::CompletionsClient,
    model: String,
    // Cumulative token usage across all calls (for live progress + a final total).
    input_tokens: Arc<AtomicU64>,
    output_tokens: Arc<AtomicU64>,
}

/// LLM-generated enrichment for one symbol.
#[derive(Debug, Clone)]
pub struct Enrichment {
    pub summary: String,
    pub queries: Vec<String>,
}

impl LlmClient {
    /// Build a client from config + environment. Model and base URL resolve via
    /// [`crate::config`] (env → `config.toml` → default); the key stays env-only
    /// (`OPENAI_API_KEY`).
    pub fn from_env() -> Result<Self> {
        let cfg = crate::config::Config::load();
        let model = cfg.llm_model();
        // Default: OpenAI via OPENAI_API_KEY. Point at any OpenAI-compatible endpoint
        // (LiteLLM proxy, Ollama, vLLM, Azure) with COMIND_LLM_BASE_URL.
        let client = match cfg.llm_base_url() {
            Some(base) => {
                let key =
                    std::env::var("OPENAI_API_KEY").unwrap_or_else(|_| "sk-noauth".to_string());
                openai::CompletionsClient::builder()
                    .api_key(&key)
                    .base_url(&base)
                    .build()
                    .map_err(|e| anyhow::anyhow!("LLM client (COMIND_LLM_BASE_URL): {e}"))?
            }
            // Rig's from_env reads OPENAI_API_KEY (and honours OPENAI_BASE_URL too).
            None => openai::CompletionsClient::from_env().map_err(|e| {
                anyhow::anyhow!(
                    "OpenAI client (set OPENAI_API_KEY, or COMIND_LLM_BASE_URL for an OpenAI-compatible endpoint): {e}"
                )
            })?,
        };
        Ok(Self {
            client,
            model,
            input_tokens: Arc::new(AtomicU64::new(0)),
            output_tokens: Arc::new(AtomicU64::new(0)),
        })
    }

    /// Cumulative `(input, output)` tokens used so far, across all calls.
    pub fn token_usage(&self) -> (u64, u64) {
        (
            self.input_tokens.load(Ordering::Relaxed),
            self.output_tokens.load(Ordering::Relaxed),
        )
    }

    async fn complete(&self, system: &str, user: &str, max_tokens: u32) -> Result<String> {
        // A Rig "agent" with a preamble is just a system-prompted completion — no tools/RAG.
        // Use the completion API (not `prompt()`) so we get token `usage` back from the provider.
        let agent = self
            .client
            .agent(&self.model)
            .preamble(system)
            .max_tokens(max_tokens as u64)
            .build();
        let resp = agent
            .completion(user, Vec::<rig::completion::Message>::new())
            .await
            .context("LLM request")?
            .send()
            .await
            .context("LLM completion")?;
        self.input_tokens
            .fetch_add(resp.usage.input_tokens, Ordering::Relaxed);
        self.output_tokens
            .fetch_add(resp.usage.output_tokens, Ordering::Relaxed);
        let text: String = resp
            .choice
            .iter()
            .filter_map(|c| match c {
                AssistantContent::Text(t) => Some(t.text.as_str()),
                _ => None,
            })
            .collect();
        Ok(text.trim().to_string())
    }

    /// One-line summary of what a symbol does, plus a few natural-language queries it answers.
    pub async fn enrich_symbol(
        &self,
        name: &str,
        signature: &str,
        context: &str,
    ) -> Result<Enrichment> {
        let system = "You document code for a search index. Reply in exactly this format:\n\
             SUMMARY: <one concise sentence describing what this does>\n\
             QUERIES: <3 natural-language questions a developer might ask that this code answers, separated by ` | `>\n\
             No preamble, no code fences.";
        let user = format!("Symbol: {name}\nSignature: {signature}\nContext: {context}");
        let raw = self.complete(system, &user, 160).await?;
        Ok(parse_enrichment(&raw))
    }

    /// Enrich many symbols concurrently. Each item is `(name, signature, context)`; failures
    /// map to `None` so one bad call doesn't sink the batch.
    pub async fn enrich_batch(
        &self,
        items: &[(String, String, String)],
    ) -> Vec<Option<Enrichment>> {
        stream::iter(items.iter())
            .map(|(n, s, c)| async move { self.enrich_symbol(n, s, c).await.ok() })
            .buffered(CONCURRENCY)
            .collect()
            .await
    }

    /// Synthesize an evidence-based coding style guide for one repo from a measured evidence pack
    /// (naming/size/idiom stats + enforced-config facts). The prompt forbids generic advice and
    /// requires every rule to cite the evidence.
    pub async fn style_guide(&self, repo: &str, evidence: &str) -> Result<String> {
        let system = "You are a senior engineer writing the house-style guide a new hire (and an AI \
            coding agent) must follow to write code INDISTINGUISHABLE from this repository's. You are \
            given real evidence: the dependency stack, an import-frequency table, measured stats, and \
            REPRESENTATIVE CODE EXCERPTS. Read the excerpts carefully — the valuable conventions \
            (how they use each library, AWS/DB/HTTP I/O, their own shared modules, architecture) live \
            in the code, not in the stats.\n\
            Hard rules:\n\
            - Cite a real FILE PATH (and symbol when possible) for every non-trivial claim. If you \
            can't point to evidence, don't say it.\n\
            - Prefer the repo's OWN wrapper/abstraction over the raw library wherever the excerpts \
            show one (e.g. 'AWS I/O goes through <module>.<fn>(), never raw boto3.client'). Name it.\n\
            - NEVER output generic advice ('use meaningful names', 'write tests', 'handle errors \
            gracefully', 'follow best practices'). If a line would apply to ANY codebase, delete it.\n\
            - Strength: MUST/NEVER for patterns seen across ≥3 files or enforced by config; 'prefer' \
            for weaker signals. If there's no consistent pattern for a section, write \
            'No consistent convention observed' — do NOT invent one.\n\
            - Each rule: a bold imperative + a short real snippet or file:path reference. Terse.\n\
            Use these `##` sections, dropping any with no evidence:\n\
            1. Stack & dependencies  2. Project layout & architecture (layers, entry points)  \
            3. Library usage (a subsection per KEY dependency: the idiomatic call pattern + a real \
            call-site + what NOT to do)  4. External I/O & infra (AWS/DB/HTTP: wrappers, creds, \
            retries, pagination)  5. Configuration & secrets  6. Error handling & logging  \
            7. API / DTO / validation patterns  8. Testing  9. Naming / typing / docstrings.\n\
            End with a one-line note that this is AI-generated and should be reviewed.";
        let user = format!("Repository: {repo}\n\n# Evidence\n\n{evidence}");
        self.complete(system, &user, 2400).await
    }

    /// Narrate an execution flow: given an entry point and its ordered call trace, produce a
    /// concise markdown walkthrough plus a few flow-oriented questions the flow answers.
    pub async fn narrate_flow(
        &self,
        entry: &str,
        signature: &str,
        trace: &str,
    ) -> Result<(String, Vec<String>)> {
        let system = "You explain how a code flow works to a developer new to the codebase. \
             Given an entry point and its call trace (each line: depth, relation, symbol, \
             location), reply in exactly this format:\n\
             NARRATION: <2-5 sentence walkthrough of what happens step by step; reference the \
             actual symbol names>\n\
             QUERIES: <3 questions a developer might ask about this flow, separated by ` | `>\n\
             No preamble, no code fences.";
        let user = format!("Entry point: {entry}\nSignature: {signature}\nCall trace:\n{trace}");
        let raw = self.complete(system, &user, 320).await?;
        Ok(parse_flow(&raw))
    }
}

/// Parse the `NARRATION:` / `QUERIES:` reply format, tolerantly (narration may span lines).
fn parse_flow(raw: &str) -> (String, Vec<String>) {
    let (narr_part, q_part) = raw.split_once("QUERIES:").unwrap_or((raw, ""));
    let narration = narr_part
        .trim()
        .trim_start_matches("NARRATION:")
        .trim()
        .to_string();
    let queries = q_part
        .split('|')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    (
        if narration.is_empty() {
            raw.trim().to_string()
        } else {
            narration
        },
        queries,
    )
}

/// Parse the `SUMMARY:` / `QUERIES:` reply format, tolerantly.
fn parse_enrichment(raw: &str) -> Enrichment {
    let mut summary = String::new();
    let mut queries = Vec::new();
    for line in raw.lines() {
        let l = line.trim();
        if let Some(rest) = l.strip_prefix("SUMMARY:") {
            summary = rest.trim().to_string();
        } else if let Some(rest) = l.strip_prefix("QUERIES:") {
            queries = rest
                .split('|')
                .map(|q| q.trim().to_string())
                .filter(|q| !q.is_empty())
                .collect();
        }
    }
    if summary.is_empty() {
        // Model didn't follow format — fall back to the first non-empty line.
        summary = raw
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or("")
            .to_string();
    }
    Enrichment { summary, queries }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_enrichment_format() {
        let raw = "SUMMARY: Opens a pooled Postgres connection.\nQUERIES: how to connect to postgres | get a db connection | open database pool";
        let e = parse_enrichment(raw);
        assert_eq!(e.summary, "Opens a pooled Postgres connection.");
        assert_eq!(e.queries.len(), 3);
        assert_eq!(e.queries[0], "how to connect to postgres");
    }

    // Real API call — run with a key: `cargo test -p comind-llm -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn live_enrich_symbol() {
        let c = LlmClient::from_env().expect("OPENAI_API_KEY");
        let e = c
            .enrich_symbol(
                "AsyncTaskRunner",
                "class AsyncTaskRunner(BaseTaskRunner)",
                "acme/database/executors.py",
            )
            .await
            .expect("enrich");
        assert!(!e.summary.is_empty());
        eprintln!("summary: {}", e.summary);
        eprintln!("queries: {:?}", e.queries);
    }
}
