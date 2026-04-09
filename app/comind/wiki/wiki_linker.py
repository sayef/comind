"""
Wiki-Graph Linker

Post-processes generated wiki pages to create bidirectional links between
wiki documentation and knowledge graph symbols.

Strategy:
1. Parse wiki markdown to extract symbol mentions (functions, classes, etc.)
2. Match mentions to graph symbols using fuzzy matching
3. Update symbols with wiki_page_id and wiki_section
4. Track mentioned_symbols per wiki page
"""

import re
from pathlib import Path
from typing import Dict, List, Set, Tuple, Optional
from dataclasses import dataclass

from comind.core.graph import Symbol, SymbolType
from comind.storage.graph_adapter import KnowledgeGraph


@dataclass
class SymbolMention:
    """Represents a symbol mention in wiki text"""
    name: str
    context: str  # Surrounding text for disambiguation
    line_number: int
    section: str  # Wiki section (e.g., "## Authentication")


class WikiGraphLinker:
    """Links wiki pages with knowledge graph symbols"""
    
    def __init__(self, graph: KnowledgeGraph, wiki_dir: Path):
        self.graph = graph
        self.wiki_dir = wiki_dir
        
        # Patterns to detect symbol mentions in markdown
        self.patterns = {
            'function': re.compile(r'`([a-zA-Z_][a-zA-Z0-9_]*)\(\)`'),  # `function()`
            'class': re.compile(r'`([A-Z][a-zA-Z0-9_]*)`'),  # `ClassName`
            'code_block': re.compile(r'```python\n(.*?)\n```', re.DOTALL),  # Code blocks
            'section': re.compile(r'^#{1,6}\s+(.+)$', re.MULTILINE),  # Headers
        }
    
    async def link_all_pages(self) -> Dict[str, List[str]]:
        """
        Process all wiki pages and create bidirectional links.
        
        Returns:
            Dict mapping page_id -> list of symbol_ids
        """
        page_symbols = {}
        
        # Process each wiki page
        for wiki_file in self.wiki_dir.glob("*.md"):
            page_id = wiki_file.stem
            content = wiki_file.read_text()
            
            # Extract symbol mentions
            mentions = self._extract_mentions(content)
            
            # Match mentions to graph symbols
            matched_symbols = await self._match_symbols(mentions, page_id)
            
            # Update symbols with wiki references
            await self._update_symbols(matched_symbols, page_id)
            
            # Track symbols per page
            page_symbols[page_id] = [s.id for s in matched_symbols]
        
        return page_symbols
    
    def _extract_mentions(self, content: str) -> List[SymbolMention]:
        """Extract symbol mentions from wiki markdown"""
        mentions = []
        lines = content.split('\n')
        current_section = ""
        
        for line_num, line in enumerate(lines, 1):
            # Track current section
            section_match = self.patterns['section'].match(line)
            if section_match:
                current_section = section_match.group(1)
                continue
            
            # Extract function mentions: `function()`
            for match in self.patterns['function'].finditer(line):
                mentions.append(SymbolMention(
                    name=match.group(1),
                    context=line.strip(),
                    line_number=line_num,
                    section=current_section
                ))
            
            # Extract class mentions: `ClassName`
            for match in self.patterns['class'].finditer(line):
                name = match.group(1)
                # Skip common words that look like classes
                if name not in {'The', 'This', 'That', 'When', 'Where', 'What', 'Which'}:
                    mentions.append(SymbolMention(
                        name=name,
                        context=line.strip(),
                        line_number=line_num,
                        section=current_section
                    ))
        
        return mentions
    
    async def _match_symbols(
        self, 
        mentions: List[SymbolMention], 
        page_id: str
    ) -> List[Symbol]:
        """Match symbol mentions to graph symbols"""
        matched = []
        seen_ids = set()
        
        for mention in mentions:
            # Try exact name match first
            candidates = await self._find_symbol_candidates(mention.name)
            
            if not candidates:
                continue
            
            # If multiple candidates, use context for disambiguation
            symbol = self._disambiguate(candidates, mention)
            
            if symbol and symbol.id not in seen_ids:
                matched.append(symbol)
                seen_ids.add(symbol.id)
        
        return matched
    
    async def _find_symbol_candidates(self, name: str) -> List[Symbol]:
        """Find symbols matching the given name"""
        candidates = []
        
        # Search all symbols in graph
        all_symbols = await self.graph.get_all_symbols()
        
        for symbol in all_symbols:
            if symbol.name == name:
                candidates.append(symbol)
            # Also check for partial matches (e.g., module.function)
            elif symbol.name.endswith(f".{name}") or symbol.name.endswith(f"/{name}"):
                candidates.append(symbol)
        
        return candidates
    
    def _disambiguate(
        self, 
        candidates: List[Symbol], 
        mention: SymbolMention
    ) -> Optional[Symbol]:
        """Disambiguate between multiple symbol candidates"""
        if len(candidates) == 1:
            return candidates[0]
        
        # Prefer functions/methods if mention has ()
        if '()' in mention.context:
            for symbol in candidates:
                if symbol.type in (SymbolType.FUNCTION, SymbolType.METHOD):
                    return symbol
        
        # Prefer classes if name starts with uppercase
        if mention.name[0].isupper():
            for symbol in candidates:
                if symbol.type == SymbolType.CLASS:
                    return symbol
        
        # Default to first candidate
        return candidates[0]
    
    async def _update_symbols(
        self, 
        symbols: List[Symbol], 
        page_id: str
    ) -> None:
        """Update symbols with wiki page references"""
        for symbol in symbols:
            # Update symbol with wiki reference
            symbol.wiki_page_id = page_id
            symbol.wiki_section = None  # Could extract section from mention
            
            # Update in graph
            await self.graph.update_symbol(symbol)


async def link_wiki_to_graph(
    graph: KnowledgeGraph, 
    wiki_dir: Path
) -> Dict[str, List[str]]:
    """
    Create bidirectional links between wiki pages and graph symbols.
    
    Args:
        graph: Knowledge graph instance
        wiki_dir: Directory containing wiki markdown files
    
    Returns:
        Dict mapping page_id -> list of symbol_ids
    """
    linker = WikiGraphLinker(graph, wiki_dir)
    return await linker.link_all_pages()
