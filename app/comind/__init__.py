"""
CoMind - Collaborative code intelligence for AI agents

A minimalist implementation that helps AI agents understand codebases
through knowledge graphs, wiki-enhanced search, and rich context.
"""

__version__ = "0.1.0"
__author__ = "CoMind Team"

from comind.core.graph import Symbol, Relationship, SymbolType, RelationType
from comind.indexing.indexer import PythonIndexer
from comind.search.query_engine import WikiEnhancedQueryEngine
from comind.wiki.wiki_generator import WikiGenerator
from comind.api.server import create_app

__all__ = [
    "Symbol",
    "Relationship",
    "SymbolType",
    "RelationType",
    "PythonIndexer", 
    "WikiEnhancedQueryEngine",
    "WikiGenerator",
    "create_app",
]
