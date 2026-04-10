"""
CoMind CLI

Commands
--------
  analyze  Index a repository into the knowledge graph
  search   Semantic + BM25 code search
  repos    List all indexed repositories
  serve    Start the REST API server
  mcp      Start the MCP server (stdio or http)
"""

from __future__ import annotations

import asyncio
import json
import logging
import os
import re
import shutil
import subprocess
import time
from contextlib import suppress
from pathlib import Path
from typing import Annotated

import git
import typer
from rich.console import Console
from rich.panel import Panel
from rich.progress import (
    BarColumn,
    Progress,
    SpinnerColumn,
    TaskProgressColumn,
    TextColumn,
    TimeElapsedColumn,
)
from rich.table import Table

from comind.config import get_settings
from comind.indexing.incremental_indexer import IncrementalIndexer
from comind.indexing.indexer import PythonIndexer
from comind.llm.llm_client import LLMClient, resolve_llm_config
from comind.llm.query_association_indexer import QueryAssociationIndexer
from comind.models import (
    FindResponse,
    FindResult,
    IngestResult,
    RepoInfo,
    ReposResponse,
)
from comind.search.duckdb_search_engine import DuckDBSemanticSearchEngine, create_search_engines
from comind.search.query_engine import WikiEnhancedQueryEngine
from comind.storage.duckdb_backend import DuckDBBackend
from comind.storage.graph_adapter import GraphAdapter
from comind.style.style_extractor import extract_style_guide
from comind.style.style_guide_generator import generate_style_guide_markdown
from comind.style.style_guide_store import StyleGuideStore
from comind.utils.markdown_formatter import MarkdownFormatter
from comind.utils.snippet_extractor import CodeSnippetExtractor
from comind.wiki.graph_wiki_generator import GraphWikiGenerator
from comind.wiki.wiki import generate_wiki as _gen_wiki, load_wiki_pages

# Silence noisy third-party libraries; keep WARNING+ for all others so
# real errors are still visible.  Rich console is the primary UI output.
for _noisy_lib in (
    "fastembed",
    "bm25s",
    "httpx",
    "httpcore",
    "urllib3",
    "filelock",
    "huggingface_hub",
    "tokenizers",
    "onnxruntime",
):
    logging.getLogger(_noisy_lib).setLevel(logging.WARNING)
os.environ["TQDM_DISABLE"] = "1"

# Monkey-patch tqdm so every bar is created with disable=True.
# The env-var alone isn't enough because libraries like bm25s may pass
# disable=False explicitly, overriding TQDM_DISABLE.
try:
    import tqdm as _tqdm_module

    _real_tqdm_init = _tqdm_module.tqdm.__init__

    def _silent_tqdm_init(self, *args, **kwargs):
        kwargs["disable"] = True
        _real_tqdm_init(self, *args, **kwargs)

    _tqdm_module.tqdm.__init__ = _silent_tqdm_init
except Exception:
    pass

console = Console()
err = Console(stderr=True)

app = typer.Typer(
    name="comind",
    help="CoMind — graph-powered AI coding assistant",
    no_args_is_help=True,
    pretty_exceptions_enable=False,
    rich_markup_mode="rich",
)

OutputFmt = Annotated[str, typer.Option("--output", "-o", help="json | markdown")]


# ─── helpers ─────────────────────────────────────────────────────────────────


def _run(coro):  # type: ignore[no-untyped-def]
    return asyncio.run(coro)


def _is_git_url(repo: str) -> bool:
    return repo.startswith(("http://", "https://", "git@", "ssh://"))


def _repo_name_from(repo: str, *, is_git: bool) -> str:
    if is_git:
        return Path(repo.rstrip("/")).stem.replace(".git", "")
    return Path(repo).name


def _clone_or_pull(repo_url: str, repo_name: str, branch: str) -> tuple[str, bool]:
    """Clone or pull a git repo into ~/.comind/data/repos/<name>.

    Returns (local_path, was_cloned).  Raises RuntimeError on failure.
    """
    settings = get_settings()
    repo_dir = settings.storage.repos_dir / repo_name
    settings.storage.repos_dir.mkdir(parents=True, exist_ok=True)

    clone_url = repo_url
    gitlab_token = os.getenv("GITLAB_API_PRIVATE_TOKEN")
    if gitlab_token:
        gitlab_token = gitlab_token.strip()
    if gitlab_token and "gitlab.com" in repo_url:
        # Strip any existing credentials from URL
        clean_url = re.sub(r"https://[^@]+@gitlab\.com", "https://gitlab.com", repo_url)
        if clean_url.startswith("https://gitlab.com"):
            clone_url = clean_url.replace(
                "https://gitlab.com", f"https://oauth2:{gitlab_token}@gitlab.com"
            )

    if repo_dir.exists() and (repo_dir / ".git").exists():
        try:
            r = git.Repo(repo_dir)
            origin = r.remotes.origin
            origin.fetch()
            if branch in r.heads:
                r.heads[branch].checkout()
            else:
                r.git.checkout(branch, track=f"origin/{branch}")
            origin.pull(branch)
        except git.GitCommandError as exc:
            err.print(f"[yellow]Warning:[/] git pull failed ({exc}), using existing clone")
        return str(repo_dir), False

    if repo_dir.exists():
        shutil.rmtree(repo_dir, ignore_errors=True)

    try:
        env = os.environ.copy()
        env["GIT_TERMINAL_PROMPT"] = "0"

        cmd = [
            "git",
            "clone",
            "--branch",
            branch,
            "--depth",
            "1",
            "-c",
            "credential.helper=",
            clone_url,
            str(repo_dir),
        ]

        result = subprocess.run(cmd, check=False, env=env, capture_output=True, text=True)
    except Exception as exc:
        shutil.rmtree(repo_dir, ignore_errors=True)
        raise RuntimeError(f"git clone failed: {exc}") from exc

    if result.returncode != 0:
        shutil.rmtree(repo_dir, ignore_errors=True)
        raise RuntimeError(f"git clone failed: {result.stderr}")

    return str(repo_dir), True


async def _load_engine(repo_name: str | None = None):
    """Initialise engine and load persisted state from ~/.comind/data."""
    settings = get_settings()

    # Load single shared DuckDB database
    db_path = settings.storage.duckdb_path
    if not db_path.exists():
        return None, None, []

    backend = DuckDBBackend(str(db_path))

    # Wrap in adapter for compatibility
    graph = GraphAdapter(backend)

    qe = WikiEnhancedQueryEngine(graph)
    await qe.initialize_search_indexes()

    # Register DuckDB-backed search engines for all repos before loading

    text_engine, semantic_engine = create_search_engines(graph)

    loaded = await qe.load_all_indexes(settings.storage.indexes_dir)

    for rname in loaded:
        # Ensure DuckDB-backed search engines are available for every repo
        if rname not in qe.repo_text_engines:
            qe.repo_text_engines[rname] = text_engine
        if rname not in qe.repo_semantic_engines:
            qe.repo_semantic_engines[rname] = semantic_engine
        if repo_name and rname != repo_name:
            continue
        wiki_dir = settings.storage.wiki_dir / rname / "wiki"
        if wiki_dir.exists():
            try:
                pages = await load_wiki_pages(str(wiki_dir))
                ws = await qe.get_or_create_wiki_store(rname)
                for page in pages:
                    await ws.add_page(
                        page_id=page.module_name,
                        title=page.title,
                        content=page.content,
                        metadata=page.metadata,
                    )
                if ws.semantic_engine:
                    await ws.semantic_engine.build_index()
            except Exception:
                pass

    return graph, qe, loaded


def _require_repo(repo_name: str, loaded: list[str]) -> None:
    if repo_name not in loaded:
        err.print(
            f"[red]Error:[/] repository '[cyan]{repo_name}[/]' not indexed.\n"
            "  Run [bold]comind analyze[/] to index it first."
        )
        raise typer.Exit(1)


def _resolve_repo_root(repo_name: str) -> Path:
    """Return the local root directory for an indexed repo.

    Reads repo_path from metadata.json (preferred) or repo_path.txt (legacy).
    Falls back to the default git clone location.
    """
    settings = get_settings()
    repo_index_dir = settings.storage.indexes_dir / repo_name

    # Preferred: metadata.json (written by save_repo_index)
    metadata_file = repo_index_dir / "metadata.json"
    if metadata_file.exists():
        try:
            meta = json.loads(metadata_file.read_text())
            rp = meta.get("repo_path")
            if rp:
                root = Path(rp)
                if root.exists():
                    return root
        except Exception:
            pass

    # fallback: git clone location
    fallback = settings.storage.repos_dir / repo_name
    if fallback.exists():
        return fallback
    err.print(
        f"[red]Error:[/] cannot resolve local path for '[cyan]{repo_name}[/]'.\n"
        "  Re-run [bold]comind analyze[/] to fix."
    )
    raise typer.Exit(1)


# ─── analyze ─────────────────────────────────────────────────────────────────


@app.command()
def analyze(
    repo: Annotated[str, typer.Argument(help="Local path or Git URL to analyze")],
    repo_name: Annotated[
        str | None, typer.Option("--name", "-n", help="Name for this repo")
    ] = None,
    branch: Annotated[str, typer.Option("--branch", "-b", help="Git branch")] = "main",
    force: Annotated[bool, typer.Option("--force", "-f", help="Force full re-index")] = False,
    gen_wiki: Annotated[
        bool,
        typer.Option(
            "--gen-wiki/--no-gen-wiki", help="Generate wiki documentation (requires OPENAI_API_KEY)"
        ),
    ] = False,
    gen_style: Annotated[
        bool,
        typer.Option(
            "--gen-style/--no-gen-style",
            help="Extract coding style guide (requires OPENAI_API_KEY)",
        ),
    ] = False,
    gen_queries: Annotated[
        bool, typer.Option("--gen-queries/--no-gen-queries", help="Generate query associations")
    ] = False,
) -> None:
    """Analyze a repository: index symbols, build knowledge graph, generate wiki and style guide.

    Accepts a local path or any Git URL (HTTPS / SSH).

    By default runs symbol indexing, wiki generation, and style guide extraction.
    Use [bold]--no-gen-wiki[/] or [bold]--no-gen-style[/] to skip specific phases.
    Use [bold]--gen-queries[/] to generate natural language query associations (requires LLM).
    """
    _run(
        _ingest(
            repo,
            repo_name,
            branch,
            force,
            gen_wiki=gen_wiki,
            gen_style=gen_style,
            gen_queries=gen_queries,
        )
    )


async def _ingest(
    repo: str,
    repo_name: str | None,
    branch: str,
    force: bool,
    *,
    gen_wiki: bool = True,
    gen_style: bool = True,
    gen_queries: bool = False,
) -> None:
    settings = get_settings()
    start = time.time()
    is_git = _is_git_url(repo)
    if not repo_name:
        repo_name = _repo_name_from(repo, is_git=is_git)

    # ── resolve local path ────────────────────────────────────────────────
    if is_git:
        console.print(f"[bold]Cloning/updating[/] [cyan]{repo}[/] (branch: [cyan]{branch}[/])…")
        try:
            repo_path, was_cloned = _clone_or_pull(repo, repo_name, branch)
        except RuntimeError as exc:
            err.print(f"[red]Error:[/] {exc}")
            raise typer.Exit(1) from exc
        console.print(f"  [green]✓[/] {'Cloned' if was_cloned else 'Updated'}: {repo_path}")
    else:
        repo_path = str(Path(repo).resolve())
        if not Path(repo_path).exists():
            err.print(f"[red]Error:[/] path not found: {repo_path}")
            raise typer.Exit(1)
        console.print(f"[bold]Ingesting[/] [cyan]{repo_path}[/] as [cyan]{repo_name}[/]")

    repo_id = repo_name

    # ── init DuckDB backend (single shared database) ─────────────────────
    db_path = settings.storage.duckdb_path
    db_path.parent.mkdir(parents=True, exist_ok=True)
    backend = DuckDBBackend(str(db_path))
    incremental = IncrementalIndexer(backend)

    # Wrap backend in adapter for compatibility
    graph = GraphAdapter(backend)

    # Create DuckDB-based search engines
    text_engine, semantic_engine = create_search_engines(graph)

    qe = WikiEnhancedQueryEngine(graph)
    indexer = PythonIndexer(graph, qe)

    # Use DuckDB search engines instead of BM25S/FastEmbed
    qe.repo_text_engines[repo_id] = text_engine
    qe.repo_semantic_engines[repo_id] = semantic_engine

    await qe.initialize_search_indexes()

    total_phases = 2 + (3 if gen_wiki else 0) + (1 if gen_style else 0) + (1 if gen_queries else 0)
    phase = 0

    def _phase(label: str) -> str:
        nonlocal phase
        phase += 1
        return f"[cyan]{phase}/{total_phases}  {label}"

    wiki_pages_count = 0

    with Progress(
        SpinnerColumn(),
        TextColumn("[progress.description]{task.description}"),
        BarColumn(),
        TaskProgressColumn(),
        TimeElapsedColumn(),
        console=console,
    ) as prog:
        # phase — parse & index (with incremental updates)
        t = prog.add_task(_phase("Scanning repository…"), total=100, transient=True)

        if not force:
            # Detect changes for incremental update
            changed_files = await incremental.detect_changes(repo_id, repo_path)
            if changed_files:
                new_count = sum(1 for status in changed_files.values() if status == "new")
                modified_count = sum(1 for status in changed_files.values() if status == "modified")
                deleted_count = sum(1 for status in changed_files.values() if status == "deleted")
                prog.update(
                    t,
                    description=f"[cyan]Detected changes: {new_count} new, {modified_count} modified, {deleted_count} deleted[/]",
                )

        prog.update(t, description="[cyan]Parsing Python files…[/]")

        # Progress callback to update CLI in real-time with different phases
        def on_progress(phase, current, total, symbols, relationships):
            if phase == "parsing":
                pct = int((current / total) * 100) if total > 0 else 0
                prog.update(
                    t,
                    completed=pct * 0.6,  # Parsing is 60% of the work
                    description=f"[cyan]Parsing {current}/{total} files ({pct}%) - {symbols} symbols, {relationships} rels[/]",
                )
            elif phase == "bulk_insert":
                prog.update(
                    t,
                    completed=65,
                    description=f"[cyan]Building knowledge graph - {symbols} symbols, {relationships} rels[/]",
                )
            elif phase == "resolving_calls":
                prog.update(t, completed=75, description="[cyan]Resolving function calls…[/]")
            elif phase == "detecting_processes":
                prog.update(t, completed=85, description="[cyan]Detecting execution flows…[/]")
            elif phase == "generating_queries":
                prog.update(
                    t,
                    completed=95,
                    description="[cyan]Generating searchable queries for processes…[/]",
                )

        index_result = await indexer.index_repository(
            repo_path=repo_path, repo_id=repo_id, _force=force, progress_callback=on_progress
        )
        if "error" in index_result:
            err.print(f"[red]Indexing failed:[/] {index_result['error']}")
            raise typer.Exit(1)

        qe.repo_roots[repo_id] = Path(repo_path)
        syms = index_result.get("total_symbols", 0)
        rels = index_result.get("total_relationships", 0)
        procs = index_result.get("processes_detected", 0)
        files_processed = index_result.get("files_processed", 0)
        prog.update(
            t,
            completed=100,
            description=f"[green]✓[/]  {phase}/{total_phases}  Parsed {files_processed} files → {syms} symbols, {rels} relationships",
        )
        prog.stop_task(t)

        # phase — persist indexes (DuckDB auto-persists)
        t = prog.add_task(_phase("Persisting indexes…"), total=100, transient=True)
        settings.storage.indexes_dir.mkdir(parents=True, exist_ok=True)

        # Build search indexes before saving
        prog.update(t, completed=10)
        await qe.build_repo_index(repo_id)

        # Generate embeddings in bulk (stored directly in DuckDB symbol_embeddings)
        if isinstance(semantic_engine, DuckDBSemanticSearchEngine):
            prog.update(t, completed=30, description="[cyan]Generating semantic embeddings…[/]")
            try:
                n_embeddings = await semantic_engine.generate_embeddings_for_repo(repo_id)
                prog.update(
                    t, completed=80, description=f"[cyan]Generated {n_embeddings} embeddings[/]"
                )
            except Exception as _emb_err:
                prog.update(t, description=f"[yellow]⚠ Embeddings skipped: {_emb_err}[/]")

        await qe.save_repo_index(repo_id, settings.storage.indexes_dir, repo_path)

        # Register repository in DuckDB
        await backend.register_repository(
            repo_id=repo_id,
            name=repo_name,
            path=repo_path,
            branch=branch,
            metadata={"indexed_at": time.time()},
        )

        # Persist repo root path so grep/read can resolve it at query time
        (settings.storage.indexes_dir / repo_name / "repo_path.txt").write_text(repo_path)
        prog.update(
            t, completed=100, description=f"[green]✓[/]  {phase}/{total_phases}  Indexes persisted"
        )
        prog.stop_task(t)

        if gen_wiki:
            # phase — generate wiki docs
            t = prog.add_task(_phase("Generating wiki…"), total=100, transient=True)
            wiki_storage = settings.storage.wiki_dir / repo_name
            wiki_result = await _gen_wiki(
                repo_path=repo_path,
                storage_path=str(wiki_storage),
                graph=graph,
                llm_config={},
                force=force,
                on_progress=lambda _phase, _pct, _detail=None: None,
            )
            wiki_pages_count = wiki_result.pages_generated
            # If generator skipped (already exists, force=False), count files on disk
            if wiki_pages_count == 0:
                wiki_dir_check = wiki_storage / "wiki"
                if wiki_dir_check.exists():
                    wiki_pages_count = len(list(wiki_dir_check.glob("*.md")))
            prog.update(
                t,
                completed=100,
                description=f"[green]✓[/]  {phase}/{total_phases}  {wiki_pages_count} wiki pages",
            )
            prog.stop_task(t)

            # phase — index wiki content
            t = prog.add_task(_phase("Indexing wiki…"), total=100, transient=True)
            wiki_dir = wiki_storage / "wiki"
            wiki_pages = await load_wiki_pages(str(wiki_dir))
            ws = await qe.get_or_create_wiki_store(repo_id)
            for page in wiki_pages:
                await ws.add_page(
                    page_id=page.module_name,
                    title=page.title,
                    content=page.content,
                    metadata=page.metadata,
                )
            if ws.semantic_engine:
                await ws.semantic_engine.build_index()
            await ws.save(str(settings.storage.indexes_dir / repo_name))
            prog.update(
                t,
                completed=100,
                description=f"[green]✓[/]  {phase}/{total_phases}  {len(wiki_pages)} wiki pages indexed",
            )
            prog.stop_task(t)

            # phase — annotate graph nodes with per-symbol descriptions
            t = prog.add_task(_phase("Annotating nodes…"), total=100, transient=True)
            _llm_cfg = resolve_llm_config({})
            _llm = LLMClient(_llm_cfg) if _llm_cfg.api_key else None
            _snippet_extractor = CodeSnippetExtractor(repo_path)
            _gw = GraphWikiGenerator(graph, _snippet_extractor, llm_client=_llm)
            annotated = await _gw.annotate_graph_descriptions(repo_id)
            # DuckDB auto-persists changes
            prog.update(
                t,
                completed=100,
                description=f"[green]✓[/]  {phase}/{total_phases}  {annotated} nodes annotated",
            )
            prog.stop_task(t)

        if gen_style:
            # phase — extract style guide
            t = prog.add_task(_phase("Extracting style guide…"), total=100, transient=True)
            style_patterns = await extract_style_guide(graph, repo_id)
            style_markdown = await generate_style_guide_markdown(style_patterns, repo_name)
            store = StyleGuideStore(repo_name)
            await store.save(style_patterns, style_markdown)
            prog.update(
                t,
                completed=100,
                description=f"[green]✓[/]  {phase}/{total_phases}  Style guide extracted",
            )
            prog.stop_task(t)

        if gen_queries:
            # phase — generate query associations
            t = prog.add_task(_phase("Generating query associations…"), total=100, transient=True)
            _llm_cfg = resolve_llm_config({})
            if not _llm_cfg.api_key:
                prog.update(
                    t, description=f"[yellow]⚠[/]  {phase}/{total_phases}  Skipped (no LLM API key)"
                )
                prog.stop_task(t)
            else:
                query_indexer = QueryAssociationIndexer(
                    graph=graph,
                    llm_config=_llm_cfg,
                    repo_root=Path(repo_path),
                    incremental_indexer=incremental,
                )

                # Progress callback to update Rich progress bar
                def on_query_progress(current_batch, total_batches):
                    pct = int((current_batch / total_batches) * 100)
                    prog.update(
                        t,
                        completed=pct,
                        description=f"[cyan]Generating query associations: {current_batch}/{total_batches} batches[/]",
                    )

                query_stats = await query_indexer.generate_queries_for_repo(
                    repo_id=repo_id, concurrency=5, progress_callback=on_query_progress
                )
                # DuckDB auto-persists changes
                prog.update(
                    t,
                    completed=100,
                    description=f"[green]✓[/]  {phase}/{total_phases}  {query_stats['total_queries_generated']} queries for {query_stats['symbols_processed']} symbols",
                )
                prog.stop_task(t)

        # Regenerate embeddings with enriched data if wiki or queries were generated
        if (gen_wiki or gen_queries) and isinstance(semantic_engine, DuckDBSemanticSearchEngine):
            t = prog.add_task(
                _phase("Regenerating enriched embeddings…"), total=100, transient=True
            )
            try:
                n_embeddings = await semantic_engine.generate_embeddings_for_repo(
                    repo_id, include_enriched=True
                )
                prog.update(
                    t,
                    completed=100,
                    description=f"[green]✓[/]  {phase}/{total_phases}  {n_embeddings} enriched embeddings",
                )
            except Exception as e:
                prog.update(
                    t,
                    completed=100,
                    description=f"[yellow]⚠[/]  {phase}/{total_phases}  Enriched embeddings skipped: {e}",
                )
            prog.stop_task(t)

    elapsed = time.time() - start
    result = IngestResult(
        repo_name=repo_name,
        repo_path=repo_path,
        symbols=syms,
        relationships=rels,
        processes=procs,
        wiki_pages=wiki_pages_count,
        elapsed_seconds=round(elapsed, 1),
    )

    # Calculate some useful metrics
    symbols_per_sec = (
        int(result.symbols / result.elapsed_seconds) if result.elapsed_seconds > 0 else 0
    )

    console.print(
        Panel(
            f"[bold green]✓ Repository indexed successfully[/]\n\n"
            f"  [bold]Repository[/]\n"
            f"    [dim]name:[/]   [cyan]{result.repo_name}[/]\n"
            f"    [dim]path:[/]   {result.repo_path}\n\n"
            f"  [bold]Extracted[/]\n"
            f"    [dim]symbols:[/]       [green]{result.symbols:,}[/] ({symbols_per_sec}/sec)\n"
            f"    [dim]relationships:[/] [green]{result.relationships:,}[/]\n"
            f"    [dim]processes:[/]     [green]{result.processes}[/]\n"
            f"    [dim]wiki pages:[/]    [green]{result.wiki_pages}[/]\n\n"
            f"  [bold]Performance[/]\n"
            f"    [dim]total time:[/] [cyan]{result.elapsed_seconds}s[/]",
            title="[bold]CoMind Analysis Complete[/]",
            border_style="green",
            padding=(1, 2),
        )
    )


# ─── search ──────────────────────────────────────────────────────────────────


@app.command()
def search(
    query: Annotated[str, typer.Argument(help="Natural language or code search query")],
    repo: Annotated[str, typer.Option("--repo", "-r", help="Repository name")],
    limit: Annotated[int, typer.Option("--limit", "-l", help="Max results")] = 10,
    code: Annotated[bool, typer.Option("--code/--no-code", help="Include code snippets")] = True,
    output: OutputFmt = "markdown",
) -> None:
    """Search the knowledge graph using hybrid BM25 + semantic search."""
    _run(_find(query, repo, limit, code, output))


async def _find(query: str, repo_name: str, limit: int, include_code: bool, output: str) -> None:
    console.print(f"[dim]Loading [cyan]{repo_name}[/]…[/]")
    _, qe, loaded = await _load_engine(repo_name)
    _require_repo(repo_name, loaded)

    raw = await qe.search(
        query=query, repo_id=repo_name, include_wiki=True, include_snippets=True, max_results=limit
    )

    if "error" in raw:
        err.print(f"[red]Error:[/] {raw['error']}")
        raise typer.Exit(1)

    parsed_results = []
    for r in raw.get("results", []):
        with suppress(Exception):
            parsed_results.append(FindResult.from_dict(r))
    response = FindResponse(
        query=query,
        repo_name=repo_name,
        total=raw.get("total_results", 0),
        results=parsed_results,
    )

    if output == "json":
        console.print_json(response.model_dump_json())
        return

    md = MarkdownFormatter.format_search_results(
        query=query,
        results=raw.get("results", []),
        total_results=response.total,
        include_code=include_code,
    )
    console.print(md)


# ─── grep ────────────────────────────────────────────────────────────────────


@app.command()
def grep(
    pattern: Annotated[str, typer.Argument(help="Regex pattern to search")],
    repo: Annotated[str, typer.Option("--repo", "-r", help="Repository name")],
    glob: Annotated[
        str | None, typer.Option("--glob", "-g", help="File filter, e.g. '*.py'")
    ] = None,
    mode: Annotated[str, typer.Option("--mode", "-m", help="content | files | count")] = "content",
    context: Annotated[
        int, typer.Option("--context", "-C", help="Context lines (content mode)")
    ] = 2,
    limit: Annotated[int, typer.Option("--limit", "-l", help="Max results")] = 50,
    output: OutputFmt = "markdown",
) -> None:
    """Search file content with regex (ripgrep-backed, Python fallback)."""
    from comind.indexing.file_search import GrepEngine

    mode_map = {"files": "files_with_matches", "content": "content", "count": "count"}
    output_mode = mode_map.get(mode, "content")
    root = _resolve_repo_root(repo)
    engine = GrepEngine()
    result = engine.search(
        pattern,
        root,
        glob=glob,
        output_mode=output_mode,
        context_lines=context,
        max_results=limit,
    )

    if output == "json":
        console.print_json(json.dumps(result.to_dict()))
        return

    if output_mode == "files_with_matches":
        console.print(f"[bold]{len(result.files)} file(s) match `{pattern}`[/]")
        for f in result.files:
            console.print(f"  [cyan]{f}[/]")
    elif output_mode == "count":
        console.print(f"[bold]Match counts for `{pattern}`[/]")
        for f, n in sorted(result.counts.items(), key=lambda x: -x[1]):
            console.print(f"  [cyan]{f}[/]  [dim]{n}[/]")
    else:
        console.print(
            f"[bold]{result.total} match(es) for `{pattern}`[/]"
            + (" [yellow](truncated)[/]" if result.truncated else "")
        )
        for m in result.matches:
            console.print(f"\n[cyan]{m.file}[/]:[bold]{m.line}[/]")
            for l in m.context_before:
                console.print(f"  [dim]{l}[/]")
            console.print(f"  {m.text}")
            for l in m.context_after:
                console.print(f"  [dim]{l}[/]")


# ─── glob ─────────────────────────────────────────────────────────────────────


@app.command(name="glob")
def glob_cmd(
    pattern: Annotated[str, typer.Argument(help="Glob pattern, e.g. 'src/**/*.py'")],
    repo: Annotated[str, typer.Option("--repo", "-r", help="Repository name")],
    limit: Annotated[int, typer.Option("--limit", "-l", help="Max results")] = 100,
    output: OutputFmt = "markdown",
) -> None:
    """Find files by path pattern (glob syntax, brace expansion supported)."""
    from comind.indexing.file_search import GlobEngine

    root = _resolve_repo_root(repo)
    files = GlobEngine().search(pattern, root, max_results=limit)

    if output == "json":
        console.print_json(json.dumps({"pattern": pattern, "files": files}))
        return

    console.print(f"[bold]{len(files)} file(s) matching `{pattern}`[/]")
    for f in files:
        console.print(f"  [cyan]{f}[/]")


# ─── read ─────────────────────────────────────────────────────────────────────


@app.command()
def read(
    file: Annotated[
        str,
        typer.Argument(
            help="File path, optionally with :start-end (e.g. @repo/path/file.py:9-109)"
        ),
    ],
    repo: Annotated[str, typer.Option("--repo", "-r", help="Repository name")],
    start: Annotated[int | None, typer.Option("--start", "-s", help="Start line (1-based)")] = None,
    end: Annotated[int | None, typer.Option("--end", "-e", help="End line (inclusive)")] = None,
    output: OutputFmt = "markdown",
) -> None:
    """Read a file (or line range) from an indexed repository.

    Accepts @repo/path/file.py:9-109 syntax — line range is optional.
    --start/--end can also be used explicitly.
    """
    import re as _re

    from comind.indexing.file_search import FileReader

    # Parse :start-end suffix from file argument
    _range_match = _re.search(r":(\d+)-(\d+)$", file)
    if _range_match:
        if start is None:
            start = int(_range_match.group(1))
        if end is None:
            end = int(_range_match.group(2))
        file = file[: _range_match.start()]

    # Strip @repo-name/ prefix (e.g. @skills-api/app/... → app/...)
    _prefix_match = _re.match(r"^@[^/]+/(.+)$", file)
    if _prefix_match:
        file = _prefix_match.group(1)

    root = _resolve_repo_root(repo)
    result = FileReader().read(file, root, start_line=start, end_line=end)
    if result is None:
        err.print(f"[red]Error:[/] cannot read '{file}' in '{repo}'")
        raise typer.Exit(1)

    if output == "json":
        console.print_json(json.dumps(result.to_dict()))
        return

    header = f"[cyan]{result.file}[/] lines [bold]{result.start_line}-{result.end_line}[/] / {result.total_lines}"
    console.print(header)
    console.print("```")
    console.print(result.content)
    console.print("```")


# ─── repos ───────────────────────────────────────────────────────────────────


@app.command()
def repos() -> None:
    """List all repositories absorbed into CoMind."""
    _run(_repos())


async def _repos() -> None:
    from comind.config import get_settings

    settings = get_settings()
    indexes_dir = settings.storage.indexes_dir

    if not indexes_dir.exists() or not any(indexes_dir.iterdir()):
        console.print(
            "[yellow]No repositories indexed yet.[/]  Run [bold]comind analyze <path>[/] to get started."
        )
        return

    repo_list: list[RepoInfo] = []
    for repo_dir in sorted(d for d in indexes_dir.iterdir() if d.is_dir()):
        rname = repo_dir.name
        graph_file = settings.storage.graphs_dir / f"{rname}.pkl"
        wiki_dir = settings.storage.wiki_dir / rname / "wiki"
        repo_list.append(
            RepoInfo(
                name=rname,
                has_graph=graph_file.exists(),
                wiki_pages=len(list(wiki_dir.glob("*.md"))) if wiki_dir.exists() else 0,
                index_path=str(repo_dir),
            )
        )

    response = ReposResponse(repos=repo_list, total=len(repo_list))

    t = Table(title=f"CoMind Repositories ({response.total})", header_style="bold cyan")
    t.add_column("Name", style="cyan")
    t.add_column("Graph", justify="center")
    t.add_column("Wiki pages", justify="right")
    t.add_column("Style guide", justify="center")
    t.add_column("Index")

    for r in response.repos:
        style_path = settings.storage.wiki_dir / r.name / "style_patterns.json"
        has_style = "[green]✓[/]" if style_path.exists() else "[dim]—[/]"
        t.add_row(
            r.name,
            "[green]✓[/]" if r.has_graph else "[red]✗[/]",
            str(r.wiki_pages),
            has_style,
            r.index_path,
        )

    console.print(t)


# ─── serve ───────────────────────────────────────────────────────────────────


@app.command()
def serve(
    host: Annotated[str, typer.Option("--host", help="Bind host")] = "0.0.0.0",
    port: Annotated[int, typer.Option("--port", "-p", help="Port")] = 8000,
    reload: Annotated[bool, typer.Option("--reload", help="Auto-reload on file changes")] = False,
    workers: Annotated[int, typer.Option("--workers", "-w", help="Worker processes")] = 1,
) -> None:
    """Start the CoMind REST API server."""
    import uvicorn

    from comind.api.server import create_app
    from comind.logging_config import configure_logging

    configure_logging()
    console.print(
        Panel(
            f"[bold green]CoMind API[/] starting on [cyan]http://{host}:{port}[/]\n"
            f"  Docs:   [link]http://{host}:{port}/docs[/link]\n"
            f"  Health: [link]http://{host}:{port}/health[/link]",
            border_style="green",
        )
    )
    uvicorn.run(
        create_app(),
        host=host,
        port=port,
        reload=reload,
        workers=workers if not reload else 1,
        log_config=None,
        access_log=False,
    )


# ─── mcp ─────────────────────────────────────────────────────────────────────


@app.command()
def mcp(
    transport: Annotated[
        str, typer.Option("--transport", "-t", help="stdio (default) | http")
    ] = "stdio",
    host: Annotated[str, typer.Option("--host", help="Host for HTTP transport")] = "0.0.0.0",
    port: Annotated[int, typer.Option("--port", "-p", help="Port for HTTP transport")] = 8000,
) -> None:
    """Start the MCP server for AI agent integration.

    [bold]stdio[/] (default) — for Claude Code, Cursor, Windsurf direct integration.\n
    [bold]http[/]  — start the REST server with SSE MCP endpoint.
    """
    if transport == "http":
        console.print(
            f"[bold]Starting CoMind MCP (HTTP/SSE)[/] on [cyan]http://{host}:{port}/mcp[/]"
        )
        serve(host=host, port=port)
    else:
        from comind.mcp_server import run_stdio_server

        run_stdio_server()
