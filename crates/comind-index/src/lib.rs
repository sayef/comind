//! CoMind index — persist the symbol/edge graph to LanceDB (local dir or S3), versioned.
//!
//! Phase 2 scope: write `symbols` and `edges` tables and read their row counts back,
//! proving the round-trip against real object storage. Incremental change detection and
//! the versioned "latest" pointer build on top of Lance's native table versioning next.
//!
//! Credentials/region come from the standard AWS environment (`AWS_ACCESS_KEY_ID`,
//! `AWS_SESSION_TOKEN`, `AWS_REGION`, ...), so an SSO profile works via
//! `aws configure export-credentials --profile <p> --format env`.

use std::sync::Arc;

use anyhow::{Context, Result};
use comind_core::{Edge, Symbol};
use lancedb::arrow::arrow_array::{
    ArrayRef, BooleanArray, FixedSizeListArray, Float32Array, RecordBatch, StringArray,
    UInt32Array,
};
use lancedb::arrow::arrow_schema::{DataType, Field, Schema};
use lancedb::table::AddDataMode;

fn symbols_schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("symbol_id", DataType::Utf8, false),
        Field::new("scheme", DataType::Utf8, false),
        Field::new("package_manager", DataType::Utf8, false),
        Field::new("package", DataType::Utf8, false),
        Field::new("version", DataType::Utf8, false),
        Field::new("descriptor", DataType::Utf8, false),
        Field::new("name", DataType::Utf8, false),
        Field::new("kind", DataType::Utf8, false),
        Field::new("language", DataType::Utf8, false),
        Field::new("repo", DataType::Utf8, false),
        Field::new("file_path", DataType::Utf8, false),
        Field::new("line_start", DataType::UInt32, false),
        Field::new("col_start", DataType::UInt32, false),
        Field::new("line_end", DataType::UInt32, false),
        Field::new("col_end", DataType::UInt32, false),
        Field::new("signature", DataType::Utf8, true),
        Field::new("docstring", DataType::Utf8, true),
    ]))
}

fn edges_schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("src_id", DataType::Utf8, false),
        Field::new("dst_id", DataType::Utf8, false),
        Field::new("kind", DataType::Utf8, false),
        Field::new("confidence", DataType::Float32, false),
        Field::new("cross_repo", DataType::Boolean, false),
    ]))
}

fn symbols_batch(symbols: &[Symbol]) -> Result<RecordBatch> {
    let arrays: Vec<ArrayRef> = vec![
        Arc::new(StringArray::from_iter_values(
            symbols.iter().map(|s| s.id.render()),
        )),
        Arc::new(StringArray::from_iter_values(
            symbols.iter().map(|s| s.id.scheme.clone()),
        )),
        Arc::new(StringArray::from_iter_values(
            symbols.iter().map(|s| s.id.package_manager.clone()),
        )),
        Arc::new(StringArray::from_iter_values(
            symbols.iter().map(|s| s.id.package.clone()),
        )),
        Arc::new(StringArray::from_iter_values(
            symbols.iter().map(|s| s.id.version.clone()),
        )),
        Arc::new(StringArray::from_iter_values(
            symbols.iter().map(|s| s.id.descriptor.clone()),
        )),
        Arc::new(StringArray::from_iter_values(
            symbols.iter().map(|s| s.name.clone()),
        )),
        Arc::new(StringArray::from_iter_values(
            symbols.iter().map(|s| format!("{:?}", s.kind)),
        )),
        Arc::new(StringArray::from_iter_values(
            symbols.iter().map(|s| format!("{:?}", s.language)),
        )),
        Arc::new(StringArray::from_iter_values(
            symbols.iter().map(|s| s.repo.0.clone()),
        )),
        Arc::new(StringArray::from_iter_values(
            symbols.iter().map(|s| s.file_path.clone()),
        )),
        Arc::new(UInt32Array::from_iter_values(
            symbols.iter().map(|s| s.range.start.line),
        )),
        Arc::new(UInt32Array::from_iter_values(
            symbols.iter().map(|s| s.range.start.column),
        )),
        Arc::new(UInt32Array::from_iter_values(
            symbols.iter().map(|s| s.range.end.line),
        )),
        Arc::new(UInt32Array::from_iter_values(
            symbols.iter().map(|s| s.range.end.column),
        )),
        Arc::new(StringArray::from_iter(
            symbols.iter().map(|s| s.signature.clone()),
        )),
        Arc::new(StringArray::from_iter(
            symbols.iter().map(|s| s.docstring.clone()),
        )),
    ];
    RecordBatch::try_new(symbols_schema(), arrays).context("build symbols batch")
}

fn edges_batch(edges: &[Edge]) -> Result<RecordBatch> {
    let arrays: Vec<ArrayRef> = vec![
        Arc::new(StringArray::from_iter_values(
            edges.iter().map(|e| e.src.render()),
        )),
        Arc::new(StringArray::from_iter_values(
            edges.iter().map(|e| e.dst.render()),
        )),
        Arc::new(StringArray::from_iter_values(
            edges.iter().map(|e| format!("{:?}", e.kind)),
        )),
        Arc::new(Float32Array::from_iter_values(
            edges.iter().map(|e| e.confidence),
        )),
        Arc::new(BooleanArray::from(
            edges.iter().map(|e| e.cross_repo).collect::<Vec<_>>(),
        )),
    ];
    RecordBatch::try_new(edges_schema(), arrays).context("build edges batch")
}

/// Write `batch` as the full contents of table `name`, creating it if absent and otherwise
/// overwriting in place. Overwrite creates a **new Lance version** while retaining prior
/// versions — so the newest version manifest is the atomic "latest" pointer, and older
/// versions remain available for pinning/rollback (`checkout`). Returns the new version.
async fn write_versioned(db: &lancedb::Connection, name: &str, batch: RecordBatch) -> Result<u64> {
    // lancedb 0.31 implements `Scannable` for `RecordBatch`, so pass it directly.
    let tbl = match db.open_table(name).execute().await {
        Ok(t) => {
            t.add(batch)
                .mode(AddDataMode::Overwrite)
                .execute()
                .await
                .with_context(|| format!("overwrite table `{name}`"))?;
            t
        }
        Err(_) => db
            .create_table(name, batch)
            .execute()
            .await
            .with_context(|| format!("create table `{name}`"))?,
    };
    tbl.version()
        .await
        .with_context(|| format!("read version of `{name}`"))
}

/// Write the symbol/edge graph to a LanceDB dataset at `uri`
/// (e.g. `s3://bucket/prefix` or a local path). Returns the new `(symbols, edges)` versions.
pub async fn write_graph(uri: &str, symbols: &[Symbol], edges: &[Edge]) -> Result<(u64, u64)> {
    let db = lancedb::connect(uri)
        .execute()
        .await
        .with_context(|| format!("connect lancedb at {uri}"))?;
    let sv = write_versioned(&db, "symbols", symbols_batch(symbols)?).await?;
    let ev = write_versioned(&db, "edges", edges_batch(edges)?).await?;
    Ok((sv, ev))
}

/// Read the current `(symbols, edges)` versions — the atomic latest pointer.
pub async fn latest_versions(uri: &str) -> Result<(u64, u64)> {
    let db = lancedb::connect(uri).execute().await?;
    let sv = db.open_table("symbols").execute().await?.version().await?;
    let ev = db.open_table("edges").execute().await?.version().await?;
    Ok((sv, ev))
}

// ---- reading the persisted graph back into core types ----------------------------------

use comind_core::{EdgeKind, GlobalSymbolId, Language, Position, Range, RepoId, SymbolKind};
use futures::TryStreamExt;
use lancedb::arrow::arrow_array::Array;
use lancedb::query::ExecutableQuery;

fn kind_from_str(s: &str) -> SymbolKind {
    match s {
        "File" => SymbolKind::File,
        "Module" => SymbolKind::Module,
        "Namespace" => SymbolKind::Namespace,
        "Class" => SymbolKind::Class,
        "Interface" => SymbolKind::Interface,
        "Trait" => SymbolKind::Trait,
        "Enum" => SymbolKind::Enum,
        "Struct" => SymbolKind::Struct,
        "Function" => SymbolKind::Function,
        "Method" => SymbolKind::Method,
        "Field" => SymbolKind::Field,
        "Variable" => SymbolKind::Variable,
        "Constant" => SymbolKind::Constant,
        "Import" => SymbolKind::Import,
        "TypeAlias" => SymbolKind::TypeAlias,
        "Process" => SymbolKind::Process,
        _ => SymbolKind::Variable,
    }
}

fn lang_from_str(s: &str) -> Language {
    match s {
        "Python" => Language::Python,
        "TypeScript" => Language::TypeScript,
        "JavaScript" => Language::JavaScript,
        "Go" => Language::Go,
        "Rust" => Language::Rust,
        "Java" => Language::Java,
        "Kotlin" => Language::Kotlin,
        "Ruby" => Language::Ruby,
        "CSharp" => Language::CSharp,
        "Cpp" => Language::Cpp,
        "C" => Language::C,
        "Php" => Language::Php,
        "Scala" => Language::Scala,
        "Swift" => Language::Swift,
        // `Other("x")` Debug form, or anything unknown
        other => Language::Other(
            other
                .strip_prefix("Other(\"")
                .and_then(|s| s.strip_suffix("\")"))
                .unwrap_or(other)
                .to_string(),
        ),
    }
}

/// Parse a rendered SCIP id (`scheme pm package version descriptor`) back into components.
fn id_from_rendered(s: &str) -> GlobalSymbolId {
    let mut it = s.splitn(5, ' ');
    let mut next = || it.next().unwrap_or(".").to_string();
    GlobalSymbolId {
        scheme: next(),
        package_manager: next(),
        package: next(),
        version: next(),
        descriptor: next(),
    }
}

fn str_col<'a>(batch: &'a lancedb::arrow::arrow_array::RecordBatch, name: &str) -> Result<&'a StringArray> {
    batch
        .column_by_name(name)
        .and_then(|c| c.as_any().downcast_ref::<StringArray>())
        .with_context(|| format!("column `{name}` missing or not Utf8"))
}

fn u32_col<'a>(batch: &'a lancedb::arrow::arrow_array::RecordBatch, name: &str) -> Result<&'a UInt32Array> {
    batch
        .column_by_name(name)
        .and_then(|c| c.as_any().downcast_ref::<UInt32Array>())
        .with_context(|| format!("column `{name}` missing or not UInt32"))
}

/// Load the persisted org graph back into core types. This is the fast consumer path:
/// `serve`/`explore` load once from Lance (local mmap or S3) instead of re-parsing.
pub async fn read_graph(uri: &str) -> Result<(Vec<Symbol>, Vec<Edge>)> {
    let db = lancedb::connect(uri).execute().await?;

    // symbols
    let mut symbols = Vec::new();
    let mut st = db.open_table("symbols").execute().await?.query().execute().await?;
    while let Some(b) = st.try_next().await? {
        let (scheme, pm, pkg, ver, desc) = (
            str_col(&b, "scheme")?,
            str_col(&b, "package_manager")?,
            str_col(&b, "package")?,
            str_col(&b, "version")?,
            str_col(&b, "descriptor")?,
        );
        let (name, kind, lang, repo, file) = (
            str_col(&b, "name")?,
            str_col(&b, "kind")?,
            str_col(&b, "language")?,
            str_col(&b, "repo")?,
            str_col(&b, "file_path")?,
        );
        let (ls, cs, le, ce) = (
            u32_col(&b, "line_start")?,
            u32_col(&b, "col_start")?,
            u32_col(&b, "line_end")?,
            u32_col(&b, "col_end")?,
        );
        let sig = str_col(&b, "signature")?;
        let doc = str_col(&b, "docstring")?;
        for i in 0..b.num_rows() {
            symbols.push(Symbol {
                id: GlobalSymbolId {
                    scheme: scheme.value(i).to_string(),
                    package_manager: pm.value(i).to_string(),
                    package: pkg.value(i).to_string(),
                    version: ver.value(i).to_string(),
                    descriptor: desc.value(i).to_string(),
                },
                name: name.value(i).to_string(),
                kind: kind_from_str(kind.value(i)),
                language: lang_from_str(lang.value(i)),
                repo: RepoId(repo.value(i).to_string()),
                file_path: file.value(i).to_string(),
                range: Range {
                    start: Position { line: ls.value(i), column: cs.value(i) },
                    end: Position { line: le.value(i), column: ce.value(i) },
                },
                signature: (!sig.is_null(i)).then(|| sig.value(i).to_string()),
                docstring: (!doc.is_null(i)).then(|| doc.value(i).to_string()),
            });
        }
    }

    // edges
    let mut edges = Vec::new();
    let mut st = db.open_table("edges").execute().await?.query().execute().await?;
    while let Some(b) = st.try_next().await? {
        let src = str_col(&b, "src_id")?;
        let dst = str_col(&b, "dst_id")?;
        let kind = str_col(&b, "kind")?;
        let conf = b
            .column_by_name("confidence")
            .and_then(|c| c.as_any().downcast_ref::<Float32Array>())
            .context("confidence column")?;
        let cross = b
            .column_by_name("cross_repo")
            .and_then(|c| c.as_any().downcast_ref::<BooleanArray>())
            .context("cross_repo column")?;
        for i in 0..b.num_rows() {
            edges.push(Edge {
                src: id_from_rendered(src.value(i)),
                dst: id_from_rendered(dst.value(i)),
                kind: edge_kind_from_str(kind.value(i)),
                confidence: conf.value(i),
                cross_repo: cross.value(i),
            });
        }
    }

    Ok((symbols, edges))
}

fn edge_kind_from_str(s: &str) -> EdgeKind {
    match s {
        "Contains" => EdgeKind::Contains,
        "Imports" => EdgeKind::Imports,
        "Calls" => EdgeKind::Calls,
        "Inherits" => EdgeKind::Inherits,
        "Implements" => EdgeKind::Implements,
        "References" => EdgeKind::References,
        "Defines" => EdgeKind::Defines,
        "Uses" => EdgeKind::Uses,
        "ParticipatesIn" => EdgeKind::ParticipatesIn,
        _ => EdgeKind::References,
    }
}

/// Blocking wrapper for [`read_graph`].
pub fn read_graph_blocking(uri: &str) -> Result<(Vec<Symbol>, Vec<Edge>)> {
    runtime()?.block_on(read_graph(uri))
}

/// Read back `(symbol_count, edge_count)` — proves the persisted tables are queryable.
pub async fn count_rows(uri: &str) -> Result<(usize, usize)> {
    let db = lancedb::connect(uri).execute().await?;
    let symbols = db
        .open_table("symbols")
        .execute()
        .await?
        .count_rows(None)
        .await?;
    let edges = db
        .open_table("edges")
        .execute()
        .await?
        .count_rows(None)
        .await?;
    Ok((symbols, edges))
}

/// Blocking wrapper so the (sync) CLI can persist without an async main.
/// Returns the new `(symbols, edges)` Lance versions.
pub fn write_graph_blocking(uri: &str, symbols: &[Symbol], edges: &[Edge]) -> Result<(u64, u64)> {
    runtime()?.block_on(write_graph(uri, symbols, edges))
}

/// Blocking wrapper for the read-back count.
pub fn count_rows_blocking(uri: &str) -> Result<(usize, usize)> {
    runtime()?.block_on(count_rows(uri))
}

/// Blocking wrapper for the current `(symbols, edges)` versions.
pub fn latest_versions_blocking(uri: &str) -> Result<(u64, u64)> {
    runtime()?.block_on(latest_versions(uri))
}

// ---- embeddings (semantic search vectors) -----------------------------------------------

fn embeddings_schema(dim: usize) -> Arc<Schema> {
    let item = Arc::new(Field::new("item", DataType::Float32, true));
    Arc::new(Schema::new(vec![
        Field::new("symbol_id", DataType::Utf8, false),
        Field::new("vector", DataType::FixedSizeList(item, dim as i32), false),
    ]))
}

/// Persist per-symbol embedding vectors as a Lance `embeddings` table
/// (`symbol_id`, `vector: FixedSizeList<Float32>[dim]`) — the fixed-size vector column a
/// Lance vector index can later be built on. Returns the new table version. No-op if empty.
pub async fn write_embeddings(uri: &str, rows: &[(GlobalSymbolId, Vec<f32>)]) -> Result<u64> {
    let Some((_, first)) = rows.first() else {
        return Ok(0);
    };
    let dim = first.len();
    let ids = StringArray::from_iter_values(rows.iter().map(|(id, _)| id.render()));
    let flat: Vec<f32> = rows.iter().flat_map(|(_, v)| v.iter().copied()).collect();
    let values = Arc::new(Float32Array::from(flat));
    let item = Arc::new(Field::new("item", DataType::Float32, true));
    let vectors = FixedSizeListArray::try_new(item, dim as i32, values, None)
        .context("build FixedSizeList vector column")?;
    let batch = RecordBatch::try_new(
        embeddings_schema(dim),
        vec![Arc::new(ids) as ArrayRef, Arc::new(vectors) as ArrayRef],
    )
    .context("build embeddings batch")?;

    let db = lancedb::connect(uri).execute().await?;
    write_versioned(&db, "embeddings", batch).await
}

/// Load persisted embeddings back as `(symbol_id, vector)` pairs. `Ok(None)` if the table
/// doesn't exist yet (search then falls back to embedding on the fly).
pub async fn read_embeddings(uri: &str) -> Result<Option<Vec<(GlobalSymbolId, Vec<f32>)>>> {
    let db = lancedb::connect(uri).execute().await?;
    let Ok(tbl) = db.open_table("embeddings").execute().await else {
        return Ok(None);
    };
    let mut out = Vec::new();
    let mut st = tbl.query().execute().await?;
    while let Some(b) = st.try_next().await? {
        let ids = str_col(&b, "symbol_id")?;
        let vecs = b
            .column_by_name("vector")
            .and_then(|c| c.as_any().downcast_ref::<FixedSizeListArray>())
            .context("vector column not FixedSizeList")?;
        for i in 0..b.num_rows() {
            let sub = vecs.value(i);
            let f = sub
                .as_any()
                .downcast_ref::<Float32Array>()
                .context("vector items not Float32")?;
            out.push((id_from_rendered(ids.value(i)), f.values().to_vec()));
        }
    }
    Ok(Some(out))
}

/// Blocking wrapper for [`write_embeddings`].
pub fn write_embeddings_blocking(uri: &str, rows: &[(GlobalSymbolId, Vec<f32>)]) -> Result<u64> {
    runtime()?.block_on(write_embeddings(uri, rows))
}

// ---- repo metadata (last-indexed commit for incremental) --------------------------------

/// Record the commit a repo dataset was indexed at (single-row `repo_meta` table).
pub async fn write_repo_meta(uri: &str, repo: &str, commit: &str) -> Result<()> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("repo", DataType::Utf8, false),
        Field::new("commit", DataType::Utf8, false),
    ]));
    let batch = RecordBatch::try_new(
        schema,
        vec![
            Arc::new(StringArray::from(vec![repo])) as ArrayRef,
            Arc::new(StringArray::from(vec![commit])) as ArrayRef,
        ],
    )
    .context("build repo_meta batch")?;
    let db = lancedb::connect(uri).execute().await?;
    write_versioned(&db, "repo_meta", batch).await?;
    Ok(())
}

/// The commit a repo dataset was last indexed at, if recorded.
pub async fn read_repo_meta(uri: &str) -> Result<Option<String>> {
    let db = lancedb::connect(uri).execute().await?;
    let Ok(tbl) = db.open_table("repo_meta").execute().await else {
        return Ok(None);
    };
    let mut st = tbl.query().execute().await?;
    while let Some(b) = st.try_next().await? {
        if b.num_rows() > 0 {
            return Ok(Some(str_col(&b, "commit")?.value(0).to_string()));
        }
    }
    Ok(None)
}

/// Blocking wrapper for [`write_repo_meta`].
pub fn write_repo_meta_blocking(uri: &str, repo: &str, commit: &str) -> Result<()> {
    runtime()?.block_on(write_repo_meta(uri, repo, commit))
}

/// Record the indexed commit for *several* repos (multi-row `repo_meta`) — used by the org
/// `link` so an incremental rebuild knows which repos changed.
pub async fn write_repo_commits(uri: &str, commits: &[(String, String)]) -> Result<()> {
    if commits.is_empty() {
        return Ok(());
    }
    let schema = Arc::new(Schema::new(vec![
        Field::new("repo", DataType::Utf8, false),
        Field::new("commit", DataType::Utf8, false),
    ]));
    let batch = RecordBatch::try_new(
        schema,
        vec![
            Arc::new(StringArray::from_iter_values(commits.iter().map(|(r, _)| r.clone())))
                as ArrayRef,
            Arc::new(StringArray::from_iter_values(commits.iter().map(|(_, c)| c.clone())))
                as ArrayRef,
        ],
    )
    .context("build repo_commits batch")?;
    let db = lancedb::connect(uri).execute().await?;
    write_versioned(&db, "repo_meta", batch).await?;
    Ok(())
}

/// Read all recorded `repo -> commit` pairs (empty map if none).
pub async fn read_repo_commits(uri: &str) -> Result<std::collections::HashMap<String, String>> {
    let db = lancedb::connect(uri).execute().await?;
    let mut map = std::collections::HashMap::new();
    let Ok(tbl) = db.open_table("repo_meta").execute().await else {
        return Ok(map);
    };
    let mut st = tbl.query().execute().await?;
    while let Some(b) = st.try_next().await? {
        let repo = str_col(&b, "repo")?;
        let commit = str_col(&b, "commit")?;
        for i in 0..b.num_rows() {
            map.insert(repo.value(i).to_string(), commit.value(i).to_string());
        }
    }
    Ok(map)
}

/// Blocking wrapper for [`write_repo_commits`].
pub fn write_repo_commits_blocking(uri: &str, commits: &[(String, String)]) -> Result<()> {
    runtime()?.block_on(write_repo_commits(uri, commits))
}

/// Blocking wrapper for [`read_repo_commits`].
pub fn read_repo_commits_blocking(uri: &str) -> Result<std::collections::HashMap<String, String>> {
    runtime()?.block_on(read_repo_commits(uri))
}

/// Blocking wrapper for [`read_repo_meta`].
pub fn read_repo_meta_blocking(uri: &str) -> Result<Option<String>> {
    runtime()?.block_on(read_repo_meta(uri))
}

// ---- hybrid search table (BM25 FTS + vector, native LanceDB fusion) ---------------------

use lance_index::scalar::FullTextSearchQuery;
use lancedb::index::scalar::FtsIndexBuilder;
use lancedb::index::Index;
use lancedb::query::{QueryBase, QueryExecutionOptions, Select};

fn search_schema(dim: usize) -> Arc<Schema> {
    let item = Arc::new(Field::new("item", DataType::Float32, true));
    Arc::new(Schema::new(vec![
        Field::new("symbol_id", DataType::Utf8, false),
        Field::new("text", DataType::Utf8, false),
        Field::new("vector", DataType::FixedSizeList(item, dim as i32), false),
    ]))
}

/// Write the denormalized retrieval table (`symbol_id`, `text`, `vector`) and build a native
/// BM25 full-text index on `text`. Hybrid search then fuses this FTS index with vector search
/// using LanceDB's own RRF reranker — no hand-rolled BM25.
pub async fn write_search_table(
    uri: &str,
    rows: &[(GlobalSymbolId, String, Vec<f32>)],
) -> Result<u64> {
    let Some((_, _, first)) = rows.first() else {
        return Ok(0);
    };
    let dim = first.len();
    let ids = StringArray::from_iter_values(rows.iter().map(|(id, _, _)| id.render()));
    let texts = StringArray::from_iter_values(rows.iter().map(|(_, t, _)| t.clone()));
    let flat: Vec<f32> = rows.iter().flat_map(|(_, _, v)| v.iter().copied()).collect();
    let item = Arc::new(Field::new("item", DataType::Float32, true));
    let vectors = FixedSizeListArray::try_new(item, dim as i32, Arc::new(Float32Array::from(flat)), None)
        .context("build vector column")?;
    let batch = RecordBatch::try_new(
        search_schema(dim),
        vec![Arc::new(ids) as ArrayRef, Arc::new(texts) as ArrayRef, Arc::new(vectors) as ArrayRef],
    )
    .context("build search batch")?;

    let db = lancedb::connect(uri).execute().await?;
    let version = write_versioned(&db, "search", batch).await?;

    // (Re)build the BM25 full-text index on `text`.
    let tbl = db.open_table("search").execute().await?;
    tbl.create_index(&["text"], Index::FTS(FtsIndexBuilder::default()))
        .execute()
        .await
        .context("create FTS index")?;

    // For large corpora, add an approximate vector index (IVF/PQ auto). Below this, LanceDB's
    // exact flat scan is both faster and simpler, so we skip it (IVF also needs enough rows).
    if rows.len() >= 10_000 {
        if let Err(e) = tbl.create_index(&["vector"], Index::Auto).execute().await {
            eprintln!("comind: vector ANN index skipped (non-fatal): {e}");
        }
    }
    Ok(version)
}

/// Read `(symbol_id, vector)` from the search table for incremental reuse. `Ok(None)` if absent.
pub async fn read_search_vectors(uri: &str) -> Result<Option<Vec<(GlobalSymbolId, Vec<f32>)>>> {
    let db = lancedb::connect(uri).execute().await?;
    let Ok(tbl) = db.open_table("search").execute().await else {
        return Ok(None);
    };
    let mut out = Vec::new();
    let mut st = tbl
        .query()
        .select(Select::columns(&["symbol_id", "vector"]))
        .execute()
        .await?;
    while let Some(b) = st.try_next().await? {
        let ids = str_col(&b, "symbol_id")?;
        let vecs = b
            .column_by_name("vector")
            .and_then(|c| c.as_any().downcast_ref::<FixedSizeListArray>())
            .context("vector column")?;
        for i in 0..b.num_rows() {
            let sub = vecs.value(i);
            let f = sub.as_any().downcast_ref::<Float32Array>().context("f32")?;
            out.push((id_from_rendered(ids.value(i)), f.values().to_vec()));
        }
    }
    Ok(Some(out))
}

/// Native hybrid search: BM25 full-text + vector, fused by LanceDB's RRF reranker. Returns
/// `(rendered_symbol_id, relevance_score)` best-first. `query_vector` must match the index dim.
pub async fn hybrid_search(
    uri: &str,
    query_text: &str,
    query_vector: Vec<f32>,
    k: usize,
) -> Result<Vec<(String, f32)>> {
    let db = lancedb::connect(uri).execute().await?;
    let tbl = db.open_table("search").execute().await?;
    let mut st = tbl
        .query()
        .full_text_search(FullTextSearchQuery::new(query_text.to_owned()))
        .nearest_to(query_vector)?
        .limit(k)
        // execute_hybrid fuses FTS + vector with an RRF reranker (k=60) by default.
        .execute_hybrid(QueryExecutionOptions::default())
        .await?;

    let mut out = Vec::new();
    let mut rank = 0usize;
    while let Some(b) = st.try_next().await? {
        let ids = str_col(&b, "symbol_id")?;
        // LanceDB attaches a `_relevance_score` column after reranking; fall back to rank order.
        let scores = b
            .column_by_name("_relevance_score")
            .and_then(|c| c.as_any().downcast_ref::<Float32Array>());
        for i in 0..b.num_rows() {
            let score = scores.map(|s| s.value(i)).unwrap_or(1.0 / (rank as f32 + 1.0));
            out.push((ids.value(i).to_string(), score));
            rank += 1;
        }
    }
    Ok(out)
}

/// Blocking wrappers.
pub fn write_search_table_blocking(uri: &str, rows: &[(GlobalSymbolId, String, Vec<f32>)]) -> Result<u64> {
    runtime()?.block_on(write_search_table(uri, rows))
}
pub fn read_search_vectors_blocking(uri: &str) -> Result<Option<Vec<(GlobalSymbolId, Vec<f32>)>>> {
    runtime()?.block_on(read_search_vectors(uri))
}
pub fn hybrid_search_blocking(uri: &str, text: &str, vector: Vec<f32>, k: usize) -> Result<Vec<(String, f32)>> {
    runtime()?.block_on(hybrid_search(uri, text, vector, k))
}

// ---- LLM enrichment (summaries + generated queries) -------------------------------------

fn enrichment_schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("symbol_id", DataType::Utf8, false),
        Field::new("summary", DataType::Utf8, false),
        Field::new("queries", DataType::Utf8, false), // JSON array of strings
    ]))
}

/// Persist LLM enrichment (one-line summary + generated NL queries) as a Lance `enrichment`
/// table. `queries` is a JSON array string. Returns the new version. No-op if empty.
pub async fn write_enrichment(
    uri: &str,
    rows: &[(GlobalSymbolId, String, Vec<String>)],
) -> Result<u64> {
    if rows.is_empty() {
        return Ok(0);
    }
    let ids = StringArray::from_iter_values(rows.iter().map(|(id, _, _)| id.render()));
    let summaries = StringArray::from_iter_values(rows.iter().map(|(_, s, _)| s.clone()));
    let queries = StringArray::from_iter_values(
        rows.iter().map(|(_, _, q)| serde_json::to_string(q).unwrap_or_else(|_| "[]".into())),
    );
    let batch = RecordBatch::try_new(
        enrichment_schema(),
        vec![
            Arc::new(ids) as ArrayRef,
            Arc::new(summaries) as ArrayRef,
            Arc::new(queries) as ArrayRef,
        ],
    )
    .context("build enrichment batch")?;
    let db = lancedb::connect(uri).execute().await?;
    write_versioned(&db, "enrichment", batch).await
}

/// Load persisted enrichment as `(symbol_id, summary, queries)`. `Ok(None)` if absent.
pub async fn read_enrichment(
    uri: &str,
) -> Result<Option<Vec<(GlobalSymbolId, String, Vec<String>)>>> {
    let db = lancedb::connect(uri).execute().await?;
    let Ok(tbl) = db.open_table("enrichment").execute().await else {
        return Ok(None);
    };
    let mut out = Vec::new();
    let mut st = tbl.query().execute().await?;
    while let Some(b) = st.try_next().await? {
        let ids = str_col(&b, "symbol_id")?;
        let summaries = str_col(&b, "summary")?;
        let queries = str_col(&b, "queries")?;
        for i in 0..b.num_rows() {
            let q: Vec<String> = serde_json::from_str(queries.value(i)).unwrap_or_default();
            out.push((id_from_rendered(ids.value(i)), summaries.value(i).to_string(), q));
        }
    }
    Ok(Some(out))
}

/// Blocking wrapper for [`write_enrichment`].
pub fn write_enrichment_blocking(
    uri: &str,
    rows: &[(GlobalSymbolId, String, Vec<String>)],
) -> Result<u64> {
    runtime()?.block_on(write_enrichment(uri, rows))
}

/// Blocking wrapper for [`read_enrichment`].
pub fn read_enrichment_blocking(
    uri: &str,
) -> Result<Option<Vec<(GlobalSymbolId, String, Vec<String>)>>> {
    runtime()?.block_on(read_enrichment(uri))
}

/// Persist the inferred repo style guide (single-row `style_guide` table).
pub async fn write_style_guide(uri: &str, content: &str) -> Result<()> {
    let schema = Arc::new(Schema::new(vec![Field::new("content", DataType::Utf8, false)]));
    let batch = RecordBatch::try_new(schema, vec![Arc::new(StringArray::from(vec![content])) as ArrayRef])
        .context("build style_guide batch")?;
    let db = lancedb::connect(uri).execute().await?;
    write_versioned(&db, "style_guide", batch).await?;
    Ok(())
}

/// Read the persisted style guide, if any.
pub async fn read_style_guide(uri: &str) -> Result<Option<String>> {
    let db = lancedb::connect(uri).execute().await?;
    let Ok(tbl) = db.open_table("style_guide").execute().await else {
        return Ok(None);
    };
    let mut st = tbl.query().execute().await?;
    while let Some(b) = st.try_next().await? {
        if b.num_rows() > 0 {
            return Ok(Some(str_col(&b, "content")?.value(0).to_string()));
        }
    }
    Ok(None)
}

/// Blocking wrapper for [`write_style_guide`].
pub fn write_style_guide_blocking(uri: &str, content: &str) -> Result<()> {
    runtime()?.block_on(write_style_guide(uri, content))
}

/// Blocking wrapper for [`read_style_guide`].
pub fn read_style_guide_blocking(uri: &str) -> Result<Option<String>> {
    runtime()?.block_on(read_style_guide(uri))
}

/// Blocking wrapper for [`read_embeddings`].
pub fn read_embeddings_blocking(uri: &str) -> Result<Option<Vec<(GlobalSymbolId, Vec<f32>)>>> {
    runtime()?.block_on(read_embeddings(uri))
}

fn runtime() -> Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("build tokio runtime")
}
