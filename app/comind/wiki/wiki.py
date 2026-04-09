"""
Wiki generator for GitNexus Python

LLM-powered documentation generation from knowledge graph.
"""

import asyncio
from pathlib import Path
from typing import Any, Dict, List, Optional
from dataclasses import dataclass

from comind.llm.llm_client import LLMConfig, resolve_llm_config
from comind.storage.graph_adapter import KnowledgeGraph
from comind.wiki.wiki_generator import WikiGenerator, WikiRunResult


@dataclass
class WikiPage:
    """Represents a wiki documentation page"""
    module_name: str
    title: str
    content: str
    metadata: Dict[str, Any]
    symbols: List[str]  # Symbol IDs referenced in this page
    
    def to_dict(self) -> Dict[str, Any]:
        """Convert to dictionary representation"""
        return {
            "module_name": self.module_name,
            "title": self.title,
            "content": self.content,
            "metadata": self.metadata,
            "symbols": self.symbols
        }


async def generate_wiki(
    repo_path: str,
    storage_path: str,
    graph: KnowledgeGraph,
    llm_config: Optional[Dict[str, Any]] = None,
    force: bool = False,
    on_progress: Optional[Any] = None
) -> WikiRunResult:
    """
    Generate LLM-powered wiki documentation from knowledge graph.
    
    Args:
        repo_path: Path to the repository
        storage_path: Path to storage directory (for wiki output)
        graph: Knowledge graph instance
        llm_config: LLM configuration overrides (api_key, model, etc.)
        force: Force full regeneration
        on_progress: Progress callback
    
    Returns:
        WikiRunResult with generation statistics
    """
    # Resolve LLM configuration
    config = resolve_llm_config(llm_config or {})
    
    if not config.api_key:
        raise ValueError(
            "No API key found. Set OPENAI_API_KEY or GITNEXUS_API_KEY environment variable, "
            "or pass api_key in llm_config parameter."
        )
    
    # Create wiki generator
    generator = WikiGenerator(
        repo_path=repo_path,
        storage_path=storage_path,
        graph=graph,
        llm_config=config,
        force=force,
        on_progress=on_progress
    )
    
    # Run generation
    result = await generator.run()
    
    return result


async def load_wiki_pages(wiki_dir: str) -> List[WikiPage]:
    """
    Load generated wiki pages from disk.
    
    Args:
        wiki_dir: Directory containing wiki markdown files
    
    Returns:
        List of WikiPage objects
    """
    wiki_path = Path(wiki_dir)
    if not wiki_path.exists():
        return []
    
    pages = []
    for md_file in wiki_path.glob("*.md"):
        if md_file.name == "README.md":
            continue  # Skip overview page
        
        with open(md_file, 'r', encoding='utf-8') as f:
            content = f.read()
        
        # Extract title from first heading
        title = md_file.stem.replace("-", " ").title()
        first_line = content.split("\n")[0] if content else ""
        if first_line.startswith("# "):
            title = first_line[2:].strip()
        
        page = WikiPage(
            module_name=md_file.stem,
            title=title,
            content=content,
            metadata={"file": str(md_file)},
            symbols=[]  # Would need to parse content to extract symbol references
        )
        pages.append(page)
    
    return pages
