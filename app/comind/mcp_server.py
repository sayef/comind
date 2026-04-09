"""
CoMind MCP Server — stdio transport for direct AI agent integration.

Tools
-----
  repos    Discover indexed repositories
  find     Hybrid BM25 + semantic code search
  zoom     360° symbol context (callers, callees, processes, wiki)
  ripple   Blast-radius: what breaks if a symbol changes?
  thread   Trace the execution path from an entry point
  guide    Query coding style and conventions for a repo
"""

from __future__ import annotations

from typing import Any

from mcp.server.fastmcp import FastMCP

from comind.models import (
    FindResponse,
    FindResult,
    GuideResponse,
    RepoInfo,
    ReposResponse,
    RippleResponse,
    ThreadResponse,
    ZoomResponse,
)

mcp = FastMCP(
    "comind",
    instructions=(
        "CoMind is a graph-powered code intelligence engine. "
        "Start by calling `repos` to discover what has been indexed, "
        "then use `find`, `flows`, `zoom`, `ripple`, `thread`, or `guide` with the repo name.\n\n"
        "Tool quick-reference:\n"
        "  repos   — list indexed repos\n"
        "  find    — semantic search ('user authentication', 'parse_token')\n"
        "  flows   — search execution flows ('authentication flow', 'how does login work')\n"
        "  zoom    — full symbol context including callers, callees, processes\n"
        "  ripple  — blast radius: what breaks if symbol X changes?\n"
        "  thread  — execution trace from an entry point\n"
        "  guide   — ask about coding conventions ('how to name functions?')\n"
        "  grep    — regex search in source files (ripgrep-backed)\n"
        "  glob    — find files by path pattern\n"
        "  read    — read a file or line range (@repo/path:start-end syntax)"
    ),
)

# ─── shared engine (lazy-loaded, cached per repo) ─────────────────────────────

_engine_cache: dict[str, Any] = {}


async def _get_engine(repo_name: str | None = None) -> tuple:
    cache_key = repo_name or "__all__"
    if cache_key in _engine_cache:
        return _engine_cache[cache_key]

    from comind.config import get_settings
    from comind.search.duckdb_search_engine import create_search_engines
    from comind.search.query_engine import WikiEnhancedQueryEngine
    from comind.storage.duckdb_backend import DuckDBBackend
    from comind.storage.graph_adapter import GraphAdapter
    from comind.wiki.wiki import load_wiki_pages

    settings = get_settings()

    # Load single shared DuckDB database in read-only mode
    # (allows concurrent queries while indexing is happening)
    db_path = settings.storage.duckdb_path
    if not db_path.exists():
        return None, None, []

    backend = DuckDBBackend(str(db_path), read_only=True)

    # Wrap in adapter for compatibility
    graph = GraphAdapter(backend)

    # Create DuckDB-based search engines
    text_engine, semantic_engine = create_search_engines(graph)

    qe = WikiEnhancedQueryEngine(graph)
    await qe.initialize_search_indexes()

    loaded = await qe.load_all_indexes(settings.storage.indexes_dir)

    # Register search engines for loaded repos
    for rname in loaded:
        if repo_name and rname != repo_name:
            continue
        qe.repo_text_engines[rname] = text_engine
        qe.repo_semantic_engines[rname] = semantic_engine

    for rname in loaded:
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

    result = (graph, qe, loaded)
    _engine_cache[cache_key] = result
    return result


def _missing_repo_msg(repo_name: str, loaded: list[str]) -> str:
    return (
        f"Repository '{repo_name}' is not indexed.\n"
        f"Indexed repos: {loaded or ['(none)']}\n"
        "Run: comind ingest <path-or-git-url>"
    )


# ─── repos ───────────────────────────────────────────────────────────────────


@mcp.tool()
async def repos() -> str:
    """List all repositories indexed by CoMind.

    Returns a JSON object with repository names, graph status, and wiki page counts.
    Always call this first to discover what repos are available.
    """
    from comind.config import get_settings

    settings = get_settings()
    indexes_dir = settings.storage.indexes_dir

    if not indexes_dir.exists():
        return ReposResponse(repos=[], total=0).model_dump_json()

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

    return ReposResponse(repos=repo_list, total=len(repo_list)).model_dump_json()


# ─── find ────────────────────────────────────────────────────────────────────


@mcp.tool()
async def find(
    query: str,
    repo_name: str,
    limit: int = 10,
    output_format: str = "markdown",
) -> str:
    """Search the codebase using hybrid BM25 + semantic search.

    Searches both individual symbols (functions, classes) and execution flows (multi-step processes).
    Returns the most relevant results combining both types.

    Args:
        query: Natural language or code query (e.g. "user authentication", "parse_token function")
        repo_name: Repository name from `repos`
        limit: Max results to return (1–50, default 10)
        output_format: "markdown" (default, human-readable) or "json" (structured)

    Returns:
        Matching symbols with code snippets, callers/callees counts, wiki excerpts, and relevant execution flows.
    """
    import numpy as np
    from fastembed import TextEmbedding

    from comind.config import get_settings
    from comind.utils.markdown_formatter import MarkdownFormatter

    graph, qe, loaded = await _get_engine(repo_name)
    if repo_name not in loaded:
        return _missing_repo_msg(repo_name, loaded)

    # Search symbols
    raw = await qe.search(
        query=query,
        repo_id=repo_name,
        include_wiki=True,
        include_snippets=True,
        max_results=min(limit, 50),
    )

    if "error" in raw:
        return f"Search error: {raw['error']}"

    # Also search process queries
    settings = get_settings()
    model = TextEmbedding(model_name=settings.search.embedding_model)
    query_embedding = next(model.embed([query]))

    process_results = await graph.backend.search_process_queries(
        query_embedding=np.array(query_embedding),
        repo_id=repo_name,
        limit=5,  # Top 5 relevant flows
    )

    response = FindResponse(
        query=query,
        repo_name=repo_name,
        total=raw.get("total_results", 0),
        results=[FindResult.from_dict(r) for r in raw.get("results", [])],
    )

    if output_format == "json":
        result_dict = response.model_dump()
        result_dict["execution_flows"] = process_results
        import json

        return json.dumps(result_dict, indent=2)

    # Format markdown with both symbols and flows
    markdown = MarkdownFormatter.format_search_results(
        query=query,
        results=raw.get("results", []),
        total_results=response.total,
        include_code=True,
    )

    # Append execution flows if found
    if process_results:
        markdown += f"\n\n---\n\n## Relevant Execution Flows ({len(process_results)})\n\n"
        for i, flow in enumerate(process_results, 1):
            markdown += f"### {i}. {flow['process_name']}\n"
            markdown += (
                f"**Similarity:** {flow['similarity']:.2%} | **Entry:** `{flow['entry_point']}`\n\n"
            )

            steps = flow.get("steps", [])
            if steps:
                markdown += "**Steps:**\n"
                for step in steps[:5]:  # Show first 5 steps
                    markdown += f"{step['step']}. `{step['name']}` — `{step['file_path']}`\n"
                if len(steps) > 5:
                    markdown += f"... and {len(steps) - 5} more steps\n"
            markdown += "\n"

    return markdown


# ─── zoom ────────────────────────────────────────────────────────────────────


@mcp.tool()
async def zoom(
    symbol_name: str,
    repo_name: str,
    depth: int = 2,
    output_format: str = "markdown",
) -> str:
    """Get 360° context for a symbol: callers, callees, processes, and wiki docs.

    Args:
        symbol_name: Function, class, or method name (e.g. "authenticate", "UserModel.save")
        repo_name: Repository name from `repos`
        depth: Relationship traversal depth (1–5, default 2)
        output_format: "markdown" (default) or "json"

    Returns:
        Complete symbol profile including all references, execution processes, and documentation.
    """
    _, qe, loaded = await _get_engine(repo_name)
    if repo_name not in loaded:
        return _missing_repo_msg(repo_name, loaded)

    raw = await qe.get_context(symbol_name=symbol_name, depth=depth, include_wiki=True)
    if "error" in raw:
        return f"Symbol not found: {raw['error']}"

    response = ZoomResponse.from_dict(raw, depth=depth)

    if output_format == "json":
        return response.model_dump_json()

    sym = response.symbol
    lines = [
        f"# `{sym.name}`  ({sym.kind})",
        f"**File:** `{sym.file_path}:{sym.line_start}`",
    ]
    if sym.signature:
        lines += ["", f"```python\n{sym.signature}\n```"]
    if sym.docstring:
        lines += ["", f"> {sym.docstring[:400]}"]

    if response.callers:
        lines += [f"\n## Callers ({len(response.callers)})\n"]
        lines += [f"- `{c.name}` — `{c.file_path}:{c.line_start}`" for c in response.callers[:20]]

    if response.callees:
        lines += [f"\n## Callees ({len(response.callees)})\n"]
        lines += [f"- `{c.name}` — `{c.file_path}:{c.line_start}`" for c in response.callees[:20]]

    if response.processes:
        lines += [f"\n## Processes ({len(response.processes)})\n"]
        lines += [f"- {p}" for p in response.processes[:10]]

    if response.wiki_excerpt:
        lines += [f"\n## Documentation\n\n{response.wiki_excerpt[:600]}"]

    return "\n".join(lines)


# ─── flows ───────────────────────────────────────────────────────────────────


@mcp.tool()
async def flows(
    query: str,
    repo_name: str,
    limit: int = 10,
    output_format: str = "markdown",
) -> str:
    """Search for execution flows and processes in the codebase.

    Use this to find multi-step execution paths, understand how features work end-to-end,
    or discover architectural patterns. Processes are detected execution flows that show
    how functions call each other across the codebase.

    Args:
        query: Natural language query (e.g. "authentication flow", "how does login work", "user registration process")
        repo_name: Repository name from `repos`
        limit: Maximum number of flows to return (1-20, default 10)
        output_format: "markdown" (default) or "json"

    Returns:
        Matching execution flows with step-by-step breakdown and similarity scores.
    """
    graph, qe, loaded = await _get_engine(repo_name)
    if repo_name not in loaded:
        return _missing_repo_msg(repo_name, loaded)

    # Generate embedding for query
    import numpy as np
    from fastembed import TextEmbedding

    from comind.config import get_settings

    settings = get_settings()
    model = TextEmbedding(model_name=settings.search.embedding_model)
    query_embedding = next(model.embed([query]))

    # Search process queries
    results = await graph.backend.search_process_queries(
        query_embedding=np.array(query_embedding), repo_id=repo_name, limit=min(limit, 20)
    )

    if not results:
        return f"No execution flows found matching '{query}'"

    if output_format == "json":
        import json

        return json.dumps({"query": query, "flows": results}, indent=2)

    # Format as markdown
    lines = [f"# Execution Flows: {query}\n"]
    lines.append(f"Found {len(results)} matching flows\n")

    for i, flow in enumerate(results, 1):
        lines.append(f"## {i}. {flow['process_name']}")
        lines.append(f"**Similarity:** {flow['similarity']:.2%}")
        lines.append(f'**Matched query:** "{flow["matched_query"]}"')
        lines.append(f"**Entry point:** `{flow['entry_point']}`")

        steps = flow.get("steps", [])
        if steps:
            lines.append(f"\n**Execution steps ({len(steps)}):**")
            for step in steps:
                lines.append(f"{step['step']}. `{step['name']}` — `{step['file_path']}`")

        lines.append("")  # Blank line between flows

    return "\n".join(lines)


# ─── flows ───────────────────────────────────────────────────────────────────


@mcp.tool()
async def flows(
    query: str,
    repo_name: str,
    limit: int = 10,
    output_format: str = "markdown",
) -> str:
    """Search for execution flows and processes in the codebase.

    Use this to find multi-step execution paths, understand how features work end-to-end,
    or discover architectural patterns. Processes are detected execution flows that show
    how functions call each other across the codebase.

    Args:
        query: Natural language query (e.g. "authentication flow", "how does login work", "user registration process")
        repo_name: Repository name from `repos`
        limit: Maximum number of flows to return (1-20, default 10)
        output_format: "markdown" (default) or "json"

    Returns:
        Matching execution flows with step-by-step breakdown and similarity scores.
    """
    import numpy as np
    from fastembed import TextEmbedding

    from comind.config import get_settings

    graph, _, loaded = await _get_engine(repo_name)
    if repo_name not in loaded:
        return _missing_repo_msg(repo_name, loaded)

    # Generate query embedding
    settings = get_settings()
    model = TextEmbedding(model_name=settings.search.embedding_model)
    query_embedding = next(model.embed([query]))

    # Search process queries
    process_results = await graph.backend.search_process_queries(
        query_embedding=np.array(query_embedding), repo_id=repo_name, limit=min(limit, 20)
    )

    if output_format == "json":
        import json

        return json.dumps(
            {
                "query": query,
                "repo_name": repo_name,
                "total": len(process_results),
                "flows": process_results,
            },
            indent=2,
        )

    # Format markdown
    if not process_results:
        return f"No execution flows found for query: '{query}'"

    lines = [
        f"# Execution Flows — `{repo_name}`",
        f"**Query:** {query}",
        f"**Found:** {len(process_results)} flows\n",
    ]

    for i, flow in enumerate(process_results, 1):
        lines.append(f"## {i}. {flow['process_name']}")
        lines.append(
            f"**Similarity:** {flow['similarity']:.2%} | **Entry:** `{flow['entry_point']}` | **Priority:** {flow['priority']}"
        )
        lines.append(f'**Matched Query:** "{flow["matched_query"]}"\n')

        steps = flow.get("steps", [])
        if steps:
            lines.append("**Execution Steps:**")
            for step in steps[:10]:  # Show first 10 steps
                lines.append(f"{step['step']:>3}. `{step['name']}` — `{step['file_path']}`")
            if len(steps) > 10:
                lines.append(f"... and {len(steps) - 10} more steps")
        lines.append("")

    return "\n".join(lines)


# ─── ripple ──────────────────────────────────────────────────────────────────


@mcp.tool()
async def ripple(
    symbol_name: str,
    repo_name: str,
    direction: str = "upstream",
    max_depth: int = 3,
    min_confidence: float = 0.7,
    output_format: str = "markdown",
) -> str:
    """Analyse blast radius: what will break if this symbol changes?

    Args:
        symbol_name: Symbol to analyse (e.g. "parse_token", "UserModel")
        repo_name: Repository name from `repos`
        direction: "upstream" (callers), "downstream" (callees), or "both"
        max_depth: Traversal depth (1–5, default 3)
        min_confidence: Minimum confidence threshold 0.0–1.0 (default 0.7)
        output_format: "markdown" (default) or "json"

    Returns:
        Affected symbols grouped by depth with confidence scores and risk level.
    """
    _, qe, loaded = await _get_engine(repo_name)
    if repo_name not in loaded:
        return _missing_repo_msg(repo_name, loaded)

    raw = await qe.analyze_impact(
        symbol_name=symbol_name,
        direction=direction,
        max_depth=max_depth,
        min_confidence=min_confidence,
    )
    if "error" in raw:
        return f"Symbol not found: {raw['error']}"

    response = RippleResponse.from_dict(raw)

    if output_format == "json":
        return response.model_dump_json()

    lines = [
        f"# Ripple — `{response.symbol.name}`",
        f"**Direction:** {response.direction}  |  **Risk:** {response.risk_level}  |  **Affected:** {response.total_affected}",
    ]

    if not response.affected:
        lines.append("\nNo dependants found — safe to change.")
        return "\n".join(lines)

    # Group by depth
    by_depth: dict[int, list] = {}
    for e in response.affected:
        by_depth.setdefault(e.depth, []).append(e)

    for d in sorted(by_depth):
        lines.append(f"\n## Depth {d}\n")
        for e in by_depth[d][:30]:
            lines.append(
                f"- `{e.symbol.name}` ({e.symbol.kind}) — {e.confidence:.0%} — `{e.symbol.file_path}`"
            )

    if response.affected_processes:
        lines.append(f"\n## Affected Processes ({len(response.affected_processes)})\n")
        lines += [f"- {p}" for p in response.affected_processes[:10]]

    return "\n".join(lines)


# ─── thread ──────────────────────────────────────────────────────────────────


@mcp.tool()
async def thread(
    entry_point: str,
    repo_name: str,
    max_depth: int = 10,
    output_format: str = "markdown",
) -> str:
    """Trace the execution thread from a function or method entry point.

    Args:
        entry_point: Starting function/method name (e.g. "handle_request", "main")
        repo_name: Repository name from `repos`
        max_depth: Maximum trace depth (1–20, default 10)
        output_format: "markdown" (default) or "json"

    Returns:
        Step-by-step execution path showing the full call chain.
    """
    _, qe, loaded = await _get_engine(repo_name)
    if repo_name not in loaded:
        return _missing_repo_msg(repo_name, loaded)

    raw = await qe.trace_execution(entry_point=entry_point, max_depth=max_depth)
    if "error" in raw:
        return f"Entry point not found: {raw['error']}"

    response = ThreadResponse.from_dict(raw, entry_point=entry_point)

    if output_format == "json":
        return response.model_dump_json()

    lines = [f"# Thread — `{entry_point}`", f"**{response.total_steps} steps**\n"]
    for step in response.steps:
        suffix = f"  `{step.file_path}`" if step.file_path else ""
        lines.append(f"{step.step:>3}. `{step.name}`  ({step.kind}){suffix}")

    return "\n".join(lines)


# ─── guide ───────────────────────────────────────────────────────────────────


@mcp.tool()
async def guide(
    repo_name: str,
    query: str | None = None,
    output_format: str = "markdown",
) -> str:
    """Query the coding style and conventions extracted from a repository.

    Use this tool when you need to understand how code should be written in
    a specific repository — naming conventions, typing style, error handling,
    logging, async patterns, docstring format, etc.

    Args:
        repo_name: Repository name from `repos`
        query: Optional style question in natural language, e.g.:
               "how should I name functions?"
               "what typing style is used?"
               "how are errors handled?"
               "what logging pattern is preferred?"
               (leave None to get the full style guide)
        output_format: "markdown" (default) or "json"

    Returns:
        Targeted style guidance and a direct recommendation, or the full guide
        if no query is provided.
    """
    from comind.style.style_guide_store import StyleGuideStore

    store = StyleGuideStore(repo_name)
    response = await store.query(query, repo_name=repo_name)

    if output_format == "json":
        return response.model_dump_json()

    if not query:
        full = await store.full_guide()
        return full or f"Style guide not yet generated for '{repo_name}'. Run: comind ingest <path>"

    return _render_guide_md(response)


def _render_guide_md(r: GuideResponse) -> str:
    lines = [f"# Style Guide — `{r.repo_name}`"]
    if r.query:
        lines.append(f"**Query:** {r.query}\n")

    if r.recommendation:
        lines += ["## Recommendation\n", r.recommendation, ""]

    for section in r.sections:
        lines.append(f"## {section.category.title()}")
        lines.append(f"**{section.summary}**")
        if section.prevalence:
            lines.append(f"*Prevalence: {section.prevalence}*")
        for detail in section.details:
            lines.append(f"- {detail}")
        if section.examples:
            lines.append(f"\nExamples: `{'`, `'.join(section.examples[:4])}`")
        lines.append("")

    return "\n".join(lines)


# ─── grep ────────────────────────────────────────────────────────────────────


def _repo_root(repo_name: str) -> Path | None:
    """Resolve the local root directory for an indexed repo."""
    from pathlib import Path

    from comind.config import get_settings

    settings = get_settings()
    path_file = settings.storage.indexes_dir / repo_name / "repo_path.txt"
    if path_file.exists():
        root = Path(path_file.read_text().strip())
        if root.exists():
            return root
    fallback = settings.storage.repos_dir / repo_name
    return fallback if fallback.exists() else None


@mcp.tool()
async def grep(
    pattern: str,
    repo_name: str,
    glob: str | None = None,
    mode: str = "content",
    context_lines: int = 2,
    limit: int = 50,
) -> str:
    """Search file content with a regex pattern (ripgrep-backed, Python fallback).

    Use this after `find` to drill into raw file content — find all usages of a
    constant, locate TODO comments, trace error messages, etc.

    Args:
        pattern: Regular expression (e.g. "parse_token", "TODO.*auth", "raise.*Error")
        repo_name: Repository name from `repos`
        glob: Optional file filter (e.g. "*.py", "tests/**/*.py", "*.{py,pyi}")
        mode: "content" (default, lines+context) | "files" (paths only) | "count" (per-file counts)
        context_lines: Lines of context around each match (content mode, default 2)
        limit: Max matches to return (default 50)

    Returns:
        Matching lines with file paths and line numbers, or file list, or counts.
    """
    from comind.indexing.file_search import GrepEngine

    root = _repo_root(repo_name)
    if root is None:
        return f"Cannot resolve local path for '{repo_name}'. Re-run: comind analyze <path>"

    mode_map = {"files": "files_with_matches", "content": "content", "count": "count"}
    result = GrepEngine().search(
        pattern,
        root,
        glob=glob,
        output_mode=mode_map.get(mode, "content"),
        context_lines=context_lines,
        max_results=limit,
    )

    lines = [f"# grep `{pattern}` in `{repo_name}`"]
    trunc = " *(truncated)*" if result.truncated else ""

    if result.output_mode == "files_with_matches":
        lines.append(f"**{len(result.files)} file(s)**{trunc}\n")
        lines += [f"- `{f}`" for f in result.files]
    elif result.output_mode == "count":
        lines.append(f"**Match counts**{trunc}\n")
        for f, n in sorted(result.counts.items(), key=lambda x: -x[1]):
            lines.append(f"- `{f}`: {n}")
    else:
        lines.append(f"**{result.total} match(es)**{trunc}\n")
        for m in result.matches:
            lines.append(f"`{m.file}:{m.line}`")
            if m.context_before:
                lines += [f"  {l}" for l in m.context_before]
            lines.append(f"**→** {m.text}")
            if m.context_after:
                lines += [f"  {l}" for l in m.context_after]
            lines.append("")

    return "\n".join(lines)


# ─── glob ─────────────────────────────────────────────────────────────────────


@mcp.tool()
async def glob(
    pattern: str,
    repo_name: str,
    limit: int = 100,
) -> str:
    """Find files in a repository by path pattern (glob syntax).

    Use this to discover files before reading them, or to understand the project
    layout (e.g. find all test files, all migration scripts, all config files).

    Args:
        pattern: Glob pattern relative to repo root (e.g. "**/*.py", "tests/**", "*.{yaml,yml}")
        repo_name: Repository name from `repos`
        limit: Max files to return (default 100)

    Returns:
        Repo-relative file paths sorted by modification time (newest first).
    """
    from comind.indexing.file_search import GlobEngine

    root = _repo_root(repo_name)
    if root is None:
        return f"Cannot resolve local path for '{repo_name}'. Re-run: comind analyze <path>"

    files = GlobEngine().search(pattern, root, max_results=limit)
    lines = [f"# glob `{pattern}` in `{repo_name}`", f"\n**{len(files)} file(s)**\n"]
    lines += [f"- `{f}`" for f in files]
    return "\n".join(lines)


# ─── read ─────────────────────────────────────────────────────────────────────


@mcp.tool()
async def read(
    file_path: str,
    repo_name: str,
    start_line: int | None = None,
    end_line: int | None = None,
) -> str:
    """Read a file or a specific line range from an indexed repository.

    Use this to fetch the exact source code of a symbol after `find` or `zoom`
    returns its location (file_path + line_start/line_end).

    Args:
        file_path: Repo-relative path with optional line range suffix, e.g.
                   "@skills-api/app/api/skill_lookup.py:9-109" or just
                   "app/api/skill_lookup.py". The @repo/ prefix is stripped automatically.
        repo_name: Repository name from `repos`
        start_line: First line to return, 1-based (overrides suffix if both given)
        end_line: Last line to return, inclusive

    Returns:
        File content with line range information.
    """
    import re as _re

    from comind.indexing.file_search import FileReader

    # Parse :start-end suffix
    _m = _re.search(r":(\d+)-(\d+)$", file_path)
    if _m:
        if start_line is None:
            start_line = int(_m.group(1))
        if end_line is None:
            end_line = int(_m.group(2))
        file_path = file_path[: _m.start()]

    # Strip @repo-name/ prefix
    _p = _re.match(r"^@[^/]+/(.+)$", file_path)
    if _p:
        file_path = _p.group(1)

    root = _repo_root(repo_name)
    if root is None:
        return f"Cannot resolve local path for '{repo_name}'. Re-run: comind analyze <path>"

    result = FileReader().read(file_path, root, start_line=start_line, end_line=end_line)
    if result is None:
        return f"Cannot read '{file_path}' in '{repo_name}' — file not found or path is unsafe."

    header = (
        f"# `{result.file}` (lines {result.start_line}–{result.end_line} of {result.total_lines})\n"
    )
    return header + f"```python\n{result.content}\n```"


# ─── entry point ─────────────────────────────────────────────────────────────


def run_stdio_server() -> None:
    """Start the MCP server on stdio (for Claude Code / Cursor / Windsurf)."""
    mcp.run(transport="stdio")
