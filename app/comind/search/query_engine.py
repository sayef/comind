"""
Wiki-enhanced query engine for GitNexus Python

Combines graph-based search with wiki content to provide
rich, contextual search results.
"""

import json
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import bm25s
import numpy as np
import Stemmer
from fastembed import TextEmbedding

from comind.core.graph import Symbol, SymbolType
from comind.logging_config import get_logger
from comind.storage.graph_adapter import KnowledgeGraph
from comind.utils.snippet_extractor import CodeSnippetExtractor

logger = get_logger(__name__)


@dataclass
class SearchResult:
    """Represents a search result with enriched context"""

    symbol: Symbol
    score: float
    score_breakdown: dict[str, float]
    code_snippet: dict[str, Any] | None = None
    graph_context: dict[str, Any] | None = None
    wiki_context: dict[str, Any] | None = None
    usage_examples: list[dict[str, Any]] = None

    def to_dict(self) -> dict[str, Any]:
        """Convert to dictionary representation"""
        return {
            "symbol": self.symbol.to_dict(),
            "score": self.score,
            "score_breakdown": self.score_breakdown,
            "code_snippet": self.code_snippet,
            "graph_context": self.graph_context,
            "wiki_context": self.wiki_context,
            "usage_examples": self.usage_examples or [],
        }


class TextSearchEngine:
    """Text-based search using BM25S with disk persistence"""

    def __init__(self):
        self.retriever = None
        self.corpus_tokens = []
        self.symbol_ids = []
        self.stemmer = Stemmer.Stemmer("english")
        self.index_path = None  # Will be set when saving

    async def add_symbol(self, symbol: Symbol, content: str = ""):
        """Add symbol to search index"""
        # Combine all searchable text
        text = f"{symbol.name} {symbol.signature or ''} {symbol.docstring or ''} {content}"

        # Store raw text for now, will tokenize in batch during build_index
        self.corpus_tokens.append(text)
        self.symbol_ids.append(symbol.id)

    async def build_index(self):
        """Build BM25 index from accumulated symbols"""
        if not self.corpus_tokens:
            return

        # Tokenize all texts in batch
        # bm25s.tokenize expects a list of strings and returns a list of token lists
        tokenized_corpus = bm25s.tokenize(self.corpus_tokens, stemmer=self.stemmer)

        # Create BM25 retriever and index the tokenized corpus
        self.retriever = bm25s.BM25()
        self.retriever.index(tokenized_corpus)

    async def save(self, index_dir: str):
        """Save BM25S index to disk"""
        if not self.retriever:
            return

        from pathlib import Path

        index_path = Path(index_dir)
        index_path.mkdir(parents=True, exist_ok=True)

        # Save BM25S index
        self.retriever.save(str(index_path / "bm25s_index"))

        # Save symbol IDs mapping
        with open(index_path / "symbol_ids.json", "w") as f:
            json.dump(self.symbol_ids, f)

        self.index_path = str(index_path)

    async def load(self, index_dir: str) -> bool:
        """Load BM25S index from disk"""
        from pathlib import Path

        index_path = Path(index_dir)
        if not index_path.exists():
            return False

        bm25s_path = index_path / "bm25s_index"
        symbol_ids_path = index_path / "symbol_ids.json"

        if not bm25s_path.exists() or not symbol_ids_path.exists():
            return False

        # Load BM25S index
        self.retriever = bm25s.BM25.load(str(bm25s_path))

        # Load symbol IDs
        with open(symbol_ids_path) as f:
            self.symbol_ids = json.load(f)

        self.index_path = str(index_path)
        return True

    async def search(self, query: str, limit: int = 10) -> list[tuple[str, float]]:
        """Search for symbols using BM25"""
        if not self.retriever or not self.symbol_ids:
            return []

        # Tokenize query
        query_tokens = bm25s.tokenize(query, stemmer=self.stemmer)

        # Get top-k results
        results, scores = self.retriever.retrieve(query_tokens, k=limit)

        # Return symbol_id and score pairs
        return [(self.symbol_ids[idx], float(scores[0][i])) for i, idx in enumerate(results[0])]


class SemanticSearchEngine:
    """Semantic search using fastembed for lightweight, fast embeddings"""

    def __init__(self, model_name: str = "sentence-transformers/all-MiniLM-L6-v2"):
        """
        Initialize with fastembed

        Args:
            model_name: FastEmbed model name
                       Options:
                       - "sentence-transformers/all-MiniLM-L6-v2" (default, 384-dim, fast)
                       - "BAAI/bge-small-en-v1.5" (384-dim, high quality)
        """
        logger.info(f"Initializing fastembed model: {model_name} (first run may download model)")

        self.model = TextEmbedding(model_name=model_name)
        self.embeddings: dict[str, np.ndarray] = {}
        # Batch accumulation for efficient encoding
        self.pending_symbols: list[tuple[str, str]] = []  # (symbol_id, text)

        logger.info("Fastembed model ready")

    async def add_symbol(self, symbol: Symbol, text: str = ""):
        """Add symbol to pending batch (will be encoded in batch later)"""
        # Combine symbol information for embedding
        combined_text = f"{symbol.name} {symbol.signature or ''} {symbol.docstring or ''} {text}"
        self.pending_symbols.append((symbol.id, combined_text))

    async def build_index(self):
        """Encode all pending symbols in batch with progress logging"""
        if not self.pending_symbols:
            return

        # Extract texts and symbol IDs
        symbol_ids = [sid for sid, _ in self.pending_symbols]
        texts = [text for _, text in self.pending_symbols]

        total = len(texts)
        logger.info(f"Building semantic embeddings for {total} symbols...")

        # Process embeddings incrementally with progress logging
        # fastembed processes in batches internally (batch_size=32)
        embeddings_gen = self.model.embed(texts, batch_size=32)

        processed = 0
        for i, (symbol_id, embedding) in enumerate(zip(symbol_ids, embeddings_gen)):
            self.embeddings[symbol_id] = np.array(embedding)
            processed += 1

            # Log progress every 500 symbols
            if processed % 500 == 0 or processed == total:
                logger.info(
                    f"Semantic embeddings progress: {processed}/{total} ({100 * processed // total}%)"
                )

        logger.info(f"Semantic embeddings complete: {processed} vectors generated")

        # Clear pending batch
        self.pending_symbols.clear()

    async def save(self, index_dir: str):
        """Save embeddings to disk with metadata"""
        if not self.embeddings:
            return

        import pickle
        from pathlib import Path

        index_path = Path(index_dir)
        index_path.mkdir(parents=True, exist_ok=True)

        # Get embedding dimension from first embedding
        embedding_dim = next(iter(self.embeddings.values())).shape[0] if self.embeddings else 0

        # Save embeddings and metadata
        data = {
            "embeddings": self.embeddings,
            "model_name": self.model.model_name if hasattr(self.model, "model_name") else "unknown",
            "embedding_dim": embedding_dim,
            "version": "model2vec-v1",
        }

        with open(index_path / "embeddings.pkl", "wb") as f:
            pickle.dump(data, f)

    async def load(self, index_dir: str) -> bool:
        """Load embeddings from disk with version checking"""
        import pickle
        from pathlib import Path

        index_path = Path(index_dir)
        embeddings_path = index_path / "embeddings.pkl"

        if not embeddings_path.exists():
            return False

        # Load embeddings
        with open(embeddings_path, "rb") as f:
            data = pickle.load(f)

        # Handle old format (just dict of embeddings) vs new format (dict with metadata)
        if isinstance(data, dict) and "embeddings" in data:
            # New format with metadata
            self.embeddings = data["embeddings"]
            stored_dim = data.get("embedding_dim", 0)
            stored_version = data.get("version", "unknown")

            # Get current model dimension
            test_embedding = next(self.model.embed(["test"]))
            current_dim = test_embedding.shape[0]

            # Validate dimension compatibility
            if stored_dim != current_dim:
                logger.warning(
                    f"Embedding dimension mismatch! Stored: {stored_dim}-dim ({stored_version}), "
                    f"Current model: {current_dim}-dim. Index needs re-analysis."
                )
                self.embeddings = {}  # Clear incompatible embeddings
                return False

            logger.info(
                f"Loaded {len(self.embeddings)} embeddings ({stored_dim}-dim, {stored_version})"
            )
        else:
            # Old format - just dict of embeddings, assume incompatible
            logger.warning(
                "Found old embedding format without metadata. "
                "Index needs re-analysis with current model."
            )
            self.embeddings = {}
            return False

        return True

    async def search(self, query: str, limit: int = 10) -> list[tuple[str, float]]:
        """Search for symbols semantically using fastembed"""
        if not self.embeddings:
            logger.warning("No semantic embeddings available. Run re-analysis to build embeddings.")
            return []

        # Embed query using fastembed (returns generator)
        query_embedding_gen = self.model.embed([query])
        query_embedding = np.array(next(query_embedding_gen))

        # Validate dimension compatibility with stored embeddings
        first_embedding = next(iter(self.embeddings.values()))
        if query_embedding.shape[0] != first_embedding.shape[0]:
            logger.error(
                f"Embedding dimension mismatch! Query: {query_embedding.shape[0]}-dim, "
                f"Stored: {first_embedding.shape[0]}-dim. Index needs re-analysis."
            )
            return []

        similarities = []
        for symbol_id, embedding in self.embeddings.items():
            # Calculate cosine similarity
            similarity = np.dot(query_embedding, embedding) / (
                np.linalg.norm(query_embedding) * np.linalg.norm(embedding)
            )
            similarities.append((symbol_id, float(similarity)))

        # Sort by similarity and return top results
        similarities.sort(key=lambda x: x[1], reverse=True)
        return similarities[:limit]


class WikiStore:
    """Storage and search for wiki content with graph integration"""

    def __init__(self):
        self.pages: dict[str, dict[str, Any]] = {}
        self.embeddings: dict[str, np.ndarray] = {}
        self.semantic_engine = SemanticSearchEngine()
        # Track symbols mentioned in each wiki page (for hybrid wiki-graph approach)
        self.page_symbols: dict[str, list[str]] = {}  # page_id -> [symbol_ids]

    async def add_page(
        self,
        page_id: str,
        title: str,
        content: str,
        metadata: dict[str, Any] = None,
        mentioned_symbols: list[str] = None,
    ):
        """Add a wiki page with optional symbol references"""
        self.pages[page_id] = {
            "id": page_id,
            "title": title,
            "content": content,
            "metadata": metadata or {},
            "sections": self._parse_sections(content),
        }

        # Track mentioned symbols for bidirectional linking
        if mentioned_symbols:
            self.page_symbols[page_id] = mentioned_symbols

        if self.semantic_engine:
            await self.semantic_engine.add_symbol(
                Symbol(
                    id=page_id,
                    name=title,
                    type=SymbolType.MODULE,  # Use MODULE as wiki page type
                    file_path="",
                    line_start=0,
                    line_end=0,
                ),
                content,
            )

    async def search(self, query: str, limit: int = 5) -> list[tuple[str, float]]:
        """Search wiki content"""
        if self.semantic_engine:
            return await self.semantic_engine.search(query, limit)

        # Fallback to simple text search
        results = []
        query_lower = query.lower()

        for page_id, page in self.pages.items():
            content = f"{page['title']} {page['content']}".lower()
            if query_lower in content:
                # Simple relevance score based on occurrences
                score = content.count(query_lower) / len(content.split())
                results.append((page_id, score))

        results.sort(key=lambda x: x[1], reverse=True)
        return results[:limit]

    async def get_relevant_section(self, page_id: str, query: str) -> str | None:
        """Get relevant section from a wiki page"""
        if page_id not in self.pages:
            return None

        page = self.pages[page_id]
        query_lower = query.lower()

        # Find most relevant section by checking both title and content
        best_section = None
        best_score = 0

        for section in page["sections"]:
            section_title = section["title"].lower()
            section_content = section["content"].lower()

            score = 0

            # Prioritize matches in section title (10x weight)
            if query_lower in section_title:
                score += section_title.count(query_lower) * 10

            # Also check content
            if query_lower in section_content:
                score += section_content.count(query_lower)

            if score > best_score:
                best_score = score
                best_section = section["content"]

        return best_section or page["content"]

    def get_mentioned_symbols(self, page_id: str) -> list[str]:
        """Get list of symbol IDs mentioned in a wiki page"""
        return self.page_symbols.get(page_id, [])

    def _parse_sections(self, content: str) -> list[dict[str, str]]:
        """Parse markdown sections"""
        sections = []
        lines = content.split("\n")
        current_section = {"title": "", "content": ""}

        for line in lines:
            if line.startswith("#"):
                # Save previous section
                if current_section["content"].strip():
                    sections.append(current_section.copy())

                # Start new section
                current_section = {"title": line.strip(), "content": ""}
            else:
                current_section["content"] += line + "\n"

        # Add last section
        if current_section["content"].strip():
            sections.append(current_section)

        return sections

    async def save(self, index_dir: str):
        """Save wiki store to disk"""
        import pickle
        from pathlib import Path

        index_path = Path(index_dir)
        index_path.mkdir(parents=True, exist_ok=True)

        # Save pages and symbol mappings
        data = {"pages": self.pages, "page_symbols": self.page_symbols}

        with open(index_path / "wiki.pkl", "wb") as f:
            pickle.dump(data, f)

        # Save semantic embeddings if available
        if self.semantic_engine:
            await self.semantic_engine.save(str(index_path / "wiki_embeddings"))

    async def load(self, index_dir: str) -> bool:
        """Load wiki store from disk"""
        import pickle
        from pathlib import Path

        index_path = Path(index_dir)
        wiki_path = index_path / "wiki.pkl"

        if not wiki_path.exists():
            return False

        # Load pages and symbol mappings
        with open(wiki_path, "rb") as f:
            data = pickle.load(f)

        self.pages = data.get("pages", {})
        self.page_symbols = data.get("page_symbols", {})

        # Load semantic embeddings if available
        if self.semantic_engine:
            await self.semantic_engine.load(str(index_path / "wiki_embeddings"))

        return True


class WikiEnhancedQueryEngine:
    """Main query engine that manages per-repository search indexes"""

    def __init__(self, graph: KnowledgeGraph):
        self.graph = graph
        # Per-repository search engines
        self.repo_text_engines: dict[str, TextSearchEngine] = {}
        self.repo_semantic_engines: dict[str, SemanticSearchEngine] = {}
        self.repo_wiki_stores: dict[str, WikiStore] = {}
        # Per-repository snippet extractors (need repo_root for relative paths)
        self.repo_snippet_extractors: dict[str, CodeSnippetExtractor] = {}
        # Track repository roots for resolving relative paths
        self.repo_roots: dict[str, Path] = {}

    async def initialize_search_indexes(self, index_dir: str = "search_index"):
        """Initialize search indexes - now per-repository"""
        # Indexes are created on-demand when repositories are indexed

    def get_or_create_snippet_extractor(self, repo_id: str) -> CodeSnippetExtractor:
        """Get or create snippet extractor for a repository"""
        if repo_id not in self.repo_snippet_extractors:
            repo_root = self.repo_roots.get(repo_id)
            self.repo_snippet_extractors[repo_id] = CodeSnippetExtractor(repo_root=repo_root)
        return self.repo_snippet_extractors[repo_id]

    async def get_or_create_text_engine(self, repo_id: str) -> TextSearchEngine:
        """Get or create text search engine for a repository"""
        if repo_id not in self.repo_text_engines:
            self.repo_text_engines[repo_id] = TextSearchEngine()
        return self.repo_text_engines[repo_id]

    async def get_or_create_semantic_engine(self, repo_id: str) -> SemanticSearchEngine:
        """Get or create semantic search engine for a repository"""
        if repo_id not in self.repo_semantic_engines:
            self.repo_semantic_engines[repo_id] = SemanticSearchEngine()
        return self.repo_semantic_engines[repo_id]

    async def get_or_create_wiki_store(self, repo_id: str) -> WikiStore:
        """Get or create wiki store for a repository"""
        if repo_id not in self.repo_wiki_stores:
            self.repo_wiki_stores[repo_id] = WikiStore()
        return self.repo_wiki_stores[repo_id]

    async def add_symbol_to_index(self, repo_id: str, symbol: Symbol, content: str = ""):
        """Add a symbol to the repository-specific search index"""
        text_engine = await self.get_or_create_text_engine(repo_id)
        await text_engine.add_symbol(symbol, content)

        semantic_engine = await self.get_or_create_semantic_engine(repo_id)
        await semantic_engine.add_symbol(symbol, content)

    async def build_repo_index(self, repo_id: str):
        """Build search index for a specific repository"""
        if repo_id in self.repo_text_engines:
            await self.repo_text_engines[repo_id].build_index()

        if repo_id in self.repo_semantic_engines:
            await self.repo_semantic_engines[repo_id].build_index()

    async def save_repo_index(self, repo_id: str, indexes_base_dir: Path, repo_path: str = None):
        """Save repository indexes to centralized storage

        Args:
            repo_id: Repository name (e.g., 'skills-api')
            indexes_base_dir: Base directory for indexes
            repo_path: Full path to repository (stored in metadata)
        """

        # Use repo_id (repo_name) directly as directory name
        repo_index_dir = indexes_base_dir / repo_id

        # Save text search index
        if repo_id in self.repo_text_engines:
            text_dir = repo_index_dir / "text"
            await self.repo_text_engines[repo_id].save(str(text_dir))

        # Save semantic search index
        if repo_id in self.repo_semantic_engines:
            semantic_dir = repo_index_dir / "semantic"
            await self.repo_semantic_engines[repo_id].save(str(semantic_dir))

        # Save repo metadata
        metadata_file = repo_index_dir / "metadata.json"
        metadata_file.parent.mkdir(parents=True, exist_ok=True)
        with open(metadata_file, "w") as f:
            json.dump({"repo_name": repo_id, "repo_path": repo_path or repo_id}, f)

    async def load_repo_index(self, repo_id: str, indexes_base_dir: Path) -> bool:
        """Load repository indexes from centralized storage

        Args:
            repo_id: Repository name (e.g., 'skills-api')
            indexes_base_dir: Base directory for indexes

        Returns:
            True if the repo is considered loaded (DuckDB backend or
            file-based text/semantic indexes).
        """
        # Use repo_id (repo_name) directly as directory name
        repo_index_dir = indexes_base_dir / repo_id

        if not repo_index_dir.exists():
            return False

        loaded_any = False

        # Load text search index (file-based BM25S)
        text_dir = repo_index_dir / "text"
        if text_dir.exists():
            text_engine = TextSearchEngine()
            if await text_engine.load(str(text_dir)):
                self.repo_text_engines[repo_id] = text_engine
                loaded_any = True

        # Load semantic search index (file-based FastEmbed)
        semantic_dir = repo_index_dir / "semantic"
        if semantic_dir.exists():
            semantic_engine = SemanticSearchEngine()
            if await semantic_engine.load(str(semantic_dir)):
                self.repo_semantic_engines[repo_id] = semantic_engine
                loaded_any = True

        # If no file-based indexes but DuckDB-backed engines are already
        # registered (set up by _load_engine or analyze), consider loaded.
        if not loaded_any:
            if repo_id in self.repo_text_engines or repo_id in self.repo_semantic_engines:
                loaded_any = True

        # If metadata.json exists the repo was indexed (DuckDB is the
        # primary backend now — file-based indexes are optional).
        if not loaded_any and (repo_index_dir / "metadata.json").exists():
            loaded_any = True

        return loaded_any

    async def load_all_indexes(self, indexes_base_dir: Path) -> list[str]:
        """Load all persisted indexes from centralized storage"""

        if not indexes_base_dir.exists():
            return []

        loaded_repos = []

        # Scan all repo directories (now using repo_name as directory name)
        for repo_dir in indexes_base_dir.iterdir():
            if not repo_dir.is_dir():
                continue

            metadata_file = repo_dir / "metadata.json"
            if not metadata_file.exists():
                continue

            # Load metadata to get repo_name and repo_path
            with open(metadata_file) as f:
                metadata = json.load(f)
                repo_name = metadata.get("repo_name") or metadata.get("repo_id")  # Backward compat
                repo_path = metadata.get("repo_path")

            if repo_name and await self.load_repo_index(repo_name, indexes_base_dir):
                # Set repo_root for snippet extraction
                if repo_path:
                    self.repo_roots[repo_name] = Path(repo_path)
                loaded_repos.append(repo_name)

        return loaded_repos

    async def clear_repo_index(self, repo_id: str):
        """Clear all indexes for a specific repository"""
        if repo_id in self.repo_text_engines:
            del self.repo_text_engines[repo_id]
        if repo_id in self.repo_semantic_engines:
            del self.repo_semantic_engines[repo_id]
        if repo_id in self.repo_wiki_stores:
            del self.repo_wiki_stores[repo_id]

    async def search(
        self,
        query: str,
        repo_id: str,
        include_wiki: bool = True,
        include_snippets: bool = True,
        max_results: int = 10,
    ) -> dict[str, Any]:
        """
        Enhanced search using repository-specific indexes

        Args:
            query: Search query string
            repo_id: Repository ID (REQUIRED - no global search)
            include_wiki: Include wiki content in search
            include_snippets: Include code snippets in results
            max_results: Maximum number of results to return

        Returns:
            Dictionary with search results and metadata
        """
        if not repo_id:
            return {"error": "repo_id is required - global search is not supported", "results": []}

        # Get repository-specific search engines
        text_engine = self.repo_text_engines.get(repo_id)
        semantic_engine = self.repo_semantic_engines.get(repo_id)
        wiki_store = self.repo_wiki_stores.get(repo_id)

        if not text_engine:
            return {"error": f"Repository {repo_id} not indexed", "results": []}

        # Step 1: Graph-based search (filtered by repo_id)
        graph_results = await self.graph.query(query)
        # Filter graph results by repo_id
        graph_results = [r for r in graph_results if r.get("repo_id") == repo_id]

        # Step 2: Text search (repo-specific index)
        text_results = await text_engine.search(query, max_results)

        # Step 3: Semantic search (repo-specific index)
        semantic_results = []
        if semantic_engine:
            semantic_results = await semantic_engine.search(query, max_results)

        # Step 4: Wiki search (repo-specific wiki)
        wiki_results = []
        if include_wiki and wiki_store:
            wiki_results = await wiki_store.search(query, max_results // 2)

        # Step 5: Merge and rank results
        merged_results = await self._merge_results(
            graph_results, text_results, semantic_results, wiki_results, max_results
        )

        # Step 6: Enrich results with context (all from same repo)
        enriched_results = []
        seen_symbols = set()  # Track symbol IDs to avoid duplicates

        for symbol_id, score, breakdown in merged_results:
            # Skip duplicates - keep only the first (highest scoring) occurrence
            if symbol_id in seen_symbols:
                continue
            seen_symbols.add(symbol_id)

            symbol = await self.graph.get_symbol(symbol_id)
            if not symbol:
                continue

            # Double-check repo_id (should already be filtered)
            if symbol.repo_id != repo_id:
                continue

            # Extract code snippet using repository-specific extractor
            code_snippet = None
            snippet_extractor = self.get_or_create_snippet_extractor(repo_id)
            if include_snippets:
                if symbol.type == SymbolType.FUNCTION or symbol.type == SymbolType.METHOD:
                    code_snippet = await snippet_extractor.extract_function_snippet(symbol)
                elif symbol.type == SymbolType.CLASS:
                    code_snippet = await snippet_extractor.extract_class_snippet(symbol)
                else:
                    code_snippet = await snippet_extractor.extract_snippet(symbol)

            # Get graph context
            graph_context = await self._get_graph_context(symbol)

            # Get wiki context
            wiki_context = None
            if include_wiki:
                wiki_context = await self._get_wiki_context(symbol, query, repo_id)

            # Get usage examples
            usage_examples = await snippet_extractor.extract_usage_examples(symbol)

            result = SearchResult(
                symbol=symbol,
                score=score,
                score_breakdown=breakdown,
                code_snippet=code_snippet,
                graph_context=graph_context,
                wiki_context=wiki_context,
                usage_examples=usage_examples,
            )

            enriched_results.append(result)

        return {
            "query": query,
            "total_results": len(enriched_results),
            "results": [result.to_dict() for result in enriched_results],
            "search_strategies": {
                "graph": len(graph_results),
                "text": len(text_results),
                "semantic": len(semantic_results),
                "wiki": len(wiki_results),
            },
        }

    async def get_context(
        self,
        symbol_name: str,
        file_path: str | None = None,
        depth: int = 2,
        include_wiki: bool = True,
    ) -> dict[str, Any]:
        """Get 360-degree context for a symbol"""
        # Find the symbol (handle disambiguation if needed)
        symbol = await self._find_symbol(symbol_name, file_path)
        if not symbol:
            return {"error": f"Symbol '{symbol_name}' not found"}

        # Get graph relationships
        callers = await self.graph.get_callers(symbol.id)
        callees = await self.graph.get_callees(symbol.id)
        dependencies = await self.graph.get_dependencies(symbol.id)
        community = await self.graph.get_community(symbol.id)
        processes = await self.graph.get_symbol_processes(symbol.id)

        # Get code snippet
        repo_id = symbol.repo_id or ""
        snippet_extractor = self.get_or_create_snippet_extractor(repo_id)
        code_snippet = await snippet_extractor.extract_snippet(symbol)

        # Get wiki context
        wiki_context = None
        if include_wiki:
            wiki_context = await self._get_wiki_context(symbol, symbol_name, repo_id)

        return {
            "symbol": symbol.to_dict(),
            "relationships": {
                "callers": [caller.to_dict() for caller in callers],
                "callees": [callee.to_dict() for callee in callees],
                "dependencies": [dep.to_dict() for dep in dependencies],
                "community": community,
                "processes": processes,
            },
            "code_snippet": code_snippet,
            "wiki_context": wiki_context,
            "depth": depth,
        }

    async def analyze_impact(
        self,
        symbol_name: str,
        direction: str = "upstream",
        max_depth: int = 3,
        min_confidence: float = 0.7,
    ) -> dict[str, Any]:
        """Analyze blast radius of changing a symbol"""
        symbol = await self._find_symbol(symbol_name)
        if not symbol:
            return {"error": f"Symbol '{symbol_name}' not found"}

        impact = {
            "symbol": symbol.to_dict(),
            "direction": direction,
            "max_depth": max_depth,
            "upstream": {"depth_1": [], "depth_2": [], "depth_3": []},
            "downstream": {"depth_1": [], "depth_2": [], "depth_3": []},
            "risk_level": "LOW",
            "affected_processes": [],
            "affected_modules": [],
        }

        if direction in ["upstream", "both"]:
            await self._trace_upstream(symbol.id, impact, max_depth, min_confidence)

        if direction in ["downstream", "both"]:
            await self._trace_downstream(symbol.id, impact, max_depth, min_confidence)

        # Calculate risk level
        total_affected = len(impact["upstream"]["depth_1"]) + len(impact["downstream"]["depth_1"])

        if total_affected > 10:
            impact["risk_level"] = "HIGH"
        elif total_affected > 3:
            impact["risk_level"] = "MEDIUM"

        return impact

    async def trace_execution(self, entry_point: str, max_depth: int = 10) -> dict[str, Any]:
        """Trace execution flow from an entry point"""
        symbol = await self._find_symbol(entry_point)
        if not symbol:
            return {"error": f"Entry point '{entry_point}' not found"}

        execution_flow = {"entry_point": symbol.to_dict(), "flow": [], "max_depth": max_depth}

        await self._trace_execution_flow(symbol.id, execution_flow, max_depth, 0)

        return execution_flow

    async def _merge_results(
        self,
        graph_results: list[dict],
        text_results: list[tuple[str, float]],
        semantic_results: list[tuple[str, float]],
        wiki_results: list[tuple[str, float]],
        max_results: int,
    ) -> list[tuple[str, float, dict[str, float]]]:
        """
        Merge results from different search strategies using Reciprocal Rank Fusion (RRF)
        with smart symbol type weighting and quality filtering.

        RRF Formula: score = Σ 1/(k + rank) for each ranking list
        where k is a constant (typically 60) to reduce impact of high ranks

        Improvements:
        - Symbol type weighting (implementations > imports)
        - Semantic score threshold for false positive prevention
        - Context-aware import filtering
        """
        RRF_K = 60  # RRF constant (standard value from research)

        # Build ranked lists for each search strategy
        ranked_lists = {}

        # Text search rankings
        text_rankings = {
            symbol_id: rank
            for rank, (symbol_id, _) in enumerate(text_results)
            if isinstance(symbol_id, str)
        }
        if text_rankings:
            ranked_lists["text"] = text_rankings

        # Semantic search rankings
        semantic_rankings = {
            symbol_id: rank
            for rank, (symbol_id, _) in enumerate(semantic_results)
            if isinstance(symbol_id, str)
        }
        if semantic_rankings:
            ranked_lists["semantic"] = semantic_rankings

        # Graph search rankings (lower priority)
        graph_rankings = {}
        for rank, result in enumerate(graph_results):
            symbol_id = None
            if isinstance(result, dict):
                if "symbol" in result:
                    symbol_data = result["symbol"]
                    if isinstance(symbol_data, dict):
                        symbol_id = symbol_data.get("id")
                    else:
                        symbol_id = getattr(symbol_data, "id", None)
                elif "id" in result:
                    symbol_id = result.get("id")

            if symbol_id and isinstance(symbol_id, str):
                graph_rankings[symbol_id] = rank

        if graph_rankings:
            ranked_lists["graph"] = graph_rankings

        # Wiki search rankings (boosted)
        wiki_rankings = {
            symbol_id: rank
            for rank, (symbol_id, _) in enumerate(wiki_results)
            if isinstance(symbol_id, str)
        }
        if wiki_rankings:
            ranked_lists["wiki"] = wiki_rankings

        # Calculate RRF scores for all symbols
        rrf_scores = {}
        score_breakdowns = {}
        raw_scores = {}  # Store raw scores for quality filtering

        for strategy, rankings in ranked_lists.items():
            for symbol_id, rank in rankings.items():
                # RRF score: 1 / (k + rank)
                rrf_score = 1.0 / (RRF_K + rank)

                if symbol_id not in rrf_scores:
                    rrf_scores[symbol_id] = 0
                    score_breakdowns[symbol_id] = {}
                    raw_scores[symbol_id] = {}

                rrf_scores[symbol_id] += rrf_score
                score_breakdowns[symbol_id][strategy] = rrf_score

        # Preserve original scores for breakdown and filtering
        for symbol_id, score in text_results:
            if isinstance(symbol_id, str) and symbol_id in score_breakdowns:
                score_breakdowns[symbol_id]["text_raw"] = score
                raw_scores[symbol_id]["text"] = score

        for symbol_id, score in semantic_results:
            if isinstance(symbol_id, str) and symbol_id in score_breakdowns:
                score_breakdowns[symbol_id]["semantic_raw"] = score
                raw_scores[symbol_id]["semantic"] = score

        # Apply symbol type weighting and quality filtering
        final_results = []
        for symbol_id, rrf_score in rrf_scores.items():
            # Get symbol to check type
            symbol = await self.graph.get_symbol(symbol_id)
            if not symbol:
                continue

            # Calculate quality-adjusted score
            adjusted_score = await self._apply_quality_adjustments(
                symbol, rrf_score, raw_scores.get(symbol_id, {})
            )

            # Skip if score is too low after adjustments
            if adjusted_score <= 0:
                continue

            # Normalize breakdown scores for display
            breakdown = {}
            if "text_raw" in score_breakdowns[symbol_id]:
                breakdown["text"] = score_breakdowns[symbol_id]["text_raw"]
            if "semantic_raw" in score_breakdowns[symbol_id]:
                breakdown["semantic"] = score_breakdowns[symbol_id]["semantic_raw"]
            if "wiki" in score_breakdowns[symbol_id]:
                breakdown["wiki"] = score_breakdowns[symbol_id]["wiki"]
            if "graph" in score_breakdowns[symbol_id]:
                breakdown["graph"] = score_breakdowns[symbol_id]["graph"]

            final_results.append((symbol_id, adjusted_score, breakdown))

        # Sort by adjusted score (higher is better) and return top results
        final_results.sort(key=lambda x: x[1], reverse=True)
        return final_results[:max_results]

    async def _apply_quality_adjustments(
        self, symbol: Symbol, base_score: float, raw_scores: dict[str, float]
    ) -> float:
        """
        Apply smart quality adjustments based on symbol type and search context.

        Strategy:
        - Boost implementations (functions, classes, methods) over references (imports)
        - Apply semantic threshold to prevent false positives
        - Context-aware import filtering (keep if high semantic relevance)
        """
        adjusted_score = base_score

        # Symbol type weighting (flexible, not rigid)
        symbol_type_multipliers = {
            SymbolType.CLASS: 1.3,  # Boost classes
            SymbolType.FUNCTION: 1.2,  # Boost functions
            SymbolType.METHOD: 1.2,  # Boost methods
            SymbolType.MODULE: 1.0,  # Neutral for modules
            SymbolType.IMPORT: 0.7,  # Reduce imports (but don't eliminate)
        }

        multiplier = symbol_type_multipliers.get(symbol.type, 1.0)
        adjusted_score *= multiplier

        # Smart import filtering: Keep imports if they have high semantic relevance
        # This prevents filtering out legitimate import searches
        if symbol.type == SymbolType.IMPORT:
            semantic_score = raw_scores.get("semantic", 0)
            text_score = raw_scores.get("text", 0)

            # If import has high semantic score, it's likely relevant
            if semantic_score > 0.5:
                # Restore some of the penalty for highly relevant imports
                adjusted_score *= 1.3  # Partially compensate for the 0.7 multiplier
            # If only text match with low semantic, apply additional penalty
            elif semantic_score < 0.3 and text_score > 0:
                adjusted_score *= 0.5  # Further reduce low-quality import matches

        # Semantic quality threshold: Penalize results with only text matches
        # and very low semantic scores (likely false positives)
        if "semantic" in raw_scores and "text" in raw_scores:
            semantic_score = raw_scores["semantic"]
            text_score = raw_scores["text"]

            # If text score is high but semantic is very low, it's likely a false positive
            if text_score > 2.0 and semantic_score < 0.25:
                # Apply penalty but don't eliminate (could be legitimate keyword match)
                adjusted_score *= 0.6

        # Boost symbols with both text and semantic matches (high confidence)
        if "semantic" in raw_scores and "text" in raw_scores:
            if raw_scores["semantic"] > 0.4 and raw_scores["text"] > 1.0:
                adjusted_score *= 1.15  # Small boost for multi-signal matches

        return adjusted_score

    async def _find_symbol(self, name: str, file_path: str | None = None) -> Symbol | None:
        """Find a symbol by name (with optional file disambiguation)"""
        symbols = await self.graph.query(name)

        if file_path:
            # Filter by file path
            for result in symbols:
                if "symbol" in result and result["symbol"]["file_path"] == file_path:
                    return Symbol(**result["symbol"])
        else:
            # Return first match
            for result in symbols:
                if "symbol" in result:
                    return Symbol(**result["symbol"])

        return None

    async def _get_graph_context(self, symbol: Symbol) -> dict[str, Any]:
        """Get graph context for a symbol"""
        from comind.core.graph import RelationType

        # Fetch relationships directly to access call site properties
        caller_rels = await self.graph.backend.get_relationships(
            symbol.id, RelationType.CALLS, direction="incoming"
        )
        callee_rels = await self.graph.backend.get_relationships(
            symbol.id, RelationType.CALLS, direction="outgoing"
        )
        dependencies = await self.graph.get_dependencies(symbol.id)

        async def _qualified_name(s: Symbol) -> str:
            props = s.properties or {}
            parent_id = props.get("parent_id")
            if parent_id:
                parent = await self.graph.get_symbol(parent_id)
                if parent and parent.type == SymbolType.CLASS:
                    return f"{parent.name}.{s.name}"
            return s.name

        # Deduplicate by symbol id, preserving relationship properties
        unique_callers: dict[str, dict] = {}
        for rel in caller_rels:
            sid = rel.source_id
            if sid not in unique_callers:
                s = await self.graph.get_symbol(sid)
                if s:
                    unique_callers[sid] = {
                        "name": await _qualified_name(s),
                        "file": s.file_path,
                        "line": s.line_start,
                        "call_line": rel.properties.get("call_line"),
                        "call_text": rel.properties.get("call_text"),
                    }

        unique_callees: dict[str, dict] = {}
        for rel in callee_rels:
            tid = rel.target_id
            if tid not in unique_callees:
                s = await self.graph.get_symbol(tid)
                if s:
                    unique_callees[tid] = {
                        "name": await _qualified_name(s),
                        "file": s.file_path,
                        "line": s.line_start,
                        "call_line": rel.properties.get("call_line"),
                        "call_text": rel.properties.get("call_text"),
                    }

        unique_dependencies = {}
        for d in dependencies:
            if d.id not in unique_dependencies:
                unique_dependencies[d.id] = d

        MAX_GRAPH_ITEMS = 20
        return {
            "callers": list(unique_callers.values())[:MAX_GRAPH_ITEMS],
            "callees": list(unique_callees.values())[:MAX_GRAPH_ITEMS],
            "callers_total": len(unique_callers),
            "callees_total": len(unique_callees),
            "dependencies": [
                {"name": d.name, "file": d.file_path} for d in unique_dependencies.values()
            ][:MAX_GRAPH_ITEMS],
        }

    async def _get_wiki_context(
        self, symbol: Symbol, query: str, repo_id: str
    ) -> dict[str, Any] | None:
        """Get wiki context for a symbol from repo-specific wiki store"""
        # Get repo-specific wiki store
        wiki_store = self.repo_wiki_stores.get(repo_id)
        if not wiki_store:
            return None

        # Search wiki using symbol name and file path for more specific results
        # Include file path to help find the right wiki page
        file_component = symbol.file_path.split("/")[-2] if "/" in symbol.file_path else ""
        search_query = f"{symbol.name} {file_component} {symbol.type.value}"
        wiki_results = await wiki_store.search(search_query, 5)

        # If no results, try without file component
        if not wiki_results:
            search_query = f"{symbol.name} {symbol.type.value}"
            wiki_results = await wiki_store.search(search_query, 5)

        # If still no results, try the original query
        if not wiki_results:
            wiki_results = await wiki_store.search(query, 5)

        if not wiki_results:
            return None

        # Prioritize pages that mention the symbol's file path
        best_page_id = None
        best_score = 0

        for page_id, score in wiki_results:
            page = wiki_store.pages.get(page_id)
            if page:
                # Boost score if page content mentions the symbol's file
                file_name = symbol.file_path.split("/")[-1]
                if file_name in page["content"]:
                    score *= 2.0

                if score > best_score:
                    best_score = score
                    best_page_id = page_id

        if not best_page_id:
            best_page_id, _ = wiki_results[0]

        # Use symbol name for section extraction to get more specific content
        relevant_section = await wiki_store.get_relevant_section(best_page_id, symbol.name)

        summary = relevant_section.strip() if relevant_section else ""

        return {"page_id": best_page_id, "relevant_section": relevant_section, "summary": summary}

    async def _trace_upstream(
        self,
        symbol_id: str,
        impact: dict,
        max_depth: int,
        min_confidence: float,
        current_depth: int = 1,
    ):
        """Trace upstream dependencies"""
        if current_depth > max_depth:
            return

        callers = await self.graph.get_callers(symbol_id)

        for caller in callers:
            depth_key = f"depth_{current_depth}"

            if caller.id not in [s["id"] for s in impact["upstream"][depth_key]]:
                impact["upstream"][depth_key].append(caller.to_dict())

                # Recurse
                await self._trace_upstream(
                    caller.id, impact, max_depth, min_confidence, current_depth + 1
                )

    async def _trace_downstream(
        self,
        symbol_id: str,
        impact: dict,
        max_depth: int,
        min_confidence: float,
        current_depth: int = 1,
    ):
        """Trace downstream dependencies"""
        if current_depth > max_depth:
            return

        callees = await self.graph.get_callees(symbol_id)

        for callee in callees:
            depth_key = f"depth_{current_depth}"

            if callee.id not in [s["id"] for s in impact["downstream"][depth_key]]:
                impact["downstream"][depth_key].append(callee.to_dict())

                # Recurse
                await self._trace_downstream(
                    callee.id, impact, max_depth, min_confidence, current_depth + 1
                )

    async def _trace_execution_flow(
        self, symbol_id: str, flow: dict, max_depth: int, current_depth: int
    ):
        """Trace execution flow"""
        if current_depth >= max_depth:
            return

        symbol = await self.graph.get_symbol(symbol_id)
        if not symbol:
            return

        flow["flow"].append(
            {"step": current_depth, "symbol": symbol.to_dict(), "type": symbol.type.value}
        )

        # Get callees and continue tracing
        callees = await self.graph.get_callees(symbol_id)

        for callee in callees[:3]:  # Limit to avoid explosion
            await self._trace_execution_flow(callee.id, flow, max_depth, current_depth + 1)
