"""
Unit tests for DuckDB backend
"""

import tempfile
from pathlib import Path

import numpy as np
import pytest

from comind.core.graph import Relationship, RelationType, Symbol, SymbolType
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
def sample_symbol():
    """Create a sample symbol for testing"""
    return Symbol(
        id="test_func_1",
        name="test_function",
        type=SymbolType.FUNCTION,
        file_path="test.py",
        line_start=10,
        line_end=20,
        signature="def test_function(x: int) -> str",
        docstring="Test function docstring",
        repo_id="test_repo",
    )


@pytest.fixture
def sample_relationship(sample_symbol):
    """Create a sample relationship for testing"""
    caller = Symbol(
        id="caller_func",
        name="caller",
        type=SymbolType.FUNCTION,
        file_path="caller.py",
        line_start=5,
        line_end=10,
        repo_id="test_repo",
    )
    return Relationship(
        source_id=caller.id, target_id=sample_symbol.id, type=RelationType.CALLS
    ), caller


class TestDuckDBBackend:
    """Test suite for DuckDB backend"""

    @pytest.mark.asyncio
    async def test_backend_initialization(self, temp_db):
        """Test that backend initializes correctly"""
        assert temp_db.conn is not None

        # Check that tables exist using DuckDB's information schema
        tables = temp_db.conn.execute("""
            SELECT table_name FROM information_schema.tables
            WHERE table_schema = 'main'
        """).fetchall()

        table_names = [t[0] for t in tables]
        assert "symbols" in table_names
        assert "relationships" in table_names
        assert "symbol_embeddings" in table_names
        assert "file_metadata" in table_names
        assert "llm_cache" in table_names

    @pytest.mark.asyncio
    async def test_add_symbol(self, temp_db, sample_symbol):
        """Test adding a symbol to the database"""
        await temp_db.add_symbol(sample_symbol)

        # Verify symbol was added
        result = temp_db.conn.execute(
            """
            SELECT * FROM symbols WHERE id = ?
        """,
            (sample_symbol.id,),
        ).fetchone()

        assert result is not None
        assert result[1] == sample_symbol.name
        assert result[2] == sample_symbol.type.value

    @pytest.mark.asyncio
    async def test_get_symbol(self, temp_db, sample_symbol):
        """Test retrieving a symbol from the database"""
        await temp_db.add_symbol(sample_symbol)

        retrieved = await temp_db.get_symbol(sample_symbol.id)

        assert retrieved is not None
        assert retrieved.id == sample_symbol.id
        assert retrieved.name == sample_symbol.name
        assert retrieved.type == sample_symbol.type

    @pytest.mark.asyncio
    async def test_add_relationship(self, temp_db, sample_relationship):
        """Test adding a relationship to the database"""
        relationship, caller = sample_relationship

        # Add both symbols first
        await temp_db.add_symbol(caller)
        target = Symbol(
            id=relationship.target_id,
            name="target",
            type=SymbolType.FUNCTION,
            file_path="target.py",
            line_start=1,
            line_end=5,
            repo_id="test_repo",
        )
        await temp_db.add_symbol(target)

        # Add relationship
        await temp_db.add_relationship(relationship)

        # Verify relationship was added
        result = temp_db.conn.execute(
            """
            SELECT * FROM relationships
            WHERE source_id = ? AND target_id = ?
        """,
            (relationship.source_id, relationship.target_id),
        ).fetchone()

        assert result is not None
        # Schema: id, source_id, target_id, type, confidence, properties, created_at
        assert result[3] == relationship.type.value

    @pytest.mark.asyncio
    async def test_get_callers(self, temp_db, sample_relationship):
        """Test getting callers of a symbol"""
        relationship, caller = sample_relationship

        # Setup
        await temp_db.add_symbol(caller)
        target = Symbol(
            id=relationship.target_id,
            name="target",
            type=SymbolType.FUNCTION,
            file_path="target.py",
            line_start=1,
            line_end=5,
            repo_id="test_repo",
        )
        await temp_db.add_symbol(target)
        await temp_db.add_relationship(relationship)

        # Get callers
        callers = await temp_db.get_callers(target.id)

        assert len(callers) == 1
        assert callers[0].id == caller.id

    @pytest.mark.asyncio
    async def test_get_callees(self, temp_db, sample_relationship):
        """Test getting callees of a symbol"""
        relationship, caller = sample_relationship

        # Setup
        await temp_db.add_symbol(caller)
        target = Symbol(
            id=relationship.target_id,
            name="target",
            type=SymbolType.FUNCTION,
            file_path="target.py",
            line_start=1,
            line_end=5,
            repo_id="test_repo",
        )
        await temp_db.add_symbol(target)
        await temp_db.add_relationship(relationship)

        # Get callees
        callees = await temp_db.get_callees(caller.id)

        assert len(callees) == 1
        assert callees[0].id == target.id

    @pytest.mark.asyncio
    async def test_add_embedding(self, temp_db, sample_symbol):
        """Test adding an embedding for a symbol"""
        await temp_db.add_symbol(sample_symbol)

        embedding = np.random.rand(384).astype(np.float32)

        await temp_db.add_embedding(
            symbol_id=sample_symbol.id, embedding=embedding, model="test-model"
        )

        # Verify embedding was added
        result = temp_db.conn.execute(
            """
            SELECT * FROM symbol_embeddings WHERE symbol_id = ?
        """,
            (sample_symbol.id,),
        ).fetchone()

        assert result is not None
        assert result[2] == "test-model"

    @pytest.mark.asyncio
    async def test_text_search(self, temp_db, sample_symbol):
        """Test full-text search"""
        await temp_db.add_symbol(sample_symbol)

        # Search for the symbol
        results = await temp_db.text_search("test_function", limit=10)

        assert len(results) > 0
        assert results[0][0].id == sample_symbol.id

    @pytest.mark.asyncio
    async def test_file_metadata_tracking(self, temp_db):
        """Test file metadata tracking for incremental updates"""
        file_path = "test.py"
        repo_id = "test_repo"
        file_hash = "abc123"

        await temp_db.update_file_metadata(
            file_path=file_path,
            repo_id=repo_id,
            file_hash=file_hash,
            mtime=1234567890,
            size=1024,
            symbol_count=5,
        )

        # Verify metadata was added
        result = temp_db.conn.execute(
            """
            SELECT * FROM file_metadata WHERE file_path = ? AND repo_id = ?
        """,
            (file_path, repo_id),
        ).fetchone()

        assert result is not None
        assert result[2] == file_hash
        assert result[5] == 5  # symbol_count

    @pytest.mark.asyncio
    async def test_detect_changed_files(self, temp_db):
        """Test change detection for incremental updates"""
        repo_id = "test_repo"

        # Add initial file metadata
        await temp_db.update_file_metadata(
            file_path="file1.py",
            repo_id=repo_id,
            file_hash="hash1",
            mtime=1000,
            size=100,
            symbol_count=2,
        )

        # Simulate current files
        current_files = {
            "file1.py": "hash1",  # Unchanged
            "file2.py": "hash2",  # New
            "file3.py": "hash3",  # New
        }

        changed = await temp_db.detect_changed_files(repo_id, current_files)

        assert "file2.py" in changed
        assert changed["file2.py"] == "new"
        assert "file3.py" in changed
        assert changed["file3.py"] == "new"

    @pytest.mark.asyncio
    async def test_llm_cache(self, temp_db, sample_symbol):
        """Test LLM cache functionality"""
        await temp_db.add_symbol(sample_symbol)

        cache_key = "wiki:test_func_1"
        content_hash = "content_hash_123"
        content = "This is cached wiki content"

        # Save to cache
        await temp_db.save_llm_cache(
            cache_key=cache_key,
            symbol_id=sample_symbol.id,
            content_hash=content_hash,
            cache_type="wiki",
            content=content,
            model="gpt-4o-mini",
        )

        # Retrieve from cache
        cached = await temp_db.get_llm_cache(cache_key, content_hash)

        assert cached is not None
        assert cached == content

        # Check access count increased
        result = temp_db.conn.execute(
            """
            SELECT access_count FROM llm_cache WHERE cache_key = ?
        """,
            (cache_key,),
        ).fetchone()

        assert result[0] == 1

    @pytest.mark.asyncio
    async def test_register_repository(self, temp_db):
        """Test repository registration"""
        await temp_db.register_repository(
            repo_id="test_repo",
            name="Test Repository",
            path="/path/to/repo",
            metadata={"branch": "main"},
        )

        # Verify repository was registered
        result = temp_db.conn.execute(
            """
            SELECT * FROM repositories WHERE repo_id = ?
        """,
            ("test_repo",),
        ).fetchone()

        assert result is not None
        assert result[1] == "Test Repository"

    @pytest.mark.asyncio
    async def test_get_repository_stats(self, temp_db, sample_symbol):
        """Test getting repository statistics"""
        repo_id = "test_repo"

        # Add some data
        await temp_db.register_repository(repo_id=repo_id, name="Test Repo", path="/test")
        await temp_db.add_symbol(sample_symbol)

        # Get stats
        stats = await temp_db.get_repository_stats(repo_id)

        assert stats is not None
        assert stats["symbol_count"] == 1
        assert stats["file_count"] == 1
