"""
File search utilities — grep and glob over repo source files.

Used by CLI commands and MCP tools so AI agents can drill into raw file
content after semantic search surfaces the relevant symbols.

Grep  — ripgrep subprocess (fast), Python regex fallback (portable)
Glob  — pathlib.Path.rglob with brace-expansion support
Read  — read a file or a line range from a repo-relative path
"""

from __future__ import annotations

import json
import re
import subprocess
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

# Maximum size of a single file we'll read (10 MB)
_MAX_FILE_BYTES = 10 * 1024 * 1024


# ── helpers ────────────────────────────────────────────────────────────────


def _safe_path(root: Path, relative: str) -> Path | None:
    """Return resolved path only if it stays inside *root*. Blocks traversal."""
    if ".." in relative or relative.startswith("~"):
        return None
    resolved = (root / relative).resolve()
    try:
        resolved.relative_to(root.resolve())
    except ValueError:
        return None
    return resolved


def _expand_braces(pattern: str) -> list[str]:
    """Expand a single brace group, e.g. '*.{py,pyi}' → ['*.py', '*.pyi']."""
    m = re.search(r"\{([^{}]+)\}", pattern)
    if not m:
        return [pattern]
    prefix, suffix = pattern[: m.start()], pattern[m.end() :]
    return [f"{prefix}{alt}{suffix}" for alt in m.group(1).split(",")]


# ── data classes ───────────────────────────────────────────────────────────


@dataclass
class GrepMatch:
    file: str  # repo-relative path
    line: int
    column: int
    text: str  # the matched line (stripped)
    context_before: list[str] = field(default_factory=list)
    context_after: list[str] = field(default_factory=list)

    def to_dict(self) -> dict[str, Any]:
        d: dict[str, Any] = {"file": self.file, "line": self.line, "text": self.text}
        if self.context_before:
            d["context_before"] = self.context_before
        if self.context_after:
            d["context_after"] = self.context_after
        return d


@dataclass
class GrepResult:
    pattern: str
    output_mode: str
    matches: list[GrepMatch] = field(default_factory=list)
    files: list[str] = field(default_factory=list)  # for files_with_matches
    counts: dict[str, int] = field(default_factory=dict)  # for count
    total: int = 0
    truncated: bool = False

    def to_dict(self) -> dict[str, Any]:
        return {
            "pattern": self.pattern,
            "output_mode": self.output_mode,
            "total": self.total,
            "truncated": self.truncated,
            "matches": [m.to_dict() for m in self.matches],
            "files": self.files,
            "counts": self.counts,
        }


@dataclass
class ReadResult:
    file: str  # repo-relative path
    content: str
    start_line: int
    end_line: int
    total_lines: int

    def to_dict(self) -> dict[str, Any]:
        return {
            "file": self.file,
            "content": self.content,
            "start_line": self.start_line,
            "end_line": self.end_line,
            "total_lines": self.total_lines,
        }


# ── grep ───────────────────────────────────────────────────────────────────


class GrepEngine:
    """Regex search over repo source files.

    Tries ripgrep (``rg``) first for speed, falls back to pure-Python.
    """

    def search(
        self,
        pattern: str,
        root: Path,
        *,
        glob: str | None = None,
        output_mode: str = "content",  # "content" | "files_with_matches" | "count"
        context_lines: int = 2,
        case_sensitive: bool = False,
        max_results: int = 100,
    ) -> GrepResult:
        result = GrepResult(pattern=pattern, output_mode=output_mode)

        try:
            self._rg(
                result, pattern, root, glob, output_mode, context_lines, case_sensitive, max_results
            )
        except FileNotFoundError:
            # ripgrep not installed — use Python fallback
            self._py(
                result, pattern, root, glob, output_mode, context_lines, case_sensitive, max_results
            )

        result.total = (
            len(result.matches)
            if output_mode == "content"
            else len(result.files)
            if output_mode == "files_with_matches"
            else sum(result.counts.values())
        )
        return result

    # ── ripgrep ────────────────────────────────────────────────────────────

    def _rg(
        self,
        result: GrepResult,
        pattern: str,
        root: Path,
        glob: str | None,
        output_mode: str,
        context_lines: int,
        case_sensitive: bool,
        max_results: int,
    ) -> None:
        cmd = ["rg", "--json"]
        if not case_sensitive:
            cmd.append("-i")
        if output_mode == "files_with_matches":
            cmd.append("-l")
        elif output_mode == "count":
            cmd.append("-c")
        else:
            cmd += [f"-A{context_lines}", f"-B{context_lines}"]
        if glob:
            for g in _expand_braces(glob):
                cmd += ["-g", g]
        cmd += [pattern, str(root)]

        proc = subprocess.run(cmd, check=False, capture_output=True, text=True, timeout=30)
        seen_files: set[str] = set()
        count = 0

        for raw in proc.stdout.splitlines():
            if not raw.strip():
                continue
            try:
                obj = json.loads(raw)
            except json.JSONDecodeError:
                continue

            kind = obj.get("type")
            data = obj.get("data", {})

            rel = self._rel(data.get("path", {}).get("text", ""), root)

            if output_mode == "files_with_matches" and kind == "begin":
                if rel and rel not in seen_files:
                    result.files.append(rel)
                    seen_files.add(rel)
                    count += 1
                    if count >= max_results:
                        result.truncated = True
                        return

            elif output_mode == "count" and kind == "summary":
                for _stat in data.get("stats", {}).get("matched_lines", []):
                    pass  # handled below

            elif output_mode == "count" and kind == "match":
                result.counts[rel] = result.counts.get(rel, 0) + 1

            elif output_mode == "content" and kind == "match":
                for sub in data.get("submatches", [{}]):
                    line_no = data.get("line_number", 0)
                    col = sub.get("start", 0)
                    text = data.get("lines", {}).get("text", "").rstrip("\n")
                    result.matches.append(GrepMatch(file=rel, line=line_no, column=col, text=text))
                    count += 1
                    if count >= max_results:
                        result.truncated = True
                        return

    # ── python fallback ────────────────────────���───────────────────────────

    def _py(
        self,
        result: GrepResult,
        pattern: str,
        root: Path,
        glob: str | None,
        output_mode: str,
        context_lines: int,
        case_sensitive: bool,
        max_results: int,
    ) -> None:
        flags = 0 if case_sensitive else re.IGNORECASE
        try:
            rx = re.compile(pattern, flags)
        except re.error:
            return

        globs = _expand_braces(glob) if glob else ["**/*"]
        files: list[Path] = []
        for g in globs:
            files.extend(root.rglob(g))
        files = sorted({f for f in files if f.is_file()})

        count = 0
        for path in files:
            if path.stat().st_size > _MAX_FILE_BYTES:
                continue
            rel = self._rel(str(path), root)
            try:
                lines = path.read_text(errors="replace").splitlines()
            except OSError:
                continue

            file_count = 0
            for i, line in enumerate(lines):
                if rx.search(line):
                    file_count += 1
                    if output_mode == "files_with_matches":
                        if rel not in result.files:
                            result.files.append(rel)
                            count += 1
                        break
                    if output_mode == "count":
                        result.counts[rel] = result.counts.get(rel, 0) + 1
                    else:
                        before = [lines[j] for j in range(max(0, i - context_lines), i)]
                        after = [
                            lines[j] for j in range(i + 1, min(len(lines), i + 1 + context_lines))
                        ]
                        result.matches.append(
                            GrepMatch(
                                file=rel,
                                line=i + 1,
                                column=0,
                                text=line,
                                context_before=before,
                                context_after=after,
                            )
                        )
                        count += 1

                    if count >= max_results:
                        result.truncated = True
                        return

    @staticmethod
    def _rel(path_str: str, root: Path) -> str:
        try:
            return str(Path(path_str).relative_to(root))
        except ValueError:
            return path_str


# ── glob ───────────────────────────────────────────────────────────────────


class GlobEngine:
    """File-path pattern search within a repo."""

    def search(
        self,
        pattern: str,
        root: Path,
        max_results: int = 200,
    ) -> list[str]:
        """Return repo-relative paths matching *pattern* (glob syntax)."""
        patterns = _expand_braces(pattern)
        seen: set[str] = set()
        results: list[str] = []

        for pat in patterns:
            for path in sorted(root.rglob(pat), key=lambda p: p.stat().st_mtime, reverse=True):
                if not path.is_file():
                    continue
                rel = str(path.relative_to(root))
                if rel not in seen:
                    seen.add(rel)
                    results.append(rel)
                    if len(results) >= max_results:
                        return results

        return results


# ── read ───────────────────────────────────────────────────────────────────


class FileReader:
    """Read a file or a line range from a repo."""

    def read(
        self,
        relative_path: str,
        root: Path,
        start_line: int | None = None,
        end_line: int | None = None,
    ) -> ReadResult | None:
        """Return file content (or a slice) for *relative_path* inside *root*.

        Line numbers are 1-based and inclusive.
        Returns None if the path is unsafe or the file doesn't exist.
        """
        path = _safe_path(root, relative_path)
        if path is None or not path.exists() or not path.is_file():
            return None
        if path.stat().st_size > _MAX_FILE_BYTES:
            return None

        try:
            all_lines = path.read_text(errors="replace").splitlines()
        except OSError:
            return None

        total = len(all_lines)
        s = max(1, start_line or 1)
        e = min(total, end_line or total)
        content = "\n".join(all_lines[s - 1 : e])

        return ReadResult(
            file=relative_path,
            content=content,
            start_line=s,
            end_line=e,
            total_lines=total,
        )
