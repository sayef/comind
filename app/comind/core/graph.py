"""
Knowledge Graph data models and interfaces

Core data models for symbols, relationships, and graph backend interface.
Graph operations are implemented using DuckDB backend.
"""

from __future__ import annotations

from abc import ABC, abstractmethod
from enum import StrEnum
from typing import Any

from pydantic import BaseModel, ConfigDict, Field


class SymbolType(StrEnum):
    """Types of symbols in the codebase"""
    FILE = "file"
    MODULE = "module"
    CLASS = "class"
    FUNCTION = "function"
    METHOD = "method"
    VARIABLE = "variable"
    IMPORT = "import"
    COMMUNITY = "community"
    PROCESS = "process"


class RelationType(StrEnum):
    """Types of relationships between symbols"""
    CONTAINS = "contains"
    IMPORTS = "imports"
    CALLS = "calls"
    INHERITS = "inherits"
    IMPLEMENTS = "implements"
    MEMBER_OF = "member_of"
    PARTICIPATES_IN = "participates_in"
    DEFINES = "defines"
    USES = "uses"


class Symbol(BaseModel):
    """Represents a symbol in the codebase"""
    model_config = ConfigDict(
        populate_by_name=True,
        frozen=False,
        validate_assignment=True,
        arbitrary_types_allowed=False,
        str_strip_whitespace=True,
    )
    
    id: str
    name: str
    type: SymbolType
    file_path: str
    line_start: int
    line_end: int
    repo_id: str | None = None  # Repository identifier for multi-repo support
    signature: str | None = None
    docstring: str | None = None
    properties: dict[str, Any] = Field(default_factory=dict)
    # Wiki integration for hybrid wiki-graph approach
    wiki_page_id: str | None = None  # ID of wiki page that documents this symbol
    wiki_section: str | None = None  # Section within the wiki page
    # LLM-generated per-node description (stored in graph, populated during analyze --wiki)
    description: str | None = None
    # Query associations: natural language queries this symbol answers
    associated_queries: list[str] = Field(default_factory=list)

    def to_dict(self) -> dict[str, Any]:
        """Convert to dictionary representation"""
        return {
            "id": self.id,
            "name": self.name,
            "type": self.type.value,
            "file_path": self.file_path,
            "line_start": self.line_start,
            "line_end": self.line_end,
            "repo_id": self.repo_id,
            "signature": self.signature,
            "docstring": self.docstring,
            "description": self.description,
            "properties": self.properties,
            "wiki_page_id": self.wiki_page_id,
            "wiki_section": self.wiki_section,
            "associated_queries": self.associated_queries,
        }


class Relationship(BaseModel):
    """Represents a relationship between symbols"""
    model_config = ConfigDict(
        frozen=False,
        validate_assignment=True,
    )
    
    source_id: str
    target_id: str
    type: RelationType
    confidence: float = Field(default=1.0, ge=0.0, le=1.0)
    properties: dict[str, Any] = Field(default_factory=dict)
    
    def to_dict(self) -> dict[str, Any]:
        """Convert to dictionary representation"""
        return {
            "source_id": self.source_id,
            "target_id": self.target_id,
            "type": self.type.value,
            "confidence": self.confidence,
            "properties": self.properties,
        }


class GraphBackend(ABC):
    """Abstract base class for graph backends"""
    
    @abstractmethod
    async def add_symbol(self, symbol: Symbol) -> None:
        """Add a symbol to the graph"""
        pass
    
    @abstractmethod
    async def add_relationship(self, relationship: Relationship) -> None:
        """Add a relationship to the graph"""
        pass
    
    @abstractmethod
    async def get_symbol(self, symbol_id: str) -> Symbol | None:
        """Get a symbol by ID"""
        pass
    
    @abstractmethod
    async def get_relationships(
        self,
        symbol_id: str,
        relation_type: RelationType | None = None,
        direction: str = "outgoing",
    ) -> list[Relationship]:
        """Get relationships for a symbol"""
        pass
    
    @abstractmethod
    async def query(self, query: str) -> list[dict[str, Any]]:
        """Execute a graph query"""
        pass
    
    @abstractmethod
    async def get_communities(self) -> list[dict[str, Any]]:
        """Get detected communities/clusters"""
        pass
    
    @abstractmethod
    async def get_processes(self) -> list[dict[str, Any]]:
        """Get detected execution flows"""
        pass


# Legacy rustworkx-based implementation has been removed.
# All graph operations now use DuckDB backend via GraphAdapter.
# See comind.storage.duckdb_backend and comind.storage.graph_adapter
