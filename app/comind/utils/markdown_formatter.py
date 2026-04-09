"""
Markdown formatter for search results

Formats search results as clean, readable markdown for AI agents,
reducing token usage while maintaining clarity.
"""

from typing import Any


class MarkdownFormatter:
    """Format search results as markdown"""

    @staticmethod
    def _format_file_path(file_path: str, repo_name: str | None = None) -> str:
        """
        Convert file path to @repo-name/relative/path format

        Handles both:
        - Absolute paths (old data): /Users/msi/.comind/data/repos/skill-graph/app/tests/test_auth.py
        - Relative paths (new data): app/tests/test_auth.py

        Returns: @skill-graph/app/tests/test_auth.py
        """
        if not file_path:
            return ""

        # Check if path is already relative (new format)
        if not file_path.startswith("/"):
            # Already relative, just add repo prefix
            if repo_name:
                return f"@{repo_name}/{file_path}"
            else:
                return file_path

        # Handle absolute paths (old format) - extract repo name and relative path
        if not repo_name:
            parts = file_path.split("/repos/")
            if len(parts) > 1:
                repo_part = parts[1].split("/", 1)
                if len(repo_part) > 1:
                    repo_name = repo_part[0]
                    relative_path = repo_part[1]
                else:
                    return file_path
            else:
                return file_path
        else:
            # Find repo name in path and extract relative part
            if f"/repos/{repo_name}/" in file_path:
                relative_path = file_path.split(f"/repos/{repo_name}/", 1)[1]
            else:
                return file_path

        return f"@{repo_name}/{relative_path}"

    @staticmethod
    def format_search_results(
        query: str,
        results: list[dict[str, Any]],
        total_results: int,
        include_code: bool = True,
        compact: bool = False,
        include_wiki: bool = True,
        include_graph: bool = True,
    ) -> str:
        """
        Format search results as clean, readable markdown.

        Shows for each result:
        - Symbol name, type, location
        - Code snippet (optional)
        - Callers and callees
        - Wiki/documentation context
        
        Args:
            query: Search query string
            results: List of search result dictionaries
            total_results: Total number of results found
            include_code: Whether to include code snippets
            compact: Whether to use compact formatting
            include_wiki: Whether to include wiki context
            include_graph: Whether to include graph context (callers/callees)
        """
        lines = []

        lines.append(f"# Search Results: `{query}`")
        lines.append(f"\n**{total_results} result(s) found**\n")

        if not results:
            lines.append("*No results found*")
            return "\n".join(lines)

        for i, result in enumerate(results, 1):
            symbol = result.get("symbol", {})
            repo_id = symbol.get("repo_id")
            file_path = symbol.get("file_path", "")
            line_start = symbol.get("line_start", 0)
            line_end = symbol.get("line_end", 0)
            formatted_path = MarkdownFormatter._format_file_path(file_path, repo_id)

            # ── header ──────────────────────────────────────────────────────
            lines.append(f"## {i}. `{symbol.get('name')}` ({symbol.get('type')})")
            lines.append(f"`{formatted_path}:{line_start}-{line_end}`")
            lines.append("")

            # ── signature ───────────────────────────────────────────────────
            if signature := symbol.get("signature"):
                lines.append("```python")
                lines.append(signature)
                lines.append("```")
                lines.append("")

            # ── code snippet ────────────────────────────────────────────────
            if include_code and (code_snippet := result.get("code_snippet")):
                code_text = (
                    code_snippet.get("code", "")
                    if isinstance(code_snippet, dict)
                    else str(code_snippet)
                )
                if code_text.strip():
                    snippet_end = (
                        code_snippet.get("absolute_lines", {}).get("end", line_end)
                        if isinstance(code_snippet, dict) else line_end
                    )
                    is_truncated = isinstance(code_snippet, dict) and line_end > snippet_end
                    label = f"**Code** *(lines {line_start}–{line_end})*:" if not is_truncated else f"**Code** *(showing lines {line_start}–{snippet_end} of {line_end})*:"
                    lines.append(label)
                    lines.append("```python")
                    lines.append(code_text.rstrip())
                    if is_truncated:
                        lines.append(f"# ... truncated. Use: comind read --repo {repo_id} {formatted_path}:{line_start}-{line_end}")
                    lines.append("```")
                    lines.append("")

            # ── callers / callees ────────────────────────────────────────────
            if include_graph:
                graph_context = result.get("graph_context", {})
                callers = graph_context.get("callers", [])
                callees = graph_context.get("callees", [])

                def _ref_lines(c: dict) -> list[str]:
                    name = c.get("name", "unknown")
                    file = MarkdownFormatter._format_file_path(c.get("file", ""), repo_id)
                    line = c.get("line")
                    call_line = c.get("call_line")
                    call_text = (c.get("call_text") or "").strip()
                    loc = f"{file}:{line}" if line else file
                    out = [f"- `{name}` — `{loc}`"]
                    if call_text and call_line and not compact:
                        out.append(f"  > `{call_text}` *(line {call_line})*")
                    return out

                callers_total = graph_context.get("callers_total", len(callers))
                callees_total = graph_context.get("callees_total", len(callees))

                if callers:
                    suffix = f", showing {len(callers)}" if callers_total > len(callers) else ""
                    lines.append(f"**Called by ({callers_total}{suffix}):**")
                    for c in callers:
                        lines.extend(_ref_lines(c))
                    lines.append("")

                if callees:
                    suffix = f", showing {len(callees)}" if callees_total > len(callees) else ""
                    lines.append(f"**Calls ({callees_total}{suffix}):**")
                    for c in callees:
                        lines.extend(_ref_lines(c))
                    lines.append("")

            # ── about (LLM description > docstring) ─────────────────────────
            if include_wiki:
                about = symbol.get("description") or symbol.get("docstring")
                if about:
                    lines.append("**About:**")
                    lines.append("")
                    # Prefix every line with > so multi-paragraph text stays in blockquote
                    for bl in about.strip().splitlines():
                        lines.append(f"> {bl}" if bl.strip() else ">")
                    lines.append("")

            # ── separator ────────────────────────────────────────────────────
            if i < len(results):
                lines.append("---")
                lines.append("")

        return "\n".join(lines)

    @staticmethod
    def format_compact_results(
        query: str,
        results: list[dict[str, Any]],
        total_results: int,
    ) -> str:
        """
        Ultra-compact format for AI agents (minimal tokens)

        Format:
        # Results: query (N found)
        1. symbol_name (type) - @repo/path/file:line - score
        2. ...
        """
        lines = []
        lines.append(f"# Results: `{query}` ({total_results} found)\n")

        for i, result in enumerate(results, 1):
            symbol = result.get("symbol", {})
            score = result.get("score", 0)

            name = symbol.get("name", "unknown")
            sym_type = symbol.get("type", "unknown")
            file_path = symbol.get("file_path", "")
            repo_id = symbol.get("repo_id")
            line = symbol.get("line_start", 0)

            formatted_path = MarkdownFormatter._format_file_path(file_path, repo_id)

            lines.append(f"{i}. **`{name}`** ({sym_type}) - `{formatted_path}:{line}` - score: {score:.2f}")

        return "\n".join(lines)

    @staticmethod
    def format_trace_results(
        query: str,
        mode: str,  # "callers", "callees", "graph"
        results: list[dict[str, Any]],
    ) -> str:
        """
        Format call graph trace results

        Args:
            query: Symbol name being traced
            mode: Type of trace (callers/callees/graph)
            results: List of trace results

        Returns:
            Markdown-formatted string
        """
        lines = []

        # Header
        mode_title = {
            "callers": "Functions that call",
            "callees": "Functions called by",
            "graph": "Call graph for",
        }.get(mode, "Trace results for")

        lines.append(f"# {mode_title} `{query}`")
        lines.append(f"\n**Found {len(results)} result(s)**\n")

        if not results:
            lines.append("*No results found*")
            return "\n".join(lines)

        # Results
        for i, result in enumerate(results, 1):
            name = result.get("name", "unknown")
            file_path = result.get("file", "")
            line = result.get("line", 0)
            context = result.get("context", "")

            # Format the file path
            formatted_path = MarkdownFormatter._format_file_path(file_path)

            lines.append(f"## {i}. `{name}`")
            lines.append(f"**Location:** `{formatted_path}:{line}`")

            if context:
                lines.append(f"**Context:**")
                lines.append(f"```python")
                lines.append(context.strip())
                lines.append("```")

            lines.append("")

        return "\n".join(lines)
