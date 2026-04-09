"""
Unit tests for incremental indexer
"""

import hashlib
import os
import tempfile
from pathlib import Path

import pytest

from comind.core.graph import Symbol, SymbolType
from comind.indexing.incremental_indexer import IncrementalIndexer
from comind.storage.duckdb_backend import DuckDBBackend


@pytest.fixture
def temp_db():
    """Create a temporary DuckDB database for testing"""
    with tempfile.TemporaryDirectory() as tmpdir:
        db_path = os.path.join(tmpdir, "test.duckdb")
        backend = DuckDBBackend(db_path)
        yield backend
        backend.close()


@pytest.fixture
def temp_repo():
    """Create a temporary repository for testing"""
    with tempfile.TemporaryDirectory() as tmpdir:
        repo_path = Path(tmpdir)

        # Create some test files
        (repo_path / "file1.py").write_text("def func1(): pass")
        (repo_path / "file2.py").write_text("def func2(): pass")
        (repo_path / "subdir").mkdir()
        (repo_path / "subdir" / "file3.py").write_text("def func3(): pass")

        yield repo_path


@pytest.fixture
def incremental_indexer(temp_db):
    """Create an incremental indexer for testing"""
    return IncrementalIndexer(temp_db)


class TestIncrementalIndexer:
    """Test suite for incremental indexer"""

    @pytest.mark.asyncio
    async def test_detect_changes_new_repo(self, incremental_indexer, temp_repo):
        """Test detecting changes for a new repository"""
        repo_id = "test_repo"

        changed_files = await incremental_indexer.detect_changes(
            repo_id=repo_id, repo_path=str(temp_repo), file_extensions=[".py"]
        )

        # All files should be marked as new
        assert len(changed_files) == 3
        assert all(status == "new" for status in changed_files.values())
        assert "file1.py" in changed_files
        assert "file2.py" in changed_files
        assert "subdir/file3.py" in changed_files

    @pytest.mark.asyncio
    async def test_detect_changes_no_changes(self, incremental_indexer, temp_repo, temp_db):
        """Test detecting changes when nothing has changed"""
        repo_id = "test_repo"

        # First detection - all new
        changed_files = await incremental_indexer.detect_changes(
            repo_id=repo_id, repo_path=str(temp_repo), file_extensions=[".py"]
        )

        # Update file metadata to simulate indexed state
        for file_path, status in changed_files.items():
            full_path = temp_repo / file_path
            file_hash = incremental_indexer._compute_file_hash(full_path)
            await temp_db.update_file_metadata(
                file_path=file_path,
                repo_id=repo_id,
                file_hash=file_hash,
                mtime=int(full_path.stat().st_mtime),
                size=full_path.stat().st_size,
                symbol_count=1,
            )

        # Second detection - no changes
        changed_files = await incremental_indexer.detect_changes(
            repo_id=repo_id, repo_path=str(temp_repo), file_extensions=[".py"]
        )

        assert len(changed_files) == 0

    @pytest.mark.asyncio
    async def test_detect_changes_modified_file(self, incremental_indexer, temp_repo, temp_db):
        """Test detecting modified files"""
        repo_id = "test_repo"

        # Initial detection
        changed_files = await incremental_indexer.detect_changes(
            repo_id=repo_id, repo_path=str(temp_repo), file_extensions=[".py"]
        )

        # Update metadata
        for file_path, status in changed_files.items():
            full_path = temp_repo / file_path
            file_hash = incremental_indexer._compute_file_hash(full_path)
            await temp_db.update_file_metadata(
                file_path=file_path,
                repo_id=repo_id,
                file_hash=file_hash,
                mtime=int(full_path.stat().st_mtime),
                size=full_path.stat().st_size,
                symbol_count=1,
            )

        # Modify a file
        (temp_repo / "file1.py").write_text("def func1(): return 42")

        # Detect changes again
        changed_files = await incremental_indexer.detect_changes(
            repo_id=repo_id, repo_path=str(temp_repo), file_extensions=[".py"]
        )

        assert len(changed_files) == 1
        assert "file1.py" in changed_files
        assert changed_files["file1.py"] == "modified"

    @pytest.mark.asyncio
    async def test_detect_changes_deleted_file(self, incremental_indexer, temp_repo, temp_db):
        """Test detecting deleted files"""
        repo_id = "test_repo"

        # Initial detection
        changed_files = await incremental_indexer.detect_changes(
            repo_id=repo_id, repo_path=str(temp_repo), file_extensions=[".py"]
        )

        # Update metadata
        for file_path, status in changed_files.items():
            full_path = temp_repo / file_path
            file_hash = incremental_indexer._compute_file_hash(full_path)
            await temp_db.update_file_metadata(
                file_path=file_path,
                repo_id=repo_id,
                file_hash=file_hash,
                mtime=int(full_path.stat().st_mtime),
                size=full_path.stat().st_size,
                symbol_count=1,
            )

        # Delete a file
        (temp_repo / "file2.py").unlink()

        # Detect changes
        changed_files = await incremental_indexer.detect_changes(
            repo_id=repo_id, repo_path=str(temp_repo), file_extensions=[".py"]
        )

        assert "file2.py" in changed_files
        assert changed_files["file2.py"] == "deleted"

    @pytest.mark.asyncio
    async def test_compute_file_hash(self, incremental_indexer, temp_repo):
        """Test file hash computation"""
        file_path = temp_repo / "file1.py"

        hash1 = incremental_indexer._compute_file_hash(file_path)
        assert len(hash1) == 64  # SHA256 hex digest

        # Same file should produce same hash
        hash2 = incremental_indexer._compute_file_hash(file_path)
        assert hash1 == hash2

        # Modified file should produce different hash
        file_path.write_text("def func1(): return 100")
        hash3 = incremental_indexer._compute_file_hash(file_path)
        assert hash1 != hash3

    @pytest.mark.asyncio
    async def test_should_index_file(self, incremental_indexer, temp_repo):
        """Test file filtering logic"""
        # Normal Python file should be indexed
        assert incremental_indexer._should_index_file(temp_repo / "file1.py")

        # __pycache__ should be skipped
        pycache_dir = temp_repo / "__pycache__"
        pycache_dir.mkdir()
        pycache_file = pycache_dir / "test.pyc"
        pycache_file.write_text("compiled")
        assert not incremental_indexer._should_index_file(pycache_file)

        # .git directory should be skipped
        git_dir = temp_repo / ".git"
        git_dir.mkdir()
        git_file = git_dir / "config"
        git_file.write_text("git config")
        assert not incremental_indexer._should_index_file(git_file)

    @pytest.mark.asyncio
    async def test_should_regenerate_llm_content(self, incremental_indexer, temp_db):
        """Test LLM content regeneration check"""
        symbol = Symbol(
            id="test_func",
            name="test_function",
            type=SymbolType.FUNCTION,
            file_path="test.py",
            line_start=1,
            line_end=5,
            signature="def test_function(x: int) -> str",
            docstring="Test docstring",
            repo_id="test_repo",
        )

        await temp_db.add_symbol(symbol)

        # First check - should regenerate (no cache)
        should_regen = await incremental_indexer.should_regenerate_llm_content(symbol, "wiki")
        assert should_regen is True

        # Save to cache
        content_parts = [
            symbol.signature or "",
            symbol.docstring or "",
            symbol.name,
            symbol.type.value,
        ]
        content_hash = hashlib.sha256("|".join(content_parts).encode()).hexdigest()

        cache_key = f"wiki:{symbol.id}"
        await temp_db.save_llm_cache(
            cache_key=cache_key,
            symbol_id=symbol.id,
            content_hash=content_hash,
            cache_type="wiki",
            content="Cached wiki content",
            model="gpt-4o-mini",
        )

        # Second check - should not regenerate (cache hit)
        should_regen = await incremental_indexer.should_regenerate_llm_content(symbol, "wiki")
        assert should_regen is False

    @pytest.mark.asyncio
    async def test_save_llm_result(self, incremental_indexer, temp_db):
        """Test saving LLM generation result"""
        symbol = Symbol(
            id="test_func",
            name="test_function",
            type=SymbolType.FUNCTION,
            file_path="test.py",
            line_start=1,
            line_end=5,
            signature="def test_function(x: int) -> str",
            docstring="Test docstring",
            repo_id="test_repo",
        )

        await temp_db.add_symbol(symbol)

        # Save LLM result
        await incremental_indexer.save_llm_result(
            symbol=symbol,
            cache_type="queries",
            content='["query1", "query2"]',
            model="gpt-4o-mini",
            metadata={"query_count": 2},
        )

        # Verify it was saved
        result = temp_db.conn.execute(
            """
            SELECT * FROM llm_cache WHERE symbol_id = ?
        """,
            (symbol.id,),
        ).fetchone()

        assert result is not None
        assert result[3] == "queries"
        assert '"query1"' in result[4]

    @pytest.mark.asyncio
    async def test_get_incremental_stats(self, incremental_indexer, temp_db):
        """Test getting incremental indexing statistics"""
        repo_id = "test_repo"

        # Register repository
        await temp_db.register_repository(repo_id=repo_id, name="Test Repo", path="/test")

        # Add some data
        symbol = Symbol(
            id="test_func",
            name="test_function",
            type=SymbolType.FUNCTION,
            file_path="test.py",
            line_start=1,
            line_end=5,
            repo_id=repo_id,
        )
        await temp_db.add_symbol(symbol)

        await temp_db.update_file_metadata(
            file_path="test.py",
            repo_id=repo_id,
            file_hash="hash123",
            mtime=1000,
            size=100,
            symbol_count=1,
        )

        # Get stats
        stats = await incremental_indexer.get_incremental_stats(repo_id)

        assert stats is not None
        assert "repository" in stats
        assert "cache" in stats
        assert "files" in stats
        assert stats["files"]["total"] == 1
