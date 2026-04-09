"""
Integration tests for CLI with DuckDB backend
"""

import tempfile
from pathlib import Path

import pytest

from comind.core.graph import Symbol, SymbolType
from comind.indexing.incremental_indexer import IncrementalIndexer
from comind.storage.duckdb_backend import DuckDBBackend
from comind.storage.graph_adapter import GraphAdapter


@pytest.fixture
def temp_workspace():
    """Create a temporary workspace for testing"""
    with tempfile.TemporaryDirectory() as tmpdir:
        workspace = Path(tmpdir)

        # Create directory structure
        indexes_dir = workspace / "indexes"
        indexes_dir.mkdir()

        # Create a test repository
        repo_dir = workspace / "test_repo"
        repo_dir.mkdir()

        # Create test Python files
        (repo_dir / "main.py").write_text("""
def main():
    '''Main entry point'''
    result = process_data()
    return result

def process_data():
    '''Process the data'''
    return 42
""")

        (repo_dir / "utils.py").write_text("""
class Helper:
    '''Helper class'''
    
    def help(self):
        '''Provide help'''
        return "help"
""")

        yield {"workspace": workspace, "indexes_dir": indexes_dir, "repo_dir": repo_dir}


class TestCLIIntegration:
    """Integration tests for CLI commands"""

    @pytest.mark.asyncio
    async def test_analyze_command_creates_database(self, temp_workspace):
        """Test that analyze command creates DuckDB database"""
        repo_name = "test_repo"
        db_path = temp_workspace["indexes_dir"] / repo_name / "graph.duckdb"
        db_path.parent.mkdir(parents=True, exist_ok=True)

        # Initialize backend (simulates CLI analyze command)
        backend = DuckDBBackend(str(db_path))

        # Verify database was created
        assert db_path.exists()

        # Verify tables exist using DuckDB information schema
        tables = backend.conn.execute("""
            SELECT table_name FROM information_schema.tables 
            WHERE table_schema = 'main'
        """).fetchall()

        table_names = [t[0] for t in tables]
        assert "symbols" in table_names
        assert "relationships" in table_names

        backend.close()

    @pytest.mark.asyncio
    async def test_incremental_reindex_workflow(self, temp_workspace):
        """Test full incremental re-index workflow"""
        repo_name = "test_repo"
        repo_path = temp_workspace["repo_dir"]
        db_path = temp_workspace["indexes_dir"] / repo_name / "graph.duckdb"
        db_path.parent.mkdir(parents=True, exist_ok=True)

        # First index
        backend = DuckDBBackend(str(db_path))
        incremental = IncrementalIndexer(backend)

        # Detect changes (all new)
        changed_files = await incremental.detect_changes(
            repo_id=repo_name, repo_path=str(repo_path), file_extensions=[".py"]
        )

        assert len(changed_files) > 0
        assert all(status == "new" for status in changed_files.values())

        # Simulate indexing by updating metadata
        for file_path in changed_files.keys():
            full_path = repo_path / file_path
            file_hash = incremental._compute_file_hash(full_path)
            await backend.update_file_metadata(
                file_path=file_path,
                repo_id=repo_name,
                file_hash=file_hash,
                mtime=int(full_path.stat().st_mtime),
                size=full_path.stat().st_size,
                symbol_count=1,
            )

        # Second index - no changes
        changed_files = await incremental.detect_changes(
            repo_id=repo_name, repo_path=str(repo_path), file_extensions=[".py"]
        )

        assert len(changed_files) == 0

        # Modify a file
        (repo_path / "main.py").write_text("""
def main():
    '''Main entry point - updated'''
    result = process_data()
    return result + 1

def process_data():
    '''Process the data'''
    return 42
""")

        # Third index - one file changed
        changed_files = await incremental.detect_changes(
            repo_id=repo_name, repo_path=str(repo_path), file_extensions=[".py"]
        )

        assert len(changed_files) == 1
        assert "main.py" in changed_files
        assert changed_files["main.py"] == "modified"

        backend.close()

    @pytest.mark.asyncio
    async def test_search_workflow(self, temp_workspace):
        """Test search workflow with DuckDB backend"""
        repo_name = "test_repo"
        db_path = temp_workspace["indexes_dir"] / repo_name / "graph.duckdb"
        db_path.parent.mkdir(parents=True, exist_ok=True)

        backend = DuckDBBackend(str(db_path))
        graph = GraphAdapter(backend)

        # Add test symbols
        symbols = [
            Symbol(
                id="main_func",
                name="main",
                type=SymbolType.FUNCTION,
                file_path="main.py",
                line_start=1,
                line_end=5,
                signature="def main()",
                docstring="Main entry point",
                repo_id=repo_name,
            ),
            Symbol(
                id="process_func",
                name="process_data",
                type=SymbolType.FUNCTION,
                file_path="main.py",
                line_start=7,
                line_end=10,
                signature="def process_data()",
                docstring="Process the data",
                repo_id=repo_name,
            ),
        ]

        for symbol in symbols:
            await graph.add_symbol(symbol)

        # Search
        results = await backend.text_search("main", limit=10)

        assert len(results) > 0
        assert any(s.name == "main" for s, _ in results)

        backend.close()

    @pytest.mark.asyncio
    async def test_repository_registration(self, temp_workspace):
        """Test repository registration workflow"""
        repo_name = "test_repo"
        repo_path = str(temp_workspace["repo_dir"])
        db_path = temp_workspace["indexes_dir"] / repo_name / "graph.duckdb"
        db_path.parent.mkdir(parents=True, exist_ok=True)

        backend = DuckDBBackend(str(db_path))

        # Register repository
        await backend.register_repository(
            repo_id=repo_name,
            name=repo_name,
            path=repo_path,
            branch="main",
            metadata={"indexed_at": 1234567890},
        )

        # Get stats
        stats = await backend.get_repository_stats(repo_name)

        assert stats is not None
        assert stats["name"] == repo_name
        assert stats["repo_id"] == repo_name

        backend.close()

    @pytest.mark.asyncio
    async def test_llm_cache_workflow(self, temp_workspace):
        """Test LLM caching workflow"""
        repo_name = "test_repo"
        db_path = temp_workspace["indexes_dir"] / repo_name / "graph.duckdb"
        db_path.parent.mkdir(parents=True, exist_ok=True)

        backend = DuckDBBackend(str(db_path))
        incremental = IncrementalIndexer(backend)

        # Add a symbol
        symbol = Symbol(
            id="test_func",
            name="test_function",
            type=SymbolType.FUNCTION,
            file_path="test.py",
            line_start=1,
            line_end=5,
            signature="def test_function()",
            docstring="Test function",
            repo_id=repo_name,
        )
        await backend.add_symbol(symbol)

        # First check - should regenerate
        should_regen = await incremental.should_regenerate_llm_content(symbol, "queries")
        assert should_regen is True

        # Save LLM result
        await incremental.save_llm_result(
            symbol=symbol,
            cache_type="queries",
            content='["query1", "query2"]',
            model="gpt-4o-mini",
            metadata={"query_count": 2},
        )

        # Second check - should use cache
        should_regen = await incremental.should_regenerate_llm_content(symbol, "queries")
        assert should_regen is False

        backend.close()

    @pytest.mark.asyncio
    async def test_graph_queries(self, temp_workspace):
        """Test graph query workflow"""
        repo_name = "test_repo"
        db_path = temp_workspace["indexes_dir"] / repo_name / "graph.duckdb"
        db_path.parent.mkdir(parents=True, exist_ok=True)

        backend = DuckDBBackend(str(db_path))
        graph = GraphAdapter(backend)

        # Create symbols
        caller = Symbol(
            id="caller",
            name="caller_func",
            type=SymbolType.FUNCTION,
            file_path="test.py",
            line_start=1,
            line_end=5,
            repo_id=repo_name,
        )
        callee = Symbol(
            id="callee",
            name="callee_func",
            type=SymbolType.FUNCTION,
            file_path="test.py",
            line_start=10,
            line_end=15,
            repo_id=repo_name,
        )

        await graph.add_symbol(caller)
        await graph.add_symbol(callee)

        # Add relationship
        from comind.core.graph import Relationship, RelationType

        rel = Relationship(source_id=caller.id, target_id=callee.id, type=RelationType.CALLS)
        await graph.add_relationship(rel)

        # Query callers
        callers = await graph.get_callers(callee.id)
        assert len(callers) == 1
        assert callers[0].id == caller.id

        # Query callees
        callees = await graph.get_callees(caller.id)
        assert len(callees) == 1
        assert callees[0].id == callee.id

        backend.close()


class TestCLIEdgeCases:
    """Test edge cases and error handling"""

    @pytest.mark.asyncio
    async def test_nonexistent_database(self, temp_workspace):
        """Test loading nonexistent database"""
        db_path = temp_workspace["indexes_dir"] / "nonexistent" / "graph.duckdb"

        # Should create new database
        backend = DuckDBBackend(str(db_path))
        assert backend.conn is not None
        backend.close()

    @pytest.mark.asyncio
    async def test_empty_repository(self, temp_workspace):
        """Test indexing empty repository"""
        empty_repo = temp_workspace["workspace"] / "empty_repo"
        empty_repo.mkdir()

        db_path = temp_workspace["indexes_dir"] / "empty" / "graph.duckdb"
        db_path.parent.mkdir(parents=True, exist_ok=True)

        backend = DuckDBBackend(str(db_path))
        incremental = IncrementalIndexer(backend)

        changed_files = await incremental.detect_changes(
            repo_id="empty", repo_path=str(empty_repo), file_extensions=[".py"]
        )

        assert len(changed_files) == 0
        backend.close()

    @pytest.mark.asyncio
    async def test_concurrent_access(self, temp_workspace):
        """Test concurrent database access"""
        db_path = temp_workspace["indexes_dir"] / "test" / "graph.duckdb"
        db_path.parent.mkdir(parents=True, exist_ok=True)

        # Create two backend instances
        backend1 = DuckDBBackend(str(db_path))
        backend2 = DuckDBBackend(str(db_path))

        # Both should work
        symbol1 = Symbol(
            id="sym1",
            name="symbol1",
            type=SymbolType.FUNCTION,
            file_path="test.py",
            line_start=1,
            line_end=5,
            repo_id="test",
        )

        await backend1.add_symbol(symbol1)

        # Backend2 should see the symbol
        retrieved = await backend2.get_symbol("sym1")
        assert retrieved is not None

        backend1.close()
        backend2.close()
