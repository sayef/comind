"""
DuckDB Backend for CoMind Knowledge Graph

Unified storage backend using DuckDB with:
- DuckPGQ for native graph queries (from community repository)
- VSS for vector similarity search
- FTS for full-text search
- Built-in incremental update support
"""

import hashlib
import json
from datetime import datetime
from pathlib import Path
from typing import Any

import duckdb
import numpy as np

from comind.core.graph import Relationship, RelationType, Symbol, SymbolType
from comind.logging_config import get_logger

logger = get_logger(__name__)


class DuckDBBackend:
    """DuckDB-based knowledge graph backend with incremental update support"""

    def __init__(self, db_path: str, read_only: bool = False):
        """Initialize DuckDB backend

        Args:
            db_path: Path to DuckDB database file
            read_only: If True, open in read-only mode (allows concurrent reads during writes)
        """
        self.db_path = db_path
        self.read_only = read_only
        self.conn = None
        self._ensure_database()

    def _ensure_database(self):
        """Ensure database exists and is properly initialized"""
        Path(self.db_path).parent.mkdir(parents=True, exist_ok=True)

        # Connect to database
        if self.read_only:
            self.conn = duckdb.connect(self.db_path, read_only=True)
            logger.debug(f"Connected to DuckDB in read-only mode: {self.db_path}")
        else:
            self.conn = duckdb.connect(self.db_path)
            logger.debug(f"Connected to DuckDB in read-write mode: {self.db_path}")

        # Install required extensions
        self._install_extensions()

        # Load schema (skip in read-only mode)
        if not self.read_only:
            self._load_schema()

        logger.debug(f"DuckDB backend initialized at {self.db_path}")

    def _install_extensions(self):
        """Install and load required DuckDB extensions"""
        # DuckPGQ for native graph queries
        try:
            self.conn.execute("INSTALL duckpgq FROM community")
            self.conn.execute("LOAD duckpgq")
            logger.debug("DuckPGQ extension loaded")
        except Exception as e:
            logger.debug(f"DuckPGQ not loaded (using SQL fallbacks): {e}")

        # VSS for vector similarity search
        try:
            self.conn.execute("INSTALL vss")
            self.conn.execute("LOAD vss")
            logger.debug("VSS extension loaded")
        except Exception as e:
            logger.debug(f"VSS not loaded: {e}")

        # FTS for full-text search
        try:
            self.conn.execute("INSTALL fts")
            self.conn.execute("LOAD fts")
            logger.debug("FTS extension loaded")
        except Exception as e:
            logger.debug(f"FTS not loaded: {e}")

    def _load_schema(self):
        """Load and execute schema from SQL file"""
        schema_file = Path(__file__).parent / "duckdb_schema.sql"

        if not schema_file.exists():
            logger.error(f"Schema file not found: {schema_file}")
            return

        try:
            with open(schema_file) as f:
                schema_sql = f.read()

            # Split into individual statements and execute
            # DuckDB doesn't support all multi-statement execution
            statements = [s.strip() for s in schema_sql.split(";") if s.strip()]

            for statement in statements:
                if statement:
                    try:
                        self.conn.execute(statement)
                    except Exception as e:
                        # Skip errors for already existing objects or unsupported features
                        if (
                            "already exists" not in str(e).lower()
                            and "does not exist" not in str(e).lower()
                        ):
                            logger.warning(f"Schema statement warning: {e}")

            logger.debug("DuckDB schema loaded successfully")
        except Exception as e:
            logger.error(f"Failed to load schema: {e}")
            raise

        logger.debug("Database schema loaded")
        self._create_fts_index()
        self._create_vss_indexes()

    def _create_fts_index(self):
        """Create FTS index on symbols table (idempotent)."""
        try:
            self.conn.execute(
                "PRAGMA create_fts_index('symbols', 'id', 'name', 'signature', 'docstring', 'description', overwrite=1)"
            )
            logger.debug("FTS index created/refreshed on symbols")
        except Exception as e:
            logger.debug(f"FTS index creation skipped (extension unavailable?): {e}")

    def _create_vss_indexes(self):
        """Create VSS HNSW indexes for vector similarity search (idempotent)."""
        try:
            # Index for symbol embeddings
            self.conn.execute("""
                CREATE INDEX IF NOT EXISTS idx_symbol_embeddings_hnsw 
                ON symbol_embeddings 
                USING HNSW (embedding)
            """)
            logger.debug("VSS HNSW index created on symbol_embeddings")
        except Exception as e:
            logger.debug(f"Symbol embeddings VSS index creation skipped: {e}")

        try:
            # Index for process query embeddings
            self.conn.execute("""
                CREATE INDEX IF NOT EXISTS idx_process_queries_hnsw 
                ON process_queries 
                USING HNSW (embedding)
            """)
            logger.debug("VSS HNSW index created on process_queries")
        except Exception as e:
            logger.debug(f"Process queries VSS index creation skipped: {e}")

    # =========================================================================
    # Symbol Operations
    # =========================================================================

    async def add_symbol(self, symbol: Symbol) -> None:
        """Add or update a symbol.

        Cleans up dependent rows (relationships, embeddings, etc.) before
        replacing so DuckDB foreign-key constraints are satisfied.
        """
        self._cascade_delete_symbol_deps([symbol.id])
        self.conn.execute(
            """
            INSERT OR REPLACE INTO symbols (
                id, name, type, file_path, line_start, line_end,
                repo_id, signature, docstring, description,
                properties, associated_queries, wiki_page_id, wiki_section
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        """,
            (
                symbol.id,
                symbol.name,
                symbol.type.value,
                symbol.file_path,
                symbol.line_start,
                symbol.line_end,
                symbol.repo_id,
                symbol.signature,
                symbol.docstring,
                symbol.description,
                json.dumps(symbol.properties) if symbol.properties else None,
                json.dumps(symbol.associated_queries) if symbol.associated_queries else None,
                symbol.wiki_page_id,
                symbol.wiki_section,
            ),
        )
        self._create_fts_index()

    async def get_symbol(self, symbol_id: str) -> Symbol | None:
        """Get a symbol by ID"""
        result = self.conn.execute(
            """
            SELECT * FROM symbols WHERE id = ?
        """,
            (symbol_id,),
        ).fetchone()

        if not result:
            return None

        return self._row_to_symbol(result)

    async def get_symbols_by_file(self, file_path: str) -> list[Symbol]:
        """Get all symbols in a file"""
        result = self.conn.execute(
            """
            SELECT * FROM symbols WHERE file_path = ?
        """,
            (file_path,),
        ).fetchall()

        return [self._row_to_symbol(row) for row in result]

    async def delete_symbols_by_file(self, file_path: str) -> int:
        """Delete all symbols in a file, returns count deleted"""
        result = self.conn.execute(
            """
            DELETE FROM symbols WHERE file_path = ?
        """,
            (file_path,),
        )

        return result.fetchone()[0] if result else 0

    def _row_to_symbol(self, row: tuple) -> Symbol:
        """Convert database row to Symbol object"""
        return Symbol(
            id=row[0],
            name=row[1],
            type=SymbolType(row[2]),
            file_path=row[3],
            line_start=row[4],
            line_end=row[5],
            repo_id=row[6],
            signature=row[7],
            docstring=row[8],
            description=row[9],
            properties=json.loads(row[10]) if row[10] else {},
            associated_queries=json.loads(row[11]) if row[11] else [],
            wiki_page_id=row[12],
            wiki_section=row[13],
        )

    # =========================================================================
    # Cascade helpers
    # =========================================================================

    def _cascade_delete_symbol_deps(self, symbol_ids: list[str]) -> None:
        """Delete rows in child tables that reference the given symbol IDs.

        DuckDB's INSERT OR REPLACE internally does DELETE + INSERT, which
        fails when foreign-key constraints exist.  We pre-delete the
        dependent rows so the REPLACE can proceed.
        """
        if not symbol_ids:
            return

        # Find which of these IDs actually exist in the DB already
        placeholders = ",".join("?" * len(symbol_ids))
        existing = {
            row[0]
            for row in self.conn.execute(
                f"SELECT id FROM symbols WHERE id IN ({placeholders})",
                tuple(symbol_ids),
            ).fetchall()
        }
        if not existing:
            return

        ph = ",".join("?" * len(existing))
        ids = tuple(existing)

        # Order matters: delete from leaf tables first
        self.conn.execute(f"DELETE FROM file_symbols WHERE symbol_id IN ({ph})", ids)
        self.conn.execute(f"DELETE FROM symbol_embeddings WHERE symbol_id IN ({ph})", ids)
        self.conn.execute(f"DELETE FROM llm_cache WHERE symbol_id IN ({ph})", ids)
        self.conn.execute(
            f"DELETE FROM relationships WHERE source_id IN ({ph}) OR target_id IN ({ph})",
            ids + ids,
        )

    # =========================================================================
    # Relationship Operations
    # =========================================================================

    async def add_symbols_batch(self, symbols: list[Symbol]) -> None:
        """Bulk insert symbols - much faster than individual inserts.

        Cleans up dependent rows first so INSERT OR REPLACE doesn't
        violate foreign-key constraints.
        """
        if not symbols:
            return

        # Remove FK dependents for symbols that already exist
        self._cascade_delete_symbol_deps([s.id for s in symbols])

        # Prepare batch data
        data = [
            (
                s.id,
                s.name,
                s.type.value,
                s.file_path,
                s.line_start,
                s.line_end,
                s.repo_id,
                s.signature,
                s.docstring,
                s.description,
                json.dumps(s.properties) if s.properties else None,
                json.dumps(s.associated_queries) if s.associated_queries else None,
                s.wiki_page_id,
                s.wiki_section,
            )
            for s in symbols
        ]

        # Bulk insert
        self.conn.executemany(
            """
            INSERT OR REPLACE INTO symbols (
                id, name, type, file_path, line_start, line_end, repo_id,
                signature, docstring, description, properties, associated_queries,
                wiki_page_id, wiki_section
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        """,
            data,
        )
        # Refresh FTS index so new symbols are searchable immediately
        self._create_fts_index()

    async def add_relationships_batch(self, relationships: list[Relationship]) -> None:
        """Bulk insert relationships - much faster than individual inserts"""
        if not relationships:
            return

        # Get all unique symbol IDs we need to check
        symbol_ids = set()
        for rel in relationships:
            symbol_ids.add(rel.source_id)
            symbol_ids.add(rel.target_id)

        # Check which symbols exist in one query
        placeholders = ",".join("?" * len(symbol_ids))
        existing_ids = {
            row[0]
            for row in self.conn.execute(
                f"SELECT id FROM symbols WHERE id IN ({placeholders})", tuple(symbol_ids)
            ).fetchall()
        }

        # Filter to only relationships where both symbols exist
        valid_relationships = [
            rel
            for rel in relationships
            if rel.source_id in existing_ids and rel.target_id in existing_ids
        ]

        if not valid_relationships:
            return

        # Prepare batch data
        data = [
            (
                rel.source_id,
                rel.target_id,
                rel.type.value,
                rel.confidence,
                json.dumps(rel.properties) if rel.properties else None,
            )
            for rel in valid_relationships
        ]

        # Bulk insert
        self.conn.executemany(
            """
            INSERT OR REPLACE INTO relationships (
                source_id, target_id, type, confidence, properties
            ) VALUES (?, ?, ?, ?, ?)
        """,
            data,
        )

    async def add_relationship(self, relationship: Relationship) -> None:
        """Add a relationship"""
        # Check if both source and target symbols exist to avoid foreign key violations
        source_exists = self.conn.execute(
            "SELECT 1 FROM symbols WHERE id = ?", (relationship.source_id,)
        ).fetchone()
        target_exists = self.conn.execute(
            "SELECT 1 FROM symbols WHERE id = ?", (relationship.target_id,)
        ).fetchone()

        if not source_exists:
            logger.warning(
                "Skipping relationship: source symbol not found", source_id=relationship.source_id
            )
            return
        if not target_exists:
            logger.debug(
                "Skipping relationship: target symbol not found (likely external/builtin)",
                target_id=relationship.target_id,
                type=relationship.type.value,
            )
            return

        self.conn.execute(
            """
            INSERT INTO relationships (
                source_id, target_id, type, confidence, properties
            ) VALUES (?, ?, ?, ?, ?)
        """,
            (
                relationship.source_id,
                relationship.target_id,
                relationship.type.value,
                relationship.confidence,
                json.dumps(relationship.properties) if relationship.properties else None,
            ),
        )

    async def get_relationships(
        self, symbol_id: str, relation_type: RelationType | None = None, direction: str = "outgoing"
    ) -> list[Relationship]:
        """Get relationships for a symbol"""
        if direction == "outgoing":
            query = "SELECT * FROM relationships WHERE source_id = ?"
        else:
            query = "SELECT * FROM relationships WHERE target_id = ?"

        if relation_type:
            query += f" AND type = '{relation_type.value}'"

        result = self.conn.execute(query, (symbol_id,)).fetchall()

        return [self._row_to_relationship(row) for row in result]

    def _row_to_relationship(self, row: tuple) -> Relationship:
        """Convert database row to Relationship object"""
        return Relationship(
            source_id=row[1],
            target_id=row[2],
            type=RelationType(row[3]),
            confidence=row[4],
            properties=json.loads(row[5]) if row[5] else {},
        )

    # =========================================================================
    # Graph Queries (GraphPQG)
    # =========================================================================

    async def get_callers(self, symbol_id: str, repo_id: str | None = None) -> list[Symbol]:
        """Get all symbols that call this symbol"""
        # Use SQL-based graph traversal (DuckPGQ requires property graph setup)
        if repo_id:
            result = self.conn.execute(
                """
                SELECT s.* FROM symbols s
                JOIN relationships r ON s.id = r.source_id
                WHERE r.target_id = ? AND r.type = 'calls' AND s.repo_id = ?
            """,
                (symbol_id, repo_id),
            ).fetchall()
        else:
            result = self.conn.execute(
                """
                SELECT s.* FROM symbols s
                JOIN relationships r ON s.id = r.source_id
                WHERE r.target_id = ? AND r.type = 'calls'
            """,
                (symbol_id,),
            ).fetchall()

        return [self._row_to_symbol(row) for row in result]

    async def get_callees(self, symbol_id: str, repo_id: str | None = None) -> list[Symbol]:
        """Get all symbols this symbol calls"""
        # Use SQL-based graph traversal (DuckPGQ requires property graph setup)
        if repo_id:
            result = self.conn.execute(
                """
                SELECT s.* FROM symbols s
                JOIN relationships r ON s.id = r.target_id
                WHERE r.source_id = ? AND r.type = 'calls' AND s.repo_id = ?
            """,
                (symbol_id, repo_id),
            ).fetchall()
        else:
            result = self.conn.execute(
                """
                SELECT s.* FROM symbols s
                JOIN relationships r ON s.id = r.target_id
                WHERE r.source_id = ? AND r.type = 'calls'
            """,
                (symbol_id,),
            ).fetchall()

        return [self._row_to_symbol(row) for row in result]

    async def trace_execution(self, entry_point: str, max_depth: int = 10) -> list[dict[str, Any]]:
        """Trace execution flow from entry point"""
        # TODO: Implement recursive CTE for execution tracing
        # For now, return empty (DuckPGQ property graph not set up)
        return []

    # =========================================================================
    # File Metadata & Incremental Updates
    # =========================================================================

    async def update_file_metadata(
        self, file_path: str, repo_id: str, file_hash: str, mtime: int, size: int, symbol_count: int
    ) -> None:
        """Update file metadata for change tracking.

        Deletes dependent file_symbols rows first so INSERT OR REPLACE
        doesn't violate foreign-key constraints.
        """
        # Clean up FK dependents
        self.conn.execute("DELETE FROM file_symbols WHERE file_path = ?", (file_path,))
        self.conn.execute(
            """
            INSERT OR REPLACE INTO file_metadata (
                file_path, repo_id, hash, mtime, size, 
                symbol_count, last_indexed, needs_reindex
            ) VALUES (?, ?, ?, ?, ?, ?, ?, FALSE)
        """,
            (file_path, repo_id, file_hash, mtime, size, symbol_count, datetime.now()),
        )

    async def get_file_metadata(self, file_path: str) -> dict[str, Any] | None:
        """Get metadata for a file"""
        result = self.conn.execute(
            """
            SELECT * FROM file_metadata WHERE file_path = ?
        """,
            (file_path,),
        ).fetchone()

        if not result:
            return None

        return {
            "file_path": result[0],
            "repo_id": result[1],
            "hash": result[2],
            "mtime": result[3],
            "size": result[4],
            "symbol_count": result[5],
            "last_indexed": result[6],
            "needs_reindex": result[7],
        }

    async def get_changed_files(self, repo_id: str) -> list[str]:
        """Get list of files that need reindexing"""
        result = self.conn.execute(
            """
            SELECT file_path FROM file_metadata
            WHERE repo_id = ? AND needs_reindex = TRUE
        """,
            (repo_id,),
        ).fetchall()

        return [row[0] for row in result]

    async def mark_files_for_reindex(self, file_paths: list[str]) -> None:
        """Mark files as needing reindex"""
        for file_path in file_paths:
            self.conn.execute(
                """
                UPDATE file_metadata 
                SET needs_reindex = TRUE 
                WHERE file_path = ?
            """,
                (file_path,),
            )

    def compute_file_hash(self, file_path: str) -> str:
        """Compute SHA256 hash of file content"""
        try:
            with open(file_path, "rb") as f:
                return hashlib.sha256(f.read()).hexdigest()
        except Exception as e:
            logger.error(f"Failed to hash file {file_path}: {e}")
            return ""

    async def detect_changed_files(
        self, repo_id: str, current_files: dict[str, str]
    ) -> dict[str, str]:
        """Detect which files have changed since last index

        Args:
            repo_id: Repository ID
            current_files: Dict of file_path -> current_hash

        Returns:
            Dict of changed file_path -> status ('new', 'modified', 'deleted')
        """
        changed = {}

        # Get existing file metadata
        result = self.conn.execute(
            """
            SELECT file_path, hash FROM file_metadata
            WHERE repo_id = ?
        """,
            (repo_id,),
        ).fetchall()

        existing_files = {row[0]: row[1] for row in result}

        # Check for new and modified files
        for file_path, current_hash in current_files.items():
            if file_path not in existing_files:
                changed[file_path] = "new"
            elif existing_files[file_path] != current_hash:
                changed[file_path] = "modified"

        # Check for deleted files
        for file_path in existing_files:
            if file_path not in current_files:
                changed[file_path] = "deleted"

        return changed

    # =========================================================================
    # LLM Cache
    # =========================================================================

    async def get_llm_cache(self, cache_key: str, content_hash: str) -> str | None:
        """Get cached LLM output if hash matches"""
        result = self.conn.execute(
            """
            SELECT content FROM llm_cache
            WHERE cache_key = ? AND content_hash = ?
        """,
            (cache_key, content_hash),
        ).fetchone()

        if result:
            # Update access tracking
            self.conn.execute(
                """
                UPDATE llm_cache 
                SET access_count = access_count + 1,
                    last_accessed = CURRENT_TIMESTAMP
                WHERE cache_key = ?
            """,
                (cache_key,),
            )

            return result[0]

        return None

    async def save_llm_cache(
        self,
        cache_key: str,
        symbol_id: str,
        content_hash: str,
        cache_type: str,
        content: str,
        model: str,
        metadata: dict | None = None,
    ) -> None:
        """Save LLM output to cache"""
        self.conn.execute(
            """
            INSERT OR REPLACE INTO llm_cache (
                cache_key, symbol_id, content_hash, cache_type,
                content, metadata, model
            ) VALUES (?, ?, ?, ?, ?, ?, ?)
        """,
            (
                cache_key,
                symbol_id,
                content_hash,
                cache_type,
                content,
                json.dumps(metadata) if metadata else None,
                model,
            ),
        )

    # =========================================================================
    # Process Query Search
    # =========================================================================

    async def search_process_queries(
        self, query_embedding: np.ndarray, repo_id: str | None = None, limit: int = 10
    ) -> list[dict[str, Any]]:
        """Search process queries using semantic similarity"""
        embedding_list = query_embedding.tolist()

        # Build query with optional repo filter
        sql = """
            SELECT 
                pq.process_id,
                pq.query,
                p.name,
                p.entry_point,
                p.steps,
                p.priority,
                p.repo_id,
                array_cosine_similarity(pq.embedding, ?::FLOAT[384]) as similarity
            FROM process_queries pq
            JOIN processes p ON pq.process_id = p.process_id
        """

        params = [embedding_list]

        if repo_id:
            sql += " WHERE p.repo_id = ?"
            params.append(repo_id)

        sql += " ORDER BY similarity DESC LIMIT ?"
        params.append(limit)

        try:
            results = self.conn.execute(sql, params).fetchall()

            return [
                {
                    "process_id": row[0],
                    "matched_query": row[1],
                    "process_name": row[2],
                    "entry_point": row[3],
                    "steps": json.loads(row[4]) if row[4] else [],
                    "priority": row[5],
                    "repo_id": row[6],
                    "similarity": float(row[7]),
                }
                for row in results
            ]
        except Exception as e:
            logger.warning(f"Process query search failed (VSS not available?): {e}")
            # Fallback to text search
            return await self._search_process_queries_fallback(query_embedding, repo_id, limit)

    async def _search_process_queries_fallback(
        self, query_embedding: np.ndarray, repo_id: str | None = None, limit: int = 10
    ) -> list[dict[str, Any]]:
        """Fallback process search using manual similarity calculation"""
        sql = """
            SELECT 
                pq.process_id,
                pq.query,
                pq.embedding,
                p.name,
                p.entry_point,
                p.steps,
                p.priority,
                p.repo_id
            FROM process_queries pq
            JOIN processes p ON pq.process_id = p.process_id
        """

        if repo_id:
            sql += " WHERE p.repo_id = ?"
            results = self.conn.execute(sql, (repo_id,)).fetchall()
        else:
            results = self.conn.execute(sql).fetchall()

        # Calculate similarities manually
        scored_results = []
        for row in results:
            stored_embedding = np.array(row[2])
            similarity = np.dot(query_embedding, stored_embedding) / (
                np.linalg.norm(query_embedding) * np.linalg.norm(stored_embedding)
            )
            scored_results.append(
                {
                    "process_id": row[0],
                    "matched_query": row[1],
                    "process_name": row[3],
                    "entry_point": row[4],
                    "steps": json.loads(row[5]) if row[5] else [],
                    "priority": row[6],
                    "repo_id": row[7],
                    "similarity": float(similarity),
                }
            )

        # Sort by similarity and limit
        scored_results.sort(key=lambda x: x["similarity"], reverse=True)
        return scored_results[:limit]

    # =========================================================================
    # Vector Search
    # =========================================================================

    async def add_embedding(
        self, symbol_id: str, embedding: np.ndarray, model: str = "all-MiniLM-L6-v2"
    ) -> None:
        """Add vector embedding for a symbol"""
        self.conn.execute(
            """
            INSERT OR REPLACE INTO symbol_embeddings (
                symbol_id, embedding, model
            ) VALUES (?, ?, ?)
        """,
            (symbol_id, embedding.tolist(), model),
        )

    async def semantic_search(
        self, query_embedding: np.ndarray, limit: int = 10, min_similarity: float = 0.0
    ) -> list[tuple[Symbol, float]]:
        """Search symbols by vector similarity"""
        try:
            result = self.conn.execute(
                """
                SELECT s.*, array_cosine_similarity(e.embedding, ?::FLOAT[384]) as similarity
                FROM symbols s
                JOIN symbol_embeddings e ON s.id = e.symbol_id
                WHERE similarity >= ?
                ORDER BY similarity DESC
                LIMIT ?
            """,
                (query_embedding.tolist(), min_similarity, limit),
            ).fetchall()

            return [(self._row_to_symbol(row[:-1]), row[-1]) for row in result]
        except Exception as e:
            logger.error(f"Vector search failed: {e}")
            return []

    # =========================================================================
    # Text Search
    # =========================================================================

    async def text_search(self, query: str, limit: int = 10) -> list[tuple[Symbol, float]]:
        """Full-text search using DuckDB FTS or fallback to LIKE"""
        try:
            # Try FTS first (DuckDB FTS: column arg is the id column name, unquoted)
            result = self.conn.execute(
                """
                SELECT s.*, fts_main_symbols.match_bm25(id, ?) as score
                FROM symbols s
                WHERE score IS NOT NULL
                ORDER BY score DESC
                LIMIT ?
            """,
                (query, limit),
            ).fetchall()

            return [(self._row_to_symbol(row[:-1]), row[-1]) for row in result]
        except Exception:
            # Fallback to simple LIKE search
            try:
                search_pattern = f"%{query}%"
                result = self.conn.execute(
                    """
                    SELECT *, 
                        CASE 
                            WHEN name LIKE ? THEN 3.0
                            WHEN signature LIKE ? THEN 2.0
                            WHEN docstring LIKE ? THEN 1.0
                            ELSE 0.5
                        END as score
                    FROM symbols
                    WHERE name LIKE ? 
                        OR signature LIKE ? 
                        OR docstring LIKE ?
                        OR description LIKE ?
                    ORDER BY score DESC, name
                    LIMIT ?
                """,
                    (
                        search_pattern,
                        search_pattern,
                        search_pattern,
                        search_pattern,
                        search_pattern,
                        search_pattern,
                        search_pattern,
                        limit,
                    ),
                ).fetchall()

                return [(self._row_to_symbol(row[:-1]), row[-1]) for row in result]
            except Exception as e:
                logger.error(f"Text search failed: {e}")
                return []

    # =========================================================================
    # Repository Management
    # =========================================================================

    async def register_repository(
        self,
        repo_id: str,
        name: str,
        path: str,
        branch: str = "main",
        last_commit: str | None = None,
        metadata: dict | None = None,
    ) -> None:
        """Register a repository.

        Cleans up dependent process_queries/processes rows first so
        INSERT OR REPLACE doesn't violate foreign-key constraints.
        """
        # Clean up FK chain: process_queries → processes → repositories
        existing = self.conn.execute(
            "SELECT repo_id FROM repositories WHERE repo_id = ?", (repo_id,)
        ).fetchone()
        if existing:
            self.conn.execute(
                "DELETE FROM process_queries WHERE process_id IN "
                "(SELECT process_id FROM processes WHERE repo_id = ?)",
                (repo_id,),
            )
            self.conn.execute("DELETE FROM processes WHERE repo_id = ?", (repo_id,))

        self.conn.execute(
            """
            INSERT OR REPLACE INTO repositories (
                repo_id, name, path, branch, last_commit, indexed_at, metadata
            ) VALUES (?, ?, ?, ?, ?, ?, ?)
        """,
            (
                repo_id,
                name,
                path,
                branch,
                last_commit,
                datetime.now(),
                json.dumps(metadata) if metadata else None,
            ),
        )

    async def get_repository_stats(self, repo_id: str) -> dict[str, Any] | None:
        """Get statistics for a repository"""
        result = self.conn.execute(
            """
            SELECT 
                r.repo_id,
                r.name,
                r.indexed_at,
                COUNT(DISTINCT s.id) as symbol_count,
                COUNT(DISTINCT s.file_path) as file_count,
                COUNT(DISTINCT rel.id) as relationship_count,
                COUNT(DISTINCT wp.page_id) as wiki_page_count
            FROM repositories r
            LEFT JOIN symbols s ON r.repo_id = s.repo_id
            LEFT JOIN relationships rel ON s.id = rel.source_id
            LEFT JOIN wiki_pages wp ON r.repo_id = wp.repo_id
            WHERE r.repo_id = ?
            GROUP BY r.repo_id, r.name, r.indexed_at
        """,
            (repo_id,),
        ).fetchone()

        if not result:
            return None

        return {
            "repo_id": result[0],
            "name": result[1],
            "indexed_at": result[2],
            "symbol_count": result[3],
            "file_count": result[4],
            "relationship_count": result[5],
            "wiki_page_count": result[6],
        }

    # =========================================================================
    # Cleanup
    # =========================================================================

    def close(self):
        """Close database connection"""
        if self.conn:
            self.conn.close()
            logger.info("DuckDB connection closed")

    def __del__(self):
        """Cleanup on deletion — guard against interpreter shutdown"""
        try:
            self.close()
        except Exception:
            pass
