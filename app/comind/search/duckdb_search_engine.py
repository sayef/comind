"""
DuckDB-based search engines for text and semantic search

Replaces separate BM25S and FastEmbed indexes with DuckDB FTS and VSS.
"""

import numpy as np

from comind.core.graph import Symbol
from comind.logging_config import get_logger

logger = get_logger(__name__)


class DuckDBTextSearchEngine:
    """Text search using DuckDB FTS instead of BM25S"""

    def __init__(self, backend):
        """Initialize with DuckDB backend

        Args:
            backend: DuckDBBackend or GraphAdapter instance
        """
        self.backend = backend
        # Get the actual DuckDB backend if wrapped in adapter
        if hasattr(backend, "backend"):
            self.db_backend = backend.backend
        else:
            self.db_backend = backend

    async def add_symbol(self, symbol: Symbol, content: str = ""):
        """Add symbol to search index (handled by DuckDB automatically)"""
        # Symbols are already in DuckDB, FTS index is automatic

    async def build_index(self):
        """Build search index (handled by DuckDB automatically)"""
        # FTS index is created automatically by schema

    async def save(self, index_dir: str):
        """Save index (handled by DuckDB automatically)"""
        # DuckDB persists automatically

    async def load(self, index_dir: str) -> bool:
        """Load index (handled by DuckDB automatically)"""
        # DuckDB loads automatically
        return True

    async def search(self, query: str, limit: int = 10) -> list[tuple[str, float]]:
        """Search for symbols using DuckDB FTS"""
        try:
            results = await self.db_backend.text_search(query, limit=limit)
            # Convert to (symbol_id, score) tuples
            return [(symbol.id, score) for symbol, score in results]
        except Exception:
            logger.exception("DuckDB text search failed")
            return []


class DuckDBSemanticSearchEngine:
    """Semantic search using DuckDB VSS instead of separate FastEmbed index"""

    def __init__(self, backend, embedding_model: str | None = None):
        """Initialize with DuckDB backend

        Args:
            backend: DuckDBBackend or GraphAdapter instance
            embedding_model: Model name (default: BAAI/bge-small-en-v1.5)
        """
        self.backend = backend
        # Get the actual DuckDB backend if wrapped in adapter
        if hasattr(backend, "backend"):
            self.db_backend = backend.backend
        else:
            self.db_backend = backend

        # Use BAAI/bge-small-en-v1.5 (67MB, 384 dim) - fast and lightweight
        self.embedding_model = embedding_model or "BAAI/bge-small-en-v1.5"
        self.embedder = None
        self._init_embedder()

    def _init_embedder(self):
        """Initialize FastEmbed model for generating query embeddings"""
        try:
            from fastembed import TextEmbedding

            self.embedder = TextEmbedding(model_name=self.embedding_model)
            logger.debug(f"Initialized FastEmbed model: {self.embedding_model}")
        except Exception:
            logger.exception("Failed to initialize FastEmbed")
            self.embedder = None

    async def add_symbol(self, symbol: Symbol, content: str = ""):
        """Add symbol embedding to index"""
        if not self.embedder:
            return

        try:
            # Generate embedding for symbol
            text = f"{symbol.name} {symbol.signature or ''} {symbol.docstring or ''} {content}"
            embeddings = list(self.embedder.embed([text]))

            if embeddings:
                embedding = np.array(embeddings[0])
                await self.db_backend.add_embedding(
                    symbol_id=symbol.id, embedding=embedding, model=self.embedding_model
                )
        except Exception:
            logger.exception(f"Failed to add embedding for {symbol.id}")

    async def generate_embeddings_for_repo(
        self, repo_id: str, batch_size: int = 64, include_enriched: bool = False
    ) -> int:
        """Generate and store embeddings for all symbols in a repo (batch mode).

        Called once after indexing completes. Can be called again with include_enriched=True
        after wiki/queries are generated to include richer context.

        Args:
            repo_id: Repository identifier
            batch_size: Number of embeddings to generate per batch
            include_enriched: If True, include wiki summaries, queries, and descriptions in embeddings

        Returns:
            Number of embeddings stored
        """
        if not self.embedder:
            logger.warning("FastEmbed not initialized; skipping embedding generation")
            return 0

        # Fetch symbols with optional enriched data
        if include_enriched:
            result = self.db_backend.conn.execute(
                """
                SELECT
                    s.id,
                    s.name,
                    s.signature,
                    s.docstring,
                    s.description,
                    s.associated_queries
                FROM symbols s
                WHERE s.repo_id = ?
            """,
                (repo_id,),
            ).fetchall()
        else:
            result = self.db_backend.conn.execute(
                "SELECT id, name, signature, docstring, NULL, NULL FROM symbols WHERE repo_id = ?",
                (repo_id,),
            ).fetchall()

        if not result:
            return 0

        ids = [row[0] for row in result]

        # Build richer embedding text
        texts = []
        for row in result:
            parts = [row[1], row[2], row[3]]  # name, signature, docstring

            if include_enriched:
                # Add description if available
                if row[4]:
                    parts.append(row[4])

                # Add associated queries if available
                if row[5]:
                    try:
                        import json

                        queries = json.loads(row[5]) if isinstance(row[5], str) else row[5]
                        if queries:
                            parts.append(" ".join(queries[:5]))  # Add top 5 queries
                    except:
                        pass

            texts.append(" ".join(filter(None, parts)))

        total = len(ids)
        stored = 0
        total_batches = (total + batch_size - 1) // batch_size
        enriched_str = " (enriched)" if include_enriched else ""

        # Log every 10 batches or at key milestones
        log_interval = max(1, total_batches // 10)

        for batch_num, batch_start in enumerate(range(0, total, batch_size), 1):
            batch_ids = ids[batch_start : batch_start + batch_size]
            batch_texts = texts[batch_start : batch_start + batch_size]
            embeddings = list(self.embedder.embed(batch_texts))
            for sid, emb in zip(batch_ids, embeddings):
                await self.db_backend.add_embedding(
                    symbol_id=sid,
                    embedding=np.array(emb),
                    model=self.embedding_model,
                )
            stored += len(batch_ids)

            # Log progress every N batches or at completion (DEBUG to avoid interfering with Rich progress)
            if batch_num % log_interval == 0 or batch_num == total_batches:
                logger.debug(
                    f"Generating{enriched_str} embeddings for {total} symbols: {batch_num}/{total_batches} batches, {stored} embedded"
                )

        logger.debug(f"Embeddings complete: {stored} stored for repo '{repo_id}'")
        return stored

    async def build_index(self):
        """Build semantic index (handled by DuckDB automatically)"""
        # HNSW index is created automatically by schema

    async def save(self, index_dir: str):
        """Save index (handled by DuckDB automatically)"""
        # DuckDB persists automatically

    async def load(self, index_dir: str) -> bool:
        """Load index (handled by DuckDB automatically)"""
        # DuckDB loads automatically
        return True

    async def search(
        self, query: str, limit: int = 10, min_similarity: float = 0.0
    ) -> list[tuple[str, float]]:
        """Search for symbols using DuckDB VSS"""
        if not self.embedder:
            logger.warning("FastEmbed not initialized, skipping semantic search")
            return []

        try:
            # Generate query embedding
            embeddings = list(self.embedder.embed([query]))
            if not embeddings:
                return []

            query_embedding = np.array(embeddings[0])

            # Search using DuckDB VSS
            results = await self.db_backend.semantic_search(
                query_embedding=query_embedding, limit=limit, min_similarity=min_similarity
            )

            # Convert to (symbol_id, score) tuples
            return [(symbol.id, score) for symbol, score in results]
        except Exception as e:
            logger.error(f"DuckDB semantic search failed: {e}")
            return []


def create_search_engines(backend, embedding_model: str | None = None):
    """Factory function to create DuckDB-based search engines

    Args:
        backend: DuckDBBackend or GraphAdapter instance
        embedding_model: Model name for semantic search

    Returns:
        Tuple of (text_engine, semantic_engine)
    """
    text_engine = DuckDBTextSearchEngine(backend)
    semantic_engine = DuckDBSemanticSearchEngine(backend, embedding_model)

    return text_engine, semantic_engine
