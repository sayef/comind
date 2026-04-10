"""
Graph Wiki Generator - Generate LLM-enhanced documentation from knowledge graph

This module generates natural language documentation for:
1. Nodes (functions, classes, modules) - What they do and how they work
2. Relationships (calls, imports, etc) - Why interactions exist and what data flows

The LLM receives rich context from the graph including:
- Code snippets
- Caller/callee relationships with usage examples
- Community/module context
- Execution flow participation

This ensures documentation is:
- Grounded in actual code structure (can't hallucinate)
- Rich with natural language (matches user queries)
- Always accurate (regenerated from graph)
"""

import asyncio
import json
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Any

from comind.core.graph import GraphBackend, Relationship, Symbol, SymbolType
from comind.logging_config import get_logger
from comind.utils.snippet_extractor import CodeSnippetExtractor

logger = get_logger(__name__)


@dataclass
class NodeWiki:
    """Wiki page for a single node (symbol)"""

    node_id: str
    node_type: str
    name: str
    file_path: str
    signature: str
    natural_language: str  # LLM-generated description
    structured_data: dict[str, Any]  # Graph context

    def to_dict(self) -> dict:
        return asdict(self)


@dataclass
class RelationshipWiki:
    """Wiki page for a relationship (edge)"""

    relationship_id: str
    relationship_type: str
    source_id: str
    source_name: str
    target_id: str
    target_name: str
    natural_language: str  # LLM-generated description
    structured_data: dict[str, Any]  # Context about the relationship

    def to_dict(self) -> dict:
        return asdict(self)


class GraphWikiGenerator:
    """Generate LLM-enhanced wikis from knowledge graph"""

    def __init__(
        self,
        graph: GraphBackend,
        snippet_extractor: CodeSnippetExtractor,
        llm_client=None,
        output_dir: Path | None = None,
    ):
        self.graph = graph
        self.snippet_extractor = snippet_extractor
        self.llm = llm_client
        self.output_dir = output_dir or Path.home() / ".comind/data/graph_wikis"

        # Create output directories
        self.nodes_dir = self.output_dir / "nodes"
        self.relationships_dir = self.output_dir / "relationships"
        self.nodes_dir.mkdir(parents=True, exist_ok=True)
        self.relationships_dir.mkdir(parents=True, exist_ok=True)

    async def generate_all_wikis(self, repo_id: str, batch_size: int = 10) -> dict[str, int]:
        """
        Generate wikis for all nodes and relationships in the graph

        Args:
            repo_id: Repository identifier
            batch_size: Number of wikis to generate in parallel

        Returns:
            Statistics about generation (nodes_generated, relationships_generated)
        """
        logger.info("Starting graph wiki generation for repo: %s", repo_id)

        # Get all symbols from graph
        symbols = await self.graph.get_all_symbols(repo_id)
        logger.info("Found %d symbols to document", len(symbols))

        # Generate node wikis in batches
        node_count = 0
        for i in range(0, len(symbols), batch_size):
            batch = symbols[i : i + batch_size]
            tasks = [self.generate_node_wiki(symbol) for symbol in batch]
            node_wikis = await asyncio.gather(*tasks, return_exceptions=True)

            # Save successful wikis
            for wiki in node_wikis:
                if isinstance(wiki, NodeWiki):
                    await self.save_node_wiki(wiki)
                    node_count += 1
                elif isinstance(wiki, Exception):
                    logger.error("Failed to generate node wiki: %s", wiki)

            logger.info("Generated %d/%d node wikis", node_count, len(symbols))

        # Get all relationships
        relationships = await self.graph.get_all_relationships(repo_id)
        logger.info("Found %d relationships to document", len(relationships))

        # Generate relationship wikis in batches
        rel_count = 0
        for i in range(0, len(relationships), batch_size):
            batch = relationships[i : i + batch_size]
            rel_tasks: list[Any] = [self.generate_relationship_wiki(rel) for rel in batch]  # type: ignore[assignment]
            rel_wikis = await asyncio.gather(*rel_tasks, return_exceptions=True)

            # Save successful wikis
            for wiki in rel_wikis:
                if isinstance(wiki, RelationshipWiki):
                    await self.save_relationship_wiki(wiki)
                    rel_count += 1
                elif isinstance(wiki, Exception):
                    logger.error("Failed to generate relationship wiki: %s", wiki)

            logger.info("Generated %d/%d relationship wikis", rel_count, len(relationships))

        logger.info(
            "Graph wiki generation complete: %d nodes, %d relationships", node_count, rel_count
        )

        return {
            "nodes_generated": node_count,
            "relationships_generated": rel_count,
            "total": node_count + rel_count,
        }

    async def annotate_graph_descriptions(
        self,
        repo_id: str,
        batch_size: int = 20,
        skip_types: tuple = ("import",),
    ) -> int:
        """Generate short per-node descriptions and store them in the graph.

        Uses the LLM when available; falls back to the symbol's docstring
        (or a bare "{type} {name}" string).  The description is written back
        into ``symbol.description`` on every Symbol in the graph so it is
        persisted the next time the graph is saved to disk.

        Returns the number of symbols annotated.
        """
        symbols = await self.graph.get_all_symbols(repo_id)
        # Skip trivial node types that don't need prose descriptions
        symbols = [s for s in symbols if s.type.value not in skip_types]

        annotated = 0
        for i in range(0, len(symbols), batch_size):
            batch = symbols[i : i + batch_size]

            async def _describe(sym: Symbol) -> tuple[str, str]:
                if self.llm:
                    context = await self._extract_node_context(sym)
                    text = await self._generate_node_description(sym, context)
                else:
                    text = sym.docstring or f"{sym.type.value} {sym.name}"
                return sym.id, text

            results = await asyncio.gather(*[_describe(s) for s in batch], return_exceptions=True)
            for res in results:
                if isinstance(res, Exception):
                    logger.warning("Failed to annotate symbol: %s", res)
                    continue
                sym_id, description = res  # type: ignore[misc]
                await self.graph.update_symbol_description(sym_id, description)
                annotated += 1

        return annotated

    async def generate_node_wiki(self, symbol: Symbol) -> NodeWiki:
        """Generate wiki for a single node (function/class/module)"""
        logger.debug("Generating node wiki for: %s (%s)", symbol.name, symbol.type.value)

        # Extract rich context from graph
        context = await self._extract_node_context(symbol)

        # Generate natural language description using LLM
        if self.llm:
            natural_language = await self._generate_node_description(symbol, context)
        else:
            # Fallback: Use docstring or basic description
            natural_language = symbol.docstring or f"{symbol.type.value} {symbol.name}"

        return NodeWiki(
            node_id=symbol.id,
            node_type=symbol.type.value,
            name=symbol.name,
            file_path=symbol.file_path,
            signature=symbol.signature or "",
            natural_language=natural_language,
            structured_data=context,
        )

    async def generate_relationship_wiki(self, rel: Relationship) -> RelationshipWiki:
        """Generate wiki for a relationship (edge)"""
        logger.debug(
            "Generating relationship wiki: %s (%s -> %s)", rel.type, rel.source_id, rel.target_id
        )

        # Get source and target symbols
        source = await self.graph.get_symbol(rel.source_id)
        target = await self.graph.get_symbol(rel.target_id)

        if not source or not target:
            raise ValueError("Cannot generate wiki for relationship with missing symbols")

        # Generate relationship ID (Relationship model doesn't have id field)
        rel_id = f"{rel.source_id}_{rel.type}_{rel.target_id}".replace("/", "_").replace(":", "_")

        # Extract relationship context
        context = await self._extract_relationship_context(rel, source, target)

        # Generate natural language description using LLM
        if self.llm:
            natural_language = await self._generate_relationship_description(
                rel, source, target, context
            )
        else:
            # Fallback: Basic description
            natural_language = f"{source.name} {rel.type.lower()} {target.name}"

        return RelationshipWiki(
            relationship_id=rel_id,
            relationship_type=rel.type,
            source_id=source.id,
            source_name=source.name,
            target_id=target.id,
            target_name=target.name,
            natural_language=natural_language,
            structured_data=context,
        )

    async def _extract_node_context(self, symbol: Symbol) -> dict[str, Any]:
        """Extract rich context from graph for a node"""
        context = {
            "signature": symbol.signature or "",
            "docstring": symbol.docstring or "",
            "type": symbol.type.value,
            "file_path": symbol.file_path,
            "line_range": f"{symbol.line_start}-{symbol.line_end}",
        }

        # Get code snippet — extractors return dicts; pull out the code string
        try:
            if symbol.type in {SymbolType.FUNCTION, SymbolType.METHOD}:
                code = await self.snippet_extractor.extract_function_snippet(symbol)
            elif symbol.type == SymbolType.CLASS:
                code = await self.snippet_extractor.extract_class_snippet(symbol)
            else:
                code = await self.snippet_extractor.extract_snippet(symbol)
            if isinstance(code, dict):
                code = code.get("code", "")
            context["code"] = code or ""
        except Exception as e:
            logger.warning("Failed to extract code snippet for %s: %s", symbol.name, e)
            context["code"] = ""

        # Get callers with usage examples
        try:
            callers = await self.graph.get_callers(symbol.id)
            context["callers"] = []
            for caller in callers[:5]:
                # extract_usage_examples(symbol) — no caller argument
                usage_list = await self.snippet_extractor.extract_usage_examples(symbol)
                first = usage_list[0] if usage_list else {}
                context["callers"].append(
                    {
                        "name": caller.name,
                        "file": caller.file_path,
                        "usage": first.get("line", "") if isinstance(first, dict) else "",
                    }
                )
        except Exception as e:
            logger.warning("Failed to get callers for %s: %s", symbol.name, e)
            context["callers"] = []

        # Get callees
        try:
            callees = await self.graph.get_callees(symbol.id)
            context["callees"] = [
                {"name": c.name, "file": c.file_path, "type": c.type.value}
                for c in callees[:10]  # Limit to top 10
            ]
        except Exception as e:
            logger.warning("Failed to get callees for %s: %s", symbol.name, e)
            context["callees"] = []

        # Get community/module context
        try:
            community = await self.graph.get_community(symbol.id)
            context["community"] = community.get("name") if community else None
        except Exception as e:
            logger.warning("Failed to get community for %s: %s", symbol.name, e)
            context["community"] = None

        # Get process participation
        try:
            processes = await self.graph.get_symbol_processes(symbol.id)
            context["processes"] = [p.get("name") for p in processes[:3]]
        except Exception as e:
            logger.warning("Failed to get processes for %s: %s", symbol.name, e)
            context["processes"] = []

        # Add properties
        context["properties"] = symbol.properties or {}

        return context

    async def _extract_relationship_context(
        self, rel: Relationship, source: Symbol, target: Symbol
    ) -> dict[str, Any]:
        """Extract context for a relationship"""
        context = {
            "relationship_type": rel.type,
            "confidence": rel.confidence,
            "source": {"name": source.name, "type": source.type.value, "file": source.file_path},
            "target": {"name": target.name, "type": target.type.value, "file": target.file_path},
        }

        # For CALLS relationships, get the call site
        if rel.type == "CALLS":
            try:
                usage = await self.snippet_extractor.extract_usage_examples(target, source)
                context["call_site"] = usage[:300] if usage else ""
            except Exception as e:
                logger.warning("Failed to extract call site: %s", e)
                context["call_site"] = ""

        # Add relationship properties
        context["properties"] = rel.properties or {}

        return context

    async def _generate_node_description(self, symbol: Symbol, context: dict) -> str:
        """Generate natural language description for a node using LLM"""
        prompt = self._build_node_prompt(symbol, context)

        try:
            response = await self.llm.generate(prompt)
            return response.strip()
        except Exception:
            logger.exception("LLM generation failed for %s", symbol.name)
            return symbol.docstring or f"{symbol.type.value} {symbol.name}"

    async def _generate_relationship_description(
        self, rel: Relationship, source: Symbol, target: Symbol, context: dict
    ) -> str:
        """Generate natural language description for a relationship using LLM"""
        prompt = self._build_relationship_prompt(rel, source, target, context)

        try:
            response = await self.llm.generate(prompt)
            return response.strip()
        except Exception:
            logger.exception("LLM generation failed for relationship")
            return f"{source.name} {rel.type.lower()} {target.name}"

    def _build_node_prompt(self, symbol: Symbol, context: dict) -> str:
        """Build LLM prompt for node documentation"""
        callers_text = (
            "\n".join(
                [
                    f"  - {c['name']} in {c['file']}\n    Usage: {c['usage']}"
                    for c in context.get("callers", [])[:3]
                ]
            )
            or "  None"
        )

        callees_text = (
            "\n".join(
                [
                    f"  - {c['name']} ({c['type']}) in {c['file']}"
                    for c in context.get("callees", [])[:5]
                ]
            )
            or "  None"
        )

        processes_text = ", ".join(context.get("processes", [])) or "None"

        return f"""Generate clear, concise technical documentation for this code symbol.

SYMBOL INFORMATION:
Name: {symbol.name}
Type: {symbol.type.value}
Location: {context["file_path"]}:{context["line_range"]}

SIGNATURE:
```python
{context["signature"]}
```

CODE IMPLEMENTATION:
```python
{context["code"][:500]}{"..." if len(str(context["code"])) > 500 else ""}
```

DOCSTRING:
{context["docstring"] or "None"}

RELATIONSHIPS:
Called by ({len(context.get("callers", []))} function(s)):
{callers_text}

Calls ({len(context.get("callees", []))} function(s)):
{callees_text}

CONTEXT:
Module/Community: {context.get("community") or "Unknown"}
Used in execution flows: {processes_text}

TASK:
Generate natural language documentation with these sections:
1. **Purpose** - What does this do and why does it exist? (2-3 sentences)
2. **How It Works** - Explain the key implementation logic (2-3 sentences)
3. **Usage Context** - When and how is this used in the codebase? (1-2 sentences)
4. **Important Notes** - Any gotchas, security considerations, or best practices (1-2 sentences, optional)

Write in clear, professional language. Focus on understanding, not just describing. Be concise."""

    def _build_relationship_prompt(
        self, rel: Relationship, source: Symbol, target: Symbol, context: dict
    ) -> str:
        """Build LLM prompt for relationship documentation"""
        call_site = context.get("call_site", "")

        return f"""Generate clear documentation for this code relationship.

RELATIONSHIP: {source.name} {rel.type} {target.name}

SOURCE:
Name: {source.name}
Type: {source.type.value}
File: {source.file_path}

TARGET:
Name: {target.name}
Type: {target.type.value}
File: {target.file_path}

CALL SITE (where this happens):
```python
{call_site}
```

TASK:
Generate a 2-3 sentence explanation covering:
1. Why this relationship exists - What purpose does this interaction serve?
2. What happens - What data flows or what functionality is accessed?
3. Context - When/where in the execution flow does this occur?

Be concise and focus on the "why" and "what" of the interaction."""

    async def save_node_wiki(self, wiki: NodeWiki):
        """Save node wiki to disk"""
        file_path = self.nodes_dir / f"{wiki.node_id}.json"
        content = json.dumps(wiki.to_dict(), indent=2)
        await asyncio.to_thread(file_path.write_text, content, encoding="utf-8")

    async def save_relationship_wiki(self, wiki: RelationshipWiki):
        """Save relationship wiki to disk"""
        file_path = self.relationships_dir / f"{wiki.relationship_id}.json"
        content = json.dumps(wiki.to_dict(), indent=2)
        await asyncio.to_thread(file_path.write_text, content, encoding="utf-8")

    async def load_node_wiki(self, node_id: str) -> NodeWiki | None:
        """Load node wiki from disk"""
        file_path = self.nodes_dir / f"{node_id}.json"
        if not file_path.exists():
            return None

        data = await asyncio.to_thread(file_path.read_text, encoding="utf-8")
        return NodeWiki(**json.loads(data))

    async def load_relationship_wiki(self, rel_id: str) -> RelationshipWiki | None:
        """Load relationship wiki from disk"""
        file_path = self.relationships_dir / f"{rel_id}.json"
        if not file_path.exists():
            return None

        data = await asyncio.to_thread(file_path.read_text, encoding="utf-8")
        return RelationshipWiki(**json.loads(data))
