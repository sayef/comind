"""
CoMind - Collaborative code intelligence for AI agents

A minimalist implementation that helps AI agents understand codebases
through knowledge graphs, wiki-enhanced search, and rich context.
"""

__version__ = "0.1.0"
__author__ = "CoMind Team"

from comind.api.server import create_app
from comind.core.graph import Relationship, RelationType, Symbol, SymbolType
from comind.indexing.indexer import PythonIndexer
from comind.search.query_engine import WikiEnhancedQueryEngine
from comind.wiki.wiki_generator import WikiGenerator

__all__ = [
    "PythonIndexer",
    "RelationType",
    "Relationship",
    "Symbol",
    "SymbolType",
    "WikiEnhancedQueryEngine",
    "WikiGenerator",
    "create_app",
]
