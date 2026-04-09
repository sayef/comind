"""
FastAPI server for GitNexus Python

Provides REST API endpoints for code intelligence operations.
"""

from __future__ import annotations

import os
import shutil
import subprocess
import time
from contextlib import asynccontextmanager
from pathlib import Path
from typing import Any

import git
from fastapi import Body, FastAPI, HTTPException
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import PlainTextResponse
from pydantic import BaseModel, ConfigDict, Field

from comind.config import get_settings
from comind.indexing.indexer import PythonIndexer
from comind.indexing.process_detector import ProcessDetector
from comind.logging_config import configure_logging, get_logger
from comind.search.query_engine import WikiEnhancedQueryEngine
from comind.storage.duckdb_backend import DuckDBBackend
from comind.storage.graph_adapter import GraphAdapter, KnowledgeGraph
from comind.style.style_extractor import extract_style_guide
from comind.style.style_guide_generator import generate_style_guide_markdown
from comind.utils.markdown_formatter import MarkdownFormatter
from comind.utils.snippet_extractor import CodeSnippetExtractor
from comind.wiki.wiki import generate_wiki as generate_wiki_docs, load_wiki_pages

# Configure logging
configure_logging()
logger = get_logger(__name__)


def is_git_url(repo: str) -> bool:
    """Check if the repo string is a git URL"""
    return repo.startswith(("http://", "https://", "git@", "ssh://"))


def clone_or_pull_git_repo(repo_url: str, repo_name: str, branch: str = "main") -> tuple[str, bool]:
    """Clone or update a git repository in persistent storage using GitPython

    Args:
        repo_url: Git repository URL
        repo_name: Repository name for storage
        branch: Branch or revision to checkout

    Returns:
        Tuple of (repo_path, was_cloned) where was_cloned is True if newly cloned

    Raises:
        HTTPException: If git operations fail
    """
    settings = get_settings()
    repo_dir = settings.storage.repos_dir / repo_name

    # Ensure repos directory exists
    settings.storage.repos_dir.mkdir(parents=True, exist_ok=True)

    # Prepare URL with GitLab token if available
    clone_url = repo_url
    gitlab_token = os.getenv("GITLAB_API_PRIVATE_TOKEN")
    if gitlab_token and "gitlab.com" in repo_url:
        # Strip any existing credentials from URL
        import re

        clean_url = re.sub(r"https://[^@]+@gitlab\.com", "https://gitlab.com", repo_url)
        if clean_url.startswith("https://gitlab.com"):
            clone_url = clean_url.replace(
                "https://gitlab.com", f"https://imsaif:{gitlab_token}@gitlab.com"
            )

    try:
        if repo_dir.exists() and (repo_dir / ".git").exists():
            # Repository exists - pull latest changes
            logger.info("Updating existing repository", repo_name=repo_name, path=str(repo_dir))

            try:
                repo = git.Repo(repo_dir)

                # Fetch latest changes
                origin = repo.remotes.origin
                origin.fetch()

                # Checkout the specified branch
                if branch in repo.heads:
                    repo.heads[branch].checkout()
                else:
                    # Create tracking branch if it doesn't exist locally
                    repo.git.checkout(branch, track=f"origin/{branch}")

                # Pull latest changes
                origin.pull(branch)

                logger.info("Repository updated successfully", repo_name=repo_name, branch=branch)
                return str(repo_dir), False

            except git.GitCommandError as e:
                logger.warning("Git update failed", error=str(e), repo_name=repo_name)
                # Continue anyway - repo might still be usable
                return str(repo_dir), False

        else:
            # Clone new repository
            logger.info(
                "Cloning git repository",
                url=repo_url,
                branch=branch,
                repo_name=repo_name,
                path=str(repo_dir),
            )

            # Remove partial clone if exists
            if repo_dir.exists():
                shutil.rmtree(repo_dir, ignore_errors=True)

            try:
                git.Repo.clone_from(
                    clone_url,
                    str(repo_dir),
                    branch=branch,
                    depth=1,  # Shallow clone for speed
                )

                logger.info(
                    "Git repository cloned successfully", repo_name=repo_name, path=str(repo_dir)
                )
                return str(repo_dir), True

            except git.GitCommandError as e:
                if repo_dir.exists():
                    shutil.rmtree(repo_dir, ignore_errors=True)
                raise HTTPException(status_code=400, detail=f"Git clone failed: {e!s}")

    except git.GitCommandError as e:
        if repo_dir.exists() and not repo_dir.joinpath(".git").exists():
            shutil.rmtree(repo_dir, ignore_errors=True)
        raise HTTPException(status_code=500, detail=f"Git operation failed: {e!s}")
    except Exception as e:
        if repo_dir.exists() and not repo_dir.joinpath(".git").exists():
            shutil.rmtree(repo_dir, ignore_errors=True)
        raise HTTPException(status_code=500, detail=f"Failed to clone/update repository: {e!s}")


# Pydantic models for API
class SearchRequest(BaseModel):
    model_config = ConfigDict(str_strip_whitespace=True)

    query: str = Field(..., description="Search query")
    repo_name: str | None = Field(None, description="Repository name (optional)")
    max_results: int = Field(10, ge=1, le=100, description="Maximum results")
    include_wiki: bool = Field(True, description="Include wiki context")
    include_snippets: bool = Field(True, description="Include code snippets")
    search_type: str = Field("hybrid", description="Search type: text, semantic, or hybrid")
    format: str = Field("json", description="Output format: json, markdown, or compact")
    include_graph: bool = Field(True, description="Include graph context (callers/callees)")
    include_code: bool = Field(True, description="Include code snippets in markdown")


class ContextRequest(BaseModel):
    model_config = ConfigDict(str_strip_whitespace=True)

    symbol_id: str | None = Field(None, description="Symbol ID")
    symbol_name: str | None = Field(None, description="Symbol name")
    file_path: str | None = Field(None, description="File path for disambiguation")
    depth: int = Field(2, ge=1, le=10, description="Depth of relationships")
    include_wiki: bool = Field(True, description="Include wiki context")


class ImpactRequest(BaseModel):
    model_config = ConfigDict(str_strip_whitespace=True)

    symbol_id: str | None = Field(None, description="Symbol ID")
    symbol_name: str | None = Field(None, description="Symbol name")
    direction: str = Field("upstream", description="Direction: upstream, downstream, both")
    max_depth: int = Field(3, ge=1, le=10, description="Maximum depth")
    min_confidence: float = Field(0.7, ge=0.0, le=1.0, description="Minimum confidence")


class TraceRequest(BaseModel):
    model_config = ConfigDict(str_strip_whitespace=True)

    entry_point: str = Field(..., description="Entry point function/method name")
    max_depth: int = Field(10, ge=1, le=50, description="Maximum depth")


class IndexRequest(BaseModel):
    model_config = ConfigDict(str_strip_whitespace=True)

    repo: str = Field(
        ...,
        description="Repository path or Git URL (e.g., '/path/to/repo' or 'https://gitlab.com/user/repo.git')",
    )
    repo_name: str | None = Field(None, description="Unique repository name (e.g., 'skills-api')")
    branch: str = Field("main", description="Git branch or revision to clone (default: 'main')")
    force: bool = Field(False, description="Force re-indexing")


class WikiRequest(BaseModel):
    model_config = ConfigDict(str_strip_whitespace=True)

    repo_path: str = Field(..., description="Path to repository")
    force: bool = Field(False, description="Force regeneration")


# Global instances
graph: KnowledgeGraph | None = None
query_engine: WikiEnhancedQueryEngine | None = None
indexer: PythonIndexer | None = None
process_detector: ProcessDetector | None = None


@asynccontextmanager
async def lifespan(app: FastAPI):
    """Manage application lifecycle"""
    global graph, query_engine, indexer, process_detector

    settings = get_settings()
    logger.info("Starting CoMind API server", environment=settings.environment)

    # Initialize components
    logger.info("Initializing knowledge graph", backend="duckdb")
    from comind.config import get_settings

    settings = get_settings()
    backend = DuckDBBackend(str(settings.storage.duckdb_path))
    graph = GraphAdapter(backend)

    logger.info("Initializing query engine")
    query_engine = WikiEnhancedQueryEngine(graph)

    # Note: WikiGenerator is now created on-demand in the /api/wiki/generate endpoint
    # with LLM configuration from environment variables or request parameters

    logger.info("Initializing indexer")
    indexer = PythonIndexer(graph, query_engine)

    logger.info("Initializing process detector")
    process_detector = ProcessDetector(graph)

    # Initialize search indexes
    logger.info("Initializing search indexes")
    await query_engine.initialize_search_indexes()

    # Auto-load persisted indexes from disk
    logger.info("Loading persisted indexes from disk")
    loaded_repos = await _load_persisted_indexes()
    if loaded_repos:
        logger.info("Loaded indexes for repositories", repos=loaded_repos)
    else:
        logger.info("No persisted indexes found")

    logger.info("CoMind API server started successfully")
    yield

    # Cleanup
    logger.info("Shutting down CoMind API server")


async def _load_persisted_indexes() -> list[str]:
    """Load persisted indexes and graphs from centralized storage on startup"""
    settings = get_settings()

    # Load all indexes from centralized storage
    loaded_repos = await query_engine.load_all_indexes(settings.storage.indexes_dir)

    # Load knowledge graphs and wiki pages for each repository
    for repo_name in loaded_repos:
        # Load knowledge graph (using repo_name directly)
        # DuckDB backend already persists data — no separate graph file needed
        logger.debug("Knowledge graph data is in DuckDB", repo_name=repo_name)

        # Load wiki pages into wiki store (using repo_name directly)
        wiki_dir = settings.storage.wiki_dir / repo_name / "wiki"
        if wiki_dir.exists():
            try:
                wiki_pages = await load_wiki_pages(str(wiki_dir))
                wiki_store = await query_engine.get_or_create_wiki_store(repo_name)

                for page in wiki_pages:
                    await wiki_store.add_page(
                        page_id=page.module_name,
                        title=page.title,
                        content=page.content,
                        metadata=page.metadata,
                    )

                # Build semantic index for wiki pages
                if wiki_store.semantic_engine:
                    await wiki_store.semantic_engine.build_index()

                logger.info("Loaded wiki pages", repo_name=repo_name, pages=len(wiki_pages))
            except Exception as e:
                logger.warning("Failed to load wiki pages", repo_name=repo_name, error=str(e))

    return loaded_repos


def create_app() -> FastAPI:
    """Create FastAPI application"""
    settings = get_settings()

    app = FastAPI(
        title="CoMind API",
        description="Graph-powered code intelligence for AI agents - Modern Python implementation",
        version=settings.app_version,
        lifespan=lifespan,
        debug=settings.debug,
    )

    # Add CORS middleware
    app.add_middleware(
        CORSMiddleware,
        allow_origins=settings.server.cors_origins,
        allow_credentials=True,
        allow_methods=["*"],
        allow_headers=["*"],
    )

    # Health check
    @app.get("/health")
    async def health_check() -> dict[str, Any]:
        """Health check endpoint"""
        return {
            "status": "healthy",
            "service": "comind",
            "version": settings.app_version,
            "environment": settings.environment,
        }

    @app.get("/")
    async def root() -> dict[str, Any]:
        """Root endpoint with API information"""
        return {
            "service": "CoMind API",
            "version": settings.app_version,
            "docs": "/docs",
            "health": "/health",
        }

    # Repository endpoints
    @app.post("/api/analyze")
    async def analyze_repository(request: IndexRequest) -> dict[str, Any]:
        """
        Complete repository analysis pipeline:
        1. Clone repository if Git URL (or use local path)
        2. Index code and build knowledge graph
        3. Build BM25S and semantic search indexes
        4. Generate LLM-powered wiki documentation
        5. Index wiki content for search
        6. Cleanup cloned repository if needed

        Supports both local paths and Git URLs!
        """
        start_time = time.time()

        # Determine if we need to clone or use local path
        is_git = is_git_url(request.repo)

        # Use repo_name if provided, otherwise derive from repo string
        repo_name = request.repo_name
        if not repo_name:
            if is_git:
                # Extract name from git URL (e.g., 'repo.git' -> 'repo')
                repo_name = Path(request.repo.rstrip("/")).stem.replace(".git", "")
            else:
                # Auto-generate from last path component
                repo_name = Path(request.repo).name

        if is_git:
            # Clone or update the repository in persistent storage
            repo_path, was_cloned = clone_or_pull_git_repo(request.repo, repo_name, request.branch)
            action = "cloned" if was_cloned else "updated"
            logger.info(
                f"Repository {action}",
                url=request.repo,
                branch=request.branch,
                path=repo_path,
                repo_name=repo_name,
            )
        else:
            # Use local path
            repo_path = request.repo
            logger.info("Using local repository", path=repo_path, repo_name=repo_name)

        logger.info(
            "Starting complete repository analysis",
            repo_path=repo_path,
            repo_name=repo_name,
            is_git=is_git,
        )

        try:
            # Phase 1: Index repository and build knowledge graph
            logger.info("Phase 1: Indexing repository and building knowledge graph")

            # Use repo_name as the identifier for all indexes
            repo_id = repo_name
            index_result = await indexer.index_repository(repo_path=repo_path, repo_id=repo_id)

            if "error" in index_result:
                logger.error("Indexing failed", error=index_result["error"])
                raise HTTPException(status_code=400, detail=index_result["error"])

            # Store repo_root in query engine for snippet extraction
            from pathlib import Path

            query_engine.repo_roots[repo_id] = Path(repo_path)
            logger.info(
                "Phase 1 complete",
                symbols=index_result.get("total_symbols", 0),
                relationships=index_result.get("total_relationships", 0),
                processes=index_result.get("processes_detected", 0),
            )

            # Phase 2: Save indexes and graph to centralized storage
            logger.info("Phase 2: Saving search indexes and graph to centralized storage")
            settings = get_settings()

            # Save with repo_name as directory name (store repo_path in metadata)
            await query_engine.save_repo_index(repo_id, settings.storage.indexes_dir, repo_path)

            # DuckDB backend auto-persists — no separate graph save needed

            logger.info("Phase 2 complete: Indexes and graph saved")

            # Phase 3: Generate wiki documentation
            logger.info("Phase 3: Generating LLM-powered wiki documentation")

            wiki_result = None
            wiki_error = None
            # Get LLM config from environment
            llm_config = {}

            def on_progress(phase: str, percent: int, detail: str | None = None):
                logger.info("Wiki generation progress", phase=phase, percent=percent, detail=detail)

            # Use centralized wiki storage with repo_name
            wiki_storage = settings.storage.wiki_dir / repo_name

            wiki_result = await generate_wiki_docs(
                repo_path=repo_path,
                storage_path=str(wiki_storage),
                graph=graph,
                llm_config=llm_config,
                force=request.force,
                on_progress=on_progress,
            )

            logger.info(
                "Phase 3 complete", pages=wiki_result.pages_generated, mode=wiki_result.mode
            )

            # Phase 4: Index wiki content for search
            logger.info("Phase 4: Indexing wiki content for search")

            wiki_dir = wiki_storage / "wiki"
            wiki_pages = await load_wiki_pages(str(wiki_dir))

            # Add wiki pages to repo-specific wiki store
            wiki_store = await query_engine.get_or_create_wiki_store(repo_id)
            for page in wiki_pages:
                await wiki_store.add_page(
                    page_id=page.module_name,
                    title=page.title,
                    content=page.content,
                    metadata=page.metadata,
                )

            # Build semantic index for wiki pages
            if wiki_store.semantic_engine:
                await wiki_store.semantic_engine.build_index()

            # Save wiki store to disk
            wiki_index_dir = settings.storage.indexes_dir / repo_name
            await wiki_store.save(str(wiki_index_dir))

            logger.info("Phase 4 complete", wiki_pages_indexed=len(wiki_pages))

            # Phase 4.5: Generate graph-native wikis for nodes and relationships
            logger.info("Phase 4.5: Generating graph-native wikis from knowledge graph")

            from comind.wiki.graph_wiki_generator import GraphWikiGenerator

            # Get snippet extractor for the repo
            snippet_extractor = query_engine.get_or_create_snippet_extractor(repo_id)

            # Create graph wiki generator (with LLM if available)
            graph_wiki_dir = settings.storage.wiki_dir / repo_name / "graph_wikis"
            graph_wiki_generator = GraphWikiGenerator(
                graph=graph,
                snippet_extractor=snippet_extractor,
                llm_client=None,  # TODO: Add LLM client when available
                output_dir=graph_wiki_dir,
            )

            # Generate wikis for all nodes and relationships
            try:
                wiki_stats = await graph_wiki_generator.generate_all_wikis(
                    repo_id=repo_id, batch_size=10
                )
                logger.info(
                    "Phase 4.5 complete",
                    nodes_generated=wiki_stats["nodes_generated"],
                    relationships_generated=wiki_stats["relationships_generated"],
                )
            except Exception as e:
                logger.error(f"Graph wiki generation failed: {e}", exc_info=True)
                # Continue even if graph wiki generation fails

            # Phase 5: Generate style guide
            logger.info("Phase 5: Generating style guide from code patterns")

            style_patterns = await extract_style_guide(graph, repo_id)

            style_guide_path = wiki_storage / f"{repo_name.upper()}_STYLE_GUIDE.md"
            await generate_style_guide_markdown(style_patterns, repo_name, str(style_guide_path))

            logger.info("Phase 5 complete", style_guide_path=str(style_guide_path))

            # Calculate total time
            elapsed = time.time() - start_time

            logger.info(
                "Repository analysis complete", repo_path=repo_path, total_time_seconds=elapsed
            )

            return {
                "repo_path": repo_path,
                "repo_id": repo_id,
                "status": "success",
                "phases": {
                    "indexing": {
                        "status": "completed",
                        "symbols_indexed": index_result.get("total_symbols", 0),
                        "relationships_indexed": index_result.get("total_relationships", 0),
                        "processes_detected": index_result.get("processes_detected", 0),
                        "files_processed": index_result.get("files_processed", 0),
                    },
                    "search_indexes": {"status": "completed", "bm25s": True, "semantic": True},
                    "wiki_generation": {
                        "status": "completed" if wiki_result else "failed",
                        "pages_generated": wiki_result.pages_generated if wiki_result else 0,
                        "error": wiki_error,
                    },
                    "wiki_indexing": {
                        "status": "completed" if wiki_result and not wiki_error else "skipped",
                        "pages_indexed": len(wiki_pages) if wiki_result and not wiki_error else 0,
                    },
                    "style_guide": {
                        "status": "completed",
                        "output_path": str(style_guide_path),
                        "type_hints_coverage": f"{style_patterns.type_hints_usage.percentage:.1f}%",
                        "async_functions": f"{style_patterns.async_usage.percentage:.1f}%",
                    },
                },
                "elapsed_seconds": elapsed,
            }

        except Exception as e:
            logger.error("Repository analysis failed", error=str(e), repo_name=repo_name)
            raise

    @app.get("/api/repos")
    async def list_repos() -> dict[str, Any]:
        """List all indexed repositories"""
        repos = []

        # Get repositories from query engine indexes
        for repo_id in query_engine.repo_text_engines.keys():
            repos.append(
                {
                    "repo_id": repo_id,
                    "indexed": True,
                    "has_text_index": repo_id in query_engine.repo_text_engines,
                    "has_semantic_index": repo_id in query_engine.repo_semantic_engines,
                    "has_wiki": repo_id in query_engine.repo_wiki_stores,
                }
            )

        # Also get from graph
        graph_repos = await graph.list_repositories()

        return {
            "repos": repos if repos else graph_repos,
            "total": len(repos) if repos else len(graph_repos),
        }

    @app.get("/api/repos/{repo_id}/stats")
    async def get_repo_stats(repo_id: str) -> dict[str, Any]:
        """Get repository status and statistics"""
        stats = await graph.get_repository_stats(repo_id)

        if not stats:
            logger.warning("Repository not found", repo_id=repo_id)
            raise HTTPException(status_code=404, detail="Repository not found")

        return stats

    @app.delete("/api/repos/{repo_name}")
    async def delete_repository(repo_name: str) -> dict[str, Any]:
        """Delete repository indexes, graph, and wiki data

        Args:
            repo_name: Repository name (e.g., 'skills-api')
        """
        import shutil

        logger.info("Deleting repository data", repo_name=repo_name)

        # Check if repo exists
        if repo_name not in query_engine.repo_text_engines:
            raise HTTPException(status_code=404, detail="Repository not found")

        settings = get_settings()
        deleted_items = []

        # Delete from memory
        await query_engine.clear_repo_index(repo_name)
        deleted_items.append("in-memory indexes")

        # Delete search indexes from disk (using repo_name directly)
        index_dir = settings.storage.indexes_dir / repo_name
        if index_dir.exists():
            shutil.rmtree(index_dir)
            deleted_items.append("search indexes")

        # Delete knowledge graph from disk (using repo_name directly)
        graph_file = settings.storage.graphs_dir / f"{repo_name}.pkl"
        if graph_file.exists():
            graph_file.unlink()
            deleted_items.append("knowledge graph")

        # Delete wiki from disk (using repo_name directly)
        wiki_dir = settings.storage.wiki_dir / repo_name
        if wiki_dir.exists():
            shutil.rmtree(wiki_dir)
            deleted_items.append("wiki content")

        # Delete cloned repository from disk (if it was a git clone)
        repo_dir = settings.storage.repos_dir / repo_name
        if repo_dir.exists():
            shutil.rmtree(repo_dir)
            deleted_items.append("cloned repository")

        logger.info("Repository deleted", repo_name=repo_name, deleted=deleted_items)

        return {"repo_name": repo_name, "status": "deleted", "deleted_items": deleted_items}

    @app.get("/api/repos/{repo_name}/search/code")
    async def search_code(
        repo_name: str,
        query: str,
        case_sensitive: bool = False,
        file_pattern: str | None = None,
        max_results: int = 50,
    ) -> dict[str, Any]:
        """Search code in repository using grep

        Args:
            repo_name: Repository name
            query: Search query (supports regex)
            case_sensitive: Whether search is case-sensitive
            file_pattern: Optional file pattern (e.g., '*.py', '*.js')
            max_results: Maximum number of results
        """
        settings = get_settings()
        repo_dir = settings.storage.repos_dir / repo_name

        if not repo_dir.exists():
            raise HTTPException(
                status_code=404,
                detail=f"Repository '{repo_name}' not found. Clone it first with POST /api/analyze",
            )

        logger.info(
            "Searching code", repo_name=repo_name, query=query, case_sensitive=case_sensitive
        )

        # Build grep command
        cmd = ["grep", "-r", "-n"]  # Recursive, show line numbers

        if not case_sensitive:
            cmd.append("-i")  # Case insensitive

        if file_pattern:
            cmd.extend(["--include", file_pattern])

        # Exclude common directories
        cmd.extend(
            [
                "--exclude-dir=.git",
                "--exclude-dir=__pycache__",
                "--exclude-dir=node_modules",
                "--exclude-dir=.venv",
                "--exclude-dir=venv",
            ]
        )

        cmd.extend([query, str(repo_dir)])

        try:
            result = subprocess.run(cmd, check=False, capture_output=True, text=True, timeout=30)

            # grep returns 1 if no matches found, which is not an error
            if result.returncode not in (0, 1):
                raise HTTPException(status_code=500, detail=f"Grep search failed: {result.stderr}")

            # Parse results
            matches = []
            for line in result.stdout.splitlines()[:max_results]:
                # Format: filepath:line_number:content
                parts = line.split(":", 2)
                if len(parts) >= 3:
                    filepath = parts[0].replace(str(repo_dir) + "/", "")
                    line_number = parts[1]
                    content = parts[2]

                    matches.append(
                        {
                            "file": filepath,
                            "line": int(line_number) if line_number.isdigit() else 0,
                            "content": content.strip(),
                            "match": line,
                        }
                    )

            logger.info("Code search complete", repo_name=repo_name, matches=len(matches))

            return {
                "repo_name": repo_name,
                "query": query,
                "matches": matches,
                "total": len(matches),
                "truncated": len(result.stdout.splitlines()) > max_results,
            }

        except subprocess.TimeoutExpired:
            raise HTTPException(status_code=408, detail="Search timeout (exceeded 30 seconds)")
        except Exception as e:
            logger.error("Code search failed", error=str(e), repo_name=repo_name)
            raise HTTPException(status_code=500, detail=f"Search failed: {e!s}")

    @app.post("/api/repos/{repo_name}/style-guide")
    async def generate_style_guide(repo_name: str) -> dict[str, Any]:
        """Generate a comprehensive style guide from repository patterns

        Analyzes the repository's code patterns, conventions, and practices
        to create a team-specific style guide that AI agents and developers
        can follow for consistency.

        Args:
            repo_name: Repository name (e.g., 'skills-api')
        """
        logger.info("Generating style guide", repo_name=repo_name)

        # Check if repo exists
        if repo_name not in query_engine.repo_text_engines:
            raise HTTPException(status_code=404, detail="Repository not found")

        start_time = time.time()

        # Extract patterns from knowledge graph
        patterns = await extract_style_guide(graph, repo_name)

        # Generate markdown
        settings = get_settings()
        output_dir = settings.storage.wiki_dir / repo_name
        output_dir.mkdir(parents=True, exist_ok=True)
        output_path = output_dir / f"{repo_name.upper()}_STYLE_GUIDE.md"

        markdown = await generate_style_guide_markdown(patterns, repo_name, str(output_path))

        elapsed = time.time() - start_time

        logger.info(
            "Style guide generated",
            repo_name=repo_name,
            output_path=str(output_path),
            elapsed_seconds=elapsed,
        )

        return {
            "repo_name": repo_name,
            "status": "success",
            "output_path": str(output_path),
            "patterns_analyzed": {
                "type_hints_coverage": f"{patterns.type_hints_usage.percentage:.1f}%",
                "async_functions": f"{patterns.async_usage.percentage:.1f}%",
                "docstring_coverage": f"{patterns.docstring_coverage.percentage:.1f}%",
                "dominant_naming": {
                    "functions": patterns.function_naming.most_common(1)[0][0]
                    if patterns.function_naming
                    else "snake_case",
                    "classes": patterns.class_naming.most_common(1)[0][0]
                    if patterns.class_naming
                    else "PascalCase",
                },
            },
            "markdown_preview": markdown[:500] + "...",
            "elapsed_seconds": elapsed,
        }

    # Query endpoints
    @app.post("/api/search")
    async def search(request: SearchRequest):
        """Search for code symbols in a specific repository

        Supports multiple output formats:
        - json: Full JSON response (default)
        - markdown: Formatted markdown for AI agents
        - compact: Ultra-compact markdown (minimal tokens)
        """
        start_time = time.time()
        logger.info(
            "Search request",
            query=request.query,
            repo_name=request.repo_name,
            format=request.format,
        )

        # Enforce repo_name requirement
        if not request.repo_name:
            raise HTTPException(
                status_code=400, detail="repo_name is required - global search is not supported"
            )

        results = await query_engine.search(
            query=request.query,
            repo_id=request.repo_name,  # query_engine still uses repo_id internally
            include_wiki=request.include_wiki,
            include_snippets=request.include_snippets,
            max_results=request.max_results,
        )

        # Check for errors from query engine
        if "error" in results:
            logger.warning("Search error", error=results["error"], repo_name=request.repo_name)
            raise HTTPException(status_code=404, detail=results["error"])

        elapsed_ms = (time.time() - start_time) * 1000
        logger.info(
            "Search completed",
            query=request.query,
            repo_name=request.repo_name,
            results=len(results.get("results", [])),
            elapsed_ms=elapsed_ms,
            format=request.format,
        )

        # Format output based on request
        if request.format == "markdown":
            markdown = MarkdownFormatter.format_search_results(
                query=request.query,
                results=results.get("results", []),
                total_results=results.get("total_results", 0),
                compact=False,
                include_wiki=request.include_wiki,
                include_graph=request.include_graph,
                include_code=request.include_code,
            )
            return PlainTextResponse(content=markdown, media_type="text/markdown")
        if request.format == "compact":
            markdown = MarkdownFormatter.format_compact_results(
                query=request.query,
                results=results.get("results", []),
                total_results=results.get("total_results", 0),
            )
            return PlainTextResponse(content=markdown, media_type="text/markdown")
        # Default JSON format
        return results

    @app.post("/api/symbols/context")
    async def get_context(request: ContextRequest) -> dict[str, Any]:
        """Get 360-degree symbol context"""
        logger.info("Context request", symbol_id=request.symbol_id, symbol_name=request.symbol_name)

        result = await query_engine.get_context(
            symbol_name=request.symbol_name,
            file_path=request.file_path,
            depth=request.depth,
            include_wiki=request.include_wiki,
        )

        if "error" in result:
            logger.warning("Symbol not found", symbol_name=request.symbol_name)
            raise HTTPException(status_code=404, detail=result["error"])

        return result

    @app.post("/api/symbols/impact")
    async def analyze_impact(request: ImpactRequest) -> dict[str, Any]:
        """Analyze blast radius of changing a symbol"""
        logger.info("Impact analysis", symbol_name=request.symbol_name, direction=request.direction)

        result = await query_engine.analyze_impact(
            symbol_name=request.symbol_name,
            direction=request.direction,
            max_depth=request.max_depth,
            min_confidence=request.min_confidence,
        )

        if "error" in result:
            logger.warning("Symbol not found for impact analysis", symbol_name=request.symbol_name)
            raise HTTPException(status_code=404, detail=result["error"])

        return result

    @app.post("/api/trace")
    async def trace_execution(request: TraceRequest) -> dict[str, Any]:
        """Trace execution flow from an entry point"""
        logger.info("Execution trace", entry_point=request.entry_point)

        result = await query_engine.trace_execution(
            entry_point=request.entry_point, max_depth=request.max_depth
        )

        if "error" in result:
            logger.warning("Entry point not found", entry_point=request.entry_point)
            raise HTTPException(status_code=404, detail=result["error"])

        return result

    # Wiki endpoints
    @app.post("/api/wiki/generate")
    async def generate_wiki(request: WikiRequest) -> dict[str, Any]:
        """Generate LLM-powered wiki documentation from knowledge graph"""
        logger.info("Generating wiki", repo_path=request.repo_path, force=request.force)

        # Get LLM config from environment or request
        llm_config = {}
        if hasattr(request, "model"):
            llm_config["model"] = request.model
        if hasattr(request, "api_key"):
            llm_config["api_key"] = request.api_key

        # Progress tracking
        progress_data = {"phase": "", "percent": 0, "detail": ""}

        def on_progress(phase: str, percent: int, detail: str | None = None):
            progress_data["phase"] = phase
            progress_data["percent"] = percent
            progress_data["detail"] = detail or ""
            logger.info("Wiki generation progress", **progress_data)

        result = await generate_wiki_docs(
            repo_path=request.repo_path,
            storage_path=str(Path(request.repo_path) / ".comind"),
            graph=graph,
            llm_config=llm_config,
            force=request.force,
            on_progress=on_progress,
        )

        logger.info(
            "Wiki generated",
            pages=result.pages_generated,
            mode=result.mode,
            failed=len(result.failed_modules),
        )

        return {
            "pages_generated": result.pages_generated,
            "mode": result.mode,
            "failed_modules": result.failed_modules,
            "wiki_dir": str(Path(request.repo_path) / ".comind" / "wiki"),
        }

    @app.get("/api/wiki/{repo_id}")
    async def get_wiki(repo_id: str) -> dict[str, Any]:
        """Get complete wiki for a repository"""
        wiki_dir = Path(repo_id) / ".comind" / "wiki"
        pages = await load_wiki_pages(str(wiki_dir))

        if not pages:
            raise HTTPException(
                status_code=404,
                detail="Wiki not found. Generate it first with POST /api/wiki/generate",
            )

        # Return overview page (README.md)
        readme_path = wiki_dir / "README.md"
        if readme_path.exists():
            with open(readme_path) as f:
                overview_content = f.read()
            return {
                "title": "Overview",
                "content": overview_content,
                "pages": [{"module_name": p.module_name, "title": p.title} for p in pages],
            }

        return {"pages": [p.to_dict() for p in pages]}

    @app.get("/api/wiki/{repo_id}/modules/{module_name}")
    async def get_module_wiki(repo_id: str, module_name: str) -> dict[str, Any]:
        """Get module-specific documentation"""
        wiki_dir = Path(repo_id) / ".comind" / "wiki"
        pages = await load_wiki_pages(str(wiki_dir))

        page = next((p for p in pages if p.module_name == module_name), None)

        if not page:
            raise HTTPException(status_code=404, detail="Module wiki not found")

        return page.to_dict()

    # Graph endpoints
    @app.post("/api/graph/query")
    async def graph_query(
        query: str = Body(..., description="Graph query string"),
        repo_id: str | None = Body(None, description="Repository ID"),
    ) -> dict[str, Any]:
        """Execute graph query"""
        results = await graph.query(query)
        return {"results": results}

    @app.get("/api/graph/schema")
    async def get_graph_schema() -> dict[str, Any]:
        """Get graph schema"""
        return {
            "nodes": {
                "types": [
                    "file",
                    "module",
                    "class",
                    "function",
                    "method",
                    "variable",
                    "import",
                    "community",
                    "process",
                ],
                "properties": [
                    "id",
                    "name",
                    "type",
                    "file_path",
                    "line_start",
                    "line_end",
                    "signature",
                    "docstring",
                ],
            },
            "relationships": {
                "types": [
                    "contains",
                    "imports",
                    "calls",
                    "inherits",
                    "implements",
                    "member_of",
                    "participates_in",
                    "defines",
                    "uses",
                ],
                "properties": ["source_id", "target_id", "type", "confidence"],
            },
            "query_examples": [
                "Find all functions that call 'authenticate_user'",
                "Get all classes in the 'auth' module",
                "Trace execution from 'main' function",
            ],
        }

    @app.get("/api/graph/communities")
    async def get_communities() -> dict[str, Any]:
        """Get detected communities"""
        communities = await graph.get_communities()
        return {"communities": communities, "total": len(communities)}

    @app.get("/api/graph/processes")
    async def get_processes() -> dict[str, Any]:
        """Get detected processes"""
        processes = await process_detector.get_processes()
        return {"processes": [process.__dict__ for process in processes], "total": len(processes)}

    @app.get("/api/graph/processes/{process_id}")
    async def get_process(process_id: str) -> dict[str, Any]:
        """Get a specific process by ID"""
        process = await process_detector.get_process_by_id(process_id)
        if not process:
            raise HTTPException(status_code=404, detail="Process not found")
        return process.__dict__

    @app.get("/api/graph/stats")
    async def get_graph_stats() -> dict[str, Any]:
        """Get graph statistics"""
        try:
            stats_row = graph.backend.conn.execute("SELECT COUNT(*) FROM symbols").fetchone()
            symbol_count = stats_row[0] if stats_row else 0

            rel_row = graph.backend.conn.execute("SELECT COUNT(*) FROM relationships").fetchone()
            relationship_count = rel_row[0] if rel_row else 0

            type_rows = graph.backend.conn.execute(
                "SELECT type, COUNT(*) FROM symbols GROUP BY type"
            ).fetchall()
            symbol_types = {row[0]: row[1] for row in type_rows}
        except Exception:
            symbol_count = 0
            relationship_count = 0
            symbol_types = {}

        return {
            "symbols": symbol_count,
            "relationships": relationship_count,
            "symbol_types": symbol_types,
        }

    # Code snippet endpoints
    @app.get("/api/code/snippet/{symbol_id}")
    async def get_code_snippet(
        symbol_id: str, context_lines: int = 5, include_structure: bool = True
    ) -> dict[str, Any]:
        """Get code snippet with context"""
        symbol = await graph.get_symbol(symbol_id)

        if not symbol:
            raise HTTPException(status_code=404, detail="Symbol not found")

        # Extract snippet
        extractor = CodeSnippetExtractor()

        if symbol.type.value in ["function", "method"]:
            snippet = await extractor.extract_function_snippet(symbol)
        elif symbol.type.value == "class":
            snippet = await extractor.extract_class_snippet(symbol)
        else:
            snippet = await extractor.extract_snippet(symbol, context_lines)

        return snippet

    @app.get("/api/code/file/{repo_id}/{file_path:path}")
    async def get_file(repo_id: str, file_path: str) -> dict[str, Any]:
        """Get complete file content"""
        # This is a simplified implementation
        # In practice, you'd validate the file belongs to the repo
        from pathlib import Path

        file_obj = Path(file_path)
        if not file_obj.exists():
            raise HTTPException(status_code=404, detail="File not found")

        with open(file_obj, encoding="utf-8") as f:
            content = f.read()

        return {
            "file_path": file_path,
            "content": content,
            "language": "python",  # Would detect from extension
            "size": len(content),
            "lines": len(content.split("\n")),
        }

    # MCP endpoint (for HTTP transport)
    @app.post("/api/mcp/message")
    async def handle_mcp_message(message: dict[str, Any]) -> dict[str, Any]:
        """Handle MCP protocol messages"""
        # This would implement the MCP protocol over HTTP
        # For now, return a placeholder
        return {
            "jsonrpc": "2.0",
            "id": message.get("id"),
            "result": {"message": "MCP over HTTP not fully implemented yet", "received": message},
        }

    return app


# Create app instance
app = create_app()


# For running directly with uvicorn
if __name__ == "__main__":
    import uvicorn

    settings = get_settings()
    uvicorn.run(
        app,
        host=settings.server.host,
        port=settings.server.port,
        reload=settings.server.reload,
        log_level=settings.server.log_level,
    )
