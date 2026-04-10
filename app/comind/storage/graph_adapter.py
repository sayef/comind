"""
Graph Adapter for DuckDB Backend

Provides KnowledgeGraph-compatible interface for DuckDB backend
to minimize changes to existing code.
"""

from typing import Any

from comind.core.graph import Relationship, RelationType, Symbol
from comind.storage.duckdb_backend import DuckDBBackend


class GraphAdapter:
    """Adapter to make DuckDB backend compatible with KnowledgeGraph interface"""

    def __init__(self, backend: DuckDBBackend):
        self.backend = backend
        self._repositories: dict[str, dict[str, Any]] = {}

    async def add_symbol(self, symbol: Symbol) -> None:
        """Add a symbol to the graph"""
        await self.backend.add_symbol(symbol)

    async def add_symbols_batch(self, symbols: list[Symbol]) -> None:
        """Bulk add symbols - much faster than individual adds"""
        await self.backend.add_symbols_batch(symbols)

    async def add_relationship(self, relationship: Relationship) -> None:
        """Add a relationship to the graph"""
        await self.backend.add_relationship(relationship)

    async def add_relationships_batch(self, relationships: list[Relationship]) -> None:
        """Bulk add relationships - much faster than individual adds"""
        await self.backend.add_relationships_batch(relationships)

    async def get_symbol(self, symbol_id: str) -> Symbol | None:
        """Get a symbol by ID"""
        return await self.backend.get_symbol(symbol_id)

    async def get_callers(self, symbol_id: str) -> list[Symbol]:
        """Get all symbols that call the given symbol"""
        return await self.backend.get_callers(symbol_id)

    async def get_callees(self, symbol_id: str) -> list[Symbol]:
        """Get all symbols that the given symbol calls"""
        return await self.backend.get_callees(symbol_id)

    async def get_dependencies(self, symbol_id: str) -> list[Symbol]:
        """Get all dependencies of a symbol"""
        relationships = await self.backend.get_relationships(
            symbol_id, RelationType.IMPORTS, direction="outgoing"
        )

        dependencies = []
        for rel in relationships:
            dep = await self.backend.get_symbol(rel.target_id)
            if dep:
                dependencies.append(dep)

        return dependencies

    async def get_relationships(
        self, symbol_id: str, relation_type: RelationType | None = None, direction: str = "outgoing"
    ) -> list[Relationship]:
        """Get relationships for a symbol"""
        return await self.backend.get_relationships(symbol_id, relation_type, direction)

    async def query(self, query: str) -> list[dict[str, Any]]:
        """Execute a simple graph query"""
        # Simple text search fallback
        results = []

        # Use DuckDB text search
        search_results = await self.backend.text_search(query, limit=50)
        for symbol, score in search_results:
            results.append({"symbol": symbol.model_dump(), "type": "symbol_match", "score": score})

        return results

    async def get_community(self, symbol_id: str) -> dict[str, Any] | None:
        """Get the community that a symbol belongs to"""
        # Not implemented in DuckDB yet - would need community detection algorithm
        return None

    async def get_communities(self, symbol_id: str | None = None) -> list[dict[str, Any]]:
        """Get detected communities/clusters

        Args:
            symbol_id: Optional symbol ID to get communities for specific symbol
        """
        if symbol_id:
            # Get community for specific symbol
            community = await self.get_community(symbol_id)
            return [community] if community else []

        # Get all communities - not implemented in DuckDB yet
        return []

    async def get_processes(self) -> list[dict[str, Any]]:
        """Get detected execution flows"""
        import json

        # Query from DuckDB
        result = self.backend.conn.execute("""
            SELECT * FROM processes
        """).fetchall()

        processes = []
        for row in result:
            # Parse JSON steps field - DuckDB JSON column may return dict/list or string
            steps = row[4]
            if isinstance(steps, str):
                try:
                    steps = json.loads(steps)
                except (json.JSONDecodeError, TypeError):
                    steps = []
            elif isinstance(steps, (list, dict)):
                # DuckDB JSON column returns native Python types
                steps = steps if isinstance(steps, list) else []
            elif steps is None:
                steps = []
            else:
                steps = []

            # Ensure each step is a dict, not a string
            if isinstance(steps, list):
                parsed_steps = []
                for step in steps:
                    if isinstance(step, str):
                        try:
                            parsed_steps.append(json.loads(step))
                        except (json.JSONDecodeError, TypeError):
                            pass
                    elif isinstance(step, dict):
                        parsed_steps.append(step)
                steps = parsed_steps

            processes.append(
                {
                    "process_id": row[0],
                    "repo_id": row[1],
                    "name": row[2],
                    "label": row[3],
                    "type": row[4],
                    "entry_point": row[5],
                    "steps": steps,
                    "priority": row[7],
                }
            )

        return processes

    async def get_symbol_processes(self, symbol_id: str) -> list[dict[str, Any]]:
        """Get processes that a specific symbol participates in"""
        processes = await self.get_processes()

        symbol_processes = []
        for process in processes:
            steps = process.get("steps", [])
            for step in steps:
                if step.get("id") == symbol_id:
                    symbol_processes.append(process)
                    break

        return symbol_processes

    async def update_symbol_description(self, symbol_id: str, description: str) -> None:
        """Update the LLM-generated description of a symbol"""
        self.backend.conn.execute(
            """
            UPDATE symbols SET description = ? WHERE id = ?
        """,
            (description, symbol_id),
        )

    async def store_processes(self, processes: list[dict[str, Any]]) -> None:
        """Persist detected processes into the graph backend.

        Deletes dependent process_queries rows first so INSERT OR REPLACE
        doesn't violate foreign-key constraints.
        """
        import json

        # Collect process IDs to clean up FK dependents
        process_ids = [process.get("id", process.get("process_id")) for process in processes]
        process_ids = [pid for pid in process_ids if pid]

        if process_ids:
            ph = ",".join("?" * len(process_ids))
            self.backend.conn.execute(
                f"DELETE FROM process_queries WHERE process_id IN ({ph})",
                tuple(process_ids),
            )

        for process in processes:
            self.backend.conn.execute(
                """
                INSERT OR REPLACE INTO processes (
                    process_id, repo_id, name, label, type, entry_point, steps, priority
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            """,
                (
                    process.get("id", process.get("process_id")),
                    process.get("repo_id"),
                    process.get("name"),
                    process.get("label"),
                    process.get("type"),
                    process.get("entry_point"),
                    json.dumps(process.get("steps", [])),
                    process.get("priority", 0),
                ),
            )

    async def get_all_symbols(self, repo_id: str | None = None) -> list[Symbol]:
        """Get all symbols in the graph, optionally filtered by repo_id"""
        if repo_id:
            result = self.backend.conn.execute(
                """
                SELECT * FROM symbols WHERE repo_id = ?
            """,
                (repo_id,),
            ).fetchall()
        else:
            result = self.backend.conn.execute("""
                SELECT * FROM symbols
            """).fetchall()

        return [self.backend._row_to_symbol(row) for row in result]

    async def get_all_relationships(self, repo_id: str | None = None) -> list[Relationship]:
        """Get all relationships in the graph, optionally filtered by repo_id"""
        if repo_id:
            result = self.backend.conn.execute(
                """
                SELECT r.* FROM relationships r
                JOIN symbols s ON r.source_id = s.id
                WHERE s.repo_id = ?
            """,
                (repo_id,),
            ).fetchall()
        else:
            result = self.backend.conn.execute("""
                SELECT * FROM relationships
            """).fetchall()

        return [self.backend._row_to_relationship(row) for row in result]

    async def register_repository(
        self, repo_path: str, metadata: dict[str, Any], repo_id: str | None = None
    ) -> None:
        """Register a repository in the graph"""
        from pathlib import Path

        if not repo_id:
            repo_id = Path(repo_path).name
        await self.backend.register_repository(
            repo_id=repo_id, name=repo_id, path=repo_path, metadata=metadata
        )

    async def list_repositories(self) -> list[dict[str, Any]]:
        """List all indexed repositories"""
        result = self.backend.conn.execute("""
            SELECT repo_id, name, path, indexed_at, metadata
            FROM repositories
        """).fetchall()

        repos = []
        for row in result:
            repos.append(
                {
                    "repo_id": row[0],
                    "name": row[1],
                    "path": row[2],
                    "indexed_at": row[3],
                    "metadata": row[4],
                }
            )

        return repos

    async def get_repository_stats(self, repo_id: str) -> dict[str, Any] | None:
        """Get statistics for a repository"""
        return await self.backend.get_repository_stats(repo_id)

    def close(self):
        """Close backend connection"""
        self.backend.close()


# Backward-compatible alias — many modules reference KnowledgeGraph
KnowledgeGraph = GraphAdapter
