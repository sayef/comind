"""
Wiki Generator

Orchestrates the full wiki generation pipeline:
  Phase 0: Validate prerequisites + gather graph structure
  Phase 1: Build module tree (one LLM call)
  Phase 2: Generate module pages (one LLM call per module, bottom-up)
  Phase 3: Generate overview page

Supports incremental updates via git diff + module-file mapping.
"""

import asyncio
import json
import subprocess
from collections.abc import Callable
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

from comind.llm.llm_client import LLMConfig, call_llm
from comind.storage.graph_adapter import KnowledgeGraph
from comind.wiki.wiki_graph_queries import (
    get_all_files,
    get_all_processes,
    get_files_with_exports,
    get_inter_module_call_edges,
    get_inter_module_edges_for_overview,
    get_intra_module_call_edges,
    get_processes_for_files,
    read_file_content,
)
from comind.wiki.wiki_prompts import (
    GROUPING_SYSTEM_PROMPT,
    GROUPING_USER_PROMPT,
    MODULE_SYSTEM_PROMPT,
    MODULE_USER_PROMPT,
    OVERVIEW_SYSTEM_PROMPT,
    OVERVIEW_USER_PROMPT,
    PARENT_SYSTEM_PROMPT,
    PARENT_USER_PROMPT,
    fill_template,
    format_call_edges,
    format_directory_tree,
    format_file_list_for_grouping,
    format_processes,
)


@dataclass
class ModuleTreeNode:
    """Module tree node"""

    name: str
    slug: str
    files: list[str]
    children: list["ModuleTreeNode"] | None = None


@dataclass
class WikiMeta:
    """Wiki metadata"""

    from_commit: str
    generated_at: str
    model: str
    module_files: dict[str, list[str]]
    module_tree: list[dict[str, Any]]


@dataclass
class WikiRunResult:
    """Wiki generation result"""

    pages_generated: int
    mode: str  # 'full' | 'incremental' | 'up-to-date'
    failed_modules: list[str]
    module_tree: list[ModuleTreeNode] | None = None


ProgressCallback = Callable[[str, int, str | None], None]


class WikiGenerator:
    """LLM-powered wiki generator"""

    def __init__(
        self,
        repo_path: str,
        storage_path: str,
        graph: KnowledgeGraph,
        llm_config: LLMConfig,
        max_tokens_per_module: int = 30000,
        concurrency: int = 3,
        force: bool = False,
        on_progress: ProgressCallback | None = None,
    ):
        self.repo_path = Path(repo_path)
        self.storage_path = Path(storage_path)
        self.wiki_dir = self.storage_path / "wiki"
        self.graph = graph
        self.llm_config = llm_config
        self.max_tokens_per_module = max_tokens_per_module
        self.concurrency = concurrency
        self.force = force
        self.on_progress = on_progress or (lambda *_args: None)
        self.failed_modules: list[str] = []
        self.last_percent = 0

    def _progress(self, phase: str, percent: int, detail: str | None = None):
        """Report progress"""
        if percent > 0:
            self.last_percent = percent
        self.on_progress(phase, percent, detail)

    def _stream_opts(self, label: str, fixed_percent: int | None = None, percent_range: int = 10):
        """Create streaming options for LLM progress tracking"""
        has_fixed_start = fixed_percent is not None
        start_percent = fixed_percent if fixed_percent is not None else self.last_percent
        expected_tokens = 2000

        def on_chunk(chars: int):
            tokens = chars // 4

            if has_fixed_start:
                progress = min(1.0, tokens / expected_tokens)
                pct = int(start_percent + progress * percent_range)
                self._progress("stream", pct, f"{label} ({tokens} tok)")
            else:
                self._progress("stream", self.last_percent, f"{label} ({tokens} tok)")

        return on_chunk

    async def run(self) -> WikiRunResult:
        """Main entry point - runs full pipeline or incremental update"""
        self.wiki_dir.mkdir(parents=True, exist_ok=True)

        existing_meta = await self._load_wiki_meta()
        current_commit = self._get_current_commit()

        # Up-to-date check (skip if --force)
        if not self.force and existing_meta and existing_meta.from_commit == current_commit:
            return WikiRunResult(pages_generated=0, mode="up-to-date", failed_modules=[])

        # Force mode: delete snapshot to force full re-grouping
        if self.force:
            snapshot_path = self.wiki_dir / "first_module_tree.json"
            if snapshot_path.exists():
                snapshot_path.unlink()

            # Delete existing module pages
            for md_file in self.wiki_dir.glob("*.md"):
                md_file.unlink()

        # Run full generation
        return await self._full_generation(current_commit)

    async def _full_generation(self, current_commit: str) -> WikiRunResult:
        """Full wiki generation pipeline"""
        pages_generated = 0

        # Phase 0: Gather structure
        self._progress("gather", 5, "Querying graph for file structure...")
        files_with_exports = await get_files_with_exports(self.graph)
        all_files = await get_all_files(self.graph)

        # Filter to source files only (exclude tests, configs, etc.)
        source_files = [f for f in all_files if self._is_source_file(f)]

        if not source_files:
            raise ValueError("No source files found in the knowledge graph")

        # Build enriched file list
        export_map = {f["file_path"]: f for f in files_with_exports}
        enriched_files = [
            export_map.get(fp, {"file_path": fp, "symbols": []}) for fp in source_files
        ]

        self._progress("gather", 10, f"Found {len(source_files)} source files")

        # Phase 1: Build module tree
        module_tree = await self._build_module_tree(enriched_files)

        # Phase 2: Generate module pages
        total_modules = self._count_modules(module_tree)
        modules_processed = 0

        def report_progress(module_name: str | None = None):
            nonlocal modules_processed
            modules_processed += 1
            percent = 30 + int((modules_processed / total_modules) * 55)
            detail = (
                f"{modules_processed}/{total_modules} — {module_name}"
                if module_name
                else f"{modules_processed}/{total_modules} modules"
            )
            self._progress("modules", percent, detail)

        # Flatten tree into layers: leaves first, then parents
        leaves, parents = self._flatten_module_tree(module_tree)

        # Process leaf modules in parallel
        leaf_tasks = [self._generate_leaf_page(node) for node in leaves]

        # Run with concurrency limit
        for i in range(0, len(leaf_tasks), self.concurrency):
            batch = leaf_tasks[i : i + self.concurrency]
            results = await asyncio.gather(*batch, return_exceptions=True)

            for j, result in enumerate(results):
                node = leaves[i + j]
                if isinstance(result, Exception):
                    self.failed_modules.append(node.name)
                    report_progress(f"Failed: {node.name}")
                else:
                    pages_generated += 1
                    report_progress(node.name)

        # Process parent modules sequentially
        for node in parents:
            try:
                await self._generate_parent_page(node)
                pages_generated += 1
                report_progress(node.name)
            except Exception:
                self.failed_modules.append(node.name)
                report_progress(f"Failed: {node.name}")

        # Phase 3: Generate overview
        self._progress("overview", 88, "Generating overview page...")
        await self._generate_overview(module_tree)
        pages_generated += 1

        # Save metadata
        self._progress("finalize", 95, "Saving metadata...")
        module_files = self._extract_module_files(module_tree)
        await self._save_module_tree(module_tree)
        await self._save_wiki_meta(
            WikiMeta(
                from_commit=current_commit,
                generated_at="",  # Will be set in save
                model=self.llm_config.model,
                module_files=module_files,
                module_tree=[self._node_to_dict(n) for n in module_tree],
            )
        )

        self._progress("done", 100, "Wiki generation complete")
        return WikiRunResult(
            pages_generated=pages_generated, mode="full", failed_modules=self.failed_modules.copy()
        )

    async def _build_module_tree(self, files: list[dict[str, Any]]) -> list[ModuleTreeNode]:
        """Phase 1: Build module tree using LLM"""
        # Check for existing snapshot
        snapshot_path = self.wiki_dir / "first_module_tree.json"
        if snapshot_path.exists() and not self.force:
            data = await asyncio.to_thread(snapshot_path.read_text, encoding="utf-8")
            snapshot_data = json.loads(data)
            self._progress("grouping", 25, "Using cached module tree")
            return [self._dict_to_node(n) for n in snapshot_data]

        # Call LLM to group files into modules
        self._progress("grouping", 15, "Grouping files into modules...")

        file_list = format_file_list_for_grouping(files)
        dir_tree = format_directory_tree([f["file_path"] for f in files])

        prompt = fill_template(
            GROUPING_USER_PROMPT, {"FILE_LIST": file_list, "DIRECTORY_TREE": dir_tree}
        )

        response = await call_llm(
            prompt,
            self.llm_config,
            GROUPING_SYSTEM_PROMPT,
            None,  # No streaming
        )

        # Parse JSON response
        content = response.content.strip()
        # Remove markdown code blocks if present
        if content.startswith("```"):
            lines = content.split("\n")
            content = "\n".join(lines[1:-1])

        module_map = json.loads(content)

        # Convert to tree nodes
        tree = []
        for module_name, file_paths in module_map.items():
            slug = module_name.lower().replace(" ", "-").replace("/", "-")
            tree.append(
                ModuleTreeNode(name=module_name, slug=slug, files=file_paths, children=None)
            )

        # Save snapshot
        snapshot_content = json.dumps([self._node_to_dict(n) for n in tree], indent=2)
        await asyncio.to_thread(snapshot_path.write_text, snapshot_content, encoding="utf-8")

        self._progress("grouping", 28, f"Created {len(tree)} modules")
        return tree

    async def _generate_leaf_page(self, node: ModuleTreeNode):
        """Generate documentation for a leaf module"""
        page_path = self.wiki_dir / f"{node.slug}.md"
        if page_path.exists() and not self.force:
            return

        # Read source code
        source_parts = []
        for file_path in node.files[:10]:  # Limit to 10 files
            content = await read_file_content(file_path, max_lines=200)
            source_parts.append(f"### {file_path}\n\n```python\n{content}\n```\n")

        source_code = "\n".join(source_parts)

        # Get call graph data
        intra_calls = await get_intra_module_call_edges(self.graph, node.files)
        inter_module = await get_inter_module_call_edges(self.graph, node.files)
        processes = await get_processes_for_files(self.graph, node.files)

        # Build prompt
        prompt = fill_template(
            MODULE_USER_PROMPT,
            {
                "MODULE_NAME": node.name,
                "SOURCE_CODE": source_code[: self.max_tokens_per_module * 4],  # Rough token limit
                "INTRA_CALLS": format_call_edges(intra_calls),
                "OUTGOING_CALLS": format_call_edges(inter_module["outgoing"]),
                "INCOMING_CALLS": format_call_edges(inter_module["incoming"]),
                "PROCESSES": format_processes(processes),
            },
        )

        # Call LLM
        response = await call_llm(
            prompt,
            self.llm_config,
            MODULE_SYSTEM_PROMPT,
            None,  # No streaming
        )

        # Save page
        with page_path.open("w") as f:
            f.write(response.content)

    async def _generate_parent_page(self, node: ModuleTreeNode):
        """Generate documentation for a parent module"""
        page_path = self.wiki_dir / f"{node.slug}.md"
        if page_path.exists() and not self.force:
            return

        # Read child docs
        children_docs = []
        for child in node.children or []:
            child_path = self.wiki_dir / f"{child.slug}.md"
            if child_path.exists():
                child_content = await asyncio.to_thread(child_path.read_text, encoding="utf-8")
                children_docs.append(f"## {child.name}\n\n{child_content}")

        # Get cross-module data
        all_files = node.files
        for child in node.children or []:
            all_files.extend(child.files)

        inter_module = await get_inter_module_call_edges(self.graph, all_files)
        processes = await get_processes_for_files(self.graph, all_files)

        # Build prompt
        prompt = fill_template(
            PARENT_USER_PROMPT,
            {
                "MODULE_NAME": node.name,
                "CHILDREN_DOCS": "\n\n".join(children_docs),
                "CROSS_MODULE_CALLS": format_call_edges(
                    inter_module["outgoing"] + inter_module["incoming"]
                ),
                "CROSS_PROCESSES": format_processes(processes),
            },
        )

        # Call LLM
        response = await call_llm(
            prompt,
            self.llm_config,
            PARENT_SYSTEM_PROMPT,
            None,  # No streaming
        )

        # Save page
        with page_path.open("w") as f:
            f.write(response.content)

    async def _generate_overview(self, module_tree: list[ModuleTreeNode]):
        """Generate overview page"""
        # Read module summaries
        module_summaries = []
        for node in module_tree:
            page_path = self.wiki_dir / f"{node.slug}.md"
            if page_path.exists():
                content = await asyncio.to_thread(page_path.read_text, encoding="utf-8")
                # Extract first paragraph as summary
                lines = content.split("\n")
                summary = "\n".join(lines[:5])
                module_summaries.append(f"### {node.name}\n\n{summary}")

        # Get inter-module edges
        module_files = self._extract_module_files(module_tree)
        module_edges = await get_inter_module_edges_for_overview(self.graph, module_files)

        # Get top processes
        all_processes = await get_all_processes(self.graph)
        top_processes = all_processes[:10]

        # Build prompt
        project_info = f"Repository: {self.repo_path.name}"

        prompt = fill_template(
            OVERVIEW_USER_PROMPT,
            {
                "PROJECT_INFO": project_info,
                "MODULE_SUMMARIES": "\n\n".join(module_summaries),
                "MODULE_EDGES": format_call_edges(module_edges),
                "TOP_PROCESSES": format_processes(top_processes),
            },
        )

        # Call LLM
        response = await call_llm(
            prompt,
            self.llm_config,
            OVERVIEW_SYSTEM_PROMPT,
            None,  # No streaming
        )

        # Save overview
        with (self.wiki_dir / "README.md").open("w") as f:
            f.write(response.content)

    # Helper methods

    def _is_source_file(self, file_path: str) -> bool:
        """Check if file is a source file (not test, config, etc.)"""
        path = Path(file_path)

        # Exclude patterns
        exclude_patterns = [
            "test",
            "tests",
            "__pycache__",
            ".git",
            "node_modules",
            "venv",
            "env",
            ".env",
            "dist",
            "build",
            ".pytest_cache",
        ]

        for pattern in exclude_patterns:
            if pattern in path.parts:
                return False

        # Include only Python files
        return path.suffix == ".py"

    def _count_modules(self, tree: list[ModuleTreeNode]) -> int:
        """Count total modules in tree"""
        count = len(tree)
        for node in tree:
            if node.children:
                count += len(node.children)
        return count

    def _flatten_module_tree(
        self, tree: list[ModuleTreeNode]
    ) -> tuple[list[ModuleTreeNode], list[ModuleTreeNode]]:
        """Flatten tree into leaves and parents"""
        leaves = []
        parents = []

        for node in tree:
            if node.children:
                parents.append(node)
                leaves.extend(node.children)
            else:
                leaves.append(node)

        return leaves, parents

    def _extract_module_files(self, tree: list[ModuleTreeNode]) -> dict[str, list[str]]:
        """Extract module -> files mapping"""
        result = {}
        for node in tree:
            result[node.name] = node.files.copy()
            if node.children:
                for child in node.children:
                    result[child.name] = child.files.copy()
        return result

    def _get_current_commit(self) -> str:
        """Get current git commit hash"""
        try:
            result = subprocess.run(
                ["git", "rev-parse", "HEAD"],
                check=False,
                cwd=self.repo_path,
                capture_output=True,
                text=True,
            )
            return result.stdout.strip()
        except Exception:
            return "unknown"

    async def _load_wiki_meta(self) -> WikiMeta | None:
        """Load wiki metadata"""
        meta_path = self.wiki_dir / "meta.json"
        if not meta_path.exists():
            return None

        with meta_path.open() as f:
            data = json.load(f)
            return WikiMeta(**data)

    async def _save_wiki_meta(self, meta: WikiMeta):
        """Save wiki metadata"""
        meta.generated_at = datetime.now(UTC).isoformat()

        meta_path = self.wiki_dir / "meta.json"
        with meta_path.open("w") as f:
            json.dump(
                {
                    "from_commit": meta.from_commit,
                    "generated_at": meta.generated_at,
                    "model": meta.model,
                    "module_files": meta.module_files,
                    "module_tree": meta.module_tree,
                },
                f,
                indent=2,
            )

    async def _save_module_tree(self, tree: list[ModuleTreeNode]):
        """Save module tree for editing"""
        tree_path = self.wiki_dir / "module_tree.json"
        with tree_path.open("w") as f:
            json.dump([self._node_to_dict(n) for n in tree], f, indent=2)  # type: ignore[arg-type]

    def _node_to_dict(self, node: ModuleTreeNode) -> dict[str, Any]:
        """Convert node to dict"""
        result = {"name": node.name, "slug": node.slug, "files": node.files}
        if node.children:
            result["children"] = [self._node_to_dict(c) for c in node.children]  # type: ignore[misc]
        return result

    def _dict_to_node(self, data: dict[str, Any]) -> ModuleTreeNode:
        """Convert dict to node"""
        children = None
        if "children" in data:
            children = [self._dict_to_node(c) for c in data["children"]]

        return ModuleTreeNode(
            name=data["name"], slug=data["slug"], files=data["files"], children=children
        )
