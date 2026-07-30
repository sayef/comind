//! Comind LLM enrichment — per-symbol summaries, symbol-aware query generation, and a
//! repo style guide, via OpenAI.
//!
//! **Opt-in / data egress:** these functions send code (signatures, snippets) to the OpenAI
//! API. They run only when the caller explicitly enables enrichment (`comind link --enrich`),
//! never as part of plain indexing. Requires `OPENAI_API_KEY`.

use anyhow::{Context, Result};
use async_openai::config::OpenAIConfig;
use async_openai::types::chat::{
    ChatCompletionRequestSystemMessageArgs, ChatCompletionRequestUserMessageArgs,
    CreateChatCompletionRequestArgs,
};
use async_openai::Client;
use futures::stream::{self, StreamExt};

/// Default model — cheap and fast, suitable for bulk symbol summaries.
pub const DEFAULT_MODEL: &str = "gpt-4o-mini";

/// Max concurrent in-flight enrichment requests.
const CONCURRENCY: usize = 8;

pub struct LlmClient {
    client: Client<OpenAIConfig>,
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
        if std::env::var("OPENAI_API_KEY").is_err() {
            anyhow::bail!("OPENAI_API_KEY not set (LLM enrichment is opt-in and needs a key)");
        }
        let model = std::env::var("COMIND_LLM_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.to_string());
        Ok(Self {
            client: Client::new(),
            model,
        })
    }

    async fn complete(&self, system: &str, user: &str, max_tokens: u32) -> Result<String> {
        let req = CreateChatCompletionRequestArgs::default()
            .model(&self.model)
            .max_tokens(max_tokens)
            .messages(vec![
                ChatCompletionRequestSystemMessageArgs::default()
                    .content(system)
                    .build()?
                    .into(),
                ChatCompletionRequestUserMessageArgs::default()
                    .content(user)
                    .build()?
                    .into(),
            ])
            .build()?;
        let resp = self
            .client
            .chat()
            .create(req)
            .await
            .context("openai chat")?;
        Ok(resp
            .choices
            .into_iter()
            .next()
            .and_then(|c| c.message.content)
            .unwrap_or_default()
            .trim()
            .to_string())
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
