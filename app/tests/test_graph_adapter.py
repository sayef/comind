"""
Unit tests for GraphAdapter
"""

import pytest
import tempfile
import os

from comind.storage.duckdb_backend import DuckDBBackend
from comind.storage.graph_adapter import GraphAdapter
from comind.core.graph import Symbol, Relationship, SymbolType, RelationType


@pytest.fixture
def temp_db():
    """Create a temporary DuckDB database for testing"""
    with tempfile.TemporaryDirectory() as tmpdir:
        db_path = os.path.join(tmpdir, "test.duckdb")
        backend = DuckDBBackend(db_path)
        yield backend
        backend.close()


@pytest.fixture
def adapter(temp_db):
    """Create a GraphAdapter for testing"""
    return GraphAdapter(temp_db)


@pytest.fixture
def sample_symbols():
    """Create sample symbols for testing"""
    return [
        Symbol(
            id="func1",
            name="function_one",
            type=SymbolType.FUNCTION,
            file_path="test.py",
            line_start=1,
            line_end=10,
            repo_id="test_repo"
        ),
        Symbol(
            id="func2",
            name="function_two",
            type=SymbolType.FUNCTION,
            file_path="test.py",
            line_start=15,
            line_end=25,
            repo_id="test_repo"
        )
    ]


class TestGraphAdapter:
    """Test suite for GraphAdapter"""
    
    @pytest.mark.asyncio
    async def test_initialization(self, adapter, temp_db):
        """Test adapter initialization"""
        assert adapter.backend == temp_db
        assert adapter._repositories == {}
    
    @pytest.mark.asyncio
    async def test_add_symbol(self, adapter, sample_symbols):
        """Test adding symbols through adapter"""
        symbol = sample_symbols[0]
        await adapter.add_symbol(symbol)
        
        # Verify symbol was added
        retrieved = await adapter.get_symbol(symbol.id)
        assert retrieved is not None
        assert retrieved.id == symbol.id
    
    @pytest.mark.asyncio
    async def test_get_symbol(self, adapter, sample_symbols):
        """Test getting symbols through adapter"""
        symbol = sample_symbols[0]
        await adapter.add_symbol(symbol)
        
        retrieved = await adapter.get_symbol(symbol.id)
        assert retrieved.name == symbol.name
        assert retrieved.type == symbol.type
    
    @pytest.mark.asyncio
    async def test_add_relationship(self, adapter, sample_symbols):
        """Test adding relationships through adapter"""
        # Add both symbols
        for symbol in sample_symbols:
            await adapter.add_symbol(symbol)
        
        # Add relationship
        relationship = Relationship(
            source_id=sample_symbols[0].id,
            target_id=sample_symbols[1].id,
            type=RelationType.CALLS
        )
        await adapter.add_relationship(relationship)
        
        # Verify relationship exists
        relationships = await adapter.get_relationships(
            sample_symbols[0].id,
            RelationType.CALLS,
            direction="outgoing"
        )
        assert len(relationships) == 1
    
    @pytest.mark.asyncio
    async def test_get_callers(self, adapter, sample_symbols):
        """Test getting callers through adapter"""
        # Setup
        for symbol in sample_symbols:
            await adapter.add_symbol(symbol)
        
        relationship = Relationship(
            source_id=sample_symbols[0].id,
            target_id=sample_symbols[1].id,
            type=RelationType.CALLS
        )
        await adapter.add_relationship(relationship)
        
        # Get callers
        callers = await adapter.get_callers(sample_symbols[1].id)
        assert len(callers) == 1
        assert callers[0].id == sample_symbols[0].id
    
    @pytest.mark.asyncio
    async def test_get_callees(self, adapter, sample_symbols):
        """Test getting callees through adapter"""
        # Setup
        for symbol in sample_symbols:
            await adapter.add_symbol(symbol)
        
        relationship = Relationship(
            source_id=sample_symbols[0].id,
            target_id=sample_symbols[1].id,
            type=RelationType.CALLS
        )
        await adapter.add_relationship(relationship)
        
        # Get callees
        callees = await adapter.get_callees(sample_symbols[0].id)
        assert len(callees) == 1
        assert callees[0].id == sample_symbols[1].id
    
    @pytest.mark.asyncio
    async def test_get_dependencies(self, adapter, sample_symbols):
        """Test getting dependencies through adapter"""
        # Setup
        for symbol in sample_symbols:
            await adapter.add_symbol(symbol)
        
        relationship = Relationship(
            source_id=sample_symbols[0].id,
            target_id=sample_symbols[1].id,
            type=RelationType.IMPORTS
        )
        await adapter.add_relationship(relationship)
        
        # Get dependencies
        deps = await adapter.get_dependencies(sample_symbols[0].id)
        assert len(deps) == 1
        assert deps[0].id == sample_symbols[1].id
    
    @pytest.mark.asyncio
    async def test_query(self, adapter, sample_symbols):
        """Test query method"""
        for symbol in sample_symbols:
            await adapter.add_symbol(symbol)
        
        results = await adapter.query("function")
        assert isinstance(results, list)
    
    @pytest.mark.asyncio
    async def test_update_symbol_description(self, adapter, sample_symbols):
        """Test updating symbol description"""
        symbol = sample_symbols[0]
        await adapter.add_symbol(symbol)
        
        new_description = "Updated description"
        await adapter.update_symbol_description(symbol.id, new_description)
        
        # Verify description was updated
        result = adapter.backend.conn.execute("""
            SELECT description FROM symbols WHERE id = ?
        """, (symbol.id,)).fetchone()
        
        assert result[0] == new_description
    
    @pytest.mark.asyncio
    async def test_store_processes(self, adapter):
        """Test storing processes"""
        # Register repository first (required by foreign key)
        await adapter.backend.register_repository(
            repo_id="test_repo",
            name="Test Repo",
            path="/test"
        )
        
        processes = [
            {
                "id": "proc1",
                "repo_id": "test_repo",
                "name": "Authentication Flow",
                "entry_point": "authenticate",
                "steps": [
                    {"id": "step1", "name": "validate"},
                    {"id": "step2", "name": "authorize"}
                ],
                "priority": 1
            }
        ]
        
        await adapter.store_processes(processes)
        
        # Verify process was stored
        result = adapter.backend.conn.execute("""
            SELECT * FROM processes WHERE process_id = ?
        """, ("proc1",)).fetchone()
        
        assert result is not None
        assert result[2] == "Authentication Flow"
    
    @pytest.mark.asyncio
    async def test_get_processes(self, adapter):
        """Test getting processes"""
        # Register repository first (required by foreign key)
        await adapter.backend.register_repository(
            repo_id="test_repo",
            name="Test Repo",
            path="/test"
        )
        
        processes = [
            {
                "id": "proc1",
                "repo_id": "test_repo",
                "name": "Test Process",
                "entry_point": "main",
                "steps": [],
                "priority": 1
            }
        ]
        
        await adapter.store_processes(processes)
        
        retrieved = await adapter.get_processes()
        assert len(retrieved) == 1
        assert retrieved[0]["name"] == "Test Process"
    
    @pytest.mark.asyncio
    async def test_get_all_symbols(self, adapter, sample_symbols):
        """Test getting all symbols"""
        for symbol in sample_symbols:
            await adapter.add_symbol(symbol)
        
        all_symbols = await adapter.get_all_symbols()
        assert len(all_symbols) == 2
    
    @pytest.mark.asyncio
    async def test_get_all_symbols_filtered_by_repo(self, adapter, sample_symbols):
        """Test getting symbols filtered by repo_id"""
        for symbol in sample_symbols:
            await adapter.add_symbol(symbol)
        
        # Add symbol from different repo
        other_symbol = Symbol(
            id="other",
            name="other_func",
            type=SymbolType.FUNCTION,
            file_path="other.py",
            line_start=1,
            line_end=5,
            repo_id="other_repo"
        )
        await adapter.add_symbol(other_symbol)
        
        # Get symbols for test_repo only
        repo_symbols = await adapter.get_all_symbols(repo_id="test_repo")
        assert len(repo_symbols) == 2
        assert all(s.repo_id == "test_repo" for s in repo_symbols)
    
    @pytest.mark.asyncio
    async def test_get_all_relationships(self, adapter, sample_symbols):
        """Test getting all relationships"""
        for symbol in sample_symbols:
            await adapter.add_symbol(symbol)
        
        relationship = Relationship(
            source_id=sample_symbols[0].id,
            target_id=sample_symbols[1].id,
            type=RelationType.CALLS
        )
        await adapter.add_relationship(relationship)
        
        all_rels = await adapter.get_all_relationships()
        assert len(all_rels) == 1
    
    @pytest.mark.asyncio
    async def test_register_repository(self, adapter):
        """Test registering a repository"""
        await adapter.register_repository(
            repo_path="/test/repo",
            metadata={"branch": "main"}
        )
        
        repos = await adapter.list_repositories()
        assert len(repos) == 1
        assert repos[0]["name"] == "repo"
    
    @pytest.mark.asyncio
    async def test_list_repositories(self, adapter):
        """Test listing repositories"""
        await adapter.register_repository(
            repo_path="/test/repo1",
            metadata={}
        )
        await adapter.register_repository(
            repo_path="/test/repo2",
            metadata={}
        )
        
        repos = await adapter.list_repositories()
        assert len(repos) == 2
    
    @pytest.mark.asyncio
    async def test_get_repository_stats(self, adapter, sample_symbols):
        """Test getting repository statistics"""
        repo_id = "test_repo"
        
        # Register repo
        await adapter.backend.register_repository(
            repo_id=repo_id,
            name="Test Repo",
            path="/test"
        )
        
        # Add symbols
        for symbol in sample_symbols:
            await adapter.add_symbol(symbol)
        
        # Get stats
        stats = await adapter.get_repository_stats(repo_id)
        assert stats is not None
        assert stats["symbol_count"] == 2
