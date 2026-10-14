//! Comind — deterministic, cross-repo code intelligence for coding agents.
//!
//! One crate, one module per pipeline stage:
//! `model` (SCIP identity + Symbol/Edge) → `parse` (tree-sitter) → `git` (incremental) →
//! `resolve` (cross-repo binding) → `index` (LanceDB/S3 store) → `graph` (ripple/thread/…) →
//! `embed` (Model2Vec + hybrid rank) → `llm` (enrichment) → `mcp` (agent server).

pub mod model;

pub mod config;
pub mod conventions;

pub mod facts;
pub mod git;
pub mod lang;
pub mod manifests;
pub mod packages;
pub mod parse;
pub mod recipes;
pub mod resolve;
pub mod scip;

pub mod eval;

pub mod index;
pub mod search;

pub mod embed;
pub mod graph;

pub mod ledger;
pub mod llm;

pub mod styleguide;

pub mod mcp;

pub mod ui;
