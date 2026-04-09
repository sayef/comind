"""
CoMind response models — typed, validated, serialisable.

All CLI and MCP tools return instances of these models.  Using Pydantic
ensures that the data is always valid and structured, never raw dicts.
"""

from __future__ import annotations

from typing import Literal

from pydantic import BaseModel, ConfigDict, Field

# ─── shared primitives ────────────────────────────────────────────────────────


class SymbolRef(BaseModel):
    """Lightweight symbol reference used inside other responses."""

    model_config = ConfigDict(populate_by_name=True)

    id: str
    name: str
    kind: str = Field(alias="type", serialization_alias="kind")
    file_path: str
    line_start: int = 0
    line_end: int = 0
    signature: str | None = None
    docstring: str | None = None

    @classmethod
    def from_dict(cls, d: dict) -> SymbolRef:
        return cls(
            id=d.get("id", ""),
            name=d.get("name", ""),
            type=d.get("type", d.get("kind", "unknown")),
            file_path=d.get("file_path", ""),
            line_start=d.get("line_start", 0),
            line_end=d.get("line_end", 0),
            signature=d.get("signature"),
            docstring=d.get("docstring"),
        )


class CodeSnippet(BaseModel):
    content: str
    file_path: str
    line_start: int
    line_end: int


class ScoreBreakdown(BaseModel):
    text: float = 0.0
    semantic: float = 0.0
    graph: float = 0.0
    wiki: float = 0.0


# ─── ingest ───────────────────────────────────────────────────────────────────


class IngestResult(BaseModel):
    """Result of ingesting (indexing) a repository."""

    repo_name: str
    repo_path: str
    symbols: int
    relationships: int
    processes: int
    wiki_pages: int
    elapsed_seconds: float


# ─── repos ───────────────────────────────────────────────────────────────────


class RepoInfo(BaseModel):
    name: str
    has_graph: bool
    wiki_pages: int
    index_path: str


class ReposResponse(BaseModel):
    repos: list[RepoInfo]
    total: int


# ─── find ────────────────────────────────────────────────────────────────────


class FindResult(BaseModel):
    """Single result from a `find` query."""

    symbol: SymbolRef
    score: float
    breakdown: ScoreBreakdown
    snippet: CodeSnippet | None = None
    wiki_excerpt: str | None = None
    callers_count: int = 0
    callees_count: int = 0

    @classmethod
    def from_dict(cls, d: dict) -> FindResult:
        sym = d.get("symbol", {})
        bd = d.get("score_breakdown", {})
        snip = d.get("code_snippet")
        graph_ctx = d.get("graph_context", {}) or {}

        snippet = None
        if snip and snip.get("content"):
            snippet = CodeSnippet(
                content=snip["content"],
                file_path=snip.get("file_path", sym.get("file_path", "")),
                line_start=snip.get("line_start", 0),
                line_end=snip.get("line_end", 0),
            )

        wiki_ctx = d.get("wiki_context")
        wiki_excerpt: str | None = None
        if wiki_ctx:
            if isinstance(wiki_ctx, str):
                wiki_excerpt = wiki_ctx[:400]
            elif isinstance(wiki_ctx, dict):
                wiki_excerpt = wiki_ctx.get("summary") or wiki_ctx.get("content", "")[:400]

        return cls(
            symbol=SymbolRef.from_dict(sym),
            score=d.get("score", 0.0),
            breakdown=ScoreBreakdown(
                text=bd.get("text", 0.0),
                semantic=bd.get("semantic", 0.0),
                graph=bd.get("graph", 0.0),
                wiki=bd.get("wiki", 0.0),
            ),
            snippet=snippet,
            wiki_excerpt=wiki_excerpt,
            callers_count=len(graph_ctx.get("callers", [])),
            callees_count=len(graph_ctx.get("callees", [])),
        )


class FindResponse(BaseModel):
    query: str
    repo_name: str
    total: int
    results: list[FindResult]


# ─── zoom ────────────────────────────────────────────────────────────────────


class ZoomResponse(BaseModel):
    """360° symbol context returned by `zoom`."""

    symbol: SymbolRef
    callers: list[SymbolRef]
    callees: list[SymbolRef]
    dependencies: list[SymbolRef]
    processes: list[str]
    wiki_excerpt: str | None = None
    depth: int

    @classmethod
    def from_dict(cls, d: dict, depth: int = 2) -> ZoomResponse:
        rels = d.get("relationships", {})

        def _to_refs(items: list[dict]) -> list[SymbolRef]:
            return [SymbolRef.from_dict(i) for i in items if isinstance(i, dict)]

        processes: list[str] = []
        raw_procs = rels.get("processes", [])
        for p in raw_procs:
            if isinstance(p, dict):
                processes.append(p.get("name", str(p)))
            else:
                processes.append(str(p))

        wiki_excerpt: str | None = None
        wiki_ctx = d.get("wiki_context")
        if wiki_ctx:
            if isinstance(wiki_ctx, str):
                wiki_excerpt = wiki_ctx[:600]
            elif isinstance(wiki_ctx, dict):
                wiki_excerpt = wiki_ctx.get("summary") or wiki_ctx.get("content", "")[:600]

        return cls(
            symbol=SymbolRef.from_dict(d.get("symbol", {})),
            callers=_to_refs(rels.get("callers", [])),
            callees=_to_refs(rels.get("callees", [])),
            dependencies=_to_refs(rels.get("dependencies", [])),
            processes=processes,
            wiki_excerpt=wiki_excerpt,
            depth=depth,
        )


# ─── ripple ──────────────────────────────────────────────────────────────────


class RippleEntry(BaseModel):
    symbol: SymbolRef
    depth: int
    confidence: float


class RippleResponse(BaseModel):
    """Blast-radius response from `ripple`."""

    symbol: SymbolRef
    direction: str
    risk_level: Literal["LOW", "MEDIUM", "HIGH"]
    affected: list[RippleEntry]
    affected_processes: list[str]
    affected_modules: list[str]
    total_affected: int

    @classmethod
    def from_dict(cls, d: dict) -> RippleResponse:
        sym = SymbolRef.from_dict(d.get("symbol", {}))

        # Flatten upstream/downstream into a single affected list with depth tags
        affected: list[RippleEntry] = []
        for direction_key in ("upstream", "downstream"):
            section = d.get(direction_key, {})
            if isinstance(section, dict):
                for depth_key, syms in section.items():
                    try:
                        depth = int(depth_key.split("_")[-1])
                    except (ValueError, IndexError):
                        depth = 0
                    for s in syms or []:
                        affected.append(
                            RippleEntry(
                                symbol=SymbolRef.from_dict(s),
                                depth=depth,
                                confidence=s.get("confidence", 1.0),
                            )
                        )

        # Also handle flat "affected_symbols" list
        for s in d.get("affected_symbols", []):
            affected.append(
                RippleEntry(
                    symbol=SymbolRef.from_dict(s),
                    depth=s.get("depth", 1),
                    confidence=s.get("confidence", 1.0),
                )
            )

        procs = [
            (p.get("name", str(p)) if isinstance(p, dict) else str(p))
            for p in d.get("affected_processes", [])
        ]

        return cls(
            symbol=sym,
            direction=d.get("direction", "upstream"),
            risk_level=d.get("risk_level", "LOW"),
            affected=affected,
            affected_processes=procs,
            affected_modules=d.get("affected_modules", []),
            total_affected=len(affected),
        )


# ─── thread ──────────────────────────────────────────────────────────────────


class ThreadStep(BaseModel):
    step: int
    name: str
    kind: str = "function"
    file_path: str | None = None


class ThreadResponse(BaseModel):
    """Execution trace from `thread`."""

    entry_point: str
    steps: list[ThreadStep]
    total_steps: int

    @classmethod
    def from_dict(cls, d: dict, entry_point: str) -> ThreadResponse:
        steps: list[ThreadStep] = []
        for raw in d.get("flow", d.get("steps", [])):
            if isinstance(raw, dict):
                sym = raw.get("symbol", raw)
                name = sym.get("name", "") if isinstance(sym, dict) else str(sym)
                steps.append(
                    ThreadStep(
                        step=raw.get("step", len(steps)),
                        name=name,
                        kind=sym.get("type", "function") if isinstance(sym, dict) else "function",
                        file_path=sym.get("file_path") if isinstance(sym, dict) else None,
                    )
                )
            else:
                steps.append(ThreadStep(step=len(steps), name=str(raw)))
        return cls(entry_point=entry_point, steps=steps, total_steps=len(steps))


# ─── guide ───────────────────────────────────────────────────────────────────


class StyleSection(BaseModel):
    """A single section of the style guide, optionally filtered by a query."""

    category: str
    summary: str
    details: list[str] = Field(default_factory=list)
    prevalence: str | None = None
    examples: list[str] = Field(default_factory=list)


class GuideResponse(BaseModel):
    """Style guide response from `guide`."""

    repo_name: str
    query: str | None = None
    sections: list[StyleSection]
    recommendation: str | None = None
