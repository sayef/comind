-- ============================================================================
-- CoMind DuckDB Schema
-- Unified schema for all CoMind data: symbols, relationships, embeddings,
-- metadata, and caching
-- ============================================================================

-- ============================================================================
-- Core Tables
-- ============================================================================

-- Symbols: All code entities (functions, classes, methods, etc.)
CREATE TABLE IF NOT EXISTS symbols (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    type TEXT NOT NULL,
    file_path TEXT NOT NULL,
    line_start INTEGER NOT NULL,
    line_end INTEGER NOT NULL,
    repo_id TEXT,
    signature TEXT,
    docstring TEXT,
    description TEXT,
    properties JSON,
    associated_queries JSON,
    wiki_page_id TEXT,
    wiki_section TEXT,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Relationships: Connections between symbols (calls, imports, inherits, etc.)
CREATE SEQUENCE IF NOT EXISTS seq_relationships_id START 1;
CREATE TABLE IF NOT EXISTS relationships (
    id INTEGER PRIMARY KEY DEFAULT nextval('seq_relationships_id'),
    source_id TEXT NOT NULL,
    target_id TEXT NOT NULL,
    type TEXT NOT NULL,
    confidence REAL DEFAULT 1.0,
    properties JSON,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (source_id) REFERENCES symbols(id),
    FOREIGN KEY (target_id) REFERENCES symbols(id)
);

-- ============================================================================
-- Search & Embeddings
-- ============================================================================

-- Symbol embeddings for semantic search
CREATE TABLE IF NOT EXISTS symbol_embeddings (
    symbol_id TEXT PRIMARY KEY,
    embedding FLOAT[384],
    model TEXT DEFAULT 'all-MiniLM-L6-v2',
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (symbol_id) REFERENCES symbols(id)
);

-- ============================================================================
-- Incremental Update Support
-- ============================================================================

-- File metadata for change detection and incremental updates
CREATE TABLE IF NOT EXISTS file_metadata (
    file_path TEXT PRIMARY KEY,
    repo_id TEXT NOT NULL,
    hash TEXT NOT NULL,
    mtime INTEGER NOT NULL,
    size INTEGER NOT NULL,
    symbol_count INTEGER DEFAULT 0,
    last_indexed TIMESTAMP NOT NULL,
    needs_reindex BOOLEAN DEFAULT FALSE,
    index_version INTEGER DEFAULT 1
);

-- Track which symbols belong to which files for efficient updates
CREATE TABLE IF NOT EXISTS file_symbols (
    file_path TEXT NOT NULL,
    symbol_id TEXT NOT NULL,
    PRIMARY KEY (file_path, symbol_id),
    FOREIGN KEY (file_path) REFERENCES file_metadata(file_path),
    FOREIGN KEY (symbol_id) REFERENCES symbols(id)
);

-- ============================================================================
-- LLM Cache (Cost Optimization)
-- ============================================================================

-- Cache LLM-generated content to avoid redundant API calls
CREATE TABLE IF NOT EXISTS llm_cache (
    cache_key TEXT PRIMARY KEY,
    symbol_id TEXT,
    content_hash TEXT NOT NULL,
    cache_type TEXT NOT NULL,
    content TEXT NOT NULL,
    metadata JSON,
    model TEXT,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    last_accessed TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    access_count INTEGER DEFAULT 0,
    FOREIGN KEY (symbol_id) REFERENCES symbols(id)
);

-- ============================================================================
-- Wiki & Documentation
-- ============================================================================

-- Wiki pages generated for modules/packages
CREATE TABLE IF NOT EXISTS wiki_pages (
    page_id TEXT PRIMARY KEY,
    repo_id TEXT NOT NULL,
    title TEXT NOT NULL,
    content TEXT NOT NULL,
    module_path TEXT,
    symbols JSON,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Style guides extracted from repositories
CREATE TABLE IF NOT EXISTS style_guides (
    repo_id TEXT PRIMARY KEY,
    content JSON NOT NULL,
    version INTEGER DEFAULT 1,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- ============================================================================
-- Repository Management
-- ============================================================================

-- Repository metadata
CREATE TABLE IF NOT EXISTS repositories (
    repo_id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    path TEXT NOT NULL,
    branch TEXT DEFAULT 'main',
    last_commit TEXT,
    indexed_at TIMESTAMP NOT NULL,
    symbol_count INTEGER DEFAULT 0,
    file_count INTEGER DEFAULT 0,
    metadata JSON
);

-- ============================================================================
-- Process Detection
-- ============================================================================

-- Detected execution flows/processes
CREATE TABLE IF NOT EXISTS processes (
    process_id TEXT PRIMARY KEY,
    repo_id TEXT NOT NULL,
    name TEXT NOT NULL,
    label TEXT,
    type TEXT,
    entry_point TEXT NOT NULL,
    steps JSON NOT NULL,
    priority INTEGER DEFAULT 0,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (repo_id) REFERENCES repositories(repo_id)
);

-- Process queries for semantic search
CREATE SEQUENCE IF NOT EXISTS seq_process_queries_id START 1;
CREATE TABLE IF NOT EXISTS process_queries (
    id INTEGER PRIMARY KEY DEFAULT nextval('seq_process_queries_id'),
    process_id TEXT NOT NULL,
    query TEXT NOT NULL,
    embedding FLOAT[384],
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (process_id) REFERENCES processes(process_id)
);

-- ============================================================================
-- Indexes for Performance
-- ============================================================================

-- Symbol indexes
CREATE INDEX IF NOT EXISTS idx_symbols_file ON symbols(file_path);
CREATE INDEX IF NOT EXISTS idx_symbols_type ON symbols(type);
CREATE INDEX IF NOT EXISTS idx_symbols_repo ON symbols(repo_id);
CREATE INDEX IF NOT EXISTS idx_symbols_name ON symbols(name);

-- Relationship indexes
CREATE INDEX IF NOT EXISTS idx_relationships_source ON relationships(source_id);
CREATE INDEX IF NOT EXISTS idx_relationships_target ON relationships(target_id);
CREATE INDEX IF NOT EXISTS idx_relationships_type ON relationships(type);

-- File metadata indexes
CREATE INDEX IF NOT EXISTS idx_file_metadata_repo ON file_metadata(repo_id);
CREATE INDEX IF NOT EXISTS idx_file_metadata_hash ON file_metadata(hash);
CREATE INDEX IF NOT EXISTS idx_file_metadata_needs_reindex ON file_metadata(needs_reindex);

-- LLM cache indexes
CREATE INDEX IF NOT EXISTS idx_llm_cache_symbol ON llm_cache(symbol_id);
CREATE INDEX IF NOT EXISTS idx_llm_cache_type ON llm_cache(cache_type);
CREATE INDEX IF NOT EXISTS idx_llm_cache_hash ON llm_cache(content_hash);

-- Wiki indexes
CREATE INDEX IF NOT EXISTS idx_wiki_pages_repo ON wiki_pages(repo_id);

-- Process query indexes
CREATE INDEX IF NOT EXISTS idx_process_queries_process ON process_queries(process_id);
CREATE INDEX IF NOT EXISTS idx_process_queries_query ON process_queries(query);

-- ============================================================================
-- Note: Advanced features like PROPERTY GRAPH, FTS, and VSS are handled
-- programmatically in the backend code when extensions are available
-- ============================================================================

-- ============================================================================
-- Views for Common Queries
-- ============================================================================

-- View: Symbols with their file metadata
CREATE OR REPLACE VIEW symbols_with_metadata AS
SELECT 
    s.*,
    fm.hash as file_hash,
    fm.mtime as file_mtime,
    fm.last_indexed as file_last_indexed
FROM symbols s
LEFT JOIN file_metadata fm ON s.file_path = fm.file_path;

-- View: Symbols with caller/callee counts
CREATE OR REPLACE VIEW symbol_stats AS
SELECT 
    s.id,
    s.name,
    s.type,
    s.file_path,
    COUNT(DISTINCT r_in.source_id) as caller_count,
    COUNT(DISTINCT r_out.target_id) as callee_count
FROM symbols s
LEFT JOIN relationships r_in ON s.id = r_in.target_id AND r_in.type = 'calls'
LEFT JOIN relationships r_out ON s.id = r_out.source_id AND r_out.type = 'calls'
GROUP BY s.id, s.name, s.type, s.file_path;

-- View: Files needing reindex
CREATE OR REPLACE VIEW files_needing_reindex AS
SELECT 
    file_path,
    repo_id,
    hash,
    last_indexed,
    symbol_count
FROM file_metadata
WHERE needs_reindex = TRUE
ORDER BY last_indexed ASC;

-- View: LLM cache statistics
CREATE OR REPLACE VIEW llm_cache_stats AS
SELECT 
    cache_type,
    COUNT(*) as total_entries,
    SUM(access_count) as total_accesses,
    AVG(access_count) as avg_accesses_per_entry,
    MAX(last_accessed) as most_recent_access
FROM llm_cache
GROUP BY cache_type;
