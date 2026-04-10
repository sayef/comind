"""
Style Guide Store — persist, load, and query extracted style patterns.

Usage
-----
During ingest:
    store = StyleGuideStore(repo_name)
    await store.save(style_patterns)

At query time:
    store = StyleGuideStore(repo_name)
    response = await store.query("how should I name functions?")
    full    = await store.full_guide()
"""

from __future__ import annotations

import json
from typing import TYPE_CHECKING, Any

from comind.config import get_settings
from comind.logging_config import get_logger
from comind.models import GuideResponse, StyleSection

if TYPE_CHECKING:
    from pathlib import Path

    from comind.style.style_extractor import StylePatterns

logger = get_logger(__name__)

# ─── keyword → category mappings ─────────────────────────────────────────────

_TOPIC_MAP: dict[str, list[str]] = {
    "naming": [
        "naming",
        "convention",
        "name",
        "snake",
        "camel",
        "pascal",
        "function name",
        "variable name",
    ],
    "typing": [
        "type",
        "hint",
        "annotation",
        "typed",
        "typevar",
        "protocol",
        "generic",
        "optional",
        "union",
    ],
    "docstring": [
        "doc",
        "docstring",
        "comment",
        "documentation",
        "sphinx",
        "google",
        "numpy",
        "rest",
        "reStructured",
    ],
    "error": ["error", "exception", "raise", "try", "catch", "handling", "validation"],
    "logging": ["log", "logging", "logger", "structlog", "observability", "debug", "info", "warn"],
    "async": ["async", "await", "coroutine", "asyncio", "concurrent", "parallel"],
    "imports": ["import", "from import", "dependency", "module"],
    "formatting": ["format", "string", "f-string", "style", "lint", "line length", "ruff", "black"],
    "environment": [
        "python version",
        "package manager",
        "poetry",
        "pipenv",
        "uv",
        "lockfile",
        "tooling",
    ],
    "patterns": ["comprehension", "list comp", "context manager", "with statement", "idiom"],
}


def _match_topics(query: str) -> list[str]:
    """Return category keys that match the query."""
    q = query.lower()
    return [cat for cat, keywords in _TOPIC_MAP.items() if any(kw in q for kw in keywords)]


# ─── serialisation helpers ────────────────────────────────────────────────────


def _counter_to_dict(c: object) -> dict[str, int]:
    if hasattr(c, "most_common"):
        return dict(c.most_common())
    if isinstance(c, dict):
        return c
    return {}


def _pattern_stats_to_dict(ps: Any) -> dict:
    if hasattr(ps, "count"):
        return {
            "count": ps.count,
            "total": ps.total,
            "percentage": round(ps.percentage, 1),
            "prevalence": ps.prevalence,
            "examples": list(ps.examples)[:5],
        }
    return {}


def _patterns_to_json(p: StylePatterns) -> dict:
    """Convert StylePatterns dataclass to a JSON-safe dict."""
    return {
        "environment": {
            "python_version": p.python_version,
            "package_manager": p.package_manager,
            "has_lockfile": p.has_lockfile,
            "max_line_length": p.max_line_length,
        },
        "naming": {
            "functions": _counter_to_dict(p.function_naming),
            "classes": _counter_to_dict(p.class_naming),
            "constants": _counter_to_dict(p.constant_naming),
            "private": _counter_to_dict(p.private_naming),
        },
        "typing": {
            "type_hints": _pattern_stats_to_dict(p.type_hints_usage),
            "return_types": _pattern_stats_to_dict(p.return_type_usage),
            "advanced": {k: _pattern_stats_to_dict(v) for k, v in p.advanced_typing.items()},
        },
        "docstring": {
            "format": p.docstring_format,
            "coverage": _pattern_stats_to_dict(p.docstring_coverage),
        },
        "error_handling": {
            "exception_patterns": list(p.exception_patterns)[:20],
            "styles": _counter_to_dict(p.error_handling_style),
        },
        "logging": {
            "init_pattern": p.logger_init_pattern,
            "structured": p.structured_logging,
        },
        "async": {
            "usage": _pattern_stats_to_dict(p.async_usage),
        },
        "imports": {
            "style": _counter_to_dict(p.import_style),
            "common": _counter_to_dict(p.common_imports),
        },
        "formatting": {
            "string_style": _counter_to_dict(p.string_formatting),
            "comprehensions": _pattern_stats_to_dict(p.comprehension_usage),
            "context_managers": _pattern_stats_to_dict(p.context_manager_usage),
        },
    }


# ─── store ────────────────────────────────────────────────────────────────────


class StyleGuideStore:
    """Persist and query style patterns for a repository."""

    def __init__(self, repo_name: str, data_dir: Path | None = None) -> None:
        if data_dir is None:
            data_dir = get_settings().storage.wiki_dir
        self._base = data_dir / repo_name
        self._json_path = self._base / "style_patterns.json"
        self._md_path = self._base / f"{repo_name.upper()}_STYLE_GUIDE.md"
        self._repo_name = repo_name
        self._data: dict | None = None

    async def save(self, patterns: StylePatterns, markdown: str) -> None:
        """Persist both the structured JSON and the markdown style guide."""
        self._base.mkdir(parents=True, exist_ok=True)
        data = _patterns_to_json(patterns)
        self._json_path.write_text(json.dumps(data, indent=2))
        self._md_path.write_text(markdown)
        self._data = data
        logger.info("Style guide saved", repo=self._repo_name, path=str(self._base))

    def _load(self) -> dict | None:
        if self._data is not None:
            return self._data
        if self._json_path.exists():
            self._data = json.loads(self._json_path.read_text())
        return self._data

    async def full_guide(self) -> str | None:
        """Return the full markdown style guide, or None if not yet generated."""
        if self._md_path.exists():
            return self._md_path.read_text()
        return None

    async def query(self, query: str | None, repo_name: str) -> GuideResponse:
        """Answer a natural-language style question.

        With no query, returns all sections.  With a query, returns only the
        sections most relevant to the question.
        """
        data = self._load()
        if data is None:
            return GuideResponse(
                repo_name=repo_name,
                query=query,
                sections=[],
                recommendation="Style guide not yet generated. Run: comind ingest <repo>",
            )

        all_sections = _build_sections(data)

        if not query:
            return GuideResponse(repo_name=repo_name, query=None, sections=all_sections)

        matched_topics = _match_topics(query)
        if matched_topics:
            filtered = [s for s in all_sections if s.category.lower() in matched_topics]
        else:
            # Fallback: return all sections when no specific topic matched
            filtered = all_sections

        recommendation = _build_recommendation(query, data, filtered)
        return GuideResponse(
            repo_name=repo_name,
            query=query,
            sections=filtered,
            recommendation=recommendation,
        )


# ─── section builders ─────────────────────────────────────────────────────────


def _dominant(counter_dict: dict[str, int]) -> str | None:
    if not counter_dict:
        return None
    return max(counter_dict, key=lambda k: counter_dict[k])


def _build_sections(data: dict) -> list[StyleSection]:
    sections: list[StyleSection] = []

    # Environment
    env = data.get("environment", {})
    sections.append(
        StyleSection(
            category="environment",
            summary=f"Python {env.get('python_version', '?')}, {env.get('package_manager', 'unknown')} package manager",
            details=[
                f"Python version: {env.get('python_version', 'unknown')}",
                f"Package manager: {env.get('package_manager', 'unknown')}",
                f"Lockfile present: {env.get('has_lockfile', False)}",
                f"Max line length: {env.get('max_line_length', 88)}",
            ],
        )
    )

    # Naming
    naming = data.get("naming", {})
    fn_style = _dominant(naming.get("functions", {})) or "snake_case"
    cls_style = _dominant(naming.get("classes", {})) or "PascalCase"
    sections.append(
        StyleSection(
            category="naming",
            summary=f"Functions: {fn_style}  |  Classes: {cls_style}",
            details=[
                f"Function naming: {fn_style} (use this for all functions and methods)",
                f"Class naming: {cls_style} (use this for all class definitions)",
                *(
                    [f"Constant naming: {_dominant(naming.get('constants', {}))}"]
                    if naming.get("constants")
                    else []
                ),
                *(
                    [f"Private naming: {_dominant(naming.get('private', {}))}"]
                    if naming.get("private")
                    else []
                ),
            ],
            examples=list(naming.get("functions", {}).keys())[:4],
        )
    )

    # Typing
    typing_ = data.get("typing", {})
    th = typing_.get("type_hints", {})
    rt = typing_.get("return_types", {})
    sections.append(
        StyleSection(
            category="typing",
            summary=f"Type hints: {th.get('prevalence', 'Unknown')} ({th.get('percentage', 0):.0f}%)  |  Return types: {rt.get('prevalence', 'Unknown')} ({rt.get('percentage', 0):.0f}%)",
            details=[
                f"Type hint usage: {th.get('prevalence', '?')} ({th.get('percentage', 0):.1f}% of symbols annotated)",
                f"Return type annotation: {rt.get('prevalence', '?')} ({rt.get('percentage', 0):.1f}% of functions)",
                *[
                    f"{k}: {v.get('prevalence', '?')}"
                    for k, v in typing_.get("advanced", {}).items()
                    if v.get("count", 0) > 0
                ],
            ],
            prevalence=th.get("prevalence"),
        )
    )

    # Docstrings
    doc = data.get("docstring", {})
    cov = doc.get("coverage", {})
    sections.append(
        StyleSection(
            category="docstring",
            summary=f"{doc.get('format', 'Unknown')} format, {cov.get('prevalence', 'Unknown')} coverage ({cov.get('percentage', 0):.0f}%)",
            details=[
                f"Docstring format: {doc.get('format', 'Unknown')} — use this format for all docstrings",
                f"Coverage: {cov.get('prevalence', '?')} ({cov.get('percentage', 0):.1f}% of public symbols documented)",
            ],
            examples=cov.get("examples", [])[:3],
            prevalence=cov.get("prevalence"),
        )
    )

    # Error handling
    err = data.get("error_handling", {})
    dominant_style = _dominant(err.get("styles", {}))
    sections.append(
        StyleSection(
            category="error",
            summary=f"Exception style: {dominant_style or 'standard'}",
            details=[
                *([f"Primary style: {dominant_style}"] if dominant_style else []),
                *[f"Exception type used: {e}" for e in err.get("exception_patterns", [])[:5]],
            ],
            examples=err.get("exception_patterns", [])[:5],
        )
    )

    # Logging
    log = data.get("logging", {})
    sections.append(
        StyleSection(
            category="logging",
            summary=(
                f"{'Structured' if log.get('structured') else 'Standard'} logging  |  init: `{log.get('init_pattern', 'logging.getLogger')}`"
            ),
            details=[
                f"Logger initialisation: `{log.get('init_pattern', 'logging.getLogger(__name__)')}`",
                f"Structured logging: {'yes (use key=value pairs)' if log.get('structured') else 'no (use string messages)'}",
            ],
        )
    )

    # Async
    async_ = data.get("async", {}).get("usage", {})
    sections.append(
        StyleSection(
            category="async",
            summary=f"Async usage: {async_.get('prevalence', 'Unknown')} ({async_.get('percentage', 0):.0f}%)",
            details=[
                f"Async/await prevalence: {async_.get('prevalence', '?')} ({async_.get('percentage', 0):.1f}% of functions are async)",
                *(
                    ["Prefer async patterns for I/O operations"]
                    if async_.get("percentage", 0) > 50
                    else []
                ),
            ],
            prevalence=async_.get("prevalence"),
        )
    )

    # Imports
    imports = data.get("imports", {})
    imp_style = _dominant(imports.get("style", {})) or "from_import"
    common = list(imports.get("common", {}).keys())[:6]
    sections.append(
        StyleSection(
            category="imports",
            summary=f"Style: {imp_style}",
            details=[
                f"Preferred import style: {imp_style}",
                *(["Common deps: " + ", ".join(common)] if common else []),
            ],
            examples=common,
        )
    )

    # Formatting / idioms
    fmt = data.get("formatting", {})
    str_style = _dominant(fmt.get("string_style", {})) or "f-string"
    comp = fmt.get("comprehensions", {})
    sections.append(
        StyleSection(
            category="patterns",
            summary=f"Strings: {str_style}  |  Comprehensions: {comp.get('prevalence', 'Unknown')}",
            details=[
                f"String formatting: {str_style} (use this for string interpolation)",
                f"Comprehension usage: {comp.get('prevalence', '?')} ({comp.get('percentage', 0):.1f}%)",
                f"Context managers: {fmt.get('context_managers', {}).get('prevalence', '?')}",
            ],
        )
    )

    return sections


def _build_recommendation(_query: str, _data: dict, sections: list[StyleSection]) -> str:
    """Generate a concise, actionable recommendation based on matched sections."""
    if not sections:
        return "No specific style guidance found for this query."

    parts: list[str] = []
    for section in sections[:3]:
        parts.append(f"**{section.category.title()}**: {section.summary}")
        if section.details:
            parts.append(section.details[0])

    return "\n".join(parts)
