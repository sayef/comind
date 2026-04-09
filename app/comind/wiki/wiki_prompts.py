"""
LLM Prompt Templates for Wiki Generation

All prompts produce deterministic, source-grounded documentation.
Templates use {{PLACEHOLDER}} substitution.
"""

from typing import Any

# ─── Grouping Prompt ──────────────────────────────────────────────────

GROUPING_SYSTEM_PROMPT = """You are a documentation architect. Given a list of source files with their exported symbols, group them into logical documentation modules.

Rules:
- Each module should represent a cohesive feature, layer, or domain
- Every file must appear in exactly one module
- Module names should be human-readable (e.g. "Authentication", "Database Layer", "API Routes")
- Aim for 5-15 modules for a typical project. Fewer for small projects, more for large ones
- Group by functionality, not by file type or directory structure alone
- Do NOT create modules for tests, configs, or non-source files"""

GROUPING_USER_PROMPT = """Group these source files into documentation modules.

**Files and their exports:**
{{FILE_LIST}}

**Directory structure:**
{{DIRECTORY_TREE}}

Respond with ONLY a JSON object mapping module names to file path arrays. No markdown, no explanation.
Example format:
{
  "Authentication": ["src/auth/login.py", "src/auth/session.py"],
  "Database": ["src/db/connection.py", "src/db/models.py"]
}"""

# ─── Leaf Module Prompt ───────────────────────────────────────────────

MODULE_SYSTEM_PROMPT = """You are a technical documentation writer. Write clear, developer-focused documentation for a code module.

Rules:
- Output ONLY the documentation content — no meta-commentary like "I've written...", "Here's the documentation...", "The documentation covers...", or similar
- Start directly with the module heading and content
- Reference actual function names, class names, and code patterns — do NOT invent APIs
- Use the call graph and execution flow data for accuracy, but do NOT mechanically list every edge
- Include Mermaid diagrams only when they genuinely help understanding. Keep them small (5-10 nodes max)
- Structure the document however makes sense for this module — there is no mandatory format
- Write for a developer who needs to understand and contribute to this code"""

MODULE_USER_PROMPT = """Write documentation for the **{{MODULE_NAME}}** module.

## Source Code

{{SOURCE_CODE}}

## Call Graph & Execution Flows (reference for accuracy)

Internal calls: {{INTRA_CALLS}}
Outgoing calls: {{OUTGOING_CALLS}}
Incoming calls: {{INCOMING_CALLS}}
Execution flows: {{PROCESSES}}

---

Write comprehensive documentation for this module. Cover its purpose, how it works, its key components, and how it connects to the rest of the codebase. Use whatever structure best fits this module — you decide the sections and headings. Include a Mermaid diagram only if it genuinely clarifies the architecture."""

# ─── Parent Module Prompt ─────────────────────────────────────────────

PARENT_SYSTEM_PROMPT = """You are a technical documentation writer. Write a summary page for a module that contains sub-modules. Synthesize the children's documentation — do not re-read source code.

Rules:
- Output ONLY the documentation content — no meta-commentary like "I've written...", "Here's the documentation...", "The documentation covers...", or similar
- Start directly with the module heading and content
- Reference actual components from the child modules
- Focus on how the sub-modules work together, not repeating their individual docs
- Keep it concise — the reader can click through to child pages for detail
- Include a Mermaid diagram only if it genuinely clarifies how the sub-modules relate"""

PARENT_USER_PROMPT = """Write documentation for the **{{MODULE_NAME}}** module, which contains these sub-modules:

{{CHILDREN_DOCS}}

Cross-module calls: {{CROSS_MODULE_CALLS}}
Shared execution flows: {{CROSS_PROCESSES}}

---

Write a concise overview of this module group. Explain its purpose, how the sub-modules fit together, and the key workflows that span them. Link to sub-module pages (e.g. `[Sub-module Name](sub-module-slug.md)`) rather than repeating their content. Use whatever structure fits best."""

# ─── Overview Prompt ──────────────────────────────────────────────────

OVERVIEW_SYSTEM_PROMPT = """You are a technical documentation writer. Write the top-level overview page for a repository wiki. This is the first page a new developer sees.

Rules:
- Output ONLY the documentation content — no meta-commentary like "I've written...", "Here's the documentation...", "The page has been rewritten...", or similar
- Start directly with the project heading and content
- Be clear and welcoming — this is the entry point to the entire codebase
- Reference actual module names so readers can navigate to their docs
- Include a high-level Mermaid architecture diagram showing only the most important modules and their relationships (max 10 nodes). A new dev should grasp it in 10 seconds
- Do NOT create module index tables or list every module with descriptions — just link to module pages naturally within the text
- Use the inter-module edges and execution flow data for accuracy, but do NOT dump them raw"""

OVERVIEW_USER_PROMPT = """Write the overview page for this repository's wiki.

## Project Info

{{PROJECT_INFO}}

## Module Summaries

{{MODULE_SUMMARIES}}

## Reference Data (for accuracy — do not reproduce verbatim)

Inter-module call edges: {{MODULE_EDGES}}
Key system flows: {{TOP_PROCESSES}}

---

Write a clear overview of this project: what it does, how it's architected, and the key end-to-end flows. Include a simple Mermaid architecture diagram (max 10 nodes, big-picture only). Link to module pages (e.g. `[Module Name](module-slug.md)`) naturally in the text rather than listing them in a table. If project config was provided, include brief setup instructions. Structure the page however reads best."""

# ─── Template Substitution Helper ─────────────────────────────────────


def fill_template(template: str, vars: dict[str, str]) -> str:
    """Replace {{PLACEHOLDER}} tokens in a template string"""
    result = template
    for key, value in vars.items():
        result = result.replace(f"{{{{{key}}}}}", value)
    return result


# ─── Formatting Helpers ───────────────────────────────────────────────


def format_file_list_for_grouping(files: list[dict[str, Any]]) -> str:
    """Format file list with exports for the grouping prompt"""
    lines = []
    for f in files:
        symbols = f.get("symbols", [])
        if symbols:
            exports = ", ".join(f"{s['name']} ({s['type']})" for s in symbols)
        else:
            exports = "no exports"
        lines.append(f"- {f['file_path']}: {exports}")
    return "\n".join(lines)


def format_directory_tree(file_paths: list[str]) -> str:
    """Build a directory tree string from file paths"""
    dirs = set()
    for fp in file_paths:
        parts = fp.replace("\\", "/").split("/")
        for i in range(1, len(parts)):
            dirs.add("/".join(parts[:i]))

    sorted_dirs = sorted(dirs)
    if not sorted_dirs:
        return "(flat structure)"

    result = "\n".join(sorted_dirs[:50])
    if len(sorted_dirs) > 50:
        result += f"\n... and {len(sorted_dirs) - 50} more directories"
    return result


def format_call_edges(edges: list[dict[str, str]]) -> str:
    """Format call edges as readable text"""
    if not edges:
        return "None"

    lines = []
    for e in edges[:30]:
        from_file = short_path(e.get("from_file", ""))
        to_file = short_path(e.get("to_file", ""))
        lines.append(f"{e.get('from_name', '')} ({from_file}) → {e.get('to_name', '')} ({to_file})")
    return "\n".join(lines)


def format_processes(processes: list[dict[str, Any]]) -> str:
    """Format process traces as readable text"""
    if not processes:
        return "No execution flows detected for this module."

    lines = []
    for p in processes:
        steps_text = "\n".join(
            f"  {s['step']}. {s['name']} ({short_path(s['file_path'])})" for s in p.get("steps", [])
        )
        lines.append(f"**{p['label']}** ({p['type']}):\n{steps_text}")

    return "\n\n".join(lines)


def short_path(fp: str) -> str:
    """Shorten a file path for readability"""
    parts = fp.replace("\\", "/").split("/")
    return "/".join(parts[-3:]) if len(parts) > 3 else fp
