"""
Graph Queries for Wiki Generation

Queries against the knowledge graph to extract structure for wiki generation.
"""

import asyncio
from pathlib import Path
from typing import Any

from comind.core.graph import GraphBackend, RelationType, SymbolType


async def get_files_with_exports(graph: GraphBackend) -> list[dict[str, Any]]:
    """Get all source files with their exported symbol names and types"""
    file_map: dict[str, dict[str, Any]] = {}

    # Get all symbols from database
    all_symbols = await graph.get_all_symbols()

    for symbol in all_symbols:
        if symbol.type == SymbolType.MODULE:
            continue

        # Check if symbol is exported (has properties indicating export)
        is_exported = symbol.properties.get("is_exported", True) if symbol.properties else True

        if is_exported:
            file_path = symbol.file_path
            if file_path not in file_map:
                file_map[file_path] = {"file_path": file_path, "symbols": []}

            file_map[file_path]["symbols"].append({"name": symbol.name, "type": symbol.type.value})

    return sorted(file_map.values(), key=lambda x: x["file_path"])


async def get_all_files(graph: GraphBackend) -> list[str]:
    """Get all files tracked in the graph"""
    files = set()
    all_symbols = await graph.get_all_symbols()
    for symbol in all_symbols:
        if symbol.file_path:
            files.add(symbol.file_path)
    return sorted(files)


async def get_inter_file_call_edges(graph: GraphBackend) -> list[dict[str, str]]:
    """Get inter-file call edges (calls between different files)"""
    edges = []

    all_symbols = await graph.get_all_symbols()
    symbol_dict = {s.id: s for s in all_symbols}

    for symbol in all_symbols:
        # Get outgoing CALLS relationships
        relationships = await graph.get_relationships(
            symbol.id, direction="outgoing", relation_type=RelationType.CALLS
        )

        for rel in relationships:
            target_symbol = symbol_dict.get(rel.target_id)
            if target_symbol and symbol.file_path != target_symbol.file_path:
                edges.append(
                    {
                        "from_file": symbol.file_path,
                        "from_name": symbol.name,
                        "to_file": target_symbol.file_path,
                        "to_name": target_symbol.name,
                    }
                )

    return edges


async def get_intra_module_call_edges(
    graph: GraphBackend, file_paths: list[str]
) -> list[dict[str, str]]:
    """Get call edges between files within a specific set (intra-module)"""
    if not file_paths:
        return []

    file_set = set(file_paths)
    edges = []

    all_symbols = await graph.get_all_symbols()
    symbol_dict = {s.id: s for s in all_symbols}

    for symbol in all_symbols:
        if symbol.file_path not in file_set:
            continue

        # Get outgoing CALLS relationships
        relationships = await graph.get_relationships(
            symbol.id, direction="outgoing", relation_type=RelationType.CALLS
        )

        for rel in relationships:
            target_symbol = symbol_dict.get(rel.target_id)
            if target_symbol and target_symbol.file_path in file_set:
                edges.append(
                    {
                        "from_file": symbol.file_path,
                        "from_name": symbol.name,
                        "to_file": target_symbol.file_path,
                        "to_name": target_symbol.name,
                    }
                )

    return edges


async def get_inter_module_call_edges(
    graph: GraphBackend, file_paths: list[str]
) -> dict[str, list[dict[str, str]]]:
    """Get call edges crossing module boundaries"""
    if not file_paths:
        return {"outgoing": [], "incoming": []}

    file_set = set(file_paths)
    outgoing = []
    incoming = []

    all_symbols = await graph.get_all_symbols()
    symbol_dict = {s.id: s for s in all_symbols}

    for symbol in all_symbols:
        # Get outgoing CALLS relationships
        relationships = await graph.get_relationships(
            symbol.id, direction="outgoing", relation_type=RelationType.CALLS
        )

        for rel in relationships:
            target_symbol = symbol_dict.get(rel.target_id)
            if not target_symbol:
                continue

            # Outgoing: from module to outside
            if symbol.file_path in file_set and target_symbol.file_path not in file_set:
                outgoing.append(
                    {
                        "from_file": symbol.file_path,
                        "from_name": symbol.name,
                        "to_file": target_symbol.file_path,
                        "to_name": target_symbol.name,
                    }
                )

            # Incoming: from outside to module
            if symbol.file_path not in file_set and target_symbol.file_path in file_set:
                incoming.append(
                    {
                        "from_file": symbol.file_path,
                        "from_name": symbol.name,
                        "to_file": target_symbol.file_path,
                        "to_name": target_symbol.name,
                    }
                )

    return {
        "outgoing": outgoing[:30],  # Limit to 30
        "incoming": incoming[:30],
    }


async def get_processes_for_files(
    graph: GraphBackend, file_paths: list[str]
) -> list[dict[str, Any]]:
    """Get execution flows (processes) that involve the given files"""
    if not file_paths:
        return []

    file_set = set(file_paths)
    processes = []

    # Get all processes from the graph
    all_processes = await graph.get_processes()

    for process in all_processes:
        # Check if any step involves files in our set
        steps_in_module = [
            step for step in process.get("steps", []) if step.get("file_path") in file_set
        ]

        if steps_in_module:
            processes.append(
                {
                    "id": process.get("id", ""),
                    "label": process.get("label", ""),
                    "type": process.get("type", ""),
                    "step_count": len(process.get("steps", [])),
                    "steps": steps_in_module,
                }
            )

    return processes[:10]  # Limit to 10 processes


async def get_all_processes(graph: GraphBackend) -> list[dict[str, Any]]:
    """Get all execution flows in the graph"""
    return await graph.get_processes()


async def get_inter_module_edges_for_overview(
    graph: GraphBackend, module_files: dict[str, list[str]]
) -> list[dict[str, str]]:
    """Get call edges between different modules for the overview diagram"""
    edges = []

    # Build reverse map: file -> module
    file_to_module = {}
    for module_name, files in module_files.items():
        for file_path in files:
            file_to_module[file_path] = module_name

    # Get all symbols
    all_symbols = await graph.get_all_symbols()
    symbol_dict = {s.id: s for s in all_symbols}

    # Find inter-module calls
    for symbol in all_symbols:
        source_module = file_to_module.get(symbol.file_path)
        if not source_module:
            continue

        # Get outgoing CALLS relationships
        relationships = await graph.get_relationships(
            symbol.id, direction="outgoing", relation_type=RelationType.CALLS
        )

        for rel in relationships:
            target_symbol = symbol_dict.get(rel.target_id)
            if not target_symbol:
                continue

            target_module = file_to_module.get(target_symbol.file_path)
            if target_module and source_module != target_module:
                edges.append(
                    {
                        "from_module": source_module,
                        "to_module": target_module,
                        "from_name": symbol.name,
                        "to_name": target_symbol.name,
                    }
                )

    return edges[:50]  # Limit to 50


async def read_file_content(file_path: str, max_lines: int = 500) -> str:
    """Read file content with line limit"""
    try:
        content = await asyncio.to_thread(Path(file_path).read_text, encoding="utf-8")
        lines = content.splitlines()[:max_lines]
        truncated_content = "\n".join(lines)
        if len(lines) >= max_lines:
            truncated_content += f"\n... (truncated at {max_lines} lines)"
        else:
            return truncated_content
    except Exception as e:
        return f"Error reading file: {e}"
    return truncated_content
