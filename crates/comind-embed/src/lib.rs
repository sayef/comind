//! Comind embed — local static embeddings for semantic code search.
//!
//! Uses **Model2Vec** (`model2vec-rs`) static embeddings: pure-Rust, CPU-only, no ONNX
//! runtime, tiny + fast (~ms/query) — the approach semble validated for agentic code
//! search. Vectors are L2-normalized, so cosine similarity is a plain dot product.
//!
//! This is the *optional semantic layer*. It rides on top of the deterministic graph:
//! search fuses this signal with lexical BM25 and graph centrality (see comind's hybrid
//! search) — never the sole retrieval path.

pub mod rank;

use anyhow::{Context, Result};
use comind_core::{GlobalSymbolId, Symbol, SymbolKind};
use model2vec_rs::model::StaticModel;

/// A small, well-known static model. Swap for a code-specialized Model2Vec distillation
/// (e.g. a `potion-code` variant) for higher code accuracy — same API.
pub const DEFAULT_MODEL: &str = "minishlab/potion-base-8M";

/// Loads a Model2Vec model (from a Hugging Face repo id or a local path) and encodes text.
pub struct Embedder {
    model: StaticModel,
}

impl Embedder {
    /// Load a model by HF repo id or local path. Normalizes output (cosine == dot product).
    pub fn load(repo_or_path: &str) -> Result<Self> {
        let model = StaticModel::from_pretrained(repo_or_path, None, Some(true), None)
            .with_context(|| format!("load Model2Vec model `{repo_or_path}`"))?;
        Ok(Self { model })
    }

    pub fn load_default() -> Result<Self> {
        Self::load(DEFAULT_MODEL)
    }

    /// Embed a batch of documents.
    pub fn embed(&self, texts: &[String]) -> Vec<Vec<f32>> {
        self.model.encode(texts)
    }

    /// Embed a single query string.
    pub fn embed_query(&self, query: &str) -> Vec<f32> {
        self.model.encode_single(query)
    }
}

/// Build the text used to represent a symbol for embedding: name, signature, the
/// human-readable descriptor path, and docstring — the tokens a natural-language query
/// about this code would plausibly match.
pub fn symbol_text(s: &Symbol) -> String {
    let mut parts = vec![s.name.clone()];
    if let Some(sig) = &s.signature {
        parts.push(sig.clone());
    }
    // descriptor path as words, e.g. "acme/database/config/DbConfig" -> readable tokens
    parts.push(s.id.descriptor.replace(['/', '_', '.'], " "));
    if let Some(doc) = &s.docstring {
        parts.push(doc.clone());
    }
    parts.join("  ")
}

/// Cosine similarity for L2-normalized vectors == dot product.
fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// An in-memory semantic index over symbols: ids + normalized vectors. Small and fast;
/// for the shared org index these vectors are persisted as a Lance column instead.
pub struct SemanticIndex {
    pub ids: Vec<GlobalSymbolId>,
    pub vectors: Vec<Vec<f32>>,
}

impl SemanticIndex {
    /// Embed all non-file symbols (files aren't useful semantic-search targets).
    pub fn build(embedder: &Embedder, symbols: &[Symbol]) -> Self {
        let targets: Vec<&Symbol> = symbols
            .iter()
            .filter(|s| !matches!(s.kind, SymbolKind::File))
            .collect();
        let texts: Vec<String> = targets.iter().map(|s| symbol_text(s)).collect();
        let vectors = embedder.embed(&texts);
        let ids = targets.iter().map(|s| s.id.clone()).collect();
        Self { ids, vectors }
    }

    /// Top-`k` symbols by semantic similarity to `query`. Returns `(index, score)` where
    /// `index` points into `self.ids`.
    pub fn search(&self, embedder: &Embedder, query: &str, k: usize) -> Vec<(usize, f32)> {
        let q = embedder.embed_query(query);
        let mut scored: Vec<(usize, f32)> = self
            .vectors
            .iter()
            .enumerate()
            .map(|(i, v)| (i, dot(&q, v)))
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(k);
        scored
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Downloads a model from Hugging Face — run explicitly: `cargo test -p comind-embed -- --ignored`.
    #[test]
    #[ignore]
    fn embeds_and_ranks_semantically() {
        let e = Embedder::load_default().expect("load model");
        let docs: Vec<String> = vec![
            "open_database_connection  def open_database_connection(host, port)".into(),
            "parse_json  def parse_json(text: str) -> dict".into(),
            "UserAuthentication  class UserAuthentication  login verify password".into(),
        ];
        let vecs = e.embed(&docs);
        assert_eq!(vecs.len(), 3);
        assert!(!vecs[0].is_empty());

        // A query about auth should rank the authentication doc highest.
        let q = e.embed_query("log in and check a user's password");
        let best = (0..3)
            .max_by(|&i, &j| dot(&q, &vecs[i]).partial_cmp(&dot(&q, &vecs[j])).unwrap())
            .unwrap();
        assert_eq!(best, 2, "auth doc should rank first");
    }
}
