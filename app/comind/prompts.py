"""
Centralized prompt templates for LLM operations.

All prompts used across the application should be defined here.
"""

# Style Guide Extraction Specification
STYLE_GUIDE_SCAN_SPEC = """
Repository Style & Practice Extraction Specification

Objective

Analyze the provided repositories and extract implicit and explicit coding conventions, documentation styles, usage patterns, and micro-practices.

The goal is to infer exactly how this organization writes Python code so that a future coding agent or human developer can replicate these patterns precisely, matching the existing environment, tooling, and architectural decisions.

Focus on recurring behaviors, not just configuration files.

Output must be a single markdown file named:
{repo_name}_STYLE_GUIDE.md

General Instructions

Infer patterns based on statistical prevalence across repositories.

Prefer dominant patterns over rare occurrences.

Ignore outliers unless they are clearly intentional.

For every detected pattern:

Provide explanation

Provide canonical example snippet

Provide prevalence estimate (High / Medium / Low)

Focus on HOW code is written, not just which tools are used. Identify why certain decisions are made if obvious.

If multiple styles exist, document the dominant one and mention alternatives.

Output Structure

Your output MUST follow this exact structure:

TEAM PYTHON STYLE GUIDE

1. Environment & Tooling Assumptions
2. General Coding Style
3. Documentation Style
4. Error Handling & Validation
5. Logging & Observability
6. Architecture & Concurrency Patterns
7. Library & Framework Usage Patterns
8. Common Micro-Idioms & Performance
9. Style Consistency Assessment

Important Constraints

Do NOT list every small variation.

Focus on dominant recurring patterns.

Ignore one-off experiments.

Use real snippets from repositories when possible.

Do not hallucinate missing practices.

If something is inconsistent, explicitly state so.

Differentiate between external open-source tools and what appears to be internal/proprietary tooling.

Output Format Rules

The output must be valid Markdown.

Use the exact section headings provided.

Include code blocks for canonical examples.

Do not include your own meta-analysis of the extraction process.

Be concise but highly precise.
"""


# Wiki Generation Prompts
WIKI_MODULE_PROMPT = """
Generate comprehensive documentation for the following Python module.

Module: {module_name}
Files: {file_count}

Code Structure:
{symbols_summary}

Requirements:
1. Provide a clear overview of the module's purpose
2. Document key classes and functions
3. Explain relationships and dependencies
4. Include usage examples where appropriate
5. Note any important patterns or conventions

Output format: Markdown
"""

WIKI_OVERVIEW_PROMPT = """
Generate a high-level overview documentation for the repository.

Repository: {repo_name}
Total Modules: {module_count}
Total Symbols: {symbol_count}

Modules:
{modules_list}

Requirements:
1. Provide repository overview and purpose
2. Describe the overall architecture
3. List main modules and their responsibilities
4. Document key dependencies
5. Include getting started information

Output format: Markdown
"""


# Code Analysis Prompts
FUNCTION_SUMMARY_PROMPT = """
Summarize the following function:

{function_signature}

{function_body}

Provide a concise 1-2 sentence summary of what this function does.
"""

CLASS_SUMMARY_PROMPT = """
Summarize the following class:

{class_name}

Methods:
{methods_list}

Attributes:
{attributes_list}

Provide a concise summary of the class's purpose and main responsibilities.
"""


# Query Association Generation
QUERY_GENERATION_PROMPT = """
You are analyzing code to help developers find it using natural language queries.

Given this code symbol:

**Symbol Name:** {symbol_name}
**Type:** {symbol_type}
**Signature:** {signature}
**Docstring:** {docstring}
**File:** {file_path}

**Code Context:**
{code_snippet}

**What it calls:** {callees}
**Called by:** {callers}

Generate 5-10 natural language queries that a developer might ask when looking for this code.

Focus on:
- What problem does this solve? (e.g., "how to authenticate users", "validate API requests")
- What actions does it perform? (e.g., "create session", "parse token", "handle errors")
- When would someone use this? (e.g., "login flow", "request validation", "error handling")
- What concepts does it relate to? (e.g., "fastapi session", "jwt authentication", "database connection")

Rules:
- Use natural developer language, not formal documentation language
- Include variations (e.g., "how does X work", "X implementation", "handle X")
- Focus on intent and use cases, not just the function name
- Keep queries short (2-6 words typically)
- Include both specific and general queries
- Generate diverse queries covering different aspects and use cases
"""
