//! Comind — deterministic, cross-repo code intelligence for coding agents.
//!
//! One crate, one module per pipeline stage:
//! `model` (SCIP identity + Symbol/Edge) → `parse` (tree-sitter) → `git` (incremental) →
//! `resolve` (cross-repo binding) → `index` (LanceDB/S3 store) → `graph` (ripple/thread/…) →
//! `embed` (Model2Vec + hybrid rank) → `llm` (enrichment) → `mcp` (agent server).

pub mod model;

pub mod config;

pub mod git;
pub mod parse;
pub mod resolve;

pub mod index;
pub mod search;

pub mod embed;
pub mod graph;

pub mod llm;

pub mod mcp;

pub mod ui;
