"""
Code snippet extractor for GitNexus Python

Extracts code snippets with context and structural information
for use in query results and documentation.
"""

import re
from pathlib import Path
from typing import Any

from tree_sitter import Language, Parser
from tree_sitter_python import language

from comind.core.graph import Symbol, SymbolType


class CodeSnippetExtractor:
    """Extract code snippets with context and structure analysis"""

    def __init__(self, repo_root: Path | None = None):
        """
        Initialize snippet extractor

        Args:
            repo_root: Root directory of the repository (needed to resolve relative paths)
        """
        self.max_context_lines = 0
        self.max_snippet_lines = 50
        self.repo_root = repo_root

        # Initialize tree-sitter parser
        self.parser = Parser(Language(language()))
        self.python_language = Language(language())

    def _resolve_path(self, file_path: str) -> Path:
        """Resolve file path to absolute path"""
        path = Path(file_path)

        # If already absolute, return as-is
        if path.is_absolute():
            return path

        # If relative and we have repo_root, resolve it
        if self.repo_root:
            return self.repo_root / path

        # Otherwise, assume it's relative to current directory
        return path

    async def extract_snippet(self, symbol: Symbol, context_lines: int = None) -> dict[str, Any]:
        """Extract code snippet for a symbol with context"""
        if context_lines is None:
            context_lines = self.max_context_lines

        # Resolve file path to absolute
        file_path = self._resolve_path(symbol.file_path)

        try:
            with open(file_path, encoding="utf-8") as f:
                lines = f.readlines()
        except (FileNotFoundError, OSError):
            return {
                "error": f"File not found: {symbol.file_path}",
                "code": "",
                "file_path": symbol.file_path,
            }

        # Calculate snippet boundaries (start exactly at symbol, no pre-context)
        start_line = symbol.line_start - 1  # 0-indexed
        end_line = min(len(lines), symbol.line_end + context_lines)

        # Limit total snippet size
        if end_line - start_line > self.max_snippet_lines:
            symbol_lines = symbol.line_end - symbol.line_start + 1
            if symbol_lines >= self.max_snippet_lines:
                # Symbol is larger than max — show from the beginning of the symbol
                start_line = symbol.line_start - 1  # 0-indexed
                end_line = min(len(lines), start_line + self.max_snippet_lines)
            else:
                available_context = self.max_snippet_lines - symbol_lines
                start_line = max(0, symbol.line_start - available_context // 2 - 1)
                end_line = min(len(lines), symbol.line_end + available_context // 2)

        # Extract snippet
        snippet_lines = lines[start_line:end_line]
        snippet_code = "".join(snippet_lines)

        # Find symbol boundaries within snippet
        snippet_start_abs = start_line + 1  # Convert to 1-based
        symbol_start_in_snippet = symbol.line_start - snippet_start_abs + 1
        symbol_end_in_snippet = symbol.line_end - snippet_start_abs + 1

        return {
            "code": snippet_code,
            "file_path": symbol.file_path,
            "absolute_lines": {"start": start_line + 1, "end": end_line},
            "symbol_lines": {
                "start": symbol.line_start,
                "end": symbol.line_end,
                "start_in_snippet": symbol_start_in_snippet,
                "end_in_snippet": symbol_end_in_snippet,
            },
            "language": "python",
            "total_lines": len(snippet_lines),
            "symbol_type": symbol.type.value,
            "symbol_name": symbol.name,
        }

    async def extract_function_snippet(self, symbol: Symbol) -> dict[str, Any]:
        """Extract function/method snippet with enhanced structure"""
        base_snippet = await self.extract_snippet(symbol)

        if "error" in base_snippet:
            return base_snippet

        # Parse the snippet to extract structure using tree-sitter
        tree = self.parser.parse(bytes(base_snippet["code"], "utf8"))
        structure = self._analyze_function_structure_ts(tree.root_node, symbol)

        return {**base_snippet, "structure": structure, "type": "function"}

    async def extract_class_snippet(self, symbol: Symbol) -> dict[str, Any]:
        """Extract class snippet with enhanced structure"""
        base_snippet = await self.extract_snippet(symbol)

        if "error" in base_snippet:
            return base_snippet

        # Parse the snippet to extract structure using tree-sitter
        tree = self.parser.parse(bytes(base_snippet["code"], "utf8"))
        structure = self._analyze_class_structure_ts(tree.root_node, symbol)

        return {**base_snippet, "structure": structure, "type": "class"}

    async def extract_module_snippet(self, symbol: Symbol, max_lines: int = 100) -> dict[str, Any]:
        """Extract module snippet (typically the beginning of the file)"""
        # Resolve file path to absolute
        file_path = self._resolve_path(symbol.file_path)

        try:
            with open(file_path, encoding="utf-8") as f:
                lines = f.readlines()
        except (FileNotFoundError, OSError):
            return {
                "error": f"File not found: {symbol.file_path}",
                "code": "",
                "file_path": symbol.file_path,
            }

        # Take the first max_lines lines
        snippet_lines = lines[:max_lines]
        snippet_code = "".join(snippet_lines)

        # Parse module structure using tree-sitter
        tree = self.parser.parse(bytes(snippet_code, "utf8"))
        structure = self._analyze_module_structure_ts(tree.root_node)

        return {
            "code": snippet_code,
            "file_path": symbol.file_path,
            "lines": {"start": 1, "end": len(snippet_lines)},
            "language": "python",
            "total_lines": len(snippet_lines),
            "structure": structure,
            "type": "module",
        }

    async def extract_usage_examples(
        self, symbol: Symbol, max_examples: int = 3
    ) -> list[dict[str, Any]]:
        """Find usage examples of a symbol in the same file"""
        # Resolve file path to absolute
        file_path = self._resolve_path(symbol.file_path)

        try:
            with open(file_path, encoding="utf-8") as f:
                content = f.read()
                lines = content.split("\n")
        except (FileNotFoundError, OSError):
            return []

        examples = []

        # Look for usage patterns based on symbol type
        if symbol.type in [SymbolType.FUNCTION, SymbolType.METHOD]:
            examples = self._find_function_usages(symbol, lines, max_examples)
        elif symbol.type == SymbolType.CLASS:
            examples = self._find_class_usages(symbol, lines, max_examples)
        elif symbol.type == SymbolType.VARIABLE:
            examples = self._find_variable_usages(symbol, lines, max_examples)

        return examples

    def _analyze_function_structure_ts(self, node, symbol: Symbol) -> dict[str, Any]:
        """Analyze function structure using tree-sitter"""
        # Simplified structure - just return basic info
        return {
            "name": symbol.name,
            "signature": symbol.signature or "",
            "docstring": symbol.docstring or None,
            "type": "function",
        }

    def _analyze_class_structure_ts(self, node, symbol: Symbol) -> dict[str, Any]:
        """Analyze class structure using tree-sitter"""
        # Simplified structure - just return basic info
        return {
            "name": symbol.name,
            "docstring": symbol.docstring or None,
            "bases": symbol.properties.get("bases", []) if symbol.properties else [],
            "type": "class",
        }

    def _analyze_module_structure_ts(self, node) -> dict[str, Any]:
        """Analyze module structure using tree-sitter"""
        # Simplified structure - just return basic info
        return {"type": "module", "has_content": True}

    def _find_function_usages(
        self, symbol: Symbol, lines: list[str], max_examples: int
    ) -> list[dict[str, Any]]:
        """Find usage examples of a function"""
        examples = []
        pattern = re.compile(r"\b" + re.escape(symbol.name) + r"\s*\(")

        for i, line in enumerate(lines):
            if pattern.search(line) and i + 1 != symbol.line_start:  # Exclude definition
                # Extract context around usage
                start = max(0, i - 2)
                end = min(len(lines), i + 3)
                context_lines = lines[start:end]

                examples.append(
                    {
                        "line_number": i + 1,
                        "line": line.strip(),
                        "context": "".join(context_lines),
                        "context_start": start + 1,
                        "context_end": end,
                    }
                )

                if len(examples) >= max_examples:
                    break

        return examples

    def _find_class_usages(
        self, symbol: Symbol, lines: list[str], max_examples: int
    ) -> list[dict[str, Any]]:
        """Find usage examples of a class"""
        examples = []
        patterns = [
            re.compile(r"\b" + re.escape(symbol.name) + r"\s*\("),  # Instantiation
            re.compile(r"\b" + re.escape(symbol.name) + r"\s*\."),  # Method/property access
            re.compile(r":\s*" + re.escape(symbol.name) + r"\s*$"),  # Type annotation
        ]

        for i, line in enumerate(lines):
            for pattern in patterns:
                if pattern.search(line) and i + 1 != symbol.line_start:
                    start = max(0, i - 2)
                    end = min(len(lines), i + 3)
                    context_lines = lines[start:end]

                    examples.append(
                        {
                            "line_number": i + 1,
                            "line": line.strip(),
                            "context": "".join(context_lines),
                            "context_start": start + 1,
                            "context_end": end,
                            "usage_type": "instantiation" if "(" in line else "access",
                        }
                    )

                    if len(examples) >= max_examples:
                        break

            if len(examples) >= max_examples:
                break

        return examples

    def _find_variable_usages(
        self, symbol: Symbol, lines: list[str], max_examples: int
    ) -> list[dict[str, Any]]:
        """Find usage examples of a variable"""
        examples = []
        pattern = re.compile(r"\b" + re.escape(symbol.name) + r"\b")

        for i, line in enumerate(lines):
            if pattern.search(line) and i + 1 != symbol.line_start:
                # Skip assignment lines (definition)
                if "=" in line and line.split("=")[0].strip().endswith(symbol.name):
                    continue

                start = max(0, i - 2)
                end = min(len(lines), i + 3)
                context_lines = lines[start:end]

                examples.append(
                    {
                        "line_number": i + 1,
                        "line": line.strip(),
                        "context": "".join(context_lines),
                        "context_start": start + 1,
                        "context_end": end,
                    }
                )

                if len(examples) >= max_examples:
                    break

        return examples

    # Helper methods for tree-sitter
    def _get_node_text(self, node) -> str:
        """Get text content of a tree-sitter node"""
        return node.text.decode("utf8") if node else ""
