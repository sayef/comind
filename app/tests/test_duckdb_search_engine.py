"""
Unit tests for DuckDB search engines
"""

import tempfile
from decimal import Decimal
from pathlib import Path

import pytest

from comind.core.graph import Symbol, SymbolType
from comind.search.duckdb_search_engine import (
    DuckDBSemanticSearchEngine,
    DuckDBTextSearchEngine,
    create_search_engines,
)
from comind.storage.duckdb_backend import DuckDBBackend


@pytest.fixture
def temp_db():
    """Create a temporary DuckDB database for testing"""
    with tempfile.TemporaryDirectory() as tmpdir:
        db_path = Path(tmpdir) / "test.duckdb"
        backend = DuckDBBackend(str(db_path))
        yield backend
        backend.close()


@pytest.fixture
def sample_symbols():
    """Create sample symbols for testing"""
    return [
        Symbol(
            id="auth_func",
            name="authenticate_user",
            type=SymbolType.FUNCTION,
            file_path="auth.py",
            line_start=10,
            line_end=20,
            signature="def authenticate_user(username: str, password: str) -> bool",
            docstring="Authenticate a user with username and password",
            repo_id="test_repo",
        ),
        Symbol(
            id="parse_func",
            name="parse_token",
            type=SymbolType.FUNCTION,
            file_path="parser.py",
            line_start=5,
            line_end=15,
            signature="def parse_token(token: str) -> dict",
            docstring="Parse JWT token and return claims",
            repo_id="test_repo",
        ),
        Symbol(
            id="user_class",
            name="User",
            type=SymbolType.CLASS,
            file_path="models.py",
            line_start=1,
            line_end=50,
            signature="class User",
            docstring="User model class for authentication",
            repo_id="test_repo",
        ),
    ]


class TestDuckDBTextSearchEngine:
    """Test suite for DuckDB text search engine"""

    @pytest.mark.asyncio
    async def test_initialization(self, temp_db):
        """Test search engine initialization"""
        engine = DuckDBTextSearchEngine(temp_db)
        assert engine.backend is not None
        assert engine.db_backend is not None

    @pytest.mark.asyncio
    async def test_add_symbol(self, temp_db, sample_symbols):
        """Test adding symbols (should be no-op)"""
        engine = DuckDBTextSearchEngine(temp_db)

        # add_symbol should not raise any errors
        for symbol in sample_symbols:
            await engine.add_symbol(symbol, content="")

    @pytest.mark.asyncio
    async def test_text_search(self, temp_db, sample_symbols):
        """Test text search functionality"""
        # Add symbols to database
        for symbol in sample_symbols:
            await temp_db.add_symbol(symbol)

        engine = DuckDBTextSearchEngine(temp_db)

        # Search for "authenticate"
        results = await engine.search("authenticate", limit=10)

        assert len(results) > 0
        # Should find authenticate_user function
        symbol_ids = [r[0] for r in results]
        assert "auth_func" in symbol_ids

    @pytest.mark.asyncio
    async def test_search_returns_scores(self, temp_db, sample_symbols):
        """Test that search returns symbol IDs and scores"""
        for symbol in sample_symbols:
            await temp_db.add_symbol(symbol)

        engine = DuckDBTextSearchEngine(temp_db)
        results = await engine.search("user", limit=10)

        assert len(results) > 0
        # Each result should be (symbol_id, score) tuple
        for result in results:
            assert len(result) == 2
            assert isinstance(result[0], str)  # symbol_id
            assert isinstance(result[1], (int, float, Decimal))  # score (DuckDB returns Decimal)

    @pytest.mark.asyncio
    async def test_search_limit(self, temp_db, sample_symbols):
        """Test search result limit"""
        for symbol in sample_symbols:
            await temp_db.add_symbol(symbol)

        engine = DuckDBTextSearchEngine(temp_db)
        results = await engine.search("function", limit=2)

        assert len(results) <= 2

    @pytest.mark.asyncio
    async def test_search_empty_query(self, temp_db):
        """Test search with empty query"""
        engine = DuckDBTextSearchEngine(temp_db)
        results = await engine.search("", limit=10)

        # Should return empty results or handle gracefully
        assert isinstance(results, list)


class TestDuckDBSemanticSearchEngine:
    """Test suite for DuckDB semantic search engine"""

    @pytest.mark.asyncio
    async def test_initialization(self, temp_db):
        """Test semantic search engine initialization"""
        engine = DuckDBSemanticSearchEngine(temp_db)
        assert engine.backend is not None
        assert engine.db_backend is not None
        assert engine.embedding_model == "BAAI/bge-small-en-v1.5"

    @pytest.mark.asyncio
    async def test_custom_model(self, temp_db):
        """Test initialization with custom model"""
        engine = DuckDBSemanticSearchEngine(temp_db, embedding_model="custom-model")
        assert engine.embedding_model == "custom-model"

    @pytest.mark.asyncio
    async def test_add_symbol_with_embedding(self, temp_db, sample_symbols):
        """Test adding symbol with embedding generation"""
        engine = DuckDBSemanticSearchEngine(temp_db)

        symbol = sample_symbols[0]
        await temp_db.add_symbol(symbol)

        # Add embedding
        await engine.add_symbol(symbol, content="")

        # Verify embedding was added
        result = temp_db.conn.execute(
            """
            SELECT * FROM symbol_embeddings WHERE symbol_id = ?
        """,
            (symbol.id,),
        ).fetchone()

        # May be None if FastEmbed not available
        if engine.embedder:
            assert result is not None

    @pytest.mark.asyncio
    async def test_semantic_search(self, temp_db, sample_symbols):
        """Test semantic search functionality"""
        engine = DuckDBSemanticSearchEngine(temp_db)

        # Skip if embedder not available
        if not engine.embedder:
            pytest.skip("FastEmbed not available")

        # Add symbols with embeddings
        for symbol in sample_symbols:
            await temp_db.add_symbol(symbol)
            await engine.add_symbol(symbol, content="")

        # Search
        results = await engine.search("user authentication", limit=10)

        # Should find relevant symbols
        assert isinstance(results, list)

    @pytest.mark.asyncio
    async def test_semantic_search_with_min_similarity(self, temp_db, sample_symbols):
        """Test semantic search with minimum similarity threshold"""
        engine = DuckDBSemanticSearchEngine(temp_db)

        if not engine.embedder:
            pytest.skip("FastEmbed not available")

        for symbol in sample_symbols:
            await temp_db.add_symbol(symbol)
            await engine.add_symbol(symbol, content="")

        # Search with high similarity threshold
        results = await engine.search("authentication", limit=10, min_similarity=0.8)

        # Results should have high similarity
        assert isinstance(results, list)
        for result in results:
            if len(result) == 2:
                assert result[1] >= 0.8


class TestSearchEngineFactory:
    """Test suite for search engine factory"""

    @pytest.mark.asyncio
    async def test_create_search_engines(self, temp_db):
        """Test factory function creates both engines"""
        text_engine, semantic_engine = create_search_engines(temp_db)

        assert isinstance(text_engine, DuckDBTextSearchEngine)
        assert isinstance(semantic_engine, DuckDBSemanticSearchEngine)

    @pytest.mark.asyncio
    async def test_create_with_custom_model(self, temp_db):
        """Test factory with custom embedding model"""
        _text_engine, semantic_engine = create_search_engines(
            temp_db, embedding_model="custom-model"
        )

        assert semantic_engine.embedding_model == "custom-model"

    @pytest.mark.asyncio
    async def test_engines_share_backend(self, temp_db):
        """Test that engines share the same backend"""
        text_engine, semantic_engine = create_search_engines(temp_db)

        # Both should reference the same backend
        assert text_engine.db_backend == semantic_engine.db_backend
