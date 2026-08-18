//! Comind LLM enrichment — per-symbol summaries, symbol-aware query generation, and a
//! repo style guide, via **Rig** (provider-agnostic: OpenAI by default; any OpenAI-compatible
//! endpoint via `COMIND_LLM_BASE_URL`; Bedrock/Vertex/others can be added behind cargo features).
//!
//! **Opt-in / data egress:** these functions send code (signatures, snippets) to the configured
//! LLM provider. They run only when the caller explicitly enables enrichment
//! (`comind link --enrich`), never as part of plain indexing. Requires `OPENAI_API_KEY` (or a
//! provider key + `COMIND_LLM_BASE_URL`).

use anyhow::{Context, Result};
use futures::stream::{self, StreamExt};
use rig::client::{CompletionClient, ProviderClient};
use rig::completion::Prompt;
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
}

/// LLM-generated enrichment for one symbol.
#[derive(Debug, Clone)]
pub struct Enrichment {
    pub summary: String,
    pub queries: Vec<String>,
}

impl LlmClient {
    /// Build a client from the environment (`OPENAI_API_KEY`, optional `COMIND_LLM_MODEL`).
    pub fn from_env() -> Result<Self> {
        let model = std::env::var("COMIND_LLM_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.to_string());
        // Default: OpenAI via OPENAI_API_KEY. Point at any OpenAI-compatible endpoint
        // (LiteLLM proxy, Ollama, vLLM, Azure) with COMIND_LLM_BASE_URL.
        let client = match std::env::var("COMIND_LLM_BASE_URL") {
            Ok(base) => {
                let key =
                    std::env::var("OPENAI_API_KEY").unwrap_or_else(|_| "sk-noauth".to_string());
                openai::CompletionsClient::builder()
                    .api_key(&key)
                    .base_url(&base)
                    .build()
                    .map_err(|e| anyhow::anyhow!("LLM client (COMIND_LLM_BASE_URL): {e}"))?
            }
            // Rig's from_env reads OPENAI_API_KEY (and honours OPENAI_BASE_URL too).
            Err(_) => openai::CompletionsClient::from_env().map_err(|e| {
                anyhow::anyhow!(
                    "OpenAI client (set OPENAI_API_KEY, or COMIND_LLM_BASE_URL for an OpenAI-compatible endpoint): {e}"
                )
            })?,
        };
        Ok(Self { client, model })
    }

    async fn complete(&self, system: &str, user: &str, max_tokens: u32) -> Result<String> {
        // A Rig "agent" with a preamble is just a system-prompted completion — no tools/RAG.
        let agent = self
            .client
            .agent(&self.model)
            .preamble(system)
            .max_tokens(max_tokens as u64)
            .build();
        let resp = agent.prompt(user).await.context("LLM completion")?;
        Ok(resp.trim().to_string())
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

    /// Infer a short coding style guide from a sample of signatures/snippets.
    pub async fn style_guide(&self, samples: &[String]) -> Result<String> {
        let system = "You are a senior engineer. From these code samples, infer a concise, \
             practical coding style guide (naming, structure, error handling, typing). \
             Use terse markdown bullets. No preamble.";
        let joined = samples.join("\n---\n");
        let user = format!("Samples:\n{joined}");
        self.complete(system, &user, 500).await
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
