"""
Process Detection for CoMind

Detects execution flows (Processes) in the code graph by:
1. Finding entry points (functions with no internal callers)
2. Tracing forward via CALLS edges (BFS)
3. Grouping and deduplicating similar paths
4. Labeling with heuristic names

Processes help AI agents understand how features work through the codebase.
"""

from collections import deque
from dataclasses import dataclass

from comind.core.graph import Symbol, SymbolType
from comind.storage.graph_adapter import KnowledgeGraph


@dataclass
class ProcessNode:
    """Represents a detected execution process"""

    id: str
    label: str
    heuristic_label: str
    process_type: str  # 'intra_community' | 'cross_community'
    step_count: int
    communities: list[str]
    entry_point_id: str
    terminal_id: str
    priority: float
    symbol_count: int


@dataclass
class ProcessConfig:
    """Configuration for process detection"""

    max_trace_depth: int = 10
    max_branching: int = 4
    max_processes: int = 75
    min_steps: int = 3  # 3+ steps = genuine multi-hop flow


class ProcessDetector:
    """Detects execution processes in the code graph"""

    def __init__(self, graph: KnowledgeGraph, config: ProcessConfig = None):
        self.graph = graph
        self.config = config or ProcessConfig()
        self.processes: list[ProcessNode] = []
        self.process_traces: dict[str, list[Symbol]] = {}  # Store traces for query generation

    async def detect_processes(self) -> list[ProcessNode]:
        """Detect all execution processes in the graph"""
        # Step 1: Find entry points
        entry_points = await self._find_entry_points()

        # Step 2: Trace execution flows from each entry point
        all_traces = []
        for entry_point in entry_points:
            traces = await self._trace_execution_from_entry(entry_point)
            all_traces.extend(traces)

        # Step 3: Deduplicate and rank processes
        deduplicated = self._deduplicate_processes(all_traces)

        # Step 4: Label and prioritize
        labeled_processes = []
        for trace in deduplicated:
            process = await self._create_process_node(trace)
            if process:
                labeled_processes.append(process)
                # Store trace for query generation
                self.process_traces[process.id] = trace

        # Step 5: Sort by priority and limit
        labeled_processes.sort(key=lambda p: p.priority, reverse=True)
        self.processes = labeled_processes[: self.config.max_processes]

        return self.processes

    async def _find_entry_points(self) -> list[Symbol]:
        """Find potential entry points (functions with no internal callers)"""
        all_symbols = await self._get_all_functions()
        entry_points = []

        for symbol in all_symbols:
            # Check if this function has any internal callers
            callers = await self.graph.get_callers(symbol.id)

            # Filter out calls from the same file (likely internal)
            internal_callers = [
                caller
                for caller in callers
                if self._is_same_module(caller.file_path, symbol.file_path)
            ]

            # Entry point if no internal callers (or very few)
            if len(internal_callers) <= 1:  # Allow 1 for recursive/self calls
                entry_points.append(symbol)

        return entry_points

    async def _trace_execution_from_entry(self, entry_point: Symbol) -> list[list[Symbol]]:
        """Trace execution flow from an entry point using BFS"""
        traces: list[list[Symbol]] = []
        visited: set[str] = set()
        queue = deque([(entry_point, [entry_point])])

        while queue and len(traces) < self.config.max_processes:
            current_symbol, current_trace = queue.popleft()

            # Check depth limit
            if len(current_trace) > self.config.max_trace_depth:
                continue

            # Check if we've seen this symbol in this trace
            if current_symbol.id in visited:
                continue

            visited.add(current_symbol.id)

            # Get callees
            callees = await self.graph.get_callees(current_symbol.id)

            # Limit branching
            if len(callees) > self.config.max_branching:
                # Prioritize by some heuristic (e.g., cross-module calls)
                callees = self._prioritize_callees(callees, current_symbol)
                callees = callees[: self.config.max_branching]

            # Continue tracing
            if callees:
                for callee in callees[: self.config.max_branching]:
                    new_trace = current_trace + [callee]

                    # Save trace if it meets minimum length
                    if len(new_trace) >= self.config.min_steps:
                        traces.append(new_trace)

                    queue.append((callee, new_trace))
            # End of trace - save if it meets minimum length
            elif len(current_trace) >= self.config.min_steps:
                traces.append(current_trace)

        return traces

    def _prioritize_callees(self, callees: list[Symbol], caller: Symbol) -> list[Symbol]:
        """Prioritize callees based on heuristics"""
        prioritized = []

        for callee in callees:
            priority = 0

            # Cross-module calls get higher priority
            if not self._is_same_module(callee.file_path, caller.file_path):
                priority += 2

            # Public functions (not starting with _) get higher priority
            if not callee.name.startswith("_"):
                priority += 1

            prioritized.append((priority, callee))

        # Sort by priority (descending)
        prioritized.sort(key=lambda x: x[0], reverse=True)

        return [callee for _, callee in prioritized]

    def _deduplicate_processes(self, traces: list[list[Symbol]]) -> list[list[Symbol]]:
        """Remove duplicate or very similar traces"""
        unique_traces = []
        seen_signatures = set()

        for trace in traces:
            # Create a signature based on symbol types and names
            signature = self._create_trace_signature(trace)

            if signature not in seen_signatures:
                seen_signatures.add(signature)
                unique_traces.append(trace)

        return unique_traces

    def _create_trace_signature(self, trace: list[Symbol]) -> str:
        """Create a signature for a trace to detect duplicates"""
        # Use symbol types and names (ignoring specific line numbers)
        signature_parts = []
        for symbol in trace:
            part = f"{symbol.type.value}:{symbol.name}"
            signature_parts.append(part)

        return " -> ".join(signature_parts)

    async def _create_process_node(self, trace: list[Symbol]) -> ProcessNode | None:
        """Create a ProcessNode from a trace"""
        if not trace or len(trace) < 2:
            return None

        entry_point = trace[0]
        terminal = trace[-1]

        # Calculate communities touched
        communities = await self._get_trace_communities(trace)

        # Determine process type
        process_type = "cross_community" if len(communities) > 1 else "intra_community"

        # Calculate priority based on length and cross-community nature
        priority = len(trace) * 0.1
        if process_type == "cross_community":
            priority *= 1.5

        # Create label
        label = self._create_process_label(trace)

        # Generate ID
        process_id = self._generate_process_id(trace)

        return ProcessNode(
            id=process_id,
            label=label,
            heuristic_label=label,  # Same for now
            process_type=process_type,
            step_count=len(trace),
            communities=communities,
            entry_point_id=entry_point.id,
            terminal_id=terminal.id,
            priority=priority,
            symbol_count=len(trace),
        )

    def generate_queries_for_process(self, process: ProcessNode, trace: list[Symbol]) -> list[str]:
        """Generate natural language queries for a process to enable semantic search"""
        queries: list[str] = []

        if not trace or len(trace) < 2:
            return queries

        entry = trace[0]
        terminal = trace[-1]

        # Extract meaningful words from function names (split on _ and camelCase)
        def extract_words(name: str) -> list[str]:
            import re

            # Split on underscores and camelCase
            words = re.sub("([A-Z][a-z]+)", r" \1", re.sub("([A-Z]+)", r" \1", name)).split()
            words = [w.lower() for part in name.split("_") for w in part.split() if w]
            return [w for w in words if len(w) > 2]  # Filter short words

        entry_words = extract_words(entry.name)
        terminal_words = extract_words(terminal.name)

        # Generate query variations
        if entry_words:
            # "how does X work"
            queries.append(f"how does {' '.join(entry_words)} work")
            queries.append(f"{' '.join(entry_words)} flow")
            queries.append(f"{' '.join(entry_words)} execution")

        if entry_words and terminal_words:
            # "X to Y flow"
            queries.append(f"{' '.join(entry_words)} to {' '.join(terminal_words)}")
            queries.append(f"{' '.join(entry_words)} {' '.join(terminal_words)} process")

        # Add middle steps for richer context
        if len(trace) > 2:
            middle_words = []
            for symbol in trace[1:-1]:
                middle_words.extend(extract_words(symbol.name))
            if middle_words:
                # Include key middle steps
                unique_middle = list(set(middle_words))[:3]
                queries.append(f"{' '.join(entry_words)} {' '.join(unique_middle)}")

        # Cross-community queries
        if process.process_type == "cross_community":
            if entry_words:
                queries.append(f"{' '.join(entry_words)} cross module")
                queries.append(f"{' '.join(entry_words)} architecture")

        # Add generic execution flow query
        if entry_words:
            queries.append(f"execution flow {' '.join(entry_words)}")

        return queries

    def _create_process_label(self, trace: list[Symbol]) -> str:
        """Create a human-readable label for the process"""
        if len(trace) == 2:
            return f"{trace[0].name} → {trace[1].name}"
        if len(trace) == 3:
            return f"{trace[0].name} → {trace[1].name} → {trace[2].name}"
        return f"{trace[0].name} → {trace[-1].name} ({len(trace)} steps)"

    def _generate_process_id(self, trace: list[Symbol]) -> str:
        """Generate a unique ID for the process"""
        entry_name = trace[0].name
        terminal_name = trace[-1].name
        return f"proc_{len(self.processes)}_{entry_name}_{terminal_name}"

    async def _get_trace_communities(self, trace: list[Symbol]) -> list[str]:
        """Get all communities touched by this trace"""
        communities = set()

        for symbol in trace:
            symbol_communities = await self.graph.get_communities(symbol.id)
            if symbol_communities:
                for community in symbol_communities:
                    communities.add(community.get("name", "unknown"))

        return list(communities)

    def _is_same_module(self, file1: str, file2: str) -> bool:
        """Check if two files are in the same module"""
        # Simple heuristic: same directory
        from pathlib import Path

        return Path(file1).parent == Path(file2).parent

    async def _get_all_functions(self) -> list[Symbol]:
        """Get all function/method symbols from the graph."""
        all_symbols = await self.graph.get_all_symbols()
        return [s for s in all_symbols if s.type in (SymbolType.FUNCTION, SymbolType.METHOD)]

    async def get_processes(self) -> list[ProcessNode]:
        """Get all detected processes"""
        if not self.processes:
            await self.detect_processes()
        return self.processes

    async def get_process_by_id(self, process_id: str) -> ProcessNode | None:
        """Get a specific process by ID"""
        processes = await self.get_processes()
        for process in processes:
            if process.id == process_id:
                return process
        return None

    async def get_processes_for_symbol(self, symbol_id: str) -> list[ProcessNode]:
        """Get all processes that include this symbol"""
        # This would need to be implemented based on how we store process steps
        # For now, return all processes as a placeholder
        return await self.get_processes()
