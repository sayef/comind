"""
Style Guide Extraction System

Analyzes repository patterns and generates comprehensive style guides
based on statistical analysis of code patterns, conventions, and practices.
"""

from __future__ import annotations

import re
from collections import Counter, defaultdict
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from comind.core.graph import Symbol
from comind.storage.graph_adapter import KnowledgeGraph
from comind.logging_config import get_logger

logger = get_logger(__name__)


@dataclass
class PatternStats:
    """Statistics for a detected pattern"""
    count: int = 0
    total: int = 0
    examples: list[str] = field(default_factory=list)
    files: set[str] = field(default_factory=set)
    
    @property
    def prevalence(self) -> str:
        """Calculate prevalence level"""
        if self.total == 0:
            return "Unknown"
        ratio = self.count / self.total
        if ratio >= 0.7:
            return "High"
        elif ratio >= 0.3:
            return "Medium"
        else:
            return "Low"
    
    @property
    def percentage(self) -> float:
        """Calculate percentage"""
        if self.total == 0:
            return 0.0
        return (self.count / self.total) * 100


@dataclass
class StylePatterns:
    """Collected style patterns from repository analysis"""
    
    # Environment & Tooling
    python_version: str = "Unknown"
    package_manager: str = "Unknown"
    has_lockfile: bool = False
    max_line_length: int = 88
    
    # Typing
    type_hints_usage: PatternStats = field(default_factory=PatternStats)
    return_type_usage: PatternStats = field(default_factory=PatternStats)
    advanced_typing: dict[str, PatternStats] = field(default_factory=dict)
    
    # Naming Conventions
    function_naming: Counter = field(default_factory=Counter)
    class_naming: Counter = field(default_factory=Counter)
    constant_naming: Counter = field(default_factory=Counter)
    private_naming: Counter = field(default_factory=Counter)
    
    # Documentation
    docstring_format: str = "Unknown"
    docstring_coverage: PatternStats = field(default_factory=PatternStats)
    
    # Error Handling
    exception_patterns: list[str] = field(default_factory=list)
    error_handling_style: Counter = field(default_factory=Counter)
    
    # Logging
    logger_init_pattern: str = "Unknown"
    structured_logging: bool = False
    
    # Async Usage
    async_usage: PatternStats = field(default_factory=PatternStats)
    
    # Imports
    import_style: Counter = field(default_factory=Counter)
    common_imports: Counter = field(default_factory=Counter)
    
    # Micro-idioms
    string_formatting: Counter = field(default_factory=Counter)
    comprehension_usage: PatternStats = field(default_factory=PatternStats)
    context_manager_usage: PatternStats = field(default_factory=PatternStats)


class StyleExtractor:
    """Extract coding style patterns from repository"""
    
    def __init__(self, graph: KnowledgeGraph):
        self.graph = graph
        self.patterns = StylePatterns()
    
    async def analyze_repository(self, repo_id: str) -> StylePatterns:
        """Analyze repository and extract style patterns"""
        logger.info("Starting style guide extraction", repo_id=repo_id)
        
        # Get all symbols for the repository
        symbols = await self._get_repo_symbols(repo_id)
        
        if not symbols:
            logger.warning("No symbols found for repository", repo_id=repo_id)
            return self.patterns
        
        logger.info("Analyzing symbols", count=len(symbols))
        
        # Analyze different aspects
        await self._analyze_typing(symbols)
        await self._analyze_naming(symbols)
        await self._analyze_documentation(symbols)
        await self._analyze_async_usage(symbols)
        await self._analyze_imports(symbols)
        await self._detect_tooling(symbols)
        
        logger.info("Style guide extraction complete")
        return self.patterns
    
    async def _get_repo_symbols(self, repo_id: str) -> list[Symbol]:
        """Get all symbols for a repository"""
        # Get all symbols from DuckDB backend
        all_symbols = await self.graph.get_all_symbols()
        
        # Filter by repo_id
        symbols = [s for s in all_symbols if s.repo_id == repo_id]
        
        return symbols
    
    async def _analyze_typing(self, symbols: list[Symbol]):
        """Analyze type hint usage patterns"""
        functions = [s for s in symbols if s.type in ("function", "method")]
        
        if not functions:
            return
        
        typed_functions = 0
        return_typed = 0
        
        # Track advanced typing usage
        advanced_patterns = {
            "TypeVar": PatternStats(),
            "Protocol": PatternStats(),
            "Generic": PatternStats(),
            "Callable": PatternStats(),
            "Union": PatternStats(),
            "Optional": PatternStats(),
        }
        
        for func in functions:
            signature = func.signature or ""
            
            # Check for type hints in parameters
            if "->" in signature or ":" in signature:
                typed_functions += 1
            
            # Check for return type
            if "->" in signature:
                return_typed += 1
            
            # Check for advanced typing
            for pattern_name in advanced_patterns:
                if pattern_name in signature:
                    advanced_patterns[pattern_name].count += 1
                    advanced_patterns[pattern_name].examples.append(signature[:100])
                advanced_patterns[pattern_name].total += 1
        
        self.patterns.type_hints_usage = PatternStats(
            count=typed_functions,
            total=len(functions)
        )
        
        self.patterns.return_type_usage = PatternStats(
            count=return_typed,
            total=len(functions)
        )
        
        self.patterns.advanced_typing = advanced_patterns
    
    async def _analyze_naming(self, symbols: list[Symbol]):
        """Analyze naming conventions"""
        for symbol in symbols:
            name = symbol.name
            
            if symbol.type == "function":
                # Detect naming pattern
                if name.islower() and "_" in name:
                    self.patterns.function_naming["snake_case"] += 1
                elif name[0].islower() and name[0].isalpha():
                    self.patterns.function_naming["camelCase"] += 1
                
                # Private function detection
                if name.startswith("_") and not name.startswith("__"):
                    self.patterns.private_naming["single_underscore"] += 1
                elif name.startswith("__"):
                    self.patterns.private_naming["double_underscore"] += 1
            
            elif symbol.type == "class":
                # Class naming
                if name[0].isupper():
                    self.patterns.class_naming["PascalCase"] += 1
            
            elif symbol.type == "variable":
                # Constant detection (ALL_CAPS)
                if name.isupper() and "_" in name:
                    self.patterns.constant_naming["UPPER_SNAKE_CASE"] += 1
    
    async def _analyze_documentation(self, symbols: list[Symbol]):
        """Analyze documentation patterns"""
        functions = [s for s in symbols if s.type in ("function", "method")]
        
        if not functions:
            return
        
        documented = 0
        docstring_styles = Counter()
        
        for func in functions:
            # Check if function has docstring
            if func.docstring:
                documented += 1
                
                # Detect docstring style (Google, NumPy, reST)
                docstring = func.docstring
                if "Args:" in docstring or "Returns:" in docstring:
                    docstring_styles["Google"] += 1
                elif "Parameters" in docstring:
                    docstring_styles["NumPy"] += 1
                elif ":param" in docstring or ":return:" in docstring:
                    docstring_styles["reST"] += 1
        
        self.patterns.docstring_coverage = PatternStats(
            count=documented,
            total=len(functions)
        )
        
        if docstring_styles:
            self.patterns.docstring_format = docstring_styles.most_common(1)[0][0]
    
    async def _analyze_async_usage(self, symbols: list[Symbol]):
        """Analyze async/await usage"""
        functions = [s for s in symbols if s.type in ("function", "method")]
        
        if not functions:
            return
        
        async_count = 0
        
        for func in functions:
            signature = func.signature or ""
            if signature.startswith("async ") or "async def" in signature:
                async_count += 1
                self.patterns.async_usage.examples.append(func.name)
        
        self.patterns.async_usage = PatternStats(
            count=async_count,
            total=len(functions)
        )
    
    async def _analyze_imports(self, symbols: list[Symbol]):
        """Analyze import patterns"""
        imports = [s for s in symbols if s.type == "import"]
        
        for imp in imports:
            name = imp.name
            
            # Track common imports
            self.patterns.common_imports[name] += 1
            
            # Detect import style
            signature = imp.signature or ""
            if signature.startswith("from "):
                self.patterns.import_style["from_import"] += 1
            elif signature.startswith("import "):
                self.patterns.import_style["direct_import"] += 1
    
    async def _detect_tooling(self, symbols: list[Symbol]):
        """Detect tooling from imports and file patterns"""
        import_names = [s.name for s in symbols if s.type == "import"]
        
        # Detect package manager
        if "poetry" in import_names:
            self.patterns.package_manager = "Poetry"
        elif "pipenv" in import_names:
            self.patterns.package_manager = "Pipenv"
        
        # Detect logging pattern
        if "structlog" in import_names:
            self.patterns.logger_init_pattern = "structlog.get_logger(__name__)"
            self.patterns.structured_logging = True
        elif "logging" in import_names:
            self.patterns.logger_init_pattern = "logging.getLogger(__name__)"
        
        # Detect typing usage
        if "typing" in import_names:
            self.patterns.type_hints_usage.count += 1


async def extract_style_guide(graph: KnowledgeGraph, repo_id: str) -> StylePatterns:
    """Main entry point for style guide extraction"""
    extractor = StyleExtractor(graph)
    patterns = await extractor.analyze_repository(repo_id)
    return patterns
